//! The ninja rope (notes §1.5, §1.8): `AI_MoveToTarget`'s rope timers and
//! `AI_GetBestRopeSpot`, its 20 rays spread over ticks under the budget and
//! kept for 5 ticks (notes §4.3).

use rl_math::{AbsTime, Vec2};
use rl_sim::Worm;
use rl_terrain::{PX_DIRT, PX_ROCK};

use crate::bot::{Act, Bot, Ctx};
use crate::budget::{Afford, Budget};
use crate::nav::{fast_trace_line, fast_worst};

/// `AI_GetBestRopeSpot`'s rays: half a circle in steps of 0.05π.
const RAYS: u32 = 20;
/// How long a rope spot is kept (notes §4.3: at most every 5 ticks).
const SPOT_MS: u64 = 50;

/// The rope's timers and the rope spot being looked for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct RopeMind {
    /// `fRopeAttachedTime`, ms.
    pub attached_ms: u64,
    /// `fRopeHookFallingTime`, ms.
    pub falling_ms: u64,
    /// The rays cast so far for a node.
    job: Option<Rays>,
    /// The last spot found: for which node, where, and when.
    found: Option<(Vec2, Vec2, AbsTime)>,
}

/// `AI_GetBestRopeSpot`'s search in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rays {
    node: Vec2,
    from: Vec2,
    /// The worm's aim when it started (`getFaceDirection`).
    aim: Vec2,
    dir: Vec2,
    next: u32,
    best: Option<(f32, Vec2)>,
}

impl Bot {
    /// The rope timers (`CWormBot.cpp:3402-3422`): let go after 5 s hanging,
    /// or when the hook has flown 2 s without catching.
    pub(crate) fn rope_timers(&mut self, me: &Worm, act: &mut Act) {
        let r = &mut self.rope;
        r.attached_ms = if me.rope.attached { r.attached_ms.saturating_add(10) } else { 0 };
        if r.attached_ms > 5000 {
            act.rope_release = true;
            r.attached_ms = 0;
        }
        r.falling_ms =
            if me.rope.shooting && !me.rope.attached { r.falling_ms.saturating_add(10) } else { 0 };
        if r.falling_ms >= 2000 {
            act.rope_release = true;
            r.falling_ms = 0;
        }
    }

    /// `AI_GetBestRopeSpot(trg)` (`CWormBot.cpp:3200-3280`): of 20 rays of
    /// `RopeMaxLength` over half a circle facing `trg`, the first rock or
    /// dirt each meets, scored by nearness to 30 px above `trg`, by the
    /// angle to the aim (OLX normalises the hit's absolute position here;
    /// this takes its direction from the worm, notes §4.2) and by length.
    /// The rays are cast as the budget allows, a few ticks at most; until
    /// they are all cast the last spot for this node is used, or `trg`.
    pub(crate) fn best_rope_spot(&mut self, c: &Ctx, b: &mut Budget, trg: Vec2) -> Vec2 {
        let me = c.me;
        if let Some((node, spot, at)) = self.rope.found
            && node == trg
            && c.now.0.saturating_sub(at.0) < SPOT_MS
        {
            return spot;
        }
        let dir = trg - me.pos;
        if dir.length2() < 0.001 {
            return trg;
        }
        let fresh = self.rope.job.is_none_or(|j| j.node != trg);
        if fresh {
            let len = c.s.rope_max_length as f32;
            let d = dir * (len / dir.length());
            self.rope.job = Some(Rays {
                node: trg,
                from: me.pos,
                aim: me.face_direction(),
                dir: Vec2::new(-d.y, d.x),
                next: 0,
                best: None,
            });
        }
        let Some(mut job) = self.rope.job else { return trg };
        let (sin, cos) =
            (libm::sinf(-0.05 * core::f32::consts::PI), libm::cosf(-0.05 * core::f32::consts::PI));
        while job.next < RAYS {
            let end = job.from + job.dir;
            let worst = fast_worst(end, job.from);
            match b.afford(worst) {
                Afford::Yes => {
                    let (hit, probes) = fast_trace_line(c.t, end, job.from, PX_ROCK | PX_DIRT, worst);
                    b.spend(probes);
                    if let Some((x, y)) = hit {
                        score(&mut job, trg, Vec2::new(x as f32, y as f32));
                    }
                }
                Afford::Later => break,
                Afford::Never => {}
            }
            job.dir = Vec2::new(cos * job.dir.x - sin * job.dir.y, sin * job.dir.x + cos * job.dir.y);
            job.next += 1;
        }
        if job.next < RAYS {
            self.rope.job = Some(job);
            return match self.rope.found {
                Some((node, spot, _)) if node == trg => spot,
                _ => trg,
            };
        }
        self.rope.job = None;
        let spot = job.best.map_or(trg, |(_, p)| p);
        self.rope.found = Some((trg, spot, c.now));
        spot
    }
}

/// `bestropespot_collision_action` (`CWormBot.cpp:3200-3249`) for the hit
/// `p`, with the angle term taken from the worm.
fn score(job: &mut Rays, trg: Vec2, p: Vec2) {
    let target = trg - Vec2::new(0.0, 30.0);
    let pos = job.from;
    let mut len = (pos - p).length();
    if len < 30.0 {
        return;
    }
    if pos.y > target.y && p.y > pos.y - 10.0 {
        // Lower than the worm, but it wants to go up.
        return;
    }
    let d = (p - target).length();
    let near = if d != 0.0 { 1.0 / d } else { 1e15 };
    let angle = (job.aim - (p - pos).normalize()).length();
    let angle = if angle != 0.0 { 1.0 / angle } else { 1e15 };
    len = (-len * (len - 200.0)).max(0.0);
    let mut value = near * 100_000.0 + angle * 10_000.0 + len * 0.001;
    if pos.y - target.y > 40.0 {
        // Going up: only the nearness counts.
        value = near;
    }
    if job.best.is_none_or(|(v, _)| v < value) {
        job.best = Some((value, p));
    }
}
