//! The skin editor in the game (issue #14): `SkinHost` holds
//! `olr-skin-editor`'s state and does its disk work. It opens one of the
//! user's skin packs (`Store::packs_dir`) in place, or a new skin: blank
//! (double resolution) or a copy of any installed skin (a built-in or OLX
//! skin is only copied from, never written). Saving writes the user's own
//! pack in place, or at the first save a new pack in `packs_dir` named as
//! asked; the shell lists the content again, so the profiles' skin picker
//! offers it at once. No window: the app draws `olr_skin_editor::ui::show`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_content::Content;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::new::pack_id;
use olr_edit::pack::{Pack, Source};
use olr_format::skins::SkinSource;
use olr_skin_editor::ui::SkinEditorData;
use olr_skin_editor::{SkinAction, SkinCommand, SkinDoc, SkinEditorState};
use olr_ui::AppCommand;

use super::files::{inside, skin_dirs};

/// A new blank skin's name.
pub const NEW_SKIN_NAME: &str = "New skin";

/// What the shell does after a frame of the skin editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkinOut {
    /// A pack was written: the content is listed again.
    Saved,
    /// The editor closed.
    Closed,
}

/// The skin editor and its pack.
pub struct SkinHost {
    state: SkinEditorState,
    data: SkinEditorData,
    /// The pack as opened (manifest and other files), its skin replaced at
    /// each save.
    pack: Pack,
    /// Where it saves: one of the user's packs, once it has one.
    dir: Option<PathBuf>,
    packs_dir: PathBuf,
    /// Every root but `packs_dir`: never written.
    bundled: Vec<PathBuf>,
    /// Every pack id installed when it opened (a new pack's id avoids them).
    taken: BTreeSet<String>,
    last_tick: Option<Instant>,
}

/// A pack holding `source` with a fresh manifest (id and name set at the
/// first save).
fn fresh(source: SkinSource) -> Pack {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: String::new(),
        name: source.name.clone(),
        version: "0.1.0".into(),
        kind: PackKind::Skin,
        authors: Vec::new(),
        origin: None,
        license: None,
    };
    Pack { manifest, source: Source::Skin(source), composition: None, other: Default::default() }
}

/// The skin pack at `dir`, checked as the game loads it: a bad pack is
/// refused with why, never opened half (never a panic).
fn load_skin(dir: &Path) -> Result<(Pack, SkinSource), String> {
    let checked = olr_edit::skin::check_dir(dir);
    if let Some(e) = checked.errors.first() {
        return Err(format!("{}: {e}", dir.display()));
    }
    let pack = olr_edit::pack::load(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let Source::Skin(src) = &pack.source else {
        return Err(format!("{}: not a skin pack", dir.display()));
    };
    let src = src.clone();
    Ok((pack, src))
}

impl SkinHost {
    /// The editor for the Editor screen's `EditSkin` or `NewSkin`: one of
    /// the user's skins in `packs_dir` in place, a blank one, or a copy of
    /// any skin of `content`; why it cannot open otherwise. `taken`: every
    /// pack id installed (a new pack's id avoids them).
    pub fn open(
        command: &AppCommand,
        packs_dir: &Path,
        bundled: &[PathBuf],
        content: Option<&Content>,
        taken: BTreeSet<String>,
    ) -> Result<SkinHost, String> {
        let (pack, src, dir) = match command {
            AppCommand::EditSkin(id) => {
                let dirs = skin_dirs(packs_dir);
                let dir =
                    dirs.get(id).ok_or_else(|| format!("skin {id} is not one of yours: edit a copy"))?;
                if inside(dir, bundled) {
                    return Err(format!("skin {id} is built in: edit a copy"));
                }
                let (pack, src) = load_skin(dir)?;
                (pack, src, Some(dir.clone()))
            }
            AppCommand::NewSkin(None) => {
                let src = olr_edit::skin::blank(NEW_SKIN_NAME, true);
                (fresh(src.clone()), src, None)
            }
            AppCommand::NewSkin(Some(id)) => {
                let dir = content
                    .and_then(|c| c.pack_dir(id))
                    .ok_or_else(|| format!("skin {id} is not installed"))?;
                let (mut pack, mut src) = load_skin(dir)?;
                src.name = format!("{} copy", src.name);
                pack.manifest.origin = None;
                (pack, src, None)
            }
            _ => return Err("not a skin editor command".into()),
        };
        let doc = SkinDoc::new(src).map_err(|e| e.to_string())?;
        let id = dir.as_ref().map(|_| pack.manifest.id.clone());
        Ok(SkinHost {
            state: SkinEditorState::new(doc, id),
            data: SkinEditorData::default(),
            pack,
            dir,
            packs_dir: packs_dir.to_path_buf(),
            bundled: bundled.to_vec(),
            taken,
            last_tick: None,
        })
    }

    pub fn state(&self) -> &SkinEditorState {
        &self.state
    }

    /// The state and the panels' data, for `olr_skin_editor::ui::show`.
    pub fn parts(&mut self) -> (&SkinEditorState, &mut SkinEditorData) {
        (&self.state, &mut self.data)
    }

    /// Where the skin saves, once it has a pack.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// A frame's actions carried out, the host's answers fed back.
    pub fn act(&mut self, actions: Vec<SkinAction>) -> Vec<SkinOut> {
        let mut out = Vec::new();
        let mut queue: std::collections::VecDeque<SkinAction> = actions.into();
        while let Some(a) = queue.pop_front() {
            for c in self.state.update(a) {
                match c {
                    SkinCommand::Save { id, name, source } => match self.save(id, &name, *source) {
                        Ok(id) => {
                            out.push(SkinOut::Saved);
                            queue.push_front(SkinAction::Saved(id));
                        }
                        Err(why) => queue.push_front(SkinAction::Failed(why)),
                    },
                    SkinCommand::Close => out.push(SkinOut::Closed),
                }
            }
        }
        out
    }

    /// The preview's clock: the time since the last frame (at most a
    /// quarter second).
    pub fn tick(&mut self, now: Instant) {
        let dt = self.last_tick.map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
        self.last_tick = Some(now);
        self.state.update(SkinAction::Tick(dt.as_secs_f32().min(0.25)));
    }

    /// Writes the skin: into its own pack (`id`), or a new pack named
    /// `name` in `packs_dir`. Returns the pack's id.
    fn save(&mut self, id: Option<String>, name: &str, source: SkinSource) -> Result<String, String> {
        let mut pack = self.pack.clone();
        pack.source = Source::Skin(source);
        pack.manifest.name = name.to_string();
        if let (Some(id), Some(dir)) = (id, self.dir.clone()) {
            if inside(&dir, &self.bundled) {
                return Err("a built-in skin is never changed".into());
            }
            olr_edit::pack::save(&dir, &pack).map_err(|e| format!("saving {}: {e}", dir.display()))?;
            self.pack = pack;
            return Ok(id);
        }
        // A new pack: an id free among the installed packs and the folders.
        let base = if name.chars().any(|c| c.is_ascii_alphanumeric()) { name } else { "skin" };
        let id = pack_id(base, |c| self.taken.contains(c) || self.packs_dir.join(c).exists());
        let dir = self.packs_dir.join(&id);
        if inside(&dir, &self.bundled) {
            return Err("the packs folder is inside the bundled content".into());
        }
        pack.manifest.id = id.clone();
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        if let Err(e) = olr_edit::pack::save(&dir, &pack) {
            // Nothing half made stays behind.
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("saving {}: {e}", dir.display()));
        }
        self.taken.insert(id.clone());
        self.dir = Some(dir);
        self.pack = pack;
        Ok(id)
    }
}
