//! Identity translation for phone SyncFull; PC backup uses a different scope namespace.

use crate::{chat::Thread, session::Session};
use anyhow::{anyhow, ensure, Result};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// Native SyncFull buildPrepareQuery groups no more than 30 scopes per request.
pub(crate) const PHONE_SCOPES_PER_REQUEST: usize = 30;

/// Context for one request within a user-triggered phone transfer.
pub(crate) struct PhoneSyncRequestBatch {
    /// Identities belonging to this request only.
    pub identities: PhoneSyncIds,
    /// Zero-based native user-sync sequence index.
    pub index: usize,
    /// Number of requests needed for the entire scope set.
    pub total_requests: usize,
    /// Total verified scopes before splitting.
    pub total_scopes: usize,
    /// Rows committed by earlier requests in this operation.
    pub imported_before: usize,
    /// Missing-on-phone scopes from completed requests in this operation.
    pub unavailable_before: usize,
}

/// Identities retained only for one account's phone transfer.
#[derive(Clone, Default)]
pub(crate) struct PhoneSyncIds {
    /// Exact phone partition to the original desktop thread ID.
    pub partition_threads: BTreeMap<String, String>,
    /// Opaque global sender ID to the desktop user ID.
    pub sender_ids: BTreeMap<String, String>,
    /// Threads excluded because the server supplied no global identity.
    pub skipped_threads: usize,
}

impl PhoneSyncIds {
    /// Split the verified scopes using the native client's request boundary.
    pub fn request_batches(&self) -> Vec<Self> {
        let scopes: Vec<_> = self.partition_threads.iter().collect();
        scopes
            .chunks(PHONE_SCOPES_PER_REQUEST)
            .map(|chunk| Self {
                partition_threads: chunk
                    .iter()
                    .map(|(key, value)| ((*key).clone(), (*value).clone()))
                    .collect(),
                sender_ids: self.sender_ids.clone(),
                skipped_threads: self.skipped_threads,
            })
            .collect()
    }

    /// Build the phone query shape without exposing raw desktop IDs as partitions.
    pub fn queries(&self) -> Vec<Value> {
        self.partition_threads
            .keys()
            .map(|partition| {
                json!({
                    "partition":partition, "from":0, "to":9_007_199_254_740_991_i64, "limit":100_000
                })
            })
            .collect()
    }
}

fn local_target(thread: &Thread) -> Option<(String, bool)> {
    let group = thread.is_group || thread.id.starts_with('g');
    let raw = if group {
        thread
            .id
            .strip_prefix("group_")
            .or_else(|| thread.id.strip_prefix('g'))
            .unwrap_or(&thread.id)
    } else {
        &thread.id
    };
    valid_local_id(raw).then(|| (raw.to_string(), group))
}

fn valid_local_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.bytes().any(|b| b != b'0')
}

fn global_id(value: &Value) -> Option<&str> {
    // Global IDs are opaque strings, including base64 punctuation; never parse them as integers.
    value.as_str().filter(|id| {
        !id.is_empty() && *id != "0" && id.len() <= 4096 && !id.chars().any(char::is_control)
    })
}

fn from_metadata(
    owner: &str,
    threads: &[Thread],
    users: &BTreeMap<String, String>,
    groups: &Map<String, Value>,
) -> Result<PhoneSyncIds> {
    ensure!(
        users.contains_key(owner),
        "Không lấy được định danh chủ tài khoản để đồng bộ"
    );
    let mut result = PhoneSyncIds::default();
    for (uid, global) in users {
        if let Some(previous) = result.sender_ids.insert(global.clone(), uid.clone()) {
            ensure!(
                previous == *uid,
                "Phản hồi định danh người gửi bị trùng lặp"
            );
        }
    }
    let mut partition_local_ids = BTreeMap::new();
    for thread in threads {
        let Some((id, group)) = local_target(thread) else {
            result.skipped_threads += 1;
            continue;
        };
        let global = if group {
            groups
                .get(&id)
                .or_else(|| groups.get(&format!("g{id}")))
                .and_then(|profile| profile.get("globalId"))
                .and_then(global_id)
        } else {
            users.get(&id).map(String::as_str)
        };
        let Some(global) = global else {
            result.skipped_threads += 1;
            continue;
        };
        // Phone QueryFactory/aSMs, not WorkerScopeBuilder for PC-to-phone backup (re-notes/39).
        let partition = format!("{}/{global}", if group { "group" } else { "oneone" });
        if let Some(previous) = partition_local_ids.insert(partition.clone(), id.clone()) {
            ensure!(previous == id, "Phản hồi định danh hội thoại bị trùng lặp");
        }
        result
            .partition_threads
            .entry(partition)
            .or_insert_with(|| thread.id.clone());
    }
    ensure!(
        !result.partition_threads.is_empty(),
        "Không có hội thoại có định danh đồng bộ hợp lệ"
    );
    Ok(result)
}

/// Resolve global IDs before generating keys or sending any phone prompt.
///
/// # Errors
/// Returns a sanitized failure if metadata lookup fails or no safe scope is available.
pub(crate) async fn resolve_phone_sync_ids(
    s: &Session,
    threads: &[Thread],
) -> Result<PhoneSyncIds> {
    tokio::time::timeout(Duration::from_secs(45), async {
        let owner = s
            .login
            .as_ref()
            .ok_or_else(|| anyhow!("Chưa đăng nhập"))?
            .uin
            .as_str();
        let mut users = BTreeSet::from([owner.to_string()]);
        let mut group_ids = BTreeSet::new();
        for thread in threads {
            if let Some((id, group)) = local_target(thread) {
                if group {
                    group_ids.insert(id);
                } else {
                    users.insert(id);
                }
            }
        }
        let mut mapped = BTreeMap::new();
        for chunk in users.into_iter().collect::<Vec<_>>().chunks(100) {
            ensure!(
                !super::PHONE_SYNC_CANCEL.load(std::sync::atomic::Ordering::Relaxed),
                "Đã dừng đồng bộ điện thoại"
            );
            mapped.extend(map_user_ids(s, chunk, false).await?);
        }
        let mut groups = Map::new();
        for chunk in group_ids.into_iter().collect::<Vec<_>>().chunks(50) {
            ensure!(
                !super::PHONE_SYNC_CANCEL.load(std::sync::atomic::Ordering::Relaxed),
                "Đã dừng đồng bộ điện thoại"
            );
            let (profiles, _) = tokio::time::timeout(
                Duration::from_secs(12),
                crate::chat::fetch_group_profiles(s, chunk),
            )
            .await
            .map_err(|_| anyhow!("Máy chủ hồ sơ nhóm phản hồi quá lâu"))?
            .map_err(|_| anyhow!("Không lấy được định danh nhóm để đồng bộ"))?;
            groups.extend(profiles);
        }
        from_metadata(owner, threads, &mapped, &groups)
    })
    .await
    .map_err(|_| anyhow!("Máy chủ định danh đồng bộ phản hồi quá lâu"))?
}

fn mapping_body(ids: &[String], to_local: bool) -> Result<Value> {
    Ok(if to_local {
        json!({"globalUids":serde_json::to_string(ids)?})
    } else {
        json!({"noiseUids":ids})
    })
}

fn parse_mapping(
    mut response: Value,
    requested: &[String],
    to_local: bool,
) -> Result<BTreeMap<String, String>> {
    let mut checked_envelope = false;
    // ParamCipher may wrap an encrypted success envelope in another HTTP success envelope.
    for _ in 0..4 {
        if let Value::String(ref text) = response {
            response = serde_json::from_str(text)
                .map_err(|_| anyhow!("Phản hồi định danh đồng bộ không hợp lệ"))?;
        }
        if response.get("error_code").is_some() {
            let code = response
                .get("error_code")
                .and_then(Value::as_i64)
                .ok_or_else(|| anyhow!("Phản hồi định danh đồng bộ không hợp lệ"))?;
            ensure!(
                code == 0,
                "Máy chủ từ chối ánh xạ định danh đồng bộ (mã {code})"
            );
            checked_envelope = true;
            response = response
                .get_mut("data")
                .ok_or_else(|| anyhow!("Phản hồi định danh đồng bộ thiếu dữ liệu"))?
                .take();
        } else if checked_envelope
            && response.as_object().is_some_and(|map| map.len() == 1)
            && response.get("data").is_some_and(Value::is_object)
        {
            // Observed decrypted gid replies add a data-only wrapper beneath the checked code.
            response = response
                .get_mut("data")
                .ok_or_else(|| anyhow!("Phản hồi định danh đồng bộ thiếu dữ liệu"))?
                .take();
        } else {
            break;
        }
    }
    ensure!(
        checked_envelope,
        "Phản hồi định danh đồng bộ thiếu mã kết quả"
    );
    let data = response
        .as_object()
        .ok_or_else(|| anyhow!("Phản hồi định danh đồng bộ không hợp lệ"))?;
    let mut result = BTreeMap::new();
    for id in requested {
        if let Some(value) = data.get(id) {
            let mapped = if to_local {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_u64().map(|v| v.to_string()))
                    .filter(|id| valid_local_id(id))
            } else {
                global_id(value).map(str::to_owned)
            };
            if let Some(mapped) = mapped {
                result.insert(id.clone(), mapped);
            }
        }
    }
    Ok(result)
}

async fn map_user_ids(
    s: &Session,
    ids: &[String],
    to_local: bool,
) -> Result<BTreeMap<String, String>> {
    // Shipped PC cmd 12053/12054: noiseUids is an array; globalUids is a JSON string.
    let body = mapping_body(ids, to_local)?;
    let params = s
        .encoded_params(&body)
        .map_err(|_| anyhow!("Không mã hóa được yêu cầu định danh đồng bộ"))?;
    let endpoint = if to_local { "decrypt" } else { "encrypt" };
    let path = format!("/api/gid/{endpoint}?{}&params={params}", s.common_params());
    let response = tokio::time::timeout(Duration::from_secs(12), s.get_endpoint("profile", &path))
        .await
        .map_err(|_| anyhow!("Máy chủ định danh đồng bộ phản hồi quá lâu"))?
        .map_err(|_| anyhow!("Không lấy được định danh đồng bộ từ máy chủ"))?;
    parse_mapping(
        s.maybe_decrypt(response, s.session_key_bytes()).await,
        ids,
        to_local,
    )
}

/// Resolve unknown global senders before a batch enters SQLite.
///
/// # Errors
/// Rejects malformed or unmapped senders instead of attributing them to another account.
pub(crate) async fn resolve_phone_senders(
    s: &Session,
    identities: &PhoneSyncIds,
    partitions: &[super::ParsedProtoPartition],
) -> Result<BTreeMap<String, String>> {
    let mut result = identities.sender_ids.clone();
    let missing: BTreeSet<_> = partitions
        .iter()
        .flat_map(|p| p.messages.iter())
        .map(|m| m.sender_id.clone())
        .filter(|id| !result.contains_key(id))
        .collect();
    ensure!(
        missing
            .iter()
            .all(|id| global_id(&Value::String(id.clone())).is_some()),
        "Gói đồng bộ có định danh người gửi không hợp lệ"
    );
    for chunk in missing.into_iter().collect::<Vec<_>>().chunks(100) {
        let mapped = map_user_ids(s, chunk, true).await?;
        ensure!(
            chunk.iter().all(|id| mapped.contains_key(id)),
            "Chưa ánh xạ được người gửi của gói đồng bộ"
        );
        result.extend(mapped);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_transfer_is_split_into_native_sized_requests_without_losing_scopes() {
        let mut ids = PhoneSyncIds {
            skipped_threads: 2,
            ..Default::default()
        };
        for index in 1..=504 {
            ids.partition_threads
                .insert(format!("oneone/fake-global-{index}"), index.to_string());
        }
        let batches = ids.request_batches();
        assert_eq!(batches.len(), 17);
        assert!(batches
            .iter()
            .all(|batch| batch.partition_threads.len() <= 30 && batch.skipped_threads == 2));
        assert_eq!(batches.last().unwrap().partition_threads.len(), 24);
        let merged: BTreeMap<_, _> = batches
            .into_iter()
            .flat_map(|batch| batch.partition_threads)
            .collect();
        assert_eq!(merged, ids.partition_threads);
    }

    #[test]
    fn scope_boundary_requires_no_extra_empty_request() {
        let mut ids = PhoneSyncIds::default();
        assert!(ids.request_batches().is_empty());
        for index in 1..=30 {
            ids.partition_threads
                .insert(format!("group/fake-{index}"), index.to_string());
        }
        assert_eq!(ids.request_batches().len(), 1);
        ids.partition_threads
            .insert("group/fake-next".into(), "31".into());
        assert_eq!(
            ids.request_batches()
                .iter()
                .map(|batch| batch.partition_threads.len())
                .collect::<Vec<_>>(),
            vec![30, 1]
        );
    }
    fn thread(id: &str, group: bool) -> Thread {
        Thread {
            id: id.into(),
            is_group: group,
            ..Default::default()
        }
    }
    #[test]
    fn phone_scopes_preserve_opaque_ids_and_route_to_original_threads() {
        let users = BTreeMap::from([
            ("111".into(), "owner+/=".into()),
            ("222".into(), "peer/opaque==".into()),
        ]);
        let groups = Map::from_iter([("333".into(), json!({"globalId":"group+opaque="}))]);
        let ids = from_metadata(
            "111",
            &[
                thread("222", false),
                thread("g333", true),
                thread("222", false),
            ],
            &users,
            &groups,
        )
        .unwrap();
        assert_eq!(ids.partition_threads["oneone/peer/opaque=="], "222");
        assert_eq!(ids.partition_threads["group/group+opaque="], "g333");
        assert_eq!(ids.queries().len(), 2);
        assert_eq!(ids.sender_ids["owner+/="], "111");
    }
    #[test]
    fn unresolved_targets_never_fall_back_to_desktop_ids() {
        let users = BTreeMap::from([
            ("111".into(), "owner==".into()),
            ("222".into(), "peer==".into()),
        ]);
        let ids = from_metadata(
            "111",
            &[
                thread("222", false),
                thread("333", false),
                thread("g444", true),
            ],
            &users,
            &Map::new(),
        )
        .unwrap();
        assert_eq!(ids.skipped_threads, 2);
        assert_eq!(ids.queries()[0]["partition"], "oneone/peer==");
        assert!(from_metadata("111", &[thread("333", false)], &users, &Map::new()).is_err());
        assert!(from_metadata("555", &[thread("222", false)], &users, &Map::new()).is_err());
    }

    #[test]
    fn conflicting_global_mapping_cannot_merge_two_local_identities() {
        let conflicting = BTreeMap::from([
            ("111".into(), "same==".into()),
            ("222".into(), "same==".into()),
        ]);
        assert!(from_metadata("111", &[thread("222", false)], &conflicting, &Map::new()).is_err());
        let owner = BTreeMap::from([("111".into(), "owner==".into())]);
        let groups = Map::from_iter([
            ("333".into(), json!({"globalId":"same=="})),
            ("444".into(), json!({"globalId":"same=="})),
        ]);
        assert!(from_metadata(
            "111",
            &[thread("g333", true), thread("g444", true)],
            &owner,
            &groups
        )
        .is_err());
    }
    #[test]
    fn mapping_contract_and_nested_envelopes_preserve_64_bit_ids() {
        let ids = vec!["opaque/==".to_string()];
        assert_eq!(
            mapping_body(&ids, false).unwrap(),
            json!({"noiseUids":["opaque/=="]})
        );
        assert_eq!(
            mapping_body(&ids, true).unwrap(),
            json!({"globalUids":"[\"opaque/==\"]"})
        );
        let map=parse_mapping(json!({"error_code":0,"data":{"error_code":0,"data":{"opaque/==":"9007199254740993","unsolicited":"456"}}}),&ids,true).unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["opaque/=="], "9007199254740993");
    }

    #[test]
    fn checked_gid_reply_can_contain_a_data_only_wrapper() {
        let requested = vec!["111".to_string()];
        let result = parse_mapping(
            json!({"error_code":0,"data":{"data":{"111":"opaque/=="}}}),
            &requested,
            false,
        )
        .unwrap();
        assert_eq!(result["111"], "opaque/==");
        assert!(parse_mapping(json!({"data":{"111":"opaque/=="}}), &requested, false).is_err());
    }
    #[test]
    fn malformed_mapping_is_rejected_without_exposing_server_content() {
        let ids = vec!["222".into()];
        let error = parse_mapping(
            json!({"error_code":114,"error_message":"private-value","data":{}}),
            &ids,
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(!error.contains("private-value"));
        assert!(parse_mapping(json!({"data":{}}), &ids, false).is_err());
        assert!(parse_mapping(
            json!({"error_code":0,"data":{"error_code":600,"data":{}}}),
            &ids,
            false
        )
        .is_err());
        assert!(
            parse_mapping(json!({"error_code":0,"data":{"222":0}}), &ids, false)
                .unwrap()
                .is_empty()
        );
    }
}
