//! Issue #84: the library-change notice on opening a level (Update,
//! Update all, Update anyway with its resolutions, Add to your library,
//! Make it the level's own, Later and File ▾ Library changes…), and a
//! level's export through the browser's File name and its import.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use olr_edit::asset::Asset;
use olr_edit::library::Status;
use olr_editor::browser::Purpose;
use olr_editor::level_tab::CopyChange;
use olr_editor::notice::LibraryAction as L;
use olr_editor::ui::labels;
use olr_editor::{Choice, Dialog, EditorAction as A, EditorCommand as C, EditorState, Pending};
use olr_format::assets::ValueSource as V;
use olr_format::levels::AssetOrigin;
use olr_pack::Severity;

use crate::common::{built_in_assets, content, list_dir, run, set_library};
use crate::copy_panel::{console, harness, level, message, place, set};

/// A saved level holding a Console (its `glow` set to 10) and the strip
/// light with a second state, both from the user's library `yours`.
struct Saved {
    _t: tempfile::TempDir,
    t2: tempfile::TempDir,
    s: EditorState,
    yours: PathBuf,
    dir: PathBuf,
    console: String,
    strip: String,
}

fn strip() -> Asset {
    let mut a = olr_edit::asset::load_dir(&built_in_assets().join("strip-light")).unwrap();
    a.name = "Strip two".into();
    olr_edit::asset::add_state(&mut a, "Off", None).unwrap();
    a
}

fn saved(root: &Path) -> Saved {
    let (t, t2, mut s, yours) = level(root);
    let console = place(&mut s, &yours, &console(), (60, 60));
    set(&mut s, &console, CopyChange::Value("glow".into(), Some(V::Int(10))));
    let strip = place(&mut s, &yours, &strip(), (120, 60));
    run(&mut s, A::Save);
    assert!(!s.dirty(), "{:?}", s.dialog());
    let dir = s.document().unwrap().dir().to_path_buf();
    Saved { _t: t, t2, s, yours, dir, console, strip }
}

impl Saved {
    /// Copy `id`'s library folder.
    fn folder(&self, id: &str) -> String {
        match self.s.document().unwrap().copy_origin(id) {
            Some(AssetOrigin::Yours(f)) => f,
            other => panic!("{id}: {other:?}"),
        }
    }

    /// The library's version of copy `id`'s asset made `asset`.
    fn change(&self, id: &str, asset: &Asset) {
        olr_edit::asset::add_to_library(&self.yours, asset, Some(&self.folder(id))).unwrap();
    }

    /// The level opened again (as File ▾ Open… does), then the library
    /// listed as it now is.
    fn reopen(&mut self) {
        let list = set_library(&built_in_assets(), &self.yours);
        run(&mut self.s, A::Open(self.dir.clone()));
        assert!(self.s.document().is_some_and(|d| d.dir() == self.dir), "{:?}", self.s.status().message);
        assert!(!matches!(self.s.dialog(), Some(Dialog::Unsaved { .. })));
        run(&mut self.s, A::AssetsChanged(list));
    }

    fn key(&self, id: &str) -> String {
        self.s.document().unwrap().placed_copy(id).unwrap().asset
    }

    fn name(&self, id: &str) -> String {
        let d = self.s.document().unwrap();
        d.asset_name(&self.key(id)).unwrap()
    }

    fn status(&self, id: &str) -> Status {
        let key = self.key(id);
        self.s.library_links().iter().find(|l| l.key == key).expect("linked").status.clone()
    }
}

fn renamed(mut a: Asset, name: &str) -> Asset {
    a.name = name.into();
    a
}

/// The console without its `glow`: the level's copy setting it breaks.
fn console_without_glow() -> Asset {
    let mut a = console();
    a.exposed.retain(|x| x.name != "glow");
    a
}

/// The browser shown at `dir` (the host's listing of it).
fn browse(s: &mut EditorState, dir: &Path) {
    run(s, A::Listed { dir: dir.to_path_buf(), entries: Ok(list_dir(dir)) });
    assert_eq!(s.browser().map(|b| b.dir()), Some(dir));
}

fn errors(s: &EditorState) -> Vec<String> {
    s.status().problems.iter().filter(|p| p.severity == Severity::Error).map(|p| p.text.clone()).collect()
}

fn notice(s: &EditorState) -> bool {
    matches!(s.dialog(), Some(Dialog::LibraryChanged))
}

#[test]
fn opening_a_level_with_library_changes_shows_the_notice() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    l.reopen();
    assert!(!notice(&l.s), "nothing changed: no notice");
    assert!(l.s.library_links().iter().all(|k| k.status == Status::Same));
    l.change(&l.console.clone(), &renamed(console(), "Console mark 2"));
    l.reopen();
    assert!(notice(&l.s));
    assert!(matches!(l.status(&l.console), Status::Changed { breaks } if breaks.is_empty()));
    assert_eq!(l.status(&l.strip), Status::Same);
    // On screen: the title and the Console's Update.
    let mut h = harness(l.s);
    h.run();
    h.get_by_label(labels::LIBRARY_CHANGED);
    h.get_by_role_and_label(Role::Button, &labels::update("Console")).click();
    h.run();
    let s = &h.state().state;
    assert!(s.dialog().is_none(), "nothing left to list");
    assert_eq!(s.undo_name().as_deref(), Some("Update Console mark 2"));
    assert!(message(s).contains("1 copy of Console mark 2 updated"), "{}", message(s));
}

#[test]
fn update_all_applies_non_breaking_in_steps() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    l.change(&l.console.clone(), &renamed(console(), "Console mark 2"));
    l.change(&l.strip.clone(), &renamed(strip(), "Strip three"));
    l.reopen();
    assert!(notice(&l.s));
    run(&mut l.s, A::Library(L::UpdateAll));
    assert!(l.s.dialog().is_none());
    assert_eq!((l.name(&l.console), l.name(&l.strip)), ("Console mark 2".into(), "Strip three".into()));
    // One step each.
    run(&mut l.s, A::Undo);
    let names = (l.name(&l.console), l.name(&l.strip));
    assert!(names.0 == "Console" || names.1 == "Strip two", "{names:?}");
    assert!(names.0 != "Console" || names.1 != "Strip two", "{names:?}");
    run(&mut l.s, A::Undo);
    assert_eq!((l.name(&l.console), l.name(&l.strip)), ("Console".into(), "Strip two".into()));
}

#[test]
fn update_anyway_lists_resolutions() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    l.change(&l.console.clone(), &console_without_glow());
    l.reopen();
    assert!(notice(&l.s));
    assert!(matches!(l.status(&l.console), Status::Changed { breaks } if breaks.len() == 1));
    assert!(errors(&l.s).iter().any(|e| e.contains("glow")), "{:?}", errors(&l.s));
    let notes = l.s.document().unwrap().meta().notes;
    // On screen: the break listed under the asset, Update anyway.
    let mut h = harness(l.s);
    h.run();
    h.get_by_role_and_label(Role::Button, &labels::update_anyway("Console")).click();
    h.run();
    l.s =
        std::mem::replace(&mut h.state_mut().state, EditorState::new(Default::default(), Default::default()));
    drop(h);
    let id = l.console.clone();
    let copy = l.s.document().unwrap().placed_copy(&id).unwrap();
    assert!(!copy.values.contains_key("glow"), "resolved");
    assert_eq!(l.s.resolutions().len(), 1, "{:?}", l.s.resolutions());
    let said = &l.s.resolutions()[0];
    assert!(message(&l.s).contains(said.as_str()), "{} / {said}", message(&l.s));
    assert!(l.s.document().unwrap().meta().notes.contains(said.as_str()), "in the level's notes");
    assert!(errors(&l.s).is_empty(), "{:?}", errors(&l.s));
    assert!(l.s.dialog().is_none());
    assert_eq!(l.s.undo_name().as_deref(), Some("Update Console anyway"));
    run(&mut l.s, A::Undo);
    let copy = l.s.document().unwrap().placed_copy(&id).unwrap();
    assert_eq!(copy.values.get("glow"), Some(&V::Int(10)));
    assert_eq!(l.s.document().unwrap().meta().notes, notes, "one step");
}

#[test]
fn restore_and_adopt_from_the_notice() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    let old = l.folder(&l.console);
    for id in [l.console.clone(), l.strip.clone()] {
        std::fs::remove_dir_all(l.yours.join(l.folder(&id))).unwrap();
    }
    // Another asset of the user's took the name.
    olr_edit::asset::add_to_library(&l.yours, &renamed(strip(), "Console"), Some("other")).unwrap();
    l.reopen();
    assert!(notice(&l.s));
    assert_eq!((l.status(&l.console), l.status(&l.strip)), (Status::Missing, Status::Missing));
    let said = format!("{}: Console is not in your library", l.console);
    assert!(errors(&l.s).contains(&said), "{:?}", errors(&l.s));
    // Add to your library: the level's copy saved there, numbered, and
    // linked to it.
    let key = l.key(&l.console);
    run(&mut l.s, A::Library(L::Restore(key)));
    let folder = l.folder(&l.console);
    assert_ne!(folder, old);
    assert!(l.yours.join(&folder).join("asset.ron").is_file());
    assert_eq!(l.name(&l.console), "Console 2");
    assert_eq!(l.status(&l.console), Status::Same);
    assert!(notice(&l.s), "the strip light is still missing");
    // Make it the level's own.
    let key = l.key(&l.strip);
    run(&mut l.s, A::Library(L::Adopt(key)));
    assert_eq!(l.s.document().unwrap().copy_origin(&l.strip), Some(AssetOrigin::Own));
    assert!(l.s.dialog().is_none() && errors(&l.s).is_empty(), "{:?}", errors(&l.s));
    // Each one step.
    run(&mut l.s, A::Undo);
    assert_eq!(l.status(&l.strip), Status::Missing);
    run(&mut l.s, A::Undo);
    assert_eq!(l.folder(&l.console), old);
    assert_eq!(l.status(&l.console), Status::Missing);
}

#[test]
fn later_keeps_the_errors_and_file_reopens_the_notice() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    l.change(&l.console.clone(), &console_without_glow());
    l.reopen();
    assert!(notice(&l.s));
    let kept = errors(&l.s);
    assert!(!kept.is_empty());
    let mut h = harness(l.s);
    h.run();
    h.get_by_role_and_label(Role::Button, labels::LATER).click();
    h.run();
    l.s =
        std::mem::replace(&mut h.state_mut().state, EditorState::new(Default::default(), Default::default()));
    drop(h);
    assert!(l.s.dialog().is_none());
    assert_eq!(errors(&l.s), kept);
    // A later library write does not bring it back.
    olr_edit::asset::add_to_library(&l.yours, &renamed(console(), "Another"), None).unwrap();
    run(&mut l.s, A::AssetsChanged(set_library(&built_in_assets(), &l.yours)));
    assert!(l.s.dialog().is_none(), "{:?}", l.s.dialog());
    assert_eq!(errors(&l.s), kept);
    // File ▾ Library changes… does.
    run(&mut l.s, A::LibraryChanges);
    assert!(notice(&l.s));
}

#[test]
fn export_asks_to_save_then_names_the_file() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    let out = tempfile::tempdir().unwrap();
    let s = &mut l.s;
    set(s, &l.strip.clone(), CopyChange::At((130, 60)));
    run(s, A::ExportLevel);
    assert!(matches!(s.dialog(), Some(Dialog::Unsaved { then: Pending::Export })));
    // No Don't save: Cancel exports nothing.
    run(s, A::Choose(Choice::Discard));
    assert!(s.browser().is_none());
    run(s, A::Choose(Choice::Cancel));
    assert!(s.dialog().is_none() && s.browser().is_none() && s.dirty());
    run(s, A::ExportLevel);
    run(s, A::Choose(Choice::Save));
    assert!(!s.dirty());
    let id = s.document().unwrap().manifest().id.clone();
    let b = s.browser().expect("the export browser");
    assert_eq!(b.purpose(), Purpose::ExportLevel);
    assert_eq!(b.file_name(), Some(format!("{id}.olrlevel").as_str()));
    browse(s, out.path());
    let cmds = run(s, A::ExportTo { name: "castle".into(), replace: false });
    let to = out.path().join("castle.olrlevel");
    assert!(cmds.contains(&C::ExportLevel { pack_dir: l.dir.clone(), to: to.clone() }), "{cmds:?}");
    assert!(to.is_file(), "{}", message(s));
    assert!(s.browser().is_none());
    // Again: the file there asks first.
    run(s, A::ExportLevel);
    browse(s, out.path());
    run(s, A::ExportTo { name: "castle".into(), replace: false });
    assert_eq!(s.browser().and_then(|b| b.replacing()), Some("castle.olrlevel"));
    run(s, A::CancelReplace);
    assert_eq!(s.browser().and_then(|b| b.replacing()), None);
    // On screen: the File name field, Export, the question, Replace.
    let mut h = harness(l.s);
    h.run();
    h.get_by_role_and_label(Role::TextInput, labels::FILE_NAME);
    h.get_by_role_and_label(Role::Button, labels::EXPORT);
    h.state_mut().state.update(A::ExportTo { name: "castle.olrlevel".into(), replace: false });
    h.run();
    h.get_by_label("Replace castle.olrlevel?");
    std::fs::remove_file(&to).unwrap();
    h.get_by_role_and_label(Role::Button, labels::REPLACE).click();
    h.run();
    assert!(to.is_file());
    assert!(h.state().state.browser().is_none());
}

#[test]
fn import_opens_the_level_and_reports_assets() {
    let Some(root) = content() else { return };
    let mut l = saved(&root);
    let out = tempfile::tempdir().unwrap();
    let file = out.path().join("castle.olrlevel");
    run(&mut l.s, A::ExportLevel);
    browse(&mut l.s, out.path());
    run(&mut l.s, A::ExportTo { name: "castle".into(), replace: false });
    assert!(file.is_file(), "{}", message(&l.s));
    // Another profile: an empty library of its own, its own packs.
    let other = tempfile::tempdir().unwrap();
    let yours = other.path().join("yours");
    std::fs::create_dir_all(&yours).unwrap();
    run(&mut l.s, A::AssetsChanged(set_library(&built_in_assets(), &yours)));
    run(&mut l.s, A::Browse(Purpose::ImportLevel));
    browse(&mut l.s, out.path());
    let cmds = run(&mut l.s, A::Pick("castle.olrlevel".into()));
    assert!(cmds.contains(&C::ImportLevel(file.clone())), "{cmds:?}");
    let doc = l.s.document().expect("the imported level open");
    assert!(doc.dir().starts_with(other.path().join("packs")), "{}", doc.dir().display());
    let said = message(&l.s);
    assert!(said.contains("added Console") && said.contains("added Strip two"), "{said}");
    assert!(said.contains("reused built-in"), "{said}");
    assert!(l.s.dialog().is_none(), "the library has them: {:?}", l.s.dialog());
    // The copies link to the user's new assets.
    let console = l.s.document().unwrap().copy_origin(&l.console);
    assert!(matches!(console, Some(AssetOrigin::Yours(_))), "{console:?}");
    drop(l.t2);
}
