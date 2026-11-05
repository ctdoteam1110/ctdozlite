//! Empirical adversarial stress testing suite for Milestone M2 (Requirement R2).
//! Validates:
//! 1. Malformed hex color parsing and formatting under adversarial edge cases.
//! 2. Extreme luminance values and WCAG text contrast readability calculations.
//! 3. Font switching, missing font files, invalid names, and fallback chains.
//! 4. AppSettings persistence round-trip with extreme, corrupted, and boundary values.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use eframe::egui::{self, Color32};
use zalo_rust::gui::AppSettings;

static STRESS_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempStressFile {
    path: PathBuf,
}

impl TempStressFile {
    fn new(suffix: &str) -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = STRESS_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "m2_stress_{}_{nonce}_{seq}_{suffix}",
            std::process::id()
        ));
        Self { path }
    }
}

impl Drop for TempStressFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

// Mirror the exact algorithms from src/gui/theme.rs
fn parse_hex_color(s: &str) -> Option<Color32> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
    } else if hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some(Color32::from_rgba_unmultiplied(r, g, b, a))
    } else {
        None
    }
}

fn format_hex_color(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct MockPalette {
    primary: Color32,
    primary_hover: Color32,
    accent_online: Color32,
    bubble_out_bg: Color32,
    bubble_out_text: Color32,
    bubble_out_meta: Color32,
    bubble_in_bg: Color32,
    bubble_in_text: Color32,
    bubble_in_meta: Color32,
}

impl Default for MockPalette {
    fn default() -> Self {
        Self {
            primary: Color32::from_rgb(0, 104, 255),
            primary_hover: Color32::from_rgb(25, 129, 255),
            accent_online: Color32::from_rgb(5, 150, 105),
            bubble_out_bg: Color32::from_rgb(0, 104, 255),
            bubble_out_text: Color32::from_rgb(235, 240, 246),
            bubble_out_meta: Color32::from_rgb(154, 171, 192),
            bubble_in_bg: Color32::from_rgb(37, 43, 54),
            bubble_in_text: Color32::from_rgb(215, 218, 224),
            bubble_in_meta: Color32::from_rgb(142, 150, 165),
        }
    }
}

impl MockPalette {
    fn apply_custom_colors(
        &mut self,
        primary_hex: Option<&str>,
        accent_hex: Option<&str>,
        bubble_out_hex: Option<&str>,
        bubble_in_hex: Option<&str>,
    ) {
        if let Some(hex) = primary_hex {
            if let Some(c) = parse_hex_color(hex) {
                self.primary = c;
                self.primary_hover = Color32::from_rgb(
                    c.r().saturating_add(25),
                    c.g().saturating_add(25),
                    c.b().saturating_add(25),
                );
            }
        }
        if let Some(hex) = accent_hex {
            if let Some(c) = parse_hex_color(hex) {
                self.accent_online = c;
            }
        }
        if let Some(hex) = bubble_out_hex {
            if let Some(c) = parse_hex_color(hex).filter(|c| c.a() == u8::MAX) {
                self.bubble_out_bg = c;
                let lum = 0.299 * (c.r() as f32) + 0.587 * (c.g() as f32) + 0.114 * (c.b() as f32);
                if lum > 140.0 {
                    self.bubble_out_text = Color32::from_rgb(15, 23, 42);
                    self.bubble_out_meta = Color32::from_rgb(71, 92, 122);
                } else {
                    self.bubble_out_text = Color32::from_rgb(235, 240, 246);
                    self.bubble_out_meta = Color32::from_rgb(154, 171, 192);
                }
            }
        }
        if let Some(hex) = bubble_in_hex {
            if let Some(c) = parse_hex_color(hex).filter(|c| c.a() == u8::MAX) {
                self.bubble_in_bg = c;
                let lum = 0.299 * (c.r() as f32) + 0.587 * (c.g() as f32) + 0.114 * (c.b() as f32);
                if lum > 140.0 {
                    self.bubble_in_text = Color32::from_rgb(15, 23, 42);
                    self.bubble_in_meta = Color32::from_rgb(100, 116, 139);
                } else {
                    self.bubble_in_text = Color32::from_rgb(215, 218, 224);
                    self.bubble_in_meta = Color32::from_rgb(142, 150, 165);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Adversarial Color Parsing Stress Tests
// ---------------------------------------------------------------------------

#[test]
fn test_adversarial_malformed_hex_strings_never_panic_and_return_none() {
    let malformed_inputs = [
        "#123",
        "#xyz",
        "not-a-color",
        "#FFFFFFFFF",
        "",
        "   ",
        "#",
        "##",
        "#1",
        "#12",
        "#1234",
        "#12345",
        "#1234567",
        "#123456789",
        "#GGGGGG",
        "#ZZZZZZ",
        "rgb(255, 0, 0)",
        "hsl(0, 100%, 50%)",
        "# 123456",
        "#12 3456",
        "#12345 6",
        "#123456!",
        "#123456_extra",
        "\\0#123456",
        "undefined",
        "null",
        "NaN",
        "[object Object]",
        "#0068FF\0extra",
        "#００６８ＦＦ",
        "red",
        "blue",
        "#fffffg",
        "+#0068FF",
        "-#0068FF",
    ];

    for input in malformed_inputs {
        let result = parse_hex_color(input);
        assert_eq!(
            result, None,
            "Expected malformed input '{input}' to return None, got {result:?}"
        );
    }
}

#[test]
fn test_adversarial_huge_hex_string_handled_safely() {
    let huge_string = format!("#{}", "A".repeat(100_000));
    assert_eq!(parse_hex_color(&huge_string), None);

    // Trimming 50,000 outer spaces must succeed safely and yield the valid color
    let huge_outer_spaces = format!("   #0068FF   {}", " ".repeat(50_000));
    assert_eq!(
        parse_hex_color(&huge_outer_spaces),
        Some(Color32::from_rgb(0, 104, 255))
    );

    // Internal space within a huge string must be safely rejected
    let huge_internal_spaces = format!("   #00 68FF   {}", " ".repeat(50_000));
    assert_eq!(parse_hex_color(&huge_internal_spaces), None);
}

#[test]
fn test_valid_hex_parsing_variations() {
    // 6-digit with #
    assert_eq!(
        parse_hex_color("#0068FF"),
        Some(Color32::from_rgb(0, 104, 255))
    );
    // 6-digit lowercase
    assert_eq!(
        parse_hex_color("#0068ff"),
        Some(Color32::from_rgb(0, 104, 255))
    );
    // 6-digit without #
    assert_eq!(
        parse_hex_color("0068FF"),
        Some(Color32::from_rgb(0, 104, 255))
    );
    // 6-digit with outer whitespace
    assert_eq!(
        parse_hex_color("  #059669  "),
        Some(Color32::from_rgb(5, 150, 105))
    );
    // 8-digit with alpha
    assert_eq!(
        parse_hex_color("#528BFF80"),
        Some(Color32::from_rgba_unmultiplied(82, 139, 255, 128))
    );
    // 8-digit without #
    assert_eq!(
        parse_hex_color("E11D48FF"),
        Some(Color32::from_rgba_unmultiplied(225, 29, 72, 255))
    );
}

#[test]
fn test_color_format_roundtrip_property() {
    // Test 256 sample colors across RGB spectrum
    for step in (0..=255).step_by(17) {
        let r = step as u8;
        let g = ((step * 3) % 256) as u8;
        let b = ((step * 7) % 256) as u8;
        let col = Color32::from_rgb(r, g, b);

        let hex = format_hex_color(col);
        assert_eq!(hex.len(), 7, "Hex must be #RRGGBB");
        assert!(hex.starts_with('#'), "Hex must start with #");

        let parsed = parse_hex_color(&hex).expect("Must parse formatted hex");
        assert_eq!(parsed.r(), r);
        assert_eq!(parsed.g(), g);
        assert_eq!(parsed.b(), b);
        assert_eq!(parsed.a(), 255);
    }

    // Translucent color format and roundtrip with alpha
    let translucent = Color32::from_rgba_unmultiplied(82, 139, 255, 128);
    let hex8 = format_hex_color(translucent);
    assert_eq!(hex8, "#528BFF80");
    let parsed8 = parse_hex_color(&hex8).expect("Must parse 8-digit hex");
    assert_eq!(parsed8, translucent);
}

// ---------------------------------------------------------------------------
// 2. Extreme Luminance and Text Contrast Readability Tests
// ---------------------------------------------------------------------------

/// Calculate WCAG 2.1 relative luminance
fn wcag_relative_luminance(c: Color32) -> f64 {
    let to_linear = |val: u8| -> f64 {
        let s = (val as f64) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * to_linear(c.r()) + 0.7152 * to_linear(c.g()) + 0.0722 * to_linear(c.b())
}

/// Calculate WCAG 2.1 contrast ratio between two colors: (L1 + 0.05) / (L2 + 0.05)
fn wcag_contrast_ratio(c1: Color32, c2: Color32) -> f64 {
    let l1 = wcag_relative_luminance(c1);
    let l2 = wcag_relative_luminance(c2);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

#[test]
fn test_extreme_luminance_contrast_readability() {
    let mut pal = MockPalette::default();

    // 1. Pure Black (#000000)
    pal.apply_custom_colors(None, None, Some("#000000"), Some("#000000"));
    assert_eq!(pal.bubble_out_bg, Color32::from_rgb(0, 0, 0));
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(235, 240, 246)); // light text
    let ratio_out_black = wcag_contrast_ratio(pal.bubble_out_bg, pal.bubble_out_text);
    assert!(
        ratio_out_black >= 15.0,
        "Contrast on pure black must be >= 15.0, got {ratio_out_black}"
    );

    // 2. Pure White (#FFFFFF)
    pal.apply_custom_colors(None, None, Some("#FFFFFF"), Some("#FFFFFF"));
    assert_eq!(pal.bubble_out_bg, Color32::from_rgb(255, 255, 255));
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(15, 23, 42)); // dark text
    let ratio_out_white = wcag_contrast_ratio(pal.bubble_out_bg, pal.bubble_out_text);
    assert!(
        ratio_out_white >= 15.0,
        "Contrast on pure white must be >= 15.0, got {ratio_out_white}"
    );

    // 3. Neon Green (#00FF00) - high human eye sensitivity
    pal.apply_custom_colors(None, None, Some("#00FF00"), Some("#00FF00"));
    assert_eq!(pal.bubble_out_bg, Color32::from_rgb(0, 255, 0));
    // lum = 0.587 * 255 = 149.685 > 140.0 -> Must pick dark text!
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(15, 23, 42));
    let ratio_green = wcag_contrast_ratio(pal.bubble_out_bg, pal.bubble_out_text);
    assert!(
        ratio_green >= 10.0,
        "Contrast on neon green must be >= 10.0 (WCAG AAA), got {ratio_green}"
    );

    // 4. Neon Blue (#0000FF) - low human eye sensitivity
    pal.apply_custom_colors(None, None, Some("#0000FF"), Some("#0000FF"));
    assert_eq!(pal.bubble_out_bg, Color32::from_rgb(0, 0, 255));
    // lum = 0.114 * 255 = 29.07 <= 140.0 -> Must pick light text!
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(235, 240, 246));
    let ratio_blue = wcag_contrast_ratio(pal.bubble_out_bg, pal.bubble_out_text);
    assert!(
        ratio_blue >= 7.0,
        "Contrast on neon blue must be >= 7.0 (WCAG AAA), got {ratio_blue}"
    );

    // 5. Neon Yellow (#FFFF00)
    pal.apply_custom_colors(None, None, Some("#FFFF00"), Some("#FFFF00"));
    assert_eq!(pal.bubble_out_bg, Color32::from_rgb(255, 255, 0));
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(15, 23, 42)); // dark text
    let ratio_yellow = wcag_contrast_ratio(pal.bubble_out_bg, pal.bubble_out_text);
    assert!(
        ratio_yellow >= 14.0,
        "Contrast on neon yellow must be >= 14.0, got {ratio_yellow}"
    );

    // 6. Transparent colors (#00000000, #FFFFFF00) - must be rejected, preserving valid palette
    let prev_out_bg = pal.bubble_out_bg;
    let prev_in_bg = pal.bubble_in_bg;
    pal.apply_custom_colors(None, None, Some("#00000000"), Some("#FFFFFF00"));
    assert_eq!(pal.bubble_out_bg, prev_out_bg);
    assert_eq!(pal.bubble_in_bg, prev_in_bg);
}

#[test]
fn test_luminance_threshold_boundary_continuity() {
    let mut pal = MockPalette::default();

    // Just below threshold: lum = 140.0 exactly (140, 140, 140)
    pal.apply_custom_colors(None, None, Some("#8C8C8C"), None);
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(235, 240, 246)); // <= 140 -> light

    // Just above threshold: lum = 141.0 (141, 141, 141) -> #8D8D8D
    pal.apply_custom_colors(None, None, Some("#8D8D8D"), None);
    assert_eq!(pal.bubble_out_text, Color32::from_rgb(15, 23, 42)); // > 140 -> dark
}

#[test]
fn test_palette_apply_malformed_hex_preserves_previous_state() {
    let mut pal = MockPalette::default();
    let original = pal.clone();

    // Applying all malformed strings must NOT corrupt or change existing colors
    pal.apply_custom_colors(
        Some("#invalid1"),
        Some("not-a-color"),
        Some("#123"),
        Some("#FFFFFFFFF"),
    );
    assert_eq!(
        pal, original,
        "Malformed overrides must leave palette unchanged"
    );
}

// ---------------------------------------------------------------------------
// 3. Font Switching & Fallback Safety Stress Tests
// ---------------------------------------------------------------------------

#[test]
fn test_font_switching_stress_in_headless_egui_context() {
    let ctx = egui::Context::default();

    let font_choices = [
        "default",
        "segoe_ui",
        "arial",
        "tahoma",
        "nonexistent_font_123",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/nonexistent.ttf",
        "",
        "   ",
        "🎨 Emojis",
        "Tiếng Việt Có Dấu",
        "default",
    ];

    // Stress test rapid font rebuilding across 100 iterations
    for i in 0..100 {
        let choice = font_choices[i % font_choices.len()];
        let mut fonts = egui::FontDefinitions::default();

        // Check fallback logic identical to theme.rs build_fonts
        let mut selected_font_loaded = false;
        let choice_norm = choice.trim().to_lowercase();
        match choice_norm.as_str() {
            "segoe_ui" | "segoe" => {
                if let Ok(data) = fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), egui::FontData::from_owned(data));
                    selected_font_loaded = true;
                }
            }
            "arial" => {
                if let Ok(data) = fs::read("C:\\Windows\\Fonts\\arial.ttf") {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), egui::FontData::from_owned(data));
                    selected_font_loaded = true;
                }
            }
            "tahoma" => {
                if let Ok(data) = fs::read("C:\\Windows\\Fonts\\tahoma.ttf") {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), egui::FontData::from_owned(data));
                    selected_font_loaded = true;
                }
            }
            "default" | "" => {}
            custom_path => {
                if let Ok(data) = fs::read(custom_path) {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), egui::FontData::from_owned(data));
                    selected_font_loaded = true;
                }
            }
        }

        // Bundled Be Vietnam Pro fallback representation
        fonts.font_data.insert(
            "bundled_sans".to_owned(),
            egui::FontData::from_static(include_bytes!(
                "../src/assets/fonts/BeVietnamPro-Regular.ttf"
            )),
        );

        let prop = fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default();
        if selected_font_loaded {
            prop.insert(0, "selected_font".to_owned());
            prop.push("bundled_sans".to_owned());
        } else {
            prop.insert(0, "bundled_sans".to_owned());
        }

        let mono = fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default();
        if selected_font_loaded {
            mono.insert(0, "selected_font".to_owned());
            mono.push("bundled_sans".to_owned());
        } else {
            mono.insert(0, "bundled_sans".to_owned());
        }

        // Apply to egui context
        ctx.set_fonts(fonts);

        // Exercise the layout engine with Vietnamese diacritics and symbols
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Cộng hòa Xã hội Chủ nghĩa Việt Nam - Độc lập Tự do Hạnh phúc");
                ui.label("Ký tự đặc biệt: ể, ễ, ặ, ồ, ố, ộ, ừ, ứ, ự");
                ui.label("Emoji test: 🎉🔥🚀");
            });
        });
    }
}

// ---------------------------------------------------------------------------
// 4. AppSettings Persistence Round-Trip with Extreme & Boundary Values
// ---------------------------------------------------------------------------

#[test]
fn test_app_settings_boundary_zoom_scales() {
    let fixture = TempStressFile::new("zoom_boundary.json");

    let boundary_scales = [0.80, 0.90, 1.00, 1.10, 1.25, 1.40, 0.0, -1.0, 10.0, 0.0001];

    for &scale in &boundary_scales {
        let mut settings = AppSettings::default();
        settings.font_scale = scale;

        let json = serde_json::to_string_pretty(&settings).expect("Serialize AppSettings");
        fs::write(&fixture.path, &json).expect("Write AppSettings");

        let read_back = fs::read_to_string(&fixture.path).expect("Read AppSettings");
        let loaded: AppSettings =
            serde_json::from_str(&read_back).expect("Deserialize AppSettings");

        assert_eq!(
            loaded.font_scale, scale,
            "Font scale {scale} must survive roundtrip exactly"
        );
    }
}

#[test]
fn test_app_settings_adversarial_font_family_names() {
    let fixture = TempStressFile::new("font_family_adversarial.json");

    let repeat_str = "A".repeat(10_000);
    let adversarial_names = [
        "",
        "   ",
        repeat_str.as_str(),
        "../../../../Windows/System32/cmd.exe",
        "C:\\Fonts\\My Special Font.ttf",
        "/etc/passwd",
        "Phông chữ tiếng Việt với dấu: à, á, ả, ã, ạ, ê, ế, ể, ễ, ệ, ô, ố, ổ, ỗ, ộ, ư, ứ, ử, ữ, ự",
        "Emoji Font: 🚀🎉🔥❤️💯",
        "CJK: 微软雅黑 / 宋体 / メイリオ",
        "Special chars: !@#$%^&*()_+-=[]{}|;':\",./<>?",
        "\"quotes\" and \\slashes\\",
    ];

    for name in adversarial_names {
        let mut settings = AppSettings::default();
        settings.font_family = name.to_string();

        let json = serde_json::to_string_pretty(&settings).expect("Serialize AppSettings");
        fs::write(&fixture.path, &json).expect("Write AppSettings");

        let read_back = fs::read_to_string(&fixture.path).expect("Read AppSettings");
        let loaded: AppSettings =
            serde_json::from_str(&read_back).expect("Deserialize AppSettings");

        assert_eq!(
            loaded.font_family, name,
            "Adversarial font family '{name}' must survive serialization"
        );
    }
}

#[test]
fn test_app_settings_adversarial_custom_colors_roundtrip() {
    let fixture = TempStressFile::new("colors_adversarial.json");

    let adversarial_color_sets = [
        // All None
        (None, None, None, None),
        // All Extreme Hex
        (
            Some("#000000".to_string()),
            Some("#FFFFFF".to_string()),
            Some("#00FF00".to_string()),
            Some("#0000FF".to_string()),
        ),
        // 8-character hex with alpha
        (
            Some("#0068FF80".to_string()),
            Some("#059669FF".to_string()),
            Some("#FFFFFF00".to_string()),
            Some("#00000000".to_string()),
        ),
        // Lowercase and unhashed
        (
            Some("0068ff".to_string()),
            Some("7c3aed".to_string()),
            Some("e11d48".to_string()),
            Some("475569".to_string()),
        ),
        // Malformed inputs stored in settings (must not crash serializer/deserializer)
        (
            Some("#123".to_string()),
            Some("not-a-color".to_string()),
            Some("".to_string()),
            Some("#FFFFFFFFF".to_string()),
        ),
    ];

    for (p, a, bo, bi) in adversarial_color_sets {
        let mut settings = AppSettings::default();
        settings.custom_primary_color = p.clone();
        settings.custom_accent_color = a.clone();
        settings.custom_bubble_out_bg = bo.clone();
        settings.custom_bubble_in_bg = bi.clone();

        let json = serde_json::to_string_pretty(&settings).expect("Serialize AppSettings");
        fs::write(&fixture.path, &json).expect("Write AppSettings");

        let read_back = fs::read_to_string(&fixture.path).expect("Read AppSettings");
        let loaded: AppSettings =
            serde_json::from_str(&read_back).expect("Deserialize AppSettings");

        assert_eq!(loaded.custom_primary_color, p);
        assert_eq!(loaded.custom_accent_color, a);
        assert_eq!(loaded.custom_bubble_out_bg, bo);
        assert_eq!(loaded.custom_bubble_in_bg, bi);
    }
}

#[test]
fn test_app_settings_corrupted_json_graceful_fallback() {
    let corrupted_payloads = [
        "",                                       // Empty file
        "{ invalid json",                         // Truncated syntax
        "[]",                                     // Array instead of object
        "\"string value\"",                       // Primitive string
        "12345",                                  // Primitive number
        "null",                                   // Null
        "{\"font_scale\": \"not_a_number\"}",     // Type mismatch on numeric field
        "{\"custom_primary_color\": 12345}",      // Type mismatch on string field
        "{\"pinned_threads\": \"not_an_array\"}", // Type mismatch on vec field
    ];

    for payload in corrupted_payloads {
        // AppSettings::load uses:
        // serde_json::from_str::<Self>(&s).ok().unwrap_or_default()
        let recovered = serde_json::from_str::<AppSettings>(payload).unwrap_or_default();
        // Regardless of corruption syntax, recovery must guarantee valid theme and font defaults
        assert_eq!(
            recovered.font_family, "default",
            "Payload {:?} failed to recover default font_family",
            payload
        );
        assert_eq!(
            recovered.font_scale, 1.0,
            "Payload {:?} failed to recover default font_scale",
            payload
        );
        assert_eq!(
            recovered.custom_primary_color, None,
            "Payload {:?} failed to recover None primary color",
            payload
        );
        assert_eq!(
            recovered.custom_accent_color, None,
            "Payload {:?} failed to recover None accent color",
            payload
        );
        assert_eq!(
            recovered.custom_bubble_out_bg, None,
            "Payload {:?} failed to recover None bubble out",
            payload
        );
        assert_eq!(
            recovered.custom_bubble_in_bg, None,
            "Payload {:?} failed to recover None bubble in",
            payload
        );
    }
}

#[test]
fn test_app_settings_forward_compatibility_extra_fields() {
    let forward_json = r##"{
        "font_family": "segoe_ui",
        "font_scale": 1.10,
        "custom_primary_color": "#0068FF",
        "future_unrecognized_field_v2": true,
        "another_future_field": { "nested": [1, 2, 3] }
    }"##;

    let parsed: AppSettings =
        serde_json::from_str(forward_json).expect("Must ignore unknown fields");
    assert_eq!(parsed.font_family, "segoe_ui");
    assert_eq!(parsed.font_scale, 1.10);
    assert_eq!(parsed.custom_primary_color, Some("#0068FF".to_string()));
}
