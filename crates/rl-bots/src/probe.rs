//! The brain's terrain probes besides the worm lines (`nav::trace`), each
//! paid in work units, one per pixel read (spec §20 F7). Every probe checks
//! its worst case first and gives `None` when it does not fit in what is
//! left of the budget this tick; a probe longer than a whole budget gives
//! what a blocked probe gives.

use rl_math::{Vec2, normalize_vector};
use rl_terrain::{PX_DIRT, PX_EMPTY, PX_ROCK, Terrain};

use crate::budget::{Afford, Budget};

/// The length OLX takes from `NormalizeVector` as an `int`, and the unit
/// direction; non-finite input gives length 0.
fn length_and_dir(from: Vec2, to: Vec2) -> (i32, Vec2) {
    let mut dir = to - from;
    let len = normalize_vector(&mut dir);
    if !len.is_finite() || !dir.x.is_finite() || !dir.y.is_finite() {
        return (0, Vec2::ZERO);
    }
    (len.min(i32::MAX as f32) as i32, dir)
}

/// Whether `worst` units may be spent: `Some(true)` now, `None` not this
/// tick, `Some(false)` never.
fn afford(b: &Budget, worst: u64) -> Option<bool> {
    match b.afford(worst) {
        Afford::Yes => Some(true),
        Afford::Later => None,
        Afford::Never => Some(false),
    }
}

/// `CWorm::traceLine(target, start, &type, divs)` (`src/common/CWormBot.cpp:2647-2680`)
/// with its default check flag `PX_EMPTY`: from `start` towards `target`
/// in steps of `divs` px while the pixel is empty. Gives the distance
/// covered (OLX's `int`) and the flags of the pixel that stopped it
/// (`PX_EMPTY` when none did). A trace longer than a whole budget stops at
/// once on "rock".
pub(crate) fn trace_line(
    t: &Terrain,
    b: &mut Budget,
    start: Vec2,
    target: Vec2,
    divs: i32,
) -> Option<(i32, u8)> {
    let (total, dir) = length_and_dir(start, target);
    let divisions = divs.min(total).max(1);
    if !afford(b, (total / divisions) as u64 + 1)? {
        return Some((0, PX_ROCK));
    }
    let (mut pos, mut i, mut probes) = (start, 0, 0u32);
    while i < total {
        probes += 1;
        let px = t.flags(pos.x as i64, pos.y as i64);
        if px & PX_EMPTY == 0 {
            b.spend(probes);
            return Some((i, px));
        }
        pos += dir * divisions as f32;
        i += divisions;
    }
    b.spend(probes);
    Some((total, PX_EMPTY))
}

/// `CWorm::traceLine(target, &fDist, &type, divs)` (`CWormBot.cpp:2641-2645`):
/// [`trace_line`] from the worm, with `fDist` (`(int)length / reached`, an
/// integer division, as OLX has it; 0 when nothing was reached).
pub(crate) fn trace_line_dist(
    t: &Terrain,
    b: &mut Budget,
    pos: Vec2,
    target: Vec2,
    divs: i32,
) -> Option<(i32, f32, u8)> {
    let (reached, ty) = trace_line(t, b, pos, target, divs)?;
    let len = (pos - target).length();
    let dist = if reached != 0 && len.is_finite() { ((len as i32) / reached) as f32 } else { 0.0 };
    Some((reached, dist, ty))
}

/// `traceWeaponLine` (`CWormBot.cpp:2695-2809`) without team mates: from
/// `pos` to `target`, the first step `first` px (the worm's 5 px already
/// added), then `divs` px, until dirt or rock. Gives the fraction of the
/// way travelled and the flags met (`PX_EMPTY` when clear).
pub(crate) fn trace_weapon_line(
    t: &Terrain,
    b: &mut Budget,
    pos: Vec2,
    target: Vec2,
    first: i32,
    divs: i32,
) -> Option<(f32, u8)> {
    let (total, dir) = length_and_dir(pos, target);
    let divisions = divs.min(total).max(1);
    let first = first.max(1);
    if !afford(b, (total / divisions) as u64 + 2)? {
        return Some((0.0, PX_ROCK));
    }
    let frac = |i: i32| if total != 0 { i as f32 / total as f32 } else { 0.0 };
    let (mut p, mut i, mut step, mut probes) = (pos, 0, first, 0u32);
    while i < total {
        probes += 1;
        let px = t.flags(p.x as i64, p.y as i64);
        if i > first {
            step = divisions;
        }
        if px & (PX_DIRT | PX_ROCK) != 0 {
            b.spend(probes);
            return Some((frac(i), px));
        }
        p += dir * step as f32;
        i += step;
    }
    b.spend(probes);
    Some((frac(i), PX_EMPTY))
}

/// `AI_GetRockBetween` (`CWormBot.cpp:2958-2983`): how many 4 px steps from
/// `pos` to `trg` land on rock.
pub(crate) fn rock_between(t: &Terrain, b: &mut Budget, pos: Vec2, trg: Vec2) -> Option<i32> {
    let (total, dir) = length_and_dir(pos, trg);
    if !afford(b, (total / 4) as u64 + 1)? {
        return Some(i32::MAX);
    }
    let (mut p, mut i, mut rock, mut probes) = (pos, 0, 0, 0u32);
    while i < total {
        probes += 1;
        if t.flags(p.x as i64, p.y as i64) & PX_ROCK != 0 {
            rock += 1;
        }
        p += dir * 4.0;
        i += 4;
    }
    b.spend(probes);
    Some(rock)
}

/// `checkArea_AllHaveNot<flags>(x, y, x2, y2)` (`src/game/CMap.h:427-441`):
/// whether no pixel of `x..x2` × `y..y2` (ends excluded) has `flags`
/// (outside the map is rock).
pub(crate) fn area_clear(
    t: &Terrain,
    b: &mut Budget,
    (x, y, x2, y2): (i64, i64, i64, i64),
    flags: u8,
) -> Option<bool> {
    let (w, h) = ((x2 - x).max(0) as u64, (y2 - y).max(0) as u64);
    if !afford(b, w.saturating_mul(h))? {
        return Some(false);
    }
    let mut probes = 0u32;
    for py in y..y2 {
        for px in x..x2 {
            probes += 1;
            if t.flags(px, py) & flags != 0 {
                b.spend(probes);
                return Some(false);
            }
        }
    }
    b.spend(probes);
    Some(true)
}

/// `CMap::IsGoodSpawnPoint` (`src/game/CMap.h:460`): no rock in the 6 × 6
/// box from `(x - 3, y - 3)`.
pub(crate) fn good_spawn_point(t: &Terrain, b: &mut Budget, x: i64, y: i64) -> Option<bool> {
    area_clear(t, b, (x - 3, y - 3, x + 3, y + 3), PX_ROCK)
}

/// `CMap::IsEmptyForWorm` (`src/game/CMap.h:462`): no rock or dirt in the
/// 6 × 6 box from `(x - 3, y - 3)`.
pub(crate) fn empty_for_worm(t: &Terrain, b: &mut Budget, p: Vec2) -> Option<bool> {
    let (x, y) = (p.x as i64, p.y as i64);
    area_clear(t, b, (x - 3, y - 3, x + 3, y + 3), PX_ROCK | PX_DIRT)
}

/// `AI_IsInAir(pos, area_a)` (`CWormBot.cpp:3287-3297`): inside the map and
/// no rock or dirt in the `15·area_a` square around `pos`.
pub(crate) fn in_air(t: &Terrain, b: &mut Budget, pos: Vec2, area_a: i64) -> Option<bool> {
    if !(pos.x >= 0.0 && pos.y >= 0.0 && pos.x < t.width() as f32 && pos.y < t.height() as f32) {
        return Some(false);
    }
    let w = area_a * 15;
    let (x, y) = (pos.x as i64 - w / 2, pos.y as i64 - w / 2);
    area_clear(t, b, (x, y, x + w, y + w), PX_ROCK | PX_DIRT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use rl_data::level::{Material, MaterialGrid};

    fn terrain() -> Terrain {
        // Dirt at x 50..=59, rock at x 80..=89, on a 100 × 20 map.
        let cells: Vec<Material> = (0..100 * 20)
            .map(|i| match i % 100 {
                50..=59 => Material::Dirt,
                80..=89 => Material::Rock,
                _ => Material::Background,
            })
            .collect();
        Terrain::from_grid(&MaterialGrid::from_cells(100, 20, cells).unwrap())
    }

    #[test]
    fn traces_stop_where_olx_stops_and_pay_per_pixel() {
        let t = terrain();
        let mut b = Budget::new(1000);
        let (from, to) = (Vec2::new(10.5, 10.5), Vec2::new(95.5, 10.5));
        assert_eq!(trace_line(&t, &mut b, from, to, 1), Some((40, PX_DIRT)));
        assert_eq!(b.used(), 41);
        assert_eq!(trace_line(&t, &mut b, from, Vec2::new(40.5, 10.5), 5), Some((30, PX_EMPTY)));
        // Steps of 12 px until past the first 12, then 5: 0, 12, 24, 29, ... 44.
        let (frac, ty) = trace_weapon_line(&t, &mut b, from, to, 12, 5).unwrap();
        assert_eq!(ty, PX_DIRT);
        assert!((frac - 44.0 / 85.0).abs() < 1e-6, "{frac}");
        assert_eq!(rock_between(&t, &mut b, from, to), Some(2), "x 82.5 and 86.5 are rock");
        assert_eq!(area_clear(&t, &mut b, (60, 0, 80, 20), PX_ROCK | PX_DIRT), Some(true));
        assert_eq!(area_clear(&t, &mut b, (60, 0, 81, 20), PX_ROCK), Some(false));
        assert_eq!(in_air(&t, &mut b, Vec2::new(70.0, 10.0), 1), Some(true));
        assert_eq!(in_air(&t, &mut b, Vec2::new(-1.0, 10.0), 1), Some(false));
        let mut small = Budget::new(10);
        small.spend(5);
        assert_eq!(trace_line(&t, &mut small, from, to, 1), Some((0, PX_ROCK)), "longer than a budget");
        assert_eq!(trace_line(&t, &mut small, from, Vec2::new(18.5, 10.5), 1), None, "not this tick");
        assert_eq!(small.used(), 5);
    }
}
