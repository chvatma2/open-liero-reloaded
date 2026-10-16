//! Profiles (plan 7 task 3): create, edit, delete and select them through
//! the widgets, their names validated, their files round-tripped, and the
//! skin preview drawn with the game's colorization.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, LoadedSkin, SkinInfo};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use olr_scene::Rect;
use olr_sim::Dir;
use olr_ui::profiles::NameError;
use olr_ui::screens::{self, MenuData};
use olr_ui::skin_preview::{self, PreviewAnim, PreviewKey};
use olr_ui::{AppCommand, MAX_NAME_CHARS, Profile, Profiles, Screen, Settings, Store, UiAction, UiState};

fn catalog() -> Catalog {
    let skin = |id: &str, name: &str| SkinInfo { id: id.into(), name: name.into() };
    Catalog {
        skins: vec![
            skin("olx-skin-ant", "ant"),
            skin("olx-skin-batman", "batman"),
            skin("olx-skin-default", "default"),
        ],
        ..Catalog::default()
    }
}

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(ui: UiState) -> Harness<'static, App> {
    let app = App { ui, data: MenuData::new(catalog(), None), commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

/// The profiles the menus last asked to save.
fn last_saved(app: &App) -> Option<&Profiles> {
    app.commands.iter().rev().find_map(|c| match c {
        AppCommand::SaveProfiles(p) => Some(p),
        _ => None,
    })
}

/// Replaces the profile's name in the Name field.
fn retype(h: &mut Harness<'static, App>, new: &str) {
    h.get_by_role_and_label(Role::TextInput, "Name").focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Name").type_text(new);
    h.run();
}

#[test]
fn profile_edits_persist() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    h.get_by_label("Profiles").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Profiles);
    // The default profile is listed, playing, and its look is previewed.
    h.get_by_label("OpenLieroXor");
    let key = PreviewKey { skin_id: "default".into(), colour: [0, 177, 0] };
    assert_eq!(h.state().data.preview_wanted, Some(key));

    // A new profile: a free name, then renamed, a skin found by search,
    // a colour, and played.
    h.get_by_label("New").click();
    h.run();
    let p = h.state().ui.profiles().clone();
    assert_eq!(p.list.len(), 2);
    assert_eq!(p.list[1].name, "Player 2");
    retype(&mut h, "Gusanos");
    assert_eq!(h.state().ui.profiles().list[1].name, "Gusanos");
    h.get_by_role_and_label(Role::TextInput, "Skin").focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Skin").type_text("BAT");
    h.run();
    assert!(h.query_by_label("ant").is_none(), "the search hides other skins");
    h.get_by_label("batman").click();
    h.run();
    assert_eq!(h.state().ui.profiles().list[1].skin_id, "olx-skin-batman");
    let mut p = h.state().ui.profiles().clone();
    p.list[1].colour = [20, 90, 255];
    h.state_mut().ui.update(UiAction::SetProfiles(p));
    h.run();
    let key = PreviewKey { skin_id: "olx-skin-batman".into(), colour: [20, 90, 255] };
    assert_eq!(h.state().data.preview_wanted, Some(key), "the edited profile is previewed");
    h.get_by_label("Play as this").click();
    h.run();
    assert_eq!(h.state().ui.profiles().selected, 1);

    // Saved, they load back the same.
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path());
    let saved = last_saved(h.state()).expect("profiles saved").clone();
    assert_eq!(&saved, h.state().ui.profiles());
    store.save_profiles(&saved).unwrap();
    let loaded = Store::open(tmp.path()).load_profiles();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.value, saved);
    assert_eq!(loaded.value.selected().unwrap().name, "Gusanos");

    // Deleting the first asks first; the one playing stays selected.
    h.get_by_label("OpenLieroXor").click();
    h.run();
    h.get_by_label("Delete").click();
    h.run();
    assert_eq!(h.state().ui.profiles().list.len(), 2, "not before it is confirmed");
    h.get_by_label("Yes, delete").click();
    h.run();
    let p = h.state().ui.profiles().clone();
    assert_eq!(p.list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Gusanos"]);
    assert_eq!(p.selected().unwrap().name, "Gusanos");
    // The last profile cannot be deleted.
    assert!(h.get_by_label("Delete").accesskit_node().is_disabled());
    let saved = last_saved(h.state()).unwrap().clone();
    store.save_profiles(&saved).unwrap();
    assert_eq!(Store::open(tmp.path()).load_profiles().value, saved);
}

#[test]
fn the_profiles_screen_opens_on_the_profile_playing() {
    let mut p = Profiles::default();
    p.add();
    p.selected = 1;
    let mut h = harness(UiState::new(Settings::default(), p, 1));
    h.get_by_label("Profiles").click();
    h.run();
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Name").value().as_deref(), Some("Player 2"));
}

#[test]
fn profile_names_are_validated() {
    let mut p = Profiles::default();
    // OLX: trimmed, not empty, at most 20 characters (the text box's limit).
    assert_eq!(p.check_name("  Worm  ", None), Ok("Worm".into()));
    assert_eq!(p.check_name("   ", None), Err(NameError::Empty));
    let longest = "é".repeat(MAX_NAME_CHARS);
    assert_eq!(p.check_name(&longest, None), Ok(longest.clone()));
    assert_eq!(p.check_name(&format!("{longest}x"), None), Err(NameError::TooLong));
    assert_eq!(p.check_name("a\tb", None), Err(NameError::Control));
    // Unique, ignoring case; a profile keeps its own name.
    assert_eq!(p.check_name(" openlieroxor", None), Err(NameError::Taken));
    assert_eq!(p.check_name("OpenLieroXor", Some(0)), Ok("OpenLieroXor".into()));
    // Renaming refuses a bad name and keeps the old one.
    assert_eq!(p.rename(0, ""), Err(NameError::Empty));
    assert_eq!(p.list[0].name, "OpenLieroXor");
    // New profiles get free names; the last profile is never removed.
    let a = p.add();
    let b = p.add();
    assert_eq!((p.list[a].name.as_str(), p.list[b].name.as_str()), ("Player 2", "Player 3"));
    p.rename(a, "Player 3x").unwrap();
    p.selected = b;
    assert!(p.remove(0));
    assert_eq!(p.selected().unwrap().name, "Player 3", "the one playing stays selected");
    assert!(p.remove(0) && !p.remove(0));
    assert_eq!(p.list.len(), 1);
    // A file's control characters go when it is loaded.
    let mut bad = Profiles { list: vec![Profile { name: "a\nb".into(), ..Profile::default() }], selected: 0 };
    assert_eq!(bad.sanitize().len(), 1);
    assert_eq!(bad.list[0].name, "ab");

    // The widget: an empty name shows why and is not applied.
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    h.get_by_label("Profiles").click();
    h.run();
    retype(&mut h, " ");
    h.get_by_label(&NameError::Empty.to_string());
    assert_eq!(h.state().ui.profiles().list[0].name, "OpenLieroXor");
    assert!(last_saved(h.state()).is_none(), "nothing to save");
}

/// A sheet whose every pixel differs, with every mask kind.
fn test_skin() -> LoadedSkin {
    let (w, h) = (672, 18);
    let mut image = RgbaImage::new(w, h);
    let mut mask = Vec::new();
    for y in 0..h {
        for x in 0..w {
            image.set(x, y, Rgba::rgb((x % 256) as u8, (y * 13) as u8, (x / 256 * 90) as u8));
            mask.push(
                [MaskKind::Keep, MaskKind::Colorize, MaskKind::Colorize, MaskKind::Transparent]
                    [(x % 4) as usize],
            );
        }
    }
    let def = SkinDef { name: "test".into(), layout: FrameLayout::OLX };
    LoadedSkin { skin: Skin { def, image, mask }, warnings: vec![] }
}

/// `src` of `sheet` as the renderer draws a sprite: mirrored with `flip`.
fn drawn(sheet: &RgbaImage, src: Rect, flip: bool) -> RgbaImage {
    let mut out = RgbaImage::new(src.w, src.h);
    for y in 0..src.h {
        for x in 0..src.w {
            let sx = if flip { src.x + src.w - 1 - x } else { src.x + x };
            out.set(x, y, sheet.get(sx, src.y + y));
        }
    }
    out
}

#[test]
fn skin_preview_uses_colorization_like_the_game() {
    let skin = test_skin();
    let colour = [0, 177, 0];
    let frames = skin_preview::frames(&skin, colour);
    assert_eq!(frames.len(), 21);
    // The game's sheet: what `olr_scene::SceneAssets::add_worm` uploads.
    let sheet = skin.colorized(Rgba::rgb(0, 177, 0));
    for walk in [0.0, 1.5, 2.9] {
        for angle in [-90.0, -60.0, -10.0, 0.0, 35.0, 60.0] {
            for face in [Dir::Right, Dir::Left] {
                let (src, flip) = olr_scene::skin_frame(angle, face, walk, 21);
                let f = (src.x - 4) / 32;
                assert_eq!(frames.get(f, face), &drawn(&sheet, src, flip), "frame {f} {face:?}");
            }
        }
    }

    // OLX's menu animation: it aims from 0 down to 60, back up to −90 at
    // 200° a second, walking 7.5 frames a second.
    let mut a = PreviewAnim::default();
    assert_eq!(a.frame(), 4, "level aim, walk frame 0");
    a.step(0.2);
    assert_eq!(a.frame(), 7 + 1 + 4, "40° down, walk 1.5");
    a.step(0.1);
    assert_eq!(a.frame(), 14 + 2 + 4, "60° down (the end), walk 2.25");
    a.step(0.12);
    assert_eq!(a.frame(), 1 + 4, "back up to 36°, the walk wrapped to 0");
    for _ in 0..100 {
        a.step(0.05);
        assert!(a.frame() < 21);
    }

    // With OLX's content: the very image the scene draws worms from.
    let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set; skipping the imported content");
        return;
    };
    let content = olr_content::Content::open(std::path::Path::new(&dir)).unwrap();
    let m = content.load_mod("olx-classic").unwrap();
    let mut assets = olr_scene::SceneAssets::new(content.core(), &m);
    let skin = content.skin_or_default("olx-skin-default");
    assert!(skin.warnings.is_empty(), "{:?}", skin.warnings);
    let i = assets.add_worm(&skin, Rgba::rgb(20, 90, 255), true, true);
    let game = &assets.image(assets.worm(i).skin).image;
    let frames = skin_preview::frames(&skin, [20, 90, 255]);
    for face in [Dir::Right, Dir::Left] {
        let (src, flip) = olr_scene::skin_frame(0.0, face, 1.0, 21);
        assert_eq!(frames.get(11, face), &drawn(game, src, flip), "{face:?}");
    }
}
