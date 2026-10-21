//! The right panel's tabs (issue #8): Elements (the element list, top
//! first: drag a row to reorder, eye and lock toggles, add a layer),
//! Assets (the asset library, issue #9: thumbnails grouped Built-in and
//! Yours; a click picks one for the next press on the canvas, a drag drops
//! it there) and Spawns (the level's objects). The tab follows the
//! selection (`EditorState::tab`).

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{ElementKind, LevelObject};

use super::{EditorData, labels};
use crate::catalog::{AssetEntry, AssetKey};
use crate::state::{EditorAction, EditorState, PanelTab, Selection};

/// The lists' share of the panel's height: the properties take the rest.
const LIST_SHARE: f32 = 0.4;

/// A thumbnail's size in the Assets tab (points): half `THUMB_SIZE`.
const THUMB: egui::Vec2 = egui::vec2(80.0, 48.0);

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    ui.horizontal(|ui| {
        for (tab, label) in [
            (PanelTab::Elements, labels::ELEMENTS),
            (PanelTab::Assets, labels::ASSETS),
            (PanelTab::Spawns, labels::SPAWNS),
        ] {
            if ui.selectable_label(state.tab() == tab, label).clicked() && state.tab() != tab {
                out.push(EditorAction::Tab(tab));
            }
        }
    });
    ui.separator();
    match state.tab() {
        PanelTab::Elements => elements(ui, state, out),
        PanelTab::Assets => assets(ui, state, data, out),
        PanelTab::Spawns => spawns(ui, state, out),
    }
}

fn elements(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-element-list")
        .max_height(ui.available_height() * LIST_SHARE)
        .show(ui, |ui| {
            for (index, item) in doc.items().iter().enumerate().rev() {
                let e = &item.element;
                let row = ui.horizontal(|ui| {
                    let mut visible = e.visible;
                    if ui.toggle_value(&mut visible, "👁").on_hover_text(labels::VISIBLE).clicked() {
                        out.push(EditorAction::Set(e.id.clone(), Property::Visible(visible)));
                    }
                    let mut locked = e.locked;
                    if ui.toggle_value(&mut locked, "🔒").on_hover_text(labels::LOCKED).clicked() {
                        out.push(EditorAction::Set(e.id.clone(), Property::Locked(locked)));
                    }
                    let kind = match e.kind {
                        ElementKind::Image { .. } => "🖼",
                        ElementKind::Drawn { .. } => "✏",
                        ElementKind::Raster => "▦",
                    };
                    ui.label(kind);
                    let name = if e.name.is_empty() { e.id.as_str() } else { e.name.as_str() };
                    let selected = state.selection().has(&e.id);
                    // The name selects on a click and reorders on a drag.
                    let r =
                        ui.add(egui::Button::selectable(selected, name).sense(egui::Sense::click_and_drag()));
                    r.dnd_set_drag_payload(e.id.clone());
                    // Ctrl toggles it in a group, Shift selects a range (plan 14e R8).
                    if r.clicked() {
                        let m = ui.input(|i| i.modifiers);
                        out.push(if m.command {
                            EditorAction::SelectToggle(e.id.clone())
                        } else if m.shift {
                            EditorAction::SelectRange(e.id.clone())
                        } else {
                            EditorAction::Select(Selection::Element(e.id.clone()))
                        });
                    }
                });
                if let Some(dragged) = row.response.dnd_release_payload::<String>()
                    && *dragged != e.id
                {
                    out.push(EditorAction::MoveElement { id: (*dragged).clone(), to: index });
                }
            }
        });
    if ui.button(labels::ADD_LAYER).on_hover_text("an empty layer to paint on").clicked() {
        out.push(EditorAction::AddLayer);
    }
}

fn assets(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    // Thumbnails of an older library go.
    if data.asset_generation != state.assets_generation() {
        data.asset_thumbs.clear();
        data.asset_generation = state.assets_generation();
    }
    let all = &state.catalog().assets;
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-asset-list")
        .max_height(ui.available_height() * LIST_SHARE)
        .show(ui, |ui| {
            for (builtin, title) in [(true, labels::BUILT_IN), (false, labels::YOURS)] {
                ui.strong(title);
                let group: Vec<&AssetEntry> = all.iter().filter(|a| a.key.builtin == builtin).collect();
                if group.is_empty() {
                    ui.weak(if builtin {
                        "None found."
                    } else {
                        "None yet: select elements or pixels and use Save as asset…, or import one."
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    for a in group {
                        card(ui, state, data, a, out);
                    }
                });
            }
        });
    ui.horizontal(|ui| {
        if ui.button(labels::IMPORT_ASSET).on_hover_text("add an asset folder someone shared").clicked() {
            out.push(EditorAction::Browse(crate::browser::Purpose::ImportAsset));
        }
        if let Some(key) = state.picked_asset().filter(|k| !k.builtin)
            && ui.button(labels::REMOVE_ASSET).clicked()
        {
            out.push(EditorAction::RemoveAsset(key.id.clone()));
        }
    });
}

/// An asset's thumbnail (a click picks it, a drag carries it to the
/// canvas), its name and its tag.
fn card(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    a: &AssetEntry,
    out: &mut Vec<EditorAction>,
) {
    let picked = state.picked_asset() == Some(&a.key);
    ui.vertical(|ui| {
        ui.set_width(THUMB.x + 8.0);
        let texture = thumb(ui, data, a);
        let button = match &texture {
            Some(t) => egui::Button::image(egui::Image::new(t).fit_to_exact_size(THUMB)),
            None => egui::Button::new("?").min_size(THUMB),
        };
        let r = ui.add(button.selected(picked).sense(egui::Sense::click_and_drag()));
        super::relabel(&r, egui::WidgetType::Button, Some(picked), &a.name);
        r.dnd_set_drag_payload(a.key.clone());
        if r.clicked() {
            let again = picked && state.placing();
            out.push(EditorAction::PickAsset((!again).then(|| a.key.clone())));
        }
        ui.add(egui::Label::new(&a.name).truncate());
        ui.add(egui::Label::new(egui::RichText::new(a.tag()).small().weak()).truncate());
    });
}

/// `a`'s thumbnail texture, made when first drawn.
fn thumb(ui: &Ui, data: &mut EditorData, a: &AssetEntry) -> Option<egui::TextureHandle> {
    let img = a.thumb.as_ref()?;
    let key: &AssetKey = &a.key;
    Some(
        data.asset_thumbs
            .entry(key.clone())
            .or_insert_with(|| {
                let (w, h) = (img.width(), img.height());
                let raw: Vec<u8> = (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .flat_map(|(x, y)| {
                        let p = img.get(x, y);
                        [p.r, p.g, p.b, p.a]
                    })
                    .collect();
                ui.ctx().load_texture(
                    format!("olr-editor-asset-{}-{}", key.builtin, key.id),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [img.width() as usize, img.height() as usize],
                        &raw,
                    ),
                    egui::TextureOptions::NEAREST,
                )
            })
            .clone(),
    )
}

fn spawns(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    if doc.level().objects.is_empty() {
        ui.weak("No spawns yet: place them with the Spawn tool (P).");
    }
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-spawn-list")
        .max_height(ui.available_height() * LIST_SHARE)
        .show(ui, |ui| spawn_rows(ui, state, out));
}

fn spawn_rows(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    for (i, o) in doc.level().objects.iter().enumerate() {
        let at = o.at();
        let kind = match o {
            LevelObject::Spawn { .. } => "Spawn",
        };
        let selected = *state.selection() == Selection::Object(i);
        if ui.selectable_label(selected, format!("{kind} {} at ({}, {})", i + 1, at.0, at.1)).clicked() {
            out.push(EditorAction::Select(Selection::Object(i)));
        }
    }
}
