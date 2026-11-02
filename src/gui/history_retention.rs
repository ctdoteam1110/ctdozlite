//! Bounded immutable history deltas retained until durable acknowledgement.
use super::{history, GuiEvent, GuiTx};
use crate::{
    chat::Thread,
    latest_snapshot::{LatestSnapshot, UpdateError},
    session::Session,
    store_v2::AccountStore,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
};

const INGRESS_CAPACITY: usize = 8;
const MAX_BATCH_BYTES: usize = 32 * 1024 * 1024;
type Key = (PathBuf, String, u64);
#[derive(Clone, Copy)]
pub(super) enum Origin {
    Network,
    Cached,
}
#[derive(Debug)]
pub(super) struct IngressPermit {
    _slot: tokio::sync::OwnedSemaphorePermit,
}
/// A rejected page remains owned by the caller, including its reserved capacity.
pub(super) struct StageRejected {
    pub message: String,
    pub threads: Vec<Thread>,
    pub permit: Arc<IngressPermit>,
}
impl std::fmt::Display for StageRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::fmt::Debug for StageRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StageRejected")
            .field("message", &self.message)
            .field("thread_count", &self.threads.len())
            .field("capacity_owners", &Arc::strong_count(&self.permit))
            .finish()
    }
}
impl std::error::Error for StageRejected {}
fn within_budget(threads: &[Thread]) -> bool {
    let rows = threads
        .iter()
        .map(|thread| thread.messages.len())
        .sum::<usize>();
    let overhead = rows.saturating_mul(std::mem::size_of::<crate::chat::ChatMessage>());
    overhead <= MAX_BATCH_BYTES
        && serde_json::to_writer(ByteBudget(MAX_BATCH_BYTES - overhead), threads).is_ok()
}
#[derive(Clone)]
struct Batch {
    id: u64,
    version: u64,
    threads: Vec<Thread>,
    origin: Origin,
    live: bool,
    _permit: Arc<IngressPermit>,
    raw: Option<Arc<super::phone_pending::Page>>,
    raw_id: Option<String>,
    chunks: Option<Arc<Vec<Vec<Thread>>>>,
    next_chunk: usize,
}
#[derive(Clone, Default)]
struct Pending {
    batches: Vec<Arc<Batch>>,
}
#[derive(Clone, Debug)]
pub(super) struct Ticket {
    key: Key,
    id: u64,
    permit: Arc<IngressPermit>,
    raw_id: Option<String>,
}
impl Ticket {
    pub(super) fn raw_id(&self) -> Option<&str> {
        self.raw_id.as_deref()
    }
}
pub(super) struct RawRejected {
    pub page: Box<super::phone_pending::Page>,
    pub permit: IngressPermit,
    pub message: String,
}
impl std::fmt::Display for RawRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}
impl std::fmt::Debug for RawRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawRejected")
            .field("message", &self.message)
            .field("objects", &self.page.objects.len())
            .field("permit", &self.permit)
            .finish()
    }
}
impl std::error::Error for RawRejected {}
struct Retention {
    mailbox: LatestSnapshot<Key, Pending>,
    capacity: Arc<tokio::sync::Semaphore>,
    next: AtomicU64,
}
impl Retention {
    fn new(capacity: usize) -> Self {
        Self {
            mailbox: LatestSnapshot::new(capacity),
            capacity: Arc::new(tokio::sync::Semaphore::new(capacity)),
            next: AtomicU64::new(0),
        }
    }
    async fn reserve(&self) -> IngressPermit {
        IngressPermit {
            _slot: self
                .capacity
                .clone()
                .acquire_owned()
                .await
                .expect("history semaphore remains open"),
        }
    }
    fn stage(
        &self,
        root: PathBuf,
        account: String,
        threads: Vec<Thread>,
        origin: Origin,
        live: bool,
        permit: IngressPermit,
    ) -> Result<Ticket, StageRejected> {
        let permit = Arc::new(permit);
        let reject = |message: &str, threads| StageRejected {
            message: message.into(),
            threads,
            permit: permit.clone(),
        };
        if account.is_empty() {
            return Err(reject("Tài khoản lịch sử không hợp lệ", threads));
        }
        if !within_budget(&threads) {
            return Err(reject(
                "Trang lịch sử vượt giới hạn giữ chờ; con trỏ chưa được xác nhận",
                threads,
            ));
        }
        let _mutation = match crate::runtime_scope::mutation() {
            Ok(guard) => guard,
            Err(_) => return Err(reject("Phiên lịch sử đã thay đổi", threads)),
        };
        let generation = crate::runtime_scope::execution_scope()
            .map(|scope| scope.generation)
            .unwrap_or(0);
        let key = (root, account, generation);
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let batch = Arc::new(Batch {
            id,
            version: id,
            threads,
            origin,
            live,
            _permit: permit.clone(),
            raw: None,
            raw_id: None,
            chunks: None,
            next_chunk: 0,
        });
        let recover = batch.clone();
        if self
            .mailbox
            .update(key.clone(), |prior| {
                let mut pending = prior.cloned().unwrap_or_default();
                pending.batches.push(batch);
                Ok::<_, ()>(pending)
            })
            .is_err()
        {
            let batch = Arc::try_unwrap(recover)
                .unwrap_or_else(|_| unreachable!("rejected update cannot retain the batch"));
            return Err(reject("Hàng đợi lịch sử đã đầy", batch.threads));
        }
        Ok(Ticket {
            key,
            id,
            permit,
            raw_id: None,
        })
    }
    fn ack(&self, key: &Key, id: u64, version: u64) {
        let _ = self.mailbox.update(key.clone(), |prior| {
            let mut pending = prior.cloned().unwrap_or_default();
            pending
                .batches
                .retain(|batch| batch.id != id || batch.version != version);
            Ok::<_, ()>(pending)
        });
        self.mailbox
            .remove_if(key, |pending| pending.batches.is_empty());
    }
    fn flush_sync(&self) -> anyhow::Result<()> {
        // The caller owns the existing fallible lifecycle mutation barrier. It drains
        // in-flight writes first; no second lock, queue or transition barrier is created.
        for (key, pending) in self.mailbox.snapshot() {
            let mut store = None;
            let mut batches = pending.batches;
            // Opaque accepted pages must reach their durable spool even if message storage fails.
            batches.sort_by_key(|batch| batch.raw.is_none());
            for batch in batches {
                apply_one(&mut store, &key, &batch)?;
                self.ack(&key, batch.id, batch.version);
            }
        }
        self.mailbox.release_wake();
        Ok(())
    }
}
fn retained() -> &'static Retention {
    static RETAINED: OnceLock<Retention> = OnceLock::new();
    RETAINED.get_or_init(|| Retention::new(INGRESS_CAPACITY))
}
fn apply_one(
    store: &mut Option<AccountStore>,
    key: &Key,
    batch: &Batch,
) -> anyhow::Result<history::Reply> {
    if let (Some(page), Some(id)) = (&batch.raw, &batch.raw_id) {
        super::phone_pending::persist(&key.0, &key.1, id, page)?;
        return Ok(history::Reply::Threads(Vec::new()));
    }
    if store.is_none() {
        *store = Some(AccountStore::open(&key.0, &key.1)?);
    }
    let store = store.as_mut().expect("history store opened");
    if let Some(chunks) = &batch.chunks {
        for (index, chunk) in chunks.iter().enumerate().skip(batch.next_chunk) {
            let request = match batch.origin {
                Origin::Network => history::Request::Reconcile(chunk.clone()),
                Origin::Cached => history::Request::CachedDelta(chunk.clone()),
            };
            history::apply(store, request)?;
            // The whole tail remains mailbox-owned; advance only after this SQL acknowledgement.
            let _ =
                retained().mailbox.update(key.clone(), |prior| {
                    let mut pending = prior.cloned().unwrap_or_default();
                    if let Some(position) = pending.batches.iter().position(|current| {
                        current.id == batch.id && current.version == batch.version
                    }) {
                        let mut current = (*pending.batches[position]).clone();
                        current.next_chunk = current.next_chunk.max(index + 1);
                        pending.batches[position] = Arc::new(current);
                    }
                    Ok::<_, ()>(pending)
                });
        }
        let metadata = store.all_thread_metadata()?;
        let mut projected: Vec<Thread> = Vec::new();
        for chunk in chunks.iter() {
            for incoming in chunk {
                let Some(durable) = metadata.iter().find(|thread| thread.id == incoming.id) else {
                    continue;
                };
                let position = match projected.iter().position(|thread| thread.id == incoming.id) {
                    Some(position) => position,
                    None => {
                        projected.push(durable.clone());
                        projected.len() - 1
                    }
                };
                let target = &mut projected[position];
                for message in &incoming.messages {
                    if let Some(message) = store.message_for(&incoming.id, message)? {
                        if !target
                            .messages
                            .iter()
                            .any(|existing| existing.id == message.id)
                        {
                            target.messages.push(message);
                        }
                    }
                    if target.messages.len() > 200 {
                        target
                            .messages
                            .sort_by(|a, b| (a.ts, &a.id).cmp(&(b.ts, &b.id)));
                        target.messages.remove(0);
                    }
                }
                target
                    .messages
                    .sort_by(|a, b| (a.ts, &a.id).cmp(&(b.ts, &b.id)));
            }
        }
        return Ok(history::Reply::Threads(projected));
    }
    history::apply(store, request(batch))
}
/// Capture opaque phone data without inferring any thread or sender identity.
pub(super) fn stage_raw(
    root: PathBuf,
    account: String,
    page: super::phone_pending::Page,
    permit: IngressPermit,
) -> Result<Ticket, RawRejected> {
    if super::phone_pending::validate(&account, &page).is_err() {
        return Err(RawRejected {
            page: Box::new(page),
            permit,
            message: "Trang điện thoại không hợp lệ hoặc vượt giới hạn giữ chờ".into(),
        });
    }
    let _mutation = match crate::runtime_scope::mutation() {
        Ok(guard) => guard,
        Err(_) => {
            return Err(RawRejected {
                page: Box::new(page),
                permit,
                message: "Phiên điện thoại đã thay đổi".into(),
            })
        }
    };
    let generation = crate::runtime_scope::execution_scope()
        .map(|scope| scope.generation)
        .unwrap_or(0);
    let id = retained().next.fetch_add(1, Ordering::Relaxed);
    let raw_id = format!(
        "{}-{}-{id}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let key = (root, account, generation);
    let permit = Arc::new(permit);
    let batch = Arc::new(Batch {
        id,
        version: id,
        threads: Vec::new(),
        origin: Origin::Cached,
        live: false,
        _permit: permit.clone(),
        raw: Some(Arc::new(page)),
        raw_id: Some(raw_id.clone()),
        chunks: None,
        next_chunk: 0,
    });
    let recover = batch.clone();
    if retained()
        .mailbox
        .update(key.clone(), |prior| {
            let mut pending = prior.cloned().unwrap_or_default();
            pending.batches.push(batch);
            Ok::<_, ()>(pending)
        })
        .is_err()
    {
        let batch = Arc::try_unwrap(recover)
            .unwrap_or_else(|_| unreachable!("rejected update cannot retain raw page"));
        let page = Arc::try_unwrap(batch.raw.expect("raw payload"))
            .unwrap_or_else(|_| unreachable!("raw payload not shared on rejection"));
        drop(batch._permit);
        let permit = Arc::try_unwrap(permit)
            .unwrap_or_else(|_| unreachable!("raw capacity not shared on rejection"));
        return Err(RawRejected {
            page: Box::new(page),
            permit,
            message: "Hàng đợi điện thoại đã đầy".into(),
        });
    }
    Ok(Ticket {
        key,
        id,
        permit,
        raw_id: Some(raw_id),
    })
}
fn request(batch: &Batch) -> history::Request {
    match batch.origin {
        Origin::Network => history::Request::Reconcile(batch.threads.clone()),
        Origin::Cached => history::Request::CachedDelta(batch.threads.clone()),
    }
}
/// Reserve bounded capacity before reading another network or transfer page.
pub(super) async fn reserve() -> IngressPermit {
    retained().reserve().await
}
/// Capture an immutable page before any cancellable enrichment or storage await.
pub(super) fn stage(
    root: PathBuf,
    account: String,
    threads: Vec<Thread>,
    origin: Origin,
    live: bool,
    permit: IngressPermit,
) -> Result<Ticket, StageRejected> {
    retained().stage(root, account, threads, origin, live, permit)
}
/// Replace one reserved poll accumulator after each parsed response, before another await.
pub(super) fn replace(ticket: &Ticket, threads: Vec<Thread>) -> Result<(), String> {
    let chunks = page_chunks(threads)?;
    let version = retained().next.fetch_add(1, Ordering::Relaxed);
    let batch = Arc::new(Batch {
        id: ticket.id,
        version,
        threads: Vec::new(),
        origin: Origin::Network,
        live: false,
        _permit: ticket.permit.clone(),
        raw: None,
        raw_id: None,
        chunks: Some(Arc::new(chunks)),
        next_chunk: 0,
    });
    let _mutation = crate::runtime_scope::mutation().map_err(|_| "Phiên lịch sử đã thay đổi")?;
    retained()
        .mailbox
        .update(ticket.key.clone(), |prior| {
            let mut pending = prior.cloned().unwrap_or_default();
            if let Some(position) = pending
                .batches
                .iter()
                .position(|batch| batch.id == ticket.id)
            {
                pending.batches[position] = batch;
            } else {
                pending.batches.push(batch);
            }
            Ok::<_, ()>(pending)
        })
        .map_err(|_| "Hàng đợi lịch sử đã đầy".to_string())?;
    Ok(())
}
/// Flush all retained owner-bound deltas from the authorized lifecycle callback.
pub(super) fn flush_pending_sync() -> anyhow::Result<()> {
    retained().flush_sync()
}
/// Persist a ticket and earlier accepted pages. Failed writes stay in the mailbox.
pub(super) async fn persist(
    ticket: &Ticket,
    session: Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
    sender: Option<GuiTx>,
) -> Result<Vec<Thread>, String> {
    let reply = history::execute(
        ticket.key.0.clone(),
        session,
        captured,
        ticket.key.1.clone(),
        history::Request::FlushRetained {
            ticket: ticket.clone(),
            sender,
        },
    )
    .await?;
    match reply {
        history::Reply::Threads(threads) => Ok(threads),
        _ => Err("Lịch sử chưa xác nhận ghi".into()),
    }
}
/// Run under the existing history serialization lane and captured mutation guard.
pub(super) fn apply(
    root: &std::path::Path,
    account: &str,
    ticket: &Ticket,
    sender: Option<GuiTx>,
) -> anyhow::Result<history::Reply> {
    let generation = crate::runtime_scope::execution_scope()
        .map(|scope| scope.generation)
        .unwrap_or(0);
    anyhow::ensure!(
        generation == ticket.key.2,
        "Stale retained history generation"
    );
    let Some(pending) = retained().mailbox.snapshot().get(&ticket.key).cloned() else {
        return Ok(history::Reply::Threads(Vec::new()));
    };
    anyhow::ensure!(
        root == ticket.key.0 && account == ticket.key.1,
        "Retained history owner mismatch"
    );
    if !pending.batches.iter().any(|batch| batch.id == ticket.id) {
        return Ok(history::Reply::Threads(Vec::new()));
    }
    let mut store = None;
    let mut result = Vec::new();
    let mut batches = pending.batches;
    batches.sort_by_key(|batch| batch.raw.is_none());
    for batch in batches {
        let reply = apply_one(&mut store, &ticket.key, &batch)?;
        let history::Reply::Threads(threads) = reply else {
            anyhow::bail!("History write acknowledgement missing");
        };
        if let Some(sender) = sender.as_ref().filter(|_| batch.raw.is_none()) {
            let _ = sender.send(GuiEvent::AccountThreadsUpdated {
                account: ticket.key.1.clone(),
                threads: threads.clone(),
                live: batch.live,
            });
        }
        let target = batch.id == ticket.id;
        if target {
            result = threads;
        }
        retained().ack(&ticket.key, batch.id, batch.version);
        if target {
            break;
        }
    }
    retained().mailbox.release_wake();
    Ok(history::Reply::Threads(result))
}

// GUI snapshots are replaceable, unlike incoming network/transfer deltas. Keeping a
// separate value prevents a later 200-body projection from erasing an offscreen page.
type GuiKey = (String, u64);
fn gui_snapshots() -> &'static LatestSnapshot<GuiKey, Arc<Vec<Thread>>> {
    static GUI: OnceLock<LatestSnapshot<GuiKey, Arc<Vec<Thread>>>> = OnceLock::new();
    GUI.get_or_init(|| LatestSnapshot::new(8))
}
struct ByteBudget(usize);
impl std::io::Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("snapshot budget exceeded"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// Publish one bounded replaceable snapshot using only in-memory work and a payload-free wake.
pub(super) fn stage_gui(
    account: String,
    generation: u64,
    threads: Vec<Thread>,
) -> Result<bool, String> {
    if account.is_empty() {
        return Ok(false);
    }
    // A counting writer stops at 1 MiB and never allocates another serialized snapshot.
    serde_json::to_writer(ByteBudget(1024 * 1024), &threads).map_err(|_| {
        "Ảnh chụp lịch sử vượt giới hạn lưu nền; dữ liệu trên màn hình vẫn được giữ".to_string()
    })?;
    gui_snapshots()
        .update((account, generation), |_| {
            if crate::runtime_scope::registry().generation() != generation {
                return Err("Phiên đã đổi; ảnh chụp cũ chưa được nhận".to_string());
            }
            Ok(Arc::new(threads))
        })
        .map_err(|error| match error {
            UpdateError::Full => "Hàng đợi ảnh chụp lịch sử đã đầy".into(),
            UpdateError::Rejected(error) => error,
        })
}
pub(super) struct GuiWakeLease(u64);
impl GuiWakeLease {
    pub(super) fn new() -> Self {
        Self(gui_snapshots().wake_token())
    }
}
impl Drop for GuiWakeLease {
    fn drop(&mut self) {
        gui_snapshots().release_wake_for(self.0);
    }
}
pub(super) async fn flush_gui(
    root: PathBuf,
    session: Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
) -> Result<(), String> {
    let account = captured
        .login
        .as_ref()
        .map(|login| login.uin.clone())
        .unwrap_or_default();
    let generation = crate::runtime_scope::execution_scope()
        .map(|scope| scope.generation)
        .unwrap_or(0);
    let key = (account.clone(), generation);
    if let Some(threads) = gui_snapshots().snapshot().get(&key).cloned() {
        history::execute(
            root,
            session,
            captured,
            account,
            history::Request::CachedDelta((*threads).clone()),
        )
        .await?;
        gui_snapshots().remove_if(&key, |current| Arc::ptr_eq(current, &threads));
    }
    Ok(())
}
pub(super) fn flush_gui_sync(root: &std::path::Path) -> anyhow::Result<()> {
    for (key, threads) in gui_snapshots().snapshot() {
        let mut store = AccountStore::open(root, &key.0)?;
        history::apply(
            &mut store,
            history::Request::CachedDelta((*threads).clone()),
        )?;
        gui_snapshots().remove_if(&key, |current| Arc::ptr_eq(current, &threads));
    }
    gui_snapshots().release_wake();
    Ok(())
}

/// Retry retained deltas in the currently captured epoch; no network request is repeated.
pub(super) async fn retry_current(
    session: Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
    sender: GuiTx,
) {
    let account = captured
        .login
        .as_ref()
        .map(|login| login.uin.as_str())
        .unwrap_or("");
    let generation = crate::runtime_scope::execution_scope()
        .map(|scope| scope.generation)
        .unwrap_or(0);
    for (key, pending) in retained().mailbox.snapshot() {
        if key.1 != account || key.2 != generation {
            continue;
        }
        if let Some(batch) = pending.batches.last() {
            let ticket = Ticket {
                key,
                id: batch.id,
                permit: batch._permit.clone(),
                raw_id: batch.raw_id.clone(),
            };
            let _ = persist(
                &ticket,
                session.clone(),
                captured.clone(),
                Some(sender.clone()),
            )
            .await;
        }
    }
    if let Ok(root) = history::root() {
        let _ = flush_gui(root, session, captured).await;
    }
}

/// A transfer advances its source cursor only after this durable acknowledgement.
#[cfg(test)]
pub(super) async fn persist_until_acked(
    ticket: &Ticket,
    session: Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
) -> Vec<Thread> {
    loop {
        if let Ok(threads) = persist(ticket, session.clone(), captured.clone(), None).await {
            return threads;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

// The original page stays owned by this cursor. Only the next bounded chunk is materialized.
struct PageCursor {
    threads: std::vec::IntoIter<Thread>,
    current: Option<(Thread, std::vec::IntoIter<crate::chat::ChatMessage>)>,
    peeked: Option<crate::chat::ChatMessage>,
}
fn encoded_size<T: serde::Serialize>(value: &T) -> Result<usize, String> {
    let mut budget = ByteBudget(MAX_BATCH_BYTES);
    serde_json::to_writer(&mut budget, value).map_err(|_| {
        "Một tin hoặc metadata vượt giới hạn 32 MiB; trang chưa được xác nhận".to_string()
    })?;
    Ok(MAX_BATCH_BYTES - budget.0)
}
impl PageCursor {
    fn new(threads: Vec<Thread>) -> Self {
        Self {
            threads: threads.into_iter(),
            current: None,
            peeked: None,
        }
    }
    fn next(&mut self) -> Result<Option<Vec<Thread>>, String> {
        let mut chunk = Vec::new();
        let mut used = 32usize;
        loop {
            if self.current.is_none() {
                let Some(mut thread) = self.threads.next() else {
                    return Ok((!chunk.is_empty()).then_some(chunk));
                };
                let messages = std::mem::take(&mut thread.messages).into_iter();
                self.current = Some((thread, messages));
            }
            let (metadata, mut messages) = self.current.take().expect("current source thread");
            let metadata_cost =
                encoded_size(&metadata)?.saturating_add(std::mem::size_of::<Thread>() + 32);
            if metadata_cost >= MAX_BATCH_BYTES {
                return Err(
                    "Metadata hội thoại vượt giới hạn 32 MiB; trang chưa được xác nhận".into(),
                );
            }
            if used + metadata_cost >= MAX_BATCH_BYTES && !chunk.is_empty() {
                self.current = Some((metadata, messages));
                return Ok(Some(chunk));
            }
            let mut piece = metadata.clone();
            let mut piece_cost = metadata_cost;
            loop {
                let Some(message) = self.peeked.take().or_else(|| messages.next()) else {
                    break;
                };
                let cost = encoded_size(&message)?
                    .saturating_add(std::mem::size_of::<crate::chat::ChatMessage>() + 8);
                if metadata_cost.saturating_add(cost).saturating_add(32) >= MAX_BATCH_BYTES {
                    return Err("Một tin vượt giới hạn 32 MiB; trang chưa được xác nhận".into());
                }
                if used.saturating_add(piece_cost).saturating_add(cost) >= MAX_BATCH_BYTES {
                    self.peeked = Some(message);
                    self.current = Some((metadata, messages));
                    if !piece.messages.is_empty() {
                        chunk.push(piece);
                    }
                    return Ok(Some(chunk));
                }
                piece_cost += cost;
                piece.messages.push(message);
            }
            used += piece_cost;
            chunk.push(piece);
        }
    }
}
/// Persist one accepted page in bounded chunks without advancing its provider cursor early.
/// Opaque phone pages remain durably spooled until this complete acknowledgement.
pub(super) async fn persist_page(
    context: (PathBuf, Arc<tokio::sync::Mutex<Session>>, Session, String),
    threads: Vec<Thread>,
    origin: Origin,
    live: bool,
    first_permit: IngressPermit,
    sender: Option<GuiTx>,
) -> Result<Vec<Thread>, String> {
    let (root, session, captured, account) = context;
    let ticket = capture_page(root, account, threads, origin, live, first_permit)?;
    persist(&ticket, session, captured, sender).await
}
fn page_chunks(threads: Vec<Thread>) -> Result<Vec<Vec<Thread>>, String> {
    // A complete accepted page, including every future SQL chunk, enters the mailbox
    // before the first await. Limits apply to the page, not just its first SQL batch.
    let overhead = threads
        .iter()
        .map(|thread| thread.messages.len())
        .sum::<usize>()
        .saturating_mul(std::mem::size_of::<crate::chat::ChatMessage>());
    const MAX_PAGE_BYTES: usize = 128 * 1024 * 1024;
    if overhead > MAX_PAGE_BYTES
        || serde_json::to_writer(ByteBudget(MAX_PAGE_BYTES - overhead), &threads).is_err()
    {
        return Err("Trang lịch sử vượt giới hạn 128 MiB; trang chưa được xác nhận".into());
    }
    let mut cursor = PageCursor::new(threads);
    let mut chunks = Vec::new();
    while let Some(chunk) = cursor.next()? {
        chunks.push(chunk);
    }
    Ok(chunks)
}
/// Capture the complete page synchronously, including when its later sink future is never polled.
pub(super) fn capture_page(
    root: PathBuf,
    account: String,
    threads: Vec<Thread>,
    origin: Origin,
    live: bool,
    first_permit: IngressPermit,
) -> Result<Ticket, String> {
    let chunks = page_chunks(threads)?;
    let ticket = {
        let _mutation = crate::runtime_scope::mutation()
            .map_err(|_| "Phiên lịch sử đã thay đổi".to_string())?;
        let generation = crate::runtime_scope::execution_scope()
            .map(|scope| scope.generation)
            .unwrap_or(0);
        let key = (root, account, generation);
        let id = retained().next.fetch_add(1, Ordering::Relaxed);
        let permit = Arc::new(first_permit);
        let batch = Arc::new(Batch {
            id,
            version: id,
            threads: Vec::new(),
            origin,
            live,
            _permit: permit.clone(),
            raw: None,
            raw_id: None,
            chunks: Some(Arc::new(chunks)),
            next_chunk: 0,
        });
        retained()
            .mailbox
            .update(key.clone(), |prior| {
                let mut pending = prior.cloned().unwrap_or_default();
                pending.batches.push(batch);
                Ok::<_, ()>(pending)
            })
            .map_err(|_| "Hàng đợi lịch sử đã đầy".to_string())?;
        Ticket {
            key,
            id,
            permit,
            raw_id: None,
        }
    };
    Ok(ticket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatMessage;
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zl-retained-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            Self(path)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn rows(start: usize, count: usize) -> Vec<Thread> {
        vec![Thread {
            id: "exact-thread".into(),
            messages: (start..start + count)
                .map(|n| ChatMessage {
                    id: format!("{n:06}"),
                    text: format!("Body {n}"),
                    ts: n as i64,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }]
    }
    fn session() -> Session {
        let mut s = Session::new(false, false, false).unwrap();
        s.login = Some(crate::session::LoginInfo {
            uin: "owner".into(),
            ..Default::default()
        });
        s.zpw_sek = Some("fixture-key".into());
        s
    }
    #[tokio::test]
    async fn failed_delta_remains_bounded_and_retries_all_offscreen_rows_after_reopen() {
        let root = Root::new();
        std::fs::write(&root.0, b"block storage").unwrap();
        let queue = Retention::new(2);
        let first = queue
            .stage(
                root.0.clone(),
                "owner".into(),
                rows(0, 700),
                Origin::Network,
                true,
                queue.reserve().await,
            )
            .unwrap();
        let second = queue
            .stage(
                root.0.clone(),
                "owner".into(),
                rows(700, 700),
                Origin::Cached,
                false,
                queue.reserve().await,
            )
            .unwrap();
        assert_eq!(queue.capacity.available_permits(), 0);
        assert!(queue.flush_sync().is_err());
        assert_eq!(queue.mailbox.snapshot().len(), 1);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), queue.reserve())
                .await
                .is_err()
        );
        std::fs::remove_file(&root.0).unwrap();
        queue.flush_sync().unwrap();
        drop(first);
        drop(second);
        assert_eq!(queue.capacity.available_permits(), 2);
        assert!(queue.mailbox.snapshot().is_empty());
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 1400);
    }
    #[tokio::test]
    async fn oversize_rejection_returns_original_page_and_reserved_capacity_without_cursor_commit()
    {
        let root = Root::new();
        let queue = Retention::new(1);
        let mut threads = rows(0, 1);
        threads[0].messages[0].text = "x".repeat(33 * 1024 * 1024);
        let rejected = queue
            .stage(
                root.0.clone(),
                "owner".into(),
                threads,
                Origin::Network,
                true,
                queue.reserve().await,
            )
            .unwrap_err();
        assert_eq!(rejected.threads[0].messages[0].text.len(), 33 * 1024 * 1024);
        assert_eq!(queue.capacity.available_permits(), 0);
        assert!(queue.mailbox.snapshot().is_empty());
        assert!(!root.0.exists());
        drop(rejected);
        assert_eq!(queue.capacity.available_permits(), 1);
    }
    #[tokio::test]
    async fn actual_canceled_transfer_waiter_leaves_page_for_lifecycle_flush() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        std::fs::write(&root.0, b"block storage").unwrap();
        let captured = session();
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut waiter = Box::pin(super::super::worker::persist_cached_transfer_at(
            Ok(root.0.clone()),
            session,
            captured,
            "owner",
            rows(2000, 501),
        ));
        let waker = futures_util::task::noop_waker();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(std::future::Future::poll(waiter.as_mut(), &mut context).is_pending());
        assert!(retained()
            .mailbox
            .snapshot()
            .keys()
            .any(|key| key.0 == root.0));
        drop(waiter);
        crate::runtime_scope::drain().await;
        assert!(flush_pending_sync().is_err());
        std::fs::remove_file(&root.0).unwrap();
        flush_pending_sync().unwrap();
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 501);
    }
    #[tokio::test]
    async fn gui_wake_lease_releases_failed_unpolled_and_late_consumers_without_losing_newer_wake()
    {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let generation = crate::runtime_scope::registry().generation();
        let key = ("wake-owner".to_string(), generation);
        assert!(stage_gui(key.0.clone(), generation, rows(0, 1)).unwrap());
        let lease = GuiWakeLease::new();
        let unpolled = async move {
            let _lease = lease;
            std::future::pending::<()>().await;
        };
        drop(unpolled);
        assert!(stage_gui(key.0.clone(), generation, rows(1, 1)).unwrap());
        let late = GuiWakeLease::new();
        gui_snapshots().release_wake();
        assert!(stage_gui(key.0.clone(), generation, rows(2, 1)).unwrap());
        drop(late);
        assert!(!stage_gui(key.0.clone(), generation, rows(3, 1)).unwrap());
        gui_snapshots().remove_if(&key, |_| true);
        gui_snapshots().release_wake();
    }
    #[tokio::test]
    async fn old_gui_generation_cannot_publish_after_lifecycle_drain() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        let old = crate::runtime_scope::transition("owner").unwrap();
        assert!(stage_gui("owner".into(), old.generation, rows(0, 1)).unwrap());
        let admitted = crate::runtime_scope::registry()
            .invalidate_if_current(old.generation)
            .unwrap();
        crate::runtime_scope::transition_if_current_fallible(admitted, "", || {
            flush_gui_sync(&root.0)?;
            assert!(stage_gui("owner".into(), old.generation, rows(1, 1)).is_err());
            Ok(())
        })
        .unwrap();
        assert!(gui_snapshots().snapshot().is_empty());
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 1);
    }
    #[tokio::test]
    async fn actual_network_consumer_chunks_large_valid_page_before_acknowledging_it() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        let captured = session();
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut page = rows(0, 40);
        for row in &mut page[0].messages {
            row.text = "x".repeat(1024 * 1024);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::worker::publish_account_threads_at(
            root.0.clone(),
            session,
            captured,
            GuiTx::for_test(tx),
            page,
            true,
        )
        .await;
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            events.len(),
            1,
            "Publish one accepted page only after all bounded SQL chunks acknowledge"
        );
        assert!(events
            .iter()
            .all(|event| matches!(event, GuiEvent::AccountThreadsUpdated { .. })));
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 40);
    }
    #[tokio::test]
    async fn actual_sync_sink_page_one_ack_page_two_unpolled_cancel_preserves_cursor_resume() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        let captured = session();
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut first = rows(100, 2);
        let mut second = rows(98, 4);
        let inserted = super::super::worker::persist_history_sync_page_at(
            Ok(root.0.clone()),
            session.clone(),
            captured.clone(),
            "owner".into(),
            first.remove(0),
            reserve().await,
        )
        .await
        .unwrap();
        assert_eq!(inserted, 2);
        let unpolled = super::super::worker::persist_history_sync_page_at(
            Ok(root.0.clone()),
            session.clone(),
            captured.clone(),
            "owner".into(),
            second.remove(0),
            reserve().await,
        );
        drop(unpolled);
        let cursor = history::execute(
            root.0.clone(),
            session.clone(),
            captured.clone(),
            "owner".into(),
            history::Request::OldestCursor("exact-thread".into()),
        )
        .await
        .unwrap();
        assert!(matches!(cursor,history::Reply::Cursor(ref value) if value=="000100"));
        flush_pending_sync().unwrap();
        let cursor = history::execute(
            root.0.clone(),
            session,
            captured,
            "owner".into(),
            history::Request::OldestCursor("exact-thread".into()),
        )
        .await
        .unwrap();
        assert!(matches!(cursor,history::Reply::Cursor(ref value) if value=="000098"));
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 4);
    }
    #[tokio::test]
    async fn actual_large_network_page_cancellation_retains_every_unwritten_chunk() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        std::fs::write(&root.0, b"block storage").unwrap();
        let captured = session();
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut page = rows(0, 40);
        for row in &mut page[0].messages {
            row.text = "x".repeat(1024 * 1024);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let mut waiter = Box::pin(super::super::worker::publish_account_threads_at(
            root.0.clone(),
            session,
            captured,
            GuiTx::for_test(tx),
            page,
            true,
        ));
        let waker = futures_util::task::noop_waker();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(std::future::Future::poll(waiter.as_mut(), &mut context).is_pending());
        let snapshot = retained().mailbox.snapshot();
        let pending = snapshot.iter().find(|(key, _)| key.0 == root.0).unwrap().1;
        let batch = pending.batches.last().unwrap();
        let chunks = batch.chunks.as_ref().unwrap();
        assert!(chunks.len() >= 2);
        assert_eq!(
            chunks
                .iter()
                .flatten()
                .map(|thread| thread.messages.len())
                .sum::<usize>(),
            40
        );
        assert_eq!(batch.next_chunk, 0);
        drop(snapshot);
        drop(waiter);
        crate::runtime_scope::drain().await;
        assert!(!rx
            .try_iter()
            .any(|event| matches!(event, GuiEvent::AccountThreadsUpdated { .. })));
        assert!(flush_pending_sync().is_err());
        std::fs::remove_file(&root.0).unwrap();
        flush_pending_sync().unwrap();
        assert!(!retained()
            .mailbox
            .snapshot()
            .keys()
            .any(|key| key.0 == root.0));
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 40);
    }
    #[tokio::test]
    async fn actual_network_consumer_rejects_one_unsupported_row_without_success_or_storage_ack() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        let captured = session();
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut page = rows(0, 1);
        page[0].messages[0].text = "x".repeat(33 * 1024 * 1024);
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::worker::publish_account_threads_at(
            root.0.clone(),
            session,
            captured,
            GuiTx::for_test(tx),
            page,
            true,
        )
        .await;
        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0],GuiEvent::StatusUpdate(error) if error.contains("32 MiB")));
        assert!(!root.0.exists());
    }

    #[tokio::test]
    async fn authorized_empty_auth_cleanup_flushes_immutable_history_but_normal_cross_account_rejects(
    ) {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        let old = crate::runtime_scope::transition("owner").unwrap();
        let ticket = crate::runtime_scope::run(old, async {
            stage(
                root.0.clone(),
                "owner".into(),
                rows(0, 3),
                Origin::Network,
                false,
                reserve().await,
            )
            .unwrap()
        })
        .await
        .unwrap();
        drop(ticket);
        let auth = crate::runtime_scope::transition("").unwrap();
        crate::runtime_scope::run(auth.clone(), async {
            assert!(AccountStore::open(&root.0, "owner").is_err());
            crate::runtime_scope::spawn_transition_blocking(move || {
                crate::runtime_scope::transition_if_current_fallible(
                    auth.generation,
                    "next",
                    flush_pending_sync,
                )
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        })
        .await
        .unwrap();
        let store = AccountStore::open(&root.0, "owner").unwrap();
        assert_eq!(store.thread_message_counts().unwrap()["exact-thread"], 3);
    }
    #[tokio::test]
    async fn raw_spool_ack_is_not_blocked_by_an_earlier_incompatible_history_database() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Root::new();
        let store = AccountStore::open(&root.0, "owner").unwrap();
        let db = store.path.clone().unwrap();
        drop(store);
        rusqlite::Connection::open(&db)
            .unwrap()
            .pragma_update(None, "user_version", 999)
            .unwrap();
        let normal = stage(
            root.0.clone(),
            "owner".into(),
            rows(0, 1),
            Origin::Network,
            false,
            reserve().await,
        )
        .unwrap();
        let raw = super::super::phone_pending::Page {
            owner: "owner".into(),
            own: "owner".into(),
            objects: Vec::new(),
            mapping: Default::default(),
            groups: Default::default(),
        };
        let raw = stage_raw(root.0.clone(), "owner".into(), raw, reserve().await).unwrap();
        let captured = session();
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        assert!(persist(&raw, session, captured, None).await.is_ok());
        assert_eq!(
            super::super::phone_pending::load(&root.0, "owner")
                .unwrap()
                .len(),
            1
        );
        rusqlite::Connection::open(&db)
            .unwrap()
            .pragma_update(None, "user_version", 3)
            .unwrap();
        flush_pending_sync().unwrap();
        drop(normal);
        super::super::phone_pending::remove(&root.0, "owner", raw.raw_id().unwrap()).unwrap();
    }
}
