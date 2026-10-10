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
    /// Children computed in full ([`crate::rng::Draws::spawn_work`]).
    pub work: &'a mut u64,
    /// [`crate::rng::Draws::rot_bumped`].
    pub rot_bumped: &'a mut [bool],
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
///
/// Once the pool is full, the remaining children cannot spawn (nothing frees
/// a slot during `apply`), and [`skip_children`] advances only what OLX's
/// loop advances for them, so a hostile mod's spawn costs no more than the
/// slots it can fill (Review Focus 2).
pub(crate) fn spawn_info_apply(
    info: &SpawnInfo,
    mut parent: SpawnParent,
    spawn_time: AbsTime,
    ctx: &mut SpawnCtx,
) {
    let heading = if info.use_angle { parent.angle() } else { 0.0 };
    let rot_increment =
        if info.use_random_rot { ctx.sim_mod.projectiles()[info.proj].rotation() } else { None };
    if rot_increment == Some(0)
        && info.amount > 0
        && let Some(bumped) = ctx.rot_bumped.get_mut(info.proj)
    {
        // The first child sets the mod's `RotIncrement` to 1 (`:590-593`),
        // even one the full pool then drops.
        *bumped = true;
    }
    for i in 0..info.amount {
        if ctx.pool.is_full() {
            skip_children(info, &mut parent, info.amount.abs_diff(i), rot_increment, ctx.rand);
            return;
        }
        *ctx.work = ctx.work.wrapping_add(1);
        let sprd = if info.use_parent_velocity_for_spread {
            parent.velocity() * info.parent_vel_spread_factor
        } else {
            let r = parent.fixed_random_float();
            vec_from_angle(info.angle as f32 + heading + r * info.spread)
        };
        let mut rot = 0;
        if let Some(inc) = rot_increment {
            // OLX sets a zero RotIncrement to 1 in the mod itself (`:592-593`),
            // for good; `CProjectile::Draw` (`CProjectile.cpp:291`) reads it
            // too. The SimMod stays as loaded (docs/porting/STATUS.md).
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

/// The last `n` children of [`spawn_info_apply`] when the pool has no slot:
/// what OLX's loop body does for each (`PhysicsLX56_Projectiles.cpp:577-638`)
/// before `CClient::SpawnProjectile` returns at `getNewObj() == NULL`
/// (`src/client/CClient_Game.cpp:718-724`), without the vectors. The colour
/// and wall-shooting draws are made in `CProjectile::Spawn`, so never here.
/// - The rotation's `GetRandomInt` (one `rand()`), whose value is unused.
/// - A shot parent's `nRandom`: `+1`, `*5`, `+1`, each `% 255`.
/// - A projectile parent's `iRandom`: one `getRandomFloat()` for the spread
///   (unless `UseParentVelocityForSpread`) and one for the speed variation,
///   each `iRandom = (iRandom + 1) % 255`, in closed form.
fn skip_children(
    info: &SpawnInfo,
    parent: &mut SpawnParent,
    n: u32,
    rot_increment: Option<i32>,
    rand: &mut GlibcRand,
) {
    if rot_increment.is_some() {
        for _ in 0..n {
            rand.next();
        }
    }
    match parent {
        SpawnParent::Shot(s) => {
            for _ in 0..n {
                s.random = (s.random + 1) % 255;
                s.random = (s.random * 5) % 255;
                s.random = (s.random + 1) % 255;
            }
        }
        SpawnParent::Proj(p) => {
            let per_child = if info.use_parent_velocity_for_spread { 1 } else { 2 };
            p.random = advance_random(p.random, per_child * n as u64);
        }
    }
}

/// `k` times `r = (r + 1) % 255` (`CProjectile::getRandomFloat`, C's `%`).
fn advance_random(r: i32, k: u64) -> i32 {
    if k == 0 {
        return r;
    }
    // After one step, |r| < 255: a negative r counts up to 0, then wraps at 255.
    let t = (r.wrapping_add(1) % 255) as i64 + (k - 1) as i64;
    if t <= 0 { t as i32 } else { (t % 255) as i32 }
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
#[path = "spawn_tests.rs"]
mod tests;
