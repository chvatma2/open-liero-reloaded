//! `rl-input`: rebinding (plan 7 task 4, Review Focus 2): conflicts are
//! resolved explicitly, no action is ever left without an input on a
//! device, a hand-edited file is repaired, and the options capture the
//! next key or gamepad input.

use rl_input::{
    Action, Axis, Binding, Bindings, Button, Capture, Device, KeyCode, PadEvent, PadInput, RebindError,
    Resolve,
};

const KB: Device = Device::Keyboard;
const PAD: Device = Device::Gamepad;

#[test]
fn bindings_list_and_find_inputs_by_action() {
    let b = Bindings::olx();
    assert_eq!(b.of(Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert_eq!(
        b.of(Action::Left, PAD),
        [
            Binding::Pad(PadInput::Button(Button::DPadLeft)),
            Binding::Pad(PadInput::AxisMinus(Axis::LeftStickX))
        ]
    );
    assert_eq!(b.actions_of(Binding::Key(KeyCode::KeyX)), [Action::Rope]);
    assert!(b.actions_of(Binding::Key(KeyCode::KeyZ)).is_empty());
    assert!(b.unreachable().is_empty());
    assert_eq!(Binding::Key(KeyCode::KeyZ).device(), KB);
    assert_eq!(Binding::Pad(PadInput::Button(Button::South)).device(), PAD);
}

#[test]
fn binding_a_used_key_is_resolved_explicitly() {
    let mut b = Bindings::olx();
    let ctrl = Binding::Key(KeyCode::ControlLeft);
    let z = Binding::Key(KeyCode::KeyZ);
    // Shared: both actions keep it.
    b.bind(Action::Jump, ctrl, Resolve::Share).unwrap();
    assert_eq!(b.actions_of(ctrl), [Action::Jump, Action::Shoot]);
    // Moved: it leaves every other action (Shoot has no other key: refused,
    // nothing changes).
    let before = b.clone();
    assert_eq!(
        b.bind(Action::Rope, ctrl, Resolve::Move),
        Err(RebindError::Unreachable { action: Action::Shoot, device: KB })
    );
    assert_eq!(b, before, "a refused move changes nothing");
    // With another key on Shoot the move is allowed.
    b.bind(Action::Shoot, z, Resolve::Share).unwrap();
    b.bind(Action::Rope, ctrl, Resolve::Move).unwrap();
    assert_eq!(b.actions_of(ctrl), [Action::Rope]);
    assert_eq!(b.of(Action::Shoot, KB), [z]);
    assert_eq!(b.of(Action::Jump, KB), [Binding::Key(KeyCode::AltLeft)]);
    // Binding what an action already has changes nothing.
    let before = b.clone();
    b.bind(Action::Rope, ctrl, Resolve::Share).unwrap();
    assert_eq!(b, before);
    assert!(b.unreachable().is_empty());
}

#[test]
fn the_last_input_of_a_device_cannot_be_unbound() {
    let mut b = Bindings::olx();
    let x = Binding::Key(KeyCode::KeyX);
    assert_eq!(b.unbind(Action::Rope, x), Err(RebindError::Unreachable { action: Action::Rope, device: KB }));
    b.bind(Action::Rope, Binding::Key(KeyCode::KeyC), Resolve::Share).unwrap();
    b.unbind(Action::Rope, x).unwrap();
    assert_eq!(b.of(Action::Rope, KB), [Binding::Key(KeyCode::KeyC)]);
    // Left has two pad inputs: one goes, the other stays and cannot.
    let dpad = Binding::Pad(PadInput::Button(Button::DPadLeft));
    let stick = Binding::Pad(PadInput::AxisMinus(Axis::LeftStickX));
    b.unbind(Action::Left, dpad).unwrap();
    assert_eq!(
        b.unbind(Action::Left, stick),
        Err(RebindError::Unreachable { action: Action::Left, device: PAD })
    );
    // Unbinding what the action does not have is no error and no change.
    let before = b.clone();
    b.unbind(Action::Left, dpad).unwrap();
    assert_eq!(b, before);
    // The keyboard and the pad on one action are no conflict.
    b.bind(Action::Shoot, Binding::Pad(PadInput::Button(Button::West)), Resolve::Move).unwrap();
    assert_eq!(b.of(Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert!(b.unreachable().is_empty());
    assert!(RebindError::Unreachable { action: Action::Shoot, device: KB }.to_string().contains("Shoot"));
}

#[test]
fn repair_restores_olx_defaults_for_unreachable_actions() {
    let mut b = Bindings::olx();
    b.keys.retain(|(a, _)| *a != Action::Shoot);
    b.keys.push((Action::Jump, KeyCode::ControlLeft));
    b.pad.clear();
    // A pair listed twice is listed once.
    b.keys.push((Action::Rope, KeyCode::KeyX));
    let w = b.repair();
    assert!(w.iter().any(|w| w.contains("Shoot") && w.contains("Left Ctrl")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("gamepad")), "{w:?}");
    assert!(b.unreachable().is_empty());
    assert_eq!(b.actions_of(Binding::Key(KeyCode::ControlLeft)), [Action::Jump, Action::Shoot]);
    assert_eq!(b.of(Action::Rope, KB).len(), 1);
    let olx = Bindings::olx().pad;
    assert!(b.pad.len() == olx.len() && olx.iter().all(|p| b.pad.contains(p)), "every pad input restored");
    assert!(b.repair().is_empty(), "repaired once");
    assert!(Bindings::olx().clone().repair().is_empty());
}

#[test]
fn a_file_without_the_pad_section_takes_olx_pad_defaults() {
    let b: Bindings = ron::from_str("(keys: [(Up, KeyW)])").unwrap();
    assert_eq!(b.keys, [(Action::Up, KeyCode::KeyW)]);
    assert_eq!(b.pad, Bindings::olx().pad);
    let b: Bindings = ron::from_str("()").unwrap();
    assert_eq!(b, Bindings::olx());
}

#[test]
fn capture_takes_the_first_input_of_its_device() {
    let mut c = Capture::new(KB);
    assert_eq!(c.device(), KB);
    assert_eq!(c.result(), None);
    c.on_pad(PadEvent::Button(Button::South, 1.0));
    c.on_key(KeyCode::KeyQ, false);
    assert_eq!(c.result(), None, "the gamepad and releases are ignored");
    c.on_key(KeyCode::ControlLeft, true);
    c.on_key(KeyCode::KeyQ, true);
    assert_eq!(c.result(), Some(Binding::Key(KeyCode::ControlLeft)), "modifiers too, the first wins");

    let mut c = Capture::new(PAD);
    c.on_key(KeyCode::KeyQ, true);
    // A trigger half pulled and a stick resting off centre are not presses.
    c.on_pad(PadEvent::Button(Button::RightTrigger2, 0.5));
    c.on_pad(PadEvent::Axis(Axis::LeftStickX, 0.3));
    assert_eq!(c.result(), None);
    c.on_pad(PadEvent::Axis(Axis::LeftStickY, -0.9));
    assert_eq!(c.result(), Some(Binding::Pad(PadInput::AxisMinus(Axis::LeftStickY))));
    let mut c = Capture::new(PAD);
    c.on_pad(PadEvent::Button(Button::RightTrigger2, 0.8));
    assert_eq!(c.result(), Some(Binding::Pad(PadInput::Button(Button::RightTrigger2))));
}

#[test]
fn inputs_have_names_a_player_reads() {
    let name = |b: Binding| b.to_string();
    assert_eq!(name(Binding::Key(KeyCode::ControlLeft)), "Left Ctrl");
    assert_eq!(name(Binding::Key(KeyCode::ArrowUp)), "Arrow Up");
    assert_eq!(name(Binding::Key(KeyCode::KeyX)), "X");
    assert_eq!(name(Binding::Key(KeyCode::Digit4)), "4");
    assert_eq!(name(Binding::Pad(PadInput::Button(Button::South))), "Pad A");
    assert_eq!(name(Binding::Pad(PadInput::Button(Button::RightTrigger2))), "Pad RT");
    assert_eq!(name(Binding::Pad(PadInput::AxisMinus(Axis::LeftStickX))), "Pad left stick left");
    assert_eq!(name(Binding::Pad(PadInput::AxisPlus(Axis::RightStickY))), "Pad right stick up");
}
