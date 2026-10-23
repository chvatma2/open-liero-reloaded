//! The right panel's tabs (issue #8): Elements (the element list, top
//! first: drag a row to reorder, eye and lock toggles, add a layer; issue
//! #61: the folder tree, a row dragged beside another, into a folder or
//! out of one, New folder),
//! Assets (the asset library, issue #9: thumbnails grouped Built-in and
//! Yours; a click picks one for the next press on the canvas, a drag drops
//! it there) and Spawns (the level's objects). The tab follows the
//! selection (`EditorState::tab`).

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{ElementKind, Folder, LevelObject};

use super::{EditorData, labels};
use crate::catalog::{AssetEntry, AssetKey};
use crate::folder_ops::{FolderField, RowDrop};
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

/// One folder level's indent in the list, points.
const INDENT: f32 = 14.0;

/// A folder row's share of its height, at its top (and at its bottom when
/// folded), that drops beside it rather than into it.
const EDGE: f32 = 0.3;

/// The element list (issue #61: the tree): rows top first, a folder's
/// members under it, indented one step per level; Add layer and New folder.
fn elements(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    let rows = doc.rows();
    let folders = doc.folders();
    let carried = egui::DragAndDrop::payload::<String>(ui.ctx()).map(|p| (*p).clone());
    let accent = ui.visuals().selection.stroke;
    egui::ScrollArea::vertical()
        .id_salt("olr-editor-element-list")
        .max_height(ui.available_height() * LIST_SHARE)
        .show(ui, |ui| {
            for (k, row) in rows.iter().enumerate() {
                let r = ui.horizontal(|ui| {
                    ui.add_space(INDENT * row.depth as f32);
                    match folders.iter().find(|f| row.folder && f.id == row.id) {
                        Some(f) => folder_row(ui, state, f, out),
                        None => element_row(ui, state, &row.id, out),
                    }
                });
                // Where a carried row would land (issue #61): beside this
                // one, taking its place (below it when it comes from above),
                // or into this folder.
                let Some(dragged) = carried.as_ref().filter(|d| **d != row.id) else { continue };
                let rect = r.response.rect;
                let Some(p) = ui.input(|i| i.pointer.interact_pos()).filter(|p| rect.contains(*p)) else {
                    continue;
                };
                let from_above = rows.iter().position(|r| r.id == *dragged).is_some_and(|d| d < k);
                let t = (p.y - rect.top()) / rect.height().max(1.0);
                let folded = folders.iter().any(|f| f.id == row.id && f.collapsed);
                let at = if !row.folder {
                    if from_above { RowDrop::Below(row.id.clone()) } else { RowDrop::Above(row.id.clone()) }
                } else if t < EDGE {
                    RowDrop::Above(row.id.clone())
                } else if folded && t > 1.0 - EDGE {
                    RowDrop::Below(row.id.clone())
                } else {
                    RowDrop::Into(row.id.clone())
                };
                let painter = ui.painter();
                match &at {
                    RowDrop::Above(_) => painter.hline(rect.x_range(), rect.top(), accent),
                    RowDrop::Below(_) => painter.hline(rect.x_range(), rect.bottom(), accent),
                    RowDrop::Into(_) => painter.rect_stroke(rect, 2.0, accent, egui::StrokeKind::Inside),
                };
                if r.response.dnd_release_payload::<String>().is_some() {
                    // A level without folders reorders as it always has.
                    let flat = folders.is_empty() && !row.folder;
                    out.push(match (flat, doc.index_of(&row.id)) {
                        (true, Some(to)) => EditorAction::MoveElement { id: dragged.clone(), to },
                        _ => EditorAction::DropRow { id: dragged.clone(), at },
                    });
                }
            }
        });
    ui.horizontal_wrapped(|ui| {
        if ui.button(labels::ADD_LAYER).on_hover_text("an empty layer to paint on").clicked() {
            out.push(EditorAction::AddLayer);
        }
        let any = !state.selection().ids().is_empty();
        if ui
            .add_enabled(any, egui::Button::new(labels::NEW_FOLDER))
            .on_hover_text(format!("the selection into a new folder ({})", labels::NEW_FOLDER_KEYS))
            .clicked()
        {
            out.push(EditorAction::GroupSelection);
        }
    });
}

/// The selecting click on a row's name: Ctrl toggles it in a group, Shift
/// selects a range (plan 14e R8; issue #61: of the rows shown).
fn row_click(ui: &Ui, r: &egui::Response, id: &str, single: Selection, out: &mut Vec<EditorAction>) {
    r.dnd_set_drag_payload(id.to_string());
    if r.clicked() {
        let m = ui.input(|i| i.modifiers);
        out.push(if m.command {
            EditorAction::SelectToggle(id.to_string())
        } else if m.shift {
            EditorAction::SelectRange(id.to_string())
        } else {
            EditorAction::Select(single)
        });
    }
}

/// An eye or lock toggle, dimmed when a folder holding it hides or locks it.
fn toggle(ui: &mut Ui, on: &mut bool, icon: &str, dimmed: bool, hover: &str) -> egui::Response {
    let text = egui::RichText::new(icon);
    ui.toggle_value(on, if dimmed { text.weak() } else { text }).on_hover_text(hover)
}

/// An element's row: eye, lock, kind, the distant mark, the name.
fn element_row(ui: &mut Ui, state: &EditorState, id: &str, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    let Some(index) = doc.index_of(id) else { return };
    let e = &doc.items()[index].element;
    let mut visible = e.visible;
    if toggle(ui, &mut visible, "👁", e.visible && !doc.shown_at(index), labels::VISIBLE).clicked() {
        out.push(EditorAction::Set(e.id.clone(), Property::Visible(visible)));
    }
    let mut locked = e.locked;
    if toggle(ui, &mut locked, "🔒", !e.locked && doc.locked_at(index), labels::LOCKED).clicked() {
        out.push(EditorAction::Set(e.id.clone(), Property::Locked(locked)));
    }
    let kind = match e.kind {
        ElementKind::Image { .. } => "🖼",
        ElementKind::Drawn { .. } => "✏",
        ElementKind::Raster => "▦",
    };
    ui.label(kind);
    if let Some(depth) = e.distant {
        ui.label("🌄").on_hover_text(format!("{}: depth {depth} %", labels::DISTANT));
    }
    let name = if e.name.is_empty() { e.id.as_str() } else { e.name.as_str() };
    let selected = state.selection().has(&e.id);
    // The name selects on a click and reorders on a drag.
    let r = ui.add(egui::Button::selectable(selected, name).sense(egui::Sense::click_and_drag()));
    row_click(ui, &r, &e.id, Selection::Element(e.id.clone()), out);
}

/// A folder's row (issue #61): fold arrow, eye, lock, the folder mark and
/// its name.
fn folder_row(ui: &mut Ui, state: &EditorState, f: &Folder, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    let set = |out: &mut Vec<EditorAction>, field| out.push(EditorAction::SetFolder(f.id.clone(), field));
    let name = if f.name.is_empty() { f.id.as_str() } else { f.name.as_str() };
    let arrow = ui.small_button(if f.collapsed { "▶" } else { "▼" });
    super::relabel(&arrow, egui::WidgetType::Button, None, &labels::fold(name));
    if arrow.on_hover_text(if f.collapsed { "unfold" } else { "fold" }).clicked() {
        set(out, FolderField::Collapsed(!f.collapsed));
    }
    // The folders holding it hide or lock it too.
    let all = doc.folders();
    let outer: Vec<&Folder> =
        doc.folders_holding(&f.id).iter().filter_map(|h| all.iter().find(|o| o.id == *h)).collect();
    let mut visible = f.visible;
    let eye = toggle(ui, &mut visible, "👁", f.visible && outer.iter().any(|o| !o.visible), labels::VISIBLE);
    super::relabel(&eye, egui::WidgetType::Button, Some(f.visible), &labels::folder_visible(name));
    if eye.clicked() {
        set(out, FolderField::Visible(visible));
    }
    let mut locked = f.locked;
    let lock = toggle(ui, &mut locked, "🔒", !f.locked && outer.iter().any(|o| o.locked), labels::LOCKED);
    super::relabel(&lock, egui::WidgetType::Button, Some(f.locked), &labels::folder_locked(name));
    if lock.clicked() {
        set(out, FolderField::Locked(locked));
    }
    ui.label("📁");
    let selected = state.selection().has(&f.id);
    let r = ui.add(egui::Button::selectable(selected, name).sense(egui::Sense::click_and_drag()));
    row_click(ui, &r, &f.id, Selection::Folder(f.id.clone()), out);
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
