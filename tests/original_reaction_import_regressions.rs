//! Explicit cache adoption uses synthetic files only and never discovers original-app data.
use serde_json::{json, Value};
use std::path::PathBuf;
use zalo_rust::{
    chat::{ChatMessage, Thread},
    original_reactions::{import_selected_cache, load_imported_cache, ImportError},
};

static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "zl-original-import-{}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn write(&self, value: &Value) -> PathBuf {
        let path = self.0.join("explicit-selection.json");
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn envelope(captured: i64) -> Value {
    json!({"schema":1,"account_id":"account-fixture","source":"zalo-macos-local-cache","captured_at_ms":captured,
        "snapshots":[{"idTo":"peer-fixture","rClientMsgId":"client-fixture","rMsgId":"message-fixture","reactions":{"reactor-fixture":{"5":2}}}]})
}
fn loaded() -> Vec<Thread> {
    vec![Thread {
        id: "peer-fixture".into(),
        messages: vec![ChatMessage {
            id: "message-fixture".into(),
            client_message_id: "client-fixture".into(),
            ..Default::default()
        }],
        ..Default::default()
    }]
}
fn managed_path(root: &std::path::Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    root.join("accounts")
        .join(format!("{:x}", Sha256::digest(b"account-fixture")))
        .join("original-reactions-v1.json")
}

#[test]
fn explicit_import_retains_private_validated_copy_for_late_hydrate_without_input_discovery() {
    let fixture = Fixture::new();
    let managed = fixture.0.join("managed");
    let path = fixture.write(&envelope(1700000000000));
    let before = std::fs::read(&path).unwrap();
    assert!(load_imported_cache(&managed, "account-fixture")
        .unwrap()
        .is_none());
    import_selected_cache(&managed, &path, "account-fixture").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(managed_path(&managed)).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(managed_path(&managed))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(managed_path(&managed).parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    std::fs::remove_file(path).unwrap();
    let cache = load_imported_cache(&managed, "account-fixture")
        .unwrap()
        .unwrap();
    let mut messages = loaded();
    assert_eq!(cache.apply_to_threads(&mut messages).messages_updated, 1);
    assert_eq!(messages[0].messages[0].reaction_summaries[0].count, 2);
    assert!(load_imported_cache(&managed, "another-account")
        .unwrap()
        .is_none());
}

#[test]
fn invalid_account_provenance_or_input_never_creates_managed_copy() {
    let fixture = Fixture::new();
    let managed = fixture.0.join("managed");
    for bad in [
        json!({"private":"value-not-to-log"}),
        {
            let mut v = envelope(1700000000000);
            v["account_id"] = json!("other-account");
            v
        },
        {
            let mut v = envelope(1700000000000);
            v["source"] = json!("unknown-source");
            v
        },
        {
            let mut v = envelope(1700000000000);
            v["schema"] = json!(2);
            v
        },
    ] {
        let path = fixture.write(&bad);
        assert_eq!(
            import_selected_cache(&managed, &path, "account-fixture").unwrap_err(),
            ImportError::InvalidCache
        );
        assert!(!managed.exists());
    }
    assert_eq!(
        import_selected_cache(&managed, &fixture.0.join("missing.json"), "account-fixture")
            .unwrap_err(),
        ImportError::MissingFile
    );
    assert!(!managed.exists());
}

#[test]
fn existing_owner_mismatch_or_newer_capture_is_not_silently_overwritten() {
    let fixture = Fixture::new();
    let managed = fixture.0.join("managed");
    let path = fixture.write(&envelope(1700000000010));
    import_selected_cache(&managed, &path, "account-fixture").unwrap();
    let previous = std::fs::read(managed_path(&managed)).unwrap();
    let older = fixture.write(&envelope(1700000000000));
    assert_eq!(
        import_selected_cache(&managed, &older, "account-fixture").unwrap_err(),
        ImportError::ExistingCacheConflict
    );
    assert_eq!(std::fs::read(managed_path(&managed)).unwrap(), previous);
    let mut wrong_owner = envelope(1700000000010);
    wrong_owner["account_id"] = json!("another-account");
    let wrong = serde_json::to_vec(&wrong_owner).unwrap();
    std::fs::write(managed_path(&managed), &wrong).unwrap();
    let newer = fixture.write(&envelope(1700000000020));
    assert_eq!(
        import_selected_cache(&managed, &newer, "account-fixture").unwrap_err(),
        ImportError::ExistingCacheConflict
    );
    assert_eq!(std::fs::read(managed_path(&managed)).unwrap(), wrong);
}

#[test]
fn same_capture_conflicts_reject_but_exact_reimport_is_idempotent() {
    let fixture = Fixture::new();
    let managed = fixture.0.join("managed");
    let value = envelope(1700000000000);
    let path = fixture.write(&value);
    import_selected_cache(&managed, &path, "account-fixture").unwrap();
    let previous = std::fs::read(managed_path(&managed)).unwrap();
    import_selected_cache(&managed, &path, "account-fixture").unwrap();
    let mut conflict = value;
    conflict["snapshots"][0]["reactions"]["reactor-fixture"]["5"] = json!(3);
    let path = fixture.write(&conflict);
    assert_eq!(
        import_selected_cache(&managed, &path, "account-fixture").unwrap_err(),
        ImportError::ExistingCacheConflict
    );
    assert_eq!(std::fs::read(managed_path(&managed)).unwrap(), previous);
}

#[test]
fn current_live_clear_and_nonreaction_fields_survive_delayed_import_result() {
    let fixture = Fixture::new();
    let managed = fixture.0.join("managed");
    let path = fixture.write(&envelope(1700000000000));
    let cache = import_selected_cache(&managed, &path, "account-fixture").unwrap();
    let mut messages = loaded();
    let message = &mut messages[0].messages[0];
    message.text = "Current text".into();
    message.sender = "Current sender".into();
    message.ts = 1700000000099;
    let event = zalo_rust::reactions::ReactionEvent {
        sender_id: "reactor-fixture".into(),
        event_id: "new-add".into(),
        r_type: 5,
        emoji: "❤️".into(),
        ts: 1700000000010,
        ..Default::default()
    };
    message.apply_reaction(&event);
    message.apply_reaction(&zalo_rust::reactions::ReactionEvent {
        event_id: "new-clear".into(),
        r_type: -1,
        ts: 1700000000020,
        ..event
    });
    let expected = serde_json::to_value(&messages).unwrap();
    let summary = cache.apply_to_threads(&mut messages);
    assert_eq!(summary.messages_protected, 1);
    assert_eq!(summary.messages_updated, 0);
    assert_eq!(serde_json::to_value(&messages).unwrap(), expected);
}

#[test]
fn adopted_artifact_retains_only_validated_fields_not_unrelated_export_metadata() {
    let fixture = Fixture::new();
    let managed = fixture.0.join("managed");
    let mut value = envelope(1700000000000);
    value["unrelated_private_field"] = json!("fixture-private-value");
    value["snapshots"][0]["unrelated_row"] = json!("fixture-row-value");
    let path = fixture.write(&value);
    let before = std::fs::read(&path).unwrap();
    import_selected_cache(&managed, &path, "account-fixture").unwrap();
    let adopted: Value =
        serde_json::from_slice(&std::fs::read(managed_path(&managed)).unwrap()).unwrap();
    assert!(adopted.get("unrelated_private_field").is_none());
    assert!(adopted["snapshots"][0].get("unrelated_row").is_none());
    assert_eq!(
        adopted["snapshots"][0]["reactions"]["reactor-fixture"]["5"],
        2
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
}
