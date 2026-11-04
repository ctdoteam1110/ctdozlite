use aes::Aes256;
use cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use sha2::{Digest, Sha256};
use zalo_rust::backup::{
    decrypt_v2, merge_desktop_records, parse_desktop_record, validate_tar, verify_v2_header,
};

fn fixture(uin: &str) -> Vec<u8> {
    let mut header = [0u8; 512];
    header[..8].copy_from_slice(b"demo.txt");
    header[100..108].copy_from_slice(b"0000644\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(b"00000000000\0");
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let key = Sha256::digest(uin.as_bytes());
    let iv = format!("zie{}", &uin[..13]);
    let mut buf = vec![0; 528];
    buf[..512].copy_from_slice(&header);
    cbc::Encryptor::<Aes256>::new_from_slices(&key, iv.as_bytes())
        .unwrap()
        .encrypt_padded_mut::<Pkcs7>(&mut buf, 512)
        .unwrap();
    buf
}

#[test]
fn recognizes_valid_v2_tar_header() {
    let uin = "0123456789abcdef0123456789abcdef";
    assert!(verify_v2_header(&fixture(uin), uin).is_ok());
}

#[test]
fn matches_independent_aes_cbc_known_answer() {
    let ciphertext = fixture("0123456789abcdef0123456789abcdef");
    assert_eq!(
        &ciphertext[..32],
        &[
            0x3f, 0xd9, 0xaa, 0x9a, 0x6a, 0x43, 0x43, 0xa7, 0x7c, 0x98, 0x91, 0x91, 0x8e, 0xd4,
            0x49, 0xd3, 0x91, 0xb5, 0xe5, 0x98, 0xe1, 0x19, 0xf9, 0xb6, 0x8d, 0xda, 0x94, 0x9a,
            0x42, 0x99, 0x44, 0x5c
        ]
    );
}

#[test]
fn rejects_wrong_account_value() {
    let ciphertext = fixture("0123456789abcdef0123456789abcdef");
    assert!(verify_v2_header(&ciphertext, "ffffffffffffffffffffffffffffffff").is_err());
}

#[test]
fn rejects_corrupt_or_truncated_header() {
    let uin = "0123456789abcdef0123456789abcdef";
    let mut ciphertext = fixture(uin);
    ciphertext[240] ^= 0x80;
    assert!(verify_v2_header(&ciphertext, uin).is_err());
    assert!(verify_v2_header(&ciphertext[..200], uin).is_err());
}

#[test]
fn decrypts_complete_archive_and_removes_padding() {
    let uin = "0123456789abcdef0123456789abcdef";
    let ciphertext = fixture(uin);
    let mut output = Vec::new();
    decrypt_v2(&ciphertext[..], &mut output, uin).unwrap();
    assert_eq!(output.len(), 512);
    assert_eq!(&output[..8], b"demo.txt");
}

#[test]
fn decrypts_across_multiple_chunks_with_short_reads() {
    use std::io::{self, Read};
    struct ShortReads<R>(R);
    impl<R: Read> Read for ShortReads<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let len = buffer.len().min(113);
            self.0.read(&mut buffer[..len])
        }
    }
    let uin = "0123456789abcdef0123456789abcdef";
    let mut plain = Vec::new();
    decrypt_v2(&fixture(uin)[..], &mut plain, uin).unwrap();
    plain.extend((0..140_000).map(|i| (i % 251) as u8));
    let len = plain.len();
    let mut encrypted = plain.clone();
    encrypted.resize(len + 16, 0);
    let key = Sha256::digest(uin.as_bytes());
    let iv = format!("zie{}", &uin[..13]);
    let cipher_len = cbc::Encryptor::<Aes256>::new_from_slices(&key, iv.as_bytes())
        .unwrap()
        .encrypt_padded_mut::<Pkcs7>(&mut encrypted, len)
        .unwrap()
        .len();
    encrypted.truncate(cipher_len);
    let mut recovered = Vec::new();
    decrypt_v2(ShortReads(&encrypted[..]), &mut recovered, uin).unwrap();
    assert_eq!(recovered, plain);
}

#[test]
fn rejects_invalid_padding_and_partial_blocks() {
    let uin = "0123456789abcdef0123456789abcdef";
    let mut ciphertext = fixture(uin);
    let last = ciphertext.len() - 1;
    ciphertext[last] ^= 0x80;
    assert!(decrypt_v2(&ciphertext[..], Vec::new(), uin).is_err());
    assert!(decrypt_v2(&ciphertext[..last], Vec::new(), uin).is_err());
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, body) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_size(body.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        builder.append_data(&mut header, name, *body).unwrap();
    }
    builder.into_inner().unwrap()
}

#[test]
fn accepts_regular_archive_entries() {
    let tar = archive(&[("account/database/messages.zdb", b"demo")]);
    assert!(validate_tar(&tar[..]).is_ok());
}

#[test]
fn rejects_duplicate_archive_destinations() {
    let tar = archive(&[("account/file", b"one"), ("account/file", b"two")]);
    assert!(validate_tar(&tar[..]).is_err());
}

#[test]
fn rejects_windows_path_separator_in_archive_name() {
    let mut tar = archive(&[("account/escape", b"demo")]);
    tar[7] = b'\\';
    tar[148..156].fill(b' ');
    let sum: u32 = tar[..512].iter().map(|&b| u32::from(b)).sum();
    tar[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    assert!(validate_tar(&tar[..]).is_err());
}

#[test]
fn rejects_symlink_entries() {
    let mut tar = archive(&[("account/target", b"")]);
    tar[156] = b'2';
    tar[148..156].fill(b' ');
    let sum: u32 = tar[..512].iter().map(|&b| u32::from(b)).sum();
    tar[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    assert!(validate_tar(&tar[..]).is_err());
}

#[test]
fn rejects_archive_missing_end_markers() {
    let mut tar = archive(&[("account/file", b"demo")]);
    tar.truncate(1024);
    assert!(validate_tar(&tar[..]).is_err());
}

#[test]
fn desktop_text_record_keeps_thread_id_and_server_time() {
    let record = serde_json::json!({
        "msgId": "demo-message", "toUid": "demo-thread", "fromUid": "0",
        "serverTime": 1700000000000_i64, "cliMsgId": "12", "msgType": 1,
        "message": "hello", "content": null
    });
    let message = parse_desktop_record(&record, "demo-self").unwrap();
    assert_eq!(message.msg_id, "demo-message");
    assert_eq!(message.thread_id, "demo-thread");
    assert_eq!(message.ts, 1700000000000_i64);
    assert_eq!(message.text, "hello");
    assert!(message.is_outgoing);
}

#[test]
fn desktop_type_four_is_a_sticker_not_a_video() {
    let record = serde_json::json!({
        "msgId": "demo-sticker", "toUid": "demo-thread", "fromUid": "demo-peer",
        "serverTime": 1700000000000_i64, "msgType": 4,
        "message": {"id": 123, "catId": 9, "type": 7}, "content": null
    });
    let message = parse_desktop_record(&record, "demo-self").unwrap();
    assert_eq!(message.msg_type, "chat.sticker");
    assert!(message.text.contains("Nhãn dán"));
    assert!(!message.is_outgoing);
}

#[test]
fn desktop_sender_can_be_account_id_instead_of_zero() {
    let record = serde_json::json!({"msgId":"demo-message","toUid":"demo-thread","fromUid":"demo-self","serverTime":1700000000000_i64,"msgType":1,"message":"hello"});
    let message = parse_desktop_record(&record, "demo-self").unwrap();
    assert!(message.is_outgoing);
}

#[test]
fn merging_backup_records_is_idempotent_and_preserves_existing_messages() {
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let thread = zalo_rust::store::StoredThread {
        thread_id: "demo-thread".into(),
        name: "Friend".into(),
        is_group: false,
        last_ts: 1,
    };
    let existing = zalo_rust::store::StoredMessage {
        msg_id: "demo-message".into(),
        thread_id: "demo-thread".into(),
        sender_id: "0".into(),
        sender_name: "Me".into(),
        text: "original".into(),
        msg_type: "chat.text".into(),
        ts: 1,
        is_outgoing: true,
        raw_json: "{}".into(),
    };
    store.upsert_message(&existing).unwrap();
    let record = serde_json::json!({"msgId":"demo-message","toUid":"demo-thread","fromUid":"0","serverTime":1700000000000_i64,"msgType":1,"message":"replacement"});
    let fresh = serde_json::json!({"msgId":"demo-new","toUid":"demo-thread","fromUid":"demo-peer","serverTime":1700000000001_i64,"msgType":1,"message":"new"});
    let first = merge_desktop_records(
        &mut store,
        &[thread.clone()],
        vec![record.clone(), fresh.clone()],
        "demo-self",
    )
    .unwrap();
    let second =
        merge_desktop_records(&mut store, &[thread], vec![record, fresh], "demo-self").unwrap();
    assert_eq!(first, 1);
    assert_eq!(second, 0);
    let saved = store.messages_for("demo-thread", 10).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(
        saved
            .iter()
            .find(|m| m.msg_id == "demo-message")
            .unwrap()
            .text,
        "original"
    );
}

#[test]
fn merge_rolls_back_when_record_has_unknown_conversation() {
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let thread = zalo_rust::store::StoredThread {
        thread_id: "demo-thread".into(),
        name: "Friend".into(),
        is_group: false,
        last_ts: 1,
    };
    let record = serde_json::json!({"msgId":"demo-message","toUid":"unknown","fromUid":"0","serverTime":1700000000000_i64,"msgType":1,"message":"hello"});
    assert!(merge_desktop_records(&mut store, &[thread], vec![record], "demo-self").is_err());
    assert!(store.all_threads().unwrap().is_empty());
}

#[test]
fn unknown_desktop_message_is_preserved_for_later_recovery() {
    let record = serde_json::json!({"msgId":"unknown-1","toUid":"thread-1","fromUid":"peer-1","serverTime":1700000000000_i64,"msgType":999,"message":{"future":"opaque"}});
    let parsed = parse_desktop_record(&record, "self-1").unwrap();
    assert_eq!(parsed.msg_id, "unknown-1");
    assert!(parsed.raw_json.contains("opaque"));
}

#[test]
fn validated_desktop_archive_merges_conversations_and_messages() {
    use zalo_rust::backup::import_tar_records;
    let conversation = br#"{"userId":"thread-1","isGroup":false}
"#;
    let message = br#"{"msgId":"message-1","toUid":"thread-1","fromUid":"0","serverTime":1700000000000,"msgType":1,"message":"hello"}
"#;
    let tar = archive(&[
        (
            "self-1/ZaloDownloads/database/self-1_zthread.zdb",
            conversation,
        ),
        ("self-1/ZaloDownloads/database/self-1_zmessage.zdb", message),
    ]);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let count = import_tar_records(std::io::Cursor::new(&tar), &mut store, "self-1").unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.messages_for("thread-1", 10).unwrap()[0].text, "hello");
    assert_eq!(
        import_tar_records(std::io::Cursor::new(&tar), &mut store, "self-1").unwrap(),
        0
    );
}

#[test]
fn desktop_archive_rejects_a_different_account_before_writing() {
    use zalo_rust::backup::import_tar_records;
    let tar = archive(&[
        (
            "self-1/ZaloDownloads/database/self-1_zthread.zdb",
            br#"{"userId":"thread-1","isGroup":false}
"#,
        ),
        ("self-1/ZaloDownloads/database/self-1_zmessage.zdb", b""),
    ]);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    assert!(import_tar_records(std::io::Cursor::new(&tar), &mut store, "other-account").is_err());
    assert!(store.all_threads().unwrap().is_empty());
}

#[test]
fn desktop_system_record_without_sender_is_preserved() {
    let record = serde_json::json!({"msgId":"system-1","toUid":"thread-1","fromUid":"","serverTime":1700000000000_i64,"msgType":-4,"message":"event"});
    let parsed = parse_desktop_record(&record, "self-1").unwrap();
    assert_eq!(parsed.msg_type, "backup.unknown");
    assert!(parsed.raw_json.contains("system-1"));
}

#[test]
fn encrypted_desktop_backup_imports_through_private_staging() {
    use std::sync::atomic::AtomicBool;
    use zalo_rust::backup::import_encrypted_backup;
    let uin = "0123456789abcdef0123456789abcdef";
    let tar = archive(&[("self-1/ZaloDownloads/database/self-1_zthread.zdb", br#"{"userId":"thread-1","isGroup":false}
"#), ("self-1/ZaloDownloads/database/self-1_zmessage.zdb", br#"{"msgId":"message-1","toUid":"thread-1","fromUid":"0","serverTime":1700000000000,"msgType":1,"message":"hello"}
"#)]);
    let key = Sha256::digest(uin.as_bytes());
    let iv = format!("zie{}", &uin[..13]);
    let mut ciphertext = tar.clone();
    ciphertext.resize(tar.len() + 16, 0);
    let len = cbc::Encryptor::<Aes256>::new_from_slices(&key, iv.as_bytes())
        .unwrap()
        .encrypt_padded_mut::<Pkcs7>(&mut ciphertext, tar.len())
        .unwrap()
        .len();
    ciphertext.truncate(len);
    let stage = tempfile::tempdir().unwrap();
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let count = import_encrypted_backup(
        std::io::Cursor::new(&ciphertext[..]),
        uin,
        "self-1",
        &mut store,
        stage.path(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.messages_for("thread-1", 10).unwrap()[0].text, "hello");
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
}

#[test]
fn cancelled_backup_import_does_not_write_messages_or_leave_staging() {
    use std::sync::atomic::AtomicBool;
    use zalo_rust::backup::import_encrypted_backup;
    let stage = tempfile::tempdir().unwrap();
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    assert!(import_encrypted_backup(
        std::io::Cursor::new(fixture("0123456789abcdef0123456789abcdef")),
        "0123456789abcdef0123456789abcdef",
        "self-1",
        &mut store,
        stage.path(),
        &AtomicBool::new(true)
    )
    .is_err());
    assert!(store.all_threads().unwrap().is_empty());
    assert_eq!(std::fs::read_dir(stage.path()).unwrap().count(), 0);
}

#[test]
fn matches_desktop_media_name_only_when_message_identity_and_url_hash_agree() {
    use zalo_rust::backup::desktop_media_matches;
    let record = serde_json::json!({"cliMsgId":"101","fromUid":"0","toUid":"202","message":{"oriUrl":"https://example.test/image"}});
    let digest = zalo_rust::crypto::md5_hex("https://example.test/image");
    assert!(desktop_media_matches(
        &record,
        "303",
        &format!("101_303_202_{digest}.jxl")
    ));
    assert!(!desktop_media_matches(
        &record,
        "303",
        &format!("101_999_202_{digest}.jxl")
    ));
    assert!(!desktop_media_matches(
        &record,
        "303",
        "101_303_202_00000000000000000000000000000000.jxl"
    ));
}

#[test]
fn matches_group_media_only_when_group_id_and_url_match() {
    let record = serde_json::json!({"msgId":"m1","cliMsgId":"101","toUid":"202","fromUid":"0","msgType":2,"message":{"oriUrl":"https://example.test/image"}});
    let digest = zalo_rust::crypto::md5_hex("https://example.test/image");
    assert!(zalo_rust::backup::desktop_media_matches(&record, "303", &format!("101_303_g202_{digest}.jxl")));
    assert!(!zalo_rust::backup::desktop_media_matches(&record, "303", &format!("101_303_g999_{digest}.jxl")));
    assert!(!zalo_rust::backup::desktop_media_matches(&record, "303", "101_303_g202_00000000000000000000000000000000.jxl"));
}

#[test]
fn decodes_jpeg_xl_backup_image() {
    let image =
        zalo_rust::backup::decode_backup_image(include_bytes!("fixtures/3x3_srgb_lossy.jxl"))
            .unwrap();
    assert_eq!(image.dimensions(), (3, 3));
}

#[test]
fn rejects_invalid_backup_image() {
    assert!(zalo_rust::backup::decode_backup_image(b"invalid image").is_err());
}

#[test]
fn imported_messages_with_distinct_ids_are_not_collapsed_on_load() {
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    store
        .upsert_thread(&zalo_rust::store::StoredThread {
            thread_id: "thread-1".into(),
            name: "Friend".into(),
            is_group: false,
            last_ts: 1700000000000,
        })
        .unwrap();
    for id in ["message-1", "message-2"] {
        store
            .upsert_message(&zalo_rust::store::StoredMessage {
                msg_id: id.into(),
                thread_id: "thread-1".into(),
                sender_id: "peer-1".into(),
                sender_name: "Friend".into(),
                text: "same text".into(),
                msg_type: "chat.text".into(),
                ts: 1700000000000,
                is_outgoing: false,
                raw_json: String::new(),
            })
            .unwrap();
    }
    let mut threads = Vec::new();
    store.hydrate_threads(&mut threads, 20).unwrap();
    assert_eq!(threads[0].messages.len(), 2);
}

#[test]
fn archive_import_stores_matched_jxl_with_its_message() {
    use zalo_rust::backup::import_tar_records;
    let url = "https://example.test/image";
    let name = format!(
        "303/ZaloDownloads/picture/202/101_303_202_{}.jxl",
        zalo_rust::crypto::md5_hex(url)
    );
    let message = format!("{{\"msgId\":\"message-1\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":2,\"message\":{{\"oriUrl\":\"{url}\"}}}}\n");
    let bytes = include_bytes!("fixtures/3x3_srgb_lossy.jxl");
    let tar = archive(&[
        (
            "303/ZaloDownloads/database/303_zthread.zdb",
            br#"{"userId":"202","isGroup":false}
"#,
        ),
        (
            "303/ZaloDownloads/database/303_zmessage.zdb",
            message.as_bytes(),
        ),
        (&name, bytes),
    ]);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    assert_eq!(
        import_tar_records(std::io::Cursor::new(&tar), &mut store, "303").unwrap(),
        1
    );
    let saved: Vec<u8> = store
        .conn
        .query_row(
            "SELECT data FROM backup_media WHERE msg_id='message-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saved, bytes);
}

#[test]
fn imported_image_is_available_after_history_reload() {
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    store
        .upsert_thread(&zalo_rust::store::StoredThread {
            thread_id: "thread-1".into(),
            name: "Friend".into(),
            is_group: false,
            last_ts: 1,
        })
        .unwrap();
    store
        .upsert_message(&zalo_rust::store::StoredMessage {
            msg_id: "message-1".into(),
            thread_id: "thread-1".into(),
            sender_id: "0".into(),
            sender_name: "Me".into(),
            text: "[Hình ảnh]".into(),
            msg_type: "chat.photo".into(),
            ts: 1,
            is_outgoing: true,
            raw_json: String::new(),
        })
        .unwrap();
    store
        .conn
        .execute(
            "INSERT INTO backup_media(msg_id,data,extension) VALUES (?1,?2,?3)",
            rusqlite::params!["message-1", b"image".as_slice(), "jxl"],
        )
        .unwrap();
    let mut threads = Vec::new();
    store.hydrate_threads(&mut threads, 20).unwrap();
    assert_eq!(
        threads[0].messages[0].backup_media_ref.as_deref(),
        Some("backup-media:message-1")
    );
}

#[test]
fn recovered_attachment_can_be_streamed_from_store() {
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    store
        .upsert_message(&zalo_rust::store::StoredMessage {
            msg_id: "message-1".into(),
            thread_id: "thread-1".into(),
            sender_id: "0".into(),
            sender_name: "Me".into(),
            text: "[Tập tin]".into(),
            msg_type: "chat.file".into(),
            ts: 1,
            is_outgoing: true,
            raw_json: String::new(),
        })
        .unwrap();
    store
        .conn
        .execute(
            "INSERT INTO backup_media(msg_id,data,extension) VALUES (?1,?2,?3)",
            rusqlite::params!["message-1", b"sample".as_slice(), "txt"],
        )
        .unwrap();
    let mut output = Vec::new();
    assert_eq!(
        zalo_rust::backup::copy_backup_media(&store, "message-1", &mut output).unwrap(),
        6
    );
    assert_eq!(output, b"sample");
}

#[test]
fn desktop_video_is_labeled_for_attachment_display() {
    let record = serde_json::json!({"msgId":"video-1","toUid":"202","fromUid":"0","serverTime":1700000000000_i64,"msgType":18,"message":{"oriUrl":"https://example.test/video.mp4","duration":4}});
    let parsed = parse_desktop_record(&record, "303").unwrap();
    assert_eq!(parsed.msg_type, "chat.video");
    assert!(parsed.raw_json.contains("Video Zalo"));
}

#[test]
fn desktop_video_cache_matches_exact_message_identity() {
    let record = serde_json::json!({"msgType":18,"cliMsgId":"101","fromUid":"0","toUid":"202","message":{"oriUrl":"https://example.test/video"}});
    assert!(zalo_rust::backup::desktop_media_matches(
        &record,
        "303",
        "101_303_202.mp4"
    ));
    assert!(!zalo_rust::backup::desktop_media_matches(
        &record,
        "303",
        "101_999_202.mp4"
    ));
}

#[test]
fn import_summary_counts_added_missing_and_unsupported_records() {
    let account = "303";
    let conversation = br#"{"userId":"202","isGroup":false}
"#;
    let messages = b"{\"msgId\":\"m1\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":2,\"message\":{\"oriUrl\":\"https://example.test/missing\"}}\n{\"msgId\":\"m2\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000001,\"msgType\":999,\"message\":\"future\"}\n";
    let tar = archive(&[
        ("303/ZaloDownloads/database/303_zthread.zdb", conversation),
        ("303/ZaloDownloads/database/303_zmessage.zdb", messages),
    ]);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let report = zalo_rust::backup::import_tar_records_report(
        std::io::Cursor::new(tar),
        &mut store,
        account,
    )
    .unwrap();
    assert_eq!(report.messages_added, 2);
    assert_eq!(report.unsupported_records, 1);
    assert_eq!(report.missing_media, 1);
    assert_eq!(report.media_added, 0);
}

#[test]
fn recovered_media_survives_merge_with_existing_live_message() {
    let mut thread = zalo_rust::chat::Thread {
        id: "thread-1".into(),
        messages: vec![zalo_rust::chat::ChatMessage {
            id: "message-1".into(),
            text: "hello".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let incoming = zalo_rust::chat::Thread {
        id: "thread-1".into(),
        messages: vec![zalo_rust::chat::ChatMessage {
            id: "message-1".into(),
            text: "hello".into(),
            backup_media_ref: Some("backup-media:message-1".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    zalo_rust::chat::merge_thread_into(&mut thread, incoming, false);
    assert_eq!(
        thread.messages[0].backup_media_ref.as_deref(),
        Some("backup-media:message-1")
    );
}

#[test]
fn unsupported_backup_record_survives_normal_message_updates() {
    let archive=archive(&[("303/ZaloDownloads/database/303_zthread.zdb", br#"{"userId":"202","isGroup":false}
"#), ("303/ZaloDownloads/database/303_zmessage.zdb", br#"{"msgId":"unknown-1","toUid":"202","fromUid":"0","serverTime":1700000000000,"msgType":999,"message":{"future":"preserved"}}
"#)]);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    zalo_rust::backup::import_tar_records(std::io::Cursor::new(archive), &mut store, "303")
        .unwrap();
    let mut msg = store.messages_for("202", 1).unwrap().remove(0);
    msg.raw_json = "{\"updated\":true}".into();
    store.upsert_message(&msg).unwrap();
    let raw: String = store
        .conn
        .query_row(
            "SELECT payload FROM backup_unknown_records WHERE msg_id='unknown-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(raw.contains("preserved"));
}

#[test]
fn encrypted_import_reports_progress_without_leaking_records() {
    use std::sync::atomic::AtomicBool;
    let uin = "0123456789abcdef0123456789abcdef";
    let tar=archive(&[("303/ZaloDownloads/database/303_zthread.zdb", br#"{"userId":"202","isGroup":false}
"#), ("303/ZaloDownloads/database/303_zmessage.zdb", br#"{"msgId":"m1","toUid":"202","fromUid":"0","serverTime":1700000000000,"msgType":1,"message":"private text"}
"#)]);
    let key = Sha256::digest(uin.as_bytes());
    let iv = format!("zie{}", &uin[..13]);
    let mut buf = tar.clone();
    buf.resize(tar.len() + 16, 0);
    let len = cbc::Encryptor::<Aes256>::new_from_slices(&key, iv.as_bytes())
        .unwrap()
        .encrypt_padded_mut::<Pkcs7>(&mut buf, tar.len())
        .unwrap()
        .len();
    buf.truncate(len);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let stages = std::cell::RefCell::new(Vec::new());
    let report = zalo_rust::backup::import_encrypted_backup_with_progress(
        std::io::Cursor::new(buf),
        uin,
        "303",
        &mut store,
        &AtomicBool::new(false),
        &|s| stages.borrow_mut().push(s.to_owned()),
    )
    .unwrap();
    assert_eq!(report.messages_added, 1);
    assert!(stages.borrow().len() >= 3);
    assert!(!format!("{:?}", stages.borrow()).contains("private text"));
}

#[test]
fn desktop_photo_variant_seven_is_renderable_as_a_photo() {
    let record = serde_json::json!({"msgId":"photo-7","toUid":"202","fromUid":"0","serverTime":1700000000000_i64,"msgType":7,"message":{"oriUrl":"https://example.test/picture.jpg","thumbUrl":"https://example.test/thumb.jpg"}});
    let parsed = parse_desktop_record(&record, "303").unwrap();
    assert_eq!(parsed.msg_type, "chat.photo");
    assert!(parsed.raw_json.contains("Hình ảnh Zalo"));
}

#[test]
fn cancellation_after_message_writes_rolls_back_everything() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let uin = "0123456789abcdef0123456789abcdef";
    let tar=archive(&[("303/ZaloDownloads/database/303_zthread.zdb", br#"{"userId":"202","isGroup":false}
"#), ("303/ZaloDownloads/database/303_zmessage.zdb", br#"{"msgId":"m1","toUid":"202","fromUid":"0","serverTime":1700000000000,"msgType":1,"message":"hello"}
"#)]);
    let key = Sha256::digest(uin.as_bytes());
    let iv = format!("zie{}", &uin[..13]);
    let mut buf = tar.clone();
    buf.resize(tar.len() + 16, 0);
    let len = cbc::Encryptor::<Aes256>::new_from_slices(&key, iv.as_bytes())
        .unwrap()
        .encrypt_padded_mut::<Pkcs7>(&mut buf, tar.len())
        .unwrap()
        .len();
    buf.truncate(len);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let cancel = AtomicBool::new(false);
    let result = zalo_rust::backup::import_encrypted_backup_with_progress(
        std::io::Cursor::new(buf),
        uin,
        "303",
        &mut store,
        &cancel,
        &|phase| {
            if phase.contains("khôi phục") {
                cancel.store(true, Ordering::Relaxed)
            }
        },
    );
    assert!(result.is_err());
    assert!(store.all_threads().unwrap().is_empty());
}

#[test]
fn record_without_server_time_is_retained_without_inventing_a_timestamp() {
    let record = serde_json::json!({"msgId":"unknown-time","toUid":"202","fromUid":"0","serverTime":0,"msgType":999,"message":"notice"});
    let parsed = parse_desktop_record(&record, "303").unwrap();
    assert_eq!(parsed.ts, 0);
    assert_eq!(parsed.msg_type, "backup.unknown");
    assert!(parsed.raw_json.contains("notice"));
}

#[test]
fn concurrent_opens_can_upgrade_existing_history_safely() {
    use std::sync::{Arc, Barrier};
    let root=tempfile::tempdir().unwrap();
    for iteration in 0..6 {
        let path=root.path().join(format!("history-{iteration}.db"));
        { let db=rusqlite::Connection::open(&path).unwrap(); db.execute_batch("CREATE TABLE messages(msg_id TEXT PRIMARY KEY, thread_id TEXT NOT NULL, sender_id TEXT, sender_name TEXT, text TEXT, msg_type TEXT, ts INTEGER, is_outgoing INTEGER, raw_json TEXT); CREATE TABLE threads(thread_id TEXT PRIMARY KEY,name TEXT,is_group INTEGER,last_ts INTEGER); PRAGMA user_version=1;").unwrap(); }
        let gate=Arc::new(Barrier::new(12));
        let handles:Vec<_>=(0..12).map(|_|{ let path=path.clone();let gate=gate.clone();std::thread::spawn(move||{gate.wait();zalo_rust::store::MessageStore::open(path.to_str().unwrap()).is_ok()}) }).collect();
        assert!(handles.into_iter().all(|h|h.join().unwrap()));
    }
}

#[test]
fn duplicate_message_records_with_different_media_do_not_abort_import() {
    let a="https://example.test/a"; let b="https://example.test/b";
    let first=format!("303/ZaloDownloads/picture/202/101_303_202_{}.jxl",zalo_rust::crypto::md5_hex(a));
    let second=format!("303/ZaloDownloads/picture/202/101_303_202_{}.jxl",zalo_rust::crypto::md5_hex(b));
    let messages=format!("{{\"msgId\":\"m1\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":2,\"message\":{{\"oriUrl\":\"{a}\"}}}}\n{{\"msgId\":\"m1\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":2,\"message\":{{\"oriUrl\":\"{b}\"}}}}\n");
    let tar=archive(&[("303/ZaloDownloads/database/303_zthread.zdb",br#"{"userId":"202","isGroup":false}
"#),("303/ZaloDownloads/database/303_zmessage.zdb",messages.as_bytes()),(&first,b"a"),(&second,b"b")]);
    let mut store=zalo_rust::store::MessageStore::open(":memory:").unwrap();
    assert_eq!(zalo_rust::backup::import_tar_records(std::io::Cursor::new(tar),&mut store,"303").unwrap(),1);
}

#[test]
fn history_profile_rejects_a_different_account() {
    let mut store=zalo_rust::store::MessageStore::open(":memory:").unwrap();
    store.ensure_profile_owner("303").unwrap();
    assert!(store.profile_is_bound().unwrap());
    assert!(!store.profile_allows_account("404").unwrap());
    assert!(store.ensure_profile_owner("404").is_err());
    assert!(store.ensure_profile_owner("303").is_ok());
}

#[test]
fn existing_unbound_history_is_not_claimed_by_an_unverified_account() {
    let mut store=zalo_rust::store::MessageStore::open(":memory:").unwrap();
    store.upsert_message(&zalo_rust::store::StoredMessage { msg_id:"m1".into(),thread_id:"t1".into(),sender_id:"0".into(),sender_name:String::new(),text:"hello".into(),msg_type:"chat.text".into(),ts:1,is_outgoing:true,raw_json:String::new() }).unwrap();
    assert!(store.ensure_profile_owner("303").is_err());
}

#[test]
fn opening_up_to_date_store_does_not_require_a_writer_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let name = path.to_str().unwrap();
    let first = zalo_rust::store::MessageStore::open(name).unwrap();
    first.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let other = zalo_rust::store::MessageStore::open(name).unwrap();
    assert!(!other.profile_is_bound().unwrap());
    first.conn.execute_batch("ROLLBACK").unwrap();
}
#[tokio::test]
async fn bound_history_is_not_exposed_by_local_api_when_logged_out() {
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let name = path.to_str().unwrap();
    let mut store = zalo_rust::store::MessageStore::open(name).unwrap();
    store.ensure_profile_owner("303").unwrap();
    let session = Arc::new(tokio::sync::Mutex::new(zalo_rust::session::Session::new(false, false, false).unwrap()));
    let (status, _) = zalo_rust::api_mcp::handle_rest_request(&session, "GET", "/api/v1/threads", "", name).await;
    assert_eq!(status, 403);
}
#[test]
fn automatic_recovery_selects_only_a_verified_candidate() {
    let key = "0123456789abcdef0123456789abcdef";
    let header = fixture(key);
    let response = serde_json::json!({"result":{"result":{"value":["ffffffffffffffffffffffffffffffff",key]}}});
    let selected = zalo_rust::backup_recovery::verified_candidate(&response, &header).unwrap();
    assert_eq!(&*selected, key);
    let wrong = serde_json::json!({"result":{"result":{"value":["ffffffffffffffffffffffffffffffff"]}}});
    assert!(zalo_rust::backup_recovery::verified_candidate(&wrong, &header).is_none());
}
#[test]
fn colliding_existing_message_is_preserved_and_backup_message_is_idempotent() {
    let conversations = b"{\"userId\":\"202\",\"isGroup\":false}\n";
    let message = b"{\"msgId\":\"same-id\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":1,\"message\":\"backup\"}\n";
    let tar = archive(&[("303/ZaloDownloads/database/303_zthread.zdb", conversations), ("303/ZaloDownloads/database/303_zmessage.zdb", message)]);
    let mut store = zalo_rust::store::MessageStore::open(":memory:").unwrap();
    store.ensure_profile_owner("303").unwrap();
    store.upsert_message(&zalo_rust::store::StoredMessage { msg_id: "same-id".into(), thread_id: "other".into(), sender_id: "0".into(), sender_name: String::new(), text: "existing".into(), msg_type: "chat.text".into(), ts: 1, is_outgoing: true, raw_json: String::new() }).unwrap();
    assert_eq!(zalo_rust::backup::import_tar_records(std::io::Cursor::new(&tar), &mut store, "303").unwrap(), 1);
    assert_eq!(zalo_rust::backup::import_tar_records(std::io::Cursor::new(&tar), &mut store, "303").unwrap(), 0);
    assert_eq!(store.messages_for("other", 10).unwrap()[0].text, "existing");
    store.upsert_message(&zalo_rust::store::StoredMessage { msg_id: "same-id".into(), thread_id: "202".into(), sender_id: "0".into(), sender_name: String::new(), text: "must not overwrite".into(), msg_type: "chat.text".into(), ts: 1, is_outgoing: true, raw_json: "{}".into() }).unwrap();
    assert_eq!(store.messages_for("other", 10).unwrap()[0].text, "existing");
    let imported = store.messages_for("202", 10).unwrap();
    assert_eq!(imported.len(), 1);
    assert_ne!(imported[0].msg_id, "same-id");
    assert!(imported[0].raw_json.contains("same-id"));
    let mut threads = Vec::new();
    store.hydrate_threads(&mut threads, 20).unwrap();
    let restored = threads.iter().find(|thread| thread.id == "202").unwrap().messages.iter().find(|m| m.id == imported[0].msg_id).unwrap();
    assert_eq!(restored.wire_ref.server_id.as_ref().and_then(|v| v.as_str()), Some("same-id"));
    let persisted = zalo_rust::store::StoredMessage::from(restored);
    assert!(persisted.raw_json.contains("same-id"));
    store.conn.execute("DELETE FROM messages WHERE msg_id='same-id'", []).unwrap();
    assert_eq!(zalo_rust::backup::import_tar_records(std::io::Cursor::new(&tar), &mut store, "303").unwrap(), 0);
}
#[test]
fn archive_import_recovers_group_prefixed_jxl() {
    let url="https://example.test/image";
    let name=format!("303/ZaloDownloads/picture/g202/101_303_g202_{}.jxl",zalo_rust::crypto::md5_hex(url));
    let message=format!("{{\"msgId\":\"m1\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":2,\"message\":{{\"oriUrl\":\"{url}\"}}}}\n");
    let tar=archive(&[("303/ZaloDownloads/database/303_zthread.zdb",b"{\"userId\":\"202\",\"isGroup\":true}\n"),("303/ZaloDownloads/database/303_zmessage.zdb",message.as_bytes()),(&name,b"image")]);
    let mut store=zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let report=zalo_rust::backup::import_tar_records_report(std::io::Cursor::new(tar),&mut store,"303").unwrap();
    assert_eq!(report.media_added,1);
    assert_eq!(report.missing_media,0);
}
#[test]
fn group_photo_does_not_select_direct_chat_variant() {
    let url="https://example.test/image";
    let name=format!("303/ZaloDownloads/picture/202/101_303_202_{}.jxl",zalo_rust::crypto::md5_hex(url));
    let message=format!("{{\"msgId\":\"m1\",\"cliMsgId\":\"101\",\"toUid\":\"202\",\"fromUid\":\"0\",\"serverTime\":1700000000000,\"msgType\":2,\"message\":{{\"oriUrl\":\"{url}\"}}}}\n");
    let tar=archive(&[("303/ZaloDownloads/database/303_zthread.zdb",b"{\"userId\":\"202\",\"isGroup\":true}\n"),("303/ZaloDownloads/database/303_zmessage.zdb",message.as_bytes()),(&name,b"image")]);
    let mut store=zalo_rust::store::MessageStore::open(":memory:").unwrap();
    let report=zalo_rust::backup::import_tar_records_report(std::io::Cursor::new(tar),&mut store,"303").unwrap();
    assert_eq!(report.media_added,0);
    assert_eq!(report.missing_media,1);
}