//! The fixed-tick accumulator (spec §8.6): the simulation runs at 100 Hz
//! whatever the display does, and frames are drawn between the last two
//! ticks.

/// One simulation tick in seconds (OLX's 100 Hz frame).
pub const TICK_SECONDS: f64 = 0.01;

/// The most ticks one drawn frame may run. A frame later than this (a
/// stall, a dragged window, a debugger) drops the rest of the backlog
/// instead of running ever more ticks to catch up (no spiral of death).
pub const MAX_CATCHUP_TICKS: u32 = 10;

/// Real time owed to the simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TickClock {
    /// Seconds not yet simulated, less than one tick after [`TickClock::advance`].
    owed: f64,
    /// Ticks dropped so far by the catch-up cap.
    dropped: u64,
}

impl TickClock {
    pub fn new() -> TickClock {
        TickClock::default()
    }

    /// `elapsed` seconds of real time passed: how many ticks to run now, at
    /// most [`MAX_CATCHUP_TICKS`]. Negative or non-finite times count as 0.
    pub fn advance(&mut self, elapsed: f64) -> u32 {
        if elapsed.is_finite() && elapsed > 0.0 {
            self.owed += elapsed;
        }
        let due = (self.owed / TICK_SECONDS).floor();
        let run = due.min(f64::from(MAX_CATCHUP_TICKS));
        if due > run {
            self.dropped += (due - run) as u64;
        }
        self.owed = (self.owed - due * TICK_SECONDS).max(0.0);
        run as u32
    }

    /// Where the drawn frame is between the last tick (0) and the next (1).
    pub fn alpha(&self) -> f32 {
        ((self.owed / TICK_SECONDS) as f32).clamp(0.0, 1.0)
    }

    /// Ticks dropped by the catch-up cap since the start.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_never_runs_more_than_n_catchup_ticks() {
        let mut c = TickClock::new();
        // A 10 s stall runs MAX_CATCHUP_TICKS, and drops the rest.
        assert_eq!(c.advance(10.0), MAX_CATCHUP_TICKS);
        assert_eq!(c.dropped(), 1000 - u64::from(MAX_CATCHUP_TICKS));
        // The backlog is gone: the next short frame runs nothing new.
        assert_eq!(c.advance(0.004), 0);
        assert!((c.alpha() - 0.4).abs() < 1e-3, "{}", c.alpha());
        // Steady 144 Hz frames run 100 ticks a second, each frame 0 or 1.
        let mut c = TickClock::new();
        let mut ticks = 0;
        for _ in 0..144 {
            let n = c.advance(1.0 / 144.0);
            assert!(n <= 1);
            ticks += n;
        }
        assert!((99..=100).contains(&ticks), "{ticks}");
        // 30 Hz frames run 3 or 4 ticks each, nothing dropped.
        let mut c = TickClock::new();
        let ticks: u32 = (0..30).map(|_| c.advance(1.0 / 30.0)).sum();
        assert!((99..=100).contains(&ticks) && c.dropped() == 0, "{ticks}");
        // Nonsense times are no time.
        assert_eq!(c.advance(f64::NAN) + c.advance(-1.0) + c.advance(f64::INFINITY), 0);
        assert!((0.0..=1.0).contains(&c.alpha()));
    }
}
