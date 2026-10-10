//! `sinf` and `cosf` exactly as the oracle's C library computes them.
//!
//! A port of glibc 2.35's `sysdeps/ieee754/flt-32/s_sinf.c`, `s_cosf.c`,
//! `s_sincosf.h`, `sincosf_poly.h` and `s_sincosf_data.c` (LGPL-2.1-or-later,
//! from Arm's optimized-routines), in the variant x86-64 glibc picks on CPUs
//! with FMA (`sysdeps/x86_64/fpu/multiarch/s_sinf-fma.c`, built with `-mfma`).
//! There GCC fuses every `a + b * c` into one multiply-add, written here as
//! `fma(b, c, a)`. Verified against the oracle container's glibc for every
//! `f32` input.
//!
//! OLX aims with `sinf`/`cosf` (`CWorm::getFaceDirection`, `src/game/CWorm.h:514-516`).
//! The `libm` crate's versions differ from glibc's by 1 ulp for about one input in 1300.

use libm::fma;

/// `sincos_t`: one quadrant pair's signs, reduction constants and polynomials.
struct Poly {
    sign: [f64; 4],
    /// 2/π · 2²⁴ (glibc without `TOINT_INTRINSICS`, as on x86-64).
    hpi_inv: f64,
    hpi: f64,
    c: [f64; 5],
    s: [f64; 3],
}

const C: [f64; 5] = [
    f64::from_bits(0x3ff0_0000_0000_0000),
    f64::from_bits(0xbfdf_ffff_fd0c_621c),
    f64::from_bits(0x3fa5_5553_e106_8f19),
    f64::from_bits(0xbf56_c087_e89a_359d),
    f64::from_bits(0x3ef9_9343_027b_f8c3),
];
const S: [f64; 3] = [
    f64::from_bits(0xbfc5_5554_5995_a603),
    f64::from_bits(0x3f81_1076_0523_0bc4),
    f64::from_bits(0xbf29_94eb_3774_cf24),
];
const SIGN: [f64; 4] = [1.0, -1.0, -1.0, 1.0];
const HPI_INV: f64 = f64::from_bits(0x4164_5f30_6dc9_c883);
const HPI: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);

/// `__sincosf_table`: the second entry negates the cosine polynomial.
const TABLE: [Poly; 2] = [
    Poly { sign: SIGN, hpi_inv: HPI_INV, hpi: HPI, c: C, s: S },
    Poly { sign: SIGN, hpi_inv: HPI_INV, hpi: HPI, c: [-C[0], -C[1], -C[2], -C[3], -C[4]], s: S },
];

/// `__inv_pio4`: 4/π to 192 bits, 8 new bits per entry.
const INV_PIO4: [u32; 24] = [
    0xa2, 0xa2f9, 0xa2f983, 0xa2f9836e, 0xf9836e4e, 0x836e4e44, 0x6e4e4415, 0x4e441529, 0x441529fc,
    0x1529fc27, 0x29fc2757, 0xfc2757d1, 0x2757d1f5, 0x57d1f534, 0xd1f534dd, 0xf534ddc0, 0x34ddc0db,
    0xddc0db62, 0xc0db6295, 0xdb629599, 0x6295993c, 0x95993c43, 0x993c4390, 0x3c439041,
];
/// 2π · 2⁻⁶⁴.
const PI63: f64 = f64::from_bits(0x3c19_21fb_5444_2d18);

/// Top 12 bits of the float, sign cleared.
fn abstop12(x: f32) -> u32 {
    (x.to_bits() >> 20) & 0x7ff
}

const TOP_PIO4: u32 = 0x3f4;
const TOP_TINY: u32 = 0x398; // 0x1p-12f
const TOP_120: u32 = 0x42f;
const TOP_INF: u32 = 0x7f8;

/// `reduce_fast`: `x` modulo π/2 in [-π/4, π/4], and the quadrant. For |x| < 120.
fn reduce_fast(x: f64, p: &Poly) -> (f64, i32) {
    let r = x * p.hpi_inv;
    let n = ((r as i32) + 0x80_0000) >> 24;
    (fma(-(n as f64), p.hpi, x), n)
}

/// `reduce_large`: the same for larger `|x|`, with 4/π to 192 bits. `xi` is the
/// float's bits; its sign is ignored.
fn reduce_large(xi: u32) -> (f64, i32) {
    let arr = &INV_PIO4[((xi >> 26) & 15) as usize..];
    let shift = (xi >> 23) & 7;
    let xi = ((xi & 0xff_ffff) | 0x80_0000) << shift;
    let res0 = xi.wrapping_mul(arr[0]) as u64;
    let res1 = xi as u64 * arr[4] as u64;
    let res2 = xi as u64 * arr[8] as u64;
    let res0 = ((res2 >> 32) | (res0 << 32)).wrapping_add(res1);
    let n = res0.wrapping_add(1 << 61) >> 62;
    let res0 = res0.wrapping_sub(n << 62);
    (res0 as i64 as f64 * PI63, n as i32)
}

/// `sinf_poly`: the sine of `x` (`x2` = x²), or the cosine if `n` is odd.
fn sinf_poly(x: f64, x2: f64, p: &Poly, n: i32) -> f32 {
    if n & 1 == 0 {
        let x3 = x * x2;
        let s1 = fma(x2, p.s[2], p.s[1]);
        let x7 = x3 * x2;
        let s = fma(x3, p.s[0], x);
        fma(x7, s1, s) as f32
    } else {
        let x4 = x2 * x2;
        let c2 = fma(x2, p.c[4], p.c[3]);
        let c1 = fma(x2, p.c[1], p.c[0]);
        let x6 = x4 * x2;
        let c = fma(x4, p.c[2], c1);
        fma(x6, c2, c) as f32
    }
}

/// glibc's `__math_invalidf`, `(y - y) / (y - y)`: NaN for ±∞ and NaN. Here
/// always the canonical NaN, since NaN payloads differ between platforms (wasm).
fn invalid(_: f32) -> f32 {
    f32::NAN
}

/// glibc 2.35 `sinf` (`s_sinf.c`).
pub fn sinf(y: f32) -> f32 {
    let x = y as f64;
    let top = abstop12(y);
    if top < TOP_PIO4 {
        if top < TOP_TINY {
            return y;
        }
        sinf_poly(x, x * x, &TABLE[0], 0)
    } else if top < TOP_120 {
        let (x, n) = reduce_fast(x, &TABLE[0]);
        let s = TABLE[0].sign[(n & 3) as usize];
        let p = &TABLE[((n & 2) >> 1) as usize];
        sinf_poly(x * s, x * x, p, n)
    } else if top < TOP_INF {
        let xi = y.to_bits();
        let sign = (xi >> 31) as i32;
        let (x, n) = reduce_large(xi);
        let s = TABLE[0].sign[((n + sign) & 3) as usize];
        let p = &TABLE[(((n + sign) & 2) >> 1) as usize];
        sinf_poly(x * s, x * x, p, n)
    } else {
        invalid(y)
    }
}

/// glibc 2.35 `cosf` (`s_cosf.c`).
pub fn cosf(y: f32) -> f32 {
    let x = y as f64;
    let top = abstop12(y);
    if top < TOP_PIO4 {
        if top < TOP_TINY {
            return 1.0;
        }
        sinf_poly(x, x * x, &TABLE[0], 1)
    } else if top < TOP_120 {
        let (x, n) = reduce_fast(x, &TABLE[0]);
        let s = TABLE[0].sign[(n & 3) as usize];
        let p = &TABLE[((n & 2) >> 1) as usize];
        sinf_poly(x * s, x * x, p, n ^ 1)
    } else if top < TOP_INF {
        let xi = y.to_bits();
        let sign = (xi >> 31) as i32;
        let (x, n) = reduce_large(xi);
        let s = TABLE[0].sign[((n + sign) & 3) as usize];
        let p = &TABLE[(((n + sign) & 2) >> 1) as usize];
        sinf_poly(x * s, x * x, p, n ^ 1)
    } else {
        invalid(y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(bits: u32) -> f32 {
        f32::from_bits(bits)
    }

    /// `(x, sinf(x), cosf(x))` from glibc 2.35 on x86-64 with FMA (the oracle
    /// container), all as bits. The first rows are inputs where the `libm`
    /// crate is 1 ulp off.
    const GLIBC: [(u32, u32, u32); 11] = [
        (0x39e8_9769, 0x39e8_9769, 0x3f7f_fffe),
        (0xba12_8600, 0xba12_8600, 0x3f7f_fffd),
        (0x3f00_0019, 0x3ef5_776f, 0x3f60_a934),
        (0x3f49_0fdd, 0x3f35_04f5, 0x3f35_04f1),
        (0x3f49_1006, 0x3f35_0512, 0x3f35_04d4),
        (0x3f49_0fdb, 0x3f35_04f3, 0x3f35_04f3),
        (0x4049_0fdb, 0xb3bb_bd2e, 0xbf80_0000),
        (0x42f0_0000, 0x3f14_a2ef, 0x3f50_6e2a),
        (0xc2f0_0000, 0xbf14_a2ef, 0x3f50_6e2a),
        (0x447a_0000, 0x3f53_ae61, 0x3f0f_f813),
        (0x7149_f2ca, 0xbf4a_89b0, 0xbf1c_9222),
    ];

    #[test]
    fn matches_glibc_bit_for_bit() {
        for (x, s, c) in GLIBC {
            assert_eq!(sinf(f(x)).to_bits(), s, "sinf({x:#010x})");
            assert_eq!(cosf(f(x)).to_bits(), c, "cosf({x:#010x})");
        }
    }

    #[test]
    fn zeros_and_non_finite_inputs() {
        assert_eq!(sinf(0.0).to_bits(), 0);
        assert_eq!(sinf(-0.0).to_bits(), (-0.0f32).to_bits());
        assert_eq!(cosf(-0.0), 1.0);
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(sinf(x).is_nan() && cosf(x).is_nan(), "{x}");
        }
    }
}
