//! `olr-input`: bindings, gamepad mapping and latching between ticks.

use olr_input::{
    Action, Axis, Bindings, Button, DIG, InputState, KeyCode, PadEvent, PadInput, TickLatch,
    is_browser_reserved,
};
use olr_sim::Keys;

const L: u16 = Keys::LEFT;
const R: u16 = Keys::RIGHT;

#[test]
fn tap_between_ticks_is_latched() {
    let mut s = InputState::default();
    // A tap of Rope (X) between two ticks: down for the next tick only.
    s.on_key(KeyCode::KeyX, true);
    s.on_key(KeyCode::KeyX, false);
    assert_eq!(s.held(), Keys(0));
    assert_eq!(s.latch_tick(), Keys(Keys::ROPE));
    assert_eq!(s.latch_tick(), Keys(0));

    // A held key stays down; its release is seen by the next tick.
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(), Keys(L));
    assert_eq!(s.latch_tick(), Keys(L));
    // Released and pressed again within one tick (digging taps the opposite
    // direction): up for one tick, then down again, so the sim sees a press.
    s.on_key(KeyCode::ArrowLeft, false);
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(), Keys(0));
    assert_eq!(s.latch_tick(), Keys(L));
    // Released, pressed and released again within one tick: up, then the
    // tap, then up.
    s.on_key(KeyCode::ArrowLeft, false);
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::ArrowLeft, false);
    assert_eq!(s.latch_tick(), Keys(0));
    assert_eq!(s.latch_tick(), Keys(L));
    assert_eq!(s.latch_tick(), Keys(0));

    // The dig: Left held, Right tapped between ticks.
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(), Keys(L));
    s.on_key(KeyCode::ArrowRight, true);
    s.on_key(KeyCode::ArrowRight, false);
    assert_eq!(s.latch_tick(), Keys(L | R));
    assert_eq!(s.latch_tick(), Keys(L));

    // Key repeats and unbound keys change nothing; a second key bound to a
    // held action does not release it.
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::KeyQ, true);
    assert_eq!(s.latch_tick(), Keys(L));
    let mut b = Bindings::default();
    b.keys.push((Action::Left, KeyCode::KeyJ));
    let mut s = InputState::new(b);
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::KeyJ, true);
    s.on_key(KeyCode::ArrowLeft, false);
    assert_eq!(s.latch_tick(), Keys(L));
    s.on_key(KeyCode::KeyJ, false);
    assert_eq!(s.latch_tick(), Keys(0));

    // Focus loss releases everything held.
    s.on_key(KeyCode::ControlLeft, true);
    assert_eq!(s.latch_tick(), Keys(Keys::FIRE));
    s.release_keys();
    assert_eq!(s.latch_tick(), Keys(0));
}

#[test]
fn tick_latch_sees_every_press_and_release() {
    // Exhaustive over every sequence of up to 4 edges of one key between
    // two ticks: a press since the last tick is seen down, and a release of
    // a key seen down is seen up, within two ticks.
    for start in [false, true] {
        for n in 0..=4u32 {
            let mut l = TickLatch::default();
            l.set(start as u32);
            l.latch();
            let mut now = start;
            for _ in 0..n {
                now = !now;
                l.set(now as u32);
            }
            let a = l.latch() & 1 == 1;
            let b = l.latch() & 1 == 1;
            let c = l.latch() & 1 == 1;
            let pressed = (!start && n >= 1) || (start && n >= 2);
            let released = start && n >= 1;
            if pressed {
                assert!(a || b, "start {start} edges {n}: press lost");
            }
            if released {
                assert!(!a, "start {start} edges {n}: release lost");
            }
            assert_eq!(c, now, "start {start} edges {n}: settles to the device");
        }
    }
}

#[test]
fn default_bindings_match_olx() {
    // OLX: src/client/Options.cpp:45-51 (ply_def1, non-Apple), :64-73.
    let b = Bindings::default();
    let key =
        |a: Action| -> Vec<KeyCode> { b.keys.iter().filter(|(x, _)| *x == a).map(|(_, k)| *k).collect() };
    assert_eq!(key(Action::Up), [KeyCode::ArrowUp]);
    assert_eq!(key(Action::Down), [KeyCode::ArrowDown]);
    assert_eq!(key(Action::Left), [KeyCode::ArrowLeft]);
    assert_eq!(key(Action::Right), [KeyCode::ArrowRight]);
    assert_eq!(key(Action::Shoot), [KeyCode::ControlLeft]);
    assert_eq!(key(Action::Jump), [KeyCode::AltLeft]);
    assert_eq!(key(Action::Rope), [KeyCode::KeyX]);
    assert_eq!(key(Action::SelectWeapon), [KeyCode::ShiftLeft]);
    assert_eq!(key(Action::Strafe), [KeyCode::KeyZ]);
    assert_eq!(key(Action::PreviousWeapon), [KeyCode::KeyA]);
    assert_eq!(key(Action::NextWeapon), [KeyCode::KeyS]);
    assert_eq!(key(Action::Dig), [KeyCode::KeyC]);
    // One action per default key.
    let mut keys: Vec<KeyCode> = b.keys.iter().map(|(_, k)| *k).collect();
    keys.sort_by_key(|k| format!("{k:?}"));
    keys.dedup();
    assert_eq!(keys.len(), b.keys.len());
    let pad =
        |a: Action| -> Vec<PadInput> { b.pad.iter().filter(|(x, _)| *x == a).map(|(_, p)| *p).collect() };
    assert_eq!(pad(Action::Shoot), [PadInput::Button(Button::RightTrigger2)]);
    assert_eq!(pad(Action::Jump), [PadInput::Button(Button::South)]);
    assert_eq!(pad(Action::Rope), [PadInput::Button(Button::North)]);
    assert_eq!(pad(Action::SelectWeapon), [PadInput::Button(Button::LeftTrigger2)]);
    assert!(pad(Action::Left).contains(&PadInput::Button(Button::DPadLeft)));
    assert_eq!(pad(Action::Strafe), [PadInput::Button(Button::West)]);
    assert_eq!(pad(Action::PreviousWeapon), [PadInput::Button(Button::LeftTrigger)]);
    assert_eq!(pad(Action::NextWeapon), [PadInput::Button(Button::RightTrigger)]);
    assert_eq!(pad(Action::Dig), [PadInput::Button(Button::East)]);
    let mut inputs: Vec<String> = b.pad.iter().map(|(_, p)| format!("{p:?}")).collect();
    inputs.sort();
    inputs.dedup();
    assert_eq!(inputs.len(), b.pad.len(), "one action per default pad input");

    // Every action maps to its own Keys bit; Dig to olr-input's own,
    // beyond the simulation's 16 bits.
    let bits: Vec<u32> = Action::ALL.iter().map(|a| a.bit()).collect();
    assert_eq!(bits, [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, DIG]);
    assert!(DIG > u32::from(u16::MAX));

    // Spec §8.5: no default key, or pair of them, is a shortcut the browser
    // keeps (such as Ctrl+W); such a binding is reported.
    assert_eq!(b.browser_conflicts(), Vec::<Vec<KeyCode>>::new());
    assert!(is_browser_reserved(&[KeyCode::ControlLeft, KeyCode::KeyW]));
    assert!(!is_browser_reserved(&[KeyCode::ControlLeft, KeyCode::KeyX]));
    let mut bad = b.clone();
    bad.keys.push((Action::Rope, KeyCode::KeyW));
    assert_eq!(bad.browser_conflicts(), [vec![KeyCode::ControlLeft, KeyCode::KeyW]]);

    // Bindings round-trip through serde (M7's options file).
    let text = ron::to_string(&b).expect("serialise");
    let back: Bindings = ron::from_str(&text).expect("deserialise");
    assert_eq!(back, b);
}

#[test]
fn gamepad_maps_to_keys() {
    let mut s = InputState::default();
    // Buttons: A jumps, Y ropes; a trigger counts from 75 % of its travel.
    s.on_pad(0, PadEvent::Button(Button::South, 1.0));
    assert_eq!(s.held(), Keys(Keys::JUMP));
    s.on_pad(0, PadEvent::Button(Button::RightTrigger2, 0.7));
    assert_eq!(s.held(), Keys(Keys::JUMP));
    s.on_pad(0, PadEvent::Button(Button::RightTrigger2, 0.8));
    assert_eq!(s.held(), Keys(Keys::JUMP | Keys::FIRE));
    s.on_pad(0, PadEvent::Button(Button::South, 0.0));
    s.on_pad(0, PadEvent::Button(Button::RightTrigger2, 0.0));
    assert_eq!(s.held(), Keys(0));
    s.on_pad(0, PadEvent::Button(Button::North, 1.0));
    s.on_pad(0, PadEvent::Button(Button::LeftTrigger2, 1.0));
    s.on_pad(0, PadEvent::Button(Button::DPadUp, 1.0));
    assert_eq!(s.held(), Keys(Keys::ROPE | Keys::CHANGE | Keys::UP));
    s.on_pad(0, PadEvent::Disconnected);
    assert_eq!(s.held(), Keys(0));

    // Sticks: left X moves, right Y aims, from 60 % deflection.
    s.on_pad(1, PadEvent::Axis(Axis::LeftStickX, -0.5));
    assert_eq!(s.held(), Keys(0));
    s.on_pad(1, PadEvent::Axis(Axis::LeftStickX, -0.9));
    assert_eq!(s.held(), Keys(L));
    s.on_pad(1, PadEvent::Axis(Axis::LeftStickX, 0.9));
    assert_eq!(s.held(), Keys(R));
    s.on_pad(1, PadEvent::Axis(Axis::RightStickY, 0.9));
    assert_eq!(s.held(), Keys(R | Keys::UP));
    s.on_pad(1, PadEvent::Axis(Axis::RightStickY, -0.9));
    assert_eq!(s.held(), Keys(R | Keys::DOWN));
    s.on_pad(1, PadEvent::Axis(Axis::LeftStickX, 0.0));
    s.on_pad(1, PadEvent::Axis(Axis::RightStickY, 0.0));
    assert_eq!(s.held(), Keys(0));

    // Two pads and the keyboard share the player; one letting go does not
    // release what another holds. A pad tap is latched like a key tap.
    // (Every press so far is still latched for the next tick.)
    assert_eq!(s.latch_tick(), Keys(0xff));
    assert_eq!(s.latch_tick(), Keys(0));
    s.on_pad(0, PadEvent::Button(Button::South, 1.0));
    s.on_pad(1, PadEvent::Button(Button::South, 1.0));
    s.on_key(KeyCode::AltLeft, true);
    s.on_pad(0, PadEvent::Button(Button::South, 0.0));
    s.on_pad(1, PadEvent::Disconnected);
    assert_eq!(s.held(), Keys(Keys::JUMP));
    s.on_key(KeyCode::AltLeft, false);
    assert_eq!(s.latch_tick(), Keys(Keys::JUMP));
    assert_eq!(s.latch_tick(), Keys(0));
    s.on_pad(2, PadEvent::Button(Button::North, 1.0));
    s.on_pad(2, PadEvent::Button(Button::North, 0.0));
    assert_eq!(s.latch_tick(), Keys(Keys::ROPE));
    assert_eq!(s.latch_tick(), Keys(0));

    // The new keys: X strafes, the shoulders change weapon.
    s.on_pad(0, PadEvent::Button(Button::West, 1.0));
    s.on_pad(0, PadEvent::Button(Button::LeftTrigger, 1.0));
    s.on_pad(0, PadEvent::Button(Button::RightTrigger, 1.0));
    assert_eq!(s.held(), Keys(Keys::STRAFE | Keys::PREV_WEAPON | Keys::NEXT_WEAPON));
}

#[test]
fn the_new_keys_reach_the_simulation() {
    let mut s = InputState::default();
    s.on_key(KeyCode::KeyZ, true);
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(), Keys(Keys::STRAFE | L));
    s.on_key(KeyCode::KeyA, true);
    s.on_key(KeyCode::KeyA, false);
    s.on_key(KeyCode::KeyS, true);
    assert_eq!(s.latch_tick(), Keys(Keys::STRAFE | L | Keys::PREV_WEAPON | Keys::NEXT_WEAPON));
    assert_eq!(s.latch_tick(), Keys(Keys::STRAFE | L | Keys::NEXT_WEAPON));
}
