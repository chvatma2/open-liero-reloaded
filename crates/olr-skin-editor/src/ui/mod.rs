//! The skin editor's egui panels (issue #14): the session bar on top
//! (Save, Close, Undo, Redo, the unsaved mark), the tools on the left
//! (what is painted, the tools, the colour, the onion skin, the frame
//! operations), the frames and the preview and the skin's settings on the
//! right, the chosen frame large in the middle, and the dialogs. `show`
//! draws them from the state and returns the frame's actions; the host
//! applies `olr_ui::theme`. Images are egui textures made here (the
//! level editor's swatches' way): the host does no GPU work.

mod canvas;
mod dialogs;
mod frames;
mod tools;

use egui::{Color32, ColorImage, Key, Modifiers, TextureHandle, TextureOptions, Ui};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::skin::Skin;

use crate::doc::FRAMES;
use crate::paint::Target;
use crate::state::{SkinAction, SkinEditorState, Tool};

/// The labels tests and scripts find widgets by.
pub mod labels {
    pub const SAVE: &str = "Save";
    pub const CLOSE: &str = "Close";
    pub const UNDO: &str = "Undo";
    pub const REDO: &str = "Redo";
    /// The unsaved changes' mark.
    pub const UNSAVED: &str = "Unsaved changes";
    /// The large frame's accessible name.
    pub const CANVAS: &str = "Frame canvas";
    pub const ZOOM_IN: &str = "Zoom in";
    pub const ZOOM_OUT: &str = "Zoom out";
    // What is painted (`Target::name`).
    pub const TARGET_COLOUR: &str = "Colour";
    pub const TARGET_TINT: &str = "Player colour";
    pub const TARGET_GLOW: &str = "Glow";
    pub const SIZE_1: &str = "1 pixel";
    pub const SIZE_2: &str = "2 × 2 pixels";
    /// The paint colour's picker, and its `#rrggbb` field.
    pub const PAINT_COLOUR: &str = "Paint colour";
    pub const HEX: &str = "Hex";
    /// The glow painted (`25 %` to `100 %` buttons beside it).
    pub const GLOW_STRENGTH: &str = "Glow strength";
    pub const ONION: &str = "Onion skin";
    pub const COPY_FRAME: &str = "Copy frame";
    pub const PASTE: &str = "Paste";
    pub const PASTE_ROW: &str = "Paste to row";
    pub const PASTE_COLUMN: &str = "Paste to column";
    pub const FLIP: &str = "Flip";
    pub const FLIP_VERTICAL: &str = "Flip vertically";
    pub const SHIFT_LEFT: &str = "Shift left";
    pub const SHIFT_RIGHT: &str = "Shift right";
    pub const SHIFT_UP: &str = "Shift up";
    pub const SHIFT_DOWN: &str = "Shift down";
    pub const SHIFT_ALL: &str = "Shift all frames";
    // The skin's settings.
    pub const NAME: &str = "Name";
    pub const SKIN_LIGHT: &str = "Skin light";
    pub const LIGHT_INTENSITY: &str = "Light intensity %";
    pub const LIGHT_REACH: &str = "Light reach";
    pub const MAKE_DOUBLE: &str = "Make double resolution";
    // The preview.
    pub const ANIMATE: &str = "Walk and aim";
    pub const PREVIEW_COLOUR: &str = "Preview colour";
    // The dialogs.
    pub const SKIN_NAME: &str = "Skin name";
    pub const SAVE_SKIN: &str = "Save skin";
    pub const SAVE_AND_CLOSE: &str = "Save and close";
    pub const DONT_SAVE: &str = "Don't save";
    pub const CANCEL: &str = "Cancel";
    pub const OK: &str = "OK";
}

/// What the preview's textures show: the skin's generation, the frame,
/// facing left, the player's colour.
type PreviewKey = (u64, usize, bool, Rgba);

/// Transient UI state: the textures made from the skin, drafts, the
/// canvas's rectangle and drag.
#[derive(Default)]
pub struct SkinEditorData {
    /// Each frame's image, of its frame generation.
    thumbs: Vec<Option<(u64, TextureHandle)>>,
    /// The chosen frame's tint or glow overlay, for (frame, generation,
    /// target).
    overlay: Option<((usize, u64, Target), TextureHandle)>,
    /// The transparent pixels' checks, for a frame size.
    checks: Option<((u32, u32), TextureHandle)>,
    /// The skin as the game loads it, for the preview, of a generation
    /// (`None`: it does not compile).
    compiled: Option<(u64, Option<Skin>)>,
    /// The preview's two cells (light, dark) for (generation, frame,
    /// facing left, colour).
    preview: Option<(PreviewKey, [TextureHandle; 2])>,
    /// The name field and the hex field while typed into.
    name: Option<String>,
    hex: Option<String>,
    /// The light's sliders while dragged.
    light: Option<(u16, u16)>,
    /// The name dialog's field.
    save_name: Option<String>,
    canvas_rect: Option<egui::Rect>,
    /// A paint drag that began on the canvas, and the art pixel last sent.
    canvas_drag: bool,
    last_pixel: Option<(i32, i32)>,
}

impl SkinEditorData {
    /// The canvas's rectangle as last drawn.
    pub fn canvas_rect(&self) -> Option<egui::Rect> {
        self.canvas_rect
    }

    /// Whether the preview's cells were made (tests).
    #[doc(hidden)]
    pub fn preview_shown(&self) -> bool {
        self.preview.is_some()
    }
}

/// Draws the skin editor into `ui` and returns this frame's actions.
pub fn show(ui: &mut Ui, state: &SkinEditorState, data: &mut SkinEditorData) -> Vec<SkinAction> {
    let mut out = Vec::new();
    let mut fields = Vec::new();
    egui::Panel::top("olr-skin-editor-session").show(ui, |ui| session(ui, state, &mut out));
    egui::Panel::bottom("olr-skin-editor-status").show(ui, |ui| {
        ui.horizontal(|ui| {
            let doc = state.doc();
            let (w, h) = doc.frame_size();
            let kind = if doc.is_classic() { "classic" } else { "double resolution" };
            ui.weak(format!("{w} × {h} pixels a frame, {kind}"));
            ui.separator();
            ui.label(state.status());
        });
    });
    egui::Panel::left("olr-skin-editor-tools").exact_size(220.0).resizable(false).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("olr-skin-editor-tools").show(ui, |ui| {
            tools::show(ui, state, data, &mut out);
        });
    });
    egui::Panel::right("olr-skin-editor-frames").exact_size(470.0).resizable(false).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("olr-skin-editor-frames").show(ui, |ui| {
            frames::show(ui, state, data, &mut out, &mut fields);
        });
    });
    egui::CentralPanel::default().show(ui, |ui| canvas::show(ui, state, data, &mut out));
    dialogs::show(ui.ctx(), state, data, &mut out);
    shortcuts(ui, state, &mut out);
    // A field's last value is in before a click that ended its edit acts.
    fields.extend(out);
    fields
}

/// Save, Close, Undo and Redo, the skin's name and the unsaved mark.
fn session(ui: &mut Ui, state: &SkinEditorState, out: &mut Vec<SkinAction>) {
    ui.horizontal(|ui| {
        if ui.button(labels::SAVE).clicked() {
            out.push(SkinAction::Save);
        }
        if ui.button(labels::CLOSE).clicked() {
            out.push(SkinAction::Close);
        }
        ui.separator();
        if ui.add_enabled(state.can_undo(), egui::Button::new(labels::UNDO)).clicked() {
            out.push(SkinAction::Undo);
        }
        if ui.add_enabled(state.can_redo(), egui::Button::new(labels::REDO)).clicked() {
            out.push(SkinAction::Redo);
        }
        ui.separator();
        ui.strong(format!("Skin: {}", state.doc().name()));
        if state.pack_id().is_none() {
            ui.weak("(not saved yet)");
        }
        // The mark keeps its space: the bar never moves.
        let mark = ui.label(if state.dirty() { "●" } else { " " });
        if state.dirty() {
            relabel(&mark, egui::WidgetType::Label, labels::UNSAVED);
            mark.on_hover_text(labels::UNSAVED);
        }
    });
}

/// Ctrl+Z, Ctrl+Y, Ctrl+S, Ctrl+C and Ctrl+V (the frame), the tools' keys;
/// not while a dialog is open or a text field has the keyboard.
fn shortcuts(ui: &Ui, state: &SkinEditorState, out: &mut Vec<SkinAction>) {
    if state.dialog().is_some() || ui.ctx().egui_wants_keyboard_input() || ui.ctx().dragged_id().is_some() {
        return;
    }
    ui.input_mut(|i| {
        let command = [
            (Key::Z, SkinAction::Undo),
            (Key::Y, SkinAction::Redo),
            (Key::S, SkinAction::Save),
            (Key::C, SkinAction::CopyFrame),
            (Key::V, SkinAction::Paste(crate::state::PasteTo::Frame)),
        ];
        for (key, action) in command {
            if i.consume_key(Modifiers::COMMAND, key) {
                out.push(action);
            }
        }
        for tool in Tool::ALL {
            if let Some(key) = Key::from_name(&tool.key().to_string())
                && i.consume_key(Modifiers::NONE, key)
            {
                out.push(SkinAction::Tool(tool));
            }
        }
    });
}

/// Names `r` `label` for AccessKit, whatever it shows.
pub(crate) fn relabel(r: &egui::Response, typ: egui::WidgetType, label: &str) {
    let enabled = r.enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(typ, enabled, label));
}

/// An egui texture of `img`, its pixels sharp.
pub(crate) fn texture(ui: &Ui, name: &str, img: &RgbaImage) -> TextureHandle {
    let size = [img.width() as usize, img.height() as usize];
    ui.ctx().load_texture(
        name,
        ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
        TextureOptions::NEAREST,
    )
}

/// Frame `f`'s image, made again when the frame changed.
pub(crate) fn frame_texture(
    ui: &Ui,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    f: usize,
) -> TextureHandle {
    if data.thumbs.len() != FRAMES {
        data.thumbs = vec![None; FRAMES];
    }
    let generation = state.doc().frame_generation(f);
    if let Some((g, t)) = &data.thumbs[f]
        && *g == generation
    {
        return t.clone();
    }
    let t = texture(ui, &format!("olr-skin-editor-frame-{f}"), &state.doc().frame_image(f));
    data.thumbs[f] = Some((generation, t.clone()));
    t
}

/// The checks shown under transparent pixels: one per art pixel.
pub(crate) fn checks(ui: &Ui, data: &mut SkinEditorData, size: (u32, u32)) -> TextureHandle {
    if let Some((s, t)) = &data.checks
        && *s == size
    {
        return t.clone();
    }
    let mut img = RgbaImage::new(size.0, size.1);
    for y in 0..size.1 {
        for x in 0..size.0 {
            let v = if (x + y) % 2 == 0 { 92 } else { 112 };
            img.set(x, y, Rgba::rgb(v, v, v));
        }
    }
    let t = texture(ui, "olr-skin-editor-checks", &img);
    data.checks = Some((size, t.clone()));
    t
}

/// `c` as egui's colour.
pub(crate) fn colour32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// `#rrggbb`.
pub(crate) fn hex(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// A `#rrggbb` (or `rrggbb`) colour.
pub(crate) fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some(Rgba::rgb(v(0)?, v(2)?, v(4)?))
}
