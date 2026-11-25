//! Zalo client in Rust — complete TUI & GUI.
//!
//! Protocol reimplemented from Zalo PC 26.10.11.

#![allow(dead_code)]
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use session::Session;
pub use zalo_rust::{
    account_manager, agent, api_mcp, auth, audio, call, chat, clock, crypto, data_dir, endpoints, feed,
    gui, history_sync, llm, logger, login, media, media_resources, message_model, param_cipher,
    qr, reactions, scheduler, session, socket, store, tg_bridge, tui, ui, unread, updater, voip_log,
};

pub const POLL_SECS: u64 = 4;

fn run_async<F: std::future::Future<Output = Result<()>>>(fut: F) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    rt.block_on(fut)
}

fn main() -> Result<()> {
    let arguments: Vec<String> = std::env::args().collect();
    if zalo_rust::video::camera::run_helper_if_requested(&arguments)? { return Ok(()); }
    let args: Vec<String> = arguments.into_iter().skip(1).collect();
    if args.iter().any(|argument| argument == "--video-self-test") { println!("{}", zalo_rust::video::self_test()?); return Ok(()); }
    if args.iter().any(|argument| argument == "--video-licenses") { println!("{}", zalo_rust::video::license_notices()); return Ok(()); }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ctdozlite v{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--check-update") {
        return run_async(async {
            match gui::check_github_release_cli().await {
                Ok(info) => {
                    if info.has_update {
                        println!(
                            "🚀 Có bản cập nhật mới v{} trên GitHub (hiện tại: v{})",
                            info.latest_version, info.current_version
                        );
                        println!("   Tiêu đề: {}", info.release_title);
                        println!("   Trang phát hành: {}", info.html_url);
                        if let Some(ref dl) = info.download_url {
                            println!("   Tải xuống: {dl}");
                        }
                        if !info.release_notes.trim().is_empty() {
                            println!("\n📋 Nội dung cập nhật & sửa lỗi (GitHub Releases API):");
                            for line in info.release_notes.lines() {
                                let trimmed = line.trim();
                                if trimmed.starts_with("- ")
                                    || trimmed.starts_with("* ")
                                    || trimmed.starts_with("### ")
                                {
                                    println!("   {trimmed}");
                                }
                            }
                        }
                        updater::notify_os_new_update(
                            &info.latest_version,
                            &info.current_version,
                            &info.release_title,
                        );
                    } else {
                        println!(
                            "✅ Bạn đang sử dụng phiên bản mới nhất (v{})",
                            info.current_version
                        );
                    }
                    Ok(())
                }
                Err(e) => {
                    eprintln!("⚠️ Không kiểm tra được bản cập nhật từ GitHub API: {e}");
                    std::process::exit(1);
                }
            }
        });
    }
    let data_path = data_dir::initialize(&args)?;
    if args.iter().any(|a| a == "--print-data-dir") {
        println!("{}", data_path.display());
        return Ok(());
    }
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("[{ts}] PANIC: {info}\n{bt}\n");
        eprint!("{msg}");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("crash.log")
        {
            let _ = f.write_all(msg.as_bytes());
        }
    }));
    let dev_mode = args
        .iter()
        .any(|a| a == "--dev" || a == "-v" || a == "--verbose");
    let dump = args.iter().any(|a| a == "-d" || a == "--dump");
    let dump_raw = args.iter().any(|a| a == "--dump-raw");
    let force_tui = args.iter().any(|a| a == "--tui");
    let force_gui = args.iter().any(|a| a == "--gui");
    let force_qr = args.iter().any(|a| a == "--qr");
    let force_logout = args.iter().any(|a| a == "--logout");
    let force_mcp = args.iter().any(|a| a == "--mcp");
    // --auth status|set <key>|clear : manage the LLM API key (own auth, never opencode's).
    if let Some(i) = args.iter().position(|a| a == "--auth") {
        let sub = args.get(i + 1).cloned().unwrap_or_else(|| "status".into());
        match sub.as_str() {
            "status" => {
                // settings key for source display: read raw file (no GUI loaded here)
                let set_key = std::fs::read_to_string("app_settings.json")
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v.get("llm_api_key").and_then(|k| k.as_str()).map(str::to_string))
                    .unwrap_or_default();
                let (key, src) = auth::resolve_key(&set_key);
                println!("LLM key: {} ({})", auth::mask(&key), auth::describe_source(src));
                return Ok(());
            }
            "set" => {
                let key = args.get(i + 2).cloned().unwrap_or_default();
                if key.trim().is_empty() {
                    anyhow::bail!("--auth set <key>: thiếu key");
                }
                auth::keychain_set(key.trim())?;
                println!("Đã lưu key vào OS keychain (ctdozlite/llm-api-key).");
                return Ok(());
            }
            "clear" => {
                auth::keychain_clear()?;
                println!("Đã xóa key khỏi OS keychain.");
                return Ok(());
            }
            _ => anyhow::bail!("--auth status|set <key>|clear"),
        }
    }
    let offline_mode = args.iter().any(|a| a == "--offline");
    let thread_arg: Option<String> = args
        .iter()
        .position(|a| a == "--thread")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let agent_mode = args.iter().any(|a| a == "--agent");
    let agent_send = args.iter().any(|a| a == "--agent-send");
    let agent_poll: u64 = args
        .iter()
        .position(|a| a == "--agent-poll")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);
    let tg_bridge = args.iter().any(|a| a == "--tg-bridge");
    let tg_map: String = args
        .iter()
        .position(|a| a == "--tg-map")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let tg_owner: i64 = args
        .iter()
        .position(|a| a == "--tg-owner")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    let tg_quota_gb: u64 = args
        .iter()
        .position(|a| a == "--bridge-quota-gb")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(10);
    let tg_dry = args.iter().any(|a| a == "--dry-run");
    let tg_media = args.iter().any(|a| a == "--media");
    let api_port: Option<u16> = args
        .iter()
        .position(|a| a == "--api-port")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u16>().ok())
        .or_else(|| {
            std::env::var("CTDOZLITE_API_PORT")
                .ok()
                .and_then(|v| v.parse::<u16>().ok())
        });
    let cli_proxy: Option<String> = args
        .iter()
        .position(|a| a == "--proxy")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| {
            args.iter()
                .find_map(|a| a.strip_prefix("--proxy=").map(str::to_string))
        });

    if force_logout {
        let uin = std::fs::read("session.json")
            .ok()
            .and_then(|bytes| serde_json::from_slice::<session::SavedSession>(&bytes).ok())
            .and_then(|saved| saved.login.map(|login| login.uin))
            .unwrap_or_default();
        account_manager::logout_account(&uin)?;
        println!("✓ Đã đăng xuất và xoá các phiên đăng nhập đã lưu của tài khoản.");
        if !force_gui && !force_tui && !force_qr {
            return Ok(());
        }
    }

    logger::init(dev_mode);

    // MCP stdio mode: stdout is JSON-RPC only — human logs go to stderr.
    macro_rules! user_msg {
        ($($arg:tt)*) => {
            if force_mcp {
                eprintln!($($arg)*);
            } else {
                println!($($arg)*);
            }
        };
    }

    if dev_mode {
        user_msg!(
            "🔧 [DEV MODE ACTIVATED] – Quét log, chẩn đoán giao thức và trace chi tiết được bật."
        );
    } else {
        user_msg!("🔒 [PRODUCTION MODE] – Toàn bộ nhật ký và dev inspector được ẩn.");
    }

    let mut s = Session::new(dev_mode, dump, dump_raw)?;
    if let Some(ref p) = cli_proxy {
        if let Some(norm) = s.set_proxy(Some(p))? {
            user_msg!(
                "🌐 Đã kích hoạt Proxy cho phiên này: {}",
                session::redact_proxy_url(&norm)
            );
        }
    }

    // Attempt to restore persistent session from disk
    let session_restored = if !force_logout {
        if let Ok(_) = s.load_from_file("session.json") {
            if let Some(li) = &s.login {
                let name = if li.display_name.is_empty() {
                    "Zalo User"
                } else {
                    &li.display_name
                };
                user_msg!(
                    "✓ Đã khôi phục phiên từ session.json: {name} (UIN: {})",
                    li.uin
                );
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    // `--agent --thread <id>`: per-thread autonomous loop (dry-run unless --agent-send).
    if agent_mode {
        let Some(tid) = thread_arg.clone() else {
            anyhow::bail!("--agent yêu cầu --thread <id>");
        };
        let db = store::db_path_for_account(s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""));
        return run_async(agent::run_agent_loop(s, db, tid, agent_poll, agent_send));
    }

    // `--tg-bridge --tg-map "<tg_chat>[/topic]:<zalo_tid>[,...]" [--tg-owner ID] [--dry-run]`.
    if tg_bridge {
        let map = tg_bridge::parse_topic_map(&tg_map);
        if map.is_empty() {
            anyhow::bail!("--tg-bridge yêu cầu --tg-map \"<tg_chat>[/topic]:<zalo_tid>[,...]\"");
        }
        let db = store::db_path_for_account(s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or(""));
        let data_dir = data_dir::initialize(&args)?;
        return run_async(tg_bridge::run_bridge_loop_full(
            s,
            db,
            data_dir,
            tg_quota_gb * 1024 * 1024 * 1024,
            map,
            tg_owner,
            tg_dry,
            tg_media,
        ));
    }
    // If `--mcp` is requested, run the Model Context Protocol JSON-RPC 2.0 server over stdio (#35).
    // `--thread <id>` locks thread-scoped tools to one thread (per-thread agent session).
    let cli_uin = s
        .login
        .as_ref()
        .map(|l| l.uin.as_str())
        .unwrap_or("")
        .to_string();
    if force_mcp {
        let cli_db = authenticated_db_path(&s)?;
        account_manager::set_active_uin(&cli_uin);
        let shared = std::sync::Arc::new(tokio::sync::Mutex::new(s));
        return run_async(api_mcp::run_mcp_stdio_server_bound(shared, &cli_db, thread_arg));
    }

    let launch_gui = force_gui
        || (!force_tui && !force_qr && !args.iter().any(|a| a.starts_with("--phone")));

    // If `--api-port <PORT>` is set without `--gui`/`--tui`, run headless REST + MCP server if `--api-only` is passed;
    // otherwise spawn the REST server inside the worker (GUI) or TUI async runtime (#35).
    if let Some(port) = api_port {
        account_manager::set_active_uin(&cli_uin);
        let api_only = args.iter().any(|a| a == "--api-only");
        if let Some(db_path) = api_startup_path(&s, api_only)? {
            if api_only {
                let shared = std::sync::Arc::new(tokio::sync::Mutex::new(s));
                println!("🌐 REST API & MCP HTTP server đang lắng nghe tại http://127.0.0.1:{port}/api/v1/status");
                return run_async(api_mcp::run_local_http_server(shared, port, db_path));
            }
            if launch_gui {
                std::env::set_var("CTDOZLITE_API_PORT", port.to_string());
            }
        }
    }

    // Determine launch mode:
    // If `--gui` is specified (or default when not `--tui` / `--qr` / `--phone`):
    // Open the native GUI window IMMEDIATELY without blocking on network requests!
    // Background worker in `gui.rs` handles `get_server_info`, `get_friends`, and `poll_new_messages`.
    if launch_gui {
        println!("🚀 Đang khởi chạy giao diện đồ họa ctdozlite (egui)…");
        #[cfg(target_os = "windows")]
        if dev_mode {
            #[link(name = "kernel32")]
            extern "system" {
                fn AttachConsole(dw_process_id: u32) -> i32;
            }
            unsafe {
                let _ = AttachConsole(0xFFFF_FFFF); // ATTACH_PARENT_PROCESS
            }
        }
        let mut viewport = eframe::egui::ViewportBuilder::default()
            .with_app_id("ctdozlite")
            .with_decorations(false)
            .with_inner_size([1020.0, 700.0])
            .with_min_inner_size([480.0, 360.0])
            .with_title("ctdozlite");
        if let Some(icon) = gui::load_app_icon() {
            viewport = viewport.with_icon(std::sync::Arc::new(icon));
        }
        let native_options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };

        let startup_uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or("");
        account_manager::set_active_uin(startup_uin);
        if !startup_uin.is_empty() {
            let _ = account_manager::save_current_account_snapshot(startup_uin);
        }
        let initial_threads = startup_account_threads(&s, store::MessageStore::PRELOAD_MSGS);

        let app_session = s;
        let res = eframe::run_native(
            "ctdozlite",
            native_options,
            Box::new(move |cc| {
                Ok(Box::new(gui::ZaloGuiApp::new(
                    cc,
                    app_session,
                    dev_mode,
                    initial_threads,
                )))
            }),
        )
        .map_err(|e| anyhow::anyhow!("Không thể khởi chạy GUI: {e}"));
        if let Err(ref e) = res {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("crash.log")
            {
                let _ = writeln!(f, "[{ts}] EFRAME_EXIT_ERR: {e}");
            }
        }
        if res.is_ok() {
            std::process::exit(0);
        }
        return res;
    }

    run_async(async move {
    if let Some(port) = api_port {
        if let Some(api_db) = api_startup_path(&s, false)? {
        let shared = std::sync::Arc::new(tokio::sync::Mutex::new(s.clone()));
        println!("🌐 REST API & MCP HTTP server đang lắng nghe tại http://127.0.0.1:{port}/api/v1/status");
        tokio::spawn(async move {
            let _ = api_mcp::run_local_http_server(shared, port, api_db).await;
        });
        }
    }
    // 1. Initial server info check for TUI mode (skipped offline).
    if !offline_mode {
        if let Ok(resp) = login::get_server_info(&s).await {
        if let Some(d) = resp.get("data") {
            s.apply_server_info(d);
        }
        if dev_mode {
            println!(
                "✓ getServerInfo: chat {} • convers {}",
                s.domains.chat, s.domains.conversation
            );
        }
        }
    }

    // Otherwise, run in TUI mode:
    println!("🖥️ Khởi chạy chế độ dòng lệnh TUI…");

    // Validate the stored session, but never destroy it for anything but a server
    // verdict: only `600` / `-102` mean the credentials are gone (#21).
    let mut session_restored = session_restored;
    let mut server_ok = false;
    if session_restored && s.is_expired_marked() {
        // A previous run already read `600` / `-102`; the file still holds the old
        // credentials until a new login replaces them.
        println!(
            "⚠️ Phiên trong session.json đã bị máy chủ từ chối ở lần chạy trước. Cần đăng nhập lại."
        );
        session_restored = false;
    } else if session_restored && !offline_mode {
        match s.session_status().await {
            session::SessionStatus::Valid => server_ok = true,
            session::SessionStatus::Expired => {
                println!(
                    "⚠️ Phiên đăng nhập đã hết hạn (máy chủ từ chối zpw_sek). Cần đăng nhập lại."
                );
                // Keep every credential and only record the verdict; the file is not
                // replaced until a fresh login succeeds.
                s.mark_expired();
                if let Err(e) = s.save_to_file("session.json") {
                    eprintln!("! Không ghi được dấu hết hạn vào session.json: {e}");
                }
                session_restored = false;
            }
            session::SessionStatus::Unreachable => {
                // Offline: keep the session and start with cached threads and local
                // history. `r` retries inside the TUI; no QR, no logout.
                println!(
                    "⚠️ Không kết nối được máy chủ Zalo — giữ nguyên phiên, chạy tiếp với dữ liệu cục bộ. Bấm 'r' để thử lại."
                );
            }
        }
    }

    // Login via QR or credentials if not already restored
    let mut logged_in_now = false;
    if offline_mode && !session_restored {
        println!("📴 Chế độ offline: chạy với dữ liệu cục bộ, không kết nối máy chủ.");
    } else if force_qr || !session_restored {
        if force_qr {
            println!("… Đang tạo mã QR");
            let _ = qr::login_via_qr(&mut s, "vi").await?;
            logged_in_now = true;
        } else {
            let arg_val = |name: &str| -> Option<String> {
                args.iter()
                    .position(|a| a == name)
                    .and_then(|i| args.get(i + 1))
                    .cloned()
            };

            let phone = match arg_val("--phone") {
                Some(v) => v,
                None => ui::prompt("Số điện thoại: ")?,
            };
            let password = match arg_val("--pass") {
                Some(v) => v,
                None => ui::prompt_password("Mật khẩu: ")?,
            };
            let iso = match arg_val("--iso") {
                Some(v) => v,
                None => ui::prompt_default("Mã quốc gia", "VN")?,
            };

            if !phone.trim().is_empty() && !password.is_empty() {
                println!("… Đang đăng nhập mật khẩu");
                let v1 = args.iter().any(|a| a == "--v1");
                let _ = if v1 {
                    login::login_by_password_v1(&mut s, phone.trim(), &password).await?
                } else {
                    login::login_by_password(&mut s, phone.trim(), &password, iso.trim()).await?
                };
                logged_in_now = true;
            }
        }
    }

    if logged_in_now {
        // First moment the previous `session.json` may be replaced (#21). `qr::login_via_qr`
        // already saved once; the password path never persisted before this.
        if let Err(e) = s.save_to_file("session.json") {
            eprintln!("! Không lưu được session.json: {e}");
        }
        server_ok = true;
    }

    // Fetch initial threads and hydrate with local SQLite history
    let mut initial_threads = startup_account_threads(&s, 200);
    if s.login.is_some() && server_ok && !offline_mode {
        match chat::get_friends(&s).await {
            Ok(list) => {
                for ft in list {
                    if let Some(ex) = initial_threads.iter_mut().find(|t| t.id == ft.id) {
                        if !ft.name.is_empty() {
                            ex.name = ft.name;
                        }
                        ex.is_group = ex.is_group || ft.is_group;
                    } else {
                        initial_threads.push(ft);
                    }
                }
                let mut poll_state = chat::PollState::default();
                if let Err(e) =
                    chat::poll_new_messages(&s, &mut poll_state, &mut initial_threads).await
                {
                    crate::logger::log_error("main::poll_new_messages", &e.to_string());
                    eprintln!("! Lỗi tải tin nhắn gần nhất: {e}");
                }
            }
            Err(e) => {
                crate::logger::log_error("main::get_friends", &e.to_string());
                eprintln!("! Lỗi tải danh sách bạn bè: {e}");
            }
        }
    }

    let mut tui_app = tui::TuiApp::new(&mut s, initial_threads);
    if let Some(ref want) = thread_arg {
        if tui_app.select_thread_by_id(want) {
            println!("✓ Đã mở hội thoại: {want}");
        } else {
            eprintln!("! Không tìm thấy hội thoại: {want}");
        }
    }
    tui::run_tui(tui_app).await?;

    Ok(())
    })
}

/// Hydrate only the captured authenticated account before either UI first paints.
fn startup_account_threads(session: &Session, limit: usize) -> Vec<chat::Thread> {
    let Some(login) = session.login.as_ref() else {
        return Vec::new();
    };
    let mut threads = chat::load_threads_cache_for_account(&login.uin);
    match store::MessageStore::open_account(&login.uin) {
        Ok(store) => {
            if let Err(error) = store.hydrate_threads(&mut threads, limit) {
                logger::log_error("main::hydrate_account_history", &error.to_string());
            }
        }
        Err(error) => logger::log_error("main::open_account_history", &error.to_string()),
    }
    threads
}

/// Open the authenticated account's history store before exposing its path to the local API.
///
/// # Errors
/// Returns an error if login is required or the store cannot be opened.
fn authenticated_db_path(session: &Session) -> Result<String> {
    let login = session
        .login
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Vui lòng đăng nhập trước khi mở API"))?;
    store::MessageStore::open_account(&login.uin)?;
    Ok(store::db_path_for_account(&login.uin))
}

/// Resolve the API database, allowing an interactive client to start with its API disabled.
///
/// # Errors
/// Propagates account resolution failures when API-only mode is requested.
fn api_startup_path(session: &Session, api_only: bool) -> Result<Option<String>> {
    match authenticated_db_path(session) {
        Ok(path) => Ok(Some(path)),
        Err(error) if !api_only => {
            eprintln!("Không thể mở API nền: {error}");
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_background_api_does_not_prevent_login_ui_startup() {
        let session = Session::new(false, false, false).unwrap();
        assert_eq!(api_startup_path(&session, false).unwrap(), None);
        assert!(api_startup_path(&session, true).is_err());
    }
}
