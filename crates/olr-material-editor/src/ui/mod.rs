//! The material editor's egui panels (issue #44): the session bar on top
//! (Save, Close, Undo, Redo, the material, the unsaved mark), the library
//! on the left (the built-in materials, then the user's; New, Copy,
//! Rename, Delete), the open material's fields in the middle (every one
//! within its bounds, the colour ramp's keys and its strip), the live heat
//! preview on the right (issue #45: the host's picture, click to blast, the
//! blast size, the time scale, Restart, the material compared), the status
//! at the bottom, and the dialogs. `show` draws them from the state and
//! returns the frame's actions; the host applies `olr_ui::theme`.

mod dialogs;
mod fields;

use std::collections::BTreeMap;

use egui::{Color32, Key as EKey, Modifiers, Ui};
use olr_look::{MAX_TEMPERATURE, RampKey, ramp_at};

use crate::preview::{self as scene, BlastSize, PreviewAction, TimeScale};
use crate::state::{Key, MaterialAction, MaterialEditorState};

/// The labels tests and scripts find widgets by.
pub mod labels {
    pub const SAVE: &str = "Save";
    pub const CLOSE: &str = "Close";
    pub const UNDO: &str = "Undo";
    pub const REDO: &str = "Redo";
    /// The unsaved changes' mark.
    pub const UNSAVED: &str = "Unsaved changes";
    // The library.
    pub const NEW: &str = "New material";
    pub const COPY: &str = "Copy";
    pub const RENAME: &str = "Rename";
    pub const DELETE: &str = "Delete";
    /// A built-in material's way to change it.
    pub const EDIT_A_COPY: &str = "Edit a copy";
    // The fields.
    pub const NOTES: &str = "Notes";
    /// Issue #57: the light soak's choice, the level's or its own.
    pub const LIGHT_SOAK: &str = "Light soak";
    pub const LIGHT_SOAK_LEVEL: &str = "Level's";
    pub const HEATS: &str = "Takes heat";
    pub const SENSITIVITY: &str = "Sensitivity %";
    pub const CAPACITY: &str = "Capacity %";
    pub const CONDUCTIVITY: &str = "Conductivity %";
    pub const COOLING: &str = "Cooling %";
    pub const LIGHT: &str = "Light %";
    pub const OWN_RAMP: &str = "Own colour ramp";
    /// The ramp drawn as a strip.
    pub const RAMP: &str = "Colour ramp";
    pub const ADD_KEY: &str = "Add key";
    /// The live preview's picture (issue #45): a click blasts it.
    pub const PREVIEW: &str = "Heat preview";
    /// The scene afresh.
    pub const RESTART: &str = "Restart";
    /// The material compared's picker, and its entry for none.
    pub const COMPARE: &str = "Compare with";
    pub const COMPARE_NONE: &str = "No material";
    // The blast sizes and time scales are found by their own labels
    // (`BlastSize::label`, `TimeScale::label`).

    /// The compare picker's entry of a library material.
    pub fn compare_entry(name: &str, builtin: bool) -> String {
        format!("{name} ({})", if builtin { "built in" } else { "yours" })
    }
    // The dialogs.
    pub const MATERIAL_NAME: &str = "Material name";
    pub const CREATE: &str = "Create";
    pub const RENAME_OK: &str = "Rename material";
    pub const DELETE_OK: &str = "Delete material";
    pub const SAVE_CHANGES: &str = "Save changes";
    pub const DONT_SAVE: &str = "Don't save";
    pub const CANCEL: &str = "Cancel";
    pub const OK: &str = "OK";

    /// A number field's slider.
    /// The light soak's choice of `px` world pixels.
    pub fn soak_px(px: u8) -> String {
        format!("{px} px")
    }

    pub fn slider(name: &str) -> String {
        format!("{name} slider")
    }
    /// Ramp key `i`'s (from 0) fields, numbered from 1.
    pub fn key_temperature(i: usize) -> String {
        format!("Key {} temperature", i + 1)
    }
    pub fn key_colour(i: usize) -> String {
        format!("Key {} colour", i + 1)
    }
    pub fn key_colour_picker(i: usize) -> String {
        format!("Key {} colour picker", i + 1)
    }
    pub fn key_glow(i: usize) -> String {
        format!("Key {} glow %", i + 1)
    }
    pub fn remove_key(i: usize) -> String {
        format!("Remove key {}", i + 1)
    }
}

/// Transient UI state: the fields' drafts while typed into, the sliders
/// while dragged, the name dialog's text.
#[derive(Default)]
pub struct MaterialEditorData {
    /// The state's generation the drafts were made for.
    generation: u64,
    /// A text field's text while it has the keyboard, by its label.
    drafts: BTreeMap<String, String>,
    /// A slider's value while dragged: (label, value).
    slide: Option<(String, f64)>,
    /// The name dialog's text.
    name: Option<String>,
    /// The live preview's picture, the host's (issue #45).
    preview: Option<PreviewImage>,
}

/// The live preview's picture as the host renders it (issue #45): an egui
/// texture of the scene ([`crate::preview::SCENE`]), drawn whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewImage {
    pub texture: egui::TextureId,
}

impl MaterialEditorData {
    /// The picture the preview shows from now on (none: its place, empty).
    pub fn set_preview(&mut self, image: Option<PreviewImage>) {
        self.preview = image;
    }

    pub fn preview(&self) -> Option<PreviewImage> {
        self.preview
    }
}

/// Draws the material editor into `ui` and returns this frame's actions.
pub fn show(ui: &mut Ui, state: &MaterialEditorState, data: &mut MaterialEditorData) -> Vec<MaterialAction> {
    if data.generation != state.generation() {
        data.generation = state.generation();
        data.drafts.clear();
        data.slide = None;
    }
    let mut out = Vec::new();
    let mut commits = Vec::new();
    egui::Panel::top("olr-material-editor-session").show(ui, |ui| session(ui, state, &mut out));
    egui::Panel::bottom("olr-material-editor-status").show(ui, |ui| {
        ui.label(state.status());
    });
    egui::Panel::left("olr-material-editor-list").exact_size(260.0).resizable(false).show(ui, |ui| {
        list(ui, state, &mut out);
    });
    egui::Panel::right("olr-material-editor-preview").exact_size(360.0).resizable(false).show(ui, |ui| {
        preview(ui, state, data.preview, &mut out);
    });
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("olr-material-editor-fields").show(ui, |ui| {
            fields::show(ui, state, data, &mut out, &mut commits);
        });
    });
    dialogs::show(ui.ctx(), state, data, &mut out);
    shortcuts(ui, state, &mut out);
    // A field's last value is in before a click that ended its edit acts.
    commits.extend(out);
    commits
}

/// Save, Close, Undo and Redo, the material and the unsaved mark.
fn session(ui: &mut Ui, state: &MaterialEditorState, out: &mut Vec<MaterialAction>) {
    ui.horizontal(|ui| {
        let editable = state.doc().is_some() && !state.read_only();
        if ui.add_enabled(editable, egui::Button::new(labels::SAVE)).clicked() {
            out.push(MaterialAction::Save);
        }
        if ui.button(labels::CLOSE).clicked() {
            out.push(MaterialAction::Close);
        }
        ui.separator();
        if ui.add_enabled(state.can_undo(), egui::Button::new(labels::UNDO)).clicked() {
            out.push(MaterialAction::Undo);
        }
        if ui.add_enabled(state.can_redo(), egui::Button::new(labels::REDO)).clicked() {
            out.push(MaterialAction::Redo);
        }
        ui.separator();
        match state.doc() {
            Some(d) => {
                ui.strong(format!("Material: {}", d.name));
                if state.read_only() {
                    ui.weak("(built in: read-only)");
                }
            }
            None => {
                ui.weak("No material open");
            }
        }
        // The mark keeps its space: the bar never moves.
        let mark = ui.label(if state.dirty() { "●" } else { " " });
        if state.dirty() {
            relabel(&mark, egui::WidgetType::Label, labels::UNSAVED);
            mark.on_hover_text(labels::UNSAVED);
        }
    });
}

/// The library: the built-in materials, the user's (and a recovered one
/// never written), then New, Copy, Rename and Delete.
fn list(ui: &mut Ui, state: &MaterialEditorState, out: &mut Vec<MaterialAction>) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button(labels::NEW).clicked() {
            out.push(MaterialAction::New);
        }
        let open = state.doc().is_some();
        if ui
            .add_enabled(open, egui::Button::new(labels::COPY))
            .on_hover_text("A copy in your materials")
            .clicked()
        {
            out.push(MaterialAction::Copy);
        }
        let yours = open && !state.read_only();
        if ui.add_enabled(yours, egui::Button::new(labels::RENAME)).clicked() {
            out.push(MaterialAction::Rename);
        }
        if ui.add_enabled(yours, egui::Button::new(labels::DELETE)).clicked() {
            out.push(MaterialAction::Delete);
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt("olr-material-editor-library").show(ui, |ui| {
        let selected = state.selected();
        for (builtin, heading) in [(true, "Built in"), (false, "Yours")] {
            ui.add_space(4.0);
            ui.strong(heading);
            let mut any = false;
            for e in state.library().iter().filter(|e| e.builtin == builtin) {
                any = true;
                let key = Key { builtin, id: e.material.id.clone() };
                let r = ui
                    .selectable_label(selected == Some(&key), &e.material.name)
                    .on_hover_text(format!("{}.ron", e.material.id));
                if r.clicked() {
                    out.push(MaterialAction::Select(key));
                }
            }
            if !builtin && state.unsaved_file() {
                any = true;
                if let Some(d) = state.doc() {
                    let _ = ui.selectable_label(true, format!("{} (not saved)", d.name));
                }
            }
            if !any {
                ui.weak(if builtin { "none found" } else { "none yet: New, or Copy one" });
            }
        }
    });
}

/// The live preview (issue #45): the host's picture of the two patches,
/// the material open's and the one compared, named over them; a click on
/// a patch blasts both there. Under it the blast size, the time scale,
/// Restart and the material compared.
fn preview(
    ui: &mut Ui,
    state: &MaterialEditorState,
    image: Option<PreviewImage>,
    out: &mut Vec<MaterialAction>,
) {
    let act = |out: &mut Vec<MaterialAction>, a| out.push(MaterialAction::Preview(a));
    ui.add_space(4.0);
    ui.strong("Preview");
    let (w, h) = (scene::SCENE.0 as f32, scene::SCENE.1 as f32);
    let size = egui::vec2(ui.available_width(), ui.available_width() * h / w);
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    relabel(&r, egui::WidgetType::Image, labels::PREVIEW);
    let p = ui.painter_at(rect);
    match image {
        Some(img) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            p.image(img.texture, rect, uv, Color32::WHITE);
        }
        None => {
            p.rect_filled(rect, 4.0, Color32::from_gray(16));
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "The preview needs a graphics device.",
                egui::FontId::proportional(13.0),
                Color32::from_gray(140),
            );
        }
    }
    let names = [
        state.doc().map_or("No material open", |d| d.name.as_str()),
        state.compare_material().map_or("No material", |m| m.name.as_str()),
    ];
    for (i, name) in names.into_iter().enumerate() {
        let x = rect.min.x + scene::patch_x(i) as f32 / w * rect.width();
        p.text(
            egui::pos2(x + 4.0, rect.min.y + 3.0),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::proportional(12.0),
            Color32::from_gray(200),
        );
    }
    if r.clicked()
        && let Some(at) = r.interact_pointer_pos()
    {
        let world = [(at.x - rect.min.x) / rect.width() * w, (at.y - rect.min.y) / rect.height() * h];
        if let Some((_, px)) = scene::patch_at(world) {
            act(out, PreviewAction::Blast(px));
        }
    }
    r.on_hover_text("Click a patch to blast both there");
    let settings = state.preview();
    ui.horizontal(|ui| {
        caption(ui, "Blast");
        for s in BlastSize::ALL {
            if ui.selectable_label(settings.size == s, s.label()).clicked() {
                act(out, PreviewAction::Size(s));
            }
        }
    });
    ui.horizontal(|ui| {
        caption(ui, "Speed");
        for t in TimeScale::ALL {
            if ui.selectable_label(settings.scale == t, t.label()).clicked() {
                act(out, PreviewAction::Scale(t));
            }
        }
        ui.separator();
        if ui.button(labels::RESTART).on_hover_text("Craters filled, the heat gone").clicked() {
            act(out, PreviewAction::Restart);
        }
    });
    ui.horizontal(|ui| {
        let shown = state
            .compare_material()
            .zip(settings.compare.as_ref())
            .map_or(labels::COMPARE_NONE.to_string(), |(m, k)| labels::compare_entry(&m.name, k.builtin));
        egui::ComboBox::from_label(labels::COMPARE).selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(settings.compare.is_none(), labels::COMPARE_NONE).clicked() {
                act(out, PreviewAction::Compare(None));
            }
            for e in state.library() {
                let key = Key { builtin: e.builtin, id: e.material.id.clone() };
                let on = settings.compare.as_ref() == Some(&key);
                if ui.selectable_label(on, labels::compare_entry(&e.material.name, e.builtin)).clicked() {
                    act(out, PreviewAction::Compare(Some(key)));
                }
            }
        });
    });
    ui.weak("Click a patch to blast both. The left is the material open, as edited.");
}

/// The ramp `keys` as a strip `size` large, coldest left: the colour a
/// pixel shows at each temperature over the glow it gives (a pixel below
/// the first key shows its art, drawn as checks); the keys marked under it.
pub(crate) fn strip(ui: &mut Ui, keys: &[RampKey], size: egui::Vec2) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::hover());
    relabel(&r, egui::WidgetType::Image, labels::RAMP);
    let p = ui.painter_at(rect.expand(1.0));
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() * 0.7));
    let glow = egui::Rect::from_min_max(
        egui::pos2(rect.min.x, bar.max.y + 2.0),
        egui::pos2(rect.max.x, rect.max.y - 4.0),
    );
    let columns = (rect.width().max(1.0) as usize).min(400);
    let w = rect.width() / columns as f32;
    for i in 0..columns {
        let t = ((i as f32 + 0.5) / columns as f32 * f32::from(MAX_TEMPERATURE)) as u16;
        let x = rect.min.x + i as f32 * w;
        let column = |r: egui::Rect| {
            egui::Rect::from_min_max(egui::pos2(x, r.min.y), egui::pos2(x + w + 0.5, r.max.y))
        };
        match ramp_at(keys, t) {
            Some(([r, g, b], gl)) => {
                p.rect_filled(column(bar), 0.0, Color32::from_rgb(r, g, b));
                let v = (u32::from(gl) * 255 / 100) as u8;
                p.rect_filled(column(glow), 0.0, Color32::from_gray(v));
            }
            None => {
                let c = if (i / 4) % 2 == 0 { 70 } else { 90 };
                p.rect_filled(column(bar), 0.0, Color32::from_gray(c));
                p.rect_filled(column(glow), 0.0, Color32::from_gray(0));
            }
        }
    }
    for k in keys {
        let x = rect.min.x + rect.width() * f32::from(k.temperature) / f32::from(MAX_TEMPERATURE);
        p.line_segment([egui::pos2(x, rect.max.y - 4.0), egui::pos2(x, rect.max.y)], (2.0, Color32::WHITE));
    }
    p.rect_stroke(bar, 0.0, (1.0, Color32::from_gray(60)), egui::StrokeKind::Outside);
    r.on_hover_text("The colour from 0 (cold, left) to 1000 (a fresh crater's rim), the glow under it")
}

/// Ctrl+Z, Ctrl+Y and Ctrl+S, not while a dialog is open or a text field
/// has the keyboard.
fn shortcuts(ui: &Ui, state: &MaterialEditorState, out: &mut Vec<MaterialAction>) {
    if state.dialog().is_some() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    ui.input_mut(|i| {
        for (key, action) in [
            (EKey::Z, MaterialAction::Undo),
            (EKey::Y, MaterialAction::Redo),
            (EKey::S, MaterialAction::Save),
        ] {
            if i.consume_key(Modifiers::COMMAND, key) {
                out.push(action);
            }
        }
    });
}

/// Names `r` `label` for AccessKit, whatever it shows.
pub(crate) fn relabel(r: &egui::Response, typ: egui::WidgetType, label: &str) {
    let enabled = r.enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(typ, enabled, label));
}

/// Text painted beside a widget (not a widget: each name is one node).
pub(crate) fn caption(ui: &mut Ui, text: &str) -> egui::Response {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let (rect, r) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter().galley(rect.min, galley, ui.visuals().text_color());
    r
}
