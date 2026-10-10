//! Weapon slots and firing, run in every physics sub-step for each worm in
//! OLX's order (`CClient::Simulation`, `src/client/CClient_Game.cpp:97-176`):
//! [`simulate_worm_weapon`] at the end of the worm's physics, then the
//! server's [`worm_shoot`], then the client's [`player_shoot`]. In a local
//! game both work on the same slots, the server first
//! (`docs/porting/olx-notes/weapons-firing.md` §1, §3).

use rl_math::{GlibcRand, TimeDiff};

use crate::SimSettings;
use crate::mod_rt::{SimMod, SimWeapon, WeaponType};
use crate::rng::Draws;
use crate::shots::ShootList;
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

/// `CClient::PlayerShoot` (`src/client/CClient_Game.cpp:496-544`) for a worm
/// that holds fire and is alive. After the server's shot it only re-sets
/// `LastFire` (a no-op when ROF > 0). A beam's slot counts down by the
/// frame time while the beam draws; `DrawBeam` and `ShootSpecial` come with Task 8.
pub(crate) fn player_shoot(w: &mut Worm, m: &SimMod) {
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
            // tLX->fDeltaTime is FixedFrameTime (10 ms) inside the frame loop.
            slot.last_fire -= TimeDiff(10).seconds();
        }
        return;
    }
    if slot.last_fire > 0.0 {
        return;
    }
    slot.last_fire = wpn.rof;
}

/// `CClient::LaserSight` (`src/client/CClient_Game.cpp:751-808`), called for
/// a living worm whose current weapon has a laser sight and is not reloading
/// (`:179-181`). Its line trace only marks bot targets; what the simulation
/// keeps is the `ENT_LASERSIGHT` entity it spawns every sub-step (the worm is
/// visible to everybody outside Hide and Seek), which fills the dedicated
/// server's entity pool (spec §18 D4).
pub(crate) fn laser_sight(w: &Worm, m: &SimMod, draws: &mut Draws) {
    if w.slots.is_empty() {
        return;
    }
    let slot = w.current_slot();
    if m.weapon(slot.weapon).is_some_and(|wpn| wpn.laser_sight) && !slot.reloading {
        draws.spawn_entity();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
    use alloc::collections::BTreeMap;
    use alloc::vec;
    use rl_math::{AbsTime, Vec2};

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
            player_shoot(w, &m);
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
}
