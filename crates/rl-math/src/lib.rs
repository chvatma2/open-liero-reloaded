//! Bit-exact ports of the OpenLieroX math the simulation depends on.
//!
//! `#![no_std]`: everything here must give the same bits on every platform,
//! including `wasm32`. Square roots and trigonometry go through the `libm`
//! crate, never through the platform's math library.
#![no_std]

mod time;
mod vec2;

pub use time::{AbsTime, TimeDiff};
pub use vec2::Vec2;

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
