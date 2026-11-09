//! Working endpoint table, taken from a live Zalo Web session (2026-10-21).
//!
//! Replaces guesswork with observed values. Every entry below answered `error_code 0`
//! during a real QR-authenticated session.

/// Service map as returned by `zpw_service_map_v3` on `chat.zalo.me`.
///
/// Note the domains are `*.chat.zalo.me`, **not** `*.zaloapp.com` — the PC bundle's
/// default table is stale for the web/QR session flavour.
pub const SERVICE_MAP_V3: &[(&str, &str)] = &[
    ("profile", "https://tt-profile-wpa.chat.zalo.me"),
    ("friend", "https://tt-friend-wpa.chat.zalo.me"),
    ("conversation", "https://tt-convers-wpa.chat.zalo.me"),
    ("chat", "https://tt-chat3-wpa.chat.zalo.me"),
    ("group", "https://tt-group-wpa.chat.zalo.me"),
    ("sticker", "https://tt-sticker-wpa.chat.zalo.me"),
    ("alias", "https://tt-alias-wpa.chat.zalo.me"),
    ("label", "https://label-wpa.chat.zalo.me"),
    ("reaction", "https://reaction.chat.zalo.me"),
    ("emoji_status", "https://emoji-status.chat.zalo.me"),
    ("zcloud", "https://zcld.chat.zalo.me"),
    ("file", "https://tt-files-wpa.chat.zalo.me"),
    ("voice_call", "https://voicecall-wpa.chat.zalo.me"),
    // `getGroupBoardDomain()` — pinned topics (`/api/board/pin/list`, cmd 12001). The PC
    // bundle's legacy host is `groupboard-wpa.api.zaloapp.com` (re-notes/01_login.md).
    ("group_board", "https://groupboard-wpa.chat.zalo.me"),
];

/// Animated sticker frame strip: `?eid={sticker_id}&size=130` returns one PNG with the frames
/// side by side (width = frames × height; 130×130 for static stickers). Zalo PC plays them at
/// `duration || 250` ms per frame (`_getStickerUrl` / `start()` in the PC bundle). Verified
/// against live responses, e.g. eid 32063 → 3120×130 (24 frames).
pub const STICKER_SPRITE: &str = "https://zalo-api.zadn.vn/api/emoticon/sprite";

/// Legacy fallback for [`SERVICE_MAP_V3`]'s `group_board` entry.
pub const GROUP_BOARD_LEGACY: &str = "https://groupboard-wpa.api.zaloapp.com";

/// `zpw_ctrs_new[0]` — conversation control plane.
pub const CONVERS_CTL: &str = "https://p5-ctl-chat.zaloapp.com";

/// `zpw_chts_new[0]` — message plane.
pub const MSG_CHAT: &str = "https://p5-msg-chat.zaloapp.com";

/// Auth host for `reqqr` / password login.
pub const AUTH: &str = "https://wpa.zaloapp.com";

/// Base host that serves `getServerInfo` for an authenticated web session.
pub const WPA_CHAT: &str = "https://wpa.chat.zalo.me";

/// Look up a service domain by key.
pub fn service(key: &str) -> Option<&'static str> {
    SERVICE_MAP_V3
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
}
/// Whether a host belongs to the authenticated Zalo API service family.
pub fn is_zalo_api_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    ["zalo.me", "zaloapp.com"]
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

/// Whether a host is an approved Zalo API or signed-download CDN host.
pub fn is_zalo_download_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    is_zalo_api_host(&host) || host == "zdn.vn" || host.ends_with(".zdn.vn")
}

#[cfg(test)]
mod download_host_tests {
    use super::*;
    #[test]
    fn host_checks_require_exact_domain_boundary() {
        assert!(is_zalo_api_host("file.zalo.me"));
        assert!(is_zalo_download_host("PHOTO.ZDN.VN"));
        assert!(!is_zalo_api_host("photo.zdn.vn"));
        for host in [
            "evilzalo.me",
            "zalo.me.evil.test",
            "evilzdn.vn",
            "zdn.vn.evil.test",
        ] {
            assert!(!is_zalo_download_host(host)); // (INV-2)
        }
    }
}
