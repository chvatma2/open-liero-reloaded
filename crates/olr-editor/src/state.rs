//! The editor's state (spec §4, `olr-ui`'s pattern):
//! `EditorState::update(EditorAction) -> Vec<EditorCommand>` is pure. A
//! host carries out the commands (open, save, play) and answers with
//! actions (`Opened`, `Saved`, `PlayEnded`).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use olr_data::image::Rgba;
use olr_data::level::Material;
use olr_edit::compose::Rect;
use olr_edit::element::Property;
use olr_edit::level::StampMask;
use olr_edit::pack::Pack;
use olr_edit::textures::Texture;
use olr_format::levels::{ElementMaterial, LevelSource};

use crate::asset_tab::{BUILT_IN_NOTE, NewAsset};
use crate::browser::{Browser, DirEntry, Purpose};
use crate::catalog::{AssetEntry, AssetKey, Catalog, EditorSettings, TextureEntry};
use crate::clipboard::{Clipboard, PASTE_STEP};
use crate::document::{DocError, Document, Problem};
use crate::history::{Edit, History, LevelMeta};
use crate::ops::Change;
use crate::pointer::Drag;
use crate::tools::{Brush, MAX_BRUSH, PaintChoice, PointerKind, TextureRef, Tool, ToolGroup};
use crate::view::View;

/// A selection by colour takes a glow only (2026-10-19).
const BY_COLOUR_TAKES_A_RECTANGLE: &str =
    "copy, cut and delete take a rectangle selection (Select pixels without By colour)";

/// What is selected.
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    None,
    Element(String),
    /// A layer folder, standing for its whole run (issue #61).
    Folder(String),
    /// A level object, by index.
    Object(usize),
    /// Two elements or folders or more, in stack order, bottom first (plan
    /// 14e R8; a folder by where its run starts, issue #61); none inside
    /// another.
    Many(Vec<String>),
}

impl Selection {
    /// Whether element or folder `id` is selected itself, alone or with
    /// others (a folder's members are not).
    pub fn has(&self, id: &str) -> bool {
        match self {
            Selection::Element(e) | Selection::Folder(e) => e == id,
            Selection::Many(ids) => ids.iter().any(|e| e == id),
            _ => false,
        }
    }

    /// The selected elements' and folders' ids (none for an object).
    pub fn ids(&self) -> Vec<String> {
        match self {
            Selection::Element(e) | Selection::Folder(e) => vec![e.clone()],
            Selection::Many(ids) => ids.clone(),
            _ => Vec::new(),
        }
    }
}

/// Added to a refusal of a tool needing one element while a folder is
/// selected (issue #61).
pub const DOUBLE_CLICK_INTO: &str =
    "a folder is selected: double-click into the object (or Alt+click) to select one of its elements";

/// The right panel's tabs (issue #8): the element list, the asset library
/// (issue #9) and the spawns. The tab follows the selection; otherwise only
/// a click changes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelTab {
    #[default]
    Elements,
    Assets,
    Spawns,
}

/// The status bar's contents.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Live diagnostics (ruling R6), errors first.
    pub problems: Vec<Problem>,
    /// The last refusal or result.
    pub message: Option<String>,
    /// The pointer's world pixel.
    pub cursor: Option<(i32, i32)>,
    /// The material under the pointer (inside the level only).
    pub material: Option<Material>,
}

/// A modal question.
#[derive(Clone, Debug, PartialEq)]
pub enum Dialog {
    /// Unsaved changes before `then`: Save, Don't save, Cancel.
    Unsaved { then: Pending },
    /// An art-scale change rescales drawn and raster layers (spec §5): Change, Cancel.
    ArtScale { to: u32 },
    /// An error to acknowledge: OK.
    Error(String),
    /// Removing glow group `id` takes the glow off its elements: Remove, Cancel.
    RemoveGroup { id: String },
    /// Save as asset's name (issue #9), the field starting at `suggested`:
    /// Save (`NameAsset`), Cancel.
    AssetName { suggested: String },
    /// An asset of the user's (folder `id`) is named `name` already:
    /// Replace (`Confirm`), Keep both (`KeepBoth`), Cancel.
    AssetExists { name: String, id: String },
    /// Removing the user's asset `id` (named `name`): Remove, Cancel.
    RemoveAsset { id: String, name: String },
    /// New asset… (issue #77): its name, size and art scale, the fields
    /// starting at these: Create (`MakeAsset`), Cancel.
    NewAsset(NewAsset),
}

/// The surface materials' tints in the Materials view and its legend
/// (issue #43): value `v` (`materials[v - 1]`) shows `SURFACE_TINTS[(v -
/// 1) % 12]`; 0, no material, shows the art greyed.
pub const SURFACE_TINTS: [Rgba; 12] = [
    Rgba::rgb(230, 25, 75),
    Rgba::rgb(60, 180, 75),
    Rgba::rgb(255, 225, 25),
    Rgba::rgb(0, 130, 200),
    Rgba::rgb(245, 130, 48),
    Rgba::rgb(145, 30, 180),
    Rgba::rgb(70, 240, 240),
    Rgba::rgb(240, 50, 230),
    Rgba::rgb(210, 245, 60),
    Rgba::rgb(250, 190, 212),
    Rgba::rgb(0, 128, 128),
    Rgba::rgb(170, 110, 40),
];

/// The Materials view's tint of map value `v`; `None` for 0 (no material).
pub fn surface_tint(v: u8) -> Option<Rgba> {
    v.checked_sub(1).map(|i| SURFACE_TINTS[usize::from(i) % SURFACE_TINTS.len()])
}

/// The look preview's clock (plan 15a Task 17; spec §The level editor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClockAction {
    Play,
    Pause,
    /// To this time, ms (a time that is not finite or below 0 is ignored).
    Scrub(f64),
}

/// What waits on the unsaved-changes dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    Open(PathBuf),
    New(NewLevel),
    Close,
    /// Issue #77: the asset tab opens `asset`, read from the library's
    /// `key`, in place of the asset open there.
    EditAsset {
        key: AssetKey,
        asset: Box<olr_edit::asset::Asset>,
    },
    /// The asset tab makes this new asset in place of the asset open there.
    NewAsset(NewAsset),
    /// Close asks for the asset tab's changes first, then the level's.
    CloseAsset,
}

/// The editor's two documents (issue #77): the level and the asset tab's
/// library asset, each its own session (document, history, unsaved mark,
/// selection, view, tool and options, status).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocTab {
    #[default]
    Level,
    Asset,
}

/// A tab's own state (issue #77). The shown tab's lives in `EditorState`'s
/// fields; the other tab's waits in `EditorState::other`, and they swap.
pub(crate) struct Session {
    doc: Option<Document>,
    history: History,
    dirty: bool,
    status: Status,
    selection: Selection,
    saves: VecDeque<(u64, Option<Pending>)>,
    tool: Tool,
    prev_tool: Tool,
    brush: Brush,
    paint: PaintChoice,
    paint_material: Option<ElementMaterial>,
    restore: bool,
    tolerance: u8,
    fill_tolerance: u8,
    view: View,
    fit_pending: bool,
    surface_paint: Option<String>,
    surface_back: bool,
    drag: Option<Drag>,
    changes: u64,
    pixel_rect: Option<(String, Rect)>,
    pixel_region: Option<Arc<Vec<bool>>>,
    pixel_world: Option<Rect>,
    glow_by_colour: bool,
    select_by_colour: bool,
    glow_strength: u8,
    haze_strength: u8,
    last_tools: [Tool; 2],
    tab: PanelTab,
    distant_sent: Option<u64>,
    focus: Option<String>,
}

impl Session {
    /// A tab with nothing open, every tool option at its start.
    fn new() -> Session {
        Session {
            doc: None,
            history: History::new(),
            dirty: false,
            status: Status::default(),
            selection: Selection::None,
            saves: VecDeque::new(),
            tool: Tool::Select,
            prev_tool: Tool::Select,
            brush: Brush::default(),
            paint: PaintChoice::default(),
            paint_material: None,
            restore: false,
            tolerance: DEFAULT_TOLERANCE,
            fill_tolerance: DEFAULT_FILL_TOLERANCE,
            view: View { zoom: 2, offset: [0.0, 0.0] },
            fit_pending: false,
            surface_paint: None,
            surface_back: false,
            drag: None,
            changes: 0,
            pixel_rect: None,
            pixel_region: None,
            pixel_world: None,
            glow_by_colour: false,
            select_by_colour: false,
            glow_strength: 100,
            haze_strength: 100,
            last_tools: ToolGroup::ALL.map(ToolGroup::first),
            tab: PanelTab::Elements,
            distant_sent: None,
            focus: None,
        }
    }
}

/// Swaps a session's fields between `$a` and `$b` (an `EditorState` and a
/// `Session`): every field of `Session`.
macro_rules! swap_session {
    ($a:expr, $b:expr) => {
        swap_session!(@ $a, $b; doc, history, dirty, status, selection, saves, tool, prev_tool, brush, paint,
            paint_material, restore, tolerance, fill_tolerance, view, fit_pending, surface_paint, surface_back,
            drag, changes, pixel_rect, pixel_region, pixel_world, glow_by_colour, select_by_colour,
            glow_strength, haze_strength, last_tools, tab, distant_sent, focus)
    };
    (@ $a:expr, $b:expr; $($f:ident),*) => {
        $(std::mem::swap(&mut $a.$f, &mut $b.$f);)*
    };
}

/// A new level pack to make.
#[derive(Clone, Debug, PartialEq)]
pub struct NewLevel {
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
    pub size: (u32, u32),
    pub theme: String,
}

/// A dialog's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Save,
    Discard,
    Cancel,
    Confirm,
    /// An asset named as one of the user's: saved beside it, renamed.
    KeepBoth,
}

/// Mouse, keyboard and widget events, and the host's answers.
#[derive(Debug)]
pub enum EditorAction {
    // The host's answers.
    Opened(Box<Document>),
    OpenFailed(String),
    /// A save's warnings, or why it failed.
    Saved(Result<Vec<String>, String>),
    PlayEnded,
    /// As `Opened`, but the document holds unsaved changes (a recovery
    /// copy reopened, plan 14c R7).
    Recovered(Box<Document>),
    /// A directory's entries (`List`'s answer), or why it cannot be read.
    Listed {
        dir: PathBuf,
        entries: Result<Vec<DirEntry>, String>,
    },
    /// Why the image `ReadImage` asked for cannot be imported.
    ImportFailed(String),
    /// Something the host could not do for the editor (a Play that could
    /// not start): shown as an error to acknowledge.
    Failed(String),
    /// What the host has to say in passing (a recovery copy that could not
    /// be written): the status bar's message.
    Note(String),
    /// The user's texture library as it now is (plan 14e R2): the answer
    /// to `AddTexture` and `RemoveTexture`.
    TexturesChanged(Vec<TextureEntry>),
    /// The asset library as it now is (issue #9): the built-in assets, then
    /// the user's; the answer to `SaveAsset`, `ImportAsset` and
    /// `RemoveAsset`.
    AssetsChanged(Vec<AssetEntry>),
    /// An asset was saved or imported into the user's library: it is
    /// selected in the Assets tab.
    AssetSaved(AssetKey),
    /// The asset `ReadAsset` asked for, to place at world pixel `at`, or why
    /// it cannot be read.
    AssetRead {
        at: (i32, i32),
        asset: Result<Box<olr_edit::asset::Asset>, String>,
    },
    // The file browser (R9).
    /// Opens the browser at the remembered folder, else the home directory.
    Browse(Purpose),
    /// A folder clicked, `..`, or a typed path (relative to the browser's).
    BrowseTo(PathBuf),
    /// An entry chosen: a file to import, or a pack folder to open (another
    /// folder is entered).
    Pick(String),
    /// An image selected in the import's browser: its preview, please.
    PreviewImage(String),
    /// The host's preview of an image (`ReadPreview`): a small copy and the
    /// image's own size, or why there is none.
    Previewed {
        path: PathBuf,
        image: Result<(olr_data::image::RgbaImage, (u32, u32)), String>,
    },
    CloseBrowser,
    // Files.
    Open(PathBuf),
    New(NewLevel),
    Save,
    Close,
    Choose(Choice),
    // Elements and objects.
    ImportImage {
        file_name: String,
        bytes: Vec<u8>,
    },
    AddLayer,
    Select(Selection),
    Set(String, Property),
    /// One property on each of several elements, one step (plan 14e R8).
    SetMany(Vec<String>, Property),
    /// Ctrl+click: element or folder `id` in or out of the selection.
    SelectToggle(String),
    /// Shift+click: the Elements list's rows shown from the single
    /// selection's to `id`'s (a folder row standing for its folder).
    SelectRange(String),
    /// Escape (issue #61): a folder's member selects its folder; at the top
    /// level the selection clears.
    SelectParent,
    /// Ctrl+G: the selected elements and folders into a new folder, which is
    /// selected; one step.
    GroupSelection,
    /// Ctrl+Shift+G: the selected folder's members take its place; one step.
    Ungroup,
    /// A field of folder `id` (the list's eye, lock and fold arrow, the
    /// properties); one step.
    SetFolder(String, crate::folder_ops::FolderField),
    /// A row dragged in the Elements list and dropped (issue #61); one step.
    DropRow {
        id: String,
        at: crate::folder_ops::RowDrop,
    },
    /// Deletes the selected element(s) or object.
    Remove,
    /// Copies the selected elements to the editor's clipboard (plan 14e R6).
    Copy,
    /// Copies, then deletes them: one step.
    Cut,
    /// Pastes the clipboard on top, selected: one step.
    Paste,
    /// Drops the pixel rectangle (Escape; plan 14e R7).
    DropPixels,
    /// Splits raster element `id` into background, dirt and rock layers.
    Split(String),
    MoveElement {
        id: String,
        to: usize,
    },
    // Level.
    Meta(LevelMeta),
    ArtScale(u32),
    Resize {
        size: (u32, u32),
        offset: (i32, i32),
        material: Material,
        colour: Rgba,
    },
    Undo,
    Redo,
    // Play.
    Play,
    PlayMod(String),
    PlayBots(u8),
    // Canvas and tools.
    Tool(Tool),
    Brush(Brush),
    Paint(PaintChoice),
    /// What painting lays (plan 14d R6): none, the layer's own material.
    PaintMaterial(Option<ElementMaterial>),
    /// Whether the eraser restores a picture's masked pixels.
    Restore(bool),
    /// The chosen tool's tolerance, percent (0 to 100): Remove colour's,
    /// or with Fill chosen, Fill's.
    Tolerance(u8),
    /// Removes a texture from the user's library (plan 14e R2).
    RemoveTexture(String),
    /// Merges element `id` into the drawn layer below it (Ctrl+E).
    MergeDown(String),
    /// A pointer event on the canvas, in world pixels (ruling R7); `toggle`
    /// is Ctrl (plan 14e R8).
    Pointer {
        kind: PointerKind,
        world: (i32, i32),
        shift: bool,
        toggle: bool,
    },
    /// A double click on the canvas (issue #61): with Select, one level
    /// deeper into the selected folder toward the element under it.
    DoubleClick {
        world: (i32, i32),
    },
    /// An Alt+press on the canvas (issue #61): with Select, the element under
    /// it itself, at any depth, then as a press; another tool takes it as a
    /// press.
    AltDown {
        world: (i32, i32),
    },
    /// Moves the selected image's box (the arrow keys).
    Nudge {
        dx: i32,
        dy: i32,
    },
    /// The canvas's size in points.
    Canvas([f32; 2]),
    Pan([f32; 2]),
    Zoom {
        steps: i32,
        about: [f32; 2],
    },
    MaterialView(bool),
    Grid(bool),
    /// View ▾ Preview parallax (issue #66): distant layers move with the
    /// view as in play; off, they show where they are placed.
    PreviewParallax(bool),
    // Surface materials (issue #43).
    /// The Materials view: the canvas tints each pixel by its surface
    /// material (front, or back where the front is carved), with a legend.
    SurfaceView(bool),
    /// Copies the library's material (`Catalog::materials`, by where it is
    /// and its id) into the level's materials; one step.
    AddMaterial {
        builtin: bool,
        id: String,
    },
    /// Removes the level's material `id` (refused while in use, naming the
    /// elements using it); one step.
    RemoveMaterial(String),
    /// What the Surface brush paints: one of the level's materials, by id;
    /// `None` the element's default.
    SurfacePaint(Option<String>),
    /// The Surface brush paints the selected element's back art (when it
    /// has back art), not its own pixels.
    SurfaceBack(bool),
    // The look (plan 15a Task 17).
    /// Element `id`'s glow; `None` removes it. A glow naming no group of the
    /// look gets a new one named after the element.
    SetGlow {
        id: String,
        glow: Option<olr_format::levels::ElementGlow>,
    },
    /// The Look panel's commit: the settings and groups (`None` removes it).
    SetLook(Option<olr_format::look::LookSource>),
    AddGroup(olr_format::look::GlowGroupSource),
    SetGroup {
        id: String,
        group: olr_format::look::GlowGroupSource,
    },
    /// Removes glow group `id` (asks first: its elements lose their glow).
    RemoveGroup(String),
    /// The glow brush's strength, percent (0 erases).
    GlowStrength(u8),
    /// The Haze brush's strength, percent (0 clears the haze).
    HazeStrength(u8),
    /// The Glow brush lights the connected pixels alike in colour.
    GlowByColour(bool),
    /// Select pixels picks the connected pixels alike in colour (Ctrl adds).
    SelectByColour(bool),
    /// The pixel selection's glow, percent: those pixels of its layer glow
    /// (a layer without a glow gets one), one step.
    GlowSelection(u8),
    /// What the canvas shows: lit, full bright or the light alone.
    SetViewMode(olr_look::ViewMode),
    Clock(ClockAction),
    // The asset library (issue #9).
    /// Saves the selected elements, or the pixel rectangle cut from every
    /// visible layer, as an asset: asks its name first.
    SaveAsAsset,
    /// The name dialog's Save.
    NameAsset(String),
    /// A thumbnail clicked: the asset is selected (`None`: none) and the
    /// next press on the canvas places it.
    PickAsset(Option<AssetKey>),
    /// Escape: the picked asset is no longer placed by a press.
    CancelPlacing,
    /// Places asset `key` with its top-left at world pixel `at` (a
    /// thumbnail dropped on the canvas, or a press with one picked).
    PlaceAsset {
        key: AssetKey,
        at: (i32, i32),
    },
    /// Removes the user's asset `id` (asks first).
    RemoveAsset(String),
    // The asset tab (issue #77).
    /// The session bar's Level | Asset switch (Asset once one is open).
    ShowTab(DocTab),
    /// Edit asset: reads library asset `key` to open in the asset tab.
    EditAsset(AssetKey),
    /// The asset `ReadAssetToEdit` asked for, or why it cannot be read.
    AssetToEdit {
        key: AssetKey,
        asset: Result<Box<olr_edit::asset::Asset>, String>,
    },
    /// New asset…: asks its name, size and art scale.
    NewAssetDialog,
    /// The New asset dialog's Create.
    MakeAsset(NewAsset),
    // The layout (issue #8).
    /// The right panel's tab, by hand.
    Tab(PanelTab),
    /// Opens or folds the properties' "More" (remembered in the settings).
    ShowMore(bool),
    /// Opens or folds the lighting's "Fine tuning" (remembered too).
    ShowFineTuning(bool),
}

/// What the host carries out.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorCommand {
    /// Re-upload this world rectangle of the bake (the canvas, in 14c).
    Upload(Rect),
    /// The level's distant layers (issue #11): their images, places and
    /// depths, sent again whenever they change (`Renderer::set_distant`).
    Distant(Vec<olr_data::level::DistantLayer>),
    /// Read the pack at this path (`pack::load`) and answer `Opened` /
    /// `OpenFailed` (`Document::open` with the catalog).
    Open(PathBuf),
    /// Make a new level pack (`olr_edit::new::new_pack`, `Template::Level`),
    /// then open it as `Open` does.
    Create(NewLevel),
    /// `pack::save(dir, pack)`; answer `Saved`.
    Save {
        dir: PathBuf,
        pack: Box<Pack>,
    },
    /// Start a sandbox match on this bake; answer `PlayEnded` when it stops.
    Play {
        level: Box<LevelSource>,
        mod_id: String,
        bots: u8,
    },
    SaveSettings(EditorSettings),
    /// List this directory; answer `Listed`.
    List(PathBuf),
    /// Read this image (bounded); answer `ImportImage` or `ImportFailed`.
    ReadImage(PathBuf),
    /// Read this image and make it small for the browser; answer `Previewed`.
    ReadPreview(PathBuf),
    /// Add this image to the user's textures (plan 14e R2); answer
    /// `TexturesChanged`, or `Failed` with why not.
    AddTexture(PathBuf),
    /// Remove this texture from the user's library; answer `TexturesChanged`.
    RemoveTexture(String),
    /// Read asset `key` whole (`olr_edit::asset::load_dir`); answer
    /// `AssetRead` with `at`.
    ReadAsset {
        key: AssetKey,
        at: (i32, i32),
    },
    /// Write this asset into the user's library (`olr_edit::asset::encode`,
    /// `save_dir`): over the folder `replace`, else in a new one named after
    /// it; answer `AssetsChanged` and `AssetSaved`, or `Failed`.
    SaveAsset {
        asset: Box<olr_edit::asset::Asset>,
        replace: Option<String>,
    },
    /// Add the asset folder at this path to the user's library (checked and
    /// decoded first, then written anew); answer `AssetsChanged` and
    /// `AssetSaved`, or `Failed`.
    ImportAsset(PathBuf),
    /// Remove the user's asset folder `id`; answer `AssetsChanged`.
    RemoveAsset(String),
    /// Read asset `key` whole (`olr_edit::asset::load_dir`) to edit it in
    /// the asset tab (issue #77); answer `AssetToEdit`.
    ReadAssetToEdit(AssetKey),
    /// The document was saved, or is being closed or left: drop its
    /// recovery copy (R7).
    RecoveryDone,
    /// The editor is done (after any unsaved-changes dialog).
    Close,
}

/// Remove colour's tolerance at first, percent (plan 14d R8).
pub const DEFAULT_TOLERANCE: u8 = 10;

/// Fill's tolerance at first, percent (plan 14e R5): exact.
pub const DEFAULT_FILL_TOLERANCE: u8 = 0;

/// What `update` notes before an asset tab's action (issue #77): the
/// history's step count, the unsaved mark, the change count, the selection
/// and the focus.
type GuardMark = (u64, bool, u64, Selection, Option<String>);

/// Bots a Play match takes at most.
pub const MAX_BOTS: u8 = 8;

pub struct EditorState {
    pub(crate) settings: EditorSettings,
    pub(crate) catalog: Catalog,
    pub(crate) doc: Option<Document>,
    pub(crate) history: History,
    pub(crate) dirty: bool,
    pub(crate) playing: bool,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) status: Status,
    pub(crate) selection: Selection,
    /// The saves in progress, oldest first: `changes` when each was sent,
    /// and what it carries out once it succeeds.
    pub(crate) saves: VecDeque<(u64, Option<Pending>)>,
    pub(crate) tool: Tool,
    /// The tool the eyedropper returns to.
    pub(crate) prev_tool: Tool,
    pub(crate) brush: Brush,
    pub(crate) paint: PaintChoice,
    pub(crate) paint_material: Option<ElementMaterial>,
    pub(crate) restore: bool,
    pub(crate) tolerance: u8,
    pub(crate) fill_tolerance: u8,
    /// The built-in textures (plan 14e R1), made once.
    pub(crate) builtin: Arc<Vec<Texture>>,
    /// Counts `TexturesChanged`: the palette's user swatches follow it.
    pub(crate) textures_generation: u64,
    pub(crate) view: View,
    /// The canvas's size in points, once the UI reported it.
    pub(crate) canvas: Option<[f32; 2]>,
    /// The view fits the level on the next `Canvas`.
    pub(crate) fit_pending: bool,
    pub(crate) material_view: bool,
    /// The Materials view (issue #43); never with the material view.
    pub(crate) surface_view: bool,
    /// The Surface brush's material (an id of the level's; `None` the
    /// element's default) and whether it paints back art.
    pub(crate) surface_paint: Option<String>,
    pub(crate) surface_back: bool,
    pub(crate) grid: bool,
    /// View ▾ Preview parallax (issue #66).
    pub(crate) preview_parallax: bool,
    /// The pointer drag in progress.
    pub(crate) drag: Option<Drag>,
    /// Changes committed so far (and undone or redone).
    pub(crate) changes: u64,
    /// The file browser, while it is open.
    pub(crate) browser: Option<Browser>,
    /// What was copied; kept across Open, New and Close (plan 14e R6).
    pub(crate) clipboard: Option<Clipboard>,
    /// The pixel rectangle (a drawn layer, level pixels; plan 14e R7):
    /// dropped by a tool change, Escape, Open and Close.
    pub(crate) pixel_rect: Option<(String, Rect)>,
    /// The pixel selection's own pixels when it was picked by colour: a
    /// flag per art pixel of `pixel_rect` (row-major); `None` all of it.
    pub(crate) pixel_region: Option<Arc<Vec<bool>>>,
    /// The Glow brush lights alike pixels in one click; Select pixels picks
    /// them (the user's request, 2026-10-19).
    pub(crate) glow_by_colour: bool,
    pub(crate) select_by_colour: bool,
    /// The glow brush's strength, percent.
    pub(crate) glow_strength: u8,
    /// The Haze brush's strength, percent.
    pub(crate) haze_strength: u8,
    /// What the canvas shows (plan 15a).
    pub(crate) view_mode: olr_look::ViewMode,
    /// The look preview's clock, ms, and whether it runs.
    pub(crate) clock_ms: f64,
    pub(crate) clock_running: bool,
    /// The tool each rail group chose last (`ToolGroup::ALL`'s order): its
    /// button brings it back (issue #8).
    pub(crate) last_tools: [Tool; 2],
    /// The right panel's tab.
    pub(crate) tab: PanelTab,
    /// The pixel rectangle's world rectangle before it was clipped to its
    /// layer (within the level): what Save as asset cuts (issue #9).
    pub(crate) pixel_world: Option<Rect>,
    /// The asset Save as asset made, waiting for its name.
    pub(crate) pending_asset: Option<Box<olr_edit::asset::Asset>>,
    /// The asset selected in the Assets tab, and whether the next press on
    /// the canvas places it.
    pub(crate) picked_asset: Option<AssetKey>,
    pub(crate) placing: bool,
    /// Counts `AssetsChanged`: the tab's thumbnails follow it.
    pub(crate) assets_generation: u64,
    /// The document's `distant_generation` last sent (`Distant`); `None`
    /// before a document's first.
    pub(crate) distant_sent: Option<u64>,
    /// The folder a double click went into (issue #61): plain clicks on its
    /// members select its children; a click outside it, or Escape, leaves.
    pub(crate) focus: Option<String>,
    // The asset tab (issue #77).
    /// The tab the user sees, and the tab whose session is in the fields
    /// above (they differ only inside `on_tab`).
    pub(crate) shown: DocTab,
    pub(crate) in_place: DocTab,
    /// The other tab's session.
    pub(crate) other: Box<Session>,
    /// Where the asset tab's asset is in the library; `None` for a new one.
    pub(crate) asset_key: Option<AssetKey>,
    /// The asset tab's save in progress: its change count when sent, and
    /// what it carries out once saved.
    pub(crate) tab_save: Option<(u64, Option<Pending>)>,
    /// The same while `pending_asset` is the asset tab's, waiting on
    /// `Dialog::AssetExists`.
    pub(crate) pending_tab: Option<(u64, Option<Pending>)>,
    /// Counts the times the canvas's document was replaced: a tab switch,
    /// an asset opened (the host uploads the canvas whole).
    pub(crate) shown_generation: u64,
}

impl EditorState {
    pub fn new(settings: EditorSettings, catalog: Catalog) -> EditorState {
        let Session {
            doc,
            history,
            dirty,
            status,
            selection,
            saves,
            tool,
            prev_tool,
            brush,
            paint,
            paint_material,
            restore,
            tolerance,
            fill_tolerance,
            view,
            fit_pending,
            surface_paint,
            surface_back,
            drag,
            changes,
            pixel_rect,
            pixel_region,
            pixel_world,
            glow_by_colour,
            select_by_colour,
            glow_strength,
            haze_strength,
            last_tools,
            tab,
            distant_sent,
            focus,
        } = Session::new();
        EditorState {
            settings,
            catalog,
            doc,
            history,
            dirty,
            playing: false,
            dialog: None,
            status,
            selection,
            saves,
            tool,
            prev_tool,
            brush,
            paint,
            paint_material,
            restore,
            tolerance,
            fill_tolerance,
            builtin: Arc::new(olr_edit::textures::builtin()),
            textures_generation: 0,
            view,
            canvas: None,
            fit_pending,
            material_view: false,
            surface_view: false,
            surface_paint,
            surface_back,
            grid: false,
            preview_parallax: false,
            drag,
            changes,
            browser: None,
            clipboard: None,
            pixel_rect,
            pixel_region,
            glow_by_colour,
            select_by_colour,
            glow_strength,
            haze_strength,
            view_mode: olr_look::ViewMode::Lit,
            clock_ms: 0.0,
            clock_running: false,
            last_tools,
            tab,
            pixel_world,
            pending_asset: None,
            picked_asset: None,
            placing: false,
            assets_generation: 0,
            distant_sent,
            focus,
            shown: DocTab::Level,
            in_place: DocTab::Level,
            other: Box::new(Session::new()),
            asset_key: None,
            tab_save: None,
            pending_tab: None,
            shown_generation: 0,
        }
    }

    /// The tool of `group` chosen last (its first before any was).
    pub fn last_tool(&self, group: ToolGroup) -> Tool {
        let i = ToolGroup::ALL.iter().position(|g| *g == group).unwrap_or(0);
        self.last_tools[i]
    }

    /// The right panel's tab.
    pub fn tab(&self) -> PanelTab {
        self.tab
    }

    /// The asset selected in the Assets tab.
    pub fn picked_asset(&self) -> Option<&AssetKey> {
        self.picked_asset.as_ref()
    }

    /// Whether the next press on the canvas places the picked asset.
    pub fn placing(&self) -> bool {
        self.placing && self.picked_asset.is_some()
    }

    /// The library's entry of `key`.
    pub fn asset(&self, key: &AssetKey) -> Option<&AssetEntry> {
        self.catalog.assets.iter().find(|a| a.key == *key)
    }

    /// How many times the asset library changed (`AssetsChanged`).
    pub fn assets_generation(&self) -> u64 {
        self.assets_generation
    }

    /// What the canvas shows: lit, full bright or the light alone.
    pub fn view_mode(&self) -> olr_look::ViewMode {
        self.view_mode
    }

    /// The look preview's clock, ms: the animations' time on the canvas.
    pub fn clock_ms(&self) -> f64 {
        self.clock_ms
    }

    /// Whether the look preview's clock runs.
    pub fn clock_running(&self) -> bool {
        self.clock_running
    }

    /// The host's frame time, ms: the clock moves on while it runs.
    pub fn advance_clock(&mut self, dt_ms: f64) {
        if self.clock_running && dt_ms.is_finite() && dt_ms > 0.0 {
            self.clock_ms += dt_ms;
        }
    }

    /// The glow brush's strength, percent.
    pub fn glow_strength(&self) -> u8 {
        self.glow_strength
    }

    /// The Haze brush's strength, percent.
    pub fn haze_strength(&self) -> u8 {
        self.haze_strength
    }

    /// Whether the Glow brush, and Select pixels, pick by colour.
    pub fn glow_by_colour(&self) -> bool {
        self.glow_by_colour
    }

    pub fn select_by_colour(&self) -> bool {
        self.select_by_colour
    }

    /// The pixel selection picked by colour: a flag per art pixel of its
    /// rectangle; `None` for a rectangle.
    pub fn pixel_region(&self) -> Option<&[bool]> {
        self.pixel_rect.as_ref().and(self.pixel_region.as_deref().map(Vec::as_slice))
    }

    /// The level's document, whichever tab shows (issue #77: crash
    /// recovery covers the level only).
    pub fn document(&self) -> Option<&Document> {
        match self.in_place {
            DocTab::Level => self.doc.as_ref(),
            DocTab::Asset => self.other.doc.as_ref(),
        }
    }

    /// The shown tab's document: what the panels and the canvas show.
    pub fn active_document(&self) -> Option<&Document> {
        self.doc.as_ref()
    }

    /// The asset tab's document, if an asset is open there.
    pub fn asset_document(&self) -> Option<&Document> {
        match self.in_place {
            DocTab::Asset => self.doc.as_ref(),
            DocTab::Level => self.other.doc.as_ref(),
        }
    }

    /// The tab shown (issue #77).
    pub fn doc_tab(&self) -> DocTab {
        self.shown
    }

    /// Where the asset tab's asset is in the library (`builtin` until a
    /// built-in one is first saved as the user's); `None` for a new one.
    pub fn asset_key(&self) -> Option<&AssetKey> {
        self.asset_key.as_ref()
    }

    /// Whether a pointer drag (a stroke, a move, a shape) is in progress
    /// (issue #78: the asset tab's previews wait for its release).
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Counts the times the canvas's document was replaced (a tab switch,
    /// an asset opened): the host then uploads it whole.
    pub fn shown_generation(&self) -> u64 {
        self.shown_generation
    }

    /// Whether the shown tab offers `tool`: the Spawn tool and the Haze
    /// brush are the level's.
    pub fn offers(&self, tool: Tool) -> bool {
        self.shown == DocTab::Level || !matches!(tool, Tool::Spawn | Tool::Haze)
    }

    pub fn settings(&self) -> &EditorSettings {
        &self.settings
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Whether the level has unsaved changes, whichever tab shows (issue
    /// #77: crash recovery covers the level only).
    pub fn dirty(&self) -> bool {
        match self.in_place {
            DocTab::Level => self.dirty,
            DocTab::Asset => self.other.dirty,
        }
    }

    /// Whether the shown tab's document has unsaved changes.
    pub fn active_dirty(&self) -> bool {
        self.dirty
    }

    /// Whether the asset tab's asset has unsaved changes.
    pub fn asset_dirty(&self) -> bool {
        match self.in_place {
            DocTab::Asset => self.dirty && self.doc.is_some(),
            DocTab::Level => self.other.dirty && self.other.doc.is_some(),
        }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    /// The file browser, while it is open.
    pub fn browser(&self) -> Option<&Browser> {
        self.browser.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The folder clicks on the canvas are inside (issue #61): its members'
    /// clicks select its children.
    pub fn focus(&self) -> Option<&str> {
        let f = self.focus.as_deref()?;
        self.doc.as_ref().is_some_and(|d| d.is_folder(f)).then_some(f)
    }

    /// The selected elements, a folder's members included, in stack order
    /// (issue #61).
    pub fn selected_elements(&self) -> Vec<String> {
        self.elements_of(self.selection.ids())
    }

    /// `ids`' elements, a folder's members for a folder, each once, in
    /// stack order.
    pub(crate) fn elements_of(&self, ids: Vec<String>) -> Vec<String> {
        let Some(doc) = &self.doc else { return Vec::new() };
        let mut found: Vec<(usize, String)> = ids
            .into_iter()
            .flat_map(|id| match doc.index_of(&id) {
                Some(_) => vec![id],
                None => doc.folder_members(&id),
            })
            .filter_map(|id| Some((doc.index_of(&id)?, id)))
            .collect();
        found.sort();
        found.dedup();
        found.into_iter().map(|f| f.1).collect()
    }

    /// Whether the selection holds a folder (issue #61).
    pub(crate) fn selects_folder(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| self.selection.ids().iter().any(|id| d.is_folder(id)))
    }

    /// Why a tool needing one element refused: `why`, and with a folder
    /// selected, how to select one of its elements (issue #61).
    pub(crate) fn one_element(&self, why: &str) -> String {
        if self.selects_folder() { format!("{why} ({DOUBLE_CLICK_INTO})") } else { why.to_string() }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn update(&mut self, action: EditorAction) -> Vec<EditorCommand> {
        if self.playing {
            if matches!(action, EditorAction::PlayEnded) {
                self.playing = false;
            }
            return Vec::new();
        }
        // While a dialog is open only its answer, the host's answers and the
        // view act (a pointer's Up still ends a drag).
        let waits = !matches!(
            action,
            EditorAction::Choose(_)
                | EditorAction::Opened(_)
                | EditorAction::OpenFailed(_)
                | EditorAction::Saved(_)
                | EditorAction::PlayEnded
                | EditorAction::Recovered(_)
                | EditorAction::Listed { .. }
                | EditorAction::ImportFailed(_)
                | EditorAction::Failed(_)
                | EditorAction::Note(_)
                | EditorAction::TexturesChanged(_)
                | EditorAction::AssetsChanged(_)
                | EditorAction::AssetSaved(_)
                | EditorAction::AssetRead { .. }
                | EditorAction::AssetToEdit { .. }
                | EditorAction::NameAsset(_)
                | EditorAction::MakeAsset(_)
                | EditorAction::Previewed { .. }
                | EditorAction::Pointer { .. }
                | EditorAction::Canvas(_)
                | EditorAction::Pan(_)
                | EditorAction::Zoom { .. }
                | EditorAction::MaterialView(_)
                | EditorAction::SurfaceView(_)
                | EditorAction::Grid(_)
                | EditorAction::PreviewParallax(_)
                | EditorAction::SetViewMode(_)
                | EditorAction::Clock(_)
        );
        if self.dialog.is_some() && waits {
            return Vec::new();
        }
        // An action that can change the document ends a drag first, so the
        // drag's step is recorded against the document it changed.
        let view_only = matches!(
            action,
            EditorAction::Pointer { .. }
                | EditorAction::DoubleClick { .. }
                | EditorAction::AltDown { .. }
                | EditorAction::SelectParent
                | EditorAction::Canvas(_)
                | EditorAction::Pan(_)
                | EditorAction::Zoom { .. }
                | EditorAction::MaterialView(_)
                | EditorAction::SurfaceView(_)
                | EditorAction::SurfacePaint(_)
                | EditorAction::SurfaceBack(_)
                | EditorAction::Grid(_)
                | EditorAction::PreviewParallax(_)
                | EditorAction::Brush(_)
                | EditorAction::Paint(_)
                | EditorAction::PaintMaterial(_)
                | EditorAction::Restore(_)
                | EditorAction::Tolerance(_)
                | EditorAction::TexturesChanged(_)
                | EditorAction::RemoveTexture(_)
                | EditorAction::AssetsChanged(_)
                | EditorAction::AssetSaved(_)
                | EditorAction::PickAsset(_)
                | EditorAction::EditAsset(_)
                | EditorAction::NewAssetDialog
                | EditorAction::CancelPlacing
                | EditorAction::RemoveAsset(_)
                | EditorAction::Select(_)
                | EditorAction::SelectToggle(_)
                | EditorAction::SelectRange(_)
                | EditorAction::Copy
                | EditorAction::DropPixels
                | EditorAction::PlayMod(_)
                | EditorAction::PlayBots(_)
                | EditorAction::Listed { .. }
                | EditorAction::Note(_)
                | EditorAction::PreviewImage(_)
                | EditorAction::Previewed { .. }
                | EditorAction::Browse(_)
                | EditorAction::BrowseTo(_)
                | EditorAction::CloseBrowser
                | EditorAction::SetViewMode(_)
                | EditorAction::Clock(_)
                | EditorAction::GlowStrength(_)
                | EditorAction::HazeStrength(_)
                | EditorAction::GlowByColour(_)
                | EditorAction::SelectByColour(_)
                | EditorAction::Tab(_)
                | EditorAction::ShowMore(_)
                | EditorAction::ShowFineTuning(_)
        );
        // A selection made, even of what was selected: a click in the list,
        // or on the canvas with Select or Spawn.
        let selects = match action {
            EditorAction::Select(_) => true,
            EditorAction::Pointer { kind: PointerKind::Down, .. } | EditorAction::AltDown { .. } => {
                matches!(self.tool, Tool::Select | Tool::Spawn)
            }
            _ => false,
        };
        let before = self.selection.clone();
        // Issue #77: the asset tab's edits are checked once made.
        let guard = (self.shown == DocTab::Asset)
            .then(|| (self.history.pushes(), self.dirty, self.changes, before.clone(), self.focus.clone()));
        let mut cmds = if view_only { Vec::new() } else { self.release() };
        cmds.extend(self.act(action));
        if let Some(g) = guard
            && self.shown == DocTab::Asset
            && self.history.pushes() > g.0
        {
            self.guard(g, &mut cmds);
        }
        // The tab follows the selection (issue #8): a spawn shows Spawns,
        // an element Elements.
        if selects || self.selection != before {
            match self.selection {
                Selection::Object(_) => self.tab = PanelTab::Spawns,
                Selection::Element(_) | Selection::Folder(_) | Selection::Many(_) => {
                    self.tab = PanelTab::Elements
                }
                Selection::None => {}
            }
        }
        // The pixel rectangle is the selected layer's: it goes with the
        // selection, or with its layer (batch C review).
        if let Some((id, _)) = &self.pixel_rect
            && (self.selection != Selection::Element(id.clone())
                || self.doc.as_ref().is_none_or(|d| d.index_of(id).is_none()))
        {
            self.pixel_rect = None;
            self.pixel_region = None;
        }
        if self.pixel_rect.is_none() {
            self.pixel_world = None;
        }
        // The distant layers follow every change of them (issue #11).
        if let Some(d) = &self.doc
            && self.distant_sent != Some(d.distant_generation())
        {
            self.distant_sent = Some(d.distant_generation());
            cmds.push(EditorCommand::Distant(d.level().distant.clone()));
        }
        cmds
    }

    fn act(&mut self, action: EditorAction) -> Vec<EditorCommand> {
        match action {
            EditorAction::Opened(doc) => {
                self.switch_tab(DocTab::Level);
                self.opened(doc)
            }
            EditorAction::Recovered(doc) => {
                self.switch_tab(DocTab::Level);
                let cmds = self.opened(doc);
                self.dirty = true;
                cmds
            }
            EditorAction::Listed { dir, entries } => {
                if let Some(b) = &mut self.browser {
                    b.listed(dir, entries);
                }
                Vec::new()
            }
            EditorAction::ImportFailed(why) | EditorAction::Failed(why) => {
                // An asset tab's save that failed is over.
                self.tab_save = None;
                self.dialog = Some(Dialog::Error(why));
                Vec::new()
            }
            EditorAction::Note(text) => {
                self.status.message = Some(text);
                Vec::new()
            }
            EditorAction::TexturesChanged(textures) => {
                self.catalog.textures = textures;
                self.textures_generation += 1;
                if self.texture(&self.paint).is_none() {
                    self.paint = PaintChoice::default();
                }
                Vec::new()
            }
            EditorAction::RemoveTexture(name) => vec![EditorCommand::RemoveTexture(name)],
            EditorAction::AssetsChanged(assets) => {
                self.catalog.assets = assets;
                self.assets_generation += 1;
                if self.picked_asset.as_ref().is_some_and(|k| self.asset(k).is_none()) {
                    self.picked_asset = None;
                }
                // The asset tab's asset removed from the library: saving
                // makes a new one (issue #77).
                if self.asset_key.as_ref().is_some_and(|k| self.asset(k).is_none()) {
                    self.asset_key = None;
                }
                Vec::new()
            }
            EditorAction::AssetSaved(key) if self.tab_save.is_some() => self.tab_saved(key),
            EditorAction::AssetSaved(key) => {
                let name = self.asset(&key).map_or_else(|| key.id.clone(), |a| a.name.clone());
                self.status.message = Some(format!("saved the asset {name:?} in your assets"));
                self.picked_asset = Some(key);
                self.placing = false;
                self.tab = PanelTab::Assets;
                Vec::new()
            }
            EditorAction::AssetRead { at, asset } => match asset {
                Ok(asset) => self.place_asset(&asset, at),
                Err(why) => {
                    self.dialog = Some(Dialog::Error(why));
                    Vec::new()
                }
            },
            EditorAction::SaveAsAsset => self.save_as_asset(),
            EditorAction::NameAsset(name) => self.name_asset(name),
            EditorAction::PickAsset(key) => {
                self.placing = key.is_some();
                if let Some(a) = key.as_ref().and_then(|k| self.asset(k)) {
                    self.status.message = Some(format!(
                        "click the level to place {:?} (Escape cancels), or drag it there",
                        a.name
                    ));
                }
                self.picked_asset = key;
                Vec::new()
            }
            EditorAction::CancelPlacing => {
                self.placing = false;
                Vec::new()
            }
            EditorAction::PlaceAsset { key, at } => {
                self.placing = false;
                if self.doc.is_none() || self.asset(&key).is_none() {
                    return Vec::new();
                }
                vec![EditorCommand::ReadAsset { key, at }]
            }
            EditorAction::ShowTab(tab) if tab == self.shown => Vec::new(),
            EditorAction::ShowTab(DocTab::Asset) if self.asset_document().is_none() => Vec::new(),
            EditorAction::ShowTab(tab) => {
                self.switch_tab(tab);
                vec![EditorCommand::Upload(
                    self.doc.as_ref().map_or(Rect { x: 0, y: 0, w: 0, h: 0 }, Document::all),
                )]
            }
            EditorAction::EditAsset(key) => match self.asset(&key) {
                Some(_) => vec![EditorCommand::ReadAssetToEdit(key)],
                None => Vec::new(),
            },
            EditorAction::AssetToEdit { key, asset } => match asset {
                Ok(asset) => self.leave(Pending::EditAsset { key, asset }),
                Err(why) => {
                    self.dialog = Some(Dialog::Error(why));
                    Vec::new()
                }
            },
            EditorAction::NewAssetDialog => {
                let art_scale = self.document().map_or(1, |d| d.level().art_scale);
                self.dialog =
                    Some(Dialog::NewAsset(NewAsset { name: "Asset".into(), size: (64, 64), art_scale }));
                Vec::new()
            }
            EditorAction::MakeAsset(n) => {
                if !matches!(self.dialog, Some(Dialog::NewAsset(_))) {
                    return Vec::new();
                }
                if let Some(why) = n.refusal() {
                    self.status.message = Some(why);
                    self.dialog = Some(Dialog::NewAsset(n));
                    return Vec::new();
                }
                self.dialog = None;
                self.leave(Pending::NewAsset(n))
            }
            EditorAction::RemoveAsset(id) => {
                let key = AssetKey { builtin: false, id: id.clone() };
                if let Some(a) = self.asset(&key) {
                    self.dialog = Some(Dialog::RemoveAsset { id, name: a.name.clone() });
                }
                Vec::new()
            }
            EditorAction::Browse(purpose) => {
                let dir = self
                    .settings
                    .browse_dir
                    .clone()
                    .or_else(|| self.catalog.home.clone())
                    .unwrap_or_else(|| PathBuf::from("."));
                self.browser = Some(Browser::new(purpose, dir.clone()));
                vec![EditorCommand::List(dir)]
            }
            EditorAction::BrowseTo(path) => match &self.browser {
                Some(b) => vec![EditorCommand::List(b.resolve(&path))],
                None => Vec::new(),
            },
            EditorAction::Pick(name) => self.pick(&name),
            EditorAction::PreviewImage(name) => self
                .browser
                .as_mut()
                .and_then(|b| b.ask_preview(&name))
                .map(EditorCommand::ReadPreview)
                .into_iter()
                .collect(),
            EditorAction::Previewed { path, image } => {
                if let Some(b) = &mut self.browser {
                    b.previewed(&path, image);
                }
                Vec::new()
            }
            EditorAction::CloseBrowser => {
                self.browser = None;
                Vec::new()
            }
            EditorAction::OpenFailed(why) => {
                self.dialog = Some(Dialog::Error(why));
                Vec::new()
            }
            EditorAction::Saved(result) => self.on_tab(DocTab::Level, |s| s.saved(result)),
            EditorAction::PlayEnded => Vec::new(),
            EditorAction::Open(dir) => self.leave(Pending::Open(dir)),
            EditorAction::New(n) => self.leave(Pending::New(n)),
            EditorAction::Close => self.leave(Pending::Close),
            EditorAction::Save if self.shown == DocTab::Asset => self.save_asset(None),
            EditorAction::Save => self.save(None),
            EditorAction::Choose(choice) => self.choose(choice),
            EditorAction::ImportImage { file_name, bytes } => {
                let centre = self.import_centre();
                let cmds = self.edit(|d| d.add_image(&file_name, bytes, centre));
                self.select_top(&cmds);
                cmds
            }
            EditorAction::AddLayer => {
                let cmds = self.edit(|d| d.add_layer(ElementMaterial::Dirt, "Layer"));
                self.select_top(&cmds);
                cmds
            }
            EditorAction::Select(selection) => {
                self.selection = match selection {
                    Selection::Many(ids) => self.group(ids),
                    s => s,
                };
                // Clicks on the canvas pick the selected item's siblings
                // (issue #61).
                self.focus = match (&self.selection, &self.doc) {
                    (Selection::Element(id) | Selection::Folder(id), Some(d)) => d.parent_of(id),
                    _ => None,
                };
                Vec::new()
            }
            EditorAction::SelectParent => {
                let parent = match (&self.selection, &self.doc) {
                    (Selection::Element(id) | Selection::Folder(id), Some(d)) => d.parent_of(id),
                    _ => None,
                };
                self.focus = parent.as_ref().and_then(|p| self.doc.as_ref()?.parent_of(p));
                self.selection = parent.map_or(Selection::None, Selection::Folder);
                Vec::new()
            }
            EditorAction::GroupSelection => self.group_selection(),
            EditorAction::Ungroup => self.ungroup(),
            EditorAction::SetFolder(id, field) => self.edit(|d| d.set_folder(&id, field)),
            EditorAction::DropRow { id, at } => self.edits(|d| d.drop_row(&id, &at)),
            EditorAction::DoubleClick { world } => self.double_click(world),
            EditorAction::AltDown { world } => self.alt_down(world),
            EditorAction::SelectToggle(id) => {
                let mut ids = self.selection.ids();
                match ids.iter().position(|e| *e == id) {
                    Some(i) => {
                        ids.remove(i);
                    }
                    None => ids.push(id),
                }
                self.selection = self.group(ids);
                Vec::new()
            }
            EditorAction::SelectRange(id) => {
                let anchor = match &self.selection {
                    Selection::Element(a) | Selection::Folder(a) => Some(a.clone()),
                    _ => None,
                };
                // The rows shown in between, in list order (issue #61).
                let range = self.doc.as_ref().and_then(|d| {
                    let rows = d.rows();
                    let (a, b) = (
                        rows.iter().position(|r| Some(&r.id) == anchor.as_ref())?,
                        rows.iter().position(|r| r.id == id)?,
                    );
                    Some(rows[a.min(b)..=a.max(b)].iter().map(|r| r.id.clone()).collect())
                });
                self.selection = self.group(range.unwrap_or_else(|| vec![id]));
                Vec::new()
            }
            EditorAction::Copy => {
                self.copy();
                Vec::new()
            }
            EditorAction::Cut => {
                if !self.copy() {
                    return Vec::new();
                }
                self.act(EditorAction::Remove)
            }
            EditorAction::DropPixels => {
                self.pixel_rect = None;
                self.pixel_region = None;
                Vec::new()
            }
            EditorAction::Remove if self.pixel_rect.is_some() => self.erase_pixels(),
            EditorAction::Paste => self.paste(),
            EditorAction::Set(id, property) => self.edits(|d| d.set_property(&id, property)),
            EditorAction::SetGlow { id, glow } => self.edits(|d| d.set_glow(&id, glow)),
            EditorAction::SetLook(look) => self.edits(|d| d.set_look(look)),
            EditorAction::AddGroup(group) => self.edits(|d| d.add_group(group)),
            EditorAction::SetGroup { id, group } => self.edits(|d| d.set_group(&id, group)),
            EditorAction::RemoveGroup(id) => {
                if self
                    .doc
                    .as_ref()
                    .is_some_and(|d| d.level().look.iter().any(|l| l.glow_groups.iter().any(|g| g.id == id)))
                {
                    self.dialog = Some(Dialog::RemoveGroup { id });
                }
                Vec::new()
            }
            EditorAction::GlowStrength(v) => {
                self.glow_strength = v.min(100);
                Vec::new()
            }
            EditorAction::HazeStrength(v) => {
                self.haze_strength = v.min(100);
                Vec::new()
            }
            EditorAction::GlowByColour(on) => {
                self.glow_by_colour = on;
                Vec::new()
            }
            EditorAction::SelectByColour(on) => {
                self.select_by_colour = on;
                Vec::new()
            }
            EditorAction::GlowSelection(v) => {
                let Some((id, rect)) = self.pixel_rect.clone() else {
                    self.status.message = Some("select pixels first (Select pixels, M)".into());
                    return Vec::new();
                };
                let region = self.pixel_region.clone();
                self.edits(|d| d.glow_pixels(&id, rect, region.as_deref().map(Vec::as_slice), v.min(100)))
            }
            EditorAction::SetViewMode(mode) => {
                self.view_mode = mode;
                Vec::new()
            }
            EditorAction::Clock(c) => {
                match c {
                    ClockAction::Play => self.clock_running = true,
                    ClockAction::Pause => self.clock_running = false,
                    ClockAction::Scrub(t) if t.is_finite() && t >= 0.0 => self.clock_ms = t,
                    ClockAction::Scrub(_) => {}
                }
                Vec::new()
            }
            EditorAction::SetMany(ids, property) => {
                // A folder stands for its elements (issue #61).
                let ids = self.elements_of(ids);
                self.group_edit(|d| d.set_many(&ids, property))
            }
            EditorAction::Remove => match self.selection.clone() {
                Selection::Element(id) => self.removed(|d| d.remove(&id)),
                Selection::Object(i) => self.removed(|d| d.remove_object(i)),
                Selection::Folder(_) | Selection::Many(_) => {
                    let ids = self.selection.ids();
                    let cmds = self.group_edit(|d| d.remove_items(&ids));
                    if !cmds.is_empty() {
                        self.selection = Selection::None;
                    }
                    cmds
                }
                Selection::None => Vec::new(),
            },
            EditorAction::MoveElement { id, to } => self.edit(|d| d.move_to(&id, to)),
            EditorAction::Split(id) => self.restack(&id, false),
            EditorAction::MergeDown(id) => self.restack(&id, true),
            EditorAction::Meta(mut meta) => {
                // The asset tab's name only: its theme is the level's.
                if let (DocTab::Asset, Some(d)) = (self.shown, &self.doc) {
                    let now = d.meta();
                    (meta.theme, meta.notes) = (now.theme, now.notes);
                }
                let catalog = self.catalog.clone();
                self.edit(|d| d.set_meta(meta, &catalog))
            }
            EditorAction::ArtScale(_) if self.shown == DocTab::Asset => {
                self.status.message = Some("an asset's art scale is shown, not changed".into());
                Vec::new()
            }
            EditorAction::ArtScale(to) => {
                if self.doc.as_ref().is_some_and(|d| d.level().art_scale != to) {
                    self.dialog = Some(Dialog::ArtScale { to });
                }
                Vec::new()
            }
            EditorAction::Resize { size, offset, material, colour } => {
                self.edit(|d| d.resize(size, offset, material, colour))
            }
            EditorAction::Undo => self.step(true),
            EditorAction::Redo => self.step(false),
            EditorAction::Play if self.shown == DocTab::Asset => {
                self.status.message = Some("Play is the level tab's".into());
                Vec::new()
            }
            EditorAction::Play => match &self.doc {
                Some(d) => {
                    self.playing = true;
                    vec![EditorCommand::Play {
                        level: Box::new(d.level().clone()),
                        mod_id: self.settings.play_mod.clone(),
                        bots: self.settings.play_bots,
                    }]
                }
                None => Vec::new(),
            },
            EditorAction::PlayMod(m) => {
                self.settings.play_mod = m;
                vec![EditorCommand::SaveSettings(self.settings.clone())]
            }
            EditorAction::PlayBots(n) => {
                self.settings.play_bots = n.min(MAX_BOTS);
                vec![EditorCommand::SaveSettings(self.settings.clone())]
            }
            EditorAction::Tab(PanelTab::Spawns) if self.shown == DocTab::Asset => Vec::new(),
            EditorAction::Tab(tab) => {
                self.tab = tab;
                Vec::new()
            }
            EditorAction::ShowMore(open) => {
                self.settings.more_open = open;
                vec![EditorCommand::SaveSettings(self.settings.clone())]
            }
            EditorAction::ShowFineTuning(open) => {
                self.settings.fine_tuning_open = open;
                vec![EditorCommand::SaveSettings(self.settings.clone())]
            }
            EditorAction::Tool(tool) if !self.offers(tool) => {
                self.status.message = Some(format!("the {} is the level tab's", tool.name()));
                Vec::new()
            }
            EditorAction::Tool(tool) => {
                self.placing = false;
                if let Some(i) = ToolGroup::ALL.iter().position(|g| tool.group() == Some(*g)) {
                    self.last_tools[i] = tool;
                }
                if tool == Tool::Eyedropper && self.tool != Tool::Eyedropper {
                    self.prev_tool = self.tool;
                }
                if tool != self.tool {
                    self.pixel_rect = None;
                    self.pixel_region = None;
                }
                self.tool = tool;
                Vec::new()
            }
            EditorAction::Brush(b) => {
                self.brush = Brush { shape: b.shape, size: b.size.clamp(1, MAX_BRUSH) };
                Vec::new()
            }
            EditorAction::Paint(p) => {
                // A texture that is not there paints the colour (R3).
                self.paint = if self.texture(&p).is_some() { p } else { PaintChoice::default() };
                Vec::new()
            }
            EditorAction::PaintMaterial(m) => {
                self.paint_material = m;
                Vec::new()
            }
            EditorAction::Restore(on) => {
                self.restore = on;
                Vec::new()
            }
            EditorAction::Tolerance(t) => {
                match self.tool {
                    Tool::Fill => self.fill_tolerance = t.min(100),
                    _ => self.tolerance = t.min(100),
                }
                Vec::new()
            }
            EditorAction::Pointer { kind, world, shift, toggle } => self.pointer(kind, world, shift, toggle),
            EditorAction::Nudge { dx, dy } => self.nudge(dx, dy),
            EditorAction::Canvas(size) => {
                self.canvas = Some(size);
                self.fit();
                Vec::new()
            }
            EditorAction::Pan(delta) => {
                self.view.pan(delta);
                Vec::new()
            }
            EditorAction::Zoom { steps, about } => {
                self.view.zoom_about(steps, about);
                Vec::new()
            }
            EditorAction::MaterialView(on) => {
                self.material_view = on;
                self.surface_view &= !on;
                Vec::new()
            }
            EditorAction::SurfaceView(on) => {
                self.surface_view = on;
                self.material_view &= !on;
                Vec::new()
            }
            EditorAction::SurfacePaint(id) => {
                self.surface_paint = id;
                Vec::new()
            }
            EditorAction::SurfaceBack(on) => {
                self.surface_back = on;
                Vec::new()
            }
            EditorAction::AddMaterial { builtin, id } => {
                let found =
                    self.catalog.materials.iter().find(|m| m.builtin == builtin && m.material.id == id);
                let Some(m) = found.map(|m| m.material.clone()) else { return Vec::new() };
                let name = m.name.clone();
                let cmds = self.edits(|d| d.add_material(m));
                if !cmds.is_empty() {
                    self.status.message = Some(format!("added {name} to the level's materials"));
                }
                cmds
            }
            EditorAction::RemoveMaterial(id) => {
                let cmds = self.edits(|d| d.remove_material(&id));
                if self.surface_paint.as_ref() == Some(&id) && !cmds.is_empty() {
                    self.surface_paint = None;
                }
                cmds
            }
            EditorAction::Grid(on) => {
                self.grid = on;
                Vec::new()
            }
            EditorAction::PreviewParallax(on) => {
                self.preview_parallax = on;
                Vec::new()
            }
        }
    }

    /// A document opened (or recovered): a fresh history and view.
    fn opened(&mut self, doc: Box<Document>) -> Vec<EditorCommand> {
        self.pixel_rect = None;
        self.pixel_region = None;
        let all = doc.all();
        self.doc = Some(*doc);
        self.distant_sent = None;
        self.fit_pending = true;
        self.fit();
        self.history = History::new();
        self.selection = Selection::None;
        self.focus = None;
        self.dialog = None;
        // Issue #74: a level converted on opening is unsaved till saved.
        self.dirty = self.doc.as_ref().is_some_and(Document::converted);
        self.status.message = self.dirty.then(|| crate::document::CONVERTED.to_string());
        self.refresh_problems();
        vec![EditorCommand::Upload(all)]
    }

    /// The browser's `Pick`: a file to import is read (its folder
    /// remembered), a pack folder to open is opened as `Open` is (the
    /// unsaved-changes dialog first), any other folder is entered.
    fn pick(&mut self, name: &str) -> Vec<EditorCommand> {
        let Some(b) = &self.browser else { return Vec::new() };
        let Some(e) = b.entry(name) else { return Vec::new() };
        let (dir, path) = (b.dir().to_path_buf(), b.dir().join(name));
        let take = match b.purpose() {
            Purpose::ImportImage | Purpose::ImportTexture => !e.is_dir,
            Purpose::OpenPack => e.is_pack,
            Purpose::ImportAsset => e.is_asset,
        };
        if !take {
            return if e.is_dir { vec![EditorCommand::List(path)] } else { Vec::new() };
        }
        let purpose = b.purpose();
        self.browser = None;
        self.settings.browse_dir = Some(dir);
        let mut cmds = vec![EditorCommand::SaveSettings(self.settings.clone())];
        match purpose {
            Purpose::ImportImage => cmds.insert(0, EditorCommand::ReadImage(path)),
            Purpose::ImportTexture => cmds.insert(0, EditorCommand::AddTexture(path)),
            Purpose::ImportAsset => cmds.insert(0, EditorCommand::ImportAsset(path)),
            Purpose::OpenPack => cmds.extend(self.leave(Pending::Open(path))),
        }
        cmds
    }

    /// Selects the top element after an add (`cmds` not empty: it was made).
    fn select_top(&mut self, cmds: &[EditorCommand]) {
        if !cmds.is_empty()
            && let Some(top) = self.doc.as_ref().and_then(|d| d.items().last())
        {
            self.selection = Selection::Element(top.element.id.clone());
        }
    }

    /// Fits the view to the level once a canvas size is known after `Opened`.
    fn fit(&mut self) {
        if let (true, Some(canvas), Some(doc)) = (self.fit_pending, self.canvas, &self.doc) {
            let l = doc.level();
            self.view = View::fit(canvas, (l.size.0 * l.art_scale, l.size.1 * l.art_scale));
            self.fit_pending = false;
        }
    }

    /// Where an imported image's box is centred: the world point at the
    /// canvas's centre, or the level's centre before the first `Canvas`.
    fn import_centre(&self) -> (i32, i32) {
        let Some(d) = &self.doc else { return (0, 0) };
        match self.canvas {
            Some(c) => self.view.to_world([c[0] / 2.0, c[1] / 2.0], d.level().art_scale),
            None => {
                let (w, h) = d.level().size;
                ((w / 2) as i32, (h / 2) as i32)
            }
        }
    }

    /// Splits element `id`, or merges it down (`merge`): one step; the
    /// merged layer is selected, a split's parts are not.
    fn restack(&mut self, id: &str, merge: bool) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        let below =
            doc.index_of(id).and_then(|i| i.checked_sub(1)).map(|i| doc.items()[i].element.id.clone());
        let done = if merge { doc.merge_down(id) } else { doc.split(id) };
        match done {
            Ok(edits) => {
                let all = doc.all();
                self.selection = match (merge, below) {
                    (true, Some(b)) => Selection::Element(b),
                    _ => Selection::None,
                };
                self.history.push(edits);
                self.committed(all)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// The pixel rectangle: its drawn layer and level rectangle (plan 14e R7).
    pub fn pixel_rect(&self) -> Option<&(String, Rect)> {
        self.pixel_rect.as_ref()
    }

    pub fn brush(&self) -> Brush {
        self.brush
    }

    pub fn paint(&self) -> &PaintChoice {
        &self.paint
    }

    /// How many times the user's library changed (`TexturesChanged`).
    pub fn textures_generation(&self) -> u64 {
        self.textures_generation
    }

    /// The built-in textures (plan 14e R1), in `TextureRef::Builtin` order.
    pub fn builtin_textures(&self) -> &[Texture] {
        &self.builtin
    }

    /// The image `paint` paints with when it is a texture, from the
    /// built-in set or the user's library; `Some(None)` for a paint that
    /// is no texture, `None` for a texture that is not there.
    pub(crate) fn texture(&self, paint: &PaintChoice) -> Option<Option<&olr_data::image::RgbaImage>> {
        match paint {
            PaintChoice::Texture(TextureRef::Builtin(i)) => self.builtin.get(*i).map(|t| Some(&t.image)),
            PaintChoice::Texture(TextureRef::User(name)) => {
                self.catalog.textures.iter().find(|t| t.name == *name).map(|t| Some(&t.image))
            }
            _ => Some(None),
        }
    }

    /// The open level theme's holes, the crater brushes (plan 14e R4).
    pub fn holes(&self) -> &[Arc<StampMask>] {
        let Some(doc) = &self.doc else { return &[] };
        let theme = &doc.level().theme;
        self.catalog.themes.iter().find(|t| t.id == *theme).map_or(&[], |t| &t.holes[..])
    }

    /// What painting lays; none is the layer's own material.
    pub fn paint_material(&self) -> Option<ElementMaterial> {
        self.paint_material
    }

    pub fn restore(&self) -> bool {
        self.restore
    }

    /// Remove colour's tolerance.
    pub fn tolerance(&self) -> u8 {
        self.tolerance
    }

    /// Fill's tolerance.
    pub fn fill_tolerance(&self) -> u8 {
        self.fill_tolerance
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn material_view(&self) -> bool {
        self.material_view
    }

    /// Whether the canvas shows the Materials view (issue #43).
    pub fn surface_view(&self) -> bool {
        self.surface_view
    }

    /// The Surface brush's material: one of the level's, by id; `None`
    /// the element's default (a material the level no longer has reads as
    /// `None`).
    pub fn surface_paint(&self) -> Option<&str> {
        let id = self.surface_paint.as_deref()?;
        self.doc.as_ref().and_then(|d| d.material_value(id)).map(|_| id)
    }

    /// Whether the Surface brush paints the selected element's back art.
    pub fn surface_back(&self) -> bool {
        self.surface_back
    }

    pub fn grid(&self) -> bool {
        self.grid
    }

    /// Whether View ▾ Preview parallax is on (issue #66).
    pub fn preview_parallax(&self) -> bool {
        self.preview_parallax
    }

    fn refresh_problems(&mut self) {
        let mut problems = self.doc.as_ref().map(Document::diagnostics).unwrap_or_default();
        problems.sort_by_key(|p| p.severity);
        self.status.problems = problems;
    }

    /// A change made by `op` as one history step, or its refusal shown.
    pub(crate) fn edit(
        &mut self,
        op: impl FnOnce(&mut Document) -> Result<Change, DocError>,
    ) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        match op(doc) {
            // A change of nothing is no step and leaves the level clean.
            Ok(change) if change.edit.is_noop() => Vec::new(),
            Ok(change) => {
                self.drop_stale_pixels(std::slice::from_ref(&change.edit));
                self.history.push(vec![change.edit]);
                self.committed(change.dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// Copies the pixel rectangle, else the selected elements, to the
    /// clipboard (plan 14e R6, R7); whether there was anything.
    fn copy(&mut self) -> bool {
        let Some(doc) = &self.doc else { return false };
        // Nothing to copy is said (plan 14e minors).
        const NOTHING_TO_COPY: &str =
            "nothing to copy: select elements, a spawn or a light, or pixels with Select pixels (M)";
        if self.pixel_region.is_some() {
            self.status.message = Some(BY_COLOUR_TAKES_A_RECTANGLE.into());
            return false;
        }
        if let Some((id, rect)) = &self.pixel_rect {
            let Some((clip, at)) = doc.copy_pixels(id, *rect) else {
                self.status.message = Some(format!("no pixels of {id:?} in the rectangle to copy"));
                return false;
            };
            let art_scale = doc.level().art_scale;
            self.status.message = Some(format!("copied {} × {} pixels", clip.size.0, clip.size.1));
            self.clipboard = Some(Clipboard::Pixels { art_scale, at, clip, offset: PASTE_STEP });
            return true;
        }
        // A spawn (the user's request 2026-10-19).
        if let Selection::Object(i) = self.selection
            && let Some(o) = doc.level().objects.get(i)
        {
            self.status.message = Some("copied the spawn".into());
            self.clipboard = Some(Clipboard::Objects { objects: vec![o.clone()], offset: PASTE_STEP });
            return true;
        }
        // A folder all of whose elements are selected comes along (issue #60).
        let (items, folders) = doc.copy_tree(&self.selection.ids());
        if items.is_empty() {
            self.status.message = Some(NOTHING_TO_COPY.into());
            return false;
        }
        let n = items.len();
        self.clipboard = Some(Clipboard::Elements {
            art_scale: doc.level().art_scale,
            items,
            folders,
            offset: PASTE_STEP,
        });
        self.status.message = Some(match n {
            1 => "copied 1 element".into(),
            n => format!("copied {n} elements"),
        });
        true
    }

    /// Erases the pixel rectangle from its layer (14d's erase: dirt or rock
    /// over back art digs): one step.
    fn erase_pixels(&mut self) -> Vec<EditorCommand> {
        let Some((id, r)) = self.pixel_rect.clone() else { return Vec::new() };
        if self.pixel_region.is_some() {
            self.status.message = Some(BY_COLOUR_TAKES_A_RECTANGLE.into());
            return Vec::new();
        }
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        let clamp = |v: u32| v.min(i32::MAX as u32) as i32;
        let shape =
            olr_edit::level::Shape::Rect { x: clamp(r.x), y: clamp(r.y), w: clamp(r.w), h: clamp(r.h) };
        let begun = match doc.begin_stroke(&id, crate::ops::Stroke::Erase) {
            Ok(b) => b,
            Err(e) => {
                self.status.message = Some(e.to_string());
                return Vec::new();
            }
        };
        let before = doc.stroke_before(&id);
        let erased = doc.erase(&id, &shape, false);
        let edit = match (&erased, &before) {
            (Ok(Some(rect)), Some(b)) => doc.stroke_edit(&id, *rect, b).filter(|e| !e.is_noop()),
            _ => None,
        };
        if let Err(e) = &erased {
            self.status.message = Some(e.to_string());
        }
        let Some(edit) = edit else {
            if let Some(b) = &begun {
                doc.apply(b, false);
            }
            return Vec::new();
        };
        let dirty = erased.ok().flatten().unwrap_or(r);
        self.history.push(begun.into_iter().chain([edit]).collect());
        self.committed(dirty)
    }

    /// Pastes the clipboard on top as one step, the copies selected; the
    /// next paste lands `PASTE_STEP` further.
    fn paste(&mut self) -> Vec<EditorCommand> {
        let (Some(doc), Some(clip)) = (self.doc.as_mut(), self.clipboard.as_mut()) else { return Vec::new() };
        if let Clipboard::Objects { objects, offset } = clip {
            return match doc.paste_objects(objects, *offset) {
                Ok((change, indices)) => {
                    *offset = (offset.0.saturating_add(PASTE_STEP.0), offset.1.saturating_add(PASTE_STEP.1));
                    self.history.push(vec![change.edit]);
                    if let Some(&last) = indices.last() {
                        self.selection = Selection::Object(last);
                    }
                    self.committed(change.dirty)
                }
                Err(e) => {
                    self.status.message = Some(e.to_string());
                    Vec::new()
                }
            };
        }
        let done = match clip {
            Clipboard::Elements { art_scale, items, folders, offset } => {
                doc.paste_tree(items, folders, *art_scale, *offset).map(|(edits, ids)| (edits, ids, offset))
            }
            Clipboard::Pixels { art_scale, at, clip, offset } => {
                let to = (at.0.saturating_add(offset.0), at.1.saturating_add(offset.1));
                doc.paste_pixels(clip, *art_scale, to).map(|(edit, id)| (vec![edit], vec![id], offset))
            }
            // Pasted above.
            Clipboard::Objects { .. } => return Vec::new(),
        };
        match done {
            Ok((edits, ids, offset)) => {
                *offset = (offset.0.saturating_add(PASTE_STEP.0), offset.1.saturating_add(PASTE_STEP.1));
                let dirty = doc.all();
                self.history.push(edits);
                // A pasted folder is selected whole (issue #61).
                let tops =
                    ids.into_iter().map(|id| doc.folders_holding(&id).into_iter().next().unwrap_or(id));
                let tops: Vec<String> = tops.collect();
                self.selection = self.group(tops);
                self.committed(dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// Save as asset (issue #9): the pixel rectangle cut from every visible
    /// layer, else the selected elements; asks the name next.
    fn save_as_asset(&mut self) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        if self.pixel_region.is_some() {
            self.status.message =
                Some("Save as asset takes a rectangle selection (Select pixels without By colour)".into());
            return Vec::new();
        }
        let made = if let Some(rect) = self.pixel_world {
            doc.asset_of_rect(rect, "Asset")
        } else {
            let ids = self.selected_elements();
            if ids.is_empty() {
                self.status.message =
                    Some("nothing to save: select elements, or pixels with Select pixels (M)".into());
                return Vec::new();
            }
            // A folder's asset is named after it (issue #61).
            let folders = doc.folders();
            let name = match self.selection.ids().as_slice() {
                [id] => doc.index_of(id).map_or_else(
                    || folders.iter().find(|f| f.id == *id).map_or("Asset", |f| f.name.as_str()),
                    |i| doc.items[i].element.name.as_str(),
                ),
                _ => "Asset",
            };
            doc.asset_of_elements(&ids, name)
        };
        match made {
            Ok(asset) => {
                let suggested: String = asset.name.chars().take(olr_format::assets::MAX_ASSET_NAME).collect();
                self.pending_asset = Some(Box::new(asset));
                self.dialog = Some(Dialog::AssetName { suggested });
            }
            Err(e) => self.status.message = Some(e.to_string()),
        }
        Vec::new()
    }

    /// The name dialog's Save: an empty or too long name asks again; a name
    /// one of the user's assets has asks to replace it or keep both.
    fn name_asset(&mut self, name: String) -> Vec<EditorCommand> {
        if !matches!(self.dialog, Some(Dialog::AssetName { .. })) || self.pending_asset.is_none() {
            return Vec::new();
        }
        let name = name.trim().to_string();
        let len = name.chars().count();
        if len == 0 || len > olr_format::assets::MAX_ASSET_NAME || name.chars().any(char::is_control) {
            self.status.message =
                Some(format!("an asset's name is 1 to {} characters", olr_format::assets::MAX_ASSET_NAME));
            self.dialog = Some(Dialog::AssetName { suggested: name });
            return Vec::new();
        }
        if let Some(a) = self.pending_asset.as_mut() {
            a.name = name.clone();
        }
        let clash = self.catalog.assets.iter().find(|a| !a.key.builtin && a.name == name);
        if let Some(a) = clash {
            self.dialog = Some(Dialog::AssetExists { name, id: a.key.id.clone() });
            return Vec::new();
        }
        self.dialog = None;
        self.send_asset(None)
    }

    /// The named asset to the host (over folder `replace`). The asset
    /// tab's own (issue #77) takes the name it is saved under.
    fn send_asset(&mut self, replace: Option<String>) -> Vec<EditorCommand> {
        let Some(asset) = self.pending_asset.take() else { return Vec::new() };
        if let Some(t) = self.pending_tab.take() {
            self.tab_save = Some(t);
            let name = asset.name.clone();
            self.on_tab(DocTab::Asset, |s| {
                if let Some(d) = s.doc.as_mut() {
                    d.level.name = name;
                }
            });
        }
        vec![EditorCommand::SaveAsset { asset, replace }]
    }

    /// Places `asset` (`AssetRead`) at world pixel `at`: one step, the new
    /// elements selected.
    fn place_asset(&mut self, asset: &olr_edit::asset::Asset, at: (i32, i32)) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        match doc.place_asset(asset, at) {
            Ok(placed) => {
                let dirty = doc.all();
                self.history.push(placed.edits);
                // Two pieces or more are a folder, selected (issue #61).
                self.selection = match placed.folder {
                    Some(f) => Selection::Folder(f),
                    None => self.group(placed.ids),
                };
                self.tab = PanelTab::Elements;
                let cmds = self.committed(dirty);
                self.status.message = placed.note;
                cmds
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// The selection of `ids` (plan 14e R8): the elements and folders that
    /// exist, each once, in stack order (a folder where its run starts, before
    /// its subfolders); one lying in another selected folder is left out (the
    /// folder stands for it, issue #61); one is `Element` or `Folder`, none
    /// `None`.
    pub(crate) fn group(&self, ids: Vec<String>) -> Selection {
        let Some(doc) = &self.doc else { return Selection::None };
        // (where it starts, how deep, whether an element, id)
        let mut found: Vec<(usize, usize, bool, String)> = ids
            .into_iter()
            .filter_map(|id| {
                let depth = doc.folders_holding(&id).len();
                match doc.index_of(&id) {
                    Some(i) => Some((i, depth, true, id)),
                    None => {
                        let first = doc.folder_members(&id).first().and_then(|m| doc.index_of(m))?;
                        Some((first, depth, false, id))
                    }
                }
            })
            .collect();
        found.sort();
        found.dedup();
        let folders: Vec<String> = found.iter().filter(|f| !f.2).map(|f| f.3.clone()).collect();
        found.retain(|f| !doc.folders_holding(&f.3).iter().any(|h| folders.contains(h)));
        match found.len() {
            0 => Selection::None,
            1 => match found.remove(0) {
                (_, _, true, id) => Selection::Element(id),
                (_, _, false, id) => Selection::Folder(id),
            },
            _ => Selection::Many(found.into_iter().map(|f| f.3).collect()),
        }
    }

    /// Ctrl+G (issue #61): the selected elements and folders into a new
    /// folder, selected; one step.
    fn group_selection(&mut self) -> Vec<EditorCommand> {
        let ids = self.selection.ids();
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        if ids.is_empty() {
            self.status.message = Some("select elements or folders to put in a new folder".into());
            return Vec::new();
        }
        match doc.group(&ids, "") {
            Ok((edits, dirty, id)) => {
                self.history.push(edits);
                self.selection = Selection::Folder(id);
                self.committed(dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// Ctrl+Shift+G (issue #61): the selected folder's members take its
    /// place, selected; one step.
    fn ungroup(&mut self) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        let Selection::Folder(id) = self.selection.clone() else {
            self.status.message = Some("select a folder to ungroup".into());
            return Vec::new();
        };
        let children = doc.children(Some(&id));
        let parent = doc.parent_of(&id);
        match doc.ungroup(&id) {
            Ok((edits, dirty)) => {
                self.history.push(edits);
                if self.focus.as_ref() == Some(&id) {
                    self.focus = parent;
                }
                self.selection = self.group(children);
                self.committed(dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// A group's change made by `op` as one history step, or its refusal shown.
    pub(crate) fn group_edit(
        &mut self,
        op: impl FnOnce(&mut Document) -> Result<(Vec<crate::history::Edit>, Rect), DocError>,
    ) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        match op(doc) {
            Ok((edits, _)) if edits.iter().all(|e| e.is_noop()) => Vec::new(),
            Ok((edits, dirty)) => {
                self.drop_stale_pixels(&edits);
                self.history.push(edits);
                self.committed(dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// After a committed change: dirty, the diagnostics, an upload.
    pub(crate) fn committed(&mut self, dirty: Rect) -> Vec<EditorCommand> {
        self.dirty = true;
        self.changes += 1;
        self.status.message = None;
        self.refresh_problems();
        vec![EditorCommand::Upload(dirty)]
    }

    /// Several edits of one operation as one step (the look's).
    pub(crate) fn edits(
        &mut self,
        op: impl FnOnce(&mut Document) -> Result<(Vec<Edit>, Rect), DocError>,
    ) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        match op(doc) {
            Ok((edits, _)) if edits.iter().all(Edit::is_noop) => Vec::new(),
            Ok((edits, dirty)) => {
                self.history.push(edits);
                self.committed(dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// A removal; the selection clears when it succeeds.
    fn removed(&mut self, op: impl FnOnce(&mut Document) -> Result<Change, DocError>) -> Vec<EditorCommand> {
        let cmds = self.edit(op);
        if !cmds.is_empty() {
            self.selection = Selection::None;
        }
        cmds
    }

    /// Undo (`back`) or redo one step.
    fn step(&mut self, back: bool) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        let step = if back { self.history.undo() } else { self.history.redo() };
        let Some(step) = step else { return Vec::new() };
        let mut dirty: Option<Rect> = None;
        let edits: Box<dyn Iterator<Item = _>> =
            if back { Box::new(step.iter().rev()) } else { Box::new(step.iter()) };
        for e in edits {
            let r = doc.apply(e, !back);
            if r.w > 0 && r.h > 0 {
                dirty = Some(Rect::union(dirty, r));
            }
        }
        let whole = step.iter().any(|e| matches!(e, Edit::Whole { .. }));
        // A selection the step removed goes.
        match &self.selection {
            Selection::Element(id) if doc.index_of(id).is_none() => self.selection = Selection::None,
            Selection::Folder(id) if !doc.is_folder(id) => self.selection = Selection::None,
            Selection::Object(i) if *i >= doc.level().objects.len() => self.selection = Selection::None,
            Selection::Many(ids) => self.selection = self.group(ids.clone()),
            _ => {}
        }
        if whole {
            self.drop_pixels();
        }
        self.committed(dirty.unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 }))
    }

    /// A whole change (art scale, resize, crop) moves every layer's pixels:
    /// the pixel selection, laid out for the old ones, goes (plan 15a's
    /// final review: a stale colour selection once overran its flags).
    fn drop_stale_pixels(&mut self, edits: &[Edit]) {
        if edits.iter().any(|e| matches!(e, Edit::Whole { .. })) {
            self.drop_pixels();
        }
    }

    fn drop_pixels(&mut self) {
        self.pixel_rect = None;
        self.pixel_region = None;
    }

    /// Open, New or Close (and issue #77's asset tab openings): asks first
    /// when the document it leaves has unsaved changes; Close asks for the
    /// asset tab's, then the level's.
    fn leave(&mut self, then: Pending) -> Vec<EditorCommand> {
        match then {
            Pending::EditAsset { .. } | Pending::NewAsset(_) | Pending::CloseAsset => {
                if self.asset_dirty() {
                    self.dialog = Some(Dialog::Unsaved { then });
                    return Vec::new();
                }
                self.carry_out(then)
            }
            Pending::Close if self.asset_dirty() => {
                self.dialog = Some(Dialog::Unsaved { then: Pending::CloseAsset });
                Vec::new()
            }
            then => {
                if self.dirty() && self.document().is_some() {
                    self.dialog = Some(Dialog::Unsaved { then });
                    return Vec::new();
                }
                self.carry_out(then)
            }
        }
    }

    /// What `then` asks of the host; the level left behind (closed, or
    /// replaced by another) drops its recovery copy first. The asset tab's
    /// are carried out here.
    fn carry_out(&mut self, then: Pending) -> Vec<EditorCommand> {
        let level = match then {
            Pending::EditAsset { key, asset } => return self.open_asset_tab(Some(key), &asset),
            Pending::NewAsset(n) => return self.open_asset_tab(None, &n.asset()),
            Pending::CloseAsset => {
                self.close_asset_tab();
                return self.leave(Pending::Close);
            }
            level => level,
        };
        let mut cmds = if self.document().is_some() { vec![EditorCommand::RecoveryDone] } else { Vec::new() };
        cmds.push(match level {
            Pending::Open(dir) => EditorCommand::Open(dir),
            Pending::New(n) => EditorCommand::Create(n),
            _ => {
                self.pixel_rect = None;
                self.pixel_region = None;
                EditorCommand::Close
            }
        });
        cmds
    }

    /// Puts `tab`'s session in the fields (the other one in `other`).
    fn place(&mut self, tab: DocTab) {
        if self.in_place != tab {
            swap_session!(self, self.other);
            self.in_place = tab;
        }
    }

    /// `f` on `tab`'s session, the shown one's back in place after.
    fn on_tab<R>(&mut self, tab: DocTab, f: impl FnOnce(&mut Self) -> R) -> R {
        self.place(tab);
        let r = f(self);
        self.place(self.shown);
        r
    }

    /// Shows `tab` (issue #77): its session in place, its distant layers
    /// sent again and the canvas uploaded whole.
    fn switch_tab(&mut self, tab: DocTab) {
        if self.shown != tab {
            self.shown = tab;
            self.placing = false;
            self.shown_generation += 1;
        }
        self.place(tab);
        self.distant_sent = None;
    }

    /// The asset tab with `asset` open, read from the library's `key` (none
    /// for a new asset, unsaved till saved): a fresh session, shown.
    fn open_asset_tab(
        &mut self,
        key: Option<AssetKey>,
        asset: &olr_edit::asset::Asset,
    ) -> Vec<EditorCommand> {
        let theme = self.document().map(|d| d.level().theme.clone()).unwrap_or_default();
        let doc = match Document::open_asset(asset, &theme, &self.catalog) {
            Ok(doc) => doc,
            Err(e) => {
                self.dialog = Some(Dialog::Error(e.to_string()));
                return Vec::new();
            }
        };
        let all = doc.all();
        self.close_asset_tab();
        self.switch_tab(DocTab::Asset);
        self.shown_generation += 1;
        self.doc = Some(doc);
        self.dirty = key.is_none();
        self.status.message = key.as_ref().filter(|k| k.builtin).map(|_| BUILT_IN_NOTE.to_string());
        self.asset_key = key;
        self.fit_pending = true;
        self.fit();
        self.refresh_problems();
        vec![EditorCommand::Upload(all)]
    }

    /// Empties the asset tab (its session made anew), showing the level.
    fn close_asset_tab(&mut self) {
        self.on_tab(DocTab::Asset, |s| {
            let mut fresh = Session::new();
            swap_session!(s, fresh);
        });
        self.asset_key = None;
        self.tab_save = None;
        if self.shown == DocTab::Asset {
            self.switch_tab(DocTab::Level);
        }
    }

    /// Saves the asset tab's asset (issue #77) into the user's library,
    /// then carries out `then`: the user's own over its folder; a built-in
    /// or new one as a new asset of the user's, a name the user has asking
    /// Replace or Keep both first. A built-in folder is never written.
    fn save_asset(&mut self, then: Option<Pending>) -> Vec<EditorCommand> {
        let made = self.on_tab(DocTab::Asset, |s| s.doc.as_ref().map(|d| (d.to_asset(), s.changes)));
        let Some((asset, changes)) = made else { return Vec::new() };
        match self.asset_key.clone().filter(|k| !k.builtin) {
            Some(k) => {
                self.tab_save = Some((changes, then));
                vec![EditorCommand::SaveAsset { asset: Box::new(asset), replace: Some(k.id) }]
            }
            None => {
                let clash = self.catalog.assets.iter().find(|a| !a.key.builtin && a.name == asset.name);
                let asks = clash.map(|a| Dialog::AssetExists { name: a.name.clone(), id: a.key.id.clone() });
                self.pending_asset = Some(Box::new(asset));
                self.pending_tab = Some((changes, then));
                match asks {
                    Some(d) => {
                        self.dialog = Some(d);
                        Vec::new()
                    }
                    None => self.send_asset(None),
                }
            }
        }
    }

    /// The asset tab's save done (`AssetSaved`): the tab edits the saved
    /// key from now on; what waited on the save is carried out.
    fn tab_saved(&mut self, key: AssetKey) -> Vec<EditorCommand> {
        let Some((sent, then)) = self.tab_save.take() else { return Vec::new() };
        let name = self.asset(&key).map_or_else(|| key.id.clone(), |a| a.name.clone());
        self.asset_key = Some(key);
        self.on_tab(DocTab::Asset, |s| {
            // A change made after the save was sent is not in the saved asset.
            s.dirty = s.changes != sent;
            s.status.message = Some(format!("saved the asset {name:?} in your assets"));
        });
        then.map(|t| self.carry_out(t)).unwrap_or_default()
    }

    /// Issue #77: refuses the asset tab's step(s) just made when they leave
    /// the asset's states or exposure invalid (`Document::asset_refusal`):
    /// the steps reverted and forgotten, the unsaved mark, selection and
    /// focus as they were, the status line saying why.
    fn guard(
        &mut self,
        (pushes, dirty, changes, selection, focus): GuardMark,
        cmds: &mut Vec<EditorCommand>,
    ) {
        let Some(doc) = self.doc.as_mut() else { return };
        let Some(why) = doc.asset_refusal() else { return };
        while self.history.pushes() > pushes {
            let Some(step) = self.history.retract() else { break };
            for e in step.iter().rev() {
                doc.apply(e, false);
            }
            if step.iter().any(|e| matches!(e, Edit::Whole { .. })) {
                self.pixel_rect = None;
                self.pixel_region = None;
            }
        }
        let all = doc.all();
        (self.dirty, self.changes, self.selection, self.focus) = (dirty, changes, selection, focus);
        self.refresh_problems();
        self.status.message = Some(why);
        cmds.retain(|c| !matches!(c, EditorCommand::Upload(_)));
        cmds.push(EditorCommand::Upload(all));
    }

    fn save(&mut self, then: Option<Pending>) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        self.saves.push_back((self.changes, then));
        vec![EditorCommand::Save { dir: doc.dir().to_path_buf(), pack: Box::new(doc.to_pack()) }]
    }

    fn saved(&mut self, result: Result<Vec<String>, String>) -> Vec<EditorCommand> {
        // Answers come in the order the saves were sent.
        let (sent, then) = self.saves.pop_front().unwrap_or((self.changes, None));
        match result {
            Ok(warnings) => {
                // A change made after the save was sent is not in the saved pack.
                self.dirty = self.changes != sent;
                self.status.message = Some(if warnings.is_empty() {
                    "saved".into()
                } else {
                    format!("saved; {}", warnings.join("; "))
                });
                let mut cmds = vec![EditorCommand::RecoveryDone];
                cmds.extend(then.map(|t| self.carry_out(t)).unwrap_or_default());
                cmds.dedup();
                cmds
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error(e));
                Vec::new()
            }
        }
    }

    fn choose(&mut self, choice: Choice) -> Vec<EditorCommand> {
        let Some(dialog) = self.dialog.take() else { return Vec::new() };
        match (dialog, choice) {
            (Dialog::Unsaved { then }, Choice::Save) => match then {
                Pending::EditAsset { .. } | Pending::NewAsset(_) | Pending::CloseAsset => {
                    self.save_asset(Some(then))
                }
                then => self.on_tab(DocTab::Level, |s| s.save(Some(then))),
            },
            (Dialog::Unsaved { then }, Choice::Discard) => self.carry_out(then),
            (Dialog::ArtScale { to }, Choice::Confirm) => self.edit(|d| d.set_art_scale(to)),
            (Dialog::RemoveGroup { id }, Choice::Confirm) => self.edits(|d| d.remove_group(&id)),
            (Dialog::AssetExists { id, .. }, Choice::Confirm) => self.send_asset(Some(id)),
            (Dialog::AssetExists { name, .. }, Choice::KeepBoth) => {
                let yours: Vec<String> =
                    self.catalog.assets.iter().filter(|a| !a.key.builtin).map(|a| a.name.clone()).collect();
                let name = olr_edit::element::copy_name(&name, yours.iter().map(String::as_str));
                if let Some(a) = self.pending_asset.as_mut() {
                    a.name = name;
                }
                self.send_asset(None)
            }
            (Dialog::RemoveAsset { id, .. }, Choice::Confirm) => vec![EditorCommand::RemoveAsset(id)],
            (Dialog::AssetName { .. } | Dialog::AssetExists { .. }, Choice::Cancel) => {
                self.pending_asset = None;
                self.pending_tab = None;
                Vec::new()
            }
            (Dialog::Error(_), _) | (_, Choice::Cancel) => Vec::new(),
            // Any other answer leaves the question open.
            (dialog, _) => {
                self.dialog = Some(dialog);
                Vec::new()
            }
        }
    }
}
