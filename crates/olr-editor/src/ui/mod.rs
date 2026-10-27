//! The editor's egui panels (spec §4; the layout of issue #8): the session
//! bar and the tool options bar on top, the tool rail on the left, the
//! right panel (its tabs, then the selection's properties or the level's),
//! the status bar, the canvas and the dialogs. The asset tab is laid out
//! as the skin editor (issue #78, `asset`). `show` draws
//! them from the state and returns the frame's actions, as
//! `olr_ui::screens::show` does; the host applies `olr_ui::theme`.

mod asset;
mod browser;
mod canvas;
mod dialogs;
mod elements;
mod icons;
mod level;
mod look;
mod previews;
mod properties;
mod rail;
mod status;
mod toolbar;

use std::collections::BTreeMap;

use egui::{Key, Modifiers, Ui};

pub use asset::NARROW;

use crate::state::{DocTab, EditorAction, EditorState, Selection};
use crate::tools::Tool;

/// The labels tests and 14c's e2e script find widgets by.
pub mod labels {
    // The session bar (issue #8): File ▾, View ▾ and Play's ▾ are named
    // without their arrow.
    pub const FILE: &str = "File";
    pub const VIEW: &str = "View";
    pub const PLAY_OPTIONS: &str = "Play options";
    /// The collision view: rock, dirt and background in their colours.
    pub const MATERIAL_VIEW: &str = "Collision view";
    /// Issue #43: the surface materials' tints, with a legend.
    pub const SURFACE_VIEW: &str = "Materials view";
    pub const LEGEND: &str = "Legend";
    pub const GRID: &str = "Grid";
    /// Issue #66: distant layers move with the view, as in play.
    pub const PREVIEW_PARALLAX: &str = "Preview parallax";
    // The rail's group buttons (`ToolGroup::name`); its other buttons are
    // named by their tool (`Tool::name`), Import by `IMPORT_IMAGE`.
    pub const DRAW: &str = "Draw";
    pub const PIXELS: &str = "Pixels";
    // The tool options bar's paint.
    pub const COLOUR: &str = "Colour";
    pub const FRONT_TILE: &str = "Front tile";
    pub const BACK_TILE: &str = "Back tile";
    // The right panel's tabs and properties.
    pub const ELEMENTS: &str = "Elements";
    pub const ASSETS: &str = "Assets";
    pub const SPAWNS: &str = "Spawns";
    pub const WHAT_IS_IT: &str = "What is it?";
    pub const PLACE: &str = "Place";
    pub const MAKE_PARTS_GLOW: &str = "Make parts glow…";
    pub const MORE: &str = "More";
    pub const FINE_TUNING: &str = "Fine tuning";
    pub const UNDO: &str = "Undo";
    pub const REDO: &str = "Redo";
    pub const SAVE: &str = "Save";
    pub const PLAY: &str = "Play";
    pub const ADD_LAYER: &str = "Add layer";
    // Layer folders (issue #61): the button beside Add layer and the
    // folder panel's, with their shortcuts.
    pub const NEW_FOLDER: &str = "New folder";
    pub const NEW_FOLDER_KEYS: &str = "Ctrl+G";
    pub const UNGROUP: &str = "Ungroup";
    pub const UNGROUP_KEYS: &str = "Ctrl+Shift+G";
    pub const IMPORT_IMAGE: &str = "Import image…";
    pub const OPEN: &str = "Open…";
    pub const CLOSE: &str = "Close";
    /// The canvas's accessible name (the e2e script drags on it).
    pub const CANVAS: &str = "Canvas";
    pub const BROWSER_PATH: &str = "Path";
    pub const CHOOSE: &str = "Choose";
    pub const SPLIT: &str = "Split into layers";
    pub const MERGE_DOWN: &str = "Merge down";
    pub const PAINT_AS: &str = "Paint as";
    pub const RESTORE: &str = "Restore";
    pub const TOLERANCE: &str = "Tolerance";
    pub const BY_COLOUR: &str = "By colour";
    pub const GLOW_SELECTION: &str = "Glow selection";
    /// The palette's button (plan 14e).
    pub const TEXTURES: &str = "Textures…";
    pub const IMPORT_TEXTURE: &str = "Import texture…";
    /// The crater brush's picker.
    pub const CRATER: &str = "Crater";
    /// The palette's swatches of the level theme's tiles.
    pub const THEME_FRONT: &str = "Theme front";
    pub const THEME_BACK: &str = "Theme back";
    /// Removes the chosen texture of the user's (the palette).
    pub const REMOVE: &str = "Remove";
    pub const PIXEL_SIZE: &str = "Pixel size";
    pub const COLOURS: &str = "Colours";
    pub const DITHER: &str = "Dither";
    pub const THRESHOLD: &str = "Threshold";
    pub const VISIBLE: &str = "Visible";
    pub const LOCKED: &str = "Locked";
    pub const NAME: &str = "Name";
    pub const DONT_SAVE: &str = "Don't save";
    pub const CANCEL: &str = "Cancel";
    // The look (plan 15a Task 18).
    pub const LOOK: &str = "Lighting";
    pub const ADD_LOOK: &str = "Add look";
    pub const REMOVE_LOOK: &str = "Remove look";
    pub const LOOK_AMBIENT: &str = "Ambient %";
    pub const LOOK_AMBIENT_COLOUR: &str = "Ambient colour";
    // Issue #10.
    pub const LOOK_OCCLUSION: &str = "Occlusion %";
    pub const LOOK_OCCLUSION_REACH: &str = "Occlusion reach";
    pub const LOOK_MAX_LIGHT: &str = "Max light %";
    pub const LOOK_STEPS: &str = "Light steps";
    pub const LOOK_SOFTNESS: &str = "Softness";
    pub const LOOK_SOAK: &str = "Light soak";
    pub const LOOK_FLOOR: &str = "Solid floor %";
    pub const LOOK_EXPOSURE: &str = "Exposure";
    pub const LOOK_CONTRAST: &str = "Contrast %";
    pub const LOOK_SATURATION: &str = "Saturation %";
    pub const LOOK_TINT: &str = "Tint";
    pub const ADD_GROUP: &str = "Add glow group";
    pub const GROUP_COLOUR: &str = "Group colour";
    pub const GROUP_INTENSITY: &str = "Group intensity %";
    pub const GROUP_REACH: &str = "Group reach";
    pub const ANIMATION: &str = "Animation";
    pub const UNLIMITED: &str = "unlimited";
    pub const GLOW: &str = "Glow";
    pub const GLOW_STRENGTH: &str = "Glow strength %";
    pub const GLOW_GROUP: &str = "Glow group";
    pub const CLEAR: &str = "Lets light through";
    // Distant (parallax) layers (issue #11).
    pub const DISTANT: &str = "Distant";
    pub const DEPTH: &str = "Depth";
    /// Shown while a distant layer is selected and no back pixel is
    /// see-through.
    pub const DISTANT_HINT: &str = "Nothing in front is see-through yet: erase parts of the background layer, or use Remove colour on it, to show this layer";
    /// The Glow switch's hint on a distant layer (issue #62): its light
    /// is cast from its reference placement.
    pub const DISTANT_GLOW_HINT: &str = "the whole layer glows and shows its art in the dark; its light falls on the level from where the layer lies with the camera at the level's centre, so a small bright spot (a far window) lights the level off its drawn place when the camera is far from the centre; a broad sky or sunset shows no difference";
    // Background glass (the background glass design).
    pub const GLASS: &str = "Glass";
    pub const OPACITY: &str = "Opacity %";
    pub const TINT: &str = "Tint %";
    pub const FROST: &str = "Frost";
    /// Shown under Glass while the level has no distant layer.
    pub const GLASS_HINT: &str =
        "Glass shows the distant layers behind it: make a background element Distant to see through it";
    /// Shown under Tint and Frost while the level has no lighting.
    pub const GLASS_LOOK_HINT: &str = "shows only in a lit level: add lighting to the level";
    /// A glow group's colour: each pixel's art at full strength, times
    /// the group's colour (the glass's light).
    pub const FILTERED: &str = "Filtered";
    pub const TOOL_GLOW: &str = "Glow brush";
    /// The Glow brush's and Glow selection's glow % (one value).
    pub const GLOW_STRENGTH_BRUSH: &str = "Glow %";
    pub const VIEW_LIT: &str = "Lit";
    pub const VIEW_FULL: &str = "Full bright";
    pub const VIEW_LIGHT: &str = "Light only";
    pub const CLOCK_PLAY: &str = "Play lights";
    pub const CLOCK_PAUSE: &str = "Pause lights";
    pub const CLOCK_SCRUB: &str = "Time ms";
    // The asset library (issue #9).
    pub const SAVE_AS_ASSET: &str = "Save as asset…";
    pub const IMPORT_ASSET: &str = "Import asset…";
    pub const REMOVE_ASSET: &str = "Remove asset";
    pub const ASSET_NAME: &str = "Asset name";
    pub const REPLACE: &str = "Replace";
    pub const KEEP_BOTH: &str = "Keep both";
    pub const BUILT_IN: &str = "Built-in";
    pub const YOURS: &str = "Yours";
    // The asset tab (issue #77).
    pub const LEVEL_TAB: &str = "Level";
    pub const ASSET_TAB: &str = "Asset";
    /// The Asset switch's hint while no asset is open.
    pub const ASSET_TAB_HINT: &str = "Open or make an asset from the Assets tab";
    /// The Assets tab's button beside a picked asset (issue #81's "Edit in
    /// level" is the level's own copies').
    pub const EDIT_LIBRARY_ASSET: &str = "Edit asset";
    pub const NEW_ASSET: &str = "New asset…";
    /// The asset's properties' size, its Width and Height fields.
    pub const ASSET_SIZE: &str = "Asset size";
    pub const WIDTH: &str = "Width";
    pub const HEIGHT: &str = "Height";
    pub const ART_SCALE: &str = "Art scale";
    /// The New asset dialog's button.
    pub const CREATE: &str = "Create";
    // The asset tab's layout (issue #78): the narrow window's switch
    // between its side panels, the layers' heading, the previews and the
    // unsaved mark.
    pub const TOOLS_TAB: &str = "Tools";
    pub const LAYERS_TAB: &str = "Layers";
    pub const LAYERS: &str = "Layers";
    pub const PREVIEW_LIGHT: &str = "Light";
    pub const PREVIEW_DARK: &str = "Dark";
    pub const UNSAVED: &str = "Unsaved changes";
    // Surface materials (issue #43).
    pub const MATERIALS: &str = "Materials";
    pub const ADD_MATERIAL: &str = "Add from library";
    pub const SURFACE: &str = "Surface";
    pub const BACK_SURFACE: &str = "Back surface";
    /// A surface picker's "no material".
    pub const NO_SURFACE: &str = "None";
    pub const TOOL_SURFACE: &str = "Surface brush";
    // Directional glow and haze (the directional-glow design).
    pub const DIRECTIONAL: &str = "Directional";
    pub const DIRECTION: &str = "Direction °";
    pub const SPREAD: &str = "Spread °";
    /// The lighting's haze: 50 is the factory's look, 100 twice as hazy.
    pub const HAZE: &str = "Haze %";
    pub const TOOL_HAZE: &str = "Haze brush";
    /// The Haze brush's strength: the haze map's value, percent.
    pub const HAZE_STRENGTH_BRUSH: &str = "Haze strength %";
    /// Shown instead of Directional on a dirt layer.
    pub const DIRECTIONAL_DIRT_HINT: &str =
        "A dirt layer cannot glow in a direction: dirt digs away. Use a rock or background layer.";
    /// The Surface brush's material picker, its "the element's default",
    /// and its front and back switch.
    pub const BRUSH_MATERIAL: &str = "Material";
    pub const DEFAULT_SURFACE: &str = "Element's";
    pub const SURFACE_FRONT: &str = "Front";
    pub const SURFACE_BACK: &str = "Back";

    /// A material's Remove button's name (issue #43).
    pub fn remove_material(name: &str) -> String {
        format!("Remove {name}")
    }

    /// A folder row's fold arrow, eye and lock in the Elements list (issue
    /// #61), by the folder's name.
    pub fn fold(name: &str) -> String {
        format!("Fold {name}")
    }

    pub fn folder_visible(name: &str) -> String {
        format!("{VISIBLE}: {name}")
    }

    pub fn folder_locked(name: &str) -> String {
        format!("{LOCKED}: {name}")
    }

    /// A material's swatch's name in the Materials view's legend.
    pub fn legend_tint(name: &str) -> String {
        format!("{name} tint")
    }
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
    /// The tab, the open document's directory and the asset tab's asset:
    /// drafts belong to them.
    dir: Option<(crate::state::DocTab, std::path::PathBuf, Option<crate::catalog::AssetKey>)>,
    /// The draft keys of the fields drawn this frame.
    drawn: std::collections::BTreeSet<String>,
    /// The file browser's typed path and selection, while it is open.
    browser: Option<BrowserDraft>,
    /// The palette's swatches by key (`builtin/<i>`, `user/<name>`,
    /// `theme/<id>/front|back`), made when first drawn; those no longer
    /// listed, or of an older user library, are dropped.
    swatches: BTreeMap<String, egui::TextureHandle>,
    /// The user library's generation the `user/` swatches were made from.
    swatch_generation: u64,
    /// The Assets tab's thumbnails by asset, of the library's generation
    /// `asset_generation` (issue #9).
    asset_thumbs: BTreeMap<crate::catalog::AssetKey, egui::TextureHandle>,
    asset_generation: u64,
    /// The asset name dialog's field, while it is open.
    asset_name: Option<String>,
    /// The New asset dialog's fields, while it is open (issue #77).
    new_asset: Option<crate::asset_tab::NewAsset>,
    /// Issue #78: the panels fit their width (the asset tab's), their rows
    /// wrapping or a label above its field where they do not.
    fit: bool,
    /// The asset tab's side panel a narrow window shows.
    side: asset::Side,
    /// The focus last seen, and the passes left to follow it (the asset
    /// tab's side panels scroll to it).
    seen_focus: Option<egui::Id>,
    follow: u8,
    /// The asset tab's previews, light and dark, for (the canvas's
    /// document generation, its bake generation).
    previews: Option<((u64, u64), [egui::TextureHandle; 2])>,
}

/// The file browser's drafts, for the directory shown.
#[derive(Default)]
struct BrowserDraft {
    dir: Option<std::path::PathBuf>,
    path: String,
    selected: Option<String>,
    /// The preview's texture, for the entry it shows.
    preview: Option<(String, egui::TextureHandle)>,
}

impl EditorData {
    /// The canvas's rectangle as last drawn.
    pub fn canvas_rect(&self) -> Option<egui::Rect> {
        self.canvas_rect
    }

    /// The palette's swatches held, by key (tests).
    #[doc(hidden)]
    pub fn swatch_keys(&self) -> Vec<String> {
        self.swatches.keys().cloned().collect()
    }

    /// The bake generation the asset tab's previews were made at (tests).
    #[doc(hidden)]
    pub fn preview_generation(&self) -> Option<u64> {
        self.previews.as_ref().map(|((_, g), _)| *g)
    }
}

/// Draws the editor into `ui` and returns the actions of this frame. The
/// properties' commits come first: a field's last value is in before a
/// toolbar or list click that ended its edit (Undo, Save) acts.
pub fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData) -> Vec<EditorAction> {
    let Some(doc) = state.active_document() else {
        ui.centered_and_justified(|ui| ui.label("No level is open."));
        return Vec::new();
    };
    // Drafts belong to the open level, or the asset tab's asset (issue
    // #77): a switch or another document drops them.
    let owner = (state.doc_tab(), doc.dir().to_path_buf(), state.asset_key().cloned());
    if data.dir.as_ref() != Some(&owner) {
        let canvas = (data.canvas_rect, data.canvas_size);
        let thumbs = (std::mem::take(&mut data.asset_thumbs), data.asset_generation);
        let side = data.side;
        *data = EditorData { dir: Some(owner), side, ..EditorData::default() };
        (data.canvas_rect, data.canvas_size) = canvas;
        (data.asset_thumbs, data.asset_generation) = thumbs;
    }
    data.drawn.clear();
    data.fit = state.doc_tab() == DocTab::Asset;
    // A popup open (a flyout, a menu) takes Escape for itself.
    let popup = egui::Popup::is_any_open(ui.ctx());
    // A text field's Escape (it drops the draft) leaves the selection.
    let typing = ui.ctx().egui_wants_keyboard_input();
    let (mut toolbar, mut list, mut fields, mut canvas, mut dialogs) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    if state.doc_tab() == DocTab::Asset {
        asset::show(ui, state, data, [&mut toolbar, &mut list, &mut fields, &mut canvas]);
    } else {
        egui::Panel::top("olr-editor-session").show(ui, |ui| toolbar::session(ui, state, data, &mut toolbar));
        egui::Panel::top("olr-editor-options")
            .show(ui, |ui| toolbar::options_bar(ui, state, data, &mut toolbar));
        egui::Panel::bottom("olr-editor-status").show(ui, |ui| status::show(ui, state));
        egui::Panel::left("olr-editor-rail")
            .exact_size(rail::RAIL_WIDTH)
            .resizable(false)
            .show(ui, |ui| rail::show(ui, state, &mut toolbar));
        // At least its first width: a narrow panel (a group's) never lets the
        // canvas grow under the pointer.
        egui::Panel::right("olr-editor-panel").default_size(260.0).min_size(260.0).show(ui, |ui| {
            elements::show(ui, state, data, &mut list);
            ui.separator();
            egui::ScrollArea::vertical().id_salt("olr-editor-properties").show(ui, |ui| {
                match state.selection() {
                    Selection::None => level::show(ui, state, data, &mut fields),
                    _ => properties::show(ui, state, data, &mut fields),
                }
            })
        });
        // No fill: the host draws the bake under the canvas (plan 14c).
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| canvas::show(ui, state, data, &mut canvas));
    }
    browser::show(ui.ctx(), state, data, &mut dialogs);
    dialogs::show(ui.ctx(), state, data, &mut dialogs);
    // A draft whose field was not drawn this frame is dropped, never
    // committed later to whatever shows that field again.
    let drawn = std::mem::take(&mut data.drawn);
    data.drafts.retain(|k, _| drawn.contains(k));
    data.texts.retain(|k, _| drawn.contains(k));
    data.drawn = drawn;
    let mut out = fields;
    shortcuts(ui, state, popup || typing, &mut out);
    out.extend(list.into_iter().chain(canvas).chain(toolbar).chain(dialogs));
    out
}

/// The shortcuts, when no text field has the keyboard and no dialog is
/// open; Escape not while a popup was open (it closes the popup) nor a
/// text field had the keyboard as the frame began (it drops the draft):
/// `popup`.
fn shortcuts(ui: &Ui, state: &EditorState, popup: bool, out: &mut Vec<EditorAction>) {
    // Not while typing, nor while a widget is dragged (its draft would
    // outlive the change a shortcut makes).
    if state.dialog().is_some()
        || state.browser().is_some()
        || ui.ctx().egui_wants_keyboard_input()
        || ui.ctx().dragged_id().is_some()
    {
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
        if !popup && state.placing() && i.consume_key(Modifiers::NONE, Key::Escape) {
            out.push(EditorAction::CancelPlacing);
        }
        if !popup && state.pixel_rect().is_some() && i.consume_key(Modifiers::NONE, Key::Escape) {
            out.push(EditorAction::DropPixels);
        }
        // Then up a level, and at the top level no selection (issue #61).
        if !popup && i.consume_key(Modifiers::NONE, Key::Escape) {
            out.push(EditorAction::SelectParent);
        }
        // Ctrl+Shift+G before Ctrl+G (`consume_key` ignores an extra
        // Shift), both before the tools' G (Fill).
        if i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::G) {
            out.push(EditorAction::Ungroup);
        }
        if i.consume_key(Modifiers::COMMAND, Key::G) {
            out.push(EditorAction::GroupSelection);
        }
        // Ctrl+C, X and V: as keys, or as the window's copy, cut and paste
        // events (plan 14e R9); one action each per frame.
        let clipboard =
            [(Key::C, EditorAction::Copy), (Key::X, EditorAction::Cut), (Key::V, EditorAction::Paste)];
        for (key, action) in clipboard {
            let event = i.events.iter().any(|e| {
                matches!(
                    (e, &action),
                    (egui::Event::Copy, EditorAction::Copy)
                        | (egui::Event::Cut, EditorAction::Cut)
                        | (egui::Event::Paste(_), EditorAction::Paste)
                )
            });
            if i.consume_key(Modifiers::COMMAND, key) || event {
                out.push(action);
            }
        }
        if i.consume_key(Modifiers::COMMAND, Key::E)
            && let Selection::Element(id) = state.selection()
        {
            out.push(EditorAction::MergeDown(id.clone()));
        }
        for tool in Tool::ALL.into_iter().filter(|t| state.offers(*t)) {
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

/// Names `r` `label` for AccessKit, whatever it shows (tests and the e2e
/// script find widgets by name: "File ▾" is "File", the rail's Glow
/// "Glow brush").
pub(crate) fn relabel(r: &egui::Response, typ: egui::WidgetType, selected: Option<bool>, label: &str) {
    let enabled = r.enabled();
    r.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(typ, enabled, label);
        info.selected = selected;
        info
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
    number_by(ui, data, (key, label), current, range, 1)
}

/// [`number`] in steps of `step` (at least 1): a value commits as the
/// step at or below it (from the range's start), and only when that step
/// is not `current`'s (final review M3: a glow beam's direction is stored
/// in 2° steps, so an odd degree adds no step of history).
pub(crate) fn number_by(
    ui: &mut Ui,
    data: &mut EditorData,
    (key, label): (&str, &str),
    current: i64,
    range: std::ops::RangeInclusive<i64>,
    step: i64,
) -> Option<i64> {
    let step = step.max(1);
    let lo = *range.start();
    let snap = |v: i64| lo + (v - lo).div_euclid(step) * step;
    data.drawn.insert(key.into());
    let mut v = data.drafts.get(key).copied().unwrap_or(current as f64);
    // The value's box, as wide as the range's widest value.
    let widest = [*range.start(), *range.end()].map(|n| n.to_string()).into_iter().max_by_key(String::len);
    let needs = text_width(ui, &widest.unwrap_or_default(), egui::TextStyle::Button)
        + 2.0 * ui.spacing().button_padding.x;
    let needs = needs.max(ui.spacing().interact_size.x);
    beside(ui, data.fit, label, needs, |ui, l| {
        let r = ui
            .add(
                egui::DragValue::new(&mut v)
                    .range(*range.start() as f64..=*range.end() as f64)
                    .speed(0.25 * step as f64)
                    .max_decimals(0)
                    .update_while_editing(false),
            )
            .labelled_by(l);
        if r.dragged() || r.has_focus() {
            data.drafts.insert(key.into(), v);
            return None;
        }
        data.drafts.remove(key);
        let v = snap((v.round() as i64).clamp(*range.start(), *range.end()));
        (v != snap(current)).then_some(v)
    })
}

/// The width (points) of `text` in `style`, unwrapped.
pub(crate) fn text_width(ui: &Ui, text: &str, style: egui::TextStyle) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
        .size()
        .x
}

/// `label` and the field `add` makes (given the label's id), side by side;
/// when `fit` (issue #78, the skin editor's `beside`) and the field has
/// not `needs` points beside the label, the label above it.
pub(crate) fn beside<R>(
    ui: &mut Ui,
    fit: bool,
    label: &str,
    needs: f32,
    add: impl FnOnce(&mut Ui, egui::Id) -> R,
) -> R {
    let room =
        ui.available_width() - text_width(ui, label, egui::TextStyle::Body) - ui.spacing().item_spacing.x;
    if !fit || room >= needs {
        ui.horizontal(|ui| {
            let l = ui.label(label);
            add(ui, l.id)
        })
        .inner
    } else {
        let l = ui.label(label);
        add(ui, l.id)
    }
}

/// A labelled slider of whole numbers (`key` names its draft) that commits
/// once, when the drag stops; `low` and `high` name its ends. Returns the
/// new value then.
pub(crate) fn slider(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    label: &str,
    ends: (&str, &str),
    current: i64,
    range: std::ops::RangeInclusive<i64>,
) -> Option<i64> {
    slider_shown(ui, data, key, (label, ends), current, range, None)
}

/// [`slider`] showing its value as `shown` gives it (the background glass
/// design: frost in tenths shown in world pixels).
pub(crate) fn slider_shown(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    (label, (low, high)): (&str, (&str, &str)),
    current: i64,
    range: std::ops::RangeInclusive<i64>,
    shown: Option<fn(f64) -> String>,
) -> Option<i64> {
    data.drawn.insert(key.into());
    let mut v = data.drafts.get(key).copied().unwrap_or(current as f64);
    let gap = ui.spacing().item_spacing.x;
    let ends = text_width(ui, low, egui::TextStyle::Body) + text_width(ui, high, egui::TextStyle::Body);
    // The value's box: the widest value shown.
    let widest = [*range.start(), *range.end()]
        .map(|n| shown.map_or_else(|| n.to_string(), |f| f(n as f64)))
        .into_iter()
        .map(|t| text_width(ui, &t, egui::TextStyle::Button))
        .fold(0.0, f32::max);
    let value = (widest + 2.0 * ui.spacing().button_padding.x).max(ui.spacing().interact_size.x) + gap;
    let fit = data.fit;
    let add = |ui: &mut Ui, l: egui::Id, v: &mut f64| {
        ui.horizontal(|ui| {
            if fit {
                // The track takes the room left, at most egui's.
                let track = ui.available_width() - ends - 3.0 * gap - value;
                ui.spacing_mut().slider_width = ui.spacing().slider_width.min(track.max(SLIDER_MIN / 2.0));
            }
            ui.weak(low);
            let mut s =
                egui::Slider::new(v, *range.start() as f64..=*range.end() as f64).integer().show_value(true);
            if let Some(shown) = shown {
                s = s.custom_formatter(move |v, _| shown(v));
            }
            let r = ui.add(s).labelled_by(l);
            ui.weak(high);
            r
        })
        .inner
    };
    // Beside its label while the whole row fits, else under it, the track
    // shortened to the room left.
    let needs = ends + 3.0 * gap + value + ui.spacing().slider_width;
    let r = beside(ui, fit, label, needs, |ui, l| add(ui, l, &mut v));
    {
        if r.dragged() {
            data.drafts.insert(key.into(), v);
            return None;
        }
        data.drafts.remove(key);
        let v = (v.round() as i64).clamp(*range.start(), *range.end());
        (v != current).then_some(v)
    }
}

/// A fitted slider's narrowest track (points).
const SLIDER_MIN: f32 = 60.0;

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
    let mut v = data.texts.get(key).cloned().unwrap_or_else(|| current.to_string());
    beside(ui, data.fit, label, TEXT_MIN, |ui, l| {
        let edit = if multiline {
            egui::TextEdit::multiline(&mut v).desired_rows(3)
        } else {
            egui::TextEdit::singleline(&mut v)
        };
        let r = ui.add(edit).labelled_by(l);
        if r.has_focus() {
            data.texts.insert(key.into(), v);
            return None;
        }
        data.texts.remove(key);
        // Escape drops what was typed.
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        (r.lost_focus() && !escaped && v != current).then_some(v)
    })
}

/// A fitted text field's narrowest beside its label (points).
const TEXT_MIN: f32 = 120.0;
