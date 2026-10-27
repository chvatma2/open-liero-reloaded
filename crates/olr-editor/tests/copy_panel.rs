//! Issue #84: the copy panel's Id, Live (hidden, disabled with its
//! reason, or a switch, as the design's call 2 says), Distant and Live
//! excluding each other, the starting state and the values (the canvas
//! posing the copy), Default, the copies' and the level's scripts (one
//! step each, their syntax errors as warnings), and a level whose copies
//! the library breaks saving with its errors.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_edit::asset::Asset;
use olr_editor::copy_ops::{CopyFacts, DISTANT_LIVE, LIVE_DIRT, LIVE_DISTANT, LiveOffer, ScriptTarget};
use olr_editor::level_tab::CopyChange;
use olr_editor::ui::{self, EditorData, labels};
use olr_editor::{AssetKey, Choice, Dialog, EditorAction as A, EditorState, Selection};
use olr_format::assets::{ExposedSource, PropertySource as P, ValueSource as V};
use olr_format::levels::{ElementKind, ElementMaterial};
use olr_pack::Severity;

use crate::common::{built_in_assets, content, run, set_library};
use crate::level_tab::placed;
use crate::states::alarm_lamp;

pub struct App {
    pub state: EditorState,
    pub data: EditorData,
}

/// The editor on `state`, its actions carried out by the test host.
pub fn harness(state: EditorState) -> Harness<'static, App> {
    let app = App { state, data: EditorData::default() };
    Harness::builder().with_size(egui::vec2(1280.0, 900.0)).with_max_steps(16).build_ui_state(
        |ui, app: &mut App| {
            for a in ui::show(ui, &app.state, &mut app.data) {
                run(&mut app.state, a);
            }
        },
        app,
    )
}

/// The alarm lamp (states Alarm and Off) named Console, its lamps' glow
/// strength exposed as `glow` and its rock's visibility as `shown`.
pub fn console() -> Asset {
    let mut a = alarm_lamp();
    a.name = "Console".into();
    for (name, property) in
        [("glow", P::GlowStrength("lamps".into())), ("shown", P::LayerVisible("rock".into()))]
    {
        a.exposed.push(ExposedSource { name: name.into(), property, default: None, min: None, max: None });
    }
    a
}

/// `level_tab::placed` (the built-in library, the user's at `yours`),
/// nothing open over it.
pub fn level(root: &Path) -> (tempfile::TempDir, tempfile::TempDir, EditorState, PathBuf) {
    let (t, t2, s) = placed(root);
    let yours = t2.path().join("yours");
    std::fs::create_dir_all(&yours).unwrap();
    assert!(s.dialog().is_none(), "{:?}", s.dialog());
    (t, t2, s, yours)
}

/// `asset` saved in the user's library `yours` and a copy of it placed
/// at `at`; the copy's id.
pub fn place(s: &mut EditorState, yours: &Path, asset: &Asset, at: (i32, i32)) -> String {
    let id = olr_edit::asset::add_to_library(yours, asset, None).unwrap();
    run(s, A::AssetsChanged(set_library(&built_in_assets(), yours)));
    run(s, A::PlaceAsset { key: AssetKey { builtin: false, id }, at });
    match s.selection() {
        Selection::Copy(id) => id.clone(),
        other => panic!("placed and selected, not {other:?}: {:?}", s.status().message),
    }
}

pub fn facts(s: &EditorState, id: &str) -> CopyFacts {
    s.document().unwrap().copy_facts(id).expect("a copy's facts")
}

pub fn set(s: &mut EditorState, id: &str, change: CopyChange) {
    run(s, A::SetCopy { id: id.into(), change });
}

pub fn message(s: &EditorState) -> String {
    s.status().message.clone().unwrap_or_default()
}

fn copy(s: &EditorState, id: &str) -> olr_format::levels::PlacedCopy {
    s.document().unwrap().placed_copy(id).expect(id)
}

/// The level's problems of `severity`.
fn problems(s: &EditorState, severity: Severity) -> Vec<String> {
    s.status().problems.iter().filter(|p| p.severity == severity).map(|p| p.text.clone()).collect()
}

#[test]
fn renaming_a_copy_refuses_taken_and_keeps_its_script() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    let id = place(&mut s, &yours, &console(), (60, 60));
    let target = ScriptTarget::Copy(id.clone());
    run(&mut s, A::OpenScript(target.clone()));
    run(&mut s, A::SetTargetScript { target, text: "let speed = 2;\n".into() });
    let step = s.undo_name();
    for (bad, says) in [("torch", "the id is taken"), ("Big crane", "not an id")] {
        set(&mut s, &id, CopyChange::Id(bad.into()));
        assert!(message(&s).contains(says), "{bad}: {}", message(&s));
        assert_eq!(copy(&s, &id).id, id);
        assert_eq!(s.undo_name(), step, "no step");
    }
    set(&mut s, &id, CopyChange::Id("crane".into()));
    let doc = s.document().unwrap();
    assert!(doc.placed_copy(&id).is_none());
    assert_eq!(
        doc.script_of(&ScriptTarget::Copy("crane".into())),
        Some("let speed = 2;\n"),
        "the script kept"
    );
    assert_eq!(s.selection(), &Selection::Copy("crane".into()));
    assert_eq!(s.script_open(), Some(&ScriptTarget::Copy("crane".into())), "its editor follows");
    // One step.
    run(&mut s, A::Undo);
    assert_eq!(copy(&s, &id).id, id);
    assert_eq!(s.document().unwrap().script_of(&ScriptTarget::Copy(id.clone())), Some("let speed = 2;\n"));
    // The panel's Id field and its hint.
    run(&mut s, A::CloseScript);
    run(&mut s, A::Select(Selection::Copy(id.clone())));
    let h = harness(s);
    h.get_by_role_and_label(Role::TextInput, labels::COPY_ID);
    h.get_by_label(labels::ID_HINT);
}

/// The console with its rock made dirt (nothing of it exposed).
fn dirt_console() -> Asset {
    let mut a = console();
    a.name = "Dirt console".into();
    a.exposed.retain(|x| x.name != "shown");
    let rock = a.pieces.iter_mut().find(|p| p.element.id == "rock").unwrap();
    if let ElementKind::Drawn { material, .. } = &mut rock.element.kind {
        *material = ElementMaterial::Dirt;
    }
    a
}

#[test]
fn live_is_hidden_disabled_or_offered_as_call_2_says() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    // The built-in torch drives nothing: no Live.
    assert_eq!(facts(&s, "torch").live, LiveOffer::Hidden);
    let dirt = place(&mut s, &yours, &dirt_console(), (60, 60));
    assert_eq!(facts(&s, &dirt).live, LiveOffer::Disabled(LIVE_DIRT.into()));
    let id = place(&mut s, &yours, &console(), (120, 60));
    assert_eq!(facts(&s, &id).live, LiveOffer::Offered);
    set(&mut s, &id, CopyChange::Live(true));
    assert!(copy(&s, &id).live, "{}", message(&s));
    // Issue #85: Save and Play carry its played form, out of the ground.
    let doc = s.document().unwrap();
    let olr_edit::pack::Source::Level(saved) = doc.to_pack().source else { panic!("a level") };
    let played = saved.live.as_ref().expect("the played form");
    assert_eq!(played.file.copies.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [id.as_str()]);
    assert_eq!(doc.playable().live, saved.live);
    // A dirt copy cannot be made live.
    set(&mut s, &dirt, CopyChange::Live(true));
    assert!(!copy(&s, &dirt).live);

    // On screen: none, disabled with the reason under it, a switch saying
    // it is left out of play for now (issue #85).
    run(&mut s, A::Select(Selection::Copy("torch".into())));
    let mut h = harness(s);
    h.run();
    assert!(h.query_by_role_and_label(Role::CheckBox, labels::LIVE).is_none(), "hidden");
    run(&mut h.state_mut().state, A::Select(Selection::Copy(dirt.clone())));
    h.run();
    let live = h.get_by_role_and_label(Role::CheckBox, labels::LIVE);
    assert!(live.accesskit_node().is_disabled(), "disabled");
    h.get_by_label(LIVE_DIRT);
    run(&mut h.state_mut().state, A::Select(Selection::Copy(id.clone())));
    h.run();
    let live = h.get_by_role_and_label(Role::CheckBox, labels::LIVE);
    assert!(!live.accesskit_node().is_disabled());
    h.get_by_label(labels::LIVE_NOT_YET);
    // The switch turns it off, one step.
    live.click();
    h.run();
    assert!(!copy(&h.state().state, &id).live);
}

/// The strip light (Background layers only) with a second state: it can
/// be Live or Distant.
fn strip_with_states() -> Asset {
    let mut a = olr_edit::asset::load_dir(&built_in_assets().join("strip-light")).unwrap();
    a.name = "Strip two".into();
    olr_edit::asset::add_state(&mut a, "Off", None).unwrap();
    a
}

#[test]
fn distant_and_live_exclude_each_other_with_reasons() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    let id = place(&mut s, &yours, &strip_with_states(), (60, 60));
    let f = facts(&s, &id);
    assert_eq!((f.live, f.distant), (LiveOffer::Offered, Some(None)));
    // The console holds rock: never distant.
    let console = place(&mut s, &yours, &console(), (120, 60));
    assert_eq!(facts(&s, &console).distant, None);

    set(&mut s, &id, CopyChange::Live(true));
    assert_eq!(facts(&s, &id).distant, Some(Some(DISTANT_LIVE.into())));
    set(&mut s, &id, CopyChange::Distant(Some(50)));
    assert_eq!(copy(&s, &id).distant, None, "refused: {}", message(&s));
    set(&mut s, &id, CopyChange::Live(false));
    set(&mut s, &id, CopyChange::Distant(Some(50)));
    assert_eq!(copy(&s, &id).distant, Some(50), "{}", message(&s));
    assert_eq!(facts(&s, &id).live, LiveOffer::Disabled(LIVE_DISTANT.into()));
    set(&mut s, &id, CopyChange::Live(true));
    assert!(!copy(&s, &id).live, "refused");

    // On screen: Live disabled with its reason, Distant on with its Depth.
    run(&mut s, A::Select(Selection::Copy(id.clone())));
    let mut h = harness(s);
    h.run();
    assert!(h.get_by_role_and_label(Role::CheckBox, labels::LIVE).accesskit_node().is_disabled());
    h.get_by_label(LIVE_DISTANT);
    h.get_by_role_and_label(Role::CheckBox, labels::DISTANT).click();
    h.run();
    assert_eq!(copy(&h.state().state, &id).distant, None, "Distant off");
    assert!(h.query_by_label(LIVE_DISTANT).is_none());
}

#[test]
fn the_starting_state_picker_and_values_set_the_copy_and_the_canvas_follows() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    let id = place(&mut s, &yours, &console(), (60, 60));
    let f = facts(&s, &id);
    assert_eq!(f.states.len(), 3, "{:?}", f.states);
    let start = f.start.clone();
    assert_eq!(f.values.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["glow", "shown"]);
    assert!(f.values.iter().all(|v| !v.set));
    let front = |s: &EditorState| s.document().unwrap().level().front.clone();
    let before = front(&s);

    // The picker: the asset's start first, then each state.
    run(&mut s, A::Select(Selection::Copy(id.clone())));
    let mut h = harness(s);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, labels::STARTING_STATE).click();
    h.run();
    h.get_by_label(&labels::asset_start(&start));
    h.get_by_label("Alarm").click();
    h.run();
    let s = &mut h.state_mut().state;
    assert_eq!(copy(s, &id).state.as_deref(), Some("Alarm"));
    let alarm = front(s);
    assert!(alarm != before, "the canvas shows the copy in Alarm");

    // A value, set: the row marked; the canvas follows.
    set(s, &id, CopyChange::Value("shown".into(), Some(V::Bool(false))));
    let f = facts(s, &id);
    let shown = f.values.iter().find(|v| v.name == "shown").unwrap();
    assert!(shown.set && shown.value == Some(V::Bool(false)), "{shown:?}");
    assert!(front(s) != alarm, "the rock hidden");
    // The asset's start again.
    set(s, &id, CopyChange::State(None));
    assert_eq!(copy(s, &id).state, None);
    run(s, A::Undo);
    run(s, A::Undo);
    assert_eq!(front(s), alarm, "each change was a step");
}

#[test]
fn default_clears_a_value() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    let id = place(&mut s, &yours, &console(), (60, 60));
    set(&mut s, &id, CopyChange::Value("glow".into(), Some(V::Int(10))));
    assert_eq!(copy(&s, &id).values.get("glow"), Some(&V::Int(10)), "{}", message(&s));
    run(&mut s, A::Select(Selection::Copy(id.clone())));
    let mut h = harness(s);
    h.run();
    assert!(h.query_by_label(&labels::default_value("shown")).is_none(), "an unset row has no Default");
    h.get_by_label(&labels::default_value("glow")).click();
    h.run();
    let s = &mut h.state_mut().state;
    assert!(copy(s, &id).values.is_empty());
    assert!(facts(s, &id).values.iter().all(|v| !v.set));
    run(s, A::Undo);
    assert_eq!(copy(s, &id).values.get("glow"), Some(&V::Int(10)), "one step");
}

#[test]
fn copy_and_level_scripts_open_commit_and_list_syntax_warnings() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    let mut asset = console();
    asset.script = Some("fn alarm() {\n  let = 1;\n}\n".into());
    let id = place(&mut s, &yours, &asset, (60, 60));
    let warnings = problems(&s, Severity::Warning);
    assert!(warnings.iter().any(|w| w.starts_with("Console's default script, line 2:")), "{warnings:?}");
    let mine = ScriptTarget::Copy(id.clone());
    run(&mut s, A::OpenScript(mine.clone()));
    assert_eq!(s.script_open(), Some(&mine));
    assert!(s.document().unwrap().script_of(&mine).is_some(), "a new script");
    // Made in one step: undone, the editor closes with it.
    run(&mut s, A::Undo);
    assert_eq!((s.document().unwrap().script_of(&mine), s.script_open()), (None, None));
    run(&mut s, A::OpenScript(mine.clone()));
    run(&mut s, A::SetTargetScript { target: mine.clone(), text: "let x = ;\n".into() });
    let said = format!("{id}'s script, line 1:");
    assert!(
        problems(&s, Severity::Warning).iter().any(|w| w.starts_with(&said)),
        "{:?}",
        s.status().problems
    );
    run(&mut s, A::SetTargetScript { target: mine.clone(), text: "let x = 1;\n".into() });
    assert!(!problems(&s, Severity::Warning).iter().any(|w| w.starts_with(&said)));

    // The level script, from the level's panel.
    run(&mut s, A::Select(Selection::None));
    let mut h = harness(s);
    h.run();
    h.get_by_label(&format!("{} {}", labels::LEVEL_SCRIPT, labels::NEW_SCRIPT)).click();
    h.run();
    assert_eq!(h.state().state.script_open(), Some(&ScriptTarget::Level));
    h.get_by_label(olr_format::levels::LEVEL_SCRIPT_FILE);
    let editor = h.get_by_role_and_label(Role::TextInput, labels::SCRIPT_EDITOR);
    editor.focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, labels::SCRIPT_EDITOR).type_text("fn broken( {\n");
    h.run();
    // Back to level commits what was typed.
    h.get_by_label(labels::BACK_TO_LEVEL).click();
    h.run();
    h.run();
    let s = &mut h.state_mut().state;
    assert_eq!(s.script_open(), None);
    let level = s.document().unwrap().script_of(&ScriptTarget::Level).unwrap().to_string();
    assert!(level.contains("fn broken( {"), "{level:?}");
    assert!(
        problems(s, Severity::Warning).iter().any(|w| w.starts_with("the level script, line")),
        "{:?}",
        s.status().problems
    );
    // Remove script asks first.
    run(s, A::RemoveTargetScript(ScriptTarget::Level));
    assert!(matches!(s.dialog(), Some(Dialog::RemoveTargetScript(ScriptTarget::Level))));
    run(s, A::Choose(Choice::Confirm));
    assert!(s.document().unwrap().script_of(&ScriptTarget::Level).is_none());
    assert!(!problems(s, Severity::Warning).iter().any(|w| w.starts_with("the level script")));
    // Saved, the scripts are the pack's files.
    run(s, A::Save);
    let dir = s.document().unwrap().dir().to_path_buf();
    let read = olr_format::levels::read_placed_files(&olr_vfs::DirFs::new(&dir)).unwrap().unwrap();
    assert_eq!(read.scripts.get(&id).map(String::as_str), Some("let x = 1;\n"));
    assert_eq!(read.level_script, None);
}

#[test]
fn a_level_with_breaks_saves_and_keeps_the_errors() {
    let Some(root) = content() else { return };
    let (_t, _t2, mut s, yours) = level(&root);
    let id = place(&mut s, &yours, &console(), (60, 60));
    set(&mut s, &id, CopyChange::Value("glow".into(), Some(V::Int(10))));
    run(&mut s, A::Save);
    assert!(!s.dirty());
    // The library's Console no longer exposes glow.
    let folder = match s.document().unwrap().copy_origin(&id) {
        Some(olr_format::levels::AssetOrigin::Yours(f)) => f,
        other => panic!("{other:?}"),
    };
    let mut v2 = console();
    v2.exposed.retain(|x| x.name != "glow");
    olr_edit::asset::add_to_library(&yours, &v2, Some(&folder)).unwrap();
    run(&mut s, A::AssetsChanged(set_library(&built_in_assets(), &yours)));
    let errors = problems(&s, Severity::Error);
    assert!(errors.iter().any(|e| e.contains("glow") && e.contains("Console")), "{errors:?}");
    assert!(copy(&s, &id).values.contains_key("glow"), "not applied");
    // A change and a save: saved, the errors kept.
    set(&mut s, &id, CopyChange::At((70, 60)));
    run(&mut s, A::Save);
    assert!(!s.dirty(), "{:?}", s.dialog());
    assert!(s.dialog().is_none());
    assert_eq!(problems(&s, Severity::Error), errors);
}
