//! Projectiles: OLX's `CProjectile` (`include/CProjectile.h:74-212`), its
//! container (`FastVector<CProjectile, 3000>`, `include/FastVector.h`) and
//! spawning (`Proj_SpawnInfo::apply`, `CClient::SpawnProjectile`,
//! `CProjectile::Spawn`, in [`crate::spawn`]); motion and collision are in
//! [`crate::proj_sim`].

use alloc::vec;
use alloc::vec::Vec;

use rl_math::{AbsTime, Vec2, get_fixed_random_num};

/// C's `(int)f` as x86-64 compiles it (`cvttss2si`): truncation toward zero,
/// and `INT_MIN` for NaN or a value outside `int`'s range.
pub(crate) fn c_int(f: f32) -> i32 {
    if f.is_nan() || f >= 2_147_483_648.0 || f < -2_147_483_648.0 { i32::MIN } else { f as i32 }
}

/// `cvttss2si` with a 64-bit destination: `i64::MIN` for NaN or out of range.
pub(crate) fn c_i64(f: f32) -> i64 {
    if f.is_nan() || f >= 9.223_372e18 || f < -9.223_372e18 { i64::MIN } else { f as i64 }
}

/// C's `(Uint64)f` as GCC compiles it for x86-64 without AVX-512: below
/// 2^63 (and for NaN) a 64-bit `cvttss2si`, so negative values wrap; from
/// 2^63 on, `cvttss2si(f - 2^63) ^ 2^63`, so infinity and values from 2^64
/// on give 0. `TimeDiff(float)` and `AbsTime + float` go through it.
pub(crate) fn c_u64(f: f32) -> u64 {
    const TWO_63: f32 = 9_223_372_036_854_775_808.0;
    if f >= TWO_63 { (c_i64(f - TWO_63) as u64) ^ (1 << 63) } else { c_i64(f) as u64 }
}

/// An assignment through OLX's `Attr<CVec>::operator=` (`src/game/Attr.h:194-204`),
/// which `vPos` and `vVelocity` of projectiles and worms use: it is skipped
/// when the new value compares equal, so a zero keeps its old sign (a reused
/// slot keeps the previous projectile's `-0`). `.write()` paths assign directly.
pub(crate) fn attr_set(dst: &mut Vec2, v: Vec2) {
    if !(dst.x == v.x && dst.y == v.y) {
        *dst = v;
    }
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
    pub(crate) fn calculate_check_steps(&mut self, hit_bounces: bool) {
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

    /// Slot `i`, used or not (spawning fills a slot [`get_new`](Self::get_new) returned).
    pub(crate) fn slot_mut(&mut self, i: usize) -> &mut Projectile {
        &mut self.slots[i]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn c_u64_wraps_like_gcc() {
        // Values from a GCC 15 -O2 build on x86-64 (`(uint64_t)f`).
        let cases = [
            (-1500.0, 18_446_744_073_709_550_116),
            (-0.0, 0),
            (f32::NAN, 1 << 63),
            (f32::NEG_INFINITY, 1 << 63),
            (f32::INFINITY, 0),
            (1e22, 0),
            (-1e22, 1 << 63),
            (5500.0, 5500),
            (1e19, 9_999_999_980_506_447_872),
        ];
        for (f, want) in cases {
            assert_eq!(c_u64(f), want, "{f}");
        }
    }

    #[test]
    fn attr_set_keeps_the_sign_of_zero() {
        let mut v = Vec2::new(-0.0, 0.0);
        attr_set(&mut v, Vec2::new(0.0, -0.0));
        assert_eq!(
            (v.x.to_bits(), v.y.to_bits()),
            ((-0.0f32).to_bits(), 0.0f32.to_bits()),
            "equal: not written"
        );
        attr_set(&mut v, Vec2::new(0.0, 1.0));
        assert_eq!((v.x.to_bits(), v.y), (0.0f32.to_bits(), 1.0), "one component differs: both written");
        let mut n = Vec2::new(f32::NAN, 0.0);
        attr_set(&mut n, Vec2::new(f32::NAN, 0.0));
        assert!(n.x.is_nan(), "NaN never compares equal, so it is written");
    }

    #[test]
    fn c_int_is_x86_truncation() {
        assert_eq!((c_int(2.9), c_int(-2.9), c_int(f32::NAN), c_int(3e9)), (2, -2, i32::MIN, i32::MIN));
    }
}
