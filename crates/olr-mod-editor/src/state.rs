//! The mod editor's state (issue #17): `ModEditorState::update(ModAction)
//! -> Vec<ModCommand>` is pure; the host carries out the commands (`Save`,
//! `LoadMod`, `Play`, `SavePlaySettings`, `Close`; issue #19's `List`,
//! `ReadFile`, `PlaySound`) and answers (`Saved`, `ModLoaded`,
//! `PlayEnded`, `Failed`; `Listed`, `FileRead`). The Art and Sounds
//! sections' work is in `assets.rs`.
//!
//! Every change goes through `olr_edit::mods` on the open pack and is one
//! undo step. A field's value is set with `set_lenient`: a change that
//! adds an error is kept and the error listed (the check runs after every
//! change), so a mod can pass through an invalid state while it is being
//! edited; Save refuses a mod with errors.

use std::collections::BTreeMap;
use std::path::PathBuf;

use olr_data::image::Rgba;
use olr_data::restrictions::{Preset, ShippedLoadout, WeaponState};
use olr_edit::mods::path::File;
use olr_edit::mods::{self, Issue, ModError, WeaponFrom};
use olr_edit::pack::{Pack, Source};
use olr_format::Colour;
use olr_format::mods::{
    Effect, EventContext, EventSpawn, Gfx, LIFTED_SPAWN, MOD_EDITOR_FILE, ModEditorFile, ModSource,
    Projectile, ShotContext, Spawn, SpawnContext, Timer, TrailContext,
};
use olr_format::ron_io::{from_canonical, to_canonical};
use olr_pack::Severity;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::art::{ArtDoc, Target, Tool};
use crate::browser::{Browser, FileEntry, Import};
use crate::history::{History, Snapshot};
use crate::tree::{self, Item, Node, SpawnAt};

/// What the editor says the first time a weapon is moved.
pub const ORDER_NOTE: &str = "A weapon's place in the list is its id in the game: moving one renumbers the \
     weapons after it. Replays and bot loadouts of this mod follow the new order.";
/// A fresh projectile's look: one white pixel.
pub const PIXEL: &str = "Pixels([\"#ffffff\"])";
/// A new weapon's name before it is renamed.
pub const NEW_WEAPON: &str = "New weapon";
/// The most bots a Play match has (the level editor's `MAX_BOTS`).
pub const MAX_BOTS: u8 = 8;

/// Play's level and bots (issue #18), remembered between sessions (the
/// host stores them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaySettings {
    /// The installed level Play starts the match on (its pack id).
    pub level: String,
    /// Bots in a Play match, 0 (the sandbox) to [`MAX_BOTS`].
    pub bots: u8,
}

impl Default for PlaySettings {
    /// The level editor's default: one bot; the level is the host's choice.
    fn default() -> PlaySettings {
        PlaySettings { level: String::new(), bots: 1 }
    }
}

/// One of a projectile's events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    HitTerrain,
    HitWorm,
    Timer,
}

impl Event {
    pub const ALL: [Event; 3] = [Event::HitTerrain, Event::HitWorm, Event::Timer];

    pub fn name(self) -> &'static str {
        match self {
            Event::HitTerrain => "Hit terrain",
            Event::HitWorm => "Hit worm",
            Event::Timer => "Timer",
        }
    }

    /// The path of the event's list under projectile `p`.
    pub fn list(self, p: &str) -> String {
        match self {
            Event::HitTerrain => format!("{p}.hit_terrain"),
            Event::HitWorm => format!("{p}.hit_worm"),
            Event::Timer => format!("{p}.timer.effects"),
        }
    }

    fn of(p: &mut Projectile, e: Event) -> Option<&mut Vec<Effect>> {
        match e {
            Event::HitTerrain => Some(&mut p.hit_terrain),
            Event::HitWorm => Some(&mut p.hit_worm),
            Event::Timer => p.timer.as_mut().map(|t| &mut t.effects),
        }
    }
}

/// An event as the editor shows it: one effect and whether it spawns.
#[derive(Clone, Debug, PartialEq)]
pub struct EventView {
    /// The effect and its index in the list, if any.
    pub effect: Option<(usize, Effect)>,
    /// Effects besides the first and the spawn (LX56 runs one: an error).
    pub extra: usize,
    /// The spawn's key, when the event spawns.
    pub spawn: Option<String>,
}

/// A question or a message over the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// Close with unsaved changes: save, don't save or cancel.
    Unsaved,
    /// A weapon's fire becomes another kind, dropping its settings: asked.
    FireKind { weapon: String, kind: String },
    /// Add a weapon: blank, or a copy of one of `from` (a mod's pack id).
    AddWeapon { from: Option<String>, weapon: Option<String> },
    /// Something to read once (the weapons' order).
    Note(String),
    /// What the host could not do.
    Error(String),
    /// A new image (issue #19): blank of a size, or a copy of `copy_of`.
    NewImage { copy_of: Option<String> },
    /// An image or sound's new name.
    Rename(String),
    /// The import browser (`ModEditorState::browser`).
    Browse,
    /// An image's frame count changes (Add or Remove frame, a PNG of
    /// another width): asked, since the frame count reaches play.
    FrameCount { image: String, before: u32, after: u32 },
}

/// The Art section's painting (issue #19), on the image shown.
#[derive(Clone, Debug, PartialEq)]
pub enum ArtAction {
    Tool(Tool),
    /// The brush, 1 or 2 pixels.
    Size(u32),
    /// What is painted: colour or glow.
    Target(Target),
    Colour(Rgba),
    GlowStrength(u8),
    /// Zoom in (positive) or out by steps of `art::ZOOMS`.
    Zoom(i32),
    Pan(f32, f32),
    /// The previous frame shown faintly under the one painted.
    Onion(bool),
    /// The frame painted.
    Frame(usize),
    /// A press, a drag and a release on the canvas, in pixels from the
    /// frame's top left: a stroke is one step.
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
    /// The copied frame over the chosen one (one step).
    PasteFrame,
    /// A copy of the chosen frame after it (asked: the frame count changes).
    AddFrame,
    /// The chosen frame removed (asked).
    RemoveFrame,
}

/// The import browser's actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowseAction {
    /// A typed folder (relative to the one shown).
    To(PathBuf),
    Up,
    /// An entry clicked.
    Select(String),
    /// An entry double clicked: a folder entered, a file imported.
    Pick(String),
    /// Choose: the entry selected picked.
    Choose,
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
pub enum ModAction {
    /// Show an item of the left list.
    Select(Item),
    /// Show a node of the item's tree (its path).
    Node(String),
    /// Go to a problem's field (its path, or its file's).
    GoTo(String),
    /// Set the value at a path (RON, or a variant's name).
    Set {
        path: String,
        value: String,
    },
    /// Set the value at a path back to the one in the mod this was copied from.
    Reset(String),
    /// Make a weapon's fire `kind` (`Projectile`, `Beam`, `Special`): asked first.
    FireKind {
        weapon: String,
        kind: String,
    },
    /// An event's effect (a variant's name, its fields at their defaults), or none.
    Effect {
        projectile: String,
        event: Event,
        effect: Option<String>,
    },
    /// Whether an event spawns (the projectile's spawn, made when it has none).
    EventSpawn {
        projectile: String,
        event: Event,
        on: bool,
    },
    /// A projectile's timer on (1 s, no effect) or off.
    Timer {
        projectile: String,
        on: bool,
    },
    /// An image's animation on (10 frames a second, looping) or off.
    Animation {
        gfx: String,
        on: bool,
    },
    /// A spawn's projectile: a shared one by id.
    SpawnShared {
        spawn: String,
        shared: String,
    },
    /// A spawn's shared projectile made its own copy.
    MakeCopy(String),
    /// A spawn's projectile made a shared one.
    Share(String),
    /// Open the Add weapon dialog.
    AddWeapon,
    /// Add weapon's source: blank (`None`) or a mod's pack id.
    AddFrom(Option<String>),
    /// Add weapon's weapon to copy (an id of the chosen mod).
    AddPick(String),
    /// Add the weapon chosen.
    AddConfirm,
    RemoveWeapon(String),
    /// Move a weapon up (negative) or down by places.
    MoveWeapon {
        id: String,
        by: i32,
    },
    DuplicateWeapon(String),
    /// The restrictions shown (a preset's name).
    Preset(String),
    /// A weapon's (by name) state in a preset.
    PresetState {
        preset: String,
        weapon: String,
        state: WeaponState,
    },
    AddPreset(String),
    RemovePreset(String),
    /// A preset's shipped loadout, or none.
    Loadout {
        preset: String,
        loadout: Option<ShippedLoadout>,
    },
    /// The problems' list shown or hidden.
    ShowProblems(bool),
    Undo,
    Redo,
    Save,
    /// A test match with the mod as it is, unsaved changes included.
    Play,
    /// Play's level (an installed level's pack id).
    PlayLevel(String),
    /// Play's bots (0 to `MAX_BOTS`).
    PlayBots(u8),
    Close,
    Choice(Choice),
    /// The answer to a question (Change, Add, OK): yes, or no.
    Confirm(bool),
    /// The host loaded mod `id` (Add weapon's copies).
    ModLoaded(String, Box<Pack>),
    /// The host saved the pack.
    Saved,
    /// The host's Play ended (Escape, Stop, the match over, or it did not start).
    PlayEnded,
    /// The host could not do what was asked: an error dialog.
    Failed(String),
    /// A message for the status bar.
    Note(String),
    /// The Art section's painting (issue #19).
    Art(ArtAction),
    /// Ask for a new image: blank, or a copy of image `copy_of`.
    AskNewImage(Option<String>),
    /// A new image `name` (`gfx/` and `.png` added when missing): a copy
    /// of `copy_of` (with its glow), else blank, `frames` square frames
    /// of `side` pixels.
    NewImage {
        name: String,
        side: u32,
        frames: u32,
        copy_of: Option<String>,
    },
    /// Ask for an image or sound's new name.
    AskRename(String),
    /// An image or sound renamed (its folder and an image's `.png` added
    /// when missing), with every field naming it.
    RenameAsset {
        from: String,
        to: String,
    },
    /// An image (with its glow mask) or sound removed: refused while used.
    RemoveAsset(String),
    /// Play sound `path` of the mod (the host plays it).
    PlaySound(String),
    /// Open the import browser.
    Import(Import),
    Browse(BrowseAction),
    /// The host listed folder `dir` for the browser.
    Listed {
        dir: PathBuf,
        entries: Result<Vec<FileEntry>, String>,
    },
    /// The host read the file picked for `import`: its file name and bytes.
    FileRead {
        import: Import,
        name: String,
        bytes: Vec<u8>,
    },
}

/// What the host carries out.
#[derive(Clone, Debug, PartialEq)]
pub enum ModCommand {
    /// Write the pack where it was opened from (checked: no errors).
    Save(Box<Pack>),
    /// Load installed mod `id` (answer `ModLoaded` or `Failed`).
    LoadMod(String),
    /// Play a test match with mod `pack` (checked: no errors) on installed
    /// level `level` against `bots` bots; answer `PlayEnded` when it ends.
    Play { pack: Box<Pack>, level: String, bots: u8 },
    /// Remember Play's level and bots.
    SavePlaySettings(PlaySettings),
    /// Close the editor.
    Close,
    /// List folder `dir` for the import browser (answer `Listed`).
    List(PathBuf),
    /// Read file `path` for `import` (answer `FileRead`, or `Failed`).
    ReadFile { import: Import, path: PathBuf },
    /// Play sound file `bytes` (the mod's `path`) once.
    PlaySound { path: String, bytes: Vec<u8> },
}

/// The mod editor.
#[derive(Debug)]
pub struct ModEditorState {
    pack: Pack,
    /// The mod it was copied from, for the Original values.
    base: Option<ModSource>,
    base_id: Option<String>,
    /// The installed mods (pack id, name), for Add weapon.
    mods: Vec<(String, String)>,
    /// Mods loaded for Add weapon.
    others: BTreeMap<String, Pack>,
    /// The installed levels (pack id, name), for Play.
    levels: Vec<(String, String)>,
    play: PlaySettings,
    /// Play's match runs (the host shows it instead of the editor).
    playing: bool,
    history: History,
    saved_at: u64,
    saving: bool,
    pending_at: u64,
    close_after_save: bool,
    item: Item,
    node: String,
    /// The field a problem's link went to.
    focus: Option<String>,
    /// Bumped by each `GoTo` (the panel scrolls to the field once).
    focus_gen: u64,
    issues: Vec<Issue>,
    show_problems: bool,
    /// The preset the World shows.
    preset: Option<String>,
    order_noted: bool,
    generation: u64,
    dialog: Option<Dialog>,
    status: String,
    /// Issue #19: the image the Art section shows, as painted, or why it
    /// does not paint.
    pub(crate) art: Option<Result<ArtDoc, String>>,
    /// The paint tools (kept from image to image).
    pub(crate) tools: crate::assets::ArtTools,
    pub(crate) stroke: Option<crate::assets::ArtStroke>,
    /// Bumped by every change of the image shown (its textures follow).
    pub(crate) art_gen: u64,
    pub(crate) browser: Option<Browser>,
    /// Where the browser opens (the host's home, then the last folder picked from).
    pub(crate) browse_dir: PathBuf,
    /// An image change waiting for `Dialog::FrameCount`'s answer.
    pub(crate) pending_image: Option<crate::assets::PendingImage>,
}

/// The value at `path` as `T`, if it reads.
pub fn get_as<T: DeserializeOwned>(src: &ModSource, path: &str) -> Option<T> {
    let text = mods::get(src, path).ok()?;
    from_canonical(path, &text).ok()
}

/// `v` as RON (one value, no trailing newline).
pub fn ron<T: Serialize>(v: &T) -> String {
    to_canonical("value", v).map(|s| s.trim_end().to_string()).unwrap_or_default()
}

/// Where the spawn at `path` is (its defaults depend on it).
pub fn spawn_at(path: &str) -> SpawnAt {
    if path.ends_with(".fire.shot") {
        SpawnAt::Shot
    } else if path.ends_with(".trail.spawn") {
        SpawnAt::Trail
    } else {
        SpawnAt::Event
    }
}

/// Edits the value at `path` as `T` and sets it back whole (one change).
fn edit<T: Serialize + DeserializeOwned>(
    pack: &mut Pack,
    path: &str,
    f: impl FnOnce(&mut T) -> Result<(), String>,
) -> Result<Vec<Issue>, ModError> {
    let src = mods::source(pack)?;
    let mut v: T = get_as(src, path).ok_or_else(|| ModError::Path(format!("{path}: nothing to edit")))?;
    f(&mut v).map_err(ModError::Refused)?;
    mods::set_lenient(pack, path, &ron(&v))
}

/// Edits the spawn at `path` (of its context) through its common fields.
fn edit_spawn(
    pack: &mut Pack,
    path: &str,
    f: impl FnOnce(&mut Option<String>, &mut Option<Box<Projectile>>),
) -> Result<Vec<Issue>, ModError> {
    fn go<C: SpawnContext>(
        pack: &mut Pack,
        path: &str,
        f: impl FnOnce(&mut Option<String>, &mut Option<Box<Projectile>>),
    ) -> Result<Vec<Issue>, ModError> {
        edit::<Spawn<C>>(pack, path, |s| {
            f(&mut s.shared, &mut s.projectile);
            Ok(())
        })
    }
    match spawn_at(path) {
        SpawnAt::Shot => go::<ShotContext>(pack, path, f),
        SpawnAt::Event => go::<EventContext>(pack, path, f),
        SpawnAt::Trail => go::<TrailContext>(pack, path, f),
    }
}

/// Effect `name` with its fields at their defaults.
pub fn effect_named(name: &str) -> Option<Effect> {
    from_canonical::<Effect>("effect", name)
        .or_else(|_| from_canonical::<Effect>("effect", &format!("{name}()")))
        .ok()
        .filter(|e| !matches!(e, Effect::Spawn(_) | Effect::Unknown(_)))
}

/// The effects an event offers, by name (never `Spawn` or `Unknown`).
pub fn effect_names() -> Vec<&'static str> {
    crate::fields::rows()
        .iter()
        .filter(|r| r.ty == "variant")
        .filter_map(|r| r.key.strip_prefix("Effect::"))
        .filter(|n| effect_named(n).is_some())
        .collect()
}

/// A weapon id for `name` that `taken` does not hold.
fn new_weapon_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base = String::new();
    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            base.push(c);
        } else if !base.ends_with('-') {
            base.push('-');
        }
    }
    let base = match base.trim_matches('-') {
        "" => "weapon".to_string(),
        b => b.chars().take(48).collect(),
    };
    std::iter::once(base.clone())
        .chain((2u32..).map(|i| format!("{base}-{i}")))
        .find(|id| !taken(id))
        .unwrap_or(base)
}

/// Weapon names as presets compare them (OLX: trimmed, ASCII case ignored).
pub fn same_name(a: &str, b: &str) -> bool {
    let t = |s: &str| s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')).to_string();
    t(a).eq_ignore_ascii_case(&t(b))
}

impl ModEditorState {
    /// The editor on mod `pack` (an error if it is no mod pack). `base` is
    /// the source of the mod named by its `editor/mod.ron` (see
    /// [`ModEditorState::base_of`]), when it is installed; `mods` the
    /// installed mods for Add weapon. `recovered`: the pack is unsaved work.
    pub fn new(
        pack: Pack,
        base: Option<ModSource>,
        mods: Vec<(String, String)>,
        recovered: bool,
    ) -> Result<ModEditorState, String> {
        if !matches!(pack.source, Source::Mod(_)) {
            return Err(format!("{} is not a mod pack", pack.manifest.id));
        }
        let base_id = Self::base_of(&pack);
        let mut s = ModEditorState {
            pack,
            base,
            base_id,
            mods,
            others: BTreeMap::new(),
            levels: Vec::new(),
            play: PlaySettings::default(),
            playing: false,
            history: History::default(),
            saved_at: if recovered { u64::MAX } else { 0 },
            saving: false,
            pending_at: 0,
            close_after_save: false,
            item: Item::World,
            node: Item::World.path(),
            focus: None,
            focus_gen: 0,
            issues: Vec::new(),
            show_problems: false,
            preset: None,
            order_noted: false,
            generation: 0,
            dialog: None,
            status: String::new(),
            art: None,
            tools: crate::assets::ArtTools::default(),
            stroke: None,
            art_gen: 0,
            browser: None,
            browse_dir: PathBuf::from("."),
            pending_image: None,
        };
        s.refresh();
        Ok(s)
    }

    /// The base mod's pack id `pack`'s `editor/mod.ron` names, if any.
    pub fn base_of(pack: &Pack) -> Option<String> {
        let path = olr_vfs::VPath::new(MOD_EDITOR_FILE).ok()?;
        let text = String::from_utf8_lossy(pack.other.get(&path)?);
        from_canonical::<ModEditorFile>(MOD_EDITOR_FILE, &text).ok()?.base
    }

    /// `pack` with its `editor/mod.ron` naming `base`.
    pub fn with_base(pack: &mut Pack, base: Option<&str>) {
        let Ok(path) = olr_vfs::VPath::new(MOD_EDITOR_FILE) else { return };
        let file = ModEditorFile { base: base.map(String::from) };
        if let Ok(text) = to_canonical(MOD_EDITOR_FILE, &file) {
            pack.other.insert(path, text.into_bytes());
        }
    }

    /// Play's levels (the installed ones: pack id, name) and its settings
    /// remembered (a level not among them is not chosen).
    pub fn set_play(&mut self, levels: Vec<(String, String)>, play: PlaySettings) {
        self.levels = levels;
        self.play = PlaySettings { bots: play.bots.min(MAX_BOTS), ..play };
    }

    pub fn pack(&self) -> &Pack {
        &self.pack
    }
    pub fn src(&self) -> &ModSource {
        match &self.pack.source {
            Source::Mod(m) => m,
            // `new` takes only a mod pack, and every change keeps one.
            _ => unreachable!("the mod editor holds a mod pack"),
        }
    }
    pub fn base(&self) -> Option<&ModSource> {
        self.base.as_ref()
    }
    pub fn base_id(&self) -> Option<&str> {
        self.base_id.as_deref()
    }
    pub fn mods(&self) -> &[(String, String)] {
        &self.mods
    }
    /// Play's levels (pack id, name).
    pub fn levels(&self) -> &[(String, String)] {
        &self.levels
    }
    /// The level Play starts on: the one chosen when installed, else the first.
    pub fn play_level(&self) -> Option<&(String, String)> {
        self.levels.iter().find(|(id, _)| *id == self.play.level).or(self.levels.first())
    }
    pub fn play_bots(&self) -> u8 {
        self.play.bots
    }
    /// Whether Play's match runs.
    pub fn playing(&self) -> bool {
        self.playing
    }
    /// A mod loaded for Add weapon (or this one, by its own id).
    pub fn other(&self, id: &str) -> Option<&ModSource> {
        if id == self.pack.manifest.id {
            return Some(self.src());
        }
        match &self.others.get(id)?.source {
            Source::Mod(m) => Some(m),
            _ => None,
        }
    }
    /// Unsaved changes.
    pub fn dirty(&self) -> bool {
        self.history.at() != self.saved_at
    }
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }
    pub fn item(&self) -> &Item {
        &self.item
    }
    /// The node shown (its path).
    pub fn node(&self) -> &str {
        &self.node
    }
    /// The item's tree.
    pub fn nodes(&self) -> Vec<Node> {
        tree::nodes(self.src(), &self.item)
    }
    pub fn focus(&self) -> Option<&str> {
        self.focus.as_deref()
    }
    pub fn focus_gen(&self) -> u64 {
        self.focus_gen
    }
    /// Every error, then every warning, of the mod as it is.
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }
    pub fn errors(&self) -> usize {
        self.issues.iter().filter(|i| i.is_error()).count()
    }
    pub fn show_problems(&self) -> bool {
        self.show_problems
    }
    /// The preset the World shows: the chosen one, else the first.
    pub fn preset(&self) -> Option<&str> {
        let r = &self.src().restrictions;
        match &self.preset {
            Some(p) if r.contains_key(p) => Some(p),
            _ => r.keys().next().map(String::as_str),
        }
    }
    /// Changes since the editor opened (bumped by every change, undo and redo).
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }
    pub fn status(&self) -> &str {
        &self.status
    }
    pub(crate) fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }
    pub(crate) fn set_dialog(&mut self, d: Option<Dialog>) {
        self.dialog = d;
    }

    /// The value at `path` as canonical RON.
    pub fn value(&self, path: &str) -> Option<String> {
        mods::get(self.src(), path).ok()
    }
    /// The value at `path` in the base mod, when it differs from this one's.
    pub fn original(&self, path: &str) -> Option<String> {
        let base = mods::get(self.base.as_ref()?, path).ok()?;
        (Some(&base) != self.value(path).as_ref()).then_some(base)
    }
    /// The variant of the enum at `path`.
    pub fn variant(&self, path: &str) -> Option<String> {
        mods::variant(self.src(), path).ok().flatten()
    }
    /// The variants the enum at `path` offers.
    pub fn choices(&self, path: &str) -> Vec<&'static str> {
        mods::choices(self.src(), path).unwrap_or_default()
    }
    /// The pack's files in asset folder `folder` (`gfx`, `sfx`), sorted;
    /// `gfx`'s images only (no glow masks).
    pub fn assets(&self, folder: &str) -> Vec<String> {
        if folder == "gfx" {
            return mods::art::images(&self.pack);
        }
        let prefix = format!("{folder}/");
        self.pack.other.keys().map(|p| p.to_string()).filter(|p| p.starts_with(&prefix)).collect()
    }
    /// An asset file's bytes.
    pub fn asset(&self, path: &str) -> Option<&[u8]> {
        self.pack.other.get(&olr_vfs::VPath::new(path).ok()?).map(Vec::as_slice)
    }
    /// The event `e` of the projectile at `p`.
    pub fn event(&self, p: &str, e: Event) -> Option<EventView> {
        let list: Vec<Effect> = get_as(self.src(), &e.list(p))?;
        let mut effects = list.iter().enumerate().filter(|(_, e)| !matches!(e, Effect::Spawn(_)));
        let effect = effects.next().map(|(i, e)| (i, e.clone()));
        let spawn = list.iter().find_map(|e| match e {
            Effect::Spawn(k) => Some(k.clone()),
            _ => None,
        });
        Some(EventView { effect, extra: effects.count(), spawn })
    }
    /// Each shared projectile's users (the spawns naming it).
    pub fn used_by(&self) -> BTreeMap<String, Vec<String>> {
        mods::used_by(self.src())
    }

    /// Carries out `a`; returns what the host must do.
    pub fn update(&mut self, a: ModAction) -> Vec<ModCommand> {
        // The host's answers act whatever is open.
        match a {
            ModAction::Saved => return self.saved(),
            ModAction::Failed(why) => {
                self.saving = false;
                self.close_after_save = false;
                self.dialog = Some(Dialog::Error(why));
                return Vec::new();
            }
            ModAction::ModLoaded(id, pack) => {
                if matches!(pack.source, Source::Mod(_)) {
                    self.others.insert(id, *pack);
                }
                return Vec::new();
            }
            ModAction::Note(text) => {
                self.status = text;
                return Vec::new();
            }
            ModAction::PlayEnded => {
                self.playing = false;
                return Vec::new();
            }
            ModAction::Listed { dir, entries } => {
                if let Some(b) = &mut self.browser {
                    b.listed(dir, entries);
                }
                return Vec::new();
            }
            ModAction::FileRead { import, name, bytes } if !self.playing => {
                self.file_read(import, &name, &bytes);
                return Vec::new();
            }
            // Nothing else acts while Play's match runs.
            _ if self.playing => return Vec::new(),
            _ => {}
        }
        if let Some(d) = self.dialog.clone() {
            return self.answer(d, a);
        }
        // A stroke ends before anything else changes the mod.
        if self.stroke.is_some() && !matches!(a, ModAction::Art(ArtAction::Drag { .. } | ArtAction::Release))
        {
            self.end_stroke();
        }
        match a {
            ModAction::Select(item) => self.select(item),
            ModAction::Art(a) => self.art_action(a),
            ModAction::AskNewImage(copy_of) => self.dialog = Some(Dialog::NewImage { copy_of }),
            ModAction::AskRename(path) => self.dialog = Some(Dialog::Rename(path)),
            ModAction::RenameAsset { from, to } => self.rename_asset(&from, &to),
            ModAction::RemoveAsset(path) => self.remove_asset(&path),
            ModAction::PlaySound(path) => return self.play_sound(path),
            ModAction::Import(import) => return self.open_browser(import),
            ModAction::Node(path) => {
                if self.nodes().iter().any(|n| n.path == path) {
                    self.node = path;
                }
            }
            ModAction::GoTo(path) => self.go_to(&path),
            ModAction::Set { path, value } => {
                self.change(false, |p| mods::set_lenient(p, &path, &value));
            }
            ModAction::Reset(path) => {
                if let Some(v) = self.base.as_ref().and_then(|b| mods::get(b, &path).ok()) {
                    self.change(false, |p| mods::set_lenient(p, &path, &v));
                }
            }
            ModAction::FireKind { weapon, kind } => {
                if self.variant(&format!("weapons/{weapon}.fire")).as_deref() != Some(kind.as_str()) {
                    self.dialog = Some(Dialog::FireKind { weapon, kind });
                }
            }
            ModAction::Effect { projectile, event, effect } => self.set_effect(&projectile, event, effect),
            ModAction::EventSpawn { projectile, event, on } => self.event_spawn(&projectile, event, on),
            ModAction::Timer { projectile, on } => {
                self.change(false, |p| {
                    edit::<Projectile>(p, &projectile, |pr| {
                        if on {
                            pr.timer.get_or_insert(Timer {
                                after_s: 1.0,
                                variation_s: 0.0,
                                effects: Vec::new(),
                            });
                        } else if let Some(t) = pr.timer.take() {
                            let keys: Vec<String> = spawn_keys(&t.effects);
                            for k in keys {
                                drop_unused_spawn(pr, &k);
                            }
                        }
                        Ok(())
                    })
                });
            }
            ModAction::Animation { gfx, on } => {
                let value = if on { "(fps: 10.0, mode: Loop)" } else { "None" };
                self.change(false, |p| mods::set_lenient(p, &format!("{gfx}.animation"), value));
            }
            ModAction::SpawnShared { spawn, shared } => {
                self.change(false, |p| {
                    edit_spawn(p, &spawn, |s, pr| {
                        *s = Some(shared);
                        *pr = None;
                    })
                });
            }
            ModAction::MakeCopy(spawn) => {
                if self.change(false, |p| mods::inline(p, &spawn).map(|()| Vec::new())) {
                    self.status = "The projectile is this spawn's own copy now".into();
                }
            }
            ModAction::Share(spawn) => {
                let mut id = String::new();
                if self.change(false, |p| {
                    id = mods::share(p, &spawn, None)?;
                    Ok(Vec::new())
                }) {
                    self.status = format!("Shared as projectiles/{id}");
                }
            }
            ModAction::AddWeapon => self.dialog = Some(Dialog::AddWeapon { from: None, weapon: None }),
            ModAction::RemoveWeapon(id) => {
                let at = self.src().weapons.iter().position(|(w, _)| *w == id);
                if self.change(false, |p| mods::remove_weapon(p, &id).map(|()| Vec::new())) {
                    let ids = &self.src().weapons;
                    let next = at.and_then(|i| ids.get(i).or(ids.last())).map(|(w, _)| w.clone());
                    self.select(next.map_or(Item::World, Item::Weapon));
                }
            }
            ModAction::MoveWeapon { id, by } => self.move_weapon(&id, by),
            ModAction::DuplicateWeapon(id) => {
                let new_id = self.weapon_id(&id);
                if self.change(false, |p| mods::copy_weapon(p, &id, &new_id, None).map(|()| Vec::new())) {
                    self.select(Item::Weapon(new_id));
                }
            }
            ModAction::Preset(name) => self.preset = Some(name),
            ModAction::PresetState { preset, weapon, state } => {
                self.change(false, |p| {
                    edit::<BTreeMap<String, Preset>>(p, "restrictions", |r| {
                        let entries = r.get_mut(&preset).ok_or_else(|| format!("no preset {preset:?}"))?;
                        let key = entries.keys().find(|k| same_name(k, &weapon)).cloned().unwrap_or(weapon);
                        entries.insert(key, state);
                        Ok(())
                    })
                });
            }
            ModAction::AddPreset(name) => {
                let name = name.trim().to_string();
                if name.is_empty() || self.src().restrictions.contains_key(&name) {
                    self.status = format!("{name:?} is not a new preset's name");
                } else if self
                    .change(false, |p| mods::set_lenient(p, &format!("restrictions[{}]", quote(&name)), "{}"))
                {
                    self.preset = Some(name);
                }
            }
            ModAction::RemovePreset(name) => {
                self.change(false, |p| {
                    let mut new = edit::<BTreeMap<String, Preset>>(p, "restrictions", |r| {
                        r.remove(&name);
                        Ok(())
                    })?;
                    if mods::source(p)?.loadouts.contains_key(&name) {
                        new.extend(edit::<BTreeMap<String, ShippedLoadout>>(p, "mod.loadouts", |l| {
                            l.remove(&name);
                            Ok(())
                        })?);
                    }
                    Ok(new)
                });
            }
            ModAction::Loadout { preset, loadout } => {
                self.change(false, |p| {
                    edit::<BTreeMap<String, ShippedLoadout>>(p, "mod.loadouts", |l| {
                        match loadout {
                            Some(v) => l.insert(preset, v),
                            None => l.remove(&preset),
                        };
                        Ok(())
                    })
                });
            }
            ModAction::ShowProblems(on) => self.show_problems = on,
            ModAction::Undo => {
                if self.history.undo(&mut self.pack) {
                    self.status = "Undone".into();
                    self.changed();
                }
            }
            ModAction::Redo => {
                if self.history.redo(&mut self.pack) {
                    self.status = "Redone".into();
                    self.changed();
                }
            }
            ModAction::Save => return self.save(),
            ModAction::Play => return self.play(),
            ModAction::PlayLevel(id) => {
                if self.levels.iter().any(|(l, _)| *l == id) && id != self.play.level {
                    self.play.level = id;
                    return vec![ModCommand::SavePlaySettings(self.play.clone())];
                }
            }
            ModAction::PlayBots(n) => {
                let n = n.min(MAX_BOTS);
                if n != self.play.bots {
                    self.play.bots = n;
                    return vec![ModCommand::SavePlaySettings(self.play.clone())];
                }
            }
            ModAction::Close => {
                if self.dirty() {
                    self.dialog = Some(Dialog::Unsaved);
                } else {
                    return vec![ModCommand::Close];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// An action while dialog `d` is open: only its answers act.
    fn answer(&mut self, d: Dialog, a: ModAction) -> Vec<ModCommand> {
        match (d, a) {
            (Dialog::Unsaved, ModAction::Choice(Choice::Save)) => {
                self.dialog = None;
                self.close_after_save = true;
                let out = self.save();
                if out.is_empty() {
                    self.close_after_save = false;
                }
                out
            }
            (Dialog::Unsaved, ModAction::Choice(Choice::DontSave)) => {
                self.dialog = None;
                vec![ModCommand::Close]
            }
            (Dialog::FireKind { weapon, kind }, ModAction::Confirm(true)) => {
                self.dialog = None;
                let path = format!("weapons/{weapon}.fire");
                // A variant set by name starts valid (`olr_edit::mods::set`).
                if self.change(false, |p| mods::set_lenient(p, &path, &kind)) {
                    self.node = format!("weapons/{weapon}");
                }
                Vec::new()
            }
            (Dialog::AddWeapon { .. }, ModAction::AddFrom(from)) => {
                let load = from.as_ref().filter(|id| self.other(id).is_none()).cloned();
                self.dialog = Some(Dialog::AddWeapon { from, weapon: None });
                load.map(ModCommand::LoadMod).into_iter().collect()
            }
            (Dialog::AddWeapon { from, .. }, ModAction::AddPick(w)) => {
                self.dialog = Some(Dialog::AddWeapon { from, weapon: Some(w) });
                Vec::new()
            }
            (Dialog::AddWeapon { from, weapon }, ModAction::AddConfirm) => {
                self.add_weapon(from, weapon);
                Vec::new()
            }
            (Dialog::NewImage { .. }, ModAction::NewImage { name, side, frames, copy_of }) => {
                self.new_image(&name, side, frames, copy_of.as_deref());
                Vec::new()
            }
            (Dialog::Rename(_), ModAction::RenameAsset { from, to }) => {
                self.dialog = None;
                self.rename_asset(&from, &to);
                Vec::new()
            }
            (Dialog::Browse, ModAction::Browse(b)) => self.browse(b),
            (Dialog::Browse, ModAction::Choice(_) | ModAction::Confirm(_)) => {
                self.dialog = None;
                self.browser = None;
                Vec::new()
            }
            (Dialog::FrameCount { .. }, ModAction::Confirm(yes)) => {
                self.dialog = None;
                if let Some(p) = self.pending_image.take().filter(|_| yes) {
                    self.apply_image(p);
                }
                Vec::new()
            }
            (_, ModAction::Choice(_) | ModAction::Confirm(_)) => {
                self.dialog = None;
                self.close_after_save = false;
                Vec::new()
            }
            (d, _) => {
                self.dialog = Some(d);
                Vec::new()
            }
        }
    }

    /// Runs `f` on the pack as one step (none when it changed nothing); a
    /// refusal leaves the pack as it was and says why. Whether it changed.
    pub(crate) fn change(
        &mut self,
        with_files: bool,
        f: impl FnOnce(&mut Pack) -> Result<Vec<Issue>, ModError>,
    ) -> bool {
        let Some(mut before) = Snapshot::of(&self.pack, with_files) else { return false };
        let result = f(&mut self.pack);
        before.keep_changed(&self.pack);
        match result {
            Ok(new) => {
                if before.same_as(&self.pack) {
                    return false;
                }
                self.history.push(before);
                self.changed();
                self.status = match new.iter().find(|i| i.is_error()) {
                    Some(e) => format!("This change has an error (it is listed below): {e}"),
                    None => String::new(),
                };
                if new.iter().any(Issue::is_error) {
                    self.show_problems = true;
                }
                true
            }
            Err(e) => {
                // A change of several steps may have made some of them.
                if !before.same_as(&self.pack) {
                    before.restore(&mut self.pack);
                }
                self.status = format!("Not changed: {e}");
                false
            }
        }
    }

    /// After every change, undo and redo: the problems again, and the item
    /// and node shown kept when they still exist.
    fn changed(&mut self) {
        self.generation += 1;
        self.refresh();
        if !self.exists(&self.item) {
            self.item = Item::World;
        }
        self.load_art();
        if !self.nodes().iter().any(|n| n.path == self.node) {
            self.node = self.item.path();
        }
    }

    fn refresh(&mut self) {
        self.issues = mods::check(&self.pack);
    }

    /// Whether `item` is in the mod.
    fn exists(&self, item: &Item) -> bool {
        match item {
            Item::World | Item::Sounds => true,
            Item::Weapon(id) => self.src().weapons.iter().any(|(w, _)| w == id),
            Item::Shared(id) => self.src().shared.contains_key(id),
            Item::Art(path) => mods::art::images(&self.pack).contains(path),
        }
    }

    pub(crate) fn select(&mut self, item: Item) {
        if self.exists(&item) {
            let other_image = self.item != item;
            self.node = item.path();
            self.item = item;
            self.focus = None;
            if other_image {
                self.tools.frame = 0;
                self.tools.pan = (0.0, 0.0);
            }
            self.load_art();
        }
    }

    /// Shows the item and node holding `path` (a field, or a file's path)
    /// and marks the field.
    fn go_to(&mut self, path: &str) {
        // Issue #19: an image's or a mask's problem shows the image, a
        // sound's the sounds.
        let image = olr_format::mods::image_of_mask(path).unwrap_or_else(|| path.to_string());
        if self.exists(&Item::Art(image.clone())) {
            self.select(Item::Art(image));
            return;
        }
        if path.starts_with(olr_format::mods::SFX_DIR) {
            self.select(Item::Sounds);
            return;
        }
        let file = olr_edit::mods::path::parse(self.src(), path).ok().map(|p| p.file).or_else(|| {
            let stem = path.strip_suffix(".ron").unwrap_or(path);
            File::of_pack_path(&format!("{stem}.ron"))
        });
        let item = match file {
            Some(File::Weapon(id)) => Item::Weapon(id),
            Some(File::Shared(id)) => Item::Shared(id),
            Some(File::Mod | File::Restrictions) | None => Item::World,
        };
        self.select(item);
        let nodes = self.nodes();
        if let Some(n) = tree::node_of(&nodes, path) {
            self.node = n.path.clone();
        }
        self.focus = Some(path.to_string());
        self.focus_gen += 1;
    }

    fn set_effect(&mut self, projectile: &str, event: Event, effect: Option<String>) {
        let new = match effect.as_deref().map(effect_named) {
            Some(None) => {
                self.status = format!("{} is no effect", effect.unwrap_or_default());
                return;
            }
            Some(Some(e)) => Some(e),
            None => None,
        };
        self.change(false, |p| {
            edit::<Vec<Effect>>(p, &event.list(projectile), |l| {
                let at = l.iter().position(|e| !matches!(e, Effect::Spawn(_)));
                match (new, at) {
                    (Some(e), Some(i)) if std::mem::discriminant(&e) != std::mem::discriminant(&l[i]) => {
                        l[i] = e
                    }
                    (Some(_), Some(_)) => {}
                    (Some(e), None) => l.insert(0, e),
                    (None, Some(i)) => {
                        l.remove(i);
                    }
                    (None, None) => {}
                }
                Ok(())
            })
        });
    }

    fn event_spawn(&mut self, projectile: &str, event: Event, on: bool) {
        self.change(false, |p| {
            edit::<Projectile>(p, projectile, |pr| {
                if on {
                    let key = pr.spawns.keys().next().cloned().unwrap_or_else(|| LIFTED_SPAWN.to_string());
                    pr.spawns.entry(key.clone()).or_insert_with(|| EventSpawn {
                        projectile: Some(Box::new(Projectile::new(Gfx::Pixels(vec![Colour(Rgba::rgb(
                            255, 255, 255,
                        ))])))),
                        ..EventSpawn::default()
                    });
                    let list = Event::of(pr, event).ok_or("the projectile has no timer")?;
                    if !list.iter().any(|e| matches!(e, Effect::Spawn(_))) {
                        list.push(Effect::Spawn(key));
                    }
                } else {
                    let list = Event::of(pr, event).ok_or("the projectile has no timer")?;
                    let keys = spawn_keys(list);
                    list.retain(|e| !matches!(e, Effect::Spawn(_)));
                    for k in keys {
                        drop_unused_spawn(pr, &k);
                    }
                }
                Ok(())
            })
        });
    }

    fn move_weapon(&mut self, id: &str, by: i32) {
        let ids = &self.src().weapons;
        let Some(i) = ids.iter().position(|(w, _)| w == id) else { return };
        let to = (i as i64 + i64::from(by)).clamp(0, ids.len() as i64 - 1) as usize;
        if to == i {
            return;
        }
        if self.change(false, |p| mods::move_weapon(p, id, to).map(|()| Vec::new())) && !self.order_noted {
            self.order_noted = true;
            self.dialog = Some(Dialog::Note(ORDER_NOTE.into()));
        }
    }

    /// A new weapon id for a weapon named or with id `like`.
    fn weapon_id(&self, like: &str) -> String {
        let src = self.src();
        new_weapon_id(like, |id| src.weapons.iter().any(|(w, _)| w == id))
    }

    fn add_weapon(&mut self, from: Option<String>, weapon: Option<String>) {
        let done = match (&from, &weapon) {
            (None, _) => {
                let id = self.weapon_id(NEW_WEAPON);
                self.change(false, |p| {
                    mods::add_weapon(p, &id, None, WeaponFrom::Blank, None).map(|()| Vec::new())
                })
                .then_some(id)
            }
            (Some(m), Some(w)) => {
                let other = if *m == self.pack.manifest.id {
                    Some(self.pack.clone())
                } else {
                    self.others.get(m).cloned()
                };
                let Some(other) = other else {
                    self.status = format!("mod {m} is not loaded yet");
                    return;
                };
                let name = match &other.source {
                    Source::Mod(o) => o.weapons.iter().find(|(i, _)| i == w).map(|(_, x)| x.name.clone()),
                    _ => None,
                };
                let id = self.weapon_id(name.as_deref().unwrap_or(w));
                self.change(true, |p| {
                    mods::add_weapon(p, &id, None, WeaponFrom::Copy { pack: &other, id: w }, None)
                        .map(|()| Vec::new())
                })
                .then_some(id)
            }
            (Some(_), None) => {
                self.status = "Choose a weapon to copy".into();
                return;
            }
        };
        self.dialog = None;
        if let Some(id) = done {
            self.select(Item::Weapon(id));
        }
    }

    fn save(&mut self) -> Vec<ModCommand> {
        if self.saving {
            return Vec::new();
        }
        self.refresh();
        let errors = self.errors();
        if errors > 0 {
            self.show_problems = true;
            self.status = format!(
                "Not saved: the mod has {errors} error{} (listed below; click one to go to it)",
                if errors == 1 { "" } else { "s" }
            );
            return Vec::new();
        }
        self.saving = true;
        self.pending_at = self.history.at();
        vec![ModCommand::Save(Box::new(self.pack.clone()))]
    }

    /// Play: the mod as it is, unsaved changes included, unless it has
    /// errors (they are listed, as for Save).
    fn play(&mut self) -> Vec<ModCommand> {
        self.refresh();
        let errors = self.errors();
        if errors > 0 {
            self.show_problems = true;
            self.status = format!(
                "Not played: the mod has {errors} error{} (listed below; click one to go to it)",
                if errors == 1 { "" } else { "s" }
            );
            return Vec::new();
        }
        let Some((level, _)) = self.play_level().cloned() else {
            self.status = "Not played: there is no level installed".into();
            return Vec::new();
        };
        self.playing = true;
        vec![ModCommand::Play { pack: Box::new(self.pack.clone()), level, bots: self.play.bots }]
    }

    fn saved(&mut self) -> Vec<ModCommand> {
        self.saving = false;
        // An edit made meanwhile stays unsaved.
        self.saved_at = self.pending_at;
        let warnings = self.issues.iter().filter(|i| i.severity == Severity::Warning).count();
        self.status = match warnings {
            0 => "Saved".into(),
            n => format!("Saved, with {n} warning{}", if n == 1 { "" } else { "s" }),
        };
        if std::mem::take(&mut self.close_after_save) && !self.dirty() {
            return vec![ModCommand::Close];
        }
        Vec::new()
    }
}

/// The spawn keys an event's list names.
fn spawn_keys(list: &[Effect]) -> Vec<String> {
    list.iter()
        .filter_map(|e| match e {
            Effect::Spawn(k) => Some(k.clone()),
            _ => None,
        })
        .collect()
}

/// Removes spawn `key` from `p` when no event names it any more.
fn drop_unused_spawn(p: &mut Projectile, key: &str) {
    let named = |l: &[Effect]| l.iter().any(|e| matches!(e, Effect::Spawn(k) if k == key));
    let used =
        named(&p.hit_terrain) || named(&p.hit_worm) || p.timer.as_ref().is_some_and(|t| named(&t.effects));
    if !used {
        p.spawns.remove(key);
    }
}

/// A map key as a path step's quoted text.
fn quote(key: &str) -> String {
    ron(&key.to_string())
}
