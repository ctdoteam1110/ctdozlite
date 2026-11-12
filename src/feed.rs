//! Social Feed (Nhật ký) & Social Profile module for Zalo PC / Android
//! -------------------------------------------------------------------
//! Cung cấp cấu trúc dữ liệu và các API tương tác mạng xã hội thời gian thực:
//! - Lấy hồ sơ chi tiết bạn bè & chính mình (Ảnh bìa Cover HD, Avatar, Ngày sinh, Bio, lastActionTime)
//! - Lấy kho ảnh Nhật ký cá nhân (CDN zpsocial) & hợp nhất media chia sẻ thực tế
//! - Đóng gói binary RequestPacket chuẩn bytecode Zalo (`pw0.e`: cmd=1230/subcmd=6, `mw0.c`: cmd=2030/subcmd=0, `o41.g`: cmd=1234/2045)
//! - Nạp và hợp nhất bài viết Nhật ký thực tế + trạng thái like/comment thời gian thực từ SQLite (`timeline_feed_1`, `user_feed`, `table_feed_real_time_like_comment`)

use anyhow::{anyhow, Result};
use chrono::{FixedOffset, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedAuthor {
    pub uid: String,
    pub display_name: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedMedia {
    pub photo_id: String,
    pub media_type: String, // "image" | "video"
    pub url: String,
    pub thumbnail: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedReaction {
    pub like_count: u32,
    pub comment_count: u32,
    pub is_liked: bool,
    pub top_reactions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedComment {
    pub comment_id: String,
    pub author_name: String,
    pub author_uid: String,
    pub content: String,
    pub timestamp_str: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedItem {
    pub feed_id: String,
    pub author: FeedAuthor,
    pub date_str: String,
    pub content: String,
    pub status_badge: Option<String>,
    pub media_list: Vec<FeedMedia>,
    pub reaction: FeedReaction,
    pub comments: Vec<FeedComment>,
    #[serde(default)]
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialUserProfile {
    pub uid: String,
    pub username: String,
    pub display_name: String,
    pub zalo_name: String,
    pub avatar_url: String,
    pub cover_url: String,
    pub gender: i32,
    pub sdob: String,
    pub status: String,
    pub is_friend: bool,
    pub last_action_time: Option<u64>,
}

fn parse_u64_any(val: Option<&Value>) -> Option<u64> {
    let v = val?;
    v.as_u64()
        .or_else(|| v.as_i64().and_then(|n| u64::try_from(n).ok()))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
}

pub fn format_vn_timestamp_ms(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return String::new();
    }
    let normalized_ms = if ts_ms < 100_000_000_000 {
        ts_ms * 1000
    } else {
        ts_ms
    };
    let tz = FixedOffset::east_opt(7 * 3600).unwrap();
    tz.timestamp_millis_opt(normalized_ms)
        .single()
        .map(|dt| dt.format("%d/%m/%Y lúc %H:%M:%S").to_string())
        .unwrap_or_default()
}

pub fn normalize_social_media_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.ends_with(".jxl") && trimmed.len() > 4 {
        format!("{}.jpg", &trimmed[..trimmed.len() - 4])
    } else {
        trimmed.to_string()
    }
}

pub fn reaction_id_to_label(reaction_id: i64) -> &'static str {
    match reaction_id {
        1 => "Thích",
        2 => "Yêu thích",
        3 => "Haha",
        4 => "Wow",
        5 => "Buồn",
        6 => "Phẫn nộ",
        _ => "Thích",
    }
}

impl SocialUserProfile {
    pub fn from_json(uid: &str, val: &Value) -> Option<Self> {
        let obj = val.as_object()?;
        let resolved_uid = obj
            .get("userId")
            .or_else(|| obj.get("uid"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(uid);
        let display_name = obj
            .get("displayName")
            .or_else(|| obj.get("dpn"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(resolved_uid)
            .to_string();
        let zalo_name = obj
            .get("zaloName")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(&display_name)
            .to_string();
        let username = obj
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let avatar_url = obj
            .get("avatar")
            .or_else(|| obj.get("avt"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let cover_url = obj
            .get("cover")
            .or_else(|| obj.get("bgavatar"))
            .and_then(Value::as_str)
            .or_else(|| {
                obj.get("cover")
                    .and_then(|c| c.get("coverUrl").or_else(|| c.get("url")))
                    .and_then(Value::as_str)
            })
            .unwrap_or("")
            .to_string();
        let gender = obj.get("gender").and_then(Value::as_i64).unwrap_or(0) as i32;
        let sdob = obj
            .get("sdob")
            .or_else(|| obj.get("dob"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let status = obj
            .get("status")
            .or_else(|| obj.get("statusMsg"))
            .or_else(|| obj.get("bio"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let is_friend = obj
            .get("isFr")
            .and_then(Value::as_i64)
            .map(|v| v == 1)
            .unwrap_or(true);
        let last_action_time = parse_u64_any(obj.get("lastActionTime"))
            .or_else(|| parse_u64_any(obj.get("lastUpdateTime")))
            .or_else(|| parse_u64_any(obj.get("lastUpdate")));

        Some(Self {
            uid: resolved_uid.to_string(),
            username,
            display_name,
            zalo_name,
            avatar_url,
            cover_url,
            gender,
            sdob,
            status,
            is_friend,
            last_action_time,
        })
    }
}

/// Đóng gói binary `RequestPacket` chuẩn giao thức Zalo Social Feed (reverse từ `pw0.e`, `mw0.c`, `o41.g`, `nu0.v`)
pub mod packet {
    /// `pw0.e.d`: Get Timeline (`cmd = 1230` / `0x04CE`, `subcmd = 6`, `cmd_type = 3`)
    pub const CMD_FEED_TIMELINE: u16 = 1230;
    pub const SUBCMD_GET_FEED: u8 = 6;

    /// `mw0.c.k`: Get Friend User Timeline (`cmd = 2030` / `0x07EE`, `subcmd = 0`, `cmd_type = 3`)
    pub const CMD_USER_FEED: u16 = 2030;
    pub const SUBCMD_USER_FEED: u8 = 0;

    /// `o41.g`: Post Feed (`cmd = 1234` / `0x04D2`, `subcmd = 8`) & Edit Feed (`cmd = 2045` / `0x07FD`, `subcmd = 0`)
    pub const CMD_POST_FEED: u16 = 1234;
    pub const SUBCMD_POST_FEED: u8 = 8;
    pub const CMD_EDIT_FEED: u16 = 2045;
    pub const SUBCMD_EDIT_FEED: u8 = 0;

    /// Mã lỗi client trả về khi mất mạng (`!b2.M(false)`) trong `pw0.e`, `mw0.c`, `o41.g`
    pub const ERR_FEED_NO_NETWORK: i32 = 50001;

    /// Phiên bản client Zalo (`AbstractC27691d.f126612a` trong APK 26.10.02)
    pub const CLIENT_VERSION: i32 = 26_10_02_903;
    pub const CMD_TYPE_REQUEST: u8 = 3;

    /// Tạo payload nhị phân chuẩn `pw0.e.d` (52 bytes):
    /// `u8(1) + i32(client_version) + i32(lang_platform) + u8(tab) + i32(page) + i32(count)`
    /// `+ i64(last_feed_id_server) + i64(last_feed_id_local) + i64(max_feed_id) + i64(feed_version) + i16(source_enum)`
    pub fn build_timeline_payload_with_cursors(
        tab: u8,
        page: i32,
        count: i32,
        last_feed_id_server: i64,
        last_feed_id_local: i64,
        max_feed_id: i64,
        feed_version: i64,
        source_enum: i16,
    ) -> Vec<u8> {
        let mut buf = Vec::with_capacity(52);
        buf.push(0x01);
        buf.extend_from_slice(&CLIENT_VERSION.to_be_bytes());
        buf.extend_from_slice(&1i32.to_be_bytes()); // AbstractC21220r.l() = 1 (vi/android)
        buf.push(tab);
        buf.extend_from_slice(&page.to_be_bytes());
        buf.extend_from_slice(&count.to_be_bytes());
        buf.extend_from_slice(&last_feed_id_server.to_be_bytes());
        buf.extend_from_slice(&last_feed_id_local.to_be_bytes());
        buf.extend_from_slice(&max_feed_id.to_be_bytes());
        buf.extend_from_slice(&feed_version.to_be_bytes());
        buf.extend_from_slice(&source_enum.to_be_bytes());
        buf
    }

    /// Tạo payload nhị phân chuẩn `mw0.c.k` (18 bytes) khi xem dòng thời gian của 1 người dùng cụ thể (`cmd = 2030`)
    pub fn build_user_timeline_payload(owner_id: u32, flag: u8) -> Vec<u8> {
        let mut buf = Vec::with_capacity(18);
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.push(0x01);
        buf.extend_from_slice(&CLIENT_VERSION.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&(owner_id as i32).to_be_bytes());
        buf.extend_from_slice(&1i32.to_be_bytes());
        buf.push(flag);
        buf
    }

    /// Tạo payload cho lệnh GET_FEED (tương thích chữ ký caller `modals.rs`, xuất đúng 49 bytes theo `pw0.e.d`)
    pub fn build_get_feed_payload(
        owner_id: u64,
        viewer_id: u64,
        tab: u32,
        page: u32,
        count: u32,
    ) -> Vec<u8> {
        let _ = viewer_id;
        build_timeline_payload_with_cursors(
            tab as u8,
            page as i32,
            count as i32,
            0,
            0,
            owner_id as i64,
            2, // C26856y.y() feed_version = 2
            0,
        )
    }

    /// Đóng gói payload vào khung `RequestPacket` chuẩn (`nu0.v`):
    /// `[total_len: u32 BE][version=1: u8][encrypt=0: u8][uin: u32 BE][cmd_type=3: u8][cmd: u16 BE][subcmd: u8][seq_id: u16 BE][params_len: u32 BE][payload]`
    pub fn build_request_packet_with_uin(
        uin: u32,
        cmd: u16,
        subcmd: u8,
        seq_id: u16,
        payload: &[u8],
    ) -> Vec<u8> {
        let header_len = 1 + 1 + 4 + 1 + 2 + 1 + 2 + 4; // 16 bytes
        let body_len = header_len + payload.len();
        let mut packet = Vec::with_capacity(4 + body_len);
        packet.extend_from_slice(&(body_len as u32).to_be_bytes());
        packet.push(0x01); // version = 1 (n(1))
        packet.push(0x00); // encrypt = 0 (g(0))
        packet.extend_from_slice(&uin.to_be_bytes()); // m(uin)
        packet.push(CMD_TYPE_REQUEST); // f((byte) 3)
        packet.extend_from_slice(&cmd.to_be_bytes()); // d((short) cmd)
        packet.push(subcmd); // l((byte) subcmd)
        packet.extend_from_slice(&seq_id.to_be_bytes()); // k(seqId)
        packet.extend_from_slice(&(payload.len() as u32).to_be_bytes()); // i(payload.len)
        packet.extend_from_slice(payload);
        packet
    }

    /// Đóng gói `RequestPacket` mặc định (tương thích chữ ký caller `modals.rs`)
    pub fn build_request_packet(cmd: u16, subcmd: u8, payload: &[u8]) -> Vec<u8> {
        build_request_packet_with_uin(0, cmd, subcmd, 1, payload)
    }
}

// Mở rộng các phương thức mạng xã hội cho Session
impl crate::session::Session {
    /// Lấy hồ sơ mạng xã hội của chính mình (`/api/social/profile/me-v2` hoặc fallback `getprofiles`)
    pub async fn get_my_social_profile(&self) -> Result<SocialUserProfile> {
        let my_uid = self
            .login
            .as_ref()
            .map(|l| l.uin.clone())
            .unwrap_or_default();
        if my_uid.is_empty() {
            return Err(anyhow!("Missing logged-in UIN"));
        }

        let body = json!({
            "avatar_size": 240,
            "imei": self.imei,
        });
        if let Ok(enc) = self.encoded_params(&body) {
            let url = format!(
                "{}/api/social/profile/me-v2?{}&params={enc}&os=8",
                self.domains.profile,
                self.common_params()
            );
            if let Ok(resp) = self.get(&url).await {
                let dec = self.maybe_decrypt(resp, self.session_key_bytes()).await;
                if dec.get("error_code").and_then(Value::as_i64) == Some(0) {
                    if let Some(data) = dec.get("data") {
                        let target_obj = data.get(&my_uid).unwrap_or(data);
                        if let Some(p) = SocialUserProfile::from_json(&my_uid, target_obj) {
                            return Ok(p);
                        }
                    }
                }
            }
        }

        self.get_friend_social_profile(&my_uid).await
    }

    /// Lấy hồ sơ chi tiết bạn bè (Ảnh bìa Cover HD, Avatar, Ngày sinh, Giới tính, Bio)
    pub async fn get_friend_social_profile(&self, target_uid: &str) -> Result<SocialUserProfile> {
        let body = json!({
            "avatar_size": 240,
            "friend_ids": [target_uid],
            "imei": self.imei,
        });

        let enc = self.encoded_params(&body)?;
        let url = format!(
            "{}/api/social/friend/getprofiles?{}&params={enc}&os=8",
            self.domains.profile,
            self.common_params()
        );

        let resp = self.get(&url).await?;
        let dec = self.maybe_decrypt(resp, self.session_key_bytes()).await;

        if dec.get("error_code").and_then(Value::as_i64) == Some(0) {
            if let Some(prof_obj) = dec.get("data").and_then(|d| d.get(target_uid).or(Some(d))) {
                if let Some(p) = SocialUserProfile::from_json(target_uid, prof_obj) {
                    return Ok(p);
                }
            }
        }

        // Fallback sang batch v2 cho 1 UID nếu endpoint v1 trả lỗi
        if let Ok(mut list) = self
            .get_friend_social_profiles_batch(&[target_uid.to_string()])
            .await
        {
            if let Some(p) = list.pop() {
                return Ok(p);
            }
        }

        Err(anyhow!(
            "Server returned error or missing data for uid {target_uid}: {dec}"
        ))
    }

    /// Lấy kho ảnh Nhật ký cá nhân (CDN zpsocial)
    pub async fn get_my_social_avatar_list(&self, page: u32, count: u32) -> Result<Vec<FeedMedia>> {
        let body = json!({
            "page": page,
            "count": count,
            "imei": self.imei,
        });

        let enc = self.encoded_params(&body)?;
        let url = format!(
            "{}/api/social/avatar-list?{}&params={enc}&os=8",
            self.domains.profile,
            self.common_params()
        );

        let resp = self.get(&url).await?;
        let dec = self.maybe_decrypt(resp, self.session_key_bytes()).await;

        let mut photos = Vec::new();
        if let Some(photo_arr) = dec
            .get("data")
            .and_then(|d| d.get("photos"))
            .and_then(Value::as_array)
        {
            for p in photo_arr {
                let photo_id = p
                    .get("photoId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let raw_url = p.get("url").and_then(Value::as_str).unwrap_or("");
                let url = normalize_social_media_url(raw_url);
                let thumbnail = p
                    .get("thumbnail")
                    .and_then(Value::as_str)
                    .map(normalize_social_media_url)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| url.clone());
                let width = p.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
                let height = p.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
                if !url.is_empty() {
                    photos.push(FeedMedia {
                        photo_id,
                        media_type: "image".to_string(),
                        url,
                        thumbnail,
                        width,
                        height,
                    });
                }
            }
        }

        Ok(photos)
    }

    /// Lấy hàng loạt hồ sơ mạng xã hội của nhiều bạn bè (`POST /api/social/friend/getprofiles/v2`)
    pub async fn get_friend_social_profiles_batch(
        &self,
        target_uids: &[String],
    ) -> Result<Vec<SocialUserProfile>> {
        if target_uids.is_empty() {
            return Ok(Vec::new());
        }
        let mut all_profiles = Vec::new();
        let mut seen_uids = HashSet::new();

        for chunk in target_uids.chunks(40) {
            let pversion_map: Vec<String> = chunk.iter().map(|u| format!("{u}_0")).collect();
            let body = json!({
                "phonebook_version": 0,
                "friend_pversion_map": pversion_map,
                "avatar_size": 240,
                "language": "vi",
                "show_online_status": 1,
                "imei": self.imei,
            });
            let enc = self.encoded_params(&body)?;
            let url = format!(
                "{}/api/social/friend/getprofiles/v2?{}",
                self.domains.profile,
                self.common_params()
            );
            let form = format!("params={enc}");
            let resp = self.post_form(&url, form).await?;
            let dec = self.maybe_decrypt(resp, self.session_key_bytes()).await;

            if let Some(changed) = dec
                .get("data")
                .and_then(|d| d.get("changed_profiles").or(Some(d)))
                .and_then(Value::as_object)
            {
                for (uid, pval) in changed {
                    if let Some(p) = SocialUserProfile::from_json(uid, pval) {
                        if seen_uids.insert(p.uid.clone()) {
                            all_profiles.push(p);
                        }
                    }
                }
            }
        }

        Ok(all_profiles)
    }

    /// Tổng hợp Dòng thời gian (Nhật ký) thời gian thực từ:
    /// 1. Hồ sơ cá nhân & kho ảnh đại diện/nhật ký (`/api/social/avatar-list`, `/api/social/profile/me-v2`)
    /// 2. Hồ sơ mạng xã hội thời gian thực của toàn bộ bạn bè (`/api/social/friend/getprofiles/v2`)
    /// 3. Cơ sở dữ liệu Nhật ký thực tế (`timeline_feed_1`, `user_feed`, `table_feed_real_time_like_comment`)
    /// 4. Media thực tế chia sẻ trong phiên (`shared_media_by_uid`) và cập nhật trạng thái/ảnh bìa thực tế của bạn bè
    pub async fn fetch_timeline_feed(
        &self,
        friend_uids: &[String],
        shared_media_by_uid: &HashMap<String, Vec<FeedMedia>>,
    ) -> Result<(Vec<SocialUserProfile>, Vec<FeedItem>)> {
        let mut items: Vec<FeedItem> = Vec::new();
        let mut seen_feed_ids: HashSet<String> = HashSet::new();
        let now_ms = chrono::Utc::now().timestamp_millis();

        // 1. Lấy hồ sơ cá nhân & kho ảnh đại diện/nhật ký của chính mình
        let my_uid = self
            .login
            .as_ref()
            .map(|l| l.uin.clone())
            .unwrap_or_default();
        let mut my_name = self
            .login
            .as_ref()
            .map(|l| l.display_name.clone())
            .unwrap_or_else(|| "Tôi".to_string());
        let mut my_ava = self
            .login
            .as_ref()
            .map(|l| l.avatar.clone())
            .unwrap_or_default();

        let my_profile_opt = self.get_my_social_profile().await.ok();
        if let Some(ref mp) = my_profile_opt {
            if !mp.display_name.is_empty() {
                my_name = mp.display_name.clone();
            }
            if !mp.avatar_url.is_empty() {
                my_ava = mp.avatar_url.clone();
            }
        }

        if let Ok(my_photos) = self.get_my_social_avatar_list(1, 20).await {
            if !my_photos.is_empty() {
                let my_ts_ms = my_profile_opt
                    .as_ref()
                    .and_then(|p| p.last_action_time)
                    .map(|t| if t < 100_000_000_000 { (t * 1000) as i64 } else { t as i64 })
                    .unwrap_or(now_ms);
                let feed_id = format!("my_album_{my_uid}");
                seen_feed_ids.insert(feed_id.clone());
                items.push(FeedItem {
                    feed_id,
                    author: FeedAuthor {
                        uid: my_uid.clone(),
                        display_name: my_name.clone(),
                        avatar_url: my_ava.clone(),
                    },
                    date_str: if my_ts_ms > 0 {
                        format_vn_timestamp_ms(my_ts_ms)
                    } else {
                        "Kho ảnh cá nhân".to_string()
                    },
                    content: my_profile_opt
                        .as_ref()
                        .filter(|p| !p.status.trim().is_empty())
                        .map(|p| p.status.clone())
                        .unwrap_or_else(|| {
                            format!("Album ảnh đại diện mạng xã hội ({} ảnh)", my_photos.len())
                        }),
                    status_badge: Some("Album ảnh".to_string()),
                    media_list: my_photos,
                    reaction: FeedReaction {
                        like_count: 0,
                        comment_count: 0,
                        is_liked: false,
                        top_reactions: Vec::new(),
                    },
                    comments: Vec::new(),
                    timestamp_ms: my_ts_ms,
                });
            }
        }

        // 2. Lấy hàng loạt hồ sơ mạng xã hội của bạn bè theo thời gian thực
        let mut ordered_uids: Vec<String> = Vec::new();
        let mut uid_set: HashSet<String> = HashSet::new();
        for u in friend_uids {
            let trimmed = u.trim();
            if !trimmed.is_empty() && trimmed != my_uid && uid_set.insert(trimmed.to_string()) {
                ordered_uids.push(trimmed.to_string());
            }
        }
        for u in shared_media_by_uid.keys() {
            let trimmed = u.trim();
            if !trimmed.is_empty() && trimmed != my_uid && uid_set.insert(trimmed.to_string()) {
                ordered_uids.push(trimmed.to_string());
            }
        }

        let batch_ids: Vec<String> = ordered_uids.into_iter().take(120).collect();
        let mut profiles = self
            .get_friend_social_profiles_batch(&batch_ids)
            .await
            .unwrap_or_default();

        // Fallback: nếu getprofiles/v2 trả rỗng thì gọi get_friend_social_profile cho 10 bạn đầu
        if profiles.is_empty() {
            for uid in batch_ids.iter().take(10) {
                if let Ok(p) = self.get_friend_social_profile(uid).await {
                    profiles.push(p);
                }
            }
        }

        if let Some(mp) = my_profile_opt {
            if !profiles.iter().any(|p| p.uid == mp.uid) {
                profiles.push(mp);
            }
        }

        // Sắp xếp hồ sơ theo thời gian hoạt động gần nhất (real-time)
        profiles.sort_by(|a, b| {
            b.last_action_time
                .unwrap_or(0)
                .cmp(&a.last_action_time.unwrap_or(0))
        });

        // 3. Nạp bài đăng Nhật ký thực tế từ cơ sở dữ liệu SQLite (nếu có bảng timeline_feed_1 / user_feed)
        let mut db_candidates = Vec::new();
        if let Ok(env_db) = std::env::var("ZALO_TIMELINE_DB") {
            if !env_db.trim().is_empty() {
                db_candidates.push(env_db);
            }
        }
        db_candidates.push("zalo_history.db".to_string());
        db_candidates.push("databases/zalo".to_string());

        for db_path in db_candidates {
            if std::path::Path::new(&db_path).exists() {
                if let Ok(real_feeds) = load_real_timeline_from_sqlite(&db_path, "") {
                    for mut rf in real_feeds {
                        if !seen_feed_ids.insert(rf.feed_id.clone()) {
                            continue;
                        }
                        if let Some(prof) = profiles.iter().find(|p| {
                            p.uid == rf.author.uid
                                || p.display_name
                                    .trim()
                                    .eq_ignore_ascii_case(rf.author.display_name.trim())
                                || p.zalo_name
                                    .trim()
                                    .eq_ignore_ascii_case(rf.author.display_name.trim())
                        }) {
                            rf.author.uid = prof.uid.clone();
                            if !prof.display_name.is_empty() {
                                rf.author.display_name = prof.display_name.clone();
                            }
                            if !prof.avatar_url.is_empty() {
                                rf.author.avatar_url = prof.avatar_url.clone();
                            }
                        }
                        items.push(rf);
                    }
                }
            }
        }

        // 4. Hợp nhất dữ liệu hoạt động thực tế từ phiên live (media chia sẻ thực tế + cập nhật tiểu sử/ảnh bìa thật từ API)
        let profile_map: HashMap<String, &SocialUserProfile> =
            profiles.iter().map(|p| (p.uid.clone(), p)).collect();

        for (uid, media_vec) in shared_media_by_uid {
            if media_vec.is_empty() {
                continue;
            }
            let feed_id = format!("shared_media_{uid}");
            if !seen_feed_ids.insert(feed_id.clone()) {
                continue;
            }
            let (display_name, avatar_url, status_text, ts_ms) =
                if let Some(prof) = profile_map.get(uid) {
                    let ts = prof
                        .last_action_time
                        .map(|t| if t < 100_000_000_000 { (t * 1000) as i64 } else { t as i64 })
                        .unwrap_or(now_ms - 1_000);
                    (
                        prof.display_name.clone(),
                        prof.avatar_url.clone(),
                        prof.status.clone(),
                        ts,
                    )
                } else {
                    (uid.clone(), String::new(), String::new(), now_ms - 1_000)
                };

            let date_str = format_vn_timestamp_ms(ts_ms);
            let content = if !status_text.trim().is_empty() {
                status_text
            } else {
                format!("Đã chia sẻ {} ảnh/video", media_vec.len())
            };

            items.push(FeedItem {
                feed_id,
                author: FeedAuthor {
                    uid: uid.clone(),
                    display_name,
                    avatar_url,
                },
                date_str,
                content,
                status_badge: Some("Ảnh chia sẻ".to_string()),
                media_list: media_vec.clone(),
                reaction: FeedReaction {
                    like_count: 0,
                    comment_count: 0,
                    is_liked: false,
                    top_reactions: Vec::new(),
                },
                comments: Vec::new(),
                timestamp_ms: ts_ms,
            });
        }

        // Với bạn bè có trạng thái (status) hoặc ảnh bìa (cover_url) thực tế từ `/api/social/friend/getprofiles/v2`
        // mà chưa có bài đăng nào trong danh sách, hiển thị thẻ hoạt động hồ sơ thực tế của họ
        for prof in &profiles {
            if prof.uid == my_uid {
                continue;
            }
            let has_status = !prof.status.trim().is_empty();
            let has_cover = !prof.cover_url.trim().is_empty();
            if !has_status && !has_cover {
                continue;
            }
            if items.iter().any(|it| it.author.uid == prof.uid) {
                continue;
            }

            let feed_id = format!("profile_activity_{}", prof.uid);
            if !seen_feed_ids.insert(feed_id.clone()) {
                continue;
            }

            let ts_ms = prof
                .last_action_time
                .map(|t| if t < 100_000_000_000 { (t * 1000) as i64 } else { t as i64 })
                .unwrap_or(0);
            let date_str = if ts_ms > 0 {
                format_vn_timestamp_ms(ts_ms)
            } else {
                "Hoạt động gần đây".to_string()
            };

            let mut media_list = Vec::new();
            if has_cover {
                let norm_cover = normalize_social_media_url(&prof.cover_url);
                media_list.push(FeedMedia {
                    photo_id: format!("cover_{}", prof.uid),
                    media_type: "image".to_string(),
                    url: norm_cover.clone(),
                    thumbnail: norm_cover,
                    width: 0,
                    height: 0,
                });
            }

            let badge = if has_status && has_cover {
                Some("Tiểu sử & Ảnh bìa".to_string())
            } else if has_status {
                Some("Trạng thái".to_string())
            } else {
                Some("Ảnh bìa".to_string())
            };

            items.push(FeedItem {
                feed_id,
                author: FeedAuthor {
                    uid: prof.uid.clone(),
                    display_name: prof.display_name.clone(),
                    avatar_url: prof.avatar_url.clone(),
                },
                date_str,
                content: prof.status.clone(),
                status_badge: badge,
                media_list,
                reaction: FeedReaction {
                    like_count: 0,
                    comment_count: 0,
                    is_liked: false,
                    top_reactions: Vec::new(),
                },
                comments: Vec::new(),
                timestamp_ms: ts_ms,
            });
        }

        // Sắp xếp toàn bộ dòng thời gian theo timestamp thực tế mới nhất lên đầu
        items.sort_by(|a, b| {
            b.timestamp_ms
                .cmp(&a.timestamp_ms)
                .then_with(|| b.feed_id.cmp(&a.feed_id))
        });

        Ok((profiles, items))
    }
}

#[derive(Debug, Clone, Default)]
struct RealtimeFeedStat {
    like_count: Option<u32>,
    comment_count: Option<u32>,
    is_liked: Option<bool>,
}

fn load_realtime_like_comment_map(conn: &rusqlite::Connection) -> HashMap<String, RealtimeFeedStat> {
    let mut map = HashMap::new();
    let exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='table_feed_real_time_like_comment'",
            [],
            |row| row.get(0),
        )
        .map(|count: i64| count > 0)
        .unwrap_or(false);

    if !exists {
        return map;
    }

    if let Ok(mut stmt) = conn.prepare(
        "SELECT fid, tot, is_like, tot_cmt FROM table_feed_real_time_like_comment",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            let fid: String = row.get(0)?;
            let tot: i64 = row.get(1).unwrap_or(-1);
            let is_like: i64 = row.get(2).unwrap_or(-1);
            let tot_cmt: i64 = row.get(3).unwrap_or(-1);
            Ok((fid, tot, is_like, tot_cmt))
        }) {
            for r in rows.flatten() {
                let (fid, tot, is_like, tot_cmt) = r;
                if fid.is_empty() {
                    continue;
                }
                map.insert(
                    fid,
                    RealtimeFeedStat {
                        like_count: if tot >= 0 { Some(tot as u32) } else { None },
                        comment_count: if tot_cmt >= 0 {
                            Some(tot_cmt as u32)
                        } else {
                            None
                        },
                        is_liked: if is_like >= 0 {
                            Some(is_like > 0)
                        } else {
                            None
                        },
                    },
                );
            }
        }
    }

    map
}

fn extract_media_from_photo_val(photo: &Value, default_dim: Option<&Value>) -> Option<FeedMedia> {
    let photoid = photo
        .get("photoid")
        .or_else(|| photo.get("photoId"))
        .or_else(|| photo.get("id"))
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .unwrap_or_default();

    let raw_origin = photo
        .get("origin")
        .or_else(|| photo.get("url"))
        .or_else(|| photo.get("hd"))
        .or_else(|| photo.get("large"))
        .or_else(|| photo.get("thumb"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let raw_video = photo
        .get("videourl")
        .or_else(|| photo.get("videoUrl"))
        .or_else(|| photo.get("streamingUrl"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let (media_type, primary_url) = if !raw_video.is_empty() {
        ("video".to_string(), normalize_social_media_url(raw_video))
    } else if !raw_origin.is_empty() {
        ("image".to_string(), normalize_social_media_url(raw_origin))
    } else {
        return None;
    };

    let thumbnail = photo
        .get("thumb")
        .or_else(|| photo.get("thumbnail"))
        .and_then(Value::as_str)
        .map(normalize_social_media_url)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| primary_url.clone());

    let dim = photo.get("photo_dimension").or(default_dim);
    let width = dim
        .and_then(|d| d.get("width"))
        .or_else(|| photo.get("width"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let height = dim
        .and_then(|d| d.get("height"))
        .or_else(|| photo.get("height"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;

    Some(FeedMedia {
        photo_id: photoid,
        media_type,
        url: primary_url,
        thumbnail,
        width,
        height,
    })
}

fn parse_feed_item_json(
    val: &Value,
    target_uid: &str,
    rt_stats: &HashMap<String, RealtimeFeedStat>,
) -> Option<FeedItem> {
    let item = val.get("item").unwrap_or(val);
    let header = item.get("header")?;

    let author_uid = header
        .get("uid")
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .unwrap_or_default();
    let author_dpn = header
        .get("dpn")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let matches_target = target_uid.is_empty()
        || author_uid == target_uid
        || author_dpn.trim().eq_ignore_ascii_case(target_uid.trim());
    if !matches_target {
        return None;
    }

    let fid = item
        .get("fid")
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .unwrap_or_default();
    if fid.is_empty() {
        return None;
    }

    let cts = item.get("cts").and_then(Value::as_i64).unwrap_or(0);
    let date_str = format_vn_timestamp_ms(cts);

    let author = FeedAuthor {
        uid: author_uid,
        display_name: author_dpn,
        avatar_url: header
            .get("avt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    };

    let content_obj = item.get("content").cloned().unwrap_or(json!({}));
    let caption = content_obj
        .get("caption")
        .or_else(|| content_obj.get("msg"))
        .or_else(|| content_obj.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let default_dim = content_obj.get("photo_dimension");
    let mut media_list = Vec::new();
    let mut seen_urls = HashSet::new();

    if let Some(photo) = content_obj.get("photo") {
        if let Some(arr) = photo.as_array() {
            for p in arr {
                if let Some(m) = extract_media_from_photo_val(p, default_dim) {
                    if seen_urls.insert(m.url.clone()) {
                        media_list.push(m);
                    }
                }
            }
        } else if let Some(m) = extract_media_from_photo_val(photo, default_dim) {
            if seen_urls.insert(m.url.clone()) {
                media_list.push(m);
            }
        }
    }

    for key in ["photos", "items", "media_items", "medias"] {
        if let Some(arr) = content_obj.get(key).and_then(Value::as_array) {
            for p in arr {
                let inner = p.get("photo").unwrap_or(p);
                if let Some(m) = extract_media_from_photo_val(inner, default_dim) {
                    if seen_urls.insert(m.url.clone()) {
                        media_list.push(m);
                    }
                }
            }
        }
    }

    if let Some(video) = content_obj.get("video") {
        if let Some(m) = extract_media_from_photo_val(video, default_dim) {
            if seen_urls.insert(m.url.clone()) {
                media_list.push(m);
            }
        }
    }

    let footer = item.get("footer").cloned().unwrap_or(json!({}));
    let mut numlike = footer.get("numlike").and_then(Value::as_u64).unwrap_or(0) as u32;
    let mut numcmt = footer.get("numcmt").and_then(Value::as_u64).unwrap_or(0) as u32;
    let mut is_liked = item.get("islike").and_then(Value::as_i64).unwrap_or(0) > 0;

    let mut top_reactions = Vec::new();
    if let Some(rinfo) = footer.get("reaction_info") {
        if rinfo
            .get("current_reaction_id")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            > 0
        {
            is_liked = true;
        }
        if let Some(top_arr) = rinfo.get("top_reactions").and_then(Value::as_array) {
            for r in top_arr {
                if let Some(rid) = r.as_i64() {
                    let label = reaction_id_to_label(rid).to_string();
                    if !top_reactions.contains(&label) {
                        top_reactions.push(label);
                    }
                }
            }
        }
    }
    if top_reactions.is_empty() && numlike > 0 {
        top_reactions.push("Thích".to_string());
    }

    // Ghi đè bằng số liệu real-time trong table_feed_real_time_like_comment nếu có
    if let Some(rt) = rt_stats.get(&fid) {
        if let Some(lk) = rt.like_count {
            numlike = lk;
        }
        if let Some(cm) = rt.comment_count {
            numcmt = cm;
        }
        if let Some(il) = rt.is_liked {
            is_liked = il;
        }
    }

    let privacy = item.get("privacy").and_then(Value::as_i64).unwrap_or(0);
    let status_badge = match privacy {
        2 => Some("Chỉ mình tôi".to_string()),
        1 => Some("Bạn bè".to_string()),
        _ => None,
    };

    Some(FeedItem {
        feed_id: fid,
        author,
        date_str,
        content: caption,
        status_badge,
        media_list,
        reaction: FeedReaction {
            like_count: numlike,
            comment_count: numcmt,
            is_liked,
            top_reactions,
        },
        comments: Vec::new(),
        timestamp_ms: cts,
    })
}

/// Nạp dữ liệu bài viết thật từ bảng `timeline_feed_1`, `user_feed` và hợp nhất số liệu real-time từ `table_feed_real_time_like_comment`
pub fn load_real_timeline_from_sqlite(db_path: &str, target_uid: &str) -> Result<Vec<FeedItem>> {
    let conn = rusqlite::Connection::open(db_path)?;
    let rt_stats = load_realtime_like_comment_map(&conn);

    let mut feeds = Vec::new();
    let mut seen_fids = HashSet::new();

    for table_name in ["timeline_feed_1", "user_feed"] {
        let table_exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table_name],
                |row| row.get(0),
            )
            .map(|count: i64| count > 0)
            .unwrap_or(false);

        if !table_exists {
            continue;
        }

        let query = format!("SELECT content FROM {table_name}");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            let content_str: String = row.get(0)?;
            Ok(content_str)
        })?;

        for r in rows {
            let content_str = r?;
            if let Ok(val) = serde_json::from_str::<Value>(&content_str) {
                if let Some(feed_item) = parse_feed_item_json(&val, target_uid, &rt_stats) {
                    if seen_fids.insert(feed_item.feed_id.clone()) {
                        feeds.push(feed_item);
                    }
                }
            }
        }
    }

    // Sắp xếp bài đăng mới nhất (timestamp_ms lớn nhất) lên đầu danh sách
    feeds.sort_by(|a, b| {
        b.timestamp_ms
            .cmp(&a.timestamp_ms)
            .then_with(|| b.feed_id.cmp(&a.feed_id))
    });

    Ok(feeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn social_profile_and_feed_packet_encode_correctly() {
        let payload = packet::build_get_feed_payload(435550501, 435550501, 0, 1, 20);
        assert_eq!(payload.len(), 52, "Payload pw0.e.d phải chuẩn 52 bytes");
        assert_eq!(payload[0], 0x01);
        assert_eq!(
            i32::from_be_bytes([payload[1], payload[2], payload[3], payload[4]]),
            packet::CLIENT_VERSION
        );
        assert_eq!(payload[9], 0); // tab = 0
        assert_eq!(
            i32::from_be_bytes([payload[10], payload[11], payload[12], payload[13]]),
            1
        ); // page = 1
        assert_eq!(
            i32::from_be_bytes([payload[14], payload[15], payload[16], payload[17]]),
            20
        ); // count = 20

        let pkt = packet::build_request_packet(
            packet::CMD_FEED_TIMELINE,
            packet::SUBCMD_GET_FEED,
            &payload,
        );
        assert_eq!(pkt.len(), 4 + 16 + 52);
        assert_eq!(pkt[4], 0x01); // version = 1
        assert_eq!(pkt[5], 0x00); // encrypt = 0
        assert_eq!(pkt[10], packet::CMD_TYPE_REQUEST); // cmd_type = 3
        assert_eq!(u16::from_be_bytes([pkt[11], pkt[12]]), 1230); // CMD_FEED_TIMELINE = 1230
        assert_eq!(pkt[13], 6); // SUBCMD_GET_FEED = 6

        let user_payload = packet::build_user_timeline_payload(257404931, 0);
        assert_eq!(user_payload.len(), 18, "Payload mw0.c.k phải chuẩn 18 bytes");

        let prof_val = json!({
            "displayName": "Nguyễn Thành",
            "zaloName": "Thanh",
            "avatar": "https://s120.ava.talk.zdn.vn/a.jpg",
            "cover": "https://cover-talk.zadn.vn/c.jpg",
            "gender": 0,
            "sdob": "01/01/2000",
            "status": "Zalo Rust",
            "isFr": 1,
            "lastActionTime": 1762958439000u64
        });
        let p = SocialUserProfile::from_json("435550501", &prof_val).unwrap();
        assert_eq!(p.display_name, "Nguyễn Thành");
        assert!(p.is_friend);
        assert_eq!(p.cover_url, "https://cover-talk.zadn.vn/c.jpg");
        assert_eq!(p.last_action_time, Some(1762958439000u64));
    }

    #[test]
    fn load_real_dtc_timeline_from_sqlite_works() {
        let db_path = "test_lab/databases/zalo";
        if std::path::Path::new(db_path).exists() {
            let feeds = load_real_timeline_from_sqlite(db_path, "257404931").unwrap();
            assert_eq!(
                feeds.len(),
                2,
                "Phải nạp được đúng 2 bài viết thật của Đtc"
            );
            assert!(
                feeds[0].timestamp_ms >= feeds[1].timestamp_ms,
                "Bài mới hơn phải xếp trước"
            );

            // Kiểm tra bài đăng 1 (FPT University)
            let f1 = feeds
                .iter()
                .find(|f| f.feed_id == "18223066329")
                .expect("Phải có feed 18223066329");
            assert_eq!(f1.author.display_name, "Đtc");
            assert_eq!(f1.reaction.like_count, 16);
            assert_eq!(f1.reaction.comment_count, 6);
            assert!(f1.content.contains("FPT University"));
            assert!(!f1.media_list.is_empty());
            assert!(f1.media_list[0].url.ends_with(".jpg"));
            assert_eq!(f1.timestamp_ms, 1726408850459);

            // Kiểm tra bài đăng 2 (Bão lũ an toàn)
            let f2 = feeds
                .iter()
                .find(|f| f.feed_id == "18193461996")
                .expect("Phải có feed 18193461996");
            assert_eq!(f2.author.display_name, "Đtc");
            assert_eq!(f2.reaction.like_count, 6);
            assert_eq!(f2.reaction.comment_count, 0);
            assert!(f2.content.contains("an toàn"));
            assert!(!f2.media_list.is_empty());
            assert!(f2.media_list[0].url.ends_with(".jpg"));
            assert_eq!(f2.timestamp_ms, 1725774890913);
        }
    }
}
