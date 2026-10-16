//! Rebinding for the options (plan 7 task 4, Review Focus 2): an input of
//! either device, binding it with an explicit choice when another action
//! has it, unbinding, repairing a file's bindings, and capturing the next
//! input the player presses.
//!
//! The rule that keeps the game controllable: **every action keeps at
//! least one keyboard key and one gamepad input** ([`Bindings::unreachable`]
//! is empty). A step that would break it is refused and changes nothing;
//! bindings from a file that break it are repaired with OLX's defaults.

use std::fmt;

use gilrs::{Axis, Button};
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::bindings::{Action, Bindings, PadInput, STICK_THRESHOLD, TRIGGER_THRESHOLD};
use crate::state::PadEvent;

/// The two kinds of input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Device {
    Keyboard,
    Gamepad,
}

impl Device {
    pub const ALL: [Device; 2] = [Device::Keyboard, Device::Gamepad];

    /// "key" or "gamepad input", for messages.
    pub fn input_noun(self) -> &'static str {
        match self {
            Device::Keyboard => "key",
            Device::Gamepad => "gamepad input",
        }
    }
}

/// One input of either device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(KeyCode),
    Pad(PadInput),
}

impl Binding {
    pub fn device(self) -> Device {
        match self {
            Binding::Key(_) => Device::Keyboard,
            Binding::Pad(_) => Device::Gamepad,
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Binding::Key(k) => f.write_str(&key_name(k)),
            Binding::Pad(p) => write!(f, "Pad {}", pad_name(p)),
        }
    }
}

/// What to do with an input another action already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolve {
    /// Both keep it: one press does both.
    Share,
    /// It leaves the other actions.
    Move,
}

/// Why a rebinding step is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebindError {
    /// `action` would have no input on `device` left.
    Unreachable { action: Action, device: Device },
}

impl fmt::Display for RebindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RebindError::Unreachable { action, device } => {
                write!(f, "{} would have no {}", action.name(), device.input_noun())
            }
        }
    }
}

impl std::error::Error for RebindError {}

impl Action {
    /// The name the options show (OLX's control names).
    pub fn name(self) -> &'static str {
        match self {
            Action::Up => "Up",
            Action::Down => "Down",
            Action::Left => "Left",
            Action::Right => "Right",
            Action::Shoot => "Shoot",
            Action::Jump => "Jump",
            Action::Rope => "Rope",
            Action::SelectWeapon => "Select weapon",
        }
    }
}

impl Bindings {
    /// The inputs of `device` bound to `action`, in binding order.
    pub fn of(&self, action: Action, device: Device) -> Vec<Binding> {
        match device {
            Device::Keyboard => {
                self.keys.iter().filter(|(a, _)| *a == action).map(|&(_, k)| Binding::Key(k)).collect()
            }
            Device::Gamepad => {
                self.pad.iter().filter(|(a, _)| *a == action).map(|&(_, p)| Binding::Pad(p)).collect()
            }
        }
    }

    /// The actions `input` is bound to, in [`Action::ALL`] order.
    pub fn actions_of(&self, input: Binding) -> Vec<Action> {
        Action::ALL.into_iter().filter(|a| self.has(*a, input)).collect()
    }

    fn has(&self, action: Action, input: Binding) -> bool {
        match input {
            Binding::Key(k) => self.keys.contains(&(action, k)),
            Binding::Pad(p) => self.pad.contains(&(action, p)),
        }
    }

    fn count(&self, action: Action, device: Device) -> usize {
        match device {
            Device::Keyboard => self.keys.iter().filter(|(a, _)| *a == action).count(),
            Device::Gamepad => self.pad.iter().filter(|(a, _)| *a == action).count(),
        }
    }

    /// The actions a device cannot press (none, unless edited by hand).
    pub fn unreachable(&self) -> Vec<(Action, Device)> {
        Action::ALL
            .into_iter()
            .flat_map(|a| Device::ALL.map(|d| (a, d)))
            .filter(|&(a, d)| self.count(a, d) == 0)
            .collect()
    }

    /// Binds `input` to `action`. When other actions have it, `how` says
    /// whether they keep it ([`Resolve::Share`]) or lose it
    /// ([`Resolve::Move`]); a move that would leave one of them without an
    /// input of that device is refused and nothing changes.
    pub fn bind(&mut self, action: Action, input: Binding, how: Resolve) -> Result<(), RebindError> {
        let others: Vec<Action> = self.actions_of(input).into_iter().filter(|a| *a != action).collect();
        if how == Resolve::Move {
            let device = input.device();
            if let Some(&a) = others.iter().find(|a| self.count(**a, device) == 1) {
                return Err(RebindError::Unreachable { action: a, device });
            }
            for a in others {
                self.remove(a, input);
            }
        }
        if !self.has(action, input) {
            match input {
                Binding::Key(k) => self.keys.push((action, k)),
                Binding::Pad(p) => self.pad.push((action, p)),
            }
        }
        Ok(())
    }

    /// Unbinds `input` from `action`; refused when it is the action's last
    /// input of its device. Unbinding what is not bound changes nothing.
    pub fn unbind(&mut self, action: Action, input: Binding) -> Result<(), RebindError> {
        if !self.has(action, input) {
            return Ok(());
        }
        let device = input.device();
        if self.count(action, device) == 1 {
            return Err(RebindError::Unreachable { action, device });
        }
        self.remove(action, input);
        Ok(())
    }

    fn remove(&mut self, action: Action, input: Binding) {
        match input {
            Binding::Key(k) => self.keys.retain(|e| *e != (action, k)),
            Binding::Pad(p) => self.pad.retain(|e| *e != (action, p)),
        }
    }

    /// Makes bindings from a file usable: a pair listed twice is listed
    /// once, and an action a device cannot press gets OLX's defaults for
    /// it back (its keys may then be shared with another action). One
    /// warning per fix.
    pub fn repair(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        let (keys, pad) = (self.keys.len(), self.pad.len());
        dedup(&mut self.keys);
        dedup(&mut self.pad);
        if self.keys.len() + self.pad.len() != keys + pad {
            w.push(format!(
                "{} bindings listed twice, kept once",
                keys + pad - self.keys.len() - self.pad.len()
            ));
        }
        let olx = Bindings::olx();
        for (action, device) in self.unreachable() {
            let restored = olx.of(action, device);
            for b in &restored {
                // Shared with whatever action has it now: never taken away.
                let _ = self.bind(action, *b, Resolve::Share);
            }
            let names: Vec<String> = restored.iter().map(Binding::to_string).collect();
            let what = device.input_noun();
            w.push(format!("{} had no {what}: OLX's {} restored", action.name(), names.join(", ")));
        }
        w
    }
}

/// Keeps the first of each pair.
fn dedup<T: PartialEq + Copy>(list: &mut Vec<T>) {
    let mut seen: Vec<T> = Vec::with_capacity(list.len());
    list.retain(|e| {
        let new = !seen.contains(e);
        if new {
            seen.push(*e);
        }
        new
    });
}

/// The options' "press a key" box (OLX: src/client/DeprecatedGUI/
/// Menu_Options.cpp:1381-1420, `Menu_OptionsWaitInput`): the first key
/// pressed (modifiers too: winit physical keys), or the first gamepad
/// button or stick direction pushed past its threshold, of one device.
/// The caller feeds it every event while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    device: Device,
    got: Option<Binding>,
}

impl Capture {
    pub fn new(device: Device) -> Capture {
        Capture { device, got: None }
    }

    pub fn device(&self) -> Device {
        self.device
    }

    /// The input captured, once there is one.
    pub fn result(&self) -> Option<Binding> {
        self.got
    }

    /// A winit window event: key presses (not repeats).
    pub fn on_window_event(&mut self, ev: &WindowEvent) {
        if let WindowEvent::KeyboardInput { event, .. } = ev
            && let PhysicalKey::Code(code) = event.physical_key
            && !event.repeat
        {
            self.on_key(code, event.state == ElementState::Pressed);
        }
    }

    /// Physical key `code` went down or up.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        if down && self.device == Device::Keyboard {
            self.take(Binding::Key(code));
        }
    }

    /// A gilrs event from any pad.
    pub fn on_gamepad(&mut self, ev: &gilrs::Event) {
        if let Some(p) = PadEvent::from_gilrs(&ev.event) {
            self.on_pad(p);
        }
    }

    /// A gamepad event.
    pub fn on_pad(&mut self, ev: PadEvent) {
        if self.device != Device::Gamepad {
            return;
        }
        let input = match ev {
            PadEvent::Button(b, v) if v >= TRIGGER_THRESHOLD => PadInput::Button(b),
            PadEvent::Axis(a, v) if v >= STICK_THRESHOLD => PadInput::AxisPlus(a),
            PadEvent::Axis(a, v) if v <= -STICK_THRESHOLD => PadInput::AxisMinus(a),
            _ => return,
        };
        self.take(Binding::Pad(input));
    }

    fn take(&mut self, b: Binding) {
        self.got.get_or_insert(b);
    }
}

/// A key's name as the options show it.
pub fn key_name(k: KeyCode) -> String {
    use KeyCode::*;
    let fixed = match k {
        ControlLeft => "Left Ctrl",
        ControlRight => "Right Ctrl",
        AltLeft => "Left Alt",
        AltRight => "Right Alt",
        ShiftLeft => "Left Shift",
        ShiftRight => "Right Shift",
        SuperLeft => "Left Super",
        SuperRight => "Right Super",
        ArrowUp => "Arrow Up",
        ArrowDown => "Arrow Down",
        ArrowLeft => "Arrow Left",
        ArrowRight => "Arrow Right",
        Escape => "Esc",
        Space => "Space",
        Enter => "Enter",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    let debug = format!("{k:?}");
    for prefix in ["Key", "Digit"] {
        if let Some(rest) = debug.strip_prefix(prefix)
            && !rest.is_empty()
        {
            return rest.to_string();
        }
    }
    if let Some(rest) = debug.strip_prefix("Numpad") {
        return format!("Num {rest}");
    }
    debug
}

/// A gamepad input's name (Xbox labels, gilrs's south/east/... layout).
fn pad_name(p: PadInput) -> String {
    let stick = |a: Axis, plus: bool| -> String {
        let (name, x) = match a {
            Axis::LeftStickX => ("left stick", true),
            Axis::LeftStickY => ("left stick", false),
            Axis::RightStickX => ("right stick", true),
            Axis::RightStickY => ("right stick", false),
            Axis::DPadX => ("d-pad", true),
            Axis::DPadY => ("d-pad", false),
            other => return format!("{other:?} {}", if plus { "+" } else { "-" }),
        };
        // gilrs: X grows to the right, Y grows up.
        let dir = match (x, plus) {
            (true, true) => "right",
            (true, false) => "left",
            (false, true) => "up",
            (false, false) => "down",
        };
        format!("{name} {dir}")
    };
    match p {
        PadInput::Button(b) => button_name(b),
        PadInput::AxisPlus(a) => stick(a, true),
        PadInput::AxisMinus(a) => stick(a, false),
    }
}

fn button_name(b: Button) -> String {
    use Button::*;
    let s = match b {
        South => "A",
        East => "B",
        North => "Y",
        West => "X",
        LeftTrigger => "LB",
        LeftTrigger2 => "LT",
        RightTrigger => "RB",
        RightTrigger2 => "RT",
        Select => "Back",
        Start => "Start",
        Mode => "Guide",
        LeftThumb => "left stick press",
        RightThumb => "right stick press",
        DPadUp => "d-pad up",
        DPadDown => "d-pad down",
        DPadLeft => "d-pad left",
        DPadRight => "d-pad right",
        other => return format!("{other:?}"),
    };
    s.to_string()
}
