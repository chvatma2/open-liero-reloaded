//! What projectile events do to the map and the worms: `CClient::Explosion`,
//! `CarveHole`, `PlaceDirt` and `PlaceGreenDirt` with their dirt counts.

use rl_math::{AbsTime, TimeDiff, Vec2};
use rl_terrain::{PX_DIRT, Stamp, Terrain};

use crate::SimSettings;
use crate::bonus::{Bonus, MAX_BONUSES};
use crate::damage::{Harm, injure_worm};
use crate::events::{CosmeticKind, Event, StampId, TerrainOp, TickEvents};
use crate::proj_sim::ProjCtx;
use crate::projectile::{c_i64, c_int};
use crate::rng::Draws;
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
    carve_at(ctx.terrain, ctx.holes, &mut ctx.draws.events, TerrainOp::Carve, size, pos)
}

/// [`carve_hole`] on a terrain and its theme's holes, reported as `op`.
pub(crate) fn carve_at(
    terrain: &mut Terrain,
    holes: &[Stamp],
    events: &mut TickEvents,
    op: TerrainOp,
    size: i32,
    pos: Vec2,
) -> i32 {
    carve_px(terrain, holes, events, op, size.clamp(0, 4) as usize, c_int(pos.x), c_int(pos.y)) as i32
}

/// `Terrain::carve_hole` with the theme's hole `hole` at `(x, y)`, reported as
/// a [`Event::TerrainEdit`]; a missing hole carves and reports nothing.
pub(crate) fn carve_px(
    terrain: &mut Terrain,
    holes: &[Stamp],
    events: &mut TickEvents,
    op: TerrainOp,
    hole: usize,
    x: i32,
    y: i32,
) -> u32 {
    let Some(stamp) = holes.get(hole) else { return 0 };
    events.edit(op, StampId::Hole(hole as u8), x, y, (stamp.width(), stamp.height()));
    terrain.carve_hole(stamp, x, y)
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
            let (x, y) = (c_int(p.x), c_int(p.y));
            ctx.draws.events.edit(TerrainOp::Dirt, StampId::Hole(4), x, y, (hole.width(), hole.height()));
            d = d.wrapping_add(ctx.terrain.place_dirt(hole, x, y) as i32);
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
    let (x, y) = (c_int(pos.x), c_int(pos.y));
    ctx.draws.events.edit(TerrainOp::GreenDirt, StampId::GreenBall, x, y, (mask.width(), mask.height()));
    let rand = &mut ctx.draws.rand;
    let d = ctx.terrain.place_green_dirt(mask, x, y, &mut || rand.random_int(3)) as i32;
    if has_owner(owner) {
        increment_dirt_count(ctx.worms, owner, d.wrapping_neg());
    }
}

/// What `CClient::Explosion` reads or changes.
pub(crate) struct Blast<'a> {
    pub settings: &'a SimSettings,
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub worms: &'a mut [Worm],
    pub draws: &'a mut Draws,
    /// The client's bonuses.
    pub bonuses: &'a mut [Bonus; MAX_BONUSES],
    /// `GetPhysicsTime()`: the frame clock.
    pub now: AbsTime,
    /// `tLX->currentTime`, pinned to `now + K` (spec §18 D2).
    pub wall_clock: AbsTime,
    /// `LX56PhysicsDT`.
    pub physics_dt: TimeDiff,
}

impl Blast<'_> {
    fn harm(&mut self) -> Harm<'_> {
        Harm { settings: self.settings, worms: self.worms, draws: self.draws, now: self.now }
    }
}

impl<'a> ProjCtx<'a> {
    /// The explosion's part of the context.
    pub fn blast(&mut self) -> Blast<'_> {
        Blast {
            settings: self.settings,
            terrain: self.terrain,
            holes: self.holes,
            worms: self.worms,
            draws: self.draws,
            bonuses: self.bonuses,
            now: self.now,
            wall_clock: self.wall_clock,
            physics_dt: self.physics_dt,
        }
    }
}

/// `CClient::Explosion` (`src/client/CClient_Game.cpp:227-330`): the dirt
/// probe; with `Bonuses` on, each client bonus within 15 px on both axes is
/// destroyed (the server is not told) and explodes in turn, a recursive
/// `Explosion(time, pos, 15, 5, owner)` at this explosion's position
/// (`:255-270`); then the probe's two particles (4 `rand()`), the explosion
/// entity, a hole of size `(int)damage` counted for `owner`, and the full
/// `damage` to every alive worm whose recorded position (`getBest`, as the
/// hit test) is within 5 px: no falloff. The viewport shake is cosmetic.
/// Reported as an [`Event::Explosion`], the `ENT_EXPLOSION` of size
/// `(int)damage` (8 up to damage 5) and the hole's terrain edit.
pub(crate) fn explosion(ctx: &mut Blast, time: AbsTime, pos: Vec2, damage: f32, owner: i32) {
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
    if ctx.settings.bonuses {
        for i in 0..MAX_BONUSES {
            let b = &ctx.bonuses[i];
            if !b.used || libm::fabsf(b.pos.x - pos.x) > 15.0 || libm::fabsf(b.pos.y - pos.y) > 15.0 {
                continue;
            }
            ctx.bonuses[i].used = false;
            explosion(ctx, time, pos, 15.0, owner);
        }
    }
    if got_dirt {
        ctx.draws.particles(2, pos, |v| Vec2::new(v.x * 30.0, v.y * 10.0));
    }
    let size = if damage > 5.0 { c_int(damage) } else { 8 };
    ctx.draws.events.push(Event::Explosion { pos, damage });
    ctx.draws.entity(CosmeticKind::Explosion { size }, pos, Vec2::ZERO);
    let d = carve_at(ctx.terrain, ctx.holes, &mut ctx.draws.events, TerrainOp::Carve, c_int(damage), pos);
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
            injure_worm(&mut ctx.harm(), id, damage, owner);
        }
    }
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
