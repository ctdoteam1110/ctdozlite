//! Sync V2 probe: gửi 590 rồi nghe phone phản hồi, KHÔNG cần user bấm gì.
//!
//! Run: `cargo run --release --bin sync_probe` (chạy trong data dir có session.json)
//! Kết quả cho biết: phone có nhận 590 không (status 3?), có tự bypass không (status 5?),
//! có gửi batch không — mà không cần ai chạm vào điện thoại.

use anyhow::{bail, Result};
use std::time::{Duration, Instant};
use zalo_rust::session::Session;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== SYNC V2 PROBE (không cần chạm điện thoại) ===");

    let mut s = Session::new(false, false, false)?;
    if let Err(e) = s.load_from_file("session.json") {
        bail!("Không đọc được session.json ({e})");
    }
    let li = s.login.clone().unwrap();
    println!("✓ Session: {} ({})", li.display_name, li.uin);

    // Nối WS để nhận events.
    let mut rx = zalo_rust::socket::spawn(s.clone());
    // Chờ WS kết nối.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Lấy 3 thread 1-1 đầu tiên → znoise → noised ids → partition thật.
    let threads = zalo_rust::chat::load_threads_cache();
    let ones: Vec<&zalo_rust::chat::Thread> =
        threads.iter().filter(|t| !t.is_group).take(3).collect();
    println!("✓ Threads 1-1: {}", ones.len());
    for t in &ones {
        println!("    - {} ({})", t.name, t.id);
    }
    let fids: Vec<i64> = ones.iter().filter_map(|t| t.id.parse().ok()).collect();
    let noise_body = serde_json::json!({ "fids": fids, "gids": [] });
    let enc = s.encoded_params(&noise_body)?;
    let url = format!("https://zwid.api.zalo.me/api/znoise?{}", s.common_params());
    let resp = s.post_form(&url, format!("params={enc}")).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    println!("✓ znoise: {}", dec.to_string().chars().take(300).collect::<String>());
    let noised: Vec<String> = dec
        .get("data").and_then(|d| d.get("fids")).and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    println!("✓ noised ids: {noised:?}");

    // Mở session V2 + gửi 590 với partition thật.
    let sync_id = zalo_rust::chat::wake_phone_sync_v2(&s).await?;
    println!("✓ syncId: {sync_id}");
    let sent = zalo_rust::chat::send_sync_v2_request_with_queries(&s, &sync_id, &noised)?;
    println!("✓ 590 đã gửi: {sent}");

    // Nghe 90s: chỉ tính events của syncId HIỆN TẠI (bỏ batch cũ replay).
    println!("\n--- Nghe phản hồi 90s cho sync {sync_id} (không cần bấm gì) ---");
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut saw_status = false;
    let mut saw_batch = false;
    let mut saw_any_sync = 0;
    while Instant::now() < deadline {
        let timeout = tokio::time::sleep(Duration::from_millis(500));
        tokio::pin!(timeout);
        tokio::select! {
            _ = &mut timeout => {}
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                let txt = ev.payload.to_string();
                // Lọc sync-related.
                let low = txt.to_lowercase();
                let is_mine = txt.contains(&sync_id);
                if low.contains("sync") || low.contains("transfer") || low.contains("user_confirm")
                    || low.contains("msgurl") || low.contains("batch") || low.contains("mobile")
                    || ev.cmd == 590 || ev.cmd == 591 || ev.cmd == 592
                {
                    saw_any_sync += 1;
                    let tag = if is_mine { "MỚI✓" } else { "cũ" };
                    println!("[{:>3}s][{}] cmd={} sub={} :: {}",
                        deadline.duration_since(Instant::now()).as_secs(), tag,
                        ev.cmd, ev.sub_cmd,
                        txt.chars().take(if is_mine { 3000 } else { 200 }).collect::<String>());
                    if is_mine {
                        if low.contains("status") || low.contains("user_confirm") || low.contains("waiting") || low.contains("confirm") { saw_status = true; }
                        if low.contains("msgurl") {
                            saw_batch = true;
                            // Parse batch events của mình → import ngay (cùng keys trong RAM).
                            let data_node = ev.payload.get("data").unwrap_or(&ev.payload);
                            for batch in zalo_rust::chat::extract_mobile_sync_controls(data_node) {
                                if let zalo_rust::chat::MobileSyncControlEvent::SyncV2Batch { url, sync_id: sid, batch_type, is_last } = batch {
                                    if sid != sync_id { continue; }
                                    if batch_type == 2 {
                                        println!("  → IMPORT META {url}...");
                                        match zalo_rust::history_sync::import_sync_v2_meta(&s, &url).await {
                                            Ok(meta) => {
                                                println!("  ✓ META OK session={}", meta.session_id);
                                                zalo_rust::history_sync::record_sync_v2_session(&sid, &meta.session_id);
                                            }
                                            Err(e) => println!("  ✗ META FAIL: {e}"),
                                        }
                                    } else if batch_type == 1 {
                                        println!("  → IMPORT MSG is_last={is_last} {url}...");
                                        match zalo_rust::history_sync::import_sync_v2_messages(&s, &url, is_last).await {
                                            Ok(n) => println!("  ✓ MSG OK: {n} tin"),
                                            Err(e) => println!("  ✗ MSG FAIL: {e}"),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let _ = saw_batch;
    }

    // Dọn session.
    zalo_rust::socket::send_sync_ack_delete(&sync_id, "PROBE_DONE");
    println!("\n=== KẾT QUẢ ===");
    println!("  Tổng events sync (kể cả cũ): {saw_any_sync}");
    println!("  Phone phản hồi sync MỚI (status/confirm): {}", if saw_status { "CÓ ✓" } else { "KHÔNG ✗" });
    println!("  Phone gửi batch cho sync MỚI: {}", if saw_batch { "CÓ ✓" } else { "KHÔNG ✗" });
    if !saw_status && !saw_batch {
        println!("  → Phone không phản ứng gì với 590. Payload còn sai chỗ nào đó.");
    }
    Ok(())
}
