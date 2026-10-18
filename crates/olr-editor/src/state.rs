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

use crate::browser::{Browser, DirEntry, Purpose};
use crate::catalog::{Catalog, EditorSettings, TextureEntry};
use crate::clipboard::{Clipboard, PASTE_STEP};
use crate::document::{DocError, Document, Problem};
use crate::history::{History, LevelMeta};
use crate::ops::Change;
use crate::pointer::Drag;
use crate::tools::{Brush, MAX_BRUSH, PaintChoice, PointerKind, TextureRef, Tool};
use crate::view::View;

/// What is selected.
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    None,
    Element(String),
    /// A level object, by index.
    Object(usize),
    /// Two elements or more, in stack order, bottom first (plan 14e R8).
    Many(Vec<String>),
}

impl Selection {
    /// Whether element `id` is selected, alone or in a group.
    pub fn has(&self, id: &str) -> bool {
        match self {
            Selection::Element(e) => e == id,
            Selection::Many(ids) => ids.iter().any(|e| e == id),
            _ => false,
        }
    }

    /// The selected elements' ids (none for an object).
    pub fn ids(&self) -> Vec<String> {
        match self {
            Selection::Element(e) => vec![e.clone()],
            Selection::Many(ids) => ids.clone(),
            _ => Vec::new(),
        }
    }
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
}

/// What waits on the unsaved-changes dialog.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    Open(PathBuf),
    New(NewLevel),
    Close,
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
    /// Ctrl+click: element `id` in or out of the selection.
    SelectToggle(String),
    /// Shift+click: the elements from the single selection to `id`.
    SelectRange(String),
    /// Deletes the selected element(s) or object.
    Remove,
    /// Copies the selected elements to the editor's clipboard (plan 14e R6).
    Copy,
    /// Copies, then deletes them: one step.
    Cut,
    /// Pastes the clipboard on top, selected: one step.
    Paste,
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
}

/// What the host carries out.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorCommand {
    /// Re-upload this world rectangle of the bake (the canvas, in 14c).
    Upload(Rect),
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
    pub(crate) grid: bool,
    /// The pointer drag in progress.
    pub(crate) drag: Option<Drag>,
    /// Changes committed so far (and undone or redone).
    pub(crate) changes: u64,
    /// The file browser, while it is open.
    pub(crate) browser: Option<Browser>,
    /// What was copied; kept across Open, New and Close (plan 14e R6).
    pub(crate) clipboard: Option<Clipboard>,
}

impl EditorState {
    pub fn new(settings: EditorSettings, catalog: Catalog) -> EditorState {
        EditorState {
            settings,
            catalog,
            doc: None,
            history: History::new(),
            dirty: false,
            playing: false,
            dialog: None,
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
            builtin: Arc::new(olr_edit::textures::builtin()),
            textures_generation: 0,
            view: View { zoom: 2, offset: [0.0, 0.0] },
            canvas: None,
            fit_pending: false,
            material_view: false,
            grid: false,
            drag: None,
            changes: 0,
            browser: None,
            clipboard: None,
        }
    }

    pub fn document(&self) -> Option<&Document> {
        self.doc.as_ref()
    }

    pub fn settings(&self) -> &EditorSettings {
        &self.settings
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// Whether there are unsaved changes.
    pub fn dirty(&self) -> bool {
        self.dirty
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
                | EditorAction::Previewed { .. }
                | EditorAction::Pointer { .. }
                | EditorAction::Canvas(_)
                | EditorAction::Pan(_)
                | EditorAction::Zoom { .. }
                | EditorAction::MaterialView(_)
                | EditorAction::Grid(_)
        );
        if self.dialog.is_some() && waits {
            return Vec::new();
        }
        // An action that can change the document ends a drag first, so the
        // drag's step is recorded against the document it changed.
        let view_only = matches!(
            action,
            EditorAction::Pointer { .. }
                | EditorAction::Canvas(_)
                | EditorAction::Pan(_)
                | EditorAction::Zoom { .. }
                | EditorAction::MaterialView(_)
                | EditorAction::Grid(_)
                | EditorAction::Brush(_)
                | EditorAction::Paint(_)
                | EditorAction::PaintMaterial(_)
                | EditorAction::Restore(_)
                | EditorAction::Tolerance(_)
                | EditorAction::TexturesChanged(_)
                | EditorAction::RemoveTexture(_)
                | EditorAction::Select(_)
                | EditorAction::SelectToggle(_)
                | EditorAction::SelectRange(_)
                | EditorAction::Copy
                | EditorAction::PlayMod(_)
                | EditorAction::PlayBots(_)
                | EditorAction::Listed { .. }
                | EditorAction::Note(_)
                | EditorAction::PreviewImage(_)
                | EditorAction::Previewed { .. }
                | EditorAction::Browse(_)
                | EditorAction::BrowseTo(_)
                | EditorAction::CloseBrowser
        );
        let mut cmds = if view_only { Vec::new() } else { self.release() };
        cmds.extend(self.act(action));
        cmds
    }

    fn act(&mut self, action: EditorAction) -> Vec<EditorCommand> {
        match action {
            EditorAction::Opened(doc) => self.opened(doc),
            EditorAction::Recovered(doc) => {
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
            EditorAction::Saved(result) => self.saved(result),
            EditorAction::PlayEnded => Vec::new(),
            EditorAction::Open(dir) => self.leave(Pending::Open(dir)),
            EditorAction::New(n) => self.leave(Pending::New(n)),
            EditorAction::Close => self.leave(Pending::Close),
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
                Vec::new()
            }
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
                    Selection::Element(a) => Some(a.clone()),
                    _ => None,
                };
                let range = self.doc.as_ref().and_then(|d| {
                    let (a, b) = (d.index_of(anchor.as_deref()?)?, d.index_of(&id)?);
                    Some(d.items()[a.min(b)..=a.max(b)].iter().map(|i| i.element.id.clone()).collect())
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
            EditorAction::Paste => self.paste(),
            EditorAction::Set(id, property) => self.edit(|d| d.set(&id, property)),
            EditorAction::SetMany(ids, property) => self.group_edit(|d| d.set_many(&ids, property)),
            EditorAction::Remove => match self.selection.clone() {
                Selection::Element(id) => self.removed(|d| d.remove(&id)),
                Selection::Object(i) => self.removed(|d| d.remove_object(i)),
                Selection::Many(ids) => {
                    let cmds = self.group_edit(|d| d.remove_many(&ids));
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
            EditorAction::Meta(meta) => {
                let catalog = self.catalog.clone();
                self.edit(|d| d.set_meta(meta, &catalog))
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
            EditorAction::Tool(tool) => {
                if tool == Tool::Eyedropper && self.tool != Tool::Eyedropper {
                    self.prev_tool = self.tool;
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
                Vec::new()
            }
            EditorAction::Grid(on) => {
                self.grid = on;
                Vec::new()
            }
        }
    }

    /// A document opened (or recovered): a fresh history and view.
    fn opened(&mut self, doc: Box<Document>) -> Vec<EditorCommand> {
        let all = doc.all();
        self.doc = Some(*doc);
        self.fit_pending = true;
        self.fit();
        self.history = History::new();
        self.selection = Selection::None;
        self.dirty = false;
        self.dialog = None;
        self.status.message = None;
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

    pub fn grid(&self) -> bool {
        self.grid
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
                self.history.push(vec![change.edit]);
                self.committed(change.dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// Copies the selected elements to the clipboard (plan 14e R6); whether
    /// there were any.
    fn copy(&mut self) -> bool {
        let Some(doc) = &self.doc else { return false };
        let items = doc.copy(&self.selection.ids());
        if items.is_empty() {
            return false;
        }
        let n = items.len();
        self.clipboard =
            Some(Clipboard::Elements { art_scale: doc.level().art_scale, items, offset: PASTE_STEP });
        self.status.message = Some(match n {
            1 => "copied 1 element".into(),
            n => format!("copied {n} elements"),
        });
        true
    }

    /// Pastes the clipboard on top as one step, the copies selected; the
    /// next paste lands `PASTE_STEP` further.
    fn paste(&mut self) -> Vec<EditorCommand> {
        let (Some(doc), Some(clip)) = (self.doc.as_mut(), self.clipboard.as_mut()) else { return Vec::new() };
        let Clipboard::Elements { art_scale, items, offset } = clip;
        match doc.paste(items, *art_scale, *offset) {
            Ok((edits, ids)) => {
                *offset = (offset.0.saturating_add(PASTE_STEP.0), offset.1.saturating_add(PASTE_STEP.1));
                let dirty = doc.all();
                self.history.push(edits);
                self.selection = self.group(ids);
                self.committed(dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                Vec::new()
            }
        }
    }

    /// The selection of `ids` (plan 14e R8): those that exist, each once, in
    /// stack order; one is `Element`, none `None`.
    pub(crate) fn group(&self, ids: Vec<String>) -> Selection {
        let Some(doc) = &self.doc else { return Selection::None };
        let mut found: Vec<(usize, String)> =
            ids.into_iter().filter_map(|id| Some((doc.index_of(&id)?, id))).collect();
        found.sort();
        found.dedup();
        match found.len() {
            0 => Selection::None,
            1 => Selection::Element(found.remove(0).1),
            _ => Selection::Many(found.into_iter().map(|f| f.1).collect()),
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
        // A selection the step removed goes.
        match &self.selection {
            Selection::Element(id) if doc.index_of(id).is_none() => self.selection = Selection::None,
            Selection::Object(i) if *i >= doc.level().objects.len() => self.selection = Selection::None,
            Selection::Many(ids) => self.selection = self.group(ids.clone()),
            _ => {}
        }
        self.committed(dirty.unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 }))
    }

    /// Open, New or Close: asks first when there are unsaved changes.
    fn leave(&mut self, then: Pending) -> Vec<EditorCommand> {
        if self.dirty && self.doc.is_some() {
            self.dialog = Some(Dialog::Unsaved { then });
            return Vec::new();
        }
        self.carry_out(then)
    }

    /// What `then` asks of the host; the document left behind (closed, or
    /// replaced by another) drops its recovery copy first.
    fn carry_out(&mut self, then: Pending) -> Vec<EditorCommand> {
        let mut cmds = if self.doc.is_some() { vec![EditorCommand::RecoveryDone] } else { Vec::new() };
        cmds.push(match then {
            Pending::Open(dir) => EditorCommand::Open(dir),
            Pending::New(n) => EditorCommand::Create(n),
            Pending::Close => EditorCommand::Close,
        });
        cmds
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
            (Dialog::Unsaved { then }, Choice::Save) => self.save(Some(then)),
            (Dialog::Unsaved { then }, Choice::Discard) => self.carry_out(then),
            (Dialog::ArtScale { to }, Choice::Confirm) => self.edit(|d| d.set_art_scale(to)),
            (Dialog::Error(_), _) | (_, Choice::Cancel) => Vec::new(),
            // Any other answer leaves the question open.
            (dialog, _) => {
                self.dialog = Some(dialog);
                Vec::new()
            }
        }
    }
}
