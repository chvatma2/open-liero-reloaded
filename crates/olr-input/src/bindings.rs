//! What each device input does: the simulation's 11 keys and Dig as
//! [`Action`]s, and the [`Bindings`] from keyboard keys and gamepad inputs
//! to them.

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

/// The keys mouse controls fix (issue #24): A and D move, Space jumps, S
/// digs. The buttons and the wheel are [`crate::InputState::on_mouse`]'s.
pub const MOUSE_KEYS: [(Action, KeyCode); 4] = [
    (Action::Left, KeyCode::KeyA),
    (Action::Right, KeyCode::KeyD),
    (Action::Jump, KeyCode::Space),
    (Action::Dig, KeyCode::KeyS),
];

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
/// action. Several inputs may share an action; the action is held while
/// any of them is. Serialisable for the options (M7).
///
/// A file may leave either list out: it takes OLX's (`#[serde(default)]`);
/// [`Bindings::repair`] restores what leaves an action unreachable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    pub keys: Vec<(Action, KeyCode)>,
    pub pad: Vec<(Action, PadInput)>,
}

impl Default for Bindings {
    /// OLX's player 1 defaults (spec §19 E5).
    fn default() -> Self {
        Bindings::olx()
    }
}

impl Bindings {
    /// OLX's player 1 defaults (non-Apple): arrows, Shoot `LCtrl`, Jump
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
        Bindings { keys, pad }
    }

    /// The bindings mouse controls play with (issue #24): these, without
    /// the actions the controller presses itself (Up, Down, SelectWeapon,
    /// Strafe) and without any binding of [`MOUSE_KEYS`], which are added.
    pub fn for_mouse(&self) -> Bindings {
        use Action::*;
        let unused = |a: &Action| matches!(a, Up | Down | SelectWeapon | Strafe);
        let fixed = |k: &KeyCode| MOUSE_KEYS.iter().any(|(_, f)| f == k);
        let keys =
            self.keys.iter().filter(|(a, k)| !unused(a) && !fixed(k)).copied().chain(MOUSE_KEYS).collect();
        let pad = self.pad.iter().filter(|(a, _)| !unused(a)).copied().collect();
        Bindings { keys, pad }
    }

    /// The [`Keys`] bits (and [`DIG`]) the held keyboard keys set.
    pub fn key_bits(&self, held: &[KeyCode]) -> u16 {
        self.keys.iter().filter(|(_, k)| held.contains(k)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The [`Keys`] bits (and [`DIG`]) the held gamepad inputs set.
    pub fn pad_bits(&self, held: impl Fn(PadInput) -> bool) -> u16 {
        self.pad.iter().filter(|(_, p)| held(*p)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The bound keys that, pressed together, make a shortcut a browser keeps
    /// for itself ([`is_browser_reserved`]): pairs (and single keys) in
    /// binding order. Spec §8.5: the defaults have none.
    pub fn browser_conflicts(&self) -> Vec<Vec<KeyCode>> {
        let keys: Vec<KeyCode> = self.keys.iter().map(|(_, k)| *k).collect();
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
