//! Options (plan 7 task 4): key bindings that always keep the game
//! controllable (Review Focus 2), rebinding through the widgets with the
//! "press a key" capture, and the video and audio sections.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_input::{
    Action, Binding, Bindings, Button, Device, KeyCode, MOUSE_BUTTONS, MOUSE_KEYS, Mode, MouseInput,
    PAD_OPTIONAL, PadEvent, PadInput, RebindError, Resolve,
};
use olr_ui::screens::{self, MenuData};
use olr_ui::{
    AppCommand, AudioSettings, DownloadCacheView, FrameRate, HudArrangement, Profile, Profiles, Screen,
    Settings, Store, UiAction, UiState, UpscaleFilter, VideoSettings, WindowPreset,
};

const KB: Device = Device::Keyboard;
const PAD: Device = Device::Gamepad;
const MOUSE: Device = Device::Mouse;
const K: Mode = Mode::Keys;
const M: Mode = Mode::Mouse;

/// Keyboard 1's Weapon 1-5 keys (issue #48), as an options file lists them.
const WEAPON_KEYS: &str =
    "(Weapon1, Digit1), (Weapon2, Digit2), (Weapon3, Digit3), (Weapon4, Digit4), (Weapon5, Digit5)";

/// Every action can be pressed with one key alone, and with one pad input
/// (but Weapon 1-5, `PAD_OPTIONAL`, which need none).
fn controllable(b: &Bindings) -> bool {
    Action::ALL.iter().all(|a| {
        let key = b.keys.iter().any(|(_, k)| b.key_bits(&[*k]) & a.bit() != 0);
        let pad = b.pad.iter().any(|(_, p)| b.pad_bits(|q| q == *p) & a.bit() != 0);
        key && (pad || PAD_OPTIONAL.contains(a))
    })
}

/// A small deterministic generator (no rand in the tests' dependencies).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

#[test]
fn rebinding_never_leaves_an_action_unreachable() {
    // Any sequence of rebinds: keys used by other actions moved or shared,
    // every key of an action unbound, the keyboard and the pad on one
    // action. Refused steps change nothing; every step keeps the game
    // controllable.
    let inputs: Vec<Binding> = [
        KeyCode::ArrowUp,
        KeyCode::ArrowLeft,
        KeyCode::ControlLeft,
        KeyCode::AltLeft,
        KeyCode::KeyX,
        KeyCode::KeyZ,
        KeyCode::Space,
    ]
    .into_iter()
    .map(Binding::Key)
    .chain(
        [Button::South, Button::North, Button::DPadUp, Button::RightTrigger2]
            .into_iter()
            .map(|b| Binding::Pad(PadInput::Button(b))),
    )
    .collect();
    let mut rng = Lcg(7);
    let mut b = Bindings::olx();
    let (mut refused, mut moved) = (0, 0);
    for _ in 0..5000 {
        let action = Action::ALL[rng.next(Action::ALL.len())];
        let input = inputs[rng.next(inputs.len())];
        let before = b.clone();
        let r = match rng.next(3) {
            0 => b.bind(K, action, input, Resolve::Share),
            1 => b.bind(K, action, input, Resolve::Move).inspect(|_| moved += 1),
            _ => b.unbind(K, action, input),
        };
        if let Err(RebindError::Unreachable { .. }) = r {
            refused += 1;
            assert_eq!(b, before, "a refused step changes nothing");
        }
        assert!(b.unreachable().is_empty() && controllable(&b), "after {action:?} {input:?}: {b:?}");
    }
    assert!(refused > 100 && moved > 100, "the walk tried both: {refused} refused, {moved} moved");

    // Bindings that leave actions unreachable, from anywhere, are repaired
    // when the menus get them: OLX's defaults for what is missing.
    let mut broken = Bindings::olx();
    broken.keys.retain(|(a, _)| *a != Action::Shoot);
    broken.pad.clear();
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let out = ui.update(UiAction::SetBindings(broken));
    assert!(controllable(&ui.bindings()));
    assert!(out.contains(&AppCommand::ApplyBindings(ui.bindings().clone())));
    assert_eq!(ui.bindings().of(K, Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);

    // A hand-edited settings file: Shoot has no key and the pad section is
    // gone. It loads with Shoot's OLX key and OLX's pad, a warning, and the
    // rest of the file kept (not reset).
    let tmp = tempfile::tempdir().unwrap();
    let text = "(version: 1, settings: (video: (window: W1920x1080), input: (keys: [(Up, KeyW), (Down, KeyS), \
                (Left, KeyA), (Right, KeyD), (Jump, Space), (Rope, KeyE), (SelectWeapon, ShiftLeft)])))";
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert_eq!(got.value.video.window, WindowPreset::W1920x1080, "the rest of the file is kept");
    let input = &got.value.input;
    assert_eq!(input.of(K, Action::Up, KB), [Binding::Key(KeyCode::KeyW)]);
    assert_eq!(input.of(K, Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert_eq!(input.pad, Bindings::olx().pad);
    assert!(got.warnings.iter().any(|w| w.contains("Shoot")), "{:?}", got.warnings);
    // The fixed file was saved: it loads again without a warning.
    let again = Store::open(tmp.path()).load_settings();
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(again.value, got.value);

    // A profile's own keys are repaired the same way.
    let mut own = Bindings::olx();
    own.keys.retain(|(a, _)| *a != Action::Up);
    let mut p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    let w = p.sanitize();
    assert!(w.iter().any(|w| w.contains("Up")), "{w:?}");
    assert!(controllable(p.list[0].bindings.as_ref().unwrap()));
}

/// The menus and what they asked the app to do.
struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(ui: UiState) -> Harness<'static, App> {
    let app = App { ui, data: MenuData::new(Default::default(), None), commands: Vec::new() };
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

/// What the app does with a key pressed while the options capture one.
fn press(h: &mut Harness<'static, App>, key: KeyCode) {
    h.state_mut().data.capture.as_mut().expect("capturing").on_key(key, true);
    h.run();
}

fn keys(h: &Harness<'static, App>, action: Action) -> Vec<Binding> {
    h.state().ui.bindings().of(K, action, KB)
}

/// Options → Keys' seat chooser (issue #28): "Keyboard 1", "Keyboard 2"
/// or "Gamepads".
fn seat(h: &mut Harness<'static, App>, name: &str) {
    tap(h, name);
}

/// Clicks the widget labelled `label`, scrolled into view first (the Keys
/// tab's rows run below a 720-pixel screen since Weapon 1-5, issue #48).
fn tap(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

fn open_keys(h: &mut Harness<'static, App>) {
    tap(h, "Options");
    assert_eq!(h.state().ui.screen(), Screen::Options);
    tap(h, "Keys");
}

#[test]
fn options_rebind_a_key() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    let (z, ctrl, alt) =
        (Binding::Key(KeyCode::KeyQ), Binding::Key(KeyCode::ControlLeft), Binding::Key(KeyCode::AltLeft));

    // "Press a key" (OLX's "Input for:" box): a free key is added.
    tap(&mut h, "Add key for Jump");
    h.get_by_label_contains("Press a key for Jump");
    assert_eq!(h.state().data.capture.as_ref().map(|c| c.device()), Some(KB));
    press(&mut h, KeyCode::KeyQ);
    assert!(h.state().data.capture.is_none());
    assert_eq!(keys(&h, Action::Jump), [alt, z]);
    let applied = AppCommand::ApplyBindings(h.state().ui.bindings().clone());
    assert!(h.state().commands.contains(&applied));
    assert!(matches!(h.state().commands.last(), Some(AppCommand::SaveSettings(_))));

    // A key another action has: shown, and moved on request.
    tap(&mut h, "Add key for Shoot");
    press(&mut h, KeyCode::KeyQ);
    h.get_by_label("Q is Jump's.");
    assert_eq!(keys(&h, Action::Shoot), [ctrl], "nothing changes before the choice");
    tap(&mut h, "Move it to Shoot");
    assert_eq!(keys(&h, Action::Shoot), [ctrl, z]);
    assert_eq!(keys(&h, Action::Jump), [alt]);

    // Moving Shoot's last key away is not offered: shared instead.
    tap(&mut h, "Remove Left Ctrl from Shoot");
    assert_eq!(keys(&h, Action::Shoot), [z]);
    assert!(h.get_by_label("Remove Q from Shoot").accesskit_node().is_disabled(), "the last key stays");
    tap(&mut h, "Add key for Rope");
    press(&mut h, KeyCode::KeyQ);
    assert!(h.get_by_label("Move it to Rope").accesskit_node().is_disabled());
    h.get_by_label_contains("Shoot would have no key");
    tap(&mut h, "Use it for both");
    assert_eq!(keys(&h, Action::Shoot), [z]);
    assert_eq!(keys(&h, Action::Rope), [Binding::Key(KeyCode::KeyZ), z]);

    // Cancel leaves everything as it was.
    let before = h.state().ui.bindings().clone();
    tap(&mut h, "Add key for Up");
    press(&mut h, KeyCode::KeyQ);
    tap(&mut h, "Cancel");
    assert_eq!(h.state().ui.bindings(), before);

    // The game's own keys are refused; Esc cancels and stays in options.
    tap(&mut h, "Add key for Up");
    press(&mut h, KeyCode::F2);
    h.get_by_label_contains("F2 is the game's");
    assert_eq!(h.state().ui.bindings(), before);
    tap(&mut h, "Add key for Up");
    // The app feeds the capture every key and egui none (it swallows them).
    press(&mut h, KeyCode::Escape);
    assert!(h.state().data.capture.is_none());
    assert_eq!(h.state().ui.screen(), Screen::Options);
    assert_eq!(h.state().ui.bindings(), before);

    // A gamepad input on the same action as its keys: the Gamepads' page.
    assert!(h.query_by_label("Add pad input for Shoot").is_none());
    seat(&mut h, "Gamepads");
    tap(&mut h, "Add pad input for Shoot");
    assert_eq!(h.state().data.capture.as_ref().map(|c| c.device()), Some(PAD));
    h.state_mut().data.capture.as_mut().unwrap().on_pad(PadEvent::Button(Button::LeftThumb, 1.0));
    h.run();
    let west = Binding::Pad(PadInput::Button(Button::LeftThumb));
    assert_eq!(h.state().ui.bindings().of(K, Action::Shoot, PAD).last(), Some(&west));
    assert_eq!(keys(&h, Action::Shoot), [z]);

    // A browser shortcut is pointed out (Alt+F4 with Jump's Alt, spec §8.5).
    seat(&mut h, "Keyboard 1");
    tap(&mut h, "Add key for Jump");
    press(&mut h, KeyCode::F4);
    h.get_by_label_contains("Left Alt + F4");

    // Back to OLX's keys, then pad inputs: each page's Reset is its own.
    tap(&mut h, "Reset to OLX's keys");
    assert_eq!(h.state().ui.bindings().keys, Bindings::olx().keys);
    assert_eq!(h.state().ui.bindings().of(K, Action::Shoot, PAD).last(), Some(&west));
    seat(&mut h, "Gamepads");
    tap(&mut h, olr_ui::screens::RESET_PAD);
    assert_eq!(h.state().ui.bindings(), Bindings::olx());
    assert!(h.state().commands.contains(&AppCommand::ApplyBindings(Bindings::olx())));
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert!(h.state().ui.bindings().unreachable().is_empty());
}

/// Issue #23: Strafe, Previous/Next weapon and Dig are listed with their
/// defaults and rebind like the classic eight.
#[test]
fn the_keys_screen_lists_and_rebinds_the_new_actions() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    for a in [Action::Strafe, Action::PreviousWeapon, Action::NextWeapon, Action::Dig] {
        h.get_by_label(&format!("Add key for {}", a.name()));
    }
    h.get_by_label("Remove C from Dig");
    tap(&mut h, "Add key for Dig");
    press(&mut h, KeyCode::KeyQ);
    assert_eq!(keys(&h, Action::Dig), [Binding::Key(KeyCode::KeyC), Binding::Key(KeyCode::KeyQ)]);
    tap(&mut h, "Remove C from Dig");
    assert_eq!(keys(&h, Action::Dig), [Binding::Key(KeyCode::KeyQ)]);
    seat(&mut h, "Gamepads");
    for a in [Action::Strafe, Action::PreviousWeapon, Action::NextWeapon, Action::Dig] {
        h.get_by_label(&format!("Add pad input for {}", a.name()));
    }
    h.get_by_label("Remove Pad RB from Next weapon");
    tap(&mut h, "Add pad input for Strafe");
    h.state_mut().data.capture.as_mut().unwrap().on_pad(PadEvent::Button(Button::LeftThumb, 1.0));
    h.run();
    let thumb = Binding::Pad(PadInput::Button(Button::LeftThumb));
    assert_eq!(h.state().ui.bindings().of(K, Action::Strafe, PAD).last(), Some(&thumb));
}

/// Issue #48: Weapon 1-5 are listed with their keys (1-5, keyboard 2's
/// 6-0), have no pad input by default, rebind like any action, and a pad
/// input bound to one can be removed again.
#[test]
fn the_keys_screen_lists_and_rebinds_the_weapon_keys() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    for (i, a) in Action::WEAPONS.into_iter().enumerate() {
        h.get_by_label(&format!("Remove {} from {}", i + 1, a.name()));
    }
    tap(&mut h, "Add key for Weapon 3");
    press(&mut h, KeyCode::KeyE);
    assert_eq!(keys(&h, Action::Weapon3), [Binding::Key(KeyCode::Digit3), Binding::Key(KeyCode::KeyE)]);
    tap(&mut h, "Remove 3 from Weapon 3");
    assert_eq!(keys(&h, Action::Weapon3), [Binding::Key(KeyCode::KeyE)]);
    seat(&mut h, "Keyboard 2");
    for (a, k) in Action::WEAPONS.into_iter().zip(["6", "7", "8", "9", "0"]) {
        h.get_by_label(&format!("Remove {k} from {}", a.name()));
    }
    seat(&mut h, "Gamepads");
    tap(&mut h, "Add pad input for Weapon 1");
    h.state_mut().data.capture.as_mut().unwrap().on_pad(PadEvent::Button(Button::LeftThumb, 1.0));
    h.run();
    let thumb = Binding::Pad(PadInput::Button(Button::LeftThumb));
    assert_eq!(h.state().ui.bindings().of(K, Action::Weapon1, PAD), [thumb]);
    tap(&mut h, "Remove Pad left stick press from Weapon 1");
    assert!(h.state().ui.bindings().of(K, Action::Weapon1, PAD).is_empty(), "a pad needs none");
    assert!(h.state().ui.bindings().unreachable().is_empty());
}

/// Issue #48: an options file from before Weapon 1-5 loads with OLX's
/// keys for them (1-5, keyboard 2's 6-0, mouse controls' 1-5), one
/// warning per list and key, and no pad input.
#[test]
fn an_options_file_from_before_the_weapon_keys_gets_their_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let mut old = Bindings::olx();
    for list in [&mut old.keys, &mut old.mouse_keys, &mut old.keys2] {
        list.retain(|(a, _)| !Action::WEAPONS.contains(a));
    }
    let text = format!("(version: 1, settings: (input: {}))", ron::to_string(&old).unwrap());
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert_eq!(got.warnings.len(), 15, "{:?}", got.warnings);
    assert_eq!(got.value.input, Bindings::olx());
}

/// Issue #24: Options → Keys → Mouse controls (experimental), off by
/// default, applied and saved when switched.
#[test]
fn mouse_controls_are_switched_in_the_keys_tab() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    assert!(!Settings::default().mouse.enabled, "off by default");
    open_keys(&mut h);
    h.get_by_label(olr_ui::screens::MOUSE_NOTE);
    tap(&mut h, olr_ui::screens::MOUSE_CONTROLS);
    assert!(h.state().ui.settings().mouse.enabled);
    let c = &h.state().commands;
    assert!(c.contains(&AppCommand::ApplyMouseControls(true)), "{c:?}");
    assert!(c.iter().any(|c| matches!(c, AppCommand::SaveSettings(s) if s.mouse.enabled)), "{c:?}");
    tap(&mut h, olr_ui::screens::MOUSE_CONTROLS);
    assert!(!h.state().ui.settings().mouse.enabled);
    assert_eq!(h.state().commands.last().map(|c| matches!(c, AppCommand::SaveSettings(_))), Some(true));
}

fn mouse_inputs(h: &Harness<'static, App>, action: Action) -> Vec<Binding> {
    h.state().ui.bindings().of(M, action, MOUSE)
}

/// What the app does with a mouse button pressed while the options
/// capture a mouse input.
fn click(h: &mut Harness<'static, App>, button: MouseInput) {
    h.state_mut().data.capture.as_mut().expect("capturing").on_mouse_button(button, true);
    h.run();
}

/// Issue #25: with mouse controls on, the Keys tab shows their own keys, a
/// Mouse column and the gamepad, rebinds them under mouse mode's rule, and
/// leaves the keyboard's own bindings untouched.
#[test]
fn the_keys_screen_rebinds_mouse_controls() {
    use olr_ui::screens::{MOUSE_CONTROLS, MOUSE_KEYS_NOTE, MOUSE_MADE, RESET_KEYS, RESET_MOUSE};
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    assert!(h.query_by_label("Mouse").is_none() && h.query_by_label(MOUSE_KEYS_NOTE).is_none());
    tap(&mut h, MOUSE_CONTROLS);
    h.get_by_label(MOUSE_KEYS_NOTE);
    h.get_by_label("Mouse");
    h.get_by_label(RESET_MOUSE);
    assert!(h.query_by_label(RESET_KEYS).is_none());
    let (lmb, rmb, mmb) = (
        Binding::Mouse(MouseInput::Left),
        Binding::Mouse(MouseInput::Right),
        Binding::Mouse(MouseInput::Middle),
    );
    // The defaults: #24's; Up, Down, Select weapon and Strafe the mouse's.
    h.get_by_label("Remove A from Left");
    h.get_by_label("Remove Space from Jump");
    h.get_by_label("Remove Wheel down from Next weapon");
    assert!(h.get_by_label("Remove Left click from Shoot").accesskit_node().is_disabled(), "Shoot's last");
    assert!(h.query_by_label("Remove Arrow Left from Left").is_none(), "the keyboard's own are not shown");
    assert_eq!(h.get_all_by_label(MOUSE_MADE).count(), 8, "Up, Down, Select weapon, Strafe: key and mouse");
    assert!(
        h.query_by_label("Add key for Up").is_none()
            && h.query_by_label("Add mouse input for Strafe").is_none()
    );
    assert!(h.query_by_label("Add pad input for Up").is_none(), "the pads' are their own page");

    // A free mouse button: bound. The capture box has no Cancel to click.
    tap(&mut h, "Add mouse input for Rope");
    h.get_by_label_contains("Click a mouse button or turn the wheel for Rope");
    assert_eq!(h.state().data.capture.as_ref().map(|c| c.device()), Some(MOUSE));
    assert!(h.query_by_label("Cancel").is_none());
    click(&mut h, MouseInput::Middle);
    assert!(h.state().data.capture.is_none());
    assert_eq!(mouse_inputs(&h, Action::Rope), [rmb, mmb]);

    // A button another action has: moved on request (Rope keeps the middle one).
    tap(&mut h, "Add mouse input for Shoot");
    click(&mut h, MouseInput::Right);
    h.get_by_label("Right click is Rope's.");
    tap(&mut h, "Move it to Shoot");
    assert_eq!(mouse_inputs(&h, Action::Shoot), [lmb, rmb]);
    assert_eq!(mouse_inputs(&h, Action::Rope), [mmb]);
    tap(&mut h, "Remove Left click from Shoot");
    assert_eq!(mouse_inputs(&h, Action::Shoot), [rmb]);
    // Rope's last mouse input but a key: the middle button stays removable
    // only once Rope has a key.
    assert!(h.get_by_label("Remove Middle click from Rope").accesskit_node().is_disabled());
    tap(&mut h, "Add key for Rope");
    press(&mut h, KeyCode::KeyE);
    assert_eq!(h.state().ui.bindings().of(M, Action::Rope, KB), [Binding::Key(KeyCode::KeyE)]);
    assert!(!h.get_by_label("Remove Middle click from Rope").accesskit_node().is_disabled());

    // A wheel notch, shared with what has it.
    tap(&mut h, "Add mouse input for Dig");
    h.state_mut().data.capture.as_mut().unwrap().on_wheel(-1.0);
    h.run();
    h.get_by_label("Wheel down is Next weapon's.");
    tap(&mut h, "Use it for both");
    assert_eq!(mouse_inputs(&h, Action::Dig), [Binding::Mouse(MouseInput::WheelDown)]);
    assert_eq!(mouse_inputs(&h, Action::NextWeapon), [Binding::Mouse(MouseInput::WheelDown)]);

    // Esc cancels a mouse capture (the app feeds it the key, egui none).
    let before = h.state().ui.bindings().clone();
    tap(&mut h, "Add mouse input for Jump");
    h.state_mut().data.capture.as_mut().unwrap().on_key(KeyCode::Escape, true);
    h.run();
    assert!(h.state().data.capture.is_none());
    assert_eq!(h.state().ui.screen(), Screen::Options);
    assert_eq!(h.state().ui.bindings(), before);
    assert_eq!(h.state().ui.bindings().keys, Bindings::olx().keys, "the keyboard's untouched");
    assert!(h.state().ui.bindings().unreachable().is_empty());
    let applied = AppCommand::ApplyBindings(h.state().ui.bindings().clone());
    assert!(h.state().commands.contains(&applied));

    // Reset: mouse controls' defaults; the keyboard's and the pad's stay.
    tap(&mut h, RESET_MOUSE);
    let b = h.state().ui.bindings().clone();
    assert_eq!((b.mouse_keys.as_slice(), b.mouse.as_slice()), (&MOUSE_KEYS[..], &MOUSE_BUTTONS[..]));

    // Off: the keyboard's own keys, as they were; no Mouse column.
    tap(&mut h, MOUSE_CONTROLS);
    assert!(h.query_by_label("Mouse").is_none());
    h.get_by_label("Remove Arrow Left from Left");
    h.get_by_label("Add key for Up");
    assert_eq!(h.state().ui.bindings(), Bindings::olx());
}

/// Issue #25: an options file from before mouse bindings loads with #24's
/// mouse controls and its own keys kept; mouse lists that leave an action
/// unreachable with mouse controls get the defaults back, with a warning.
#[test]
fn an_options_file_from_before_mouse_bindings_is_repaired() {
    let tmp = tempfile::tempdir().unwrap();
    let text = format!(
        "(version: 1, settings: (input: (keys: [(Up, KeyW), (Down, KeyS), (Left, KeyA), (Right, KeyD), \
         (Shoot, ControlLeft), (Jump, Space), (Rope, KeyE), (SelectWeapon, ShiftLeft), (Strafe, KeyZ), \
         (PreviousWeapon, KeyQ), (NextWeapon, KeyR), (Dig, KeyC), {WEAPON_KEYS}]), mouse: (enabled: true)))"
    );
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    let input = &got.value.input;
    assert_eq!(input.of(K, Action::Up, KB), [Binding::Key(KeyCode::KeyW)]);
    assert_eq!((input.mouse_keys.as_slice(), input.mouse.as_slice()), (&MOUSE_KEYS[..], &MOUSE_BUTTONS[..]));
    assert!(input.unreachable().is_empty());

    // Mouse lists edited by hand: the game's keys dropped, the actions
    // left without a key or mouse input given #24's back.
    let text = "(version: 1, settings: (input: (mouse_keys: [(Left, KeyA), (Right, KeyD), (Jump, Tab), \
                (Up, KeyW)], mouse: [(Shoot, Middle)])))";
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    let input = &got.value.input;
    assert!(input.unreachable().is_empty());
    assert_eq!(input.keys, Bindings::olx().keys, "the keyboard's untouched");
    assert_eq!(input.of(M, Action::Shoot, MOUSE), [Binding::Mouse(MouseInput::Middle)]);
    assert_eq!(input.of(M, Action::Jump, KB), [Binding::Key(KeyCode::Space)]);
    assert_eq!(input.of(M, Action::Rope, MOUSE), [Binding::Mouse(MouseInput::Right)]);
    assert!(input.mouse_keys.iter().all(|(a, k)| *k != KeyCode::Tab && *a != Action::Up));
    for w in ["Tab", "Up", "Jump", "Rope", "Dig", "Previous weapon", "Next weapon"] {
        assert!(got.warnings.iter().any(|x| x.contains(w)), "{w}: {:?}", got.warnings);
    }
    let again = Store::open(tmp.path()).load_settings();
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
}

#[test]
fn a_profile_with_its_own_keys_is_rebound_there() {
    let own = Bindings::olx();
    let p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    let mut h = harness(UiState::new(Settings::default(), p, 1));
    open_keys(&mut h);
    h.get_by_label_contains("OpenLieroXor has its own keys");
    tap(&mut h, "Add key for Jump");
    press(&mut h, KeyCode::KeyQ);
    let profile = h.state().ui.profiles().list[0].bindings.clone().unwrap();
    assert!(profile.of(K, Action::Jump, KB).contains(&Binding::Key(KeyCode::KeyQ)));
    assert_eq!(h.state().ui.settings().input, Bindings::olx(), "the options' keys are not the ones used");
    assert!(matches!(h.state().commands.first(), Some(AppCommand::SaveProfiles(_))));
}

/// Issue #28: Options → Keys' seats. Keyboard 2 shows OLX's player 2
/// keys alone (no pad, no mouse controls) and rebinds under its own rule;
/// the Gamepads' page their inputs alone, Start refused; a key both
/// keyboards bind is shown on both pages; a profile with its own keys
/// still rebinds keyboard 2 in the options.
#[test]
fn the_keys_screen_rebinds_each_seat() {
    use olr_ui::screens::{MOUSE_CONTROLS, RESET_KEYS2, seat_note, shared_note};
    const K2: Mode = Mode::Keyboard2;
    let own = Bindings::olx();
    let p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    let mut h = harness(UiState::new(Settings::default(), p, 1));
    open_keys(&mut h);
    h.get_by_label(seat_note(olr_input::SeatKind::Keyboard1));
    seat(&mut h, "Keyboard 2");
    h.get_by_label(seat_note(olr_input::SeatKind::Keyboard2));
    assert!(h.query_by_label(MOUSE_CONTROLS).is_none(), "keyboard 1's alone");
    assert!(h.query_by_label("Add pad input for Shoot").is_none() && h.query_by_label("Gamepad").is_none());
    assert!(h.query_by_label("Remove Arrow Left from Left").is_none());
    for name in ["Num 8", "Num 5", "Num 4", "Num 6", "Num 0", "Num 7", "Num 9", "Num 3"] {
        assert_eq!(h.get_all_by_label_contains(&format!("Remove {name} from")).count(), 1, "{name}");
    }
    // Keyboard 2's last key stays; another, and it can go.
    assert!(h.get_by_label("Remove Num 3 from Dig").accesskit_node().is_disabled());
    tap(&mut h, "Add key for Dig");
    press(&mut h, KeyCode::Numpad2);
    let k2 = |h: &Harness<'static, App>, a| h.state().ui.bindings().of(K2, a, KB);
    assert_eq!(k2(&h, Action::Dig), [Binding::Key(KeyCode::Numpad3), Binding::Key(KeyCode::Numpad2)]);
    tap(&mut h, "Remove Num 3 from Dig");
    assert_eq!(k2(&h, Action::Dig), [Binding::Key(KeyCode::Numpad2)]);
    // The options' keyboard 2 changed, the profile's keys did not.
    assert_eq!(h.state().ui.settings().input.keys2, h.state().ui.bindings().keys2);
    assert_eq!(h.state().ui.profiles().list[0].bindings, Some(Bindings::olx()));
    assert!(h.state().ui.bindings().unreachable().is_empty());
    // The game's keys are refused here too.
    tap(&mut h, "Add key for Up");
    press(&mut h, KeyCode::F5);
    h.get_by_label_contains("F5 is the game's");

    // A key keyboard 1 has: bound to keyboard 2 and shown as a conflict,
    // on both keyboards' pages.
    assert!(h.query_by_label(&shared_note(KeyCode::ArrowUp)).is_none());
    tap(&mut h, "Add key for Jump");
    press(&mut h, KeyCode::ArrowUp);
    assert!(k2(&h, Action::Jump).contains(&Binding::Key(KeyCode::ArrowUp)));
    h.get_by_label(&shared_note(KeyCode::ArrowUp));
    seat(&mut h, "Keyboard 1");
    h.get_by_label(&shared_note(KeyCode::ArrowUp));
    seat(&mut h, "Keyboard 2");
    tap(&mut h, RESET_KEYS2);
    assert_eq!(h.state().ui.bindings().keys2, Bindings::olx().keys2);
    assert!(h.query_by_label(&shared_note(KeyCode::ArrowUp)).is_none());

    // The gamepads: their inputs alone; Start is the game's (the pause).
    seat(&mut h, "Gamepads");
    h.get_by_label(seat_note(olr_input::SeatKind::Gamepad));
    assert!(h.query_by_label("Add key for Shoot").is_none());
    tap(&mut h, "Add pad input for Shoot");
    h.state_mut().data.capture.as_mut().unwrap().on_pad(PadEvent::Button(Button::Start, 1.0));
    h.run();
    h.get_by_label_contains("Pad Start is the game's (pause)");
    assert_eq!(h.state().ui.bindings().pad, Bindings::olx().pad);
}

/// Issue #28: an options file from before seats keeps every binding it
/// has and gets OLX's player 2 for keyboard 2; a pad's Start bound by
/// hand is unbound (the pause), and an action it leaves without a pad
/// input gets its default back.
#[test]
fn an_options_file_from_before_seats_is_repaired() {
    let tmp = tempfile::tempdir().unwrap();
    let text = format!(
        "(version: 1, settings: (input: (keys: [(Up, KeyW), (Down, KeyS), (Left, KeyA), (Right, KeyD), \
         (Shoot, ControlLeft), (Jump, Space), (Rope, KeyE), (SelectWeapon, ShiftLeft), (Strafe, KeyZ), \
         (PreviousWeapon, KeyQ), (NextWeapon, KeyR), (Dig, KeyC), {WEAPON_KEYS}])))"
    );
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    let input = &got.value.input;
    assert_eq!(input.of(K, Action::Up, KB), [Binding::Key(KeyCode::KeyW)], "keyboard 1's kept");
    assert_eq!(input.keys2, Bindings::olx_keys2());
    assert_eq!(input.pad, Bindings::olx().pad);

    let mut pad = Bindings::olx().pad;
    pad.retain(|(a, _)| *a != Action::Jump);
    pad.push((Action::Jump, PadInput::Button(Button::Start)));
    let b = Bindings { pad, ..Bindings::olx() };
    let text = format!("(version: 1, settings: (input: {}))", ron::to_string(&b).unwrap());
    std::fs::write(tmp.path().join("settings.ron"), text).unwrap();
    let got = Store::open(tmp.path()).load_settings();
    let input = &got.value.input;
    assert!(got.warnings.iter().any(|w| w.contains("Pad Start") && w.contains("Jump")), "{:?}", got.warnings);
    assert_eq!(input.of(K, Action::Jump, PAD), [Binding::Pad(PadInput::Button(Button::South))]);
    assert!(input.unreachable().is_empty());
}

/// The settings the menus last asked to save.
fn saved(h: &Harness<'static, App>) -> Settings {
    h.state()
        .commands
        .iter()
        .rev()
        .find_map(|c| match c {
            AppCommand::SaveSettings(s) => Some(s.clone()),
            _ => None,
        })
        .expect("saved")
}

fn video(h: &Harness<'static, App>) -> Vec<VideoSettings> {
    h.state()
        .commands
        .iter()
        .filter_map(|c| match c {
            AppCommand::ApplyVideo(v) => Some(v.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn options_set_video_and_audio() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    tap(&mut h, "Options");
    // Video opens first.
    tap(&mut h, "Display");
    tap(&mut h, "1920 × 1080");
    assert_eq!(video(&h).last().map(|v| v.window), Some(WindowPreset::W1920x1080));
    // Fullscreen is the list's last entry; it keeps the window's size.
    tap(&mut h, "Display");
    tap(&mut h, "Fullscreen");
    assert_eq!(video(&h).last().map(|v| (v.window, v.fullscreen)), Some((WindowPreset::W1920x1080, true)));
    tap(&mut h, "Uncapped");
    tap(&mut h, "Nearest");
    tap(&mut h, "Smooth motion");
    let v = video(&h).last().cloned().unwrap();
    assert!(v.fullscreen && !v.interpolation);
    assert_eq!((v.frame_rate, v.filter), (FrameRate::Uncapped, UpscaleFilter::Nearest));
    assert_eq!(video(&h).len(), 5, "one apply per change");
    assert_eq!(saved(&h).video, v);
    // A window size picked while fullscreen ends it.
    tap(&mut h, "Display");
    tap(&mut h, "1600 × 900");
    assert_eq!(video(&h).last().map(|v| (v.window, v.fullscreen)), Some((WindowPreset::W1600x900, false)));

    tap(&mut h, "Audio");
    h.get_by_role_and_label(Role::Slider, "Master volume");
    h.get_by_role_and_label(Role::Slider, "Effects volume");
    // OLX's default volume (Audio.Volume 70).
    assert_eq!(AudioSettings::default().master, 0.7);

    // The HUD's layout: the bar, or side panels on an ultrawide window.
    assert_eq!(v.hud_layout, HudArrangement::Auto, "Auto by default");
    tap(&mut h, "Video");
    tap(&mut h, "HUD layout");
    tap(&mut h, "Side panels");
    assert_eq!(video(&h).last().map(|v| v.hud_layout), Some(HudArrangement::Sides));
    assert_eq!(saved(&h).video.hud_layout, HudArrangement::Sides);
    tap(&mut h, "HUD layout");
    tap(&mut h, "Bottom bar");
    assert_eq!(saved(&h).video.hud_layout, HudArrangement::Bar);
    // A file from before the setting loads with Auto; the setting is kept.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("settings.ron"), "(version: 1, settings: (video: (ui_scale: 1.5)))")
        .unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert_eq!((got.value.video.hud_layout, got.value.video.ui_scale), (HudArrangement::Auto, 1.5));
    assert!(got.warnings.is_empty(), "{:?}", got.warnings);
    let mut s = Settings::default();
    s.video.hud_layout = HudArrangement::Sides;
    Store::open(tmp.path()).save_settings(&s).unwrap();
    assert_eq!(Store::open(tmp.path()).load_settings().value.video.hud_layout, HudArrangement::Sides);

    // The match is local play's, set up before each game: no tab here.
    assert!(h.query_by_label("Gameplay").is_none());
    assert!(h.query_by_label("Lives").is_none());
}

/// The game's own keys (`GAME_KEYS`: Esc, F5, F2, F3, T, Tab, H, and PageUp and
/// PageDown for a watched game's fast-forward) are never bound,
/// even from a file edited by hand: they are dropped, and an action left
/// without a key gets OLX's back (Review Focus 2).
#[test]
fn game_keys_are_never_bound_from_a_file() {
    use olr_ui::screens::GAME_KEYS;
    let game = |b: &Bindings| b.keys.iter().any(|(_, k)| GAME_KEYS.iter().any(|(g, _)| g == k));
    let mut s = Settings::default();
    s.input.keys.push((Action::Shoot, KeyCode::Tab));
    s.input.keys.retain(|(a, _)| *a != Action::Jump);
    s.input.keys.push((Action::Jump, KeyCode::Escape));
    s.input.keys.push((Action::Rope, KeyCode::KeyH));
    // The fast-forward keys too, even as an action's only key.
    s.input.keys.retain(|(a, _)| *a != Action::Down);
    s.input.keys.push((Action::Down, KeyCode::PageDown));
    s.input.keys.push((Action::Up, KeyCode::PageUp));
    let w = s.sanitize();
    assert!(!game(&s.input), "{:?}", s.input.keys);
    assert!(controllable(&s.input));
    assert_eq!(s.input.of(K, Action::Shoot, KB), [Binding::Key(KeyCode::ControlLeft)]);
    assert_eq!(s.input.of(K, Action::Jump, KB), [Binding::Key(KeyCode::AltLeft)], "OLX's back");
    assert_eq!(s.input.of(K, Action::Down, KB), [Binding::Key(KeyCode::ArrowDown)], "OLX's back");
    let names = [olr_input::key_name(KeyCode::PageUp), olr_input::key_name(KeyCode::PageDown)];
    for k in ["Tab", "Esc", "H", &names[0], &names[1]] {
        assert!(w.iter().any(|w| w.contains(k)), "{k}: {w:?}");
    }
    assert!(s.sanitize().is_empty(), "fixed once");

    // Through the options' state, and a profile's own keys.
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut b = Bindings::olx();
    b.keys.push((Action::Up, KeyCode::F5));
    ui.update(UiAction::SetBindings(b));
    assert!(!game(&ui.bindings()));
    let mut own = Bindings::olx();
    own.keys = vec![(Action::Up, KeyCode::F2)];
    let mut p = Profiles { list: vec![Profile { bindings: Some(own), ..Profile::default() }], selected: 0 };
    p.sanitize();
    let own = p.list[0].bindings.as_ref().unwrap();
    assert!(!game(own) && controllable(own), "{:?}", own.keys);
}

/// The key the capture takes is the capture's alone: egui does not also
/// act on it (Space would press the focused button, Esc leave the screen).
#[test]
fn capture_swallows_the_captured_key() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    tap(&mut h, "Add key for Jump");
    // Keys egui hears while the box is open do nothing.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().data.capture.is_some(), "Esc reaches the capture, not egui");
    assert_eq!(h.state().ui.screen(), Screen::Options);
    // The captured key, also seen by egui in the same frame, binds once.
    h.state_mut().data.capture.as_mut().unwrap().on_key(KeyCode::Space, true);
    h.key_press(egui::Key::Space);
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert!(h.state().data.capture.is_none(), "no second capture opened");
    assert_eq!(keys(&h, Action::Jump), [Binding::Key(KeyCode::AltLeft), Binding::Key(KeyCode::Space)]);
    assert_eq!(h.state().ui.screen(), Screen::Options);
}

/// The chat key (online) is rebound in the Keys tab; the game's keys are refused.
#[test]
fn options_rebind_the_chat_key() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    open_keys(&mut h);
    assert_eq!(h.state().ui.settings().online.chat_key, KeyCode::KeyT);
    tap(&mut h, "Change chat key");
    h.get_by_label_contains("Press a key for Chat");
    press(&mut h, KeyCode::F2);
    h.get_by_label_contains("F2 is the game's");
    assert_eq!(h.state().ui.settings().online.chat_key, KeyCode::KeyT);
    tap(&mut h, "Change chat key");
    press(&mut h, KeyCode::KeyY);
    h.run();
    assert_eq!(h.state().ui.settings().online.chat_key, KeyCode::KeyY);
    assert!(matches!(h.state().commands.last(), Some(AppCommand::SaveSettings(_))));
    assert_eq!(h.state().ui.bindings(), Bindings::olx(), "the action keys are untouched");
}

fn clears(h: &Harness<'static, App>) -> usize {
    h.state().commands.iter().filter(|c| **c == AppCommand::ClearDownloadCache).count()
}

/// Plan 11: the Online tab's "Download cache: N MB" and its Clear, which
/// the app disables while an online game is on (a hover says why); no
/// cache says why.
#[test]
fn options_clears_the_download_cache() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    h.state_mut().data.download_cache = Ok(DownloadCacheView { bytes: 312 << 20, clear: Ok(()) });
    // Only the options clear.
    assert!(h.state_mut().ui.update(UiAction::ClearDownloadCache).is_empty());
    tap(&mut h, "Options");
    tap(&mut h, "Online");
    h.get_by_label("Download cache: 312 MB");
    // Below the Online tab's lists: scrolled into view first.
    h.get_by_role_and_label(Role::Button, "Clear").scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::Button, "Clear").click();
    h.run();
    assert_eq!(clears(&h), 1);
    let busy = Err("leave the online game first".to_string());
    h.state_mut().data.download_cache = Ok(DownloadCacheView { bytes: 0, clear: busy });
    h.run();
    h.get_by_label("Download cache: 0.0 MB");
    let clear = h.get_by_role_and_label(Role::Button, "Clear");
    assert!(clear.accesskit_node().is_disabled());
    clear.click();
    h.run();
    assert_eq!(clears(&h), 1, "disabled while online");
    h.get_by_role_and_label(Role::Button, "Clear").hover();
    h.run();
    h.get_by_label("leave the online game first");
    h.state_mut().data.download_cache = Err("no cache directory".into());
    h.run();
    h.get_by_label("Download cache: none (no cache directory)");
    assert!(h.query_by_label("Clear").is_none());
}

/// Plan 12a: the Online tab edits the region, the relays and STUN servers
/// (Remove, Add with a valid entry only, Reset to the game's defaults) and
/// unblocks a host; each change saved.
#[test]
fn options_edit_relays_and_unblock() {
    let mut settings = Settings::default();
    settings.online.finding.blocked = vec!["ab".repeat(32), "cd".repeat(32)];
    let mut h = harness(UiState::new(settings, Profiles::default(), 1));
    h.state_mut().data.default_relays = vec!["wss://relay.one".into(), "wss://relay.two".into()];
    h.state_mut().data.default_stun = vec!["stun.one:3478".into()];
    tap(&mut h, "Options");
    tap(&mut h, "Online");
    // The defaults show while the settings name none.
    h.get_by_label("wss://relay.one");
    h.get_by_label("stun.one:3478");
    assert!(h.get_by_label("Reset relays to defaults").accesskit_node().is_disabled());
    // Remove the first relay: the list is the player's from now on.
    h.get_all_by_label("Remove").next().unwrap().click();
    h.run();
    assert_eq!(saved(&h).online.finding.relays, Some(vec!["wss://relay.two".to_string()]));
    // Add one: refused until it is a relay URL.
    h.get_by_role_and_label(Role::TextInput, "New relay").focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New relay").type_text("relay.example.org");
    h.run();
    let add = h.get_by_label("Add relay");
    assert!(add.accesskit_node().is_disabled());
    h.get_by_label("needs wss:// or ws://");
    add.hover();
    h.run();
    assert_eq!(h.get_all_by_label("needs wss:// or ws://").count(), 2, "beside the field and on hover");
    // One already listed.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New relay").type_text("wss://relay.two");
    h.run();
    // Beside the field (and on Add's hover, the pointer still there).
    assert!(h.get_all_by_label("already listed").count() >= 1);
    assert!(h.get_by_label("Add relay").accesskit_node().is_disabled());
    // Replace the text: select all, then the URL.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New relay").type_text("wss://relay.example.org");
    h.run();
    tap(&mut h, "Add relay");
    assert_eq!(
        saved(&h).online.finding.relays,
        Some(vec!["wss://relay.two".to_string(), "wss://relay.example.org".to_string()])
    );
    // Reset: the defaults again.
    tap(&mut h, "Reset relays to defaults");
    assert_eq!(saved(&h).online.finding.relays, None);
    h.get_by_label("wss://relay.one");
    // STUN: none asked at all.
    let stun_remove = h.get_all_by_label("Remove").count() - 1;
    h.get_all_by_label("Remove").nth(stun_remove).unwrap().click();
    h.run();
    assert_eq!(saved(&h).online.finding.stun, Some(Vec::new()));
    h.get_by_label("No STUN servers: none is asked.");
    h.get_by_role_and_label(Role::TextInput, "New STUN server").focus();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "New STUN server").type_text("stun.example.org");
    h.run();
    h.get_by_label("needs host:port");
    assert!(h.get_by_label("Add STUN server").accesskit_node().is_disabled());
    // The region.
    tap(&mut h, "Region");
    tap(&mut h, "Oceania");
    assert_eq!(saved(&h).online.finding.region, olr_ui::Region::Oceania);
    // Unblock the first host (below the lists: scrolled into view first).
    h.get_all_by_label("Unblock").next().unwrap().scroll_to_me();
    h.run();
    h.get_all_by_label("Unblock").next().unwrap().click();
    h.run();
    assert_eq!(saved(&h).online.finding.blocked, ["cd".repeat(32)]);
    assert_eq!(h.state().ui.settings().online.finding.blocked, ["cd".repeat(32)]);
}

/// A full list of relays takes no more, and says why.
#[test]
fn a_full_relay_list_says_so() {
    let mut settings = Settings::default();
    settings.online.finding.relays =
        Some((0..olr_ui::MAX_SERVERS).map(|i| format!("wss://relay{i}.example")).collect());
    let mut h = harness(UiState::new(settings, Profiles::default(), 1));
    tap(&mut h, "Options");
    tap(&mut h, "Online");
    h.get_by_label("the list is full");
    assert!(h.get_by_label("Add relay").accesskit_node().is_disabled());
}

/// A hand-edited file's finding settings are fixed: bad relays, STUN
/// servers, addresses and keys dropped, keys lowercased, duplicates gone.
#[test]
fn finding_settings_are_sanitized() {
    let mut f = olr_ui::FindingSettings {
        region: olr_ui::Region::Asia,
        relays: Some(vec![
            "wss://a.example".into(),
            "https://not.a.relay".into(),
            " wss://a.example ".into(),
            "ws://127.0.0.1:7000".into(),
            "wss://".into(),
        ]),
        stun: Some(vec!["stun.example:3478".into(), "nope".into(), "x:0".into()]),
        public_address: "not an ip".into(),
        blocked: vec!["AB".repeat(32), "ab".repeat(32), "zz".into()],
        port_mapping: false,
        turn: vec![
            "me:secret@turn.example:3478".into(),
            "turn.example:3478".into(),
            " me:secret@turn.example:3478 ".into(),
            ":secret@turn.example:3478".into(),
            "me:secret@turn.example".into(),
            "me:secret".into(),
            "me:secret turn.example:3478".into(),
        ],
    };
    let w = f.sanitize();
    assert_eq!(f.relays, Some(vec!["wss://a.example".to_string(), "ws://127.0.0.1:7000".to_string()]));
    assert_eq!(f.stun, Some(vec!["stun.example:3478".to_string()]));
    assert_eq!(f.public_address, "");
    assert_eq!(f.blocked, ["ab".repeat(32)]);
    assert_eq!(f.turn, ["me:secret@turn.example:3478"]);
    assert!(!f.port_mapping, "the player's choice stays");
    assert_eq!(w.len(), 11, "{w:?}");
    assert!(!w.iter().any(|w| w.contains("secret")), "no password in a warning: {w:?}");
    let mut ok = olr_ui::FindingSettings { public_address: " 81.2.69.142 ".into(), ..Default::default() };
    assert!(ok.sanitize().is_empty());
    assert_eq!(ok.public_address, "81.2.69.142");
    // Printing the settings, an action or a command shows no password.
    let settings = Settings {
        online: olr_ui::OnlineSettings { finding: f.clone(), ..Default::default() },
        ..Settings::default()
    };
    for printed in [
        format!("{f:?}"),
        format!("{:?}", UiAction::SetFinding(f.clone())),
        format!("{:?}", AppCommand::SaveSettings(settings)),
    ] {
        assert!(!printed.contains("secret") && printed.contains("me:•••@turn.example:3478"), "{printed}");
    }
    // At most `MAX_TURN` TURN servers.
    let mut many = olr_ui::FindingSettings {
        turn: (0..12).map(|i| format!("u:p@turn{i}.example:3478")).collect(),
        ..Default::default()
    };
    assert_eq!(many.sanitize().len(), 1);
    assert_eq!(many.turn.len(), olr_ui::MAX_TURN);
    // Port mapping is on by default, and for a file written before it.
    assert!(olr_ui::FindingSettings::default().port_mapping);
    let old: olr_ui::FindingSettings = ron::from_str("(region: Europe)").unwrap();
    assert!(old.port_mapping && old.turn.is_empty());
}

/// Plan 12b (N51, N52): the Online tab's port mapping switch and the
/// player's own TURN servers, `user:pass@host:port`, each refused with
/// why (as relays are), at most `MAX_TURN`; every change saved.
#[test]
fn options_edit_turn_relays_and_port_mapping() {
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    tap(&mut h, "Options");
    tap(&mut h, "Online");
    // On by default; unchecked, it is saved off.
    let switch = "Open the game's port on my router (UPnP/NAT-PMP/PCP)";
    assert!(h.state().ui.settings().online.finding.port_mapping);
    tap(&mut h, switch);
    assert!(!saved(&h).online.finding.port_mapping);
    // None by default (the project ships no TURN list, N52).
    h.get_by_label("No TURN relays: none is asked.");
    // Final review C-M5: the new TURN relay is masked as it is typed (its
    // password), unless "Show password" is ticked.
    let field = Role::PasswordInput;
    h.get_by_role_and_label(field, "New TURN relay").focus();
    h.run();
    h.get_by_role_and_label(field, "New TURN relay").type_text("turn.example.org:3478");
    h.run();
    h.get_by_label("needs user:pass@host:port");
    let add = h.get_by_label("Add TURN relay");
    assert!(add.accesskit_node().is_disabled());
    add.hover();
    h.run();
    assert_eq!(h.get_all_by_label("needs user:pass@host:port").count(), 2, "beside the field and on hover");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.get_by_role_and_label(field, "New TURN relay").type_text("anna:s3cret@turn.example.org:3478");
    h.run();
    let typed = h.get_by_role_and_label(field, "New TURN relay").accesskit_node().value().unwrap_or_default();
    assert!(!typed.contains("s3cret"), "masked while typed: {typed}");
    tap(&mut h, "Show password");
    let shown = h.get_by_role_and_label(Role::TextInput, "New TURN relay").accesskit_node().value();
    assert_eq!(shown.as_deref(), Some("anna:s3cret@turn.example.org:3478"), "shown when asked");
    tap(&mut h, "Add TURN relay");
    assert_eq!(saved(&h).online.finding.turn, ["anna:s3cret@turn.example.org:3478"]);
    // Listed without its password.
    h.get_by_label("anna:•••@turn.example.org:3478");
    assert!(h.query_by_label_contains("s3cret").is_none(), "the password is not shown");
    // Removed again.
    h.get_by_role_and_label(Role::Button, "Remove").click();
    h.run();
    assert!(saved(&h).online.finding.turn.is_empty());
    // A full list takes no more.
    let mut settings = Settings::default();
    settings.online.finding.turn =
        (0..olr_ui::MAX_TURN).map(|i| format!("u:p@turn{i}.example:3478")).collect();
    let mut h = harness(UiState::new(settings, Profiles::default(), 1));
    tap(&mut h, "Options");
    tap(&mut h, "Online");
    h.get_by_label("the list is full");
    assert!(h.get_by_label("Add TURN relay").accesskit_node().is_disabled());
}

/// Options → Video's Blood (issue #6): 0-500 % in steps of 10, 100 by
/// default and for a file from before it, applied and saved at once; a
/// hand-edited amount past the end is clamped.
#[test]
fn the_blood_slider_sets_the_players_blood_amount() {
    assert_eq!(VideoSettings::default().blood, 100);
    let mut h = harness(UiState::new(Settings::default(), Profiles::default(), 1));
    tap(&mut h, "Options");
    let slider = h.get_by_role_and_label(Role::Slider, "Blood");
    slider.focus();
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(video(&h).last().map(|v| v.blood), Some(110), "one step");
    assert_eq!(saved(&h).video.blood, 110);
    h.key_press(egui::Key::ArrowLeft);
    h.run();
    h.key_press(egui::Key::ArrowLeft);
    h.run();
    assert_eq!(video(&h).last().map(|v| v.blood), Some(90));

    // Clamped from a file; a file without it shows OLX's amount.
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let v = VideoSettings { blood: 900, ..VideoSettings::default() };
    let applied = ui.update(UiAction::SetVideo(v));
    assert!(applied.iter().any(|c| matches!(c, AppCommand::ApplyVideo(v) if v.blood == 500)), "{applied:?}");
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("settings.ron"), "(version: 1, settings: (video: (ui_scale: 1.5)))")
        .unwrap();
    assert_eq!(Store::open(tmp.path()).load_settings().value.video.blood, 100);
    std::fs::write(tmp.path().join("settings.ron"), "(version: 1, settings: (video: (blood: 4000)))")
        .unwrap();
    let got = Store::open(tmp.path()).load_settings();
    assert_eq!(got.value.video.blood, 500);
    assert!(got.warnings.iter().any(|w| w.contains("video.blood")), "{:?}", got.warnings);
}
