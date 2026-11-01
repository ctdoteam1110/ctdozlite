//! Bounded history pagination over the shared command-511 WebSocket stream.
//!
//! Wire contract: zca-js listen.ts at 2a7e360257dfe98e2eb48dc46bbf389834be7e0b.
//! This is a global group-message stream, not a per-contact history endpoint.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const BATCH_PAGES: usize = 20;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(25);

fn identifier(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) if !s.is_empty() && s != "0" => Some(s.clone()),
        Value::Number(n) if n.as_u64().is_some_and(|v| v > 0) => Some(n.to_string()),
        _ => None,
    }
}

/// One validated global group-history page, with its wire-order continuation cursor.
#[derive(Debug)]
pub(super) struct GroupHistoryPage {
    /// Raw source messages retained in wire order.
    pub messages: Vec<Value>,
    next_cursor: Option<String>,
}

impl GroupHistoryPage {
    /// Decode supported envelopes without accepting malformed errors or message rows.
    ///
    /// # Errors
    /// Returns an error for a rejected envelope, invalid JSON, missing list or invalid row.
    pub fn parse(payload: &Value) -> Result<Self> {
        fn check_error(value: &Value) -> Result<()> {
            if let Some(code) = value.get("error_code") {
                let code = code
                    .as_i64()
                    .or_else(|| code.as_str().and_then(|s| s.parse().ok()))
                    .context("Mã lỗi lịch sử nhóm sai định dạng")?;
                if code != 0 {
                    bail!("Máy chủ trả lỗi khi tải lịch sử nhóm");
                }
            }
            Ok(())
        }
        check_error(payload)?;
        let raw = payload.get("data").unwrap_or(payload);
        let decoded;
        let data = if let Some(text) = raw.as_str() {
            decoded = serde_json::from_str::<Value>(text)
                .context("Phản hồi lịch sử nhóm chưa giải mã được")?;
            &decoded
        } else {
            raw
        };
        check_error(data)?;
        let data = data.get("data").unwrap_or(data);
        check_error(data)?;
        let messages = data
            .get("groupMsgs")
            .and_then(Value::as_array)
            .context("Phản hồi lịch sử nhóm thiếu danh sách groupMsgs")?;
        if messages.len() > 10_000 {
            bail!("Trang lịch sử nhóm vượt giới hạn số tin");
        }
        let messages = messages.clone();
        if messages.iter().any(|m| !m.is_object()) {
            bail!("Phản hồi lịch sử nhóm chứa tin nhắn sai định dạng");
        }
        // The owning consumer uses the last wire-order message, not a timestamp sort.
        let next_cursor = messages
            .last()
            .and_then(|m| identifier(m.get("actionId")).or_else(|| identifier(m.get("msgId"))));
        Ok(Self {
            messages,
            next_cursor,
        })
    }

    /// Normalize only known own sender aliases; never rewrite a group destination.
    pub fn normalized_payload(&self, own_uid: &str) -> Value {
        let messages: Vec<Value> = self
            .messages
            .iter()
            .cloned()
            .map(|mut msg| {
                // Both Zalo sentinel and explicit own sender IDs occur across transports.
                // Normalize senders only. Group destinations must retain their group identity.
                for key in ["uidFrom", "fromUid", "fromId", "senderId"] {
                    if !own_uid.is_empty() && identifier(msg.get(key)).as_deref() == Some(own_uid) {
                        msg[key] = json!("0");
                    }
                }
                msg["isGroup"] = json!(true);
                msg
            })
            .collect();
        json!({"error_code": 0, "data": {"groupMsgs": messages}})
    }
}

/// Caller-controlled continuation request for the shared socket.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct GroupHistoryRequest {
    /// Exact cursor from the final wire-order message, or null for the first page.
    pub last_id: Option<String>,
}

/// State transition and optional next page within the bounded batch.
pub(super) struct GroupHistoryUpdate {
    /// Present only while an owned batch has pages remaining.
    pub next_request: Option<GroupHistoryRequest>,
    /// Vietnamese user-visible progress or terminal state.
    pub status: String,
}

/// Single in-flight pagination lane, owned by the socket worker.
#[derive(Default)]
pub(super) struct GroupHistorySync {
    cursor: Option<String>,
    in_flight: Option<Instant>,
    remaining: usize,
    pages: usize,
    messages: usize,
    exhausted: bool,
    status_text: String,
}

impl GroupHistorySync {
    /// Start at most twenty pages without duplicating an in-flight request.
    pub fn request_batch(&mut self, now: Instant) -> Option<GroupHistoryRequest> {
        if self.in_flight.is_some() || self.exhausted {
            return None;
        }
        self.remaining = BATCH_PAGES;
        self.in_flight = Some(now);
        self.status_text = format!("Đang tải lịch sử nhóm · {} trang đã nhận", self.pages);
        Some(GroupHistoryRequest {
            last_id: self.cursor.clone(),
        })
    }

    /// Advance only an owned request; late pages never advance an idle cursor.
    pub fn accept_page(&mut self, page: &GroupHistoryPage, now: Instant) -> GroupHistoryUpdate {
        // Late/duplicate frames may still be merged but must not advance an idle cursor.
        if self.in_flight.take().is_none() {
            return GroupHistoryUpdate {
                next_request: None,
                status: self.status(),
            };
        }
        self.pages += 1;
        self.messages += page.messages.len();
        self.remaining = self.remaining.saturating_sub(1);
        let mut next_request = None;
        if page.messages.is_empty() {
            self.exhausted = true;
            self.status_text = format!(
                "Đã tải lịch sử nhóm hiện có trên máy chủ · {} tin qua {} trang",
                self.messages, self.pages
            );
        } else if page.next_cursor.is_none() || page.next_cursor == self.cursor {
            self.remaining = 0;
            self.status_text =
                "Lịch sử nhóm tạm dừng: máy chủ thiếu hoặc lặp mốc trang; bấm tải thêm để thử lại"
                    .into();
        } else {
            self.cursor = page.next_cursor.clone();
            if self.remaining > 0 {
                self.in_flight = Some(now);
                next_request = Some(GroupHistoryRequest {
                    last_id: self.cursor.clone(),
                });
                self.status_text = format!(
                    "Đang tải lịch sử nhóm · {} tin qua {} trang",
                    self.messages, self.pages
                );
            } else {
                self.status_text = format!(
                    "Đã nhận {} tin lịch sử nhóm · bấm tải thêm để tiếp tục",
                    self.messages
                );
            }
        }
        GroupHistoryUpdate {
            next_request,
            status: self.status(),
        }
    }

    /// End a silent request after the bounded response deadline.
    pub fn check_timeout(&mut self, now: Instant) -> Option<String> {
        if self
            .in_flight
            .is_some_and(|started| now.saturating_duration_since(started) >= RESPONSE_TIMEOUT)
        {
            self.fail();
            self.status_text = "Lịch sử nhóm chưa có phản hồi; bấm tải thêm để thử lại".into();
            return Some(self.status());
        }
        None
    }

    /// Release the in-flight lane while retaining the last verified cursor.
    pub fn fail(&mut self) {
        self.in_flight = None;
        self.remaining = 0;
        self.status_text = "Tải lịch sử nhóm gặp lỗi; bấm tải thêm để thử lại".into();
    }

    /// Return the latest user-facing state without performing I/O.
    pub fn status(&self) -> String {
        if self.status_text.is_empty() {
            "Lịch sử nhóm: chờ kết nối".into()
        } else {
            self.status_text.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: &str) -> GroupHistoryPage {
        GroupHistoryPage::parse(&json!({"data":{"groupMsgs":[{
            "actionId":id,"msgId":format!("m{id}"),"uidFrom":"member-a","idTo":"group-a","content":"fixture","ts":1000
        }]}})).unwrap()
    }

    #[test]
    fn bounded_batch_continues_from_cursor_and_deduplicates_in_flight_requests() {
        let now = Instant::now();
        let mut sync = GroupHistorySync::default();
        assert_eq!(sync.request_batch(now).unwrap().last_id, None);
        assert!(sync.request_batch(now).is_none());
        for n in 1..=BATCH_PAGES {
            let update = sync.accept_page(&page(&n.to_string()), now);
            assert_eq!(update.next_request.is_some(), n < BATCH_PAGES);
        }
        assert!(!sync.exhausted);
        assert!(sync.status().contains("tiếp tục"));
        assert_eq!(
            sync.request_batch(now).unwrap().last_id.as_deref(),
            Some("20")
        );
    }

    #[test]
    fn empty_page_is_exhausted_but_timeout_and_cursor_stall_are_not() {
        let now = Instant::now();
        let mut sync = GroupHistorySync::default();
        sync.request_batch(now);
        sync.accept_page(&page("100"), now);
        assert!(sync.check_timeout(now + RESPONSE_TIMEOUT).is_some());
        assert!(!sync.exhausted);
        assert_eq!(
            sync.request_batch(now).unwrap().last_id.as_deref(),
            Some("100")
        );
        assert!(sync.accept_page(&page("100"), now).next_request.is_none());
        assert!(!sync.exhausted);
        sync.request_batch(now);
        let empty = GroupHistoryPage::parse(&json!({"data":{"groupMsgs":[]}})).unwrap();
        assert!(sync.accept_page(&empty, now).next_request.is_none());
        assert!(sync.exhausted);
        assert!(sync.request_batch(now).is_none());
    }

    #[test]
    fn malformed_and_encrypted_errors_are_not_empty_history() {
        for value in [
            json!({"data":{}}),
            json!({"data":"encrypted-blob"}),
            json!({"error_code":600,"data":{"groupMsgs":[]}}),
            json!({"data":{"error_code":600,"data":{"groupMsgs":[]}}}),
            json!({"error_code":"600","data":{"groupMsgs":[]}}),
            json!({"data":{"error_code":"600","groupMsgs":[]}}),
            json!({"data":{"data":{"error_code":600,"groupMsgs":[]}}}),
            json!({"data":{"error_code":"invalid","groupMsgs":[]}}),
            json!({"data":{"msgs":[]}}),
            json!({"data":{"groupMsgs":[null]}}),
        ] {
            assert!(GroupHistoryPage::parse(&value).is_err());
        }
    }

    #[test]
    fn cursor_uses_wire_order_and_action_id_before_message_id() {
        let p = GroupHistoryPage::parse(&json!({"data":{"groupMsgs":[
            {"msgId":"200","actionId":"900","ts":20},
            {"msgId":"100","actionId":800,"ts":10}
        ]}}))
        .unwrap();
        assert_eq!(p.next_cursor.as_deref(), Some("800"));
        let p = GroupHistoryPage::parse(&json!({"data":"{\"groupMsgs\":[{\"msgId\":\"100\"}]}"}))
            .unwrap();
        assert_eq!(p.next_cursor.as_deref(), Some("100"));
    }

    #[test]
    fn normalizer_retains_group_destinations_and_normalizes_own_sender_only() {
        let p = GroupHistoryPage::parse(&json!({"data":{"groupMsgs":[
            {"msgId":"1","uidFrom":"member-a","idTo":"group-a","content":"incoming"},
            {"msgId":"2","uidFrom":"self-user","idTo":"group-b","content":"outgoing"},
            {"msgId":"3","uidFrom":"0","idTo":"group-b","content":"sentinel"},
            {"msgId":"4","uidFrom":"member-b","idTo":"self-user","groupId":"group-c","content":"explicit"}
        ]}})).unwrap();
        let payload = p.normalized_payload("self-user");
        let messages = payload["data"]["groupMsgs"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert!(messages.iter().all(|m| m["isGroup"] == true));
        assert_eq!(messages[0]["idTo"], "group-a");
        assert_eq!(messages[1]["uidFrom"], "0");
        assert_eq!(messages[1]["idTo"], "group-b");
        assert_eq!(messages[2]["uidFrom"], "0");
        assert_eq!(messages[3]["groupId"], "group-c");
        assert_eq!(messages[3]["idTo"], "self-user");
    }

    #[test]
    fn unsolicited_page_does_not_advance_sync_or_claim_completion() {
        let mut sync = GroupHistorySync::default();
        let empty = GroupHistoryPage::parse(&json!({"data":{"groupMsgs":[]}})).unwrap();
        sync.accept_page(&empty, Instant::now());
        assert!(!sync.exhausted);
        assert_eq!(sync.pages, 0);
    }
    #[test]
    fn oversized_page_is_rejected_before_cloning_messages() {
        assert!(GroupHistoryPage::parse(
            &json!({"data":{"groupMsgs":vec![json!({"msgId":"fixture"});10_001]}})
        )
        .is_err());
    }
}
