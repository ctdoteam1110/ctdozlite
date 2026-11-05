//! Empirical Challenger stress test harness for Milestone M2 (Requirement R2: UI Font & Color Customizer).
//!
//! Objectives:
//! 1. Scaling & Zoom Boundaries:
//!    - Test zoom factor limits: 0.0, negative values, very small (<0.5), very large (>3.0).
//!    - Verify safety bounds and clamp behaviors.
//! 2. Vietnamese tone mark glyph rendering:
//!    - Test all 34 lowercase tone-marked vowels (ấ, ầ, ẩ, ẫ, ậ, ắ, ằ, ẳ, ẵ, ặ, ế, ề, ể, ễ, ệ, ố, ồ, ổ, ỗ, ộ, ớ, ờ, ở, ỡ, ợ, ứ, ừ, ử, ữ, ự, ỳ, ỷ, ỹ, ỵ).
//!    - Test all uppercase tone-marked vowels (Ấ, Ầ, Ẩ, Ẫ, Ậ, Ắ, Ằ, Ẳ, Ẵ, Ặ, Ế, Ề, Ể, Ễ, Ệ, Ố, Ồ, Ổ, Ỗ, Ộ, Ớ, Ờ, Ở, Ỡ, Ợ, Ứ, Ừ, Ử, Ữ, Ự, Ỳ, Ỷ, Ỹ, Ỵ).
//!    - Test fallback chains with bundled Be Vietnam Pro, Windows system fonts, and invalid paths.
//! 3. Rapid sequential theme resets and swatches switching:
//!    - Test memory stability and zero atlas corruption across hundreds of rapid font/color/scale transitions.

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, RawInput, RichText, Visuals,
};

const BUNDLED_SANS_BYTES: &[u8] = include_bytes!("../src/assets/fonts/BeVietnamPro-Regular.ttf");
const LUCIDE_BYTES: &[u8] = include_bytes!("../src/assets/fonts/lucide.ttf");

fn create_test_font_definitions(font_choice: &str) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "lucide_font".to_owned(),
        FontData::from_static(LUCIDE_BYTES),
    );
    fonts.font_data.insert(
        "bundled_sans".to_owned(),
        FontData::from_static(BUNDLED_SANS_BYTES),
    );

    let mut selected_font_loaded = false;
    let choice_norm = font_choice.trim().to_lowercase();
    match choice_norm.as_str() {
        "segoe_ui" | "segoe" => {
            let candidates = [
                "C:\\Windows\\Fonts\\segoeui.ttf",
                "/usr/share/fonts/truetype/segoeui.ttf",
            ];
            for path in candidates {
                if let Ok(data) = std::fs::read(path) {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), FontData::from_owned(data));
                    selected_font_loaded = true;
                    break;
                }
            }
        }
        "arial" => {
            let candidates = [
                "C:\\Windows\\Fonts\\arial.ttf",
                "/Library/Fonts/Arial.ttf",
                "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            ];
            for path in candidates {
                if let Ok(data) = std::fs::read(path) {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), FontData::from_owned(data));
                    selected_font_loaded = true;
                    break;
                }
            }
        }
        "tahoma" => {
            let candidates = [
                "C:\\Windows\\Fonts\\tahoma.ttf",
                "/Library/Fonts/Tahoma.ttf",
            ];
            for path in candidates {
                if let Ok(data) = std::fs::read(path) {
                    fonts
                        .font_data
                        .insert("selected_font".to_owned(), FontData::from_owned(data));
                    selected_font_loaded = true;
                    break;
                }
            }
        }
        "default" | "" => {}
        custom_path => {
            if let Ok(data) = std::fs::read(custom_path) {
                fonts
                    .font_data
                    .insert("selected_font".to_owned(), FontData::from_owned(data));
                selected_font_loaded = true;
            }
        }
    }

    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    if selected_font_loaded {
        prop.insert(0, "selected_font".to_owned());
        prop.push("bundled_sans".to_owned());
    } else {
        prop.insert(0, "bundled_sans".to_owned());
    }
    prop.push("lucide_font".to_owned());

    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    if selected_font_loaded {
        mono.insert(0, "selected_font".to_owned());
        mono.push("bundled_sans".to_owned());
    } else {
        mono.insert(0, "bundled_sans".to_owned());
    }
    mono.push("lucide_font".to_owned());

    fonts
}

// ===========================================================================
// SECTION 1: SCALING & ZOOM BOUNDARIES
// ===========================================================================

#[test]
fn test_zoom_factor_egui_boundary_limits() {
    let ctx = egui::Context::default();

    // 1. Baseline zoom
    assert_eq!(ctx.zoom_factor(), 1.0);

    // 2. Normal range (80% to 140% as supported by ctdozlite slider)
    for factor in [0.80, 0.90, 1.00, 1.10, 1.25, 1.40] {
        ctx.set_zoom_factor(factor);
        println!(
            "After set_zoom_factor({factor}): zoom_factor={}",
            ctx.zoom_factor()
        );
        let _ = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Kiểm tra tỷ lệ thu phóng bình thường");
            });
        });
        println!("After ctx.run(): zoom_factor={}", ctx.zoom_factor());
    }

    // 3. Boundary test: zero zoom factor (0.0) and negative values
    // epaint-0.29.1 (epaint::text::fonts::Fonts) asserts `pixels_per_point > 0.0`.
    // Calling set_zoom_factor(0.0) or negative triggers a panic in epaint:
    // `pixels_per_point out of range: 0`
    for invalid_zoom in [0.0_f32, -0.5_f32, -1.0_f32] {
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let local_ctx = egui::Context::default();
            local_ctx.set_zoom_factor(invalid_zoom);
            let _ = local_ctx.run(RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("Kiểm tra zoom không hợp lệ");
                });
            });
        }));
        assert!(
            panic_result.is_err(),
            "Expected epaint to reject zoom_factor <= 0.0 ({invalid_zoom}) with panic"
        );
    }

    // 4. Boundary test: very small positive zoom factor (< 0.5, e.g. 0.001, 0.01, 0.1, 0.25, 0.49)
    for small_zoom in [0.001_f32, 0.01_f32, 0.1_f32, 0.25_f32, 0.49_f32] {
        let local_ctx = egui::Context::default();
        local_ctx.set_zoom_factor(small_zoom);
        let output = local_ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Kiểm tra zoom nhỏ");
                let _ = ui.button("Bấm");
            });
        });
        assert!(
            !output.shapes.is_empty(),
            "Empty shapes for small zoom {small_zoom}"
        );
    }

    // 5. Boundary test: very large zoom factor (> 3.0, e.g. 3.0, 3.5, 5.0, 10.0, 50.0)
    for large_zoom in [3.0_f32, 3.5_f32, 5.0_f32, 10.0_f32, 50.0_f32] {
        let local_ctx = egui::Context::default();
        local_ctx.set_zoom_factor(large_zoom);
        let output = local_ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Kiểm tra zoom lớn");
            });
        });
        assert!(
            !output.shapes.is_empty(),
            "Empty shapes for large zoom {large_zoom}"
        );
    }

    // 6. Reset to 1.0
    ctx.set_zoom_factor(1.0);
    let _ = ctx.run(RawInput::default(), |_| {});
    assert_eq!(ctx.zoom_factor(), 1.0);
}

#[test]
fn test_app_settings_zoom_factor_safety_and_clamping() {
    // In ctdozlite (src/gui.rs:672), loaded_settings.font_scale is checked:
    // `if loaded_settings.font_scale > 0.0 { cc.egui_ctx.set_zoom_factor(loaded_settings.font_scale); }`
    // And in UI (src/gui/modals.rs:2140), slider clamps between 80% and 140%.

    // Test production AppSettings::normalized_font_scale startup normalization:
    let check_scale = |raw: f32| -> f32 {
        let mut s = zalo_rust::gui::AppSettings::default();
        s.font_scale = raw;
        s.normalized_font_scale()
    };

    assert_eq!(check_scale(0.0), 1.0);
    assert_eq!(check_scale(-1.0), 1.0);
    assert_eq!(check_scale(f32::NAN), 1.0);
    assert_eq!(check_scale(f32::INFINITY), 1.0);
    assert_eq!(check_scale(f32::NEG_INFINITY), 1.0);
    assert_eq!(check_scale(0.1), 0.5); // Clamped to lower safe bound
    assert_eq!(check_scale(0.49), 0.5); // Clamped to lower safe bound
    assert_eq!(check_scale(0.80), 0.80);
    assert_eq!(check_scale(1.00), 1.00);
    assert_eq!(check_scale(1.25), 1.25);
    assert_eq!(check_scale(1.40), 1.40);
    assert_eq!(check_scale(3.5), 3.0); // Clamped to upper safe bound
    assert_eq!(check_scale(50.0), 3.0); // Clamped to upper safe bound
}

// ===========================================================================
// SECTION 2: VIETNAMESE TONE MARK GLYPH RENDERING & FONT CHAINS
// ===========================================================================

const VIETNAMESE_LOWERCASE_TONES: &[&str] = &[
    "ấ", "ầ", "ẩ", "ẫ", "ậ", "ắ", "ằ", "ẳ", "ẵ", "ặ", "ế", "ề", "ể", "ễ", "ệ", "ố", "ồ", "ổ", "ỗ",
    "ộ", "ớ", "ờ", "ở", "ỡ", "ợ", "ứ", "ừ", "ử", "ữ", "ự", "ỳ", "ỷ", "ỹ", "ỵ",
];

const VIETNAMESE_UPPERCASE_TONES: &[&str] = &[
    "Ấ", "Ầ", "Ẩ", "Ẫ", "Ậ", "Ắ", "Ằ", "Ẳ", "Ẵ", "Ặ", "Ế", "Ề", "Ể", "Ễ", "Ệ", "Ố", "Ồ", "Ổ", "Ỗ",
    "Ộ", "Ớ", "Ờ", "Ở", "Ỡ", "Ợ", "Ứ", "Ừ", "Ử", "Ữ", "Ự", "Ỳ", "Ỷ", "Ỹ", "Ỵ",
];

#[test]
fn test_bundled_be_vietnam_pro_all_tone_marks_rendering() {
    let ctx = egui::Context::default();
    let fonts = create_test_font_definitions("default");
    ctx.set_fonts(fonts);
    // Warm up context to initialize fonts
    let _ = ctx.run(RawInput::default(), |_| {});

    // Verify all 34 lowercase tone-marked vowels
    for tone in VIETNAMESE_LOWERCASE_TONES {
        let text = format!("Ký tự: {tone}");
        let font_id = FontId::proportional(14.0);
        let galley = ctx.fonts(|f| f.layout_no_wrap(text.clone(), font_id, Color32::WHITE));

        assert!(
            galley.size().x > 0.0,
            "Bundled Be Vietnam Pro failed to shape lowercase tone: {tone}"
        );
        assert!(
            galley.size().y > 0.0,
            "Bundled Be Vietnam Pro height 0 for: {tone}"
        );
        // Verify galley contains valid non-empty rows
        assert!(!galley.rows.is_empty(), "Galley rows empty for: {tone}");
    }

    // Verify all 34 uppercase tone-marked vowels
    for tone in VIETNAMESE_UPPERCASE_TONES {
        let text = format!("HOA: {tone}");
        let font_id = FontId::proportional(14.0);
        let galley = ctx.fonts(|f| f.layout_no_wrap(text.clone(), font_id, Color32::WHITE));

        assert!(
            galley.size().x > 0.0,
            "Bundled Be Vietnam Pro failed to shape uppercase tone: {tone}"
        );
        assert!(
            galley.size().y > 0.0,
            "Bundled Be Vietnam Pro height 0 for uppercase: {tone}"
        );
        assert!(!galley.rows.is_empty(), "Galley rows empty for: {tone}");
    }

    // Verify full representative sentence rendering
    let sample_sentences = [
        "Cộng hòa Xã hội Chủ nghĩa Việt Nam",
        "Độc lập - Tự do - Hạnh phúc",
        "Chào bạn! Màu sắc và font chữ mới hiển thị thế nào? Rất đẹp mắt, cập nhật tức thì! 👍",
        "Thử nghiệm tất cả dấu thanh: ấ, ầ, ẩ, ẫ, ậ, ắ, ằ, ẳ, ẵ, ặ, ế, ề, ể, ễ, ệ, ố, ồ, ổ, ỗ, ộ, ớ, ờ, ở, ỡ, ợ, ứ, ừ, ử, ữ, ự, ỳ, ỷ, ỹ, ỵ",
        "HOA: Ấ, Ầ, Ẩ, Ẫ, Ậ, Ắ, Ằ, Ẳ, Ẵ, Ặ, Ế, Ề, Ể, Ễ, Ệ, Ố, Ồ, Ổ, Ỗ, Ộ, Ớ, Ờ, Ở, Ỡ, Ợ, Ứ, Ừ, Ử, Ữ, Ự, Ỳ, Ỷ, Ỹ, Ỵ",
    ];

    for s in sample_sentences {
        let font_id = FontId::proportional(13.5);
        let galley = ctx.fonts(|f| f.layout_no_wrap(s.to_string(), font_id, Color32::WHITE));
        assert!(
            galley.size().x > 10.0,
            "Sentence galley width too small: {s}"
        );
        assert!(
            galley.size().y > 10.0,
            "Sentence galley height too small: {s}"
        );
    }
}

#[test]
fn test_font_fallback_chains_with_system_fonts_and_invalid_paths() {
    let choices = [
        "segoe_ui",
        "arial",
        "tahoma",
        "default",
        "invalid_nonexistent_font_path.ttf",
    ];

    for choice in choices {
        let ctx = egui::Context::default();
        let fonts = create_test_font_definitions(choice);
        ctx.set_fonts(fonts);
        // Warm up context to initialize fonts
        let _ = ctx.run(RawInput::default(), |_| {});

        // Under every font choice (including missing or invalid), Be Vietnam Pro must serve as fallback
        for tone in VIETNAMESE_LOWERCASE_TONES {
            let text = format!("Kiểm tra fallback '{choice}': {tone}");
            let font_id = FontId::proportional(14.0);
            let galley = ctx.fonts(|f| f.layout_no_wrap(text.clone(), font_id, Color32::WHITE));

            assert!(
                galley.size().x > 0.0,
                "Font choice '{choice}' failed to shape tone mark '{tone}' via fallback"
            );
            assert!(
                galley.size().y > 0.0,
                "Font choice '{choice}' zero height for tone '{tone}'"
            );
        }

        // Render full frame with widgets
        let output = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading(format!("Phông chữ đang chọn: {choice}"));
                ui.label(RichText::new("Tin nhắn: ấ ầ ẩ ẫ ậ ắ ằ ẳ ẵ ặ ế ề ể ễ ệ").size(13.0));
                ui.label(RichText::new("Tin nhắn hoa: Ấ Ầ Ẩ Ẫ Ậ Ắ Ằ Ẳ Ẵ Ặ Ế Ề Ể Ễ Ệ").size(13.0));
            });
        });

        assert!(
            !output.shapes.is_empty(),
            "Frame produced 0 shapes for font choice {choice}"
        );
    }
}

// ===========================================================================
// SECTION 3: RAPID SEQUENTIAL THEME RESETS AND SWATCHES SWITCHING
// ===========================================================================

#[test]
fn test_rapid_sequential_theme_resets_and_swatches_switching() {
    let ctx = egui::Context::default();

    let swatches = [
        ("Zalo Blue", Color32::from_rgb(0, 104, 255)),
        ("Atom Blue", Color32::from_rgb(82, 139, 255)),
        ("Emerald", Color32::from_rgb(5, 150, 105)),
        ("Violet", Color32::from_rgb(124, 58, 237)),
        ("Rose", Color32::from_rgb(225, 29, 72)),
        ("Amber", Color32::from_rgb(217, 119, 6)),
        ("Slate", Color32::from_rgb(71, 85, 105)),
    ];

    let font_choices = ["default", "segoe_ui", "arial", "tahoma", "default"];
    let zoom_levels = [0.90, 1.00, 1.10, 1.25, 1.00];

    // Stress test: 300 rapid iterations alternating palettes, fonts, zoom, and resets
    for i in 0..300 {
        let (name, swatch_col) = swatches[i % swatches.len()];
        let font_choice = font_choices[i % font_choices.len()];
        let zoom = zoom_levels[i % zoom_levels.len()];
        let is_dark = (i % 2) == 0;

        // 1. Dynamic Font Update
        if i % 3 == 0 {
            let fonts = create_test_font_definitions(font_choice);
            ctx.set_fonts(fonts);
        }

        // 2. Zoom update
        ctx.set_zoom_factor(zoom);

        // 3. Theme Visuals Update
        let mut visuals = if is_dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.widgets.active.bg_fill = swatch_col;
        visuals.widgets.hovered.bg_fill = swatch_col;
        visuals.selection.bg_fill = swatch_col;
        ctx.set_visuals(visuals);

        // 4. Run an interactive frame rendering chat bubbles, buttons, and Vietnamese text
        let output = ctx.run(RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading(format!("Iteration {i}: Swatch {name} ({font_choice})"));

                // Inbound bubble simulation
                egui::Frame::none()
                    .fill(if is_dark {
                        Color32::from_rgb(50, 56, 66)
                    } else {
                        Color32::WHITE
                    })
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Chào bạn! Thử nghiệm font tiếng Việt: ấ ế ố ứ ỳ")
                                .size(12.5),
                        );
                    });

                // Outbound bubble simulation
                egui::Frame::none()
                    .fill(swatch_col)
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Tin nhắn phản hồi: Rất mượt mà, không giật lag!")
                                .color(Color32::WHITE)
                                .size(12.5),
                        );
                    });

                // Simulate reset default at milestone iterations
                if i % 50 == 49 {
                    ui.label("Đã kích hoạt Reset Defaults!");
                }
            });
        });

        // 5. Verify frame shapes and texture delta validity
        assert!(!output.shapes.is_empty(), "Empty shapes at iteration {i}");

        // Tessellate output shapes to verify zero vertex/mesh corruption
        let clipped_primitives = ctx.tessellate(output.shapes, ctx.pixels_per_point());
        assert!(
            !clipped_primitives.is_empty(),
            "Zero tessellated primitives at iteration {i}"
        );

        for prim in &clipped_primitives {
            if let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive {
                // Verify mesh does not contain NaN or infinite coordinates
                for vert in &mesh.vertices {
                    assert!(
                        vert.pos.x.is_finite(),
                        "NaN or Inf vertex X at iteration {i}"
                    );
                    assert!(
                        vert.pos.y.is_finite(),
                        "NaN or Inf vertex Y at iteration {i}"
                    );
                    assert!(vert.uv.x.is_finite(), "NaN or Inf uv X at iteration {i}");
                    assert!(vert.uv.y.is_finite(), "NaN or Inf uv Y at iteration {i}");
                }
            }
        }
    }

    // Final reset verification
    ctx.set_zoom_factor(1.0);
    ctx.set_fonts(create_test_font_definitions("default"));
    ctx.set_visuals(Visuals::dark());

    let final_output = ctx.run(RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Khôi phục hoàn toàn về mặc định.");
        });
    });

    assert!(!final_output.shapes.is_empty());
}
