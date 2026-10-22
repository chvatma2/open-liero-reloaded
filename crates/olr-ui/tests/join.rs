//! Local play's join step and the weapon selection of several local
//! players (issue #32): fake seat inputs (each seat's actions held and
//! Start, as the app reads them from keyboard and pad events) and pad
//! events, the widgets found by label (`egui_kittest`).

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, Player};
use olr_input::{Button, DIG, PadEvent, Seat};
use olr_sim::{Keys, MAX_SLOTS};
use olr_ui::join::{JoinStep, Joined, SeatInput};
use olr_ui::pad::Who;
use olr_ui::screens::MenuData;
use olr_ui::weapon_select::WeaponMenu;
use olr_ui::{AppCommand, Profiles, Screen, Settings, UiAction, UiState};

use crate::flows::{App, harness, starts};

const K1: Seat = Seat::Keyboard1;
const K2: Seat = Seat::Keyboard2;

/// Four profiles: the default (selected) and "Player 2" to "Player 4",
/// each with weapons of its own remembered.
fn profiles() -> Profiles {
    let mut p = Profiles::default();
    for _ in 0..3 {
        p.add();
    }
    p.selected = 0;
    p
}

fn input(seat: Seat, keys: impl Into<u32>) -> SeatInput {
    SeatInput { seat, held: keys.into(), start: false }
}

/// Every seat of `seats` holding nothing but `seat`, which holds `keys`.
fn only(seats: &[Seat], seat: Seat, keys: impl Into<u32>) -> Vec<SeatInput> {
    let keys = keys.into();
    seats.iter().map(|&s| input(s, if s == seat { keys } else { 0 })).collect()
}

/// A frame of the harness with the seats' inputs `seats` (as the app sets
/// them every frame).
fn seats_frame(h: &mut Harness<'static, App>, seats: Vec<SeatInput>) {
    h.state_mut().data.set_seats(seats);
    h.run();
}

/// `seat` presses `keys` and lets go, every other seat of `all` idle.
fn tap(h: &mut Harness<'static, App>, all: &[Seat], seat: Seat, keys: impl Into<u32>) {
    seats_frame(h, only(all, seat, keys));
    seats_frame(h, only(all, seat, 0u32));
}

fn lineup(h: &Harness<'static, App>) -> Vec<Joined> {
    h.state().ui.lineup().to_vec()
}

/// Local play's Start opens the join step only once two seats were used
/// this run: the solo player sees nothing new.
#[test]
fn the_join_step_shows_only_when_several_seats_were_used() {
    let mut h = harness(UiState::new(Settings::default(), profiles(), 7));
    h.get_by_label("Local Play").click();
    h.run();
    // Keyboard 1 alone (and the mouse): Start starts at once, as before.
    tap(&mut h, &[K1, K2], K1, Keys::RIGHT);
    assert!(h.query_by_label(olr_ui::screens::JOIN_HINT).is_none());
    h.get_by_label("Start").click();
    h.run();
    assert!(h.state().ui.is_starting());
    assert_eq!(starts(h.state())[0].players.iter().filter(|p| matches!(p, Player::Human)).count(), 1);

    // Keyboard 2 pressed something too: Start opens the join step,
    // keyboard 1 (the mouse's seat) in.
    let mut h = harness(UiState::new(Settings::default(), profiles(), 7));
    h.get_by_label("Local Play").click();
    h.run();
    tap(&mut h, &[K1, K2], K1, Keys::RIGHT);
    tap(&mut h, &[K1, K2], K2, Keys::FIRE);
    h.get_by_label(olr_ui::screens::JOIN_HINT);
    h.get_by_label("Start").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Join);
    assert!(starts(h.state()).is_empty());
    assert_eq!(lineup(&h), [Joined { seat: K1, profile: Some(0), ready: false }]);
    // Back: local play again, nobody joined.
    h.get_by_label("Back").click();
    h.run();
    assert_eq!((h.state().ui.screen(), h.state().ui.lineup().len()), (Screen::LocalPlay, 0));
}

/// A pad's A on local play's Start: that pad's seat is the one in.
#[test]
fn the_pad_that_pressed_start_is_in() {
    let mut h = harness(UiState::new(Settings::default(), profiles(), 7));
    h.state_mut().ui.update(UiAction::Open(Screen::LocalPlay));
    h.run();
    tap(&mut h, &[K1, K2, Seat::Pad(3)], K2, Keys::LEFT);
    tap(&mut h, &[K1, K2, Seat::Pad(3)], Seat::Pad(3), Keys::JUMP);
    // Pad 3 moves the focus to Start and presses A.
    let mut focused = false;
    for _ in 0..40 {
        let f = h.query_by(|n| n.is_focused()).and_then(|n| n.accesskit_node().label());
        if f.as_deref() == Some("Start") {
            focused = true;
            break;
        }
        pad_press(&mut h, 3, Button::DPadDown);
    }
    assert!(focused, "Start never had the focus");
    pad_press(&mut h, 3, Button::South);
    assert_eq!(h.state().ui.screen(), Screen::Join);
    assert_eq!(lineup(&h)[0].seat, Seat::Pad(3));
}

fn pad_press(h: &mut Harness<'static, App>, pad: usize, b: Button) {
    for v in [1.0, 0.0] {
        h.state_mut().data.pad.on_pad(pad, PadEvent::Button(b, v));
    }
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    let events = app.data.pad_events(&ctx, &app.ui);
    h.input_mut().events.extend(events);
    h.run_steps(12);
}

/// The join screen, keyboard 1 in (Start pressed with the mouse).
fn at_join(seats: &[Seat]) -> Harness<'static, App> {
    let mut ui = UiState::new(Settings::default(), profiles(), 7);
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::OpenJoin(K1));
    let mut h = harness(ui);
    // The first frame notes what is held.
    seats_frame(&mut h, only(seats, K1, 0u32));
    h
}

/// 2, 3 and 4 seats join with a button of their own, each picks a profile
/// with its own left and right, readies, and the match starts with a human
/// each (keyboard 1 first, keyboard 2, then the pads as they joined)
/// before the bots; no seat's press changes another's card.
#[test]
fn two_three_and_four_seats_join_pick_profiles_and_start() {
    let pads = [Seat::Pad(5), Seat::Pad(2)];
    for n in 2..=4 {
        let all: Vec<Seat> = [K1, K2].into_iter().chain(pads).collect();
        let mut h = at_join(&all);
        assert_eq!(h.query_all_by_label(olr_ui::screens::PRESS_TO_JOIN).count(), 3);
        // Join in this order: pad 5, keyboard 2, pad 2 (as many as n asks).
        let joining = [Seat::Pad(5), K2, Seat::Pad(2)];
        for &s in &joining[..n - 1] {
            tap(&mut h, &all, s, Keys::FIRE);
        }
        let seats: Vec<Seat> = lineup(&h).iter().map(|j| j.seat).collect();
        let want: Vec<Seat> = [K1, K2, Seat::Pad(5), Seat::Pad(2)]
            .into_iter()
            .filter(|s| *s == K1 || joining[..n - 1].contains(s))
            .collect();
        assert_eq!(seats, want, "keyboard 1, keyboard 2, then the pads as they joined");
        // Every player a profile of their own, keyboard 1 the selected one.
        let profiles: Vec<Option<usize>> = lineup(&h).iter().map(|j| j.profile).collect();
        assert_eq!(profiles[0], Some(0));
        for (i, p) in profiles.iter().enumerate() {
            assert!(p.is_some() && !profiles[..i].contains(p), "{profiles:?}");
        }
        // The last to join steps its profile right, past the others'.
        let last = *joining[..n - 1].last().unwrap();
        let before = lineup(&h).clone();
        tap(&mut h, &all, last, Keys::RIGHT);
        let after = lineup(&h);
        for (b, a) in before.iter().zip(&after) {
            if a.seat == last {
                assert_ne!(a.profile, b.profile, "its own profile changed");
                let others: Vec<_> = after.iter().filter(|j| j.seat != last).map(|j| j.profile).collect();
                assert!(!others.contains(&a.profile), "never another's");
            } else {
                assert_eq!(a, b, "another's card is untouched");
            }
        }
        // Everyone ready but one: no start; its press starts the match.
        for j in &after[1..] {
            tap(&mut h, &all, j.seat, Keys::JUMP);
        }
        assert!(starts(h.state()).is_empty());
        assert!(h.state().ui.lineup()[1..].iter().all(|j| j.ready));
        tap(&mut h, &all, K1, Keys::FIRE);
        let s = starts(h.state());
        assert_eq!(s.len(), 1, "{n}");
        let humans = s[0].players.iter().take_while(|p| matches!(p, Player::Human)).count();
        assert_eq!(humans, n);
        assert!(s[0].players[n..].iter().all(|p| matches!(p, Player::Bot(_))));
        assert_eq!(s[0].players.len(), n + usize::from(Settings::default().gameplay.match_prefs.bots));
    }
}

/// Leaving: Rope or Dig (a ready player is first not ready); a pad gone
/// leaves; the mouse is keyboard 1's card's alone.
#[test]
fn players_leave_and_the_mouse_acts_on_keyboard_1_alone() {
    let all = [K1, K2, Seat::Pad(0)];
    let mut h = at_join(&all);
    tap(&mut h, &all, K2, Keys::LEFT);
    tap(&mut h, &all, Seat::Pad(0), Keys::RIGHT);
    assert_eq!(lineup(&h).len(), 3);
    tap(&mut h, &all, K2, Keys::FIRE);
    assert!(lineup(&h)[1].ready);
    tap(&mut h, &all, K2, DIG);
    assert_eq!((lineup(&h).len(), lineup(&h)[1].ready), (3, false), "first not ready");
    tap(&mut h, &all, K2, Keys::ROPE);
    assert_eq!(lineup(&h).iter().map(|j| j.seat).collect::<Vec<_>>(), [K1, Seat::Pad(0)]);
    // The mouse: one "Leave", keyboard 1's.
    assert_eq!(h.query_all_by_role_and_label(Role::Button, "Leave").count(), 1);
    h.get_by_role_and_label(Role::Button, "Leave").click();
    h.run();
    assert_eq!(lineup(&h).iter().map(|j| j.seat).collect::<Vec<_>>(), [Seat::Pad(0)]);
    h.get_by_label(olr_ui::screens::JOIN_KEYBOARD_1).click();
    h.run();
    assert_eq!(lineup(&h)[0].seat, K1, "keyboard 1 first again");
    // Pad 0 unplugged: its player leaves.
    seats_frame(&mut h, vec![input(K1, 0u32), input(K2, 0u32)]);
    assert_eq!(lineup(&h).iter().map(|j| j.seat).collect::<Vec<_>>(), [K1]);
    // Join's Start: the players joined, ready or not.
    h.get_by_label("Start").click();
    h.run();
    assert_eq!(starts(h.state()).len(), 1);
}

/// A pad's Start joins and readies, as a button does; held when the step
/// opens, it is not a press.
#[test]
fn start_joins_and_readies_and_a_held_button_is_no_press() {
    let mut step = JoinStep::default();
    let pad = Seat::Pad(1);
    let held = [SeatInput { seat: pad, held: Keys::JUMP.into(), start: true }];
    assert!(step.on_seats(&[], &held).is_empty(), "held as it opened");
    let free = [SeatInput { seat: pad, held: 0, start: false }];
    assert!(step.on_seats(&[], &free).is_empty());
    let start = [SeatInput { seat: pad, held: 0, start: true }];
    assert_eq!(step.on_seats(&[], &start), [UiAction::JoinSeat(pad)]);
    let joined = [Joined { seat: pad, profile: None, ready: false }];
    step.on_seats(&joined, &free);
    assert_eq!(step.on_seats(&joined, &start), [UiAction::SetReady { seat: pad, ready: true }]);
}

/// A match of three local players: each chooses at once in their own
/// panel with their own keys (none moves another's), the mouse picks in
/// keyboard 1's alone, a player done waits; when all are, every player is
/// armed at once and each profile remembers its own weapons.
#[test]
fn every_player_selects_weapons_at_once() {
    let mut ui = UiState::new(Settings::default(), profiles(), 1);
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::OpenJoin(K1));
    ui.update(UiAction::JoinSeat(Seat::Pad(4)));
    ui.update(UiAction::JoinSeat(K2));
    ui.update(UiAction::StartMatch);
    ui.update(UiAction::MatchStarted);
    assert_eq!((ui.screen(), ui.local_players()), (Screen::WeaponSelect, 3));
    let names = ["Bazooka", "Minigun", "Laser", "Nuke", "Rifle", "Shotgun", "Mine"];
    let menu = WeaponMenu {
        names: names.iter().map(|s| s.to_string()).collect(),
        enabled: vec![true; 7],
        fallback: 0,
        seed: 3,
        unique: false,
    };
    let mut data = MenuData::new(Catalog::default(), None);
    data.weapons = Some(menu.clone());
    let app = App { ui, data, commands: Vec::new() };
    let mut h = Harness::builder().with_size(egui::vec2(1920.0, 1080.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in olr_ui::screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    );
    let hold = |h: &mut Harness<'static, App>, keys: [u16; 3]| {
        h.state_mut().data.held_by = keys.map(Keys).to_vec();
        h.run();
    };
    hold(&mut h, [0; 3]);
    // Each player's name heads their panel; Random and Done are buttons in
    // keyboard 1's (player 1's) alone.
    for k in 0..3 {
        h.get_by_label(&h.state().ui.player_name(k));
    }
    assert_eq!(h.query_all_by_role_and_label(Role::Button, "Random").count(), 1);
    assert_eq!(h.query_all_by_label("Done").count(), 3);
    let start: Vec<[i32; MAX_SLOTS]> = (0..3).map(|k| h.state().data.weapon_choice_of(k).unwrap()).collect();
    // Player 3 (keyboard 2) steps its first slot: only its own changes.
    hold(&mut h, [0, 0, Keys::RIGHT]);
    hold(&mut h, [0; 3]);
    let now: Vec<[i32; MAX_SLOTS]> = (0..3).map(|k| h.state().data.weapon_choice_of(k).unwrap()).collect();
    assert_eq!((now[0], now[1]), (start[0], start[1]));
    assert_ne!(now[2][0], start[2][0]);
    // Player 2 (pad 4) goes to Done and fires: it waits for the others.
    for _ in 0..6 {
        hold(&mut h, [0, Keys::DOWN, 0]);
        hold(&mut h, [0; 3]);
    }
    hold(&mut h, [0, Keys::FIRE, 0]);
    hold(&mut h, [0; 3]);
    assert!(h.state().ui.has_chosen(1));
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);
    h.run();
    h.get_by_label(olr_ui::screens::WAITING);
    // Player 1 with the mouse, player 3 with its keys.
    h.get_by_role_and_label(Role::Button, "Done").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);
    // Up from the first row wraps to Done.
    hold(&mut h, [0, 0, Keys::UP]);
    hold(&mut h, [0; 3]);
    hold(&mut h, [0, 0, Keys::FIRE]);
    hold(&mut h, [0; 3]);
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    let armed = h.state().commands.iter().find_map(|c| match c {
        AppCommand::ArmPlayers(w) => Some(w.clone()),
        _ => None,
    });
    assert_eq!(armed.unwrap(), [now[0], start[1], now[2]]);
    assert!(!h.state().commands.iter().any(|c| matches!(c, AppCommand::ArmPlayer(_))));
    // Each profile remembers its own player's weapons.
    let ui = &h.state().ui;
    for (k, w) in [now[0], start[1], now[2]].iter().enumerate() {
        let p = ui.lineup()[k].profile.unwrap();
        let names: Vec<String> = w.iter().map(|&i| menu.names[i as usize].clone()).collect();
        assert_eq!(ui.profiles().list[p].weapons, names, "player {k}");
    }
}

/// In a match of several, only a joined pad's Start is the pause's: an
/// unjoined pad's press is nobody's, and the pause menu hears the joined
/// pad that paused.
#[test]
fn the_pause_menu_hears_the_joined_pad_that_paused() {
    let mut ui = UiState::new(Settings::default(), profiles(), 1);
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::OpenJoin(Seat::Pad(2)));
    ui.update(UiAction::JoinSeat(Seat::Pad(6)));
    ui.update(UiAction::StartMatch);
    ui.update(UiAction::MatchStarted);
    ui.update(UiAction::ChooseWeaponsOf { player: 0, weapons: [0; MAX_SLOTS], names: vec![] });
    ui.update(UiAction::ChooseWeaponsOf { player: 1, weapons: [0; MAX_SLOTS], names: vec![] });
    assert_eq!(ui.screen(), Screen::InGame);
    let mut data = MenuData::new(Catalog::default(), None);
    data.pad.set_players(Some(vec![2, 6]));
    assert_eq!(data.pad.who(&ui), Who::Nobody);
    data.pad.on_pad(9, PadEvent::Button(Button::Start, 1.0));
    data.pad.on_pad(6, PadEvent::Button(Button::Start, 1.0));
    data.pad.on_pad(9, PadEvent::Button(Button::Start, 0.0));
    data.pad.on_pad(9, PadEvent::Button(Button::Start, 1.0));
    // The app pauses on the joined pad's Start (its seat's `take_pause`).
    ui.update(UiAction::PadPause);
    assert!(ui.is_paused());
    assert_eq!(data.pad.who(&ui), Who::Only(6));
}

/// A pad plugged in again as another pad keeps its player (the app's
/// `Reseat`), never taking a seat another player has.
#[test]
fn a_reseated_pad_keeps_its_player() {
    let mut ui = UiState::new(Settings::default(), profiles(), 1);
    ui.update(UiAction::Open(Screen::LocalPlay));
    ui.update(UiAction::OpenJoin(Seat::Pad(0)));
    ui.update(UiAction::JoinSeat(Seat::Pad(1)));
    let before = ui.lineup().to_vec();
    ui.update(UiAction::Reseat { from: Seat::Pad(0), to: Seat::Pad(1) });
    assert_eq!(ui.lineup(), before, "pad 1 is another player's");
    ui.update(UiAction::Reseat { from: Seat::Pad(0), to: Seat::Pad(7) });
    assert_eq!(ui.lineup()[0], Joined { seat: Seat::Pad(7), ..before[0] });
}
