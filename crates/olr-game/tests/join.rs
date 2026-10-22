//! Joining a local split-screen game (issue #32), through the `Shell`
//! and the menus without a window, with fake key and pad events as the
//! app feeds them (`players::LocalInputs`): 2, 3 and 4 seats join local
//! play by a button of their own, pick profiles, choose their weapons
//! together, play each from their own seat, pause from a pad, and a pad
//! unplugged and plugged in again under another id takes its worm back.
//! Needs the imported content (`OLR_CONTENT_DIR`).

use std::path::{Path, PathBuf};

use olr_audio::Audio;
use olr_game::players::LocalInputs;
use olr_game::shell::{Shell, ShellConfig};
use olr_input::{Button, KeyCode, PadEvent, Seat};
use olr_scene::{Interpolation, Viewport};
use olr_sim::Keys;
use olr_ui::pad::Who;
use olr_ui::{Profile, Profiles, Screen, Settings, Store, UiAction, UiState};

fn content() -> Option<PathBuf> {
    let dir = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("OLR_CONTENT_DIR not set; skipping");
    }
    dir
}

/// Four profiles, each a name, colour and skin of its own.
fn profiles() -> Profiles {
    let p = |name: &str, colour: [u8; 3], skin: &str| Profile {
        name: name.into(),
        colour,
        skin_id: skin.into(),
        ..Profile::default()
    };
    Profiles {
        list: vec![
            p("Ann", [250, 10, 10], "default"),
            p("Bert", [0, 240, 0], "olx-skin-ant"),
            p("Cleo", [20, 20, 250], "default"),
            p("Dan", [250, 250, 0], "olx-skin-ant"),
        ],
        selected: 0,
    }
}

fn shell(content: &Path) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let mut settings = Settings::default();
    settings.gameplay.match_prefs.bots = 2;
    Shell::new(cfg, UiState::new(settings, profiles(), 3), Store::in_memory(), Audio::silent())
}

/// The app's frame: a pad's reseat and pause first (as its event loop
/// does them), then the inputs fitted to the players' seats, the menus fed
/// every seat's and player's keys, drawn and their actions carried out; a
/// start loaded.
fn frame(sh: &mut Shell, inputs: &mut LocalInputs, ctx: &egui::Context) {
    for (from, to) in inputs.take_reseated() {
        sh.act(UiAction::Reseat { from, to });
    }
    if inputs.take_pause() {
        sh.act(UiAction::PadPause);
    }
    fit(sh, inputs);
    sh.data.held = inputs.first().held();
    sh.data.held_by = inputs.held();
    sh.data.set_seats(inputs.menu_seats());
    let seats = sh.local_seats();
    let pads = seats.iter().filter_map(|s| if let Seat::Pad(p) = s { Some(*p) } else { None }).collect();
    sh.data.pad.set_players((!seats.is_empty()).then_some(pads));
    let input = egui::RawInput { events: sh.data.pad_events(ctx, &sh.ui), ..Default::default() };
    let mut actions = Vec::new();
    let mut out = ctx.run_ui(input, |ui| actions = olr_ui::screens::show(ui, &sh.ui, &mut sh.data));
    // No renderer here: the textures go nowhere.
    out.textures_delta.clear();
    for a in actions {
        sh.act(a);
    }
    sh.poll();
}

fn fit(sh: &Shell, inputs: &mut LocalInputs) {
    let n = sh.session().map_or(1, |s| s.locals().len());
    inputs.fit_to(n, &sh.local_seats());
}

/// One tick, as the app's: every player's keys latched and played.
fn tick(sh: &mut Shell, inputs: &mut LocalInputs) -> Vec<Keys> {
    fit(sh, inputs);
    let first = inputs.first_mut().latch_tick();
    let keys = inputs.latch(first);
    sh.tick_locals(&keys);
    keys
}

/// A seat's device: keyboard 1's and keyboard 2's keys (OLX's players 1
/// and 2), a pad's buttons.
#[derive(Clone, Copy, Debug)]
enum Press {
    Fire,
    Jump,
    Right,
    Up,
}

fn device(inputs: &mut LocalInputs, seat: Seat, p: Press, down: bool) {
    let key = |k1: KeyCode, k2: KeyCode| if seat == Seat::Keyboard1 { k1 } else { k2 };
    match seat {
        Seat::Pad(id) => {
            let b = match p {
                Press::Fire => Button::RightTrigger2,
                Press::Jump => Button::South,
                Press::Right => Button::DPadRight,
                Press::Up => Button::DPadUp,
            };
            inputs.on_pad(id, PadEvent::Button(b, if down { 1.0 } else { 0.0 }));
        }
        _ => {
            let k = match p {
                Press::Fire => key(KeyCode::ControlLeft, KeyCode::NumpadAdd),
                Press::Jump => key(KeyCode::AltLeft, KeyCode::NumpadEnter),
                Press::Right => key(KeyCode::ArrowRight, KeyCode::Numpad6),
                Press::Up => key(KeyCode::ArrowUp, KeyCode::Numpad8),
            };
            inputs.on_key(k, down);
        }
    }
}

/// `seat` presses `p` for a frame and lets go for another.
fn tap(sh: &mut Shell, inputs: &mut LocalInputs, ctx: &egui::Context, seat: Seat, p: Press) {
    device(inputs, seat, p, true);
    frame(sh, inputs, ctx);
    device(inputs, seat, p, false);
    frame(sh, inputs, ctx);
}

/// The worms' positions, by slot.
fn positions(sh: &Shell) -> Vec<String> {
    sh.session().unwrap().world.worms().iter().map(|w| format!("{:?}", w.pos)).collect()
}

#[test]
fn seats_join_choose_play_pause_and_come_back() {
    let Some(dir) = content() else { return };
    // Player order: keyboard 1, keyboard 2, then the pads as they joined.
    let seats = [Seat::Keyboard1, Seat::Keyboard2, Seat::Pad(4), Seat::Pad(1)];
    for n in 2..=4 {
        let mut sh = shell(&dir);
        let mut inputs = LocalInputs::default();
        let ctx = egui::Context::default();
        sh.act(UiAction::Open(Screen::LocalPlay));
        frame(&mut sh, &mut inputs, &ctx);
        // Pad 4 is used in the menus; keyboard 2 presses a key: two seats.
        tap(&mut sh, &mut inputs, &ctx, Seat::Pad(4), Press::Up);
        tap(&mut sh, &mut inputs, &ctx, Seat::Keyboard2, Press::Up);
        assert!(sh.data.seats_used.len() >= 2);
        // Pad 4's A on Start (the menus' own path is olr-ui's tests).
        sh.act(UiAction::OpenJoin(Seat::Pad(4)));
        frame(&mut sh, &mut inputs, &ctx);
        assert_eq!(sh.ui.screen(), Screen::Join);
        // The others join with a button of their own, in turn.
        let joining: Vec<Seat> =
            [Seat::Keyboard2, Seat::Pad(1), Seat::Keyboard1].into_iter().take(n - 1).collect();
        for &s in &joining {
            tap(&mut sh, &mut inputs, &ctx, s, Press::Jump);
        }
        let want: Vec<Seat> =
            seats.iter().copied().filter(|s| *s == Seat::Pad(4) || joining.contains(s)).collect();
        let lineup: Vec<Seat> = sh.ui.lineup().iter().map(|j| j.seat).collect();
        assert_eq!(lineup, want, "{n}");
        // Each steps its profile once with its own right: its own changes.
        for &s in &want {
            let before = sh.ui.lineup().to_vec();
            tap(&mut sh, &mut inputs, &ctx, s, Press::Right);
            for (b, a) in before.iter().zip(sh.ui.lineup()) {
                assert_eq!(a.profile != b.profile, a.seat == s, "{s:?}: {before:?} -> {:?}", sh.ui.lineup());
            }
        }
        let names: Vec<String> = (0..n).map(|k| sh.ui.player_name(k)).collect();
        // Everyone ready: the match loads with a worm each before the bots.
        for &s in &want {
            assert!(sh.play.is_none());
            tap(&mut sh, &mut inputs, &ctx, s, Press::Fire);
        }
        assert_eq!(sh.ui.screen(), Screen::WeaponSelect, "{:?}", sh.ui.message());
        let s = sh.session().unwrap();
        assert_eq!(s.locals(), (0..n as u8).collect::<Vec<_>>());
        assert_eq!(s.world.worms().len(), n + 2);
        let a = &sh.play.as_ref().unwrap().assets;
        // Each player's profile (none: "Player N" in the default look).
        let looks: Vec<String> = a
            .locals
            .iter()
            .enumerate()
            .map(|(k, l)| l.look.as_ref().map_or_else(|| format!("Player {}", k + 1), |l| l.name.clone()))
            .collect();
        assert_eq!(looks, names, "each player's profile");
        for (l, j) in a.locals.iter().zip(sh.ui.lineup()) {
            let skin = j.profile.map_or("olx-skin-default", |p| ["olx-skin-default", "olx-skin-ant"][p % 2]);
            assert_eq!(l.skin_id, skin);
        }

        // Weapons together: each player to Done (Up wraps there) and Fire,
        // the last one's Done arms everyone at once.
        let menu = sh.data.weapons.clone().unwrap();
        let start: Vec<_> = (0..n).map(|k| sh.data.weapon_choice_of(k).unwrap()).collect();
        for (k, &s) in want.iter().enumerate() {
            assert_eq!(sh.ui.screen(), Screen::WeaponSelect);
            tap(&mut sh, &mut inputs, &ctx, s, Press::Up);
            tap(&mut sh, &mut inputs, &ctx, s, Press::Fire);
            assert!(sh.ui.has_chosen(k) || sh.ui.screen() == Screen::InGame, "{k}");
        }
        assert_eq!(sh.ui.screen(), Screen::InGame);
        let world = &sh.session().unwrap().world;
        for (k, w) in start.iter().enumerate() {
            let held: Vec<i32> = world.worms()[k].slots.iter().map(|sl| sl.weapon).collect();
            assert!(menu.needs_choice());
            assert_eq!(held, w, "player {k} holds its own choice");
        }

        // Play: the last player walks right, and nobody else's keys move.
        let k = n - 1;
        let s = want[k];
        device(&mut inputs, s, Press::Right, true);
        for _ in 0..30 {
            let keys = tick(&mut sh, &mut inputs);
            for (i, key) in keys.iter().enumerate() {
                assert_eq!(key.0 & Keys::RIGHT != 0, i == k, "{keys:?}");
            }
        }
        device(&mut inputs, s, Press::Right, false);
        // The release is a tick's (the latch loses no press or release).
        tick(&mut sh, &mut inputs);
        frame(&mut sh, &mut inputs, &ctx);

        // Pause from a joined pad; the pause menu hears that pad alone.
        inputs.on_pad(1, PadEvent::Button(Button::Start, 1.0));
        sh.data.pad.on_pad(1, PadEvent::Button(Button::Start, 1.0));
        frame(&mut sh, &mut inputs, &ctx);
        let pad1 = want.contains(&Seat::Pad(1));
        assert_eq!(sh.paused(), pad1, "pad 1 pauses when it joined");
        if pad1 {
            assert_eq!(sh.data.pad.who(&sh.ui), Who::Only(1));
            sh.act(UiAction::Resume);
        }
        inputs.on_pad(1, PadEvent::Button(Button::Start, 0.0));
        sh.data.pad.on_pad(1, PadEvent::Button(Button::Start, 0.0));
        frame(&mut sh, &mut inputs, &ctx);
        assert!(!sh.paused());
        // Pad 4 (player 1) pauses too.
        inputs.on_pad(4, PadEvent::Button(Button::Start, 1.0));
        frame(&mut sh, &mut inputs, &ctx);
        assert!(sh.paused());
        sh.act(UiAction::Resume);
        inputs.on_pad(4, PadEvent::Button(Button::Start, 0.0));

        // Pad 4 unplugged while its player walks: the worm stands still.
        let p4 = want.iter().position(|s| *s == Seat::Pad(4)).unwrap();
        device(&mut inputs, Seat::Pad(4), Press::Right, true);
        assert_eq!(tick(&mut sh, &mut inputs)[p4], Keys(Keys::RIGHT));
        inputs.on_pad(4, PadEvent::Disconnected);
        for _ in 0..5 {
            assert_eq!(tick(&mut sh, &mut inputs)[p4], Keys(0), "stands still");
        }
        // Plugged in again as pad 9: its first button claims the seat and
        // the next presses play its worm; the menus' lineup follows.
        inputs.on_pad(9, PadEvent::Button(Button::DPadRight, 1.0));
        frame(&mut sh, &mut inputs, &ctx);
        assert_eq!(sh.ui.lineup()[p4].seat, Seat::Pad(9));
        assert_eq!(sh.local_seats()[p4], Seat::Pad(9));
        assert_eq!(tick(&mut sh, &mut inputs)[p4], Keys(0), "the claim presses nothing");
        inputs.on_pad(9, PadEvent::Button(Button::DPadRight, 0.0));
        inputs.on_pad(9, PadEvent::Button(Button::DPadRight, 1.0));
        let before = positions(&sh);
        let mut walked = false;
        for _ in 0..60 {
            assert_eq!(tick(&mut sh, &mut inputs)[p4], Keys(Keys::RIGHT));
            walked |= positions(&sh)[p4] != before[p4];
        }
        assert!(walked, "player {p4}'s worm plays from its new pad");
        // F5's rematch keeps the players on their seats, the new pad too.
        sh.act(UiAction::Restart);
        frame(&mut sh, &mut inputs, &ctx);
        assert_eq!(sh.local_seats()[p4], Seat::Pad(9));
        assert_eq!(tick(&mut sh, &mut inputs)[p4], Keys(Keys::RIGHT));
    }
}
