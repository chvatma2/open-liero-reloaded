//! `AI_FindShootingSpot` (`src/common/CWormBot.cpp:3951-4034`): where to
//! go to shoot a worm. Its circle of up to 21 points costs a few thousand
//! work units, so it is searched over ticks as the budget allows and kept
//! for 100 ms; until a search ends the last spot for that worm is used.

use rl_math::{AbsTime, Vec2};
use rl_sim::Worm;

use crate::bot::{Bot, Ctx};
use crate::budget::Budget;
use crate::nav::worm_line_clear;
use crate::probe::{in_air, trace_line};

/// How long a shooting spot is kept.
const SPOT_MS: u64 = 100;
/// OLX's step around the circle, in radians.
const STEP: f32 = 0.3;

/// Where the search is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// The circle, at angle `j`.
    Circle(f32),
    /// Stepping right, then left, from the target (`i` of 10 steps, `x`).
    Right(u32, f32),
    Left(u32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Job {
    worm: i32,
    target: Vec2,
    upper: f32,
    phase: Phase,
}

/// The shooting spot search and the last spot found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SpotFinder {
    job: Option<Job>,
    /// For which worm, where, and when.
    found: Option<(i32, Vec2, AbsTime)>,
}

/// One step of the search: a spot, no spot here, or not this tick.
enum Try {
    Spot(Vec2),
    Next,
    Later,
}

impl Bot {
    /// The spot to shoot worm `w` from, as far as the search has got.
    pub(crate) fn shooting_spot(&mut self, c: &Ctx, b: &mut Budget, w: &Worm) -> Vec2 {
        let last = self.spot.found.filter(|(id, _, _)| *id == w.id);
        if let Some((_, spot, at)) = last
            && c.now.0.saturating_sub(at.0) < SPOT_MS
        {
            return spot;
        }
        if self.spot.job.is_none_or(|j| j.worm != w.id) {
            self.spot.job = Some(self.new_job(c, w));
        }
        let Some(mut job) = self.spot.job else { return w.pos };
        loop {
            match self.try_spot(c, b, &mut job) {
                Try::Spot(p) => {
                    self.spot.job = None;
                    self.spot.found = Some((w.id, p, c.now));
                    return p;
                }
                Try::Next => {}
                Try::Later => {
                    self.spot.job = Some(job);
                    return last.map_or(w.pos, |(_, p, _)| p);
                }
            }
        }
    }

    /// The half circle the bot's weapons call for: falling projectiles
    /// (gravity ≥ 5), or straight ones at a worm on the ground, from above;
    /// straight ones all round; rising ones from below.
    fn new_job(&self, c: &Ctx, w: &Worm) -> Job {
        let (mut falling, mut straight) = (false, false);
        for s in &c.me.slots {
            if let Some(f) = self.arsenal.get(s.weapon).filter(|f| f.projectile()) {
                let g = f.custom_gravity.map_or(100.0, |g| g as f32) * c.s.proj_gravity_factor;
                if g >= 5.0 {
                    falling = true;
                } else if g >= -5.0 {
                    straight = true;
                }
            }
        }
        let pi = core::f32::consts::PI;
        let (lower, upper) = if falling || (straight && w.on_ground) {
            (pi / 2.0, 3.0 * pi / 2.0)
        } else if straight {
            (0.0, 2.0 * pi)
        } else {
            (-pi / 2.0, pi / 2.0)
        };
        Job { worm: w.id, target: w.pos, upper, phase: Phase::Circle(lower) }
    }

    fn try_spot(&mut self, c: &Ctx, b: &mut Budget, job: &mut Job) -> Try {
        let trg = job.target;
        match job.phase {
            Phase::Circle(j) => {
                if j > job.upper {
                    job.phase = Phase::Right(0, trg.x);
                    return Try::Next;
                }
                let p = Vec2::new(40.0 * libm::sinf(j) + trg.x, 40.0 * libm::cosf(j) + trg.y);
                // The worst case of both probes first, so neither is lost.
                if u64::from(b.left()) < 15 * 15 + 40 / 5 + 2 {
                    return Try::Later;
                }
                // `traceLine(..) >= 40`: OLX truncates a length of about 40.
                let ok = match in_air(c.t, b, p, 1) {
                    Some(true) => matches!(trace_line(c.t, b, trg, p, 5), Some((len, _)) if len >= 40),
                    Some(false) => false,
                    None => return Try::Later,
                };
                job.phase = Phase::Circle(j + STEP);
                if ok { Try::Spot(p) } else { Try::Next }
            }
            Phase::Right(i, x) | Phase::Left(i, x) => {
                let right = matches!(job.phase, Phase::Right(..));
                if i >= 10 {
                    if right {
                        job.phase = Phase::Left(0, trg.x);
                        return Try::Next;
                    }
                    // Not found: the last position tried.
                    return Try::Spot(Vec2::new(x, trg.y));
                }
                let x = if right { x + 5.0 } else { x - 5.0 };
                let p = Vec2::new(x, trg.y);
                let Some(clear) = worm_line_clear(c.t, b, p, trg) else { return Try::Later };
                // One step back, towards the target. OLX's left search
                // steps a further 5 px left (`x -= 5` again,
                // `CWormBot.cpp:4026`): 10 px off by every wall, outside the
                // map at its left edge (notes §1.12).
                let back = if right { x - 5.0 } else { x + 5.0 };
                job.phase = if right { Phase::Right(i + 1, x) } else { Phase::Left(i + 1, x) };
                if !clear {
                    if libm::fabsf(trg.x - back) >= 15.0 {
                        return Try::Spot(Vec2::new(back, trg.y));
                    }
                    job.phase = if right { Phase::Right(i + 1, back) } else { Phase::Left(i + 1, back) };
                }
                Try::Next
            }
        }
    }
}
