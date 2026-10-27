//! The asset tab's previews (issue #78), as the skin editor's preview: the
//! asset light (`labels::PREVIEW_LIGHT`, full bright) and dark
//! (`labels::PREVIEW_DARK`, under `olr_edit::asset::preview_look`, its
//! glowing pixels lit), of the shown state fully posed (issue #79), each
//! at game size and enlarged (a whole number of
//! points a pixel, fitting the panel). They are drawn on the CPU
//! (`olr_edit::asset::preview`) when the document's bake changed and no
//! stroke or drag is in progress, never every frame, and kept as egui
//! textures by generation (`EditorData::previews`).

use egui::Ui;

use super::{EditorData, labels, relabel};
use crate::state::EditorState;

/// The preview's points per world pixel at game size (the skin editor's:
/// the game's world at 720 points high), shrunk to the panel's width.
const GAME_SIZE: f32 = 3.0;
/// The enlarged preview's largest zoom (points a pixel) and height
/// (points).
const MAX_ZOOM: f32 = 12.0;
const MAX_HEIGHT: f32 = 480.0;

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData) {
    let Some(doc) = state.active_document() else { return };
    ui.strong("Preview");
    let key = (state.shown_generation(), doc.bake_generation());
    if data.previews.as_ref().is_none_or(|(k, _)| *k != key) && !state.dragging() {
        let asset = doc.to_asset();
        let texture = |dark: bool, name: &str| {
            // Issue #79: the shown state, fully posed.
            let img = olr_edit::asset::preview_state(&asset, doc.shown_state().unwrap_or(asset.start), dark);
            let size = [img.width() as usize, img.height() as usize];
            ui.ctx().load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                egui::TextureOptions::NEAREST,
            )
        };
        let light = texture(false, "olr-editor-preview-light");
        let dark = texture(true, "olr-editor-preview-dark");
        data.previews = Some((key, [light, dark]));
    }
    let Some((_, textures)) = &data.previews else { return };
    let [iw, ih] = textures[0].size().map(|v| v.max(1) as f32);
    let world = doc.level().size.0.max(1) as f32;
    let width = ui.available_width();
    // Game size, shrunk to the panel; enlarged, by whole points a pixel,
    // shown when larger.
    let game = (GAME_SIZE * world / iw).min(width / iw);
    let zoom = (width / iw).min(MAX_HEIGHT / ih).floor().min(MAX_ZOOM);
    let mut scales = vec![game];
    if zoom > game {
        scales.push(zoom);
    }
    for scale in scales {
        ui.horizontal_wrapped(|ui| {
            for (t, label) in textures.iter().zip([labels::PREVIEW_LIGHT, labels::PREVIEW_DARK]) {
                let r = ui.add(egui::Image::new((t.id(), egui::vec2(iw, ih) * scale)));
                relabel(&r, egui::WidgetType::Image, None, label);
                r.on_hover_text(label);
            }
        });
    }
    ui.weak("Light and dark, at game size and enlarged: the dark shows the glow.");
}
