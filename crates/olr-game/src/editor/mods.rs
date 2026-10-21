//! The mod editor in the game (issue #17): `ModHost` holds
//! `olr-mod-editor`'s state and does its disk work. It edits one of the
//! user's mod packs (`Store::packs_dir`) in place; a new mod is first made
//! there as a copy of any installed mod (`olr_edit::new::new_pack`,
//! `Template::From`, its `editor/mod.ron` naming the mod copied: the
//! Original values). A built-in or OLX mod is only copied from, never
//! written. Saving writes the pack in place (`olr_edit::pack::save`, which
//! refuses errors); the shell lists the content again. Unsaved work is
//! kept in a recovery copy in `Store::mod_recovery_dir` (the level
//! editor's `recovery.rs`), written every `RECOVERY_INTERVAL` while there
//! are unsaved changes and removed on save and Close. No window: the app
//! draws `olr_mod_editor::ui::show`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_edit::new::{Template, new_pack, pack_id};
use olr_edit::pack::{Pack, Source};
use olr_format::mods::{MOD_EDITOR_FILE, ModEditorFile, ModSource};
use olr_mod_editor::ui::ModEditorData;
use olr_mod_editor::{ModAction, ModCommand, ModEditorState};
use olr_ui::AppCommand;

use super::RECOVERY_INTERVAL;
use super::files::{inside, mod_dirs};
use super::recovery::{self, RecoveryInfo};

/// What the shell does after a frame of the mod editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModOut {
    /// The pack was written: the content is listed again.
    Saved,
    /// The editor closed.
    Closed,
}

/// The mod editor and where its pack lives.
pub struct ModHost {
    state: ModEditorState,
    data: ModEditorData,
    /// The pack's folder, one of the user's packs.
    dir: PathBuf,
    /// Every root but `packs_dir`: never written.
    bundled: Vec<PathBuf>,
    /// The installed mods' folders (Add weapon's copies).
    mod_dirs: BTreeMap<String, PathBuf>,
    /// Where the recovery copies go (none without a data directory).
    recovery_dir: Option<PathBuf>,
    /// When the unsaved changes the next copy keeps began.
    dirty_since: Option<Instant>,
    /// What went wrong outside a command (a recovery copy), for the log.
    warnings: Vec<String>,
}

/// Mod pack `dir`, checked to be one: never a panic on a bad pack.
fn load_mod(dir: &Path) -> Result<Pack, String> {
    let pack = olr_edit::pack::load(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    match pack.source {
        Source::Mod(_) => Ok(pack),
        _ => Err(format!("{}: not a mod pack", dir.display())),
    }
}

/// The source of mod `id` of `content`, if it loads.
fn source_of(content: Option<&Content>, id: &str) -> Option<ModSource> {
    let pack = load_mod(content?.pack_dir(id)?).ok()?;
    match pack.source {
        Source::Mod(m) => Some(m),
        _ => None,
    }
}

impl ModHost {
    /// The editor for the Mod tab's `EditMod`, `NewMod` or `RecoverMod`:
    /// one of the user's mods in `packs_dir`, a new copy of any mod of
    /// `content` made there first, or a recovery copy; why it cannot open
    /// otherwise. `mods`: the installed mods (id, name); `taken`: every
    /// pack id installed (a new pack's id avoids them).
    pub fn open(
        command: &AppCommand,
        packs_dir: &Path,
        bundled: &[PathBuf],
        content: Option<&Content>,
        mods: Vec<(String, String)>,
        taken: &std::collections::BTreeSet<String>,
        recovery_dir: Option<PathBuf>,
    ) -> Result<ModHost, String> {
        let (pack, dir, recovered) = match command {
            AppCommand::EditMod(id) => {
                let dirs = mod_dirs(packs_dir);
                let dir = dirs.get(id).ok_or_else(|| format!("mod {id} is not one of yours: edit a copy"))?;
                if inside(dir, bundled) {
                    return Err(format!("mod {id} is built in: edit a copy"));
                }
                (load_mod(dir)?, dir.clone(), false)
            }
            AppCommand::NewMod { from, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err("a new mod needs a name".into());
                }
                let src = content
                    .and_then(|c| c.pack_dir(from))
                    .ok_or_else(|| format!("mod {from} is not installed"))?;
                let base = if name.chars().any(|c| c.is_ascii_alphanumeric()) { name } else { "mod" };
                let id = pack_id(base, |c| taken.contains(c) || packs_dir.join(c).exists());
                let dir = packs_dir.join(&id);
                if inside(&dir, bundled) {
                    return Err("the packs folder is inside the bundled content".into());
                }
                new_pack(&dir, PackKind::Mod, &id, name, Template::From(src))
                    .map_err(|e| format!("copying {from}: {e}"))?;
                // The mod it was copied from, for the Original values.
                let file = ModEditorFile { base: Some(from.clone()) };
                let text =
                    olr_format::ron_io::to_canonical(MOD_EDITOR_FILE, &file).map_err(|e| e.to_string())?;
                let path = dir.join(MOD_EDITOR_FILE);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                (load_mod(&dir)?, dir, false)
            }
            AppCommand::RecoverMod(id) => {
                let rdir = recovery_dir.as_deref().ok_or("there is no data directory")?;
                let (pack, info) = recovery::load(rdir, id).map_err(|e| format!("{e:#}"))?;
                if !matches!(pack.source, Source::Mod(_)) {
                    return Err(format!("recovery {id} is not a mod"));
                }
                // It goes back where it was edited, never into bundled content.
                if inside(&info.origin, bundled) || !info.origin.starts_with(packs_dir) {
                    return Err(format!("recovery {id} was not one of your mods"));
                }
                (pack, info.origin, true)
            }
            _ => return Err("not a mod editor command".into()),
        };
        let base = ModEditorState::base_of(&pack).and_then(|b| source_of(content, &b));
        let state = ModEditorState::new(pack, base, mods.clone(), recovered)?;
        let mod_dirs = mods
            .iter()
            .filter_map(|(id, _)| Some((id.clone(), content?.pack_dir(id)?.to_path_buf())))
            .collect();
        Ok(ModHost {
            state,
            data: ModEditorData::default(),
            dir,
            bundled: bundled.to_vec(),
            mod_dirs,
            recovery_dir,
            dirty_since: None,
            warnings: Vec::new(),
        })
    }

    pub fn state(&self) -> &ModEditorState {
        &self.state
    }

    /// The state and the panels' data, for `olr_mod_editor::ui::show`.
    pub fn parts(&mut self) -> (&ModEditorState, &mut ModEditorData) {
        (&self.state, &mut self.data)
    }

    /// Where the mod saves.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What went wrong since the last call (a recovery copy), for the log.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// A frame's actions carried out, the host's answers fed back.
    pub fn act(&mut self, actions: Vec<ModAction>) -> Vec<ModOut> {
        let mut out = Vec::new();
        let mut queue: std::collections::VecDeque<ModAction> = actions.into();
        while let Some(a) = queue.pop_front() {
            for c in self.state.update(a) {
                match c {
                    ModCommand::Save(pack) => match self.save(&pack) {
                        Ok(()) => {
                            out.push(ModOut::Saved);
                            queue.push_front(ModAction::Saved);
                        }
                        Err(why) => queue.push_front(ModAction::Failed(why)),
                    },
                    ModCommand::LoadMod(id) => {
                        let loaded =
                            self.mod_dirs.get(&id).ok_or_else(|| format!("mod {id} is not installed"));
                        match loaded.and_then(|d| load_mod(d)) {
                            Ok(p) => queue.push_front(ModAction::ModLoaded(id, Box::new(p))),
                            Err(why) => queue.push_front(ModAction::Failed(why)),
                        }
                    }
                    ModCommand::Close => {
                        self.discard_recovery();
                        out.push(ModOut::Closed);
                    }
                }
            }
        }
        out
    }

    /// The recovery timer, once a frame: while there are unsaved changes,
    /// the copy is written when they have waited `RECOVERY_INTERVAL`.
    pub fn tick(&mut self, now: Instant) {
        if !self.state.dirty() {
            self.dirty_since = None;
            return;
        }
        let since = *self.dirty_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= RECOVERY_INTERVAL {
            self.dirty_since = Some(now);
            self.write_recovery();
        }
    }

    /// Writes the recovery copy now when there are unsaved changes (a quit
    /// that does not ask).
    pub fn flush_recovery(&mut self) {
        if self.state.dirty() {
            self.write_recovery();
        }
    }

    /// The recovery copy's id: the pack's.
    fn recovery_id(&self) -> &str {
        &self.state.pack().manifest.id
    }

    fn write_recovery(&mut self) {
        let Some(rdir) = self.recovery_dir.clone() else { return };
        let info = RecoveryInfo { origin: self.dir.clone(), name: self.state.pack().manifest.name.clone() };
        match recovery::write(&rdir, self.recovery_id(), self.state.pack(), &info) {
            Ok(w) => self.warnings.extend(w),
            Err(e) => self.warnings.push(format!("writing the mod's recovery copy: {e:#}")),
        }
    }

    fn discard_recovery(&mut self) {
        if let Some(rdir) = self.recovery_dir.clone()
            && let Err(e) = recovery::discard_recovery(&rdir, self.recovery_id())
        {
            self.warnings.push(format!("removing the mod's recovery copy: {e:#}"));
        }
    }

    /// Writes `pack` in place (checked again by `pack::save`).
    fn save(&mut self, pack: &Pack) -> Result<(), String> {
        if inside(&self.dir, &self.bundled) {
            return Err("a built-in mod is never changed".into());
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        olr_edit::pack::save(&self.dir, pack).map_err(|e| format!("saving {}: {e}", self.dir.display()))?;
        self.dirty_since = None;
        self.discard_recovery();
        Ok(())
    }
}
