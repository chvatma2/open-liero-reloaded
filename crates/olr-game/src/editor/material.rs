//! The material editor in the game (issue #44): `MaterialHost` holds
//! `olr-material-editor`'s state and does its disk work. It lists the
//! built-in materials (`olr_content::materials_dir` beside the content,
//! read-only: never written) and the user's (`Store::materials_dir`),
//! writes the user's files (`<id>.ron`, checked by
//! `olr_format::materials::material_text`, through a hidden temporary file
//! and a rename), renames and deletes them. Unsaved changes are kept in a
//! recovery copy, `Store::material_recovery_dir`'s `<id>.ron` (the open
//! material as edited), written every `RECOVERY_INTERVAL` while there are
//! unsaved changes, as the other editors' are (`recovery.rs`), and removed
//! once they are saved or thrown away. No window: the app draws
//! `olr_material_editor::ui::show`. Issue #45: the editor's preview
//! commands wait here for the app (`take_preview`), whose
//! `heat_preview::HeatPreview` renders the picture the panels show
//! (`set_preview`).

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use olr_format::materials::{MaterialSource, material_file, material_text, valid_material_id};
use olr_format::ron_io::{MAX_RON_BYTES, from_canonical, to_canonical};
use olr_material_editor::ui::{MaterialEditorData, PreviewImage};
use olr_material_editor::{MaterialAction, MaterialCommand, MaterialEditorState, PreviewCommand};
use olr_ui::AppCommand;

use super::RECOVERY_INTERVAL;
use super::files::inside;

/// What the shell does after a frame of the material editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialOut {
    /// The editor closed.
    Closed,
}

/// The material editor and its libraries.
pub struct MaterialHost {
    state: MaterialEditorState,
    data: MaterialEditorData,
    /// The user's library: the only one written.
    user_dir: PathBuf,
    /// The built-in library, never written.
    builtin_dir: Option<PathBuf>,
    /// Where the recovery copies go (none without a data directory).
    recovery_dir: Option<PathBuf>,
    /// When the unsaved changes the next copy keeps began.
    dirty_since: Option<Instant>,
    /// The recovery copy on disk, by id: removed once its changes are
    /// saved or thrown away.
    written: Option<String>,
    /// What went wrong outside a command (a recovery copy), for the log.
    warnings: Vec<String>,
    /// The preview's commands since the app last took them (issue #45).
    preview: Vec<PreviewCommand>,
}

/// The most preview commands kept for the app (a frame asks one or two).
pub const MAX_PREVIEW_COMMANDS: usize = 64;

/// Writes `text` to `path` through a hidden sibling and a rename: a crash
/// leaves the old file or the new one, never half of one.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{}: no folder", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("material");
    let tmp = dir.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Material recovery `id`'s file in `dir`; refused unless `id` is a
/// material's id (never a path out of `dir`).
fn recovery_file(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_material_id(id) {
        return Err(format!("{id:?} is not a material recovery"));
    }
    Ok(dir.join(material_file(id)))
}

/// Writes `m` (unsaved changes, maybe not valid yet) as its recovery copy
/// in `dir`.
pub fn write_material_recovery(dir: &Path, m: &MaterialSource) -> Result<(), String> {
    let path = recovery_file(dir, &m.id)?;
    let text = to_canonical(&material_file(&m.id), m).map_err(|e| e.to_string())?;
    write_atomic(&path, &text)
}

/// Material recovery `id` of `dir`, read (bounded) as written: its id must
/// be its file's. Its values are checked when it is saved, not here.
pub fn load_material_recovery(dir: &Path, id: &str) -> Result<MaterialSource, String> {
    let path = recovery_file(dir, id)?;
    let damaged = |why: String| format!("material recovery {id} is damaged: {why}");
    let meta = std::fs::metadata(&path).map_err(|e| damaged(e.to_string()))?;
    if !meta.is_file() {
        return Err(damaged("not a file".into()));
    }
    let mut text = String::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_RON_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| damaged(e.to_string()))?;
    if text.len() as u64 > MAX_RON_BYTES {
        return Err(damaged(format!("above {MAX_RON_BYTES} bytes")));
    }
    let m: MaterialSource = from_canonical(&material_file(id), &text).map_err(|e| damaged(e.to_string()))?;
    if m.id != id {
        return Err(damaged(format!("it holds material {:?}", m.id)));
    }
    Ok(m)
}

/// The material recoveries in `dir`: (id, material name, damaged), by id.
/// A damaged one is named by its id; hidden files (a write in progress)
/// are left out.
pub fn list_material_recoveries(dir: &Path) -> Vec<(String, String, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, String, bool)> = rd
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".ron").map(str::to_string))
        .filter(|id| valid_material_id(id))
        .map(|id| match load_material_recovery(dir, &id) {
            Ok(m) => (id, m.name, false),
            Err(_) => (id.clone(), id, true),
        })
        .collect();
    out.sort();
    out
}

/// Removes material recovery `id` from `dir` (absent: nothing to do).
pub fn discard_material_recovery(dir: &Path, id: &str) -> Result<(), String> {
    let path = recovery_file(dir, id)?;
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", path.display())),
        _ => Ok(()),
    }
}

impl MaterialHost {
    /// The editor for the Material tab's `OpenMaterialEditor` or
    /// `RecoverMaterial`: the built-in library `builtin_dir` (read-only)
    /// and the user's `user_dir` listed; why it cannot open otherwise.
    pub fn open(
        command: &AppCommand,
        builtin_dir: Option<&Path>,
        user_dir: &Path,
        recovery_dir: Option<PathBuf>,
    ) -> Result<MaterialHost, String> {
        if let Some(b) = builtin_dir
            && inside(user_dir, &[b.to_path_buf()])
        {
            return Err("your materials folder is inside the built-in materials".into());
        }
        let (library, warnings) = olr_edit::material::list_materials(builtin_dir, Some(user_dir));
        let (state, written) = match command {
            AppCommand::OpenMaterialEditor => (MaterialEditorState::new(library), None),
            AppCommand::RecoverMaterial(id) => {
                let rdir = recovery_dir.as_deref().ok_or("there is no data directory")?;
                let m = load_material_recovery(rdir, id)?;
                (MaterialEditorState::recovered(library, m), Some(id.clone()))
            }
            _ => return Err("not a material editor command".into()),
        };
        Ok(MaterialHost {
            state,
            data: MaterialEditorData::default(),
            user_dir: user_dir.to_path_buf(),
            builtin_dir: builtin_dir.map(Path::to_path_buf),
            recovery_dir,
            dirty_since: None,
            written,
            warnings,
            preview: Vec::new(),
        })
    }

    pub fn state(&self) -> &MaterialEditorState {
        &self.state
    }

    /// The state and the panels' data, for `olr_material_editor::ui::show`.
    pub fn parts(&mut self) -> (&MaterialEditorState, &mut MaterialEditorData) {
        (&self.state, &mut self.data)
    }

    /// The user's library.
    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    /// The picture the preview shows (issue #45; none: its place says it
    /// needs a graphics device).
    pub fn set_preview(&mut self, image: Option<PreviewImage>) {
        self.data.set_preview(image);
    }

    /// The preview's commands since the last call, in order, for the app's
    /// `HeatPreview` (issue #45).
    pub fn take_preview(&mut self) -> Vec<PreviewCommand> {
        std::mem::take(&mut self.preview)
    }

    /// What went wrong since the last call (the listing, a recovery copy),
    /// for the log.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// A frame's actions carried out, the host's answers fed back.
    pub fn act(&mut self, actions: Vec<MaterialAction>) -> Vec<MaterialOut> {
        let mut out = Vec::new();
        let mut queue: std::collections::VecDeque<MaterialAction> = actions.into();
        while let Some(a) = queue.pop_front() {
            for c in self.state.update(a) {
                let answer = match c {
                    MaterialCommand::Create(m) => self.create(&m).map(|()| MaterialAction::Created(m)),
                    MaterialCommand::Save(m) => self.write(&m).map(|()| MaterialAction::Saved(m)),
                    MaterialCommand::Rename { from, to } => {
                        self.rename(&from, &to).map(|()| MaterialAction::Renamed { from, to })
                    }
                    MaterialCommand::Delete(id) => self.delete(&id).map(|()| MaterialAction::Deleted(id)),
                    MaterialCommand::Close => {
                        out.push(MaterialOut::Closed);
                        continue;
                    }
                    MaterialCommand::Preview(p) => {
                        // Never more than a bound: an app without a
                        // preview takes none.
                        if self.preview.len() < MAX_PREVIEW_COMMANDS {
                            self.preview.push(p);
                        }
                        continue;
                    }
                };
                queue.push_front(answer.unwrap_or_else(MaterialAction::Failed));
            }
        }
        if out.contains(&MaterialOut::Closed) {
            self.drop_recovery();
        } else {
            self.sync_recovery();
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

    fn write_recovery(&mut self) {
        let Some(rdir) = self.recovery_dir.clone() else { return };
        let Some(m) = self.state.doc().cloned() else { return };
        // Another material's copy goes first: one copy, of the one open.
        if self.written.as_ref().is_some_and(|w| *w != m.id) {
            self.drop_recovery();
        }
        match write_material_recovery(&rdir, &m) {
            Ok(()) => self.written = Some(m.id),
            Err(e) => self.warnings.push(format!("writing the material's recovery copy: {e}")),
        }
    }

    /// The recovery copy removed once it keeps nothing: its changes saved
    /// or thrown away, or another material open.
    fn sync_recovery(&mut self) {
        let keeps = |w: &String| self.state.dirty() && self.state.doc().is_some_and(|d| d.id == *w);
        if self.written.as_ref().is_some_and(|w| !keeps(w)) {
            self.dirty_since = None;
            self.drop_recovery();
        }
    }

    fn drop_recovery(&mut self) {
        let (Some(rdir), Some(id)) = (self.recovery_dir.clone(), self.written.take()) else { return };
        if let Err(e) = discard_material_recovery(&rdir, &id) {
            self.warnings.push(format!("removing the material's recovery copy: {e}"));
        }
    }

    /// The user's file of material `id`, never in the built-in library.
    fn file(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_material_id(id) {
            return Err(format!("{id:?} is not a material's id"));
        }
        if let Some(b) = &self.builtin_dir
            && inside(&self.user_dir, std::slice::from_ref(b))
        {
            return Err("a built-in material is never changed".into());
        }
        Ok(self.user_dir.join(material_file(id)))
    }

    /// Writes `m`, checked, to the user's library.
    fn write(&self, m: &MaterialSource) -> Result<(), String> {
        let path = self.file(&m.id)?;
        let text = material_text(m).map_err(|e| e.to_string())?;
        write_atomic(&path, &text)
    }

    /// Writes new material `m`, never over a file of the user's.
    fn create(&self, m: &MaterialSource) -> Result<(), String> {
        let path = self.file(&m.id)?;
        if path.exists() {
            return Err(format!("{} is already in your materials", material_file(&m.id)));
        }
        self.write(m)
    }

    /// The user's material `from` written as `to`, then `from`'s file
    /// removed (the same id: written in place).
    fn rename(&self, from: &str, to: &MaterialSource) -> Result<(), String> {
        let old = self.file(from)?;
        if to.id != from && self.file(&to.id)?.exists() {
            return Err(format!("{} is already in your materials", material_file(&to.id)));
        }
        self.write(to)?;
        if to.id != from {
            std::fs::remove_file(&old).map_err(|e| format!("{}: {e}", old.display()))?;
        }
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        let path = self.file(id)?;
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
    }
}
