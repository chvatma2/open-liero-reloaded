//! The skin editor's state (issue #14): `SkinEditorState::update(
//! SkinAction) -> Vec<SkinCommand>` is pure; the host carries out the
//! commands (`Save`, `Close`) and answers (`Saved`, `Failed`).

use olr_data::image::Rgba;
use olr_data::skin::SkinLight;
use olr_look::engine::{MAX_SKIN_LIGHT_INTENSITY, MAX_SKIN_LIGHT_REACH};

use crate::anim::Anim;
use crate::doc::{AIM_FRAMES, FRAMES, Frame, SkinDoc, WALK_FRAMES};
use crate::history::{Edit, History};
use crate::paint::{self, Ink, Target};

/// The most recent colours kept.
pub const MAX_RECENT: usize = 12;
/// The view's zooms, points per art pixel.
pub const ZOOMS: [f32; 8] = [2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0];
/// From this zoom up the canvas draws a grid between the art pixels.
pub const GRID_ZOOM: f32 = 6.0;
/// The glow strengths offered: the game draws a glow in quarters
/// (`olr_data::skin::glow_level`).
pub const GLOW_STRENGTHS: [u8; 4] = [64, 128, 192, 255];
/// A skin light when it is turned on (Firefly's reach).
pub const DEFAULT_LIGHT: SkinLight = SkinLight { intensity: 80, reach: 24 };
/// The preview's player colour at first (OLX's first player's).
pub const DEFAULT_PLAYER_COLOUR: Rgba = Rgba::rgb(200, 60, 40);
/// A pointer far off the frame is held this far out (a line stays short).
const FAR: i32 = 512;

/// The paint tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pencil,
    Eraser,
    Fill,
    Eyedropper,
    Line,
    Rect,
}

impl Tool {
    pub const ALL: [Tool; 6] =
        [Tool::Pencil, Tool::Eraser, Tool::Fill, Tool::Eyedropper, Tool::Line, Tool::Rect];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Pencil => "Pencil",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill",
            Tool::Eyedropper => "Eyedropper",
            Tool::Line => "Line",
            Tool::Rect => "Rectangle",
        }
    }

    /// Its shortcut.
    pub fn key(self) -> char {
        match self {
            Tool::Pencil => 'B',
            Tool::Eraser => 'E',
            Tool::Fill => 'G',
            Tool::Eyedropper => 'I',
            Tool::Line => 'L',
            Tool::Rect => 'R',
        }
    }

    /// Whether the brush size applies.
    pub fn sized(self) -> bool {
        matches!(self, Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Rect)
    }
}

/// The neighbouring frame shown faintly under the one painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Onion {
    #[default]
    Off,
    /// The previous aim angle (the column to the left).
    PreviousAim,
    /// The previous walk frame (the row above, the last row for the first).
    PreviousWalk,
}

impl Onion {
    pub const ALL: [Onion; 3] = [Onion::Off, Onion::PreviousAim, Onion::PreviousWalk];

    pub fn name(self) -> &'static str {
        match self {
            Onion::Off => "Off",
            Onion::PreviousAim => "Previous aim",
            Onion::PreviousWalk => "Previous walk",
        }
    }

    /// The frame shown under frame `f`, if any: the first aim has none
    /// before it, the walk wraps (the walk cycles).
    pub fn of(self, f: usize) -> Option<usize> {
        let (aim, walk) = (f % AIM_FRAMES, f / AIM_FRAMES);
        match self {
            Onion::Off => None,
            Onion::PreviousAim => (aim > 0).then(|| f - 1),
            Onion::PreviousWalk => Some(((walk + WALK_FRAMES - 1) % WALK_FRAMES) * AIM_FRAMES + aim),
        }
    }
}

/// Where the copied frame is pasted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteTo {
    /// The chosen frame.
    Frame,
    /// Every aim of the chosen frame's walk row.
    Row,
    /// Every walk frame of the chosen frame's aim column.
    Column,
}

/// A question or a message over the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// The first save's name.
    Name,
    /// Close with unsaved changes: save, don't save or cancel.
    Unsaved,
    /// What the host could not do.
    Error(String),
}

/// A dialog's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Save,
    DontSave,
    Cancel,
}

/// What the panels (and the host) ask.
#[derive(Clone, Debug, PartialEq)]
pub enum SkinAction {
    /// Choose frame `f` to paint.
    Frame(usize),
    Tool(Tool),
    /// The brush size, 1 or 2.
    Size(u32),
    Target(Target),
    /// The glow painted, 1 to 255.
    GlowStrength(u8),
    /// The paint colour (opaque).
    Colour(Rgba),
    Onion(Onion),
    /// The pointer on the large frame, art pixels from its top left.
    Press {
        x: i32,
        y: i32,
    },
    Drag {
        x: i32,
        y: i32,
    },
    Release,
    CopyFrame,
    Paste(PasteTo),
    /// Mirror the chosen frame (left to right, or top to bottom).
    Flip {
        vertical: bool,
    },
    /// Move the chosen frame (or every frame, `shift_all`) by one pixel.
    Shift {
        dx: i32,
        dy: i32,
    },
    ShiftAll(bool),
    SetName(String),
    /// The skin light, `None` off; clamped to the engine's caps.
    SetLight(Option<SkinLight>),
    /// A classic skin to double resolution (Scale2x, `olr_edit::skin::double`).
    MakeDouble,
    Undo,
    Redo,
    Save,
    /// The name dialog's answer: save under this name.
    SaveAs(String),
    Close,
    Choice(Choice),
    /// The preview's player colour.
    PlayerColour(Rgba),
    /// Whether the preview walks and aims, or shows the chosen frame.
    Animate(bool),
    /// The host's frame time, seconds (the preview's animation).
    Tick(f32),
    /// Zoom in (positive) or out by steps.
    Zoom(i32),
    /// Pan the large frame by points.
    Pan(f32, f32),
    /// The host saved the skin as pack `id`.
    Saved(String),
    /// The host could not do what was asked: an error dialog.
    Failed(String),
}

/// What the host carries out.
#[derive(Clone, Debug, PartialEq)]
pub enum SkinCommand {
    /// Write the skin: into its own pack `id` when it has one, else as a
    /// new pack named `name` (the name asked) in the user's packs.
    Save { id: Option<String>, name: String, source: Box<olr_format::skins::SkinSource> },
    /// Close the editor.
    Close,
}

/// A stroke or a drag in progress.
#[derive(Clone, Debug)]
struct Stroke {
    frame: usize,
    before: Frame,
    start: (i32, i32),
    last: (i32, i32),
}

/// The skin editor.
#[derive(Debug)]
pub struct SkinEditorState {
    doc: SkinDoc,
    history: History,
    /// The pack the skin saves to, once it has one.
    pack_id: Option<String>,
    /// The step saved last (`History::at`).
    saved_at: u64,
    /// A save is with the host, of the step `pending_at`.
    saving: bool,
    pending_at: u64,
    /// Close once the save is done.
    close_after_save: bool,
    frame: usize,
    tool: Tool,
    size: u32,
    target: Target,
    glow_strength: u8,
    colour: Rgba,
    recent: Vec<Rgba>,
    onion: Onion,
    clipboard: Option<Frame>,
    shift_all: bool,
    stroke: Option<Stroke>,
    zoom: f32,
    pan: (f32, f32),
    player_colour: Rgba,
    animate: bool,
    anim: Anim,
    dialog: Option<Dialog>,
    status: String,
}

impl SkinEditorState {
    /// The editor on `doc`, saving to pack `pack_id` (the user's own), or
    /// asking a name at the first save without one (a new skin, or a
    /// copy of another).
    pub fn new(doc: SkinDoc, pack_id: Option<String>) -> SkinEditorState {
        let zoom = if doc.is_classic() { 16.0 } else { 8.0 };
        SkinEditorState {
            doc,
            history: History::default(),
            pack_id,
            saved_at: 0,
            saving: false,
            pending_at: 0,
            close_after_save: false,
            frame: 4,
            tool: Tool::Pencil,
            size: 1,
            target: Target::Colour,
            glow_strength: 255,
            colour: Rgba::rgb(255, 255, 255),
            recent: Vec::new(),
            onion: Onion::Off,
            clipboard: None,
            shift_all: false,
            stroke: None,
            zoom,
            pan: (0.0, 0.0),
            player_colour: DEFAULT_PLAYER_COLOUR,
            animate: true,
            anim: Anim::default(),
            dialog: None,
            status: String::new(),
        }
    }

    pub fn doc(&self) -> &SkinDoc {
        &self.doc
    }
    pub fn pack_id(&self) -> Option<&str> {
        self.pack_id.as_deref()
    }
    /// Unsaved changes, or a skin never saved that was changed.
    pub fn dirty(&self) -> bool {
        self.history.at() != self.saved_at
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    pub fn frame(&self) -> usize {
        self.frame
    }
    pub fn tool(&self) -> Tool {
        self.tool
    }
    pub fn size(&self) -> u32 {
        self.size
    }
    pub fn target(&self) -> Target {
        self.target
    }
    pub fn glow_strength(&self) -> u8 {
        self.glow_strength
    }
    pub fn colour(&self) -> Rgba {
        self.colour
    }
    pub fn recent(&self) -> &[Rgba] {
        &self.recent
    }
    pub fn onion(&self) -> Onion {
        self.onion
    }
    pub fn clipboard(&self) -> Option<&Frame> {
        self.clipboard.as_ref()
    }
    pub fn shift_all(&self) -> bool {
        self.shift_all
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    pub fn pan(&self) -> (f32, f32) {
        self.pan
    }
    pub fn player_colour(&self) -> Rgba {
        self.player_colour
    }
    pub fn animate(&self) -> bool {
        self.animate
    }
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    /// A stroke or drag is in progress.
    pub fn stroking(&self) -> bool {
        self.stroke.is_some()
    }
    /// What the preview shows: its frame and whether it faces left.
    pub fn preview_pose(&self) -> (usize, bool) {
        if self.animate { (self.anim.frame(), self.anim.left) } else { (self.frame, false) }
    }

    /// Carries out `a`; returns what the host must do.
    pub fn update(&mut self, a: SkinAction) -> Vec<SkinCommand> {
        // The host's answers and the view act whatever is open.
        match a {
            SkinAction::Saved(id) => return self.saved(id),
            SkinAction::Failed(why) => {
                self.saving = false;
                self.close_after_save = false;
                self.dialog = Some(Dialog::Error(why));
                return Vec::new();
            }
            SkinAction::Tick(dt) => {
                if self.animate {
                    self.anim.step(dt);
                }
                return Vec::new();
            }
            SkinAction::Zoom(steps) => {
                self.zoom_by(steps);
                return Vec::new();
            }
            SkinAction::Pan(dx, dy) => {
                self.pan = (self.pan.0 + dx, self.pan.1 + dy);
                return Vec::new();
            }
            _ => {}
        }
        if let Some(d) = &self.dialog {
            return match (d, a) {
                (Dialog::Name, SkinAction::SaveAs(name)) => self.save_as(name),
                (Dialog::Unsaved, SkinAction::Choice(Choice::Save)) => {
                    self.dialog = None;
                    self.close_after_save = true;
                    self.save()
                }
                (Dialog::Unsaved, SkinAction::Choice(Choice::DontSave)) => {
                    self.dialog = None;
                    vec![SkinCommand::Close]
                }
                (_, SkinAction::Choice(_)) => {
                    self.dialog = None;
                    self.close_after_save = false;
                    Vec::new()
                }
                _ => Vec::new(),
            };
        }
        // A stroke ends before anything else changes the skin.
        if self.stroke.is_some() && !matches!(a, SkinAction::Drag { .. } | SkinAction::Release) {
            self.end_stroke();
        }
        match a {
            SkinAction::Frame(f) if f < FRAMES => self.frame = f,
            SkinAction::Tool(t) => self.tool = t,
            SkinAction::Size(s) => self.size = s.clamp(1, 2),
            SkinAction::Target(t) => self.target = t,
            SkinAction::GlowStrength(g) => self.glow_strength = g.max(1),
            SkinAction::Colour(c) => self.colour = Rgba { a: 255, ..c },
            SkinAction::Onion(o) => self.onion = o,
            SkinAction::Press { x, y } => self.press(x, y),
            SkinAction::Drag { x, y } => self.drag(x, y),
            SkinAction::Release => self.end_stroke(),
            SkinAction::CopyFrame => {
                self.clipboard = Some(self.doc.frame(self.frame));
                self.status = format!("Copied {}", frame_name(self.frame));
            }
            SkinAction::Paste(to) => self.paste(to),
            SkinAction::Flip { vertical } => {
                let f = self.frame;
                let before = self.doc.frame(f);
                let after = before.flipped(vertical);
                self.apply(Edit::Frames(vec![(f, before, after)]));
            }
            SkinAction::Shift { dx, dy } => {
                let frames: Vec<usize> =
                    if self.shift_all { (0..FRAMES).collect() } else { vec![self.frame] };
                let edit = frames
                    .into_iter()
                    .map(|f| {
                        let before = self.doc.frame(f);
                        let after = before.shifted(dx.signum(), dy.signum());
                        (f, before, after)
                    })
                    .collect();
                self.apply(Edit::Frames(edit));
            }
            SkinAction::ShiftAll(on) => self.shift_all = on,
            SkinAction::SetName(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    let light = self.doc.light();
                    self.apply(Edit::Settings {
                        before: (self.doc.name().to_string(), light),
                        after: (name, light),
                    });
                }
            }
            SkinAction::SetLight(light) => {
                let light = light.map(|l| SkinLight {
                    intensity: l.intensity.min(MAX_SKIN_LIGHT_INTENSITY),
                    reach: l.reach.clamp(1, MAX_SKIN_LIGHT_REACH),
                });
                let name = self.doc.name().to_string();
                self.apply(Edit::Settings { before: (name.clone(), self.doc.light()), after: (name, light) });
            }
            SkinAction::MakeDouble => self.make_double(),
            SkinAction::Undo => {
                if self.history.undo(&mut self.doc) {
                    self.status = "Undone".into();
                }
            }
            SkinAction::Redo => {
                if self.history.redo(&mut self.doc) {
                    self.status = "Redone".into();
                }
            }
            SkinAction::Save => return self.save(),
            SkinAction::Close => {
                if self.dirty() {
                    self.dialog = Some(Dialog::Unsaved);
                } else {
                    return vec![SkinCommand::Close];
                }
            }
            SkinAction::PlayerColour(c) => self.player_colour = Rgba { a: 255, ..c },
            SkinAction::Animate(on) => self.animate = on,
            _ => {}
        }
        Vec::new()
    }

    /// Runs `edit` and records it, unless it changes nothing.
    fn apply(&mut self, edit: Edit) {
        if edit.is_noop() {
            return;
        }
        edit.apply(&mut self.doc, true);
        self.history.push(edit);
    }

    fn zoom_by(&mut self, steps: i32) {
        let i = ZOOMS.iter().position(|&z| z >= self.zoom).unwrap_or(ZOOMS.len() - 1) as i32;
        self.zoom = ZOOMS[(i + steps).clamp(0, ZOOMS.len() as i32 - 1) as usize];
    }

    /// What the tool paints, for the target.
    fn ink(&self) -> Ink {
        let erase = self.tool == Tool::Eraser;
        match self.target {
            Target::Colour => Ink::Colour(if erase { Rgba::default() } else { self.colour }),
            Target::Tint => Ink::Tint(!erase),
            Target::Glow => Ink::Glow(if erase { 0 } else { self.glow_strength }),
        }
    }

    fn press(&mut self, x: i32, y: i32) {
        let (x, y) = (x.clamp(-FAR, FAR), y.clamp(-FAR, FAR));
        let f = self.frame;
        let before = self.doc.frame(f);
        match self.tool {
            Tool::Eyedropper => {
                if let Some(i) = before.at(x, y) {
                    match self.target {
                        Target::Glow => {
                            if before.glow[i] > 0 {
                                self.glow_strength = nearest_glow(before.glow[i]);
                            }
                        }
                        _ if before.px[i].a != 0 => self.colour = Rgba { a: 255, ..before.px[i] },
                        _ => {}
                    }
                    self.tool = Tool::Pencil;
                }
            }
            Tool::Fill => {
                let mut after = before.clone();
                paint::fill(&mut after, x, y, self.ink());
                self.used_colour();
                self.apply(Edit::Frames(vec![(f, before, after)]));
            }
            _ => {
                self.stroke = Some(Stroke { frame: f, before, start: (x, y), last: (x, y) });
                self.drag(x, y);
            }
        }
    }

    fn drag(&mut self, x: i32, y: i32) {
        let (x, y) = (x.clamp(-FAR, FAR), y.clamp(-FAR, FAR));
        let ink = self.ink();
        let (size, tool) = (self.size, self.tool);
        let Some(s) = &mut self.stroke else { return };
        let mut frame = match tool {
            // A line or rectangle is drawn again from the frame before.
            Tool::Line | Tool::Rect => s.before.clone(),
            _ => self.doc.frame(s.frame),
        };
        match tool {
            Tool::Line => paint::line(&mut frame, s.start, (x, y), size, ink),
            Tool::Rect => paint::rect(&mut frame, s.start, (x, y), size, ink),
            _ => paint::line(&mut frame, s.last, (x, y), size, ink),
        }
        s.last = (x, y);
        let f = s.frame;
        if frame != self.doc.frame(f) {
            self.doc.set_frame(f, &frame);
        }
    }

    /// Records the stroke as one step (none when it changed nothing).
    fn end_stroke(&mut self) {
        let Some(s) = self.stroke.take() else { return };
        let after = self.doc.frame(s.frame);
        let edit = Edit::Frames(vec![(s.frame, s.before, after)]);
        if !edit.is_noop() {
            self.history.push(edit);
            self.used_colour();
        }
    }

    /// The paint colour joins the recent colours when it was painted.
    fn used_colour(&mut self) {
        if self.target != Target::Colour || self.tool == Tool::Eraser {
            return;
        }
        let c = self.colour;
        self.recent.retain(|&r| r != c);
        self.recent.insert(0, c);
        self.recent.truncate(MAX_RECENT);
    }

    fn paste(&mut self, to: PasteTo) {
        let Some(clip) = self.clipboard.clone() else {
            self.status = "Copy a frame first".into();
            return;
        };
        if (clip.w, clip.h) != self.doc.frame_size() {
            self.status = "The copied frame is of another size".into();
            return;
        }
        let (aim, walk) = (self.frame % AIM_FRAMES, self.frame / AIM_FRAMES);
        let frames: Vec<usize> = match to {
            PasteTo::Frame => vec![self.frame],
            PasteTo::Row => (0..AIM_FRAMES).map(|a| walk * AIM_FRAMES + a).collect(),
            PasteTo::Column => (0..WALK_FRAMES).map(|w| w * AIM_FRAMES + aim).collect(),
        };
        let edit = frames.into_iter().map(|f| (f, self.doc.frame(f), clip.clone())).collect();
        self.apply(Edit::Frames(edit));
    }

    fn make_double(&mut self) {
        if !self.doc.is_classic() {
            return;
        }
        let before = self.doc.source().clone();
        match olr_edit::skin::double(&before) {
            Ok(after) => {
                self.apply(Edit::Whole { before: Box::new(before), after: Box::new(after) });
                self.zoom = 8.0;
                self.status = "Made double resolution: refine the frames".into();
            }
            Err(e) => self.dialog = Some(Dialog::Error(e.to_string())),
        }
    }

    fn save(&mut self) -> Vec<SkinCommand> {
        if self.saving {
            return Vec::new();
        }
        if self.pack_id.is_none() {
            self.dialog = Some(Dialog::Name);
            return Vec::new();
        }
        self.saving = true;
        self.pending_at = self.history.at();
        vec![SkinCommand::Save {
            id: self.pack_id.clone(),
            name: self.doc.name().to_string(),
            source: Box::new(self.doc.to_source()),
        }]
    }

    fn save_as(&mut self, name: String) -> Vec<SkinCommand> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Vec::new();
        }
        self.dialog = None;
        // The name asked is the skin's: not a step.
        self.doc.set_name(name.clone());
        self.saving = true;
        self.pending_at = self.history.at();
        vec![SkinCommand::Save { id: None, name, source: Box::new(self.doc.to_source()) }]
    }

    fn saved(&mut self, id: String) -> Vec<SkinCommand> {
        self.saving = false;
        self.pack_id = Some(id);
        // An edit made meanwhile stays unsaved.
        self.saved_at = self.pending_at;
        self.status = "Saved".into();
        if std::mem::take(&mut self.close_after_save) && !self.dirty() {
            return vec![SkinCommand::Close];
        }
        Vec::new()
    }
}

/// A frame's name as the grid labels it: "Aim 3, walk 2".
pub fn frame_name(f: usize) -> String {
    format!("Aim {}, walk {}", f % AIM_FRAMES + 1, f / AIM_FRAMES + 1)
}

/// The offered glow strength nearest `g`.
fn nearest_glow(g: u8) -> u8 {
    GLOW_STRENGTHS.into_iter().min_by_key(|&s| (i16::from(s) - i16::from(g)).abs()).unwrap_or(255)
}
