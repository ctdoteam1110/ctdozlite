//! Visible-read dwell, exact seen-v2 mappings, and source-derived Session receipt transport.

/// Advance local visibility state independently of wire-receipt eligibility.
/// The caller supplies viewport candidates after its own foreground and dwell gating.
pub fn read_local_visible(
    state: &mut crate::unread::UnreadState,
    messages: &[crate::chat::ChatMessage],
) -> bool {
    if messages.is_empty() {
        return false;
    }
    let before = state.clone();
    state.read_visible(messages);
    // A manual dot has no numeric cursor and can apply to opaque archive identities.
    state.marked_unread = false;
    *state != before
}

use crate::{message_model::WireMessageRef, session::Session};
use anyhow::{bail, Result};
use serde_json::{json, Value};
/// Exact source-verified, privacy-preserving seen-v2 request body.
#[derive(Clone)]
pub struct SeenBatch {
    /// Source command ID, not a socket frame invented by the client.
    pub command: u32,
    /// Source service path.
    pub path: &'static str,
    /// Whether the source group service owns this request.
    pub is_group: bool,
    // Private body prevents accidentally logging message/account identifiers.
    body: Value,
}
fn scalar(value: &Option<Value>) -> Option<Value> {
    value
        .as_ref()
        .filter(|v| {
            v.as_str()
                .is_some_and(|s| !s.is_empty() && s.len() <= 256 && s != "0")
                || v.as_u64().is_some_and(|n| n > 0)
        })
        .cloned()
}
impl SeenBatch {
    /// Only pass source-verified wire refs. Archive/local IDs must never be adapted here.
    /// Reject a partial mapping instead of manufacturing receipt fields from display IDs.
    pub fn from_wire(
        thread_id: &str,
        is_group: bool,
        refs: &[WireMessageRef],
        imei: &str,
    ) -> Result<Self> {
        if thread_id.is_empty()
            || thread_id.len() > 256
            || imei.is_empty()
            || imei.len() > 256
            || refs.is_empty()
            || refs.len() > 50
        {
            bail!("Invalid seen batch bounds");
        }
        let mut data = Vec::with_capacity(refs.len());
        for wire in refs {
            // Requiring original wire key msgId excludes the archive `id` fallback.
            if !wire.fields.contains_key("msgId") || wire.fields.contains_key("archive_source") {
                bail!("Unverified receipt mapping");
            }
            let (Some(cmi), Some(gmi), Some(si), Some(mut di), Some(mt)) = (
                scalar(&wire.client_id),
                scalar(&wire.server_id),
                scalar(&wire.sender),
                wire.destination
                    .as_ref()
                    .filter(|v| {
                        v.as_u64().is_some()
                            || v.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 256)
                    })
                    .cloned(),
                scalar(&wire.original_type),
            ) else {
                bail!("Incomplete receipt mapping");
            };
            if wire.fields.get("msgId") != Some(&gmi) {
                bail!("Inconsistent receipt mapping");
            }
            if let Some(id) = di.as_str() {
                di = json!(id.strip_prefix('g').unwrap_or(id));
            }
            let key = |value: &Value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            };
            if (is_group && key(&di) != thread_id.strip_prefix('g').unwrap_or(thread_id))
                || (!is_group && key(&si) != thread_id)
            {
                bail!("Receipt conversation mismatch");
            }
            if !is_group && di == si {
                di = json!("0");
            }
            for value in [&wire.st, &wire.at, &wire.cmd, &wire.ts]
                .into_iter()
                .flatten()
            {
                if !value.is_number() && !value.as_str().is_some_and(|s| s.len() <= 256) {
                    bail!("Invalid receipt field");
                }
            }
            data.push(json!({"cmi":cmi,"gmi":gmi,"si":si,"di":di,"mt":mt,
                "st":wire.st.clone().unwrap_or(json!(-1)),"at":wire.at.clone().unwrap_or(json!(-1)),
                "cmd":wire.cmd.clone().unwrap_or(json!(-1)),"ts":wire.ts.clone().unwrap_or(json!(-1))}));
        }
        let mut infos = json!({"data":data});
        infos[if is_group { "grid" } else { "senderId" }] = json!(if is_group {
            thread_id.strip_prefix('g').unwrap_or(thread_id)
        } else {
            thread_id
        });
        if serde_json::to_vec(&infos)?.len() > 65536 {
            bail!("Seen batch too large");
        }
        Ok(Self {
            command: if is_group { 12124 } else { 12118 },
            path: if is_group {
                "/api/group/seenv2"
            } else {
                "/api/message/seenv2"
            },
            is_group,
            body: json!({"msgInfos":serde_json::to_string(&infos)?,"imei":imei}),
        })
    }
    /// Return the bounded number of exact wire receipts.
    pub fn len(&self) -> usize {
        self.body
            .get("msgInfos")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| v.get("data").and_then(Value::as_array).map(Vec::len))
            .unwrap_or(0)
    }
    /// Whether the batch has no receipts.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Reject malformed, nested or nonzero-error acknowledgements.
///
/// # Errors
/// Returns a generic protocol error without private source response text.
pub fn parse_seen_response(response: &Value) -> Result<()> {
    let mut current = response.clone();
    let mut acknowledged = false;
    for _ in 0..4 {
        if let Some(code) = current
            .get("error_code")
            .or_else(|| current.get("errorCode"))
        {
            if code.as_i64().or_else(|| code.as_str()?.parse().ok()) != Some(0) {
                bail!("Seen acknowledgement rejected");
            }
            acknowledged = true;
        }
        match current.get("data") {
            Some(Value::String(text)) => {
                current = serde_json::from_str(text)
                    .map_err(|_| anyhow::anyhow!("Invalid seen acknowledgement"))?
            }
            Some(Value::Object(_)) => current = current["data"].clone(),
            _ => {
                return if current.is_object() && acknowledged {
                    Ok(())
                } else {
                    bail!("Invalid seen acknowledgement")
                }
            }
        }
    }
    bail!("Nested seen acknowledgement limit")
}

/// Logical focus alone is insufficient when a native macOS window is behind another application.
#[cfg(target_os = "macos")]
pub fn native_visible_window_foreground() -> bool {
    use std::ffi::{c_char, c_void};
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    unsafe fn object(receiver: *mut c_void, selector: &'static [u8]) -> *mut c_void {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        send(receiver, sel_registerName(selector.as_ptr().cast()))
    }
    unsafe fn flag(receiver: *mut c_void, selector: &'static [u8]) -> bool {
        let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i8 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        !receiver.is_null() && send(receiver, sel_registerName(selector.as_ptr().cast())) != 0
    }
    // SAFETY: These read-only AppKit getters run on the main GUI thread with static NUL-terminated selectors and matching signatures.
    unsafe {
        let workspace = object(
            objc_getClass(b"NSWorkspace\0".as_ptr().cast()),
            b"sharedWorkspace\0",
        );
        let front = object(workspace, b"frontmostApplication\0");
        if front.is_null() {
            return false;
        }
        let pid: unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32 =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        if pid(
            front,
            sel_registerName(b"processIdentifier\0".as_ptr().cast()),
        ) != std::process::id() as i32
        {
            return false;
        }
        let app = object(
            objc_getClass(b"NSApplication\0".as_ptr().cast()),
            b"sharedApplication\0",
        );
        if !flag(app, b"isActive\0") {
            return false;
        }
        let window = object(app, b"keyWindow\0");
        flag(window, b"isKeyWindow\0")
            && flag(window, b"isVisible\0")
            && !flag(window, b"isMiniaturized\0")
    }
}
/// Fail closed on platforms where no independent native foreground proof is implemented.
#[cfg(not(target_os = "macos"))]
pub fn native_visible_window_foreground() -> bool {
    false
}

/// A viewport ticket can cancel queued or in-flight transport after visibility changes.
#[derive(Clone)]
pub struct VisibilityTicket {
    value: u64,
    receiver: tokio::sync::watch::Receiver<(u64, std::time::Instant)>,
}
impl VisibilityTicket {
    /// Whether the same account/thread/viewport eligibility still holds.
    pub fn is_current(&self) -> bool {
        {
            let (value, sampled) = *self.receiver.borrow();
            value == self.value && sampled.elapsed() <= std::time::Duration::from_millis(250)
        }
    }
    /// Wait until the UI invalidates this specific visibility generation.
    pub async fn cancelled(&self) {
        let mut receiver = self.receiver.clone();
        loop {
            let (value, sampled) = *receiver.borrow_and_update();
            if value != self.value {
                return;
            }
            let deadline =
                tokio::time::Instant::from_std(sampled + std::time::Duration::from_millis(250));
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return,
                changed = receiver.changed() => if changed.is_err() { return; }
            }
        }
    }
}
/// Continuous sampled dwell; time gaps, changed viewport identity, or any lost gate reset it.
pub struct ReadVisibilityGate {
    key: String,
    since: Option<u64>,
    last: Option<u64>,
    generation: tokio::sync::watch::Sender<(u64, std::time::Instant)>,
}
impl Default for ReadVisibilityGate {
    fn default() -> Self {
        let (generation, _) = tokio::sync::watch::channel((0, std::time::Instant::now()));
        Self {
            key: String::new(),
            since: None,
            last: None,
            generation,
        }
    }
}
impl ReadVisibilityGate {
    /// Invalidate all outstanding visibility tickets without changing unread state.
    pub fn reset(&mut self) {
        self.since = None;
        self.last = None;
        self.key.clear();
        let value = self.generation.borrow().0.wrapping_add(1);
        self.generation
            .send_replace((value, std::time::Instant::now()));
    }
    /// Observe foreground/nonminimized/bottom eligibility at most 250 ms apart for 500 ms.
    pub fn observe(
        &mut self,
        key: &str,
        foreground: bool,
        minimized: bool,
        bottom: bool,
        now: u64,
    ) -> bool {
        let eligible = foreground && !minimized && bottom && !key.is_empty();
        if self.key != key
            || !eligible
            || self.last.is_some_and(|last| now < last || now - last > 250)
        {
            self.reset();
        }
        self.key = key.to_owned();
        self.last = Some(now);
        if !eligible {
            return false;
        }
        let value = self.generation.borrow().0;
        self.generation
            .send_replace((value, std::time::Instant::now()));
        now.saturating_sub(*self.since.get_or_insert(now)) >= 500
    }
    /// Invalidate receipts when no chat viewport was observed in this frame.
    pub fn finish_frame(&mut self, now: u64) {
        if self.last != Some(now) {
            self.reset();
        }
    }
    /// Capture only after a complete dwell for the current viewport.
    pub fn ticket(&self) -> Option<VisibilityTicket> {
        if !self
            .last
            .zip(self.since)
            .is_some_and(|(last, since)| last.saturating_sub(since) >= 500)
        {
            return None;
        }
        Some(VisibilityTicket {
            value: self.generation.borrow().0,
            receiver: self.generation.subscribe(),
        })
    }
}

/// Accept only incoming live rows with a complete source mapping, bounded to fifty per request.
pub fn verified_visible_receipts(
    thread: &str,
    is_group: bool,
    messages: &[crate::chat::ChatMessage],
) -> Vec<crate::chat::ChatMessage> {
    messages
        .iter()
        .filter(|message| {
            message.history_source.is_none()
                && !message.is_outgoing
                && message.kind != crate::message_model::MessageKind::System
                && SeenBatch::from_wire(
                    thread,
                    is_group,
                    std::slice::from_ref(&message.wire_ref),
                    "validation",
                )
                .is_ok()
        })
        .take(50)
        .cloned()
        .collect()
}
/// Start a single receipt only with a fresh ticket; only known-unsent work may be released.
pub(crate) async fn submit_visible(
    ticket: &VisibilityTicket,
    request: impl std::future::Future<Output = Result<()>>,
) -> (bool, Result<bool>) {
    if !ticket.is_current() {
        return (false, Ok(false));
    }
    let mut submitted = false;
    let result = tokio::select! {
        biased;
        _ = ticket.cancelled() => Err(anyhow::anyhow!("Visibility cancelled")),
        result = async { submitted = true; request.await } => result.map(|_| true)
    };
    if !submitted {
        (false, Ok(false))
    } else {
        (true, result)
    }
}
/// Send exactly once through the Session-owned, source-derived legacy seen-v2 contract.
///
/// # Errors
/// Returns bounded transport or protocol errors without plaintext receipt bodies.
pub async fn send_seen(session: &Session, batch: &SeenBatch) -> Result<()> {
    if session.login.is_none() {
        bail!("No active session");
    }
    let response = session
        .post_receipt_params(
            if batch.is_group { "group" } else { "chat" },
            batch.path,
            &batch.body,
        )
        .await?;
    parse_seen_response(&response)
}

fn receipt_identity(message: &crate::chat::ChatMessage) -> Option<String> {
    scalar(&message.wire_ref.server_id).map(|value| {
        value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string())
    })
}
/// Ephemeral live-arrival provenance; disk hydration, manual clears and history never populate it.
#[derive(Default)]
pub struct LiveReadLedger {
    account: String,
    capacity_reported: bool,
    live: std::collections::HashSet<(String, String)>,
    attempted: std::collections::HashSet<(String, String)>,
    order: std::collections::VecDeque<(String, String)>,
}
impl LiveReadLedger {
    /// Record exact source-verified incoming IDs only from the worker's live event variant.
    pub fn observe(
        &mut self,
        account: &str,
        current: &[crate::chat::Thread],
        updates: &[crate::chat::Thread],
    ) {
        if self.account != account {
            *self = Self {
                account: account.into(),
                ..Default::default()
            };
        }
        if account.is_empty() {
            return;
        }
        for thread in updates {
            let old: std::collections::HashSet<_> = current
                .iter()
                .filter(|old| old.id == thread.id)
                .flat_map(|old| &old.messages)
                .filter_map(receipt_identity)
                .collect();
            for message in &thread.messages {
                let Some(id) = receipt_identity(message) else {
                    continue;
                };
                let key = (thread.id.clone(), id.clone());
                if old.contains(&id)
                    || self.attempted.contains(&key)
                    || verified_visible_receipts(
                        &thread.id,
                        thread.is_group,
                        std::slice::from_ref(message),
                    )
                    .is_empty()
                {
                    continue;
                }
                if self.live.insert(key.clone()) {
                    self.order.push_back(key);
                }
                while self.order.len() > 4096 {
                    if let Some(old) = self.order.pop_front() {
                        self.live.remove(&old);
                        // Submitted identities remain excluded for this entire account epoch.
                    }
                }
            }
        }
    }
    /// Emit one user-visible capacity notice per account epoch; local reads remain independent.
    pub fn take_capacity_notice(&mut self) -> bool {
        if self.attempted.len() >= 4096 && !self.capacity_reported {
            self.capacity_reported = true;
            true
        } else {
            false
        }
    }
    /// Return only known-unsent reservations; submitted outcomes must never call this.
    pub fn release_unsent(&mut self, thread: &str, messages: &[crate::chat::ChatMessage]) {
        for message in messages {
            if let Some(id) = receipt_identity(message) {
                let key = (thread.to_owned(), id);
                if self.attempted.remove(&key) && self.order.contains(&key) {
                    self.live.insert(key);
                }
            }
        }
    }
    /// Claim at most fifty exact live receipts once; transport uncertainty never triggers replay.
    /// After 4096 reservations, fail closed for new wire work until the account epoch resets.
    pub fn claim(
        &mut self,
        thread: &str,
        is_group: bool,
        visible: &[crate::chat::ChatMessage],
    ) -> Vec<crate::chat::ChatMessage> {
        let mut selected = Vec::new();
        for message in visible {
            if selected.len() == 50 || self.attempted.len() >= 4096 {
                break;
            }
            let Some(id) = receipt_identity(message) else {
                continue;
            };
            let key = (thread.to_owned(), id);
            if self.live.contains(&key)
                && !verified_visible_receipts(thread, is_group, std::slice::from_ref(message))
                    .is_empty()
            {
                self.live.remove(&key);
                self.attempted.insert(key);
                selected.push(message.clone());
            }
        }
        selected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(id: &str, thread: &str) -> crate::chat::ChatMessage {
        crate::chat::ChatMessage {
            id: id.into(),
            kind: crate::message_model::MessageKind::Text,
            wire_ref: WireMessageRef::from_raw(
                &json!({"msgId":id,"cliMsgId":123,"uidFrom":thread,"idTo":"account-fixture","msgType":"chat.text","st":0,"at":0,"cmd":501,"ts":1700000000000_i64}),
            ),
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn continuous_dwell_cancels_for_focus_minimize_bottom_identity_and_observation_gaps() {
        for mode in 0..5 {
            let mut gate = ReadVisibilityGate::default();
            for now in [0, 100, 200, 300, 400] {
                assert!(!gate.observe("account/thread/visible", true, false, true, now));
            }
            assert!(gate.observe("account/thread/visible", true, false, true, 500));
            let ticket = gate.ticket().unwrap();
            assert!(ticket.is_current());
            assert!(!gate.observe(
                if mode == 3 {
                    "another-thread"
                } else {
                    "account/thread/visible"
                },
                mode != 0,
                mode == 1,
                mode != 2,
                if mode == 4 { 1000 } else { 501 }
            ));
            assert!(!ticket.is_current());
            tokio::time::timeout(std::time::Duration::from_millis(100), ticket.cancelled())
                .await
                .unwrap();
            assert!(!gate.observe("account/thread/visible", true, false, true, 1100));
        }
    }
    #[tokio::test]
    async fn queued_ticket_expires_without_new_viewport_samples() {
        let mut gate = ReadVisibilityGate::default();
        for now in [0, 100, 200, 300, 400, 500] {
            gate.observe("fixture", true, false, true, now);
        }
        let ticket = gate.ticket().unwrap();
        assert!(ticket.is_current());
        tokio::time::timeout(std::time::Duration::from_millis(500), ticket.cancelled())
            .await
            .unwrap();
        assert!(!ticket.is_current());
    }
    #[test]
    fn visible_local_read_survives_account_store_reopen_without_creating_wire_work() {
        use crate::store_v2::{AccountStore, StoreDelta};
        let root = std::env::temp_dir().join(format!(
            "zl-visible-read-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut store = AccountStore::open(&root, "fixture-account").unwrap();
        let thread = crate::store::StoredThread {
            thread_id: "sender-fixture".into(),
            name: "Fixture".into(),
            is_group: false,
            last_ts: 0,
        };
        let mut state = crate::unread::UnreadState::default();
        state.unread_ids.extend(["10".into(), "20".into()]);
        state.marked_unread = true;
        store
            .apply_delta(StoreDelta {
                threads: &[thread],
                unread: &[("sender-fixture", &state)],
                ..Default::default()
            })
            .unwrap();
        let visible = vec![message("10", "sender-fixture")];
        assert!(store
            .read_local_visible("sender-fixture", &visible)
            .unwrap());
        drop(store);
        let reopened = AccountStore::open(&root, "fixture-account").unwrap();
        let state = reopened.unread("sender-fixture").unwrap();
        assert_eq!(state.last_seen_id.as_deref(), Some("10"));
        assert!(!state.marked_unread);
        assert_eq!(state.unread_ids.into_iter().collect::<Vec<_>>(), vec!["20"]);
        assert!(LiveReadLedger::default()
            .claim("sender-fixture", false, &visible)
            .is_empty());
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn cancelled_before_submit_releases_live_reservation_but_unknown_submission_does_not() {
        let rows = vec![message("10", "thread-fixture")];
        let update = crate::chat::Thread {
            id: "thread-fixture".into(),
            messages: rows.clone(),
            ..Default::default()
        };
        let mut ledger = LiveReadLedger::default();
        ledger.observe("account-fixture", &[], &[update]);
        let reserved = ledger.claim("thread-fixture", false, &rows);
        let mut gate = ReadVisibilityGate::default();
        for now in [0, 100, 200, 300, 400, 500] {
            gate.observe("fixture", true, false, true, now);
        }
        let ticket = gate.ticket().unwrap();
        gate.reset();
        let polled = std::sync::atomic::AtomicBool::new(false);
        let (submitted, result) = submit_visible(&ticket, async {
            polled.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .await;
        assert!(!submitted && result.unwrap() == false);
        assert!(!polled.load(std::sync::atomic::Ordering::SeqCst));
        ledger.release_unsent("thread-fixture", &reserved);
        for now in [600, 700, 800, 900, 1000, 1100] {
            gate.observe("fixture", true, false, true, now);
        }
        assert_eq!(ledger.claim("thread-fixture", false, &rows).len(), 1);
        let (submitted, result) = submit_visible(&gate.ticket().unwrap(), async {
            Err(anyhow::anyhow!("Unknown delivery"))
        })
        .await;
        assert!(submitted && result.is_err());
        assert!(ledger.claim("thread-fixture", false, &rows).is_empty());
    }
    #[test]
    fn complete_seen_mapping_preserves_zero_large_ids_and_exact_sender_group_thread() {
        let message = message("9007199254740993001", "sender-fixture");
        let batch = SeenBatch::from_wire(
            "sender-fixture",
            false,
            &[message.wire_ref.clone()],
            "imei-fixture",
        )
        .unwrap();
        let inner: Value = serde_json::from_str(batch.body["msgInfos"].as_str().unwrap()).unwrap();
        assert_eq!(inner["data"][0]["gmi"], "9007199254740993001");
        assert_eq!(inner["data"][0]["st"], 0);
        assert_eq!(inner["data"][0]["at"], 0);
        assert!(SeenBatch::from_wire(
            "other-sender",
            false,
            &[message.wire_ref.clone()],
            "fixture"
        )
        .is_err());
        assert!(SeenBatch::from_wire("gwrong", true, &[message.wire_ref], "fixture").is_err());
        let group = WireMessageRef::from_raw(
            &json!({"msgId":"11","cliMsgId":"22","uidFrom":"sender","idTo":"groom","msgType":"chat.photo"}),
        );
        assert_eq!(
            SeenBatch::from_wire("groom", true, &[group], "fixture")
                .unwrap()
                .path,
            "/api/group/seenv2"
        );
        for response in [
            json!({}),
            json!({"error_code":1}),
            json!({"error_code":0,"data":{"error_code":4}}),
            json!({"error_code":0,"data":"bogus"}),
        ] {
            assert!(parse_seen_response(&response).is_err());
        }
        assert!(parse_seen_response(&json!({"error_code":0,"data":{}})).is_ok());
    }
    #[test]
    fn archive_manual_clear_and_reopened_local_rows_cannot_create_wire_receipts() {
        let mut row = message("10", "thread-fixture");
        row.history_source = Some(json!({"archive":true}));
        assert!(verified_visible_receipts("thread-fixture", false, &[row.clone()]).is_empty());
        let mut state = crate::unread::UnreadState {
            marked_unread: true,
            ..Default::default()
        };
        assert!(read_local_visible(&mut state, &[row]));
        assert!(!state.marked_unread);
        let mut ledger = LiveReadLedger::default();
        assert!(ledger
            .claim("thread-fixture", false, &[message("10", "thread-fixture")])
            .is_empty());
    }
    #[test]
    fn live_ledger_bounds_and_deduplicates_incoming_visible_receipts() {
        let rows: Vec<_> = (1..=70)
            .map(|id| message(&id.to_string(), "thread-fixture"))
            .collect();
        let update = crate::chat::Thread {
            id: "thread-fixture".into(),
            messages: rows.clone(),
            ..Default::default()
        };
        let mut ledger = LiveReadLedger::default();
        ledger.observe("account-fixture", &[], &[update.clone()]);
        assert_eq!(ledger.claim("thread-fixture", false, &rows).len(), 50);
        assert_eq!(ledger.claim("thread-fixture", false, &rows).len(), 20);
        ledger.observe("account-fixture", &[], &[update]);
        assert!(ledger.claim("thread-fixture", false, &rows).is_empty());
    }
    #[test]
    fn unknown_submission_stays_excluded_after_live_window_eviction_and_replay() {
        let mut ledger = LiveReadLedger::default();
        let update = |rows| crate::chat::Thread {
            id: "thread-fixture".into(),
            messages: rows,
            ..Default::default()
        };
        let original = vec![message("1", "thread-fixture")];
        ledger.observe("account-fixture", &[], &[update(original.clone())]);
        assert_eq!(ledger.claim("thread-fixture", false, &original).len(), 1);
        let later = (2..=4100)
            .map(|id| message(&id.to_string(), "thread-fixture"))
            .collect();
        ledger.observe("account-fixture", &[], &[update(later)]);
        ledger.observe("account-fixture", &[], &[update(original.clone())]);
        assert!(ledger.claim("thread-fixture", false, &original).is_empty());
        assert!(ledger.live.len() <= 4096 && ledger.attempted.len() <= 4096);
        let mut full = LiveReadLedger::default();
        let rows = (1..=4096)
            .map(|id| message(&id.to_string(), "thread-fixture"))
            .collect::<Vec<_>>();
        full.observe("account-fixture", &[], &[update(rows.clone())]);
        for chunk in rows.chunks(50) {
            assert_eq!(
                full.claim("thread-fixture", false, chunk).len(),
                chunk.len()
            );
        }
        assert_eq!(full.attempted.len(), 4096);
        assert!(full.take_capacity_notice());
        assert!(!full.take_capacity_notice());
        let next = vec![message("5000", "thread-fixture")];
        full.observe("account-fixture", &[], &[update(next.clone())]);
        assert!(full.claim("thread-fixture", false, &next).is_empty());
        assert_eq!(full.attempted.len(), 4096);
        let mut local = crate::unread::UnreadState::default();
        assert!(read_local_visible(&mut local, &next));
        assert_eq!(local.last_seen_id.as_deref(), Some("5000"));
        full.observe("next-account-epoch", &[], &[]);
        assert!(full.attempted.is_empty() && !full.capacity_reported);
    }
    #[tokio::test]
    async fn stale_runtime_ticket_blocks_session_receipt_before_any_outbound_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut session = Session::new(false, false, false).unwrap();
        session.domains.chat = format!("http://{}", listener.local_addr().unwrap());
        session.secret_key = Some(b"0123456789abcdef".to_vec());
        session.login = Some(crate::session::LoginInfo {
            uin: "account-fixture".into(),
            ..Default::default()
        });
        let batch = SeenBatch::from_wire(
            "thread-fixture",
            false,
            &[message("10", "thread-fixture").wire_ref],
            "imei-fixture",
        )
        .unwrap();
        let stale = crate::runtime_scope::SessionScope {
            account: "stale-fixture".into(),
            generation: u64::MAX,
        };
        assert!(
            crate::runtime_scope::run_stale_for_test(stale, send_seen(&session, &batch))
                .await
                .is_err()
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    #[tokio::test]
    async fn session_transport_matches_primary_legacy_cipher_cookie_path_and_response_contract() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut session = Session::new(false, false, false).unwrap();
        session.domains.chat = format!("http://{address}");
        session.imei = "imei-fixture".into();
        session.secret_key = Some(b"0123456789abcdef".to_vec());
        session.login = Some(crate::session::LoginInfo {
            uin: "account-fixture".into(),
            ..Default::default()
        });
        session.set_zpw_sek("fixture-cookie".into());
        let batch = SeenBatch::from_wire(
            "thread-fixture",
            false,
            &[message("10", "thread-fixture").wire_ref],
            &session.imei,
        )
        .unwrap();
        let expected = format!("params={}", session.encoded_params(&batch.body).unwrap());
        let encrypted = crate::crypto::encode_aes(
            session.session_key_bytes(),
            &json!({"error_code":0,"data":{}}).to_string(),
        )
        .unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 2048];
            let (header_end, length) = loop {
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let count = socket.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            let text = String::from_utf8(bytes).unwrap();
            let headers = text[..header_end].to_ascii_lowercase();
            assert!(
                text.starts_with("POST /api/message/seenv2?zpw_ver=694&zpw_type=30 HTTP/1.1\r\n")
            );
            assert!(headers.contains("cookie: zpw_sek=fixture-cookie"));
            assert_eq!(&text[header_end..], expected);
            assert!(!text.contains("signkey="));
            assert!(!text.contains("zcid="));
            assert!(!text.contains("thread-fixture"));
            let response = json!({"error_code":0,"data":encrypted}).to_string();
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });
        send_seen(&session, &batch).await.unwrap();
        server.join().unwrap();
    }
}
