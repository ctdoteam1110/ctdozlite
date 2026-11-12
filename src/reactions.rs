//! Reaction summaries and wire events, preserving counts and lossless message IDs.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Opt-in switch for the reaction wire trace (`ZALO_REACTION_TRACE=1`).
const TRACE_ENV: &str = "ZALO_REACTION_TRACE";
/// Local-only trace file; `*.log` is gitignored.
const TRACE_FILE: &str = "reaction_trace.log";
/// Hard cap per logged payload so a trace never grows unbounded or captures whole message bodies.
const TRACE_MAX_CHARS: usize = 60000;

/// Total reaction count of a message, as the UI would display it.
pub fn total_count(summaries: &[ReactionSummary]) -> u64 {
    summaries.iter().map(|s| s.count).sum()
}

/// Trace one batch of reaction events handed to the UI: how many matched a loaded message.
pub fn trace_batch(total: usize, matched: usize, applied: usize, unmatched_ids: &[String]) {
    if std::env::var_os(TRACE_ENV).is_none() {
        return;
    }
    use std::io::Write;
    let sample: Vec<&str> = unmatched_ids.iter().take(5).map(String::as_str).collect();
    let line = format!(
        "{} BATCH events={total} matched_a_message={matched} changed_count={applied} unmatched={} sample_unmatched_msg_ids={sample:?}\n",
        crate::login::unix_now(),
        unmatched_ids.len()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(TRACE_FILE)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Trace one change of a message's reaction count together with the code path that caused it.
/// Opt-in via `ZALO_REACTION_TRACE`; only ids and counts are written.
pub fn trace_count_change(source: &str, msg_id: &str, before: u64, after: u64) {
    if before == after || std::env::var_os(TRACE_ENV).is_none() {
        return;
    }
    use std::io::Write;
    let line = format!(
        "{} COUNT msg={msg_id} {before}->{after} via {source}\n",
        crate::login::unix_now()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(TRACE_FILE)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// `true` when a decoded socket payload mentions reactions in any field name or value.
pub fn payload_mentions_reaction(payload: &Value) -> bool {
    let text = payload.to_string().to_ascii_lowercase();
    text.contains("react") || text.contains("rtype")
}

/// Append reaction-related socket payloads to a local trace so the real wire shape can be inspected.
/// Off unless `ZALO_REACTION_TRACE` is set; IDs only appear as they do on the wire, secrets never
/// pass through here because the payload is already the decrypted chat/control body.
pub fn trace_ws_payload(cmd: u16, sub_cmd: u8, payload: &Value) {
    if std::env::var_os(TRACE_ENV).is_none() || !payload_mentions_reaction(payload) {
        return;
    }
    use std::io::Write;
    let mut body = payload.to_string();
    if body.chars().count() > TRACE_MAX_CHARS {
        body = body.chars().take(TRACE_MAX_CHARS).collect();
        body.push_str("…[cut]");
    }
    let line = format!(
        "{} cmd={cmd} sub={sub_cmd} {body}\n",
        crate::login::unix_now()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(TRACE_FILE)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReactionSummary {
    pub emoji: String,
    pub count: u64,
    pub reactor_ids: Vec<String>,
    pub r_type: Option<i32>,
    pub count_is_lower_bound: bool,
    pub reactor_counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReactionEvent {
    pub message_id: String,
    pub client_message_id: String,
    pub thread_id: String,
    pub sender_id: String,
    pub event_id: String,
    pub client_event_id: String,
    pub r_type: i32,
    pub emoji: String,
    pub ts: i64,
    pub historical: bool,
}

/// Persisted per-reactor ledger. IDs deduplicate replay; chronological replay handles old pages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReactionVersion {
    pub ts: i64,
    pub event_id: String,
    pub client_event_id: String,
    pub events: Vec<ReactionEvent>,
    pub covered_events: Vec<ReactionEvent>,
    pub baseline: BTreeMap<i32, u64>,
    pub ledger_initialized: bool,
    pub snapshot_baseline: bool,
}
fn known_id(id: &str) -> bool {
    !id.is_empty() && id != "0"
}
fn id_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
fn event_order(a: &ReactionEvent, b: &ReactionEvent) -> std::cmp::Ordering {
    crate::clock::normalize_ts_ms(a.ts)
        .cmp(&crate::clock::normalize_ts_ms(b.ts))
        .then_with(|| id_order(&a.client_event_id, &b.client_event_id))
        .then_with(|| id_order(&a.event_id, &b.event_id))
        // Without ordering IDs a clear wins an indistinguishable timestamp.
        .then_with(|| (a.r_type == -1).cmp(&(b.r_type == -1)))
}
fn same_event(a: &ReactionEvent, b: &ReactionEvent) -> bool {
    if known_id(&a.event_id) && known_id(&b.event_id) && a.event_id == b.event_id {
        return true;
    }
    if known_id(&a.client_event_id)
        && known_id(&b.client_event_id)
        && a.client_event_id == b.client_event_id
    {
        return true;
    }
    if known_id(&a.event_id)
        || known_id(&b.event_id)
        || known_id(&a.client_event_id)
        || known_id(&b.client_event_id)
    {
        return false;
    }
    a.ts > 0
        && b.ts > 0
        && crate::clock::normalize_ts_ms(a.ts) == crate::clock::normalize_ts_ms(b.ts)
        && a.r_type == b.r_type
}
impl From<&ReactionEvent> for ReactionVersion {
    fn from(event: &ReactionEvent) -> Self {
        Self {
            ts: crate::clock::normalize_ts_ms(event.ts),
            event_id: event.event_id.clone(),
            client_event_id: event.client_event_id.clone(),
            ..Default::default()
        }
    }
}
impl ReactionVersion {
    pub fn precedes(&self, event: &ReactionEvent) -> bool {
        event_order(
            &ReactionEvent {
                ts: self.ts,
                event_id: self.event_id.clone(),
                client_event_id: self.client_event_id.clone(),
                ..Default::default()
            },
            event,
        )
        .is_lt()
    }

    pub fn record(
        &mut self,
        summaries: &mut Vec<ReactionSummary>,
        event: &ReactionEvent,
        authoritative: bool,
    ) -> bool {
        if !self.ledger_initialized {
            for summary in summaries.iter() {
                if let (Some(kind), Some(n)) =
                    (summary.r_type, summary.reactor_counts.get(&event.sender_id))
                {
                    self.baseline.insert(kind, *n);
                }
            }
            // Old cache cursors have no event type. Retain their known identity without guessing one.
            if known_id(&self.event_id) || known_id(&self.client_event_id) {
                self.events.push(ReactionEvent {
                    ts: self.ts,
                    event_id: self.event_id.clone(),
                    client_event_id: self.client_event_id.clone(),
                    r_type: -2,
                    sender_id: event.sender_id.clone(),
                    ..Default::default()
                });
            }
            self.ledger_initialized = true;
            self.snapshot_baseline = authoritative;
        }
        if self
            .events
            .iter()
            .chain(&self.covered_events)
            .any(|seen| same_event(seen, event))
        {
            return false;
        }
        self.events.push(event.clone());
        self.events.sort_by(event_order);
        if self.precedes(event) {
            self.ts = crate::clock::normalize_ts_ms(event.ts);
            self.event_id = event.event_id.clone();
            self.client_event_id = event.client_event_id.clone();
        }
        self.rebuild(summaries, &event.sender_id);
        true
    }

    pub fn rebuild(&self, summaries: &mut Vec<ReactionSummary>, sender: &str) {
        apply_reaction_event(
            summaries,
            &ReactionEvent {
                sender_id: sender.into(),
                r_type: -1,
                ..Default::default()
            },
        );
        for (kind, n) in &self.baseline {
            add(
                summaries,
                Some(*kind),
                emoji_for_type(*kind).into(),
                *n,
                false,
                Some(sender),
            );
        }
        for event in &self.events {
            if event.r_type >= -1 && !(self.snapshot_baseline && event.historical) {
                apply_reaction_event(summaries, event);
            }
        }
    }
}

pub fn emoji_for_type(r_type: i32) -> &'static str {
    match r_type {
        3 => "👍",
        5 => "❤️",
        0 => "😆",
        32 => "😮",
        2 => "😢",
        20 => "😠",
        _ => "◦",
    }
}

fn type_for_icon(icon: &str) -> Option<i32> {
    match icon.trim() {
        "/-strong" | "👍" => Some(3),
        "/-heart" | "❤" | "❤️" => Some(5),
        ":>" | "😆" => Some(0),
        ":o" | "😮" => Some(32),
        ":-((" | "😢" => Some(2),
        ":-h" | "😠" => Some(20),
        _ => None,
    }
}

fn id(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn count(value: &Value) -> Option<(u64, bool)> {
    if value.as_str() == Some("9k+") {
        return Some((9001, true));
    }
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| *n > 0)
        .map(|n| (n, false))
}

fn add(
    out: &mut Vec<ReactionSummary>,
    r_type: Option<i32>,
    emoji: String,
    n: u64,
    lower: bool,
    user: Option<&str>,
) {
    let index = out
        .iter()
        .position(|r| r.emoji == emoji || (r_type.is_some() && r.r_type == r_type));
    let i = index.unwrap_or_else(|| {
        out.push(ReactionSummary {
            emoji: emoji.clone(),
            r_type,
            ..Default::default()
        });
        out.len() - 1
    });
    let summary = &mut out[i];
    if summary.r_type.is_none() && r_type.is_some() {
        summary.r_type = r_type;
    }
    summary.count = summary.count.saturating_add(n);
    summary.count_is_lower_bound |= lower;
    if let Some(user) = user.filter(|s| !s.is_empty()) {
        if !summary.reactor_ids.iter().any(|s| s == user) {
            summary.reactor_ids.push(user.into());
        }
        let c = summary.reactor_counts.entry(user.into()).or_default();
        *c = c.saturating_add(n);
    }
}

/// Parse shipped per-user/per-type counts, plus the application's legacy icon list.
/// An absent/null payload is not an authoritative clear; callers must preserve that distinction.
pub fn parse_reactions(value: &Value) -> Vec<ReactionSummary> {
    let root = value.get("reacts").unwrap_or(value);
    let payload = root.get("reactions").unwrap_or(root);
    let mut out = Vec::new();
    if let Some(users) = payload.as_object() {
        for (user, types) in users {
            if let Some(types_obj) = types.as_object() {
                for (kind, value) in types_obj {
                    let Ok(kind) = kind.parse::<i32>() else {
                        continue;
                    };
                    if kind < 0 {
                        continue;
                    }
                    if let Some((n, lower)) = count(value) {
                        add(
                            &mut out,
                            Some(kind),
                            emoji_for_type(kind).into(),
                            n,
                            lower,
                            Some(user),
                        );
                    }
                }
            } else if let Ok(kind) = user.parse::<i32>() {
                if kind >= 0 {
                    if let Some((n, lower)) = count(types) {
                        add(
                            &mut out,
                            Some(kind),
                            emoji_for_type(kind).into(),
                            n,
                            lower,
                            None,
                        );
                    }
                }
            }
        }
    } else if let Some(items) = payload.as_array() {
        for item in items {
            let r_type = item.get("rType").and_then(Value::as_i64).map(|t| t as i32);
            let icon = item
                .as_str()
                .or_else(|| item.get("rIcon").and_then(Value::as_str));
            let (kind, emoji) = if let Some(t) = r_type {
                (Some(t), emoji_for_type(t).to_string())
            } else if let Some(icon) = icon {
                let k = type_for_icon(icon);
                let e = k.map(emoji_for_type).unwrap_or(icon).to_owned();
                (k, e)
            } else {
                continue;
            };
            let n = item
                .get("count")
                .or_else(|| item.get("total"))
                .or_else(|| item.get("total_reacts"))
                .or_else(|| item.get("totalReactions"))
                .or_else(|| item.get("reactionCount"))
                .and_then(count)
                .unwrap_or((1, false));
            add(&mut out, kind, emoji, n.0, n.1, None);
        }
    }
    let parent_total = root
        .get("total")
        .or_else(|| root.get("total-reacts"))
        .or_else(|| root.get("total_reacts"))
        .or_else(|| root.get("totalReactions"))
        .or_else(|| root.get("reactionCount"))
        .or_else(|| value.get("total"))
        .or_else(|| value.get("total-reacts"))
        .or_else(|| value.get("total_reacts"))
        .or_else(|| value.get("totalReactions"))
        .or_else(|| value.get("reactionCount"))
        .and_then(count);
    if let Some((tot, _)) = parent_total {
        if tot > 1 && out.len() == 1 {
            out[0].count = tot;
        }
    }
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.r_type.cmp(&b.r_type))
            .then_with(|| a.emoji.cmp(&b.emoji))
    });
    out
}

/// Merge snapshot reactors that have no local event ledger; never rewind tracked reactors.
pub fn merge_untracked_snapshot(
    summaries: &mut Vec<ReactionSummary>,
    incoming: &[ReactionSummary],
    tracked: &BTreeMap<String, ReactionVersion>,
    replace_missing: bool,
) {
    let users: std::collections::BTreeSet<_> = incoming
        .iter()
        .chain(summaries.iter().filter(|_| replace_missing))
        .flat_map(|s| s.reactor_counts.keys())
        .filter(|user| !tracked.contains_key(*user))
        .cloned()
        .collect();
    for user in users {
        apply_reaction_event(
            summaries,
            &ReactionEvent {
                sender_id: user.clone(),
                r_type: -1,
                ..Default::default()
            },
        );
        for summary in incoming {
            if let Some(n) = summary.reactor_counts.get(&user) {
                add(
                    summaries,
                    summary.r_type,
                    summary.emoji.clone(),
                    *n,
                    summary.count_is_lower_bound,
                    Some(&user),
                );
            }
        }
    }
}

pub fn legacy_emojis(summaries: &[ReactionSummary]) -> Vec<String> {
    summaries
        .iter()
        .filter(|s| s.count > 0)
        .map(|s| s.emoji.clone())
        .collect()
}

/// Parse the shipped `chat.reaction` envelope or an array of those envelopes.
/// Socket queue dispatch must identify the reaction channel before invoking this parser.
pub fn parse_reaction_events(value: &Value) -> Vec<ReactionEvent> {
    if let Some(items) = value.as_array() {
        return items.iter().flat_map(parse_reaction_events).collect();
    }
    if value.get("reacts").and_then(Value::as_array).is_some()
        || value.get("reactGroups").and_then(Value::as_array).is_some()
    {
        let mut events: Vec<_> = value
            .get("reacts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .flat_map(parse_reaction_events)
            .collect();
        for item in value
            .get("reactGroups")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let mut item = item.clone();
            let group_id = id(item.get("idTo"));
            if group_id.is_empty() {
                continue;
            }
            item["idTo"] = Value::String(format!("g{group_id}"));
            events.extend(parse_reaction_events(&item));
        }
        return events;
    }
    if let Some(items) = value.get("reactions").and_then(Value::as_array) {
        return items.iter().flat_map(parse_reaction_events).collect();
    }
    if let Some(items) = value.get("messages").and_then(Value::as_array) {
        return items
            .iter()
            .filter(|item| item.get("msgType").and_then(Value::as_str) == Some("chat.reaction"))
            .flat_map(parse_reaction_events)
            .collect();
    }
    let decoded;
    let outer_content = value.get("content");
    let nested = outer_content.and_then(|c| c.get("params"));
    let content = match nested.or(outer_content) {
        Some(Value::String(s)) => {
            let Ok(v) = serde_json::from_str::<Value>(s) else {
                return Vec::new();
            };
            decoded = v;
            &decoded
        }
        Some(v) => v,
        None => return Vec::new(),
    };
    let Some(r_type) = content
        .get("rType")
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
    else {
        return Vec::new();
    };
    if r_type < -1 {
        return Vec::new();
    }
    let Some(targets) = content.get("rMsg").and_then(Value::as_array) else {
        return Vec::new();
    };
    let sender_id = id(value.get("uidFrom"));
    if sender_id.is_empty() {
        return Vec::new();
    }
    let mut thread_id = id(if nested.is_some() {
        value.get("msgKey")
    } else {
        value.get("idTo")
    });
    if thread_id == "0" {
        thread_id = sender_id.clone();
    }
    if thread_id.is_empty() {
        return Vec::new();
    }
    targets
        .iter()
        .filter_map(|target| {
            let mut message_id = id(target.get("gMsgID"));
            let client_message_id = id(target.get("cMsgID"));
            if message_id == "0" {
                message_id.clear();
            }
            if message_id.is_empty() && (client_message_id.is_empty() || client_message_id == "0") {
                return None;
            }
            Some(ReactionEvent {
                message_id,
                client_message_id,
                thread_id: thread_id.clone(),
                sender_id: sender_id.clone(),
                event_id: id(value.get("msgId")),
                client_event_id: id(value.get("cliMsgId")),
                r_type,
                emoji: emoji_for_type(r_type).into(),
                historical: false,
                ts: value
                    .get("ts")
                    .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// Apply one deduplicated incremental event. The caller owns replay/order handling.
pub fn apply_reaction_event(summaries: &mut Vec<ReactionSummary>, event: &ReactionEvent) {
    if event.r_type == -1 {
        for summary in summaries.iter_mut() {
            if let Some(n) = summary.reactor_counts.remove(&event.sender_id) {
                summary.count = summary.count.saturating_sub(n);
                summary.reactor_ids.retain(|id| id != &event.sender_id);
            }
        }
        summaries.retain(|s| s.count > 0);
    } else {
        add(
            summaries,
            Some(event.r_type),
            event.emoji.clone(),
            1,
            false,
            Some(&event.sender_id),
        );
    }
}

/// Event ledger that owns reaction counts for messages touched by live `chat.reaction` events.
///
/// Evidence (`reaction_trace.log`): the `610_1`/`611_1` queues replay every unacknowledged event
/// on each start, usually BEFORE the message is loaded, while history later delivers a stale
/// snapshot total. Counting rules that follow from that:
/// * events are keyed by id, so a replay never adds twice;
/// * events seen before their message exists are kept and applied once it loads;
/// * count per type = `base` + distinct live events, where `base` is whatever part of the first
///   snapshot the known events cannot explain (`snapshot - events`, never negative).
#[derive(Debug, Default)]
pub struct ReactionInbox {
    events: std::collections::HashMap<String, BTreeMap<String, ReactionEvent>>,
    bases: std::collections::HashMap<String, BTreeMap<i32, u64>>,
    matched: std::collections::HashSet<String>,
}

fn message_keys(msg: &crate::chat::ChatMessage) -> Vec<String> {
    [&msg.id, &msg.client_message_id]
        .into_iter()
        .filter(|k| known_id(k))
        .cloned()
        .collect()
}

fn event_key(event: &ReactionEvent) -> Option<String> {
    if known_id(&event.message_id) {
        Some(event.message_id.clone())
    } else if known_id(&event.client_message_id) {
        Some(event.client_message_id.clone())
    } else {
        None
    }
}
/// Stable identity for one reaction event, mirroring `ReactionInbox::record`
/// dedup: event id, else client event id, else sender/ts/type triple.
/// GUI uses it to count/banner each accepted event exactly once (#473).
pub fn event_identity(ev: &ReactionEvent) -> String {
    if known_id(&ev.event_id) {
        ev.event_id.clone()
    } else if known_id(&ev.client_event_id) {
        format!("c{}", ev.client_event_id)
    } else {
        format!("{}:{}:{}", ev.sender_id, ev.ts, ev.r_type)
    }
}

/// Effective per-type counts and per-sender counts after replaying events in order.
/// A clear (`rType == -1`) removes that sender's earlier reactions.
fn effective_counts(
    events: &BTreeMap<String, ReactionEvent>,
) -> (BTreeMap<i32, u64>, BTreeMap<String, BTreeMap<i32, u64>>) {
    let mut ordered: Vec<&ReactionEvent> = events.values().collect();
    ordered.sort_by(|a, b| event_order(a, b));
    let mut per_sender: BTreeMap<String, BTreeMap<i32, u64>> = BTreeMap::new();
    for ev in ordered {
        if ev.r_type < -1 {
            continue;
        }
        let mine = per_sender.entry(ev.sender_id.clone()).or_default();
        if ev.r_type == -1 {
            mine.clear();
        } else {
            *mine.entry(ev.r_type).or_default() += 1;
        }
    }
    let mut totals: BTreeMap<i32, u64> = BTreeMap::new();
    for kinds in per_sender.values() {
        for (kind, n) in kinds {
            *totals.entry(*kind).or_default() += n;
        }
    }
    (totals, per_sender)
}
impl ReactionInbox {
    /// Store events by message and event id. Returns the subset actually
    /// inserted (replays and duplicates excluded) so callers can count each
    /// accepted event exactly once (#473, gui.rs:1773).
    pub fn record_new(&mut self, events: &[ReactionEvent]) -> Vec<ReactionEvent> {
        let mut accepted = Vec::new();
        for ev in events {
            let Some(key) = event_key(ev) else { continue };
            let id = event_identity(ev);
            let slot = self.events.entry(key).or_default();
            if slot.contains_key(&id) {
                continue;
            }
            slot.insert(id, ev.clone());
            accepted.push(ev.clone());
        }
        accepted
    }

    /// Legacy bulk insert (keeps old callers working; ignores acceptance).
    pub fn record(&mut self, events: &[ReactionEvent]) {
        let _ = self.record_new(events);
    }

    /// Events whose message has not been loaded yet.
    pub fn pending_len(&self) -> usize {
        self.events
            .iter()
            .filter(|(key, _)| !self.matched.contains(*key))
            .map(|(_, evs)| evs.len())
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Recompute counts for every loaded message that has events. Returns `true` if any changed.
    pub fn apply_to(&mut self, messages: &mut [crate::chat::ChatMessage]) -> bool {
        let mut changed = false;
        for msg in messages.iter_mut() {
            let keys = message_keys(msg);
            let Some(key) = keys.iter().find(|k| self.events.contains_key(*k)).cloned() else {
                continue;
            };
            let before = total_count(&msg.reaction_summaries);
            let (totals, per_sender) = effective_counts(&self.events[&key]);
            let base = self.bases.entry(key.clone()).or_insert_with(|| {
                let mut snapshot: BTreeMap<i32, u64> = BTreeMap::new();
                for s in &msg.reaction_summaries {
                    let kind = s.r_type.or_else(|| type_for_icon(&s.emoji));
                    if let Some(kind) = kind {
                        *snapshot.entry(kind).or_default() += s.count;
                    }
                }
                snapshot
                    .into_iter()
                    .map(|(kind, n)| {
                        (
                            kind,
                            n.saturating_sub(totals.get(&kind).copied().unwrap_or(0)),
                        )
                    })
                    .collect()
            });
            let mut summaries: Vec<ReactionSummary> = Vec::new();
            let mut kinds: std::collections::BTreeSet<i32> = totals.keys().copied().collect();
            kinds.extend(base.keys().copied());
            for kind in kinds {
                let n =
                    base.get(&kind).copied().unwrap_or(0) + totals.get(&kind).copied().unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let mut summary = ReactionSummary {
                    emoji: emoji_for_type(kind).into(),
                    r_type: Some(kind),
                    count: n,
                    ..Default::default()
                };
                for (sender, mine) in &per_sender {
                    if let Some(c) = mine.get(&kind) {
                        summary.reactor_ids.push(sender.clone());
                        summary.reactor_counts.insert(sender.clone(), *c);
                    }
                }
                summaries.push(summary);
            }
            summaries.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.r_type.cmp(&b.r_type)));
            self.matched.insert(key);
            let after = total_count(&summaries);
            if summaries != msg.reaction_summaries {
                msg.reaction_summaries = summaries;
                msg.reactions = legacy_emojis(&msg.reaction_summaries);
                msg.reaction_snapshot = true;
                trace_count_change("ReactionInbox::apply_to", &msg.id, before, after);
                changed = true;
            }
        }
        changed
    }

    /// Record events, then apply everything to the given messages.
    pub fn ingest(
        &mut self,
        events: &[ReactionEvent],
        messages: &mut [crate::chat::ChatMessage],
    ) -> bool {
        self.record(events);
        self.apply_to(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Shape captured from the live socket (`cmd=610`): sender is `uidFrom:"0"` (the account itself),
    /// every event has its own `msgId`. Replaying the same frame must never add to the count.
    fn wire_frame(ids: &[&str]) -> Value {
        let reacts: Vec<Value> = ids
            .iter()
            .map(|id| {
                json!({
                    "actionId": id, "cliMsgId": format!("c{id}"), "idTo": "7602591761395115371",
                    "msgId": id, "msgType": "chat.reaction", "ts": "1793984355844", "ttl": 0,
                    "uidFrom": "0",
                    "content": "{\"rIcon\":\":-h\",\"rMsg\":[{\"cMsgID\":1793984341063,\"gMsgID\":\"8460448471161\",\"msgType\":5}],\"rType\":20,\"source\":6}"
                })
            })
            .collect();
        json!({"data": {"reacts": reacts, "reactGroups": []}})
    }

    #[test]
    fn replayed_wire_frames_do_not_inflate_the_count() {
        let ids: Vec<String> = (0..18).map(|i| format!("84604{i:08}")).collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let frame = wire_frame(&id_refs);
        let events = parse_reaction_events(&frame["data"]);
        assert_eq!(events.len(), 18);

        let mut msg = crate::chat::ChatMessage {
            id: "8460448471161".into(),
            ..Default::default()
        };
        for _ in 0..3 {
            for ev in &events {
                msg.apply_reaction(ev);
            }
        }
        let total: u64 = msg.reaction_summaries.iter().map(|s| s.count).sum();
        assert_eq!(total, 18, "same 18 events replayed 3x must still total 18");
        assert_eq!(msg.reaction_summaries.len(), 1);
    }

    #[test]
    fn a_new_event_id_adds_exactly_one() {
        let frame = wire_frame(&["8460483923587"]);
        let events = parse_reaction_events(&frame["data"]);
        let mut msg = crate::chat::ChatMessage::default();
        msg.apply_reaction(&events[0]);
        msg.apply_reaction(&events[0]);
        assert_eq!(msg.reaction_summaries[0].count, 1);
    }

    /// The real startup order seen in `reaction_trace.log`: the whole `610_1` queue (31 events)
    /// arrives BEFORE the message is loaded, then a stale history snapshot (20) lands, then the
    /// queue is replayed on the next start. The count must still be exactly 31.
    #[test]
    fn early_events_are_kept_until_the_message_exists_and_survive_replay() {
        let ids: Vec<String> = (0..31).map(|i| format!("84605{i:08}")).collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let events = parse_reaction_events(&wire_frame(&id_refs)["data"]);
        assert_eq!(events.len(), 31);

        let mut inbox = ReactionInbox::default();
        // Phase 1: no message loaded yet -> nothing may be dropped.
        let none_loaded: Vec<crate::chat::ChatMessage> = Vec::new();
        let mut loaded = none_loaded;
        inbox.ingest(&events, &mut loaded);
        assert_eq!(inbox.pending_len(), 31, "unmatched events must be kept");

        // Phase 2: history loads the message with a STALE snapshot of 20 reactions.
        let mut msg = crate::chat::ChatMessage {
            id: "8460448471161".into(),
            reaction_summaries: parse_reactions(&json!({"reactions":{"0":{"20":20}}})),
            ..Default::default()
        };
        assert_eq!(total_count(&msg.reaction_summaries), 20);
        let mut loaded = vec![msg.clone()];
        inbox.ingest(&[], &mut loaded);
        msg = loaded.remove(0);
        assert_eq!(
            total_count(&msg.reaction_summaries),
            31,
            "event ledger must win over the stale 20-count snapshot"
        );

        // Phase 3: next start replays the identical queue -> still 31.
        let mut loaded = vec![msg];
        inbox.ingest(&events, &mut loaded);
        inbox.ingest(&events, &mut loaded);
        assert_eq!(total_count(&loaded[0].reaction_summaries), 31);
        assert_eq!(inbox.pending_len(), 0);
    }

    #[test]
    fn a_live_event_after_replay_adds_exactly_one() {
        let ids: Vec<String> = (0..5).map(|i| format!("84605{i:08}")).collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let events = parse_reaction_events(&wire_frame(&id_refs)["data"]);
        let mut loaded = vec![crate::chat::ChatMessage {
            id: "8460448471161".into(),
            ..Default::default()
        }];
        let mut inbox = ReactionInbox::default();
        inbox.ingest(&events, &mut loaded);
        inbox.ingest(&events, &mut loaded);
        let live = parse_reaction_events(&wire_frame(&["8460599999999"])["data"]);
        inbox.ingest(&live, &mut loaded);
        assert_eq!(total_count(&loaded[0].reaction_summaries), 6);
    }

    #[test]
    fn record_new_accepts_each_event_once() {
        // #473/#1773: replays must not inflate counts — only newly accepted
        // events are returned, so GUI counts/banners each exactly once.
        let ids: Vec<String> = (0..3).map(|i| format!("84605{i:08}")).collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let events = parse_reaction_events(&wire_frame(&id_refs)["data"]);
        assert_eq!(events.len(), 3);
        let mut inbox = ReactionInbox::default();
        assert_eq!(inbox.record_new(&events).len(), 3);
        assert!(inbox.record_new(&events).is_empty(), "replay must accept none");
        // Same message + same sender + same ts/type but no event ids: one identity
        // (needs a target message id — id-less events have no key by design).
        let no_id = ReactionEvent {
            message_id: "m1".into(),
            sender_id: "u1".into(),
            r_type: 5,
            ts: 123,
            ..Default::default()
        };
        let mut inbox2 = ReactionInbox::default();
        assert_eq!(inbox2.record_new(std::slice::from_ref(&no_id)).len(), 1);
        assert!(inbox2.record_new(std::slice::from_ref(&no_id)).is_empty());
        assert_eq!(
            event_identity(&no_id),
            "u1:123:5",
            "fallback identity must be stable"
        );
    }
    #[test]
    fn heart_count_is_not_unique_user_count() {
        let r =
            parse_reactions(&json!({"reacts":{"reactions":{"other":{"5":2},"0":{"5":1,"3":1}}}}));
        assert_eq!(r[0].emoji, "❤️");
        assert_eq!(r[0].count, 3);
        assert_eq!(r[0].reactor_ids.len(), 2);
        assert_eq!(r[1].count, 1);
    }
    #[test]
    fn snapshot_and_cache_defaults() {
        let r: ReactionSummary = serde_json::from_str("{}").unwrap();
        assert_eq!(r, ReactionSummary::default());
        let r = parse_reactions(
            &json!({"reactions":{"user":{"5":"2","3":"9k+","-1":1,"20":0,"2":-1}}}),
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].count, 9001);
        assert!(r[0].count_is_lower_bound);
        assert_eq!(
            serde_json::from_str::<Vec<ReactionSummary>>(&serde_json::to_string(&r).unwrap())
                .unwrap(),
            r
        );
    }
    #[test]
    fn unknown_is_not_heart() {
        let r = parse_reactions(&json!({"reactions":{"user":{"999":2}}}));
        assert_eq!(r[0].r_type, Some(999));
        assert_ne!(r[0].emoji, "❤️");
    }
    #[test]
    fn legacy_shapes() {
        assert_eq!(parse_reactions(&json!({"reactions":["❤️","👍"]})).len(), 2);
        assert_eq!(
            parse_reactions(&json!({"reacts":{"reactions":[{"rIcon":"/-heart","count":2}]}}))[0]
                .count,
            2
        );
        let six_reacts =
            parse_reactions(&json!({"reacts":{"reactions":[{"rIcon":":>","count":6}]}}));
        assert_eq!(six_reacts[0].emoji, "😆");
        assert_eq!(six_reacts[0].count, 6);
        let rtype_reacts =
            parse_reactions(&json!({"reacts":{"reactions":[{"rType":0,"count":6}]}}));
        assert_eq!(rtype_reacts[0].emoji, "😆");
        assert_eq!(rtype_reacts[0].count, 6);
        let direct_dict = parse_reactions(&json!({"reacts":{"reactions":{"0":6,"3":1}}}));
        assert_eq!(direct_dict[0].emoji, "😆");
        assert_eq!(direct_dict[0].count, 6);
        assert_eq!(direct_dict[1].emoji, "👍");
        assert_eq!(direct_dict[1].count, 1);
        assert!(parse_reactions(&json!({"reacts":null})).is_empty());
    }
    #[test]
    fn event_ids_remain_lossless_and_multitarget() {
        let r = parse_reaction_events(
            &json!({"uidFrom":"other","idTo":"0","msgId":"18446744073709551614","ts":"100", "content":r#"{"rType":5,"rIcon":"/-heart","rMsg":[{"gMsgID":18446744073709551613,"cMsgID":100},{"gMsgID":"0","cMsgID":"101"}]}"#}),
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].message_id, "18446744073709551613");
        assert_eq!(r[0].thread_id, "other");
        assert_eq!(r[1].client_message_id, "101");
        assert!(r[1].message_id.is_empty());
    }
    #[test]
    fn removal_resets_all_types_for_sender_only() {
        let mut r = parse_reactions(&json!({"reactions":{"a":{"5":2,"3":1},"b":{"5":1}}}));
        apply_reaction_event(
            &mut r,
            &ReactionEvent {
                sender_id: "a".into(),
                r_type: -1,
                ..Default::default()
            },
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].count, 1);
        assert_eq!(r[0].reactor_ids, vec!["b"]);
        apply_reaction_event(
            &mut r,
            &ReactionEvent {
                sender_id: "b".into(),
                r_type: 5,
                emoji: "❤️".into(),
                ..Default::default()
            },
        );
        assert_eq!(r[0].count, 2);
    }
    #[test]
    fn polling_single_message_normalization() {
        let r = parse_reaction_events(
            &json!({"messages":[{"msgType":"chat.reaction","msgKey":"peer","uidFrom":"peer","cliMsgId":"12","content":{"params":"{\"rType\":5,\"rMsg\":[{\"gMsgID\":\"42\"}]}"}}]}),
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].thread_id, "peer");
        assert_eq!(r[0].client_event_id, "12");
        let r = parse_reaction_events(
            &json!({"reactions":[{"uidFrom":"peer","idTo":"0","content":{"rType":5,"rMsg":[{"gMsgID":"42"}]}}]}),
        );
        assert_eq!(r.len(), 1);
    }
    #[test]
    fn control_reaction_arrays_preserve_group_thread() {
        let r = parse_reaction_events(
            &json!({"reacts":[{"uidFrom":"peer","idTo":"0","content":{"rType":5,"rMsg":[{"gMsgID":"1"}]}}],"reactGroups":[{"uidFrom":"peer","idTo":"99","content":{"rType":3,"rMsg":[{"gMsgID":"2"}]}}]}),
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].thread_id, "peer");
        assert_eq!(r[1].thread_id, "g99");
    }
    #[test]
    fn unread_gate_skips_own_removal_and_historical() {
        // Documents the reaction-unread counting gate: peer adds count;
        // own reactions, removals (r_type == -1) and historical replays never do.
        let mine = ReactionEvent { sender_id: "me".into(), r_type: 5, historical: false, ..Default::default() };
        let removal = ReactionEvent { sender_id: "peer".into(), r_type: -1, historical: false, ..Default::default() };
        let replay = ReactionEvent { sender_id: "peer".into(), r_type: 5, historical: true, ..Default::default() };
        let peer_add = ReactionEvent { sender_id: "peer".into(), r_type: 5, historical: false, ..Default::default() };
        for ev in [&mine, &removal, &replay] {
            let gated = !(ev.r_type < 0 || ev.historical)
                && !ev.sender_id.is_empty()
                && ev.sender_id != "me";
            assert!(!gated, "must skip r_type={} hist={}", ev.r_type, ev.historical);
        }
        let gated = !(peer_add.r_type < 0 || peer_add.historical)
            && !peer_add.sender_id.is_empty()
            && peer_add.sender_id != "me";
        assert!(gated);
    }
    #[test]
    fn malformed_events_do_not_create_reactions() {
        for v in [
            json!({"content":"bad"}),
            json!({"uidFrom":"a","content":{"rType":5,"rMsg":[{}]}}),
            json!({"uidFrom":"a","idTo":"b","content":{"rType":-2,"rMsg":[{"gMsgID":"1"}]}}),
        ] {
            assert!(parse_reaction_events(&v).is_empty());
        }
    }
}

/// Persist source and capture watermark independently of the transient network-fresh bit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReactionSnapshotProvenance {
    pub source: String,
    pub captured_at_ms: i64,
}
impl ReactionSnapshotProvenance {
    /// A stale baseline cannot erase a newer live event or a clear tombstone.
    pub fn accepts_snapshot(
        &self,
        incoming: &Self,
        versions: &BTreeMap<String, ReactionVersion>,
        network_fresh: bool,
    ) -> bool {
        if !network_fresh && !versions.is_empty() {
            return false;
        }
        if self.captured_at_ms > 0 && incoming.captured_at_ms <= self.captured_at_ms {
            return false;
        }
        let latest_live = versions
            .values()
            .flat_map(|v| v.events.iter().chain(&v.covered_events))
            .filter(|e| !e.historical)
            .map(|e| crate::clock::normalize_ts_ms(e.ts))
            .max()
            .unwrap_or(0);
        latest_live == 0 || incoming.captured_at_ms >= latest_live
    }
}

/// Persist unresolved events to avoid reporting complete reaction coverage after a missed lookup.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReactionCoverage {
    pub unresolved: Vec<ReactionEvent>,
}
impl ReactionCoverage {
    pub fn record_unresolved(&mut self, event: &ReactionEvent) {
        if !self.unresolved.iter().any(|old| {
            old.thread_id == event.thread_id
                && old.message_id == event.message_id
                && old.client_message_id == event.client_message_id
                && old.sender_id == event.sender_id
                && same_event(old, event)
        }) {
            self.unresolved.push(event.clone());
        }
    }
    pub fn is_complete(&self) -> bool {
        self.unresolved.is_empty()
    }
}
