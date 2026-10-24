//! What each device input does: the simulation's 16 keys and Dig as
//! [`Action`]s, and the [`Bindings`] from keyboard keys, gamepad inputs and
//! (mouse controls) mouse inputs to them.

use gilrs::{Axis, Button};
use olr_sim::Keys;
use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

/// The bit [`Action::Dig`] sets in [`Bindings::key_bits`] and
/// [`Bindings::pad_bits`]: olr-input's own, above the simulation's 16-bit
/// [`Keys`] in the actions' `u32` mask, never sent to it
/// ([`crate::InputState::latch_tick`] turns it into the dig gesture).
pub const DIG: u32 = 1 << 16;

/// A control of OLX's `ply_keys`, each a simulation key (spec §19 E5;
/// Weapon1-5 since issue #48), plus [`Action::Dig`], which presses the dig
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
    /// Held, selects the first weapon slot.
    Weapon1,
    /// Held, selects the second weapon slot.
    Weapon2,
    /// Held, selects the third weapon slot.
    Weapon3,
    /// Held, selects the fourth weapon slot.
    Weapon4,
    /// Held, selects the fifth weapon slot.
    Weapon5,
    /// Digs once, the way a player digs with the keys (holding one
    /// direction, pressing the other); held, again every 0.2 s. Not one of
    /// OLX's controls (OLX has no dig key).
    Dig,
}

impl Action {
    /// Every action, in `Keys` bit order, Dig last.
    pub const ALL: [Action; 17] = [
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
        Action::Weapon1,
        Action::Weapon2,
        Action::Weapon3,
        Action::Weapon4,
        Action::Weapon5,
        Action::Dig,
    ];

    /// Weapon1 to Weapon5, in slot order.
    pub const WEAPONS: [Action; 5] =
        [Action::Weapon1, Action::Weapon2, Action::Weapon3, Action::Weapon4, Action::Weapon5];

    /// The bit this action sets in the actions' mask: its [`Keys`] bit
    /// ([`DIG`] for Dig).
    pub fn bit(self) -> u32 {
        u32::from(match self {
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
            Action::Weapon1 => Keys::WEAPON1,
            Action::Weapon2 => Keys::WEAPON2,
            Action::Weapon3 => Keys::WEAPON3,
            Action::Weapon4 => Keys::WEAPON4,
            Action::Weapon5 => Keys::WEAPON5,
            Action::Dig => return DIG,
        })
    }
}

/// Mouse controls' default keys (issue #24, [`Bindings::mouse_keys`]): A
/// and D move, Space jumps, S digs; 1 to 5 select a weapon slot, as in
/// keyboard 1's own list (issue #48).
pub const MOUSE_KEYS: [(Action, KeyCode); 9] = [
    (Action::Left, KeyCode::KeyA),
    (Action::Right, KeyCode::KeyD),
    (Action::Jump, KeyCode::Space),
    (Action::Dig, KeyCode::KeyS),
    (Action::Weapon1, KeyCode::Digit1),
    (Action::Weapon2, KeyCode::Digit2),
    (Action::Weapon3, KeyCode::Digit3),
    (Action::Weapon4, KeyCode::Digit4),
    (Action::Weapon5, KeyCode::Digit5),
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

/// The actions a gamepad need not have (issue #48): Weapon 1-5, which OLX
/// binds to no pad input (a pad has Previous and Next weapon). A player
/// may still bind them to one.
/// OLX: src/client/Options.cpp:61-63, :76-80
pub const PAD_OPTIONAL: [Action; 5] = Action::WEAPONS;

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

/// The pad's Start button: the game's (it pauses, issue #28, as Esc does),
/// never bound to an action. OLX binds no pad input to pausing.
pub const PAD_PAUSE: PadInput = PadInput::Button(Button::Start);

/// A trigger counts as held from 75 % of its travel.
/// OLX: src/client/CInput.cpp:226 (`JOY_TRIGGER_THRESHOLD = 24576`).
pub const TRIGGER_THRESHOLD: f32 = 0.75;

/// A stick direction counts as held from 60 % of its deflection.
/// OLX: src/client/CInput.cpp:200-201 (`JOY_DEADZONE_PERCENT = 60`).
pub const STICK_THRESHOLD: f32 = 0.6;

/// Every seat's bindings (issue #28): keyboard 1's keys (winit physical
/// key codes, spec §8.5, so a binding means the same place on any layout)
/// and mouse controls' own keys and mouse inputs (issue #25), keyboard 2's
/// keys, and the gamepads' inputs (every pad's seat shares them), each
/// bound to an action.
/// Several inputs may share an action; the action is held while any of
/// them is. Serialisable for the options (M7).
///
/// A file may leave any list out: it takes the defaults
/// (`#[serde(default)]`, so a file from before mouse bindings gets #24's);
/// [`Bindings::repair`] restores what leaves an action unreachable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    /// Keyboard 1, mouse controls off.
    pub keys: Vec<(Action, KeyCode)>,
    /// The gamepads, in either mode.
    pub pad: Vec<(Action, PadInput)>,
    /// The keyboard with mouse controls on, in place of `keys` (so
    /// turning them off finds `keys` untouched); never of [`MOUSE_MADE`].
    pub mouse_keys: Vec<(Action, KeyCode)>,
    /// The mouse, with mouse controls on; never of [`MOUSE_MADE`].
    pub mouse: Vec<(Action, MouseInput)>,
    /// Keyboard 2 (issue #28), on the same keyboard as keyboard 1: OLX's
    /// player 2 by default. A file from before seats gets them.
    pub keys2: Vec<(Action, KeyCode)>,
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
    /// `LAlt`, Rope `Z`, SelectWeapon `LShift`, Strafe `X`, Previous/Next
    /// weapon `A`/`S`, Weapon 1-5 the digits `1`-`5`; on the pad, the
    /// d-pad, Shoot right trigger, Jump A (south), Rope Y (north),
    /// SelectWeapon left trigger, no Weapon 1-5 ([`PAD_OPTIONAL`]).
    /// OLX: src/client/Options.cpp:45-51 (`ply_def1`), :64-73 (`ply_gamepad_def`).
    ///
    /// Not OLX's: Rope `Z` and Strafe `X` (OLX's are the other way round),
    /// Dig is `C`, beside the rope and strafe keys (OLX has no dig key), and the pad's Strafe X (west), Dig B (east) and
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
            (Rope, KeyZ),
            (SelectWeapon, ShiftLeft),
            (Strafe, KeyX),
            (PreviousWeapon, KeyA),
            (NextWeapon, KeyS),
            (Dig, KeyC),
            (Weapon1, Digit1),
            (Weapon2, Digit2),
            (Weapon3, Digit3),
            (Weapon4, Digit4),
            (Weapon5, Digit5),
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
        Bindings {
            keys,
            pad,
            mouse_keys: MOUSE_KEYS.to_vec(),
            mouse: MOUSE_BUTTONS.to_vec(),
            keys2: Bindings::olx_keys2(),
        }
    }

    /// Keyboard 2's defaults (issue #28): OLX's player 2 on the numeric
    /// keypad, Up `kp 8`, Down `kp 5`, Left `kp 4`, Right `kp 6`, Shoot
    /// `kp +`, Jump `kp enter`, Rope `kp -`, SelectWeapon `kp 0`, Strafe
    /// `kp .`, Previous/Next weapon `kp 7`/`kp 9`, Weapon 1-5 the digits
    /// `6` `7` `8` `9` `0`.
    /// OLX: src/client/Options.cpp (`ply_def2`, after `ply_def1` at :45-51;
    /// presentation note §10.1).
    ///
    /// Not OLX's: Dig is `kp 3`, beside Jump and Strafe (OLX has no dig
    /// key; the keypad's free keys are `/`, `*`, 1, 2 and 3).
    pub fn olx_keys2() -> Vec<(Action, KeyCode)> {
        use Action::*;
        use KeyCode::*;
        vec![
            (Up, Numpad8),
            (Down, Numpad5),
            (Left, Numpad4),
            (Right, Numpad6),
            (Shoot, NumpadAdd),
            (Jump, NumpadEnter),
            (Rope, NumpadSubtract),
            (SelectWeapon, Numpad0),
            (Strafe, NumpadDecimal),
            (PreviousWeapon, Numpad7),
            (NextWeapon, Numpad9),
            (Dig, Numpad3),
            (Weapon1, Digit6),
            (Weapon2, Digit7),
            (Weapon3, Digit8),
            (Weapon4, Digit9),
            (Weapon5, Digit0),
        ]
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
        Bindings { keys, pad, mouse_keys: Vec::new(), mouse, keys2: Vec::new() }
    }

    /// The actions' mask ([`Action::bit`]) the held keyboard keys set.
    pub fn key_bits(&self, held: &[KeyCode]) -> u32 {
        self.keys.iter().filter(|(_, k)| held.contains(k)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The actions' mask ([`Action::bit`]) the held gamepad inputs set.
    pub fn pad_bits(&self, held: impl Fn(PadInput) -> bool) -> u32 {
        self.pad.iter().filter(|(_, p)| held(*p)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The actions' mask ([`Action::bit`]) the held mouse inputs set.
    pub fn mouse_bits(&self, held: &[MouseInput]) -> u32 {
        self.mouse.iter().filter(|(_, m)| held.contains(m)).fold(0, |b, (a, _)| b | a.bit())
    }

    /// The bound keys that, pressed together, make a shortcut a browser keeps
    /// for itself ([`is_browser_reserved`]): pairs (and single keys) in
    /// binding order, of [`Bindings::keys`], then of
    /// [`Bindings::mouse_keys`] (never pressed with them), then of
    /// [`Bindings::keys2`]. Spec §8.5: the defaults have none.
    pub fn browser_conflicts(&self) -> Vec<Vec<KeyCode>> {
        let mut out = conflicts(&self.keys);
        for c in conflicts(&self.mouse_keys).into_iter().chain(conflicts(&self.keys2)) {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// The keys both keyboard seats have (issue #28): of keyboard 1's list
    /// with mouse controls on (`mouse_controls`: [`Bindings::mouse_keys`])
    /// or off ([`Bindings::keys`]) that [`Bindings::keys2`] binds too, in
    /// keyboard 2's order, once each. The seats share the keyboard's
    /// events, so one press of such a key presses both players' actions;
    /// the options show them. The defaults have none.
    pub fn shared_keys(&self, mouse_controls: bool) -> Vec<KeyCode> {
        let one = if mouse_controls { &self.mouse_keys } else { &self.keys };
        let mut out = Vec::new();
        for &(_, k) in &self.keys2 {
            if one.iter().any(|(_, j)| *j == k) && !out.contains(&k) {
                out.push(k);
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
