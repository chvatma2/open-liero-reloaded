//! The menus' widgets driven by label (`egui_kittest`, spec §7 GUI flows)
//! and the local-play setup's match.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_bots::{Difficulty, Personality};
use olr_content::{Catalog, LevelInfo, ModInfo, PackSource, Player, ThumbRef};
use olr_data::image::RgbaImage;
use olr_data::manifest::{Origin, PackKind};
use olr_ui::screens::{self, MenuData};
use olr_ui::{
    AppCommand, BotLevel, BotPersonality, MAX_BOTS, MatchPrefs, Profile, Profiles, Screen, Settings,
    UiAction, UiState,
};

/// A `w` × `h` thumbnail written to a directory of its own, in a temporary
/// directory of the test's thread (removed when the test ends): tests
/// running in parallel never write a file another one is reading.
fn thumb_file(name: &str, w: u32, h: u32) -> ThumbRef {
    thread_local! {
        static DIR: tempfile::TempDir = tempfile::tempdir().unwrap();
    }
    static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pack = DIR.with(|d| d.path().join(format!("{n}-{name}")));
    std::fs::create_dir_all(&pack).unwrap();
    let png = olr_pack::image_io::encode_rgba(&RgbaImage::new(w, h));
    std::fs::write(pack.join(olr_pack::THUMB_FILE), png).unwrap();
    ThumbRef::probe(&pack).unwrap().unwrap()
}

fn catalog() -> Catalog {
    let presets = |p: &[&str]| p.iter().map(|s| s.to_string()).collect();
    Catalog {
        mods: vec![
            ModInfo {
                id: "olx-classic".into(),
                name: "Liero v1.0".into(),
                presets: presets(&["All weapons", "Standard 100lt"]),
            },
            ModInfo {
                id: "olx-mw-1-0".into(),
                name: "Modern Warfare1.0".into(),
                presets: presets(&["Mortars"]),
            },
        ],
        levels: vec![
            LevelInfo {
                id: "olx-level-dirt-level".into(),
                name: "Dirt Level".into(),
                width: 504,
                height: 350,
                thumb: Some(thumb_file("dirt", 50, 35)),
            },
            LevelInfo {
                id: "olx-level-liero-factory".into(),
                name: "Liero Factory".into(),
                width: 1200,
                height: 480,
                thumb: None,
            },
        ],
        skins: vec![],
        sources: vec![
            source("olx-classic", PackKind::Mod, "Liero v1.0", "Classic"),
            source("olx-level-dirt-level", PackKind::Level, "Dirt Level", "levels/Dirt Level.lxl"),
            source("olx-theme-dirt", PackKind::Theme, "Dirt", "data/themes/Dirt"),
            source("olx-core", PackKind::Core, "OpenLieroX core", "data"),
            source("olx-skin-ant", PackKind::Skin, "ant", "skins/ant.png"),
            source("olx-skin-bat", PackKind::Skin, "bat", "skins/bat.png"),
        ],
        warnings: vec![],
    }
}

fn source(id: &str, kind: PackKind, name: &str, path: &str) -> PackSource {
    let origin = Origin {
        source: "OpenLieroX".into(),
        path: format!("share/gamedir/{path}"),
        revision: "d1ad953f4f20610f2648cce26c9739e7c088409d".into(),
    };
    PackSource { id: id.into(), kind, name: name.into(), origin: Some(origin) }
}

/// The menus and what they asked the app to do.
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

fn starts(app: &App) -> Vec<&olr_content::MatchOptions> {
    app.commands
        .iter()
        .filter_map(|c| match c {
            AppCommand::StartMatch(o) => Some(o),
            _ => None,
        })
        .collect()
}

#[test]
fn main_to_local_play_to_match() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 7));
    for label in ["Local Play", "Sandbox", "Profiles", "Options", "Replays", "About", "Quit"] {
        h.get_by_label(label);
    }
    h.get_by_label("Local Play").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::LocalPlay);
    // Both levels show: one with its thumbnail, one with a placeholder.
    h.get_by_label("Liero Factory");
    h.get_by_label("1200 × 480");
    h.get_by_label("Dirt Level").click();
    h.run();
    assert_eq!(h.state().ui.settings().gameplay.match_prefs.level_id, "olx-level-dirt-level");
    // Back and Esc both return to the main menu.
    h.get_by_label("Back").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    h.get_by_label("Local Play").click();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    h.get_by_label("Local Play").click();
    h.run();

    // Start once; the second press and Esc while it loads do nothing.
    h.get_by_label("Start").click();
    h.run();
    assert!(h.state().ui.is_starting());
    h.get_by_label("Loading…");
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::LocalPlay);
    let s = starts(h.state());
    assert_eq!(s.len(), 1, "{:?}", h.state().commands);
    assert_eq!((s[0].seed, s[0].level_id.as_str()), (7, "olx-level-dirt-level"));

    // The app reports it ready: OLX's weapon selection, then in game; Esc
    // pauses and the pause menu leaves.
    h.state_mut().ui.update(UiAction::MatchStarted);
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);
    h.state_mut().ui.update(UiAction::ChooseWeapons { weapons: [0; 5], names: vec![] });
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().ui.is_paused());
    h.get_by_label("Resume").click();
    h.run();
    assert!(!h.state().ui.is_paused());
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label("Quit to menu").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert_eq!(h.state().commands.last(), Some(&AppCommand::EndMatch));
}

#[test]
fn local_play_builds_match_options() {
    let cat = catalog();
    let mut p = MatchPrefs::default();
    // A mod keeps the preset when it has it, else takes Standard 100lt, else its first.
    p.set_mod(cat.mod_info("olx-mw-1-0").unwrap());
    assert_eq!((p.mod_id.as_str(), p.restrictions.as_str()), ("olx-mw-1-0", "Mortars"));
    p.set_mod(cat.mod_info("olx-classic").unwrap());
    assert_eq!(p.restrictions, "Standard 100lt");
    p.restrictions = "All weapons".into();
    p.set_mod(cat.mod_info("olx-classic").unwrap());
    assert_eq!(p.restrictions, "All weapons");
    p.level_id = "olx-level-dirt-level".into();
    // Bots: a count, one difficulty for all or each its own, a personality.
    p.set_bots(4);
    p.difficulty = BotLevel::Hard;
    p.set_per_bot(true);
    p.per_bot.as_mut().unwrap()[2] = BotLevel::Easy;
    p.set_bots(5);
    assert_eq!(p.per_bot.as_ref().unwrap().len(), 5, "a new bot gets the common difficulty");
    p.personality = BotPersonality::Chaos;
    p.loading_time = 250;
    p.bonuses = true;
    p.rules.lives = 5;
    p.rules.kill_limit = 0;
    p.rules.time_limit = 3.0;

    // The human plays the selected profile; the options carry everything.
    let mut profiles = Profiles::default();
    profiles.list.push(Profile { name: "Second".into(), ..Profile::default() });
    let mut ui = UiState::new(Settings::default(), profiles.clone(), 3);
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::SetMatch(p.clone()));
    profiles.selected = 1;
    ui.update(UiAction::SetProfiles(profiles));
    assert_eq!(ui.profiles().selected().unwrap().name, "Second");
    let c = ui.update(UiAction::StartMatch);
    let [AppCommand::StartMatch(o)] = &c[..] else { panic!("{c:?}") };
    assert_eq!((o.mod_id.as_str(), o.level_id.as_str(), o.seed), ("olx-classic", "olx-level-dirt-level", 3));
    assert_eq!(o.restrictions, "All weapons");
    assert!(o.settings.bonuses);
    assert_eq!(o.settings.loading_time, 250);
    assert_eq!(o.players.len(), 6);
    assert_eq!(o.players[0], Player::Human);
    let bots: Vec<_> = o.players[1..]
        .iter()
        .map(|p| match p {
            Player::Bot(b) => (b.difficulty, b.personality),
            Player::Human | Player::HumanWith(_) => panic!("one human"),
        })
        .collect();
    use Difficulty::{Easy, Hard};
    let chaos = Personality::Chaos;
    assert_eq!(bots, [(Hard, chaos), (Hard, chaos), (Easy, chaos), (Hard, chaos), (Hard, chaos)]);
    assert_eq!((o.deathmatch.lives, o.deathmatch.kill_limit, o.deathmatch.time_limit), (5, 0, 3.0));
    // One difficulty for all again.
    p.set_per_bot(false);
    assert!(p.options(1).players[1..].iter().all(|b| matches!(b, Player::Bot(c) if c.difficulty == Hard)));

    // Out of range: clamped, one warning each.
    p.set_bots(0);
    assert_eq!(p.bots, 1);
    p.set_bots(200);
    assert_eq!(p.bots, MAX_BOTS);
    p.loading_time = 9999;
    p.per_bot = Some(vec![BotLevel::Easy; 2]);
    let w = p.sanitize();
    assert_eq!(w.len(), 2, "{w:?}");
    assert_eq!((p.loading_time, p.per_bot.as_ref().unwrap().len()), (500, usize::from(MAX_BOTS)));
    // A file from before these fields gets their defaults.
    let old: MatchPrefs = ron::from_str("(mod_id: \"olx-classic\", bots: 2)").unwrap();
    assert_eq!((old.loading_time, old.per_bot, old.personality), (100, None, BotPersonality::Olx));

    // The widgets: each bot its own difficulty, through the checkbox.
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 7));
    h.get_by_label("Local Play").click();
    h.run();
    h.get_by_label("Each bot its own difficulty").click();
    h.run();
    let prefs = &h.state().ui.settings().gameplay.match_prefs;
    assert_eq!(prefs.per_bot.as_ref().map(Vec::len), Some(usize::from(prefs.bots)));
    h.get_by_label("Bot 3");

    // The match panel's reset: OLX's match, the mod and level kept, and
    // remembered (moved here from the options' Gameplay tab).
    h.get_by_label("Lives").click();
    h.run();
    h.get_by_label("Dirt Level").click();
    h.run();
    let prefs = &h.state().ui.settings().gameplay.match_prefs;
    assert!(prefs.rules.lives >= 0 && prefs.per_bot.is_some());
    h.get_by_label("Reset the match to OLX's settings").click();
    h.run();
    let olx = MatchPrefs {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-dirt-level".into(),
        ..MatchPrefs::default()
    };
    assert_eq!(h.state().ui.settings().gameplay.match_prefs, olx);
    assert!(
        matches!(h.state().commands.last(), Some(AppCommand::SaveSettings(s)) if s.gameplay.match_prefs == olx),
        "saved"
    );
}

/// Every picker in local play is found by its label (as a screen reader
/// and `--autoplay` find widgets), so a script can choose the mod.
#[test]
fn local_play_pickers_are_found_by_their_label() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 7));
    h.get_by_label("Local Play").click();
    h.run();
    for label in ["Mod", "Weapons", "Difficulty", "Personality", "Loadout", "Player"] {
        h.get_by_label(label);
    }
    h.get_by_label("Mod").click();
    h.run();
    h.get_by_label("Modern Warfare1.0").click();
    h.run();
    assert_eq!(h.state().ui.settings().gameplay.match_prefs.mod_id, "olx-mw-1-0");
    h.get_by_label("Difficulty").click();
    h.run();
    h.get_by_label("Hard").click();
    h.run();
    assert_eq!(h.state().ui.settings().gameplay.match_prefs.difficulty, BotLevel::Hard);
    h.get_by_label("Each bot its own difficulty").click();
    h.run();
    h.get_by_label("Bot 2").click();
    h.run();
    h.get_by_label("Easy").click();
    h.run();
    let per_bot = h.state().ui.settings().gameplay.match_prefs.per_bot.clone().unwrap();
    assert_eq!(per_bot[1], BotLevel::Easy, "{per_bot:?}");
}

#[test]
fn local_play_replaces_a_saved_selection_no_longer_imported() {
    let mut settings = Settings::default();
    settings.gameplay.match_prefs.mod_id = "olx-gone".into();
    settings.gameplay.match_prefs.level_id = "olx-level-gone".into();
    let mut h = harness(UiState::new(settings, Profiles::default(), 7));
    h.get_by_label("Local Play").click();
    h.run();
    let p = &h.state().ui.settings().gameplay.match_prefs;
    assert_eq!((p.mod_id.as_str(), p.level_id.as_str()), ("olx-classic", "olx-level-dirt-level"));
    assert_eq!(p.restrictions, "Standard 100lt", "the first mod's preset");
    assert!(h.query_by_label_contains("(missing)").is_none());
    assert!(
        h.state().commands.iter().any(|c| matches!(c, AppCommand::SaveSettings(_))),
        "the repaired choice is saved"
    );
}

/// A long level list decodes only the thumbnails in view, at most
/// `THUMB_DECODES_PER_FRAME` a frame, each once; an unreadable one keeps its
/// placeholder and is reported once.
#[test]
fn thumbnails_decode_lazily_and_bounded() {
    let mut c = catalog();
    c.levels = (0..60)
        .map(|i| LevelInfo {
            id: format!("lvl-{i:02}"),
            name: format!("Level {i}"),
            width: 640,
            height: 480,
            thumb: Some(thumb_file(&format!("lvl-{i:02}"), 160, 96)),
        })
        .collect();
    // The first one breaks after it was listed.
    let broken = c.levels[0].thumb.clone().unwrap();
    std::fs::write(broken.path(), b"not a png any more").unwrap();
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 7);
    ui.update(UiAction::Open(Screen::LocalPlay));
    let app = App { ui, data: MenuData::new(c, None), commands: Vec::new() };
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    );
    // The harness drew a few frames when built; from here, frame by frame.
    for _ in 0..60 {
        let before = h.state().data.thumbs_decoded();
        h.step();
        let after = h.state().data.thumbs_decoded();
        assert!(after - before <= screens::THUMB_DECODES_PER_FRAME, "{before} then {after}");
    }
    let decoded = h.state().data.thumbs_decoded();
    assert!(decoded > screens::THUMB_DECODES_PER_FRAME, "{decoded}");
    assert!(decoded < 60, "only the cards in view: {decoded}");
    let warnings = h.state_mut().data.take_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("lvl-00"), "{warnings:?}");
    for _ in 0..5 {
        h.step();
    }
    assert!(h.state_mut().data.take_warnings().is_empty(), "tried once");
    assert_eq!(h.state().data.thumbs_decoded(), decoded, "kept, not decoded again");
}

/// Plan 7 G9: the main menu's About shows the credits (Jason Boettcher's
/// Liero Xtreme, the OpenLieroX team, Joosa Riekkinen's Liero), the
/// licenses and the imported content's origin; Back returns.
#[test]
fn about_credits_the_authors_and_the_licenses() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 7));
    h.get_by_label("About").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::About);
    for text in [
        "Joosa Riekkinen",
        "Jason Boettcher, Liero Xtreme",
        "The OpenLieroX team",
        "GPL-3.0-or-later",
        "Lesser General Public License",
        "zlib-style",
    ] {
        h.get_by_label_contains(text);
    }
    // The content's origin as its pack records it.
    h.get_by_label("Mod Liero v1.0");
    h.get_by_label("OpenLieroX share/gamedir/Classic (d1ad953f)");
    h.get_by_label("Level Dirt Level");
    h.get_by_label("Theme Dirt");
    h.get_by_label("OpenLieroX share/gamedir/data/themes/Dirt (d1ad953f)");
    h.get_by_label("Core OpenLieroX core");
    h.get_by_label("2 skins");
    h.get_by_label("OpenLieroX share/gamedir/skins (d1ad953f)");
    h.get_by_label_contains("imported from OpenLieroX");
    h.get_by_label("Back").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
}

/// The store's notices (a newer game's settings file) show on the main
/// menu, not only in the log.
#[test]
fn main_menu_shows_the_store_notices() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 7));
    let text = "settings.ron is from a newer version; using defaults and leaving it untouched";
    assert!(h.query_by_label_contains(text).is_none());
    h.state_mut().data.notices = vec![text.to_string()];
    h.run();
    h.get_by_label_contains(text);
}
