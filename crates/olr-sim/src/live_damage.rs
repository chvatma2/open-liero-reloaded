//! Explosions damage destructible live copies (issue #94; the design
//! `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`, call
//! 21). No OLX counterpart: OLX's `CClient::Explosion` damages worms only,
//! and [`crate::effects::explosion`] runs this after its worm loop, so the
//! worms' damage, its order and its draws are untouched.
//!
//! A copy with health above 0 is hit when the explosion's centre lies
//! within [`REACH`] world pixels (the worms' radius) of a pixel one of its
//! visible parts covers (rock and background alike, at their place now,
//! mirrored by `Facing`; [`crate::LiveSolid::covers`]): the distance to the
//! nearest point of the pixel's square. It loses the explosion's own
//! damage truncated toward zero ([`damage_amount`]), once an explosion,
//! with no owner and no credit (no damage score, kill or mode callback);
//! a negative amount heals up to its points. At 0 it enters its `at_zero`
//! state once and stays at 0, taking no more damage until a script or a
//! state sets its health above 0 (a repair).

use olr_math::Vec2;

use crate::events::{EventKind, TickEvents};
use crate::live::{
    LiveCopy, LiveCopyView, LiveLevel, LiveSolid, LiveTarget, look_in, part_place, put_in, range_in, within,
};

/// How far an explosion reaches a copy's covered pixels, world pixels:
/// the worms' 5 (`CClient::Explosion`'s `length2() <= 25`).
pub const REACH: f64 = 5.0;

/// The health an explosion of `damage` takes (issue #94): truncated toward
/// zero as `as i64` does (saturating at `i64`'s bounds, NaN 0), so 4.9
/// takes 4 and -0.5 heals nothing.
pub(crate) fn damage_amount(damage: f32) -> i64 {
    damage as i64
}

/// The destructible copies' health, borrowed apart from the rock parts'
/// index for a physics sub-step or the shots' delivery
/// ([`crate::live::LiveWorld::physics_parts`]).
pub(crate) struct LiveHits<'a> {
    pub(crate) level: &'a LiveLevel,
    pub(crate) offsets: &'a [u32],
    pub(crate) states: &'a mut [u16],
    pub(crate) values: &'a mut [i64],
    /// Set when a copy entered its `at_zero` state: the index is rebuilt
    /// after the borrow ([`crate::live::LiveWorld::refresh_if_stale`]).
    pub(crate) stale: &'a mut bool,
}

impl LiveHits<'_> {
    /// The same, borrowed for a shorter while.
    pub(crate) fn reborrow(&mut self) -> LiveHits<'_> {
        LiveHits {
            level: self.level,
            offsets: self.offsets,
            states: self.states,
            values: self.values,
            stale: self.stale,
        }
    }

    /// An explosion of `damage` at `pos`: each destructible copy in level
    /// order whose covered pixels it reaches and whose health is above 0
    /// loses the amount, emitting [`EventKind::LiveDamaged`], and at 0
    /// enters its `at_zero` state ([`EventKind::LiveState`]). An amount of
    /// 0 hits nothing. A copy's bounding rectangle is checked first, so an
    /// explosion far from every destructible copy costs one comparison a
    /// copy. Allocates nothing.
    pub(crate) fn explode(&mut self, pos: Vec2, damage: f32, events: &mut TickEvents) {
        let amount = damage_amount(damage);
        if amount == 0 {
            return;
        }
        let (px, py) = (f64::from(pos.x), f64::from(pos.y));
        let level = self.level;
        for &(copy, (x0, y0, x1, y1)) in level.destructible() {
            // NaN fails every comparison: no hit.
            let near = px >= f64::from(x0) - REACH
                && px <= f64::from(x1) + REACH
                && py >= f64::from(y0) - REACH
                && py <= f64::from(y1) + REACH;
            if near {
                self.hit(copy, (px, py), amount, events);
            }
        }
    }

    /// Copy `copy` hit by an explosion at `pos` taking `amount`, if it
    /// has health above 0 and the explosion reaches its covered pixels.
    fn hit(&mut self, copy: u16, pos: (f64, f64), amount: i64, events: &mut TickEvents) {
        let level = self.level;
        let Some(c) = level.copies.get(usize::from(copy)) else { return };
        let Some(health) = c.health else { return };
        let Some(hp) = c.props.iter().position(|p| p.target == LiveTarget::Health) else { return };
        let Ok(prop) = u16::try_from(hp) else { return };
        let r = range_in(self.offsets, self.values.len(), copy);
        let Some(&before) = self.values[r.clone()].get(hp) else { return };
        if before <= 0 {
            return;
        }
        let state = self.states.get(usize::from(copy)).copied().unwrap_or(0);
        if !reaches(c, copy, state, &self.values[r.clone()], pos) {
            return;
        }
        let left = within(before.saturating_sub(amount), 0, i64::from(health.points));
        put_in(level, self.offsets, self.values, copy, prop, left);
        let wrecked = self.values[r].get(hp) == Some(&0);
        let amount32 = i32::try_from(amount).unwrap_or(if amount < 0 { i32::MIN } else { i32::MAX });
        events.push(EventKind::LiveDamaged { copy, amount: amount32, wrecked });
        if wrecked {
            if look_in(level, self.offsets, self.states, self.values, copy, health.at_zero) {
                events.push(EventKind::LiveState { copy, state: health.at_zero });
                *self.stale = true;
            }
            // Wrecked stays at 0, whatever health the state's look names.
            put_in(level, self.offsets, self.values, copy, prop, 0);
        }
    }
}

/// Whether a pixel a visible part of copy `copy` (`c`, in `state` with
/// `values`) covers lies within [`REACH`] of `pos`.
fn reaches(c: &LiveCopy, copy: u16, state: u16, values: &[i64], pos: (f64, f64)) -> bool {
    let view = LiveCopyView::still(copy, c, state, values);
    let flip = view.target(LiveTarget::Facing).is_some_and(|v| v != 0);
    view.visible_parts().any(|(p, _)| {
        c.pictures.get(usize::from(p.picture)).is_some_and(|pic| {
            let at = part_place(c, values, p.at, p.layer, pic.size.0, flip);
            covered_near(pic, at, flip, pos)
        })
    })
}

/// Whether `pic`, its top-left at `at` (mirrored when `flip`), covers a
/// pixel whose square lies within [`REACH`] of `pos`: at most 11 × 11
/// pixels are read.
fn covered_near(pic: &LiveSolid, at: (i32, i32), flip: bool, pos: (f64, f64)) -> bool {
    let (px, py) = pos;
    if !(px.is_finite() && py.is_finite()) {
        return false;
    }
    let (ax, ay) = (i64::from(at.0), i64::from(at.1));
    let (w, h) = (i64::from(pic.size.0), i64::from(pic.size.1));
    let lo = |v: f64, a: i64| (libm::floor(v - REACH) as i64).max(a);
    let hi = |v: f64, a: i64, n: i64| (libm::floor(v + REACH) as i64).min(a.saturating_add(n) - 1);
    let (x0, x1, y0, y1) = (lo(px, ax), hi(px, ax, w), lo(py, ay), hi(py, ay, h));
    // The distance along an axis from `v` to pixel `k`'s span `k..k + 1`.
    let gap = |v: f64, k: i64| {
        let (a, b) = (k as f64, (k + 1) as f64);
        if v < a {
            a - v
        } else if v > b {
            v - b
        } else {
            0.0
        }
    };
    (y0..=y1).any(|y| {
        let dy = gap(py, y);
        (x0..=x1).any(|x| {
            let lx = x - ax;
            let lx = if flip { w - 1 - lx } else { lx };
            let dx = gap(px, x);
            pic.covers(lx as u32, (y - ay) as u32) && dx * dx + dy * dy <= REACH * REACH
        })
    })
}

impl crate::World {
    /// `CClient::Explosion` of `damage` at `pos` (no owner, no projectile)
    /// between ticks, as a projectile's in a sub-step: worms, bonuses, the
    /// map and the destructible live copies as there, the parts' index
    /// rebuilt after. For tests and tools of issue #94 (a recorded game
    /// never calls it); its events come first in the next tick's, as a
    /// setup call's.
    #[doc(hidden)]
    pub fn explode_for_test(&mut self, pos: Vec2, damage: f32) {
        let now = self.time;
        let (_, hits) = self.live.as_mut().map_or((None, None), crate::live::LiveWorld::physics_parts);
        let mut blast = crate::effects::Blast {
            settings: &self.settings,
            terrain: &mut self.terrain,
            holes: &self.holes,
            worms: &mut self.worms,
            draws: &mut self.draws,
            bonuses: &mut self.bonuses.client,
            now,
            wall_clock: now + olr_math::TimeDiff(crate::world::CLOCK_OFFSET_MS),
            physics_dt: self.settings.lx56_physics_dt(),
            referee: &mut self.referee,
            live: hits,
        };
        crate::effects::explosion(&mut blast, now, pos, damage, 5, -1, -1);
        if let Some(l) = &mut self.live {
            l.refresh_if_stale();
        }
    }
}

#[cfg(test)]
#[path = "live_damage_tests.rs"]
mod tests;
