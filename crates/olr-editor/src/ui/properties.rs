//! The selection's properties, each committed once (`number`, `text`).

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{Dither, ElementKind, ElementMaterial, LevelObject, MAX_PIXEL_SIZE, MAX_SIDE};

use super::{EditorData, labels, number, text};
use crate::state::{EditorAction, EditorState, Selection};

const MATERIALS: [(ElementMaterial, &str); 3] = [
    (ElementMaterial::Rock, "Rock"),
    (ElementMaterial::Dirt, "Dirt"),
    (ElementMaterial::Background, "Background"),
];

const DITHERS: [(Dither, &str); 3] =
    [(Dither::None, "None"), (Dither::Ordered, "Ordered"), (Dither::Diffusion, "Diffusion")];

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    match state.selection() {
        Selection::Object(i) => {
            let Some(LevelObject::Spawn { at, .. }) = doc.level().objects.get(*i) else { return };
            ui.heading(format!("Spawn {}", i + 1));
            ui.label(format!("at ({}, {})", at.0, at.1));
            if ui.button("Delete").on_hover_text("Delete").clicked() {
                out.push(EditorAction::Remove);
            }
        }
        Selection::Element(id) => {
            let Some(index) = doc.index_of(id) else { return };
            let e = &doc.items()[index].element;
            let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::Set(id.clone(), p));
            ui.heading(match e.kind {
                ElementKind::Image { .. } => "Image",
                ElementKind::Drawn { .. } => "Drawn layer",
                ElementKind::Raster => "Raster",
            });
            ui.weak(&e.id);
            if let Some(name) = text(ui, data, &format!("{id}/name"), labels::NAME, &e.name, false) {
                set(out, Property::Name(name));
            }
            let mut visible = e.visible;
            if ui.checkbox(&mut visible, labels::VISIBLE).changed() {
                set(out, Property::Visible(visible));
            }
            let mut locked = e.locked;
            if ui.checkbox(&mut locked, labels::LOCKED).changed() {
                set(out, Property::Locked(locked));
            }
            if matches!(e.kind, ElementKind::Raster)
                && ui
                    .add_enabled(!e.locked, egui::Button::new(labels::SPLIT))
                    .on_hover_text("background, dirt and rock as drawn layers, to paint and erase")
                    .clicked()
            {
                out.push(EditorAction::Split(id.clone()));
            }
            let material = match &e.kind {
                ElementKind::Image { material, .. } | ElementKind::Drawn { material, .. } => Some(*material),
                ElementKind::Raster => None,
            };
            if let Some(material) = material {
                let name = MATERIALS.iter().find(|(m, _)| *m == material).map_or("", |(_, n)| n);
                egui::ComboBox::from_label(labels::MATERIAL).selected_text(name).show_ui(ui, |ui| {
                    for (m, n) in MATERIALS {
                        if ui.selectable_label(m == material, n).clicked() && m != material {
                            set(out, Property::Material(m));
                        }
                    }
                });
            }
            let ElementKind::Image { at, size, pixel_size, colours, dither, alpha_threshold, .. } = &e.kind
            else {
                return;
            };
            ui.separator();
            let coord = i64::from(i32::MIN)..=i64::from(i32::MAX);
            let x = number(ui, data, &format!("{id}/x"), "X", at.0.into(), coord.clone());
            let y = number(ui, data, &format!("{id}/y"), "Y", at.1.into(), coord);
            if x.is_some() || y.is_some() {
                let to = (x.map_or(at.0, |v| v as i32), y.map_or(at.1, |v| v as i32));
                set(out, Property::At(to));
            }
            let side = 1..=i64::from(MAX_SIDE);
            let w = number(ui, data, &format!("{id}/w"), "Width", size.0.into(), side.clone());
            let h = number(ui, data, &format!("{id}/h"), "Height", size.1.into(), side);
            if w.is_some() || h.is_some() {
                let to = (w.map_or(size.0, |v| v as u32), h.map_or(size.1, |v| v as u32));
                set(out, Property::Size(to));
            }
            if let Some(v) = number(
                ui,
                data,
                &format!("{id}/pixel"),
                labels::PIXEL_SIZE,
                (*pixel_size).into(),
                1..=MAX_PIXEL_SIZE.into(),
            ) {
                set(out, Property::PixelSize(v as u32));
            }
            let mut reduce = colours.is_some();
            if ui.checkbox(&mut reduce, "Reduce colours").changed() {
                set(out, Property::Colours(reduce.then_some(16)));
            }
            if let Some(n) = colours
                && let Some(v) =
                    number(ui, data, &format!("{id}/colours"), labels::COLOURS, (*n).into(), 2..=256)
            {
                set(out, Property::Colours(Some(v as u16)));
            }
            let name = DITHERS.iter().find(|(d, _)| d == dither).map_or("", |(_, n)| n);
            egui::ComboBox::from_label(labels::DITHER).selected_text(name).show_ui(ui, |ui| {
                for (d, n) in DITHERS {
                    if ui.selectable_label(d == *dither, n).clicked() && d != *dither {
                        set(out, Property::Dither(d));
                    }
                }
            });
            if let Some(v) = number(
                ui,
                data,
                &format!("{id}/threshold"),
                labels::THRESHOLD,
                (*alpha_threshold).into(),
                0..=255,
            ) {
                set(out, Property::Threshold(v as u8));
            }
        }
        Selection::None => {}
    }
}
