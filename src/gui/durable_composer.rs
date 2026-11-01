//! Durable composer consumer: coalesced draft writes, stage-before-network, explicit recovery.
use super::*;
use crate::composer_store::{ComposerStore, Draft, OutboxEntry};
use crate::message_model::DeliveryState;
use std::{collections::BTreeSet, path::PathBuf, sync::OnceLock};

type Key = (String, String);
#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    pub revision: u64,
    pub draft: Draft,
    pub generation: u64,
}
fn mailbox() -> &'static crate::latest_snapshot::LatestSnapshot<Key, Snapshot> {
    static MAILBOX: OnceLock<crate::latest_snapshot::LatestSnapshot<Key, Snapshot>> =
        OnceLock::new();
    MAILBOX.get_or_init(|| crate::latest_snapshot::LatestSnapshot::new(32))
}
// One lane orders saves, staging, recovery and acknowledgements. No UI code enters it.
static LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) struct WakeLease(u64);
impl WakeLease {
    pub(super) fn new() -> Self {
        Self(mailbox().wake_token())
    }
}
impl Drop for WakeLease {
    fn drop(&mut self) {
        mailbox().release_wake_for(self.0);
    }
}
fn active() -> &'static std::sync::Mutex<BTreeSet<(PathBuf, String, String)>> {
    static ACTIVE: OnceLock<std::sync::Mutex<BTreeSet<(PathBuf, String, String)>>> =
        OnceLock::new();
    ACTIVE.get_or_init(Default::default)
}
struct ActiveEntry((PathBuf, String, String));
impl Drop for ActiveEntry {
    fn drop(&mut self) {
        active()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}
/// GUI-owned bookkeeping contains no file handles and never performs disk I/O.
#[derive(Default)]
pub(super) struct State {
    pub attachments: Vec<PathBuf>,
    pub recovery: Vec<OutboxEntry>,
    pub staging: BTreeSet<String>,
    account: String,
    thread: String,
    revision: u64,
    fingerprint: String,
    requested: BTreeSet<String>,
    attachment_thread: String,
    by_thread: std::collections::BTreeMap<String, Vec<PathBuf>>,
}
impl State {
    pub(super) fn add_path(&mut self, thread: &str, path: PathBuf) {
        if self.attachment_thread == thread {
            self.attachments.push(path);
        } else {
            self.by_thread.entry(thread.into()).or_default().push(path);
        }
    }
    pub(super) fn switching(&self, account: &str, thread: &str) -> bool {
        self.account != account || self.attachment_thread != thread
    }
    pub(super) fn activate(&mut self, account: &str, thread: &str) {
        if self.account != account {
            *self = Self {
                account: account.into(),
                ..Default::default()
            };
        }
        if self.attachment_thread != thread {
            self.by_thread.insert(
                self.attachment_thread.clone(),
                std::mem::take(&mut self.attachments),
            );
            self.attachments = self.by_thread.remove(thread).unwrap_or_default();
            self.attachment_thread = thread.into();
        }
    }
    pub(super) fn reset_loads(&mut self) {
        mailbox().release_wake();
        self.requested.clear();
        self.thread.clear();
        self.staging.clear();
    }
    pub(super) fn staged(&mut self, thread: &str) {
        self.staging.remove(thread);
        if self.thread == thread {
            self.revision = self.revision.saturating_add(1);
        }
    }
    pub(super) fn reload_later(&mut self, thread: &str) {
        self.requested.remove(thread);
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn observe(
        &mut self,
        account: &str,
        thread: &str,
        draft: Draft,
    ) -> Result<(), String> {
        if account.is_empty() || thread.is_empty() {
            return Ok(());
        }
        if self.account != account {
            *self = Self {
                account: account.into(),
                ..Default::default()
            };
        }
        let fingerprint = serde_json::to_string(&draft).map_err(|e| e.to_string())?;
        if draft.text.len() > 128 * 1024
            || draft.attachments.len() > 32
            || fingerprint.len() > 256 * 1024
        {
            return Err(
                "Nháp vượt giới hạn lưu (128 KiB văn bản, 32 tệp, 256 KiB metadata)".into(),
            );
        }
        if self.thread != thread {
            self.thread = thread.into();
            self.fingerprint =
                if draft.text.is_empty() && draft.reply.is_none() && draft.attachments.is_empty() {
                    fingerprint.clone()
                } else {
                    String::new()
                };
            self.revision = self.revision.saturating_add(1);
            if self.requested.insert(thread.into()) {
                send(WorkerCmd::LoadComposer {
                    account: account.into(),
                    thread: thread.into(),
                    revision: self.revision,
                })?;
            }
        }
        if self.fingerprint != fingerprint {
            self.revision = self.revision.saturating_add(1);
            let wake = mailbox()
                .publish(
                    (account.into(), thread.into()),
                    Snapshot {
                        revision: self.revision,
                        draft,
                        generation: WORKER_TX
                            .get()
                            .map(|tx| tx.ui_generation())
                            .unwrap_or_else(|| crate::runtime_scope::registry().generation()),
                    },
                )
                .map_err(|_| "Hàng đợi nháp đã đầy; nội dung vẫn ở ô soạn".to_string())?;
            if wake {
                if let Err(error) = send(WorkerCmd::FlushComposer) {
                    mailbox().release_wake();
                    return Err(error);
                }
            }
            self.fingerprint = fingerprint;
        }
        Ok(())
    }
    pub(super) fn attachments_match(&self, thread: &str, paths: &[PathBuf]) -> bool {
        if self.attachment_thread == thread {
            self.attachments == paths
        } else {
            self.by_thread
                .get(thread)
                .map(Vec::as_slice)
                .unwrap_or_default()
                == paths
        }
    }
    pub(super) fn acknowledge_attachments(&mut self, thread: &str, paths: &[PathBuf]) {
        if self.attachment_thread == thread {
            if self.attachments == paths {
                self.attachments.clear();
            }
        } else if self
            .by_thread
            .get(thread)
            .is_some_and(|saved| saved == paths)
        {
            self.by_thread.remove(thread);
        }
    }
    pub(super) fn accept_load(&mut self, account: &str, thread: &str, revision: u64) -> bool {
        self.account == account && self.thread == thread && self.revision == revision
    }
    pub(super) fn invalidate_snapshot(&mut self) {
        self.fingerprint.clear();
    }
}
fn send(command: WorkerCmd) -> Result<(), String> {
    WORKER_TX
        .get()
        .ok_or_else(|| "Bộ xử lý chưa sẵn sàng".to_string())?
        .send(command)
        .map_err(|_| "Bộ xử lý đã đóng; nháp vẫn được giữ".into())
}

async fn disk<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    crate::runtime_scope::spawn_blocking(f).await??
}
/// Flush the bounded immutable batch only from an authorized lifecycle barrier.
/// Payloads are acknowledged after each successful write; a failed write remains for retry.
pub(super) fn flush_pending_sync(root: &std::path::Path) -> anyhow::Result<()> {
    let result = (|| {
        let batch = mailbox().snapshot();
        for ((account, thread), snapshot) in batch {
            ComposerStore::open(root, &account)?.set_draft(&thread, snapshot.draft)?;
            mailbox().remove_if(&(account, thread), |pending| {
                pending.revision == snapshot.revision && pending.generation == snapshot.generation
            });
        }
        Ok(())
    })();
    mailbox().release_wake();
    result
}
pub(super) async fn flush(root: PathBuf, sender: GuiTx) {
    tokio::time::sleep(Duration::from_millis(120)).await;
    let _lane = LANE.lock().await;
    let batch = mailbox().snapshot();
    for ((account, thread), snapshot) in batch {
        let generation = crate::runtime_scope::execution_scope().map(|s| s.generation);
        if generation.is_some_and(|g| g != snapshot.generation) {
            continue;
        }
        let root = root.clone();
        let owner = account.clone();
        let target = thread.clone();
        let draft = snapshot.draft;
        match disk(move || ComposerStore::open(&root, &owner)?.set_draft(&target, draft)).await {
            Ok(()) => mailbox().remove_if(&(account, thread), |pending| {
                pending.revision == snapshot.revision && pending.generation == snapshot.generation
            }),
            Err(error) => {
                let _ = sender.send(GuiEvent::StatusUpdate(format!(
                    "Nháp chưa lưu: {error}; hãy thử lưu lại trước khi đóng"
                )));
            }
        }
    }
}
fn open_recovered(root: &std::path::Path, account: &str) -> anyhow::Result<ComposerStore> {
    static RECOVERED: OnceLock<std::sync::Mutex<BTreeSet<(PathBuf, String, u64)>>> =
        OnceLock::new();
    let scope = crate::runtime_scope::execution_scope();
    if let Some(scope) = &scope {
        anyhow::ensure!(scope.account == account, "Composer account changed");
    }
    let generation = scope.map(|s| s.generation).unwrap_or(0);
    let key = (root.to_path_buf(), account.into(), generation);
    let mut recovered = RECOVERED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut store = ComposerStore::open(root, account)?;
    if !recovered.contains(&key) {
        store.recover_interrupted()?;
        recovered.insert(key);
    }
    Ok(store)
}
pub(super) async fn load(
    root: PathBuf,
    account: String,
    thread: String,
    revision: u64,
    sender: GuiTx,
) {
    let _lane = LANE.lock().await;
    let owner = account.clone();
    let target = thread.clone();
    let result = disk(move || {
        let store = open_recovered(&root, &owner)?;
        Ok((store.draft(&target)?, store.pending()?))
    })
    .await
    .map_err(|e| e.to_string());
    let _ = sender.send(GuiEvent::ComposerLoaded {
        account,
        thread,
        revision,
        result,
    });
}
pub(super) async fn pick(
    root: PathBuf,
    account: String,
    thread: String,
    path: PathBuf,
    sender: GuiTx,
) {
    flush(root.clone(), sender.clone()).await;
    let _lane = LANE.lock().await;
    let owner = account.clone();
    let target = thread.clone();
    let selected = path.clone();
    match disk(move || {
        let mut store = open_recovered(&root, &owner)?;
        let mut draft = store.draft(&target)?;
        draft.attachments.push(selected);
        store.set_draft(&target, draft)
    })
    .await
    {
        Ok(()) => {
            let _ = sender.send(GuiEvent::ComposerAttachmentPicked {
                account,
                thread,
                path,
            });
        }
        Err(error) => {
            let _ = sender.send(GuiEvent::StatusUpdate(format!(
                "Chưa lưu được tệp vào nháp: {error}"
            )));
        }
    }
}
pub(super) async fn recover(
    root: PathBuf,
    account: String,
    id: String,
    revision: u64,
    sender: GuiTx,
) {
    flush(root.clone(), sender.clone()).await;
    let _lane = LANE.lock().await;
    let owner = account.clone();
    let result = disk(move || {
        let mut store = ComposerStore::open(&root, &owner)?;
        if store
            .entry(&id)?
            .is_some_and(|entry| entry.state == DeliveryState::Pending)
        {
            anyhow::ensure!(
                !active()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .contains(&(root.clone(), owner.clone(), id.clone())),
                "Lượt gửi vẫn đang chạy"
            );
            store.complete(&id, DeliveryState::DeliveryUnknown)?;
        }
        store.recover_to_draft(&id)
    })
    .await
    .map_err(|e| e.to_string());
    let _ = sender.send(GuiEvent::ComposerRecovered {
        account,
        revision,
        result,
    });
}

/// Transport boundary used by the real worker and deterministic consumer tests.
trait Transport {
    fn text(
        &self,
        thread: &str,
        group: bool,
        draft: &Draft,
        id: u64,
    ) -> impl std::future::Future<Output = anyhow::Result<ChatMessage>> + Send;
    fn attachment(
        &self,
        thread: &str,
        group: bool,
        path: &std::path::Path,
        id: u64,
    ) -> impl std::future::Future<Output = anyhow::Result<ChatMessage>> + Send;
}
struct Live(Session);
impl Transport for Live {
    async fn text(
        &self,
        thread: &str,
        group: bool,
        draft: &Draft,
        id: u64,
    ) -> anyhow::Result<ChatMessage> {
        if draft.reply.is_none() && draft.mentions.is_empty() {
            if let Some(url) = crate::chat::extract_first_url(&draft.text) {
                let meta = crate::chat::parse_link_metadata(&self.0, &url)
                    .await
                    .unwrap_or_else(|_| crate::chat::LinkMetadata {
                        url: url.clone(),
                        title: url,
                        desc: String::new(),
                        thumb: String::new(),
                        src: "Liên kết".into(),
                    });
                let response = crate::chat::send_link_message_with_client_id(
                    &self.0,
                    thread,
                    &draft.text,
                    &meta,
                    group,
                    id,
                )
                .await?;
                let mut msg = super::worker::composed_sent_message(
                    &draft.text,
                    &[],
                    None,
                    id,
                    super::worker::extract_msg_id(&response),
                );
                msg.link_url = Some(meta.url);
                msg.link_title = Some(meta.title);
                msg.link_desc = Some(meta.desc);
                msg.link_thumb = Some(meta.thumb);
                msg.link_src = Some(meta.src);
                return Ok(msg);
            }
        }
        let response = if let Some(quote) = &draft.reply {
            crate::chat::send_quote_message_with_mentions_and_client_id(
                &self.0,
                thread,
                &draft.text,
                quote,
                group,
                id,
                &draft.mentions,
            )
            .await?
        } else if group {
            crate::chat::send_group_message_with_mentions_and_client_id(
                &self.0,
                thread,
                &draft.text,
                id,
                &draft.mentions,
            )
            .await?
        } else {
            crate::chat::send_message_with_client_id(&self.0, thread, &draft.text, id).await?
        };
        Ok(super::worker::composed_sent_message(
            &draft.text,
            &draft.mentions,
            draft.reply.as_ref(),
            id,
            super::worker::extract_msg_id(&response),
        ))
    }
    async fn attachment(
        &self,
        thread: &str,
        group: bool,
        path: &std::path::Path,
        id: u64,
    ) -> anyhow::Result<ChatMessage> {
        crate::chat::send_attachment_with_client_id(&self.0, thread, group, path, "", id)
            .await
            .map(|(_, msg)| msg)
    }
}
#[derive(Clone)]
pub(super) struct Request {
    pub account: String,
    pub thread: String,
    pub group: bool,
    pub draft: Draft,
    pub revision: u64,
}
struct Execution {
    messages: Vec<ChatMessage>,
    error: Option<String>,
}
async fn execute<T: Transport + Sync>(
    root: PathBuf,
    request: Request,
    transport: &T,
    staged: impl FnOnce(),
) -> anyhow::Result<Execution> {
    let Request {
        account,
        thread,
        group,
        draft,
        revision,
    } = request;
    crate::runtime_scope::check()?;
    crate::composer_mentions::validate_mentions(&draft.text, &draft.mentions)?;
    anyhow::ensure!(
        group || draft.mentions.is_empty(),
        "Nhắc tên chỉ dành cho nhóm"
    );
    let id = {
        let _lane = LANE.lock().await;
        let root = root.clone();
        let account = account.clone();
        let thread = thread.clone();
        let draft = draft.clone();
        let key = (account.clone(), thread.clone());
        let id = disk(move || open_recovered(&root, &account)?.stage(&thread, draft)).await?;
        mailbox().remove_if(&key, |s| s.revision <= revision);
        id
    };
    let active_key = (root.clone(), account.clone(), id.clone());
    active()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(active_key.clone());
    let _active = ActiveEntry(active_key);
    staged();
    let entry = {
        let root = root.clone();
        let owner = account.clone();
        let id = id.clone();
        disk(move || {
            ComposerStore::open(&root, &owner)?
                .entry(&id)?
                .ok_or_else(|| anyhow::anyhow!("Staged entry missing"))
        })
        .await
    };
    let entry = match entry {
        Ok(entry) => entry,
        Err(error) => {
            return Ok(Execution {
                messages: Vec::new(),
                error: Some(format!(
                    "Lượt {id} đã lưu; chưa đọc được trạng thái: {error}. Không tự gửi lại"
                )),
            })
        }
    };
    if let Err(error) = crate::runtime_scope::check() {
        return Ok(Execution { messages: Vec::new(), error: Some(format!("Lượt {id} đã lưu nhưng phiên đã đổi: {error}; kiểm tra lịch sử trước khi khôi phục")) });
    }
    let mut messages = Vec::new();
    let text_result = if draft.text.is_empty() {
        Ok(None)
    } else {
        transport
            .text(
                &thread,
                group,
                &draft,
                id.parse().expect("Persisted decimal request ID"),
            )
            .await
            .map(Some)
    };
    match text_result {
        Ok(message) => {
            if let Some(message) = message {
                messages.push(message);
            }
            if let Err(error) = complete(
                root.clone(),
                account.clone(),
                id.clone(),
                None,
                DeliveryState::ServerAccepted,
            )
            .await
            {
                return Ok(Execution { messages, error: Some(format!("Máy chủ đã xác nhận nhưng chưa lưu trạng thái {id}: {error}; không tự gửi lại")) });
            }
        }
        Err(error) => {
            let persisted = complete(
                root,
                account,
                id.clone(),
                None,
                DeliveryState::DeliveryUnknown,
            )
            .await;
            let persistence_note = persisted
                .err()
                .map(|e| format!("; chưa lưu được trạng thái chưa rõ: {e}"))
                .unwrap_or_default();
            return Ok(Execution { messages, error: Some(format!("Chưa rõ kết quả gửi {id}; kiểm tra lịch sử trước khi khôi phục: {error}{persistence_note}")) });
        }
    }
    for (path, attachment_id) in draft.attachments.iter().zip(&entry.attachment_client_ids) {
        match transport.attachment(&thread, group, path, attachment_id.parse().expect("Persisted decimal attachment ID")).await {
            Ok(message) => {
                messages.push(message);
                if let Err(error) = complete(root.clone(), account.clone(), id.clone(), Some(attachment_id.clone()), DeliveryState::ServerAccepted).await {
                    return Ok(Execution { messages, error: Some(format!("Tệp đã được máy chủ xác nhận nhưng chưa lưu trạng thái {attachment_id}: {error}; kiểm tra lịch sử trước khi khôi phục")) });
                }
            }
            Err(error) => return Ok(Execution { messages, error: Some(format!("Tệp chưa xác nhận trong lượt gửi {id}; văn bản đã xác nhận sẽ không gửi lại: {error}")) }),
        }
    }
    Ok(Execution {
        messages,
        error: None,
    })
}
async fn complete(
    root: PathBuf,
    account: String,
    id: String,
    attachment: Option<String>,
    state: DeliveryState,
) -> anyhow::Result<()> {
    let _lane = LANE.lock().await;
    disk(move || {
        let mut store = ComposerStore::open(&root, &account)?;
        if let Some(attachment) = attachment {
            store.complete_attachment_id(&id, &attachment)
        } else {
            store.complete(&id, state)
        }
    })
    .await
}
pub(super) async fn deliver(root: PathBuf, session: Session, sender: GuiTx, request: Request) {
    let Request {
        account,
        thread,
        draft,
        ..
    } = request.clone();
    if session.login.as_ref().map(|l| l.uin.as_str()) != Some(account.as_str()) {
        return;
    }
    let staged_draft = draft.clone();
    let result = execute(root.clone(), request, &Live(session), || {
        let _ = sender.send(GuiEvent::ComposerStaged {
            account: account.clone(),
            thread: thread.clone(),
            draft: staged_draft,
        });
    })
    .await;
    let owner = account.clone();
    let pending = disk(move || ComposerStore::open(&root, &owner)?.pending()).await;
    emit_result(&sender, &account, &thread, result);
    if let Ok(entries) = pending {
        let _ = sender.send(GuiEvent::ComposerOutbox { account, entries });
    }
}

fn emit_result(sender: &GuiTx, account: &str, thread: &str, result: anyhow::Result<Execution>) {
    match result {
        Ok(outcome) => {
            for msg in outcome.messages {
                let _ = sender.send(GuiEvent::ComposerMessageSent {
                    account: account.to_string(),
                    thread_id: thread.to_string(),
                    msg,
                });
            }
            if let Some(error) = outcome.error {
                let _ = sender.send(GuiEvent::StatusUpdate(error));
            }
        }
        Err(error) => {
            let _ = sender.send(GuiEvent::ComposerStageFailed {
                account: account.to_string(),
                thread: thread.to_string(),
                error: error.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Fixture {
        root: PathBuf,
        calls: Mutex<Vec<u64>>,
        fail_text: bool,
        fail_attachment: usize,
        break_ack_lock: bool,
    }
    impl Transport for Fixture {
        async fn text(
            &self,
            _thread: &str,
            _group: bool,
            draft: &Draft,
            id: u64,
        ) -> anyhow::Result<ChatMessage> {
            let entry = ComposerStore::open(&self.root, "A")?
                .entry(&id.to_string())?
                .unwrap();
            assert_eq!(
                entry.draft.text, draft.text,
                "Network consumer must observe the staged raw payload"
            );
            assert_eq!(entry.draft.mentions, draft.mentions);
            self.calls.lock().unwrap().push(id);
            if self.break_ack_lock {
                let directory = std::fs::read_dir(self.root.join("accounts"))?
                    .next()
                    .unwrap()?
                    .path();
                let lock = directory.join("composer.lock.sqlite3");
                std::fs::remove_file(&lock)?;
                std::fs::create_dir(&lock)?;
            }
            anyhow::ensure!(!self.fail_text, "Synthetic uncertain response");
            Ok(super::super::worker::composed_sent_message(
                &draft.text,
                &draft.mentions,
                draft.reply.as_ref(),
                id,
                "server-text".into(),
            ))
        }
        async fn attachment(
            &self,
            _thread: &str,
            _group: bool,
            path: &std::path::Path,
            id: u64,
        ) -> anyhow::Result<ChatMessage> {
            let store = ComposerStore::open(&self.root, "A")?;
            let entries = store.pending()?;
            assert!(
                entries.iter().any(
                    |entry| entry.attachment_client_ids.contains(&id.to_string())
                        && entry.draft.attachments.iter().any(|p| p == path)
                ),
                "Attachment request ID must exist durably before upload"
            );
            let mut calls = self.calls.lock().unwrap();
            calls.push(id);
            anyhow::ensure!(
                calls.len() != self.fail_attachment,
                "Synthetic attachment failure"
            );
            Ok(ChatMessage {
                id: format!("server-{id}"),
                client_message_id: id.to_string(),
                is_outgoing: true,
                ..Default::default()
            })
        }
    }
    fn temp(label: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "durable-consumer-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
    fn sample() -> Draft {
        Draft {
            text: "😀 @An ".into(),
            mentions: vec![crate::composer_mentions::OutgoingMention {
                uid: "22".into(),
                pos: 3,
                len: 3,
                kind: 0,
            }],
            reply: Some(ChatMessage {
                id: "123".into(),
                text: "Raw :+1:".into(),
                ..Default::default()
            }),
            attachments: vec![PathBuf::from("same.png"), PathBuf::from("same.png")],
        }
    }
    #[tokio::test]
    async fn durable_composer_stage_precedes_network_and_partial_ack_never_resends_text() {
        let root = temp("partial");
        let fake = Fixture {
            root: root.clone(),
            calls: Mutex::new(Vec::new()),
            fail_text: false,
            fail_attachment: 3,
            break_ack_lock: false,
        };
        let outcome = execute(
            root.clone(),
            Request {
                account: "A".into(),
                thread: "group-1".into(),
                group: true,
                draft: sample(),
                revision: 1,
            },
            &fake,
            || {},
        )
        .await
        .unwrap();
        assert_eq!(
            outcome.messages.len(),
            2,
            "Confirmed text and first attachment remain visible after partial failure"
        );
        assert!(outcome.error.is_some());
        let calls = fake.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 3);
        assert!(calls.windows(2).all(|p| p[0] != p[1]));
        let mut reboot = ComposerStore::open(&root, "A").unwrap();
        reboot.recover_interrupted().unwrap();
        let pending = reboot.pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].state, DeliveryState::ServerAccepted);
        assert_eq!(pending[0].attachment_client_ids, [calls[2].to_string()]);
        let (_, remaining) = reboot.recover_to_draft(&pending[0].id).unwrap();
        assert!(
            remaining.text.is_empty() && remaining.reply.is_none() && remaining.mentions.is_empty()
        );
        assert_eq!(remaining.attachments.len(), 1);
        assert_eq!(
            fake.calls.lock().unwrap().len(),
            3,
            "Recovery performs zero transport work"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_unknown_reboot_preserves_newer_edits_and_exact_mentions() {
        let root = temp("unknown");
        let fake = Fixture {
            root: root.clone(),
            calls: Mutex::new(Vec::new()),
            fail_text: true,
            fail_attachment: 0,
            break_ack_lock: false,
        };
        let mut draft = sample();
        draft.attachments.clear();
        let outcome = execute(
            root.clone(),
            Request {
                account: "A".into(),
                thread: "group-1".into(),
                group: true,
                draft: draft.clone(),
                revision: 2,
            },
            &fake,
            || {},
        )
        .await
        .unwrap();
        assert!(outcome.error.is_some() && outcome.messages.is_empty());
        let mut reboot = ComposerStore::open(&root, "A").unwrap();
        let pending = reboot.pending().unwrap();
        assert_eq!(pending[0].state, DeliveryState::DeliveryUnknown);
        reboot
            .set_draft(
                "group-1",
                Draft {
                    text: "new edit".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(reboot.recover_to_draft(&pending[0].id).is_err());
        assert_eq!(reboot.draft("group-1").unwrap().text, "new edit");
        reboot.set_draft("group-1", Draft::default()).unwrap();
        let (_, restored) = reboot.recover_to_draft(&pending[0].id).unwrap();
        let mut model = crate::composer_mentions::ComposerMentions::default();
        let mut text = String::new();
        let mut reply = None;
        model.switch_thread("group-1", &mut text, &mut reply);
        assert!(model.restore_failed(
            "group-1",
            restored.text,
            &restored.mentions,
            restored.reply,
            &mut text,
            &mut reply
        ));
        assert_eq!(model.outgoing(&text), draft.mentions);
        assert_eq!(reply.unwrap().text, "Raw :+1:");
        assert!(ComposerStore::open(&root, "B")
            .unwrap()
            .pending()
            .unwrap()
            .is_empty());
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_load_reboot_emits_draft_with_uid_reply_and_thread_guard() {
        let root = temp("load");
        let draft = sample();
        ComposerStore::open(&root, "A")
            .unwrap()
            .set_draft("group-1", draft.clone())
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        load(
            root.clone(),
            "A".into(),
            "group-1".into(),
            7,
            GuiTx {
                tx,
                ctx: egui::Context::default(),
                scope: None,
            },
        )
        .await;
        let GuiEvent::ComposerLoaded {
            account,
            thread,
            revision,
            result,
        } = rx.recv().unwrap()
        else {
            panic!("Actual load consumer must emit its terminal event")
        };
        let (loaded, _) = result.unwrap();
        assert_eq!(loaded.text, draft.text);
        assert_eq!(loaded.mentions, draft.mentions);
        assert_eq!(loaded.attachments, draft.attachments);
        let mut state = State {
            account,
            thread,
            revision,
            ..Default::default()
        };
        assert!(state.accept_load("A", "group-1", 7));
        state.revision += 1;
        assert!(!state.accept_load("A", "group-1", 7));
        assert!(!state.accept_load("B", "group-1", 8));
        assert!(!state.accept_load("A", "group-2", 8));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_stage_failure_never_calls_transport_or_clears_draft() {
        let root = temp("stage-fail");
        std::fs::write(root.join("accounts"), "Synthetic filesystem failure").unwrap();
        let fake = Fixture {
            root: root.clone(),
            calls: Mutex::new(Vec::new()),
            fail_text: false,
            fail_attachment: 0,
            break_ack_lock: false,
        };
        let draft = sample();
        let staged = std::sync::atomic::AtomicBool::new(false);
        assert!(execute(
            root.clone(),
            Request {
                account: "A".into(),
                thread: "group-1".into(),
                group: true,
                draft: draft.clone(),
                revision: 1
            },
            &fake,
            || {
                staged.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        )
        .await
        .is_err());
        assert!(!staged.load(std::sync::atomic::Ordering::Relaxed));
        assert!(fake.calls.lock().unwrap().is_empty());
        assert_eq!(draft.text, "😀 @An ");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_lifecycle_flush_retains_failed_batch_and_saves_on_explicit_retry() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = temp("flush");
        std::fs::write(root.join("accounts"), "Synthetic filesystem failure").unwrap();
        let scope = crate::runtime_scope::transition("flush-A").unwrap();
        let key = ("flush-A".to_string(), "thread".to_string());
        mailbox()
            .publish(
                key.clone(),
                Snapshot {
                    revision: 100,
                    generation: scope.generation,
                    draft: sample(),
                },
            )
            .ok()
            .unwrap();
        assert!(
            crate::runtime_scope::transition_if_current_fallible(scope.generation, "", || {
                flush_pending_sync(&root)
            })
            .is_err()
        );
        assert!(mailbox().snapshot().contains_key(&key));
        assert_eq!(
            crate::runtime_scope::registry().generation(),
            scope.generation
        );
        std::fs::remove_file(root.join("accounts")).unwrap();
        crate::runtime_scope::transition_if_current_fallible(scope.generation, "", || {
            flush_pending_sync(&root)
        })
        .unwrap();
        assert!(!mailbox().snapshot().contains_key(&key));
        assert_eq!(
            ComposerStore::open(&root, "flush-A")
                .unwrap()
                .draft("thread")
                .unwrap()
                .mentions[0]
                .uid,
            "22"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_post_submit_persistence_error_never_claims_not_sent() {
        let root = temp("ack-fail");
        let fake = Fixture {
            root: root.clone(),
            calls: Mutex::new(Vec::new()),
            fail_text: true,
            fail_attachment: 0,
            break_ack_lock: true,
        };
        let result = execute(
            root.clone(),
            Request {
                account: "A".into(),
                thread: "group-1".into(),
                group: true,
                draft: sample(),
                revision: 1,
            },
            &fake,
            || {},
        )
        .await;
        assert!(
            result.is_ok(),
            "Post-stage uncertainty must never be classified as staging failure"
        );
        let (tx, rx) = std::sync::mpsc::channel();
        emit_result(&GuiTx::for_test(tx), "A", "group-1", result);
        let event = rx.recv().unwrap();
        let GuiEvent::StatusUpdate(message) = event else {
            panic!("Expected explicit uncertain-delivery warning, not StageFailed")
        };
        assert!(
            message.contains("Chưa rõ kết quả gửi") && message.contains("chưa lưu được trạng thái")
        );
        let pending = ComposerStore::open(&root, "A").unwrap().pending().unwrap();
        assert_eq!(pending[0].state, DeliveryState::Pending);
        assert_eq!(pending[0].draft.text, "😀 @An ");
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_canceled_unpolled_flush_releases_only_its_own_wake() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let key = ("wake-A".into(), "thread".into());
        assert!(mailbox()
            .publish(
                key.clone(),
                Snapshot {
                    revision: 1,
                    generation: 0,
                    draft: Draft::default()
                }
            )
            .unwrap());
        let old = WakeLease::new();
        let never_polled = async move {
            let _wake = old;
            std::future::pending::<()>().await;
        };
        drop(never_polled);
        assert!(mailbox()
            .publish(
                key.clone(),
                Snapshot {
                    revision: 2,
                    generation: 0,
                    draft: Draft::default()
                }
            )
            .unwrap());
        let late = WakeLease::new();
        mailbox().release_wake();
        assert!(mailbox()
            .publish(
                key.clone(),
                Snapshot {
                    revision: 3,
                    generation: 0,
                    draft: Draft::default()
                }
            )
            .unwrap());
        drop(late);
        assert!(
            !mailbox()
                .publish(
                    key.clone(),
                    Snapshot {
                        revision: 4,
                        generation: 0,
                        draft: Draft::default()
                    }
                )
                .unwrap(),
            "Old cancellation must not clear the newer active wake"
        );
        mailbox().remove_if(&key, |_| true);
        mailbox().release_wake();
    }
    #[tokio::test]
    async fn durable_composer_same_account_transition_flushes_before_queued_wake_and_next_edit_saves(
    ) {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = temp("same-account");
        let initial = crate::runtime_scope::transition("A").unwrap();
        let key = ("A".into(), "thread".into());
        assert!(mailbox()
            .publish(
                key.clone(),
                Snapshot {
                    revision: 1,
                    generation: initial.generation,
                    draft: Draft {
                        text: "before transition".into(),
                        ..Default::default()
                    }
                }
            )
            .unwrap());
        let result = crate::runtime_scope::run(initial.clone(), async {
            let destination = root.clone();
            let committed = crate::runtime_scope::spawn_transition_blocking(move || {
                crate::runtime_scope::transition_if_current_fallible(
                    initial.generation,
                    "A",
                    || flush_pending_sync(&destination),
                )
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(
                ComposerStore::open(&root, "A")
                    .unwrap()
                    .draft("thread")
                    .unwrap()
                    .text,
                "before transition"
            );
            assert!(
                mailbox()
                    .publish(
                        key.clone(),
                        Snapshot {
                            revision: 2,
                            generation: committed.generation,
                            draft: Draft {
                                text: "after transition".into(),
                                ..Default::default()
                            }
                        }
                    )
                    .unwrap(),
                "Stale queued wake must not latch autosave"
            );
            let wake = WakeLease::new();
            let (tx, _rx) = std::sync::mpsc::channel();
            flush(root.clone(), GuiTx::for_test(tx)).await;
            drop(wake);
            assert_eq!(
                ComposerStore::open(&root, "A")
                    .unwrap()
                    .draft("thread")
                    .unwrap()
                    .text,
                "after transition"
            );
        })
        .await;
        assert!(result.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_selected_attachment_persists_without_sending_or_losing_reply() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = temp("selected-path");
        ComposerStore::open(&root, "A")
            .unwrap()
            .set_draft("thread-A", sample())
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        pick(
            root.clone(),
            "A".into(),
            "thread-A".into(),
            "selected.png".into(),
            GuiTx::for_test(tx),
        )
        .await;
        let GuiEvent::ComposerAttachmentPicked {
            account,
            thread,
            path,
        } = rx.recv().unwrap()
        else {
            panic!("Path selection must emit a thread-bound saved-draft event")
        };
        let draft = ComposerStore::open(&root, "A")
            .unwrap()
            .draft("thread-A")
            .unwrap();
        assert_eq!(draft.text, "😀 @An ");
        assert_eq!(draft.mentions[0].uid, "22");
        assert_eq!(draft.reply.unwrap().text, "Raw :+1:");
        assert_eq!(draft.attachments.len(), 3);
        assert!(
            ComposerStore::open(&root, "A")
                .unwrap()
                .pending()
                .unwrap()
                .is_empty(),
            "Picking never stages or sends automatically"
        );
        let mut ui = State::default();
        ui.activate(&account, "thread-B");
        ui.add_path(&thread, path);
        assert!(ui.attachments.is_empty());
        ui.activate(&account, "thread-A");
        assert_eq!(ui.attachments, [PathBuf::from("selected.png")]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn durable_composer_empty_auth_attempt_can_flush_only_intentional_old_owner_snapshot() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = temp("auth-empty");
        let scope = crate::runtime_scope::transition("").unwrap();
        let key = ("old-A".into(), "thread".into());
        mailbox()
            .publish(
                key.clone(),
                Snapshot {
                    revision: 1,
                    generation: scope.generation.saturating_sub(1),
                    draft: Draft {
                        text: "old owner raw".into(),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
        let result = crate::runtime_scope::run(scope.clone(), async {
            let destination = root.clone();
            crate::runtime_scope::spawn_transition_blocking(move || {
                crate::runtime_scope::transition_if_current_fallible(
                    scope.generation,
                    "new-B",
                    || flush_pending_sync(&destination),
                )
            })
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            assert_eq!(
                crate::runtime_scope::execution_scope().unwrap().account,
                "new-B"
            );
            assert!(ComposerStore::open(&root, "new-B")
                .unwrap()
                .draft("thread")
                .unwrap()
                .text
                .is_empty());
        })
        .await;
        assert!(result.is_some());
        assert_eq!(
            ComposerStore::open(&root, "old-A")
                .unwrap()
                .draft("thread")
                .unwrap()
                .text,
            "old owner raw"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
