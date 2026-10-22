//! Input seats (issue #28): the devices one local player plays from, so
//! several can play from one machine. Keyboard 1 (its keys and the mouse),
//! keyboard 2 on the same keyboard (OLX's player 2's keys), and one seat
//! per gamepad, by gilrs pad id. Each seat has its own state, latch and
//! Dig; the bindings belong to the seat's kind ([`SeatKind`]: every pad's
//! seat shares the gamepads').
//!
//! OLX's fork plays up to four local players, each from its own controls
//! (presentation note §1.1, §10.1; `CClient.cpp:1527-1551`).

use olr_sim::Keys;
use winit::event::WindowEvent;
use winit::keyboard::KeyCode;

use crate::bindings::{Action, Bindings};
use crate::rebind::{Device, Mode, Need};
use crate::state::{InputState, PadEvent};

/// A seat: the devices one local player plays from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Seat {
    /// Keyboard 1's keys ([`Bindings::keys`]) and the mouse (mouse
    /// controls, issue #24).
    Keyboard1,
    /// Keyboard 2's keys ([`Bindings::keys2`]) on the same keyboard.
    Keyboard2,
    /// One gamepad, by gilrs id (as `usize`), through [`Bindings::pad`].
    Pad(usize),
}

impl Seat {
    /// Whose bindings it plays with.
    pub fn kind(self) -> SeatKind {
        match self {
            Seat::Keyboard1 => SeatKind::Keyboard1,
            Seat::Keyboard2 => SeatKind::Keyboard2,
            Seat::Pad(_) => SeatKind::Gamepad,
        }
    }
}

/// Whose bindings: a seat's, as the options' Keys tab chooses them
/// (Keyboard 1, Keyboard 2, Gamepads; every pad's seat shares the
/// gamepads'). Each keeps every action reachable with its own device's
/// inputs alone ([`Bindings::unreachable_of`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SeatKind {
    #[default]
    Keyboard1,
    Keyboard2,
    Gamepad,
}

impl SeatKind {
    pub const ALL: [SeatKind; 3] = [SeatKind::Keyboard1, SeatKind::Keyboard2, SeatKind::Gamepad];

    /// The name the options show.
    pub fn name(self) -> &'static str {
        match self {
            SeatKind::Keyboard1 => "Keyboard 1",
            SeatKind::Keyboard2 => "Keyboard 2",
            SeatKind::Gamepad => "Gamepads",
        }
    }

    /// The [`Mode`] its bindings are edited in: keyboard 1's as mouse
    /// controls are on or off, keyboard 2's own, the gamepads' in
    /// [`Mode::Keys`] (their list is the same in either).
    pub fn mode(self, mouse_controls: bool) -> Mode {
        match self {
            SeatKind::Keyboard1 => Mode::of(mouse_controls),
            SeatKind::Keyboard2 => Mode::Keyboard2,
            SeatKind::Gamepad => Mode::Keys,
        }
    }

    /// The devices its bindings are of, as the options' columns: keyboard
    /// 1's keyboard (and with mouse controls on the mouse), keyboard 2's
    /// keyboard, the gamepads'.
    pub fn devices(self, mouse_controls: bool) -> &'static [Device] {
        match self {
            SeatKind::Keyboard1 if mouse_controls => &[Device::Keyboard, Device::Mouse],
            SeatKind::Keyboard1 | SeatKind::Keyboard2 => &[Device::Keyboard],
            SeatKind::Gamepad => &[Device::Gamepad],
        }
    }

    /// What its every action keeps at least one of ([`Need`]).
    pub fn needs(self) -> &'static [Need] {
        match self {
            SeatKind::Keyboard1 => &[Need::Key, Need::KeyOrMouse],
            SeatKind::Keyboard2 => &[Need::Keyboard2Key],
            SeatKind::Gamepad => &[Need::PadInput],
        }
    }
}

impl Bindings {
    /// [`Bindings::unreachable`] of one seat: the gaps its own device's
    /// inputs leave, whatever the other seats' (issue #28).
    pub fn unreachable_of(&self, seat: SeatKind) -> Vec<(Action, Need)> {
        self.unreachable().into_iter().filter(|(_, n)| seat.needs().contains(n)).collect()
    }
}

/// Every seat's input: both keyboard seats, which share the keyboard's
/// events (each hears its own keys), and a seat per pad, made at its
/// first event and gone when it disconnects. Feed it every window and
/// gamepad event, then latch each seat's keys once per tick.
#[derive(Clone, Debug)]
pub struct Seats {
    bindings: Bindings,
    keyboard1: InputState,
    keyboard2: InputState,
    /// Each pad's seat, in the order first heard.
    pads: Vec<(usize, InputState)>,
}

impl Default for Seats {
    fn default() -> Self {
        Seats::new(Bindings::default())
    }
}

impl Seats {
    pub fn new(bindings: Bindings) -> Seats {
        Seats {
            keyboard1: InputState::for_seat(Seat::Keyboard1, bindings.clone()),
            keyboard2: InputState::for_seat(Seat::Keyboard2, bindings.clone()),
            pads: Vec::new(),
            bindings,
        }
    }

    pub fn bindings(&self) -> &Bindings {
        &self.bindings
    }

    /// Replaces every seat's bindings; everything held is released.
    pub fn set_bindings(&mut self, bindings: Bindings) {
        for s in self.states_mut() {
            s.set_bindings(bindings.clone());
        }
        self.bindings = bindings;
    }

    /// Keyboard 1's mouse controls ([`InputState::set_mouse_controls`]);
    /// no other seat has them.
    pub fn set_mouse_controls(&mut self, on: bool) {
        self.keyboard1.set_mouse_controls(on);
    }

    /// Every seat: the keyboards, then each pad in the order first heard.
    pub fn seats(&self) -> Vec<Seat> {
        let pads = self.pads.iter().map(|(p, _)| Seat::Pad(*p));
        [Seat::Keyboard1, Seat::Keyboard2].into_iter().chain(pads).collect()
    }

    /// A seat's state (`None`: a pad not heard, or gone).
    pub fn get(&self, seat: Seat) -> Option<&InputState> {
        match seat {
            Seat::Keyboard1 => Some(&self.keyboard1),
            Seat::Keyboard2 => Some(&self.keyboard2),
            Seat::Pad(id) => self.pads.iter().find(|(p, _)| *p == id).map(|(_, s)| s),
        }
    }

    pub fn get_mut(&mut self, seat: Seat) -> Option<&mut InputState> {
        match seat {
            Seat::Keyboard1 => Some(&mut self.keyboard1),
            Seat::Keyboard2 => Some(&mut self.keyboard2),
            Seat::Pad(id) => self.pads.iter_mut().find(|(p, _)| *p == id).map(|(_, s)| s),
        }
    }

    fn states_mut(&mut self) -> impl Iterator<Item = &mut InputState> {
        [&mut self.keyboard1, &mut self.keyboard2].into_iter().chain(self.pads.iter_mut().map(|(_, s)| s))
    }

    /// A winit window event, to both keyboard seats
    /// ([`InputState::on_window_event`]; the mouse's go to keyboard 1's:
    /// `get_mut(Seat::Keyboard1)`, [`InputState::on_mouse`]).
    pub fn on_window_event(&mut self, ev: &WindowEvent) {
        self.keyboard1.on_window_event(ev);
        self.keyboard2.on_window_event(ev);
    }

    /// Physical key `code` went down or up: each keyboard seat hears its own.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        self.keyboard1.on_key(code, down);
        self.keyboard2.on_key(code, down);
    }

    /// Releases both keyboard seats' keys.
    pub fn release_keys(&mut self) {
        self.keyboard1.release_keys();
        self.keyboard2.release_keys();
    }

    /// A gilrs event, to its pad's seat.
    pub fn on_gamepad(&mut self, ev: &gilrs::Event) {
        if let Some(p) = PadEvent::from_gilrs(&ev.event) {
            self.on_pad(usize::from(ev.id), p);
        }
    }

    /// A gamepad event from pad `pad`: to its seat, made at its first
    /// event; a disconnected pad's seat is gone (everything it held
    /// released).
    pub fn on_pad(&mut self, pad: usize, ev: PadEvent) {
        let at = self.pads.iter().position(|(p, _)| *p == pad);
        match (ev, at) {
            (PadEvent::Disconnected, Some(i)) => {
                self.pads.remove(i);
            }
            (PadEvent::Disconnected, None) => {}
            (ev, Some(i)) => self.pads[i].1.on_pad(pad, ev),
            (ev, None) => {
                let mut s = InputState::for_seat(Seat::Pad(pad), self.bindings.clone());
                s.on_pad(pad, ev);
                self.pads.push((pad, s));
            }
        }
    }

    /// A seat's keys held now ([`InputState::held`]; none for a pad not heard).
    pub fn held(&self, seat: Seat) -> Keys {
        self.get(seat).map_or(Keys(0), InputState::held)
    }

    /// A seat's keys for the next tick ([`InputState::latch_tick`]; none
    /// for a pad not heard).
    pub fn latch_tick(&mut self, seat: Seat) -> Keys {
        self.get_mut(seat).map_or(Keys(0), InputState::latch_tick)
    }

    /// Any pad's Start was pressed since the last call
    /// ([`InputState::take_pause`]).
    pub fn take_pause(&mut self) -> bool {
        self.pads.iter_mut().fold(false, |any, (_, s)| s.take_pause() | any)
    }
}
