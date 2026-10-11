//! The Import screen in the game (`shell.rs`, plan 7 task 7) without a
//! window: an import of the committed tiny OLX game directory
//! `fixtures/olx-mini` into the data directory's `packs/` off the UI
//! thread, the content listed again when it ends (new packs in local play
//! and the profiles, re-imported levels with their new thumbnails, the
//! imported pack over the bundled one), no game starting meanwhile, and a
//! quit in the middle leaving no half-written pack.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rl_audio::Audio;
use rl_content::import::{self, ImportOptions, ImportPlan};
use rl_game::shell::{Shell, ShellConfig};
use rl_scene::{Interpolation, Viewport};
use rl_ui::import::ImportResult;
use rl_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};

const LEVEL: &str = "olx-level-tiny-level";

fn olx_mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/olx-mini")
}

fn shell(content: &Path, store: Store) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    Shell::new(cfg, ui, store, Audio::silent())
}

/// Bundled content: the fixture's level and theme, as `rl-cli import`
/// writes them.
fn bundled(dir: &Path) {
    let plan = ImportPlan {
        levels: vec!["levels/Tiny Level.lxl".into()],
        themes: vec!["data/themes/Dirt".into()],
        ..ImportPlan::default()
    };
    let o = ImportOptions { revision: "bundled".into(), prune: true };
    import::import(&olx_mini(), &plan, dir, &o, &mut |_| true).unwrap();
}

/// One frame of the menus, as the app draws them (thumbnails decode).
fn draw(sh: &mut Shell, ctx: &egui::Context) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0))),
        ..Default::default()
    };
    let mut actions = Vec::new();
    let mut out = ctx.run_ui(input, |ui| actions = rl_ui::screens::show(ui, &sh.ui, &mut sh.data));
    // No renderer here: the textures are dropped.
    out.textures_delta.clear();
    for a in actions {
        sh.act(a);
    }
}

/// Polls the shell each "frame" until the import ends (at most 60 s).
fn until_imported(sh: &mut Shell) {
    let until = Instant::now() + Duration::from_secs(60);
    while sh.ui.is_importing() {
        assert!(Instant::now() < until, "the import never ended: {:?}", sh.data.import);
        sh.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn hidden(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect()
}

#[test]
fn import_refreshes_the_menus_and_overrides_bundled() {
    let content = tempfile::tempdir().unwrap();
    bundled(content.path());
    let data = tempfile::tempdir().unwrap();
    let packs = data.path().join(rl_ui::PACKS_DIR);
    let mut sh = shell(content.path(), Store::open(data.path()));
    let ctx = egui::Context::default();
    assert!(sh.data.catalog.mods.is_empty());
    assert_eq!(sh.data.packs_dir.as_deref(), Some(packs.as_path()));
    // Local play shows the bundled level's thumbnail.
    sh.act(UiAction::Open(Screen::LocalPlay));
    for _ in 0..3 {
        draw(&mut sh, &ctx);
    }
    assert!(sh.data.thumbs_decoded() >= 1);
    let bundled_thumb = sh.data.catalog.level(LEVEL).unwrap().thumb.clone().unwrap();
    assert!(bundled_thumb.dir.starts_with(content.path()));

    // Import, pressed twice: one import, off the UI thread.
    sh.act(UiAction::Back);
    sh.act(UiAction::Open(Screen::Import));
    let path = olx_mini().display().to_string();
    sh.act(UiAction::StartImport(path.clone()));
    sh.act(UiAction::StartImport(path.clone()));
    assert!(sh.ui.is_importing() && sh.importing());
    // No game starts meanwhile.
    sh.act(UiAction::Back);
    sh.act(UiAction::Open(Screen::LocalPlay));
    sh.act(UiAction::StartMatch);
    assert!(!sh.ui.is_starting());
    until_imported(&mut sh);
    assert_eq!(sh.launches(), 0);
    assert!(!sh.importing());
    let Some(ImportResult::Done { imported: 4, failed: 2, tried: 6, cancelled: false, .. }) =
        &sh.data.import.result
    else {
        panic!("{:?}", sh.data.import)
    };
    assert_eq!(sh.data.import.entries.len(), 6, "one import");
    assert!(packs.join(rl_content::import::REPORT_FILE).is_file());
    assert_eq!(sh.ui.settings().import.last_path, path, "the path remembered");

    // The content is listed again: the new mod in local play, the new
    // skin in the profiles, the level now the imported pack's (over the
    // bundled one), its thumbnail made again.
    assert!(sh.data.catalog.mod_info("olx-tiny-mod").is_some());
    assert!(sh.data.catalog.skin("default").is_some());
    let thumb = sh.data.catalog.level(LEVEL).unwrap().thumb.clone().unwrap();
    assert!(thumb.dir.starts_with(&packs), "{thumb:?}");
    assert_eq!(sh.data.thumbs_decoded(), 0, "the thumbnails are made again");
    draw(&mut sh, &ctx);
    assert!(sh.data.thumbs_decoded() >= 1);
    let origin = sh.data.catalog.sources.iter().find(|s| s.id == LEVEL).unwrap().origin.clone().unwrap();
    assert_eq!(origin.revision, rl_ui::import::LOCAL_REVISION);

    // A game starts on the imported packs.
    let mut prefs = sh.ui.settings().gameplay.match_prefs.clone();
    prefs.set_mod(sh.data.catalog.mod_info("olx-tiny-mod").unwrap());
    prefs.level_id = LEVEL.into();
    sh.act(UiAction::SetMatch(prefs));
    sh.act(UiAction::StartMatch);
    sh.poll();
    assert!(sh.play.is_some(), "{:?}", sh.ui.message());
    assert_eq!(sh.play.as_ref().unwrap().assets.level_id, LEVEL);
}

/// Quitting while an import runs stops it after its item: no pack is
/// left half-written (hidden); without a data directory there is nothing
/// to import into, said on the screen.
#[test]
fn quitting_mid_import_leaves_whole_packs() {
    let content = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let packs = data.path().join(rl_ui::PACKS_DIR);
    let mut sh = shell(content.path(), Store::open(data.path()));
    sh.act(UiAction::Open(Screen::Import));
    sh.act(UiAction::StartImport(olx_mini().display().to_string()));
    sh.poll();
    sh.finish();
    assert!(!sh.importing());
    assert!(hidden(&packs).is_empty(), "{:?}", hidden(&packs));
    if packs.is_dir() {
        let c = rl_content::Content::open(&packs).unwrap();
        assert!(c.warnings().iter().all(|w| !w.contains("skipped")), "{:?}", c.warnings());
    }

    let mut sh = shell(content.path(), Store::in_memory());
    assert!(sh.data.packs_dir.is_none());
    sh.act(UiAction::Open(Screen::Import));
    sh.act(UiAction::StartImport(olx_mini().display().to_string()));
    assert!(!sh.ui.is_importing());
    assert!(matches!(&sh.data.import.result, Some(ImportResult::Failed(w)) if w.contains("data directory")));
}
