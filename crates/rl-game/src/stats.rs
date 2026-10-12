//! Frame and tick timing: the F2 panel's counters (refreshed each second)
//! and the whole run's summary printed on exit.

use crate::overlay::Stats;
use crate::session::Session;

/// `--stats`: the last second's counters on one line (the game's counts
/// when one plays).
pub fn print(st: &Stats, s: Option<&Session>) {
    let (entities, projectiles, voices) =
        s.map_or((0, 0, 0), |s| (s.present.fx.len(), s.world().projectiles().count(), s.audio.playing()));
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
    /// Each frame's busy time (`busy`), beside `frame_ms`.
    busy_ms: Vec<f32>,
    tick_ms: Vec<f32>,
    pub peak_entities: usize,
    pub peak_projectiles: usize,
}

impl Meter {
    pub fn new() -> Meter {
        Meter {
            frame_ms: Vec::with_capacity(KEPT_FRAMES),
            busy_ms: Vec::with_capacity(KEPT_FRAMES),
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
        if self.busy_ms.len() < self.busy_ms.capacity() {
            self.busy_ms.push((seconds * 1000.0) as f32);
        }
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

    /// The frames after the first `skip_s` seconds of the run (and the
    /// ticks after as many ticks at 100 a second), for `--frame-log`.
    pub fn frame_log(&self, skip_s: f64, size: (u32, u32), gpu: &str) -> FrameLog {
        let mut elapsed = 0.0;
        let first = self
            .frame_ms
            .iter()
            .position(|ms| {
                elapsed += f64::from(*ms) / 1000.0;
                elapsed >= skip_s
            })
            .map_or(self.frame_ms.len(), |i| i + 1);
        let frames = &self.frame_ms[first..];
        let busy = self.busy_ms.get(first..).unwrap_or(&[]);
        let skip_ticks = ((skip_s * 100.0).max(0.0) as usize).min(self.tick_ms.len());
        let seconds: f64 = frames.iter().map(|&m| f64::from(m) / 1000.0).sum();
        FrameLog {
            size,
            gpu: gpu.to_string(),
            skipped_s: skip_s,
            frames: frames.len(),
            seconds,
            fps: if seconds > 0.0 { frames.len() as f64 / seconds } else { 0.0 },
            frame_ms: Pct::of(frames),
            busy_ms: Pct::of(busy),
            tick_ms: Pct::of(&self.tick_ms[skip_ticks..]),
        }
    }
}

/// Percentiles of a measurement (nearest rank, [`percentile`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Pct {
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

impl Pct {
    fn of(values: &[f32]) -> Pct {
        let mut v = values.to_vec();
        v.sort_by(f32::total_cmp);
        Pct {
            p50: f64::from(percentile(&v, 0.5)),
            p99: f64::from(percentile(&v, 0.99)),
            max: f64::from(v.last().copied().unwrap_or(0.0)),
        }
    }
}

/// `--frame-log` (the performance envelope, plan 8): a windowed run's
/// frames after the first `skipped_s` seconds (the window settling, a size
/// change), as `cargo xtask envelope` reads it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameLog {
    /// The surface's size when the run ended.
    pub size: (u32, u32),
    /// The adapter, backend and present mode.
    pub gpu: String,
    pub skipped_s: f64,
    pub frames: usize,
    pub seconds: f64,
    pub fps: f64,
    /// From frame to frame (what the player sees).
    pub frame_ms: Pct,
    /// The frame without the swap chain's waits.
    pub busy_ms: Pct,
    pub tick_ms: Pct,
}

/// The nearest-rank percentile `p` (0 to 1) of `sorted`: the value at
/// rank `ceil(n * p)` (at least 1); 0 when empty. Every percentile of the
/// performance envelope has this one definition (plan 8 ruling PF8):
/// `rl-cli`'s `stress::Spread::of`, `drive::percentile_ms` and `xtask`'s
/// `Spread::of` agree.
fn percentile(sorted: &[f32], p: f64) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1]
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

    /// Plan 8 ruling PF8: rank `ceil(n * p)`, as `rl-cli`'s `Spread::of`.
    #[test]
    fn percentiles_by_nearest_rank() {
        let v: Vec<f32> = (1..=10).map(|i| i as f32).collect();
        assert_eq!((percentile(&v, 0.5), percentile(&v, 0.99), percentile(&v, 1.0)), (5.0, 10.0, 10.0));
        assert_eq!((percentile(&v, 0.0), percentile(&[], 0.5)), (1.0, 0.0));
        assert_eq!(Pct::of(&[3.0, 1.0, 2.0]), Pct { p50: 2.0, p99: 3.0, max: 3.0 });
    }

    #[test]
    fn frame_log_skips_the_first_seconds() {
        let (mut m, mut s) = (Meter::new(), Stats::default());
        for _ in 0..100 {
            m.frame(0.020, &mut s);
            m.busy(0.010);
        }
        for _ in 0..100 {
            m.frame(0.005, &mut s);
            m.busy(0.002);
        }
        let log = m.frame_log(1.999, (1920, 1080), "test gpu");
        assert_eq!(log.frames, 100);
        assert!((log.frame_ms.p50 - 5.0).abs() < 1e-3 && (log.busy_ms.p99 - 2.0).abs() < 1e-3, "{log:?}");
        assert!((log.fps - 200.0).abs() < 1.0, "{log:?}");
        assert_eq!((log.size, log.gpu.as_str()), ((1920, 1080), "test gpu"));
        assert_eq!(m.frame_log(100.0, (1, 1), "").frames, 0, "a run shorter than the skip");
    }

    /// `--frame-log`'s RON, written as `main.rs` writes it, equals the
    /// committed sample `xtask`'s envelope reads with its strict
    /// `FrameLogOut` (`tool_outputs_parse`): a field renamed here fails
    /// one test or the other instead of reading as a measured 0. After an
    /// intended change, rewrite it with `RL_BLESS_TOOL_OUTPUT=1` and fix `xtask`.
    #[test]
    fn frame_log_sample() {
        let pct = |b: f64| Pct { p50: b, p99: b * 2.0, max: b * 3.0 };
        let log = FrameLog {
            size: (3440, 1440),
            gpu: "D3D12 (NVIDIA GeForce RTX 5070 Ti) (Gl, Other), Fifo, Bgra8Unorm".into(),
            skipped_s: 3.0,
            frames: 702,
            seconds: 12.0,
            fps: 58.5,
            frame_ms: pct(16.9),
            busy_ms: pct(12.6),
            tick_ms: pct(0.2),
        };
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/bench/tool-output/frame-log.ron");
        let text = ron::ser::to_string_pretty(&log, ron::ser::PrettyConfig::default()).unwrap() + "\n";
        if std::env::var_os("RL_BLESS_TOOL_OUTPUT").is_some() {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, &text).unwrap();
            return;
        }
        let committed = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(committed == text, "{} differs from what rl-game writes now:\n{text}", file.display());
    }
}
