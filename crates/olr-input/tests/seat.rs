//! `olr-input`: input seats (issue #28): keyboard 1, keyboard 2 on the
//! same keyboard and a seat per gamepad, each with its own keys; each
//! seat's own rule of reachable actions; the keys both keyboard seats
//! bind; a pad's Start pausing; an options file from before seats.

use olr_input::{
    Action, Binding, Bindings, Button, Device, InputState, KeyCode, Mode, Need, PAD_PAUSE, PadEvent,
    PadInput, RebindError, Resolve, Seat, SeatKind, Seats,
};
use olr_sim::Keys;

const K2: Mode = Mode::Keyboard2;

#[test]
fn keyboard_1_and_2_held_at_once_give_each_seat_its_own_keys() {
    let mut s = Seats::default();
    // Player 1 walks left and shoots; player 2 walks right and jumps.
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_key(KeyCode::ControlLeft, true);
    s.on_key(KeyCode::Numpad6, true);
    s.on_key(KeyCode::NumpadEnter, true);
    assert_eq!(s.latch_tick(Seat::Keyboard1), Keys(Keys::LEFT | Keys::FIRE));
    assert_eq!(s.latch_tick(Seat::Keyboard2), Keys(Keys::RIGHT | Keys::JUMP));
    // A release reaches its own seat alone.
    s.on_key(KeyCode::ArrowLeft, false);
    assert_eq!(s.latch_tick(Seat::Keyboard1), Keys(Keys::FIRE));
    assert_eq!(s.latch_tick(Seat::Keyboard2), Keys(Keys::RIGHT | Keys::JUMP));
    assert_eq!(s.held(Seat::Keyboard2), Keys(Keys::RIGHT | Keys::JUMP));
    // Every OLX player 2 key presses its action, and kp 3 digs.
    let mut s = Seats::default();
    for (a, k) in Bindings::olx_keys2() {
        s.on_key(k, true);
        let keys = s.latch_tick(Seat::Keyboard2);
        if a == Action::Dig {
            // The dig gesture, from no direction held: both tapped.
            assert_eq!(keys, Keys(Keys::LEFT | Keys::RIGHT), "{a:?}");
        } else {
            assert_eq!(keys, Keys(a.bit()), "{a:?}");
        }
        assert_eq!(s.latch_tick(Seat::Keyboard1), Keys(0), "{a:?} is keyboard 2's alone");
        s.on_key(k, false);
        s.latch_tick(Seat::Keyboard2);
        s.latch_tick(Seat::Keyboard2);
    }
}

#[test]
fn the_solo_player_never_hears_keyboard_2() {
    // Nothing changes for a player who never touches the seats: the
    // numeric keypad presses nothing, the keyboard and any pad still play.
    let mut s = InputState::default();
    s.on_key(KeyCode::Numpad4, true);
    s.on_key(KeyCode::NumpadAdd, true);
    assert_eq!(s.latch_tick(), Keys(0));
    s.on_key(KeyCode::ArrowLeft, true);
    s.on_pad(3, PadEvent::Button(Button::South, 1.0));
    assert_eq!(s.latch_tick(), Keys(Keys::LEFT | Keys::JUMP));
    assert_eq!(s.seat(), None);
}

#[test]
fn pads_get_a_seat_each_by_id() {
    let mut s = Seats::default();
    assert_eq!(s.seats(), [Seat::Keyboard1, Seat::Keyboard2]);
    s.on_pad(4, PadEvent::Button(Button::RightTrigger2, 1.0));
    s.on_pad(1, PadEvent::Button(Button::South, 1.0));
    assert_eq!(s.seats(), [Seat::Keyboard1, Seat::Keyboard2, Seat::Pad(4), Seat::Pad(1)]);
    assert_eq!(s.latch_tick(Seat::Pad(4)), Keys(Keys::FIRE));
    assert_eq!(s.latch_tick(Seat::Pad(1)), Keys(Keys::JUMP));
    // No keyboard seat hears a pad, no pad seat a key.
    assert_eq!(s.latch_tick(Seat::Keyboard1), Keys(0));
    s.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(s.latch_tick(Seat::Pad(4)), Keys(Keys::FIRE));
    assert_eq!(s.latch_tick(Seat::Keyboard1), Keys(Keys::LEFT));
    // A stick on pad 1 only.
    s.on_pad(1, PadEvent::Axis(olr_input::Axis::LeftStickX, 0.9));
    assert_eq!(s.latch_tick(Seat::Pad(1)), Keys(Keys::JUMP | Keys::RIGHT));
    assert_eq!(s.latch_tick(Seat::Pad(4)), Keys(Keys::FIRE));
    // A disconnected pad's seat is gone, the others stay.
    s.on_pad(4, PadEvent::Disconnected);
    assert_eq!(s.seats(), [Seat::Keyboard1, Seat::Keyboard2, Seat::Pad(1)]);
    assert_eq!(s.latch_tick(Seat::Pad(4)), Keys(0));
    assert_eq!(s.held(Seat::Pad(1)), Keys(Keys::JUMP | Keys::RIGHT));
    // A single seat's state hears its own pad only.
    let mut p = InputState::for_seat(Seat::Pad(2), Bindings::default());
    p.on_pad(0, PadEvent::Button(Button::South, 1.0));
    p.on_key(KeyCode::ArrowLeft, true);
    assert_eq!(p.latch_tick(), Keys(0));
    p.on_pad(2, PadEvent::Button(Button::South, 1.0));
    assert_eq!(p.latch_tick(), Keys(Keys::JUMP));
    assert_eq!(Seat::Pad(2).kind(), SeatKind::Gamepad);
}

#[test]
fn only_keyboard_1_has_mouse_controls() {
    let mut s = Seats::default();
    s.set_mouse_controls(true);
    assert!(s.get(Seat::Keyboard1).unwrap().mouse_controls());
    assert!(!s.get(Seat::Keyboard2).unwrap().mouse_controls());
    let mut k2 = InputState::for_seat(Seat::Keyboard2, Bindings::default());
    k2.set_mouse_controls(true);
    assert!(!k2.mouse_controls());
    let mut pad = InputState::for_seat(Seat::Pad(0), Bindings::default());
    pad.set_mouse_controls(true);
    assert!(!pad.mouse_controls());
    // Keyboard 1 in mouse mode plays mouse controls' keys, keyboard 2 its own.
    s.on_key(KeyCode::KeyA, true);
    s.on_key(KeyCode::Numpad4, true);
    assert_eq!(s.held(Seat::Keyboard1), Keys(Keys::LEFT));
    assert_eq!(s.held(Seat::Keyboard2), Keys(Keys::LEFT));
}

#[test]
fn each_seat_keeps_its_own_actions_reachable() {
    let b = Bindings::olx();
    for seat in SeatKind::ALL {
        assert!(b.unreachable_of(seat).is_empty(), "{seat:?}");
    }
    // A keyboard-only player 2: keyboard 2 needs keys, never pad inputs.
    assert_eq!(b.of(K2, Action::Shoot, Device::Keyboard), [Binding::Key(KeyCode::NumpadAdd)]);
    assert!(b.of(K2, Action::Shoot, Device::Gamepad).is_empty());
    let mut b = Bindings::olx();
    let pad = Binding::Pad(PadInput::Button(Button::LeftThumb));
    assert_eq!(
        b.bind(K2, Action::Shoot, pad, Resolve::Share),
        Err(RebindError::KeysOnly { device: Device::Gamepad })
    );
    // Its last key cannot go; another first, then it can.
    let plus = Binding::Key(KeyCode::NumpadAdd);
    assert_eq!(
        b.unbind(K2, Action::Shoot, plus),
        Err(RebindError::Unreachable { action: Action::Shoot, need: Need::Keyboard2Key })
    );
    let star = Binding::Key(KeyCode::NumpadMultiply);
    b.bind(K2, Action::Shoot, star, Resolve::Share).unwrap();
    b.unbind(K2, Action::Shoot, plus).unwrap();
    assert_eq!(b.of(K2, Action::Shoot, Device::Keyboard), [star]);
    // A move that would leave keyboard 2's Jump without a key is refused.
    let enter = Binding::Key(KeyCode::NumpadEnter);
    let before = b.clone();
    assert_eq!(
        b.bind(K2, Action::Rope, enter, Resolve::Move),
        Err(RebindError::Unreachable { action: Action::Jump, need: Need::Keyboard2Key })
    );
    assert_eq!(b, before);
    // Keyboard 2's keys are its own list: keyboard 1's are untouched.
    assert_eq!(b.keys, Bindings::olx().keys);
    // Each seat's gaps are its own: no pad inputs leave keyboard 2 whole.
    let mut b = Bindings::olx();
    b.pad.clear();
    assert!(b.unreachable_of(SeatKind::Keyboard2).is_empty());
    assert!(b.unreachable_of(SeatKind::Keyboard1).is_empty());
    assert_eq!(b.unreachable_of(SeatKind::Gamepad).len(), Action::ALL.len());
    b.keys2.retain(|(a, _)| *a != Action::Dig);
    assert_eq!(b.unreachable_of(SeatKind::Keyboard2), [(Action::Dig, Need::Keyboard2Key)]);
    assert!(b.unreachable_of(SeatKind::Keyboard1).is_empty());
    // The options' columns and modes per seat.
    assert_eq!(SeatKind::Keyboard1.devices(false), [Device::Keyboard]);
    assert_eq!(SeatKind::Keyboard1.devices(true), [Device::Keyboard, Device::Mouse]);
    assert_eq!(SeatKind::Keyboard2.devices(true), [Device::Keyboard]);
    assert_eq!(SeatKind::Gamepad.devices(true), [Device::Gamepad]);
    assert_eq!(SeatKind::Keyboard1.mode(true), Mode::Mouse);
    assert_eq!(SeatKind::Keyboard2.mode(false), K2);
}

#[test]
fn keys_both_keyboards_bind_are_conflicts() {
    let mut b = Bindings::olx();
    assert!(b.shared_keys(false).is_empty());
    assert!(b.shared_keys(true).is_empty());
    // Keyboard 2 takes the arrow up for Jump (and keeps kp enter).
    b.bind(K2, Action::Jump, Binding::Key(KeyCode::ArrowUp), Resolve::Share).unwrap();
    assert_eq!(b.shared_keys(false), [KeyCode::ArrowUp]);
    // With mouse controls on keyboard 1 plays their keys, which lack it.
    assert!(b.shared_keys(true).is_empty());
    b.bind(K2, Action::Rope, Binding::Key(KeyCode::Space), Resolve::Share).unwrap();
    assert_eq!(b.shared_keys(true), [KeyCode::Space]);
    // One press presses both seats' actions.
    let mut s = Seats::new(b);
    s.on_key(KeyCode::ArrowUp, true);
    assert_eq!(s.latch_tick(Seat::Keyboard1), Keys(Keys::UP));
    assert_eq!(s.latch_tick(Seat::Keyboard2), Keys(Keys::JUMP));
}

#[test]
fn start_on_a_pad_pauses() {
    let mut s = Seats::default();
    assert!(!s.take_pause());
    s.on_pad(2, PadEvent::Button(Button::Start, 1.0));
    assert!(s.take_pause());
    assert!(!s.take_pause(), "once a press");
    // Held, it asks no more; pressed again, it does.
    s.on_pad(2, PadEvent::Button(Button::Start, 1.0));
    assert!(!s.take_pause());
    s.on_pad(2, PadEvent::Button(Button::Start, 0.0));
    s.on_pad(2, PadEvent::Button(Button::Start, 1.0));
    assert!(s.take_pause());
    // It presses no action.
    assert_eq!(s.latch_tick(Seat::Pad(2)), Keys(0));
    assert!(Bindings::olx().actions_of(Mode::Keys, Binding::Pad(PAD_PAUSE)).is_empty());
    // The solo player's state hears it from any pad.
    let mut solo = InputState::default();
    solo.on_pad(7, PadEvent::Button(Button::Start, 1.0));
    assert!(solo.take_pause());
    // A keyboard seat hears no pad's.
    let mut k = InputState::for_seat(Seat::Keyboard1, Bindings::default());
    k.on_pad(0, PadEvent::Button(Button::Start, 1.0));
    assert!(!k.take_pause());
}

#[test]
fn an_options_file_from_before_seats_is_repaired() {
    // Bindings written before seats: no keyboard 2 list. Every list read
    // is kept, keyboard 2 gets OLX's player 2.
    let mut old = Bindings::olx();
    old.keys.push((Action::Shoot, KeyCode::KeyQ));
    let mut text = ron::to_string(&old).unwrap();
    let at = text.find(",keys2:").expect("keys2 is written");
    text.truncate(at);
    text.push(')');
    let mut b: Bindings = ron::from_str(&text).unwrap();
    assert_eq!(b.keys, old.keys, "keyboard 1's kept");
    assert_eq!(b.keys2, Bindings::olx_keys2());
    assert!(b.repair().is_empty());
    // A keyboard 2 list missing an action gets its default back, one
    // warning; keyboard 1's are untouched.
    b.keys2.retain(|(a, _)| *a != Action::Rope);
    let w = b.repair();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("Rope") && w[0].contains("player 2") && w[0].contains("Num Subtract"), "{w:?}");
    assert!(b.unreachable().is_empty());
    assert_eq!(b.keys, old.keys);
}

/// Issue #32: a menu driven by the game's keys reads each seat's actions
/// held, Dig included, and whether its pad's Start is held; Start presses
/// no action.
#[test]
fn a_seat_shows_its_actions_held_and_start() {
    let mut s = Seats::default();
    s.on_key(KeyCode::Numpad3, true);
    let k2 = s.get(Seat::Keyboard2).unwrap();
    assert_eq!(k2.actions_held(), Keys(olr_input::DIG));
    assert_eq!(k2.held(), Keys(0), "the game's keys leave Dig out");
    assert!(!k2.start_held());
    s.on_pad(4, PadEvent::Button(Button::Start, 1.0));
    let pad = s.get(Seat::Pad(4)).unwrap();
    assert!(pad.start_held());
    assert_eq!(pad.actions_held(), Keys(0));
    s.on_pad(4, PadEvent::Button(Button::Start, 0.0));
    assert!(!s.get(Seat::Pad(4)).unwrap().start_held());
}
