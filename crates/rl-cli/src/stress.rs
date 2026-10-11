//! The engine stress test (plan 7 task 8, the user's request: 32-player
//! matches with a settable loading time): a headless, seeded Deathmatch of
//! up to `MAX_WORMS` chaos bots (every weapon, fired continuously) with
//! reloads near-instant (`LoadingTime` 0), bonuses on, and OLX's projectile
//! pool or a larger one, played through the match runner (invariants after
//! every tick), with the presentation's cosmetic entities stepped every
//! tick and a frame rendered offscreen every so often. It measures what an
//! optimization pass needs (`Report`) and asserts that nothing panics,
//! memory stays bounded and two runs are the same (`verify`).
//!
//! `rl-cli stress` runs one configuration; `cargo xtask stress` runs both
//! mods with both pool sizes for minutes each, and `cargo xtask full` a
//! short run against `fixtures/bench/stress.ron`. Nothing here changes a
//! default: the pool and loading time are this match's options only.

use std::time::Instant;

use anyhow::{Context as _, bail};
use rl_content::Content;
use rl_fx::MAX_ENTITIES;
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_render::{OUTPUT_FORMAT, Offscreen, RenderSettings};
use rl_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use rl_sim::TickEvents;
use serde::{Deserialize, Serialize};

use crate::runner::{Match, Options};
use crate::spec::{Difficulty, MatchSpec, Personality};

/// Bonuses fall every this many seconds.
pub const BONUS_FREQUENCY: f32 = 5.0;

/// Peak memory above this fails the run (bounded memory).
pub const MEMORY_CEILING_MIB: f64 = 4096.0;

/// How often (ticks) memory is read and progress reported.
const CHECK_TICKS: u32 = 1000;

/// The stress match: `worms` chaos bots (mixed difficulties) on `level`
/// with `mod_id`, `LoadingTime` 0, bonuses every [`BONUS_FREQUENCY`] s,
/// no kill or time limit, the projectile pool at `pool`, `seconds` long.
pub fn spec(mod_id: &str, level_id: &str, worms: usize, pool: usize, seconds: f32, seed: u32) -> MatchSpec {
    let ticks = (seconds.max(0.0) * 100.0).round() as u32;
    MatchSpec {
        difficulty: Difficulty::Mixed,
        personality: Personality::Chaos,
        bonus_frequency: Some(BONUS_FREQUENCY),
        kill_limit: 0,
        time_limit: 0.0,
        loading_time: Some(0),
        pool_capacity: Some(pool),
        ..MatchSpec::new(mod_id, level_id, worms, seed, ticks)
    }
}

/// How a run is played.
#[derive(Clone, Debug)]
pub struct Config {
    pub spec: MatchSpec,
    /// Render a frame offscreen every this many ticks (0: never).
    pub render_every: u32,
    /// The frame's size in pixels.
    pub render_size: (u32, u32),
    /// Play it again and replay its recording (determinism).
    pub verify: bool,
}

/// Percentiles of a measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spread {
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

impl Spread {
    /// Of `values` (any order), nearest rank.
    pub fn of(values: &[f64]) -> Spread {
        if values.is_empty() {
            return Spread::default();
        }
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v[((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1];
        Spread { p50: at(0.5), p99: at(0.99), max: at(1.0) }
    }
}

/// The offscreen frames.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderReport {
    pub adapter: String,
    pub size: (u32, u32),
    pub frames: u32,
    /// Draw list, uploads, render and the device's wait, in ms.
    pub frame_ms: Spread,
}

/// What a run measured.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The configuration (`MatchSpec::stem` and the pool).
    pub name: String,
    pub ticks: u32,
    pub end: String,
    /// The bots' thinking and the world's tick, in µs.
    pub tick_us: Spread,
    /// The world's tick alone.
    pub sim_us: Spread,
    /// The bots' thinking alone (all of them, a tick).
    pub think_us: Spread,
    /// The most work units one think spent, the mean, the smallest budget.
    pub units_max: u32,
    pub units_mean: u64,
    pub budget_min: u32,
    pub pool_capacity: usize,
    pub projectiles_peak: usize,
    pub projectiles_mean: f64,
    /// Ticks that ended with the pool full.
    pub ticks_at_pool_cap: u32,
    /// Cosmetic entities (`rl-fx`, at most `MAX_ENTITIES`).
    pub fx_peak: usize,
    pub ticks_at_fx_cap: u32,
    pub events_mean: f64,
    pub events_max: usize,
    /// Events the buffer dropped (counted, never lost silently).
    pub cosmetic_dropped: u64,
    pub gameplay_dropped: u64,
    pub shots: u64,
    pub kills: u64,
    pub render: Option<RenderReport>,
    /// The process's peak resident memory (`VmHWM`), in MiB (Linux).
    pub peak_mib: Option<f64>,
    /// Played again the same and its replay reproduced (`None`: not checked).
    pub deterministic: Option<bool>,
}

impl Report {
    /// The report `rl-cli stress` prints.
    pub fn text(&self) -> String {
        let s = |x: Spread| format!("p50 {:.0} p99 {:.0} max {:.0}", x.p50, x.p99, x.max);
        let mut t = format!(
            "{} ({} ticks, {})\n  tick (bots + sim) µs: {}\n  sim µs:              {}\n  bots think µs:       {}\n  \
             think units: max {} of budget {}, mean {}\n  projectiles: peak {} of {}, mean {:.0}, {} ticks at the cap\n  \
             cosmetic entities: peak {} of {MAX_ENTITIES}, {} ticks at the cap\n  \
             events a tick: mean {:.0}, max {}; dropped: {} cosmetic, {} gameplay\n  shots {}, kills {}\n",
            self.name,
            self.ticks,
            self.end,
            s(self.tick_us),
            s(self.sim_us),
            s(self.think_us),
            self.units_max,
            self.budget_min,
            self.units_mean,
            self.projectiles_peak,
            self.pool_capacity,
            self.projectiles_mean,
            self.ticks_at_pool_cap,
            self.fx_peak,
            self.ticks_at_fx_cap,
            self.events_mean,
            self.events_max,
            self.cosmetic_dropped,
            self.gameplay_dropped,
            self.shots,
            self.kills,
        );
        match &self.render {
            Some(r) => t.push_str(&format!(
                "  render {}x{} ms ({} frames, {}): {}\n",
                r.size.0,
                r.size.1,
                r.frames,
                r.adapter,
                format_args!("p50 {:.2} p99 {:.2} max {:.2}", r.frame_ms.p50, r.frame_ms.p99, r.frame_ms.max)
            )),
            None => t.push_str("  render: none (no adapter, or not asked)\n"),
        }
        let mem = self.peak_mib.map_or("unknown".to_string(), |m| format!("{m:.0} MiB"));
        let det = match self.deterministic {
            Some(true) => "deterministic over two runs, the replay reproduces every checksum",
            Some(false) => "NOT deterministic",
            None => "determinism not checked",
        };
        t.push_str(&format!("  peak memory {mem}; {det}\n"));
        t
    }
}

/// The process's peak and current resident memory in MiB (`VmHWM`,
/// `VmRSS` in `/proc/self/status`; `None` elsewhere).
pub fn memory_mib() -> Option<(f64, f64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| {
        let line = status.lines().find(|l| l.starts_with(name))?;
        let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib / 1024.0)
    };
    Some((field("VmHWM:")?, field("VmRSS:")?))
}

/// An offscreen renderer and its target.
struct Gpu {
    gpu: Offscreen,
    renderer: rl_render::Renderer,
    view: wgpu::TextureView,
    size: (u32, u32),
    list: DrawList,
    ms: Vec<f64>,
}

/// Plays `cfg`; `progress` hears a line every [`CHECK_TICKS`] ticks. A
/// failed match (a panic, an invariant), an unbounded buffer, memory over
/// [`MEMORY_CEILING_MIB`] or a second run that differs is an error.
pub fn run(content: &Content, cfg: &Config, progress: &mut dyn FnMut(String)) -> anyhow::Result<Report> {
    let spec = &cfg.spec;
    let mut m = Match::new(content, spec, &Options::default())?;
    let mo = content.load_mod(&spec.mod_id)?;
    let level = content.load_level(&spec.level_id)?;
    let theme = content.load_theme(&level.theme).with_context(|| format!("level {}", spec.level_id))?;
    let skin = content.skin_or_default("olx-skin-default");
    let setup = PresentSetup {
        core: content.core(),
        m: &mo,
        level: &level,
        theme: &theme,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
        options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
        seed: spec.seed,
    };
    let mut present = Presentation::new(&setup, m.world());
    let mut shown = 0;
    let mut gpu = if cfg.render_every > 0 { Offscreen::new().ok() } else { None }.map(|gpu| {
        let renderer = gpu.renderer(RenderSettings::default());
        let size = cfg.render_size;
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("stress frame"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Gpu { gpu, renderer, view, size, list: DrawList::new(), ms: Vec::new() }
    });

    let pool = m.world().projectile_capacity();
    let mut r =
        Report { name: format!("{}-pool{pool}", spec.stem()), pool_capacity: pool, ..Report::default() };
    let (mut proj_sum, mut events_sum) = (0u64, 0u64);
    let started = Instant::now();
    loop {
        let before = m.ticks();
        let more = m.step();
        if m.ticks() > before {
            let (w, ev) = (m.world(), m.events());
            while shown < w.worms().len() {
                present.add_worm(&skin, default_colour(shown), shown == 0, false);
                shown += 1;
            }
            present.on_tick(w, ev);
            let t = m.ticks();
            let projectiles = w.projectiles().count();
            if projectiles > pool || present.fx.len() > MAX_ENTITIES || ev.len() > TickEvents::CAPACITY {
                bail!(
                    "tick {t}: unbounded: {projectiles} projectiles of {pool}, {} entities of {MAX_ENTITIES}, {} events",
                    present.fx.len(),
                    ev.len()
                );
            }
            r.projectiles_peak = r.projectiles_peak.max(projectiles);
            proj_sum += projectiles as u64;
            r.ticks_at_pool_cap += u32::from(projectiles == pool);
            r.fx_peak = r.fx_peak.max(present.fx.len());
            r.ticks_at_fx_cap += u32::from(present.fx.len() >= MAX_ENTITIES);
            r.events_max = r.events_max.max(ev.len());
            events_sum += ev.len() as u64;
            r.cosmetic_dropped += u64::from(ev.cosmetic_dropped());
            r.gameplay_dropped += u64::from(ev.gameplay_dropped());
            if let Some(g) = gpu.as_mut().filter(|_| t.is_multiple_of(cfg.render_every)) {
                let t0 = Instant::now();
                present.build(w, 1.0, &mut g.list);
                present.upload(&mut g.renderer);
                g.renderer.render(&g.list, &g.view, g.size);
                g.gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
                g.ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            if t.is_multiple_of(CHECK_TICKS) {
                let mem = memory_mib();
                if let Some((peak, _)) = mem
                    && peak > MEMORY_CEILING_MIB
                {
                    bail!("tick {t}: {peak:.0} MiB resident, over {MEMORY_CEILING_MIB} MiB");
                }
                let wall = started.elapsed().as_secs_f64();
                progress(format!(
                    "{}: tick {t} of {} ({wall:.0} s), {projectiles} projectiles, {} entities, {}",
                    r.name,
                    spec.ticks,
                    present.fx.len(),
                    mem.map_or(String::new(), |(_, rss)| format!("{rss:.0} MiB"))
                ));
            }
        }
        if !more {
            break;
        }
    }
    let run = m.finish();
    if let Some(f) = run.failure() {
        let record = crate::failure::write(std::path::Path::new("target/failures"), spec, f, &run.replay);
        let saved =
            record.map_or_else(|e| format!("(not saved: {e:#})"), |p| format!("(saved {})", p.display()));
        bail!("{}: {f} {saved}", r.name);
    }
    let st = &run.stats;
    let us = |ns: &[u32]| ns.iter().map(|&n| f64::from(n) / 1000.0).collect::<Vec<_>>();
    let sim: Vec<u32> = st.tick_ns.iter().zip(&st.think_ns).map(|(t, b)| t.saturating_sub(*b)).collect();
    r.ticks = run.ticks;
    r.end = run.end.to_string();
    r.tick_us = Spread::of(&us(&st.tick_ns));
    r.sim_us = Spread::of(&us(&sim));
    r.think_us = Spread::of(&us(&st.think_ns));
    r.units_max = st.units_max;
    r.units_mean = st.units_total.checked_div(st.thinks).unwrap_or(0);
    r.budget_min = st.budget_min;
    let n = u64::from(run.ticks.max(1));
    r.projectiles_mean = proj_sum as f64 / n as f64;
    r.events_mean = events_sum as f64 / n as f64;
    r.shots = run.coverage.shots();
    r.kills = run.coverage.kills.values().sum::<u64>();
    r.render = gpu.map(|g| RenderReport {
        adapter: format!("{} ({:?})", g.gpu.adapter_info().name, g.gpu.adapter_info().backend),
        size: g.size,
        frames: g.ms.len() as u32,
        frame_ms: Spread::of(&g.ms),
    });
    r.peak_mib = memory_mib().map(|(peak, _)| peak);
    if cfg.verify {
        progress(format!("{}: playing it again and replaying it", r.name));
        if let Some(f) = crate::verify(content, &run)? {
            bail!("{}: {f}", r.name);
        }
        r.deterministic = Some(true);
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_and_the_stress_spec() {
        let s = Spread::of(&(1..=100).map(f64::from).collect::<Vec<_>>());
        assert_eq!((s.p50, s.p99, s.max), (50.0, 99.0, 100.0));
        assert_eq!(Spread::of(&[]), Spread::default());
        let spec = spec("olx-mw-1-0", "olx-level-castlestrike", 32, 20_000, 30.0, 1);
        let o = spec.match_options();
        assert_eq!((o.players.len(), o.pool_capacity, o.settings.loading_time), (32, 20_000, 0));
        assert!(o.settings.bonuses && o.deathmatch.kill_limit == 0 && o.deathmatch.time_limit == 0.0);
        assert_eq!(spec.ticks, 3000);
        // Everything else keeps OLX's defaults.
        let plain = MatchSpec::new("olx-classic", "olx-level-dirt-level", 4, 1, 100).match_options();
        assert_eq!((plain.pool_capacity, plain.settings.loading_time), (3000, 100));
    }
}
