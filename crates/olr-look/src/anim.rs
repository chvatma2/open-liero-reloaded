//! Sampling an [`Animation`] at a clock time (spec §Animation): plain `f32`
//! and `f64` arithmetic, the same on every platform, never the wall clock.

use crate::{Animation, Key, PulseShape, Rgb};

/// A sample: `scale` multiplies the intensity (1.0 = 100 %); `colour`, when
/// given, replaces the light's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub scale: f32,
    pub colour: Option<Rgb>,
}

const STEADY: Sample = Sample { scale: 1.0, colour: None };

/// SplitMix64's finaliser: a well-mixed hash of `x`.
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The default phase of a source with `identity`, 0-65 535 ms, so equal
/// lights never pulse together.
pub fn default_phase(identity: u64) -> u32 {
    (splitmix64(identity) % 65_536) as u32
}

/// `x` modulo `m` in `[0, m)` (`m` > 0).
fn rem(x: f64, m: f64) -> f64 {
    (x - libm::floor(x / m) * m).clamp(0.0, m)
}

fn percent(level: u16) -> f32 {
    f32::from(level) / 100.0
}

/// A uniform value in [0, 1) from a hash.
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

impl Animation {
    /// The value at clock `t_ms` shifted by `phase_ms`; `identity` seeds
    /// [`Animation::Flicker`]. A clock that is not finite samples as
    /// [`Animation::Steady`].
    pub fn sample(&self, t_ms: f64, phase_ms: u32, identity: u64) -> Sample {
        let t = t_ms + f64::from(phase_ms);
        if !t.is_finite() {
            return STEADY;
        }
        match self {
            Animation::Steady => STEADY,
            Animation::Pulse { period_ms, low, high, shape } => {
                let period = f64::from((*period_ms).max(1));
                let x = rem(t / period, 1.0) as f32;
                let wave = match shape {
                    PulseShape::Sine => libm::sinf(core::f32::consts::TAU * x),
                    PulseShape::Triangle if x < 0.25 => 4.0 * x,
                    PulseShape::Triangle if x < 0.75 => 2.0 - 4.0 * x,
                    PulseShape::Triangle => 4.0 * x - 4.0,
                    PulseShape::Square if x < 0.5 => 1.0,
                    PulseShape::Square => -1.0,
                };
                let (lo, hi) = (percent(*low), percent(*high));
                Sample { scale: (lo + hi) / 2.0 + (hi - lo) / 2.0 * wave, colour: None }
            }
            Animation::Flicker { speed, depth } => {
                let bucket_ms = 1000.0 / f64::from((*speed).max(1));
                let pos = t / bucket_ms;
                let b = libm::floor(pos);
                let f = (pos - b).clamp(0.0, 1.0);
                let b = b as u64;
                let n = |k: u64| unit(splitmix64(identity ^ k.wrapping_mul(0x2545_f491_4f6c_dd1d)));
                let (a, c) = (n(b), n(b.wrapping_add(1)));
                let s = f * f * (3.0 - 2.0 * f);
                let v = (a + (c - a) * s) as f32;
                let depth = percent(*depth).clamp(0.0, 1.0);
                Sample { scale: 1.0 - depth * v, colour: None }
            }
            Animation::Keys { keys, looped } => sample_keys(keys, *looped, t),
        }
    }

    /// A curve that does not loop is over past its last key; nothing else ends.
    pub fn ended(&self, t_ms: f64) -> bool {
        match self {
            Animation::Keys { keys, looped: false } => {
                keys.iter().map(|k| k.at_ms).max().is_some_and(|last| t_ms > f64::from(last))
            }
            _ => false,
        }
    }

    /// Whether the animation repeats (a default phase applies only then).
    pub fn loops(&self) -> bool {
        !matches!(self, Animation::Keys { looped: false, .. })
    }
}

fn sample_keys(keys: &[Key], looped: bool, t: f64) -> Sample {
    let Some(last) = keys.iter().map(|k| k.at_ms).max() else {
        return STEADY;
    };
    let t = if looped && last > 0 { rem(t, f64::from(last)) } else { t };
    // The key at or before t with the latest time, and the first one after it.
    let mut prev: Option<&Key> = None;
    let mut next: Option<&Key> = None;
    for k in keys {
        let at = f64::from(k.at_ms);
        if at <= t {
            if prev.is_none_or(|p| k.at_ms > p.at_ms) {
                prev = Some(k);
            }
        } else if next.is_none_or(|n| k.at_ms < n.at_ms) {
            next = Some(k);
        }
    }
    match (prev, next) {
        (None, Some(n)) => Sample { scale: percent(n.level), colour: n.colour },
        (Some(p), None) => Sample { scale: percent(p.level), colour: p.colour },
        (Some(p), Some(n)) => {
            let span = f64::from(n.at_ms - p.at_ms);
            let f = ((t - f64::from(p.at_ms)) / span) as f32;
            let scale = percent(p.level) + (percent(n.level) - percent(p.level)) * f;
            let colour = match (p.colour, n.colour) {
                (Some(a), Some(b)) => Some(core::array::from_fn(|i| {
                    libm::roundf(f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * f) as u8
                })),
                (c, _) => c,
            };
            Sample { scale, colour }
        }
        (None, None) => STEADY,
    }
}
