//! OLX's `AbsTime` and `TimeDiff` (`include/olx-types.h:29-100`): whole
//! milliseconds in a `u64`, converted to seconds as `ms * 0.001f`.

use core::ops::{Add, AddAssign, Sub};

/// A point in simulation time, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AbsTime(pub u64);

/// A duration, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeDiff(pub u64);

impl TimeDiff {
    /// `TimeDiff::seconds()`: `timeDiff * 0.001f`.
    pub fn seconds(self) -> f32 {
        self.0 as f32 * 0.001
    }

    /// `TimeDiff * float`, truncated back to whole milliseconds like OLX's `(Uint64)(timeDiff * f)`.
    pub fn scaled(self, f: f32) -> TimeDiff {
        TimeDiff((self.0 as f32 * f) as u64)
    }
}

impl Add<TimeDiff> for AbsTime {
    type Output = AbsTime;
    fn add(self, d: TimeDiff) -> AbsTime {
        AbsTime(self.0 + d.0)
    }
}

impl AddAssign<TimeDiff> for AbsTime {
    fn add_assign(&mut self, d: TimeDiff) {
        self.0 += d.0;
    }
}

impl Sub for AbsTime {
    type Output = TimeDiff;
    /// OLX asserts `self >= other`; this saturates at 0 instead of panicking.
    fn sub(self, other: AbsTime) -> TimeDiff {
        TimeDiff(self.0.saturating_sub(other.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_use_float_multiplication() {
        assert_eq!(TimeDiff(11).seconds(), 11_f32 * 0.001);
        // 20 ms is `20 * 0.001f`, which is not the same float as `0.02f`.
        assert_eq!(TimeDiff(20).seconds(), 20_f32 * 0.001);
        assert_ne!(TimeDiff(20).seconds(), 0.02_f32);
        assert_eq!((AbsTime(30) - AbsTime(10)).0, 20);
        assert_eq!(AbsTime(5) + TimeDiff(6), AbsTime(11));
        assert_eq!(TimeDiff(11).scaled(1.0 / 0.5), TimeDiff(22));
    }
}
