//! The level editor in the game (plan 14c, spec §6): `EditorHost` holds
//! `olr-editor`'s state and carries out its commands: packs opened, made,
//! copied and saved in the data directory's `packs/` (bundled content is
//! only copied from, R5), the file browser's listings and image reads
//! (R9), the recovery copy (R7), the remembered settings (`editor.ron`,
//! R8) and Play, which the app around it starts (`HostOut::Play`). No
//! window: the app draws `olr_editor::ui::show` and the canvas.

pub mod canvas;
mod files;
mod recovery;

pub use files::{MAX_ENTRIES, MAX_IMPORT_BYTES, editable_levels, level_dirs, list as list_dir};
pub use recovery::{INFO_FILE, RecoveryInfo, discard_recovery, list_recoveries};

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_edit::compose::Rect;
use olr_edit::new::{Template, new_pack, pack_id};
use olr_editor::ui::EditorData;
use olr_editor::{
    Catalog, Document, EditorAction, EditorCommand, EditorSettings, EditorState, ModEntry, ThemeEntry,
};
use olr_ui::{AppCommand, Store};

use crate::setup::EditedPlay;

/// How often the recovery copy is written while there are unsaved changes.
pub const RECOVERY_INTERVAL: Duration = Duration::from_secs(120);
/// The editor's settings in the data directory (R8).
pub const EDITOR_FILE: &str = "editor.ron";
/// What opening a recovery copy's folder as a pack says.
pub const RECOVERY_IN_PLACE: &str =
    "a recovery copy is reopened with the Editor menu's Recover, not opened in place";
/// What opening a bundled level in place says (R5, R6).
pub const BUNDLED: &str = "bundled content is not edited in place: use the Editor menu's Edit a copy";

/// What the host asks of the app around it.
#[derive(Clone, Debug, PartialEq)]
pub enum HostOut {
    /// This world rectangle of the bake changed: upload it.
    Upload(Rect),
    /// A new document (or a recovered one): rebuild the canvas whole.
    FullUpload,
    /// Start the editor's Play; answer with `EditorAction::PlayEnded`.
    Play(Box<EditedPlay>),
    /// The editor is done (after any unsaved-changes dialog).
    Closed,
    /// Something to tell the player (a pack that could not be made).
    Message(String),
}

/// The level editor's host.
pub struct EditorHost {
    state: EditorState,
    data: EditorData,
    /// The data directory's files (only `editor.ron` is written).
    store: Store,
    packs_dir: Option<PathBuf>,
    recovery_dir: Option<PathBuf>,
    /// The content layers, the data directory's packs first.
    roots: Vec<PathBuf>,
    /// Every root but the data directory's packs: never written.
    bundled: Vec<PathBuf>,
    /// The recovery id of the document open (its pack's id, or the
    /// recovery it was reopened from).
    recovery_id: Option<String>,
    /// Since when the unsaved changes wait for a recovery copy.
    dirty_since: Option<Instant>,
    warnings: Vec<String>,
}

impl EditorHost {
    /// The editor's catalog (themes with their tiles, mods) from `roots`
    /// (the content layers, `store`'s packs first); the settings from
    /// `store` (`editor.ron`).
    pub fn new(store: &mut Store, roots: &[&Path]) -> anyhow::Result<EditorHost> {
        let content = Content::open_layers(roots)?;
        let mut warnings = content.warnings().to_vec();
        let mut catalog = Catalog { home: std::env::home_dir(), ..Catalog::default() };
        for id in content.ids(PackKind::Theme) {
            match content.load_theme(&id) {
                Ok(t) => catalog.themes.push(ThemeEntry {
                    name: content.pack_name(&id).unwrap_or(&id).to_string(),
                    id,
                    front_tile: t.front_tile,
                    back_tile: t.back_tile,
                }),
                Err(e) => warnings.push(format!("{e:#}: not offered in the editor")),
            }
        }
        for id in content.ids(PackKind::Mod) {
            let name = content.pack_name(&id).unwrap_or(&id).to_string();
            catalog.mods.push(ModEntry { id, name });
        }
        let loaded = store.load_file::<EditorSettings>(EDITOR_FILE);
        warnings.extend(loaded.warnings);
        let packs_dir = store.packs_dir();
        let bundled =
            roots.iter().filter(|r| Some(**r) != packs_dir.as_deref()).map(|r| r.to_path_buf()).collect();
        Ok(EditorHost {
            state: EditorState::new(loaded.value.unwrap_or_default(), catalog),
            data: EditorData::default(),
            // The store that read `editor.ron`, with what it learnt (a
            // newer game's file is left untouched, a bad one kept).
            store: store.clone(),
            packs_dir,
            recovery_dir: store.recovery_dir(),
            roots: roots.iter().map(|r| r.to_path_buf()).collect(),
            bundled,
            recovery_id: None,
            dirty_since: None,
            warnings,
        })
    }

    pub fn state(&self) -> &EditorState {
        &self.state
    }

    pub fn data(&mut self) -> &mut EditorData {
        &mut self.data
    }

    /// The UI's data as last drawn (the canvas's rectangle).
    pub fn data_ref(&self) -> &EditorData {
        &self.data
    }

    /// The state and the UI's data together, for `olr_editor::ui::show`.
    pub fn parts(&mut self) -> (&EditorState, &mut EditorData) {
        (&self.state, &mut self.data)
    }

    /// What the host found wrong (an unreadable theme or settings file),
    /// for the app to log once.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Carries out `AppCommand::{EditLevel, CopyLevel, NewLevel, Recover,
    /// DiscardRecovery}`: resolves or makes the pack in the data
    /// directory's `packs/`, then opens it. A failure is a `Message` and
    /// opens nothing.
    pub fn open_from_menu(&mut self, c: &AppCommand) -> Vec<HostOut> {
        match self.menu_command(c) {
            Ok(out) => out,
            Err(e) => vec![HostOut::Message(format!("{e:#}"))],
        }
    }

    fn menu_command(&mut self, c: &AppCommand) -> anyhow::Result<Vec<HostOut>> {
        let packs = self
            .packs_dir
            .clone()
            .ok_or_else(|| anyhow::anyhow!("there is no data directory: nothing is edited"))?;
        match c {
            AppCommand::EditLevel(id) => {
                let dirs = level_dirs(&packs);
                let dir = dirs.get(id).ok_or_else(|| anyhow::anyhow!("level pack {id}: {BUNDLED}"))?;
                Ok(self.open_dir(&dir.clone()))
            }
            AppCommand::CopyLevel { from, name } => {
                let content = Content::open_layers(&self.root_refs())?;
                anyhow::ensure!(content.pack_kind(from) == Some(PackKind::Level), "no level pack {from}");
                let from_dir =
                    content.pack_dir(from).ok_or_else(|| anyhow::anyhow!("no level pack {from}"))?;
                let id = pack_id(name, |c| content.pack_kind(c).is_some() || packs.join(c).exists());
                let dir = packs.join(&id);
                new_pack(&dir, PackKind::Level, &id, name, Template::From(from_dir))?;
                Ok(self.open_dir(&dir))
            }
            AppCommand::NewLevel { name, size, theme } => {
                let content = Content::open_layers(&self.root_refs())?;
                let id = pack_id(name, |c| content.pack_kind(c).is_some() || packs.join(c).exists());
                let dir = packs.join(&id);
                new_pack(&dir, PackKind::Level, &id, name, Template::Level { size: *size, theme })?;
                Ok(self.open_dir(&dir))
            }
            AppCommand::Recover(id) => {
                let rec =
                    self.recovery_dir.clone().ok_or_else(|| anyhow::anyhow!("no recovery directory"))?;
                let (pack, info) = recovery::load(&rec, id)?;
                // The original's directory when it is there; else a new one
                // in packs (Save makes it).
                let dir = if info.origin.join(olr_pack::MANIFEST).is_file() && !self.is_bundled(&info.origin)
                {
                    info.origin
                } else {
                    packs.join(pack_id(id, |c| packs.join(c).exists()))
                };
                let doc = Document::open(pack, dir, self.state.catalog())?;
                self.recovery_id = Some(id.clone());
                self.dirty_since = None;
                Ok(self.act(vec![EditorAction::Recovered(Box::new(doc))]))
            }
            AppCommand::DiscardRecovery(id) => {
                let rec =
                    self.recovery_dir.clone().ok_or_else(|| anyhow::anyhow!("no recovery directory"))?;
                discard_recovery(&rec, id)?;
                Ok(Vec::new())
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Opens the pack at `dir` (`olr --edit <dir>`; the caller checked R6,
    /// and bundled content is refused here too).
    pub fn open_dir(&mut self, dir: &Path) -> Vec<HostOut> {
        let answer = self.load(dir);
        if let EditorAction::OpenFailed(why) = &answer {
            return vec![HostOut::Message(why.clone())];
        }
        self.act(vec![answer])
    }

    /// The pack at `dir` as an `Opened` answer, or why not (`OpenFailed`).
    fn load(&mut self, dir: &Path) -> EditorAction {
        if self.is_bundled(dir) {
            return EditorAction::OpenFailed(BUNDLED.into());
        }
        if self.in_recovery(dir) {
            return EditorAction::OpenFailed(RECOVERY_IN_PLACE.into());
        }
        let opened = olr_edit::pack::load(dir).map_err(|e| e.to_string()).and_then(|pack| {
            Document::open(pack, dir.to_path_buf(), self.state.catalog()).map_err(|e| e.to_string())
        });
        match opened {
            Ok(doc) => {
                // A copy of its own: an earlier session's (a crash's) stays
                // offered whatever this one does (batch B review).
                let taken = |c: &str| self.recovery_dir.as_ref().is_some_and(|r| r.join(c).exists());
                self.recovery_id = Some(pack_id(&doc.manifest().id, taken));
                self.dirty_since = None;
                EditorAction::Opened(Box::new(doc))
            }
            Err(e) => EditorAction::OpenFailed(format!("{}: {e}", dir.display())),
        }
    }

    /// One frame's actions from `olr_editor::ui::show`, the commands they
    /// lead to carried out (the answers fed back in order), and what the
    /// app must do. A new document's `FullUpload` stands for every upload
    /// before it.
    pub fn act(&mut self, actions: Vec<EditorAction>) -> Vec<HostOut> {
        let mut out = Vec::new();
        let mut queue: VecDeque<EditorAction> = actions.into();
        while let Some(a) = queue.pop_front() {
            let opened = matches!(a, EditorAction::Opened(_) | EditorAction::Recovered(_));
            let cmds = self.state.update(a);
            if opened && self.state.document().is_some() {
                out.retain(|o| !matches!(o, HostOut::Upload(_) | HostOut::FullUpload));
                out.push(HostOut::FullUpload);
            }
            for c in cmds {
                match c {
                    EditorCommand::Upload(_) if opened => {}
                    c => self.carry(c, &mut queue, &mut out),
                }
            }
        }
        out
    }

    fn carry(&mut self, c: EditorCommand, queue: &mut VecDeque<EditorAction>, out: &mut Vec<HostOut>) {
        match c {
            EditorCommand::Upload(r) => {
                if !out.contains(&HostOut::FullUpload) {
                    out.push(HostOut::Upload(r));
                }
            }
            EditorCommand::Open(dir) => queue.push_back(self.load(&dir)),
            EditorCommand::Create(n) => {
                let answer = if self.is_bundled(&n.dir) {
                    EditorAction::OpenFailed(BUNDLED.into())
                } else if self.in_recovery(&n.dir) {
                    EditorAction::OpenFailed(RECOVERY_IN_PLACE.into())
                } else {
                    let t = Template::Level { size: n.size, theme: &n.theme };
                    match new_pack(&n.dir, PackKind::Level, &n.id, &n.name, t) {
                        Ok(_) => self.load(&n.dir),
                        Err(e) => EditorAction::OpenFailed(e.to_string()),
                    }
                };
                queue.push_back(answer);
            }
            EditorCommand::Save { dir, pack } => {
                let result = if self.is_bundled(&dir) {
                    Err(BUNDLED.to_string())
                } else if self.in_recovery(&dir) {
                    Err(RECOVERY_IN_PLACE.to_string())
                } else {
                    // A recovered pack whose original is gone is saved anew.
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| format!("{}: {e}", dir.display()))
                        .and_then(|()| olr_edit::pack::save(&dir, &pack).map_err(|e| e.to_string()))
                };
                queue.push_back(EditorAction::Saved(result));
            }
            EditorCommand::Play { level, mod_id, bots } => {
                out.push(HostOut::Play(Box::new(EditedPlay { level: *level, mod_id, bots })));
            }
            EditorCommand::SaveSettings(s) => {
                if let Err(e) = self.store.save_file(EDITOR_FILE, &s) {
                    out.push(HostOut::Message(format!("saving the editor's settings: {e:#}")));
                }
            }
            EditorCommand::List(dir) => {
                let entries = files::list(&dir);
                queue.push_back(EditorAction::Listed { dir, entries });
            }
            EditorCommand::ReadImage(path) => queue.push_back(match files::read_image(&path) {
                Ok((file_name, bytes)) => EditorAction::ImportImage { file_name, bytes },
                Err(why) => EditorAction::ImportFailed(why),
            }),
            EditorCommand::RecoveryDone => {
                self.dirty_since = None;
                if let (Some(dir), Some(id)) = (&self.recovery_dir, &self.recovery_id)
                    && let Err(e) = discard_recovery(dir, id)
                {
                    out.push(HostOut::Message(format!("removing the recovery copy: {e:#}")));
                }
            }
            EditorCommand::Close => {
                self.recovery_id = None;
                out.push(HostOut::Closed);
            }
        }
    }

    /// The recovery timer: once a frame. While there are unsaved changes,
    /// the copy is written when they have waited `RECOVERY_INTERVAL` since
    /// the first tick that saw them (or since the last copy).
    pub fn tick(&mut self, now: Instant) -> Vec<HostOut> {
        if self.state.document().is_none() || !self.state.dirty() {
            self.dirty_since = None;
            return Vec::new();
        }
        let since = *self.dirty_since.get_or_insert(now);
        if now.saturating_duration_since(since) < RECOVERY_INTERVAL {
            return Vec::new();
        }
        self.dirty_since = Some(now);
        self.flush_recovery()
    }

    /// Writes the recovery copy now when there are unsaved changes (the
    /// timer's, and a quit that does not ask: `Shell::finish`).
    pub fn flush_recovery(&mut self) -> Vec<HostOut> {
        let Some(doc) = self.state.document().filter(|_| self.state.dirty()) else { return Vec::new() };
        let (Some(dir), Some(id)) = (&self.recovery_dir, &self.recovery_id) else { return Vec::new() };
        let info = RecoveryInfo { origin: doc.dir().to_path_buf(), name: doc.level().name.clone() };
        match recovery::write(dir, id, &doc.to_pack(), &info) {
            Ok(()) => Vec::new(),
            Err(e) => vec![HostOut::Message(format!("writing the recovery copy: {e:#}"))],
        }
    }

    fn root_refs(&self) -> Vec<&Path> {
        self.roots.iter().map(PathBuf::as_path).collect()
    }

    /// Whether `dir` lies in the recovery copies' folder (never a pack's
    /// home: a save there would go with its copy).
    fn in_recovery(&self, dir: &Path) -> bool {
        self.recovery_dir.as_ref().is_some_and(|r| files::inside(dir, std::slice::from_ref(r)))
    }

    /// Whether `dir` lies in bundled content (any root but the data
    /// directory's packs).
    fn is_bundled(&self, dir: &Path) -> bool {
        files::inside(dir, &self.bundled)
    }
}

/// What closing the window does (Review Focus 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDecision {
    /// Close it now.
    Quit,
    /// Wait: the editor asks about its unsaved changes first; the window
    /// closes once it answers `Closed`.
    Wait,
}

/// The window's close button with the editor `host` open (or none): the
/// editor's Close, after ending its Play (the caller ends the session).
/// Unsaved changes open the editor's dialog and the window waits.
pub fn on_close_request(host: Option<&mut EditorHost>) -> CloseDecision {
    let Some(h) = host else { return CloseDecision::Quit };
    if h.state().playing() {
        h.act(vec![EditorAction::PlayEnded]);
    }
    if h.act(vec![EditorAction::Close]).contains(&HostOut::Closed) {
        CloseDecision::Quit
    } else {
        CloseDecision::Wait
    }
}
