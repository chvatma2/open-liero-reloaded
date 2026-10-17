//! The editor's state (spec §4, `olr-ui`'s pattern):
//! `EditorState::update(EditorAction) -> Vec<EditorCommand>` is pure. A
//! host carries out the commands (open, save, play) and answers with
//! actions (`Opened`, `Saved`, `PlayEnded`).

use std::path::PathBuf;

use olr_data::image::Rgba;
use olr_data::level::Material;
use olr_edit::compose::Rect;
use olr_edit::element::Property;
use olr_edit::pack::Pack;
use olr_format::levels::{ElementMaterial, LevelSource};

use crate::catalog::{Catalog, EditorSettings};
use crate::document::{DocError, Document, Problem};
use crate::history::{History, LevelMeta};
use crate::ops::Change;
use crate::pointer::Drag;
use crate::tools::{Brush, MAX_BRUSH, PaintChoice, PointerKind, Tool};
use crate::view::View;

/// What is selected.
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    None,
    Element(String),
    /// A level object, by index.
    Object(usize),
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
    /// Deletes the selected element or object.
    Remove,
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
    /// A pointer event on the canvas, in world pixels (ruling R7).
    Pointer {
        kind: PointerKind,
        world: (i32, i32),
        shift: bool,
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
    /// The editor is done (after any unsaved-changes dialog).
    Close,
}

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
    /// What a save in progress carries out once it succeeds.
    pub(crate) after_save: Option<Pending>,
    pub(crate) tool: Tool,
    /// The tool the eyedropper returns to.
    pub(crate) prev_tool: Tool,
    pub(crate) brush: Brush,
    pub(crate) paint: PaintChoice,
    pub(crate) view: View,
    /// The canvas's size in points, once the UI reported it.
    pub(crate) canvas: Option<[f32; 2]>,
    /// The view fits the level on the next `Canvas`.
    pub(crate) fit_pending: bool,
    pub(crate) material_view: bool,
    pub(crate) grid: bool,
    /// The pointer drag in progress.
    pub(crate) drag: Option<Drag>,
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
            after_save: None,
            tool: Tool::Select,
            prev_tool: Tool::Select,
            brush: Brush::default(),
            paint: PaintChoice::default(),
            view: View { zoom: 2, offset: [0.0, 0.0] },
            canvas: None,
            fit_pending: false,
            material_view: false,
            grid: false,
            drag: None,
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
                | EditorAction::Select(_)
                | EditorAction::PlayMod(_)
                | EditorAction::PlayBots(_)
        );
        let mut cmds = if view_only { Vec::new() } else { self.release() };
        cmds.extend(self.act(action));
        cmds
    }

    fn act(&mut self, action: EditorAction) -> Vec<EditorCommand> {
        match action {
            EditorAction::Opened(doc) => {
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
                self.selection = selection;
                Vec::new()
            }
            EditorAction::Set(id, property) => self.edit(|d| d.set(&id, property)),
            EditorAction::Remove => match self.selection.clone() {
                Selection::Element(id) => self.removed(|d| d.remove(&id)),
                Selection::Object(i) => self.removed(|d| d.remove_object(i)),
                Selection::None => Vec::new(),
            },
            EditorAction::MoveElement { id, to } => self.edit(|d| d.move_to(&id, to)),
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
                self.paint = p;
                Vec::new()
            }
            EditorAction::Pointer { kind, world, shift } => self.pointer(kind, world, shift),
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

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn brush(&self) -> Brush {
        self.brush
    }

    pub fn paint(&self) -> PaintChoice {
        self.paint
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

    /// After a committed change: dirty, the diagnostics, an upload.
    pub(crate) fn committed(&mut self, dirty: Rect) -> Vec<EditorCommand> {
        self.dirty = true;
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

    fn carry_out(&mut self, then: Pending) -> Vec<EditorCommand> {
        match then {
            Pending::Open(dir) => vec![EditorCommand::Open(dir)],
            Pending::New(n) => vec![EditorCommand::Create(n)],
            Pending::Close => vec![EditorCommand::Close],
        }
    }

    fn save(&mut self, then: Option<Pending>) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        self.after_save = then;
        vec![EditorCommand::Save { dir: doc.dir().to_path_buf(), pack: Box::new(doc.to_pack()) }]
    }

    fn saved(&mut self, result: Result<Vec<String>, String>) -> Vec<EditorCommand> {
        let then = self.after_save.take();
        match result {
            Ok(warnings) => {
                self.dirty = false;
                self.status.message = Some(if warnings.is_empty() {
                    "saved".into()
                } else {
                    format!("saved; {}", warnings.join("; "))
                });
                then.map(|t| self.carry_out(t)).unwrap_or_default()
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
