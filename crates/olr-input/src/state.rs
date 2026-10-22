//! One player's input: device events in, [`Keys`] per tick out; the
//! devices of every seat (the solo player's) or of one ([`Seat`], issue #28).

use gilrs::{Axis, Button, EventType};
use olr_sim::Keys;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::bindings::{Bindings, MouseInput, PAD_PAUSE, PadInput, STICK_THRESHOLD, TRIGGER_THRESHOLD};
use crate::dig::Dig;
use crate::latch::TickLatch;
use crate::mouse::{LocalWorm, MADE_UP, MouseController, Notch};
use crate::seat::Seat;

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

/// One player's devices, through [`Bindings`]: the solo player's
/// ([`InputState::new`]: keyboard 1's keys, winit physical keys, the mouse
/// and every gamepad), or one seat's ([`InputState::for_seat`], issue
/// #28). Feed it every window and gamepad event, then call
/// [`InputState::latch_tick`] once per simulation tick.
#[derive(Clone, Debug)]
pub struct InputState {
    /// The player's bindings, as the options show them.
    bindings: Bindings,
    /// The seat heard (`None`: keyboard 1 and every pad).
    seat: Option<Seat>,
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
    /// A heard pad's Start was pressed since [`InputState::take_pause`].
    pause: bool,
}

/// Mouse mode's own state: the buttons, the wheel, the controller.
#[derive(Clone, Debug, Default)]
struct Mouse {
    /// The buttons held now (bound or not).
    buttons: Vec<MouseInput>,
    /// The wheel's turn not yet a whole notch, in notches (up positive).
    wheel: f32,
    /// A notch's actions' bits pressed and released at once (not the
    /// weapon changes, which the controller presses).
    tap: u32,
    controller: MouseController,
}

impl Default for InputState {
    fn default() -> Self {
        InputState::new(Bindings::default())
    }
}

impl InputState {
    /// The solo player's: keyboard 1's keys ([`Bindings::keys`]), mouse
    /// controls when on, and every gamepad ([`Bindings::pad`]), as before
    /// seats; keyboard 2's keys are not heard.
    pub fn new(bindings: Bindings) -> InputState {
        InputState::heard(None, bindings)
    }

    /// One seat's (issue #28): keyboard 1's keys and mouse controls,
    /// keyboard 2's keys ([`Bindings::keys2`]), or one pad's inputs
    /// ([`Bindings::pad`], events of that pad id only). Only keyboard 1
    /// plays with mouse controls.
    pub fn for_seat(seat: Seat, bindings: Bindings) -> InputState {
        InputState::heard(Some(seat), bindings)
    }

    fn heard(seat: Option<Seat>, bindings: Bindings) -> InputState {
        let mut s = InputState {
            active: Bindings::default(),
            bindings,
            seat,
            mouse: None,
            keys: Vec::new(),
            pad: Vec::new(),
            latch: TickLatch::default(),
            dig: Dig::default(),
            pause: false,
        };
        s.active = s.playing();
        s
    }

    /// The seat heard (`None`: the solo player's keyboard 1 and every pad).
    pub fn seat(&self) -> Option<Seat> {
        self.seat
    }

    /// The bindings played with: the seat's lists of
    /// [`InputState::bindings`], or in mouse mode [`Bindings::for_mouse`]
    /// of them.
    fn playing(&self) -> Bindings {
        let b = if self.mouse.is_some() { self.bindings.for_mouse() } else { self.bindings.clone() };
        let none = Bindings {
            keys: Vec::new(),
            pad: Vec::new(),
            mouse_keys: Vec::new(),
            mouse: Vec::new(),
            keys2: Vec::new(),
        };
        match self.seat {
            None => Bindings { keys2: Vec::new(), ..b },
            Some(Seat::Keyboard1) => Bindings { pad: Vec::new(), keys2: Vec::new(), ..b },
            Some(Seat::Keyboard2) => Bindings { keys: b.keys2, ..none },
            Some(Seat::Pad(_)) => Bindings { pad: b.pad, ..none },
        }
    }

    /// Whether this state hears pad `pad`'s events.
    fn hears_pad(&self, pad: usize) -> bool {
        match self.seat {
            None => true,
            Some(Seat::Pad(p)) => p == pad,
            Some(Seat::Keyboard1 | Seat::Keyboard2) => false,
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
    /// released and the controller starts afresh. In mouse mode the keys
    /// are [`Bindings::mouse_keys`] and the mouse [`Bindings::mouse`]
    /// (issue #25; by default A and D move, Strafe made by the controller,
    /// Space jumps, S digs, the left button shoots, the right one throws
    /// the rope, the wheel changes the weapon: [`InputState::on_mouse`]),
    /// and the controller aims and turns the worm
    /// ([`InputState::latch_tick_for`]). Off, the keys are
    /// [`Bindings::keys`] again, untouched. Only the solo player and
    /// keyboard 1's seat have them: another seat stays without.
    pub fn set_mouse_controls(&mut self, on: bool) {
        let on = on && matches!(self.seat, None | Some(Seat::Keyboard1));
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
        self.active = self.playing();
        self.keys.clear();
        self.pad.clear();
        if let Some(m) = &mut self.mouse {
            *m = Mouse::default();
        }
        self.update();
    }

    /// A winit window event of the mouse, in mouse mode (the caller passes
    /// them while the game has the pointer): the buttons, the wheel (line
    /// deltas a notch each; a touchpad's pixel deltas a notch every
    /// [`NOTCH_PIXELS`]), through [`Bindings::mouse`]. Nothing otherwise.
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
        let (Some(m), Some(input)) = (&mut self.mouse, MouseInput::of_button(button)) else { return };
        let at = m.buttons.iter().position(|b| *b == input);
        match (down, at) {
            (true, None) => m.buttons.push(input),
            (false, Some(i)) => {
                m.buttons.swap_remove(i);
            }
            _ => return,
        }
        self.update();
    }

    /// The wheel turned `notches` (up positive; whole ones are notches,
    /// the rest waits for more the same way), in mouse mode: each notch
    /// presses what [`MouseInput::WheelUp`] or `WheelDown` is bound to,
    /// for a tick. The weapon changes wait in the controller's queue
    /// ([`MouseController::notch`]) until the simulation can read them;
    /// the rest is a press and release at once, latched like a click.
    pub fn on_wheel(&mut self, notches: f32) {
        let Some(m) = &mut self.mouse else { return };
        if !notches.is_finite() {
            return;
        }
        if m.wheel * notches < 0.0 {
            m.wheel = 0.0;
        }
        // More than the controller's queue holds would be dropped anyway.
        let most = crate::mouse::WHEEL_QUEUE as f32;
        m.wheel = (m.wheel + notches).clamp(-most, most);
        let mut notches = Vec::new();
        while m.wheel >= 1.0 {
            m.wheel -= 1.0;
            notches.push(MouseInput::WheelUp);
        }
        while m.wheel <= -1.0 {
            m.wheel += 1.0;
            notches.push(MouseInput::WheelDown);
        }
        for n in notches {
            self.notch(n);
        }
    }

    /// One notch of the wheel.
    fn notch(&mut self, input: MouseInput) {
        let bits = self.active.mouse_bits(&[input]);
        let Some(m) = &mut self.mouse else { return };
        if bits & u32::from(Keys::PREV_WEAPON) != 0 {
            m.controller.notch(Notch::Previous);
        }
        if bits & u32::from(Keys::NEXT_WEAPON) != 0 {
            m.controller.notch(Notch::Next);
        }
        let tap = bits & !u32::from(Keys::PREV_WEAPON | Keys::NEXT_WEAPON);
        if tap != 0 {
            m.tap = tap;
            self.update();
            if let Some(m) = &mut self.mouse {
                m.tap = 0;
            }
            self.update();
        }
    }

    /// Releases the mouse buttons (the game lost the pointer).
    pub fn release_mouse(&mut self) {
        if let Some(m) = &mut self.mouse {
            m.buttons.clear();
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

    /// A gamepad event from pad `pad` (nothing for a seat that does not
    /// hear it). Its Start ([`PAD_PAUSE`]) pressed asks for the pause
    /// ([`InputState::take_pause`]).
    pub fn on_pad(&mut self, pad: usize, ev: PadEvent) {
        if !self.hears_pad(pad) {
            return;
        }
        let start = |s: &InputState| s.pad.contains(&(pad, PAD_PAUSE));
        let before = start(self);
        match ev {
            PadEvent::Button(b, v) => self.set_pad(pad, PadInput::Button(b), v >= TRIGGER_THRESHOLD),
            PadEvent::Axis(a, v) => {
                self.set_pad(pad, PadInput::AxisPlus(a), v >= STICK_THRESHOLD);
                self.set_pad(pad, PadInput::AxisMinus(a), v <= -STICK_THRESHOLD);
            }
            PadEvent::Disconnected => self.pad.retain(|(p, _)| *p != pad),
        }
        if !before && start(self) {
            self.pause = true;
        }
        self.update();
    }

    /// A heard pad's Start was pressed since the last call (issue #28:
    /// Start pauses, as Esc does). Start presses no action
    /// ([`PAD_PAUSE`] is never bound).
    pub fn take_pause(&mut self) -> bool {
        std::mem::take(&mut self.pause)
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
        sim_keys(self.latch.held())
    }

    /// The actions held now ([`crate::Action::bit`]'s mask), Dig
    /// ([`crate::DIG`]) included: what a menu driven by the game's keys reads
    /// (the join step, issue #32).
    pub fn actions_held(&self) -> u32 {
        self.latch.held()
    }

    /// A heard pad's Start ([`PAD_PAUSE`]) is held now.
    pub fn start_held(&self) -> bool {
        self.pad.iter().any(|(_, i)| *i == PAD_PAUSE)
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
        let (keys, pressed) = self.dig.tick(latched, last);
        self.latch.tap(pressed);
        let keys = sim_keys(keys);
        match &mut self.mouse {
            Some(m) => m.controller.keys(local, cursor, keys),
            None => keys,
        }
    }

    /// Feeds what the bound inputs hold now to the latch.
    fn update(&mut self) {
        let keys = self.active.key_bits(&self.keys);
        let pad = self.active.pad_bits(|p| self.pad.iter().any(|(_, q)| *q == p));
        let mouse = self.mouse.as_ref().map_or(0, |m| self.active.mouse_bits(&m.buttons) | m.tap);
        self.latch.set(keys | pad | mouse);
    }
}

/// The simulation's keys of an actions' mask: its low 16 bits, without
/// [`crate::DIG`] (olr-input's own).
fn sim_keys(bits: u32) -> Keys {
    Keys((bits & u32::from(Keys::ALL)) as u16)
}
