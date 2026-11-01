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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::chat;
use crate::scheduler;
use crate::session::Session;
use crate::store::{self, MessageStore};

// Only the embedded GUI listener installs this immutable, acceptance-time owner.
#[derive(Clone)]
struct AccountHistoryContext {
    root: std::path::PathBuf,
    account: String,
}
tokio::task_local! {
    static ACCOUNT_HISTORY: AccountHistoryContext;
}

async fn history_read<T: Send + 'static>(
    db_path: &str,
    operation: impl FnOnce(Option<crate::store_v2::AccountStore>, &str) -> Result<T> + Send + 'static,
) -> Result<T> {
    let context = ACCOUNT_HISTORY.try_with(Clone::clone).ok();
    let db_path = db_path.to_owned();
    crate::runtime_scope::spawn_blocking(move || {
        let _guard = crate::runtime_scope::mutation()?;
        let account_store = context.map(|context| {
            crate::store_v2::AccountStore::open(&context.root, &context.account)
        }).transpose()?;
        operation(account_store, &db_path)
    }).await??
}

fn account_messages(
    store: &crate::store_v2::AccountStore, thread: &str, limit: usize,
) -> Result<Vec<store::StoredMessage>> {
    use crate::store_v2::{HistoryCursor, PageDirection};
    let mut messages = Vec::new();
    let mut cursor = None;
    // At most three 200-row queries preserve the existing API cap of 500.
    for _ in 0..3 {
        if messages.len() >= limit.min(500) { break; }
        let page = store.history_page(thread, cursor.as_ref(), PageDirection::Before)?;
        let Some(first) = page.messages.first() else { break; };
        cursor = Some(HistoryCursor { ts: first.ts, msg_id: first.msg_id.clone() });
        let take = (limit.min(500) - messages.len()).min(page.messages.len());
        let start = page.messages.len() - take;
        messages.splice(0..0, page.messages.into_iter().skip(start));
        if !page.has_more { break; }
    }
    Ok(messages)
}

async fn history_threads(db_path: &str, limit: usize) -> Result<Vec<chat::Thread>> {
    history_read(db_path, move |account, db| {
        if let Some(store) = account {
            let mut threads = store.all_thread_metadata()?;
            for thread in &mut threads {
                thread.messages = account_messages(&store, &thread.id, limit)?
                    .iter().map(|row| store.chat_message(row)).collect::<Result<_>>()?;
                if let Some(last) = thread.messages.last() {
                    thread.preview = last.text.replace('\n', " ");
                    thread.time = last.time.clone();
                }
            }
            Ok(threads)
        } else {
            let mut threads = chat::load_threads_cache();
            store::load_history_into_threads(db, &mut threads, limit);
            Ok(threads)
        }
    }).await
}

pub const AUTOREPLY_FILE: &str = "autoreply_rules.json";
pub const DEFAULT_API_PORT: u16 = 17890;

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
    let _scope_guard = crate::runtime_scope::mutation()?;
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
                    match history_threads(db_path, 20).await {
                        Ok(threads) => serde_json::to_string_pretty(&threads).unwrap_or_else(|_| "[]".into()),
                        Err(_) => return Some(json!({ "jsonrpc": "2.0", "id": req_id,
                            "error": { "code": -32603, "message": "Local history unavailable" } })),
                    }
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
            let (is_err, payload) = execute_tool_call(session, name, &args, db_path).await;
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

async fn execute_tool_call(
    session: &Arc<Mutex<Session>>,
    name: &str,
    args: &Value,
    db_path: &str,
) -> (bool, Value) {
    match name {
        "zalo_status" => {
            let s = session.lock().await.clone();
            let logged_in = s.login.is_some() && s.zpw_sek.is_some();
            let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or("");
            let display_name = s
                .login
                .as_ref()
                .map(|l| l.display_name.as_str())
                .unwrap_or("");
            let thread_count = history_read(db_path, |account, db| {
                match account {
                    Some(store) => Ok(store.all_threads()?.len()),
                    None => Ok(MessageStore::open(db)?.all_threads()?.len()),
                }
            }).await.unwrap_or(0);
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
            let threads = match history_threads(db_path, 5).await {
                Ok(threads) => threads,
                Err(_) => return (true, json!({ "error": "Local history unavailable" })),
            };
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
            let Some(tid) = args.get("thread_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: thread_id" }));
            };
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(500) as usize;
            let tid_owned = tid.to_owned();
            match history_read(db_path, move |account, db| match account {
                Some(store) => account_messages(&store, &tid_owned, limit),
                None => MessageStore::open(db)?.messages_for(&tid_owned, limit),
            }).await {
                Ok(msgs) => (false, json!({ "thread_id": tid, "messages": msgs })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_search_messages" => {
            let Some(q) = args.get("query").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: query" }));
            };
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(200) as usize;
            let query = q.to_owned();
            match history_read(db_path, move |account, db| match account {
                Some(store) => Ok(store.search(&query, None, 0, limit)?.into_iter().map(|hit| hit.message).collect()),
                None => MessageStore::open(db)?.search_messages(&query, limit),
            }).await {
                Ok(hits) => (false, json!({ "query": q, "results": hits })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_send_message" => {
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
        "zalo_configure_autoreply" => {
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
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line_res in stdin.lock().lines() {
        let line = line_res?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(trimmed) {
            Ok(req) => handle_mcp_request(&session, &req, db_path).await,
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
            let (_, val) = execute_tool_call(session, "zalo_status", &json!({}), db_path).await;
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
            )
            .await;
            (if err { 400 } else { 200 }, val)
        }
        ("POST", "/api/v1/messages/send") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            let (err, val) =
                execute_tool_call(session, "zalo_send_message", &payload, db_path).await;
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
                execute_tool_call(session, "zalo_schedule_message", &normalized, db_path).await;
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
                    execute_tool_call(session, "zalo_configure_autoreply", &payload, db_path).await;
                (if err { 400 } else { 200 }, val)
            }
        }
        ("GET", "/api/v1/export") => {
            let threads = match history_threads(db_path, 1000).await {
                Ok(threads) => threads,
                Err(_) => return (500, json!({ "ok": false, "error": "Local history unavailable" })),
            };
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

/// Spawn the local HTTP REST & MCP server on `127.0.0.1:<port>`.
pub async fn run_local_http_server(
    session: Arc<Mutex<Session>>, port: u16, db_path: String,
) -> Result<()> {
    run_local_http_server_inner(session, port, db_path, false).await
}

/// Serve GUI requests with an acceptance-time session epoch while keeping the listener alive.
///
/// # Errors
/// Returns listener and socket setup errors, matching the standalone server entry point.
pub async fn run_scoped_local_http_server(
    session: Arc<Mutex<Session>>, port: u16, db_path: String,
) -> Result<()> {
    run_local_http_server_inner(session, port, db_path, true).await
}

async fn run_local_http_server_inner(
    session: Arc<Mutex<Session>>, port: u16, db_path: String, scoped: bool,
) -> Result<()> {
    let bind_addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&bind_addr).await?;
    serve_local_http_listener(session, listener, db_path, scoped).await
}

async fn serve_local_http_listener(
    session: Arc<Mutex<Session>>, listener: TcpListener, db_path: String, scoped: bool,
) -> Result<()> {
    let expected_token = std::env::var("CTDOZLITE_API_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty());

    loop {
        let (mut stream, _) = listener.accept().await?;
        let sess = session.clone();
        let db = db_path.clone();
        let tok = expected_token.clone();
        let scope = scoped.then(|| crate::runtime_scope::registry().current());
        let context = scope.as_ref().map(|scope| AccountHistoryContext {
            root: std::path::Path::new(&db_path).parent().filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new(".")).to_owned(),
            account: scope.account.clone(),
        });
        // The account and epoch belong to this accepted connection, never a later login.
        let owner_matches = if let Some(scope) = scope.as_ref() {
            session.lock().await.login.as_ref().is_some_and(|login| login.uin == scope.account)
        } else { true };
        tokio::spawn(async move {
            if !owner_matches || scope.as_ref().is_some_and(|scope| scope.account.is_empty()) {
                let response = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }
            let operation = async {
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

            if method == "OPTIONS" {
                let cors = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(cors.as_bytes()).await;
                return;
            }

            if let Some(ref expected) = tok {
                let authorized = head.lines().any(|line| {
                    let lower = line.to_ascii_lowercase();
                    if lower.starts_with("authorization:") {
                        let val = line[14..].trim().trim_start_matches("Bearer ").trim();
                        val == expected
                    } else {
                        false
                    }
                });
                if !authorized {
                    let body_str = json!({ "ok": false, "error": "Unauthorized: invalid or missing Bearer token" }).to_string();
                    let resp = format!(
                        "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
                        body_str.len(),
                        body_str
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    return;
                }
            }

            let (status, json_val) =
                handle_rest_request(&sess, method, path, body_part, &db).await;
            let status_text = match status {
                200 => "200 OK",
                202 => "202 Accepted",
                400 => "400 Bad Request",
                404 => "404 Not Found",
                _ => "500 Internal Server Error",
            };
            let body_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "{}".into());
            let resp = format!(
                "HTTP/1.1 {status_text}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body_str.len(),
                body_str
            );
            let _ = stream.write_all(resp.as_bytes()).await;
            };
            if let Some(scope) = scope {
                if let Some(context) = context {
                    crate::runtime_scope::run(scope, ACCOUNT_HISTORY.scope(context, operation)).await;
                }
            } else { operation.await; }
        });
    }
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

#[cfg(test)]
mod account_history_consumer_tests {
    use super::*;
    use crate::store::{StoredMessage, StoredThread};
    use crate::store_v2::{AccountStore, StoreDelta};

    async fn request(address: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, Value) {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        let auth = std::env::var("CTDOZLITE_API_TOKEN").ok()
            .map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
        let message = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Length: {}\r\n\r\n{body}", body.len());
        stream.write_all(message.as_bytes()).await.unwrap();
        let mut output = Vec::new();
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read_to_end(&mut output)).await.unwrap();
        if let Err(error) = read {
            // Rejected connections may close with unread request bytes after a complete 503.
            assert!(error.kind() == std::io::ErrorKind::ConnectionReset && !output.is_empty(), "{method} {path}: {error}");
        }
        let text = String::from_utf8(output).unwrap();
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, serde_json::from_str(body).unwrap_or(Value::Null))
    }

    fn seed(root: &std::path::Path, account: &str) {
        let mut store = AccountStore::open(root, account).unwrap();
        let rows: Vec<_> = (0..501).map(|n| StoredMessage {
            msg_id: format!("message-{n:04}"), thread_id: "same-thread".into(),
            sender_id: "same-sender".into(), sender_name: "Fixture".into(),
            text: format!("needle owner-{account}"), msg_type: "chat.text".into(),
            ts: n, is_outgoing: false, raw_json: String::new(),
        }).collect();
        store.apply_delta(StoreDelta {
            threads: &[StoredThread { thread_id: "same-thread".into(), name: format!("owner-{account}"), is_group: false, last_ts: 500 }],
            messages: &rows, unread: &[], deleted_messages: &[],
        }).unwrap();
    }

    #[tokio::test]
    async fn embedded_http_mcp_history_isolates_a_b_a_with_identical_request_metadata() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = std::env::temp_dir().join(format!("account-api-{}-{}", std::process::id(), crate::login::unix_now()));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("messages.db").to_string_lossy().into_owned();
        let mut legacy = MessageStore::open(&db).unwrap();
        legacy.upsert_message(&StoredMessage {
            msg_id: "legacy-only".into(), thread_id: "same-thread".into(), sender_id: "legacy".into(),
            sender_name: String::new(), text: "needle legacy-private".into(), msg_type: "chat.text".into(),
            ts: 600, is_outgoing: false, raw_json: String::new(),
        }).unwrap();
        drop(legacy);
        seed(&root, "a"); seed(&root, "b");
        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve_local_http_listener(session.clone(), listener, db.clone(), true));
        for account in ["a", "b", "a"] {
            session.lock().await.login = Some(crate::session::LoginInfo { uin: account.into(), ..Default::default() });
            crate::runtime_scope::transition(account).unwrap();
            for path in ["/api/v1/threads", "/api/v1/threads/same-thread/messages?limit=500", "/api/v1/search?q=needle", "/api/v1/export"] {
                let (status, value) = request(address, "GET", path, "").await;
                assert_eq!(status, 200, "{path}: {value}");
                let text = value.to_string();
                assert!(text.contains(&format!("owner-{account}")), "{path}");
                assert!(!text.contains("legacy-private"));
                let other = if account == "a" { "b" } else { "a" };
                assert!(!text.contains(&format!("owner-{other}")));
                if path.contains("/messages?") {
                    let messages = value["messages"].as_array().unwrap();
                    assert_eq!(messages.len(), 500);
                    assert_eq!(messages[0]["msg_id"], "message-0001");
                    assert_eq!(messages[499]["msg_id"], "message-0500");
                }
            }
            for payload in [
                json!({"jsonrpc":"2.0","id":17,"method":"tools/call","params":{"name":"zalo_search_messages","arguments":{"query":"needle"}}}),
                json!({"jsonrpc":"2.0","id":17,"method":"resources/read","params":{"uri":"zalo://threads"}}),
            ] {
                let (status, value) = request(address,"POST","/api/v1/mcp",&payload.to_string()).await;
                assert_eq!(status,200);
                assert_eq!(value["id"],17);
                let text=value.to_string();
                assert!(text.contains(&format!("owner-{account}")));
                assert!(!text.contains("legacy-private"));
                assert!(!text.contains(if account=="a" {"owner-b"} else {"owner-a"}));
            }
        }
        // A second completed request proves the earlier idle connection was accepted.
        let mut stale = tokio::net::TcpStream::connect(address).await.unwrap();
        assert_eq!(request(address, "GET", "/api/v1/status", "").await.0, 200);
        for account in ["b", "a"] {
            session.lock().await.login.as_mut().unwrap().uin = account.into();
            crate::runtime_scope::transition(account).unwrap();
        }
        let _ = stale.write_all(b"GET /api/v1/search?q=needle HTTP/1.1\r\nHost: localhost\r\n\r\n").await;
        let mut old_output = Vec::new();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), stale.read_to_end(&mut old_output)).await.unwrap();
        assert!(old_output.is_empty(), "an accepted old request survived A/B/A");
        // A fresh account must not inherit any legacy/global-cache rows.
        session.lock().await.login = Some(crate::session::LoginInfo { uin: "empty".into(), ..Default::default() });
        crate::runtime_scope::transition("empty").unwrap();
        let (_, result) = request(address,"GET","/api/v1/search?q=needle","").await;
        assert!(result["results"].as_array().unwrap().is_empty());
        // An incompatible V2 database is an error, never a legacy fallback.
        let account_path = AccountStore::open(&root, "empty").unwrap().path.unwrap();
        rusqlite::Connection::open(account_path).unwrap()
            .pragma_update(None, "user_version", 999).unwrap();
        let (status, result) = request(address,"GET","/api/v1/search?q=needle","").await;
        assert_eq!(status,400);
        assert!(result.get("error").is_some());
        assert!(!result.to_string().contains("legacy-private"));
        // A session/epoch owner mismatch fails closed before dispatch.
        session.lock().await.login.as_mut().unwrap().uin = "a".into();
        assert_eq!(request(address,"GET","/api/v1/search?q=needle","").await.0,503);
        server.abort(); let _ = server.await;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(serve_local_http_listener(session,listener,db,false));
        let (_, value) = request(address,"GET","/api/v1/search?q=needle","").await;
        assert!(value.to_string().contains("legacy-private"));
        assert!(!value.to_string().contains("owner-a"));
        server.abort(); let _ = server.await;
        crate::runtime_scope::transition("").unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
