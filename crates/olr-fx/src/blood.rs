//! The player's blood amount (issue #6; OLX's `Game.Blood`,
//! `tLXOptions->iBloodAmount`): how many of the simulation's blood
//! cosmetics become entities. The simulation always makes OLX's 100 %
//! draws (`olr-sim`'s `BLOOD_AMOUNT`), so the amount changes only what this
//! player sees, never the world (a deliberate deviation,
//! `docs/porting/STATUS.md`).
//!
//! With `s` the amount (0-5): blood events become `s` times as many,
//! droppers `min(s, 1)` times, a dropper's drops `max(s, 1)` times (none
//! at 0 %), so bleeding shows `s` times the blood like hits and deaths.
//! Fewer are spread evenly by an accumulator (50 % shows every other one);
//! more are copies with velocities jittered by this module's own
//! generator, never the simulation's nor the other entities'.
//!
//! OLX: src/client/CClient_Game.cpp:335-460 (`InjureWorm`'s rounds of blood
//! by `iBloodAmount`), src/common/PhysicsLX56.cpp:317-330 (bleeding)

use olr_math::{GlibcRand, Vec2};

/// The blood amount OLX starts with, in percent: what the simulation
/// draws, the render goldens and the oracle's scenarios show.
pub const BLOOD_DEFAULT: u16 = 100;

/// The largest blood amount, in percent (the issue's range: five times
/// OLX's default).
pub const BLOOD_MAX: u16 = 500;

/// The least spread of a copy's velocity, in pixels per second: copies of
/// a still drop still part a little.
const MIN_SPREAD: f32 = 5.0;

/// Whole entities from a percentage, rounded to the nearest over a run:
/// it starts at a half, so 50 % shows the first of each two.
#[derive(Clone, Copy, Debug)]
struct Spread(u32);

impl Spread {
    const START: Spread = Spread(50);

    /// How many entities one cosmetic becomes at `percent`.
    fn take(&mut self, percent: u32) -> u32 {
        self.0 += percent;
        let n = self.0 / 100;
        self.0 %= 100;
        n
    }
}

/// The amount, its accumulators and the copies' generator.
#[derive(Clone, Debug)]
pub(crate) struct Blood {
    percent: u32,
    events: Spread,
    droppers: Spread,
    drops: Spread,
    rand: GlibcRand,
}

impl Blood {
    /// [`BLOOD_DEFAULT`], the copies' generator seeded from `seed`.
    pub fn new(seed: u32) -> Blood {
        Blood {
            percent: u32::from(BLOOD_DEFAULT),
            events: Spread::START,
            droppers: Spread::START,
            drops: Spread::START,
            rand: GlibcRand::new(seed ^ 0xb100_d000),
        }
    }

    pub fn percent(&self) -> u16 {
        self.percent as u16
    }

    /// Sets the amount, at most [`BLOOD_MAX`].
    pub fn set_percent(&mut self, percent: u16) {
        self.percent = u32::from(percent.min(BLOOD_MAX));
    }

    /// How many entities one `CosmeticKind::Blood` event becomes.
    pub fn events(&mut self) -> u32 {
        self.events.take(self.percent)
    }

    /// How many entities one `CosmeticKind::BloodDropper` event becomes:
    /// never more than one.
    pub fn droppers(&mut self) -> u32 {
        self.droppers.take(self.percent.min(100))
    }

    /// How many drops one drip of a dropper makes: at least one (the
    /// droppers are already fewer), but none at 0 %.
    pub fn drops(&mut self) -> u32 {
        match self.percent {
            0 => 0,
            p => self.drops.take(p.max(100)),
        }
    }

    /// A copy's velocity: `vel` and a random part of half its speed (at
    /// least [`MIN_SPREAD`]).
    pub fn jitter(&mut self, vel: Vec2) -> Vec2 {
        vel + self.rand.random_vec() * (vel.length() * 0.5).max(MIN_SPREAD)
    }
}
