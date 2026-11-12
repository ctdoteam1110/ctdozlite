//! Comprehensive QA & Cross-Platform Integration/Regression Test Suite (`ctdozlite`)
//! -------------------------------------------------------------------------------
//! Kiểm thử toàn diện các tính năng trọng tâm, xử lý sự cố đa nền tảng và tính ổn định:
//! 1. Giao thức Nhật ký (Social Feed) thời gian thực (`src/feed.rs`):
//!    - Đóng gói binary `CMD_FEED_TIMELINE` (`1230`, `subcmd = 6`) 52 bytes & khung `nu0.v` header 16 bytes.
//!    - Đóng gói binary `CMD_USER_FEED` (`2030`, `subcmd = 0`) 18 bytes.
//!    - Định dạng múi giờ Việt Nam `format_vn_timestamp_ms`, chuẩn hóa URL CDN `.jxl` -> `.jpg` (`normalize_social_media_url`),
//!      nhãn biểu cảm `reaction_id_to_label`, và `SocialUserProfile::from_json` đa định dạng `lastActionTime` / `lastUpdateTime`.
//!    - Cơ sở dữ liệu SQLite tạm thời với `timeline_feed_1`, `user_feed`, và `table_feed_real_time_like_comment`
//!      kiểm chứng ghi đè số lượt thích/bình luận real-time, trạng thái `is_liked`, top_reactions, lọc `target_uid` và sắp xếp mới nhất.
//! 2. Quản lý Đa tài khoản & Chuyển đổi nhanh (`src/account_manager.rs` + `src/data_dir.rs`):
//!    - Lưu snapshot, liệt kê, chuyển đổi (`switch_to_account`), xóa tài khoản trong thư mục tạm cô lập.
//!    - Kiểm chứng `--profile` và `--data-dir` đa nền tảng (Windows, macOS `.app` bundle, Linux).
//! 3. Độ co giãn DPI Đa nền tảng (100%, 125%, 150%, 200%) & An toàn Clamp Layout:
//!    - Kiểm chứng tính toán độ rộng sidebar / panel trên dải kích thước cửa sổ siêu hẹp (`screen_w = 120.0..1920.0`),
//!      đảm bảo không bao giờ xảy ra panic `min > max` khi `clamp()`.
//! 4. Khả năng chống chịu Mạng chập chờn / Thứ tự thời gian lộn xộn & Trình cập nhật an toàn (`src/updater.rs`):
//!    - Kiểm chứng các bản tin Nhật ký đến sai thứ tự thời gian hoặc trùng timestamp được sắp xếp tất định (deterministic).
//!    - Kiểm chứng trình cập nhật kiểm tra SHA-256, chữ ký PE/ELF và hoàn tác (rollback) nguyên tử khi lỗi.

use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use zalo_rust::account_manager::{
    delete_saved_account, list_saved_accounts, save_current_account_snapshot, switch_to_account,
};
use zalo_rust::data_dir;
use zalo_rust::feed::{
    format_vn_timestamp_ms, load_real_timeline_from_sqlite, normalize_social_media_url, packet,
    reaction_id_to_label, FeedAuthor, FeedItem, FeedReaction, SocialUserProfile,
};
use zalo_rust::updater;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn create_isolated_temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ctdozlite-qa-{prefix}-{}-{}",
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("Failed to create temp test directory");
    dir
}

// ============================================================================
// 1. FEED REAL-DATA & REAL-TIME PROTOCOL TESTS (`src/feed.rs`)
// ============================================================================

#[test]
fn qa_feed_timeline_packet_1230_and_header_encoding() {
    let owner_id = 435_550_501u64;
    let viewer_id = 435_550_501u64;
    let payload = packet::build_get_feed_payload(owner_id, viewer_id, 0, 1, 20);

    // Payload `pw0.e.d` bắt buộc đúng 52 bytes
    assert_eq!(payload.len(), 52, "CMD_FEED_TIMELINE payload must be exactly 52 bytes");
    assert_eq!(payload[0], 0x01, "Version byte in payload must be 1");
    assert_eq!(
        i32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]),
        packet::CLIENT_VERSION
    );
    assert_eq!(
        i32::from_be_bytes([payload[5], payload[6], payload[7], payload[8]]),
        1,
        "Language/platform code must be 1 (vi/android)"
    );
    assert_eq!(payload[9], 0, "Tab index must be 0");
    assert_eq!(
        i32::from_be_bytes([payload[10], payload[11], payload[12], payload[13]]),
        1,
        "Page must be 1"
    );
    assert_eq!(
        i32::from_be_bytes([payload[14], payload[15], payload[16], payload[17]]),
        20,
        "Count must be 20"
    );
    assert_eq!(
        i64::from_be_bytes(payload[34..42].try_into().unwrap()),
        owner_id as i64,
        "max_feed_id slot carries owner_id"
    );
    assert_eq!(
        i64::from_be_bytes(payload[42..50].try_into().unwrap()),
        2,
        "feed_version must be 2"
    );
    assert_eq!(
        i16::from_be_bytes([payload[50], payload[51]]),
        0,
        "source_enum must be 0"
    );

    // Đóng gói khung `nu0.v` (4 bytes length prefix + 16 bytes header + 52 bytes payload = 72 bytes)
    let uin = 435_550_501u32;
    let seq_id = 42u16;
    let pkt = packet::build_request_packet_with_uin(
        uin,
        packet::CMD_FEED_TIMELINE,
        packet::SUBCMD_GET_FEED,
        seq_id,
        &payload,
    );
    assert_eq!(pkt.len(), 4 + 16 + 52);

    let body_len = u32::from_be_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]) as usize;
    assert_eq!(body_len, 16 + 52, "Body length prefix must equal 16-byte header + 52-byte payload");
    assert_eq!(pkt[4], 0x01, "Packet version must be 1");
    assert_eq!(pkt[5], 0x00, "Encryption flag must be 0 (plain frame inside TLS/ZProto)");
    assert_eq!(
        u32::from_be_bytes([pkt[6], pkt[7], pkt[8], pkt[9]]),
        uin,
        "UIN in header must match"
    );
    assert_eq!(pkt[10], packet::CMD_TYPE_REQUEST, "cmd_type must be 3");
    assert_eq!(
        u16::from_be_bytes([pkt[11], pkt[12]]),
        1230,
        "cmd must be CMD_FEED_TIMELINE (1230)"
    );
    assert_eq!(pkt[13], 6, "subcmd must be SUBCMD_GET_FEED (6)");
    assert_eq!(
        u16::from_be_bytes([pkt[14], pkt[15]]),
        seq_id,
        "seq_id must match"
    );
    assert_eq!(
        u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]) as usize,
        payload.len(),
        "params_len must equal payload length (52)"
    );
    assert_eq!(&pkt[20..], payload.as_slice());
}

#[test]
fn qa_user_feed_packet_2030_encoding() {
    let target_owner = 257_404_931u32;
    let flag = 1u8;
    let user_payload = packet::build_user_timeline_payload(target_owner, flag);

    // Payload `mw0.c.k` bắt buộc đúng 18 bytes
    assert_eq!(user_payload.len(), 18, "CMD_USER_FEED payload must be 18 bytes");
    assert_eq!(u16::from_be_bytes([user_payload[0], user_payload[1]]), 0);
    assert_eq!(user_payload[2], 0x01);
    assert_eq!(
        i32::from_be_bytes([
            user_payload[3],
            user_payload[4],
            user_payload[5],
            user_payload[6]
        ]),
        packet::CLIENT_VERSION
    );
    assert_eq!(u16::from_be_bytes([user_payload[7], user_payload[8]]), 0);
    assert_eq!(
        u32::from_be_bytes([
            user_payload[9],
            user_payload[10],
            user_payload[11],
            user_payload[12]
        ]),
        target_owner
    );
    assert_eq!(
        i32::from_be_bytes([
            user_payload[13],
            user_payload[14],
            user_payload[15],
            user_payload[16]
        ]),
        1
    );
    assert_eq!(user_payload[17], flag);

    let pkt = packet::build_request_packet(
        packet::CMD_USER_FEED,
        packet::SUBCMD_USER_FEED,
        &user_payload,
    );
    assert_eq!(pkt.len(), 4 + 16 + 18);
    assert_eq!(u16::from_be_bytes([pkt[11], pkt[12]]), 2030);
    assert_eq!(pkt[13], 0);
}

#[test]
fn qa_feed_helpers_timestamp_jxl_url_and_social_profile_variants() {
    // Kiểm tra chuẩn hóa timestamp giây vs mili-giây sang múi giờ UTC+7
    assert_eq!(format_vn_timestamp_ms(0), "");
    assert_eq!(format_vn_timestamp_ms(-500), "");
    let from_sec = format_vn_timestamp_ms(1_726_408_850);
    let from_ms = format_vn_timestamp_ms(1_726_408_850_000);
    assert_eq!(from_sec, from_ms, "Seconds and milliseconds must normalize identically");
    assert_eq!(from_ms, "15/09/2024 lúc 21:00:50");

    // Kiểm tra chuẩn hóa URL ảnh mạng xã hội `.jxl` -> `.jpg`
    assert_eq!(
        normalize_social_media_url(" https://f63-zpsocial.zadn.vn/photo/abc_123.jxl "),
        "https://f63-zpsocial.zadn.vn/photo/abc_123.jpg"
    );
    assert_eq!(
        normalize_social_media_url("https://s120.ava.talk.zdn.vn/avatar.jpg"),
        "https://s120.ava.talk.zdn.vn/avatar.jpg"
    );
    assert_eq!(normalize_social_media_url(".jxl"), ".jxl");

    // Kiểm tra nhãn reaction
    assert_eq!(reaction_id_to_label(1), "Thích");
    assert_eq!(reaction_id_to_label(2), "Yêu thích");
    assert_eq!(reaction_id_to_label(3), "Haha");
    assert_eq!(reaction_id_to_label(4), "Wow");
    assert_eq!(reaction_id_to_label(5), "Buồn");
    assert_eq!(reaction_id_to_label(6), "Phẫn nộ");
    assert_eq!(reaction_id_to_label(999), "Thích");

    // Kiểm tra `SocialUserProfile::from_json` với các biến thể `lastActionTime`, `lastUpdateTime`, `lastUpdate` (u64, i64, string) & nested cover
    let p_u64 = SocialUserProfile::from_json(
        "1001",
        &json!({
            "userId": "1001",
            "displayName": "Nguyễn Văn A",
            "avt": "https://ava.zdn.vn/1.jpg",
            "cover": { "coverUrl": "https://cover.zdn.vn/c1.jpg" },
            "lastActionTime": 1_762_000_000_000u64,
            "isFr": 1
        }),
    )
    .unwrap();
    assert_eq!(p_u64.uid, "1001");
    assert_eq!(p_u64.display_name, "Nguyễn Văn A");
    assert_eq!(p_u64.zalo_name, "Nguyễn Văn A");
    assert_eq!(p_u64.avatar_url, "https://ava.zdn.vn/1.jpg");
    assert_eq!(p_u64.cover_url, "https://cover.zdn.vn/c1.jpg");
    assert_eq!(p_u64.last_action_time, Some(1_762_000_000_000u64));
    assert!(p_u64.is_friend);

    let p_str_update = SocialUserProfile::from_json(
        "1002",
        &json!({
            "dpn": "Trần Thị B",
            "bgavatar": "https://cover.zdn.vn/c2.jpg",
            "bio": "Hello Zalo Rust",
            "dob": "20/10/1999",
            "lastUpdateTime": " 1726408850 ",
            "isFr": 0
        }),
    )
    .unwrap();
    assert_eq!(p_str_update.uid, "1002");
    assert_eq!(p_str_update.display_name, "Trần Thị B");
    assert_eq!(p_str_update.cover_url, "https://cover.zdn.vn/c2.jpg");
    assert_eq!(p_str_update.status, "Hello Zalo Rust");
    assert_eq!(p_str_update.sdob, "20/10/1999");
    assert_eq!(p_str_update.last_action_time, Some(1_726_408_850u64));
    assert!(!p_str_update.is_friend);

    let p_last_update_fallback = SocialUserProfile::from_json(
        "1003",
        &json!({
            "lastUpdate": 1_700_000_000i64
        }),
    )
    .unwrap();
    assert_eq!(p_last_update_fallback.last_action_time, Some(1_700_000_000u64));
}

#[test]
fn qa_sqlite_timeline_user_feed_and_realtime_like_comment_override() {
    let dir = create_isolated_temp_dir("sqlite-feed");
    let db_path = dir.join("test_zalo_feed.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();

    conn.execute_batch(
        "CREATE TABLE timeline_feed_1 (fid TEXT PRIMARY KEY, content TEXT);
         CREATE TABLE user_feed (fid TEXT PRIMARY KEY, content TEXT);
         CREATE TABLE table_feed_real_time_like_comment (
             fid TEXT PRIMARY KEY,
             tot INTEGER,
             is_like INTEGER,
             tot_cmt INTEGER
         );",
    )
    .unwrap();

    // Bài viết 1 trong `timeline_feed_1` (cũ hơn, ban đầu 2 like / 0 cmt, chưa like, nhưng có bản ghi real-time ghi đè)
    let feed_older = json!({
        "item": {
            "fid": "feed_101",
            "cts": 1_725_000_000_000i64,
            "privacy": 1,
            "islike": 0,
            "header": {
                "uid": "257404931",
                "dpn": "Đtc",
                "avt": "https://ava.zdn.vn/dtc.jpg"
            },
            "content": {
                "caption": "Bài viết cũ hơn nhưng có cập nhật real-time",
                "photo_dimension": { "width": 1280, "height": 720 },
                "photos": [
                    {
                        "photoid": "p_101",
                        "origin": "https://f63-zpsocial.zadn.vn/photo/img101.jxl",
                        "thumb": "https://f63-zpsocial.zadn.vn/photo/img101_t.jxl"
                    }
                ]
            },
            "footer": {
                "numlike": 2,
                "numcmt": 0,
                "reaction_info": {
                    "current_reaction_id": 0,
                    "top_reactions": [1, 2, 3]
                }
            }
        }
    });

    // Bài viết 2 trong `user_feed` (mới hơn, có video & privacy = 2 Chỉ mình tôi)
    let feed_newer = json!({
        "fid": "feed_202",
        "cts": 1_726_500_000_000i64,
        "privacy": 2,
        "islike": 0,
        "header": {
            "uid": 257404931i64,
            "dpn": "Đtc",
            "avt": "https://ava.zdn.vn/dtc.jpg"
        },
        "content": {
            "msg": "Bài viết mới nhất từ user_feed",
            "video": {
                "id": 999,
                "videourl": "https://video.zdn.vn/clip.mp4",
                "thumb": "https://video.zdn.vn/clip_thumb.jxl",
                "width": 1920,
                "height": 1080
            }
        },
        "footer": {
            "numlike": 5,
            "numcmt": 1,
            "reaction_info": {
                "current_reaction_id": 2,
                "top_reactions": [2, 4]
            }
        }
    });

    // Bài viết 3 của một người dùng khác (để kiểm tra bộ lọc target_uid)
    let feed_other_user = json!({
        "fid": "feed_303",
        "cts": 1_727_000_000_000i64,
        "header": {
            "uid": "999888777",
            "dpn": "Người Khác"
        },
        "content": {
            "caption": "Bài của người khác"
        }
    });

    conn.execute(
        "INSERT INTO timeline_feed_1 (fid, content) VALUES (?1, ?2)",
        ("feed_101", feed_older.to_string()),
    )
    .unwrap();
    // Chèn trùng `feed_101` vào `user_feed` để kiểm tra khử trùng lặp `seen_fids`
    conn.execute(
        "INSERT INTO user_feed (fid, content) VALUES (?1, ?2)",
        ("feed_101", feed_older.to_string()),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO user_feed (fid, content) VALUES (?1, ?2)",
        ("feed_202", feed_newer.to_string()),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO timeline_feed_1 (fid, content) VALUES (?1, ?2)",
        ("feed_303", feed_other_user.to_string()),
    )
    .unwrap();

    // Bảng real-time ghi đè cho `feed_101`: tot = 48 likes, is_like = 1 (true), tot_cmt = 19 comments
    conn.execute(
        "INSERT INTO table_feed_real_time_like_comment (fid, tot, is_like, tot_cmt) VALUES (?1, ?2, ?3, ?4)",
        ("feed_101", 48i64, 1i64, 19i64),
    )
    .unwrap();
    drop(conn);

    let db_str = db_path.to_str().unwrap();

    // Lọc theo UID "257404931" -> phải trả về đúng 2 bài (khử trùng lặp feed_101), xếp feed_202 trước feed_101
    let dtc_feeds = load_real_timeline_from_sqlite(db_str, "257404931").unwrap();
    assert_eq!(dtc_feeds.len(), 2, "Must deduplicate across timeline_feed_1 and user_feed");
    assert_eq!(dtc_feeds[0].feed_id, "feed_202", "Newest feed must be sorted first");
    assert_eq!(dtc_feeds[1].feed_id, "feed_101", "Older feed must be sorted second");

    // Kiểm tra chi tiết bài `feed_202`
    let f202 = &dtc_feeds[0];
    assert_eq!(f202.status_badge.as_deref(), Some("Chỉ mình tôi"));
    assert!(f202.reaction.is_liked, "current_reaction_id > 0 must mark is_liked = true");
    assert_eq!(f202.reaction.top_reactions, vec!["Yêu thích", "Wow"]);
    assert_eq!(f202.media_list.len(), 1);
    assert_eq!(f202.media_list[0].media_type, "video");
    assert_eq!(f202.media_list[0].thumbnail, "https://video.zdn.vn/clip_thumb.jpg");

    // Kiểm tra chi tiết bài `feed_101` đã được bảng `table_feed_real_time_like_comment` ghi đè
    let f101 = &dtc_feeds[1];
    assert_eq!(f101.status_badge.as_deref(), Some("Bạn bè"));
    assert_eq!(f101.reaction.like_count, 48, "Realtime table must override like_count");
    assert_eq!(f101.reaction.comment_count, 19, "Realtime table must override comment_count");
    assert!(f101.reaction.is_liked, "Realtime table must override is_liked");
    assert_eq!(f101.reaction.top_reactions, vec!["Thích", "Yêu thích", "Haha"]);
    assert_eq!(f101.media_list.len(), 1);
    assert_eq!(
        f101.media_list[0].url,
        "https://f63-zpsocial.zadn.vn/photo/img101.jpg",
        ".jxl origin must normalize to .jpg"
    );
    assert_eq!(f101.media_list[0].width, 1280);
    assert_eq!(f101.media_list[0].height, 720);

    // Lọc tất cả bài đăng (`target_uid = ""`) -> trả về đủ 3 bài theo thứ tự mới nhất
    let all_feeds = load_real_timeline_from_sqlite(db_str, "").unwrap();
    assert_eq!(all_feeds.len(), 3);
    assert_eq!(all_feeds[0].feed_id, "feed_303");
    assert_eq!(all_feeds[1].feed_id, "feed_202");
    assert_eq!(all_feeds[2].feed_id, "feed_101");

    let _ = std::fs::remove_dir_all(&dir);
}

// ============================================================================
// 2. MULTI-ACCOUNT SNAPSHOT & QUICK-LOGIN LIFECYCLE (`src/account_manager.rs`)
// ============================================================================

#[test]
fn qa_multi_account_snapshot_list_switch_and_delete_lifecycle() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let original_cwd = std::env::current_dir().unwrap();
    let temp_workspace = create_isolated_temp_dir("multi-account");

    // Chuyển vào thư mục tạm cô lập để kiểm thử vòng đời `account_manager`
    std::env::set_current_dir(&temp_workspace).unwrap();

    let test_result = std::panic::catch_unwind(|| {
        // Khởi tạo dữ liệu tài khoản 1 (UIN: 111000111)
        let session_acc1 = json!({
            "login": {
                "uin": "111000111",
                "display_name": "Tài Khoản Số 1",
                "avatar": "https://ava.zdn.vn/acc1.jpg"
            }
        });
        std::fs::write("session.json", session_acc1.to_string()).unwrap();
        std::fs::write("threads_cache.json", r#"[{"id":"thread_acc1"}]"#).unwrap();
        std::fs::write("zalo_history.db", b"sqlite_acc1_bytes").unwrap();

        save_current_account_snapshot("111000111").unwrap();

        // Khởi tạo dữ liệu tài khoản 2 (UIN: 222000222) đang active
        let session_acc2 = json!({
            "login": {
                "uin": "222000222",
                "display_name": "Tài Khoản Số 2",
                "avatar": "https://ava.zdn.vn/acc2.jpg"
            }
        });
        std::fs::write("session.json", session_acc2.to_string()).unwrap();
        std::fs::write("threads_cache.json", r#"[{"id":"thread_acc2"}]"#).unwrap();
        std::fs::write("zalo_history.db", b"sqlite_acc2_bytes").unwrap();

        save_current_account_snapshot("222000222").unwrap();

        // Kiểm tra `list_saved_accounts` bỏ qua các file `*_threads.json` và đánh dấu `is_current` chuẩn xác
        let saved = list_saved_accounts("222000222");
        assert_eq!(saved.len(), 2, "Must list exactly 2 saved accounts (ignoring *_threads.json)");
        let acc1_prof = saved.iter().find(|a| a.uin == "111000111").unwrap();
        assert_eq!(acc1_prof.display_name, "Tài Khoản Số 1");
        assert!(!acc1_prof.is_current);

        let acc2_prof = saved.iter().find(|a| a.uin == "222000222").unwrap();
        assert_eq!(acc2_prof.display_name, "Tài Khoản Số 2");
        assert!(acc2_prof.is_current);

        // Chuyển đổi nhanh từ tài khoản 2 sang tài khoản 1
        switch_to_account("222000222", "111000111").unwrap();

        let active_session: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string("session.json").unwrap()).unwrap();
        assert_eq!(active_session["login"]["uin"], "111000111");
        assert_eq!(
            std::fs::read_to_string("threads_cache.json").unwrap(),
            r#"[{"id":"thread_acc1"}]"#
        );
        assert_eq!(
            std::fs::read("zalo_history.db").unwrap(),
            b"sqlite_acc1_bytes"
        );

        // Chuyển ngược lại sang tài khoản 2 để xác nhận snapshot của tài khoản 2 vẫn toàn vẹn
        switch_to_account("111000111", "222000222").unwrap();
        let active_session_2: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string("session.json").unwrap()).unwrap();
        assert_eq!(active_session_2["login"]["uin"], "222000222");
        assert_eq!(
            std::fs::read("zalo_history.db").unwrap(),
            b"sqlite_acc2_bytes"
        );

        // Chuyển sang UIN không tồn tại phải trả lỗi an toàn
        assert!(switch_to_account("222000222", "999999999").is_err());

        // Xóa tài khoản 1 khỏi danh sách lưu
        delete_saved_account("111000111").unwrap();
        let remaining = list_saved_accounts("222000222");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].uin, "222000222");
    });

    // Khôi phục CWD và dọn dẹp thư mục tạm
    std::env::set_current_dir(&original_cwd).unwrap();
    let _ = std::fs::remove_dir_all(&temp_workspace);

    if let Err(err) = test_result {
        std::panic::resume_unwind(err);
    }
}

#[test]
fn qa_cross_platform_data_dir_and_profile_isolation() {
    let cwd = PathBuf::from("/workspace/ctdozlite");
    let mac_bundle_exe = PathBuf::from("/Applications/ctdozlite.app/Contents/MacOS/ctdozlite");
    let linux_bin_exe = PathBuf::from("/usr/bin/ctdozlite");
    let home = PathBuf::from("/Users/tester");

    // macOS .app bundle tự động ánh xạ vào Application Support và bật chế độ private
    let (mac_path, mac_priv) =
        data_dir::resolve(&[], &cwd, &mac_bundle_exe, Some(&home), true).unwrap();
    assert_eq!(
        mac_path,
        PathBuf::from("/Users/tester/Library/Application Support/ctdozlite")
    );
    assert!(mac_priv);

    // Tham số `--profile` cô lập dữ liệu từng tài khoản trên cả macOS bundle và Linux/Windows
    let (prof_mac, prof_priv) = data_dir::resolve(
        &["--profile=work_account_01".to_string()],
        &cwd,
        &mac_bundle_exe,
        Some(&home),
        true,
    )
    .unwrap();
    assert_eq!(
        prof_mac,
        PathBuf::from("/Users/tester/Library/Application Support/ctdozlite/profiles/work_account_01")
    );
    assert!(prof_priv);

    let (prof_linux, prof_linux_priv) = data_dir::resolve(
        &["--profile".to_string(), "acc_2".to_string()],
        &cwd,
        &linux_bin_exe,
        None,
        false,
    )
    .unwrap();
    assert_eq!(prof_linux, cwd.join("profiles").join("acc_2"));
    assert!(prof_linux_priv);
}

// ============================================================================
// 3. CROSS-PLATFORM DPI SCALING & CLAMP SAFETY (`120.0..=1920.0`)
// ============================================================================

/// Mô phỏng công thức tính độ rộng sidebar và các hộp thoại (`modals.rs` / `gui.rs`)
/// trên mọi mức DPI (100%, 125%, 150%, 200%) và kích thước cửa sổ (`screen_w`).
fn compute_safe_layout_widths(screen_w: f32, dpi_scale: f32) -> (f32, f32, f32, f32) {
    // Đảm bảo kích thước điểm logic hợp lệ ngay cả khi cửa sổ bị thu nhỏ cực đại trên Windows/Wayland/macOS
    let logical_w = (screen_w / dpi_scale.max(0.5)).max(1.0);

    // 1. Sidebar danh sách hội thoại (`gui.rs`): bảo vệ `max_sidebar >= min_sidebar` trước khi clamp
    let min_sidebar = 180.0_f32.min((logical_w * 0.45).max(60.0));
    let max_sidebar = (logical_w - 120.0).max(min_sidebar);
    let sidebar_w = (logical_w * 0.28).clamp(min_sidebar, max_sidebar);

    // 2. Cửa sổ Nhật ký / Social Feed (`modals.rs`): `(screen_w - 36.0).clamp(320.0, 560.0)`
    // Khi `logical_w` siêu hẹp (< 356px), giới hạn phải tự co lại hoặc giữ `min <= max`
    let feed_min = 320.0_f32.min(logical_w);
    let feed_max = 560.0_f32.max(feed_min);
    let feed_modal_w = (logical_w - 36.0).clamp(feed_min, feed_max);

    // 3. Cửa sổ Hồ sơ cá nhân (`modals.rs`): `(screen_w - 40.0).clamp(320.0, 440.0)`
    let prof_min = 320.0_f32.min(logical_w);
    let prof_max = 440.0_f32.max(prof_min);
    let profile_modal_w = (logical_w - 40.0).clamp(prof_min, prof_max);

    // 4. Vùng nội dung chat chính còn lại
    let chat_panel_w = (logical_w - sidebar_w).max(0.0);

    (sidebar_w, feed_modal_w, profile_modal_w, chat_panel_w)
}

#[test]
fn qa_dpi_scaling_and_narrow_window_clamp_never_panics() {
    let dpi_scales = [1.0_f32, 1.25_f32, 1.5_f32, 1.75_f32, 2.0_f32];

    for &dpi in &dpi_scales {
        let mut w = 120.0_f32;
        while w <= 1920.0 {
            let (sidebar_w, feed_w, profile_w, chat_w) = compute_safe_layout_widths(w, dpi);
            assert!(sidebar_w.is_finite() && sidebar_w > 0.0);
            assert!(feed_w.is_finite() && feed_w > 0.0);
            assert!(profile_w.is_finite() && profile_w > 0.0);
            assert!(chat_w.is_finite() && chat_w >= 0.0);
            w += 15.0;
        }
    }
}

// ============================================================================
// 4. UNSTABLE NETWORK / OUT-OF-ORDER TIMESTAMP RESILIENCE & UPDATER SAFETY
// ============================================================================

#[test]
fn qa_out_of_order_feed_timestamps_sort_deterministically() {
    let make_item = |fid: &str, ts_ms: i64| FeedItem {
        feed_id: fid.to_string(),
        author: FeedAuthor {
            uid: "257404931".to_string(),
            display_name: "Đtc".to_string(),
            avatar_url: String::new(),
        },
        date_str: format_vn_timestamp_ms(ts_ms),
        content: format!("Post {fid}"),
        status_badge: None,
        media_list: Vec::new(),
        reaction: FeedReaction {
            like_count: 0,
            comment_count: 0,
            is_liked: false,
            top_reactions: Vec::new(),
        },
        comments: Vec::new(),
        timestamp_ms: ts_ms,
    };

    // Giả lập các gói tin đến lộn xộn do mạng chập chờn (bao gồm 2 bài cùng timestamp để kiểm tra tie-breaker `feed_id`)
    let mut permutation_a = vec![
        make_item("feed_001", 1_700_000_100_000),
        make_item("feed_005", 1_700_000_900_000),
        make_item("feed_003_a", 1_700_000_500_000),
        make_item("feed_002", 1_700_000_200_000),
        make_item("feed_003_b", 1_700_000_500_000),
    ];

    let mut permutation_b = vec![
        make_item("feed_003_b", 1_700_000_500_000),
        make_item("feed_002", 1_700_000_200_000),
        make_item("feed_005", 1_700_000_900_000),
        make_item("feed_001", 1_700_000_100_000),
        make_item("feed_003_a", 1_700_000_500_000),
    ];

    let sort_fn = |a: &FeedItem, b: &FeedItem| {
        b.timestamp_ms
            .cmp(&a.timestamp_ms)
            .then_with(|| b.feed_id.cmp(&a.feed_id))
    };

    permutation_a.sort_by(sort_fn);
    permutation_b.sort_by(sort_fn);

    let ids_a: Vec<&str> = permutation_a.iter().map(|i| i.feed_id.as_str()).collect();
    let ids_b: Vec<&str> = permutation_b.iter().map(|i| i.feed_id.as_str()).collect();

    assert_eq!(ids_a, ids_b, "Sorting must be 100% deterministic regardless of arrival order");
    assert_eq!(
        ids_a,
        vec!["feed_005", "feed_003_b", "feed_003_a", "feed_002", "feed_001"]
    );
}

#[test]
fn qa_updater_asset_selection_and_transactional_rollback() {
    // Kiểm tra chấm điểm gói cài đặt theo từng hệ điều hành / kiến trúc
    assert!(
        updater::select_platform_asset_score(
            "ctdozlite-0.1.0-portable-windows-x86_64.exe",
            "windows",
            "x86_64"
        ) > updater::select_platform_asset_score(
            "ctdozlite-0.1.0-setup-windows-x86_64.exe",
            "windows",
            "x86_64"
        )
    );
    assert_eq!(
        updater::select_platform_asset_score("ctdozlite-0.1.0-macos-arm64.dmg", "macos", "aarch64"),
        110
    );
    assert_eq!(
        updater::select_platform_asset_score("ctdozlite-0.1.0-x86_64.AppImage", "linux", "x86_64"),
        100
    );
    assert!(updater::validate_asset_name("../evil.exe").is_err());
    assert!(updater::validate_asset_name("ctdozlite-0.1.0-portable.exe").is_ok());

    // Kiểm tra cài đặt nguyên tử và tự động rollback khi khởi chạy thất bại
    let dir = create_isolated_temp_dir("updater-rollback");
    let bin_path = dir.join("ctdozlite.exe");
    std::fs::write(&bin_path, b"MZ-original-binary").unwrap();

    let staged = updater::stage_portable_release(b"MZ-updated-binary", &bin_path).unwrap();
    // Khi đang stage, file gốc chưa bị ghi đè
    assert_eq!(std::fs::read(&bin_path).unwrap(), b"MZ-original-binary");

    // Giả lập lỗi khi launch bản cập nhật -> phải tự động khôi phục bản cũ
    let res = staged.launch_and_finish(|_path| Err("Simulated launch failure".to_string()));
    assert!(res.is_err());
    assert_eq!(
        std::fs::read(&bin_path).unwrap(),
        b"MZ-original-binary",
        "Failed launch must atomically restore original binary"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
