//! Private owner-bound decoded phone pages, replayed only after explicit transfer start.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const SCHEMA: u32 = 1;
const MAX_PAGE_BYTES: usize = 128 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PAGES: usize = 8;
const MAX_RECORDS: usize = 200_000;

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct GroupIdentity {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
}

/// Raw archive identifiers remain opaque until the authenticated resolver validates them.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Page {
    pub owner: String,
    pub objects: Vec<Value>,
    pub mapping: HashMap<String, String>,
    pub groups: HashMap<String, GroupIdentity>,
    pub own: String,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    owner: String,
    id: String,
    page: Page,
}
fn uid(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && value.bytes().any(|c| c != b'0')
}
pub(super) fn validate(account: &str, page: &Page) -> Result<()> {
    ensure!(
        !account.is_empty() && page.owner == account && page.own == account,
        "Pending phone owner mismatch"
    );
    ensure!(
        page.mapping.len() <= MAX_RECORDS && page.groups.len() <= MAX_RECORDS,
        "Pending phone metadata limit"
    );
    ensure!(
        page.mapping
            .iter()
            .all(|(key, value)| !key.is_empty() && uid(value)),
        "Invalid validated phone mapping"
    );
    ensure!(
        page.groups
            .iter()
            .all(|(key, group)| !key.is_empty() && uid(&group.id)),
        "Invalid validated phone group"
    );
    struct ByteCounter(usize);
    impl Write for ByteCounter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.0 {
                return Err(std::io::Error::other("pending phone byte limit"));
            }
            self.0 -= bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(ByteCounter(MAX_PAGE_BYTES.saturating_sub(4096)), page)?;
    let mut count = 0usize;
    for object in &page.objects {
        let parts = object
            .get("partitionsList")
            .and_then(Value::as_array)
            .context("Invalid pending partitions")?;
        ensure!(parts.len() <= MAX_RECORDS, "Pending phone partition limit");
        for part in parts {
            let id = part
                .get("id")
                .and_then(Value::as_str)
                .context("Invalid pending partition identity")?;
            ensure!(
                id.strip_prefix("oneone/")
                    .or_else(|| id.strip_prefix("group/"))
                    .is_some_and(|id| !id.is_empty()),
                "Invalid pending partition namespace"
            );
            let messages = part
                .get("messagesList")
                .and_then(Value::as_array)
                .context("Invalid pending message list")?;
            count = count
                .checked_add(messages.len())
                .context("Pending phone record limit")?;
            ensure!(count <= MAX_RECORDS, "Pending phone record limit");
        }
    }
    Ok(())
}
fn admit(account: &str) -> Result<crate::runtime_scope::MutationGuard> {
    let guard = crate::runtime_scope::mutation()?;
    if let Some(scope) = crate::runtime_scope::execution_scope() {
        ensure!(
            scope.account == account,
            "Pending phone account scope mismatch"
        );
    }
    Ok(guard)
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}
fn directory(root: &Path, account: &str) -> Result<PathBuf> {
    ensure!(!account.is_empty(), "Pending phone account required");
    use sha2::{Digest, Sha256};
    let parent = root
        .join("accounts")
        .join(format!("{:x}", Sha256::digest(account.as_bytes())));
    for directory in [root.join("accounts"), parent.clone()] {
        if let Ok(metadata) = std::fs::symlink_metadata(directory) {
            ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Invalid pending account directory"
            );
        }
    }
    let database = parent.join("history-v2.sqlite3");
    if database.exists() {
        // Raw retention must work even when a future DB version prevents message writes.
        // Read only the existing owner row; never migrate or modify history here.
        let connection = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let owners: Vec<String> = connection
            .prepare("SELECT account FROM owner LIMIT 2")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ensure!(owners == [account], "Pending phone database owner mismatch");
    }
    let path = parent.join("phone-pending-v1");
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Invalid pending phone directory"
        );
    }
    Ok(path)
}
fn entries(path: &Path) -> Result<Vec<(String, PathBuf, u64)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut result: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("Invalid pending phone filename")?;
        let id = name
            .strip_suffix(".json")
            .context("Unknown pending phone file")?;
        ensure!(valid_id(id), "Invalid pending phone ID");
        let metadata = std::fs::symlink_metadata(entry.path())?;
        ensure!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= MAX_PAGE_BYTES as u64,
            "Invalid pending phone file"
        );
        total = total
            .checked_add(metadata.len())
            .context("Pending phone disk limit")?;
        result.push((id.into(), entry.path(), metadata.len()));
        ensure!(
            result.len() <= MAX_PAGES && total <= MAX_TOTAL_BYTES,
            "Pending phone disk limit"
        );
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}
struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_PAGE_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("pending phone byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn decode(path: &Path, account: &str, id: &str) -> Result<Page> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_PAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_PAGE_BYTES, "Pending phone byte limit");
    let envelope: Envelope =
        serde_json::from_slice(&bytes).context("Invalid pending phone envelope")?;
    ensure!(
        envelope.schema == SCHEMA && envelope.owner == account && envelope.id == id,
        "Pending phone version or owner mismatch"
    );
    validate(account, &envelope.page)?;
    Ok(envelope.page)
}
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
/// Persist under the existing serialized history mutation boundary, never on the UI thread.
pub(super) fn persist(root: &Path, account: &str, id: &str, page: &Page) -> Result<()> {
    ensure!(valid_id(id), "Invalid pending phone ID");
    validate(account, page)?;
    let _mutation = crate::runtime_scope::owner_bound_mutation(account)?;
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(
        &mut bytes,
        &Envelope {
            schema: SCHEMA,
            owner: account.into(),
            id: id.into(),
            page: page.clone(),
        },
    )?;
    let dir = directory(root, account)?;
    let pending = entries(&dir)?;
    let destination = dir.join(format!("{id}.json"));
    if pending.iter().any(|(existing, _, _)| existing == id) {
        let existing = decode(&destination, account, id)?;
        ensure!(
            serde_json::to_value(&existing)? == serde_json::to_value(page)?,
            "Pending phone ID already owns another page"
        );
        sync_directory(&dir)?;
        return Ok(());
    }
    ensure!(
        pending.len() < MAX_PAGES
            && pending.iter().map(|(_, _, size)| *size).sum::<u64>() + bytes.0.len() as u64
                <= MAX_TOTAL_BYTES,
        "Pending phone disk limit"
    );
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            root.join("accounts"),
            dir.parent()
                .context("Pending account directory missing")?
                .to_path_buf(),
            dir.clone(),
        ] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    crate::data_dir::write_private_atomic(&destination, &bytes.0)?;
    sync_directory(&dir)?;
    sync_directory(dir.parent().context("Pending account directory missing")?)?;
    sync_directory(&root.join("accounts"))?;
    sync_directory(root)
}
/// Read only this account's known app-owned spool during an explicit Start command.
pub(super) fn load(root: &Path, account: &str) -> Result<Vec<(String, Page)>> {
    let dir = directory(root, account)?;
    entries(&dir)?
        .into_iter()
        .map(|(id, path, _)| decode(&path, account, &id).map(|page| (id, page)))
        .collect()
}
/// Remove an exact spool record only after its converted account page has a durable ACK.
pub(super) fn remove(root: &Path, account: &str, id: &str) -> Result<()> {
    ensure!(valid_id(id), "Invalid pending phone ID");
    let _mutation = admit(account)?;
    let dir = directory(root, account)?;
    let destination = dir.join(format!("{id}.json"));
    if !destination.exists() {
        return Ok(());
    }
    let pending = entries(&dir)?;
    ensure!(
        pending.iter().any(|(existing, _, _)| existing == id),
        "Pending phone record missing"
    );
    decode(&destination, account, id)?;
    std::fs::remove_file(destination)?;
    sync_directory(&dir)
}

/// Wipe only this account's validated managed records, after its accepted mailbox is flushed.
pub(super) fn clear(root: &Path, account: &str) -> Result<()> {
    let _mutation = admit(account)?;
    let dir = directory(root, account)?;
    let pending = entries(&dir)?;
    // Validate every owner/version before deleting any record; malformed data is terminal.
    for (id, path, _) in &pending {
        decode(path, account, id)?;
    }
    for (_, path, _) in pending {
        std::fs::remove_file(path)?;
    }
    if dir.exists() {
        sync_directory(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zl-phone-pending-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    pub(super) fn page() -> Page {
        Page {
            owner: "account-a".into(),
            own: "account-a".into(),
            mapping: HashMap::from([("opaque-peer".into(), "123".into())]),
            groups: HashMap::new(),
            objects: vec![
                serde_json::json!({"partitionsList":[{"id":"oneone/opaque-peer","messagesList":[{"globalId":"901","senderId":"opaque-own","msgType":0,"content":"Synthetic retained phone body","timestamp":1700000000000i64}]}]}),
            ],
        }
    }
    #[test]
    fn owner_bound_private_spool_reopens_without_resolving_or_overwriting() {
        let root = Fixture::new();
        let input = page();
        persist(&root.0, "account-a", "boot-1", &input).unwrap();
        persist(&root.0, "account-a", "boot-1", &input).unwrap();
        let loaded = load(&root.0, "account-a").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1.objects, input.objects);
        assert!(!loaded[0].1.mapping.contains_key("opaque-own"));
        assert!(load(&root.0, "account-b").unwrap().is_empty());
        assert!(persist(&root.0, "account-b", "boot-1", &input).is_err());
        let mut changed = input.clone();
        changed.objects[0]["partitionsList"][0]["messagesList"][0]["caption"] =
            Value::String("Different body".into());
        assert!(persist(&root.0, "account-a", "boot-1", &changed).is_err());
        let dir = directory(&root.0, "account-a").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(dir.join("boot-1.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let other = directory(&root.0, "account-b").unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::copy(dir.join("boot-1.json"), other.join("boot-1.json")).unwrap();
        assert!(load(&root.0, "account-b").is_err());
        remove(&root.0, "account-a", "boot-1").unwrap();
        assert!(load(&root.0, "account-a").unwrap().is_empty());
    }
    #[test]
    fn pending_file_version_symlink_and_count_limits_are_terminal() {
        let root = Fixture::new();
        let input = page();
        for i in 0..MAX_PAGES {
            persist(&root.0, "account-a", &format!("page-{i}"), &input).unwrap();
        }
        assert!(persist(&root.0, "account-a", "page-overflow", &input).is_err());
        let dir = directory(&root.0, "account-a").unwrap();
        let path = dir.join("page-0.json");
        let mut envelope: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        envelope["schema"] = Value::from(2);
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(load(&root.0, "account-a").is_err());
        assert!(remove(&root.0, "account-a", "page-0").is_err());
        assert!(persist(&root.0, "account-a", "../escape", &input).is_err());
        #[cfg(unix)]
        {
            std::fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(dir.join("page-1.json"), &path).unwrap();
            assert!(load(&root.0, "account-a").is_err());
        }
    }
    #[tokio::test]
    async fn auth_empty_ticket_flushes_owner_bound_raw_but_ordinary_mismatch_cannot_write() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Fixture::new();
        let owner = crate::runtime_scope::transition("account-a").unwrap();
        let ticket = crate::runtime_scope::run(owner, async {
            let permit = super::super::history_retention::reserve().await;
            super::super::history_retention::stage_raw(
                root.0.clone(),
                "account-a".into(),
                page(),
                permit,
            )
            .unwrap()
        })
        .await
        .unwrap();
        let other = crate::runtime_scope::transition("account-b").unwrap();
        crate::runtime_scope::run(other, async {
            assert!(persist(&root.0, "account-a", "ordinary-forbidden", &page()).is_err());
        })
        .await
        .unwrap();
        let auth = crate::runtime_scope::transition("").unwrap();
        let generation = auth.generation;
        let path = root.0.clone();
        crate::runtime_scope::run(auth, async move {
            crate::runtime_scope::spawn_transition_blocking(move || {
                assert!(persist(&path, "account-a", "empty-forbidden", &page()).is_err());
                crate::runtime_scope::transition_if_current_fallible(
                    generation,
                    "account-b",
                    super::super::history_retention::flush_pending_sync,
                )
                .unwrap();
            })
            .await
            .unwrap()
            .unwrap();
        })
        .await
        .unwrap();
        let pending = load(&root.0, "account-a").unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, ticket.raw_id().unwrap());
        assert_eq!(pending[0].1.objects, page().objects);
        drop(ticket);
        crate::runtime_scope::transition("").unwrap();
    }

    #[test]
    fn raw_spool_preserves_future_database_and_rejects_conflicting_database_owner() {
        let root = Fixture::new();
        let store = crate::store_v2::AccountStore::open(&root.0, "account-a").unwrap();
        let path = store.path.clone().unwrap();
        drop(store);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.pragma_update(None, "user_version", 999).unwrap();
        drop(connection);
        let before = std::fs::read(&path).unwrap();
        persist(&root.0, "account-a", "future-db", &page()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(load(&root.0, "account-a").unwrap().len(), 1);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute("UPDATE owner SET account='another-owner'", [])
            .unwrap();
        drop(connection);
        assert!(persist(&root.0, "account-a", "wrong-db-owner", &page()).is_err());
        assert!(load(&root.0, "account-a").is_err());
    }
    #[tokio::test]
    async fn clear_wipes_only_validated_current_account_and_keeps_other_data_inert() {
        let _serial = crate::runtime_scope::tests::TEST_LOCK.lock().await;
        let root = Fixture::new();
        let a = page();
        let mut b = page();
        b.owner = "account-b".into();
        b.own = "account-b".into();
        persist(&root.0, "account-a", "page-a", &a).unwrap();
        persist(&root.0, "account-b", "page-b", &b).unwrap();
        std::fs::write(root.0.join("session.json"), b"synthetic session sentinel").unwrap();
        std::fs::write(
            root.0.join("threads_cache.json"),
            b"synthetic legacy sentinel",
        )
        .unwrap();
        let b_file = directory(&root.0, "account-b").unwrap().join("page-b.json");
        let b_before = std::fs::read(&b_file).unwrap();
        let scope = crate::runtime_scope::transition("account-a").unwrap();
        crate::runtime_scope::run(scope, async {
            assert!(clear(&root.0, "account-b").is_err());
            clear(&root.0, "account-a").unwrap();
        })
        .await
        .unwrap();
        assert!(load(&root.0, "account-a").unwrap().is_empty());
        assert_eq!(load(&root.0, "account-b").unwrap().len(), 1);
        assert_eq!(std::fs::read(b_file).unwrap(), b_before);
        assert_eq!(
            std::fs::read(root.0.join("session.json")).unwrap(),
            b"synthetic session sentinel"
        );
        assert_eq!(
            std::fs::read(root.0.join("threads_cache.json")).unwrap(),
            b"synthetic legacy sentinel"
        );
        crate::runtime_scope::transition("").unwrap();
    }

    #[test]
    fn clear_rejects_wrong_owner_record_before_deleting_any_valid_record() {
        let root = Fixture::new();
        persist(&root.0, "account-a", "valid", &page()).unwrap();
        let path = directory(&root.0, "account-a").unwrap().join("wrong.json");
        let mut wrong = page();
        wrong.owner = "account-b".into();
        wrong.own = "account-b".into();
        std::fs::write(
            &path,
            serde_json::to_vec(&Envelope {
                schema: SCHEMA,
                owner: "account-b".into(),
                id: "wrong".into(),
                page: wrong,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(clear(&root.0, "account-a").is_err());
        assert!(path.exists());
        assert!(directory(&root.0, "account-a")
            .unwrap()
            .join("valid.json")
            .exists());
    }
}
