//! The skin editor in the game (issue #14), through the shell without a
//! window: a copy of a built-in skin painted and saved into the data
//! directory's `packs/` (the built-in pack untouched), offered by the
//! profiles' skin picker at once and then edited in place; a classic skin
//! made double resolution; a blank skin; a bad skin pack refused with a
//! message; the window's close asking first. Over the bundled content
//! (`OLR_CONTENT_DIR`; skipped without it).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use olr_data::image::Rgba;
use olr_game::editor::CloseDecision;
use olr_game::shell::{Shell, ShellConfig};
use olr_skin_editor::{Dialog, SkinAction as A, Target};
use olr_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// A shell on a fresh data directory, on the Editor screen.
fn shell() -> Option<(tempfile::TempDir, PathBuf, Shell)> {
    let root = content()?;
    let tmp = tempfile::tempdir().unwrap();
    let cfg = ShellConfig {
        content: root.clone(),
        interpolation: olr_scene::Interpolation::Linear,
        viewport: olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut sh = Shell::new(cfg, ui, Store::open(tmp.path()), olr_audio::Audio::silent());
    sh.act(UiAction::Open(Screen::Editor));
    Some((tmp, root, sh))
}

/// Every file under `dir` by relative path, with its bytes.
fn files(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn go(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                out.insert(p.strip_prefix(base).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

fn frame(sh: &mut Shell, actions: Vec<A>) {
    sh.skin_frame(actions, Instant::now());
}

fn packs(tmp: &tempfile::TempDir) -> PathBuf {
    tmp.path().join("packs")
}

/// A stroke of the pencil in `colour` at art pixel `(x, y)` of frame `f`.
fn paint(f: usize, colour: Rgba, x: i32, y: i32) -> Vec<A> {
    vec![A::Frame(f), A::Target(Target::Colour), A::Colour(colour), A::Press { x, y }, A::Release]
}

#[test]
fn a_copy_is_saved_to_your_packs_and_offered_by_the_skin_picker() {
    let Some((tmp, root, mut sh)) = shell() else { return };
    let firefly = root.parent().unwrap().join("reloaded/olr-skin-firefly");
    let before = files(&firefly);
    sh.act(UiAction::NewSkin(Some("olr-skin-firefly".into())));
    assert!(sh.skin_editor_open(), "{:?}", sh.data.editor_message);
    let state = sh.skin_editor().unwrap().state();
    assert_eq!((state.doc().name(), state.pack_id()), ("Firefly copy", None));
    let red = Rgba::rgb(250, 20, 20);
    let mut actions = paint(3, red, 1, 1);
    actions.extend([A::SetLight(Some(olr_data::skin::SkinLight { intensity: 120, reach: 30 })), A::Save]);
    frame(&mut sh, actions);
    assert_eq!(sh.skin_editor().unwrap().state().dialog(), Some(&Dialog::Name));
    frame(&mut sh, vec![A::SaveAs("Lantern".into())]);
    let h = sh.skin_editor().unwrap();
    assert_eq!(h.state().pack_id(), Some("lantern"));
    assert!(!h.state().dirty());
    let dir = packs(&tmp).join("lantern");
    assert_eq!(h.dir(), Some(dir.as_path()));
    // A valid skin pack, as the game loads it.
    let checked = olr_edit::skin::check_dir(&dir);
    assert!(checked.errors.is_empty(), "{:?}", checked.errors);
    let skin = checked.skin.unwrap();
    assert_eq!(skin.def.name, "Lantern");
    assert_eq!(skin.def.light, Some(olr_data::skin::SkinLight { intensity: 120, reach: 30 }));
    assert!(!skin.glow.is_empty(), "Firefly's glow kept");
    // The built-in skin is not changed.
    assert_eq!(files(&firefly), before);
    // The profiles' skin picker offers it at once, and the Skin tab as yours.
    assert!(sh.data.catalog.skins.iter().any(|s| s.id == "lantern" && s.name == "Lantern"));
    assert!(sh.data.editable_skins.contains("lantern"));
    // Close (nothing unsaved): the Editor screen again.
    frame(&mut sh, vec![A::Close]);
    assert!(!sh.skin_editor_open());
    assert_eq!(sh.ui.screen(), Screen::Editor);
    // Edit it in place: no name asked.
    sh.act(UiAction::EditSkin("lantern".into()));
    assert!(sh.skin_editor_open(), "{:?}", sh.data.editor_message);
    let blue = Rgba::rgb(10, 20, 250);
    let mut actions = paint(0, blue, 2, 2);
    actions.push(A::Save);
    frame(&mut sh, actions);
    assert_eq!(sh.skin_editor().unwrap().state().dialog(), None);
    assert!(!sh.skin_editor().unwrap().state().dirty());
    let src = match olr_edit::pack::load(&dir).unwrap().source {
        olr_edit::pack::Source::Skin(s) => s,
        _ => panic!("a skin pack"),
    };
    assert_eq!(src.image.get(2, 2), blue);
    assert_eq!(src.image.get(3 * 40 + 1, 1), red);
    assert_eq!(std::fs::read_dir(packs(&tmp)).unwrap().count(), 1, "no second pack");
}

#[test]
fn a_classic_skin_is_made_double_and_a_blank_one_is_double() {
    let Some((tmp, _root, mut sh)) = shell() else { return };
    sh.act(UiAction::NewSkin(Some("olx-skin-default".into())));
    assert!(sh.skin_editor_open(), "{:?}", sh.data.editor_message);
    assert!(sh.skin_editor().unwrap().state().doc().is_classic());
    frame(&mut sh, vec![A::MakeDouble, A::Save]);
    frame(&mut sh, vec![A::SaveAs("Default HD".into())]);
    let skin = olr_edit::skin::check_dir(&packs(&tmp).join("default-hd")).skin.expect("a valid pack");
    assert_eq!(skin.def.layout, olr_data::skin::FrameLayout::DOUBLE);
    frame(&mut sh, vec![A::Close]);
    sh.act(UiAction::NewSkin(None));
    let state = sh.skin_editor().unwrap().state();
    assert_eq!((state.doc().frame_size(), state.pack_id()), ((40, 36), None));
    // Unchanged: it closes without asking, and writes nothing.
    frame(&mut sh, vec![A::Close]);
    assert!(!sh.skin_editor_open());
    assert_eq!(std::fs::read_dir(packs(&tmp)).unwrap().count(), 1);
}

#[test]
fn a_bad_skin_pack_is_refused_with_a_message() {
    let Some((tmp, root, mut sh)) = shell() else { return };
    // A copy of OLX's default skin whose image is cut short.
    let dir = packs(&tmp).join("broken");
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["pack.ron", "skin.ron", "skin.png", "tint.png"] {
        std::fs::copy(root.join("olx-skin-default").join(f), dir.join(f)).unwrap();
    }
    let ron = std::fs::read_to_string(dir.join("pack.ron")).unwrap();
    std::fs::write(dir.join("pack.ron"), ron.replace("olx-skin-default", "broken")).unwrap();
    std::fs::write(dir.join("skin.png"), b"\x89PNG\r\n").unwrap();
    sh.act(UiAction::Back);
    sh.act(UiAction::Open(Screen::Editor));
    assert!(sh.data.editable_skins.contains("broken"));
    sh.act(UiAction::EditSkin("broken".into()));
    assert!(!sh.skin_editor_open());
    let why = sh.data.editor_message.clone().unwrap_or_default();
    assert!(why.contains("skin.png"), "{why}");
    // Not one of yours: refused too.
    sh.act(UiAction::EditSkin("olx-skin-default".into()));
    assert!(!sh.skin_editor_open());
    assert!(sh.data.editor_message.as_deref().unwrap_or_default().contains("edit a copy"));
}

#[test]
fn closing_the_window_asks_when_unsaved() {
    let Some((_tmp, _root, mut sh)) = shell() else { return };
    sh.act(UiAction::NewSkin(None));
    assert_eq!(sh.request_close(), CloseDecision::Quit, "nothing to save");
    assert!(!sh.skin_editor_open());
    sh.act(UiAction::NewSkin(None));
    frame(&mut sh, paint(0, Rgba::rgb(1, 2, 3), 0, 0));
    assert_eq!(sh.request_close(), CloseDecision::Wait);
    assert_eq!(sh.skin_editor().unwrap().state().dialog(), Some(&Dialog::Unsaved));
    frame(&mut sh, vec![A::Choice(olr_skin_editor::Choice::DontSave)]);
    assert!(!sh.skin_editor_open());
}
