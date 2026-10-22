//! An image's panel (issue #19): its name, size, frames and users with
//! Rename, Copy, Replace with PNG and Remove; then the skin editor's
//! painting (`olr-skin-editor`'s tools, its canvas's look): on the left
//! what is painted (Colour or Glow), the tools, the brush, the colour with
//! the recent ones or the glow strength, the onion skin and the frame
//! operations; in the middle the frame, large (checks, the onion frame
//! faintly, the frame, the glow in yellow while glow is painted, a pixel
//! grid when zoomed in; a primary drag paints, a secondary or middle drag
//! pans, the wheel zooms); on the right the frames and the live preview,
//! dark, as a lit match draws the sprite.

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, vec2};
use olr_data::image::{Rgba, RgbaImage};
use olr_skin_editor::Frame;

use super::{Cx, caption, labels, relabel};
use crate::art::{self, ArtDoc, GLOW_STRENGTHS, GRID_ZOOM, TARGETS, Target, Tool};
use crate::browser::Import;
use crate::state::{ArtAction, ModAction};

/// The onion skin's frame, drawn at this share.
const ONION_ALPHA: f32 = 0.35;
/// The glow overlay's colour.
const GLOW_OVERLAY: [u8; 3] = [255, 236, 64];
/// The tools' column and the frames' column, points.
const TOOLS_WIDTH: f32 = 180.0;
const FRAMES_WIDTH: f32 = 230.0;
/// A frame's button in the grid, and the preview's box, points.
const FRAME_BUTTON: f32 = 40.0;
const PREVIEW_BOX: f32 = 128.0;
/// The preview's night.
const NIGHT: Color32 = Color32::from_rgb(10, 10, 18);

/// The Art panel's own data: the canvas's drag and its textures.
#[derive(Default)]
pub(crate) struct ArtData {
    /// Where the canvas was drawn last (tests point into it).
    pub(crate) canvas: Option<Rect>,
    drag: bool,
    last_pixel: Option<(i32, i32)>,
    /// The typed `#rrggbb`, while the field has the keyboard.
    hex: Option<String>,
    /// Textures by what they show and the art generation they were made at.
    frame: Option<((u64, usize), TextureHandle)>,
    onion: Option<((u64, usize), TextureHandle)>,
    overlay: Option<((u64, usize), TextureHandle)>,
    strip: Option<(u64, TextureHandle)>,
    dark: Option<(u64, TextureHandle)>,
    checks: Option<(u32, TextureHandle)>,
}

fn colour32(c: Rgba) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn texture(ui: &Ui, name: &str, img: &RgbaImage) -> TextureHandle {
    let size = [img.width() as usize, img.height() as usize];
    ui.ctx().load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
        egui::TextureOptions::NEAREST,
    )
}

/// A cached texture: made again when its key changed.
fn cached<K: PartialEq + Copy>(
    slot: &mut Option<(K, TextureHandle)>,
    key: K,
    make: impl FnOnce() -> TextureHandle,
) -> TextureHandle {
    match slot {
        Some((k, t)) if *k == key => t.clone(),
        _ => {
            let t = make();
            *slot = Some((key, t.clone()));
            t
        }
    }
}

fn frame_image(f: &Frame) -> RgbaImage {
    let mut img = RgbaImage::new(f.w, f.h);
    for (i, &c) in f.px.iter().enumerate() {
        img.set(i as u32 % f.w, i as u32 / f.w, c);
    }
    img
}

/// The glowing pixels in yellow, as strong as their glow; the rest clear.
fn glow_overlay(f: &Frame) -> RgbaImage {
    let mut img = RgbaImage::new(f.w, f.h);
    for (i, &g) in f.glow.iter().enumerate() {
        if g != 0 {
            let [r, gr, b] = GLOW_OVERLAY;
            img.set(
                i as u32 % f.w,
                i as u32 / f.w,
                Rgba { r, g: gr, b, a: (60 + u32::from(g) * 170 / 255) as u8 },
            );
        }
    }
    img
}

/// A check per pixel of a `side` square frame.
fn checks_image(side: u32) -> RgbaImage {
    let mut img = RgbaImage::new(side, side);
    for y in 0..side {
        for x in 0..side {
            let v = if (x + y) % 2 == 0 { 92 } else { 72 };
            img.set(x, y, Rgba::rgb(v, v, v));
        }
    }
    img
}

pub(super) fn show(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    let crate::tree::Item::Art(path) = state.item().clone() else { return };
    header(ui, cx, &path);
    let doc = match state.art() {
        Some(Ok(doc)) => doc,
        Some(Err(why)) => {
            ui.colored_label(ui.visuals().error_fg_color, format!("This image does not paint: {why}"));
            ui.weak("Replace it with a PNG, or remove it.");
            return;
        }
        None => return,
    };
    if let Some(note) = doc.mask_note() {
        ui.colored_label(ui.visuals().warn_fg_color, format!("⚠ {note}"));
    }
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(TOOLS_WIDTH);
            tools(ui, cx);
        });
        let w = (ui.available_width() - FRAMES_WIDTH - 12.0).max(120.0);
        let h = ui.available_height().max(160.0);
        canvas(ui, cx, doc, vec2(w, h));
        ui.vertical(|ui| {
            ui.set_width(FRAMES_WIDTH);
            frames(ui, cx, doc);
            ui.add_space(8.0);
            preview(ui, cx, doc, &path);
        });
    });
}

/// The image's name, size, frames and users, and its operations.
fn header(ui: &mut Ui, cx: &mut Cx, path: &str) {
    let state = cx.state;
    ui.horizontal(|ui| {
        ui.heading(path);
        if let Some(Ok(doc)) = state.art() {
            let (w, h) = doc.size();
            let n = doc.frames();
            ui.weak(format!("{w} × {h}, {n} frame{} of {h} × {h}", if n == 1 { "" } else { "s" }));
        }
    });
    ui.horizontal(|ui| {
        if ui.button(format!("{}…", labels::RENAME_ASSET)).clicked() {
            cx.out.push(ModAction::AskRename(path.to_string()));
        }
        let r = ui.button("Copy…").on_hover_text("A new image, a copy of this one with its glow");
        relabel(&r, egui::WidgetType::Button, labels::COPY_IMAGE);
        if r.clicked() {
            cx.out.push(ModAction::AskNewImage(Some(path.to_string())));
        }
        let r = ui.button(format!("{}…", labels::REPLACE_PNG));
        relabel(&r, egui::WidgetType::Button, labels::REPLACE_PNG);
        if r.clicked() {
            cx.out.push(ModAction::Import(Import::Replace(path.to_string())));
        }
        let r = ui.button(labels::REMOVE_ASSET).on_hover_text("Refused while a projectile uses it");
        if r.clicked() {
            cx.out.push(ModAction::RemoveAsset(path.to_string()));
        }
    });
    let users = state.asset_users(path);
    if users.is_empty() {
        ui.weak("Not used yet: choose it as a projectile's image (Graphics: Image) to use it.");
    } else {
        let list: Vec<&str> = users.iter().map(|u| u.label.as_str()).collect();
        ui.label(format!("{}: {}", labels::USED_BY, list.join(", ")));
    }
}

/// What is painted, the tools, the brush, the colour or glow strength, the
/// onion skin, the frame operations and the zoom.
fn tools(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.state.tools();
    ui.strong("Paint");
    ui.horizontal(|ui| {
        for target in TARGETS {
            let name = if target == Target::Glow { labels::TARGET_GLOW } else { labels::TARGET_COLOUR };
            let r = ui.selectable_label(t.target == target, name);
            if r.clicked() {
                cx.out.push(ModAction::Art(ArtAction::Target(target)));
            }
        }
    });
    ui.weak(match t.target {
        Target::Glow => "Pixels that glow in the dark (yellow here), whatever the level's light.",
        _ => "The image's pixels.",
    });
    ui.separator();
    ui.strong("Tools");
    egui::Grid::new("olr-mod-editor-art-tools").num_columns(2).show(ui, |ui| {
        for (i, tool) in Tool::ALL.into_iter().enumerate() {
            let r = ui.selectable_label(t.tool == tool, tool.name()).on_hover_text(format!(
                "{} ({})",
                tool.name(),
                tool.key()
            ));
            if r.clicked() {
                cx.out.push(ModAction::Art(ArtAction::Tool(tool)));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    ui.add_enabled_ui(t.tool.sized(), |ui| {
        ui.horizontal(|ui| {
            for (size, label) in [(1, labels::SIZE_1), (2, labels::SIZE_2)] {
                if ui.selectable_label(t.size == size, label).clicked() {
                    cx.out.push(ModAction::Art(ArtAction::Size(size)));
                }
            }
        });
    });
    ui.separator();
    if t.target == Target::Glow {
        ui.label(labels::GLOW_STRENGTH);
        ui.horizontal(|ui| {
            for g in GLOW_STRENGTHS {
                let text = format!("{} %", (u32::from(g) * 100 + 127) / 255);
                if ui.selectable_label(t.glow_strength == g, text).clicked() {
                    cx.out.push(ModAction::Art(ArtAction::GlowStrength(g)));
                }
            }
        });
    } else {
        colour(ui, cx);
    }
    ui.separator();
    let mut onion = t.onion;
    let r = ui.checkbox(&mut onion, labels::ONION).on_hover_text("The previous frame, faintly");
    if r.changed() {
        cx.out.push(ModAction::Art(ArtAction::Onion(onion)));
    }
    ui.separator();
    ui.strong("Frame");
    ui.horizontal_wrapped(|ui| {
        if ui.button(labels::COPY_FRAME).clicked() {
            cx.out.push(ModAction::Art(ArtAction::CopyFrame));
        }
        let can = t.clipboard.is_some();
        if ui.add_enabled(can, egui::Button::new(labels::PASTE_FRAME)).clicked() {
            cx.out.push(ModAction::Art(ArtAction::PasteFrame));
        }
        let hover = "Changes the frame count: asked first";
        if ui.button(labels::ADD_FRAME).on_hover_text(hover).clicked() {
            cx.out.push(ModAction::Art(ArtAction::AddFrame));
        }
        if ui.button(labels::REMOVE_FRAME).on_hover_text(hover).clicked() {
            cx.out.push(ModAction::Art(ArtAction::RemoveFrame));
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        let r = ui.button("−");
        relabel(&r, egui::WidgetType::Button, labels::ZOOM_OUT);
        if r.clicked() {
            cx.out.push(ModAction::Art(ArtAction::Zoom(-1)));
        }
        let r = ui.button("+");
        relabel(&r, egui::WidgetType::Button, labels::ZOOM_IN);
        if r.clicked() {
            cx.out.push(ModAction::Art(ArtAction::Zoom(1)));
        }
        ui.weak(format!("{} points a pixel", t.zoom));
    });
    ui.weak("Right drag pans, the wheel zooms.");
}

/// The paint colour: a picker, its `#rrggbb` and the recent colours.
fn colour(ui: &mut Ui, cx: &mut Cx) {
    let t = cx.state.tools();
    ui.horizontal(|ui| {
        let mut c32 = colour32(t.colour);
        let r = egui::widgets::color_picker::color_edit_button_srgba(
            ui,
            &mut c32,
            egui::widgets::color_picker::Alpha::Opaque,
        );
        relabel(&r, egui::WidgetType::ColorButton, labels::PAINT_COLOUR);
        if r.changed() {
            cx.out.push(ModAction::Art(ArtAction::Colour(Rgba::rgb(c32.r(), c32.g(), c32.b()))));
        }
        let mut text = cx.data.art.hex.clone().unwrap_or_else(|| hex(t.colour));
        let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
        relabel(&field, egui::WidgetType::TextEdit, labels::HEX);
        if field.has_focus() {
            cx.data.art.hex = Some(text);
        } else if cx.data.art.hex.take().is_some()
            && let Ok(c) = olr_format::Colour::parse(text.trim())
        {
            cx.out.push(ModAction::Art(ArtAction::Colour(c.0)));
        }
    });
    if !t.recent.is_empty() {
        ui.weak("Recent");
        ui.horizontal_wrapped(|ui| {
            for &c in &t.recent {
                let (rect, r) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                ui.painter().rect_filled(rect, 2.0, colour32(c));
                if c == t.colour {
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        StrokeKind::Outside,
                    );
                }
                relabel(&r, egui::WidgetType::Button, &hex(c));
                if r.on_hover_text(hex(c)).clicked() {
                    cx.out.push(ModAction::Art(ArtAction::Colour(c)));
                }
            }
        });
    }
}

/// The frame painted, large.
fn canvas(ui: &mut Ui, cx: &mut Cx, doc: &ArtDoc, size: egui::Vec2) {
    let state = cx.state;
    let t = state.tools();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    relabel(&response, egui::WidgetType::Other, labels::CANVAS);
    cx.data.art.canvas = Some(rect);
    let side = doc.side();
    let z = t.zoom;
    let origin = rect.center() - vec2(side as f32 * z / 2.0, side as f32 * z / 2.0) + vec2(t.pan.0, t.pan.1);
    let origin = Pos2::new(origin.x.round(), origin.y.round());
    let frame_rect = Rect::from_min_size(origin, vec2(side as f32 * z, side as f32 * z));
    let pixel = |p: Pos2| (((p.x - origin.x) / z).floor() as i32, ((p.y - origin.y) / z).floor() as i32);

    let (pos, pressed, down, scroll, delta, pan_down) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.events
                .iter()
                .map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } if delta.y != 0.0 => delta.y.signum() as i32,
                    _ => 0,
                })
                .sum::<i32>(),
            i.pointer.delta(),
            i.pointer.secondary_down() || i.pointer.middle_down(),
        )
    });
    let over = pos.is_some_and(|p| rect.contains(p)) && ui.rect_contains_pointer(rect);
    let d = &mut cx.data.art;
    if d.drag && !down {
        d.drag = false;
        d.last_pixel = None;
        cx.out.push(ModAction::Art(ArtAction::Release));
    }
    if let Some(p) = pos
        && state.dialog().is_none()
    {
        let at = pixel(p);
        if pressed && over {
            d.drag = true;
            d.last_pixel = Some(at);
            cx.out.push(ModAction::Art(ArtAction::Press { x: at.0, y: at.1 }));
        } else if down && d.drag && d.last_pixel != Some(at) {
            d.last_pixel = Some(at);
            cx.out.push(ModAction::Art(ArtAction::Drag { x: at.0, y: at.1 }));
        }
    }
    if over && pan_down && response.dragged() && delta != egui::Vec2::ZERO {
        cx.out.push(ModAction::Art(ArtAction::Pan(delta.x, delta.y)));
    }
    if over && scroll != 0 {
        cx.out.push(ModAction::Art(ArtAction::Zoom(scroll)));
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(40));
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    let checks =
        cached(&mut d.checks, side, || texture(ui, "olr-mod-editor-art-checks", &checks_image(side)));
    painter.image(checks.id(), frame_rect, uv, Color32::WHITE);
    let (f, art_gen) = (t.frame, state.art_generation());
    if t.onion && f > 0 {
        let o = cached(&mut d.onion, (art_gen, f), || {
            texture(ui, "olr-mod-editor-art-onion", &frame_image(&doc.frame(f - 1)))
        });
        painter.image(o.id(), frame_rect, uv, Color32::from_white_alpha((ONION_ALPHA * 255.0) as u8));
    }
    let ft = cached(&mut d.frame, (art_gen, f), || {
        texture(ui, "olr-mod-editor-art-frame", &frame_image(&doc.frame(f)))
    });
    painter.image(ft.id(), frame_rect, uv, Color32::WHITE);
    if t.target == Target::Glow {
        let ov = cached(&mut d.overlay, (art_gen, f), || {
            texture(ui, "olr-mod-editor-art-overlay", &glow_overlay(&doc.frame(f)))
        });
        painter.image(ov.id(), frame_rect, uv, Color32::WHITE);
    }
    if z >= GRID_ZOOM {
        let line = Stroke::new(1.0, Color32::from_black_alpha(60));
        for i in 1..side {
            let x = origin.x + i as f32 * z;
            painter.line_segment([Pos2::new(x, frame_rect.top()), Pos2::new(x, frame_rect.bottom())], line);
            let y = origin.y + i as f32 * z;
            painter.line_segment([Pos2::new(frame_rect.left(), y), Pos2::new(frame_rect.right(), y)], line);
        }
    }
    painter.rect_stroke(frame_rect, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Outside);
    if let Some(p) = pos
        && over
    {
        let (x, y) = pixel(p);
        if (0..side as i32).contains(&x) && (0..side as i32).contains(&y) {
            let s = if t.tool.sized() { t.size as f32 } else { 1.0 };
            let r = Rect::from_min_size(origin + vec2(x as f32 * z, y as f32 * z), vec2(s * z, s * z));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
        }
    }
}

/// The strip's texture, and frame `f`'s share of it.
fn frame_uv(doc: &ArtDoc, f: usize) -> Rect {
    let (w, h) = doc.size();
    let share = h as f32 / w.max(1) as f32;
    Rect::from_min_max(Pos2::new(f as f32 * share, 0.0), Pos2::new((f as f32 + 1.0) * share, 1.0))
}

/// The frames, each a button.
fn frames(ui: &mut Ui, cx: &mut Cx, doc: &ArtDoc) {
    let state = cx.state;
    ui.strong(format!("Frames ({})", doc.frames()));
    let art_gen = state.art_generation();
    let strip =
        cached(&mut cx.data.art.strip, art_gen, || texture(ui, "olr-mod-editor-art-strip", &doc.image()));
    egui::ScrollArea::vertical().id_salt("olr-mod-editor-art-frames").max_height(220.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for f in 0..doc.frames() {
                let (rect, r) = ui.allocate_exact_size(vec2(FRAME_BUTTON, FRAME_BUTTON), Sense::click());
                ui.painter().rect_filled(rect, 2.0, Color32::from_gray(60));
                ui.painter().image(strip.id(), rect.shrink(2.0), frame_uv(doc, f), Color32::WHITE);
                if f == state.tools().frame {
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        StrokeKind::Inside,
                    );
                }
                let selected = f == state.tools().frame;
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, labels::frame(f))
                });
                if r.on_hover_text(labels::frame(f)).clicked() {
                    cx.out.push(ModAction::Art(ArtAction::Frame(f)));
                }
            }
        });
    });
}

/// The live preview: the animation playing or the heading turning, in the
/// dark, the glowing pixels bright.
fn preview(ui: &mut Ui, cx: &mut Cx, doc: &ArtDoc, path: &str) {
    let state = cx.state;
    ui.strong(labels::PREVIEW);
    let gfx = state.image_gfx(path);
    ui.weak(match &gfx {
        Some(olr_format::mods::Gfx::Image { animation: Some(_), .. }) => {
            "Its animation, as its first user plays it."
        }
        Some(olr_format::mods::Gfx::Image { frames_follow_heading: true, .. })
        | Some(olr_format::mods::Gfx::Image { frames_follow_heading_mirrored: true, .. }) => {
            "Turning through its heading frames."
        }
        _ => "Every frame in turn.",
    });
    let t = ui.input(|i| i.time);
    let f = art::preview_frame(gfx.as_ref(), doc.frames(), t);
    // No repaint is asked for: the game's loop draws every frame.
    let art_gen = state.art_generation();
    let dark = cached(&mut cx.data.art.dark, art_gen, || {
        texture(ui, "olr-mod-editor-art-dark", &art::dark(doc.strip()))
    });
    let (rect, r) = ui.allocate_exact_size(vec2(PREVIEW_BOX, PREVIEW_BOX), Sense::hover());
    relabel(&r, egui::WidgetType::Image, labels::PREVIEW);
    ui.painter().rect_filled(rect, 4.0, NIGHT);
    let side = (PREVIEW_BOX * 0.5).min(doc.side() as f32 * 8.0).max(doc.side() as f32);
    let sprite = Rect::from_center_size(rect.center(), vec2(side, side));
    ui.painter().image(dark.id(), sprite, frame_uv(doc, f), Color32::WHITE);
    ui.horizontal(|ui| {
        caption(ui, &format!("frame {} of {}", f + 1, doc.frames()));
    });
}
