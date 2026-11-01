//! Session epochs, cancellation, queued-event rejection and serialized disk mutation barriers.
//!
//! GUI tasks opt in through `spawn`; independent CLI/TUI callers remain unscoped.
use std::{
    cell::Cell,
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, OnceLock,
    },
};

/// An authentication incarnation; generation changes even for the same account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionScope {
    /// Account identity, empty before authentication or after logout.
    pub account: String,
    /// Monotonic process-local incarnation, never reused across A/B/A transitions.
    pub generation: u64,
}
/// A rejected operation belongs to an obsolete authentication incarnation.
#[derive(Clone, Copy, Debug)]
pub struct StaleScope;
impl std::fmt::Display for StaleScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session scope expired")
    }
}
impl std::error::Error for StaleScope {}

/// Registry shared by a GUI worker and its descendants.
#[derive(Default)]
pub struct ScopeRegistry {
    state: Mutex<SessionScope>,
    generation: AtomicU64,
    mutation: Mutex<()>,
    changed: tokio::sync::Notify,
    cleanups: Mutex<std::collections::BTreeMap<u64, Box<dyn FnOnce() + Send>>>,
    next_cleanup: AtomicU64,
}
impl ScopeRegistry {
    /// Snapshot the current incarnation outside the UI render path.
    pub fn current(&self) -> SessionScope {
        let mut scope = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let generation = self.generation.load(Ordering::SeqCst);
        if scope.generation != generation {
            scope.account.clear();
        }
        scope.generation = generation;
        scope
    }
    /// Read only an atomic counter; safe for UI queue filtering.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
    /// Reject stale results without taking a disk mutation lock.
    pub fn accepts(&self, scope: &SessionScope) -> bool {
        self.generation() == scope.generation
    }
    /// Immediately reject queued UI results. Worker cleanup subsequently drains the disk barrier.
    pub fn invalidate_now(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.changed.notify_waiters();
    }
    /// Atomically invalidate only the caller's still-current incarnation.
    ///
    /// # Errors
    /// A stale caller cannot invalidate or replace a newer authentication incarnation.
    pub fn invalidate_if_current(&self, generation: u64) -> Result<u64, StaleScope> {
        let next = generation.checked_add(1).ok_or(StaleScope)?;
        self.generation
            .compare_exchange(generation, next, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| StaleScope)?;
        self.changed.notify_waiters();
        Ok(next)
    }

    /// Wait for cancellation without a Notify registration/check race.
    pub async fn cancelled(&self, scope: &SessionScope) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.accepts(scope) {
                return;
            }
            notified.await;
        }
    }
}
/// Global GUI registry. Unscoped command-line runtimes do not implicitly adopt it.
pub fn registry() -> &'static ScopeRegistry {
    static REGISTRY: OnceLock<ScopeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

tokio::task_local! { static EXECUTION: Arc<Mutex<SessionScope>>; }
thread_local! {
    static BLOCKING_SCOPE: std::cell::RefCell<Option<SessionScope>> = const { std::cell::RefCell::new(None) };
    static TRANSITION_TICKET: std::cell::RefCell<Option<Arc<Mutex<SessionScope>>>> = const { std::cell::RefCell::new(None) };
    static LIFECYCLE_CLEANUP: Cell<bool> = const { Cell::new(false) };
    static MUTATION_DEPTH: Cell<usize> = const { Cell::new(0) };
}
/// Scope attached explicitly to the current async task or blocking closure.
pub fn execution_scope() -> Option<SessionScope> {
    EXECUTION
        .try_with(|scope| scope.lock().unwrap_or_else(|e| e.into_inner()).clone())
        .ok()
        .or_else(|| BLOCKING_SCOPE.with(|scope| scope.borrow().clone()))
}
/// A reentrant, synchronous-only disk mutation guard. Never hold it across an await.
///
/// Lock order: mutation barrier, then registry state. Session locks may precede this guard;
/// transition cleanup releases the barrier before awaiting a session lock.
pub struct MutationGuard {
    _guard: Option<MutexGuard<'static, ()>>,
}
impl Drop for MutationGuard {
    fn drop(&mut self) {
        MUTATION_DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}
/// Serialize scoped writes with auth transitions and reject stale/unauthenticated work.
///
/// # Errors
/// Returns `StaleScope` when the execution incarnation is obsolete or unauthenticated.
/// Unscoped CLI, test and startup persistence retains its existing behavior.
pub fn mutation() -> Result<MutationGuard, StaleScope> {
    let nested = MUTATION_DEPTH.with(|depth| depth.get() > 0);
    let guard = if nested {
        None
    } else {
        Some(
            registry()
                .mutation
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    };
    if !LIFECYCLE_CLEANUP.with(Cell::get) {
        if let Some(scope) = execution_scope() {
            if !registry().accepts(&scope) || scope.account.is_empty() {
                return Err(StaleScope);
            }
        }
    }
    MUTATION_DEPTH.with(|depth| depth.set(depth.get() + 1));
    Ok(MutationGuard { _guard: guard })
}
/// Publish a new auth incarnation under the disk barrier, updating the caller's ticket.
///
/// # Errors
/// Rejects a stale authentication attempt before it can replace a newer session.
pub fn transition(account: &str) -> Result<SessionScope, StaleScope> {
    transition_after_cleanup(account, || {})
}
/// Drain started writes, perform synchronous cleanup, then publish the next incarnation.
/// Cleanup stays synchronous; nested store guards reuse this authorized barrier. Never await.
///
/// # Errors
/// Rejects an obsolete caller before cleanup or publication.
pub fn transition_after_cleanup(
    account: &str,
    cleanup: impl FnOnce(),
) -> Result<SessionScope, StaleScope> {
    transition_checked(
        execution_scope().map(|scope| scope.generation),
        account,
        cleanup,
    )
}
/// Revalidate a queued lifecycle intent under the mutation barrier, after any wait for writers.
///
/// # Errors
/// A newer enqueue invalidation makes the older intent inert, including its cleanup callback.
pub(crate) fn transition_if_current(
    generation: u64,
    account: &str,
    cleanup: impl FnOnce(),
) -> Result<SessionScope, StaleScope> {
    transition_checked(Some(generation), account, cleanup)
}
fn transition_checked(
    expected: Option<u64>,
    account: &str,
    cleanup: impl FnOnce(),
) -> Result<SessionScope, StaleScope> {
    transition_checked_fallible(expected, account, || {
        cleanup();
        Ok(())
    })
    .map_err(|_| StaleScope)
}
/// Flush intentional lifecycle-owned state before publishing an auth transition.
/// Failed persistence leaves the incarnation unpublished so shutdown can report failure and retry.
///
/// # Errors
/// Returns a stale-intent or persistence error; never publishes a transition after cleanup failure.
pub(crate) fn transition_if_current_fallible(
    generation: u64,
    account: &str,
    cleanup: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<SessionScope> {
    transition_checked_fallible(Some(generation), account, cleanup)
}
fn transition_checked_fallible(
    expected: Option<u64>,
    account: &str,
    cleanup: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<SessionScope> {
    let _barrier = registry()
        .mutation
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Mark this authorized synchronous barrier as reentrant for nested store saves.
    MUTATION_DEPTH.with(|depth| depth.set(depth.get() + 1));
    let _barrier = MutationGuard {
        _guard: Some(_barrier),
    };
    let observed = registry().generation();
    if expected.is_some_and(|generation| generation != observed) {
        return Err(StaleScope.into());
    }
    let pending = std::mem::take(
        &mut *registry()
            .cleanups
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    for (_, settle) in pending {
        settle();
    }
    // Only this synchronous, intent-checked callback may flush immutable owner-bound data
    // while the initiating auth attempt has an empty/invalidated account ticket.
    struct CleanupPermit(bool);
    impl Drop for CleanupPermit {
        fn drop(&mut self) {
            LIFECYCLE_CLEANUP.with(|flag| flag.set(self.0));
        }
    }
    {
        let _permit = CleanupPermit(LIFECYCLE_CLEANUP.with(|flag| flag.replace(true)));
        cleanup()?;
    }
    let generation = observed.checked_add(1).ok_or(StaleScope)?;
    // UI invalidation is lock-free and can occur during cleanup. Publication must not
    // adopt that newer intent merely because the old worker already owned the disk barrier.
    registry()
        .generation
        .compare_exchange(observed, generation, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| StaleScope)?;
    let scope = SessionScope {
        account: account.to_owned(),
        generation,
    };
    *registry().state.lock().unwrap_or_else(|e| e.into_inner()) = scope.clone();
    let _ = EXECUTION
        .try_with(|ticket| *ticket.lock().unwrap_or_else(|e| e.into_inner()) = scope.clone());
    BLOCKING_SCOPE.with(|current| {
        if current.borrow().is_some() {
            *current.borrow_mut() = Some(scope.clone());
        }
    });
    TRANSITION_TICKET.with(|current| {
        if let Some(ticket) = current.borrow().as_ref() {
            *ticket.lock().unwrap_or_else(|e| e.into_inner()) = scope.clone();
        }
    });
    registry().changed.notify_waiters();
    Ok(scope)
}
/// Register a terminal cleanup while the caller holds a mutation guard.
/// Callbacks use already-open resources and must neither open paths nor acquire the mutation lock.
pub(crate) fn register_cleanup(cleanup: impl FnOnce() + Send + 'static) -> u64 {
    let id = registry().next_cleanup.fetch_add(1, Ordering::SeqCst);
    registry()
        .cleanups
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, Box::new(cleanup));
    id
}
fn settle_cleanup(id: u64) {
    let _barrier = registry()
        .mutation
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let cleanup = registry()
        .cleanups
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    if let Some(cleanup) = cleanup {
        cleanup();
    }
}
/// Settle a dropped scheduler claim under the same barrier used by wipe and authentication.
/// Transition drains registered callbacks before destructive cleanup, so late drops become no-ops.
pub(crate) fn schedule_cleanup(id: u64) {
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let active = ActiveTask::enter();
        runtime.spawn_blocking(move || {
            let _active = active;
            settle_cleanup(id);
        });
    } else {
        settle_cleanup(id);
    }
}

/// Run work with an explicit snapshot, cancelling obsolete work before its next poll.
pub async fn run<F: Future>(scope: SessionScope, future: F) -> Option<F::Output> {
    let _active = ActiveTask::enter();
    let ticket = Arc::new(Mutex::new(scope));
    EXECUTION
        .scope(ticket.clone(), async move {
            tokio::pin!(future);
            loop {
                let observed = ticket.lock().unwrap_or_else(|e| e.into_inner()).clone();
                tokio::select! {
                    biased;
                    _ = registry().cancelled(&observed) => {
                        let latest = ticket.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        if !registry().accepts(&latest) { return None; }
                    }
                    output = &mut future => return Some(output),
                }
            }
        })
        .await
}
struct ActiveTask;
static ACTIVE_TASKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static TASKS_CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();
impl ActiveTask {
    fn enter() -> Self {
        ACTIVE_TASKS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}
impl Drop for ActiveTask {
    fn drop(&mut self) {
        ACTIVE_TASKS.fetch_sub(1, Ordering::SeqCst);
        TASKS_CHANGED.notify_waiters();
    }
}
/// Wait for captured async and blocking descendants to finish dropping their owned resources.
/// Call only from an unscoped shutdown supervisor, after invalidation stops new account admission.
pub async fn drain() {
    loop {
        let notified = TASKS_CHANGED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ACTIVE_TASKS.load(Ordering::SeqCst) == 0 {
            return;
        }
        notified.await;
    }
}
/// Spawn account work, inheriting a snapshot rather than reading a future account on completion.
pub fn spawn(future: impl Future<Output = ()> + Send + 'static) -> tokio::task::JoinHandle<()> {
    let scope = execution_scope().unwrap_or_else(|| registry().current());
    spawn_scoped(scope, future)
}
/// Spawn with the already-validated command ticket, never recapturing a later login.
pub(crate) fn spawn_scoped(
    scope: SessionScope,
    future: impl Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    let active = ActiveTask::enter();
    tokio::spawn(async move {
        let _active = active;
        run(scope, future).await;
    })
}
/// Propagate the ticket into a blocking closure; guards remain effective after async cancellation.
///
/// # Errors
/// The inner result rejects closures whose ticket became stale before they started.
pub fn spawn_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<Result<T, StaleScope>> {
    spawn_blocking_mode(operation, false)
}
/// Run an authorized authentication commit off-thread while updating only its initiating task ticket.
/// Ordinary blocking descendants still capture immutable snapshots and never adopt future accounts.
pub(crate) fn spawn_transition_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<Result<T, StaleScope>> {
    spawn_blocking_mode(operation, true)
}
fn spawn_blocking_mode<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
    transition: bool,
) -> tokio::task::JoinHandle<Result<T, StaleScope>> {
    let parent = if transition {
        EXECUTION.try_with(Clone::clone).ok()
    } else {
        None
    };
    let scope = execution_scope();
    let active = scope.as_ref().map(|_| ActiveTask::enter());
    tokio::task::spawn_blocking(move || {
        let _active = active;
        BLOCKING_SCOPE.with(|slot| *slot.borrow_mut() = scope);
        TRANSITION_TICKET.with(|slot| *slot.borrow_mut() = parent);
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                BLOCKING_SCOPE.with(|slot| *slot.borrow_mut() = None);
                TRANSITION_TICKET.with(|slot| *slot.borrow_mut() = None);
            }
        }
        let _reset = Reset;
        if let Some(scope) = execution_scope() {
            if !registry().accepts(&scope) {
                return Err(StaleScope);
            }
        }
        Ok(operation())
    })
}
/// A drop reset for worker single-flight flags, including cancellation paths.
pub(crate) struct ResetFlag(pub(crate) &'static std::sync::atomic::AtomicBool);
impl Drop for ResetFlag {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) async fn run_stale_for_test<F: Future>(scope: SessionScope, future: F) -> F::Output {
    EXECUTION.scope(Arc::new(Mutex::new(scope)), future).await
}

/// Check a captured task ticket at an in-memory or outbound side-effect boundary.
///
/// # Errors
/// Returns `StaleScope` for obsolete GUI work; standalone callers remain independent.
pub fn check() -> Result<(), StaleScope> {
    if let Some(scope) = execution_scope() {
        if !registry().accepts(&scope) {
            return Err(StaleScope);
        }
    }
    Ok(())
}
/// Propagate an existing GUI ticket while preserving independent CLI/TUI task lifetimes.
pub fn spawn_inherited(
    future: impl Future<Output = ()> + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    if execution_scope().is_some() {
        spawn(future)
    } else {
        tokio::spawn(future)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::time::Duration;
    pub(crate) static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn cancellation_registers_before_check_and_observes_early_invalidation() {
        for _ in 0..200 {
            let registry = Arc::new(ScopeRegistry::default());
            let ticket = registry.current();
            let observer = registry.clone();
            let task = tokio::spawn(async move { observer.cancelled(&ticket).await });
            registry.invalidate_now();
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn generations_reject_a_b_a_and_same_account_reauthentication() {
        let _serial = TEST_LOCK.lock().await;
        let first = transition("account-a").unwrap();
        let second = transition("account-b").unwrap();
        let third = transition("account-a").unwrap();
        assert!(!registry().accepts(&first));
        assert!(!registry().accepts(&second));
        assert!(registry().accepts(&third));
        let refreshed = transition("account-a").unwrap();
        assert!(!registry().accepts(&third));
        assert!(registry().accepts(&refreshed));
        let mut polled = false;
        assert!(run(first, async { polled = true }).await.is_none());
        assert!(!polled);
    }

    #[tokio::test]
    async fn queued_gui_result_is_rejected_after_logout_even_before_worker_cleanup() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let sender = crate::gui::GuiTx::for_test(tx);
        run(scope.clone(), async {
            sender
                .send(crate::gui::GuiEvent::StatusUpdate("old".into()))
                .unwrap();
        })
        .await
        .unwrap();
        registry().invalidate_now();
        let crate::gui::GuiEvent::Scoped { scope: queued, .. } = rx.recv().unwrap() else {
            panic!("missing scope envelope")
        };
        assert!(!registry().accepts(&queued));
        assert!(registry().current().account.is_empty());
    }

    #[tokio::test]
    async fn stale_authentication_cannot_commit_after_a_new_login() {
        let _serial = TEST_LOCK.lock().await;
        let old = transition("account-a").unwrap();
        transition("account-b").unwrap();
        let result = EXECUTION
            .scope(Arc::new(Mutex::new(old)), async { transition("account-a") })
            .await;
        assert!(result.is_err());
        assert_eq!(registry().current().account, "account-b");
    }

    #[tokio::test]
    async fn delayed_blocking_io_keeps_its_original_ticket_and_cannot_recreate_wiped_store() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let path =
            std::env::temp_dir().join(format!("ctdozlite-stale-scope-{}.db", std::process::id()));
        assert!(!path.exists());
        let stale_path = path.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = run(scope, async {
            (spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                crate::store::MessageStore::open(stale_path.to_str().unwrap()).is_err()
            }),)
        })
        .await
        .unwrap().0;
        started_rx.await.unwrap();
        registry().invalidate_now();
        transition("account-b").unwrap();
        release_tx.send(()).unwrap();
        assert!(task.await.unwrap().unwrap());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn transition_barrier_waits_for_started_writes_and_prevents_late_recreation() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let effects = Arc::new(Mutex::new(Vec::new()));
        let writes = effects.clone();
        let task = run(scope, async {
            (spawn_blocking(move || {
                let _guard = mutation().unwrap();
                // Reentrant store calls cannot deadlock behind a transition waiting on the barrier.
                let mut store = crate::store::MessageStore::open(":memory:").unwrap();
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                crate::store::persist_threads(&mut store, &[]).unwrap();
                writes.lock().unwrap().push("write");
            }),)
        })
        .await
        .unwrap().0;
        started_rx.await.unwrap();
        let cleanup = effects.clone();
        let transition_task = tokio::task::spawn_blocking(move || {
            transition_after_cleanup("", || cleanup.lock().unwrap().push("cleanup")).unwrap();
        });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!transition_task.is_finished());
        release_tx.send(()).unwrap();
        task.await.unwrap().unwrap();
        transition_task.await.unwrap();
        assert_eq!(*effects.lock().unwrap(), ["write", "cleanup"]);
    }

    #[tokio::test]
    async fn newer_lifecycle_intent_wins_after_old_transition_waits_on_disk_barrier() {
        let _serial = TEST_LOCK.lock().await;
        let initial = transition("account-a").unwrap();
        let first_intent = registry()
            .invalidate_if_current(initial.generation)
            .unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _guard = mutation().unwrap();
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.await.unwrap();
        let old_effect = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let attempted = old_effect.clone();
        let old = tokio::task::spawn_blocking(move || {
            transition_if_current(first_intent, "obsolete-login", || {
                attempted.store(true, Ordering::SeqCst)
            })
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        let latest = registry().invalidate_if_current(first_intent).unwrap();
        release_tx.send(()).unwrap();
        blocker.await.unwrap();
        assert!(old.await.unwrap().is_err());
        assert!(!old_effect.load(Ordering::SeqCst));
        let accepted = transition_if_current(latest, "latest-login", || {}).unwrap();
        assert_eq!(registry().current(), accepted);
        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = polled.clone();
        spawn_scoped(initial, async move {
            result.store(true, Ordering::SeqCst);
        })
        .await
        .unwrap();
        assert!(!polled.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn lifecycle_does_not_publish_over_new_intent_arriving_during_cleanup() {
        let _serial = TEST_LOCK.lock().await;
        let initial = transition("account-a").unwrap();
        let old_intent = registry()
            .invalidate_if_current(initial.generation)
            .unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let old = tokio::task::spawn_blocking(move || {
            transition_if_current(old_intent, "obsolete", || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
        });
        started_rx.await.unwrap();
        let latest = registry().invalidate_if_current(old_intent).unwrap();
        release_tx.send(()).unwrap();
        assert!(old.await.unwrap().is_err());
        assert_eq!(registry().generation(), latest);
        assert!(registry().current().account.is_empty());
        let accepted = transition_if_current(latest, "latest-login", || {}).unwrap();
        assert_eq!(registry().current(), accepted);
    }

    #[tokio::test]
    async fn scheduler_transition_settles_unknown_first_and_unsent_second_before_wipe() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let root =
            std::env::temp_dir().join(format!("ctdozlite-schedule-cancel-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("queue.db");
        let db = path.to_str().unwrap();
        for text in ["first", "second"] {
            crate::scheduler::schedule_message(db, "thread-1", "Thread", false, text, 10, 0)
                .unwrap();
        }
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let owned_db = db.to_owned();
        let task = tokio::spawn(async move {
            run(scope, async {
                let batch = crate::scheduler::claim_due_scheduled_batch(&owned_db, 10).unwrap();
                assert_eq!(batch.messages.len(), 2);
                batch.begin_send(batch.messages[0].id);
                started_tx.send(()).unwrap();
                std::future::pending::<()>().await;
            })
            .await
        });
        started_rx.await.unwrap();
        transition("account-a").unwrap();
        assert!(task.await.unwrap().is_none());
        let rows = crate::scheduler::list_pending_scheduled(db, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.status == "failed"));
        assert!(rows[0].error_msg.contains("chưa rõ kết quả"));
        assert!(rows[1].error_msg.contains("trước khi gửi"));
        assert!(crate::scheduler::claim_due_scheduled_messages(db, 20)
            .unwrap()
            .is_empty());
        // A second claim is cancelled by wipe. Its late drop cannot recreate the database.
        crate::scheduler::update_scheduled_message(db, rows[1].id, "retry", 10).unwrap();
        let batch = run(registry().current(), async {
            crate::scheduler::claim_due_scheduled_batch(db, 20).unwrap()
        })
        .await
        .unwrap();
        transition_after_cleanup("account-a", || {
            std::fs::remove_file(&path).unwrap();
        })
        .unwrap();
        drop(batch);
        drain().await;
        assert!(!path.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn shutdown_drain_waits_for_non_abortable_blocking_descendants() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = run(scope, async {
            (spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }),)
        })
        .await
        .unwrap().0;
        started_rx.await.unwrap();
        registry().invalidate_now();
        transition("").unwrap();
        let waiting = tokio::spawn(drain());
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!waiting.is_finished());
        release_tx.send(()).unwrap();
        task.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn native_thread_sender_retains_creation_epoch() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let sender = crate::gui::GuiTx::for_test(tx);
        let pinned = run(scope, async { sender.clone() }).await.unwrap();
        transition("account-b").unwrap();
        std::thread::spawn(move || {
            pinned
                .send(crate::gui::GuiEvent::StatusUpdate(
                    "old native thread".into(),
                ))
                .unwrap()
        })
        .join()
        .unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn call_manager_shutdown_acknowledges_without_network_or_devices() {
        let _serial = TEST_LOCK.lock().await;
        transition("account-a").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let session = crate::session::Session::new(false, false, false).unwrap();
        let bridge = crate::call::CallBridge::spawn(
            Arc::new(tokio::sync::Mutex::new(session)),
            crate::gui::GuiTx::for_test(tx),
            eframe::egui::Context::default(),
        );
        // Give the supervisor its initial epoch, then verify real call-state destruction on a switch.
        tokio::task::yield_now().await;
        let mut cancelled = bridge.install_test_call().await;
        transition("account-b").unwrap();
        tokio::time::timeout(Duration::from_secs(2), cancelled.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(*cancelled.borrow());
        registry().invalidate_now();
        tokio::time::timeout(Duration::from_secs(2), bridge.shutdown())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn stale_session_save_and_outbound_request_are_rejected_at_boundary() {
        let _serial = TEST_LOCK.lock().await;
        let old = transition("account-a").unwrap();
        transition("account-a").unwrap();
        let session = crate::session::Session::new(false, false, false).unwrap();
        EXECUTION
            .scope(Arc::new(Mutex::new(old)), async {
                let error = session
                    .save_to_file("/this-path-must-not-be-accessed/stale-session.json")
                    .unwrap_err();
                assert!(error.downcast_ref::<StaleScope>().is_some());
                let error = session.get("invalid URL").await.unwrap_err();
                assert!(error.downcast_ref::<StaleScope>().is_some());
            })
            .await;
    }

    #[tokio::test]
    async fn self_reauthentication_updates_only_the_auth_task_ticket() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let child = run(scope.clone(), async {
            let (child_done_tx, child_done_rx) = tokio::sync::oneshot::channel();
            spawn(async move {
                ready_tx.send(()).unwrap();
                release_rx.await.unwrap();
                let _ = child_done_tx.send(());
            });
            (child_done_rx,)
        })
        .await
        .unwrap().0;
        ready_rx.await.unwrap();
        let result = run(scope, async {
            let refreshed = transition("account-a").unwrap();
            tokio::task::yield_now().await;
            assert_eq!(execution_scope(), Some(refreshed));
            let _guard = mutation().unwrap();
        })
        .await;
        assert!(result.is_some());
        let _ = release_tx.send(());
        assert!(child.await.is_err());
    }
    #[tokio::test]
    async fn fallible_lifecycle_flush_does_not_publish_or_override_newer_intent() {
        let _serial = TEST_LOCK.lock().await;
        let initial = transition("account-a").unwrap();
        let intent = registry()
            .invalidate_if_current(initial.generation)
            .unwrap();
        let result =
            transition_if_current_fallible(intent, "", || anyhow::bail!("Synthetic disk failure"));
        assert!(result.is_err());
        assert_eq!(
            registry().generation(),
            intent,
            "Failed flush must not publish logout/close"
        );
        let resumed = transition_if_current(intent, "account-a", || {}).unwrap();
        assert_eq!(resumed.account, "account-a");
        let newer = registry()
            .invalidate_if_current(resumed.generation)
            .unwrap();
        let mut wrote = false;
        assert!(transition_if_current_fallible(resumed.generation, "", || {
            wrote = true;
            Ok(())
        })
        .is_err());
        assert!(!wrote);
        assert_eq!(registry().generation(), newer);
        let during = registry().generation();
        assert!(transition_if_current_fallible(during, "", || {
            registry().invalidate_now();
            Ok(())
        })
        .is_err());
        assert_eq!(
            registry().generation(),
            during + 1,
            "Cleanup cannot adopt a later intent"
        );
    }
    #[tokio::test]
    async fn authorized_blocking_auth_transition_updates_only_parent_not_siblings() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("account-a").unwrap();
        let old = scope.clone();
        let continued = run(scope, async move {
            let (started_tx, started_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocking = spawn_blocking(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                assert_eq!(execution_scope().unwrap(), old);
                mutation().is_err()
            });
            started_rx.await.unwrap();
            let sibling_effect = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let effect = sibling_effect.clone();
            let sibling = spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                effect.store(true, Ordering::SeqCst);
            });
            let commit = spawn_transition_blocking(|| transition("account-a"))
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(execution_scope().unwrap(), commit);
            assert!(
                check().is_ok(),
                "Authorized parent must continue after its auth commit"
            );
            release_tx.send(()).unwrap();
            assert!(
                blocking.await.unwrap().unwrap(),
                "Unrelated blocking descendant must retain its old ticket"
            );
            sibling.await.unwrap();
            assert!(!sibling_effect.load(Ordering::SeqCst));
            commit
        })
        .await;
        assert!(continued.is_some());
    }
    #[tokio::test]
    async fn intentional_flush_permit_is_limited_to_checked_sync_cleanup_even_on_error() {
        let _serial = TEST_LOCK.lock().await;
        let scope = transition("").unwrap();
        run(scope.clone(), async move {
            let generation = scope.generation;
            spawn_transition_blocking(move || {
                assert!(
                    mutation().is_err(),
                    "An empty auth attempt cannot perform ordinary writes"
                );
                let failed = transition_if_current_fallible(generation, "account-a", || {
                    let _authorized = mutation()?;
                    anyhow::bail!("Synthetic flush failure")
                });
                assert!(failed.is_err());
                assert!(
                    mutation().is_err(),
                    "Cleanup permission must not escape the failed callback"
                );
            })
            .await
            .unwrap()
            .unwrap();
        })
        .await
        .unwrap();
    }
}
