//! Beams and special weapons (`docs/porting/olx-notes/weapons-firing.md`
//! §6–7): the client's `CClient::ProcessShot_Beam` at delivery, its
//! `CClient::DrawBeam` in every sub-step the beam's slot counts down, and
//! `CClient::ShootSpecial` (the jetpack).
//!
//! Both beam traces step 1 px at a time and stop at the first dirt or rock
//! (outside the map is rock), where a `Damage`-sized hole is carved, or at
//! the first other living worm within 5 px. Only the delivered shot injures;
//! the drawn beam carves but does no damage.

use alloc::vec::Vec;

use rl_data::image::Rgba;
use rl_data::mod_def::{BeamDef, SpecialKind};
use rl_math::{AbsTime, TimeDiff, Vec2};
use rl_terrain::{PX_DIRT, PX_ROCK, Stamp, Terrain};

use crate::SimSettings;
use crate::bonus::{Bonus, MAX_BONUSES};
use crate::damage::{Harm, injure_worm};
use crate::effects::{Blast, carve_at, explosion, increment_dirt_count};
use crate::events::{CosmeticKind, TerrainOp};
use crate::mod_rt::{SimMod, SimWeapon};
use crate::projectile::c_int;
use crate::rng::Draws;
use crate::shots::Shot;
use crate::spawn::vec_from_angle;
use crate::world::{CLOCK_OFFSET_MS, MAX_WORMS};
use crate::worm::{Dir, Worm};

/// What the client's beam and special code reads or changes besides the worms.
pub(crate) struct BeamCtx<'a> {
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub draws: &'a mut Draws,
    /// `goodWidthParts`. A beam is never wider than the map's smaller side,
    /// so with that much capacity tracing does not allocate.
    pub parts: &'a mut Vec<bool>,
}

/// What a beam part does at one point.
enum Part {
    /// It stops there (`goodWidthParts[j] = false; continue`).
    Stopped,
    /// It stops, but still keeps the beam going for this step (`DrawBeam`'s
    /// worm hit, which has no `continue`).
    StoppedLate,
    /// It goes on (`stopbeam = false`).
    Clear,
}

/// The trace loop shared by `ProcessShot_Beam` and `DrawBeam`
/// (`src/client/CClient_Game.cpp:965-1047` and `640-697`): up to `Length`
/// steps of `dir` from `pos`, `width` parts side by side along
/// `dir.orthogonal()`. `part` gets each live part's point and the width.
/// Returns where the trace stopped.
fn trace(
    beam: &BeamDef,
    mut pos: Vec2,
    dir: Vec2,
    map_min: u32,
    parts: &mut Vec<bool>,
    mut part: impl FnMut(Vec2, i32) -> Part,
) -> Vec2 {
    let orth = Vec2::new(dir.y, -dir.x);
    parts.clear();
    let mut width: i32 = 0;
    let map_min = map_min.min(i32::MAX as u32) as i32;
    for i in 0..beam.length {
        let new_width = c_int(beam.init_width as f32 + beam.width_increase * i as f32);
        // `MIN(int, uint)` resolves to `MIN(unsigned long, unsigned int)`
        // (`include/MathLib.h:68`): a negative width becomes the map's.
        let new_width = if new_width < 0 { map_min } else { new_width.min(map_min) };
        if new_width != width {
            parts.resize(new_width as usize, false);
            for j in width..new_width {
                let old = (j - 2).max(0) as usize;
                parts[j as usize] = if width > 0 { parts[old] } else { true };
            }
            width = new_width;
        }
        if width <= 0 {
            break;
        }
        let mut stop = true;
        for (j, good) in parts.iter_mut().enumerate() {
            if !*good {
                continue;
            }
            let o = if j % 2 == 0 { (j / 2) as i32 } else { -(j.div_ceil(2) as i32) };
            let p = pos + orth * o as f32;
            match part(p, width) {
                Part::Stopped => *good = false,
                Part::StoppedLate => {
                    *good = false;
                    stop = false;
                }
                Part::Clear => stop = false,
            }
        }
        if stop {
            break;
        }
        pos += dir;
    }
    pos
}

/// `(p.x <= 0 || p.y <= 0) ? PX_ROCK : GetPixelFlag((int)p.x, (int)p.y)`.
fn solid(t: &Terrain, p: Vec2) -> bool {
    let px = if p.x <= 0.0 || p.y <= 0.0 { PX_ROCK } else { t.flags(c_int(p.x) as i64, c_int(p.y) as i64) };
    px & (PX_DIRT | PX_ROCK) != 0
}

/// `DistributeDamageOverWidth`: `damage / width`, at least `SIGN(damage)`.
fn spread(damage: i32, beam: &BeamDef, width: i32) -> i32 {
    if !beam.distribute_damage_over_width {
        return damage;
    }
    let d = damage / width;
    if d == 0 { damage.signum() } else { d }
}

/// The living worms other than `id`, in game order (`std::list<CWorm*> worms`),
/// as a set of indices.
fn others(worms: &[Worm], id: i32) -> u32 {
    worms
        .iter()
        .take(MAX_WORMS)
        .enumerate()
        .filter(|(_, w)| w.alive && w.id != id)
        .fold(0, |m, (i, _)| m | 1 << i)
}

/// The first worm in `set` within 5 px of `p` (`GetLength2() < 5 * 5`).
fn hit(worms: &[Worm], set: u32, p: Vec2) -> Option<usize> {
    (0..worms.len().min(MAX_WORMS)).find(|&i| set & 1 << i != 0 && (p - worms[i].pos).length2() < 25.0)
}

/// `CClient::ProcessShot_Beam` (`src/client/CClient_Game.cpp:926-1055`), at
/// delivery. The shooter's slot holding the beam becomes current and draws
/// for 0.3 s; then the trace from the decoded position along the integer
/// angle carves the hole or injures the worm it stops at. Unless its
/// `Damage` is -1, a part that comes within 3 px of a client bonus stops
/// there first, in an explosion of damage 0 at that point (`:986-1002`),
/// which destroys the bonus when `Bonuses` is on.
pub(crate) fn process_shot_beam(
    shot: &Shot,
    wpn: &SimWeapon,
    worms: &mut [Worm],
    ctx: &mut BeamCtx,
    settings: &SimSettings,
    now: AbsTime,
    bonuses: &mut [Bonus; MAX_BONUSES],
) {
    let Some(beam) = &wpn.beam else { return };
    let by_worm = (0..MAX_WORMS as i32).contains(&shot.worm);
    if by_worm {
        let Some(w) = worms.iter_mut().find(|w| w.id == shot.worm) else { return };
        if !w.slots.is_empty() {
            if w.current_slot().weapon != shot.weapon
                && let Some(i) = w.slots.iter().position(|s| s.weapon == shot.weapon)
            {
                w.current_weapon = i as i32;
            }
            w.current_slot_mut().last_fire = 0.3;
        }
    }
    let dir = vec_from_angle(shot.angle as f32);
    let map_min = ctx.terrain.width().min(ctx.terrain.height());
    let mut set = others(worms, shot.worm);
    let BeamCtx { terrain, holes, draws, parts } = ctx;
    // The trace only ever frees bonuses, so without one at the start no part
    // needs the 32-slot scan (a wide, long beam has many parts).
    let bonus_check = beam.damage != -1 && bonuses.iter().any(|b| b.used);
    trace(beam, shot.pos, dir, map_min, parts, |p, width| {
        if bonus_check && bonuses.iter().any(|b| b.used && (p - b.pos).length() < 3.0) {
            let mut blast = Blast {
                settings,
                terrain,
                holes,
                worms,
                draws,
                bonuses,
                now,
                wall_clock: now + TimeDiff(CLOCK_OFFSET_MS),
                physics_dt: settings.lx56_physics_dt(),
            };
            explosion(&mut blast, shot.spawn, p, 0.0, 5, shot.worm);
            return Part::Stopped;
        }
        if solid(terrain, p) {
            if beam.damage != -1 {
                let size = spread(beam.damage, beam, width);
                let d = carve_at(terrain, holes, &mut draws.events, TerrainOp::Beam, size, p);
                if by_worm {
                    increment_dirt_count(worms, shot.worm, d);
                }
            }
            return Part::Stopped;
        }
        if let Some(i) = hit(worms, set, p) {
            let damage = spread(beam.player_damage, beam, width) as f32;
            let id = worms[i].id;
            injure_worm(&mut Harm { settings, worms, draws, now }, id, damage, shot.worm);
            set &= !(1 << i);
            return Part::Stopped;
        }
        Part::Clear
    });
}

/// `CClient::DrawBeam` (`src/client/CClient_Game.cpp:590-711`) for worm `i`,
/// from `PlayerShoot` while its beam's slot counts down. Not cosmetic: it
/// carves the first dirt or rock it reaches, from the worm's current
/// position at its unrounded aim, every sub-step. A worm stops it unharmed.
/// Its explosion entities take pool slots; the beam entity
/// (`SetWormBeamEntity`) takes none and is reported as a `Beam` cosmetic.
pub(crate) fn draw_beam(worms: &mut [Worm], i: usize, m: &SimMod, ctx: &mut BeamCtx) {
    let w = &worms[i];
    if w.slots.is_empty() {
        return;
    }
    let slot = w.current_slot();
    let Some(beam) = m.weapon(slot.weapon).and_then(|wpn| wpn.beam.as_ref()) else { return };
    if slot.reloading {
        return;
    }
    let mut angle = w.angle;
    if w.face == Dir::Left {
        angle = 180.0 - angle;
    }
    let (id, pos, dir) = (w.id, w.pos, vec_from_angle(angle));
    let map_min = ctx.terrain.width().min(ctx.terrain.height());
    let mut set = others(worms, id);
    let BeamCtx { terrain, holes, draws, parts } = ctx;
    let end = trace(beam, pos, dir, map_min, parts, |p, width| {
        if solid(terrain, p) {
            if beam.damage != -1 {
                if width <= 2 {
                    draws.entity(CosmeticKind::Explosion { size: 5 }, p, Vec2::ZERO);
                }
                let size = spread(beam.damage, beam, width);
                let d = carve_at(terrain, holes, &mut draws.events, TerrainOp::Beam, size, p);
                increment_dirt_count(worms, id, d);
            }
            return Part::Stopped;
        }
        if let Some(i) = hit(worms, set, p) {
            if beam.damage != -1 {
                draws.entity(CosmeticKind::Explosion { size: 3 }, p + Vec2::new(1.0, 1.0), Vec2::ZERO);
            }
            set &= !(1 << i);
            return Part::StoppedLate;
        }
        Part::Clear
    });
    // `SetWormBeamEntity` (`:699-710`), unless the beam is pink.
    if beam.colour != Rgba::MAGENTA {
        draws.events.cosmetic(CosmeticKind::Beam { worm: id }, pos, end - pos);
    }
}

/// `CClient::ShootSpecial` (`src/client/CClient_Game.cpp:549-585`). The
/// jetpack thrusts up and throws its spray entity (one `GetRandomNum()`);
/// then the charge drains, starting a reload only below 0 (unlike
/// `WormShoot`'s `<= 0`, and ignoring `DisableWpnsWhenEmpty`).
pub(crate) fn shoot_special(w: &mut Worm, m: &SimMod, draws: &mut Draws) {
    if w.slots.is_empty() {
        return;
    }
    let Some(wpn) = m.weapon(w.current_slot().weapon) else { return };
    // tLX->fDeltaTime is FixedFrameTime (10 ms) inside the frame loop.
    let dt = TimeDiff(10).seconds();
    if wpn.special == SpecialKind::Jetpack {
        w.vel.y += -50.0 * (wpn.thrust as f32 * dt);
        let s = Vec2::new(15.0, 0.0) * draws.rand.random_num();
        let vel = s + Vec2::new(0.0, 1.0) * wpn.thrust as f32;
        draws.entity(CosmeticKind::JetpackSpray, w.pos, vel);
    }
    let slot = w.current_slot_mut();
    slot.charge -= wpn.drain / 100.0;
    if slot.charge < 0.0 {
        slot.charge = 0.0;
        slot.reloading = true;
    }
}

#[cfg(test)]
#[path = "beam_tests.rs"]
mod tests;
