//! The right panel: the 21 frames (7 aim columns by 3 walk rows, each a
//! button named by its frame, `state::frame_name`), the live preview (the
//! worm walking and aiming, turning both ways, in a player colour, at
//! game size and enlarged, on a light background and a dark one where
//! the glow and the skin light show; `olr_edit::skin::preview_frame`),
//! and the skin's settings (Name, Skin light, Make double resolution).
//! It fits the panel's width (issue #55): the frames shrink, the preview
//! wraps and the help lines wrap.

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui, vec2};
use olr_data::image::Rgba;
use olr_data::skin::SkinLight;
use olr_edit::skin::{PREVIEW_CELL, preview_frame};
use olr_look::engine::{MAX_SKIN_LIGHT_INTENSITY, MAX_SKIN_LIGHT_REACH};

use super::{SkinEditorData, beside, checks, colour32, frame_texture, labels, relabel, text_width, texture};
use crate::doc::{AIM_FRAMES, WALK_FRAMES};
use crate::state::{DEFAULT_LIGHT, SkinAction, SkinEditorState, frame_name};

/// A frame's button at most: 6 points a world pixel (a worm is 10 × 9).
const THUMB: egui::Vec2 = egui::Vec2::new(60.0, 54.0);
/// A frame's button's narrowest (points; the height keeps 10 : 9).
const MIN_THUMB: f32 = 20.0;
/// The frames grid's spacing (points).
const GAP: f32 = 4.0;
/// The room (points) kept for rounding: the grid's and the panel's.
const SLACK: f32 = 1.0;
/// The preview's points per world pixel: game size and enlarged (at
/// most: it shrinks to game size in a narrow panel).
const GAME_SIZE: f32 = 3.0;
const ENLARGED: f32 = 7.0;
/// What the aim columns show (the game's seven aim ranges), above the grid.
const AIMS_HINT: &str = "Aim 1 ↑ up, 5 → level, 7 ↓ down";
/// Each aim column's hint (its number's hover text).
const AIMS: [&str; AIM_FRAMES] = ["↑ up", "", "", "", "→ level", "", "↓ down"];
/// The Name field's and a light slider's narrowest beside their labels.
const FIELD_MIN: f32 = 120.0;
const SLIDER_MIN: f32 = 60.0;

/// The walk rows' labels' column (points).
fn walk_width(ui: &Ui) -> f32 {
    (1..=WALK_FRAMES)
        .map(|w| text_width(ui, &format!("Walk {w}"), egui::TextStyle::Body))
        .fold(0.0, f32::max)
        .ceil()
}

/// The frames grid's width at full size (points): what the panel needs
/// inside (its rounding to whole points in).
pub(super) fn full_width(ui: &Ui) -> f32 {
    walk_width(ui) + AIM_FRAMES as f32 * (THUMB.x + GAP) + 2.0 * SLACK
}

/// A frame's button in `width` points: as many as are left beside the
/// walk column, within `MIN_THUMB` and `THUMB`, keeping 10 : 9.
fn thumb(ui: &Ui, width: f32) -> egui::Vec2 {
    let left = width - walk_width(ui) - AIM_FRAMES as f32 * GAP - SLACK;
    let w = (left / AIM_FRAMES as f32).floor().clamp(MIN_THUMB, THUMB.x);
    vec2(w, (w * THUMB.y / THUMB.x).round())
}

pub(super) fn show(
    ui: &mut Ui,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    out: &mut Vec<SkinAction>,
    fields: &mut Vec<SkinAction>,
) {
    ui.strong("Frames");
    ui.weak(AIMS_HINT);
    let size = thumb(ui, ui.available_width());
    let grid = egui::Grid::new("olr-skin-editor-frames").spacing([GAP, GAP]).min_col_width(0.0);
    grid.show(ui, |ui| {
        ui.label("");
        // The aims' numbers: never wider than a frame.
        for (a, hint) in AIMS.iter().enumerate() {
            let text = format!("Aim {}{}{hint}", a + 1, if hint.is_empty() { "" } else { ": " });
            let height = ui.spacing().interact_size.y;
            ui.add_sized(vec2(size.x, height), egui::Label::new((a + 1).to_string())).on_hover_text(text);
        }
        ui.end_row();
        for w in 0..WALK_FRAMES {
            ui.label(format!("Walk {}", w + 1));
            for a in 0..AIM_FRAMES {
                let f = w * AIM_FRAMES + a;
                let (rect, r) = ui.allocate_exact_size(size, Sense::click());
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
    ui.horizontal_wrapped(|ui| {
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
    // One row while the enlarged pair fits beside the game-size one at
    // game size or more, else two; the enlarged pair shrinks first, by
    // whole points a pixel.
    let gap = ui.spacing().item_spacing.x;
    let width = ui.available_width();
    let enlarged = |room: f32| (room / (2.0 * cell.x)).floor().min(ENLARGED);
    let one_row = enlarged(width - 2.0 * cell.x * GAME_SIZE - 3.0 * gap);
    let rows: &[&[f32]] = if one_row >= GAME_SIZE {
        &[&[GAME_SIZE, one_row]]
    } else {
        &[&[GAME_SIZE], &[enlarged(width - gap).max(GAME_SIZE)]]
    };
    for row in rows {
        ui.horizontal_wrapped(|ui| {
            for &scale in *row {
                for t in [light, dark] {
                    ui.add(egui::Image::new((t.id(), cell * scale)));
                }
            }
        });
    }
    ui.weak("Light and dark, at game size and enlarged: the dark shows the glow and the skin light.");
}

/// Name, Skin light (its intensity and reach), Make double resolution.
fn settings(ui: &mut Ui, state: &SkinEditorState, data: &mut SkinEditorData, out: &mut Vec<SkinAction>) {
    ui.strong("Skin");
    let doc = state.doc();
    beside(ui, labels::NAME, FIELD_MIN, |ui, l, room| {
        let mut text = data.name.clone().unwrap_or_else(|| doc.name().to_string());
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(room.min(240.0))).labelled_by(l);
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
        let value = value_width(ui, &MAX_SKIN_LIGHT_INTENSITY.to_string());
        slider(ui, labels::LIGHT_INTENSITY, value, |ui, lab| {
            let r = ui.add(egui::Slider::new(&mut intensity, 0..=MAX_SKIN_LIGHT_INTENSITY)).labelled_by(lab);
            dragging |= r.dragged();
            changed |= r.changed() || r.drag_stopped();
        });
        let value = value_width(ui, &format!("{MAX_SKIN_LIGHT_REACH} px"));
        slider(ui, labels::LIGHT_REACH, value, |ui, lab| {
            let r = ui
                .add(egui::Slider::new(&mut reach, 1..=MAX_SKIN_LIGHT_REACH).suffix(" px"))
                .labelled_by(lab);
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

/// A slider's value box's width (points) for its widest `text`.
fn value_width(ui: &Ui, text: &str) -> f32 {
    let padded = text_width(ui, text, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x;
    padded.max(ui.spacing().interact_size.x).ceil() + 2.0
}

/// A slider `add` adds labelled `label`: its track as long as egui's at
/// most and as the room left beside the label and its `value` box,
/// below the label when that is less than `SLIDER_MIN`.
fn slider(ui: &mut Ui, label: &str, value: f32, add: impl FnOnce(&mut Ui, egui::Id)) {
    let needs = SLIDER_MIN + ui.spacing().item_spacing.x + value;
    beside(ui, label, needs, |ui, l, room| {
        let track = (room - ui.spacing().item_spacing.x - value).max(SLIDER_MIN.min(room / 2.0));
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = ui.spacing().slider_width.min(track);
            add(ui, l);
        });
    });
}
