//! Account-scoped storage operations used by the GUI worker, never by the render thread.
use crate::{
    chat::{ChatMessage, Thread},
    session::Session,
    store_v2::{AccountStore, HistoryCursor, MessageKey, PageDirection, StoreDelta},
};
use std::{path::PathBuf, sync::Arc};

// Opening one connection per operation keeps every handle inside the blocking pool.
// The lane also orders snapshot writes against local tombstones and explicit clears.
static STORAGE_LANE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) enum Request {
    #[cfg(test)]
    PauseBeforeMutation {
        entered: std::sync::mpsc::SyncSender<()>,
        release: std::sync::mpsc::Receiver<()>,
        request: Box<Request>,
    },
    VisibleRead {
        thread: String,
        messages: Vec<ChatMessage>,
        ticket: crate::read_receipts::VisibilityTicket,
    },
    Load(Vec<Thread>),
    Persist(Vec<Thread>),
    Reconcile(Vec<Thread>),
    Page {
        thread: String,
        before: Option<HistoryCursor>,
    },
    Navigate {
        thread: String,
        reference: String,
    },
    Search {
        thread: String,
        query: String,
    },
    Delete {
        thread: String,
        message: String,
    },
    DeleteThread(String),
    Import(PathBuf),
    Clear,
    OldestCursor(String),
}

pub(super) enum Reply {
    Threads(Vec<Thread>),
    Written(usize),
    Page {
        messages: Vec<ChatMessage>,
        has_more: bool,
    },
    Navigation(Option<(String, Vec<ChatMessage>)>),
    Imported(crate::store_v2::MigrationReport, Vec<Thread>),
    Cursor(String),
    VisibleRead(bool),
    Done,
}

/// Execute one captured-account request off the async/UI threads.
/// The captured runtime epoch rejects same-account ABA generations.
///
/// # Errors
/// Returns a terminal error for a stale account/session, invalid input or failed storage task.
pub(super) async fn execute(
    root: PathBuf,
    session: Arc<tokio::sync::Mutex<Session>>,
    captured: Session,
    account: String,
    request: Request,
) -> Result<Reply, String> {
    if account.is_empty() || captured.login.as_ref().map(|l| l.uin.as_str()) != Some(&account) {
        return Err("Tài khoản lịch sử không hợp lệ".into());
    }
    let execution = crate::runtime_scope::execution_scope();
    let _lane = STORAGE_LANE.lock().await;
    if crate::runtime_scope::execution_scope() != execution {
        return Err("Phiên lịch sử đã thay đổi".into());
    }
    crate::runtime_scope::spawn_blocking(move || -> anyhow::Result<Reply> {
        #[cfg(test)]
        let request = match request {
            Request::PauseBeforeMutation {
                entered,
                release,
                request,
            } => {
                entered
                    .send(())
                    .map_err(|_| anyhow::anyhow!("Fixture receiver closed"))?;
                release
                    .recv()
                    .map_err(|_| anyhow::anyhow!("Fixture sender closed"))?;
                *request
            }
            request => request,
        };
        // Prevent account transitions through the irreversible storage operation.
        // The enclosing runtime fence rejects stale and orphaned blocking tasks.
        let current = session.blocking_lock();
        let _mutation = crate::runtime_scope::mutation()?;
        anyhow::ensure!(
            current.login.as_ref().map(|l| l.uin.as_str()) == Some(&account)
                && current.zpw_sek == captured.zpw_sek,
            "Stale history session"
        );
        let mut store = AccountStore::open(&root, &account)?;
        match request {
            #[cfg(test)]
            Request::PauseBeforeMutation { .. } => unreachable!("fixture unwrapped above"),

            Request::VisibleRead { thread, messages, ticket } => {
                if !ticket.is_current() { return Ok(Reply::VisibleRead(false)); }
                store.read_local_visible(&thread, &messages)?;
                Ok(Reply::VisibleRead(true))
            }
            Request::Load(live) => {
                let mut threads = store.all_thread_metadata()?;
                store.hydrate_recent(&mut threads)?;
                for incoming in live {
                    if let Some(existing) = threads.iter_mut().find(|t| t.id == incoming.id) {
                        crate::chat::merge_thread_into(existing, incoming, false);
                    } else if !incoming.id.is_empty() {
                        threads.push(incoming);
                    }
                }
                Ok(Reply::Threads(threads))
            }
            Request::Persist(threads) => {
                let stats = store.persist_threads(&threads)?;
                Ok(Reply::Written(stats.inserted))
            }
            Request::Reconcile(mut threads) => {
                store.persist_threads(&threads)?;
                for thread in &mut threads {
                    let mut messages = Vec::new();
                    for message in &thread.messages {
                        if let Some(message) = store.message_for(&thread.id, message)? {
                            messages.push(message);
                        }
                    }
                    messages.sort_by(|a, b| (a.ts, &a.id).cmp(&(b.ts, &b.id)));
                    if messages.len() > 200 {
                        messages.drain(..messages.len() - 200);
                    }
                    thread.messages = messages;
                }
                let mut retained = Vec::new();
                for thread in threads {
                    if store.thread_is_group(&thread.id)?.is_some() {
                        retained.push(thread);
                    }
                }
                Ok(Reply::Threads(retained))
            }
            Request::Page { thread, before } => {
                let page = store.history_page(&thread, before.as_ref(), PageDirection::Before)?;
                let messages = page
                    .messages
                    .iter()
                    .map(|m| store.chat_message(m))
                    .collect::<anyhow::Result<_>>()?;
                Ok(Reply::Page {
                    messages,
                    has_more: page.has_more,
                })
            }
            Request::Navigate { thread, reference } => Ok(Reply::Navigation(
                store.navigation_page(&thread, &reference)?,
            )),
            Request::Search { thread, query } => {
                let hits = store.search(&query, Some(&thread), 0, 200)?;
                let messages = hits
                    .iter()
                    .map(|hit| store.chat_message(&hit.message))
                    .collect::<anyhow::Result<_>>()?;
                Ok(Reply::Page {
                    messages,
                    has_more: false,
                })
            }
            Request::Delete { thread, message } => {
                store.apply_delta(StoreDelta {
                    deleted_messages: &[MessageKey {
                        thread_id: thread,
                        msg_id: message,
                    }],
                    ..Default::default()
                })?;
                Ok(Reply::Done)
            }
            Request::DeleteThread(thread) => {
                store.delete_thread_history(&thread)?;
                Ok(Reply::Done)
            }
            Request::Import(path) => {
                let mut header = [0; 16];
                let mut source = std::fs::File::open(&path)?;
                let count = std::io::Read::read(&mut source, &mut header)?;
                let sqlite = count == header.len() && &header == b"SQLite format 3\0";
                let report = store.migrate_legacy(
                    sqlite.then_some(path.as_path()),
                    (!sqlite).then_some(path.as_path()),
                )?;
                let mut threads = store.all_thread_metadata()?;
                store.hydrate_recent(&mut threads)?;
                Ok(Reply::Imported(report, threads))
            }
            Request::Clear => {
                store.clear_local_history()?;
                Ok(Reply::Done)
            }
            Request::OldestCursor(thread) => {
                let cursor = store
                    .history_page(&thread, None, PageDirection::After)?
                    .messages
                    .first()
                    .map(|m| m.msg_id.clone())
                    .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
                    .unwrap_or_default();
                Ok(Reply::Cursor(cursor))
            }
        }
    })
    .await
    .map_err(|_| "Tác vụ lịch sử đã dừng".to_owned())?
    .map_err(|_| "Phiên lịch sử đã hết hạn".to_owned())?
    .map_err(|_| "Chưa đọc hoặc lưu được lịch sử theo tài khoản".to_owned())
}

/// Capture the application data root once per operation after startup selected it.
///
/// # Errors
/// Returns an error when the application data directory cannot be resolved.
pub(super) fn root() -> Result<PathBuf, String> {
    std::env::current_dir().map_err(|_| "Chưa xác định được thư mục dữ liệu".to_owned())
}

/// A per-thread revision invalidates all archive replies captured before conversation deletion.
pub(super) fn revision_matches(
    revisions: &std::collections::HashMap<String, u64>,
    thread: &str,
    revision: u64,
) -> bool {
    revisions.get(thread).copied().unwrap_or(0) == revision
}

/// Merge a background cache page without reviving an exact message deleted while it ran.
pub(super) fn merge_cached_page(
    threads: &mut [Thread],
    deleted: &std::collections::HashSet<(String, String)>,
    thread_id: &str,
    mut messages: Vec<ChatMessage>,
) {
    messages.retain(|message| !deleted.contains(&(thread_id.to_owned(), message.id.clone())));
    if let Some(thread) = threads.iter_mut().find(|thread| thread.id == thread_id) {
        crate::chat::merge_cached_messages(thread, messages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::{GuiEvent, GuiTx};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "zl-history-adoption-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn session(account: &str) -> Session {
        let mut session = Session::new(false, false, false).unwrap();
        session.login = Some(crate::session::LoginInfo {
            uin: account.into(),
            ..Default::default()
        });
        session.zpw_sek = Some(format!("fixture-{account}"));
        session
    }
    fn thread(count: usize) -> Thread {
        Thread {
            id: "same-thread".into(),
            name: "Fixture".into(),
            messages: (0..count)
                .map(|i| ChatMessage {
                    id: format!("{i:05}"),
                    ts: i as i64,
                    text: format!("Stored body {i}"),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    async fn run(
        root: &Fixture,
        live: &Arc<tokio::sync::Mutex<Session>>,
        captured: &Session,
        account: &str,
        request: Request,
    ) -> Reply {
        execute(
            root.0.clone(),
            live.clone(),
            captured.clone(),
            account.into(),
            request,
        )
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn consumer_loads_a_b_a_without_implicit_legacy_import_and_reopens() {
        let root = Fixture::new();
        std::fs::write(
            root.0.join("threads_cache.json"),
            serde_json::to_vec(&vec![thread(1)]).unwrap(),
        )
        .unwrap();
        let legacy =
            crate::store::MessageStore::open(root.0.join("zalo_history.db").to_str().unwrap())
                .unwrap();
        drop(legacy);
        let before = std::fs::read(root.0.join("zalo_history.db")).unwrap();
        let a = session("account-a");
        let b = session("account-b");
        let live = Arc::new(tokio::sync::Mutex::new(a.clone()));
        run(
            &root,
            &live,
            &a,
            "account-a",
            Request::Persist(vec![thread(1)]),
        )
        .await;
        *live.lock().await = b.clone();
        let Reply::Threads(empty) = run(&root, &live, &b, "account-b", Request::Load(vec![])).await
        else {
            panic!("load missing")
        };
        assert!(empty.is_empty());
        assert!(execute(
            root.0.clone(),
            live.clone(),
            a.clone(),
            "account-a".into(),
            Request::Persist(vec![thread(2)])
        )
        .await
        .is_err());
        assert!(execute(
            root.0.clone(),
            live.clone(),
            b.clone(),
            String::new(),
            Request::Load(vec![])
        )
        .await
        .is_err());
        *live.lock().await = a.clone();
        let Reply::Threads(restored) =
            run(&root, &live, &a, "account-a", Request::Load(vec![])).await
        else {
            panic!("load missing")
        };
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].messages.len(), 1);
        assert_eq!(restored[0].messages[0].text, "Stored body 0");
        assert_eq!(
            std::fs::read(root.0.join("zalo_history.db")).unwrap(),
            before
        );
    }
    #[tokio::test]
    async fn consumer_pages_more_than_twenty_windows_without_an_unbounded_limit() {
        let root = Fixture::new();
        let captured = session("account-a");
        let live = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Persist(vec![thread(4_605)]),
        )
        .await;
        let Reply::Threads(reopened) =
            run(&root, &live, &captured, "account-a", Request::Load(vec![])).await
        else {
            panic!("load missing")
        };
        assert_eq!(reopened[0].messages.len(), 200);
        assert_eq!(reopened[0].messages.last().unwrap().id, "04604");
        let mut before = None;
        let mut seen = std::collections::HashSet::new();
        let mut pages = 0;
        loop {
            let Reply::Page { messages, has_more } = run(
                &root,
                &live,
                &captured,
                "account-a",
                Request::Page {
                    thread: "same-thread".into(),
                    before,
                },
            )
            .await
            else {
                panic!("page missing")
            };
            assert!(messages.len() <= 200);
            pages += 1;
            for message in &messages {
                assert!(seen.insert(message.id.clone()));
            }
            before = messages.first().map(|message| HistoryCursor {
                ts: message.ts,
                msg_id: message.id.clone(),
            });
            if !has_more {
                break;
            }
        }
        assert!(pages > 20);
        assert_eq!(seen.len(), 4_605);
    }
    #[tokio::test]
    async fn actual_network_consumer_filters_deleted_rows_and_keeps_terminal_recall() {
        let root = Fixture::new();
        let captured = session("account-a");
        let live = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut original = thread(2);
        original.messages[0].link_url = Some("https://fixture.invalid/photo".into());
        run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Persist(vec![original.clone()]),
        )
        .await;
        run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Delete {
                thread: "same-thread".into(),
                message: "00001".into(),
            },
        )
        .await;
        let mut recall = original.clone();
        recall.messages.truncate(1);
        recall.messages[0].text = "[Tin nhắn đã thu hồi]".into();
        recall.messages[0].kind = crate::message_model::MessageKind::Recall;
        recall.messages[0].link_url = None;
        run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Reconcile(vec![recall]),
        )
        .await;
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::worker::publish_account_threads_at(
            root.0.clone(),
            live.clone(),
            captured.clone(),
            GuiTx {
                tx,
                scope: None,
                ctx: eframe::egui::Context::default(),
            },
            vec![original],
            true,
        )
        .await;
        let GuiEvent::AccountThreadsUpdated {
            account,
            threads,
            live: arrival,
        } = rx.try_recv().unwrap()
        else {
            panic!("consumer projection missing")
        };
        assert_eq!(account, "account-a");
        assert!(arrival);
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].text, "[Tin nhắn đã thu hồi]");
        assert!(threads[0].messages[0].link_url.is_none());
        let Reply::Threads(reopened) =
            run(&root, &live, &captured, "account-a", Request::Load(vec![])).await
        else {
            panic!("load missing")
        };
        assert_eq!(reopened[0].messages.len(), 1);
        assert_eq!(reopened[0].messages[0].text, "[Tin nhắn đã thu hồi]");
    }
    #[test]
    fn late_page_cannot_resurrect_a_message_deleted_after_the_request_started() {
        let mut threads = vec![thread(1)];
        let old = threads[0].messages.clone();
        let deleted = std::collections::HashSet::from([("same-thread".into(), "00000".into())]);
        threads[0].messages.clear();
        merge_cached_page(&mut threads, &deleted, "same-thread", old);
        assert!(threads[0].messages.is_empty());
    }
    #[tokio::test]
    async fn explicit_import_is_account_bound_and_search_quote_work_offscreen() {
        let root = Fixture::new();
        let captured = session("account-a");
        let live = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut source = thread(605);
        source.messages[2].text = "Đặng đã đến thành phố".into();
        source.messages[2].client_message_id = "exact-client".into();
        let path = root.0.join("source.json");
        let bytes = serde_json::to_vec(&vec![source]).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let Reply::Imported(report, threads) = run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Import(path.clone()),
        )
        .await
        else {
            panic!("import missing")
        };
        assert_eq!(report.messages, 605);
        assert_eq!(threads[0].messages.len(), 200);
        assert!(!threads[0].messages.iter().any(|m| m.id == "00002"));
        let Reply::Navigation(Some((id, page))) = run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Navigate {
                thread: "same-thread".into(),
                reference: "exact-client".into(),
            },
        )
        .await
        else {
            panic!("quote missing")
        };
        assert_eq!(id, "00002");
        assert!(page.len() <= 200);
        assert!(page.iter().any(|m| m.id == id));
        let Reply::Page { messages, .. } = run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Search {
                thread: "same-thread".into(),
                query: "dang da den".into(),
            },
        )
        .await
        else {
            panic!("search missing")
        };
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, "00002");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let b = session("account-b");
        *live.lock().await = b.clone();
        let Reply::Threads(empty) = run(&root, &live, &b, "account-b", Request::Load(vec![])).await
        else {
            panic!("load missing")
        };
        assert!(empty.is_empty());
    }
    #[tokio::test]
    async fn deleted_conversation_stays_absent_after_reopen_and_stale_replay_but_allows_new_ids() {
        let root = Fixture::new();
        let captured = session("account-a");
        let live = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let original = thread(2);
        run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Persist(vec![original.clone()]),
        )
        .await;
        run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::DeleteThread("same-thread".into()),
        )
        .await;
        let Reply::Threads(stale) = run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Reconcile(vec![original]),
        )
        .await
        else {
            panic!("projection missing")
        };
        assert!(stale.is_empty());
        let Reply::Threads(reopened) =
            run(&root, &live, &captured, "account-a", Request::Load(vec![])).await
        else {
            panic!("load missing")
        };
        assert!(reopened.is_empty());
        let mut fresh = thread(1);
        fresh.messages[0].id = "new-server-id".into();
        let Reply::Threads(visible) = run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Reconcile(vec![fresh]),
        )
        .await
        else {
            panic!("new projection missing")
        };
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].messages.len(), 1);
        assert_eq!(visible[0].messages[0].id, "new-server-id");
    }
    #[tokio::test]
    async fn actual_network_consumer_publishes_only_newest_two_hundred_bodies() {
        let root = Fixture::new();
        let captured = session("account-a");
        let live = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let (tx, rx) = std::sync::mpsc::channel();
        super::super::worker::publish_account_threads_at(
            root.0.clone(),
            live.clone(),
            captured.clone(),
            GuiTx {
                tx,
                scope: None,
                ctx: eframe::egui::Context::default(),
            },
            vec![thread(605)],
            false,
        )
        .await;
        let GuiEvent::AccountThreadsUpdated { threads, .. } = rx.try_recv().unwrap() else {
            panic!("projection missing")
        };
        assert_eq!(threads[0].messages.len(), 200);
        assert_eq!(threads[0].messages.first().unwrap().id, "00405");
        assert_eq!(threads[0].messages.last().unwrap().id, "00604");
        let Reply::Page { messages, has_more } = run(
            &root,
            &live,
            &captured,
            "account-a",
            Request::Page {
                thread: "same-thread".into(),
                before: Some(HistoryCursor {
                    ts: 405,
                    msg_id: "00405".into(),
                }),
            },
        )
        .await
        else {
            panic!("page missing")
        };
        assert_eq!(messages.len(), 200);
        assert!(has_more);
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_consumer_blocks_same_key_a_b_a_and_orphaned_blocking_writes() {
        let _lock = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        for abort in [false, true] {
            let root = Fixture::new();
            let captured = session("account-a");
            let live = Arc::new(tokio::sync::Mutex::new(captured.clone()));
            let old = crate::runtime_scope::transition("account-a").unwrap();
            let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
            let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
            let path = root.0.clone();
            let task = tokio::spawn(async move {
                crate::runtime_scope::run_stale_for_test(old, async move {
                    execute(
                        path,
                        live,
                        captured,
                        "account-a".into(),
                        Request::PauseBeforeMutation {
                            entered: entered_tx,
                            release: release_rx,
                            request: Box::new(Request::Persist(vec![thread(1)])),
                        },
                    )
                    .await
                })
                .await
            });
            tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
                .await
                .unwrap();
            crate::runtime_scope::transition("account-b").unwrap();
            crate::runtime_scope::transition("account-a").unwrap();
            if abort {
                task.abort();
            }
            release_tx.send(()).unwrap();
            if abort {
                assert!(matches!(task.await, Err(error) if error.is_cancelled()));
            } else {
                assert!(task.await.unwrap().is_err());
            }
            crate::runtime_scope::drain().await;
            assert!(!root.0.join("accounts").exists());
        }
        crate::runtime_scope::transition("").unwrap();
    }
    #[test]
    fn deleted_thread_revision_rejects_offscreen_pages_after_a_new_message_recreates_it() {
        let captured = thread(3).messages;
        let mut revisions = std::collections::HashMap::new();
        let old_revision = 0;
        assert!(revision_matches(&revisions, "same-thread", old_revision));
        revisions.insert("same-thread".into(), 1);
        let mut fresh = thread(1);
        fresh.messages[0].id = "new-server-id".into();
        let mut threads = vec![fresh];
        if revision_matches(&revisions, "same-thread", old_revision) {
            merge_cached_page(&mut threads, &Default::default(), "same-thread", captured);
        }
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].id, "new-server-id");
        assert!(revision_matches(&revisions, "same-thread", 1));
    }
}

#[cfg(test)]
mod visible_read_queue_tests {
    use super::*;
    #[tokio::test]
    async fn visible_read_rechecks_ticket_after_blocking_queue_before_durable_mutation() {
        let root = std::env::temp_dir().join(format!("zl-visible-lane-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut captured = Session::new(false, false, false).unwrap();
        captured.login = Some(crate::session::LoginInfo { uin: "fixture-account".into(), ..Default::default() });
        let session = Arc::new(tokio::sync::Mutex::new(captured.clone()));
        let mut store = AccountStore::open(&root, "fixture-account").unwrap();
        store.persist_threads(&[Thread { id: "fixture-thread".into(), unread: crate::unread::UnreadState { marked_unread: true, ..Default::default() }, ..Default::default() }]).unwrap();
        drop(store);
        let mut gate = crate::read_receipts::ReadVisibilityGate::default();
        for now in [0,100,200,300,400,500] { gate.observe("fixture", true, false, true, now); }
        let (entered, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release, release_rx) = std::sync::mpsc::sync_channel(1);
        let request = Request::PauseBeforeMutation { entered, release: release_rx, request: Box::new(Request::VisibleRead { thread: "fixture-thread".into(), messages: vec![ChatMessage { id: "10".into(), ..Default::default() }], ticket: gate.ticket().unwrap() }) };
        let future = tokio::spawn(execute(root.clone(), session, captured, "fixture-account".into(), request));
        tokio::task::spawn_blocking(move || entered_rx.recv().unwrap()).await.unwrap();
        gate.reset();
        release.send(()).unwrap();
        assert!(matches!(future.await.unwrap().unwrap(), Reply::VisibleRead(false)));
        let reopened = AccountStore::open(&root, "fixture-account").unwrap();
        let state = reopened.unread("fixture-thread").unwrap();
        assert!(state.last_seen_id.is_none() && state.marked_unread);
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
