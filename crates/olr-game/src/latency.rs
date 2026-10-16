//! Input latency as far as the game sees it (plan 5 final review): from a
//! key press arriving to the return of the present call of the first frame
//! that draws a tick which read it. What the compositor and the display add
//! after the present call is not visible here.
//!
//! Real presses are stamped when the event loop handles them, which is late
//! when the loop was blocked in the swap chain; `--latency-probe <ms>` adds
//! probes that "arrive" on a wall-clock grid unrelated to the frames, like
//! a player's presses, and are handled at the next frame like a queued
//! event. Probes press no key.

use std::time::{Duration, Instant};

/// Samples kept for the percentiles (about an hour of probes every 50 ms).
const KEPT: usize = 1 << 16;

/// How long one frame waited in the swap chain.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PresentTiming {
    /// Acquiring the surface texture.
    pub acquire: Duration,
    /// The present call.
    pub present: Duration,
}

/// Millisecond samples with their percentiles.
#[derive(Clone, Debug, Default)]
struct Samples(Vec<f32>);

impl Samples {
    fn push(&mut self, d: Duration) {
        if self.0.len() < KEPT {
            self.0.push(d.as_secs_f32() * 1000.0);
        }
    }

    /// `n`, mean, p50, p95 and max in ms.
    fn describe(&self) -> String {
        let mut ms = self.0.clone();
        if ms.is_empty() {
            return "none".into();
        }
        ms.sort_by(f32::total_cmp);
        let pct = |p: f32| ms[((ms.len() - 1) as f32 * p).round() as usize];
        let mean = ms.iter().sum::<f32>() / ms.len() as f32;
        format!(
            "n {} mean {mean:.2} p50 {:.2} p95 {:.2} max {:.2} ms",
            ms.len(),
            pct(0.5),
            pct(0.95),
            ms[ms.len() - 1]
        )
    }
}

/// Input-to-present latency and swap-chain waits.
#[derive(Clone, Debug, Default)]
pub struct Latency {
    probe_every: Option<Duration>,
    next_probe: Option<Instant>,
    /// A press not yet read by a tick: when it arrived.
    pending: Option<Instant>,
    /// Read by a tick that no frame has presented yet.
    latched: Option<Instant>,
    input: Samples,
    acquire: Samples,
    present: Samples,
}

impl Latency {
    /// Probes every `probe_every`, if given.
    pub fn new(probe_every: Option<Duration>) -> Latency {
        Latency { probe_every, ..Latency::default() }
    }

    /// A key press arrived at `at` (one at a time: a press arriving before
    /// the last was read is not measured).
    pub fn press(&mut self, at: Instant) {
        self.pending.get_or_insert(at);
    }

    /// The frame starts at `now`: a probe due since arrives now, stamped
    /// with when it was due.
    pub fn frame_start(&mut self, now: Instant) {
        let Some(every) = self.probe_every else { return };
        let next = *self.next_probe.get_or_insert(now + every);
        if next <= now {
            self.press(next);
            // The grid goes on from the probe, never in a burst.
            let mut n = next + every;
            while n <= now {
                n += every;
            }
            self.next_probe = Some(n);
        }
    }

    /// A tick read the input: a pending press is in it.
    pub fn latched(&mut self) {
        if let Some(at) = self.pending.take() {
            self.latched.get_or_insert(at);
        }
    }

    /// A frame drawing every tick so far was presented at `at`.
    pub fn presented(&mut self, at: Instant, timing: PresentTiming) {
        if let Some(t) = self.latched.take() {
            self.input.push(at.saturating_duration_since(t));
        }
        self.acquire.push(timing.acquire);
        self.present.push(timing.present);
    }

    /// The run in one line.
    pub fn summary(&self) -> String {
        format!(
            "input to present: {}; acquire: {}; present: {}",
            self.input.describe(),
            self.acquire.describe(),
            self.present.describe()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_is_measured_to_the_frame_that_draws_its_tick() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let wait = PresentTiming { acquire: Duration::from_millis(3), present: Duration::from_millis(1) };
        let mut l = Latency::new(None);
        // A frame without a press: only the waits are counted.
        l.presented(ms(5), wait);
        l.press(ms(7));
        l.press(ms(8)); // the second before a tick read the first: ignored
        l.presented(ms(10), wait); // not read by a tick yet: no sample
        l.latched();
        l.latched();
        l.presented(ms(27), wait);
        l.presented(ms(40), wait);
        let s = l.summary();
        assert!(s.starts_with("input to present: n 1 mean 20.00"), "{s}");
        assert!(s.contains("acquire: n 4 mean 3.00"), "{s}");
    }

    #[test]
    fn probes_arrive_on_their_own_grid() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut l = Latency::new(Some(Duration::from_millis(30)));
        l.frame_start(ms(0)); // the grid starts: first probe at 30
        l.frame_start(ms(16));
        assert_eq!(l.pending, None);
        l.frame_start(ms(33));
        assert_eq!(l.pending, Some(ms(30)), "stamped when due, not when handled");
        l.latched();
        l.presented(ms(40), PresentTiming::default());
        // A long stall skips the probes it covered.
        l.frame_start(ms(200));
        assert_eq!(l.pending, Some(ms(60)));
        assert_eq!(l.next_probe, Some(ms(210)));
        assert!(l.summary().starts_with("input to present: n 1 mean 10.00"), "{}", l.summary());
    }
}
