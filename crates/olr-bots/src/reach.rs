//! Live parts' reach (issue #91; the design's call 23): a bot treats live
//! rock parts as rock where they are (its probes read `World::solid()`,
//! [`crate::ground`]), never chooses to stop where one can come
//! ([`olr_sim::LiveReach`], grown by a worm's half size), and gets out of
//! one when a part crushed it or heads for it. No OLX counterpart: OLX's
//! rock never moves.
//!
//! Every check is a rectangle test, no pixel read: it costs no work unit
//! and draws nothing from the bot's `BotRng`. On a level without live parts
//! (or with none that can move or appear) none runs at all, so a bot plays
//! the classic levels exactly as before.

use olr_math::{AbsTime, Vec2};
use olr_sim::{LiveCopyView, LiveReach, LiveTarget};

use crate::bot::{Bot, Ctx};
use crate::budget::Budget;
use crate::probe::good_spawn_point;

/// A worm's half size, world pixels: a reach grown by it is where a worm
/// can be touched by its part.
pub(crate) const HALF_WORM: i32 = 4;

/// How far past a grown reach the way out lies, world pixels.
const OUT_MARGIN: i32 = 2;

/// How long a bot heads out at most, ms.
pub(crate) const ESCAPE_MS: u64 = 1000;

/// A bot getting out of a reach: where it heads, the reach, until when,
/// and the scripted goal it had ([`Bot::set_goal`]), given back after.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Escape {
    pub point: Vec2,
    pub rect: (i32, i32, i32, i32),
    pub until: AbsTime,
    pub goal: Option<Vec2>,
}

/// `r` grown by a worm's half size.
fn grown(r: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    (
        r.0.saturating_sub(HALF_WORM),
        r.1.saturating_sub(HALF_WORM),
        r.2.saturating_add(HALF_WORM),
        r.3.saturating_add(HALF_WORM),
    )
}

/// Whether `p` lies in `r` (the ends exclusive).
fn inside(r: (i32, i32, i32, i32), p: Vec2) -> bool {
    p.x >= r.0 as f32 && p.x < r.2 as f32 && p.y >= r.1 as f32 && p.y < r.3 as f32
}

/// The sign of where a moving value heads: -1, 0 or 1.
fn heading(c: &LiveCopyView, target: LiveTarget) -> i32 {
    match (c.motion(target), c.target(target)) {
        (Some(m), Some(v)) if m.moving() => (m.target - v).signum() as i32,
        _ => 0,
    }
}

impl Bot {
    /// The reaches of the world's live parts; none (and no check counted)
    /// without live parts that can move or appear.
    fn reaches<'a>(&mut self, c: &Ctx<'a>) -> &'a [LiveReach] {
        let r = c.w.live().map_or(&[][..], |l| l.reach_rects());
        if !r.is_empty() {
            self.reach_checks = self.reach_checks.saturating_add(1);
        }
        r
    }

    /// The grown reach `p` lies in, if any.
    pub(crate) fn in_reach(&mut self, c: &Ctx, p: Vec2) -> Option<(i32, i32, i32, i32)> {
        self.reaches(c).iter().map(|r| grown(r.rect)).find(|&r| inside(r, p))
    }

    /// The nearest point out of the grown reach `r` from `p`: just past one
    /// of its sides, inside the map, preferring one in no reach and where
    /// a worm fits (OLX's spawn check, while the budget lasts); else the
    /// nearest. Draws nothing from the bot's rng.
    pub(crate) fn way_out(&mut self, c: &Ctx, b: &mut Budget, r: (i32, i32, i32, i32), p: Vec2) -> Vec2 {
        let (w, h) = (c.t.width() as f32, c.t.height() as f32);
        let mut out = [
            Vec2::new((r.0 - 1 - OUT_MARGIN) as f32, p.y),
            Vec2::new((r.2 + OUT_MARGIN) as f32, p.y),
            Vec2::new(p.x, (r.1 - 1 - OUT_MARGIN) as f32),
            Vec2::new(p.x, (r.3 + OUT_MARGIN) as f32),
        ];
        // Nearest first; the sort is stable, so ties keep the order above.
        out.sort_by(|a, b| (*a - p).length2().total_cmp(&(*b - p).length2()));
        let in_map = |q: &Vec2| q.x >= 0.0 && q.y >= 0.0 && q.x < w && q.y < h;
        let mut best = None;
        for q in out.into_iter().filter(in_map) {
            if self.in_reach(c, q).is_some() {
                continue;
            }
            match good_spawn_point(c.t, b, q.x as i64, q.y as i64) {
                Some(true) => return q,
                _ => {
                    best = best.or(Some(q));
                }
            }
        }
        best.or_else(|| out.into_iter().find(in_map)).unwrap_or(p)
    }

    /// `p`, or the way out of the reach it lies in (a stop a bot chooses
    /// is never inside one).
    pub(crate) fn out_of_reach(&mut self, c: &Ctx, b: &mut Budget, p: Vec2) -> Vec2 {
        match self.in_reach(c, p) {
            Some(r) => self.way_out(c, b, r, p),
            None => p,
        }
    }

    /// The grown reach the bot should get out of now: one of copy
    /// `crushed`'s (the part that crushed it last tick) it stands in, or
    /// one whose part is moving towards it this tick.
    fn threat(&mut self, c: &Ctx, crushed: Option<u16>) -> Option<(i32, i32, i32, i32)> {
        let reaches = self.reaches(c);
        let live = c.w.live()?;
        let me = c.me.pos;
        for r in reaches {
            let g = grown(r.rect);
            if !inside(g, me) {
                continue;
            }
            if crushed == Some(r.copy) {
                return Some(g);
            }
            let Some(cv) = live.copy(r.copy) else { continue };
            let mirrored = cv.target(LiveTarget::Facing).is_some_and(|v| v != 0);
            let (mut dx, mut dy) = (heading(&cv, LiveTarget::X), heading(&cv, LiveTarget::Y));
            if let Some(l) = r.layer {
                let ox = heading(&cv, LiveTarget::OffsetX(l));
                dx += if mirrored { -ox } else { ox };
                dy += heading(&cv, LiveTarget::OffsetY(l));
            }
            if dx == 0 && dy == 0 {
                continue;
            }
            // Where the parts are now.
            let mut now: Option<(i32, i32, i32, i32)> = None;
            for (p, (x, y)) in cv.rock_parts().filter(|(p, _)| p.layer == r.layer) {
                let Some(pic) = cv.setup().pictures.get(usize::from(p.picture)) else { continue };
                let (x1, y1) = (x.saturating_add(pic.size.0 as i32), y.saturating_add(pic.size.1 as i32));
                now = Some(match now {
                    None => (x, y, x1, y1),
                    Some(u) => (u.0.min(x), u.1.min(y), u.2.max(x1), u.3.max(y1)),
                });
            }
            let Some(n) = now else { continue };
            let (cx, cy) = ((n.0 + n.2) as f32 / 2.0, (n.1 + n.3) as f32 / 2.0);
            let across =
                |lo: i32, hi: i32, v: f32| v > (lo - HALF_WORM) as f32 && v < (hi + HALF_WORM) as f32;
            let ahead_x = dx != 0 && across(n.1, n.3, me.y) && (me.x - cx) * dx.signum() as f32 >= 0.0;
            let ahead_y = dy != 0 && across(n.0, n.2, me.x) && (me.y - cy) * dy.signum() as f32 >= 0.0;
            if ahead_x || ahead_y {
                return Some(g);
            }
        }
        None
    }

    /// Getting out (issue #91), each tick before thinking: an escape ends
    /// after [`ESCAPE_MS`] or once the bot is out of its reach, giving its
    /// scripted goal back; a bot crushed last tick by copy `crushed`, or in
    /// the way of a moving part, heads for the nearest point out of that
    /// reach as a racing goal.
    pub(crate) fn get_out(&mut self, c: &Ctx, b: &mut Budget, crushed: Option<u16>) {
        if let Some(e) = self.escape
            && (c.now >= e.until || !inside(e.rect, c.me.pos))
        {
            self.end_escape();
        }
        if self.escape.is_some() {
            return;
        }
        let Some(r) = self.threat(c, crushed) else { return };
        let point = self.way_out(c, b, r, c.me.pos);
        let goal = self.goal;
        self.goal = Some(point);
        self.escape = Some(Escape { point, rect: r, until: AbsTime(c.now.0 + ESCAPE_MS), goal });
        self.mind.state = crate::brain::State::Think;
        self.mind.pos_target = point;
        self.follow.clear();
    }

    /// The escape ends: the scripted goal comes back.
    pub(crate) fn end_escape(&mut self) {
        if let Some(e) = self.escape.take() {
            self.goal = e.goal;
            self.mind.state = crate::brain::State::Think;
        }
    }
}

#[cfg(test)]
#[path = "reach_tests.rs"]
mod tests;
