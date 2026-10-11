//! Frame and tick timing: the F2 panel's counters (refreshed each second)
//! and the whole run's summary printed on exit.

use crate::overlay::Stats;
use crate::session::Session;

/// `--stats`: the last second's counters on one line (the game's counts
/// when one plays).
pub fn print(st: &Stats, s: Option<&Session>) {
    let (entities, projectiles, voices) =
        s.map_or((0, 0, 0), |s| (s.present.fx.len(), s.world.projectiles().count(), s.audio.playing()));
    println!(
        "stats: {:.0} fps, frame {:.2} ms (max {:.2}), busy {:.2} ms, tick {:.3} ms (max {:.3}), {} ticks/s, entities {entities}, projectiles {projectiles}, voices {voices}",
        st.fps, st.frame_ms, st.frame_ms_max, st.busy_ms, st.tick_ms, st.tick_ms_max, st.ticks_per_second,
    );
}

/// Frame times kept for the summary's percentiles (about 30 minutes at
/// 144 FPS); later frames count in the means only.
const KEPT_FRAMES: usize = 1 << 18;

/// Tick times kept for the summary's percentiles (about 43 minutes at
/// 100 Hz); later ticks count in the mean and max only.
const KEPT_TICKS: usize = 1 << 18;

/// Accumulates one second of frames and ticks, and the whole run.
#[derive(Clone, Debug, Default)]
pub struct Meter {
    window: f64,
    frames: u32,
    frame_sum: f64,
    frame_max: f64,
    busy_sum: f64,
    ticks: u32,
    tick_sum: f64,
    tick_max: f64,
    /// The whole run.
    run_seconds: f64,
    run_frames: u64,
    run_ticks: u64,
    run_tick_sum: f64,
    run_tick_max: f64,
    run_busy_sum: f64,
    frame_ms: Vec<f32>,
    tick_ms: Vec<f32>,
    pub peak_entities: usize,
    pub peak_projectiles: usize,
}

impl Meter {
    pub fn new() -> Meter {
        Meter {
            frame_ms: Vec::with_capacity(KEPT_FRAMES),
            tick_ms: Vec::with_capacity(KEPT_TICKS),
            ..Meter::default()
        }
    }

    /// A tick took `seconds`.
    pub fn tick(&mut self, seconds: f64) {
        self.ticks += 1;
        self.tick_sum += seconds;
        self.tick_max = self.tick_max.max(seconds);
        self.run_ticks += 1;
        self.run_tick_sum += seconds;
        self.run_tick_max = self.run_tick_max.max(seconds);
        if self.tick_ms.len() < self.tick_ms.capacity() {
            self.tick_ms.push((seconds * 1000.0) as f32);
        }
    }

    /// A frame kept the CPU busy for `seconds` (without waiting for VSync).
    pub fn busy(&mut self, seconds: f64) {
        self.busy_sum += seconds;
        self.run_busy_sum += seconds;
    }

    /// A frame took `seconds` from the last one. Once a second has passed,
    /// writes the second's counters into `stats` and returns true.
    pub fn frame(&mut self, seconds: f64, stats: &mut Stats) -> bool {
        let seconds = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        self.window += seconds;
        self.frames += 1;
        self.frame_sum += seconds;
        self.frame_max = self.frame_max.max(seconds);
        self.run_seconds += seconds;
        self.run_frames += 1;
        if self.frame_ms.len() < self.frame_ms.capacity() {
            self.frame_ms.push((seconds * 1000.0) as f32);
        }
        if self.window < 1.0 - 1e-9 {
            return false;
        }
        stats.fps = (f64::from(self.frames) / self.window) as f32;
        stats.frame_ms = (self.frame_sum / f64::from(self.frames) * 1000.0) as f32;
        stats.frame_ms_max = (self.frame_max * 1000.0) as f32;
        stats.busy_ms = (self.busy_sum / f64::from(self.frames) * 1000.0) as f32;
        stats.ticks_per_second = (f64::from(self.ticks) / self.window).round() as u32;
        stats.tick_ms =
            if self.ticks == 0 { 0.0 } else { (self.tick_sum / f64::from(self.ticks) * 1000.0) as f32 };
        stats.tick_ms_max = (self.tick_max * 1000.0) as f32;
        (self.window, self.frames, self.frame_sum, self.frame_max, self.busy_sum) = (0.0, 0, 0.0, 0.0, 0.0);
        (self.ticks, self.tick_sum, self.tick_max) = (0, 0.0, 0.0);
        true
    }

    /// The whole run in one line: frames, FPS, frame-time percentiles,
    /// ticks and tick-time percentiles, peaks.
    pub fn summary(&self) -> String {
        let mut ms = self.frame_ms.clone();
        // The first frames include setup; the percentiles skip a second's worth.
        let skip = ms.len().min(60);
        let ms = &mut ms[skip..];
        ms.sort_by(f32::total_cmp);
        let pct = |p: f64| percentile(ms, p);
        let mut ticks = self.tick_ms.clone();
        ticks.sort_by(f32::total_cmp);
        let secs = self.run_seconds.max(1e-9);
        format!(
            "{} frames in {:.1} s ({:.0} fps); frame ms p50 {:.2} p99 {:.2} max {:.2}; busy ms mean {:.2}; {} ticks ({:.0}/s), tick ms mean {:.3} p50 {:.3} p99 {:.3} max {:.3}; peak entities {} projectiles {}",
            self.run_frames,
            self.run_seconds,
            self.run_frames as f64 / secs,
            pct(0.5),
            pct(0.99),
            ms.last().copied().unwrap_or(0.0),
            self.run_busy_sum / self.run_frames.max(1) as f64 * 1000.0,
            self.run_ticks,
            self.run_ticks as f64 / secs,
            if self.run_ticks == 0 { 0.0 } else { self.run_tick_sum / self.run_ticks as f64 * 1000.0 },
            percentile(&ticks, 0.5),
            percentile(&ticks, 0.99),
            self.run_tick_max * 1000.0,
            self.peak_entities,
            self.peak_projectiles,
        )
    }
}

/// The nearest-rank percentile `p` (0 to 1) of `sorted`; 0 when empty.
fn percentile(sorted: &[f32], p: f64) -> f32 {
    sorted.get(((sorted.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_reports_each_second() {
        let (mut m, mut s) = (Meter::new(), Stats::default());
        for i in 0..200 {
            if i % 2 == 0 {
                m.tick(0.001);
            }
            let done = m.frame(0.005, &mut s);
            assert_eq!(done, i == 199, "frame {i}");
        }
        assert!((s.fps - 200.0).abs() < 1.0, "{}", s.fps);
        assert!((s.frame_ms - 5.0).abs() < 0.01);
        assert_eq!(s.ticks_per_second, 100);
        assert!((s.tick_ms - 1.0).abs() < 0.01);
        assert!(m.summary().starts_with("200 frames in 1.0 s (200 fps)"), "{}", m.summary());
    }

    #[test]
    fn summary_gives_tick_percentiles() {
        let (mut m, mut s) = (Meter::new(), Stats::default());
        for i in 0..100 {
            m.tick(if i < 98 { 0.001 } else { 0.005 });
        }
        m.frame(1.0, &mut s);
        let sum = m.summary();
        assert!(sum.contains("tick ms mean 1.080 p50 1.000 p99 5.000 max 5.000"), "{sum}");
    }
}
