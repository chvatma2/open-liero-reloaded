//! What projectile events do to the map and the worms: `CClient::Explosion`,
//! `CarveHole`, `PlaceDirt` and `PlaceGreenDirt` with their dirt counts.

use rl_math::{AbsTime, Vec2};
use rl_terrain::PX_DIRT;

use crate::damage::injure_worm;
use crate::proj_sim::ProjCtx;
use crate::projectile::{c_i64, c_int};
use crate::world::MAX_WORMS;
use crate::worm::Worm;

/// `CWorm::incrementDirtCount` (`src/common/CWorm.cpp:1381-1389`) on
/// `wormById(owner)`. The game mode's `Carve` hook does nothing in deathmatch.
pub(crate) fn increment_dirt_count(worms: &mut [Worm], owner: i32, d: i32) {
    if d == 0 {
        return;
    }
    if let Some(w) = worms.iter_mut().find(|w| w.id == owner) {
        w.dirt_count = w.dirt_count.wrapping_add(d);
    }
}

/// `CProjectile::hasOwner` (`include/CProjectile.h:197`).
fn has_owner(owner: i32) -> bool {
    (0..MAX_WORMS as i32).contains(&owner)
}

/// `CMap::CarveHole(size, pos)` (`src/common/CMap.cpp:1005-1105`): the stamp
/// is the theme's hole `clamp(size, 0, 4)`; a missing hole carves nothing.
pub(crate) fn carve_hole(ctx: &mut ProjCtx, size: i32, pos: Vec2) -> i32 {
    let Some(hole) = ctx.holes.get(size.clamp(0, 4) as usize) else { return 0 };
    ctx.terrain.carve_hole(hole, c_int(pos.x), c_int(pos.y)) as i32
}

/// The `PJ_CARVE` action (`src/common/PhysicsLX56_Projectiles.cpp:781-793`):
/// a hole of size `Damage` at the projectile, counted for its owner.
pub(crate) fn carve(ctx: &mut ProjCtx, size: i32, pos: Vec2, owner: i32) {
    let d = carve_hole(ctx, size, pos);
    if has_owner(owner) {
        increment_dirt_count(ctx.worms, owner, d);
    }
}

/// `projectile_doMakeDirt` (`:1132-1144`): three `PlaceDirt(5, ...)` (size 5
/// clamps to the largest hole) around the projectile, taken off its owner's
/// dirt count. The offsets are added in float before `PlaceDirt` truncates.
pub(crate) fn make_dirt(ctx: &mut ProjCtx, pos: Vec2, owner: i32) {
    let mut d: i32 = 0;
    for p in [pos - Vec2::new(6.0, 6.0), pos + Vec2::new(6.0, -6.0), pos + Vec2::new(0.0, 6.0)] {
        if let Some(hole) = ctx.holes.get(4) {
            d = d.wrapping_add(ctx.terrain.place_dirt(hole, c_int(p.x), c_int(p.y)) as i32);
        }
    }
    if has_owner(owner) {
        increment_dirt_count(ctx.worms, owner, d.wrapping_neg());
    }
}

/// `projectile_doMakeGreenDirt` (`:1146-1154`): `PlaceGreenDirt(pos)`, one
/// `rand()` per green pixel placed, taken off the owner's dirt count. Without
/// the mask OLX places nothing (`CMap.cpp:1233-1236`).
pub(crate) fn make_green_dirt(ctx: &mut ProjCtx, pos: Vec2, owner: i32) {
    let Some(mask) = ctx.green_mask else { return };
    let rand = &mut ctx.draws.rand;
    let d = ctx.terrain.place_green_dirt(mask, c_int(pos.x), c_int(pos.y), &mut || rand.random_int(3)) as i32;
    if has_owner(owner) {
        increment_dirt_count(ctx.worms, owner, d.wrapping_neg());
    }
}

/// `CClient::Explosion` (`src/client/CClient_Game.cpp:227-330`): the dirt
/// probe and its two particles (4 `rand()`), the explosion entity, a hole of
/// size `(int)damage` counted for `owner`, then the full `damage` to every
/// alive worm whose recorded position (`getBest`, as the hit test) is within
/// 5 px: no falloff. With `Bonuses` on, the bonus chain is Task 9's; the
/// viewport shake is cosmetic.
pub(crate) fn explosion(ctx: &mut ProjCtx, time: AbsTime, pos: Vec2, damage: f32, owner: i32) {
    let (w, h) = (ctx.terrain.width() as i64, ctx.terrain.height() as i64);
    // MIN((uint)pos.y, H - 1); px = (uint)pos.x. x86-64 converts through a 64-bit int.
    let y = (c_i64(pos.y) as u32).min((h as u32).wrapping_sub(1)) as i32 as i64;
    let px = c_i64(pos.x) as u32 as i32 as i64;
    let mut got_dirt = false;
    for x in px - 2..px + 2 {
        if x < 0 {
            continue;
        }
        if x >= w {
            break;
        }
        if ctx.terrain.flags(x, y) & PX_DIRT != 0 {
            got_dirt = true;
            break;
        }
    }
    if got_dirt {
        // GetRandomVec().multPairwise(30, 10) each.
        ctx.draws.particles(2);
    }
    ctx.draws.spawn_entity();
    let d = carve_hole(ctx, c_int(damage), pos);
    increment_dirt_count(ctx.worms, owner, d);
    // `(tLX->currentTime - time).milliseconds()`: wraps (the assert is compiled out).
    let ms_back = ctx.wall_clock.0.wrapping_sub(time.0);
    for i in 0..ctx.worms.len() {
        let w = &ctx.worms[i];
        if !w.alive {
            continue;
        }
        let wp = w.pos_recorder.get_best(ctx.physics_dt.0, ms_back);
        if (pos - wp).length2() <= 25.0 {
            let id = w.id;
            injure_worm(ctx, id, damage, owner);
        }
    }
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
