//! Rebinding for the options (plan 7 task 4, Review Focus 2; mouse
//! inputs issue #25): an input of any device, binding it with an explicit
//! choice when another action has it, unbinding, repairing a file's
//! bindings, and capturing the next input the player presses.
//!
//! The rule that keeps the game controllable, in every [`Mode`] and for
//! every seat on its own (issue #28: [`crate::SeatKind`],
//! [`Bindings::unreachable`] is empty): **every action keeps at least one
//! key of keyboard 1's** ([`Bindings::keys`]), **one of keyboard 2's**
//! ([`Bindings::keys2`]) **and one gamepad input**; with mouse controls
//! on, **every action but [`MOUSE_MADE`]'s keeps a key or a mouse input**
//! of keyboard 1's (of [`Bindings::mouse_keys`] and [`Bindings::mouse`]).
//! A seat needs only its own device's: a keyboard seat keys, the pads pad
//! inputs. A step that would break it is refused and changes nothing;
//! bindings from a file that break it are repaired with the defaults.

use std::fmt;

use gilrs::{Axis, Button};
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::bindings::{
    Action, Bindings, MOUSE_MADE, MouseInput, PadInput, STICK_THRESHOLD, TRIGGER_THRESHOLD,
};
use crate::state::{NOTCH_PIXELS, PadEvent};

/// The kinds of input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Device {
    Keyboard,
    Gamepad,
    /// Mouse controls' buttons and wheel (issue #25).
    Mouse,
}

impl Device {
    pub const ALL: [Device; 3] = [Device::Keyboard, Device::Gamepad, Device::Mouse];

    /// "key", "gamepad input" or "mouse input", for messages.
    pub fn input_noun(self) -> &'static str {
        match self {
            Device::Keyboard => "key",
            Device::Gamepad => "gamepad input",
            Device::Mouse => "mouse input",
        }
    }
}

/// Which controls a binding is for: keyboard 1's keys (mouse controls
/// off) or mouse controls (issue #25), each with its own keyboard list
/// (the gamepads' list is the same in both), or keyboard 2's keys (issue
/// #28; its keys alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Keys,
    Mouse,
    Keyboard2,
}

impl Mode {
    /// Mouse controls on or off.
    pub fn of(mouse_controls: bool) -> Mode {
        if mouse_controls { Mode::Mouse } else { Mode::Keys }
    }

    /// The devices this mode binds, as the options show them.
    pub fn devices(self) -> &'static [Device] {
        match self {
            Mode::Keys => &[Device::Keyboard, Device::Gamepad],
            Mode::Mouse => &[Device::Keyboard, Device::Mouse, Device::Gamepad],
            Mode::Keyboard2 => &[Device::Keyboard],
        }
    }

    /// `action` takes inputs of `device` in this mode: not the mouse with
    /// mouse controls off, nor a key or mouse input for [`MOUSE_MADE`]'s
    /// with them on; keyboard 2 nothing but keys.
    pub fn binds(self, action: Action, device: Device) -> bool {
        match (self, device) {
            (Mode::Keyboard2, d) => d == Device::Keyboard,
            (_, Device::Gamepad) => true,
            (Mode::Keys, Device::Keyboard) => true,
            (Mode::Keys, Device::Mouse) => false,
            (Mode::Mouse, _) => !MOUSE_MADE.contains(&action),
        }
    }
}

/// One input of any device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(KeyCode),
    Pad(PadInput),
    Mouse(MouseInput),
}

impl Binding {
    pub fn device(self) -> Device {
        match self {
            Binding::Key(_) => Device::Keyboard,
            Binding::Pad(_) => Device::Gamepad,
            Binding::Mouse(_) => Device::Mouse,
        }
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Binding::Key(k) => f.write_str(&key_name(k)),
            Binding::Pad(p) => write!(f, "Pad {}", pad_name(p)),
            Binding::Mouse(m) => f.write_str(m.name()),
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

/// What every action must keep at least one of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Need {
    /// A key of [`Bindings::keys`] (keyboard 1, mouse controls off).
    Key,
    /// A gamepad input (either mode).
    PadInput,
    /// A key of [`Bindings::mouse_keys`] or an input of
    /// [`Bindings::mouse`], with mouse controls on; not of [`MOUSE_MADE`]'s.
    KeyOrMouse,
    /// A key of [`Bindings::keys2`] (keyboard 2, issue #28).
    Keyboard2Key,
}

impl Need {
    /// What the action would lack, for messages.
    pub fn noun(self) -> &'static str {
        match self {
            Need::Key => "key",
            Need::PadInput => "gamepad input",
            Need::KeyOrMouse => "key or mouse input with mouse controls",
            Need::Keyboard2Key => "keyboard 2 key",
        }
    }
}

/// Why a rebinding step is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebindError {
    /// `action` would have nothing of `need` left.
    Unreachable { action: Action, need: Need },
    /// Mouse controls press `action` themselves ([`MOUSE_MADE`]), or the
    /// mouse is not bound with them off.
    NotBindable { action: Action, device: Device },
    /// Keyboard 2 binds keys only (issue #28).
    KeysOnly { device: Device },
}

impl fmt::Display for RebindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RebindError::Unreachable { action, need } => {
                write!(f, "{} would have no {}", action.name(), need.noun())
            }
            RebindError::NotBindable { action, device: Device::Mouse } if !MOUSE_MADE.contains(action) => {
                write!(f, "the mouse is bound only with mouse controls on")
            }
            RebindError::NotBindable { action, .. } => {
                write!(f, "mouse controls press {} themselves", action.name())
            }
            RebindError::KeysOnly { device } => write!(f, "keyboard 2 has no {}", device.input_noun()),
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
            Action::Strafe => "Strafe",
            Action::PreviousWeapon => "Previous weapon",
            Action::NextWeapon => "Next weapon",
            Action::Dig => "Dig",
        }
    }
}

/// One of [`Bindings`]' five lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum List {
    Keys,
    Pad,
    MouseKeys,
    Mouse,
    Keys2,
}

impl List {
    /// The list `mode` binds `device` in (keyboard 2's for any device: it
    /// binds keys only, [`Mode::binds`]).
    fn of(mode: Mode, device: Device) -> List {
        match (mode, device) {
            (Mode::Keyboard2, _) => List::Keys2,
            (Mode::Keys, Device::Keyboard) => List::Keys,
            (Mode::Mouse, Device::Keyboard) => List::MouseKeys,
            (_, Device::Gamepad) => List::Pad,
            (_, Device::Mouse) => List::Mouse,
        }
    }

    /// What an action of this list's must keep (`None`: nothing).
    fn need(self, action: Action) -> Option<Need> {
        match self {
            List::Keys => Some(Need::Key),
            List::Pad => Some(Need::PadInput),
            List::Keys2 => Some(Need::Keyboard2Key),
            List::MouseKeys | List::Mouse => (!MOUSE_MADE.contains(&action)).then_some(Need::KeyOrMouse),
        }
    }
}

impl Bindings {
    /// The inputs of `device` bound to `action` in `mode`, in binding
    /// order (none where the mode binds none: [`Mode::binds`]).
    pub fn of(&self, mode: Mode, action: Action, device: Device) -> Vec<Binding> {
        if !mode.binds(action, device) {
            return Vec::new();
        }
        self.list(List::of(mode, device)).into_iter().filter(|(a, _)| *a == action).map(|(_, b)| b).collect()
    }

    /// The actions `input` is bound to in `mode`, in [`Action::ALL`] order.
    pub fn actions_of(&self, mode: Mode, input: Binding) -> Vec<Action> {
        let device = input.device();
        let list = self.list(List::of(mode, device));
        Action::ALL.into_iter().filter(|a| mode.binds(*a, device) && list.contains(&(*a, input))).collect()
    }

    /// A list as bindings.
    fn list(&self, list: List) -> Vec<(Action, Binding)> {
        match list {
            List::Keys => self.keys.iter().map(|&(a, k)| (a, Binding::Key(k))).collect(),
            List::Pad => self.pad.iter().map(|&(a, p)| (a, Binding::Pad(p))).collect(),
            List::MouseKeys => self.mouse_keys.iter().map(|&(a, k)| (a, Binding::Key(k))).collect(),
            List::Mouse => self.mouse.iter().map(|&(a, m)| (a, Binding::Mouse(m))).collect(),
            List::Keys2 => self.keys2.iter().map(|&(a, k)| (a, Binding::Key(k))).collect(),
        }
    }

    fn has(&self, list: List, action: Action, input: Binding) -> bool {
        self.list(list).contains(&(action, input))
    }

    /// How many inputs give `action` what it needs.
    fn count(&self, action: Action, need: Need) -> usize {
        let n = |list: List| self.list(list).iter().filter(|(a, _)| *a == action).count();
        match need {
            Need::Key => n(List::Keys),
            Need::PadInput => n(List::Pad),
            Need::KeyOrMouse => n(List::MouseKeys) + n(List::Mouse),
            Need::Keyboard2Key => n(List::Keys2),
        }
    }

    /// What an action cannot be pressed with (none, unless edited by
    /// hand), every seat's gaps: keyboard 1's key with mouse controls off,
    /// a gamepad input, keyboard 1's key or mouse input with them on
    /// ([`MOUSE_MADE`]'s need none), keyboard 2's key.
    pub fn unreachable(&self) -> Vec<(Action, Need)> {
        let mut out = Vec::new();
        for need in [Need::Key, Need::PadInput, Need::KeyOrMouse, Need::Keyboard2Key] {
            for a in Action::ALL {
                let needed = need != Need::KeyOrMouse || !MOUSE_MADE.contains(&a);
                if needed && self.count(a, need) == 0 {
                    out.push((a, need));
                }
            }
        }
        out
    }

    /// Binds `input` to `action` in `mode`. When other actions have it,
    /// `how` says whether they keep it ([`Resolve::Share`]) or lose it
    /// ([`Resolve::Move`]); a move that would leave one of them unreachable
    /// is refused and nothing changes. So is a key or mouse input for
    /// [`MOUSE_MADE`]'s in mouse mode, and the mouse with it off.
    pub fn bind(
        &mut self,
        mode: Mode,
        action: Action,
        input: Binding,
        how: Resolve,
    ) -> Result<(), RebindError> {
        let device = input.device();
        if mode == Mode::Keyboard2 && device != Device::Keyboard {
            return Err(RebindError::KeysOnly { device });
        }
        if !mode.binds(action, device) {
            return Err(RebindError::NotBindable { action, device });
        }
        let list = List::of(mode, device);
        let others: Vec<Action> = self.actions_of(mode, input).into_iter().filter(|a| *a != action).collect();
        if how == Resolve::Move {
            for &a in &others {
                if let Some(need) = list.need(a)
                    && self.count(a, need) == 1
                {
                    return Err(RebindError::Unreachable { action: a, need });
                }
            }
            for a in others {
                self.remove(list, a, input);
            }
        }
        if !self.has(list, action, input) {
            match (list, input) {
                (List::Keys, Binding::Key(k)) => self.keys.push((action, k)),
                (List::MouseKeys, Binding::Key(k)) => self.mouse_keys.push((action, k)),
                (List::Pad, Binding::Pad(p)) => self.pad.push((action, p)),
                (List::Mouse, Binding::Mouse(m)) => self.mouse.push((action, m)),
                (List::Keys2, Binding::Key(k)) => self.keys2.push((action, k)),
                _ => {}
            }
        }
        Ok(())
    }

    /// Unbinds `input` from `action` in `mode`; refused when the action
    /// would be unreachable. Unbinding what is not bound changes nothing.
    pub fn unbind(&mut self, mode: Mode, action: Action, input: Binding) -> Result<(), RebindError> {
        let list = List::of(mode, input.device());
        if !mode.binds(action, input.device()) || !self.has(list, action, input) {
            return Ok(());
        }
        if let Some(need) = list.need(action)
            && self.count(action, need) == 1
        {
            return Err(RebindError::Unreachable { action, need });
        }
        self.remove(list, action, input);
        Ok(())
    }

    fn remove(&mut self, list: List, action: Action, input: Binding) {
        match (list, input) {
            (List::Keys, Binding::Key(k)) => self.keys.retain(|e| *e != (action, k)),
            (List::MouseKeys, Binding::Key(k)) => self.mouse_keys.retain(|e| *e != (action, k)),
            (List::Pad, Binding::Pad(p)) => self.pad.retain(|e| *e != (action, p)),
            (List::Mouse, Binding::Mouse(m)) => self.mouse.retain(|e| *e != (action, m)),
            (List::Keys2, Binding::Key(k)) => self.keys2.retain(|e| *e != (action, k)),
            _ => {}
        }
    }

    /// Makes bindings from a file usable: a pair listed twice is listed
    /// once, a key or mouse input of [`MOUSE_MADE`]'s in mouse controls'
    /// lists is dropped, and an action left unreachable in either mode
    /// gets its defaults back (OLX's keys and pad inputs, #24's mouse
    /// controls, OLX's player 2's for keyboard 2; shared with another
    /// action that may have them). One warning per fix. A file from before
    /// mouse bindings has #24's, one from before seats OLX's player 2's
    /// (`#[serde(default)]`).
    pub fn repair(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        let lens =
            |b: &Bindings| b.keys.len() + b.pad.len() + b.mouse_keys.len() + b.mouse.len() + b.keys2.len();
        let before = lens(self);
        dedup(&mut self.keys);
        dedup(&mut self.pad);
        dedup(&mut self.mouse_keys);
        dedup(&mut self.mouse);
        dedup(&mut self.keys2);
        if lens(self) != before {
            w.push(format!("{} bindings listed twice, kept once", before - lens(self)));
        }
        for (a, b) in self.list(List::MouseKeys).into_iter().chain(self.list(List::Mouse)) {
            if MOUSE_MADE.contains(&a) {
                self.remove(List::of(Mode::Mouse, b.device()), a, b);
                w.push(format!("{b} unbound from {}: mouse controls press it themselves", a.name()));
            }
        }
        let defaults = Bindings::olx();
        for (action, need) in self.unreachable() {
            let (mode, devices): (Mode, &[Device]) = match need {
                Need::Key => (Mode::Keys, &[Device::Keyboard]),
                Need::PadInput => (Mode::Keys, &[Device::Gamepad]),
                Need::KeyOrMouse => (Mode::Mouse, &[Device::Keyboard, Device::Mouse]),
                Need::Keyboard2Key => (Mode::Keyboard2, &[Device::Keyboard]),
            };
            let restored: Vec<Binding> = devices.iter().flat_map(|d| defaults.of(mode, action, *d)).collect();
            for b in &restored {
                // Shared with whatever action has it now: never taken away.
                let _ = self.bind(mode, action, *b, Resolve::Share);
            }
            let names: Vec<String> = restored.iter().map(Binding::to_string).collect();
            let whose = match need {
                Need::KeyOrMouse => "mouse controls'",
                Need::Keyboard2Key => "OLX's player 2's",
                Need::Key | Need::PadInput => "OLX's",
            };
            w.push(format!(
                "{} had no {}: {whose} {} restored",
                action.name(),
                need.noun(),
                names.join(", ")
            ));
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
/// pressed (modifiers too: winit physical keys), the first gamepad button
/// or stick direction pushed past its threshold, or the first mouse button
/// pressed or wheel notch turned (issue #25), of one device. The caller
/// feeds it every event while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    device: Device,
    got: Option<Binding>,
    /// The wheel's turn not yet a notch (up positive).
    wheel: f32,
    /// Esc was pressed while capturing another device's input.
    cancelled: bool,
}

impl Capture {
    pub fn new(device: Device) -> Capture {
        Capture { device, got: None, wheel: 0.0, cancelled: false }
    }

    pub fn device(&self) -> Device {
        self.device
    }

    /// The input captured, once there is one.
    pub fn result(&self) -> Option<Binding> {
        self.got
    }

    /// Esc was pressed while capturing a gamepad or mouse input (the
    /// keyboard's capture takes Esc as its key: the options refuse it).
    /// The caller closes the box, as egui hears no key meanwhile.
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// A winit window event: key presses (not repeats), mouse button
    /// presses, the wheel.
    pub fn on_window_event(&mut self, ev: &WindowEvent) {
        match ev {
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && !event.repeat
                {
                    self.on_key(code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(m) = MouseInput::of_button(*button) {
                    self.on_mouse_button(m, *state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => match *delta {
                MouseScrollDelta::LineDelta(_, y) => self.on_wheel(y),
                MouseScrollDelta::PixelDelta(p) => self.on_wheel(p.y as f32 / NOTCH_PIXELS),
            },
            _ => {}
        }
    }

    /// Physical key `code` went down or up.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        if !down {
            return;
        }
        if self.device == Device::Keyboard {
            self.take(Binding::Key(code));
        } else if code == KeyCode::Escape && self.got.is_none() {
            self.cancelled = true;
        }
    }

    /// Mouse button `button` went down or up (a wheel input is ignored:
    /// [`Capture::on_wheel`]).
    pub fn on_mouse_button(&mut self, button: MouseInput, down: bool) {
        if down && self.device == Device::Mouse && !button.is_wheel() {
            self.take(Binding::Mouse(button));
        }
    }

    /// The wheel turned `notches` (up positive): a whole notch one way is
    /// captured, as [`crate::InputState::on_wheel`] counts them.
    pub fn on_wheel(&mut self, notches: f32) {
        if self.device != Device::Mouse || !notches.is_finite() {
            return;
        }
        if self.wheel * notches < 0.0 {
            self.wheel = 0.0;
        }
        self.wheel += notches;
        if self.wheel >= 1.0 {
            self.take(Binding::Mouse(MouseInput::WheelUp));
        } else if self.wheel <= -1.0 {
            self.take(Binding::Mouse(MouseInput::WheelDown));
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
