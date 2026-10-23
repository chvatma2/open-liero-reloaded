//! The skin editor's egui panels (issue #14): the session bar on top
//! (Save, Close, Undo, Redo, the unsaved mark), the tools on the left
//! (what is painted, the tools, the colour, the onion skin, the frame
//! operations), the frames and the preview and the skin's settings on the
//! right, the chosen frame large in the middle, and the dialogs. `show`
//! draws them from the state and returns the frame's actions; the host
//! applies `olr_ui::theme`. They fit any window (issue #55): the side
//! panels take a share of its width, and below [`NARROW`] one shows at a
//! time (the session bar's Tools | Frames). Images are egui textures made
//! here (the level editor's swatches' way): the host does no GPU work.

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
    // The narrow window's switch between the side panels.
    pub const SHOW_TOOLS: &str = "Tools";
    pub const SHOW_FRAMES: &str = "Frames";
}

/// Below this width (points) one side panel shows at a time.
pub const NARROW: f32 = 900.0;
/// The tools panel: its share of the window's width, and its widths
/// (points, its frame's margins in).
const TOOLS_SHARE: f32 = 0.2;
const TOOLS_MIN: f32 = 200.0;
const TOOLS_MAX: f32 = 220.0;
/// The frames panel: its share and its narrowest (the frames at about
/// `frames::MIN_THUMB` wide); at most what the frames need at full size.
const FRAMES_SHARE: f32 = 0.37;
const FRAMES_MIN: f32 = 240.0;
/// A narrow window's side panel's share.
const NARROW_SHARE: f32 = 0.55;
/// The passes the side panels' scroll areas follow a focus that moved
/// otherwise than by the pointer.
const FOLLOW_PASSES: u8 = 8;

/// The side panel a narrow window shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Side {
    #[default]
    Tools,
    Frames,
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
    /// The side panel a narrow window shows.
    side: Side,
    /// The focus last seen, and the passes left to follow it.
    seen_focus: Option<egui::Id>,
    follow: u8,
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
    see_focus(ui.ctx(), data);
    let follow = data.follow > 0;
    data.follow = data.follow.saturating_sub(1);
    if follow {
        ui.ctx().request_repaint();
    }
    let width = ui.available_rect_before_wrap().width();
    let narrow = width < NARROW;
    // A side panel's frame and its scroll bar's room.
    let margins = egui::Frame::side_top_panel(ui.style()).total_margin().sum().x
        + ui.spacing().scroll.allocated_width();
    let (tools_w, frames_w) = {
        let share = |s: f32, min: f32, max: f32| (width * s).clamp(min, max.max(min)).min(width);
        let frames_max = frames::full_width(ui) + margins;
        if narrow {
            (share(NARROW_SHARE, TOOLS_MIN, TOOLS_MAX), share(NARROW_SHARE, FRAMES_MIN, frames_max))
        } else {
            (share(TOOLS_SHARE, TOOLS_MIN, TOOLS_MAX), share(FRAMES_SHARE, FRAMES_MIN, frames_max))
        }
    };
    egui::Panel::top("olr-skin-editor-session").show(ui, |ui| session(ui, state, data, narrow, &mut out));
    egui::Panel::bottom("olr-skin-editor-status").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let doc = state.doc();
            let (w, h) = doc.frame_size();
            let kind = if doc.is_classic() { "classic" } else { "double resolution" };
            ui.weak(format!("{w} × {h} pixels a frame, {kind}"));
            ui.separator();
            ui.label(state.status());
        });
    });
    if !narrow || data.side == Side::Tools {
        egui::Panel::left("olr-skin-editor-tools").exact_size(tools_w).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("olr-skin-editor-tools").show(ui, |ui| {
                tools::show(ui, state, data, &mut out);
                follow_focus(ui, follow);
            });
        });
    }
    if !narrow || data.side == Side::Frames {
        egui::Panel::right("olr-skin-editor-frames").exact_size(frames_w).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("olr-skin-editor-frames").show(ui, |ui| {
                frames::show(ui, state, data, &mut out, &mut fields);
                follow_focus(ui, follow);
            });
        });
    }
    egui::CentralPanel::default().show(ui, |ui| canvas::show(ui, state, data, &mut out));
    dialogs::show(ui.ctx(), state, data, &mut out);
    shortcuts(ui, state, &mut out);
    see_focus(ui.ctx(), data);
    // A field's last value is in before a click that ended its edit acts.
    fields.extend(out);
    fields
}

/// The narrow window's switch, Save, Close, Undo and Redo, the skin's
/// name and the unsaved mark.
fn session(
    ui: &mut Ui,
    state: &SkinEditorState,
    data: &mut SkinEditorData,
    narrow: bool,
    out: &mut Vec<SkinAction>,
) {
    ui.horizontal_wrapped(|ui| {
        if narrow {
            for (side, label) in [(Side::Tools, labels::SHOW_TOOLS), (Side::Frames, labels::SHOW_FRAMES)] {
                if ui.selectable_label(data.side == side, label).clicked() {
                    data.side = side;
                }
            }
            ui.separator();
        }
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

/// Notes a focus that moved otherwise than by the pointer (Tab, a
/// screen reader): the side panels' scroll areas follow it for the next
/// passes (`olr_ui::pad`'s `see_focus`, the editor's own).
fn see_focus(ctx: &egui::Context, data: &mut SkinEditorData) {
    let focused = ctx.memory(|m| m.focused());
    if focused == data.seen_focus {
        return;
    }
    data.seen_focus = focused;
    let pointer = ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_released() || i.pointer.any_down());
    if focused.is_some() && !pointer {
        data.follow = FOLLOW_PASSES;
    }
}

/// Ends a side panel's scroll area's content (`ui`): while `follow`,
/// scrolls the focused widget into view when it is in this area and not
/// all shown (egui scrolls to no focused widget by itself). The editor's
/// own `olr_ui::pad::follow_focus` (it does not depend on `olr-ui`).
fn follow_focus(ui: &Ui, follow: bool) {
    if !follow {
        return;
    }
    let ctx = ui.ctx();
    let Some(r) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) else { return };
    if r.layer_id != ui.layer_id() || !ui.min_rect().contains(r.rect.center()) {
        return;
    }
    if ui.clip_rect().contains_rect(r.rect) {
        return;
    }
    // Where it was when this area last scrolled to it: it did not move.
    let last = ui.id().with("olr-skin-editor follow last");
    if ctx.data(|d| d.get_temp::<(egui::Id, egui::Rect)>(last)) == Some((r.id, r.rect)) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(last, (r.id, r.rect)));
    ui.scroll_to_rect(r.rect, None);
}

/// The width (points) of `text` in `style`, unwrapped.
pub(crate) fn text_width(ui: &Ui, text: &str, style: egui::TextStyle) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
        .size()
        .x
}

/// `label` and the control `add` makes (given the label's id and the
/// width it may take), side by side when the control has `needs` points
/// beside the label, else the label above it.
pub(crate) fn beside<R>(
    ui: &mut Ui,
    label: &str,
    needs: f32,
    add: impl FnOnce(&mut Ui, egui::Id, f32) -> R,
) -> R {
    let room =
        ui.available_width() - text_width(ui, label, egui::TextStyle::Body) - ui.spacing().item_spacing.x;
    if room >= needs {
        ui.horizontal(|ui| {
            let l = ui.label(label);
            add(ui, l.id, room)
        })
        .inner
    } else {
        let l = ui.label(label);
        add(ui, l.id, ui.available_width())
    }
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
