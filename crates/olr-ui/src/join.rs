//! The join step of local play (issue #32; not OLX's: its local players
//! are set up in the player menu): 1 to [`MAX_LOCAL_PLAYERS`] players
//! join by pressing a button on their own seat (keyboard 1, keyboard 2,
//! a gamepad), each picks a profile and readies; bots fill the rest of
//! the match as before. Each seat's own keys drive its own card, so one
//! player's input never reaches another's choices.
//!
//! On a seat that has not joined, any bound input pressed (or a pad's
//! Start) joins it. Joined: Left and Right step through the profiles no
//! other player has (then "Player N", no profile); Fire, Jump or Start
//! readies (again: not ready); Rope or Dig leaves (a ready player is first
//! not ready). The match starts when every joined player is ready.
//!
//! Pure logic: the screen (`screens/join.rs`) draws it, the app gives
//! each seat's input ([`SeatInput`], every frame).

use olr_input::{Action, Seat};

use crate::state::UiAction;

/// The most local players: one view each (split screen, issue #30).
pub const MAX_LOCAL_PLAYERS: usize = crate::hud::MAX_VIEWS;

/// A player who joined: their seat, their profile (an index of the
/// profiles' list; `None`: "Player N" in the default look), ready or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Joined {
    pub seat: Seat,
    pub profile: Option<usize>,
    pub ready: bool,
}

/// One seat's input now, as the app reads it every frame: its actions
/// held (the mask of `Action::bit`, Dig's `DIG` included:
/// `InputState::actions_held`) and whether its pad's Start is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeatInput {
    pub seat: Seat,
    pub held: u32,
    pub start: bool,
}

/// Where a seat's player stands among the local players: keyboard 1
/// first (so it is local player 1, whose view the mouse aims in), then
/// keyboard 2, then the pads in the order they joined.
pub fn seat_rank(seat: Seat) -> u8 {
    match seat {
        Seat::Keyboard1 => 0,
        Seat::Keyboard2 => 1,
        Seat::Pad(_) => 2,
    }
}

/// The name the menus give a seat: "Keyboard 1", "Keyboard 2",
/// "Gamepad N" (gilrs's id plus one).
pub fn seat_name(seat: Seat) -> String {
    match seat {
        Seat::Keyboard1 => "Keyboard 1".into(),
        Seat::Keyboard2 => "Keyboard 2".into(),
        Seat::Pad(id) => format!("Gamepad {}", id + 1),
    }
}

/// What the seats ask of the join step, from one frame to the next:
/// presses are edges of [`SeatInput`]s.
#[derive(Clone, Debug, Default)]
pub struct JoinStep {
    /// Each seat's input at the last call.
    last: Vec<SeatInput>,
    /// Called before: the first call only notes what is held (the press
    /// that opened the step is not a second one).
    primed: bool,
}

impl JoinStep {
    /// The actions `seats`' presses since the last call ask for, given
    /// the players joined now (`lineup`): at most one a seat. A joined
    /// pad no longer among `seats` (unplugged) leaves.
    pub fn on_seats(&mut self, lineup: &[Joined], seats: &[SeatInput]) -> Vec<UiAction> {
        let mut out = Vec::new();
        for s in seats {
            let last = self.last.iter().find(|l| l.seat == s.seat);
            let (held, start) = match (self.primed, last) {
                (false, _) => (s.held, s.start),
                (true, Some(l)) => (l.held, l.start),
                (true, None) => (0, false),
            };
            let pressed = s.held & !held;
            let start = s.start && !start;
            if pressed == 0 && !start {
                continue;
            }
            let Some(j) = lineup.iter().find(|j| j.seat == s.seat) else {
                out.push(UiAction::JoinSeat(s.seat));
                continue;
            };
            let seat = s.seat;
            if start || pressed & (Action::Shoot.bit() | Action::Jump.bit()) != 0 {
                out.push(UiAction::SetReady { seat, ready: !j.ready });
            } else if pressed & (Action::Rope.bit() | Action::Dig.bit()) != 0 {
                out.push(if j.ready {
                    UiAction::SetReady { seat, ready: false }
                } else {
                    UiAction::LeaveSeat(seat)
                });
            } else if !j.ready {
                let by = i32::from(pressed & Action::Right.bit() != 0)
                    - i32::from(pressed & Action::Left.bit() != 0);
                if by != 0 {
                    out.push(UiAction::StepProfile { seat, by });
                }
            }
        }
        for j in lineup {
            if matches!(j.seat, Seat::Pad(_)) && !seats.iter().any(|s| s.seat == j.seat) {
                out.push(UiAction::LeaveSeat(j.seat));
            }
        }
        self.last = seats.to_vec();
        self.primed = true;
        out
    }
}
