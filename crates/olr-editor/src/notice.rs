//! The library-change notice, export and import (issue #84; the
//! asset-based levels design's "The library, export and import", calls 7,
//! 9 and 17), the state's side:
//!
//! - **The notice**: opening a level (`Opened`, `Recovered`) asks the host
//!   for the library's versions (`CompareLibrary`); when its answer
//!   (`LibraryCompared`) shows a linked asset changed or missing,
//!   `Dialog::LibraryChanged` lists them. Its answers (`LibraryAction`):
//!   Update (one step, `olr_edit::library::update`), Update all (one step
//!   each), Update anyway (forced: one step whose resolutions the status
//!   line and the level panel's notes list), Add to your library (the
//!   level's copy goes to the host as `SaveAsset`; its `AssetSaved`
//!   relinks the asset, one step: the state never writes the library) and
//!   Make it the level's own (`adopt`). Later closes it; File ▾ Library
//!   changes… reopens it. Later answers wait in the level's problems
//!   (`link_problems`): each break and each missing asset an error about
//!   the level's links. None refuses a save: the level holds its own copy
//!   of every asset.
//! - **A library change while the level is open** (#83): a new answer
//!   takes the library's new version of an asset whose version changed
//!   since the last answer when it breaks nothing; what the user put off
//!   with Later stays put off.
//! - **Export** (`ExportLevel`): asks to save first (Save or Cancel), then
//!   the browser's save purpose (`Purpose::ExportLevel`: a File name,
//!   `<pack id>.olrlevel`); `ExportTo` sends `EditorCommand::ExportLevel`.
//! - **Import** (`Purpose::ImportLevel`'s pick, `EditorCommand::
//!   ImportLevel`): the host's `LevelImported` opens the level (asking
//!   first about unsaved changes), the status line naming what came in.

use std::path::PathBuf;

use olr_edit::library::{Imported, ImportedAsset, Libraries, Status, Updated, origin_label};
use olr_format::levels::AssetOrigin;
use olr_pack::Severity;

use crate::browser::{Browser, Purpose};
use crate::document::{Document, Problem};
use crate::links::copies_said;
use crate::state::{Dialog, DocTab, EditorCommand, EditorState, Pending};

/// The extension of a level export (`olr_edit::library::export`).
pub const LEVEL_EXPORT_EXTENSION: &str = "olrlevel";

/// One of the notice's answers (issue #84), by the level's asset key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryAction {
    /// The library's version of a changed asset that breaks nothing.
    Update(String),
    /// Every changed asset that breaks nothing, one step each.
    UpdateAll,
    /// The library's version whatever it breaks, each break resolved.
    UpdateAnyway(String),
    /// A missing asset: the level's copy into the user's library, then
    /// linked to it.
    Restore(String),
    /// A missing asset made the level's own.
    Adopt(String),
}

/// What the status line says of an import: "Castle 2: reused yours
/// console; added Console 2".
pub fn import_said(report: &Imported) -> String {
    let what: Vec<String> = report
        .assets
        .iter()
        .filter_map(|a| match a {
            ImportedAsset::Reused { origin, .. } => Some(format!("reused {}", origin_label(origin))),
            ImportedAsset::Added { name, .. } => Some(format!("added {name}")),
            ImportedAsset::Own { .. } => None,
        })
        .collect();
    if what.is_empty() {
        format!("{}: imported", report.name)
    } else {
        format!("{}: {}", report.name, what.join("; "))
    }
}

/// Whether `name` is a plain file name (no folder, not hidden).
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', ':']) && name.len() <= 200
}

impl Document {
    /// The level's links as the problems list them (issue #84): each break
    /// of a changed asset ("crane: value claw_x: no exposed property
    /// claw_x in Crane") and each copy of a missing one ("crane: Crane is
    /// not in your library"), errors.
    pub fn link_problems(&self, links: &[olr_edit::library::Link]) -> Vec<Problem> {
        let mut out = Vec::new();
        let Some(p) = self.placement() else { return out };
        for l in links {
            let name = self.asset_name(&l.key).unwrap_or_else(|| l.key.clone());
            match &l.status {
                Status::Same => {}
                Status::Changed { breaks } => out.extend(
                    breaks
                        .iter()
                        .map(|b| Problem { severity: Severity::Error, text: format!("{b} in {name}") }),
                ),
                Status::Missing => {
                    let library = match l.origin {
                        AssetOrigin::Builtin(_) => "the built-in library",
                        _ => "your library",
                    };
                    for c in p.file.copies.iter().filter(|c| c.asset == l.key) {
                        out.push(Problem {
                            severity: Severity::Error,
                            text: format!("{}: {name} is not in {library}", c.id),
                        });
                    }
                }
            }
        }
        out
    }
}

impl EditorState {
    /// The library's sources last answered, if any (the notice's).
    pub fn libraries(&self) -> Option<&Libraries> {
        self.libs.as_ref()
    }

    /// What forced updates resolved since the level opened, by copy
    /// ("crane: dropped value claw_x"), as the status line said them (the
    /// level's notes keep them too).
    pub fn resolutions(&self) -> &[String] {
        &self.resolutions
    }

    /// Whether the notice has anything to list.
    pub fn library_changed(&self) -> bool {
        self.links.iter().any(|l| l.status != Status::Same)
    }

    /// The level's links against the kept answer (the level tab in
    /// place): origins the answer lacks (a copy placed since) are left out
    /// until the next one.
    pub(crate) fn relink_links(&mut self) {
        let (Some(doc), Some(libs)) = (&self.doc, &self.libs) else {
            self.links.clear();
            return;
        };
        let mut links = doc.compare_library(libs);
        links.retain(|l| libs.contains_key(&l.origin));
        self.links = links;
    }

    /// The host's `LibraryCompared`: an opening's answer opens the notice
    /// when anything changed; a later one takes the library's new versions
    /// that break nothing (#83's library saves), leaving what was put off.
    pub(crate) fn library_compared(&mut self, libs: Libraries) -> Vec<EditorCommand> {
        let opening = std::mem::take(&mut self.notice_on_compare);
        let fresh: Libraries = match &self.libs {
            Some(old) if !opening => libs
                .iter()
                .filter(|(o, v)| old.get(*o) != Some(*v))
                .map(|(o, v)| (o.clone(), v.clone()))
                .collect(),
            _ => libs.clone(),
        };
        let mut kept = self.libs.take().unwrap_or_default();
        kept.extend(libs);
        self.libs = Some(kept);
        let (cmds, said) = self.on_tab(DocTab::Level, |s| {
            let taken = if opening { (Vec::new(), Vec::new()) } else { s.take_library(&fresh) };
            s.relink_links();
            s.refresh_problems();
            taken
        });
        if !said.is_empty() {
            self.say(said.join("; "));
        }
        if opening && self.library_changed() && self.dialog.is_none() {
            self.dialog = Some(Dialog::LibraryChanged);
        }
        if matches!(self.dialog, Some(Dialog::LibraryChanged)) && !self.library_changed() {
            self.dialog = None;
        }
        cmds
    }

    /// File ▾ Library changes…: the notice again (the library asked anew).
    pub(crate) fn library_changes(&mut self) -> Vec<EditorCommand> {
        if self.document().is_none() {
            return Vec::new();
        }
        if !self.library_changed() {
            self.say("the level's library assets are as your library has them".into());
        }
        self.dialog = Some(Dialog::LibraryChanged);
        self.compare_library()
    }

    /// The library's name of the level's asset `key` (what an update makes
    /// it), else the level's.
    fn library_name(&self, key: &str, libs: &Libraries) -> String {
        let link = self.links.iter().find(|l| l.key == key);
        let src = link.and_then(|l| libs.get(&l.origin)).and_then(Option::as_ref);
        src.map_or_else(|| self.asset_said(key), |s| s.file.name.clone())
    }

    /// The level's asset `key`'s name.
    fn asset_said(&self, key: &str) -> String {
        self.document().and_then(|d| d.asset_name(key)).unwrap_or_else(|| key.to_string())
    }

    /// One of the notice's answers, on the level tab.
    pub(crate) fn library_action(&mut self, action: LibraryAction) -> Vec<EditorCommand> {
        let Some(libs) = self.libs.clone() else { return Vec::new() };
        let cmds = match action {
            LibraryAction::Update(key) => self.update_one(&key, &libs),
            LibraryAction::UpdateAll => {
                let keys: Vec<String> = self
                    .links
                    .iter()
                    .filter(|l| matches!(&l.status, Status::Changed { breaks } if breaks.is_empty()))
                    .map(|l| l.key.clone())
                    .collect();
                keys.iter().flat_map(|k| self.update_one(k, &libs)).collect()
            }
            LibraryAction::UpdateAnyway(key) => {
                let name = self.library_name(&key, &libs);
                let step = format!("Update {name} anyway");
                let made = self.on_tab(DocTab::Level, |s| s.update_anyway(&key, &libs, step));
                match made {
                    Some((cmds, updated)) => {
                        let said: Vec<String> = updated.resolutions.iter().map(|r| r.to_string()).collect();
                        self.resolutions.extend(said.iter().cloned());
                        self.say(if said.is_empty() {
                            format!("{name} updated")
                        } else {
                            format!("{name} updated: {}", said.join("; "))
                        });
                        cmds
                    }
                    None => self.level_said(),
                }
            }
            LibraryAction::Restore(key) => self.restore_asset(&key),
            LibraryAction::Adopt(key) => {
                let name = self.asset_said(&key);
                let step = format!("Unlink {name}");
                let made = self.on_tab(DocTab::Level, |s| {
                    s.copy_edit_named(Some(step), |d| d.unlink_asset(&key).map(|c| (c, ())))
                });
                match made {
                    Some((cmds, ())) => {
                        self.say(format!("{name} is the level's own now: it follows no library"));
                        cmds
                    }
                    None => self.level_said(),
                }
            }
        };
        self.on_tab(DocTab::Level, |s| {
            s.relink_links();
            s.refresh_problems();
        });
        if matches!(self.dialog, Some(Dialog::LibraryChanged)) && !self.library_changed() {
            self.dialog = None;
        }
        cmds
    }

    /// Update anyway on the level tab: one step, the resolutions added to
    /// the level's notes (they save with it).
    fn update_anyway(
        &mut self,
        key: &str,
        libs: &Libraries,
        step: String,
    ) -> Option<(Vec<EditorCommand>, Updated)> {
        let doc = self.doc.as_mut()?;
        let (change, updated) = match doc.force_update(key, libs) {
            Ok(r) => r,
            Err(e) => {
                self.status.message = Some(e.to_string());
                return None;
            }
        };
        let mut edits = vec![change.edit];
        if !updated.resolutions.is_empty() {
            let before = doc.meta();
            let mut after = before.clone();
            let lines: Vec<String> = updated.resolutions.iter().map(|r| r.to_string()).collect();
            let head = if after.notes.is_empty() { "" } else { "\n" };
            after.notes = format!("{}{head}{}", after.notes, lines.join("\n"));
            edits.push(doc.change(crate::history::Edit::Meta { before, after }).edit);
        }
        edits.push(crate::history::Edit::Name(step));
        self.history.push(edits);
        Some((self.committed(change.dirty), updated))
    }

    /// The level tab's status message on the shown tab's line too (a
    /// refusal there).
    fn level_said(&mut self) -> Vec<EditorCommand> {
        let why = self.on_tab(DocTab::Level, |s| s.status.message.clone());
        if let Some(why) = why {
            self.status.message = Some(why);
        }
        Vec::new()
    }

    /// Update: the library's version of asset `key`, one step.
    fn update_one(&mut self, key: &str, libs: &Libraries) -> Vec<EditorCommand> {
        let name = self.library_name(key, libs);
        let n = self.document().map_or(0, |d| d.copies_of(&[key.to_string()]));
        let step = format!("Update {name}");
        let keys = [key.to_string()];
        let made = self.on_tab(DocTab::Level, |s| {
            s.copy_edit_named(Some(step), |d| d.update_assets(&keys, libs).map(|c| (c, ())))
        });
        match made {
            Some((cmds, ())) => {
                self.say(format!("{} of {name} updated", copies_said(n)));
                cmds
            }
            None => self.level_said(),
        }
    }

    /// Add to your library (call 17): the level's copy of missing asset
    /// `key` goes to the host as a new asset of the user's (its name
    /// numbered when the user has it); its `AssetSaved` links it.
    fn restore_asset(&mut self, key: &str) -> Vec<EditorCommand> {
        let made = self.document().map(|d| d.restorable(key));
        let mut asset = match made {
            Some(Ok(a)) => a,
            Some(Err(why)) => {
                self.status.message = Some(why);
                return Vec::new();
            }
            None => return Vec::new(),
        };
        let yours: Vec<String> =
            self.catalog.assets.iter().filter(|a| !a.key.builtin).map(|a| a.name.clone()).collect();
        if yours.contains(&asset.name) {
            asset.name = olr_edit::element::copy_name(&asset.name, yours.iter().map(String::as_str));
        }
        self.pending_restore = Some(key.to_string());
        self.pending_link = None;
        vec![EditorCommand::SaveAsset { asset: Box::new(asset), replace: None }]
    }

    /// File ▾ Export level…: the browser's File name once the level is
    /// saved (asking first: Save or Cancel).
    pub(crate) fn export_level(&mut self) -> Vec<EditorCommand> {
        if self.shown != DocTab::Level || self.document().is_none() {
            return Vec::new();
        }
        // The asset tab's edits of the ground or an own asset first.
        self.apply_ground();
        self.apply_own();
        if self.dirty() {
            self.dialog = Some(Dialog::Unsaved { then: Pending::Export });
            return Vec::new();
        }
        self.browse_export()
    }

    /// The export browser at the remembered folder, its File name
    /// `<pack id>.olrlevel`.
    pub(crate) fn browse_export(&mut self) -> Vec<EditorCommand> {
        let Some(doc) = self.document() else { return Vec::new() };
        let name = format!("{}.{LEVEL_EXPORT_EXTENSION}", doc.manifest().id);
        let dir = self.browse_start();
        self.browser = Some(Browser::new_save(Purpose::ExportLevel, dir.clone(), name));
        vec![EditorCommand::List(dir)]
    }

    /// Where the browser opens: the folder last picked from, else home.
    pub(crate) fn browse_start(&self) -> PathBuf {
        self.settings
            .browse_dir
            .clone()
            .or_else(|| self.catalog.home.clone())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// The export browser's Choose: `name` (`.olrlevel` added when it has
    /// another extension or none) in its folder; a file of that name there
    /// asks first unless `replace`.
    pub(crate) fn export_to(&mut self, name: &str, replace: bool) -> Vec<EditorCommand> {
        let Some(b) = &mut self.browser else { return Vec::new() };
        if b.purpose() != Purpose::ExportLevel {
            return Vec::new();
        }
        let name = name.trim();
        let ext = format!(".{LEVEL_EXPORT_EXTENSION}");
        let name =
            if name.to_ascii_lowercase().ends_with(&ext) { name.to_string() } else { format!("{name}{ext}") };
        if !plain_name(&name) || name.len() == ext.len() {
            b.say(format!("{name:?} is not a file name"));
            return Vec::new();
        }
        if !replace && b.has(&name) {
            b.ask_replace(name);
            return Vec::new();
        }
        let dir = b.dir().to_path_buf();
        let Some(doc) = self.document() else { return Vec::new() };
        let pack_dir = doc.dir().to_path_buf();
        self.browser = None;
        self.settings.browse_dir = Some(dir.clone());
        vec![
            EditorCommand::ExportLevel { pack_dir, to: dir.join(&name) },
            EditorCommand::SaveSettings(self.settings.clone()),
        ]
    }

    /// The host's `LevelImported`: the imported level opens (asking first
    /// about unsaved changes), the status line naming what came in.
    pub(crate) fn level_imported(&mut self, dir: PathBuf, report: Imported) -> Vec<EditorCommand> {
        let said = import_said(&report);
        self.say(said.clone());
        self.import_note = Some((dir.clone(), said));
        self.leave(Pending::Open(dir))
    }

    /// What `opened` does for issue #84: the last level's notice, scripts
    /// and notes go; the library is asked, its answer opening the notice.
    pub(crate) fn opened_links(&mut self) -> Vec<EditorCommand> {
        self.libs = None;
        self.resolutions.clear();
        self.pending_restore = None;
        self.script_open = None;
        // The import's note, when this is the level it brought.
        if let Some((dir, note)) = self.import_note.take()
            && self.doc.as_ref().is_some_and(|d| d.dir() == dir)
        {
            self.status.message = Some(note);
        }
        let cmds = self.compare_library();
        self.notice_on_compare = !cmds.is_empty();
        cmds
    }
}
