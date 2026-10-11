//! In game (plan 7 task 5): the HUD's layout at every UI scale (Review
//! Focus 5), OLX's weapon selection, the pause menu with the options, the
//! results' buttons, and Esc and F5 that ignore key repeat.

use egui::{Event, Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rl_content::Catalog;
use rl_sim::{Keys, MAX_SLOTS};
use rl_ui::hud::{HudLayout, Results, Row};
use rl_ui::screens::{self, MenuData};
use rl_ui::weapon_select::{WeaponMenu, WeaponSelect};
use rl_ui::{AppCommand, Game, Profiles, Screen, Settings, UiAction, UiState};

/// The overlap of two rectangles has an area (touching edges do not count).
fn overlap(a: Rect, b: Rect) -> bool {
    let i = a.intersect(b);
    i.width() > 0.0 && i.height() > 0.0
}

#[test]
fn hud_layout_holds_at_all_scales() {
    // 16:9, 21:9 and 32:9 windows from the smallest preset up, at every
    // UI scale 0.5-3 (with a 2x display on top: up to 6 pixels a point).
    let sizes = [
        (1280.0, 720.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
        (2560.0, 1440.0),
        (3840.0, 2160.0),
        (2560.0, 1080.0),
        (3440.0, 1440.0),
        (5120.0, 2160.0),
        (5120.0, 1440.0),
    ];
    let mut points = vec![];
    for &(w, h) in &sizes {
        for ppp in [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0, 6.0] {
            points.push((w / ppp, h / ppp));
        }
    }
    for (w, h) in points {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(w, h));
        let l = HudLayout::new(screen);
        let at = format!("{w}x{h} points: {l:?}");
        for (name, r) in
            [("health", l.health), ("weapons", l.weapons), ("minimap", l.minimap), ("clock", l.clock)]
        {
            assert!(screen.contains_rect(r), "{name} leaves the screen at {at}");
            assert!(r.width() >= 16.0 && r.height() >= 8.0, "{name} too small at {at}");
        }
        assert!(l.weapons.height() >= MAX_SLOTS as f32 * 8.0, "weapon rows at {at}");
        // Nothing hides the health or the weapons.
        for (name, r) in [("minimap", l.minimap), ("clock", l.clock), ("feed", l.feed)] {
            assert!(!overlap(l.health, r), "{name} over the health at {at}");
            assert!(!overlap(l.weapons, r), "{name} over the weapons at {at}");
        }
        assert!(!overlap(l.health, l.weapons), "health over weapons at {at}");
        if let Some(hint) = l.hint {
            assert!(screen.contains_rect(hint));
            assert!(
                !overlap(hint, l.health) && !overlap(hint, l.weapons) && !overlap(hint, l.minimap),
                "{at}"
            );
        }
        assert!(
            !overlap(l.minimap, l.clock) && !overlap(l.feed, l.minimap) && !overlap(l.feed, l.clock),
            "{at}"
        );
        assert!(l.feed.height() <= 0.0 || screen.contains_rect(l.feed), "feed leaves the screen at {at}");
        assert!(l.text > 0.0 && l.text.is_finite(), "{at}");
    }
    // No size panics: a window of no size (minimised), or not a number.
    for (w, h) in [(0.0, 0.0), (1.0, 1.0), (f32::NAN, 100.0), (f32::INFINITY, 1080.0)] {
        let _ = HudLayout::new(Rect::from_min_size(Pos2::ZERO, vec2(w, h)));
    }
}

/// A mod of 7 weapons; the restrictions enable 0, 2, 4 and 5.
fn menu() -> WeaponMenu {
    let names = ["Bazooka", "Minigun", "Laser", "Nuke", "Rifle", "Shotgun", "Mine"];
    WeaponMenu {
        names: names.iter().map(|s| s.to_string()).collect(),
        enabled: vec![true, false, true, false, true, true, false],
        // `findEnabledWeapon`: the first enabled by name.
        fallback: 0,
        seed: 9,
    }
}

const ENABLED: [i32; 4] = [0, 2, 4, 5];

fn state_in_weapon_select(profiles: Profiles) -> UiState {
    let mut ui = UiState::new(Settings::default(), profiles, 1);
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::StartMatch);
    ui.update(UiAction::MatchStarted);
    ui
}

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(ui: UiState, data: MenuData) -> Harness<'static, App> {
    let app = App { ui, data, commands: Vec::new() };
    Harness::builder().with_size(vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

/// The game's keys the player holds this frame (the app feeds them).
fn hold(h: &mut Harness<'static, App>, keys: u8) {
    h.state_mut().data.held = Keys(keys);
    h.run();
}

#[test]
fn weapon_selection_respects_restrictions() {
    let m = menu();
    assert!(m.needs_choice());
    // The profile's weapons by name (ignoring case); one the restrictions
    // do not enable, or the mod lacks, gets the fallback.
    let remembered = ["minigun", "LASER", "nope", "Rifle", ""].map(String::from);
    let mut s = WeaponSelect::new(&m, &remembered);
    assert_eq!(s.slots(), [0, 2, 0, 4, 0]);
    // Left and right cycle the row's slot over the enabled weapons only.
    let mut seen = vec![];
    for _ in 0..8 {
        s.change(&m, 1);
        seen.push(s.slots()[0]);
    }
    assert_eq!(seen, [2, 4, 5, 0, 2, 4, 5, 0]);
    s.change(&m, -1);
    assert_eq!(s.slots()[0], 5);
    // Select weapon held: six weapons at a time (OLX's cSelWeapon); with
    // four enabled, OLX's loop stops where it started.
    s.change(&m, 6);
    assert_eq!(s.slots()[0], 5);
    s.change(&m, 3);
    assert_eq!(s.slots()[0], 4);
    // Random: enabled weapons only.
    for _ in 0..50 {
        s.random(&m);
        assert!(s.slots().iter().all(|w| ENABLED.contains(w)), "{:?}", s.slots());
    }
    // One enabled weapon (or none): no choice to make, as OLX skips the dialog.
    let one =
        WeaponMenu { enabled: vec![false, false, true, false, false, false, false], fallback: 2, ..menu() };
    assert!(!one.needs_choice());
    assert_eq!(WeaponSelect::new(&one, &[]).slots(), [2; MAX_SLOTS]);

    // Through the screen with the game's keys (keyboard or gamepad, as the
    // app feeds them): down to a slot, right, down to Done, fire and
    // release. The choice goes to the app and into the profile.
    let ui = state_in_weapon_select(Profiles::default());
    assert_eq!(ui.screen(), Screen::WeaponSelect);
    let mut data = MenuData::new(Catalog::default(), None);
    data.weapons = Some(m.clone());
    let mut h = harness(ui, data);
    h.get_by_label("Weapons");
    h.get_by_label("Random");
    h.get_by_label("Done");
    let start = h.state().data.weapon_choice().unwrap();
    assert!(start.iter().all(|w| ENABLED.contains(w)), "{start:?}");
    hold(&mut h, Keys::DOWN);
    hold(&mut h, 0);
    hold(&mut h, Keys::RIGHT);
    hold(&mut h, 0);
    let chosen = h.state().data.weapon_choice().unwrap();
    assert_ne!(chosen[1], start[1], "the second slot changed");
    for _ in 0..5 {
        hold(&mut h, Keys::DOWN);
        hold(&mut h, 0);
    }
    hold(&mut h, Keys::FIRE);
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect, "Done on the fire key's release");
    hold(&mut h, 0);
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    assert!(h.state().commands.contains(&AppCommand::ArmPlayer(chosen)), "{:?}", h.state().commands);
    let names: Vec<String> = chosen.iter().map(|&w| m.names[w as usize].clone()).collect();
    assert_eq!(h.state().ui.profiles().selected().unwrap().weapons, names, "remembered by name");
    assert!(h.state().commands.iter().any(|c| matches!(c, AppCommand::SaveProfiles(_))));

    // The mouse: Random, then Done.
    let mut data = MenuData::new(Catalog::default(), None);
    data.weapons = Some(m.clone());
    let mut h = harness(state_in_weapon_select(Profiles::default()), data);
    h.get_by_label("Random").click();
    h.run();
    h.get_by_label("Done").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    let armed = h.state().commands.iter().find_map(|c| match c {
        AppCommand::ArmPlayer(w) => Some(*w),
        _ => None,
    });
    assert!(armed.unwrap().iter().all(|w| ENABLED.contains(w)));

    // The state passes any weapons on: refusing one the restrictions do not
    // enable is `build_match`'s (the app keeps the match's options then).
    let mut ui = state_in_weapon_select(Profiles::default());
    ui.update(UiAction::ChooseWeapons { weapons: [1; MAX_SLOTS], names: vec![] });
    assert_eq!(ui.screen(), Screen::InGame, "the app's build_match refuses it, not the menu");
}

#[test]
fn pause_menu_holds_the_game_and_opens_the_options() {
    let mut ui = state_in_weapon_select(Profiles::default());
    let c = ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec!["Bazooka".into(); 5] });
    assert!(c.contains(&AppCommand::ArmPlayer([0; MAX_SLOTS])));
    assert_eq!(ui.screen(), Screen::InGame);
    assert_eq!(ui.update(UiAction::Back), [AppCommand::SetPaused(true)]);
    // The options from the pause menu: the game stays paused, Back returns.
    assert!(ui.update(UiAction::Open(Screen::Options)).is_empty());
    assert_eq!((ui.screen(), ui.is_paused()), (Screen::Options, true));
    assert!(ui.update(UiAction::Restart).is_empty(), "no F5 in the options");
    assert!(ui.update(UiAction::Back).is_empty());
    assert_eq!((ui.screen(), ui.is_paused()), (Screen::InGame, true));
    // Other menus are not reachable from the pause menu.
    ui.update(UiAction::Open(Screen::Profiles));
    assert_eq!(ui.screen(), Screen::InGame);
    assert_eq!(ui.update(UiAction::Resume), [AppCommand::SetPaused(false)]);
    ui.update(UiAction::Open(Screen::Options));
    assert_eq!(ui.screen(), Screen::InGame, "the options only while paused");

    // Through the widgets: Esc, Options, Back, Quit to menu.
    let mut h = harness(ui, MenuData::new(Catalog::default(), None));
    h.key_press(Key::Escape);
    h.run();
    for label in ["Resume", "Options", "Quit to menu"] {
        h.get_by_label(label);
    }
    h.get_by_label("Options").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Options);
    h.get_by_label("Back").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    h.get_by_label("Quit to menu").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert_eq!(h.state().commands.last(), Some(&AppCommand::EndMatch));
}

/// A held Esc repeats: only its first press pauses (no flicker), and a
/// held F5 restarts once.
#[test]
fn esc_and_f5_ignore_key_repeat() {
    let mut ui = state_in_weapon_select(Profiles::default());
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    let mut h = harness(ui, MenuData::new(Catalog::default(), None));
    let key = |key, repeat| Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat,
        modifiers: Modifiers::NONE,
    };
    h.input_mut().events.push(key(Key::Escape, false));
    h.run();
    assert!(h.state().ui.is_paused());
    for _ in 0..5 {
        h.input_mut().events.push(key(Key::Escape, true));
        h.run();
        assert!(h.state().ui.is_paused(), "a repeat does not resume");
    }
    h.get_by_label("Resume").click();
    h.run();
    h.input_mut().events.push(key(Key::F5, false));
    h.run();
    for _ in 0..5 {
        h.input_mut().events.push(key(Key::F5, true));
        h.run();
    }
    let restarts = h.state().commands.iter().filter(|c| **c == AppCommand::Restart).count();
    assert_eq!(restarts, 1);
}

fn results() -> Results {
    let row = |worm, name: &str, kills, deaths, suicides, damage| Row {
        worm,
        name: name.into(),
        score: kills,
        lives: "∞".into(),
        kills,
        deaths,
        suicides,
        damage,
    };
    Results {
        winner: Some("Bot 2".into()),
        reason: "kill limit".into(),
        rows: vec![
            row(2, "Bot 2", 15, 3, 1, 512),
            row(0, "OpenLieroXor", 7, 9, 2, 377),
            row(1, "Bot 1", 2, 11, 0, 90),
        ],
        time_left_ms: None,
    }
}

#[test]
fn results_show_the_columns_and_their_buttons() {
    let mut ui = state_in_weapon_select(Profiles::default());
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::MatchOver);
    assert_eq!(ui.screen(), Screen::Results);
    let mut data = MenuData::new(Catalog::default(), None);
    data.results = Some(results());
    let mut h = harness(ui, data);
    h.get_by_label("Winner: Bot 2");
    for label in ["Kills", "Deaths", "Suicides", "Damage", "OpenLieroXor", "512", "377"] {
        h.get_by_label(label);
    }
    h.get_by_label("Save replay").click();
    h.run();
    assert_eq!(h.state().commands.last(), Some(&AppCommand::SaveReplay));
    h.state_mut().ui.update(UiAction::ReplaySaved(Ok("replay-001.rlrp".into())));
    h.run();
    h.get_by_label_contains("replay-001.rlrp");
    h.get_by_label("Rematch").click();
    h.run();
    let c = &h.state().commands;
    assert!(matches!(c[c.len() - 2..], [AppCommand::EndMatch, AppCommand::StartMatch(_)]), "{c:?}");
    // The rematch chooses its weapons again (OLX's selection at each game).
    h.state_mut().ui.update(UiAction::MatchStarted);
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);

    // A replay's results: watch it again, no saving.
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    ui.update(UiAction::Open(Screen::Replays));
    ui.update(UiAction::PlayReplay("r.rlrp".into()));
    ui.update(UiAction::MatchStarted);
    assert_eq!((ui.screen(), ui.game()), (Screen::InGame, Some(Game::Replay)));
    ui.update(UiAction::MatchOver);
    let mut data = MenuData::new(Catalog::default(), None);
    data.results = Some(results());
    let mut h = harness(ui, data);
    assert!(h.query_by_label("Save replay").is_none());
    h.get_by_label("Watch again").click();
    h.run();
    assert_eq!(h.state().commands.last(), Some(&AppCommand::Restart));
    assert_eq!(h.state().ui.screen(), Screen::InGame);
}

fn harness_sized(ui: UiState, data: MenuData, size: egui::Vec2) -> Harness<'static, App> {
    let app = App { ui, data, commands: Vec::new() };
    Harness::builder().with_size(size).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

/// On a large screen the results panel is as wide as its table: every
/// column shows, none hidden behind a horizontal scroll (seen at 1080p in
/// the v0.1 acceptance runs: Suicides cut, Damage gone).
#[test]
fn results_show_every_column_on_a_large_screen() {
    for size in [vec2(1920.0, 1080.0), vec2(1280.0, 720.0)] {
        let mut ui = state_in_weapon_select(Profiles::default());
        ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
        ui.update(UiAction::MatchOver);
        let mut data = MenuData::new(Catalog::default(), None);
        data.results = Some(results());
        let mut h = harness_sized(ui, data, size);
        h.run();
        let panel = h.ctx.memory(|m| m.area_rect(egui::Id::new("results"))).expect("the results panel");
        for label in ["Suicides", "Damage", "512"] {
            let r = h.get_by_label(label).rect();
            assert!(panel.contains_rect(r), "{label} at {r:?} outside the panel {panel:?} at {size:?}");
        }
        // Not wider than its table needs.
        assert!(panel.width() < 700.0, "{panel:?} at {size:?}");
    }
}

/// A 1280 × 720 window at UI scale 3 is 427 × 240 points: the results
/// of a full match and the weapon selection keep their buttons on screen
/// (the table and the slots scroll).
#[test]
fn buttons_stay_on_a_small_screen() {
    let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0 / 3.0, 720.0 / 3.0));
    let mut ui = state_in_weapon_select(Profiles::default());
    let mut data = MenuData::new(Catalog::default(), None);
    data.weapons = Some(menu());
    let mut h = harness_sized(ui.clone(), data, screen.size());
    for label in ["Random", "Done"] {
        let r = h.get_by_label(label).rect();
        assert!(screen.contains_rect(r), "{label} at {r:?}");
    }
    h.get_by_label("Done").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::InGame);

    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::MatchOver);
    let mut data = MenuData::new(Catalog::default(), None);
    let mut r = results();
    for i in 3..32 {
        r.rows.push(Row { worm: i, name: format!("Bot {i}"), ..r.rows[2].clone() });
    }
    data.results = Some(r);
    let h = harness_sized(ui, data, screen.size());
    for label in ["Rematch", "Save replay", "Back to menu"] {
        let r = h.get_by_label(label).rect();
        assert!(screen.contains_rect(r), "{label} at {r:?}");
    }
}

/// The HUD paints without a panic at every UI scale on every window size
/// (Review Focus 5; the image snapshots run in the GPU tier only).
#[test]
fn hud_paints_at_every_scale() {
    use rl_ui::hud::{Bar, Dot, HudView, MinimapView, SlotHud, WormHud};
    let slot = SlotHud { name: "Super Shotgun".into(), charge: 0.4, bar: Bar::Reloading };
    let view = HudView {
        worm: Some(WormHud {
            name: "OpenLieroXor".into(),
            health: 55.0,
            alive: false,
            lives: 2,
            kills: 3,
            deaths: 1,
            suicides: 0,
            slots: vec![slot; MAX_SLOTS],
            current: 9,
            status: Some("Dead: Jump to respawn".into()),
        }),
        feed: vec!["A killed B".into(); 8],
        time_left_ms: Some(9_000),
        kill_limit: Some(15),
        scoreboard: Some(results().rows),
        minimap: Some(MinimapView {
            texture: egui::TextureId::default(),
            map: vec2(504.0, 350.0),
            view: Some(Rect::from_min_size(Pos2::new(-50.0, 0.0), vec2(9999.0, 240.0))),
            dots: vec![Dot { pos: Pos2::new(1e9, -5.0), colour: egui::Color32::RED, big: true }],
            blink: 0.3,
        }),
        game_over: true,
        hint: Some("Tab scores   F2 debug   F5 rematch   Esc pause".into()),
        top_left_reserved: 130.0,
        ..HudView::default()
    };
    for (w, h) in [(1280.0, 720.0), (1920.0, 1080.0), (2560.0, 1440.0), (3440.0, 1440.0)] {
        for scale in [0.5, 1.0, 1.5, 2.0, 3.0] {
            let v = view.clone();
            let mut harness = Harness::builder()
                .with_size(vec2(w / scale, h / scale))
                .with_pixels_per_point(scale)
                .build_ui(move |ui| rl_ui::hud::show(ui, &v));
            harness.run();
        }
    }
}

/// Save replay keeps one file per match: once saved, the button says so
/// and asks no more.
#[test]
fn save_replay_once_per_match() {
    let mut ui = state_in_weapon_select(Profiles::default());
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::MatchOver);
    let mut data = MenuData::new(Catalog::default(), None);
    data.results = Some(results());
    let mut h = harness(ui, data);
    h.get_by_label("Save replay").click();
    h.run();
    h.state_mut().ui.update(UiAction::ReplaySaved(Ok("replay-001.rlrp".into())));
    h.run();
    assert!(h.query_by_label("Save replay").is_none());
    h.get_by_label("Saved").click();
    h.run();
    assert!(h.state_mut().ui.update(UiAction::SaveReplay).is_empty(), "saved already");
    let saves = h.state().commands.iter().filter(|c| **c == AppCommand::SaveReplay).count();
    assert_eq!(saves, 1);
    // A failed save may be tried again.
    let mut ui = state_in_weapon_select(Profiles::default());
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::MatchOver);
    assert_eq!(ui.update(UiAction::SaveReplay), [AppCommand::SaveReplay]);
    ui.update(UiAction::ReplaySaved(Err("disk full".into())));
    assert_eq!(ui.update(UiAction::SaveReplay), [AppCommand::SaveReplay]);
    // The next match saves anew.
    ui.update(UiAction::ReplaySaved(Ok("replay-001.rlrp".into())));
    ui.update(UiAction::StartMatch);
    ui.update(UiAction::MatchStarted);
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::MatchOver);
    assert_eq!(ui.update(UiAction::SaveReplay), [AppCommand::SaveReplay]);
}

/// The results' Rematch: the app ends the match (its results go) while
/// the next one loads; the screen says so.
#[test]
fn rematch_shows_loading() {
    let mut ui = state_in_weapon_select(Profiles::default());
    ui.update(UiAction::ChooseWeapons { weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::MatchOver);
    let mut data = MenuData::new(Catalog::default(), None);
    data.results = Some(results());
    let mut h = harness(ui, data);
    h.get_by_label("Rematch").click();
    h.run();
    assert!(h.state().ui.is_starting());
    h.state_mut().data.results = None; // `Shell::end`
    h.run();
    h.get_by_label("Loading…");
    assert!(h.query_by_label("Back to menu").is_none(), "nothing to press while it loads");
}

/// The HUD never hides a worm for long (the final review's Important 1):
/// a worm under the minimap or the bottom-left panel, or within a worm's
/// size of it, fades that part to `FADED` over `FADE_SECONDS`, eased, and
/// back when it leaves; with no worm near, everything is opaque. The
/// minimap's scale is capped at `MAX_MINIMAP`, so it takes less of a
/// small window.
#[test]
fn hud_fades_over_a_worm() {
    use rl_ui::hud::{Covered, FADE_SECONDS, FADED, HudFade, MAX_MINIMAP};
    for (w, h) in [(1280.0, 720.0), (1920.0, 1080.0), (2560.0, 1440.0), (640.0, 360.0)] {
        let l = HudLayout::new(Rect::from_min_size(Pos2::ZERO, vec2(w, h)));
        assert!(l.minimap_scale() <= MAX_MINIMAP + 1e-4, "{w}x{h}: {}", l.minimap_scale());
    }
    const { assert!(MAX_MINIMAP <= 1.5) };
    let l = HudLayout::new(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0)));
    let size = 20.0;
    assert_eq!(l.covered(&[], size), Covered::default(), "no worm");
    assert_eq!(l.covered(&[l.screen.center()], size), Covered::default(), "a worm mid-screen");
    let under = l.covered(&[l.minimap.center()], size);
    assert_eq!(under, Covered { minimap: true, panel: false });
    let near = l.covered(&[Pos2::new(l.minimap.left() - size, l.minimap.center().y)], size);
    assert!(near.minimap, "within a worm's size of the minimap");
    let off = l.covered(&[Pos2::new(l.minimap.left() - 3.0 * size, l.minimap.center().y)], size);
    assert!(!off.minimap, "well clear of it");
    assert_eq!(l.covered(&[l.weapons.center()], size), Covered { minimap: false, panel: true });
    assert_eq!(l.covered(&[l.health.left_top()], size), Covered { minimap: false, panel: true });

    let mut fade = HudFade::default();
    let o = fade.update(Covered::default(), 0.0);
    assert_eq!((o.minimap, o.panel), (1.0, 1.0), "no worm: full opacity");
    let o = fade.update(under, 1.0);
    assert_eq!(o.minimap, 1.0, "the fade starts now");
    let half = fade.update(under, 1.0 + FADE_SECONDS / 2.0).minimap;
    assert!(FADED < half && half < 1.0, "half way: {half}");
    let quarter = fade.update(under, 1.0 + FADE_SECONDS / 4.0).minimap;
    assert!(half < quarter, "eased, monotonic");
    let o = fade.update(under, 1.0 + FADE_SECONDS);
    assert_eq!((o.minimap, o.panel), (FADED, 1.0), "the minimap faded, the panel not");
    assert_eq!(fade.update(under, 5.0).minimap, FADED, "stays faded while the worm is under it");
    // The worm leaves half way back: the fade turns from where it is.
    fade.update(Covered::default(), 6.0);
    let back = fade.update(Covered::default(), 6.0 + FADE_SECONDS / 2.0).minimap;
    assert!(FADED < back && back < 1.0, "{back}");
    let turned = fade.update(under, 6.0 + FADE_SECONDS / 2.0).minimap;
    assert_eq!(turned, back, "no jump when it turns");
    fade.update(Covered::default(), 9.0);
    assert!(fade.moving(9.0 + FADE_SECONDS / 2.0));
    assert_eq!(fade.update(Covered::default(), 20.0).minimap, 1.0, "opaque again");
    assert!(!fade.moving(20.0));
}
