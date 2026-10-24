//! Mouse controls (issue #24): the default bindings, rebound ones (issue
//! #25), and the controller on real worlds (aim, facing, the wheel, a
//! stall's repeated keys, a mouse player against a key player pressing the
//! same keys).

use std::path::Path;

use olr_content::{Content, SandboxSetup, SpawnSpec};
use olr_input::{Action, Binding, Bindings, InputState, KeyCode, LocalWorm, Mode, MouseInput, Resolve};
use olr_sim::{Dir, Keys, SimSettings, TickEvents, World};
use winit::event::MouseButton;

const L: u16 = Keys::LEFT;
const R: u16 = Keys::RIGHT;

fn mouse() -> InputState {
    let mut s = InputState::default();
    s.set_mouse_controls(true);
    s
}

#[test]
fn mouse_mode_fixes_its_keys_and_buttons() {
    let mut s = mouse();
    assert!(s.mouse_controls());
    // A and D move (strafing), Space jumps; A and S are no weapon keys now.
    s.on_key(KeyCode::KeyA, true);
    s.on_key(KeyCode::Space, true);
    assert_eq!(s.latch_tick(), Keys(L | Keys::STRAFE | Keys::JUMP));
    s.on_key(KeyCode::KeyA, false);
    s.on_key(KeyCode::Space, false);
    s.on_key(KeyCode::KeyD, true);
    assert_eq!(s.latch_tick(), Keys(R | Keys::STRAFE));
    s.on_key(KeyCode::KeyD, false);
    // Up, Down, SelectWeapon and Strafe keys are unused.
    for k in [KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ShiftLeft, KeyCode::KeyX] {
        s.on_key(k, true);
    }
    assert_eq!(s.latch_tick(), Keys(0));
    for k in [KeyCode::ArrowUp, KeyCode::ArrowDown, KeyCode::ShiftLeft, KeyCode::KeyX] {
        s.on_key(k, false);
    }
    // The buttons: left shoots, right throws the rope; a click is latched.
    s.on_mouse_button(MouseButton::Left, true);
    assert_eq!(s.held(), Keys(Keys::FIRE));
    assert_eq!(s.latch_tick(), Keys(Keys::FIRE));
    s.on_mouse_button(MouseButton::Left, false);
    s.on_mouse_button(MouseButton::Right, true);
    s.on_mouse_button(MouseButton::Right, false);
    assert_eq!(s.latch_tick(), Keys(Keys::ROPE), "a click shorter than a tick");
    assert_eq!(s.latch_tick(), Keys(0));
    // S digs (the gesture: both directions, nothing held).
    s.on_key(KeyCode::KeyS, true);
    s.on_key(KeyCode::KeyS, false);
    assert_eq!(s.latch_tick(), Keys(L | R | Keys::STRAFE));
    // Off again: the keyboard's own bindings; the buttons do nothing.
    s.set_mouse_controls(false);
    s.on_mouse_button(MouseButton::Left, true);
    s.on_key(KeyCode::KeyA, true);
    assert_eq!(s.latch_tick(), Keys(Keys::PREV_WEAPON));
}

#[test]
fn the_controller_s_keys_are_repeatable_in_a_stall() {
    let mut s = mouse();
    s.on_key(KeyCode::KeyA, true);
    assert_eq!(s.held(), Keys(L));
    assert_eq!(
        s.repeatable().0 & (Keys::STRAFE | Keys::UP | Keys::DOWN),
        Keys::STRAFE | Keys::UP | Keys::DOWN
    );
    let latched = s.latch_tick();
    assert_eq!(Keys(latched.0 & s.repeatable().0), Keys(L | Keys::STRAFE), "Strafe is repeated with A");
    // Keyboard mode repeats only what is held.
    let k = InputState::default();
    assert_eq!(k.repeatable(), k.held());
}

#[test]
fn wheel_turns_become_notches() {
    let mut s = mouse();
    s.on_wheel(-1.0);
    s.on_wheel(-1.0);
    s.on_wheel(1.0);
    // A touchpad: pixel deltas add up to notches.
    s.on_wheel(0.4);
    s.on_wheel(0.4);
    s.on_wheel(0.4);
    // Without a worm nothing is pressed and the queue is cleared.
    assert_eq!(s.latch_tick(), Keys(0));
    let mut k = InputState::default();
    k.on_wheel(1.0);
    assert_eq!(k.latch_tick(), Keys(0), "keyboard mode ignores the wheel");
}

/// Mouse controls with Shoot on the right button and Rope on the middle
/// one (issue #25), the wheel up jumping as well.
fn rebound() -> InputState {
    let m = Mode::Mouse;
    let mut b = Bindings::olx();
    b.bind(m, Action::Rope, Binding::Mouse(MouseInput::Middle), Resolve::Share).unwrap();
    b.bind(m, Action::Shoot, Binding::Mouse(MouseInput::Right), Resolve::Move).unwrap();
    b.unbind(m, Action::Shoot, Binding::Mouse(MouseInput::Left)).unwrap();
    b.bind(m, Action::Jump, Binding::Mouse(MouseInput::WheelUp), Resolve::Share).unwrap();
    assert!(b.unreachable().is_empty());
    let mut s = InputState::new(b);
    s.set_mouse_controls(true);
    s
}

#[test]
fn rebound_mouse_inputs_press_their_actions() {
    let mut s = rebound();
    s.on_mouse_button(MouseButton::Left, true);
    assert_eq!(s.latch_tick(), Keys(0), "the left button is no one's now");
    s.on_mouse_button(MouseButton::Left, false);
    s.on_mouse_button(MouseButton::Right, true);
    assert_eq!(s.latch_tick(), Keys(Keys::FIRE));
    s.on_mouse_button(MouseButton::Right, false);
    s.on_mouse_button(MouseButton::Middle, true);
    s.on_mouse_button(MouseButton::Middle, false);
    assert_eq!(s.latch_tick(), Keys(Keys::ROPE), "a click shorter than a tick");
    assert_eq!(s.latch_tick(), Keys(0));
    // The wheel up jumps for a tick (and its weapon change waits for a worm).
    s.on_wheel(1.0);
    assert_eq!(s.latch_tick(), Keys(Keys::JUMP));
    assert_eq!(s.latch_tick(), Keys(0));
    // Held by a key too, a notch releases nothing.
    s.on_key(KeyCode::Space, true);
    s.on_wheel(1.0);
    assert_eq!(s.latch_tick(), Keys(Keys::JUMP));
    assert_eq!(s.latch_tick(), Keys(Keys::JUMP));
    s.on_key(KeyCode::Space, false);
    // Off: the keyboard's own bindings, untouched; the mouse does nothing.
    s.set_mouse_controls(false);
    s.on_mouse_button(MouseButton::Right, true);
    s.on_wheel(1.0);
    s.on_key(KeyCode::ControlLeft, true);
    assert_eq!(s.latch_tick(), Keys(Keys::FIRE));
    assert_eq!(s.bindings().keys, Bindings::olx().keys);
}

#[test]
fn a_rebound_mouse_mode_keeps_its_own_keys() {
    // Mouse mode's keyboard rebound: Space throws the rope, the arrows of
    // the keyboard's own bindings do nothing in it.
    let mut b = Bindings::olx();
    b.bind(Mode::Mouse, Action::Rope, Binding::Key(KeyCode::KeyE), Resolve::Share).unwrap();
    let mut s = InputState::new(b);
    s.set_mouse_controls(true);
    s.on_key(KeyCode::KeyE, true);
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::ControlLeft, true);
    assert_eq!(s.latch_tick(), Keys(Keys::ROPE));
    s.set_mouse_controls(false);
    s.on_key(KeyCode::KeyE, true);
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(), Keys(L), "E is nothing without mouse controls");
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

/// The cursor `deg` degrees below level on side `side`, 100 px away.
fn at(deg: f32, side: Dir) -> [f32; 2] {
    let (s, c) = deg.to_radians().sin_cos();
    [if side == Dir::Right { 100.0 * c } else { -100.0 * c }, 100.0 * s]
}

/// Plays `ticks` ticks of `world` with mouse controls, keys applying
/// `ahead` ticks late; `events(tick, input)` feeds the input and gives the
/// cursor. The keys of every tick.
fn play(
    world: &mut World,
    ticks: u32,
    ahead: u32,
    events: impl Fn(u32, &mut InputState) -> Option<[f32; 2]>,
) -> Vec<Keys> {
    play_with(mouse(), world, ticks, ahead, events)
}

/// [`play`] with `input`'s bindings.
fn play_with(
    mut input: InputState,
    world: &mut World,
    ticks: u32,
    ahead: u32,
    events: impl Fn(u32, &mut InputState) -> Option<[f32; 2]>,
) -> Vec<Keys> {
    let mut out = TickEvents::new();
    let mut flight: std::collections::VecDeque<Keys> = (0..ahead).map(|_| Keys(0)).collect();
    (0..ticks)
        .map(|t| {
            let cursor = events(t, &mut input);
            let local = LocalWorm::of(world, 0, ahead);
            let k = input.latch_tick_for(local.as_ref(), cursor);
            flight.push_back(k);
            world.tick(&[flight.pop_front().unwrap()], &mut out);
            k
        })
        .collect()
}

#[test]
fn the_cursor_aims_and_turns_a_real_worm() {
    let Some(start) = dirt_world() else { return };
    for ahead in [0, 3] {
        let mut w = start.clone();
        let face = w.worms()[0].face;
        let away = if face == Dir::Right { Dir::Left } else { Dir::Right };
        play(&mut w, 150, ahead, |_, _| Some(at(-45.0, away)));
        let worm = &w.worms()[0];
        assert_eq!(worm.face, away, "ahead {ahead}");
        assert!((worm.angle + 45.0).abs() < 1.5, "ahead {ahead}: aims at {}", worm.angle);
        assert_eq!(worm.dirt_count, start.worms()[0].dirt_count, "turning dug");
    }
}

#[test]
fn a_and_d_keep_the_facing_online_and_in_a_stall() {
    let Some(start) = dirt_world() else { return };
    let mut w = start.clone();
    let face = w.worms()[0].face;
    let back = if face == Dir::Right { KeyCode::KeyA } else { KeyCode::KeyD };
    let cursor = at(0.0, face);
    // Walking away from the cursor, 3 ticks of input delay.
    play(&mut w, 60, 3, |t, s| {
        if t == 0 {
            s.on_key(back, true);
        }
        Some(cursor)
    });
    assert_eq!(w.worms()[0].face, face, "strafing");
    // A stall: the pump repeats the last keys masked by `repeatable`.
    let mut input = mouse();
    input.on_key(back, true);
    let local = LocalWorm::of(&w, 0, 0);
    let last = input.latch_tick_for(local.as_ref(), Some(cursor));
    let repeated = Keys(last.0 & input.repeatable().0);
    let mut out = TickEvents::new();
    let x = w.worms()[0].pos.x;
    for _ in 0..25 {
        w.tick(&[repeated], &mut out);
    }
    assert_eq!(w.worms()[0].face, face, "the stall keeps strafing");
    assert_ne!(w.worms()[0].pos.x, x, "and walking");
}

#[test]
fn wheel_notches_change_the_weapon_one_each() {
    let Some(start) = dirt_world() else { return };
    let mut w = start.clone();
    let slots = w.worms()[0].slots.len() as i32;
    let first = w.worms()[0].current_weapon;
    play(&mut w, 40, 0, |t, s| {
        if t == 5 {
            // Three notches down, one up, in one frame.
            s.on_wheel(-3.0);
            s.on_wheel(1.0);
        }
        None
    });
    assert_eq!(w.worms()[0].current_weapon, (first + 2).rem_euclid(slots));
}

#[test]
fn a_mouse_player_and_a_key_player_pressing_the_same_keys_agree() {
    let Some(start) = dirt_world() else { return };
    let mut a = start.clone();
    let keys = play(&mut a, 600, 0, |t, s| {
        match t {
            20 => s.on_key(KeyCode::KeyD, true),
            120 => s.on_mouse_button(MouseButton::Left, true),
            140 => s.on_mouse_button(MouseButton::Left, false),
            200 => {
                s.on_key(KeyCode::KeyS, true);
                s.on_key(KeyCode::KeyS, false);
            }
            250 => s.on_key(KeyCode::KeyD, false),
            300 => s.on_wheel(-1.0),
            320 => s.on_key(KeyCode::Space, true),
            330 => s.on_key(KeyCode::Space, false),
            400 => s.on_mouse_button(MouseButton::Right, true),
            401 => s.on_mouse_button(MouseButton::Right, false),
            _ => {}
        }
        let deg = (t as f32 * 0.7).sin() * 70.0;
        Some(at(deg, if (t / 90) % 2 == 0 { Dir::Right } else { Dir::Left }))
    });
    // The key player presses exactly those keys.
    let mut b = start.clone();
    let mut out = TickEvents::new();
    for k in &keys {
        b.tick(&[*k], &mut out);
    }
    assert_eq!(a.sync_checksum(), b.sync_checksum());
    assert!(keys.iter().any(|k| k.0 & Keys::STRAFE != 0) && keys.iter().any(|k| k.0 & Keys::UP != 0));
}

#[test]
fn rebound_buttons_work_in_the_controller_on_a_real_worm() {
    let Some(start) = dirt_world() else { return };
    let mut w = start.clone();
    let keys = play_with(rebound(), &mut w, 80, 0, |t, s| {
        match t {
            10 => s.on_mouse_button(MouseButton::Right, true),
            30 => s.on_mouse_button(MouseButton::Right, false),
            40 => s.on_mouse_button(MouseButton::Middle, true),
            41 => s.on_mouse_button(MouseButton::Middle, false),
            50 => s.on_mouse_button(MouseButton::Left, true),
            _ => {}
        }
        Some(at(-30.0, Dir::Right))
    });
    let has = |t: usize, bit: u16| keys[t].0 & bit != 0;
    assert!((10..30).all(|t| has(t, Keys::FIRE)), "the right button shoots");
    assert!(has(40, Keys::ROPE) && !has(42, Keys::ROPE), "the middle one throws the rope");
    assert!((0..80).all(|t| !(has(t, Keys::ROPE) && !(40..42).contains(&t))));
    assert!((50..80).all(|t| !has(t, Keys::FIRE)), "the left button does nothing");
    // A key player pressing those keys plays the same match.
    let mut b = start.clone();
    let mut out = TickEvents::new();
    for k in &keys {
        b.tick(&[*k], &mut out);
    }
    assert_eq!(w.sync_checksum(), b.sync_checksum());
}
