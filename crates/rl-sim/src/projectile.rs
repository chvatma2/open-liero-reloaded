//! Projectiles: OLX's `CProjectile` (`include/CProjectile.h:74-212`), its
//! container (`FastVector<CProjectile, 3000>`, `include/FastVector.h`) and
//! spawning (`Proj_SpawnInfo::apply`, `CClient::SpawnProjectile`,
//! `CProjectile::Spawn`). Motion and collision come with Task 5.

use alloc::vec;
use alloc::vec::Vec;

use rl_data::mod_def::{ActionKind, ProjectileGfx};
use rl_math::{AbsTime, GlibcRand, TimeDiff, Vec2, get_fixed_random_num};

use crate::SimSettings;
use crate::mod_rt::{SimMod, SpawnInfo};
use crate::shots::Shot;
use crate::world::MAX_WORMS;

/// `(float)(PI / 180)`, as `GetVecsFromAngle` computes it (`src/common/MathLib.cpp:135-155`).
const DEG_TO_RAD: f32 = (core::f64::consts::PI / 180.0) as f32;

/// `GetVecFromAngle(yaw)` (`MathLib.cpp:135-155`): `(cosf, sinf)` of the angle in degrees.
pub(crate) fn vec_from_angle(yaw: f32) -> Vec2 {
    let a = yaw * DEG_TO_RAD;
    Vec2::new(rl_math::cosf(a), rl_math::sinf(a))
}

/// C's `(int)f` as x86-64 compiles it (`cvttss2si`): truncation toward zero,
/// and `INT_MIN` for NaN or a value outside `int`'s range.
pub(crate) fn c_int(f: f32) -> i32 {
    if f.is_nan() || f >= 2_147_483_648.0 || f < -2_147_483_648.0 { i32::MIN } else { f as i32 }
}

/// One projectile slot: OLX's `CProjectile` fields that matter to the
/// simulation. A slot keeps its old values when it is freed and reused,
/// as in OLX (`iColour` of an image projectile, `CollisionSide`).
#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub used: bool,
    /// `tProjInfo`, as an index into [`SimMod::projectiles`].
    pub proj: usize,
    pub spawn_time: AbsTime,
    pub life: f32,
    /// `fExtra`, the smoke-trail accumulator.
    pub extra: f32,
    /// `iOwner`: a worm id, or -1.
    pub owner: i32,
    /// `fSpeed`.
    pub speed: f32,
    /// `iColour` as `0xRRGGBB`.
    pub colour: u32,
    pub ignore_worm_coll_before: AbsTime,
    pub last_trail_proj: AbsTime,
    pub time_var_random: f32,
    pub pos: Vec2,
    pub vel: Vec2,
    /// `vOldPos`, the last terrain-safe position.
    pub old_pos: Vec2,
    pub rotation: f32,
    /// `(Width / 2, Height / 2)`.
    pub radius: (i32, i32),
    /// `iRandom`, the index into the fixed random table.
    pub random: i32,
    pub max_checkstep: i32,
    pub min_checkstep: i32,
    pub max_checkstep2: i32,
    pub min_checkstep2: i32,
    pub avg_checkstep: i32,
    /// `fWallshootTime`, seconds.
    pub wallshoot_time: f32,
    pub changes_speed: bool,
    pub check_speed_len: i32,
    pub collision_side: i32,
    pub frame_delta: bool,
    /// `fFrame`, the animation frame.
    pub frame: f32,
    pub frame_x: i32,
    pub first_bounce: bool,
    /// `fLastSimulationTime`, the projectile's own physics clock.
    pub last_sim_time: AbsTime,
    pub health: f32,
}

impl Default for Projectile {
    /// The `CProjectile()` constructor (`include/CProjectile.h:83-94`).
    fn default() -> Self {
        Self {
            used: false,
            proj: 0,
            spawn_time: AbsTime(0),
            life: 0.0,
            extra: 0.0,
            owner: -1,
            speed: 0.0,
            colour: 0,
            ignore_worm_coll_before: AbsTime(0),
            last_trail_proj: AbsTime(0),
            time_var_random: 0.0,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            old_pos: Vec2::ZERO,
            rotation: 0.0,
            radius: (0, 0),
            random: 0,
            max_checkstep: 0,
            min_checkstep: 0,
            max_checkstep2: 0,
            min_checkstep2: 0,
            avg_checkstep: 0,
            wallshoot_time: 0.0,
            changes_speed: false,
            check_speed_len: 0,
            collision_side: 0,
            frame_delta: false,
            frame: 0.0,
            frame_x: 0,
            first_bounce: false,
            last_sim_time: AbsTime(0),
            health: 0.0,
        }
    }
}

impl Projectile {
    /// `CProjectile::getRandomFloat` (`src/common/CProjectile.cpp:118-125`).
    pub fn random_float(&mut self) -> f32 {
        let r = get_fixed_random_num(self.random as u8);
        self.random = self.random.wrapping_add(1) % 255;
        r
    }

    /// `CProjectile::CalculateCheckSteps` (`CProjectile.cpp:130-166`).
    fn calculate_check_steps(&mut self, hit_bounces: bool) {
        self.check_speed_len = c_int(self.vel.length2());
        let (min, max, avg) = if self.check_speed_len < 14000 {
            (0, 3, 2)
        } else if self.check_speed_len < 75000 {
            (0, 4, 2)
        } else if self.check_speed_len < 250000 {
            (1, 5, 2)
        } else {
            let mut rnd = self.random % 3;
            // `SIGN(getRandomFloat())`: the macro evaluates its argument a second time unless it is > 0.
            let sign = if self.random_float() > 0.0 {
                1
            } else if self.random_float() < 0.0 {
                -1
            } else {
                0
            };
            rnd *= sign;
            if hit_bounces { (6, 2, 2) } else { (6, 9 + rnd, 6 + rnd) }
        };
        (self.min_checkstep, self.max_checkstep, self.avg_checkstep) = (min, max, avg);
        self.max_checkstep2 = max.wrapping_mul(max);
        self.min_checkstep2 = min.wrapping_mul(min);
    }
}

/// OLX's projectile container, `FastVector<CProjectile, MAX_PROJECTILES>`
/// (`include/FastVector.h:27-152`): a new projectile takes the lowest free
/// slot, and iteration visits used slots in index order up to the last used one.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectilePool {
    slots: Vec<Projectile>,
    first_unused: i32,
    last_used: i32,
}

impl ProjectilePool {
    /// `MAX_PROJECTILES` (`include/CProjectile.h:36`).
    pub const DEFAULT_CAPACITY: usize = 3000;

    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.min(i32::MAX as usize);
        Self { slots: vec![Projectile::default(); capacity], first_unused: 0, last_used: -1 }
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn size(&self) -> i32 {
        self.slots.len() as i32
    }

    fn is_used(&self, i: i32) -> bool {
        self.slots[i as usize].used
    }

    /// `getNewObj` (`FastVector.h:87-94`): the lowest free slot, or `None`
    /// when the pool is full. The caller must mark it used.
    pub fn get_new(&mut self) -> Option<usize> {
        if self.first_unused >= self.size() {
            return None;
        }
        let idx = self.first_unused;
        if idx > self.last_used {
            self.last_used = idx;
        }
        // findNewFirstUnused (FastVector.h:34-41).
        loop {
            self.first_unused += 1;
            if self.first_unused > self.last_used || self.first_unused >= self.size() {
                break;
            }
            if !self.is_used(self.first_unused) {
                break;
            }
        }
        Some(idx as usize)
    }

    /// `CProjectile::setUnused` (`src/common/CProjectile.cpp:34-39`), then
    /// `refreshObj` (`FastVector.h:97-102`).
    pub fn set_unused(&mut self, i: usize) {
        let Some(p) = self.slots.get_mut(i) else { return };
        p.used = false;
        let i = i as i32;
        if i < self.first_unused {
            self.first_unused = i;
        }
        if i >= self.last_used {
            // findNewLastUsed (FastVector.h:43-50).
            loop {
                self.last_used -= 1;
                if self.last_used < 0 || self.last_used < self.first_unused || self.is_used(self.last_used) {
                    break;
                }
            }
        }
    }

    /// `FastVector::Iterator::next` (`FastVector.h:116-125`): the next used
    /// slot after `after` (`None` starts at the beginning), reading the last
    /// used slot live, so slots taken during an iteration are visited too.
    pub fn next_used(&self, after: Option<usize>) -> Option<usize> {
        let mut i = after.map_or(0, |a| a as i32 + 1);
        while i < self.size() {
            if self.is_used(i) {
                return Some(i as usize);
            }
            i += 1;
            if i > self.last_used {
                return None;
            }
        }
        None
    }

    /// Used slots in index order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Projectile)> {
        let end = (self.last_used + 1).max(0) as usize;
        self.slots[..end].iter().enumerate().filter(|(_, p)| p.used)
    }

    pub fn get(&self, i: usize) -> Option<&Projectile> {
        self.slots.get(i).filter(|p| p.used)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Projectile> {
        self.slots.get_mut(i).filter(|p| p.used)
    }
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

/// What spawned the projectiles (`Proj_SpawnParent`). Projectile parents
/// (events, trails) come with Task 5.
pub(crate) enum SpawnParent<'a> {
    Shot(&'a mut Shot),
}

/// `Proj_SpawnInfo::apply` (`src/common/PhysicsLX56_Projectiles.cpp:562-640`)
/// with the `Proj_SpawnParent` accessors (`:495-554`).
pub(crate) fn spawn_info_apply(
    info: &SpawnInfo,
    parent: SpawnParent,
    spawn_time: AbsTime,
    ctx: &mut SpawnCtx,
) {
    let SpawnParent::Shot(shot) = parent;
    let heading = if info.use_angle { shot.angle as f32 } else { 0.0 };
    for _ in 0..info.amount {
        let sprd = if info.use_parent_velocity_for_spread {
            shot.worm_vel * info.parent_vel_spread_factor
        } else {
            let r = get_fixed_random_num(shot.random as u8);
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
        shot.random = (shot.random + 1) % 255;
        let speed_var_vec = if info.use_special11_vec_for_speed_var { Vec2::new(1.0, 1.0) } else { sprd };
        let r = get_fixed_random_num(shot.random as u8);
        let parent_vel = if info.add_parent_vel {
            // MatrixD2<float>(f) * v (include/CVec.h:214-216): v1 = (f, 0), v2 = (0, f).
            let (f, v) = (info.parent_vel_factor, shot.worm_vel);
            Vec2::new(f * v.x + 0.0 * v.y, 0.0 * v.x + f * v.y)
        } else {
            Vec2::ZERO
        };
        let v = sprd * info.speed as f32 + speed_var_vec * info.speed_var * r + parent_vel;
        shot.random = (shot.random * 5) % 255;
        // GetPhysicsTime() + 0.1f: AbsTime + (Uint64)(0.1f * 1000.0f).
        let ignore = ctx.now + TimeDiff((0.1f32 * 1000.0) as u64);
        let random = shot.random;
        let dir = vec_from_angle(shot.angle as f32);
        // PosDiff is (0, 0) for LX56 mods, and SnapToGrid unused.
        let pos = shot.pos + dir * ctx.settings.shoot_spawn_distance + Vec2::ZERO;
        spawn_projectile(ctx, info.proj, pos, v, rot, shot.worm, random, spawn_time, ignore);
        shot.random = (shot.random + 1) % 255;
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
    let p = &mut ctx.pool.slots[i];
    p.owner = if (0..MAX_WORMS as i32).contains(&owner) { owner } else { -1 };
    p.used = true;
    p.proj = proj;
    p.life = 0.0;
    p.extra = 0.0;
    p.old_pos = pos;
    p.pos = pos;
    p.vel = vel;
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

    #[test]
    fn pool_takes_the_lowest_free_slot() {
        let mut pool = ProjectilePool::new(4);
        let take = |pool: &mut ProjectilePool| {
            let i = pool.get_new();
            if let Some(i) = i {
                pool.slots[i].used = true;
            }
            i
        };
        assert_eq!([take(&mut pool), take(&mut pool), take(&mut pool)], [Some(0), Some(1), Some(2)]);
        pool.set_unused(1);
        pool.set_unused(0);
        assert_eq!(pool.iter().map(|(i, _)| i).collect::<Vec<_>>(), [2]);
        assert_eq!(take(&mut pool), Some(0));
        assert_eq!(take(&mut pool), Some(1));
        assert_eq!(take(&mut pool), Some(3));
        assert_eq!(take(&mut pool), None, "full");
        pool.set_unused(3);
        pool.set_unused(2);
        assert_eq!(pool.last_used, 1, "the last used slot moves down past freed ones");
        assert_eq!(take(&mut pool), Some(2));
        assert_eq!(pool.next_used(None), Some(0));
        assert_eq!(pool.next_used(Some(2)), None);
    }

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

    #[test]
    fn c_int_is_x86_truncation() {
        assert_eq!((c_int(2.9), c_int(-2.9), c_int(f32::NAN), c_int(3e9)), (2, -2, i32::MIN, i32::MIN));
    }
}
