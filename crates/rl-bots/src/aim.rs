//! Aim (notes §1.6, spec §20 F2): OLX's direct aim and its solver, the
//! ballistic solver bots use by default, and `AI_SetAim`'s facing cooldown
//! and difficulty noise.
//!
//! Angles are OLX's worm angles: degrees, negative up, relative to the side
//! the worm faces (`Worm::angle`), from -90 to 60 (90 with `FullAimAngle`).

use rl_math::{AbsTime, Vec2, normalize_vector};
use rl_sim::{Dir, SimSettings, Worm};

use crate::{BotRng, DifficultyParams};

const R2D: f32 = 180.0 / core::f32::consts::PI;

fn max_angle(s: &SimSettings) -> f32 {
    if s.full_aim_angle { 90.0 } else { 60.0 }
}

/// `AI_SetAim`'s side and angle for aiming straight at `to` from `from`
/// (`src/common/CWormBot.cpp:1651-1678`), before the difficulty's offset.
pub fn olx_direct(from: Vec2, to: Vec2) -> (Dir, f32) {
    let mut d = to - from;
    normalize_vector(&mut d);
    let side = if d.x < 0.0 { Dir::Left } else { Dir::Right };
    let mut ang = libm::atan2f(d.x, d.y) * R2D;
    if side == Dir::Left {
        ang += 90.0;
    } else {
        ang = -ang + 90.0;
    }
    (side, ang.max(-90.0))
}

/// `AI_GetAimingAngle(v, g, x, y)` (`CWormBot.cpp:1896-1960`), which OLX
/// only uses to decide whether a shot can reach (its angle is discarded):
/// `y` is up, and the angle comes back in OLX's convention but with the
/// wrong sign for level targets, and out-of-reach targets pass (`fabs`).
pub fn olx_aiming_angle(v: f32, g: f32, x: f32, mut y: f32, s: &SimSettings) -> (bool, f32) {
    let (v2, x2, g2, y2) = (v * v, x * x, g * g, y * y);
    if libm::fabsf(y) < 3.0 && y <= 0.0 {
        y = -y;
    }
    let tmp1 = libm::fabsf(-2.0 * v2 * y * g - g2 * x2 + v2 * v2);
    let tmp2 = -x2 * libm::sqrtf(tmp1) + x2 * y * g + v2 * (x2 + 2.0 * y2);
    if tmp2 < 0.0 {
        return (false, 0.0);
    }
    let tmp3 = libm::sqrtf(tmp1) - y * g + v2;
    if tmp3 <= 0.0 || x == 0.0 {
        return (false, 60.0);
    }
    let mut a = libm::atanf(libm::sqrtf(tmp2) / (x * libm::sqrtf(tmp3)));
    if x < 0.0 {
        a = -a;
    }
    if y > 0.0 {
        a = -a;
    }
    a *= R2D;
    let upper = max_angle(s);
    if a > upper {
        (false, upper)
    } else if a < -90.0 {
        (false, -90.0)
    } else {
        (true, a)
    }
}

/// The ballistic solver (spec §20 F2): the side and worm angle at which a
/// projectile leaving at speed `v` (px/s) under gravity `g` (px/s², down)
/// passes through `(dx, dy)` (px from where it leaves, `y` down). Of the
/// two solutions it takes the flatter one (the low root for `g > 0`); `None`
/// when the point is out of reach or needs an angle the worm cannot aim at.
pub fn ballistic(v: f32, g: f32, dx: f32, dy: f32, s: &SimSettings) -> Option<(Dir, f32)> {
    if !(v > 0.0 && v.is_finite() && g.is_finite() && dx.is_finite() && dy.is_finite()) {
        return None;
    }
    let side = if dx < 0.0 { Dir::Left } else { Dir::Right };
    let (x, h) = (libm::fabsf(dx), -dy);
    let elevation = if libm::fabsf(g) < 1e-3 || x < 1e-3 {
        // No gravity, or straight up or down: aim at it, if it is reached.
        if x < 1e-3 && g > 0.0 && h > 0.0 && v * v < 2.0 * g * h {
            return None;
        }
        libm::atan2f(h, x)
    } else {
        let v2 = v * v;
        let disc = v2 * v2 - g * (g * x * x + 2.0 * h * v2);
        if disc < 0.0 {
            return None;
        }
        let root = libm::sqrtf(disc);
        let lo = libm::atanf((v2 - root) / (g * x));
        let hi = libm::atanf((v2 + root) / (g * x));
        if libm::fabsf(lo) <= libm::fabsf(hi) { lo } else { hi }
    };
    let angle = -elevation * R2D;
    (angle.is_finite() && (-90.0..=max_angle(s)).contains(&angle)).then_some((side, angle))
}

/// The unit vector a worm facing `side` at `angle` aims along.
pub fn aim_direction(side: Dir, angle: f32) -> Vec2 {
    let a = angle / R2D;
    let (sin, cos) = (libm::sinf(a), libm::cosf(a));
    Vec2::new(if side == Dir::Left { -cos } else { cos }, sin)
}

/// [`ballistic`] from a worm at `from` to `to`, for a projectile that
/// spawns `spawn_distance` px along the aim (`ShootSpawnDistance`): solved
/// once from the worm, then again from the muzzle that aim gives.
pub fn ballistic_from(
    from: Vec2,
    to: Vec2,
    v: f32,
    g: f32,
    spawn_distance: f32,
    s: &SimSettings,
) -> Option<(Dir, f32)> {
    let first = ballistic(v, g, to.x - from.x, to.y - from.y, s)?;
    let muzzle = from + aim_direction(first.0, first.1) * spawn_distance;
    ballistic(v, g, to.x - muzzle.x, to.y - muzzle.y, s)
}

/// What `AI_SetAim` decided: the side to face, the angle to turn to, and
/// whether the aim is good enough to fire.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub face: Dir,
    pub angle: f32,
    pub good: bool,
}

/// `AI_SetAim`'s memory: when the bot last turned around, and its current
/// aim offset (OLX shares one offset per difficulty between all bots; here
/// each bot draws its own from its generator, notes §4.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AimState {
    /// `fLastFace`.
    last_face: Option<AbsTime>,
    offset: f32,
    offset_at: Option<AbsTime>,
}

impl AimState {
    /// `AI_SetAim` (`CWormBot.cpp:1651-1713`) for a wanted side and angle
    /// (from [`olx_direct`] or [`ballistic`]): face that side unless the bot
    /// turned around within 0.3 s; add the difficulty's offset (redrawn
    /// after `noise_refresh`); the aim is good when the worm's angle is
    /// within the fire tolerance and it faces that side. The worm turns
    /// through the [`crate::Motor`], so unlike OLX nothing snaps here.
    pub fn set_aim(
        &mut self,
        me: &Worm,
        wanted: (Dir, f32),
        now: AbsTime,
        p: &DifficultyParams,
        rng: &mut BotRng,
        s: &SimSettings,
    ) -> Aim {
        let face = if self.last_face.is_none_or(|t| now.0.saturating_sub(t.0) > 300) {
            self.last_face = Some(now);
            wanted.0
        } else {
            me.face
        };
        let mut ang = wanted.1.max(-90.0);
        if p.aim_offset_max_deg != 0.0 {
            if self.offset_at.is_none_or(|t| now.0.saturating_sub(t.0) > p.noise_refresh.0) {
                self.offset = rng.random_num() * p.aim_offset_max_deg;
                self.offset_at = Some(now);
            }
            ang = (ang + self.offset).max(-90.0);
        }
        let good = libm::fabsf(me.angle - ang) < p.fire_tolerance_deg && me.face == face;
        Aim { face, angle: ang.min(max_angle(s)), good }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn olx_direct_uses_the_worm_convention() {
        let o = Vec2::new(100.0, 100.0);
        let at = |x: f32, y: f32| olx_direct(o, Vec2::new(100.0 + x, 100.0 + y));
        assert_eq!(at(10.0, 0.0), (Dir::Right, 0.0));
        assert_eq!(at(-10.0, 0.0), (Dir::Left, 0.0));
        assert_eq!(at(0.0, -10.0), (Dir::Right, -90.0));
        let (side, a) = at(-10.0, -10.0);
        assert!(side == Dir::Left && libm::fabsf(a + 45.0) < 1e-4);
        let (side, a) = at(10.0, 10.0);
        assert!(side == Dir::Right && libm::fabsf(a - 45.0) < 1e-4);
    }

    #[test]
    fn ballistic_takes_the_low_root_and_refuses_what_is_out_of_reach() {
        let s = SimSettings::lx56();
        // v² = g·x: the 45° shot, both roots meet.
        let (side, a) = ballistic(100.0, 100.0, 100.0, 0.0, &s).unwrap();
        assert!(side == Dir::Right && libm::fabsf(a + 45.0) < 0.1, "{a}");
        // Faster: flatter; the high root (about -83°) is not taken.
        let (side, a) = ballistic(200.0, 100.0, -100.0, 0.0, &s).unwrap();
        assert!(side == Dir::Left && libm::fabsf(a + 7.24) < 0.05, "{a}");
        assert_eq!(ballistic(50.0, 100.0, 100.0, 0.0, &s), None);
        // No gravity: straight at it.
        assert_eq!(ballistic(100.0, 0.0, 100.0, 0.0, &s), Some((Dir::Right, 0.0)));
        // Far below at a steep angle needs more than the 60° the worm aims down.
        assert_eq!(ballistic(10.0, 0.0, 1.0, 100.0, &s), None);
    }

    #[test]
    fn olx_aiming_angle_keeps_its_quirks() {
        let s = SimSettings::lx56();
        // The low root, but downwards (+) for a level target.
        let (ok, a) = olx_aiming_angle(200.0, 100.0, 100.0, 0.0, &s);
        assert!(ok && libm::fabsf(a - 7.24) < 0.05, "{a}");
        // Above the bot (y up) it is negative, upwards.
        let (ok, a) = olx_aiming_angle(200.0, 100.0, 100.0, 10.0, &s);
        assert!(ok && a < 0.0, "{a}");
        assert_eq!(olx_aiming_angle(100.0, 100.0, 0.0, 10.0, &s), (false, 60.0));
    }
}
