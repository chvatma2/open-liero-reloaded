//! The Dig action (issue #23): the dig gesture of the keys, pressed for
//! the player, once per press and every 0.2 s while held.

use std::path::Path;

use olr_content::{Content, SandboxSetup, SpawnSpec};
use olr_input::{DIG_REPEAT_TICKS, InputState, KeyCode};
use olr_sim::{Keys, SimSettings, TickEvents, World};

const L: u16 = Keys::LEFT;
const R: u16 = Keys::RIGHT;
const DIG: KeyCode = KeyCode::KeyC;

fn tap(s: &mut InputState, k: KeyCode) {
    s.on_key(k, true);
    s.on_key(k, false);
}

#[test]
fn dig_presses_the_other_direction_for_one_tick() {
    let mut s = InputState::default();
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(), Keys(L));
    tap(&mut s, DIG);
    assert_eq!(s.latch_tick(), Keys(L | R));
    assert_eq!(s.latch_tick(), Keys(L));

    // The same as the gesture: Left held, Right tapped.
    let mut g = InputState::default();
    g.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(g.latch_tick(), Keys(L));
    tap(&mut g, KeyCode::ArrowRight);
    assert_eq!(g.latch_tick(), Keys(L | R));
    assert_eq!(g.latch_tick(), Keys(L));

    // Right held: Left.
    let mut s = InputState::default();
    s.on_key(KeyCode::ArrowRight, true);
    s.on_key(DIG, true);
    assert_eq!(s.latch_tick(), Keys(L | R));
    assert_eq!(s.latch_tick(), Keys(R));
}

#[test]
fn dig_with_no_direction_presses_both() {
    let mut s = InputState::default();
    tap(&mut s, DIG);
    assert_eq!(s.latch_tick(), Keys(L | R));
    assert_eq!(s.latch_tick(), Keys(0));
    assert_eq!(s.latch_tick(), Keys(0), "a tap digs once");
}

#[test]
fn held_dig_digs_every_twenty_ticks() {
    assert_eq!(DIG_REPEAT_TICKS, 20, "0.2 s at 100 Hz: OLX's auto-dig");
    let mut s = InputState::default();
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(DIG, true);
    let digs: Vec<u32> = (0..65).filter(|_| s.latch_tick().0 & R != 0).collect();
    assert_eq!(digs, [0, 20, 40, 60]);
    // Released: no more digs; pressed again, at once.
    s.on_key(DIG, false);
    assert!((0..30).all(|_| s.latch_tick() == Keys(L)));
    s.on_key(DIG, true);
    assert_eq!(s.latch_tick(), Keys(L | R));
}

#[test]
fn dig_waits_a_tick_for_a_key_just_released() {
    // Right was down last tick: the dig presses it the tick after its release.
    let mut s = InputState::default();
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::ArrowRight, true);
    assert_eq!(s.latch_tick(), Keys(L | R));
    s.on_key(KeyCode::ArrowRight, false);
    tap(&mut s, DIG);
    assert_eq!(s.latch_tick(), Keys(L));
    assert_eq!(s.latch_tick(), Keys(L | R));
    assert_eq!(s.latch_tick(), Keys(L));
}

#[test]
fn a_press_after_a_dig_is_seen_as_a_press() {
    let mut s = InputState::default();
    s.on_key(KeyCode::ArrowLeft, true);
    tap(&mut s, DIG);
    assert_eq!(s.latch_tick(), Keys(L | R));
    // The player presses Right right after: released for a tick first.
    s.on_key(KeyCode::ArrowRight, true);
    assert_eq!(s.latch_tick(), Keys(L));
    assert_eq!(s.latch_tick(), Keys(L | R));
}

#[test]
fn dig_does_nothing_where_the_gesture_cannot_dig() {
    // Both directions held: no press is left to make.
    let mut s = InputState::default();
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::ArrowRight, true);
    tap(&mut s, DIG);
    assert_eq!(s.latch_tick(), Keys(L | R));
    s.on_key(KeyCode::ArrowRight, false);
    assert_eq!(s.latch_tick(), Keys(L));
    assert_eq!(s.latch_tick(), Keys(L), "the dig is not kept for later");

    // SelectWeapon held: Left and Right change the weapon.
    let mut s = InputState::default();
    s.on_key(KeyCode::ShiftLeft, true);
    tap(&mut s, DIG);
    assert_eq!(s.latch_tick(), Keys(Keys::CHANGE));
    s.on_key(KeyCode::ShiftLeft, false);
    assert_eq!(s.latch_tick(), Keys(0));
}

#[test]
fn the_dig_bit_never_reaches_the_simulation() {
    let mut s = InputState::default();
    s.on_key(DIG, true);
    assert_eq!(s.held(), Keys(0));
    for _ in 0..50 {
        assert_eq!(s.latch_tick().0 & !Keys::ALL, 0);
    }
}

/// A Classic match on Dirt Level, its worm in the dirt, or `None` without
/// the imported content.
fn dirt_world() -> Option<World> {
    let Ok(dir) = std::env::var("OLR_CONTENT_DIR") else {
        eprintln!("OLR_CONTENT_DIR is not set; skipping");
        return None;
    };
    let content = Content::open(Path::new(&dir)).unwrap();
    let setup = SandboxSetup {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-dirt-level".into(),
        seed: 7,
        settings: SimSettings::lx56(),
        worms: vec![SpawnSpec::default()],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    Some(content.new_world(&setup).unwrap())
}

/// Plays `ticks` ticks of `world`, `events(tick, input)` feeding the
/// input before each; the checksum after every tick.
fn play(world: &mut World, ticks: u32, events: impl Fn(u32, &mut InputState)) -> Vec<u64> {
    let mut input = InputState::default();
    let mut out = TickEvents::new();
    (0..ticks)
        .map(|t| {
            events(t, &mut input);
            world.tick(&[input.latch_tick()], &mut out);
            world.checksum()
        })
        .collect()
}

#[test]
fn dig_digs_exactly_as_the_gesture() {
    let Some(start) = dirt_world() else { return };
    // Walk left, dig five times (held Left, Right tapped / Dig tapped),
    // then turn right and dig with Dig held.
    let digs = |t: u32| (50..300).contains(&t) && t.is_multiple_of(50);
    let gesture = |t: u32, s: &mut InputState| {
        if t == 0 {
            s.on_key(KeyCode::ArrowLeft, true);
        }
        if digs(t) {
            tap(s, KeyCode::ArrowRight);
        }
        if t == 300 {
            s.on_key(KeyCode::ArrowLeft, false);
            s.on_key(KeyCode::ArrowRight, true);
        }
        if t >= 310 && (t - 310).is_multiple_of(DIG_REPEAT_TICKS) {
            tap(s, KeyCode::ArrowLeft);
        }
    };
    let dig = |t: u32, s: &mut InputState| {
        if t == 0 {
            s.on_key(KeyCode::ArrowLeft, true);
        }
        if digs(t) {
            tap(s, DIG);
        }
        if t == 300 {
            s.on_key(KeyCode::ArrowLeft, false);
            s.on_key(KeyCode::ArrowRight, true);
        }
        if t == 310 {
            s.on_key(DIG, true);
        }
    };
    let (mut a, mut b) = (start.clone(), start.clone());
    let sums_a = play(&mut a, 500, gesture);
    let sums_b = play(&mut b, 500, dig);
    assert_eq!(sums_a, sums_b, "same world, tick by tick");
    assert_eq!(a.sync_checksum(), b.sync_checksum(), "same terrain");
    assert!(a.worms()[0].dirt_count > start.worms()[0].dirt_count, "it dug");
}

#[test]
fn dig_alone_digs_as_both_directions_pressed_together() {
    let Some(start) = dirt_world() else { return };
    let digs = |t: u32| t >= 20 && t.is_multiple_of(30);
    let both = |t: u32, s: &mut InputState| {
        if digs(t) {
            tap(s, KeyCode::ArrowLeft);
            tap(s, KeyCode::ArrowRight);
        }
    };
    let dig = |t: u32, s: &mut InputState| {
        if digs(t) {
            tap(s, DIG);
        }
    };
    let (mut a, mut b) = (start.clone(), start.clone());
    assert_eq!(play(&mut a, 300, both), play(&mut b, 300, dig));
    assert_eq!(a.sync_checksum(), b.sync_checksum());
    assert!(a.worms()[0].dirt_count > start.worms()[0].dirt_count, "it dug");
}
