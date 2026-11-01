//! Copy-on-write legacy import, account-owned storage and bounded UI hydration.
use super::*;
use std::path::Path;
pub(super) fn initialize_legacy_owner(account: Option<&str>) -> anyhow::Result<()> {
    let path = Path::new("legacy-owner.json");
    if !path.exists() {
        crate::data_dir::write_private_atomic(
            path,
            &serde_json::to_vec(&serde_json::json!({"account":account}))?,
        )?;
    }
    Ok(())
}
pub(super) fn open_account_store(account: &str) -> anyhow::Result<crate::store_v2::AccountStore> {
    let mut store = crate::store_v2::AccountStore::open(Path::new("."), account)?;
    let owner = std::fs::read("legacy-owner.json")
        .ok()
        .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
    if owner.as_ref().and_then(|v| v["account"].as_str()) == Some(account) {
        let db = Path::new("zalo_history.db");
        let json = Path::new("threads_cache.json");
        if db.is_file() || json.is_file() {
            store.migrate_legacy(db.is_file().then_some(db), json.is_file().then_some(json))?;
        }
    }
    Ok(store)
}
pub(super) fn hydrate_model(message: &mut ChatMessage) {
    use crate::message_model::MessageKind as K;
    if let Some(source) = message.history_source.as_ref() {
        message.kind = source["msgType"]
            .as_i64()
            .map(K::from_archive_code)
            .unwrap_or_default();
        // Archive IDs are not interchangeable with server receipt IDs.
        message.wire_ref = Default::default();
    } else if message.kind == K::Unknown && message.wire_ref.original_type.is_none() {
        message.kind = if message.sticker_id.is_some() {
            K::Sticker
        } else if message.link_src.as_deref() == Some("photo") {
            K::Photo
        } else if message.link_src.as_deref() == Some("video") {
            K::Video
        } else {
            K::Text
        };
    }
    message.ts = crate::clock::normalize_ts_ms(message.ts);
    if message.ts > 0 {
        message.time = crate::clock::message_time(message.ts);
    }
    if message.history_source.is_none() || message.kind != K::Unknown {
        message.hydrate_media_metadata();
    }
}
pub(super) fn load_page(
    account: &str,
    thread: &str,
    cursor: Option<&crate::store_v2::HistoryCursor>,
    direction: crate::store_v2::PageDirection,
) -> anyhow::Result<(Vec<ChatMessage>, bool)> {
    let store = open_account_store(account)?;
    let page = store.history_page(thread, cursor, direction)?;
    let mut messages = page
        .messages
        .iter()
        .map(|m| store.chat_message(m))
        .collect::<anyhow::Result<Vec<_>>>()?;
    for message in &mut messages {
        hydrate_model(message);
    }
    let mut threads = vec![Thread { presentation: Default::default(),
        id: thread.into(),
        name: String::new(),
        preview: String::new(),
        time: String::new(),
        avatar: None,
        is_group: thread.starts_with('g'),
        messages,
        unread: Default::default(),
    }];
    if let Ok(mut journal) = crate::reaction_journal::ReactionJournal::open(Path::new("."), account)
    {
        journal.apply_loaded(&mut threads)?;
    }
    let messages = threads.pop().unwrap().messages;
    Ok((messages, page.has_more))
}
impl ZaloGuiApp {
    pub(super) fn retain_message_window(&mut self) {
        const MAX_UI_MESSAGES: usize = 2000;
        let selected = self.selected_thread_index;
        let mut remaining = MAX_UI_MESSAGES;
        if let Some(t) = self.threads.get_mut(selected) {
            if t.messages.len() > 200 {
                t.messages.drain(..t.messages.len() - 200);
            }
            remaining = remaining.saturating_sub(t.messages.len());
        }
        for (index, t) in self.threads.iter_mut().enumerate() {
            if index == selected {
                continue;
            }
            let keep = t.messages.len().min(remaining).min(20);
            if t.messages.len() > keep {
                t.messages.drain(..t.messages.len() - keep);
            }
            remaining = remaining.saturating_sub(keep);
        }
    }
    pub(super) fn save_composer(&mut self) {
        let Some(t) = self.threads.get(self.selected_thread_index) else {
            return;
        };
        if let Some(s) = self.composer_store.as_mut() {
            let _ = s.set_draft(
                &t.id,
                crate::runtime_state::Draft {
                    text: self.input_message.clone(),
                    reply: self.replying_to.clone(),
                    attachments: self.composer_attachments.clone(),
                },
            );
        }
    }
    pub(super) fn select_conversation(&mut self, index: usize) {
        if self.selected_thread_index == index
            && self
                .threads
                .get(index)
                .is_some_and(|t| !t.messages.is_empty())
        {
            return;
        }
        self.save_composer();
        self.navigation_request=self.navigation_request.wrapping_add(1);self.pending_jump=None;self.mention_profile=None;
        self.selected_thread_index = index;
        if let Some(t) = self.threads.get(index) {
            let draft = self
                .composer_store
                .as_ref()
                .map(|s| s.draft(&t.id))
                .unwrap_or_default();
            self.input_message = draft.text;
            self.replying_to = draft.reply;
            self.composer_attachments = draft.attachments;
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::LoadLocalPage {request_id:{self.navigation_request=self.navigation_request.wrapping_add(1);self.pending_jump=None;self.navigation_request},
                    thread_id: t.id.clone(),
                    cursor: None,
                    direction: crate::store_v2::PageDirection::Before,
                    replace: true,
                });
            }
        }
    }
}
