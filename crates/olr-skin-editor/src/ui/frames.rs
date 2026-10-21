//! The right panel: the 21 frames (7 aim columns by 3 walk rows, each a
//! button named by its frame, `state::frame_name`), the live preview (the
//! worm walking and aiming, turning both ways, in a player colour, at
//! game size and enlarged, on a light background and a dark one where
//! the glow and the skin light show; `olr_edit::skin::preview_frame`),
//! and the skin's settings (Name, Skin light, Make double resolution).

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use olr_data::image::Rgba;
use olr_data::skin::SkinLight;
use olr_edit::skin::{PREVIEW_CELL, preview_frame};
use olr_look::engine::{MAX_SKIN_LIGHT_INTENSITY, MAX_SKIN_LIGHT_REACH};

use super::{SkinEditorData, checks, colour32, frame_texture, labels, relabel, texture};
use crate::doc::{AIM_FRAMES, WALK_FRAMES};
use crate::state::{DEFAULT_LIGHT, SkinAction, SkinEditorState, frame_name};

/// A frame's button: 6 points a world pixel (a worm is 10 × 9).
const THUMB: egui::Vec2 = egui::Vec2::new(60.0, 54.0);
/// The preview's points per world pixel: game size and enlarged.
const GAME_SIZE: f32 = 3.0;
const ENLARGED: f32 = 7.0;
/// What each aim column shows (the game's seven aim ranges).
const AIMS: [&str; AIM_FRAMES] = ["↑ up", "", "", "", "→ level", "", "↓ down"];

pub(super) fn show(
    ui: &mut Ui,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    out: &mut Vec<SkinAction>,
    fields: &mut Vec<SkinAction>,
) {
    ui.strong("Frames");
    egui::Grid::new("olr-skin-editor-frames").spacing([4.0, 4.0]).show(ui, |ui| {
        ui.label("");
        for (a, hint) in AIMS.iter().enumerate() {
            ui.vertical_centered(|ui| {
                ui.label(format!("Aim {}", a + 1));
                ui.weak(*hint);
            });
        }
        ui.end_row();
        for w in 0..WALK_FRAMES {
            ui.label(format!("Walk {}", w + 1));
            for a in 0..AIM_FRAMES {
                let f = w * AIM_FRAMES + a;
                let (rect, r) = ui.allocate_exact_size(THUMB, Sense::click());
                let uv = Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0));
                let size = state.doc().frame_size();
                let c = checks(ui, data, size);
                ui.painter().image(c.id(), rect, uv, Color32::WHITE);
                let t = frame_texture(ui, state, data, f);
                ui.painter().image(t.id(), rect, uv, Color32::WHITE);
                let (width, colour) = if f == state.frame() {
                    (3.0, ui.visuals().selection.stroke.color)
                } else if r.hovered() {
                    (1.0, Color32::WHITE)
                } else {
                    (1.0, Color32::from_gray(80))
                };
                ui.painter().rect_stroke(rect, 2.0, Stroke::new(width, colour), StrokeKind::Outside);
                relabel(&r, egui::WidgetType::Button, &frame_name(f));
                if r.on_hover_text(frame_name(f)).clicked() {
                    out.push(SkinAction::Frame(f));
                }
            }
            ui.end_row();
        }
    });
    ui.weak(format!("Painting {}", frame_name(state.frame()).to_lowercase()));
    ui.separator();
    preview(ui, state, data, out);
    ui.separator();
    settings(ui, state, data, fields);
}

/// The worm as it plays, light and dark, at game size and enlarged.
fn preview(ui: &mut Ui, state: &SkinEditorState, data: &mut SkinEditorData, out: &mut Vec<SkinAction>) {
    ui.horizontal(|ui| {
        ui.strong("Preview");
        let mut on = state.animate();
        if ui.checkbox(&mut on, labels::ANIMATE).changed() {
            out.push(SkinAction::Animate(on));
        }
        let mut c32 = colour32(state.player_colour());
        let r = egui::widgets::color_picker::color_edit_button_srgba(
            ui,
            &mut c32,
            egui::widgets::color_picker::Alpha::Opaque,
        );
        relabel(&r, egui::WidgetType::ColorButton, labels::PREVIEW_COLOUR);
        if r.on_hover_text(labels::PREVIEW_COLOUR).changed() {
            out.push(SkinAction::PlayerColour(Rgba::rgb(c32.r(), c32.g(), c32.b())));
        }
    });
    let doc = state.doc();
    let generation = doc.generation();
    if data.compiled.as_ref().is_none_or(|(g, _)| *g != generation) {
        let skin = olr_format::skins::compile_skin(&doc.to_source()).ok();
        data.compiled = Some((generation, skin));
    }
    let Some((_, Some(skin))) = &data.compiled else {
        ui.weak("This skin does not load in the game yet.");
        return;
    };
    let (f, left) = state.preview_pose();
    let key = (generation, f, left, state.player_colour());
    if data.preview.as_ref().is_none_or(|(k, _)| *k != key) {
        let k = doc.art_scale();
        let cell = |dark| preview_frame(skin, state.player_colour(), f as u32, left, dark, k);
        let light = texture(ui, "olr-skin-editor-preview-light", &cell(false));
        let dark = texture(ui, "olr-skin-editor-preview-dark", &cell(true));
        data.preview = Some((key, [light, dark]));
    }
    let Some((_, [light, dark])) = &data.preview else { return };
    let cell = vec2(PREVIEW_CELL.0 as f32, PREVIEW_CELL.1 as f32);
    ui.horizontal(|ui| {
        for scale in [GAME_SIZE, ENLARGED] {
            for t in [light, dark] {
                ui.add(egui::Image::new((t.id(), cell * scale)));
            }
        }
    });
    ui.weak("Light and dark, at game size and enlarged: the dark shows the glow and the skin light.");
}

/// Name, Skin light (its intensity and reach), Make double resolution.
fn settings(ui: &mut Ui, state: &SkinEditorState, data: &mut SkinEditorData, out: &mut Vec<SkinAction>) {
    ui.strong("Skin");
    let doc = state.doc();
    ui.horizontal(|ui| {
        let l = ui.label(labels::NAME);
        let mut text = data.name.clone().unwrap_or_else(|| doc.name().to_string());
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(240.0)).labelled_by(l.id);
        if r.has_focus() {
            data.name = Some(text);
        } else if data.name.take().is_some() {
            let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if !escaped && text.trim() != doc.name() && !text.trim().is_empty() {
                out.push(SkinAction::SetName(text));
            }
        }
    });
    let light = doc.light();
    let mut on = light.is_some();
    if ui
        .checkbox(&mut on, labels::SKIN_LIGHT)
        .on_hover_text("The glowing pixels light the worm's surroundings")
        .changed()
    {
        out.push(SkinAction::SetLight(on.then_some(DEFAULT_LIGHT)));
    }
    if let Some(l) = light {
        let (mut intensity, mut reach) = data.light.unwrap_or((l.intensity, l.reach));
        let mut dragging = false;
        let mut changed = false;
        ui.horizontal(|ui| {
            let lab = ui.label(labels::LIGHT_INTENSITY);
            let r =
                ui.add(egui::Slider::new(&mut intensity, 0..=MAX_SKIN_LIGHT_INTENSITY)).labelled_by(lab.id);
            dragging |= r.dragged();
            changed |= r.changed() || r.drag_stopped();
        });
        ui.horizontal(|ui| {
            let lab = ui.label(labels::LIGHT_REACH);
            let r = ui
                .add(egui::Slider::new(&mut reach, 1..=MAX_SKIN_LIGHT_REACH).suffix(" px"))
                .labelled_by(lab.id);
            dragging |= r.dragged();
            changed |= r.changed() || r.drag_stopped();
        });
        if dragging {
            data.light = Some((intensity, reach));
        } else {
            data.light = None;
            if changed && (intensity, reach) != (l.intensity, l.reach) {
                out.push(SkinAction::SetLight(Some(SkinLight { intensity, reach })));
            }
        }
    }
    if doc.is_classic() {
        ui.add_space(6.0);
        ui.weak("A classic skin (20 × 18 pixels a frame). Edit it as it is, or start a double-resolution one from it:");
        if ui.button(labels::MAKE_DOUBLE).on_hover_text("Scale2x: every frame doubled, to refine").clicked() {
            out.push(SkinAction::MakeDouble);
        }
    }
}
