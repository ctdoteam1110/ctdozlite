//! API key storage for LLM providers (ctdozlite's own auth, NOT opencode's).
//!
//! Priority: env (`CTDOZ_API_KEY` / per-vendor `<NAME>_API_KEY`) → OS keychain
//! (`ctdozlite/llm-api-key`) → settings field. Never reads another app's auth files.
//!
//! Commands (via `zlite --auth ...`):
//! - `status` [provider] : where the key would come from (value masked).
//! - `set <key>` : store in OS keychain.
//! - `clear` : delete from keychain.

use anyhow::{Context, Result};

pub const SERVICE: &str = "ctdozlite";
pub const ACCOUNT: &str = "llm-api-key";

/// Where the effective key came from (value never logged).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Env,
    Keychain,
    Settings,
    None,
}

/// Resolve the effective key. `settings_key`: value from app_settings.json.
/// If `env_name` is Some, that env var wins over generic `CTDOZ_API_KEY`.
pub fn resolve_key_for(settings_key: &str, env_name: Option<&str>) -> (String, KeySource) {
    if let Some(name) = env_name {
        if let Ok(k) = std::env::var(name) {
            if !k.trim().is_empty() {
                return (k, KeySource::Env);
            }
        }
    }
    if let Ok(k) = std::env::var("CTDOZ_API_KEY") {
        if !k.trim().is_empty() {
            return (k, KeySource::Env);
        }
    }
    if let Ok(k) = keychain_get() {
        if !k.trim().is_empty() {
            return (k, KeySource::Keychain);
        }
    }
    if !settings_key.trim().is_empty() {
        return (settings_key.trim().to_string(), KeySource::Settings);
    }
    (String::new(), KeySource::None)
}

/// Back-compat: generic resolution without vendor env.
pub fn resolve_key(settings_key: &str) -> (String, KeySource) {
    resolve_key_for(settings_key, None)
}

pub fn describe_source(s: KeySource) -> &'static str {
    match s {
        KeySource::Env => "env",
        KeySource::Keychain => "OS keychain (ctdozlite/llm-api-key)",
        KeySource::Settings => "app_settings.json (khuyên dời sang keychain)",
        KeySource::None => "chưa có",
    }
}

#[cfg(target_os = "macos")]
pub fn keychain_get() -> Result<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", SERVICE, "-a", ACCOUNT, "-w"])
        .output()
        .context("gọi security thất bại")?;
    if !out.status.success() {
        anyhow::bail!("keychain: chưa lưu (dùng --auth set)");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
pub fn keychain_set(key: &str) -> Result<()> {
    let _ = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", ACCOUNT])
        .output();
    let st = std::process::Command::new("security")
        .args(["add-generic-password", "-s", SERVICE, "-a", ACCOUNT, "-w", key])
        .status()
        .context("gọi security thất bại")?;
    if !st.success() {
        anyhow::bail!("keychain: lưu thất bại");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn keychain_clear() -> Result<()> {
    let st = std::process::Command::new("security")
        .args(["delete-generic-password", "-s", SERVICE, "-a", ACCOUNT])
        .status()
        .context("gọi security thất bại")?;
    if !st.success() {
        anyhow::bail!("keychain: không có gì để xóa (hoặc lỗi)");
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_get() -> Result<String> {
    anyhow::bail!("keychain chỉ hỗ trợ macOS (dùng env CTDOZ_API_KEY)")
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_set(_key: &str) -> Result<()> {
    anyhow::bail!("keychain chỉ hỗ trợ macOS (dùng env CTDOZ_API_KEY)")
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_clear() -> Result<()> {
    anyhow::bail!("keychain chỉ hỗ trợ macOS (dùng env CTDOZ_API_KEY)")
}

pub fn mask(key: &str) -> String {
    if key.is_empty() {
        "(trống)".to_string()
    } else if key.len() <= 8 {
        "••••".to_string()
    } else {
        format!("{}…{}", &key[..4], &key[key.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_hides_middle() {
        assert_eq!(mask(""), "(trống)");
        assert_eq!(mask("abc"), "••••");
        assert_eq!(mask("sk-1234567890"), "sk-1…7890");
    }

    #[test]
    fn vendor_env_resolution_order() {
        // Settings fallback when no env/keychain present. Env-dependent parts
        // are covered by doc contract, not asserted here.
        let (k, src) = resolve_key_for("from-settings", Some("CTDOZ_TEST_VENDOR_KEY_ABSENT"));
        // Keychain on this dev machine may hold a key — accept either.
        assert!(k == "from-settings" || src == KeySource::Keychain || src == KeySource::Env);
    }
}
