//! A search area's checklist test and OLX's area costs.

use rl_math::Vec2;
use rl_terrain::{PX_ROCK, Terrain};

use super::areas::{Rect, WORM_SIZE};
use super::trace::simple_trace_line;
use crate::budget::Budget;

/// `check_checkpoint` (`CWormBot.cpp:445-492`): the passage from edge
/// point `pt` outward over `dist` is wide enough when parallel rock-free
/// traces 1 px apart number at least 7, counting from beside `pt` (which is
/// itself never traced) first along +`dir` (at most 7), then along -`dir`
/// (at most the rest). Gives the point to add an area at: one step past the
/// last -`dir` trace, then `dist` out.
pub(super) fn check_checkpoint(
    t: &Terrain,
    b: &mut Budget,
    start: (i32, i32),
    dist: (i32, i32),
) -> Option<(i32, i32)> {
    let dir = if dist.0.abs() >= dist.1.abs() { (0, 1) } else { (1, 0) };
    let mut trace = |p: (i32, i32)| {
        let (free, probes) = simple_trace_line(t, p, dist, PX_ROCK);
        b.spend(probes);
        free
    };
    let size = WORM_SIZE as u32;
    let mut pt = (start.0 + dir.0, start.1 + dir.1);
    let mut free = trace(pt);
    let mut left = 0u32;
    while free && left < size {
        free = trace(pt);
        if free {
            left += 1;
        }
        pt = (pt.0 + dir.0, pt.1 + dir.1);
    }
    pt = (start.0 - dir.0, start.1 - dir.1);
    free = true;
    let mut right = 0u32;
    while free && right < size - left {
        free = trace(pt);
        if free {
            right += 1;
        }
        pt = (pt.0 - dir.0, pt.1 - dir.1);
    }
    (left + right >= size).then_some((pt.0 + dist.0, pt.1 + dist.1))
}

pub(super) fn v(p: (i32, i32)) -> Vec2 {
    Vec2::new(p.0 as f32, p.1 as f32)
}

pub(super) fn center_i(r: Rect) -> (i32, i32) {
    r.center()
}

pub(super) fn center(r: Rect) -> Vec2 {
    v(r.center())
}

/// `VectorD2<int>::GetLength` of `a - b`.
pub(super) fn length(a: (i32, i32), b: (i32, i32)) -> f64 {
    let (dx, dy) = (f64::from(a.0) - f64::from(b.0), f64::from(a.1) - f64::from(b.1));
    libm::sqrt(dx * dx + dy * dy)
}

/// `getConnectorPointForArea`: the centre of the intersection.
pub(super) fn connector(a: &Rect, b: &Rect) -> (i32, i32) {
    a.intersection(b).center()
}

/// `getDistToArea`: centre → connector → centre.
pub(super) fn dist_between(a: &Rect, b: &Rect) -> f64 {
    let c = connector(a, b);
    length(b.center(), c) + length(a.center(), c)
}
