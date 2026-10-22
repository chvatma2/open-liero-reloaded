//! One player's input: device events in, [`Keys`] per tick out.

use gilrs::{Axis, Button, EventType};
use olr_sim::Keys;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::bindings::{Bindings, DIG, PadInput, STICK_THRESHOLD, TRIGGER_THRESHOLD};
use crate::dig::Dig;
use crate::latch::TickLatch;
use crate::mouse::{LocalWorm, MADE_UP, MouseController, Notch};

/// A touchpad's scroll (pixel deltas) makes a wheel notch every this many
/// logical pixels.
pub const NOTCH_PIXELS: f32 = 40.0;

/// A gamepad event, as [`InputState::on_pad`] takes it (gilrs's own events
/// carry an id and a platform code that only gilrs can make).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadEvent {
    /// A button's value, 0 to 1 (gilrs's `ButtonChanged`, which it sends
    /// for every button change, digital ones as 0 and 1).
    Button(Button, f32),
    /// An axis's value, -1 to 1.
    Axis(Axis, f32),
    /// The pad is gone: everything it held is released.
    Disconnected,
}

impl PadEvent {
    /// The event gilrs reports, if it changes what is held.
    pub fn from_gilrs(ev: &EventType) -> Option<PadEvent> {
        match *ev {
            EventType::ButtonChanged(b, v, _) => Some(PadEvent::Button(b, v)),
            EventType::AxisChanged(a, v, _) => Some(PadEvent::Axis(a, v)),
            EventType::Disconnected => Some(PadEvent::Disconnected),
            _ => None,
        }
    }
}

/// One player's devices: the keyboard (winit physical keys) and any
/// gamepad, through [`Bindings`]. Feed it every window and gamepad event,
/// then call [`InputState::latch_tick`] once per simulation tick.
#[derive(Clone, Debug)]
pub struct InputState {
    /// The player's bindings, as the options show them.
    bindings: Bindings,
    /// The bindings played with: [`InputState::bindings`], or in mouse mode
    /// [`Bindings::for_mouse`] of them.
    active: Bindings,
    /// Mouse controls (issue #24), while on.
    mouse: Option<Mouse>,
    /// Bound keyboard keys held now.
    keys: Vec<KeyCode>,
    /// Gamepad inputs held now, by pad (gilrs's id as `usize`).
    pad: Vec<(usize, PadInput)>,
    latch: TickLatch,
    dig: Dig,
}

/// Mouse mode's own state: the buttons, the wheel, the controller.
#[derive(Clone, Debug, Default)]
struct Mouse {
    /// Shoot (left button) and Rope (right) while held.
    buttons: u16,
    /// The wheel's turn not yet a whole notch, in notches (up positive).
    wheel: f32,
    controller: MouseController,
}

impl Default for InputState {
    fn default() -> Self {
        InputState::new(Bindings::default())
    }
}

impl InputState {
    pub fn new(bindings: Bindings) -> InputState {
        InputState {
            active: bindings.clone(),
            bindings,
            mouse: None,
            keys: Vec::new(),
            pad: Vec::new(),
            latch: TickLatch::default(),
            dig: Dig::default(),
        }
    }

    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// Replaces the bindings; everything held is released.
    pub fn set_bindings(&mut self, bindings: Bindings) {
        self.bindings = bindings;
        self.reactivate();
    }

    /// Turns mouse controls (issue #24) on or off; everything held is
    /// released and the controller starts afresh. In mouse mode A and D
    /// move (Strafe made by the controller), Space jumps, S digs, the left
    /// button shoots, the right one throws the rope, the wheel changes the
    /// weapon ([`InputState::on_mouse`]), and the controller aims and
    /// turns the worm ([`InputState::latch_tick_for`]).
    pub fn set_mouse_controls(&mut self, on: bool) {
        if on == self.mouse.is_some() {
            return;
        }
        self.mouse = on.then(Mouse::default);
        self.reactivate();
    }

    /// Mouse controls are on.
    pub fn mouse_controls(&self) -> bool {
        self.mouse.is_some()
    }

    fn reactivate(&mut self) {
        self.active = if self.mouse.is_some() { self.bindings.for_mouse() } else { self.bindings.clone() };
        self.keys.clear();
        self.pad.clear();
        if let Some(m) = &mut self.mouse {
            *m = Mouse::default();
        }
        self.update();
    }

    /// A winit window event of the mouse, in mouse mode (the caller passes
    /// them while the game has the pointer): the left and right buttons,
    /// the wheel (line deltas a notch each; a touchpad's pixel deltas a
    /// notch every [`NOTCH_PIXELS`]). Nothing otherwise.
    pub fn on_mouse(&mut self, ev: &WindowEvent) {
        match *ev {
            WindowEvent::MouseInput { state, button, .. } => {
                self.on_mouse_button(button, state == ElementState::Pressed)
            }
            WindowEvent::MouseWheel { delta, .. } => match delta {
                MouseScrollDelta::LineDelta(_, y) => self.on_wheel(y),
                MouseScrollDelta::PixelDelta(p) => self.on_wheel(p.y as f32 / NOTCH_PIXELS),
            },
            _ => {}
        }
    }

    /// Mouse button `button` went down or up (mouse mode).
    pub fn on_mouse_button(&mut self, button: MouseButton, down: bool) {
        let Some(m) = &mut self.mouse else { return };
        let bit = match button {
            MouseButton::Left => Keys::FIRE,
            MouseButton::Right => Keys::ROPE,
            _ => return,
        };
        if down {
            m.buttons |= bit;
        } else {
            m.buttons &= !bit;
        }
        self.update();
    }

    /// The wheel turned `notches` (up positive; whole ones are notches,
    /// the rest waits for more the same way). Up selects the previous
    /// weapon, down the next (mouse mode).
    pub fn on_wheel(&mut self, notches: f32) {
        let Some(m) = &mut self.mouse else { return };
        if !notches.is_finite() {
            return;
        }
        if m.wheel * notches < 0.0 {
            m.wheel = 0.0;
        }
        m.wheel += notches;
        while m.wheel >= 1.0 {
            m.wheel -= 1.0;
            m.controller.notch(Notch::Previous);
        }
        while m.wheel <= -1.0 {
            m.wheel += 1.0;
            m.controller.notch(Notch::Next);
        }
    }

    /// Releases the mouse buttons (the game lost the pointer).
    pub fn release_mouse(&mut self) {
        if let Some(m) = &mut self.mouse {
            m.buttons = 0;
            m.wheel = 0.0;
            self.update();
        }
    }

    /// A winit window event: physical key presses and releases (repeats are
    /// ignored), and focus loss, which releases every key (the window no
    /// longer hears their releases).
    pub fn on_window_event(&mut self, ev: &WindowEvent) {
        match ev {
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && !event.repeat
                {
                    self.on_key(code, event.state == ElementState::Pressed);
                }
            }
            WindowEvent::Focused(false) => {
                self.release_keys();
                self.release_mouse();
            }
            _ => {}
        }
    }

    /// Physical key `code` went down or up.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        if !self.active.keys.iter().any(|(_, k)| *k == code) {
            return;
        }
        let at = self.keys.iter().position(|k| *k == code);
        match (down, at) {
            (true, None) => self.keys.push(code),
            (false, Some(i)) => {
                self.keys.swap_remove(i);
            }
            _ => return,
        }
        self.update();
    }

    /// Releases every keyboard key.
    pub fn release_keys(&mut self) {
        self.keys.clear();
        self.update();
    }

    /// A gilrs event from any pad.
    pub fn on_gamepad(&mut self, ev: &gilrs::Event) {
        if let Some(p) = PadEvent::from_gilrs(&ev.event) {
            self.on_pad(usize::from(ev.id), p);
        }
    }

    /// A gamepad event from pad `pad`.
    pub fn on_pad(&mut self, pad: usize, ev: PadEvent) {
        match ev {
            PadEvent::Button(b, v) => self.set_pad(pad, PadInput::Button(b), v >= TRIGGER_THRESHOLD),
            PadEvent::Axis(a, v) => {
                self.set_pad(pad, PadInput::AxisPlus(a), v >= STICK_THRESHOLD);
                self.set_pad(pad, PadInput::AxisMinus(a), v <= -STICK_THRESHOLD);
            }
            PadEvent::Disconnected => self.pad.retain(|(p, _)| *p != pad),
        }
        self.update();
    }

    fn set_pad(&mut self, pad: usize, input: PadInput, down: bool) {
        let at = self.pad.iter().position(|e| *e == (pad, input));
        match (down, at) {
            (true, None) => self.pad.push((pad, input)),
            (false, Some(i)) => {
                self.pad.swap_remove(i);
            }
            _ => {}
        }
    }

    /// The keys held now, unlatched (Dig's gesture not included; in mouse
    /// mode the buttons included, the controller's own keys not).
    pub fn held(&self) -> Keys {
        Keys(self.latch.held() & !DIG)
    }

    /// The keys a stalled online game may repeat of the last ones latched
    /// (the network pump, `olr-game`): those [`InputState::held`], and in
    /// mouse mode the controller's own Up, Down and Strafe, without which
    /// A and D would turn the worm.
    pub fn repeatable(&self) -> Keys {
        let made = if self.mouse.is_some() { MADE_UP } else { 0 };
        Keys(self.held().0 | made)
    }

    /// The keys for the next tick: what is held now, plus the presses and
    /// releases since the last call, latched so none is lost (spec §8.5),
    /// and the dig gesture when Dig digs this tick (`dig.rs`). In mouse
    /// mode, as [`InputState::latch_tick_for`] without a worm.
    pub fn latch_tick(&mut self) -> Keys {
        self.latch_tick_for(None, None)
    }

    /// [`InputState::latch_tick`], then in mouse mode the controller's
    /// keys (`mouse.rs`): `local` is the local worm before the next tick
    /// (`None`: none plays), `cursor` the cursor's direction from the drawn
    /// worm in game pixels.
    pub fn latch_tick_for(&mut self, local: Option<&LocalWorm>, cursor: Option<[f32; 2]>) -> Keys {
        let last = self.latch.last();
        let latched = self.latch.latch();
        let (keys, pressed) = self.dig.tick(latched.0, last);
        self.latch.tap(pressed);
        match &mut self.mouse {
            Some(m) => m.controller.keys(local, cursor, Keys(keys)),
            None => Keys(keys),
        }
    }

    /// Feeds what the bound inputs hold now to the latch.
    fn update(&mut self) {
        let keys = self.active.key_bits(&self.keys);
        let pad = self.active.pad_bits(|p| self.pad.iter().any(|(_, q)| *q == p));
        let buttons = self.mouse.as_ref().map_or(0, |m| m.buttons);
        self.latch.set(keys | pad | buttons);
    }
}
