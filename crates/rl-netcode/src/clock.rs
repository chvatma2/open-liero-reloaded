//! Time sync (networking spec §3.2, decision N7): a peer's pace — local time
//! turned into ticks to run, adjustable by up to 1 % and shiftable by whole
//! ticks — and the host's per-client arrival margins.

use crate::session::{MAX_CATCHUP, TICK_US};

/// The margin, in ticks, a client keeps its inputs ahead of the host's need.
pub const TARGET_MARGIN: i32 = 1;
/// The pace runs at most 1 % faster or slower.
pub const MAX_ADJUST_PPM: i64 = 10_000;
/// A margin error beyond this many ticks is corrected by a jump, not by speed.
pub const JUMP_TICKS: i32 = 8;
/// The margin is the median of this many samples.
pub const MARGIN_SAMPLES: usize = 16;

const PPM: i128 = 1_000_000;

/// Local time to due ticks. The timeline is kept in microseconds times a
/// million, re-anchored on every speed change so that a change affects only
/// the time after it, without rounding loss.
#[derive(Clone, Debug)]
pub struct Pace {
    /// Local time of the last anchor.
    origin_us: u64,
    /// The timeline at the anchor, in µs × 10⁶.
    at_origin: i128,
    adjust_ppm: i64,
    /// Whole ticks added to the timeline by jumps.
    shift: i64,
    /// The latest time seen; a speed change re-anchors there.
    last_us: u64,
}

impl Pace {
    pub fn new(start_us: u64) -> Self {
        Self { origin_us: start_us, at_origin: 0, adjust_ppm: 0, shift: 0, last_us: start_us }
    }

    /// The timeline at `now_us`, in µs × 10⁶.
    fn timeline(&self, now_us: u64) -> i128 {
        let dt = now_us.saturating_sub(self.origin_us) as i128;
        self.at_origin + dt * (PPM + self.adjust_ppm as i128)
    }

    /// The ticks the timeline has reached at `now_us`, jumps included.
    pub fn ticks(&self, now_us: u64) -> i64 {
        let t = self.timeline(now_us.max(self.last_us)) / (PPM * TICK_US as i128);
        (t as i64).saturating_add(self.shift)
    }

    /// The ticks due at `now_us` with `ticks_run` already run: at most [`MAX_CATCHUP`].
    pub fn due(&mut self, now_us: u64, ticks_run: u32) -> u32 {
        self.last_us = self.last_us.max(now_us);
        let due = self.ticks(now_us).saturating_sub(ticks_run as i64);
        due.clamp(0, MAX_CATCHUP as i64) as u32
    }

    /// Runs `ppm` parts per million faster (negative: slower), from the
    /// latest time seen on; clamped to ±[`MAX_ADJUST_PPM`].
    pub fn set_adjust_ppm(&mut self, ppm: i64) {
        let ppm = ppm.clamp(-MAX_ADJUST_PPM, MAX_ADJUST_PPM);
        if ppm == self.adjust_ppm {
            return;
        }
        self.at_origin = self.timeline(self.last_us);
        self.origin_us = self.last_us;
        self.adjust_ppm = ppm;
    }

    pub fn adjust_ppm(&self) -> i64 {
        self.adjust_ppm
    }

    /// A jump: moves the timeline `ticks` ahead (negative: holds the peer back).
    pub fn shift(&mut self, ticks: i32) {
        self.shift = self.shift.saturating_add(ticks as i64);
    }
}

/// The host's arrival margins of one client: the last [`MARGIN_SAMPLES`].
#[derive(Clone, Debug, Default)]
pub struct Margins {
    ring: [i32; MARGIN_SAMPLES],
    len: usize,
    next: usize,
}

impl Margins {
    pub fn push(&mut self, m: i32) {
        self.ring[self.next] = m;
        self.next = (self.next + 1) % MARGIN_SAMPLES;
        self.len = (self.len + 1).min(MARGIN_SAMPLES);
    }

    /// The median (the lower one of an even count), `None` without samples.
    pub fn median(&self) -> Option<i16> {
        if self.len == 0 {
            return None;
        }
        let mut v = self.ring;
        let v = &mut v[..self.len];
        v.sort_unstable();
        Some(v[(self.len - 1) / 2].clamp(i16::MIN as i32, i16::MAX as i32) as i16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_runs_at_one_tick_per_10_ms() {
        let mut p = Pace::new(0);
        assert_eq!(p.due(9_999, 0), 0);
        assert_eq!(p.due(10_000, 0), 1);
        assert_eq!(p.due(1_000_000, 1), MAX_CATCHUP);
    }

    #[test]
    fn adjusting_changes_only_the_future() {
        let mut p = Pace::new(0);
        assert_eq!(p.due(1_000_000, 0).min(100), 10); // capped per call
        let mut run = 0;
        for ms in 1..=1000u64 {
            run += p.due(ms * 1000, run);
        }
        assert_eq!(run, 100);
        p.set_adjust_ppm(10_000);
        for ms in 1001..=2000u64 {
            run += p.due(ms * 1000, run);
        }
        assert_eq!(run, 201);
    }

    #[test]
    fn shifting_jumps_and_holds() {
        let mut p = Pace::new(0);
        p.shift(5);
        assert_eq!(p.due(0, 0), 5);
        p.shift(-10);
        assert_eq!(p.due(100_000, 5), 0);
    }

    #[test]
    fn margins_take_the_median_of_the_last_16() {
        let mut m = Margins::default();
        assert_eq!(m.median(), None);
        for v in [5, -3, 1, 1, 1] {
            m.push(v);
        }
        assert_eq!(m.median(), Some(1));
        for _ in 0..16 {
            m.push(-4);
        }
        assert_eq!(m.median(), Some(-4));
    }
}
