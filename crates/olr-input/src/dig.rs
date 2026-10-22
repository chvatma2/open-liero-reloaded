//! The Dig action (issue #23): OLX has no dig key (`Options.cpp:44`,
//! `ply_keys`); LX56 digs when a player holding one direction presses the
//! other (`CWormHuman.cpp:465-503`: `rightOnce` while Left is held, and
//! the reverse). Dig presses that gesture itself, so it digs exactly as
//! the keys do and needs nothing from the simulation:
//!
//! - one direction held: the other, for one tick;
//! - none: both, for one tick (the fastest a player can press the gesture:
//!   the worm digs along its move direction, `physics.rs`);
//! - both held, or SelectWeapon held (Left and Right change the weapon
//!   then): nothing, as the gesture cannot dig then;
//! - a key it must press that the last tick saw down (just released): the
//!   next tick, so the simulation sees a press.
//!
//! A press digs once; held, Dig digs again every [`DIG_REPEAT_TICKS`],
//! OLX's own auto-dig rate for joystick, mouse and touch players, slower
//! than a player tapping as fast as a tick allows.

use olr_sim::Keys;

use crate::bindings::DIG;

/// Held, Dig digs every 20 ticks (0.2 s at 100 Hz).
/// OLX: src/common/CWormHuman.cpp:357-394 (`carveDelay = 0.2f`)
pub const DIG_REPEAT_TICKS: u32 = 20;

const SIDES: u32 = (Keys::LEFT | Keys::RIGHT) as u32;

/// The Dig action's state between ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dig {
    /// Ticks until a held Dig digs again; 0 while it is up.
    wait: u32,
    /// A dig waits for a tick that can press it.
    due: bool,
}

impl Dig {
    /// One tick: the actions' bits as latched (with the [`DIG`] bit),
    /// `last` what the last tick saw. Returns the simulation's keys'
    /// bits and the bits the dig
    /// pressed, which the latch must release next tick
    /// ([`crate::TickLatch::tap`]).
    pub(crate) fn tick(&mut self, keys: u32, last: u32) -> (u32, u32) {
        let held = keys & DIG != 0;
        let keys = keys & !DIG;
        if held {
            if self.wait == 0 {
                self.due = true;
                self.wait = DIG_REPEAT_TICKS;
            }
            self.wait -= 1;
        } else {
            self.wait = 0;
        }
        if !self.due {
            return (keys, 0);
        }
        let sides = keys & SIDES;
        if keys & u32::from(Keys::CHANGE) != 0 || sides == SIDES {
            self.due = false;
            return (keys, 0);
        }
        let press = SIDES & !sides;
        if press & last != 0 {
            return (keys, 0);
        }
        self.due = false;
        (keys | press, press)
    }
}
