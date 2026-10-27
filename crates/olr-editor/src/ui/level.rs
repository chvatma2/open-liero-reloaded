//! The level's settings, shown when nothing is selected: name, notes,
//! theme, art scale, and a resize with an offset.

use egui::Ui;
use olr_data::image::Rgba;
use olr_data::level::Material;
use olr_format::levels::{MAX_ART_SCALE, MAX_SIDE};

use super::look::LookScope;
use super::{EditorData, labels, number, text};
use crate::history::LevelMeta;
use crate::state::{EditorAction, EditorState};

const NEW_AREA: [(Material, &str); 3] =
    [(Material::Rock, "Rock"), (Material::Dirt, "Dirt"), (Material::Background, "Background")];

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let l = doc.level();
    ui.heading("Level");
    let meta = doc.meta();
    if let Some(name) = text(ui, data, "level/name", "Level name", &meta.name, false) {
        out.push(EditorAction::Meta(LevelMeta { name, ..meta.clone() }));
    }
    if let Some(notes) = text(ui, data, "level/notes", "Notes", &meta.notes, true) {
        out.push(EditorAction::Meta(LevelMeta { notes, ..meta.clone() }));
    }
    let themes = &state.catalog().themes;
    let current = themes.iter().find(|t| t.id == meta.theme).map_or(meta.theme.as_str(), |t| t.name.as_str());
    egui::ComboBox::from_label("Theme").selected_text(current).show_ui(ui, |ui| {
        for t in themes {
            if ui.selectable_label(t.id == meta.theme, &t.name).clicked() && t.id != meta.theme {
                out.push(EditorAction::Meta(LevelMeta { theme: t.id.clone(), ..meta.clone() }));
            }
        }
    });
    if let Some(to) =
        number(ui, data, "level/art-scale", "Art scale", l.art_scale.into(), 1..=MAX_ART_SCALE.into())
    {
        out.push(EditorAction::ArtScale(to as u32));
    }
    ui.separator();
    ui.label(format!("Size: {} × {}", l.size.0, l.size.1));
    let mut fields = match data.resize {
        Some((fields, from)) if from == l.size => fields,
        _ => [l.size.0.into(), l.size.1.into(), 0, 0],
    };
    let side = 1..=i64::from(MAX_SIDE);
    let offset = -i64::from(MAX_SIDE)..=i64::from(MAX_SIDE);
    let names = [
        ("level/w", "New width"),
        ("level/h", "New height"),
        ("level/ox", "Offset X"),
        ("level/oy", "Offset Y"),
    ];
    for (i, (key, label)) in names.into_iter().enumerate() {
        let range = if i < 2 { side.clone() } else { offset.clone() };
        if let Some(v) = number(ui, data, key, label, fields[i], range) {
            fields[i] = v;
        }
    }
    data.resize = Some((fields, l.size));
    let (material, name) = NEW_AREA[data.resize_material.min(2)];
    egui::ComboBox::from_label("New area").selected_text(name).show_ui(ui, |ui| {
        for (i, (_, n)) in NEW_AREA.iter().enumerate() {
            ui.selectable_value(&mut data.resize_material, i, *n);
        }
    });
    if ui.button("Resize").clicked() {
        out.push(EditorAction::Resize {
            size: (fields[0] as u32, fields[1] as u32),
            offset: (fields[2] as i32, fields[3] as i32),
            material,
            colour: Rgba::BLACK,
        });
        data.resize = None;
    }
    // Issue #81: the glow groups and materials are the ground's (Edit in
    // level).
    super::look::level_look(ui, state, data, LookScope::Level, out);
}

/// The asset tab's asset, shown when nothing is selected (issue #77): its
/// name, its size (Width and Height resize it from its top-left, one
/// step), its art scale (shown, not changed), its Lighting and Materials.
pub(super) fn asset(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let l = doc.level();
    if doc.is_ground() {
        // Issue #81: the level's ground, its size and name the level's.
        ui.heading(state.asset_title());
        ui.label(format!("{}: {} × {}", labels::ASSET_SIZE, l.size.0, l.size.1));
        ui.label(format!("{}: {}", labels::ART_SCALE, l.art_scale));
        super::look::level_look(ui, state, data, LookScope::Ground, out);
        super::look::level_materials(ui, state, out);
        return;
    }
    ui.heading("Asset");
    let meta = doc.meta();
    if let Some(name) = text(ui, data, "asset/name", labels::NAME, &meta.name, false) {
        out.push(EditorAction::Meta(LevelMeta { name, ..meta.clone() }));
    }
    ui.separator();
    ui.strong(labels::ASSET_SIZE);
    let side = 1..=i64::from(MAX_SIDE);
    let w = number(ui, data, "asset/w", labels::WIDTH, l.size.0.into(), side.clone());
    let h = number(ui, data, "asset/h", labels::HEIGHT, l.size.1.into(), side);
    if w.is_some() || h.is_some() {
        let size = (w.map_or(l.size.0, |v| v as u32), h.map_or(l.size.1, |v| v as u32));
        out.push(EditorAction::Resize {
            size,
            offset: (0, 0),
            material: Material::Background,
            colour: Rgba::BLACK,
        });
    }
    ui.label(format!("{}: {}", labels::ART_SCALE, l.art_scale));
    super::look::level_look(ui, state, data, LookScope::Asset, out);
    super::look::level_materials(ui, state, out);
}
