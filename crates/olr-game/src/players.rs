//! The local players' inputs (issue #29): one `olr_input::InputState`
//! per seat, local player `k` on seat [`SEATS`]`[k]`. A player alone plays
//! as before seats (`InputState::new`: keyboard 1's keys, the mouse and
//! every gamepad); among several, player 1 plays keyboard 1 (and the
//! mouse), player 2 keyboard 2 (OLX's player 2's keys), players 3 and 4
//! the first two gamepads, until the menus let players choose their seats
//! (issue #31). Every event reaches every seat; each hears only its own
//! devices, so one player's keys never reach another's worm.
//!
//! OLX's fork plays up to four local players, each from its own controls
//! (`docs/porting/olx-notes/presentation.md` §1.1, §10.1).
//!
//! OLX: src/client/CClient.cpp:1527-1551

use olr_input::{Bindings, InputState, KeyCode, PadEvent, Seat};
use olr_sim::Keys;
use winit::event::WindowEvent;

use crate::setup::MAX_LOCALS;

/// Each local player's seat among several, in local order.
pub const SEATS: [Seat; MAX_LOCALS] = [Seat::Keyboard1, Seat::Keyboard2, Seat::Pad(0), Seat::Pad(1)];

/// The local players' inputs: player 0's (with mouse controls, when on)
/// and the others', one per seat.
#[derive(Clone, Debug)]
pub struct LocalInputs {
    bindings: Bindings,
    mouse: bool,
    /// Local player 0's: the solo player's alone, keyboard 1's seat among
    /// several.
    first: InputState,
    /// Local players 1 and up, on seats `SEATS[1..]`.
    rest: Vec<InputState>,
}

impl Default for LocalInputs {
    fn default() -> Self {
        LocalInputs::new(Bindings::default(), false)
    }
}

impl LocalInputs {
    /// One player, alone, with `bindings` and mouse controls `mouse`.
    pub fn new(bindings: Bindings, mouse: bool) -> LocalInputs {
        let mut first = InputState::new(bindings.clone());
        first.set_mouse_controls(mouse);
        LocalInputs { bindings, mouse, first, rest: Vec::new() }
    }

    /// How many local players it holds inputs for.
    pub fn players(&self) -> usize {
        1 + self.rest.len()
    }

    /// Inputs for `n` local players (1 to [`MAX_LOCALS`]; a game nobody
    /// here plays counts as one): unchanged when it holds that many,
    /// otherwise made afresh on their seats (everything held released).
    pub fn fit(&mut self, n: usize) {
        let n = n.clamp(1, MAX_LOCALS);
        if n == self.players() {
            return;
        }
        let seat = |s: Seat| InputState::for_seat(s, self.bindings.clone());
        self.first = if n == 1 { InputState::new(self.bindings.clone()) } else { seat(SEATS[0]) };
        self.first.set_mouse_controls(self.mouse);
        self.rest = SEATS[1..n].iter().map(|&s| seat(s)).collect();
    }

    /// Local player 0's input (mouse controls', the online player's, the
    /// weapon selection's).
    pub fn first(&self) -> &InputState {
        &self.first
    }

    pub fn first_mut(&mut self) -> &mut InputState {
        &mut self.first
    }

    /// Local player `k`'s input.
    pub fn get(&self, k: usize) -> Option<&InputState> {
        if k == 0 { Some(&self.first) } else { self.rest.get(k - 1) }
    }

    fn each_mut(&mut self) -> impl Iterator<Item = &mut InputState> {
        std::iter::once(&mut self.first).chain(&mut self.rest)
    }

    /// Every seat's bindings (everything held released).
    pub fn set_bindings(&mut self, bindings: Bindings) {
        for i in self.each_mut() {
            i.set_bindings(bindings.clone());
        }
        self.bindings = bindings;
    }

    /// Mouse controls on or off: player 0's (only keyboard 1 has them).
    pub fn set_mouse_controls(&mut self, on: bool) {
        self.mouse = on;
        self.first.set_mouse_controls(on);
    }

    /// A window event, to every seat (each hears its own keys).
    pub fn on_window_event(&mut self, ev: &WindowEvent) {
        for i in self.each_mut() {
            i.on_window_event(ev);
        }
    }

    /// A key, to every seat.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        for i in self.each_mut() {
            i.on_key(code, down);
        }
    }

    /// A gilrs event, to every seat (each hears its own pad).
    pub fn on_gamepad(&mut self, ev: &gilrs::Event) {
        for i in self.each_mut() {
            i.on_gamepad(ev);
        }
    }

    /// Pad `pad`'s event, to every seat (tests: gilrs's events cannot be
    /// made outside gilrs).
    pub fn on_pad(&mut self, pad: usize, ev: PadEvent) {
        for i in self.each_mut() {
            i.on_pad(pad, ev);
        }
    }

    /// Every seat's keys released.
    pub fn release_keys(&mut self) {
        for i in self.each_mut() {
            i.release_keys();
        }
    }

    /// Whether a heard pad's Start was pressed on any seat since the last
    /// call.
    pub fn take_pause(&mut self) -> bool {
        self.each_mut().fold(false, |any, i| i.take_pause() | any)
    }

    /// Each player's keys held now, in local order.
    pub fn held(&self) -> Vec<Keys> {
        std::iter::once(&self.first).chain(&self.rest).map(InputState::held).collect()
    }

    /// Local players 1 and up's keys for this tick, after `first` (player
    /// 0's, which the caller latched with mouse controls' aim,
    /// `InputState::latch_tick_for`): every player's, in local order.
    pub fn latch(&mut self, first: Keys) -> Vec<Keys> {
        std::iter::once(first).chain(self.rest.iter_mut().map(InputState::latch_tick)).collect()
    }
}

#[cfg(test)]
mod tests {
    use olr_input::Button;

    use super::*;

    /// Issue #29: among several players each seat hears its own keys
    /// only (keyboard 1 the arrows, keyboard 2 the keypad, each pad its
    /// own), so one player's keys never reach another's; alone, the
    /// player hears keyboard 1 and every pad as before seats.
    #[test]
    fn each_player_hears_its_own_seat() {
        let mut inputs = LocalInputs::default();
        inputs.on_pad(1, PadEvent::Button(Button::DPadRight, 1.0));
        let first = inputs.first_mut().latch_tick();
        assert_eq!(inputs.latch(first), [Keys(Keys::RIGHT)], "alone: every pad");
        inputs.fit(4);
        assert_eq!(inputs.players(), 4);
        inputs.on_key(KeyCode::ArrowLeft, true);
        inputs.on_key(KeyCode::Numpad6, true);
        inputs.on_pad(0, PadEvent::Button(Button::DPadUp, 1.0));
        inputs.on_pad(1, PadEvent::Button(Button::DPadDown, 1.0));
        inputs.on_pad(7, PadEvent::Button(Button::DPadLeft, 1.0));
        let first = inputs.first_mut().latch_tick();
        let keys = inputs.latch(first);
        assert_eq!(keys, [Keys(Keys::LEFT), Keys(Keys::RIGHT), Keys(Keys::UP), Keys(Keys::DOWN)]);
        assert_eq!(inputs.held(), keys);
        inputs.release_keys();
        assert_eq!(inputs.held()[..2], [Keys(0), Keys(0)]);
        // Back to one: the solo player again, nothing held.
        inputs.fit(1);
        assert_eq!((inputs.players(), inputs.held()), (1, vec![Keys(0)]));
        assert!(inputs.first().seat().is_none());
    }

    /// A pad's Start on any seat pauses (issue #28), taken once.
    #[test]
    fn any_seats_start_pauses() {
        let mut inputs = LocalInputs::default();
        inputs.fit(3);
        inputs.on_pad(0, PadEvent::Button(Button::Start, 1.0));
        assert!(inputs.take_pause());
        assert!(!inputs.take_pause());
    }
}
