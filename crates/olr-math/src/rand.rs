//! OLX's random number generators.
//!
//! - [`GlibcRand`]: glibc 2.35's `rand()` (TYPE_3 additive feedback), the one
//!   generator every gameplay and cosmetic draw uses, plus OLX's float
//!   wrappers (`src/common/MathLib.cpp:58-130`).
//! - [`RANDOM_NUMBERS`]: OLX's fixed table (`include/RandomNumberList.h`).
//! - [`Mt19937`]: boost's `mt19937`, used by `CMap::FindSpot`.

use crate::{PI_F32, Vec2, cosf, sinf};

/// glibc `RAND_MAX`.
pub const RAND_MAX: i32 = 2147483647;

/// glibc 2.35 `rand()`/`srand()` on x86-64 (`stdlib/random_r.c`).
///
/// Serialised for world-state transfer; [`GlibcRand::is_valid`] tells
/// whether a received one can draw.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlibcRand {
    r: [i32; 31],
    fptr: usize,
    rptr: usize,
    calls: u64,
}

/// OLX `HALF_RAND` (`MathLib.cpp:29`).
const HALF_RAND: i32 = RAND_MAX / 2;

/// `GetRandomNum`'s arithmetic: `(float)(rn - HALF_RAND) / (float)HALF_RAND`.
/// `(float)HALF_RAND` rounds 1073741823 up to 2^30.
fn num_from(rn: i32) -> f32 {
    (rn - HALF_RAND) as f32 / HALF_RAND as f32
}

/// `GetRandomPosNum`'s arithmetic: `(float)rn / (float)RAND_MAX`, where
/// `(float)RAND_MAX` is 2^31.
fn pos_num_from(rn: i32) -> f32 {
    rn as f32 / RAND_MAX as f32
}

/// `GetRandomInt`'s arithmetic: `CLAMP((int)(pos * (float(max) + 1)), 0, max)`.
fn int_from(rn: i32, max: i32) -> i32 {
    let f = pos_num_from(rn) * (max as f32 + 1.0);
    let v = f as i32;
    if v < 0 {
        0
    } else if v > max {
        max
    } else {
        v
    }
}

impl GlibcRand {
    /// `srand(seed)` (`__srandom_r`): seed 0 acts as 1, and the first 310
    /// outputs are discarded.
    pub fn new(seed: u32) -> Self {
        let seed = if seed == 0 { 1 } else { seed };
        let mut r = [0i32; 31];
        let mut word = seed as i32;
        r[0] = word;
        for slot in r.iter_mut().skip(1) {
            let hi = word / 127773;
            let lo = word % 127773;
            word = (16807 * lo as i64 - 2836 * hi as i64) as i32;
            if word < 0 {
                word += 2147483647;
            }
            *slot = word;
        }
        let mut g = Self { r, fptr: 3, rptr: 0, calls: 0 };
        for _ in 0..310 {
            g.next();
        }
        g.calls = 0;
        g
    }

    /// `rand()` (`__random_r`): 0..=[`RAND_MAX`].
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> i32 {
        let val = (self.r[self.fptr] as u32).wrapping_add(self.r[self.rptr] as u32);
        self.r[self.fptr] = val as i32;
        self.fptr = (self.fptr + 1) % 31;
        self.rptr = (self.rptr + 1) % 31;
        self.calls += 1;
        (val >> 1) as i32
    }

    /// How many times [`next`](Self::next) has been called since `new`
    /// (the seeding's internal discards do not count).
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Whether both table pointers are in the table, as `new` and `next`
    /// keep them: a deserialised generator must be checked before it draws.
    pub fn is_valid(&self) -> bool {
        self.fptr < self.r.len() && self.rptr < self.r.len()
    }

    /// Feeds the whole generator state to `f`, one word at a time, for checksums.
    pub fn visit_state(&self, mut f: impl FnMut(u64)) {
        self.r.iter().for_each(|&w| f(w as u32 as u64));
        f(self.fptr as u64);
        f(self.rptr as u64);
        f(self.calls);
    }

    /// `GetRandomNum` (`MathLib.cpp:58-62`).
    pub fn random_num(&mut self) -> f32 {
        num_from(self.next())
    }

    /// `GetRandomPosNum` (`MathLib.cpp:64-67`).
    pub fn random_pos_num(&mut self) -> f32 {
        pos_num_from(self.next())
    }

    /// `GetRandomInt` (`MathLib.cpp:71-76`).
    pub fn random_int(&mut self, max: i32) -> i32 {
        int_from(self.next(), max)
    }

    /// `GetRandomVec` (`MathLib.cpp:126-130`).
    pub fn random_vec(&mut self) -> Vec2 {
        // Two statements in OLX: the angle is drawn before the length.
        let angle = self.random_pos_num() * 2.0 * PI_F32;
        Vec2::new(cosf(angle), sinf(angle)) * self.random_num()
    }
}

/// OLX `RandomNumbers[]` (`include/RandomNumberList.h`).
pub const RANDOM_NUMBERS: [f32; 256] = [
    -0.620216, 0.415797, -0.550876, -0.919490, -0.474455, 0.570042, 0.106025, -0.932674, -0.256607,
    -0.770555, 0.645852, 0.521882, 0.271989, 0.628090, 0.516755, -0.349203, -0.164683, -0.440457, -0.560581,
    -0.015138, 0.764939, -0.738754, 0.007813, -0.174510, 0.906916, 0.941525, -0.644021, 0.493682, -0.872490,
    0.513825, -0.795215, -0.558872, 0.228774, -0.898431, 0.599219, -0.711835, 0.085271, 0.874260, 0.649637,
    0.848929, 0.818409, 0.777391, -0.410059, -0.792529, 0.816517, -0.278887, -0.535677, 0.908747, -0.584203,
    -0.212782, 0.755356, -0.677776, -0.945553, -0.981810, 0.265641, 0.247513, 0.710798, 0.351462, 0.567295,
    0.817494, -0.560581, 0.259843, 0.328450, -0.233169, -0.181285, -0.337789, -0.167674, 0.125557, 0.119209,
    0.666056, -0.999817, 0.400781, -0.992004, -0.296161, 0.230178, -0.690838, -0.398645, 0.628029, -0.634926,
    0.123543, 0.676128, 0.323384, -0.897394, -0.306171, -0.145212, 0.403528, 0.157908, 0.207654, 0.575902,
    -0.449551, 0.493377, 0.750839, -0.339071, 0.062077, -0.072148, 0.731429, -0.707258, -0.831838, 0.436184,
    0.229811, 0.215711, 0.680156, -0.546786, -0.663310, -0.028932, -0.467008, 0.494537, -0.457914, 0.980773,
    0.786669, 0.405054, -0.762742, 0.290301, -0.354941, -0.449612, 0.393151, -0.214796, 0.058292, 0.261185,
    0.476103, 0.692120, 0.468840, -0.156321, 0.665263, -0.822743, 0.264787, -0.529634, 0.433010, 0.856498,
    -0.497040, -0.739303, -0.449368, -0.974425, -0.536043, 0.866630, -0.416835, 0.428676, 0.601172,
    -0.126228, -0.065861, -0.628945, -0.552463, -0.667338, 0.504425, -0.004578, -0.958188, 0.730025,
    0.026064, -0.471953, 0.436245, -0.285357, 0.135201, 0.311909, -0.150949, 0.890313, 0.244949, -0.563694,
    -0.975401, 0.985900, -0.717756, -0.222182, -0.637734, 0.718733, -0.649759, -0.737533, -0.622597,
    0.660746, -0.621254, 0.164439, 0.057254, -0.578771, -0.174388, -0.748642, -0.326253, -0.540988, 0.491729,
    0.590734, 0.615821, 0.835134, 0.511140, -0.172130, -0.196728, 0.220106, 0.580114, -0.249832, -0.753647,
    -0.532503, 0.486419, -0.051700, 0.311054, -0.933529, 0.002442, 0.387475, 0.254532, 0.785143, 0.203443,
    -0.297992, 0.592871, 0.818470, -0.725203, 0.024599, -0.137215, -0.247391, -0.130623, -0.836904, 0.340780,
    0.657694, 0.078618, -0.062016, -0.718611, -0.377831, -0.920833, -0.508271, 0.238235, 0.041506, -0.481780,
    0.384484, -0.971556, 0.972533, -0.301288, 0.285906, -0.094427, 0.867179, -0.370994, 0.078435, 0.500336,
    0.997620, -0.002686, -0.817311, -0.904962, -0.114753, 0.135079, -0.082036, 0.791613, -0.954038, 0.030703,
    -0.289629, 0.136422, -0.635415, 0.456998, 0.961607, 0.765855, 0.410120, 0.273820, -0.610633, -0.367881,
    0.005677, 0.969725, 0.654703, 0.375633, 0.364646, -0.496490, 0.084417, -0.597510, 0.104987, -0.489776,
];

/// `GetFixedRandomNum` (`src/common/Misc.cpp:84-87`).
pub fn get_fixed_random_num(i: u8) -> f32 {
    RANDOM_NUMBERS[i as usize]
}

/// boost `mt19937`.
///
/// Serialised for world-state transfer as [`MtWire`] (serde has no impl for
/// a 624-word array); [`Mt19937::is_valid`] tells whether a received one
/// can draw.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(into = "MtWire", try_from = "MtWire")]
pub struct Mt19937 {
    state: [u32; 624],
    index: usize,
    calls: u64,
}

impl Mt19937 {
    /// `seed(value)`: the standard initialisation.
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; 624];
        state[0] = seed;
        for i in 1..624 {
            state[i] = 1812433253u32.wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30)).wrapping_add(i as u32);
        }
        Self { state, index: 624, calls: 0 }
    }

    /// The next 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                let mut v = self.state[(i + 397) % 624] ^ (y >> 1);
                if y & 1 != 0 {
                    v ^= 0x9908_b0df;
                }
                self.state[i] = v;
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        self.calls += 1;
        y
    }
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Whether the index is one `new` and `next_u32` reach (at most 624).
    pub fn is_valid(&self) -> bool {
        self.index <= self.state.len()
    }

    /// Feeds the whole generator state to `f`, one word at a time, for checksums.
    pub fn visit_state(&self, mut f: impl FnMut(u64)) {
        self.state.iter().for_each(|&w| f(w as u64));
        f(self.index as u64);
        f(self.calls);
    }
}

/// [`Mt19937`] on the wire: the state as a list, which must hold 624 words.
#[derive(serde::Serialize, serde::Deserialize)]
struct MtWire {
    state: alloc::vec::Vec<u32>,
    index: u64,
    calls: u64,
}

impl From<Mt19937> for MtWire {
    fn from(m: Mt19937) -> Self {
        MtWire { state: m.state.to_vec(), index: m.index as u64, calls: m.calls }
    }
}

impl TryFrom<MtWire> for Mt19937 {
    type Error = &'static str;

    fn try_from(w: MtWire) -> Result<Self, Self::Error> {
        let state = w.state.try_into().map_err(|_| "an mt19937 state has 624 words")?;
        let index = usize::try_from(w.index).map_err(|_| "an mt19937 index fits a usize")?;
        Ok(Mt19937 { state, index, calls: w.calls })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(seed: u32, n: usize) -> alloc_free::Buf {
        let mut r = GlibcRand::new(seed);
        let mut b = alloc_free::Buf::default();
        for _ in 0..n {
            b.push(r.next());
        }
        b
    }

    /// A tiny fixed buffer.
    mod alloc_free {
        #[derive(Default)]
        pub struct Buf(pub [i32; 10], pub usize);
        impl Buf {
            pub fn push(&mut self, v: i32) {
                self.0[self.1] = v;
                self.1 += 1;
            }
        }
    }

    #[test]
    fn glibc_rand_matches_glibc_2_35() {
        let cases: [(u32, [i32; 10]); 3] = [
            (
                1,
                [
                    1804289383, 846930886, 1681692777, 1714636915, 1957747793, 424238335, 719885386,
                    1649760492, 596516649, 1189641421,
                ],
            ),
            (
                12345,
                [
                    383100999, 858300821, 357768173, 455528251, 133005921, 116285904, 591987137, 102557902,
                    689413528, 585691128,
                ],
            ),
            (
                42,
                [
                    71876166, 708592740, 1483128881, 907283241, 442951012, 537146758, 1366999021, 1854614940,
                    647800535, 53523743,
                ],
            ),
        ];
        for (seed, want) in cases {
            assert_eq!(take(seed, 10).0, want, "seed {seed}");
        }
        assert_eq!(take(0, 10).0, take(1, 10).0, "seed 0 behaves as 1");
        assert_eq!(&take(u32::MAX, 5).0[..5], &[254925627, 1205188300, 366127624, 1401405153, 76053476]);
        assert_eq!(&take(2147483648, 5).0[..5], &[1336741213, 1210407648, 1447044896, 337392383, 82502902]);
        // The 1000th and 1,000,000th outputs.
        for (seed, k1000, k1m) in
            [(1, 1143565421, 429357853), (12345, 121832119, 1485618129), (u32::MAX, 1892540048, 949151631)]
        {
            let mut r = GlibcRand::new(seed);
            for _ in 0..999 {
                r.next();
            }
            assert_eq!(r.next(), k1000, "seed {seed}, 1000th");
            for _ in 1000..999_999 {
                r.next();
            }
            assert_eq!(r.next(), k1m, "seed {seed}, 1000000th");
            assert_eq!(r.calls(), 1_000_000);
        }
    }

    #[test]
    fn float_helpers_match_olx() {
        let mut r = GlibcRand::new(12345);
        let nums = [0xbf24a95f_u32, 0xbe4d75e7, 0xbf2ab390, 0xbf1364c3, 0xbf6049f6];
        for want in nums {
            assert_eq!(r.random_num().to_bits(), want);
        }
        let mut r = GlibcRand::new(12345);
        let pos = [0x3e36ad42_u32, 0x3ecca286, 0x3e2a98df, 0x3e59367a, 0x3d7db04c];
        for want in pos {
            assert_eq!(r.random_pos_num().to_bits(), want);
        }
        let mut r = GlibcRand::new(12345);
        for want in [45, 102, 42, 54, 15, 13, 70, 12, 82, 69] {
            assert_eq!(r.random_int(255), want);
        }
        let mut r = GlibcRand::new(12345);
        for want in [7, 16, 6, 8, 2, 2, 11, 1, 13, 11] {
            assert_eq!(r.random_int(40), want);
        }
        let mut r = GlibcRand::new(12345);
        for want in [(0xbdb2b39b_u32, 0xbe3903ca_u32), (0xbe9380a2, 0xbeff3af6), (0xbf533510, 0xbead37be)] {
            let v = r.random_vec();
            assert_eq!((v.x.to_bits(), v.y.to_bits()), want);
        }
        assert_eq!(r.calls(), 6, "a random_vec draws twice");
        let mut r = GlibcRand::new(1);
        let v = r.random_vec();
        assert_eq!((v.x.to_bits(), v.y.to_bits()), (0xbde83bbd, 0x3e367e97));
    }

    #[test]
    fn float_edges_match_olx() {
        // Raw `rand()` values to (Num, PosNum, Int(255), Int(2)), `randomness.md` §5.
        let edges: [(i32, f32, f32, i32, i32); 8] = [
            (0, -1.0, 0.0, 0, 0),
            (1, -1.0, 1.0 / 2147483648.0, 0, 0),
            (1073741823, 0.0, 0.5, 128, 1),
            (1073741824, 1.0 / 1073741824.0, 0.5, 128, 1),
            (2147483583, 0.99999994, 0.99999994, 255, 2),
            (2147483584, 0.99999994, 1.0, 255, 2),
            (2147483647, 1.0, 1.0, 255, 2),
            (64, -0.99999994, 64.0 / 2147483648.0, 0, 0),
        ];
        for (rn, num, pos, i255, i2) in edges {
            assert_eq!(num_from(rn).to_bits(), num.to_bits(), "num {rn}");
            assert_eq!(pos_num_from(rn).to_bits(), pos.to_bits(), "pos {rn}");
            assert_eq!(int_from(rn, 255), i255, "int255 {rn}");
            assert_eq!(int_from(rn, 2), i2, "int2 {rn}");
        }
    }

    #[test]
    fn random_int_zero_still_draws() {
        let mut r = GlibcRand::new(7);
        assert_eq!(r.random_int(0), 0);
        assert_eq!(r.calls(), 1);
    }

    #[test]
    fn mt19937_matches_boost() {
        let mut m = Mt19937::new(5489);
        assert_eq!(m.next_u32(), 3499211612, "the first output (rnd() = 0.81472369...)");
        let mut m = Mt19937::new(5489);
        let mut last = 0;
        for _ in 0..10000 {
            last = m.next_u32();
        }
        assert_eq!(last, 4123659995);
        assert_eq!(m.calls(), 10000);
    }

    #[test]
    fn mt19937_crosses_the_wire_and_checks_its_index() {
        let mut m = Mt19937::new(5489);
        for _ in 0..700 {
            m.next_u32();
        }
        let back = Mt19937::try_from(MtWire::from(m.clone())).unwrap();
        assert_eq!(back, m);
        assert!(back.is_valid() && Mt19937::new(1).is_valid(), "624 after seeding");
        let short = MtWire { state: m.state[..623].to_vec(), index: 0, calls: 0 };
        assert!(Mt19937::try_from(short).is_err());
        let past = Mt19937::try_from(MtWire { index: 625, ..MtWire::from(m) }).unwrap();
        assert!(!past.is_valid());
        let mut r = GlibcRand::new(3);
        for _ in 0..40 {
            r.next();
            assert!(r.is_valid());
        }
        r.rptr = 31;
        assert!(!r.is_valid());
    }

    #[test]
    fn fixed_table_first_and_last() {
        assert_eq!(RANDOM_NUMBERS.len(), 256);
        assert_eq!(get_fixed_random_num(0), -0.620216);
        assert_eq!(get_fixed_random_num(1), 0.415797);
        assert_eq!(get_fixed_random_num(255), -0.489776);
        assert_eq!(RANDOM_NUMBERS[248], 0.654703);
    }
}
