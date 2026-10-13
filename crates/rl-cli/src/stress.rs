//! The engine stress test (plan 7 task 8, the user's request: 32-player
//! matches with a settable loading time): a headless, seeded Deathmatch of
//! up to `MAX_WORMS` chaos bots (every weapon, fired continuously; or
//! OLX's Deathmatch bots, `Config::personality`) with
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

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context as _, bail};
use rl_content::Content;
use rl_fx::PLAY_ENTITIES;
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_render::{OUTPUT_FORMAT, Offscreen, RenderSettings};
use rl_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use rl_sim::TickEvents;
use serde::{Deserialize, Serialize};

use crate::assets::LevelAssets;
use crate::genlevel::GenLevel;
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
    /// The cosmetic entities' pool (play's [`PLAY_ENTITIES`] unless the
    /// envelope's effects sweep sets another).
    pub fx_capacity: usize,
    /// Build the draw list every this many ticks, timed as
    /// [`Report::build_us`] (0: only for the rendered frames).
    pub build_every: u32,
    /// Peak memory above this fails the run (the envelope raises it so
    /// points past the 2 GiB budget are still measured).
    pub memory_ceiling_mib: f64,
    /// Write the match's replay here (the GPU scenes play it).
    pub record: Option<PathBuf>,
    /// How every bot plays; it replaces `spec`'s. Chaos fires everything;
    /// Deathmatch bots (OLX's, their loadouts and respawns as `rl-cli
    /// match` plays them) route through the shared area cache, so the
    /// envelope's map-size and art-scale sweeps measure route finding.
    pub personality: Personality,
}

impl Config {
    /// `spec` with today's defaults: no frames, verified, play's entity
    /// pool, no draw list, [`MEMORY_CEILING_MIB`], no recording, chaos
    /// bots.
    pub fn new(spec: MatchSpec) -> Config {
        Config {
            spec,
            render_every: 0,
            render_size: (2560, 1440),
            verify: true,
            fx_capacity: PLAY_ENTITIES,
            build_every: 0,
            memory_ceiling_mib: MEMORY_CEILING_MIB,
            record: None,
            personality: Personality::Chaos,
        }
    }
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

/// The stress runner's personality before it was settable.
fn chaos() -> Personality {
    Personality::Chaos
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
    /// The presentation's tick (`Presentation::on_tick`: the entities,
    /// which replay the tick's colour-terrain edits, then the scene), µs.
    #[serde(default)]
    pub present_us: Spread,
    /// The draw list's build (`Presentation::build`) every
    /// `Config::build_every` ticks, µs.
    #[serde(default)]
    pub build_us: Spread,
    /// The most work units one think spent, the mean, the smallest budget.
    pub units_max: u32,
    pub units_mean: u64,
    pub budget_min: u32,
    pub pool_capacity: usize,
    pub projectiles_peak: usize,
    pub projectiles_mean: f64,
    /// Ticks that ended with the pool full.
    pub ticks_at_pool_cap: u32,
    /// Cosmetic entities (`rl-fx`, at most [`Report::fx_capacity`]).
    pub fx_peak: usize,
    pub ticks_at_fx_cap: u32,
    /// The entity pool's size.
    #[serde(default)]
    pub fx_capacity: usize,
    /// How the bots played (reports before it was settable: chaos).
    #[serde(default = "chaos")]
    pub personality: Personality,
    /// The bots' shared area cache: areas held, and how many it can hold.
    #[serde(default)]
    pub area_cache: (usize, usize),
    /// The level: width and height in world pixels, art scale.
    #[serde(default)]
    pub level: (u32, u32, u32),
    /// Hard limits the run reached (the bots' area cache full, a level the
    /// content loader would refuse): each breaks an envelope point.
    #[serde(default)]
    pub caps: Vec<String>,
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
             cosmetic entities: peak {} of {}, {} ticks at the cap\n  \
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
            self.fx_capacity,
            self.ticks_at_fx_cap,
            self.events_mean,
            self.events_max,
            self.cosmetic_dropped,
            self.gameplay_dropped,
            self.shots,
            self.kills,
        );
        t.push_str(&format!("  presentation µs:     {}\n", s(self.present_us)));
        if self.build_us != Spread::default() {
            t.push_str(&format!("  draw-list build µs:  {}\n", s(self.build_us)));
        }
        let bots = match self.personality {
            Personality::Chaos => "chaos",
            Personality::Deathmatch => "deathmatch",
        };
        t.push_str(&format!(
            "  {bots} bots; level {}x{} at art scale {}; area cache {} of {}\n",
            self.level.0, self.level.1, self.level.2, self.area_cache.0, self.area_cache.1
        ));
        for c in &self.caps {
            t.push_str(&format!("  cap reached: {c}\n"));
        }
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

/// The hard limits a run reached ([`Report::caps`]): the bots' area cache
/// full (`area_cache`: held, capacity), a generated level the content
/// loader would refuse (`GenLevel::content_cap`: reported, not enforced).
fn caps(area_cache: (usize, usize), generated: Option<GenLevel>) -> Vec<String> {
    let mut caps = Vec::new();
    if area_cache.1 > 0 && area_cache.0 >= area_cache.1 {
        caps.push(format!("the bots' area cache is full ({} areas)", area_cache.1));
    }
    caps.extend(generated.and_then(|g| g.content_cap()));
    caps
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
/// failed match (a panic, an invariant), memory over
/// [`Config::memory_ceiling_mib`] (an absolute stop; `cargo xtask bench`
/// gates the peak against its baseline) or a second run that differs is an error.
/// The buffer bounds (pool, entities, events) are cheap asserts that hold
/// by construction today (each is a fixed-capacity buffer); they guard a
/// future change that makes one grow.
pub fn run(content: &Content, cfg: &Config, progress: &mut dyn FnMut(String)) -> anyhow::Result<Report> {
    let spec = &MatchSpec { personality: cfg.personality, ..cfg.spec.clone() };
    let assets = LevelAssets::load(content, &spec.mod_id, &spec.level_id)?;
    let mut m = Match::with_assets(&assets, spec, &Options::default())?;
    let (mo, level, theme) = (&assets.m, &assets.level, &assets.theme);
    let skin = content.skin_or_default("olx-skin-default");
    let setup = PresentSetup {
        core: content.core(),
        m: mo,
        level,
        theme,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
        options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
        seed: spec.seed,
    };
    let mut present = Presentation::new(&setup, m.world());
    present.set_fx_capacity(cfg.fx_capacity);
    // The pool as built (at least 1): the bound, the cap and the report.
    let fx_cap = present.fx.capacity();
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
    let (mut present_us, mut build_us) = (Vec::new(), Vec::new());
    let mut built = DrawList::new();
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
            let p0 = Instant::now();
            present.on_tick(w, ev);
            present_us.push(p0.elapsed().as_secs_f64() * 1e6);
            let t = m.ticks();
            let projectiles = w.projectiles().count();
            if projectiles > pool || present.fx.len() > fx_cap || ev.len() > TickEvents::CAPACITY {
                bail!(
                    "tick {t}: unbounded: {projectiles} projectiles of {pool}, {} entities of {}, {} events",
                    present.fx.len(),
                    fx_cap,
                    ev.len()
                );
            }
            r.projectiles_peak = r.projectiles_peak.max(projectiles);
            proj_sum += projectiles as u64;
            r.ticks_at_pool_cap += u32::from(projectiles == pool);
            r.fx_peak = r.fx_peak.max(present.fx.len());
            r.ticks_at_fx_cap += u32::from(present.fx.len() >= fx_cap);
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
            if cfg.build_every > 0 && t.is_multiple_of(cfg.build_every) {
                let b0 = Instant::now();
                present.build(w, 1.0, &mut built);
                build_us.push(b0.elapsed().as_secs_f64() * 1e6);
            }
            if t.is_multiple_of(CHECK_TICKS) {
                let mem = memory_mib();
                if let Some((peak, _)) = mem
                    && peak > cfg.memory_ceiling_mib
                {
                    bail!("tick {t}: {peak:.0} MiB resident, over {} MiB", cfg.memory_ceiling_mib);
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
    let area_cache = (m.cache_len(), m.cache_capacity());
    let run = m.finish();
    if let Some(f) = run.failure() {
        let record = crate::failure::write(std::path::Path::new("target/failures"), spec, f, &run.replay);
        let saved =
            record.map_or_else(|e| format!("(not saved: {e:#})"), |p| format!("(saved {})", p.display()));
        bail!("{}: {f} {saved}", r.name);
    }
    if let Some(path) = &cfg.record {
        std::fs::write(path, run.replay.encode()).with_context(|| format!("writing {}", path.display()))?;
    }
    let st = &run.stats;
    let us = |ns: &[u32]| ns.iter().map(|&n| f64::from(n) / 1000.0).collect::<Vec<_>>();
    let sim: Vec<u32> = st.tick_ns.iter().zip(&st.think_ns).map(|(t, b)| t.saturating_sub(*b)).collect();
    r.ticks = run.ticks;
    r.end = run.end.to_string();
    r.tick_us = Spread::of(&us(&st.tick_ns));
    r.sim_us = Spread::of(&us(&sim));
    r.think_us = Spread::of(&us(&st.think_ns));
    r.present_us = Spread::of(&present_us);
    r.build_us = Spread::of(&build_us);
    r.fx_capacity = fx_cap;
    r.personality = cfg.personality;
    r.area_cache = area_cache;
    r.level = (level.material.width(), level.material.height(), level.art_scale);
    r.caps = caps(area_cache, assets.generated);
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

    /// The caps a run reports: a full area cache, and a generated level
    /// past the content loader's colour-map cap (checked here without
    /// making one: ruling PF3 keeps the 2048² art-8 level in the envelope).
    #[test]
    fn caps_name_the_full_cache_and_the_content_cap() {
        let gen_at = |art_scale| Some(GenLevel { width: 2048, height: 2048, art_scale, seed: 1 });
        assert!(caps((10, 4096), None).is_empty());
        assert!(caps((10, 4096), gen_at(4)).is_empty());
        assert!(caps((0, 0), None).is_empty(), "no bots, no cache");
        let full = caps((4096, 4096), gen_at(8));
        assert_eq!(full.len(), 2, "{full:?}");
        assert!(full[0].contains("area cache is full (4096 areas)"), "{full:?}");
        assert!(full[1].contains("MAX_COLOR_MAP_BYTES"), "{full:?}");
    }
}
