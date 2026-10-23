//! The chosen frame, large: checks under its transparent pixels, the
//! onion skin's frame faintly, the frame, the tint or glow overlay while
//! that is painted, and a pixel grid when zoomed in. A primary press and
//! drag paints (art pixels from the frame's top left); a secondary or
//! middle drag pans; the wheel zooms.

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use olr_data::image::{Rgba, RgbaImage};

use super::{SkinEditorData, checks, frame_texture, labels, relabel, text_width, texture};
use crate::paint::Target;
use crate::state::{GRID_ZOOM, SkinAction, SkinEditorState};

/// The onion skin's frame, drawn at this share.
const ONION_ALPHA: f32 = 0.35;
/// The player-colour overlay's colour and the glow's.
const TINT_OVERLAY: Rgba = Rgba { r: 255, g: 0, b: 220, a: 150 };
const GLOW_OVERLAY: [u8; 3] = [255, 236, 64];
/// How the canvas pans and zooms.
const PAN_HINT: &str = "right drag pans, the wheel zooms";

pub(super) fn show(
    ui: &mut Ui,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    out: &mut Vec<SkinAction>,
) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("−").on_hover_text(labels::ZOOM_OUT).clicked() {
            out.push(SkinAction::Zoom(-1));
        }
        if ui.button("+").on_hover_text(labels::ZOOM_IN).clicked() {
            out.push(SkinAction::Zoom(1));
        }
        // How to pan and zoom shows when it fits on a line, else on hover.
        let zoom = format!("{} points a pixel", state.zoom());
        let hint = format!("{zoom} · {PAN_HINT}");
        if text_width(ui, &hint, egui::TextStyle::Body) <= ui.available_width() {
            ui.weak(hint);
        } else {
            ui.weak(zoom).on_hover_text(PAN_HINT);
        }
    });
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    relabel(&response, egui::WidgetType::Other, labels::CANVAS);
    data.canvas_rect = Some(rect);
    let doc = state.doc();
    let (fw, fh) = doc.frame_size();
    let z = state.zoom();
    let (px, py) = state.pan();
    let origin = rect.center() - vec2(fw as f32 * z / 2.0, fh as f32 * z / 2.0) + vec2(px, py);
    let origin = Pos2::new(origin.x.round(), origin.y.round());
    let frame_rect = Rect::from_min_size(origin, vec2(fw as f32 * z, fh as f32 * z));
    let pixel = |p: Pos2| (((p.x - origin.x) / z).floor() as i32, ((p.y - origin.y) / z).floor() as i32);

    // The pointer.
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
    let free = state.dialog().is_none();
    if data.canvas_drag && !down {
        data.canvas_drag = false;
        data.last_pixel = None;
        out.push(SkinAction::Release);
    }
    if let Some(p) = pos
        && free
    {
        let at = pixel(p);
        if pressed && over {
            data.canvas_drag = true;
            data.last_pixel = Some(at);
            out.push(SkinAction::Press { x: at.0, y: at.1 });
        } else if down && data.canvas_drag && data.last_pixel != Some(at) {
            data.last_pixel = Some(at);
            out.push(SkinAction::Drag { x: at.0, y: at.1 });
        }
    }
    if over && pan_down && response.dragged() && delta != egui::Vec2::ZERO {
        out.push(SkinAction::Pan(delta.x, delta.y));
    }
    if over && scroll != 0 {
        out.push(SkinAction::Zoom(scroll));
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(48));
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    let checks = checks(ui, data, (fw, fh));
    painter.image(checks.id(), frame_rect, uv, Color32::WHITE);
    let f = state.frame();
    if let Some(o) = state.onion().of(f) {
        let t = frame_texture(ui, state, data, o);
        painter.image(t.id(), frame_rect, uv, Color32::from_white_alpha((ONION_ALPHA * 255.0) as u8));
    }
    let t = frame_texture(ui, state, data, f);
    painter.image(t.id(), frame_rect, uv, Color32::WHITE);
    if state.target() != Target::Colour {
        let key = (f, doc.frame_generation(f), state.target());
        let t = match &data.overlay {
            Some((k, t)) if *k == key => t.clone(),
            _ => {
                let t = texture(ui, "olr-skin-editor-overlay", &overlay(state, f));
                data.overlay = Some((key, t.clone()));
                t
            }
        };
        painter.image(t.id(), frame_rect, uv, Color32::WHITE);
    }
    if z >= GRID_ZOOM {
        let line = Stroke::new(1.0, Color32::from_black_alpha(60));
        for x in 1..fw {
            let sx = origin.x + x as f32 * z;
            painter.line_segment([Pos2::new(sx, frame_rect.top()), Pos2::new(sx, frame_rect.bottom())], line);
        }
        for y in 1..fh {
            let sy = origin.y + y as f32 * z;
            painter.line_segment([Pos2::new(frame_rect.left(), sy), Pos2::new(frame_rect.right(), sy)], line);
        }
    }
    painter.rect_stroke(frame_rect, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Outside);
    // The pixel under the pointer.
    if let Some(p) = pos
        && over
    {
        let (x, y) = pixel(p);
        if (0..fw as i32).contains(&x) && (0..fh as i32).contains(&y) {
            let s = if state.tool().sized() { state.size() as f32 } else { 1.0 };
            let r = Rect::from_min_size(origin + vec2(x as f32 * z, y as f32 * z), vec2(s * z, s * z));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
        }
    }
}

/// The painted target over frame `f`: the player-colour pixels in
/// magenta, or the glowing ones in yellow as strong as their glow; the
/// rest clear.
fn overlay(state: &SkinEditorState, f: usize) -> RgbaImage {
    let fr = state.doc().frame(f);
    let mut img = RgbaImage::new(fr.w, fr.h);
    for i in 0..fr.px.len() {
        let p = match state.target() {
            Target::Tint if fr.tint[i] != 0 => TINT_OVERLAY,
            Target::Glow if fr.glow[i] != 0 => {
                let [r, g, b] = GLOW_OVERLAY;
                Rgba { r, g, b, a: (60 + u32::from(fr.glow[i]) * 170 / 255) as u8 }
            }
            _ => Rgba::default(),
        };
        img.set(i as u32 % fr.w, i as u32 / fr.w, p);
    }
    img
}
