//! Spawning projectiles: `Proj_SpawnInfo::apply` with a shot or a projectile
//! parent (`src/common/PhysicsLX56_Projectiles.cpp:495-640`), then
//! `CClient::SpawnProjectile` and `CProjectile::Spawn`.

use rl_data::mod_def::{ActionKind, ProjectileGfx};
use rl_math::{AbsTime, GlibcRand, TimeDiff, Vec2, atan2f, fmod, get_fixed_random_num, normalize_vector};

use crate::SimSettings;
use crate::mod_rt::{SimMod, SpawnInfo};
use crate::projectile::{Projectile, ProjectilePool, attr_set, c_int};
use crate::shots::Shot;
use crate::world::MAX_WORMS;

/// `(float)(PI / 180)`, as `GetVecsFromAngle` computes it (`src/common/MathLib.cpp:135-155`).
const DEG_TO_RAD: f32 = (core::f64::consts::PI / 180.0) as f32;

/// `GetVecFromAngle(yaw)` (`MathLib.cpp:135-155`): `(cosf, sinf)` of the angle in degrees.
pub(crate) fn vec_from_angle(yaw: f32) -> Vec2 {
    let a = yaw * DEG_TO_RAD;
    Vec2::new(rl_math::cosf(a), rl_math::sinf(a))
}

/// What spawning needs besides the parent.
pub(crate) struct SpawnCtx<'a> {
    pub sim_mod: &'a SimMod,
    pub pool: &'a mut ProjectilePool,
    pub rand: &'a mut GlibcRand,
    pub settings: &'a SimSettings,
    /// `GetPhysicsTime()`: the frame clock (`game.simulationAbsTime()`).
    pub now: AbsTime,
}

/// What spawned the projectiles (`Proj_SpawnParent`).
pub(crate) enum SpawnParent<'a> {
    Shot(&'a mut Shot),
    /// A projectile (an event or a trail). Spawning advances its `iRandom`.
    Proj(&'a mut Projectile),
}

impl SpawnParent<'_> {
    /// `Proj_SpawnParent::angle` (`:542-556`): for a projectile, its heading
    /// in degrees from its velocity.
    fn angle(&self) -> f32 {
        match self {
            SpawnParent::Shot(s) => s.angle as f32,
            SpawnParent::Proj(p) => {
                let mut v = p.vel;
                normalize_vector(&mut v);
                // `-atan2(v.x, v.y) * (180.0f / PI)`: a float atan2 times a double.
                let mut heading =
                    (-atan2f(v.x, v.y) as f64 * (180.0f32 as f64 / core::f64::consts::PI)) as f32;
                heading += 90.0;
                fmod(&mut heading, 360.0);
                heading
            }
        }
    }

    /// `fixedRandomFloat` (`:513-520`): a projectile parent advances its index.
    fn fixed_random_float(&mut self) -> f32 {
        match self {
            SpawnParent::Shot(s) => get_fixed_random_num(s.random as u8),
            SpawnParent::Proj(p) => p.random_float(),
        }
    }

    fn velocity(&self) -> Vec2 {
        match self {
            SpawnParent::Shot(s) => s.worm_vel,
            SpawnParent::Proj(p) => p.vel,
        }
    }
}

/// `Proj_SpawnInfo::apply` (`src/common/PhysicsLX56_Projectiles.cpp:562-640`)
/// with the `Proj_SpawnParent` accessors (`:495-556`).
pub(crate) fn spawn_info_apply(
    info: &SpawnInfo,
    mut parent: SpawnParent,
    spawn_time: AbsTime,
    ctx: &mut SpawnCtx,
) {
    let heading = if info.use_angle { parent.angle() } else { 0.0 };
    for _ in 0..info.amount {
        let sprd = if info.use_parent_velocity_for_spread {
            parent.velocity() * info.parent_vel_spread_factor
        } else {
            let r = parent.fixed_random_float();
            vec_from_angle(info.angle as f32 + heading + r * info.spread)
        };
        let mut rot = 0;
        if info.use_random_rot
            && let Some(inc) = ctx.sim_mod.projectiles()[info.proj].rotation()
        {
            // OLX sets a zero RotIncrement to 1 in the mod itself; only this reads it.
            let inc = if inc == 0 { 1 } else { inc };
            rot = ctx.rand.random_int((360 / inc).wrapping_abs()).wrapping_mul(inc);
        }
        if let SpawnParent::Shot(s) = &mut parent {
            s.random = (s.random + 1) % 255;
        }
        let speed_var_vec = if info.use_special11_vec_for_speed_var { Vec2::new(1.0, 1.0) } else { sprd };
        let r = parent.fixed_random_float();
        let parent_vel = if info.add_parent_vel {
            // MatrixD2<float>(f) * v (include/CVec.h:214-216): v1 = (f, 0), v2 = (0, f).
            let (f, v) = (info.parent_vel_factor, parent.velocity());
            Vec2::new(f * v.x + 0.0 * v.y, 0.0 * v.x + f * v.y)
        } else {
            Vec2::ZERO
        };
        let v = sprd * info.speed as f32 + speed_var_vec * info.speed_var * r + parent_vel;
        // PosDiff is (0, 0) for LX56 mods, and SnapToGrid unused.
        let (pos, ignore, random, owner) = match &mut parent {
            SpawnParent::Shot(s) => {
                s.random = (s.random * 5) % 255;
                // GetPhysicsTime() + 0.1f: AbsTime + (Uint64)(0.1f * 1000.0f).
                let ignore = ctx.now + TimeDiff((0.1f32 * 1000.0) as u64);
                let dir = vec_from_angle(s.angle as f32);
                (s.pos + dir * ctx.settings.shoot_spawn_distance + Vec2::ZERO, ignore, s.random, s.worm)
            }
            SpawnParent::Proj(p) => {
                (p.pos + Vec2::ZERO, p.ignore_worm_coll_before, p.random.wrapping_add(1), p.owner)
            }
        };
        spawn_projectile(ctx, info.proj, pos, v, rot, owner, random, spawn_time, ignore);
        if let SpawnParent::Shot(s) = &mut parent {
            s.random = (s.random + 1) % 255;
        }
    }
}

/// `CClient::SpawnProjectile` (`src/client/CClient_Game.cpp:716-730`) and
/// `CProjectile::Spawn` (`src/common/CProjectile.cpp:43-112`). A full pool
/// drops the projectile; the caller's draws have happened already.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_projectile(
    ctx: &mut SpawnCtx,
    proj: usize,
    pos: Vec2,
    vel: Vec2,
    rot: i32,
    owner: i32,
    random: i32,
    time: AbsTime,
    ignore: AbsTime,
) {
    let Some(i) = ctx.pool.get_new() else { return };
    let info = &ctx.sim_mod.projectiles()[proj];
    let def = &info.def;
    let p = ctx.pool.slot_mut(i);
    p.owner = if (0..MAX_WORMS as i32).contains(&owner) { owner } else { -1 };
    p.used = true;
    p.proj = proj;
    p.life = 0.0;
    p.extra = 0.0;
    p.old_pos = pos;
    attr_set(&mut p.pos, pos);
    attr_set(&mut p.vel, vel);
    p.rotation = rot as f32;
    p.radius = (def.width / 2, def.height / 2);
    p.health = 100.0;
    p.last_trail_proj = AbsTime(0);
    p.random = random % 255;
    p.frame_x = 0;
    p.ignore_worm_coll_before = ignore;
    p.time_var_random = get_fixed_random_num(p.random as u8);
    p.last_sim_time = time;
    p.spawn_time = time;
    p.speed = vel.length();
    let hit_bounces = def.hit.kind == ActionKind::Bounce;
    p.calculate_check_steps(hit_bounces);
    p.frame = 0.0;
    p.frame_delta = true;
    p.first_bounce = true;
    if let ProjectileGfx::Pixel { colours } = &def.gfx {
        p.colour = if colours.is_empty() {
            0
        } else {
            let c = ctx.rand.random_int(colours.len() as i32 - 1);
            let c = colours[c.clamp(0, colours.len() as i32 - 1) as usize];
            (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
        };
    }
    p.wallshoot_time = if ctx.settings.lx56_wall_shooting { 0.011 + p.random_float() / 1000.0 } else { 0.0 };
    p.changes_speed = c_int(def.dampening) == 1 && (!hit_bounces || c_int(def.hit.bounce_coeff) == 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
    use alloc::collections::BTreeMap;
    use alloc::vec;
    use alloc::vec::Vec;

    #[test]
    fn spawn_info_random_index_chain() {
        // weapons-firing.md §5.2: per projectile, spread reads T[n], then
        // n = (n+1)%255, speed variation reads T[n], n = (n*5)%255 is the
        // projectile's iRandom, and the next projectile starts at (n+1)%255.
        let mut spawn = crate::mod_rt::tests::weapon_spawn("pellet");
        spawn.amount = 3;
        spawn.speed_var = 10.0;
        let mut weapon = projectile_weapon("gun", "pellet");
        if let rl_data::mod_def::WeaponKind::Projectile { spawn: s, .. } = &mut weapon.kind {
            *s = spawn;
        }
        let def = mod_def(vec![weapon], vec![("pellet", pixel_projectile(None))]);
        let m = SimMod::compile(&def, &BTreeMap::new()).unwrap();
        let mut settings = SimSettings::lx56();
        settings.lx56_wall_shooting = false;
        let mut pool = ProjectilePool::new(10);
        let mut rand = GlibcRand::new(1);
        let mut shot = Shot { random: 250, angle: 0, pos: Vec2::new(100.0, 50.0), ..Shot::default() };
        let mut ctx = SpawnCtx {
            sim_mod: &m,
            pool: &mut pool,
            rand: &mut rand,
            settings: &settings,
            now: AbsTime(500),
        };
        let info = m.weapons()[0].spawn.clone().unwrap();
        spawn_info_apply(&info, SpawnParent::Shot(&mut shot), AbsTime(490), &mut ctx);
        // n: 250 -> 251 -> 251*5%255 = 235 (proj 0) -> 236 -> 237 -> 237*5%255 = 165 (proj 1)
        //   -> 166 -> 167 -> 167*5%255 = 70 (proj 2) -> 71.
        assert_eq!(shot.random, 71);
        let randoms: Vec<_> = pool.iter().map(|(_, p)| p.random).collect();
        assert_eq!(randoms, [235, 165, 70]);
        let p0 = pool.get(0).unwrap();
        let sprd = vec_from_angle(0.0 + 0.0 + get_fixed_random_num(250) * 3.0);
        let v = sprd * 250.0 + sprd * 10.0 * get_fixed_random_num(251) + Vec2::new(0.0, 0.0);
        assert_eq!((p0.vel.x.to_bits(), p0.vel.y.to_bits()), (v.x.to_bits(), v.y.to_bits()));
        assert_eq!(p0.pos, Vec2::new(108.0, 50.0));
        assert_eq!(
            (p0.spawn_time, p0.last_sim_time, p0.ignore_worm_coll_before),
            (AbsTime(490), AbsTime(490), AbsTime(600))
        );
        assert_eq!(p0.time_var_random, get_fixed_random_num(235));
        // Pixel projectiles draw their colour: one rand() each, nothing else here.
        assert_eq!(rand.calls(), 3);
    }
}
