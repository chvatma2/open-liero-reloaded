//! The bot's line traces over the terrain flags, with their cost in work
//! units (one per pixel read).

use rl_math::{Vec2, normalize_vector};
use rl_terrain::{PX_ROCK, Terrain};

use crate::budget::{Afford, Budget};

/// `simpleTraceLine(start, dist, checkflag)` (`src/common/CWormBot.cpp:225-262`):
/// whether the axis-aligned line from `start` over `dist` (along its larger
/// axis) has no pixel with `flag`; a line leaving the map is not free. Also
/// returns the pixels read.
pub(crate) fn simple_trace_line(t: &Terrain, start: (i32, i32), dist: (i32, i32), flag: u8) -> (bool, u32) {
    let (w, h) = (i64::from(t.width()), i64::from(t.height()));
    let (mut sx, mut sy) = (i64::from(start.0), i64::from(start.1));
    let (mut dx, mut dy) = (i64::from(dist.0), i64::from(dist.1));
    let mut probes = 0;
    if dx.abs() >= dy.abs() {
        if dx < 0 {
            sx += dx;
            dx = -dx;
        }
        if sx < 0 || sx + dx >= w || sy < 0 || sy >= h {
            return (false, 0);
        }
        for x in 0..=dx {
            probes += 1;
            if t.flags(sx + x, sy) & flag != 0 {
                return (false, probes);
            }
        }
    } else {
        if dy < 0 {
            sy += dy;
            dy = -dy;
        }
        if sy < 0 || sy + dy >= h || sx < 0 || sx >= w {
            return (false, 0);
        }
        for y in 0..=dy {
            probes += 1;
            if t.flags(sx, sy + y) & flag != 0 {
                return (false, probes);
            }
        }
    }
    (true, probes)
}

/// The most pixels [`fast_trace_line`] reads from `start` to `target`.
pub(crate) fn fast_worst(target: Vec2, start: Vec2) -> u64 {
    let d = target - start;
    let m = libm::fabsf(d.x).max(libm::fabsf(d.y));
    if m.is_finite() { m as u64 + 2 } else { u64::MAX / 4 }
}

/// `fastTraceLine(target, start, checkflag, action)` (`src/level/FastTraceLine.h:24-104`)
/// with an action that stops at the first pixel with `flag` or outside the
/// map (every bot action returns false). Returns that pixel, if any, and the
/// pixels read. At most `max` pixels are read (the caller's worst case).
pub(crate) fn fast_trace_line(
    t: &Terrain,
    target: Vec2,
    start: Vec2,
    flag: u8,
    max: u64,
) -> (Option<(i64, i64)>, u32) {
    let dir = target - start;
    if dir.x == 0.0 && dir.y == 0.0 {
        return (None, 0);
    }
    let s_x: i32 = if dir.x >= 0.0 { 1 } else { -1 };
    let s_y: i32 = if dir.y >= 0.0 { 1 } else { -1 };
    let x_dom = s_x as f32 * dir.x >= s_y as f32 * dir.y;
    let quot = if x_dom { dir.y / dir.x } else { dir.x / dir.y };
    let (w, h) = (i64::from(t.width()), i64::from(t.height()));
    let (start_x, start_y) = (i64::from(start.x as i32), i64::from(start.y as i32));
    let (end_x, end_y) = (i64::from(s_x) * i64::from(dir.x as i32), i64::from(s_y) * i64::from(dir.y as i32));
    let (mut x, mut y) = (0i32, 0i32);
    let mut probes: u32 = 0;
    loop {
        if x_dom {
            y = (quot * x as f32) as i32;
        } else {
            x = (quot * y as f32) as i32;
        }
        if i64::from(s_x) * i64::from(x) > end_x || i64::from(s_y) * i64::from(y) > end_y {
            return (None, probes);
        }
        if u64::from(probes) >= max {
            // Only a caller's wrong worst case gets here; count it as blocked.
            return (Some((start_x, start_y)), probes);
        }
        probes += 1;
        let (px, py) = (start_x + i64::from(x), start_y + i64::from(y));
        if px < 0 || px >= w || py < 0 || py >= h || t.flags(px, py) & flag != 0 {
            return (Some((px, py)), probes);
        }
        if x_dom {
            x += s_x;
        } else {
            y += s_y;
        }
    }
}

/// The most units [`worm_line_clear`] spends from `start` to `target`.
pub(crate) fn worm_line_worst(target: Vec2, start: Vec2) -> u64 {
    fast_worst(target, start).saturating_mul(3)
}

/// `traceWormLine(target, start)` (`src/common/CWormBot.cpp:2834-2853`):
/// three rock-only traces 1 px apart, across the line. Unlike OLX, it stops
/// after the first blocked trace (same result, fewer units).
///
/// `None` when the worst case does not fit in what is left of `b` (try
/// again next tick); a line longer than a whole budget is blocked.
pub fn worm_line_clear(t: &Terrain, b: &mut Budget, target: Vec2, start: Vec2) -> Option<bool> {
    let one = fast_worst(target, start);
    match b.afford(worm_line_worst(target, start)) {
        Afford::Yes => {}
        Afford::Later => return None,
        Afford::Never => return Some(false),
    }
    let (clear, probes) = worm_line(t, target, start, one);
    b.spend(probes);
    Some(clear)
}

/// [`worm_line_clear`] without a budget, for tests and tools.
pub fn trace_worm_line(t: &Terrain, target: Vec2, start: Vec2) -> bool {
    worm_line(t, target, start, fast_worst(target, start)).0
}

fn worm_line(t: &Terrain, mut target: Vec2, start: Vec2, max: u64) -> (bool, u32) {
    // `wormsize = 3`: lines at -1, 0 and +1 across.
    let mut dir = Vec2::new(target.y - start.y, start.x - target.x);
    normalize_vector(&mut dir);
    let mut from = start - dir * 2.0 / 2.0;
    target -= dir * 2.0 / 2.0;
    let mut probes = 0u32;
    for _ in 0..3 {
        let (hit, n) = fast_trace_line(t, target, from, PX_ROCK, max);
        probes = probes.saturating_add(n);
        if hit.is_some() {
            return (false, probes);
        }
        from += dir;
        target += dir;
    }
    (true, probes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use rl_data::level::{Material, MaterialGrid};

    fn terrain(w: u32, h: u32, rock: impl Fn(u32, u32) -> bool) -> Terrain {
        let cells: Vec<Material> = (0..w * h)
            .map(|i| if rock(i % w, i / w) { Material::Rock } else { Material::Background })
            .collect();
        Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap())
    }

    #[test]
    fn simple_trace_line_reads_the_whole_line_and_refuses_the_edge() {
        let t = terrain(20, 10, |x, _| x == 15);
        assert_eq!(simple_trace_line(&t, (2, 3), (5, 0), PX_ROCK), (true, 6));
        assert_eq!(simple_trace_line(&t, (12, 3), (5, 0), PX_ROCK), (false, 4));
        assert_eq!(simple_trace_line(&t, (7, 3), (-5, 0), PX_ROCK), (true, 6));
        assert_eq!(simple_trace_line(&t, (16, 3), (5, 0), PX_ROCK), (false, 0));
        assert_eq!(simple_trace_line(&t, (3, 2), (0, 5), PX_ROCK), (true, 6));
    }

    #[test]
    fn worm_lines_are_three_wide_and_stop_at_the_map_edge() {
        // A rock pixel 1 px beside the line blocks it; 2 px beside does not.
        let t = terrain(40, 20, |x, y| x == 20 && y == 11);
        assert!(!trace_worm_line(&t, Vec2::new(30.5, 10.5), Vec2::new(5.5, 10.5)));
        let t = terrain(40, 20, |x, y| x == 20 && y == 12);
        assert!(trace_worm_line(&t, Vec2::new(30.5, 10.5), Vec2::new(5.5, 10.5)));
        assert!(!trace_worm_line(&t, Vec2::new(45.0, 10.0), Vec2::new(5.0, 10.0)));
        assert!(trace_worm_line(&t, Vec2::new(5.0, 10.0), Vec2::new(5.0, 10.0)));
        let mut b = Budget::new(10);
        assert_eq!(worm_line_clear(&t, &mut b, Vec2::new(30.0, 10.0), Vec2::new(5.0, 10.0)), Some(false));
        assert_eq!(b.used(), 0, "a line longer than a whole budget is not traced");
        let inf = Vec2::new(f32::INFINITY, 3.0);
        assert_eq!(worm_line_clear(&t, &mut Budget::new(1000), inf, Vec2::new(5.0, 10.0)), Some(false));
    }
}
