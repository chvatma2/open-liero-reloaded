//! The menus' widgets driven by label (`egui_kittest`, spec §7 GUI flows)
//! and the local-play setup's match.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rl_bots::{Difficulty, Personality};
use rl_content::{Catalog, LevelInfo, ModInfo, Player};
use rl_data::image::RgbaImage;
use rl_ui::screens::{self, MenuData};
use rl_ui::{
    AppCommand, BotLevel, BotPersonality, MAX_BOTS, MatchPrefs, Profile, Profiles, Screen, Settings,
    UiAction, UiState,
};

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
                thumb: Some(RgbaImage::new(50, 35)),
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
        warnings: vec![],
    }
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

fn starts(app: &App) -> Vec<&rl_content::MatchOptions> {
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
    for label in ["Local Play", "Sandbox", "Profiles", "Options", "Replays", "Import", "About", "Quit"] {
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
