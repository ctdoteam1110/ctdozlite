//! Grapheme-safe inline emoji using existing reaction textures and asynchronous native previews.
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use unicode_segmentation::UnicodeSegmentation;

const CACHE_LIMIT: usize = 256;
const PACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/emoji-sprites.bin"));

#[derive(Deserialize)]
struct Sprite {
    offset: usize,
    length: usize,
    width: usize,
    height: usize,
    sha256: String,
}

const QUEUE_LIMIT: usize = 32;
// An empty square-advance glyph provides atomic image cells and legal wrap boundaries.
const SLOT: &str = "\u{4e00}";

#[derive(Deserialize)]
struct Index {
    sprites: Vec<Sprite>,
    aliases: HashMap<String, u16>,
    unicode: HashMap<String, u16>,
    emoji_scalar_ranges: Vec<[u32; 2]>,
}
#[derive(Default)]
struct TrieNode {
    children: HashMap<u8, usize>,
    sprite: Option<u16>,
}
struct Catalog {
    index: Index,
    trie: Vec<TrieNode>,
    canonical: HashMap<u16, String>,
}
fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let index: Index = serde_json::from_str(include_str!("../assets/emojis/zalo/index.json"))
            .expect("Bundled emoji index must be valid");
        let mut trie = vec![TrieNode::default()];
        for (key, &sprite) in &index.aliases {
            let mut node = 0;
            for byte in key.bytes().map(|b| b.to_ascii_lowercase()) {
                node = if let Some(&next) = trie[node].children.get(&byte) {
                    next
                } else {
                    let next = trie.len();
                    trie.push(TrieNode::default());
                    trie[node].children.insert(byte, next);
                    next
                };
            }
            trie[node].sprite = Some(sprite);
        }
        let mut canonical = HashMap::new();
        let mut unicode: Vec<_> = index.unicode.iter().collect();
        unicode.sort_by_key(|(text, _)| (text.len(), *text));
        for (text, &sprite) in unicode {
            canonical.entry(sprite).or_insert_with(|| text.clone());
        }
        Catalog {
            index,
            trie,
            canonical,
        }
    })
}
fn original_pack() -> &'static [u8] {
    PACK
}
fn sprite_image(sprite: u16) -> Option<ImageKey> {
    if !original_pack().is_empty() && catalog().index.sprites.get(sprite as usize).is_some() {
        Some(ImageKey::Original(sprite))
    } else {
        catalog()
            .canonical
            .get(&sprite)
            .map(|text| ImageKey::Native(text.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ImageKey {
    Original(u16),
    Native(String),
}
#[derive(Clone, Debug)]
struct Part<'a> {
    raw: &'a str,
    image: Option<ImageKey>,
}

// Match longest ASCII aliases first, then apply the original two-sided boundary
// policy to all candidates, including adjacent candidates later rejected.
fn aliases(text: &str) -> Vec<(usize, usize, u16)> {
    let trie = &catalog().trie;
    let bytes = text.as_bytes();
    let normalized: Vec<_> = text
        .char_indices()
        .map(|(offset, chr)| {
            let mut lower = chr.to_lowercase();
            let folded = lower
                .next()
                .filter(|c| c.is_ascii() && lower.next().is_none())
                .map(|c| c as u8);
            (offset, folded)
        })
        .collect();
    let mut candidates = Vec::new();
    let mut start = 0;
    while start < normalized.len() {
        let mut node = 0;
        let mut cursor = start;
        let mut found = None;
        while cursor < normalized.len() {
            let Some(byte) = normalized[cursor].1 else {
                break;
            };
            let Some(&next) = trie[node].children.get(&byte) else {
                break;
            };
            node = next;
            cursor += 1;
            if let Some(sprite) = trie[node].sprite {
                let end = normalized.get(cursor).map_or(text.len(), |v| v.0);
                found = Some((normalized[start].0, end, sprite, cursor));
            }
        }
        if let Some((a, b, sprite, next)) = found {
            start = next;
            candidates.push((a, b, sprite));
        } else {
            start += 1;
        }
    }
    let starts: HashSet<_> = candidates.iter().map(|v| v.0).collect();
    let ends: HashSet<_> = candidates.iter().map(|v| v.1).collect();
    candidates
        .into_iter()
        .filter(|&(a, b, _)| {
            (a == 0 || matches!(bytes[a - 1], b' ' | b'\n') || ends.contains(&a))
                && (b == bytes.len() || matches!(bytes[b], b' ' | b'\n') || starts.contains(&b))
        })
        .collect()
}

fn unicode_image(cluster: &str) -> Option<ImageKey> {
    if cluster.contains('\u{fe0e}') {
        return None;
    } // Explicit text presentation.
    if let Some(&sprite) = catalog().index.unicode.get(cluster) {
        return if original_pack().is_empty() {
            Some(ImageKey::Native(cluster.to_owned()))
        } else {
            sprite_image(sprite)
        };
    }
    // Normalize only a terminal presentation selector, never skin modifiers/ZWJ.
    if let Some(base) = cluster.strip_suffix('\u{fe0f}') {
        if catalog().index.unicode.contains_key(base) {
            return Some(ImageKey::Native(cluster.to_owned()));
        }
    }
    let is_emoji = cluster.chars().any(|c| {
        let code = c as u32;
        code > 0x7f
            && catalog()
                .index
                .emoji_scalar_ranges
                .iter()
                .any(|&[a, b]| a <= code && code <= b)
    }) || (cluster.contains('\u{20e3}')
        && cluster.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '#' | '*')));
    (is_emoji && cluster.len() <= 256).then(|| ImageKey::Native(cluster.to_owned()))
}

fn parts<'a>(text: &'a str) -> Vec<Part<'a>> {
    let mut parts = Vec::new();
    let mut cursor = 0;
    let append_text = |slice: &str, offset: usize, out: &mut Vec<Part<'a>>| {
        // Lifetime comes from the original text rather than this temporary slice.
        for (index, cluster) in slice.grapheme_indices(true) {
            let raw = &text[offset + index..offset + index + cluster.len()];
            out.push(Part {
                raw,
                image: unicode_image(raw),
            });
        }
    };
    for (start, end, sprite) in aliases(text) {
        append_text(&text[cursor..start], cursor, &mut parts);
        parts.push(Part {
            raw: &text[start..end],
            image: sprite_image(sprite),
        });
        cursor = end;
    }
    append_text(&text[cursor..], cursor, &mut parts);
    parts
}

#[derive(Clone)]
enum Cached {
    Ready(TextureHandle),
    Failed,
}
#[derive(Default)]
struct EmojiCache {
    entries: HashMap<ImageKey, Cached>,
    order: VecDeque<ImageKey>,
    pending: HashSet<ImageKey>,
    reactions: HashMap<String, TextureHandle>,
}
impl EmojiCache {
    fn get(&mut self, key: &ImageKey) -> Option<Cached> {
        let value = self.entries.get(key)?.clone();
        self.order.retain(|k| k != key);
        self.order.push_back(key.clone());
        Some(value)
    }
    fn insert(&mut self, key: ImageKey, value: Cached) {
        self.order.retain(|k| k != &key);
        self.order.push_back(key.clone());
        self.entries.insert(key, value);
        while self.order.len() > CACHE_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
    }
}
type SharedCache = Arc<Mutex<EmojiCache>>;
fn cache(ctx: &egui::Context) -> SharedCache {
    let id = Id::new("inline-native-emoji-cache-v1");
    ctx.data_mut(|data| {
        if let Some(cache) = data.get_temp::<SharedCache>(id) {
            return cache;
        }
        let cache = Arc::new(Mutex::new(EmojiCache::default()));
        data.insert_temp(id, cache.clone());
        cache
    })
}

/// Release cancelled jobs while retaining completed images and font-independent caches.
pub(super) fn reset_pending(ctx: &egui::Context) {
    if let Ok(mut cache) = cache(ctx).lock() {
        cache.pending.clear();
    }
}

/// Share the six already-loaded reaction textures without decoding again during layout.
pub(super) fn register_reactions(ctx: &egui::Context, reactions: &HashMap<String, TextureHandle>) {
    if let Ok(mut cache) = cache(ctx).lock() {
        cache.reactions = reactions.clone();
    }
}

/// Admit a terminal worker result and upload its bounded image on the UI thread.
pub(super) fn accept_result(
    ctx: &egui::Context,
    cluster: String,
    image: Option<egui::ColorImage>,
    sprite_id: Option<u16>,
) {
    let shared = cache(ctx);
    if let Ok(mut cache) = shared.lock() {
        let key = sprite_id
            .map(ImageKey::Original)
            .unwrap_or_else(|| ImageKey::Native(cluster));
        if !cache.pending.remove(&key) {
            return;
        }
        let value = match image.filter(|image| {
            image.size[0] == 64
                && matches!(image.size[1], 63 | 64)
                && image.pixels.len() == image.size[0] * image.size[1]
        }) {
            Some(image) => Cached::Ready(ctx.load_texture(
                "native-inline-emoji",
                image,
                TextureOptions::LINEAR,
            )),
            None => Cached::Failed,
        };
        cache.insert(key, value);
    };
}

fn reaction_key(cluster: &str) -> Option<&'static str> {
    match cluster {
        "👍" => Some("like"),
        "❤️" | "❤" => Some("heart"),
        "😆" => Some("smile"),
        "😮" => Some("surprised"),
        "😢" => Some("cry"),
        "😠" => Some("rage"),
        _ => None,
    }
}
fn image(ctx: &egui::Context, key: &ImageKey) -> Option<Cached> {
    let shared = cache(ctx);
    let mut cache = shared.lock().ok()?;
    if let ImageKey::Native(cluster) = key {
        if let Some(texture) = reaction_key(cluster).and_then(|key| cache.reactions.get(key)) {
            return Some(Cached::Ready(texture.clone()));
        }
    }
    if let Some(value) = cache.get(key) {
        return Some(value);
    }
    request_image(&mut cache, key, |command| {
        WORKER_TX.get().is_some_and(|tx| tx.send(command).is_ok())
    });
    None
}
fn request_image(cache: &mut EmojiCache, key: &ImageKey, send: impl FnOnce(WorkerCmd) -> bool) {
    let (cluster, sprite_id) = match key {
        ImageKey::Native(cluster) => (cluster.clone(), None),
        ImageKey::Original(sprite) => (String::new(), Some(*sprite)),
    };
    if !cache.pending.contains(key)
        && cache.pending.len() < QUEUE_LIMIT
        && send(WorkerCmd::RenderInlineEmoji { cluster, sprite_id })
    {
        cache.pending.insert(key.clone());
    }
}

/// Validate exact metadata bounds, dimensions and SHA-256 before decoding optional artwork.
fn decode_original(pack: &[u8], sprite: &Sprite) -> Option<egui::ColorImage> {
    let end = sprite.offset.checked_add(sprite.length)?;
    let bytes = pack.get(sprite.offset..end)?;
    if sprite.width != 64
        || !matches!(sprite.height, 63 | 64)
        || sprite.length > 128 * 1024
        || format!("{:x}", Sha256::digest(bytes)) != sprite.sha256
    {
        return None;
    }
    decode_bounded_image(bytes, sprite.height)
}

/// Decode one optional original sprite on a worker, never during UI layout.
pub(super) fn render_original(sprite_id: u16) -> Option<egui::ColorImage> {
    decode_original(PACK, catalog().index.sprites.get(sprite_id as usize)?)
}

/// Decode a bounded native helper PNG off the UI thread, rejecting dimensions before allocation.
pub(super) fn decode_native_image(bytes: &[u8]) -> Option<egui::ColorImage> {
    decode_bounded_image(bytes, 64)
}
fn decode_bounded_image(bytes: &[u8], height: usize) -> Option<egui::ColorImage> {
    if bytes.len() > 128 * 1024
        || bytes.len() < 24
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || &bytes[12..16] != b"IHDR"
        || u32::from_be_bytes(bytes[16..20].try_into().ok()?) != 64
        || u32::from_be_bytes(bytes[20..24].try_into().ok()?) != height as u32
    {
        return None;
    }
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [64, height],
        image.as_raw(),
    ))
}

/// Run the existing bounded platform helper; this function belongs on spawn_blocking only.
pub(super) fn render_native(cluster: &str) -> Option<egui::ColorImage> {
    native_preview(cluster).and_then(|bytes| decode_native_image(&bytes))
}

#[cfg(target_os = "macos")]
fn native_preview(cluster: &str) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    if cluster.len() > 256 || cluster.graphemes(true).count() != 1 {
        return None;
    }
    let packaged = std::env::current_exe()
        .ok()?
        .parent()?
        .parent()?
        .join("Resources/EmojiPreview");
    let helper = if packaged.is_file() {
        packaged
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("native/macos/build/EmojiPreview")
    };
    let mut child = {
        let _guard = crate::runtime_scope::mutation().ok()?;
        Command::new(helper)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?
    };
    if child.stdin.take()?.write_all(cluster.as_bytes()).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(128 * 1024 + 1).read_to_end(&mut bytes).ok()?;
        Some(bytes)
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader.join().ok()??;
    (success && bytes.len() <= 128 * 1024).then_some(bytes)
}
#[cfg(not(target_os = "macos"))]
fn native_preview(_: &str) -> Option<Vec<u8>> {
    None
}

struct InlineLayout {
    galley: Arc<egui::Galley>,
    images: HashMap<u32, ImageKey>,
    raw_images: HashMap<u32, String>,
}
fn layout(ui: &egui::Ui, text: &str, size: f32, color: Color32, width: f32) -> InlineLayout {
    layout_with_mode(ui, text, size, color, width, false)
}
fn layout_with_mode(
    ui: &egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
    single_line: bool,
) -> InlineLayout {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width.max(1.0);
    if single_line {
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        // Epaint inherits the last section's format for its ellipsis. That
        // section may be an empty emoji slot, so paint the ellipsis separately.
        job.wrap.overflow_character = None;
        job.break_on_newline = false;
    }
    let mut images = HashMap::new();
    let mut raw_images = HashMap::new();
    let emoji_size = (size * 1.43).round();
    let plain = egui::TextFormat {
        font_id: FontId::proportional(size),
        color,
        ..Default::default()
    };
    for part in parts(text) {
        if let Some(key) = part.image {
            let section = job.sections.len() as u32;
            job.append(
                SLOT,
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(emoji_size, egui::FontFamily::Name("emoji_slot".into())),
                    line_height: Some(emoji_size),
                    color: Color32::TRANSPARENT,
                    valign: Align::Center,
                    ..Default::default()
                },
            );
            images.insert(section, key);
            raw_images.insert(section, part.raw.to_owned());
        } else {
            // Coalesce text runs so ordinary messages retain normal kerning/layout.
            if let Some(last) = job.sections.last_mut().filter(|s| s.format == plain) {
                job.text.push_str(part.raw);
                last.byte_range.end = job.text.len();
            } else {
                job.append(part.raw, 0.0, plain.clone());
            }
        }
    }
    InlineLayout {
        galley: ui.fonts(|f| f.layout_job(job)),
        images,
        raw_images,
    }
}
/// Measure using the same atomic emoji cells as the rendered text.
pub(super) fn measure(ui: &egui::Ui, text: &str, size: f32, width: f32) -> Vec2 {
    layout(ui, text, size, Color32::WHITE, width).galley.size()
}
// Restore the original text/cursor domain without laying it out again. Each
// image keeps one visible cell; extra raw scalars have zero advance at the same
// midpoint so pointer hit testing jumps over the complete alias/grapheme.
fn selection_galley(layout: &InlineLayout) -> Arc<egui::Galley> {
    let mut galley = (*layout.galley).clone();
    let mut job = (*galley.job).clone();
    let old = job.text.clone();
    job.text.clear();
    for (index, section) in job.sections.iter_mut().enumerate() {
        let raw = layout
            .raw_images
            .get(&(index as u32))
            .map(String::as_str)
            .unwrap_or(&old[section.byte_range.clone()]);
        let start = job.text.len();
        job.text.push_str(raw);
        section.byte_range = start..job.text.len();
    }
    galley.job = Arc::new(job);
    for row in &mut galley.rows {
        let mut expanded = Vec::with_capacity(row.glyphs.len());
        for glyph in &row.glyphs {
            if let Some(raw) = layout.raw_images.get(&glyph.section_index) {
                for (index, chr) in raw.chars().enumerate() {
                    let mut copy = *glyph;
                    copy.chr = chr;
                    if index > 0 {
                        copy.pos.x += glyph.advance_width * 0.5;
                        copy.advance_width = 0.0;
                    }
                    expanded.push(copy);
                }
            } else {
                expanded.push(*glyph);
            }
        }
        row.glyphs = expanded;
    }
    Arc::new(galley)
}

/// Render selectable raw text with asynchronously populated inline emoji cells.
pub(super) fn show(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
) -> egui::Response {
    let layout = layout(ui, text, size, color, width);
    if layout.images.is_empty() {
        return ui.add(egui::Label::new(RichText::new(text).size(size).color(color)).wrap());
    }
    // Keep standard label selection, accessibility, multi-label copy and raw
    // clipboard content. The internal slot character never reaches selection.
    let response = ui.add(egui::Label::new(selection_galley(&layout)));
    paint_images(ui, &layout, response.rect.min, ui.clip_rect(), color);
    response
}

/// Use the same galley for selection, emoji artwork and mention hit testing.
/// The byte cursor follows the restored raw text, including emoji aliases.
pub(super) fn show_with_mentions(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
    mentions: &[crate::message_navigation::MentionSpan],
) -> Option<usize> {
    if mentions.is_empty() {
        show(ui, text, size, color, width);
        return None;
    }
    let layout = layout(ui, text, size, color, width);
    let galley = selection_galley(&layout);
    let response = ui.add(egui::Label::new(galley.clone()));
    paint_images(ui, &layout, response.rect.min, ui.clip_rect(), color);
    let rectangles = mention_rectangles(&galley, mentions);
    let mut clicked = None;
    for (index, row, rect) in rectangles {
        let rect = rect.translate(response.rect.min.to_vec2());
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let hit = ui
            .interact(
                rect,
                response.id.with(("mention", index, row)),
                Sense::click(),
            )
            .on_hover_cursor(CursorIcon::PointingHand);
        hit.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Link,
                ui.is_enabled(),
                format!("Mở hồ sơ {}", &text[mentions[index].byte_range.clone()]),
            )
        });
        ui.painter().line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(0.7, color),
        );
        if hit.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

fn mention_rectangles(
    galley: &egui::Galley,
    mentions: &[crate::message_navigation::MentionSpan],
) -> Vec<(usize, usize, Rect)> {
    let mut byte = 0;
    let mut rectangles = Vec::new();
    for (row_index, row) in galley.rows.iter().enumerate() {
        let mut row_rectangles: HashMap<usize, Rect> = HashMap::new();
        for glyph in &row.glyphs {
            let end = byte + glyph.chr.len_utf8();
            for (index, span) in mentions.iter().enumerate() {
                if span.byte_range.start <= byte && end <= span.byte_range.end {
                    let rect = glyph.logical_rect();
                    row_rectangles
                        .entry(index)
                        .and_modify(|old| *old = old.union(rect))
                        .or_insert(rect);
                }
            }
            byte = end;
        }
        rectangles.extend(
            row_rectangles
                .into_iter()
                .map(|(index, rect)| (index, row_index, rect)),
        );
        if row.ends_with_newline {
            byte += 1;
        }
    }
    rectangles
}

fn paint_images(ui: &egui::Ui, layout: &InlineLayout, origin: Pos2, clip: Rect, color: Color32) {
    let origin = origin.to_vec2();
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(clip));
    for row in &layout.galley.rows {
        for glyph in &row.glyphs {
            if glyph.chr == SLOT.chars().next().unwrap() {
                let Some(key) = layout.images.get(&glyph.section_index) else {
                    continue;
                };
                let cell = glyph.logical_rect().translate(origin);
                let size = Vec2::splat(glyph.advance_width.min(glyph.line_height));
                let target = Rect::from_center_size(cell.center(), size);
                // Long messages must not decode/enqueue off-screen rows.
                if !ui.is_rect_visible(target) {
                    continue;
                }
                match image(ui.ctx(), key) {
                    Some(Cached::Ready(texture)) => {
                        let actual = Vec2::new(
                            size.x,
                            size.x * texture.size()[1] as f32 / texture.size()[0] as f32,
                        );
                        painter.image(
                            texture.id(),
                            Rect::from_center_size(target.center(), actual),
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    Some(Cached::Failed) | None => {
                        let fallback = match key {
                            ImageKey::Native(cluster) => Some(cluster.as_str()),
                            ImageKey::Original(sprite) => catalog()
                                .canonical
                                .get(sprite)
                                .map(String::as_str)
                                .or_else(|| {
                                    layout
                                        .raw_images
                                        .get(&glyph.section_index)
                                        .map(String::as_str)
                                }),
                        };
                        if let Some(cluster) = fallback {
                            painter.with_clip_rect(target).text(
                                target.center(),
                                egui::Align2::CENTER_CENTER,
                                cluster,
                                FontId::proportional(size.y * 0.8),
                                color,
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Truncate only at complete extended-grapheme boundaries.
pub(super) fn truncate_graphemes(text: &str, max: usize) -> std::borrow::Cow<'_, str> {
    if let Some((end, _)) = text.grapheme_indices(true).nth(max) {
        std::borrow::Cow::Owned(format!("{}…", &text[..end]))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod inline_tests {
    use super::*;

    #[test]
    fn inline_emoji_aliases_and_modified_graphemes_preserve_source() {
        let text = "Chào :+1: 👍🏽 👩‍💻 🇻🇳 1️⃣ e\u{301}";
        let parts = parts(text);
        assert_eq!(parts.iter().map(|part| part.raw).collect::<String>(), text);
        for cluster in ["👍🏽", "👩‍💻", "🇻🇳", "1️⃣"] {
            assert!(parts
                .iter()
                .any(|part| part.raw == cluster
                    && part.image == Some(ImageKey::Native(cluster.into()))));
        }
        assert!(parts
            .iter()
            .any(|part| part.raw == ":+1:" && part.image.is_some()));
        assert_eq!(truncate_graphemes("👩‍💻👍🏽a", 1), "👩‍💻…");
        assert!(unicode_image("❤\u{fe0e}").is_none());
    }

    #[test]
    fn inline_emoji_pending_and_completed_caches_stay_bounded() {
        let mut cache = EmojiCache::default();
        let mut submitted = 0;
        for index in 0..40 {
            let cluster = format!("cluster-{index}");
            for _ in 0..2 {
                request_image(&mut cache, &ImageKey::Native(cluster.clone()), |_| {
                    submitted += 1;
                    true
                });
            }
        }
        assert_eq!(submitted, QUEUE_LIMIT);
        assert_eq!(cache.pending.len(), QUEUE_LIMIT);
        for index in 0..400 {
            cache.insert(ImageKey::Native(index.to_string()), Cached::Failed);
        }
        assert_eq!(cache.entries.len(), CACHE_LIMIT);
        assert_eq!(cache.order.len(), CACHE_LIMIT);
    }

    #[test]
    fn inline_emoji_selection_and_wrapping_keep_raw_aliases_and_clusters() {
        let ctx = egui::Context::default();
        setup_custom_fonts(&ctx);
        let text = "😀 :)) 👩‍💻👍🏽 @An";
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let layout = layout(ui, text, 14.0, Color32::WHITE, 65.0);
                let raw = selection_galley(&layout);
                assert_eq!(raw.job.text, text);
                assert_eq!(
                    raw.rows.iter().map(|row| row.glyphs.len()).sum::<usize>(),
                    text.chars().count()
                );
                assert!(raw.rows.len() > 1);
                for row in &layout.galley.rows {
                    assert!(row.rect.width() <= 66.0);
                }
            });
        });
    }

    #[test]
    fn inline_emoji_pointer_selection_copy_event_returns_raw_source() {
        let text = "Prefix :)) 👍🏽 👩‍💻\nXin chào ❤️ suffix";
        let total = text.chars().count();
        let middle_start = "Prefix ".chars().count();
        let middle_end = "Prefix :)) 👍🏽 👩‍💻".chars().count();
        for (start_index, end_index) in [(0, total), (middle_start, middle_end)] {
            let ctx = egui::Context::default();
            setup_custom_fonts(&ctx);
            let mut start = Pos2::ZERO;
            let mut end = Pos2::ZERO;
            for frame in 0..5 {
                let event = |pos, pressed| egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                };
                let events = match frame {
                    1 => vec![egui::Event::PointerMoved(start), event(start, true)],
                    2 => vec![egui::Event::PointerMoved(end)],
                    3 => vec![event(end, false)],
                    4 => vec![egui::Event::Copy],
                    _ => vec![],
                };
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 480.0))),
                        time: Some(frame as f64),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let raw =
                                selection_galley(&layout(ui, text, 14.0, Color32::WHITE, 130.0));
                            let response = show(ui, text, 14.0, Color32::WHITE, 130.0);
                            start = raw
                                .pos_from_ccursor(egui::text::CCursor::new(start_index))
                                .center()
                                + response.rect.min.to_vec2();
                            end = raw
                                .pos_from_ccursor(egui::text::CCursor::new(end_index))
                                .center()
                                + response.rect.min.to_vec2();
                        });
                    },
                );
                if frame == 4 {
                    let expected: String = text
                        .chars()
                        .skip(start_index)
                        .take(end_index - start_index)
                        .collect();
                    assert_eq!(output.platform_output.copied_text, expected);
                }
            }
        }
    }

    #[test]
    fn inline_emoji_tokenizer_matches_recovered_fixture() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/zalo-emoji-tokenizer.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let text = case["input"].as_str().unwrap();
            let actual: Vec<_> = aliases(text)
                .into_iter()
                .map(|(a, b, _)| {
                    serde_json::json!({"start_utf16":text[..a].encode_utf16().count(),
                    "end_utf16":text[..b].encode_utf16().count(),"raw":&text[a..b]})
                })
                .collect();
            let expected: Vec<_> = case["segments"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|v| v["kind"] == "emoji")
                .map(|v| {
                    serde_json::json!({
                    "start_utf16":v["start_utf16"],"end_utf16":v["end_utf16"],"raw":v["raw"]})
                })
                .collect();
            assert_eq!(actual, expected, "case {}", case["id"]);
        }
    }
    #[test]
    fn inline_emoji_original_validation_rejects_corruption_and_wrong_bounds() {
        let pixels = image::RgbaImage::from_pixel(64, 63, image::Rgba([240, 100, 20, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let mut sprite = Sprite {
            offset: 0,
            length: bytes.len(),
            width: 64,
            height: 63,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        };
        assert_eq!(decode_original(&bytes, &sprite).unwrap().size, [64, 63]);
        assert!(decode_original(&[], &sprite).is_none());
        sprite.sha256 = "wrong".into();
        assert!(decode_original(&bytes, &sprite).is_none());
        sprite.offset = usize::MAX;
        assert!(decode_original(&bytes, &sprite).is_none());
    }

    #[test]
    fn inline_emoji_noncanonical_aliases_stay_raw_without_optional_pack() {
        if !std::hint::black_box(PACK).is_empty() {
            return;
        }
        assert!(parts(":))").iter().all(|part| part.image.is_none()));
        assert_eq!(catalog().index.sprites.len(), 1383);
        assert_eq!(catalog().index.aliases.len(), 1440);
    }

    #[test]
    #[ignore = "Requires explicit ZL_EMOJI_PACK containing all 1383 original PNGs"]
    fn all_assets_are_exact_original_pngs() {
        let packed = std::hint::black_box(PACK);
        assert!(!packed.is_empty());
        assert_eq!(catalog().index.sprites.len(), 1383);
        for sprite in &catalog().index.sprites {
            assert!(decode_original(PACK, sprite).is_some());
        }
    }

    #[test]
    fn inline_emoji_decoder_rejects_oversized_and_wrong_dimension_png() {
        assert!(decode_native_image(&vec![0; 128 * 1024 + 1]).is_none());
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());
        bytes.extend_from_slice(&64u32.to_be_bytes());
        assert!(decode_native_image(&bytes).is_none());
    }
}
#[cfg(test)]
mod mention_interaction_tests {
    use super::*;
    use crate::message_navigation::MentionSpan;

    #[test]
    fn mention_hit_rectangles_follow_raw_emoji_newlines_and_wrapping() {
        let ctx = egui::Context::default();
        setup_custom_fonts(&ctx);
        let text = "😀 :))\n@Tên dài với dấu và tên thứ hai";
        let start = text.find('@').unwrap();
        let spans = vec![MentionSpan {
            user_id: "123".into(),
            byte_range: start..text.len(),
        }];
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let layout = layout(ui, text, 14.0, Color32::WHITE, 90.0);
                let galley = selection_galley(&layout);
                assert_eq!(galley.job.text, text);
                let rectangles = mention_rectangles(&galley, &spans);
                assert!(
                    rectangles.len() >= 2,
                    "Long mentions must retain multiple hit rows"
                );
                for (index, row, rect) in rectangles {
                    assert_eq!(index, 0);
                    assert!(
                        row >= 1,
                        "Emoji on the first line must not become a mention"
                    );
                    assert!(rect.width() > 0.0);
                }
            });
        });
    }

    #[test]
    fn semantic_pointer_events_click_exact_uid_after_emoji_without_changing_layout() {
        let ctx = egui::Context::default();
        setup_custom_fonts(&ctx);
        let text = "😀 :)) @An @An";
        let spans = vec![
            MentionSpan {
                user_id: "11".into(),
                byte_range: 9..12,
            },
            MentionSpan {
                user_id: "22".into(),
                byte_range: 13..16,
            },
        ];
        let mut target = Pos2::ZERO;
        let mut clicked = None;
        for frame in 0..3 {
            let events = if frame == 0 {
                vec![]
            } else {
                vec![
                    egui::Event::PointerMoved(target),
                    egui::Event::PointerButton {
                        pos: target,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 1,
                        modifiers: Default::default(),
                    },
                ]
            };
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))),
                    events,
                    time: Some(frame as f64 * 0.016),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let layout = layout(ui, text, 14.0, Color32::WHITE, 300.0);
                        let galley = selection_galley(&layout);
                        let original_size = measure(ui, text, 14.0, 300.0);
                        let rect = mention_rectangles(&galley, &spans)
                            .into_iter()
                            .find(|(index, _, _)| *index == 1)
                            .unwrap()
                            .2;
                        target = rect.center() + ui.next_widget_position().to_vec2();
                        clicked = show_with_mentions(ui, text, 14.0, Color32::WHITE, 300.0, &spans)
                            .or(clicked);
                        assert_eq!(galley.size(), original_size);
                    });
                },
            );
        }
        assert_eq!(
            clicked.map(|index| spans[index].user_id.as_str()),
            Some("22")
        );
    }
}

#[cfg(test)]
/// Admit real worker results in renderer integration tests without installing a global worker.
pub(super) fn seed_worker_results(
    ctx: &egui::Context,
    images: impl Iterator<Item = (&'static str, egui::ColorImage)>,
) {
    for (cluster, image) in images {
        cache(ctx)
            .lock()
            .unwrap()
            .pending
            .insert(ImageKey::Native(cluster.into()));
        accept_result(ctx, cluster.into(), Some(image), None);
    }
}

#[cfg(test)]
#[path = "emoji_text/visual_tests.rs"]
mod visual_tests;

#[cfg(test)]
mod scope_hook_tests {
    use super::*;

    #[test]
    fn emoji_epoch_reset_releases_pending_but_retains_completed_cache() {
        let ctx = egui::Context::default();
        let shared = cache(&ctx);
        let pending = ImageKey::Native("pending".to_owned());
        let completed = ImageKey::Native("completed".to_owned());
        {
            let mut cache = shared.lock().unwrap();
            cache.pending.insert(pending.clone());
            cache.insert(completed.clone(), Cached::Failed);
        }
        reset_pending(&ctx);
        let mut cache = shared.lock().unwrap();
        assert!(cache.pending.is_empty());
        assert!(cache.entries.contains_key(&completed));
        assert_eq!(cache.order.len(), 1);
        assert!(cache.pending.insert(pending));
    }
}
