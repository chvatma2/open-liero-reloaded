//! `atan2f` as OLX's oracle computes it: glibc 2.35's
//! `sysdeps/ieee754/flt-32/e_atan2f.c` and `s_atanf.c` (Sun's fdlibm,
//! converted to float), which x86-64 glibc uses for `atan2f`. Plain `f32`
//! arithmetic in the C source's order (glibc builds with `-ffp-contract=off`),
//! checked against the oracle container's glibc (see the tests).
// The constants are glibc's literals, digit for digit.
#![allow(clippy::excessive_precision)]

// The C source's decimal literals, which are what the compiler uses: several
// differ in the last bit from the hex values in their comments.
const ATANHI: [f32; 4] = [4.6364760399e-01, 7.8539812565e-01, 9.8279368877e-01, 1.5707962513e+00];
const ATANLO: [f32; 4] = [5.0121582440e-09, 3.7748947079e-08, 3.4473217170e-08, 7.5497894159e-08];
const AT: [f32; 11] = [
    3.3333334327e-01,
    -2.0000000298e-01,
    1.4285714924e-01,
    -1.1111110449e-01,
    9.0908870101e-02,
    -7.6918758452e-02,
    6.6610731184e-02,
    -5.8335702866e-02,
    4.9768779427e-02,
    -3.6531571299e-02,
    1.6285819933e-02,
];

/// glibc's `__atanf` (`s_atanf.c`).
fn atanf(x: f32) -> f32 {
    let hx = x.to_bits() as i32;
    let ix = hx & 0x7fff_ffff;
    let mut x = x;
    let id: i32;
    if ix >= 0x4c00_0000 {
        // |x| >= 2^25
        if ix > 0x7f80_0000 {
            return f32::NAN;
        }
        return if hx > 0 { ATANHI[3] + ATANLO[3] } else { -ATANHI[3] - ATANLO[3] };
    }
    if ix < 0x3ee0_0000 {
        // |x| < 0.4375; below 2^-29 the result is x itself.
        if ix < 0x3100_0000 {
            return x;
        }
        id = -1;
    } else {
        x = libm::fabsf(x);
        if ix < 0x3f98_0000 {
            if ix < 0x3f30_0000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x401c_0000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        return x - x * (s1 + s2);
    }
    let id = id as usize;
    let z = ATANHI[id] - ((x * (s1 + s2) - ATANLO[id]) - x);
    if hx < 0 { -z } else { z }
}

const TINY: f32 = 1.0e-30;
const PI_O_4: f32 = 7.8539818525e-01;
const PI_O_2: f32 = 1.5707963705e+00;
const PI: f32 = 3.1415927410e+00;
const PI_LO: f32 = -8.7422776573e-08;

/// glibc 2.35 `atan2f` (`__ieee754_atan2f`, `e_atan2f.c`). NaN inputs give
/// the canonical NaN (payloads differ between platforms).
pub fn atan2f(y: f32, x: f32) -> f32 {
    let hx = x.to_bits() as i32;
    let ix = hx & 0x7fff_ffff;
    let hy = y.to_bits() as i32;
    let iy = hy & 0x7fff_ffff;
    if ix > 0x7f80_0000 || iy > 0x7f80_0000 {
        return f32::NAN;
    }
    if hx == 0x3f80_0000 {
        return atanf(y);
    }
    let m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if iy == 0 {
        match m {
            0 | 1 => return y,
            2 => return PI + TINY,
            _ => return -PI - TINY,
        }
    }
    if ix == 0 {
        return if hy < 0 { -PI_O_2 - TINY } else { PI_O_2 + TINY };
    }
    if ix == 0x7f80_0000 {
        if iy == 0x7f80_0000 {
            return match m {
                0 => PI_O_4 + TINY,
                1 => -PI_O_4 - TINY,
                2 => 3.0 * PI_O_4 + TINY,
                _ => -3.0 * PI_O_4 - TINY,
            };
        }
        return match m {
            0 => 0.0,
            1 => -0.0,
            2 => PI + TINY,
            _ => -PI - TINY,
        };
    }
    if iy == 0x7f80_0000 {
        return if hy < 0 { -PI_O_2 - TINY } else { PI_O_2 + TINY };
    }
    let k = (iy - ix) >> 23;
    let z = if k > 60 {
        PI_O_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atanf(libm::fabsf(y / x))
    };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(bits: u32) -> f32 {
        f32::from_bits(bits)
    }

    /// `(y, x, atan2f(y, x))` as bits, from glibc 2.35 in the oracle container
    /// (`olr-olx-oracle:dev`): every branch of both functions, then inputs
    /// where the `libm` crate or the hex constants in glibc's comments are
    /// 1 ulp off. The port also matched glibc on 6 million random pairs.
    const GLIBC: [(u32, u32, u32); 20] = [
        (0x3f00_0000, 0x3f80_0000, 0x3eed_6338),
        (0x0000_0000, 0xbf80_0000, 0x4049_0fdb),
        (0x8000_0000, 0xbf80_0000, 0xc049_0fdb),
        (0x3f80_0000, 0x0000_0000, 0x3fc9_0fdb),
        (0x7f80_0000, 0x7f80_0000, 0x3f49_0fdb),
        (0xff80_0000, 0xff80_0000, 0xc016_cbe4),
        (0x3f80_0000, 0xff80_0000, 0x4049_0fdb),
        (0x7f80_0000, 0x3f80_0001, 0x3fc9_0fdb),
        (0x7e00_0000, 0x0100_0000, 0x3fc9_0fdb),
        (0x0100_0000, 0xfe00_0000, 0x4049_0fdb),
        (0x2f00_0000, 0x3f80_0001, 0x2eff_fffe),
        (0x3ee0_0000, 0x3f80_0001, 0x3ed3_2774),
        (0x3f30_0000, 0x3f80_0001, 0x3f1a_2f80),
        (0x3f98_0000, 0x3f80_0001, 0x3f5e_f386),
        (0x401c_0000, 0x3f80_0001, 0x3f97_3ab9),
        (0x3f35_04f3, 0xbf35_04f3, 0x4016_cbe4),
        (0xbe9b_8e68, 0x3f73_e60e, 0xbe9e_0dff),
        (0xc298_beda, 0xc1a5_6d88, 0xbfea_e849),
        (0x4228_b180, 0x42f8_6522, 0x3ea7_9a6c),
        (0x3f6f_1caf, 0xbeb6_e1b8, 0x3ff7_d0a5),
    ];

    #[test]
    fn matches_glibc_bit_for_bit() {
        for (y, x, r) in GLIBC {
            assert_eq!(atan2f(f(y), f(x)).to_bits(), r, "atan2f({:e}, {:e})", f(y), f(x));
        }
        assert!(atan2f(f32::NAN, 1.0).is_nan() && atan2f(1.0, f32::NAN).is_nan());
    }
}
