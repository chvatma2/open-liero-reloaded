//! `olr-input`: rebinding (plan 7 task 4, Review Focus 2): conflicts are
//! resolved explicitly, no action is ever left without an input on a
//! device, a hand-edited file is repaired, and the options capture the
//! next key, gamepad input or mouse input; with mouse controls (issue
//! #25) their own keys and mouse inputs, under their own rule.

use olr_input::{
    Action, Axis, Binding, Bindings, Button, Capture, Device, KeyCode, MOUSE_BUTTONS, MOUSE_KEYS, MOUSE_MADE,
    Mode, MouseInput, Need, PadEvent, PadInput, RebindError, Resolve,
};

const KB: Device = Device::Keyboard;
const PAD: Device = Device::Gamepad;
const MOUSE: Device = Device::Mouse;
const K: Mode = Mode::Keys;
const M: Mode = Mode::Mouse;

#[test]
fn bindings_list_and_find_inputs_by_action() {
    let b = Bindings::olx();
    assert_eq!(b.of(K, Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert_eq!(
        b.of(K, Action::Left, PAD),
        [
            Binding::Pad(PadInput::Button(Button::DPadLeft)),
            Binding::Pad(PadInput::AxisMinus(Axis::LeftStickX))
        ]
    );
    assert_eq!(b.actions_of(K, Binding::Key(KeyCode::KeyZ)), [Action::Rope]);
    assert!(b.actions_of(K, Binding::Key(KeyCode::KeyQ)).is_empty());
    assert_eq!(b.actions_of(K, Binding::Key(KeyCode::KeyX)), [Action::Strafe]);
    assert!(b.unreachable().is_empty());
    assert_eq!(Binding::Key(KeyCode::KeyQ).device(), KB);
    assert_eq!(Binding::Pad(PadInput::Button(Button::South)).device(), PAD);
}

#[test]
fn binding_a_used_key_is_resolved_explicitly() {
    let mut b = Bindings::olx();
    let ctrl = Binding::Key(KeyCode::ControlLeft);
    let z = Binding::Key(KeyCode::KeyQ);
    // Shared: both actions keep it.
    b.bind(K, Action::Jump, ctrl, Resolve::Share).unwrap();
    assert_eq!(b.actions_of(K, ctrl), [Action::Jump, Action::Shoot]);
    // Moved: it leaves every other action (Shoot has no other key: refused,
    // nothing changes).
    let before = b.clone();
    assert_eq!(
        b.bind(K, Action::Rope, ctrl, Resolve::Move),
        Err(RebindError::Unreachable { action: Action::Shoot, need: Need::Key })
    );
    assert_eq!(b, before, "a refused move changes nothing");
    // With another key on Shoot the move is allowed.
    b.bind(K, Action::Shoot, z, Resolve::Share).unwrap();
    b.bind(K, Action::Rope, ctrl, Resolve::Move).unwrap();
    assert_eq!(b.actions_of(K, ctrl), [Action::Rope]);
    assert_eq!(b.of(K, Action::Shoot, KB), [z]);
    assert_eq!(b.of(K, Action::Jump, KB), [Binding::Key(KeyCode::AltLeft)]);
    // Binding what an action already has changes nothing.
    let before = b.clone();
    b.bind(K, Action::Rope, ctrl, Resolve::Share).unwrap();
    assert_eq!(b, before);
    assert!(b.unreachable().is_empty());
}

#[test]
fn the_last_input_of_a_device_cannot_be_unbound() {
    let mut b = Bindings::olx();
    let x = Binding::Key(KeyCode::KeyZ);
    assert_eq!(
        b.unbind(K, Action::Rope, x),
        Err(RebindError::Unreachable { action: Action::Rope, need: Need::Key })
    );
    b.bind(K, Action::Rope, Binding::Key(KeyCode::KeyC), Resolve::Share).unwrap();
    b.unbind(K, Action::Rope, x).unwrap();
    assert_eq!(b.of(K, Action::Rope, KB), [Binding::Key(KeyCode::KeyC)]);
    // Left has two pad inputs: one goes, the other stays and cannot.
    let dpad = Binding::Pad(PadInput::Button(Button::DPadLeft));
    let stick = Binding::Pad(PadInput::AxisMinus(Axis::LeftStickX));
    b.unbind(K, Action::Left, dpad).unwrap();
    assert_eq!(
        b.unbind(K, Action::Left, stick),
        Err(RebindError::Unreachable { action: Action::Left, need: Need::PadInput })
    );
    // Unbinding what the action does not have is no error and no change.
    let before = b.clone();
    b.unbind(K, Action::Left, dpad).unwrap();
    assert_eq!(b, before);
    // The keyboard and the pad on one action are no conflict.
    b.bind(K, Action::Shoot, Binding::Pad(PadInput::Button(Button::LeftThumb)), Resolve::Move).unwrap();
    assert_eq!(b.of(K, Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert!(b.unreachable().is_empty());
    assert!(
        RebindError::Unreachable { action: Action::Shoot, need: Need::Key }.to_string().contains("Shoot")
    );
}

#[test]
fn repair_restores_olx_defaults_for_unreachable_actions() {
    let mut b = Bindings::olx();
    b.keys.retain(|(a, _)| *a != Action::Shoot);
    b.keys.push((Action::Jump, KeyCode::ControlLeft));
    b.pad.clear();
    // A pair listed twice is listed once.
    b.keys.push((Action::Rope, KeyCode::KeyZ));
    let w = b.repair();
    assert!(w.iter().any(|w| w.contains("Shoot") && w.contains("Left Ctrl")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("gamepad")), "{w:?}");
    assert!(b.unreachable().is_empty());
    assert_eq!(b.actions_of(K, Binding::Key(KeyCode::ControlLeft)), [Action::Jump, Action::Shoot]);
    assert_eq!(b.of(K, Action::Rope, KB).len(), 1);
    let olx = Bindings::olx().pad;
    assert!(b.pad.len() == olx.len() && olx.iter().all(|p| b.pad.contains(p)), "every pad input restored");
    assert!(b.repair().is_empty(), "repaired once");
    assert!(Bindings::olx().clone().repair().is_empty());
}

#[test]
fn the_new_actions_rebind_and_an_older_file_gets_their_defaults() {
    // Strafe, Previous/Next weapon and Dig rebind like any action.
    let mut b = Bindings::olx();
    let q = Binding::Key(KeyCode::KeyQ);
    b.bind(K, Action::Dig, q, Resolve::Share).unwrap();
    b.unbind(K, Action::Dig, Binding::Key(KeyCode::KeyC)).unwrap();
    assert_eq!(b.of(K, Action::Dig, KB), [q]);
    assert_eq!(
        b.unbind(K, Action::Dig, q),
        Err(RebindError::Unreachable { action: Action::Dig, need: Need::Key })
    );
    let rb = Binding::Pad(PadInput::Button(Button::RightTrigger));
    assert_eq!(
        b.bind(K, Action::Shoot, rb, Resolve::Move),
        Err(RebindError::Unreachable { action: Action::NextWeapon, need: Need::PadInput })
    );

    // An options file from before them: OLX's (and Dig's) defaults, one
    // warning per device and action.
    let old = Bindings::olx();
    let mut b = old.clone();
    let new = [Action::Strafe, Action::PreviousWeapon, Action::NextWeapon, Action::Dig];
    b.keys.retain(|(a, _)| !new.contains(a));
    b.pad.retain(|(a, _)| !new.contains(a));
    let w = b.repair();
    assert_eq!(w.len(), 8, "{w:?}");
    assert!(w.iter().any(|w| w.contains("Strafe") && w.contains("X")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("Dig") && w.contains("Pad B")), "{w:?}");
    assert!(b.unreachable().is_empty());
    for a in new {
        assert_eq!(b.of(K, a, KB), old.of(K, a, KB));
        assert_eq!(b.of(K, a, PAD), old.of(K, a, PAD));
    }
}

/// Issue #48: Weapon 1-5 rebind like any action, need no pad input (OLX
/// binds none), and a file from before them gets their keys.
#[test]
fn the_weapon_keys_rebind_need_no_pad_and_an_older_file_gets_their_defaults() {
    let mut b = Bindings::olx();
    let q = Binding::Key(KeyCode::KeyQ);
    b.bind(K, Action::Weapon2, q, Resolve::Share).unwrap();
    b.unbind(K, Action::Weapon2, Binding::Key(KeyCode::Digit2)).unwrap();
    assert_eq!(b.of(K, Action::Weapon2, KB), [q]);
    assert_eq!(
        b.unbind(K, Action::Weapon2, q),
        Err(RebindError::Unreachable { action: Action::Weapon2, need: Need::Key })
    );
    let one = Binding::Key(KeyCode::Digit1);
    assert_eq!(
        b.bind(K, Action::Rope, one, Resolve::Move),
        Err(RebindError::Unreachable { action: Action::Weapon1, need: Need::Key })
    );
    // A pad input binds, and unbinds again: none is needed.
    assert!(b.of(K, Action::Weapon1, PAD).is_empty());
    assert!(b.unreachable().is_empty());
    let x = Binding::Pad(PadInput::Button(Button::DPadUp));
    b.bind(K, Action::Weapon1, x, Resolve::Share).unwrap();
    assert_eq!(b.of(K, Action::Weapon1, PAD), [x]);
    assert_eq!(b.actions_of(K, x), [Action::Up, Action::Weapon1]);
    b.unbind(K, Action::Weapon1, x).unwrap();
    assert!(b.of(K, Action::Weapon1, PAD).is_empty());
    // Mouse controls' keys have them too, and need them.
    assert_eq!(b.of(M, Action::Weapon5, KB), [Binding::Key(KeyCode::Digit5)]);
    assert_eq!(
        b.unbind(M, Action::Weapon5, Binding::Key(KeyCode::Digit5)),
        Err(RebindError::Unreachable { action: Action::Weapon5, need: Need::KeyOrMouse })
    );

    // An options file from before them: OLX's keys (1-5, 6-0 for
    // keyboard 2, mouse controls' 1-5), one warning per list and action;
    // still no pad input.
    let old = Bindings::olx();
    let mut b = old.clone();
    let new = Action::WEAPONS;
    b.keys.retain(|(a, _)| !new.contains(a));
    b.mouse_keys.retain(|(a, _)| !new.contains(a));
    b.keys2.retain(|(a, _)| !new.contains(a));
    let w = b.repair();
    assert_eq!(w.len(), 15, "{w:?}");
    assert!(w.iter().any(|w| w.contains("Weapon 3") && w.contains("3")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("Weapon 5") && w.contains("player 2") && w.contains("0")), "{w:?}");
    assert!(b.unreachable().is_empty());
    assert_eq!(b, old);
    // The same file read: the lists given are kept as written.
    let text = ron::to_string(&Bindings { keys: vec![(Action::Up, KeyCode::KeyW)], ..old.clone() }).unwrap();
    let mut read: Bindings = ron::from_str(&text).unwrap();
    assert!(read.of(K, Action::Weapon1, KB).is_empty());
    read.repair();
    assert_eq!(read.of(K, Action::Weapon1, KB), [one]);
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
    assert_eq!(name(Binding::Key(KeyCode::KeyZ)), "Z");
    assert_eq!(name(Binding::Key(KeyCode::Digit4)), "4");
    assert_eq!(name(Binding::Pad(PadInput::Button(Button::South))), "Pad A");
    assert_eq!(name(Binding::Pad(PadInput::Button(Button::RightTrigger2))), "Pad RT");
    assert_eq!(name(Binding::Pad(PadInput::AxisMinus(Axis::LeftStickX))), "Pad left stick left");
    assert_eq!(name(Binding::Pad(PadInput::AxisPlus(Axis::RightStickY))), "Pad right stick up");
    assert_eq!(Action::PreviousWeapon.name(), "Previous weapon");
    assert_eq!(Action::Dig.name(), "Dig");
}

const LMB: Binding = Binding::Mouse(MouseInput::Left);
const RMB: Binding = Binding::Mouse(MouseInput::Right);
const MMB: Binding = Binding::Mouse(MouseInput::Middle);

#[test]
fn every_mouse_input_is_captured_and_bound() {
    for input in MouseInput::ALL {
        let mut c = Capture::new(MOUSE);
        assert_eq!(c.device(), MOUSE);
        c.on_key(KeyCode::KeyQ, true);
        c.on_pad(PadEvent::Button(Button::South, 1.0));
        match input {
            MouseInput::WheelUp => {
                c.on_wheel(0.5);
                assert_eq!(c.result(), None, "half a notch");
                c.on_wheel(0.5);
            }
            MouseInput::WheelDown => c.on_wheel(-3.0),
            button => {
                c.on_mouse_button(button, false);
                assert_eq!(c.result(), None, "a release is no press");
                c.on_mouse_button(button, true);
            }
        }
        assert_eq!(c.result(), Some(Binding::Mouse(input)), "{input:?}");
        c.on_mouse_button(MouseInput::Left, true);
        assert_eq!(c.result(), Some(Binding::Mouse(input)), "the first wins");

        // Bound to Jump (sharing what has it): Jump has it, nobody loses one.
        let mut b = Bindings::olx();
        b.bind(M, Action::Jump, Binding::Mouse(input), Resolve::Share).unwrap();
        assert!(b.of(M, Action::Jump, MOUSE).contains(&Binding::Mouse(input)));
        assert!(b.actions_of(M, Binding::Mouse(input)).contains(&Action::Jump));
        assert!(b.unreachable().is_empty());
        assert_eq!(b.keys, Bindings::olx().keys, "the keyboard's bindings untouched");
    }
    // The keyboard's and the pad's captures take no mouse input, and the
    // mouse's none of theirs.
    for d in [KB, PAD] {
        let mut c = Capture::new(d);
        c.on_mouse_button(MouseInput::Left, true);
        c.on_wheel(2.0);
        assert_eq!(c.result(), None);
    }
    // A wheel turned one way then the other starts counting afresh.
    let mut c = Capture::new(MOUSE);
    c.on_wheel(0.8);
    c.on_wheel(-0.5);
    c.on_wheel(0.8);
    assert_eq!(c.result(), None);
    // Esc cancels a mouse or pad capture; it is the keyboard's key.
    for d in [MOUSE, PAD] {
        let mut c = Capture::new(d);
        c.on_key(KeyCode::Escape, true);
        assert!(c.cancelled() && c.result().is_none());
    }
    let mut c = Capture::new(KB);
    c.on_key(KeyCode::Escape, true);
    assert!(!c.cancelled());
    assert_eq!(c.result(), Some(Binding::Key(KeyCode::Escape)));
    assert_eq!(Binding::Mouse(MouseInput::Back).to_string(), "Back button");
    assert_eq!(LMB.device(), MOUSE);
}

#[test]
fn mouse_mode_has_its_own_keys_and_rule() {
    let b = Bindings::olx();
    // The defaults: #24's.
    assert_eq!(b.mouse_keys, MOUSE_KEYS);
    assert_eq!(b.mouse, MOUSE_BUTTONS);
    assert_eq!(b.of(M, Action::Left, KB), [Binding::Key(KeyCode::KeyA)]);
    assert_eq!(b.of(K, Action::Left, KB), [Binding::Key(KeyCode::ArrowLeft)]);
    assert_eq!(b.of(M, Action::Shoot, MOUSE), [LMB]);
    assert_eq!(b.of(M, Action::Left, PAD), b.of(K, Action::Left, PAD), "one gamepad list");
    assert_eq!(b.actions_of(M, Binding::Key(KeyCode::KeyA)), [Action::Left]);
    assert_eq!(b.actions_of(K, Binding::Key(KeyCode::KeyA)), [Action::PreviousWeapon]);
    assert_eq!(Mode::of(true), M);
    assert_eq!(M.devices(), [KB, MOUSE, PAD]);
    assert_eq!(K.devices(), [KB, PAD]);

    // Mouse mode: an action needs a key or a mouse input, either one.
    let mut b = Bindings::olx();
    assert_eq!(
        b.unbind(M, Action::Shoot, LMB),
        Err(RebindError::Unreachable { action: Action::Shoot, need: Need::KeyOrMouse })
    );
    let f = Binding::Key(KeyCode::KeyF);
    b.bind(M, Action::Shoot, f, Resolve::Share).unwrap();
    b.unbind(M, Action::Shoot, LMB).unwrap();
    assert_eq!(b.of(M, Action::Shoot, KB), [f]);
    assert!(b.of(M, Action::Shoot, MOUSE).is_empty());
    assert_eq!(
        b.unbind(M, Action::Shoot, f),
        Err(RebindError::Unreachable { action: Action::Shoot, need: Need::KeyOrMouse })
    );
    assert_eq!(b.keys, Bindings::olx().keys, "mouse mode never touches the keyboard's");
    assert!(b.unreachable().is_empty());
    // A key mouse mode's Jump has, moved to Rope: refused (Jump has no
    // other), shared: fine; the keyboard's Space is no one's.
    let space = Binding::Key(KeyCode::Space);
    let before = b.clone();
    assert_eq!(
        b.bind(M, Action::Rope, space, Resolve::Move),
        Err(RebindError::Unreachable { action: Action::Jump, need: Need::KeyOrMouse })
    );
    assert_eq!(b, before);
    b.bind(M, Action::Rope, space, Resolve::Share).unwrap();
    assert_eq!(b.actions_of(M, space), [Action::Jump, Action::Rope]);
    assert!(b.actions_of(K, space).is_empty());

    // Up, Down, SelectWeapon and Strafe are the controller's: they need
    // nothing and take nothing in mouse mode.
    for a in MOUSE_MADE {
        assert!(b.of(M, a, KB).is_empty() && b.of(M, a, MOUSE).is_empty());
        for input in [Binding::Key(KeyCode::KeyW), MMB] {
            assert_eq!(
                b.bind(M, a, input, Resolve::Share),
                Err(RebindError::NotBindable { action: a, device: input.device() })
            );
        }
        assert!(!b.of(M, a, PAD).is_empty(), "the pad keeps them for the keys");
    }
    // The mouse is bound only in mouse mode.
    assert_eq!(
        b.bind(K, Action::Jump, MMB, Resolve::Share),
        Err(RebindError::NotBindable { action: Action::Jump, device: MOUSE })
    );
    assert!(RebindError::NotBindable { action: Action::Up, device: KB }.to_string().contains("Up"));
    assert!(
        RebindError::Unreachable { action: Action::Rope, need: Need::KeyOrMouse }
            .to_string()
            .contains("mouse")
    );
}

#[test]
fn the_unreachable_rule_holds_in_both_modes() {
    // Keys mode: today's rule, unchanged by the mouse lists.
    let mut b = Bindings::olx();
    b.mouse.clear();
    b.mouse_keys.clear();
    assert_eq!(
        b.unbind(K, Action::Rope, Binding::Key(KeyCode::KeyZ)).map_err(|e| e.to_string()),
        Err("Rope would have no key".to_string())
    );
    // Both modes' gaps are listed: every action but the controller's lacks
    // a key or mouse input here.
    let gaps = b.unreachable();
    let needing: Vec<Action> = Action::ALL.into_iter().filter(|a| !MOUSE_MADE.contains(a)).collect();
    assert_eq!(gaps, needing.iter().map(|a| (*a, Need::KeyOrMouse)).collect::<Vec<_>>());
    let mut keys = Bindings::olx();
    keys.keys.retain(|(a, _)| *a != Action::Up);
    assert_eq!(keys.unreachable(), [(Action::Up, Need::Key)], "Up needs a key with mouse controls off");

    // Every move and unbind of every input of each mode keeps the rule.
    let inputs: Vec<Binding> = MouseInput::ALL
        .into_iter()
        .map(Binding::Mouse)
        .chain([KeyCode::KeyA, KeyCode::KeyD, KeyCode::Space, KeyCode::KeyS, KeyCode::KeyZ].map(Binding::Key))
        // Keyboard 2's (issue #28).
        .chain([KeyCode::Numpad8, KeyCode::NumpadEnter, KeyCode::Numpad3].map(Binding::Key))
        .chain([Binding::Pad(PadInput::Button(Button::South))])
        .collect();
    for mode in [K, M, Mode::Keyboard2] {
        for &input in &inputs {
            for action in Action::ALL {
                for how in [Resolve::Move, Resolve::Share] {
                    let mut b = Bindings::olx();
                    let _ = b.bind(mode, action, input, how);
                    assert!(b.unreachable().is_empty(), "{mode:?} {action:?} {input:?} {how:?}: {b:?}");
                    for a in Action::ALL {
                        for d in Device::ALL {
                            for i in b.of(mode, a, d) {
                                let _ = b.unbind(mode, a, i);
                                assert!(b.unreachable().is_empty(), "{mode:?} unbinding {i:?} from {a:?}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_file_from_before_mouse_bindings_is_repaired() {
    // #24's options file: no mouse lists. It gets #24's defaults, and its
    // keyboard's bindings stay as they were.
    let b: Bindings = ron::from_str("(keys: [(Up, KeyW)], pad: [])").unwrap();
    assert_eq!(b.mouse_keys, MOUSE_KEYS);
    assert_eq!(b.mouse, MOUSE_BUTTONS);
    let mut b = b;
    b.repair();
    assert_eq!(b.keys[0], (Action::Up, KeyCode::KeyW));
    assert!(b.unreachable().is_empty());
    assert_eq!(b.mouse, MOUSE_BUTTONS);

    // Emptied by hand, or a controller action bound: repaired with #24's
    // defaults, one warning each.
    let mut b = Bindings::olx();
    b.mouse.retain(|(a, _)| *a != Action::Rope);
    b.mouse_keys.retain(|(a, _)| *a != Action::Jump);
    b.mouse_keys.push((Action::Up, KeyCode::KeyW));
    b.mouse.push((Action::Strafe, MouseInput::Middle));
    let keys = b.keys.clone();
    let w = b.repair();
    assert_eq!(w.len(), 4, "{w:?}");
    assert!(w.iter().any(|w| w.contains("Rope") && w.contains("Right click")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("Jump") && w.contains("Space")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("Up") && w.contains("W")), "{w:?}");
    assert_eq!(b.of(M, Action::Rope, MOUSE), [RMB]);
    assert!(b.of(M, Action::Up, KB).is_empty() && b.of(M, Action::Strafe, MOUSE).is_empty());
    assert_eq!(b.keys, keys, "the keyboard's untouched");
    assert!(b.unreachable().is_empty());
    assert!(b.repair().is_empty(), "repaired once");
}
