//! Local HTTP REST API, Model Context Protocol (MCP) JSON-RPC 2.0 server, and Auto-Reply rules (#35).
//!
//! Enables third-party tools, automation scripts, and AI agents (Claude Desktop, Cursor, MCP clients)
//! to query threads/messages, search local history, send or schedule messages, configure auto-reply
//! rules, and export/sync conversation data over `127.0.0.1` or `stdio`.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::chat;
use crate::scheduler;
use crate::session::Session;
use crate::store::{self, MessageStore};

pub const AUTOREPLY_FILE: &str = "autoreply_rules.json";
pub const DEFAULT_API_PORT: u16 = 17890;

const API_METHODS: &[&str] = &["GET", "POST", "DELETE"];
const API_HEADERS: &[&str] = &["authorization", "content-type"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoReplyRule {
    pub id: String,
    pub keyword: String,
    /// `"contains"` (default), `"exact"`, or `"prefix"`.
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    pub reply_text: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional filter: `"all"`, `"direct"`, or `"group"`.
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_match_mode() -> String {
    "contains".into()
}

fn default_true() -> bool {
    true
}

fn default_scope() -> String {
    "all".into()
}

pub fn load_autoreply_rules() -> Vec<AutoReplyRule> {
    std::fs::read_to_string(AUTOREPLY_FILE)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<AutoReplyRule>>(&s).ok())
        .unwrap_or_default()
}

pub fn save_autoreply_rules(rules: &[AutoReplyRule]) -> Result<()> {
    let text = serde_json::to_string_pretty(rules)?;
    crate::data_dir::write_private_atomic(std::path::Path::new(AUTOREPLY_FILE), text.as_bytes())?;
    Ok(())
}

/// Match an incoming message against active auto-reply rules.
pub fn match_autoreply_rule(
    rules: &[AutoReplyRule],
    incoming_text: &str,
    is_group: bool,
) -> Option<AutoReplyRule> {
    let msg_lower = incoming_text.trim().to_lowercase();
    if msg_lower.is_empty() {
        return None;
    }
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let kw = rule.keyword.trim().to_lowercase();
        if kw.is_empty() || rule.reply_text.trim().is_empty() {
            continue;
        }
        match rule.scope.as_str() {
            "direct" if is_group => continue,
            "group" if !is_group => continue,
            _ => {}
        }
        let matched = match rule.match_mode.as_str() {
            "exact" => msg_lower == kw,
            "prefix" => msg_lower.starts_with(&kw),
            _ => msg_lower.contains(&kw),
        };
        if matched {
            return Some(rule.clone());
        }
    }
    None
}

/// Handle a single MCP JSON-RPC 2.0 request object and return the JSON-RPC response (`None` for notifications).
pub async fn handle_mcp_request(
    session: &Arc<Mutex<Session>>,
    req: &Value,
    db_path: &str,
) -> Option<Value> {
    handle_mcp_request_bound(session, req, db_path, &None).await
}

/// Bound variant: `bound = Some(thread_id)` locks thread-scoped tools to one thread.
pub async fn handle_mcp_request_bound(
    session: &Arc<Mutex<Session>>,
    req: &Value,
    db_path: &str,
    bound: &BoundThread,
) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

    // JSON-RPC notifications have no `id`
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let req_id = id.unwrap_or(Value::Null);

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "ctdozlite-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "listChanged": false }
                }
            }
        })),
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {}
        })),
        "resources/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "resources": [
                    {
                        "uri": "zalo://threads",
                        "name": "Danh sách hội thoại Zalo",
                        "mimeType": "application/json",
                        "description": "Danh sách tất cả cuộc trò chuyện cá nhân, nhóm và Cloud của tôi"
                    },
                    {
                        "uri": "zalo://scheduled",
                        "name": "Danh sách tin nhắn hẹn giờ",
                        "mimeType": "application/json",
                        "description": "Các tin nhắn đang chờ phát theo lịch hẹn"
                    },
                    {
                        "uri": "zalo://autoreply",
                        "name": "Quy tắc trả lời tự động",
                        "mimeType": "application/json",
                        "description": "Cấu hình từ khóa tự động trả lời tin nhắn"
                    }
                ]
            }
        })),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            let content = match uri {
                "zalo://threads" => {
                    let mut threads = chat::load_threads_cache();
                    store::load_history_into_threads(db_path, &mut threads, 20);
                    serde_json::to_string_pretty(&threads).unwrap_or_else(|_| "[]".into())
                }
                "zalo://scheduled" => {
                    let list = scheduler::list_pending_scheduled(db_path, None).unwrap_or_default();
                    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into())
                }
                "zalo://autoreply" => {
                    let rules = load_autoreply_rules();
                    serde_json::to_string_pretty(&rules).unwrap_or_else(|_| "[]".into())
                }
                _ => json!({ "error": format!("Unknown resource URI: {uri}") }).to_string(),
            };
            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "contents": [{
                        "uri": uri,
                        "mimeType": "application/json",
                        "text": content
                    }]
                }
            }))
        }
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "tools": [
                    {
                        "name": "zalo_status",
                        "description": "Kiểm tra trạng thái phiên đăng nhập ctdozlite và thống kê dữ liệu cục bộ",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    },
                    {
                        "name": "zalo_list_threads",
                        "description": "Liệt kê danh sách hội thoại (cá nhân, nhóm, Cloud của tôi)",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "limit": { "type": "integer", "description": "Số hội thoại tối đa (mặc định 50)" }
                            }
                        }
                    },
                    {
                        "name": "zalo_get_messages",
                        "description": "Đọc lịch sử tin nhắn của một hội thoại từ cơ sở dữ liệu cục bộ",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "thread_id": { "type": "string", "description": "ID hội thoại hoặc nhóm" },
                                "limit": { "type": "integer", "description": "Số tin nhắn gần nhất (mặc định 50)" }
                            },
                            "required": ["thread_id"]
                        }
                    },
                    {
                        "name": "zalo_search_messages",
                        "description": "Tìm kiếm tin nhắn theo từ khóa trên toàn bộ lịch sử lưu trữ",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "query": { "type": "string", "description": "Từ khóa cần tìm" },
                                "limit": { "type": "integer", "description": "Số kết quả tối đa (mặc định 50)" }
                            },
                            "required": ["query"]
                        }
                    },
                    {
                        "name": "zalo_send_message",
                        "description": "Gửi tin nhắn văn bản tới người dùng hoặc nhóm Zalo",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "to_id": { "type": "string", "description": "ID người nhận hoặc nhóm" },
                                "text": { "type": "string", "description": "Nội dung tin nhắn" },
                                "is_group": { "type": "boolean", "description": "true nếu gửi vào nhóm" }
                            },
                            "required": ["to_id", "text"]
                        }
                    },
                    {
                        "name": "zalo_schedule_message",
                        "description": "Hẹn giờ gửi tin nhắn vào một thời điểm trong tương lai",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "to_id": { "type": "string", "description": "ID người nhận hoặc nhóm" },
                                "thread_name": { "type": "string", "description": "Tên hiển thị của hội thoại" },
                                "text": { "type": "string", "description": "Nội dung tin nhắn" },
                                "when": { "type": "string", "description": "Thời점 phát tin: +15m, +2h, HH:MM, hoặc YYYY-MM-DD HH:MM" },
                                "is_group": { "type": "boolean", "description": "true nếu gửi vào nhóm" }
                            },
                            "required": ["to_id", "text", "when"]
                        }
                    },
                    {
                        "name": "zalo_privacy",
                        "description": "Xem hoặc bật/tắt quyền LLM đọc ảnh/file của 1 thread (mặc định TẮT)",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "action": { "type": "string", "description": "'status', 'on', hoặc 'off'" },
                                "thread_id": { "type": "string", "description": "ID hội thoại" }
                            },
                            "required": ["action", "thread_id"]
                        }
                    },
                    {
                        "name": "zalo_configure_autoreply",
                        "description": "Xem hoặc thêm quy tắc tự động trả lời tin nhắn theo từ khóa",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "action": { "type": "string", "description": "'list', 'add', hoặc 'clear'" },
                                "keyword": { "type": "string", "description": "Từ khóa kích hoạt (khi action='add')" },
                                "reply_text": { "type": "string", "description": "Nội dung tự động trả lời (khi action='add')" },
                                "match_mode": { "type": "string", "description": "'contains', 'exact', hoặc 'prefix'" }
                            },
                            "required": ["action"]
                        }
                    }
                ]
            }
        })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let (is_err, payload) = execute_tool_call(session, name, &args, db_path, bound).await;
            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "isError": is_err,
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
                    }]
                }
            }))
        }
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {method}")
            }
        })),
    }
}

/// Bound thread scope for a per-thread agent session (`--mcp --thread <id>`).
/// `None` = global mode (legacy behaviour, all threads allowed).
pub type BoundThread = Option<String>;

/// Reject a tool call that targets a thread outside the bound scope.
fn check_thread_bound(bound: &BoundThread, tool: &str, got: Option<&str>) -> Option<(bool, Value)> {
    match (bound, got) {
        (Some(want), Some(got)) if got != want => Some((
            true,
            json!({ "error": format!("wrong_thread: tool '{tool}' locked to thread '{want}'") }),
        )),
        (Some(want), None) => Some((
            true,
            json!({ "error": format!("wrong_thread: tool '{tool}' requires thread '{want}'") }),
        )),
        _ => None,
    }
}

async fn execute_tool_call(
    session: &Arc<Mutex<Session>>,
    name: &str,
    args: &Value,
    db_path: &str,
    bound: &BoundThread,
) -> (bool, Value) {
    match name {
        "zalo_status" => {
            let s = session.lock().await;
            let logged_in = s.login.is_some() && s.zpw_sek.is_some();
            let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or("");
            let display_name = s
                .login
                .as_ref()
                .map(|l| l.display_name.as_str())
                .unwrap_or("");
            let thread_count = MessageStore::open(db_path)
                .and_then(|st| st.all_threads())
                .map(|v| v.len())
                .unwrap_or(0);
            let scheduled_count = scheduler::list_pending_scheduled(db_path, None)
                .map(|v| v.len())
                .unwrap_or(0);
            (
                false,
                json!({
                    "ok": true,
                    "app": "ctdozlite",
                    "version": env!("CARGO_PKG_VERSION"),
                    "logged_in": logged_in,
                    "uin": uin,
                    "display_name": display_name,
                    "threads_in_db": thread_count,
                    "pending_scheduled": scheduled_count,
                }),
            )
        }
        "zalo_list_threads" => {
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(500) as usize;
            let mut threads = chat::load_threads_cache();
            store::load_history_into_threads(db_path, &mut threads, 5);
            let items: Vec<Value> = threads
                .into_iter()
                .take(limit)
                .map(|t| {
                    json!({
                        "id": t.id,
                        "name": t.name,
                        "is_group": t.is_group,
                        "preview": t.preview,
                        "time": t.time,
                        "message_count": t.messages.len(),
                    })
                })
                .collect();
            (false, json!({ "threads": items }))
        }
        "zalo_get_messages" => {
            if let Some(reject) = check_thread_bound(bound, "zalo_get_messages", args.get("thread_id").and_then(Value::as_str)) {
                return reject;
            }
            let Some(tid) = args.get("thread_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: thread_id" }));
            };
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(500) as usize;
            match MessageStore::open(db_path).and_then(|st| st.messages_for(tid, limit)) {
                Ok(msgs) => (false, json!({ "thread_id": tid, "messages": msgs })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_search_messages" => {
            if bound.is_some() {
                return (
                    true,
                    json!({ "error": "wrong_thread: global search disabled in bound mode (run unbound --mcp)" }),
                );
            }
            let Some(q) = args.get("query").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: query" }));
            };
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(200) as usize;
            match MessageStore::open(db_path).and_then(|st| st.search_messages(q, limit)) {
                Ok(hits) => (false, json!({ "query": q, "results": hits })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_send_message" => {
            if let Some(reject) = check_thread_bound(bound, "zalo_send_message", args.get("to_id").and_then(Value::as_str)) {
                return reject;
            }
            let Some(to_id) = args.get("to_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: to_id" }));
            };
            let Some(text) = args.get("text").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: text" }));
            };
            let is_group = args.get("is_group").and_then(Value::as_bool).unwrap_or(false);
            let s = session.lock().await.clone();
            let res = if is_group {
                chat::send_group_message(&s, to_id, text).await
            } else {
                chat::send_message(&s, to_id, text).await
            };
            match res {
                Ok(resp) => (false, json!({ "ok": true, "to_id": to_id, "response": resp })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_schedule_message" => {
            if let Some(reject) = check_thread_bound(bound, "zalo_schedule_message", args.get("to_id").and_then(Value::as_str)) {
                return reject;
            }
            let Some(to_id) = args.get("to_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: to_id" }));
            };
            let Some(text) = args.get("text").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: text" }));
            };
            let Some(when) = args.get("when").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: when" }));
            };
            let thread_name = args
                .get("thread_name")
                .and_then(Value::as_str)
                .unwrap_or(to_id);
            let is_group = args.get("is_group").and_then(Value::as_bool).unwrap_or(false);
            let now_ts = crate::login::unix_now();
            let deliver_at = match when.parse::<i64>() {
                Ok(ts) if ts > now_ts => ts,
                _ => match scheduler::parse_custom_schedule_input(when, now_ts) {
                    Ok(ts) => ts,
                    Err(e) => return (true, json!({ "error": e })),
                },
            };
            match scheduler::schedule_message(
                db_path,
                to_id,
                thread_name,
                is_group,
                text,
                deliver_at,
                now_ts,
            ) {
                Ok(item) => (false, json!({ "ok": true, "scheduled": item })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_privacy" => {
            let Some(tid) = args.get("thread_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: thread_id" }));
            };
            if let Some(reject) = check_thread_bound(bound, "zalo_privacy", Some(tid)) {
                return reject;
            }
            let action = args.get("action").and_then(Value::as_str).unwrap_or("status");
            match crate::store::MessageStore::open(db_path) {
                Ok(mut st) => match action {
                    "on" | "off" => {
                        let val = if action == "on" { "true" } else { "false" };
                        match st.thread_kv_set(tid, crate::thread_script::PRIVACY_KEY, val) {
                            Ok(()) => {
                                let _ = st.push_script_log(tid, "privacy", "info", &format!("MCP set llm_read_attachments={val}"));
                                (false, json!({ "thread_id": tid, "llm_read_attachments": action == "on" }))
                            }
                            Err(e) => (true, json!({ "error": e.to_string() })),
                        }
                    }
                    _ => {
                        let on = crate::thread_script::privacy_allowed(&st, tid);
                        (false, json!({ "thread_id": tid, "llm_read_attachments": on }))
                    }
                },
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_configure_autoreply" => {
            // Bound sessions must not see or touch global autoreply rules:
            // rules match across ALL threads by keyword, so even `list`
            // leaks another thread's automation surface.
            if bound.is_some() {
                return (
                    true,
                    json!({ "error": "wrong_thread: autoreply is global-only (run unbound --mcp)" }),
                );
            }
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("list");
            let mut rules = load_autoreply_rules();
            match action {
                "list" => (false, json!({ "rules": rules })),
                "clear" => {
                    rules.clear();
                    let _ = save_autoreply_rules(&rules);
                    (false, json!({ "ok": true, "rules": rules }))
                }
                "add" => {
                    let kw = args
                        .get("keyword")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    let reply = args
                        .get("reply_text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if kw.is_empty() || reply.is_empty() {
                        return (
                            true,
                            json!({ "error": "Cần cung cấp cả keyword và reply_text" }),
                        );
                    }
                    let mode = args
                        .get("match_mode")
                        .and_then(Value::as_str)
                        .unwrap_or("contains")
                        .to_string();
                    let rule = AutoReplyRule {
                        id: format!("rule_{}", rules.len() + 1),
                        keyword: kw.to_string(),
                        match_mode: mode,
                        reply_text: reply.to_string(),
                        enabled: true,
                        scope: "all".into(),
                    };
                    rules.push(rule);
                    if let Err(e) = save_autoreply_rules(&rules) {
                        return (true, json!({ "error": e.to_string() }));
                    }
                    (false, json!({ "ok": true, "rules": rules }))
                }
                _ => (true, json!({ "error": format!("Unsupported action: {action}") })),
            }
        }
        _ => (true, json!({ "error": format!("Unknown tool: {name}") })),
    }
}

/// Run the MCP JSON-RPC 2.0 server over `stdin`/`stdout` (`ctdozlite --mcp`).
pub async fn run_mcp_stdio_server(session: Arc<Mutex<Session>>, db_path: &str) -> Result<()> {
    run_mcp_stdio_server_bound(session, db_path, None).await
}

/// Bound variant: `--mcp --thread <id>` locks thread-scoped tools to one thread.
pub async fn run_mcp_stdio_server_bound(
    session: Arc<Mutex<Session>>,
    db_path: &str,
    bound: BoundThread,
) -> Result<()> {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    if let Some(ref tid) = bound {
        eprintln!("🔒 MCP bound to thread: {tid}");
    }

    for line_res in stdin.lock().lines() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(trimmed) {
            Ok(req) => handle_mcp_request_bound(&session, &req, db_path, &bound).await,
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32700,
                    "message": format!("Parse error: {e}")
                }
            })),
        };
        if let Some(r) = resp {
            let mut out = serde_json::to_string(&r)?;
            out.push('\n');
            stdout.write_all(out.as_bytes())?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// Handle a REST API request and return `(status_code, json_body)`.
pub async fn handle_rest_request(
    session: &Arc<Mutex<Session>>,
    method: &str,
    path_and_query: &str,
    body: &str,
    db_path: &str,
) -> (u16, Value) {
    let (path, query_str) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    let query_params: HashMap<String, String> = query_str
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((url_decode(k), url_decode(v)))
        })
        .collect();

    match (method, path) {
        ("GET", "/api/v1/status") | ("GET", "/health") => {
            let (_, val) = execute_tool_call(session, "zalo_status", &json!({}), db_path, &None).await;
            (200, val)
        }
        ("GET", "/api/v1/threads") => {
            let limit = query_params
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(100);
            let (_, val) = execute_tool_call(
                session,
                "zalo_list_threads",
                &json!({ "limit": limit }),
                db_path,
                &None,
            )
            .await;
            (200, val)
        }
        ("GET", p) if p.starts_with("/api/v1/threads/") && p.ends_with("/messages") => {
            let tid = p
                .trim_start_matches("/api/v1/threads/")
                .trim_end_matches("/messages")
                .trim_matches('/');
            let limit = query_params
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(100);
            let (err, val) = execute_tool_call(
                session,
                "zalo_get_messages",
                &json!({ "thread_id": tid, "limit": limit }),
                db_path,
                &None,
            )
            .await;
            (if err { 400 } else { 200 }, val)
        }
        ("GET", "/api/v1/search") => {
            let q = query_params.get("q").cloned().unwrap_or_default();
            let limit = query_params
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(50);
            let (err, val) = execute_tool_call(
                session,
                "zalo_search_messages",
                &json!({ "query": q, "limit": limit }),
                db_path,
                &None,
            )
            .await;
            (if err { 400 } else { 200 }, val)
        }
        ("POST", "/api/v1/messages/send") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            let (err, val) =
                execute_tool_call(session, "zalo_send_message", &payload, db_path, &None).await;
            (if err { 400 } else { 200 }, val)
        }
        ("POST", "/api/v1/messages/schedule") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            let mut normalized = payload.clone();
            if normalized.get("when").is_none() {
                if let Some(ts) = normalized.get("deliver_at") {
                    normalized["when"] = json!(ts.to_string());
                }
            }
            let (err, val) =
                execute_tool_call(session, "zalo_schedule_message", &normalized, db_path, &None).await;
            (if err { 400 } else { 200 }, val)
        }
        ("GET", "/api/v1/scheduled") => {
            let tid = query_params.get("thread_id").map(String::as_str);
            match scheduler::list_pending_scheduled(db_path, tid) {
                Ok(list) => (200, json!({ "ok": true, "scheduled": list })),
                Err(e) => (500, json!({ "ok": false, "error": e.to_string() })),
            }
        }
        ("DELETE", p) if p.starts_with("/api/v1/scheduled/") => {
            let id_str = p.trim_start_matches("/api/v1/scheduled/").trim();
            let Ok(id) = id_str.parse::<i64>() else {
                return (400, json!({ "ok": false, "error": "Invalid scheduled id" }));
            };
            match scheduler::cancel_scheduled_message(db_path, id) {
                Ok(cancelled) => (200, json!({ "ok": true, "cancelled": cancelled })),
                Err(e) => (500, json!({ "ok": false, "error": e.to_string() })),
            }
        }
        ("GET", "/api/v1/autoreply") => (200, json!({ "ok": true, "rules": load_autoreply_rules() })),
        ("POST", "/api/v1/autoreply") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            if let Some(arr) = payload.get("rules").and_then(Value::as_array) {
                let rules: Vec<AutoReplyRule> = arr
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect();
                match save_autoreply_rules(&rules) {
                    Ok(()) => (200, json!({ "ok": true, "rules": rules })),
                    Err(e) => (500, json!({ "ok": false, "error": e.to_string() })),
                }
            } else {
                let (err, val) =
                    execute_tool_call(session, "zalo_configure_autoreply", &payload, db_path, &None).await;
                (if err { 400 } else { 200 }, val)
            }
        }
        ("GET", "/api/v1/export") => {
            let mut threads = chat::load_threads_cache();
            store::load_history_into_threads(db_path, &mut threads, 1000);
            let scheduled = scheduler::list_pending_scheduled(db_path, None).unwrap_or_default();
            let autoreply = load_autoreply_rules();
            (
                200,
                json!({
                    "ok": true,
                    "exported_at": crate::login::unix_now(),
                    "version": env!("CARGO_PKG_VERSION"),
                    "threads": threads,
                    "scheduled_messages": scheduled,
                    "autoreply_rules": autoreply,
                }),
            )
        }
        ("POST", "/api/v1/mcp") => match serde_json::from_str::<Value>(body) {
            Ok(req) => match handle_mcp_request(session, &req, db_path).await {
                Some(resp) => (200, resp),
                None => (202, json!({ "ok": true })),
            },
            Err(e) => (
                400,
                json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Invalid JSON: {e}") }
                }),
            ),
        },
        _ => (
            404,
            json!({
                "ok": false,
                "error": format!("Endpoint not found: {method} {path}"),
                "available_endpoints": [
                    "GET /api/v1/status",
                    "GET /api/v1/threads",
                    "GET /api/v1/threads/{id}/messages",
                    "GET /api/v1/search?q=<keyword>",
                    "POST /api/v1/messages/send",
                    "POST /api/v1/messages/schedule",
                    "GET /api/v1/scheduled",
                    "DELETE /api/v1/scheduled/{id}",
                    "GET /api/v1/autoreply",
                    "POST /api/v1/autoreply",
                    "GET /api/v1/export",
                    "POST /api/v1/mcp"
                ]
            }),
        ),
    }
}

#[derive(Clone)]
struct ApiHttpConfig {
    port: u16,
    token: String,
    allowed_origins: Vec<String>,
}

impl ApiHttpConfig {
    fn from_env(port: u16) -> Result<Self> {
        let token = std::env::var("CTDOZLITE_API_TOKEN").map_err(|_| {
            anyhow::anyhow!("CTDOZLITE_API_TOKEN is required to start the REST API")
        })?;
        let origins = match std::env::var("CTDOZLITE_API_ALLOWED_ORIGINS") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => String::new(),
            Err(std::env::VarError::NotUnicode(_)) => {
                anyhow::bail!("CTDOZLITE_API_ALLOWED_ORIGINS must be valid UTF-8")
            }
        };
        Self::new(port, &token, &origins)
    }

    fn new(port: u16, token: &str, origins: &str) -> Result<Self> {
        if token.is_empty() || !token.bytes().all(|byte| (0x21..=0x7e).contains(&byte)) {
            anyhow::bail!("CTDOZLITE_API_TOKEN must be a non-empty printable ASCII token")
        }

        let mut allowed_origins = Vec::new();
        for origin in origins
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let normalized = normalize_origin(origin).ok_or_else(|| {
                anyhow::anyhow!("CTDOZLITE_API_ALLOWED_ORIGINS contains an invalid origin")
            })?;
            if !allowed_origins.contains(&normalized) {
                allowed_origins.push(normalized);
            }
        }

        Ok(Self {
            port,
            token: token.to_owned(),
            allowed_origins,
        })
    }

    fn allows_host(&self, host: &str) -> bool {
        let port = self.port.to_string();
        host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}"))
            || host.eq_ignore_ascii_case(&format!("localhost:{port}"))
            || (self.port == 80
                && (host.eq_ignore_ascii_case("127.0.0.1")
                    || host.eq_ignore_ascii_case("localhost")))
    }

    fn allowed_origin(&self, origin: &str) -> Option<String> {
        let normalized = normalize_origin(origin)?;
        self.allowed_origins
            .contains(&normalized)
            .then_some(normalized)
    }
}

fn normalize_origin(value: &str) -> Option<String> {
    if value.contains('@') || value.contains('\\') {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    (origin != "null").then_some(origin)
}

enum HeaderValue<'a> {
    Missing,
    Single(&'a str),
    Duplicate,
}

fn single_header<'a>(head: &'a str, name: &str) -> HeaderValue<'a> {
    let mut values = head.lines().skip(1).filter_map(|line| {
        let (header, value) = line.split_once(':')?;
        header.eq_ignore_ascii_case(name).then_some(value.trim())
    });
    let value = values.next();
    match (value, values.next()) {
        (None, _) => HeaderValue::Missing,
        (Some(_), Some(_)) => HeaderValue::Duplicate,
        (Some(value), None) => HeaderValue::Single(value),
    }
}

fn has_valid_bearer_token(head: &str, expected: &str) -> bool {
    let HeaderValue::Single(value) = single_header(head, "authorization") else {
        return false;
    };
    let Some((scheme, token)) = value.split_once(' ') else {
        return false;
    };
    let token = token.trim();
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return false;
    }
    bool::from(token.as_bytes().ct_eq(expected.as_bytes()))
}

fn valid_preflight(head: &str) -> bool {
    let HeaderValue::Single(method) = single_header(head, "access-control-request-method") else {
        return false;
    };
    if !API_METHODS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(method))
    {
        return false;
    }

    let headers = match single_header(head, "access-control-request-headers") {
        HeaderValue::Single(value) => value,
        HeaderValue::Missing => "",
        HeaderValue::Duplicate => return false,
    };
    headers.is_empty()
        || headers.split(',').all(|header| {
            let header = header.trim();
            !header.is_empty()
                && API_HEADERS
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(header))
        })
}

async fn write_http_response(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
    origin: Option<&str>,
    preflight: bool,
) {
    let status_text = match status {
        204 => "No Content",
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n",
        body.len()
    );
    if !body.is_empty() {
        response.push_str("Content-Type: application/json; charset=utf-8\r\n");
    }
    if let Some(origin) = origin {
        response.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n"
        ));
    }
    if preflight {
        response.push_str(
            "Access-Control-Allow-Methods: GET, POST, DELETE\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\n",
        );
    }
    response.push_str("\r\n");
    response.push_str(body);
    let _ = stream.write_all(response.as_bytes()).await;
}

async fn write_http_error(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    message: &str,
    origin: Option<&str>,
) {
    let body = json!({ "ok": false, "error": message }).to_string();
    write_http_response(stream, status, &body, origin, false).await;
}

async fn handle_http_connection(
    mut stream: tokio::net::TcpStream,
    session: Arc<Mutex<Session>>,
    db_path: String,
    config: ApiHttpConfig,
) {
    let mut buf = vec![0u8; 65536];
    let Ok(n) = stream.read(&mut buf).await else {
        return;
    };
    if n == 0 {
        return;
    }
    let raw = String::from_utf8_lossy(&buf[..n]);
    let (head, body_part) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
    let mut lines = head.lines();
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");

    let HeaderValue::Single(host) = single_header(head, "host") else {
        write_http_error(&mut stream, 403, "Forbidden", None).await;
        return;
    };
    if !config.allows_host(host) {
        write_http_error(&mut stream, 403, "Forbidden", None).await;
        return;
    }

    let origin = match single_header(head, "origin") {
        HeaderValue::Single(value) => match config.allowed_origin(value) {
            Some(origin) => Some(origin),
            None => {
                write_http_error(&mut stream, 403, "Forbidden", None).await;
                return;
            }
        },
        HeaderValue::Missing => None,
        HeaderValue::Duplicate => {
            write_http_error(&mut stream, 403, "Forbidden", None).await;
            return;
        }
    };

    if method == "OPTIONS" {
        if origin.is_none() || !valid_preflight(head) {
            write_http_error(&mut stream, 403, "Forbidden", None).await;
            return;
        }
        write_http_response(&mut stream, 204, "", origin.as_deref(), true).await;
        return;
    }

    if !has_valid_bearer_token(head, &config.token) {
        write_http_error(&mut stream, 401, "Unauthorized", origin.as_deref()).await;
        return;
    }

    let (status, json_val) = handle_rest_request(&session, method, path, body_part, &db_path).await;
    let body = serde_json::to_string(&json_val).unwrap_or_else(|_| "{}".into());
    write_http_response(&mut stream, status, &body, origin.as_deref(), false).await;
}

async fn serve_local_http(
    listener: TcpListener,
    session: Arc<Mutex<Session>>,
    db_path: String,
    config: ApiHttpConfig,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_http_connection(
            stream,
            session.clone(),
            db_path.clone(),
            config.clone(),
        ));
    }
}

/// Spawn the local HTTP REST & MCP server on `127.0.0.1:<port>`.
///
/// # Errors
/// Returns an error if the required API token or origin configuration is invalid, the local port
/// cannot be bound, or accepting a connection fails.
pub async fn run_local_http_server(
    session: Arc<Mutex<Session>>,
    port: u16,
    db_path: String,
) -> Result<()> {
    let config = ApiHttpConfig::from_env(port)?;
    let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
    serve_local_http(listener, session, db_path, config).await
}

fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'+' {
            bytes.push(b' ');
            i += 1;
        } else if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(hex) = std::str::from_utf8(&raw[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    bytes.push(val);
                    i += 3;
                    continue;
                }
            }
            bytes.push(raw[i]);
            i += 1;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    async fn start_test_server(
        origins: &str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = ApiHttpConfig::new(address.port(), "test-api-token", origins).unwrap();
        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));
        let task = tokio::spawn(async move {
            let _ = serve_local_http(listener, session, ":memory:".into(), config).await;
        });
        (address, task)
    }

    async fn send_http_request(
        address: std::net::SocketAddr,
        method: &str,
        host: &str,
        headers: &str,
    ) -> String {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!(
            "{method} /api/v1/search?q=test HTTP/1.1\r\nHost: {host}\r\n{headers}Connection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        String::from_utf8(response).unwrap()
    }

    fn local_host(address: std::net::SocketAddr) -> String {
        format!("127.0.0.1:{}", address.port())
    }

    #[test]
    fn api_http_config_requires_token_and_valid_origins() {
        assert!(ApiHttpConfig::new(17890, "", "").is_err());
        assert!(ApiHttpConfig::new(17890, "test-token", "*").is_err());
        assert!(ApiHttpConfig::new(17890, "test-token", "https://trusted.example/path").is_err());
    }

    #[tokio::test]
    async fn http_api_rejects_missing_and_invalid_tokens() {
        let (address, task) = start_test_server("").await;
        let host = local_host(address);

        let missing = send_http_request(address, "GET", &host, "").await;
        assert!(missing.starts_with("HTTP/1.1 401 Unauthorized"));
        assert!(missing.contains("\r\nCache-Control: no-store\r\n"));

        let invalid = send_http_request(
            address,
            "GET",
            &host,
            "Authorization: Bearer wrong-token\r\n",
        )
        .await;
        assert!(invalid.starts_with("HTTP/1.1 401 Unauthorized"));
        assert!(invalid.contains("Unauthorized"));
        assert!(!invalid.contains("wrong-token"));

        task.abort();
    }

    #[tokio::test]
    async fn http_api_echoes_only_configured_origins() {
        let (address, task) = start_test_server("http://localhost:3000").await;
        let host = local_host(address);

        let trusted = send_http_request(
            address,
            "GET",
            &host,
            "Authorization: Bearer test-api-token\r\nOrigin: http://localhost:3000\r\n",
        )
        .await;
        assert!(trusted.starts_with("HTTP/1.1 200 OK"));
        assert!(trusted.contains("\r\nAccess-Control-Allow-Origin: http://localhost:3000\r\n"));
        assert!(!trusted.contains("Access-Control-Allow-Origin: *"));

        let untrusted = send_http_request(
            address,
            "GET",
            &host,
            "Authorization: Bearer test-api-token\r\nOrigin: https://attacker.example\r\n",
        )
        .await;
        assert!(untrusted.starts_with("HTTP/1.1 403 Forbidden"));
        assert!(!untrusted.contains("Access-Control-Allow-Origin:"));

        task.abort();
    }

    #[tokio::test]
    async fn http_api_allows_authenticated_non_browser_clients() {
        let (address, task) = start_test_server("").await;
        let response = send_http_request(
            address,
            "GET",
            &local_host(address),
            "Authorization: Bearer test-api-token\r\n",
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(!response.contains("Access-Control-Allow-Origin:"));

        task.abort();
    }

    #[tokio::test]
    async fn preflight_does_not_authorize_the_follow_up_request() {
        let (address, task) = start_test_server("http://localhost:3000").await;
        let host = local_host(address);
        let preflight = send_http_request(
            address,
            "OPTIONS",
            &host,
            "Origin: http://localhost:3000\r\nAccess-Control-Request-Method: POST\r\nAccess-Control-Request-Headers: authorization, content-type\r\n",
        )
        .await;
        assert!(preflight.starts_with("HTTP/1.1 204 No Content"));
        assert!(preflight.contains("\r\nAccess-Control-Allow-Origin: http://localhost:3000\r\n"));
        assert!(preflight.contains("\r\nAccess-Control-Allow-Methods: GET, POST, DELETE\r\n"));
        assert!(!preflight.contains("Access-Control-Allow-Origin: *"));

        let follow_up =
            send_http_request(address, "POST", &host, "Origin: http://localhost:3000\r\n").await;
        assert!(follow_up.starts_with("HTTP/1.1 401 Unauthorized"));

        task.abort();
    }

    #[tokio::test]
    async fn http_api_rejects_untrusted_host_headers() {
        let (address, task) = start_test_server("").await;
        let response = send_http_request(
            address,
            "GET",
            &format!("attacker.example:{}", address.port()),
            "Authorization: Bearer test-api-token\r\n",
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 403 Forbidden"));
        task.abort();
    }

    #[test]
    fn autoreply_rules_match_contains_exact_prefix_and_scope() {
        let rules = vec![
            AutoReplyRule {
                id: "r1".into(),
                keyword: "báo giá".into(),
                match_mode: "contains".into(),
                reply_text: "Cảm ơn bạn, mình sẽ gửi báo giá ngay.".into(),
                enabled: true,
                scope: "direct".into(),
            },
            AutoReplyRule {
                id: "r2".into(),
                keyword: "/ping".into(),
                match_mode: "prefix".into(),
                reply_text: "Pong từ ctdozlite!".into(),
                enabled: true,
                scope: "all".into(),
            },
        ];

        assert!(match_autoreply_rule(&rules, "Cho mình xin BÁO GIÁ nhé", false).is_some());
        // Scope is `direct`, so group messages do not trigger r1
        assert!(match_autoreply_rule(&rules, "Cho mình xin báo giá nhé", true).is_none());
        // r2 has scope `all` and prefix `/ping`
        let hit = match_autoreply_rule(&rules, "/ping server", true).unwrap();
        assert_eq!(hit.id, "r2");
    }

    #[tokio::test]
    async fn bound_thread_gate_rejects_foreign_thread() {
        // check_thread_bound: matching / missing / foreign
        assert!(check_thread_bound(&Some("t1".into()), "zalo_get_messages", Some("t1")).is_none());
        assert!(check_thread_bound(&None, "zalo_get_messages", Some("tX")).is_none());
        let rej = check_thread_bound(&Some("t1".into()), "zalo_send_message", Some("t2"))
            .expect("foreign thread must be rejected");
        assert!(rej.0);
        assert!(rej.1["error"].as_str().unwrap().contains("wrong_thread"));
        // search/autoreply-mutations rejected wholesale in bound mode even without a tid
        let rej2 = check_thread_bound(&Some("t1".into()), "zalo_search_messages", None)
            .expect("missing tid must be rejected");
        assert!(rej2.1["error"].as_str().unwrap().contains("wrong_thread"));
    }

    #[tokio::test]
    async fn mcp_and_rest_handlers_expose_tools_search_and_schedule() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_file = std::env::temp_dir().join(format!("ctdozlite-mcp-test-{nonce}.db"));
        let db_path = db_file.to_str().unwrap();

        {
            let mut store = MessageStore::open(db_path).unwrap();
            store
                .upsert_message(&store::StoredMessage {
                    msg_id: "m_mcp_1".into(),
                    thread_id: "t_mcp".into(),
                    sender_id: "u1".into(),
                    sender_name: "Alice".into(),
                    text: "Tích hợp MCP và REST API thành công".into(),
                    msg_type: "chat.text".into(),
                    ts: 123456,
                    is_outgoing: false,
                    raw_json: String::new(),
                })
                .unwrap();
        }

        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));

        // 1. MCP initialize
        let init_resp = handle_mcp_request(
            &session,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }),
            db_path,
        )
        .await
        .unwrap();
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "ctdozlite-mcp");

        // 2. MCP tools/list
        let tools_resp = handle_mcp_request(
            &session,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            db_path,
        )
        .await
        .unwrap();
        let tools = tools_resp["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "zalo_search_messages"));
        assert!(tools.iter().any(|t| t["name"] == "zalo_schedule_message"));

        // 3. REST search
        let (status, search_val) = handle_rest_request(
            &session,
            "GET",
            "/api/v1/search?q=MCP",
            "",
            db_path,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(search_val["results"].as_array().unwrap().len(), 1);

        // 4. REST schedule message
        let (sched_status, sched_val) = handle_rest_request(
            &session,
            "POST",
            "/api/v1/messages/schedule",
            &json!({
                "to_id": "t_mcp",
                "thread_name": "Alice",
                "text": "Tin nhắn hẹn giờ qua REST API",
                "when": "+30m"
            })
            .to_string(),
            db_path,
        )
        .await;
        assert_eq!(sched_status, 200);
        assert_eq!(sched_val["ok"], true);

        let _ = std::fs::remove_file(&db_file);
    }
}
