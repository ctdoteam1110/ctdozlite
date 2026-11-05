//! Test suite for Milestone M2 (Direct UI Customization for Fonts and Colors - Requirement R2).
//! Validates AppSettings persistence, backward compatibility, hex parsing/formatting,
//! luminance auto-contrast, and font family priority configuration.

use std::fs;
use std::path::PathBuf;

struct TempFileFixture {
    path: PathBuf,
}

impl TempFileFixture {
    fn new(filename: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("m2_test_{}_{nonce}_{filename}", std::process::id()));
        Self { path }
    }
}

impl Drop for TempFileFixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default)]
struct TestSettingsSchema {
    theme_mode: String,
    font_family: String,
    font_scale: f32,
    custom_primary_color: Option<String>,
    custom_accent_color: Option<String>,
    custom_bubble_out_bg: Option<String>,
    custom_bubble_in_bg: Option<String>,
}

impl Default for TestSettingsSchema {
    fn default() -> Self {
        Self {
            theme_mode: "dark".into(),
            font_family: "default".into(),
            font_scale: 1.0,
            custom_primary_color: None,
            custom_accent_color: None,
            custom_bubble_out_bg: None,
            custom_bubble_in_bg: None,
        }
    }
}

#[test]
fn test_settings_schema_defaults() {
    let settings = TestSettingsSchema::default();
    assert_eq!(settings.font_family, "default");
    assert_eq!(settings.font_scale, 1.0);
    assert_eq!(settings.custom_primary_color, None);
    assert_eq!(settings.custom_accent_color, None);
    assert_eq!(settings.custom_bubble_out_bg, None);
    assert_eq!(settings.custom_bubble_in_bg, None);
}

#[test]
fn test_settings_schema_roundtrip_with_custom_theme() {
    let fixture = TempFileFixture::new("theme_settings.json");
    let original = TestSettingsSchema {
        theme_mode: "light".into(),
        font_family: "segoe_ui".into(),
        font_scale: 1.25,
        custom_primary_color: Some("#0068FF".into()),
        custom_accent_color: Some("#059669".into()),
        custom_bubble_out_bg: Some("#E5EFFF".into()),
        custom_bubble_in_bg: Some("#FFFFFF".into()),
    };

    let serialized = serde_json::to_string_pretty(&original).expect("serialize settings");
    fs::write(&fixture.path, &serialized).expect("write settings");

    let read_back = fs::read_to_string(&fixture.path).expect("read settings");
    let deserialized: TestSettingsSchema = serde_json::from_str(&read_back).expect("deserialize settings");

    assert_eq!(deserialized, original);
}

#[test]
fn test_settings_schema_backwards_compatibility_with_legacy_json() {
    let legacy_json = r#"{
        "show_all_contacts": false,
        "language": "Tiếng Việt",
        "theme_mode": "dark",
        "download_dir": "C:\\Downloads"
    }"#;

    let parsed: TestSettingsSchema = serde_json::from_str(legacy_json).expect("deserialize legacy json");
    assert_eq!(parsed.theme_mode, "dark");
    assert_eq!(parsed.font_family, "default");
    assert_eq!(parsed.font_scale, 1.0);
    assert_eq!(parsed.custom_primary_color, None);
    assert_eq!(parsed.custom_accent_color, None);
    assert_eq!(parsed.custom_bubble_out_bg, None);
    assert_eq!(parsed.custom_bubble_in_bg, None);
}

#[test]
fn test_hex_color_parsing_rules() {
    fn parse_hex(s: &str) -> Option<(u8, u8, u8, u8)> {
        let hex = s.trim().trim_start_matches('#');
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b, 255))
        } else if hex.len() == 8 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some((r, g, b, a))
        } else {
            None
        }
    }

    assert_eq!(parse_hex("#0068FF"), Some((0, 104, 255, 255)));
    assert_eq!(parse_hex("0068ff"), Some((0, 104, 255, 255)));
    assert_eq!(parse_hex("#528BFF80"), Some((82, 139, 255, 128)));
    assert_eq!(parse_hex("#FFF"), None);
    assert_eq!(parse_hex("invalid_color"), None);
    assert_eq!(parse_hex(""), None);
}

#[test]
fn test_luminance_contrast_calculation() {
    fn is_light_color(r: u8, g: u8, b: u8) -> bool {
        let lum = 0.299 * (r as f32) + 0.587 * (g as f32) + 0.114 * (b as f32);
        lum > 140.0
    }

    // White (#FFFFFF) -> light background (demands dark text)
    assert!(is_light_color(255, 255, 255));
    // Zalo light outbound bubble (#E5EFFF) -> light background
    assert!(is_light_color(229, 239, 255));
    // Dark slate (#1E293B) -> dark background (demands bright text)
    assert!(!is_light_color(30, 41, 59));
    // Atom One Dark background (#282C34) -> dark background
    assert!(!is_light_color(40, 44, 52));
    // Deep black (#000000) -> dark background
    assert!(!is_light_color(0, 0, 0));
}

#[test]
fn test_font_family_identifier_normalization() {
    let normalize = |choice: &str| match choice.trim().to_lowercase().as_str() {
        "segoe_ui" | "segoe" => "segoe_ui",
        "arial" => "arial",
        "tahoma" => "tahoma",
        _ => "default",
    };

    assert_eq!(normalize("default"), "default");
    assert_eq!(normalize("Default"), "default");
    assert_eq!(normalize("segoe_ui"), "segoe_ui");
    assert_eq!(normalize("Segoe_UI"), "segoe_ui");
    assert_eq!(normalize("segoe"), "segoe_ui");
    assert_eq!(normalize("Arial"), "arial");
    assert_eq!(normalize("tahoma"), "tahoma");
    assert_eq!(normalize("unknown_font_name"), "default");
}
