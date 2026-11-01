//! Explicit headless proof of actual egui inline rendering, not a GUI screenshot.
use super::*;
use egui::epaint::{ImageData, Primitive};

struct CpuTexture {
    size: [usize; 2],
    pixels: Vec<Color32>,
    color: bool,
}

fn apply_delta(textures: &mut HashMap<egui::TextureId, CpuTexture>, delta: &egui::TexturesDelta) {
    for (id, update) in &delta.set {
        let pixels: Vec<Color32> = match &update.image {
            ImageData::Color(image) => image.pixels.clone(),
            ImageData::Font(image) => image.srgba_pixels(None).collect(),
        };
        let size = update.image.size();
        if let Some([x, y]) = update.pos {
            let target = textures
                .get_mut(id)
                .expect("Partial update requires existing texture");
            assert!(x + size[0] <= target.size[0] && y + size[1] <= target.size[1]);
            for row in 0..size[1] {
                let start = (y + row) * target.size[0] + x;
                target.pixels[start..start + size[0]]
                    .copy_from_slice(&pixels[row * size[0]..(row + 1) * size[0]]);
            }
        } else {
            textures.insert(
                *id,
                CpuTexture {
                    size,
                    pixels,
                    color: matches!(update.image, ImageData::Color(_)),
                },
            );
        }
    }
}

fn cross(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn rasterize(
    primitives: &[egui::ClippedPrimitive],
    textures: &HashMap<egui::TextureId, CpuTexture>,
    size: [usize; 2],
    background: Color32,
) -> (Vec<u8>, usize, usize) {
    // Color32 and atlas pixels are premultiplied sRGBA. Approximate the GPU
    // with nearest sampling and premultiplied source-over in sRGB space.
    let mut canvas = vec![background.to_array(); size[0] * size[1]];
    let (mut color_meshes, mut triangles) = (0, 0);
    for primitive in primitives {
        let Primitive::Mesh(mesh) = &primitive.primitive else {
            panic!("Unexpected callback in inline emoji visual proof");
        };
        let texture = textures
            .get(&mesh.texture_id)
            .expect("Missing primitive texture");
        color_meshes += usize::from(texture.color);
        for indices in mesh.indices.chunks_exact(3) {
            let v = [
                mesh.vertices[indices[0] as usize],
                mesh.vertices[indices[1] as usize],
                mesh.vertices[indices[2] as usize],
            ];
            let area = cross(v[0].pos, v[1].pos, v[2].pos);
            if area.abs() < 0.00001 {
                continue;
            }
            triangles += 1;
            let min_x = v
                .iter()
                .map(|v| v.pos.x)
                .fold(f32::INFINITY, f32::min)
                .max(primitive.clip_rect.min.x)
                .floor()
                .max(0.0) as usize;
            let max_x = v
                .iter()
                .map(|v| v.pos.x)
                .fold(f32::NEG_INFINITY, f32::max)
                .min(primitive.clip_rect.max.x)
                .ceil()
                .clamp(0.0, size[0] as f32) as usize;
            let min_y = v
                .iter()
                .map(|v| v.pos.y)
                .fold(f32::INFINITY, f32::min)
                .max(primitive.clip_rect.min.y)
                .floor()
                .max(0.0) as usize;
            let max_y = v
                .iter()
                .map(|v| v.pos.y)
                .fold(f32::NEG_INFINITY, f32::max)
                .min(primitive.clip_rect.max.y)
                .ceil()
                .clamp(0.0, size[1] as f32) as usize;
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    if !primitive.clip_rect.contains(p) {
                        continue;
                    }
                    let weights = [
                        cross(v[1].pos, v[2].pos, p) / area,
                        cross(v[2].pos, v[0].pos, p) / area,
                        cross(v[0].pos, v[1].pos, p) / area,
                    ];
                    if weights.iter().any(|w| *w < 0.0) {
                        continue;
                    }
                    // Half-open edge coverage avoids drawing shared quad diagonals twice.
                    let mut owned = true;
                    for (edge, weight) in weights.iter().enumerate() {
                        if weight.abs() < 0.000001 {
                            let a = v[(edge + 1) % 3].pos;
                            let b = v[(edge + 2) % 3].pos;
                            let (a, b) = if area > 0.0 { (a, b) } else { (b, a) };
                            if !(b.y > a.y || (b.y == a.y && b.x < a.x)) {
                                owned = false;
                            }
                        }
                    }
                    if !owned {
                        continue;
                    }
                    let uv = v
                        .iter()
                        .zip(weights)
                        .fold(egui::Vec2::ZERO, |sum, (v, w)| sum + v.uv.to_vec2() * w);
                    let tx = (uv.x * texture.size[0] as f32)
                        .floor()
                        .clamp(0.0, (texture.size[0] - 1) as f32)
                        as usize;
                    let ty = (uv.y * texture.size[1] as f32)
                        .floor()
                        .clamp(0.0, (texture.size[1] - 1) as f32)
                        as usize;
                    let texel = texture.pixels[ty * texture.size[0] + tx].to_array();
                    let mut source = [0.0; 4];
                    for channel in 0..4 {
                        let tint: f32 = v
                            .iter()
                            .zip(weights)
                            .map(|(v, w)| v.color.to_array()[channel] as f32 / 255.0 * w)
                            .sum();
                        source[channel] = texel[channel] as f32 * tint;
                    }
                    let target = &mut canvas[y * size[0] + x];
                    for channel in 0..4 {
                        target[channel] = (source[channel]
                            + target[channel] as f32 * (1.0 - source[3] / 255.0))
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
    (
        canvas.into_iter().flatten().collect(),
        color_meshes,
        triangles,
    )
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "Requires native helper and explicit ZL_EMOJI_VISUAL_DIR output directory"]
fn inline_emoji_export_actual_bubble_visual() {
    let directory = std::path::PathBuf::from(std::env::var_os("ZL_EMOJI_VISUAL_DIR").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    for dark in [false, true] {
        let ctx = egui::Context::default();
        setup_custom_fonts(&ctx);
        let pal = if dark {
            ThemePalette::dark()
        } else {
            ThemePalette::light()
        };
        ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        let reactions = load_default_emoji_textures(&ctx);
        seed_worker_results(
            &ctx,
            ["😀", "👍🏽", "👩‍💻", "🇻🇳", "1️⃣"].into_iter().map(|cluster| {
                (
                    cluster,
                    render_native(cluster).expect("Existing helper must render fixture"),
                )
            }),
        );
        let text = "Xin chào :+1: 👍🏽 👩‍💻 🇻🇳 1️⃣ @An @An\nTin nhắn dài để kiểm tra xuống dòng theo cụm emoji, dấu tiếng Việt và khả năng chọn sao chép nguyên văn. 😀";
        let second = text.rfind('@').unwrap();
        let units = text[..second].encode_utf16().count();
        let mut textures = HashMap::new();
        let size = [780, 470];
        for frame in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(size[0] as f32, size[1] as f32),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.heading("Emoji và mention: actual message bubbles");
                        ui.label("Headless egui mesh / CPU approximation / synthetic content");
                        for outgoing in [false, true] {
                            ui.add_space(15.0);
                            let msg = ChatMessage {
                                id: format!("visual-{outgoing}"),
                                text: text.into(),
                                sender: "Thành viên".into(),
                                is_outgoing: outgoing,
                                reply_to_text: Some("Trích dẫn 👍🏽 👩‍💻 :+1:".into()),
                                reply_to_id: Some("fixture-original".into()),
                                wire_ref: crate::message_model::WireMessageRef::from_raw(
                                    &serde_json::json!({
                                        "mentions":[{"uid":"22","pos":units,"len":3}]
                                    }),
                                ),
                                ..Default::default()
                            };
                            super::super::messages::render_chat_bubble(
                                ui,
                                &msg,
                                false,
                                false,
                                false,
                                true,
                                "Fixture",
                                "fixture-account",
                                "fixture-thread",
                                &mut HashSet::new(),
                                None,
                                &pal,
                                &HashMap::new(),
                                &mut HashSet::new(),
                                &reactions,
                                0,
                            );
                        }
                    });
                },
            );
            apply_delta(&mut textures, &output.textures_delta);
            if frame == 2 {
                let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
                let (rgba, color_meshes, _) =
                    rasterize(&primitives, &textures, size, Color32::TRANSPARENT);
                assert!(
                    color_meshes >= 8,
                    "Actual bubbles must paint multiple native color images"
                );
                let path = directory.join(if dark {
                    "inline-dark.png"
                } else {
                    "inline-light.png"
                });
                image::RgbaImage::from_raw(size[0] as u32, size[1] as u32, rgba)
                    .unwrap()
                    .save(&path)
                    .unwrap();
                let reopened = image::open(&path).unwrap();
                assert_eq!((reopened.width(), reopened.height()), (780, 470));
                println!(
                    "INLINE VISUAL PASS: {} color_meshes={color_meshes}",
                    path.display()
                );
            }
        }
    }
}
