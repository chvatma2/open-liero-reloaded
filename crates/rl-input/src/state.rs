//! One player's input: device events in, [`Keys`] per tick out.

use gilrs::{Axis, Button, EventType};
use rl_sim::Keys;
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::bindings::{Bindings, PadInput, STICK_THRESHOLD, TRIGGER_THRESHOLD};
use crate::latch::TickLatch;

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
    bindings: Bindings,
    /// Bound keyboard keys held now.
    keys: Vec<KeyCode>,
    /// Gamepad inputs held now, by pad (gilrs's id as `usize`).
    pad: Vec<(usize, PadInput)>,
    latch: TickLatch,
}

impl Default for InputState {
    fn default() -> Self {
        InputState::new(Bindings::default())
    }
}

impl InputState {
    pub fn new(bindings: Bindings) -> InputState {
        InputState { bindings, keys: Vec::new(), pad: Vec::new(), latch: TickLatch::default() }
    }

    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// Replaces the bindings; everything held is released.
    pub fn set_bindings(&mut self, bindings: Bindings) {
        self.bindings = bindings;
        self.keys.clear();
        self.pad.clear();
        self.update();
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
            WindowEvent::Focused(false) => self.release_keys(),
            _ => {}
        }
    }

    /// Physical key `code` went down or up.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        if !self.bindings.keys.iter().any(|(_, k)| *k == code) {
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

    /// The keys held now, unlatched.
    pub fn held(&self) -> Keys {
        Keys(self.latch.held())
    }

    /// The keys for the next tick: what is held now, plus the presses and
    /// releases since the last call, latched so none is lost (spec §8.5).
    pub fn latch_tick(&mut self) -> Keys {
        self.latch.latch()
    }

    /// Feeds what the bound inputs hold now to the latch.
    fn update(&mut self) {
        let keys = self.bindings.key_bits(&self.keys);
        let pad = self.bindings.pad_bits(|p| self.pad.iter().any(|(_, q)| *q == p));
        self.latch.set(keys | pad);
    }
}
