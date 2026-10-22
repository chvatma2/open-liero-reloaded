//! The local players' inputs (issue #29): one `olr_input::InputState`
//! per seat. A player alone plays as before seats (`InputState::new`:
//! keyboard 1's keys, the mouse and every gamepad). Several play on the
//! seats local play's join step gave them (issue #32: [`LocalInputs::fit_to`]
//! with the lineup's seats, keyboard 1 first), or, for the command line's
//! `--locals N`, player `k` on [`SEATS`]`[k]` (keyboard 1 and the mouse,
//! keyboard 2, the first two gamepads). Every event reaches every seat;
//! each hears only its own devices, so one player's keys never reach
//! another's worm.
//!
//! The menus' seats ride along ([`LocalInputs::menu_seats`]): keyboard 1,
//! keyboard 2 and every pad heard, each with its own state, for the join
//! step. A joined pad unplugged leaves its player standing still (its
//! state releases everything); a pad plugged in again takes the seat back:
//! the same gilrs id, else the pad of the same gilrs UUID as it connects
//! ([`LocalInputs::pad_seen`]), else the first pad no player has that
//! presses a button (that press is the claim, and presses nothing).
//!
//! OLX's fork plays up to four local players, each from its own controls
//! (`docs/porting/olx-notes/presentation.md` §1.1, §10.1).
//!
//! OLX: src/client/CClient.cpp:1527-1551

use olr_input::{Bindings, InputState, KeyCode, PadEvent, Seat, Seats};
use olr_sim::Keys;
use olr_ui::join::SeatInput;
use winit::event::WindowEvent;

use crate::setup::MAX_LOCALS;

/// Each local player's seat among several, in local order, for the
/// command line's `--locals N` (the menus seat players by the join step).
pub const SEATS: [Seat; MAX_LOCALS] = [Seat::Keyboard1, Seat::Keyboard2, Seat::Pad(0), Seat::Pad(1)];

/// The local players' inputs: player 0's (with mouse controls, when on
/// and on keyboard 1) and the others', one per seat; and the menus' seats.
#[derive(Clone, Debug)]
pub struct LocalInputs {
    bindings: Bindings,
    mouse: bool,
    /// Local player 0's: the solo player's alone, its seat's among
    /// several.
    first: InputState,
    /// Local players 1 and up.
    rest: Vec<InputState>,
    /// The seats the join step gave (`fit_to`), in local order; empty:
    /// one player, or `SEATS`' (`fit`).
    seats: Vec<Seat>,
    /// The players whose pad was unplugged: (local number, its pad).
    orphans: Vec<(usize, usize)>,
    /// Each pad's gilrs UUID, by gilrs id, as last heard (kept after it
    /// is unplugged: the same pad comes back with it).
    uuids: Vec<(usize, [u8; 16])>,
    /// Seats changed by a pad plugged in again (`take_reseated`).
    reseated: Vec<(Seat, Seat)>,
    /// Every seat's input for the menus (the join step, issue #32).
    menus: Seats,
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
        let mut menus = Seats::new(bindings.clone());
        menus.set_mouse_controls(mouse);
        LocalInputs {
            bindings,
            mouse,
            first,
            rest: Vec::new(),
            seats: Vec::new(),
            orphans: Vec::new(),
            uuids: Vec::new(),
            reseated: Vec::new(),
            menus,
        }
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
        if n == self.players() && self.seats.is_empty() {
            return;
        }
        let seats = if n == 1 { Vec::new() } else { SEATS[..n].to_vec() };
        self.make(&seats);
        self.seats.clear();
    }

    /// Inputs for `n` local players on `seats` (the join step's lineup,
    /// issue #32: one seat a player, in local order); without that many
    /// seats, [`LocalInputs::fit`]. Unchanged when it holds those.
    pub fn fit_to(&mut self, n: usize, seats: &[Seat]) {
        if seats.is_empty() || seats.len() != n.clamp(1, MAX_LOCALS) {
            self.fit(n);
            return;
        }
        if self.seats == seats {
            return;
        }
        self.make(seats);
        self.seats = seats.to_vec();
    }

    /// The inputs made afresh: one per seat of `seats`, or the solo
    /// player's for none.
    fn make(&mut self, seats: &[Seat]) {
        let seat = |s: Seat| InputState::for_seat(s, self.bindings.clone());
        self.first = match seats.first() {
            None => InputState::new(self.bindings.clone()),
            Some(&s) => seat(s),
        };
        self.first.set_mouse_controls(self.mouse);
        self.rest = seats.iter().skip(1).map(|&s| seat(s)).collect();
        self.orphans.clear();
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

    fn get_mut(&mut self, k: usize) -> Option<&mut InputState> {
        if k == 0 { Some(&mut self.first) } else { self.rest.get_mut(k - 1) }
    }

    fn each_mut(&mut self) -> impl Iterator<Item = &mut InputState> {
        std::iter::once(&mut self.first).chain(&mut self.rest)
    }

    /// Every seat's bindings (everything held released).
    pub fn set_bindings(&mut self, bindings: Bindings) {
        for i in self.each_mut() {
            i.set_bindings(bindings.clone());
        }
        self.menus.set_bindings(bindings.clone());
        self.bindings = bindings;
    }

    /// Mouse controls on or off: player 0's (only keyboard 1 has them).
    pub fn set_mouse_controls(&mut self, on: bool) {
        self.mouse = on;
        self.first.set_mouse_controls(on);
        self.menus.set_mouse_controls(on);
    }

    /// A window event, to every seat (each hears its own keys).
    pub fn on_window_event(&mut self, ev: &WindowEvent) {
        for i in self.each_mut() {
            i.on_window_event(ev);
        }
        self.menus.on_window_event(ev);
    }

    /// A key, to every seat.
    pub fn on_key(&mut self, code: KeyCode, down: bool) {
        for i in self.each_mut() {
            i.on_key(code, down);
        }
        self.menus.on_key(code, down);
    }

    /// A gilrs event, to every seat (each hears its own pad).
    pub fn on_gamepad(&mut self, ev: &gilrs::Event) {
        if let Some(p) = PadEvent::from_gilrs(&ev.event) {
            self.on_pad(usize::from(ev.id), p);
        }
    }

    /// Pad `pad`'s event, to every seat (tests: gilrs's events cannot be
    /// made outside gilrs). A joined pad unplugged leaves its player
    /// waiting for it; a button pressed on a pad no player has gives it
    /// the first such player's seat (the press presses nothing).
    pub fn on_pad(&mut self, pad: usize, ev: PadEvent) {
        self.menus.on_pad(pad, ev);
        if !self.seats.is_empty() {
            let mine = self.seats.iter().position(|s| *s == Seat::Pad(pad));
            match (ev, mine) {
                (PadEvent::Disconnected, Some(k)) => {
                    if !self.orphans.iter().any(|o| o.0 == k) {
                        self.orphans.push((k, pad));
                    }
                }
                (PadEvent::Button(_, v), None) if v >= 0.5 && !self.orphans.is_empty() => {
                    let (k, _) = self.orphans[0];
                    self.reseat(k, pad);
                    return;
                }
                (_, Some(k)) => self.orphans.retain(|o| o.0 != k),
                _ => {}
            }
        }
        for i in self.each_mut() {
            i.on_pad(pad, ev);
        }
    }

    /// Pad `pad` (a gilrs id) has gilrs UUID `uuid` (the app tells it of
    /// every pad it hears, at its connection first): an unplugged
    /// player's pad of that UUID back under another id takes its seat
    /// back.
    pub fn pad_seen(&mut self, pad: usize, uuid: [u8; 16]) {
        match self.uuids.iter_mut().find(|(p, _)| *p == pad) {
            Some(e) => e.1 = uuid,
            None => self.uuids.push((pad, uuid)),
        }
        if self.seats.contains(&Seat::Pad(pad)) || uuid == [0; 16] {
            return;
        }
        let back = self
            .orphans
            .iter()
            .find(|&&(_, old)| old != pad && self.uuids.iter().any(|&(p, u)| p == old && u == uuid));
        if let Some(&(k, _)) = back {
            self.reseat(k, pad);
        }
    }

    /// Local player `k` (whose pad was unplugged) plays from pad `pad`
    /// now: a fresh input, nothing held.
    fn reseat(&mut self, k: usize, pad: usize) {
        let to = Seat::Pad(pad);
        let input = InputState::for_seat(to, self.bindings.clone());
        let Some(i) = self.get_mut(k) else { return };
        *i = input;
        let from = std::mem::replace(&mut self.seats[k], to);
        self.orphans.retain(|o| o.0 != k);
        self.reseated.push((from, to));
    }

    /// The seats changed by pads plugged in again since the last call
    /// (`from`, `to`): the menus' lineup follows (`UiAction::Reseat`).
    pub fn take_reseated(&mut self) -> Vec<(Seat, Seat)> {
        std::mem::take(&mut self.reseated)
    }

    /// The players waiting for their unplugged pad, by local number.
    pub fn unplugged(&self) -> Vec<usize> {
        self.orphans.iter().map(|o| o.0).collect()
    }

    /// Every seat's keys released.
    pub fn release_keys(&mut self) {
        for i in self.each_mut() {
            i.release_keys();
        }
        self.menus.release_keys();
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

    /// Every seat's input now for the menus (issue #32's join step,
    /// `MenuData::set_seats`): keyboard 1, keyboard 2 and each pad heard,
    /// its actions held (Dig included) and its Start. The menus' own pad
    /// pauses are dropped (a pad's Start in game pauses through the
    /// players' seats).
    pub fn menu_seats(&mut self) -> Vec<SeatInput> {
        let seats = self.menus.seats();
        self.menus.take_pause();
        seats
            .into_iter()
            .filter_map(|seat| {
                let s = self.menus.get(seat)?;
                Some(SeatInput { seat, held: s.actions_held(), start: s.start_held() })
            })
            .collect()
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

    /// Issue #32: the join step's seats, keyboard 1 first; each hears its
    /// own, unchanged when given again; without a lineup, `fit`'s.
    #[test]
    fn the_join_steps_seats_are_each_players() {
        let mut inputs = LocalInputs::new(Bindings::default(), true);
        let seats = [Seat::Pad(3), Seat::Keyboard2];
        inputs.fit_to(2, &seats);
        assert_eq!(inputs.get(0).unwrap().seat(), Some(Seat::Pad(3)));
        assert!(!inputs.first().mouse_controls(), "the mouse is keyboard 1's alone");
        inputs.on_pad(3, PadEvent::Button(Button::DPadLeft, 1.0));
        inputs.on_pad(0, PadEvent::Button(Button::DPadUp, 1.0));
        inputs.on_key(KeyCode::Numpad6, true);
        inputs.on_key(KeyCode::ArrowDown, true);
        assert_eq!(inputs.held(), [Keys(Keys::LEFT), Keys(Keys::RIGHT)]);
        inputs.fit_to(2, &seats);
        assert_eq!(inputs.held(), [Keys(Keys::LEFT), Keys(Keys::RIGHT)], "the same seats: kept");
        inputs.fit_to(2, &[Seat::Keyboard1, Seat::Pad(3)]);
        assert!(inputs.first().mouse_controls());
        inputs.fit_to(2, &[]);
        assert_eq!(inputs.get(1).unwrap().seat(), Some(Seat::Keyboard2), "SEATS' without a lineup");
        // The menus hear every seat, Dig and Start too.
        let mut inputs = LocalInputs::default();
        inputs.on_key(KeyCode::Numpad3, true);
        inputs.on_pad(2, PadEvent::Button(Button::Start, 1.0));
        let menus = inputs.menu_seats();
        let of = |s: Seat| *menus.iter().find(|i| i.seat == s).unwrap();
        assert_eq!(of(Seat::Keyboard2).held, olr_input::DIG);
        assert!(of(Seat::Pad(2)).start);
        assert_eq!(of(Seat::Keyboard1).held, 0);
    }

    /// Issue #32: a joined pad unplugged leaves its worm standing still;
    /// plugged in again under another id it takes the seat back, by its
    /// UUID as it connects, else at the first button of a pad no player
    /// has (that press presses nothing).
    #[test]
    fn an_unplugged_pad_comes_back_to_its_seat() {
        let mut inputs = LocalInputs::default();
        inputs.fit_to(3, &[Seat::Keyboard1, Seat::Pad(0), Seat::Pad(1)]);
        inputs.pad_seen(0, [7; 16]);
        inputs.pad_seen(1, [8; 16]);
        inputs.on_pad(0, PadEvent::Button(Button::DPadRight, 1.0));
        assert_eq!(inputs.held()[1], Keys(Keys::RIGHT));
        inputs.on_pad(0, PadEvent::Disconnected);
        assert_eq!(inputs.held()[1], Keys(0), "stands still");
        assert_eq!(inputs.unplugged(), [1]);
        // Another pad (no UUID match) connects: nothing yet.
        inputs.pad_seen(4, [9; 16]);
        assert_eq!(inputs.unplugged(), [1]);
        // Pad 0 is back as pad 5, with its UUID: its seat again.
        inputs.pad_seen(5, [7; 16]);
        assert_eq!(inputs.take_reseated(), [(Seat::Pad(0), Seat::Pad(5))]);
        assert_eq!(inputs.get(1).unwrap().seat(), Some(Seat::Pad(5)));
        inputs.on_pad(5, PadEvent::Button(Button::DPadLeft, 1.0));
        assert_eq!(inputs.held()[1], Keys(Keys::LEFT));
        // Pad 1 unplugged; a pad no player has presses A: it is player 3's
        // pad now, the press its claim.
        inputs.on_pad(1, PadEvent::Disconnected);
        inputs.on_pad(4, PadEvent::Axis(olr_input::Axis::LeftStickX, 0.9));
        assert_eq!(inputs.unplugged(), [2], "a stick is not a button");
        inputs.on_pad(4, PadEvent::Button(Button::South, 1.0));
        assert_eq!(inputs.take_reseated(), [(Seat::Pad(1), Seat::Pad(4))]);
        assert_eq!(inputs.held()[2], Keys(0), "the claim presses nothing");
        inputs.on_pad(4, PadEvent::Button(Button::South, 0.0));
        inputs.on_pad(4, PadEvent::Button(Button::South, 1.0));
        assert_eq!(inputs.held()[2], Keys(Keys::JUMP));
        // Nobody waits: another pad's button is nobody's.
        inputs.on_pad(9, PadEvent::Button(Button::South, 1.0));
        assert!(inputs.take_reseated().is_empty());
        assert_eq!(inputs.held(), [Keys(0), Keys(Keys::LEFT), Keys(Keys::JUMP)]);
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
