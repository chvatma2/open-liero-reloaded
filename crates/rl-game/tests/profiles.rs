//! The profiles in the game (plan 7 task 3): the profiles screen's skin
//! preview uploaded as egui textures, and the selected profile's skin and
//! colour on the human's worm in a match started from the menus. The
//! match needs `RL_CONTENT_DIR`.

use std::path::{Path, PathBuf};

use rl_audio::Audio;
use rl_data::image::Rgba;
use rl_game::shell::{Shell, ShellConfig};
use rl_scene::{Interpolation, Viewport};
use rl_ui::skin_preview::PreviewKey;
use rl_ui::{Profile, Profiles, Screen, Settings, Store, UiAction, UiState};

fn shell(content: &Path, profiles: Profiles) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), profiles, 1);
    Shell::new(cfg, ui, Store::in_memory(), Audio::silent())
}

/// One frame of the menus: drawn, the actions carried out, the preview
/// uploaded.
fn frame(sh: &mut Shell, ctx: &egui::Context) {
    let mut actions = Vec::new();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        actions = rl_ui::screens::show(ui, &sh.ui, &mut sh.data);
    });
    // No renderer here: the textures go nowhere.
    out.textures_delta.clear();
    for a in actions {
        sh.act(a);
    }
    sh.update_preview(ctx);
}

#[test]
fn the_profiles_screen_gets_its_preview_textures() {
    // No content: the placeholder skin, never a panic.
    let tmp = tempfile::tempdir().unwrap();
    let mut sh = shell(tmp.path(), Profiles::default());
    let ctx = egui::Context::default();
    frame(&mut sh, &ctx);
    assert!(sh.data.preview.is_none(), "the main menu shows none");
    sh.act(UiAction::Open(Screen::Profiles));
    frame(&mut sh, &ctx);
    let key = PreviewKey { skin_id: "default".into(), colour: [0, 177, 0] };
    let t = sh.data.preview.clone().expect("uploaded");
    assert_eq!((t.key, t.right.len(), t.left.len()), (key, 21, 21));
    // Another colour: new textures; the same again: kept.
    let mut p = sh.ui.profiles().clone();
    p.list[0].colour = [255, 0, 0];
    sh.act(UiAction::SetProfiles(p));
    frame(&mut sh, &ctx);
    let t2 = sh.data.preview.clone().unwrap();
    assert_eq!(t2.key.colour, [255, 0, 0]);
    assert_ne!(t2.right, t.right);
    frame(&mut sh, &ctx);
    assert_eq!(sh.data.preview.as_ref(), Some(&t2));
    // Leaving the screen frees them.
    sh.act(UiAction::Back);
    frame(&mut sh, &ctx);
    assert!(sh.data.preview.is_none());
}

fn content_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping the imported content");
    }
    dir
}

#[test]
fn a_menu_match_plays_the_selected_profile_look() {
    let Some(dir) = content_dir() else { return };
    let mine = Profile {
        name: "Mine".into(),
        skin_id: "olx-skin-angryworm".into(),
        colour: [20, 90, 255],
        bindings: None,
    };
    let profiles = Profiles { list: vec![Profile::default(), mine], selected: 1 };
    let mut sh = shell(&dir, profiles);
    sh.act(UiAction::Open(Screen::LocalPlay));
    sh.act(UiAction::StartMatch);
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::InGame, "{:?}", sh.ui.message());
    let content = rl_content::Content::open(&dir).unwrap();
    let assets = sh.session().unwrap().present.assets();
    let skin = |i| &assets.image(assets.worm(i).skin).image;
    // The human: the profile's skin in its colour; the bots: OLX's default
    // skin in their default colours.
    let human = content.load_skin("olx-skin-angryworm").unwrap();
    assert_eq!(skin(0), &human.colorized(Rgba::rgb(20, 90, 255)));
    let default = content.load_skin("olx-skin-default").unwrap();
    assert_eq!(skin(1), &default.colorized(rl_present::default_colour(1)));
    assert_eq!(sh.session().unwrap().names.0[0], "Mine");
}
