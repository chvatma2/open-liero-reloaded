//! Bit-exact ports of the OpenLieroX math the simulation depends on.
//!
//! `#![no_std]`: everything here must give the same bits on every platform,
//! including `wasm32`. Square roots and trigonometry go through the `libm`
//! crate, never through the platform's math library.
#![no_std]

mod atan;
mod rand;
mod time;
mod trig;
mod vec2;

pub use atan::atan2f;
pub use rand::{GlibcRand, Mt19937, RAND_MAX, RANDOM_NUMBERS, get_fixed_random_num};
pub use time::{AbsTime, TimeDiff};
pub use trig::{cosf, sinf};
pub use vec2::Vec2;

/// OLX `NormalizeVector` (`src/common/MathLib.cpp:109-121`): divides `v` by
/// its length unless the length is 0, and returns the length. A NaN length
/// divides too, as in C (`if(length)` is true for NaN).
pub fn normalize_vector(v: &mut Vec2) -> f32 {
    let len = libm::sqrtf(v.x * v.x + v.y * v.y);
    if len != 0.0 {
        v.x /= len;
        v.y /= len;
    }
    len
}

/// OLX `FMOD<float>` (`include/MathLib.h:86`): `a -= b * floor(a / b)`, then
/// `+ b` if still negative. The binary inlines `floorf`.
pub fn fmod(a: &mut f32, b: f32) {
    *a -= b * libm::floorf(*a / b);
    if *a < 0.0 {
        *a += b;
    }
}

/// OLX `SIGN` (`include/MathLib.h:60`): -1, 0 or 1.
pub fn sign(x: f32) -> i32 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// OLX `CLAMP_DIRECT` (`include/MathLib.h:71-79`): clamps `v` in place and
/// returns -1, 1 or 0 for "clamped low", "clamped high", "unchanged".
pub fn clamp_direct(v: &mut f32, lo: f32, hi: f32) -> i32 {
    if *v < lo {
        *v = lo;
        -1
    } else if *v > hi {
        *v = hi;
        1
    } else {
        0
    }
}

/// OLX `REDUCE_CONST` (`include/MathLib.h:80-81`): moves `v` towards 0 by
/// `amount` without crossing it.
pub fn reduce_const(v: &mut f32, amount: f32) {
    if *v > 0.0 {
        *v = if 0.0 > *v - amount { 0.0 } else { *v - amount };
    } else if *v < 0.0 {
        *v = if 0.0 < *v + amount { 0.0 } else { *v + amount };
    }
}

/// OLX `RESET_SMALL` (`include/MathLib.h:82-83`): sets `v` to 0 if `|v| < limit`.
pub fn reset_small(v: &mut f32, limit: f32) {
    if (*v > 0.0 && *v < limit) || (*v < 0.0 && *v > -limit) {
        *v = 0.0;
    }
}

/// OLX's `PI` as the float `(float)PI` (`include/MathLib.h:31`).
pub const PI_F32: f32 = core::f32::consts::PI;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_vector_and_fmod_follow_olx() {
        let mut v = Vec2::new(3.0, 4.0);
        assert_eq!((normalize_vector(&mut v), v), (5.0, Vec2::new(0.6, 0.8)));
        let mut z = Vec2::ZERO;
        assert_eq!((normalize_vector(&mut z), z), (0.0, Vec2::ZERO));
        let (mut a, mut b, mut c) = (370.0, -90.0, 359.5);
        fmod(&mut a, 360.0);
        fmod(&mut b, 360.0);
        fmod(&mut c, 360.0);
        assert_eq!((a, b, c), (10.0, 270.0, 359.5));
    }

    #[test]
    fn macros_match_olx() {
        assert_eq!((sign(-0.5), sign(0.0), sign(-0.0), sign(3.0)), (-1, 0, 0, 1));
        let mut v = 7.0;
        assert_eq!(clamp_direct(&mut v, -1.0, 5.0), 1);
        assert_eq!(v, 5.0);
        let mut v = -3.0;
        reduce_const(&mut v, 2.0);
        assert_eq!(v, -1.0);
        reduce_const(&mut v, 2.0);
        assert_eq!(v, 0.0);
        let mut v = 4.9;
        reset_small(&mut v, 5.0);
        assert_eq!(v, 0.0);
        let mut v = -5.0;
        reset_small(&mut v, 5.0);
        assert_eq!(v, -5.0);
    }

    #[test]
    fn pi_is_olx_float_pi() {
        // OLX's PI is the double literal 3.14159265358979323846, cast with (float)PI.
        assert_eq!(PI_F32, core::f64::consts::PI as f32);
    }
}
