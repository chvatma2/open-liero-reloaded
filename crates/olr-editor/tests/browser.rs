//! The file browser (plan 14c Task 4, spec §4 "Files and safety"), recovered
//! documents and the recovery's end: state-level, and by label with a test
//! host that lists a temporary tree.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_editor::browser::{DirEntry, Purpose};
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{Dialog, EditorAction as A, EditorCommand as C, EditorState, Pending};

use crate::common::{content, editor, run};

/// `b/`, `A/` (a pack: it holds `pack.ron`), `x.PNG`, `y.txt`, `.hidden`.
fn tree() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    std::fs::create_dir(d.join("b")).unwrap();
    std::fs::create_dir(d.join("A")).unwrap();
    std::fs::write(d.join("A/pack.ron"), "()").unwrap();
    std::fs::write(d.join("x.PNG"), b"png").unwrap();
    std::fs::write(d.join("y.txt"), b"text").unwrap();
    std::fs::write(d.join(".hidden"), b"").unwrap();
    tmp
}

/// What the game's host answers to `List` (its own lister, in olr-game).
fn list(dir: &Path) -> Result<Vec<DirEntry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    Ok(rd
        .filter_map(Result::ok)
        .map(|e| {
            let is_dir = e.path().is_dir();
            DirEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                is_dir,
                is_pack: is_dir && e.path().join("pack.ron").is_file(),
            }
        })
        .collect())
}

/// Feeds every `List` back as `Listed` (as the host does); the rest is the
/// common test host's.
fn host(state: &mut EditorState, action: A) -> Vec<C> {
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([action]);
    while let Some(a) = queue.pop_front() {
        for c in run(state, a) {
            if let C::List(dir) = &c {
                queue.push_back(A::Listed { dir: dir.clone(), entries: list(dir) });
            }
            out.push(c);
        }
    }
    out
}

fn names(state: &EditorState) -> Vec<String> {
    state.browser().unwrap().shown().iter().map(|e| e.name.clone()).collect()
}

fn browsing(purpose: Purpose) -> Option<(tempfile::TempDir, tempfile::TempDir, EditorState)> {
    let root = content()?;
    let (doc, mut state) = editor(&root);
    let t = tree();
    host(&mut state, A::Browse(purpose));
    host(&mut state, A::BrowseTo(t.path().to_path_buf()));
    assert_eq!(state.browser().unwrap().dir(), t.path());
    Some((doc, t, state))
}

#[test]
fn browsing_lists_filters_and_sorts() {
    let Some((_doc, _t, mut state)) = browsing(Purpose::ImportImage) else { return };
    assert_eq!(state.browser().unwrap().purpose(), Purpose::ImportImage);
    assert_eq!(names(&state), ["A", "b", "x.PNG"], "folders first, case aside; images only; no hidden");
    host(&mut state, A::CloseBrowser);
    assert!(state.browser().is_none());
    let Some((_doc, _t, state)) = browsing(Purpose::OpenPack) else { return };
    assert_eq!(names(&state), ["A", "b"], "folders only");
    let shown = state.browser().unwrap().shown();
    assert!(shown[0].is_pack && !shown[1].is_pack);
}

#[test]
fn browsing_starts_at_the_remembered_folder() {
    let Some(root) = content() else { return };
    let (_doc, mut state) = editor(&root);
    let t = tree();
    let mut settings = state.settings().clone();
    settings.browse_dir = Some(t.path().to_path_buf());
    let mut state2 = EditorState::new(settings, state.catalog().clone());
    let cmds = state2.update(A::Browse(Purpose::ImportImage));
    assert_eq!(cmds, vec![C::List(t.path().to_path_buf())]);
    // Without one: the home directory (or the current one).
    let cmds = state.update(A::Browse(Purpose::ImportImage));
    assert!(matches!(&cmds[..], [C::List(_)]), "{cmds:?}");
}

#[test]
fn picking_an_image_reads_it_and_remembers_the_folder() {
    let Some((_doc, t, mut state)) = browsing(Purpose::ImportImage) else { return };
    let cmds = state.update(A::Pick("x.PNG".into()));
    let dir = t.path().to_path_buf();
    assert!(cmds.contains(&C::ReadImage(dir.join("x.PNG"))), "{cmds:?}");
    let saved = cmds.iter().find_map(|c| match c {
        C::SaveSettings(s) => Some(s.browse_dir.clone()),
        _ => None,
    });
    assert_eq!(saved, Some(Some(dir)));
    assert!(state.browser().is_none(), "picking closes the browser");
    // The host's answer: an import, or why not.
    state.update(A::ImportFailed("not a PNG or JPEG".into()));
    assert_eq!(state.dialog(), Some(&Dialog::Error("not a PNG or JPEG".into())));
}

#[test]
fn picking_a_folder_enters_it() {
    let Some((_doc, t, mut state)) = browsing(Purpose::ImportImage) else { return };
    let cmds = state.update(A::Pick("b".into()));
    assert_eq!(cmds, vec![C::List(t.path().join("b"))]);
    assert!(state.update(A::Pick("nothing-here".into())).is_empty(), "an unknown name does nothing");
}

#[test]
fn a_listing_error_keeps_the_old_listing() {
    let Some((_doc, t, mut state)) = browsing(Purpose::ImportImage) else { return };
    state.update(A::Listed { dir: t.path().join("gone"), entries: Err("denied".into()) });
    let b = state.browser().unwrap();
    assert_eq!(b.dir(), t.path());
    assert_eq!(names(&state), ["A", "b", "x.PNG"]);
    assert_eq!(state.browser().unwrap().message(), Some("denied"));
}

#[test]
fn opening_a_pack_through_the_browser_asks_when_unsaved() {
    let Some((_doc, t, mut state)) = browsing(Purpose::OpenPack) else { return };
    host(&mut state, A::CloseBrowser);
    host(&mut state, A::AddLayer);
    assert!(state.dirty());
    host(&mut state, A::Browse(Purpose::OpenPack));
    host(&mut state, A::BrowseTo(t.path().to_path_buf()));
    let cmds = state.update(A::Pick("A".into()));
    assert!(!cmds.iter().any(|c| matches!(c, C::Open(_))), "{cmds:?}");
    assert!(state.browser().is_none());
    assert_eq!(state.dialog(), Some(&Dialog::Unsaved { then: Pending::Open(t.path().join("A")) }));
}

#[test]
fn recovered_documents_are_unsaved() {
    let Some(root) = content() else { return };
    let (_tmp, doc) = crate::common::open_fixture(&root);
    let all = doc.all();
    let mut state = EditorState::new(Default::default(), crate::common::catalog(&root));
    let cmds = state.update(A::Recovered(Box::new(doc)));
    assert!(state.dirty(), "a recovery holds unsaved changes");
    assert_eq!(cmds, vec![C::Upload(all)]);
}

#[test]
fn saving_or_closing_drops_the_recovery() {
    let Some(root) = content() else { return };
    let (_tmp, mut state) = editor(&root);
    host(&mut state, A::AddLayer);
    let cmds = host(&mut state, A::Save);
    assert!(cmds.contains(&C::RecoveryDone), "{cmds:?}");
    assert!(!state.dirty());
    host(&mut state, A::AddLayer);
    let cmds = state.update(A::Close);
    assert!(cmds.is_empty(), "the dialog asks first");
    let cmds = state.update(A::Choose(olr_editor::Choice::Discard));
    assert_eq!(cmds, vec![C::RecoveryDone, C::Close]);
}

/// The editor in a harness whose host lists directories.
struct App {
    state: EditorState,
    data: EditorData,
    commands: Vec<C>,
}

fn harness(state: EditorState) -> Harness<'static, App> {
    let app = App { state, data: EditorData::default(), commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                let c = host(&mut app.state, a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

#[test]
fn the_browser_by_label() {
    let Some(root) = content() else { return };
    let (_doc, state) = editor(&root);
    let t = tree();
    let mut h = harness(state);
    h.run();
    h.get_by_label(labels::CANVAS);
    h.get_by_label(labels::IMPORT_IMAGE).click();
    h.run();
    assert!(h.state().state.browser().is_some());
    h.get_by_label(labels::BROWSER_PATH).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(labels::BROWSER_PATH).type_text(t.path().to_str().unwrap());
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(h.state().commands.contains(&C::List(PathBuf::from(t.path()))), "{:?}", h.state().commands);
    assert_eq!(h.state().state.browser().unwrap().dir(), t.path());
    // A click on a folder selects it (a double click's second click lands
    // on the new listing otherwise: batch B review); Choose enters it.
    h.get_by_label("b").click();
    h.run();
    assert_eq!(h.state().state.browser().unwrap().dir(), t.path(), "a click only selects");
    h.get_by_label(labels::CHOOSE).click();
    h.run();
    assert_eq!(h.state().state.browser().unwrap().dir(), t.path().join("b"));
    h.get_by_label("Up").click();
    h.run();
    assert_eq!(h.state().state.browser().unwrap().dir(), t.path());
    h.get_by_label("x.PNG").click();
    h.run();
    h.get_by_label(labels::CHOOSE).click();
    h.run();
    assert!(h.state().commands.contains(&C::ReadImage(t.path().join("x.PNG"))), "{:?}", h.state().commands);
    assert!(h.state().state.browser().is_none());
    // Open… and Close are in the toolbar.
    h.get_by_label(labels::OPEN).click();
    h.run();
    assert_eq!(h.state().state.browser().unwrap().purpose(), Purpose::OpenPack);
    h.get_by_label(labels::CANCEL).click();
    h.run();
    assert!(h.state().state.browser().is_none());
    h.get_by_label(labels::CLOSE).click();
    h.run();
    assert!(h.state().commands.contains(&C::Close), "{:?}", h.state().commands);
}

/// `..` and `.` in a typed path are resolved by name, so Up after them goes
/// up (deferred minor of batch B).
#[test]
fn typed_dots_are_resolved() {
    let Some((_doc, t, mut state)) = browsing(Purpose::ImportImage) else { return };
    host(&mut state, A::BrowseTo("b".into()));
    assert_eq!(state.browser().unwrap().dir(), t.path().join("b"));
    assert_eq!(state.update(A::BrowseTo("..".into())), vec![C::List(t.path().to_path_buf())]);
    assert_eq!(state.update(A::BrowseTo("./../b/../A".into())), vec![C::List(t.path().join("A"))]);
    let root = t.path().ancestors().last().unwrap().to_path_buf();
    assert_eq!(
        state.update(A::BrowseTo(root.join("..").join(".."))),
        vec![C::List(root)],
        "never above the root"
    );
}

/// The browser's first folder without a remembered one is the host's
/// (`Catalog::home`), not read from the environment by the state.
#[test]
fn the_host_names_the_first_folder() {
    let Some(root) = content() else { return };
    let mut catalog = crate::common::catalog(&root);
    catalog.home = Some(PathBuf::from("/somewhere/home"));
    let mut state = EditorState::new(Default::default(), catalog);
    assert_eq!(state.update(A::Browse(Purpose::ImportImage)), vec![C::List("/somewhere/home".into())]);
    let mut state = EditorState::new(Default::default(), crate::common::catalog(&root));
    assert_eq!(state.update(A::Browse(Purpose::ImportImage)), vec![C::List(".".into())]);
}

/// A listing the host cut at `MAX_ENTRIES` says so.
#[test]
fn a_cut_listing_says_so() {
    let Some((_doc, t, mut state)) = browsing(Purpose::OpenPack) else { return };
    let entries = |n: usize| -> Vec<DirEntry> {
        (0..n).map(|i| DirEntry { name: format!("d{i:05}"), is_dir: true, is_pack: false }).collect()
    };
    let max = olr_editor::browser::MAX_ENTRIES;
    state.update(A::Listed { dir: t.path().to_path_buf(), entries: Ok(entries(max)) });
    assert_eq!(state.browser().unwrap().message(), None, "exactly MAX_ENTRIES: nothing cut");
    // The host lists one more to say it cut the rest.
    state.update(A::Listed { dir: t.path().to_path_buf(), entries: Ok(entries(max + 1)) });
    let msg = state.browser().unwrap().message().unwrap_or_default().to_string();
    assert!(msg.contains("first 10000"), "{msg}");
    assert_eq!(state.browser().unwrap().shown().len(), max);
    let mut few = vec![DirEntry { name: "b".into(), is_dir: true, is_pack: false }];
    olr_editor::browser::sort_entries(&mut few);
    state.update(A::Listed { dir: t.path().to_path_buf(), entries: Ok(few) });
    assert_eq!(state.browser().unwrap().message(), None);
}
