//! Weapon slots and firing, run in every physics sub-step for each worm in
//! OLX's order (`CClient::Simulation`, `src/client/CClient_Game.cpp:97-176`):
//! [`simulate_worm_weapon`] at the end of the worm's physics, then the
//! server's [`worm_shoot`], then the client's [`player_shoot`]. In a local
//! game both work on the same slots, the server first
//! (`docs/porting/olx-notes/weapons-firing.md` §1, §3).

use rl_data::level::Material;
use rl_math::{GlibcRand, TimeDiff};
use rl_terrain::Terrain;

use crate::SimSettings;
use crate::beam::{BeamCtx, draw_beam, shoot_special};
use crate::events::CosmeticKind;
use crate::mod_rt::{SimMod, SimWeapon, WeaponType};
use crate::rng::Draws;
use crate::shots::ShootList;
use crate::spawn::vec_from_angle;
use crate::worm::{Dir, Slot, Worm};

/// `simulateWormWeapon` (`src/common/PhysicsLX56.cpp:471-498`): only the
/// current slot counts down its rate of fire and reloads.
pub(crate) fn simulate_worm_weapon(w: &mut Worm, mut dt: f32, s: &SimSettings, m: &SimMod) {
    if w.slots.is_empty() {
        return;
    }
    if s.game_speed_only_for_projs {
        dt *= s.game_speed;
    }
    let slot = w.current_slot_mut();
    let Some(wpn) = m.weapon(slot.weapon) else { return };
    if slot.last_fire > 0.0 {
        slot.last_fire -= dt;
    }
    if slot.reloading {
        if s.loading_time == 0 {
            slot.charge = 1.0;
        } else {
            slot.charge += dt * (wpn.recharge * (1.0 / (s.loading_time as f32 * 0.01)));
        }
        if slot.charge >= 1.0 {
            slot.charge = 1.0;
            slot.reloading = false;
        }
    }
}

/// What the server's fire code needs besides the worm.
pub(crate) struct ShootCtx<'a> {
    pub sim_mod: &'a SimMod,
    pub settings: &'a SimSettings,
    pub list: &'a mut ShootList,
    pub rand: &'a mut GlibcRand,
    /// `game.serverTime()`: `serverFrame * 10` ms.
    pub server_time: TimeDiff,
}

/// The charge drain at the end of `GameServer::WormShoot` (`CServer_Game.cpp:395-410`, `450-465`).
fn drain(slot: &mut Slot, wpn: &SimWeapon, s: &SimSettings) {
    slot.charge -= wpn.drain / 100.0;
    if slot.charge <= 0.0 {
        slot.charge = 0.0;
        if s.disable_wpns_when_empty {
            slot.weapon = -1;
        } else {
            slot.reloading = true;
        }
    }
}

/// `GameServer::WormShoot` (`src/server/CServer_Game.cpp:366-466`) for a
/// worm that holds fire and is alive: the rate-of-fire gate, one shot
/// (one `rand()`) for the local client's list, then the drain.
pub(crate) fn worm_shoot(w: &mut Worm, ctx: &mut ShootCtx) {
    if w.slots.is_empty() {
        return;
    }
    let slot = w.current_slot();
    if slot.reloading || slot.last_fire > 0.0 {
        return;
    }
    let Some(wpn) = ctx.sim_mod.weapon(slot.weapon) else { return };
    let weapon_id = slot.weapon;
    w.current_slot_mut().last_fire = wpn.rof;
    if wpn.kind == WeaponType::Projectile || wpn.kind == WeaponType::Beam {
        let mut angle = w.angle;
        if w.face == Dir::Left {
            angle = 180.0 - angle;
        }
        if angle < 0.0 {
            angle += 360.0;
        }
        if angle > 360.0 {
            angle -= 360.0;
        }
        if angle == 360.0 {
            angle = 0.0;
        }
        let mut speed = 0.0;
        if wpn.kind == WeaponType::Projectile {
            // NormalizeVector (MathLib.cpp:109-120) returns the length.
            speed = libm::sqrtf(w.vel.x * w.vel.x + w.vel.y * w.vel.y);
        }
        let angle = crate::projectile::c_int(angle);
        let relative = ctx.settings.proj_relative_vel;
        ctx.list.add(weapon_id, ctx.server_time, speed, angle, w, relative, ctx.rand);
    }
    drain(w.current_slot_mut(), wpn, ctx.settings);
}

/// `CClient::PlayerShoot` (`src/client/CClient_Game.cpp:496-544`) for worm
/// `i`, which holds fire and is alive. A beam draws ([`draw_beam`]) while
/// its slot counts down by the frame time. Otherwise, after the server's
/// shot it only re-sets `LastFire` (a no-op when ROF > 0), so a special
/// weapon is shot here only when its ROF is 0 or less (`weapons-firing.md` §7).
pub(crate) fn player_shoot(worms: &mut [Worm], i: usize, m: &SimMod, ctx: &mut BeamCtx) {
    let w = &mut worms[i];
    if w.slots.is_empty() {
        return;
    }
    let slot = w.current_slot_mut();
    if slot.reloading {
        return;
    }
    let Some(wpn) = m.weapon(slot.weapon) else { return };
    if wpn.kind == WeaponType::Beam {
        if slot.last_fire > 0.0 {
            draw_beam(worms, i, m, ctx);
            // tLX->fDeltaTime is FixedFrameTime (10 ms) inside the frame loop.
            worms[i].current_slot_mut().last_fire -= TimeDiff(10).seconds();
        }
        return;
    }
    if slot.last_fire > 0.0 {
        return;
    }
    slot.last_fire = wpn.rof;
    if wpn.kind == WeaponType::Special {
        shoot_special(w, m, ctx.draws);
    }
}

/// `CClient::LaserSight` (`src/client/CClient_Game.cpp:751-808`), called for
/// a living worm whose current weapon has a laser sight and is not reloading
/// (`:179-181`). Its line trace only marks bot targets; what the simulation
/// keeps is the `ENT_LASERSIGHT` entity it spawns every sub-step (the worm is
/// visible to everybody outside Hide and Seek), which fills the dedicated
/// server's entity pool (spec §18 D4) and is reported with the aim direction.
pub(crate) fn laser_sight(worms: &[Worm], i: usize, m: &SimMod, terrain: &Terrain, draws: &mut Draws) {
    let w = &worms[i];
    if w.slots.is_empty() {
        return;
    }
    let slot = w.current_slot();
    if m.weapon(slot.weapon).is_some_and(|wpn| wpn.laser_sight) && !slot.reloading {
        // `:754-765`: the aim, mirrored when facing left, in (-180, 180].
        let mut angle = w.angle;
        if w.face == Dir::Left {
            angle = 180.0 - angle;
        }
        // Aims are clamped to [-90, 90], so these loops end (infinity is left alone).
        while angle.is_finite() && angle > 180.0 {
            angle -= 360.0;
        }
        while angle.is_finite() && angle < -180.0 {
            angle += 360.0;
        }
        let dir = vec_from_angle(angle);
        // `:768-804`: 3 px steps until a pixel that stops particles or another
        // living worm within 5 px; `i` is what the entity carries.
        let (mut pos, mut len) = (w.pos, 0i16);
        let (tw, th) = (terrain.width().max(1) as i64, terrain.height().max(1) as i64);
        while len < 9999 {
            let (x, y) = ((pos.x as i64).rem_euclid(tw), (pos.y as i64).rem_euclid(th));
            if terrain.material(x, y).unwrap_or(Material::Rock) != Material::Background {
                break;
            }
            let hit = worms.iter().enumerate().any(|(j, o)| {
                let d = pos - o.pos;
                j != i && o.alive && d.x * d.x + d.y * d.y < 25.0
            });
            if hit {
                break;
            }
            pos += dir * 3.0;
            len += 3;
        }
        draws.entity(CosmeticKind::LaserSight { len }, w.pos, dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
    use alloc::collections::BTreeMap;
    use alloc::vec;
    use alloc::vec::Vec;
    use rl_data::level::{Material, MaterialGrid};
    use rl_math::{AbsTime, Mt19937, Vec2};
    use rl_terrain::Terrain;

    #[test]
    fn slot_charge_drain_and_reload() {
        // A weapon that empties in two shots: ROF 15 ms, drain 50 %, recharge 1.
        let mut weapon = projectile_weapon("gun", "pellet");
        (weapon.rof, weapon.drain, weapon.recharge) = (0.015, 50.0, 1.0);
        let m = SimMod::compile(
            &mod_def(vec![weapon], vec![("pellet", pixel_projectile(None))]),
            &BTreeMap::new(),
        )
        .unwrap();
        let s = SimSettings::lx56();
        let mut w = Worm::spawned(0, Vec2::new(10.0, 10.0), AbsTime(0), &s);
        w.slots = vec![Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 }; 5];
        let mut list = ShootList::new();
        let mut rand = GlibcRand::new(1);
        let dt = TimeDiff(11).seconds();
        let mut terrain = Terrain::from_grid(&MaterialGrid::new(1, 1, Material::Background));
        let mut draws = Draws::new(GlibcRand::new(1), Mt19937::new(1));
        let mut parts = Vec::new();
        let mut step = |w: &mut Worm| {
            simulate_worm_weapon(w, dt, &s, &m);
            let mut ctx = ShootCtx {
                sim_mod: &m,
                settings: &s,
                list: &mut list,
                rand: &mut rand,
                server_time: TimeDiff(0),
            };
            worm_shoot(w, &mut ctx);
            let mut beam =
                BeamCtx { terrain: &mut terrain, holes: &[], draws: &mut draws, parts: &mut parts };
            player_shoot(core::slice::from_mut(w), 0, &m, &mut beam);
            *w.current_slot()
        };
        // Sub-step 1 fires: LastFire = ROF, half the charge gone.
        assert_eq!(step(&mut w), Slot { weapon: 0, charge: 0.5, reloading: false, last_fire: 0.015 });
        // Sub-step 2: 0.015f - 0.011f = 0.003999999f, still waiting.
        assert_eq!(step(&mut w).last_fire, 0.015f32 - dt);
        assert_eq!(
            step(&mut w).last_fire.to_bits(),
            0.015f32.to_bits(),
            "the negative residue is overwritten"
        );
        assert_eq!(w.current_slot().charge, 0.0);
        assert!(w.current_slot().reloading, "empty after the second shot");
        // Reloading adds dt * Recharge * (1 / (LoadingTime * 0.01f)) per sub-step:
        // 90 sub-steps from 0 give 0.98999923f.
        for _ in 0..90 {
            step(&mut w);
        }
        assert_eq!(w.current_slot().charge, 0.989_999_23);
        assert!(w.current_slot().reloading);
        // The 91st ends the reload, so the server fires in the same sub-step.
        let slot = step(&mut w);
        assert_eq!((slot.charge, slot.reloading), (0.5, false));
        // Only the current slot reloads; the others never fired.
        assert!(w.slots[1..].iter().all(|s| s.charge == 1.0));
        assert_eq!(rand.calls(), 3, "one rand() per shot");
    }

    #[test]
    fn a_left_facing_worm_fires_at_180_minus_its_angle() {
        let m = SimMod::compile(
            &mod_def(vec![projectile_weapon("gun", "pellet")], vec![("pellet", pixel_projectile(None))]),
            &BTreeMap::new(),
        )
        .unwrap();
        let s = SimSettings::lx56();
        let mut list = ShootList::new();
        let mut rand = GlibcRand::new(1);
        for (face, angle, expected) in
            [(Dir::Left, 60.0, 120), (Dir::Right, -0.5, 359), (Dir::Left, -90.0, 270)]
        {
            let mut w = Worm::spawned(0, Vec2::new(10.0, 10.0), AbsTime(0), &s);
            w.slots = vec![Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 }];
            (w.face, w.angle) = (face, angle);
            let mut ctx = ShootCtx {
                sim_mod: &m,
                settings: &s,
                list: &mut list,
                rand: &mut rand,
                server_time: TimeDiff(0),
            };
            worm_shoot(&mut w, &mut ctx);
            assert_eq!(list.last().map(|s| s.angle), Some(expected));
        }
    }

    #[test]
    fn the_laser_sight_event_carries_the_trace_length() {
        let mut weapon = projectile_weapon("gun", "pellet");
        weapon.laser_sight = true;
        let m = SimMod::compile(
            &mod_def(vec![weapon], vec![("pellet", pixel_projectile(None))]),
            &BTreeMap::new(),
        )
        .unwrap();
        let s = SimSettings::lx56();
        // 100 x 10 background with rock at x = 40.
        let mut cells = vec![Material::Background; 1000];
        for y in 0..10 {
            cells[y * 100 + 40] = Material::Rock;
        }
        let grid = MaterialGrid::from_cells(100, 10, cells).unwrap();
        let terrain = Terrain::from_grid(&grid);
        let worm = |x: f32| {
            let mut w = Worm::spawned(0, Vec2::new(x, 5.0), AbsTime(0), &s);
            w.slots = vec![Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 }];
            (w.face, w.angle) = (Dir::Right, 0.0);
            w
        };
        let len = |worms: &[Worm]| {
            let mut draws = Draws::new(GlibcRand::new(1), Mt19937::new(1));
            laser_sight(worms, 0, &m, &terrain, &mut draws);
            let lens: Vec<i16> = draws
                .events
                .iter()
                .filter_map(|e| match e {
                    crate::Event::SpawnCosmetic { kind: CosmeticKind::LaserSight { len }, .. } => Some(*len),
                    _ => None,
                })
                .collect();
            assert_eq!(lens.len(), 1);
            lens[0]
        };
        // From x = 10: 10, 13, ..., 40 is the first rock pixel, 10 steps.
        assert_eq!(len(&[worm(10.0)]), 30);
        // A worm at x = 25 stops the trace at x = 22 (within 5 px): 4 steps.
        let mut other = worm(25.0);
        other.alive = true;
        assert_eq!(len(&[worm(10.0), other]), 12);
        // A dead worm does not.
        let mut dead = worm(25.0);
        dead.alive = false;
        assert_eq!(len(&[worm(10.0), dead]), 30);
    }
}
