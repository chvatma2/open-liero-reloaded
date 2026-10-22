//! What each device input does: the simulation's 11 keys and Dig as
//! [`Action`]s, and the [`Bindings`] from keyboard keys, gamepad inputs and
//! (mouse controls) mouse inputs to them.

use gilrs::{Axis, Button};
use olr_sim::Keys;
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// The bit [`Action::Dig`] sets in [`Bindings::key_bits`] and
/// [`Bindings::pad_bits`]: olr-input's own, above the simulation's
/// [`Keys::ALL`], never sent to it ([`crate::InputState::latch_tick`] turns
/// it into the dig gesture).
pub const DIG: u16 = 1 << 15;

/// A control of OLX's `ply_keys` that the simulation has a key for (spec §19
/// E5; Weapon1-5 have none), plus [`Action::Dig`], which presses the dig
/// gesture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    /// Aim up.
    Up,
    /// Aim down.
    Down,
    Left,
    Right,
    Shoot,
    Jump,
    Rope,
    /// Held: Left and Right cycle the weapon.
    SelectWeapon,
    /// Held: Left and Right move without turning the worm.
    Strafe,
    /// A press selects the previous weapon slot.
    PreviousWeapon,
    /// A press selects the next weapon slot.
    NextWeapon,
    /// Digs once, the way a player digs with the keys (holding one
    /// direction, pressing the other); held, again every 0.2 s. Not one of
    /// OLX's controls (OLX has no dig key).
    Dig,
}

impl Action {
    /// Every action, in `Keys` bit order, Dig last.
    pub const ALL: [Action; 12] = [
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Shoot,
        Action::Rope,
        Action::SelectWeapon,
        Action::Strafe,
        Action::PreviousWeapon,
        Action::NextWeapon,
        Action::Dig,
    ];

    /// The [`Keys`] bit this action sets ([`DIG`] for Dig).
    pub fn bit(self) -> u16 {
        match self {
            Action::Up => Keys::UP,
            Action::Down => Keys::DOWN,
            Action::Left => Keys::LEFT,
            Action::Right => Keys::RIGHT,
            Action::Shoot => Keys::FIRE,
            Action::Jump => Keys::JUMP,
            Action::Rope => Keys::ROPE,
            Action::SelectWeapon => Keys::CHANGE,
            Action::Strafe => Keys::STRAFE,
            Action::PreviousWeapon => Keys::PREV_WEAPON,
            Action::NextWeapon => Keys::NEXT_WEAPON,
            Action::Dig => DIG,
        }
    }
}

/// Mouse controls' default keys (issue #24, [`Bindings::mouse_keys`]): A
/// and D move, Space jumps, S digs.
pub const MOUSE_KEYS: [(Action, KeyCode); 4] = [
    (Action::Left, KeyCode::KeyA),
    (Action::Right, KeyCode::KeyD),
    (Action::Jump, KeyCode::Space),
    (Action::Dig, KeyCode::KeyS),
];

/// Mouse controls' default mouse inputs (issue #24, [`Bindings::mouse`]):
/// the left button shoots, the right one throws the rope, the wheel up
/// selects the previous weapon, down the next.
pub const MOUSE_BUTTONS: [(Action, MouseInput); 4] = [
    (Action::Shoot, MouseInput::Left),
    (Action::Rope, MouseInput::Right),
    (Action::PreviousWeapon, MouseInput::WheelUp),
    (Action::NextWeapon, MouseInput::WheelDown),
];

/// The actions mouse controls press themselves (issue #24): the aim (Up,
/// Down), and Strafe (A and D move without turning; the cursor turns the
/// worm). SelectWeapon goes unused (the wheel changes the weapon). They
/// take no key or mouse input in mouse mode.
pub const MOUSE_MADE: [Action; 4] = [Action::Up, Action::Down, Action::SelectWeapon, Action::Strafe];

/// A mouse input mouse controls can bind (issue #25): a button, or a
/// notch of the wheel one way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseInput {
    Left,
    Right,
    Middle,
    /// The side button winit calls `Back`.
    Back,
    /// The side button winit calls `Forward`.
    Forward,
    /// A notch of the wheel up (away from the player).
    WheelUp,
    /// A notch of the wheel down.
    WheelDown,
}

impl MouseInput {
    pub const ALL: [MouseInput; 7] = [
        MouseInput::Left,
        MouseInput::Right,
        MouseInput::Middle,
        MouseInput::Back,
        MouseInput::Forward,
        MouseInput::WheelUp,
        MouseInput::WheelDown,
    ];

    /// The button winit reports, if it is one of these.
    pub fn of_button(b: winit::event::MouseButton) -> Option<MouseInput> {
        use winit::event::MouseButton as B;
        Some(match b {
            B::Left => MouseInput::Left,
            B::Right => MouseInput::Right,
            B::Middle => MouseInput::Middle,
            B::Back => MouseInput::Back,
            B::Forward => MouseInput::Forward,
            B::Other(_) => return None,
        })
    }

    /// A notch of the wheel, not a button: pressed for one tick, never held.
    pub fn is_wheel(self) -> bool {
        matches!(self, MouseInput::WheelUp | MouseInput::WheelDown)
    }

    /// The name the options show.
    pub fn name(self) -> &'static str {
        match self {
            MouseInput::Left => "Left click",
            MouseInput::Right => "Right click",
            MouseInput::Middle => "Middle click",
            MouseInput::Back => "Back button",
            MouseInput::Forward => "Forward button",
            MouseInput::WheelUp => "Wheel up",
            MouseInput::WheelDown => "Wheel down",
        }
    }
}

/// A gamepad input that can be held: a button (analog ones, the triggers,
/// count from [`TRIGGER_THRESHOLD`]) or one direction of an axis (from
/// [`STICK_THRESHOLD`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PadInput {
    Button(Button),
    /// The axis above `+STICK_THRESHOLD` (gilrs: right, or up for a Y axis).
    AxisPlus(Axis),
    /// The axis below `-STICK_THRESHOLD`.
    AxisMinus(Axis),
}

/// A trigger counts as held from 75 % of its travel.
/// OLX: src/client/CInput.cpp:226 (`JOY_TRIGGER_THRESHOLD = 24576`).
pub const TRIGGER_THRESHOLD: f32 = 0.75;

/// A stick direction counts as held from 60 % of its deflection.
/// OLX: src/client/CInput.cpp:200-201 (`JOY_DEADZONE_PERCENT = 60`).
pub const STICK_THRESHOLD: f32 = 0.6;

/// Keyboard keys (winit physical key codes, spec §8.5, so a binding means
/// the same place on any layout) and gamepad inputs, each bound to an
/// action, and mouse controls' own keys and mouse inputs (issue #25).
/// Several inputs may share an action; the action is held while any of
/// them is. Serialisable for the options (M7).
///
/// A file may leave any list out: it takes the defaults
/// (`#[serde(default)]`, so a file from before mouse bindings gets #24's);
/// [`Bindings::repair`] restores what leaves an action unreachable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    /// The keyboard, mouse controls off.
    pub keys: Vec<(Action, KeyCode)>,
    /// The gamepad, in either mode.
    pub pad: Vec<(Action, PadInput)>,
    /// The keyboard with mouse controls on, in place of `keys` (so
    /// turning them off finds `keys` untouched); never of [`MOUSE_MADE`].
    pub mouse_keys: Vec<(Action, KeyCode)>,
    /// The mouse, with mouse controls on; never of [`MOUSE_MADE`].
    pub mouse: Vec<(Action, MouseInput)>,
}

impl Default for Bindings {
    /// OLX's player 1 defaults (spec §19 E5).
    fn default() -> Self {
        Bindings::olx()
    }
}

impl Bindings {
    /// OLX's player 1 defaults (non-Apple), and mouse controls' (issue #24,
    /// [`MOUSE_KEYS`], [`MOUSE_BUTTONS`]): arrows, Shoot `LCtrl`, Jump
    /// `LAlt`, Rope `X`, SelectWeapon `LShift`, Strafe `Z`, Previous/Next
    /// weapon `A`/`S`; on the pad, the d-pad, Shoot right trigger, Jump A
    /// (south), Rope Y (north), SelectWeapon left trigger.
    /// OLX: src/client/Options.cpp:45-51 (`ply_def1`), :64-73 (`ply_gamepad_def`).
    ///
    /// Not OLX's: Dig is `C`, beside the rope and strafe keys (OLX has no
    /// dig key), and the pad's Strafe X (west), Dig B (east) and
    /// Previous/Next weapon the left and right shoulders (OLX's pad
    /// defaults bind none of the four).
    ///
    /// OLX's twin-stick controls (left stick walks, right stick aims
    /// analogue; `CWormHuman.cpp`) have no analogue in the simulation's
    /// keys, so the left stick's X axis presses Left and Right and the right
    /// stick's Y axis Up and Down, from [`STICK_THRESHOLD`].
    pub fn olx() -> Bindings {
        use Action::*;
        use KeyCode::*;
        let keys = vec![
            (Up, ArrowUp),
            (Down, ArrowDown),
            (Left, ArrowLeft),
            (Right, ArrowRight),
            (Shoot, ControlLeft),
            (Jump, AltLeft),
            (Rope, KeyX),
            (SelectWeapon, ShiftLeft),
            (Strafe, KeyZ),
            (PreviousWeapon, KeyA),
            (NextWeapon, KeyS),
            (Dig, KeyC),
        ];
        let pad = vec![
            (Up, PadInput::Button(Button::DPadUp)),
            (Down, PadInput::Button(Button::DPadDown)),
            (Left, PadInput::Button(Button::DPadLeft)),
            (Right, PadInput::Button(Button::DPadRight)),
            (Shoot, PadInput::Button(Button::RightTrigger2)),
            (Jump, PadInput::Button(Button::South)),
            (Rope, PadInput::Button(Button::North)),
            (SelectWeapon, PadInput::Button(Button::LeftTrigger2)),
            (Left, PadInput::AxisMinus(Axis::LeftStickX)),
            (Right, PadInput::AxisPlus(Axis::LeftStickX)),
            (Up, PadInput::AxisPlus(Axis::RightStickY)),
            (Down, PadInput::AxisMinus(Axis::RightStickY)),
            (Strafe, PadInput::Button(Button::West)),
            (PreviousWeapon, PadInput::Button(Button::LeftTrigger)),
            (NextWeapon, PadInput::Button(Button::RightTrigger)),
            (Dig, PadInput::Button(Button::East)),
        ];
        Bindings { keys, pad, mouse_keys: MOUSE_KEYS.to_vec(), mouse: MOUSE_BUTTONS.to_vec() }
    }

    /// The bindings mouse controls play with (issue #24, #25): the keys
    /// are [`Bindings::mouse_keys`], the mouse [`Bindings::mouse`], the
    /// gamepad's [`Bindings::pad`], all without [`MOUSE_MADE`]'s actions,
    /// which the controller presses itself.
    pub fn for_mouse(&self) -> Bindings {
        let used = |a: &Action| !MOUSE_MADE.contains(a);
        let keys = self.mouse_keys.iter().filter(|(a, _)| used(a)).copied().collect();
        let pad = self.pad.iter().filter(|(a, _)| used(a)).copied().collect();
        let mouse = self.mouse.iter().filter(|(a, _)| used(a)).copied().collect();
        Bindings { keys, pad, mouse_keys: Vec::new(), mouse }
    }

    /// The [`Keys`] bits (and [`DIG`]) the held keyboard keys set.
    pub fn key_bits(&self, held: &[KeyCode]) -> u16 {
        self.keys.iter().filter(|(_, k)| held.contains(k)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The [`Keys`] bits (and [`DIG`]) the held gamepad inputs set.
    pub fn pad_bits(&self, held: impl Fn(PadInput) -> bool) -> u16 {
        self.pad.iter().filter(|(_, p)| held(*p)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The [`Keys`] bits (and [`DIG`]) the held mouse inputs set.
    pub fn mouse_bits(&self, held: &[MouseInput]) -> u16 {
        self.mouse.iter().filter(|(_, m)| held.contains(m)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The bound keys that, pressed together, make a shortcut a browser keeps
    /// for itself ([`is_browser_reserved`]): pairs (and single keys) in
    /// binding order, of [`Bindings::keys`] and then of
    /// [`Bindings::mouse_keys`] (never pressed with them). Spec §8.5: the
    /// defaults have none.
    pub fn browser_conflicts(&self) -> Vec<Vec<KeyCode>> {
        let mut out = conflicts(&self.keys);
        for c in conflicts(&self.mouse_keys) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }
}

/// [`Bindings::browser_conflicts`] of one list.
fn conflicts(list: &[(Action, KeyCode)]) -> Vec<Vec<KeyCode>> {
    let keys: Vec<KeyCode> = list.iter().map(|(_, k)| *k).collect();
    let mut out = Vec::new();
    for (i, a) in keys.iter().enumerate() {
        if is_browser_reserved(&[*a]) {
            out.push(vec![*a]);
        }
        for b in &keys[i + 1..] {
            if a != b && is_browser_reserved(&[*a, *b]) {
                out.push(vec![*a, *b]);
            }
        }
    }
    out
}

/// Whether holding `keys` together makes a shortcut that browsers handle
/// before a page can see it (closing or opening tabs and windows, switching
/// tabs, quitting, full screen) or that the OS takes (the Super/Meta keys).
/// Shortcuts a page can cancel, such as Alt+Left, are not listed.
pub fn is_browser_reserved(keys: &[KeyCode]) -> bool {
    use KeyCode::*;
    let has = |k: &[KeyCode]| keys.iter().any(|x| k.contains(x));
    if has(&[SuperLeft, SuperRight, Meta, F11]) {
        return true;
    }
    let ctrl = has(&[ControlLeft, ControlRight]);
    let alt = has(&[AltLeft, AltRight]);
    (ctrl && has(&[KeyW, KeyT, KeyN, KeyQ, Tab, PageUp, PageDown, F4])) || (alt && has(&[F4]))
}
