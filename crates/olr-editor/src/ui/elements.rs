//! The level tab's copy panel's tabs (issue #8; issue #83 moved the asset
//! library to the left panel, `library.rs`): Copies (issue #82: every
//! placed copy, top first) and Spawns (the level's objects), the tab
//! following the selection (`EditorState::tab`); and the asset tab's
//! layer tree (issue #61: a row dragged beside another, into a folder or
//! out of one, eye and lock toggles, Add layer, New folder).

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{ElementKind, Folder, LevelObject};

use super::labels;
use crate::folder_ops::{FolderField, RowDrop};
use crate::state::{EditorAction, EditorState, PanelTab, Selection};

/// The level tab's copy panel's tabs and list: Copies or Spawns, in the
/// panel's own scroll area (none of their own).
pub(super) fn show(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    ui.horizontal_wrapped(|ui| {
        for (tab, label) in [(PanelTab::Elements, labels::COPIES), (PanelTab::Spawns, labels::SPAWNS)] {
            if ui.selectable_label(state.tab() == tab, label).clicked() && state.tab() != tab {
                out.push(EditorAction::Tab(tab));
            }
        }
    });
    ui.separator();
    match state.tab() {
        PanelTab::Elements => copies(ui, state, out),
        PanelTab::Spawns => spawns(ui, state, out),
    }
}

/// A Copies list row carried to another (issue #82): the copy's id.
#[derive(Clone, Debug)]
struct CarriedCopy(String);

/// The level tab's Copies (issue #82): every copy, top first, its name and
/// id, a lock toggle (the ground's always locked) and its mark (🔗 a
/// library copy, "own" the level's own asset); a click selects (Ctrl
/// toggles), a double click opens its asset, a row dragged onto another
/// takes its place (one step).
fn copies(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    let copies = doc.copies();
    let carried = egui::DragAndDrop::payload::<CarriedCopy>(ui.ctx()).map(|p| p.0.clone());
    let accent = ui.visuals().selection.stroke;
    let selected = state.selection().copies();
    for (k, c) in copies.iter().enumerate().rev() {
        let ground = doc.is_ground_copy(&c.id);
        let row = ui.horizontal(|ui| {
            let mut locked = c.locked || ground;
            let lock = toggle(ui, &mut locked, "🔒", false, labels::LOCKED);
            if lock.clicked() && !ground {
                let change = crate::level_tab::CopyChange::Locked(locked);
                out.push(EditorAction::SetCopy { id: c.id.clone(), change });
            }
            match doc.copy_origin(&c.id) {
                Some(olr_format::levels::AssetOrigin::Own) | None => {
                    ui.weak(labels::OWN_MARK).on_hover_text("the level's own asset");
                }
                Some(_) => {
                    ui.label(labels::LIBRARY_MARK).on_hover_text("a copy of a library asset");
                }
            }
            // Issue #83: the name cut to the panel (named in full), then
            // the id in the room left.
            let name = doc.copy_name(&c.id).unwrap_or_else(|| c.id.clone());
            let on = selected.contains(&c.id);
            let r = ui.add(name_button(on, &name, true));
            super::relabel(&r, egui::WidgetType::Button, Some(on), &name);
            r.dnd_set_drag_payload(CarriedCopy(c.id.clone()));
            if r.double_clicked() {
                out.push(EditorAction::EditCopy(c.id.clone()));
            } else if r.clicked() {
                let mut ids = if ui.input(|i| i.modifiers.command) { selected.clone() } else { Vec::new() };
                match ids.iter().position(|i| *i == c.id) {
                    Some(i) => {
                        ids.remove(i);
                    }
                    None => ids.push(c.id.clone()),
                }
                out.push(EditorAction::Select(match ids.len() {
                    0 => Selection::None,
                    1 => Selection::Copy(ids.remove(0)),
                    _ => Selection::Copies(ids),
                }));
            }
            ui.add(egui::Label::new(egui::RichText::new(&c.id).weak()).truncate());
        });
        let Some(dragged) = carried.as_ref().filter(|d| **d != c.id) else { continue };
        let rect = row.response.rect;
        if ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| rect.contains(p)) {
            let from_below = copies.iter().position(|x| x.id == *dragged).is_some_and(|d| d < k);
            let y = if from_below { rect.top() } else { rect.bottom() };
            ui.painter().hline(rect.x_range(), y, accent);
        }
        if row.response.dnd_release_payload::<CarriedCopy>().is_some() {
            out.push(EditorAction::RestackCopy { id: dragged.clone(), to: k });
        }
    }
}

/// One folder level's indent in the list, points.
const INDENT: f32 = 14.0;

/// A folder row's share of its height, at its top (and at its bottom when
/// folded), that drops beside it rather than into it.
const EDGE: f32 = 0.3;

/// The asset tab's Layers (issue #78): the tree's rows in the panel's own
/// scroll area, the names cut to the panel's width, then Add layer and New
/// folder.
pub(super) fn tree(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    rows(ui, state, true, out);
    buttons(ui, state, out);
}

/// The tree's rows; `fit` cuts a name to the room left.
fn rows(ui: &mut Ui, state: &EditorState, fit: bool, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let rows = doc.rows();
    let folders = doc.folders();
    let carried = egui::DragAndDrop::payload::<String>(ui.ctx()).map(|p| (*p).clone());
    let accent = ui.visuals().selection.stroke;
    for (k, row) in rows.iter().enumerate() {
        let r = ui.horizontal(|ui| {
            ui.add_space(INDENT * row.depth as f32);
            match folders.iter().find(|f| row.folder && f.id == row.id) {
                Some(f) => folder_row(ui, state, f, fit, out),
                None => element_row(ui, state, &row.id, fit, out),
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
}

/// Add layer and New folder.
fn buttons(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
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
fn element_row(ui: &mut Ui, state: &EditorState, id: &str, fit: bool, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
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
    // Issue #79: the shown state paints it itself, a filled dot (drawn: a
    // font may lack "●").
    if doc.shown_owns(&e.id) {
        let side = ui.spacing().interact_size.y;
        let (r, painter) = ui.allocate_painter(egui::vec2(side * 0.5, side), egui::Sense::hover());
        painter.circle_filled(r.rect.center(), side * 0.18, ui.visuals().strong_text_color());
        let owner = doc.shown_state_name();
        let what = labels::owned(if e.name.is_empty() { &e.id } else { &e.name });
        super::relabel(&r, egui::WidgetType::Label, None, &what);
        r.on_hover_text(format!("{owner} paints it itself"));
    }
    if let Some(depth) = e.distant {
        ui.label("🌄").on_hover_text(format!("{}: depth {depth} %", labels::DISTANT));
    }
    let name = if e.name.is_empty() { e.id.as_str() } else { e.name.as_str() };
    let selected = state.selection().has(&e.id);
    // The name selects on a click and reorders on a drag.
    let r = ui.add(name_button(selected, name, fit));
    if fit {
        super::relabel(&r, egui::WidgetType::Button, Some(selected), name);
    }
    row_click(ui, &r, &e.id, Selection::Element(e.id.clone()), out);
}

/// A folder's row (issue #61): fold arrow, eye, lock, the folder mark and
/// its name.
fn folder_row(ui: &mut Ui, state: &EditorState, f: &Folder, fit: bool, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
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
    let r = ui.add(name_button(selected, name, fit));
    if fit {
        super::relabel(&r, egui::WidgetType::Button, Some(selected), name);
    }
    row_click(ui, &r, &f.id, Selection::Folder(f.id.clone()), out);
}

/// A row's name: a click selects, a drag reorders; `fit` cuts it to the
/// room left (named in full).
fn name_button(selected: bool, name: &str, fit: bool) -> egui::Button<'_> {
    let b = egui::Button::selectable(selected, name).sense(egui::Sense::click_and_drag());
    if fit { b.truncate() } else { b }
}

fn spawns(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    if doc.level().objects.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new("No spawns yet: place them with the Spawn tool (P).").weak(),
            )
            .wrap(),
        );
    }
    spawn_rows(ui, state, out);
}

fn spawn_rows(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
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
