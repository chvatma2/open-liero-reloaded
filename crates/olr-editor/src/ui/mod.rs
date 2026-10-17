//! The editor's egui panels (spec §4): a toolbar, the element list, the
//! properties, the status bar, the canvas and the dialogs. `show` draws
//! them from the state and returns the frame's actions, as
//! `olr_ui::screens::show` does; the host applies `olr_ui::theme`.

mod canvas;
mod dialogs;
mod elements;
mod level;
mod properties;
mod status;
mod toolbar;

use std::collections::BTreeMap;

use egui::{Key, Modifiers, Ui};

use crate::state::{EditorAction, EditorState, Selection};
use crate::tools::Tool;

/// The labels tests and 14c's e2e script find widgets by.
pub mod labels {
    pub const UNDO: &str = "Undo";
    pub const REDO: &str = "Redo";
    pub const SAVE: &str = "Save";
    pub const PLAY: &str = "Play";
    pub const ADD_LAYER: &str = "Add layer";
    pub const IMPORT_IMAGE: &str = "Import image";
    pub const OBJECTS: &str = "Objects";
    pub const PIXEL_SIZE: &str = "Pixel size";
    pub const COLOURS: &str = "Colours";
    pub const DITHER: &str = "Dither";
    pub const THRESHOLD: &str = "Threshold";
    pub const MATERIAL: &str = "Material";
    pub const VISIBLE: &str = "Visible";
    pub const LOCKED: &str = "Locked";
    pub const NAME: &str = "Name";
    pub const DONT_SAVE: &str = "Don't save";
    pub const CANCEL: &str = "Cancel";
}

/// Transient UI state: value and text drafts, the canvas rect last drawn,
/// the Space key held, the canvas's pointer drag.
#[derive(Default)]
pub struct EditorData {
    /// A `DragValue` being dragged or typed into, by field key.
    drafts: BTreeMap<String, f64>,
    /// A text field being edited, by field key.
    texts: BTreeMap<String, String>,
    canvas_rect: Option<egui::Rect>,
    /// The canvas size last reported (`EditorAction::Canvas`).
    canvas_size: Option<[f32; 2]>,
    /// A pointer drag that began on the canvas.
    canvas_drag: bool,
    /// The world pixel last reported to the state.
    last_world: Option<(i32, i32)>,
    /// The level settings' resize fields (width, height, offset x and y)
    /// and the level size they started from: a new size resets them.
    resize: Option<([i64; 4], (u32, u32))>,
    /// The new area's material for a resize: 0 rock, 1 dirt, 2 background.
    resize_material: usize,
    /// The open level's directory: drafts belong to it.
    dir: Option<std::path::PathBuf>,
    /// The draft keys of the fields drawn this frame.
    drawn: std::collections::BTreeSet<String>,
}

impl EditorData {
    /// The canvas's rectangle as last drawn.
    pub fn canvas_rect(&self) -> Option<egui::Rect> {
        self.canvas_rect
    }
}

/// Draws the editor into `ui` and returns the actions of this frame. The
/// properties' commits come first: a field's last value is in before a
/// toolbar or list click that ended its edit (Undo, Save) acts.
pub fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData) -> Vec<EditorAction> {
    let Some(doc) = state.document() else {
        ui.centered_and_justified(|ui| ui.label("No level is open."));
        return Vec::new();
    };
    // Drafts belong to the open level.
    if data.dir.as_deref() != Some(doc.dir()) {
        let canvas = (data.canvas_rect, data.canvas_size);
        *data = EditorData { dir: Some(doc.dir().to_path_buf()), ..EditorData::default() };
        (data.canvas_rect, data.canvas_size) = canvas;
    }
    data.drawn.clear();
    let (mut toolbar, mut list, mut fields, mut canvas, mut dialogs) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    egui::Panel::top("olr-editor-toolbar").show(ui, |ui| toolbar::show(ui, state, data, &mut toolbar));
    egui::Panel::bottom("olr-editor-status").show(ui, |ui| status::show(ui, state));
    egui::Panel::left("olr-editor-elements")
        .default_size(220.0)
        .show(ui, |ui| elements::show(ui, state, &mut list));
    egui::Panel::right("olr-editor-properties").default_size(260.0).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| match state.selection() {
            Selection::None => level::show(ui, state, data, &mut fields),
            _ => properties::show(ui, state, data, &mut fields),
        })
    });
    egui::CentralPanel::default().show(ui, |ui| canvas::show(ui, state, data, &mut canvas));
    dialogs::show(ui.ctx(), state, &mut dialogs);
    // A draft whose field was not drawn this frame is dropped, never
    // committed later to whatever shows that field again.
    let drawn = std::mem::take(&mut data.drawn);
    data.drafts.retain(|k, _| drawn.contains(k));
    data.texts.retain(|k, _| drawn.contains(k));
    data.drawn = drawn;
    let mut out = fields;
    shortcuts(ui, state, &mut out);
    out.extend(list.into_iter().chain(canvas).chain(toolbar).chain(dialogs));
    out
}

/// The shortcuts, when no text field has the keyboard and no dialog is open.
fn shortcuts(ui: &Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    // Not while typing, nor while a widget is dragged (its draft would
    // outlive the change a shortcut makes).
    if state.dialog().is_some() || ui.ctx().egui_wants_keyboard_input() || ui.ctx().dragged_id().is_some() {
        return;
    }
    ui.input_mut(|i| {
        let command =
            [(Key::Z, EditorAction::Undo), (Key::Y, EditorAction::Redo), (Key::S, EditorAction::Save)];
        for (key, action) in command {
            if i.consume_key(Modifiers::COMMAND, key) {
                out.push(action);
            }
        }
        if i.consume_key(Modifiers::NONE, Key::Delete) {
            out.push(EditorAction::Remove);
        }
        for tool in Tool::ALL {
            if let Some(key) = Key::from_name(&tool.key().to_string())
                && i.consume_key(Modifiers::NONE, key)
            {
                out.push(EditorAction::Tool(tool));
            }
        }
        let arrows =
            [(Key::ArrowLeft, -1, 0), (Key::ArrowRight, 1, 0), (Key::ArrowUp, 0, -1), (Key::ArrowDown, 0, 1)];
        for (key, dx, dy) in arrows {
            if i.consume_key(Modifiers::NONE, key) {
                out.push(EditorAction::Nudge { dx, dy });
            }
        }
    });
}

/// A labelled integer `DragValue` (`key` names its draft) that commits once:
/// when a drag stops or typing ends. Returns the new value then.
pub(crate) fn number(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    label: &str,
    current: i64,
    range: std::ops::RangeInclusive<i64>,
) -> Option<i64> {
    data.drawn.insert(key.into());
    ui.horizontal(|ui| {
        let l = ui.label(label);
        let mut v = data.drafts.get(key).copied().unwrap_or(current as f64);
        let r = ui
            .add(
                egui::DragValue::new(&mut v)
                    .range(*range.start() as f64..=*range.end() as f64)
                    .speed(0.25)
                    .max_decimals(0)
                    .update_while_editing(false),
            )
            .labelled_by(l.id);
        if r.dragged() || r.has_focus() {
            data.drafts.insert(key.into(), v);
            return None;
        }
        data.drafts.remove(key);
        let v = (v.round() as i64).clamp(*range.start(), *range.end());
        (v != current).then_some(v)
    })
    .inner
}

/// A labelled one-line text field (`key` names its draft) that commits when
/// it loses focus. Returns the new text then.
pub(crate) fn text(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    label: &str,
    current: &str,
    multiline: bool,
) -> Option<String> {
    data.drawn.insert(key.into());
    ui.horizontal(|ui| {
        let l = ui.label(label);
        let mut v = data.texts.get(key).cloned().unwrap_or_else(|| current.to_string());
        let edit = if multiline {
            egui::TextEdit::multiline(&mut v).desired_rows(3)
        } else {
            egui::TextEdit::singleline(&mut v)
        };
        let r = ui.add(edit).labelled_by(l.id);
        if r.has_focus() {
            data.texts.insert(key.into(), v);
            return None;
        }
        data.texts.remove(key);
        // Escape drops what was typed.
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        (r.lost_focus() && !escaped && v != current).then_some(v)
    })
    .inner
}
