use std::collections::BTreeMap;

use serde_json::json;
use zalo_rust::chat::{ChatMessage, Thread};
use zalo_rust::message_model::{DeliveryState, MessageKind, UnreadPolicy, WireMessageRef};
use zalo_rust::reactions::{
    ReactionEvent, ReactionSnapshotProvenance, ReactionSummary, ReactionVersion,
};
use zalo_rust::store::{persist_threads, MessageStore, StoredMessage};

fn message(id: &str, client: &str) -> ChatMessage {
    ChatMessage {
        id: id.into(),
        client_message_id: client.into(),
        sender: "sender-a".into(),
        text: "Repeated text".into(),
        time: zalo_rust::clock::message_time(1_793_425_680_000),
        ts: 1_793_425_680_000,
        is_outgoing: true,
        ..Default::default()
    }
}

fn thread(id: &str, messages: Vec<ChatMessage>) -> Thread {
    Thread {
        id: id.into(),
        name: "Same name".into(),
        messages,
        ..Default::default()
    }
}

fn metadata_message() -> ChatMessage {
    let mut message = message("server-a", "client-a");
    let raw = json!({"msgId":"server-a","cliMsgId":"client-a","uidFrom":"sender-a","idTo":"contact-a","msgType":"chat.photo","cmd":501,"st":0,"at":null,"ts":"1793425680000","content":{"attachments":[{"future":true}]},"paramsExt":{"countUnread":0,"opaque":null}});
    message.kind = MessageKind::Photo;
    message.wire_ref = WireMessageRef::from_raw(&raw);
    message.unread_policy = UnreadPolicy::from_raw(&raw);
    message.delivery = DeliveryState::Delivered;
    message.history_source = Some(json!({"opaque":[1,2],"cursor":"fixture-cursor"}));
    message.sticker_id = Some(1);
    message.sticker_cate_id = Some(2);
    message.sticker_type = Some(7);
    message.reply_to_id = Some("quoted-a".into());
    message.reply_to_sender = Some("sender-b".into());
    message.reply_to_text = Some("Quoted text".into());
    message.link_url = Some("https://example.invalid/asset".into());
    message.link_title = Some("Title".into());
    message.link_desc = Some("Description".into());
    message.link_thumb = Some("https://example.invalid/thumb".into());
    message.link_src = Some("Fixture".into());
    message.is_pinned = true;
    message.is_starred = true;
    message.reactions = vec!["❤️".into()];
    message.reaction_summaries = vec![ReactionSummary {
        emoji: "❤️".into(),
        count: 2,
        reactor_ids: vec!["reactor-a".into()],
        r_type: Some(5),
        count_is_lower_bound: true,
        reactor_counts: BTreeMap::from([("reactor-a".into(), 2)]),
    }];
    message.reaction_snapshot = true;
    message.reaction_authoritative_snapshot = true;
    message.reaction_snapshot_fresh = true;
    message.reaction_snapshot_provenance = ReactionSnapshotProvenance {
        source: "fixture-history".into(),
        captured_at_ms: message.ts,
    };
    message.reaction_versions.insert(
        "reactor-a".into(),
        ReactionVersion {
            ts: message.ts,
            event_id: "event-a".into(),
            client_event_id: "client-event-a".into(),
            events: vec![ReactionEvent {
                message_id: "server-a".into(),
                client_message_id: "client-a".into(),
                thread_id: "contact-a".into(),
                sender_id: "reactor-a".into(),
                event_id: "event-a".into(),
                client_event_id: "client-event-a".into(),
                r_type: 5,
                emoji: "❤️".into(),
                ts: message.ts,
                historical: false,
            }],
            covered_events: vec![ReactionEvent {
                event_id: "covered-a".into(),
                ..Default::default()
            }],
            baseline: BTreeMap::from([(5, 2)]),
            ledger_initialized: true,
            snapshot_baseline: true,
        },
    );
    message
        .resolved_mention_uids
        .insert("Mention name".into(), "mentioned-a".into());
    message
}

#[test]
fn typed_metadata_roundtrips_without_losing_wire_values_or_replay_ledgers() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let mut original = metadata_message();
    persist_threads(&mut store, &[thread("contact-a", vec![original.clone()])]).unwrap();
    let mut hydrated = Vec::new();
    assert_eq!(store.hydrate_threads(&mut hydrated, 20).unwrap(), 1);
    original.reaction_snapshot_fresh = false;
    assert_eq!(
        serde_json::to_value(&hydrated[0].messages[0]).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert!(!hydrated[0].messages[0].reaction_snapshot_fresh);
    assert_eq!(hydrated[0].messages[0].wire_ref.at, Some(json!(null)));
    assert!(!hydrated[0].messages[0].unread_policy.counts_unread());
    assert_eq!(store.hydrate_threads(&mut hydrated, 20).unwrap(), 0); // (INV-4)
    assert_eq!(hydrated[0].messages.len(), 1);
}

#[test]
fn hydration_enriches_an_existing_exact_message() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let original = metadata_message();
    persist_threads(&mut store, &[thread("contact-a", vec![original.clone()])]).unwrap();
    let mut hydrated = vec![thread("contact-a", vec![message("server-a", "")])];
    assert_eq!(store.hydrate_threads(&mut hydrated, 20).unwrap(), 0);
    let actual = &hydrated[0].messages[0];
    assert_eq!(actual.wire_ref, original.wire_ref);
    assert_eq!(actual.delivery, original.delivery);
    assert_eq!(actual.client_message_id, "client-a");
    assert_eq!(actual.reaction_versions, original.reaction_versions);
    assert_eq!(actual.resolved_mention_uids, original.resolved_mention_uids);
}

#[test]
fn hydration_does_not_match_distinct_messages_by_text_or_clock() {
    let mut store = MessageStore::open(":memory:").unwrap();
    persist_threads(
        &mut store,
        &[thread("contact-a", vec![message("server-a", "client-a")])],
    )
    .unwrap();
    let mut hydrated = vec![thread("contact-a", vec![message("server-b", "client-b")])];
    assert_eq!(store.hydrate_threads(&mut hydrated, 20).unwrap(), 1);
    assert_eq!(hydrated[0].messages.len(), 2);
}

#[test]
fn hydration_reconciles_pending_only_with_an_exact_client_reference() {
    for (pending_client, expected) in [("client-a", 1), ("different-client", 2), ("", 2)] {
        let mut store = MessageStore::open(":memory:").unwrap();
        persist_threads(
            &mut store,
            &[thread("contact-a", vec![message("server-a", "client-a")])],
        )
        .unwrap();
        let mut hydrated = vec![thread(
            "contact-a",
            vec![message("out_pending", pending_client)],
        )];
        store.hydrate_threads(&mut hydrated, 20).unwrap();
        assert_eq!(hydrated[0].messages.len(), expected);
        assert!(hydrated[0]
            .messages
            .iter()
            .any(|message| message.id == "server-a"));
    }
}

#[test]
fn hydration_rejects_ambiguous_pending_client_collisions() {
    let mut store = MessageStore::open(":memory:").unwrap();
    persist_threads(
        &mut store,
        &[thread("contact-a", vec![message("server-a", "client-a")])],
    )
    .unwrap();
    let mut hydrated = vec![thread(
        "contact-a",
        vec![message("out_a", "client-a"), message("out_b", "client-a")],
    )];
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    assert_eq!(hydrated[0].messages.len(), 3);
}

#[test]
fn hydration_keeps_same_name_threads_and_database_ownership_separate() {
    let mut store = MessageStore::open(":memory:").unwrap();
    persist_threads(
        &mut store,
        &[thread(
            "stored-contact",
            vec![message("server-a", "client-a")],
        )],
    )
    .unwrap();
    let mut live = thread("live-contact", vec![message("server-b", "client-b")]);
    live.avatar = Some("https://example.invalid/same-avatar".into());
    let mut hydrated = vec![live];
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    assert_eq!(hydrated.len(), 2);
    assert_eq!(store.messages_for("stored-contact", 20).unwrap().len(), 1);
    assert!(store.messages_for("live-contact", 20).unwrap().is_empty());
    assert!(store
        .all_threads()
        .unwrap()
        .iter()
        .any(|thread| thread.thread_id == "stored-contact"));
}

#[test]
fn hydration_keeps_live_threads_separate_when_names_and_avatars_match() {
    let store = MessageStore::open(":memory:").unwrap();
    let mut threads = vec![
        thread("contact-a", Vec::new()),
        thread("contact-b", Vec::new()),
    ];
    for thread in &mut threads {
        thread.avatar = Some("https://example.invalid/same-avatar".into());
    }
    store.hydrate_threads(&mut threads, 20).unwrap();
    assert_eq!(threads.len(), 2);
}

#[test]
fn malformed_metadata_defaults_only_the_affected_field() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let mut stored = StoredMessage::from(&message("server-a", ""));
    stored.thread_id = "contact-a".into();
    stored.raw_json = json!({"wire_ref":42,"delivery":false,"reaction_versions":[],"kind":"photo","client_message_id":"client-a","history_source":null,"resolved_mention_uids":{"Name":"mention-a"},"reaction_snapshot_fresh":true}).to_string();
    store.upsert_message(&stored).unwrap();
    let mut hydrated = vec![thread("contact-a", Vec::new())];
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    let actual = &hydrated[0].messages[0];
    assert_eq!(actual.kind, MessageKind::Photo);
    assert_eq!(actual.wire_ref, WireMessageRef::default());
    assert_eq!(actual.delivery, DeliveryState::default());
    assert!(actual.reaction_versions.is_empty());
    assert_eq!(actual.client_message_id, "client-a");
    assert_eq!(actual.history_source, Some(json!(null)));
    assert_eq!(
        actual.resolved_mention_uids.get("Name").map(String::as_str),
        Some("mention-a")
    );
    assert!(!actual.reaction_snapshot_fresh);
}

#[test]
fn legacy_sticker_metadata_and_invalid_json_keep_core_columns() {
    for raw in [
        r#"{"id":11,"catId":22,"type":7,"reactions":["👍"],"future":{"opaque":true}}"#,
        "{invalid",
        "[]",
        "",
    ] {
        let mut store = MessageStore::open(":memory:").unwrap();
        let mut stored = StoredMessage::from(&message("legacy-id", ""));
        stored.thread_id = "contact-a".into();
        stored.raw_json = raw.into();
        store.upsert_message(&stored).unwrap();
        let mut hydrated = vec![thread("contact-a", Vec::new())];
        store.hydrate_threads(&mut hydrated, 20).unwrap();
        let actual = &hydrated[0].messages[0];
        assert_eq!(actual.id, "legacy-id");
        assert_eq!(actual.text, "Repeated text");
        assert_eq!(actual.kind, MessageKind::Unknown);
        if raw.starts_with("{\"id\"") {
            assert_eq!(
                (
                    actual.sticker_id,
                    actual.sticker_cate_id,
                    actual.sticker_type
                ),
                (Some(11), Some(22), Some(7))
            );
            assert_eq!(actual.reactions, ["👍"]);
        }
    }
}

#[test]
fn bounded_hydration_keeps_the_newest_messages_and_schema_version() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let messages = (1..=30)
        .map(|ts| ChatMessage {
            ts,
            ..message(&format!("server-{ts}"), "")
        })
        .collect();
    persist_threads(&mut store, &[thread("contact-a", messages)]).unwrap();
    let version_before: i64 = store
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    let mut hydrated = Vec::new();
    store.hydrate_threads(&mut hydrated, 3).unwrap();
    assert_eq!(
        hydrated[0]
            .messages
            .iter()
            .map(|message| message.ts)
            .collect::<Vec<_>>(),
        [28, 29, 30]
    );
    assert_eq!(
        store
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        version_before
    );
    assert_eq!(version_before, 1);
}

#[test]
fn hydration_keeps_delivery_progress_and_does_not_replay_stale_reactions() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let cached = metadata_message();
    persist_threads(&mut store, &[thread("contact-a", vec![cached.clone()])]).unwrap();
    let mut live = cached;
    live.delivery = DeliveryState::Read;
    live.apply_reaction(&ReactionEvent {
        message_id: "server-a".into(),
        client_message_id: "client-a".into(),
        sender_id: "reactor-a".into(),
        event_id: "clear-newer".into(),
        client_event_id: "clear-client-newer".into(),
        r_type: -1,
        ts: live.ts + 10_000,
        ..Default::default()
    });
    assert!(live.reaction_summaries.is_empty());
    let mut hydrated = vec![thread("contact-a", vec![live])];
    for _ in 0..2 {
        store.hydrate_threads(&mut hydrated, 20).unwrap();
        let actual = &hydrated[0].messages[0];
        assert_eq!(actual.delivery, DeliveryState::Read);
        assert!(actual.reaction_summaries.is_empty());
        assert!(actual.reactions.is_empty());
        assert!(actual.reaction_versions["reactor-a"]
            .events
            .iter()
            .any(|event| event.event_id == "clear-newer"));
    }
}

#[test]
fn hydration_rejects_conflicting_wire_ids_directions_and_senders() {
    for scenario in ["wire-server", "direction", "sender"] {
        let mut store = MessageStore::open(":memory:").unwrap();
        let mut stored = message("legacy-alias", "client-a");
        stored.wire_ref = WireMessageRef::from_raw(
            &json!({"globalMsgId":"server-a","cliMsgId":"legacy-alias","uidFrom":"sender-a"}),
        );
        stored.client_message_id = "legacy-alias".into();
        persist_threads(&mut store, &[thread("contact-a", vec![stored.clone()])]).unwrap();
        let mut live = stored;
        match scenario {
            "wire-server" => {
                live.wire_ref = WireMessageRef::from_raw(
                    &json!({"globalMsgId":"server-b","cliMsgId":"legacy-alias","uidFrom":"sender-a"}),
                )
            }
            "direction" => live.is_outgoing = false,
            _ => {
                live.wire_ref = WireMessageRef::from_raw(
                    &json!({"globalMsgId":"server-a","cliMsgId":"legacy-alias","uidFrom":"sender-b"}),
                )
            }
        }
        let mut hydrated = vec![thread("contact-a", vec![live])];
        store.hydrate_threads(&mut hydrated, 20).unwrap();
        assert_eq!(hydrated[0].messages.len(), 2, "Scenario: {scenario}");
    }
}

#[test]
fn hydration_retains_legacy_canonical_id_without_inventing_client_metadata() {
    let mut store = MessageStore::open(":memory:").unwrap();
    persist_threads(
        &mut store,
        &[thread("contact-a", vec![message("client-legacy", "")])],
    )
    .unwrap();
    let mut live = message("client-legacy", "client-legacy");
    live.wire_ref = WireMessageRef::from_raw(
        &json!({"globalMsgId":"server-a","cliMsgId":"client-legacy","uidFrom":"sender-a"}),
    );
    let mut hydrated = vec![thread("contact-a", vec![live.clone()])];
    assert_eq!(store.hydrate_threads(&mut hydrated, 20).unwrap(), 0);
    assert_eq!(hydrated[0].messages.len(), 1);
    assert_eq!(hydrated[0].messages[0].id, "client-legacy");
    assert_eq!(hydrated[0].messages[0].wire_ref, live.wire_ref);
    assert_eq!(
        store.messages_for("contact-a", 20).unwrap()[0].msg_id,
        "client-legacy"
    );
}

#[test]
fn hydration_enriches_missing_metadata_without_rewinding_live_content_or_clock() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let cached = metadata_message();
    persist_threads(&mut store, &[thread("contact-a", vec![cached])]).unwrap();
    let mut live = message("server-a", "client-a");
    live.kind = MessageKind::Video;
    live.sender = "Live sender".into();
    live.text = "New live content".into();
    live.ts += 30_000;
    live.time = "live-clock".into();
    live.wire_ref = WireMessageRef::from_raw(
        &json!({"msgId":"server-a","cliMsgId":"client-a","uidFrom":"sender-a","idTo":"contact-a","msgType":"chat.video","content":"New live content","ts":live.ts,"paramsExt":{"countUnread":1}}),
    );
    live.unread_policy = UnreadPolicy::from_raw(&json!({"paramsExt":{"countUnread":1}}));
    live.history_source = Some(json!({"cursor":"live-cursor"}));
    live.resolved_mention_uids
        .insert("Mention name".into(), "live-mentioned-a".into());
    let mut hydrated = vec![thread("contact-a", vec![live.clone()])];
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    let actual = &hydrated[0].messages[0];
    assert_eq!((actual.ts, &actual.time), (live.ts, &live.time));
    assert_eq!((&actual.text, &actual.sender), (&live.text, &live.sender));
    assert_eq!(actual.kind, MessageKind::Video);
    assert_eq!(
        actual.wire_ref.fields.get("content"),
        Some(&json!("New live content"))
    );
    assert_eq!(actual.wire_ref.ts, Some(json!(live.ts)));
    assert_eq!(actual.wire_ref.at, Some(json!(null)));
    assert_eq!(actual.unread_policy, live.unread_policy);
    assert_eq!(actual.history_source, live.history_source);
    assert_eq!(actual.resolved_mention_uids, live.resolved_mention_uids);
    assert_eq!(
        actual.link_url.as_deref(),
        Some("https://example.invalid/asset")
    );
}

#[test]
fn hydration_enriches_missing_text_and_clock_without_network_recall_side_effects() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let cached = message("server-a", "client-a");
    persist_threads(&mut store, &[thread("contact-a", vec![cached.clone()])]).unwrap();
    let mut blank = message("server-a", "client-a");
    blank.text.clear();
    blank.ts = 0;
    blank.time.clear();
    let mut hydrated = vec![thread("contact-a", vec![blank])];
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    assert_eq!(hydrated[0].messages[0].text, cached.text);
    assert_eq!(hydrated[0].messages[0].ts, cached.ts);

    let mut recalled_cache = cached;
    recalled_cache.text = "[Tin nhắn đã thu hồi]".into();
    persist_threads(&mut store, &[thread("contact-a", vec![recalled_cache])]).unwrap();
    hydrated[0].messages[0].link_url = Some("https://example.invalid/live-link".into());
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    assert_eq!(hydrated[0].messages[0].text, "Repeated text");
    assert_eq!(
        hydrated[0].messages[0].link_url.as_deref(),
        Some("https://example.invalid/live-link")
    );
}

#[test]
fn hydration_preserves_known_typed_wire_slots_across_alias_field_names() {
    let mut store = MessageStore::open(":memory:").unwrap();
    let mut cached = message("server-a", "cached-client");
    cached.wire_ref = WireMessageRef::from_raw(
        &json!({"msgId":"server-a","cliMsgId":"cached-client","uidFrom":"sender-a","msgType":"chat.photo","at":null}),
    );
    persist_threads(&mut store, &[thread("contact-a", vec![cached])]).unwrap();
    let mut live = message("server-a", "live-client");
    live.wire_ref = WireMessageRef::from_raw(
        &json!({"globalMsgId":"server-a","clientId":"live-client","senderId":"sender-a","messageType":"chat.video"}),
    );
    let mut hydrated = vec![thread("contact-a", vec![live.clone()])];
    store.hydrate_threads(&mut hydrated, 20).unwrap();
    let actual = &hydrated[0].messages[0];
    assert_eq!(actual.wire_ref.client_id, live.wire_ref.client_id);
    assert_eq!(actual.wire_ref.original_type, live.wire_ref.original_type);
    assert_eq!(actual.wire_ref.sender, live.wire_ref.sender);
    assert_eq!(actual.wire_ref.at, Some(json!(null)));
    assert_eq!(
        actual.wire_ref.fields.get("cliMsgId"),
        Some(&json!("cached-client"))
    );
    assert_eq!(
        actual.wire_ref.fields.get("clientId"),
        Some(&json!("live-client"))
    );
}

#[test]
fn hydration_never_restores_content_metadata_onto_live_or_legacy_recall() {
    for legacy_text in [false, true] {
        for sticker in [false, true] {
            let mut store = MessageStore::open(":memory:").unwrap();
            let mut cached = metadata_message();
            if !sticker {
                cached.sticker_id = None;
                cached.sticker_cate_id = None;
                cached.sticker_type = None;
            }
            persist_threads(&mut store, &[thread("contact-a", vec![cached])]).unwrap();
            let mut recalled = message("server-a", "client-a");
            recalled.kind = if legacy_text {
                MessageKind::Unknown
            } else {
                MessageKind::Recall
            };
            recalled.text = "[Tin nhắn đã thu hồi]".into();
            recalled.wire_ref = WireMessageRef::from_raw(
                &json!({"msgId":"server-a","cliMsgId":"client-a","uidFrom":"sender-a","msgType":"chat.undo"}),
            );
            let mut hydrated = vec![thread("contact-a", vec![recalled.clone()])];
            for _ in 0..2 {
                store.hydrate_threads(&mut hydrated, 20).unwrap();
                let actual = &hydrated[0].messages[0];
                assert_eq!(actual.text, recalled.text);
                assert_eq!(actual.kind, recalled.kind);
                assert_eq!(actual.sticker_id, None);
                assert_eq!(actual.sticker_cate_id, None);
                assert_eq!(actual.sticker_type, None);
                assert_eq!(actual.link_url, None);
                assert_eq!(actual.link_title, None);
                assert_eq!(actual.link_desc, None);
                assert_eq!(actual.link_thumb, None);
                assert_eq!(actual.link_src, None);
                assert_eq!(actual.reply_to_id, None);
                assert_eq!(actual.reply_to_sender, None);
                assert_eq!(actual.reply_to_text, None);
                assert_eq!(actual.history_source, None);
                assert!(actual.resolved_mention_uids.is_empty());
                assert!(!actual.wire_ref.fields.contains_key("content"));
                assert_eq!(actual.wire_ref.original_type, Some(json!("chat.undo")));
            }
        }
    }
}
