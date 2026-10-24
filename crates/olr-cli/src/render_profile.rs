//! `olr-cli render-profile` (spec
//! `docs/superpowers/specs/2026-10-24-render-profile-design.md`): where a
//! heavy lit frame's time goes. A staged nuke fight (Liero v1.0 Reloaded
//! on Castle Strike Reloaded, a few bots carrying only the nukes) is
//! played headless and recorded, then drawn the way the game draws it,
//! and each frame's costs are reported by phase.

use std::time::Instant;

use anyhow::{Context as _, anyhow, bail};
use olr_content::{Content, MatchLoadout, MatchOptions, WeaponChoice};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{
    OUTPUT_FORMAT, Offscreen, OffscreenOptions, PASS_GROUPS, PassGroup, RenderSettings, UpscaleFilter,
};
use olr_replay::{Cursor, Replay};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use olr_sim::{EventKind, TickEvents, World};
use serde::{Deserialize, Serialize};

use crate::assets::LevelAssets;
use crate::gpu_bench::{check_adapter, draw, frame_sample};
use crate::runner::{Match, Options};
use crate::spec::{Difficulty, MatchSpec};

/// The fight's mod, level, players, seed and length (20 s): one local
/// player pressing `spec::local_keys`' scripted keys and three Medium
/// bots, as the maintainer's own matches. Medium bots alone fired 3 nukes
/// in 20 s, the first at 12.9 s; chaos bots change weapons from the whole
/// mod at each spawn; with the scripted player, 27 (16 Big, 11 Mini), the
/// first explosion at tick 200 (2026-10-24).
pub const FIGHT_MOD: &str = "olr-mod-liero-v1-reloaded";
pub const FIGHT_LEVEL: &str = "olr-level-castlestrike-reloaded";
pub const FIGHT_LOCALS: usize = 1;
pub const FIGHT_BOTS: usize = 3;
pub const FIGHT_SEED: u32 = 7;
pub const FIGHT_TICKS: u32 = 2000;
/// The only weapons the worms carry, and their loading time (percent:
/// quick reloads, so the fight keeps exploding).
pub const NUKES: [&str; 2] = ["Big Nuke", "Mini Nuke"];
pub const NUKE_LOADING_TIME: i32 = 20;

/// The fight's match: the scripted player and Medium bots, no time or
/// kill limit.
pub fn fight_spec() -> MatchSpec {
    let mut spec = MatchSpec::new(FIGHT_MOD, FIGHT_LEVEL, FIGHT_BOTS, FIGHT_SEED, FIGHT_TICKS);
    (spec.locals, spec.difficulty) = (FIGHT_LOCALS, Difficulty::Medium);
    (spec.time_limit, spec.kill_limit) = (0.0, 0);
    spec
}

/// `spec`'s options with a loadout of `weapons` alone.
fn options_with(spec: &MatchSpec, weapons: &[&str]) -> MatchOptions {
    let mut o = spec.match_options();
    o.weapons = WeaponChoice::Loadout(MatchLoadout {
        name: "Nukes".into(),
        weapons: weapons.iter().map(|w| w.to_string()).collect(),
        loading_time: NUKE_LOADING_TIME,
        unique: false,
    });
    o
}

/// The fight's options: [`NUKES`] alone, at [`NUKE_LOADING_TIME`].
pub fn fight_options(spec: &MatchSpec) -> MatchOptions {
    options_with(spec, &NUKES)
}

/// The staged fight, recorded.
#[derive(Clone, Debug)]
pub struct Fight {
    pub replay: Replay,
    /// The nukes that went off ([`has_blast`]), the first, and each one's tick.
    pub explosions: u32,
    pub first_explosion: Option<u32>,
    pub blasts: Vec<u32>,
    /// The shots fired, by weapon name (in name order).
    pub shots: Vec<(String, u32)>,
}

/// The `SimMod` indices of the projectiles [`NUKES`] fire: their
/// explosions are the blasts (a nuke's shrapnel explodes too, thousands
/// of times in a fight, and would make every frame a blast's).
pub fn nuke_projectiles(world: &World) -> Vec<i32> {
    let weapons = world.sim_mod().weapons().iter().filter(|w| NUKES.contains(&w.name.as_str()));
    weapons.filter_map(|w| Some(w.spawn.as_ref()?.proj as i32)).collect()
}

/// Whether a nuke went off in the tick: an explosion of one of `nukes`'
/// projectiles ([`nuke_projectiles`]).
pub fn has_blast(events: &TickEvents, nukes: &[i32]) -> bool {
    events.iter().any(|e| matches!(e.kind, EventKind::Explosion { proj, .. } if nukes.contains(&proj)))
}

/// Plays the fight headless and records it; an error when no nuke went
/// off, so a calm scene never passes for the nuke case.
pub fn stage_fight(content: &Content) -> anyhow::Result<Fight> {
    stage_with_weapons(content, &NUKES)
}

/// [`stage_fight`] with the worms carrying `weapons` (tests).
#[doc(hidden)]
pub fn stage_with_weapons(content: &Content, weapons: &[&str]) -> anyhow::Result<Fight> {
    stage_spec(content, &fight_spec(), weapons)
}

/// [`stage_fight`] of `spec` with the worms carrying `weapons` (tests).
#[doc(hidden)]
pub fn stage_spec(content: &Content, spec: &MatchSpec, weapons: &[&str]) -> anyhow::Result<Fight> {
    let spec = spec.clone();
    let assets = LevelAssets::load(content, FIGHT_MOD, FIGHT_LEVEL)?;
    let mut game =
        Match::with_match_options(&assets, &spec, options_with(&spec, weapons), &Options::default())
            .context("setting up the nuke fight")?;
    let nukes = nuke_projectiles(game.world());
    let mut blasts = Vec::new();
    let mut shots = std::collections::BTreeMap::<String, u32>::new();
    while game.step() {
        if has_blast(game.events(), &nukes) {
            blasts.push(game.ticks() - 1);
        }
        for e in game.events() {
            if let EventKind::WeaponFired { weapon, .. } = e.kind {
                let w = usize::try_from(weapon).ok().and_then(|i| game.world().sim_mod().weapons().get(i));
                *shots
                    .entry(w.map_or_else(|| format!("weapon {weapon}"), |w| w.name.clone()))
                    .or_default() += 1;
            }
        }
    }
    let run = game.finish();
    if let Some(f) = run.failure() {
        bail!("the nuke fight failed: {f}");
    }
    if blasts.is_empty() {
        bail!("the staged fight set off no nukes");
    }
    Ok(Fight {
        replay: run.replay,
        explosions: blasts.len() as u32,
        first_explosion: blasts.first().copied(),
        blasts,
        shots: shots.into_iter().collect(),
    })
}

/// A frame's phase: within [`BLAST_TICKS`] of a nuke going off
/// ([`has_blast`]), else while anything is hot, else calm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Phase {
    Calm,
    Blast,
    Hot,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Phase::Calm => "calm",
            Phase::Blast => "blast",
            Phase::Hot => "hot",
        })
    }
}

/// A blast's frames: half a second from an explosion.
pub const BLAST_TICKS: u32 = 50;

/// The phase of a frame `ticks_since_explosion` after the last explosion
/// (`None`: none yet) with `heat_area` hot pixels.
pub fn phase_of(ticks_since_explosion: Option<u32>, heat_area: u64) -> Phase {
    match ticks_since_explosion {
        Some(t) if t <= BLAST_TICKS => Phase::Blast,
        _ if heat_area > 0 => Phase::Hot,
        _ => Phase::Calm,
    }
}

/// Frames drawn a tick: about 200 a second, the maintainer's frame rate.
pub const FRAMES_PER_TICK: u32 = 2;

/// The game's playfield on a `size` screen (UI scale 1, the HUD's `Auto`
/// arrangement, the maintainer's): `[x, y, w, h]` in whole pixels.
pub fn playfield_area(size: (u32, u32)) -> [u32; 4] {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32));
    let p = olr_ui::hud::HudLayout::new(screen, olr_ui::HudArrangement::Auto).playfield;
    let px = |v: f32| v.round().max(0.0) as u32;
    let (x, y) = (px(p.min.x).min(size.0), px(p.min.y).min(size.1));
    [x, y, px(p.width()).min(size.0 - x).max(1), px(p.height()).min(size.1 - y).max(1)]
}

/// A measurement's mean and nearest-rank percentiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stat {
    pub mean: f64,
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

impl Stat {
    /// Of `values` (any order); all 0 when empty.
    pub fn of(values: &[f64]) -> Stat {
        if values.is_empty() {
            return Stat::default();
        }
        let s = crate::stress::Spread::of(values);
        Stat { mean: values.iter().sum::<f64>() / values.len() as f64, p50: s.p50, p99: s.p99, max: s.max }
    }
}

/// What to measure.
#[derive(Clone, Debug)]
pub struct ProfileConfig {
    pub sizes: Vec<(u32, u32)>,
    pub frames_per_tick: u32,
    pub options: OffscreenOptions,
    /// The ticks `[from, to)` drawn (`None`: all); the others are played
    /// undrawn, their heat run, so the heat is the same.
    pub window: Option<(u32, u32)>,
}

/// One pass group over a phase's frames.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupCost {
    /// `PassGroup::name`.
    pub pass: String,
    /// Its passes summed in each frame, ms (0 in a frame without them).
    pub ms: Stat,
    /// Its passes a frame.
    pub passes: Stat,
    /// Its largest single pass a frame, ms.
    pub largest_ms: Stat,
}

/// One phase's frames.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseReport {
    pub phase: Phase,
    pub frames: u32,
    /// The world's tick and `on_tick`, per tick (the tick's first frame's phase).
    pub tick_ms: Stat,
    /// The draw list, the uploads, the encoding and the submit, ms.
    pub cpu_ms: Stat,
    /// The frame on the GPU (timestamps only).
    pub gpu_ms: Option<Stat>,
    /// The frame less its timed passes: copies and gaps.
    pub between_ms: Option<Stat>,
    /// Every group some frame ran, in the GPU's order (timestamps only).
    pub groups: Vec<GroupCost>,
    /// Passes run without a timestamp slot left, summed.
    pub untimed: u32,
    /// Active heat boxes' pixels, glowing discs and bytes uploaded a frame.
    pub heat_area: Stat,
    pub discs: Stat,
    pub upload_bytes: Stat,
}

/// One output size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SizeProfile {
    pub size: (u32, u32),
    /// Where the view is drawn: the game's playfield, `[x, y, w, h]`.
    pub playfield: [u32; 4],
    /// The view targets' texels and the glow light's grid: what does not
    /// change with the output size.
    pub world_targets: Vec<(u32, u32)>,
    pub glow_grid: Option<(u32, u32)>,
    pub cascades: u32,
    pub phases: Vec<PhaseReport>,
}

/// What `olr-cli render-profile --out` writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileReport {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub timestamps: bool,
    /// The heat's copies were timed (`heat copy`).
    pub copies_timed: bool,
    pub scene: String,
    pub frames_per_tick: u32,
    pub sizes: Vec<SizeProfile>,
}

/// One frame's numbers.
struct Sample {
    cpu: f64,
    gpu: Option<f64>,
    between: f64,
    /// Per group: total ms, passes, largest ms.
    groups: [(f64, u32, f64); PASS_GROUPS],
    untimed: u32,
    heat_area: u64,
    discs: usize,
    upload_bytes: u64,
}

/// A phase's frames and ticks, gathered.
#[derive(Default)]
struct Gathered {
    samples: Vec<Sample>,
    ticks: Vec<f64>,
}

impl Gathered {
    fn report(&self, phase: Phase, timestamps: bool) -> PhaseReport {
        let of = |f: &dyn Fn(&Sample) -> f64| Stat::of(&self.samples.iter().map(f).collect::<Vec<_>>());
        let timed: Vec<&Sample> = self.samples.iter().filter(|s| s.gpu.is_some()).collect();
        let timed_of = |f: &dyn Fn(&Sample) -> f64| Stat::of(&timed.iter().map(|s| f(s)).collect::<Vec<_>>());
        let gpu = (timestamps && !timed.is_empty()).then(|| timed_of(&|s| s.gpu.unwrap_or(0.0)));
        let groups = if gpu.is_none() {
            Vec::new()
        } else {
            PassGroup::ALL
                .iter()
                .enumerate()
                .filter(|&(i, _)| timed.iter().any(|s| s.groups[i].1 > 0))
                .map(|(i, g)| GroupCost {
                    pass: g.name().to_string(),
                    ms: timed_of(&|s| s.groups[i].0),
                    passes: timed_of(&|s| f64::from(s.groups[i].1)),
                    largest_ms: timed_of(&|s| s.groups[i].2),
                })
                .collect()
        };
        PhaseReport {
            phase,
            frames: self.samples.len() as u32,
            tick_ms: Stat::of(&self.ticks),
            cpu_ms: of(&|s| s.cpu),
            between_ms: gpu.map(|_| timed_of(&|s| s.between)),
            gpu_ms: gpu,
            groups,
            untimed: self.samples.iter().map(|s| s.untimed).sum(),
            heat_area: of(&|s| s.heat_area as f64),
            discs: of(&|s| s.discs as f64),
            upload_bytes: of(&|s| s.upload_bytes as f64),
        }
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Draws `fight` at every size of `cfg` the way the game draws it (the
/// level's own look, the game's playfield, the maintainer's video
/// settings: the sharp filter, interpolation, blood at 100 %, one view)
/// and reports each phase's frames.
pub fn run(
    content: &Content,
    fight: &Fight,
    cfg: &ProfileConfig,
    progress: &mut dyn FnMut(String),
) -> anyhow::Result<ProfileReport> {
    let replay = &fight.replay;
    let packs = &replay.setup.packs;
    let assets = LevelAssets::load(content, &packs.mod_pack.id, &packs.level.id)?;
    let gpu = Offscreen::open(&cfg.options)?;
    let info = gpu.adapter_info().clone();
    check_adapter(cfg.options.prefer, info.device_type, &info.name)?;
    let max_side = gpu.device().limits().max_texture_dimension_2d;
    if let Some(&(w, h)) = cfg.sizes.iter().find(|&&(w, h)| w > max_side || h > max_side) {
        bail!("{w}x{h}: {} draws at most {max_side} pixels a side", info.name);
    }
    let fpt = cfg.frames_per_tick.max(1);
    let (from, to) = cfg.window.unwrap_or((0, replay.ticks()));
    let nukes: Vec<String> = fight.shots.iter().map(|(w, n)| format!("{w} {n}")).collect();
    let mut report = ProfileReport {
        adapter: info.name.clone(),
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        scene: format!(
            "{} on {}, {FIGHT_LOCALS} scripted player and {FIGHT_BOTS} Medium bots, ticks {from} to {to} of {}; fired: {}",
            packs.mod_pack.id,
            packs.level.id,
            replay.ticks(),
            nukes.join(", ")
        ),
        frames_per_tick: fpt,
        ..ProfileReport::default()
    };
    let skin = content.skin_or_default("olx-skin-default");
    for &size in &cfg.sizes {
        progress(format!("{}x{}: drawing ticks {from} to {to}, {fpt} frames a tick", size.0, size.1));
        let mut world = assets.replay_world(content, replay)?;
        let setup = PresentSetup {
            core: content.core(),
            m: &assets.m,
            level: &assets.level,
            theme: &assets.theme,
            viewport: Viewport::GAME,
            options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: false },
            seed: replay.setup.seed,
        };
        let mut present = Presentation::new(&setup, &world);
        present.set_blood(100);
        let mut renderer = gpu.renderer(RenderSettings { filter: UpscaleFilter::SharpBilinear });
        report.timestamps = renderer.enable_gpu_timing();
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("render-profile frame"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let areas = [playfield_area(size)];
        let (mut out, mut list, mut shown) = (TickEvents::new(), DrawList::new(), 0);
        let add_worms = |present: &mut Presentation, world: &olr_sim::World, shown: &mut usize| {
            while *shown < world.worms().len() {
                present.add_worm(&skin, default_colour(*shown), *shown == 0, false);
                *shown += 1;
            }
        };
        add_worms(&mut present, &world, &mut shown);
        // A first frame makes the targets and uploads everything.
        present.build(&world, 1.0, &mut list);
        present.upload(&mut renderer);
        draw(&mut renderer, (&list, &mut []), &areas, present.look(), &view);
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
        let _ = renderer.gpu_times();
        let mut phases: [Gathered; 3] = Default::default();
        let mut cursor = Cursor::default();
        let mut last_explosion: Option<u32> = None;
        let nukes = nuke_projectiles(&world);
        loop {
            let t = cursor.tick();
            let t0 = Instant::now();
            if !cursor.step(replay, &mut world, &mut out).map_err(|m| anyhow!("the replay diverged: {m}"))? {
                break;
            }
            add_worms(&mut present, &world, &mut shown);
            present.on_tick(&world, &out);
            let tick_ms = ms(t0);
            if has_blast(&out, &nukes) {
                last_explosion = Some(t);
            }
            if !(from..to).contains(&t) {
                // Undrawn: the terrain and the heat kept up as a frame would.
                present.upload(&mut renderer);
                renderer.run_heat();
                gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
                continue;
            }
            for k in 0..fpt {
                let t1 = Instant::now();
                present.build(&world, (k + 1) as f32 / fpt as f32, &mut list);
                let before = renderer.uploads();
                present.upload(&mut renderer);
                draw(&mut renderer, (&list, &mut []), &areas, present.look(), &view);
                let cpu = ms(t1);
                let up = renderer.uploads().since(before);
                gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
                let times = renderer.gpu_times();
                let mut groups = [(0.0, 0, 0.0); PASS_GROUPS];
                for &(g, m) in renderer.last_passes() {
                    let e = &mut groups[g as usize];
                    (e.0, e.1, e.2) = (e.0 + m, e.1 + 1, f64::max(e.2, m));
                }
                let gpu_ms = frame_sample(times);
                let timed: f64 = times.map_or(0.0, |t| t.passes.iter().sum());
                if let Some(t) = times {
                    report.copies_timed |= t.copies_timed;
                }
                let heat_area = renderer.workload().heat_area;
                let phase = phase_of(last_explosion.map(|e| t - e), heat_area);
                let g = &mut phases[phase as usize];
                if k == 0 {
                    g.ticks.push(tick_ms);
                }
                g.samples.push(Sample {
                    cpu,
                    gpu: gpu_ms,
                    between: gpu_ms.map_or(0.0, |f| (f - timed).max(0.0)),
                    groups,
                    untimed: times.map_or(0, |t| t.untimed),
                    heat_area,
                    discs: present.look().map_or(0, |l| l.emitters.len()),
                    upload_bytes: up.texture_bytes + up.buffer_bytes,
                });
            }
        }
        for w in renderer.warnings().drain(..) {
            progress(format!("warning: {w}"));
        }
        let w = renderer.workload();
        report.sizes.push(SizeProfile {
            size,
            playfield: areas[0],
            world_targets: w.world_targets,
            glow_grid: w.glow_grid,
            cascades: w.cascades,
            phases: [Phase::Calm, Phase::Blast, Phase::Hot]
                .into_iter()
                .filter(|&p| !phases[p as usize].samples.is_empty())
                .map(|p| phases[p as usize].report(p, report.timestamps))
                .collect(),
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, vec2};
    use olr_ui::HudArrangement;
    use olr_ui::hud::HudLayout;

    use super::*;

    #[test]
    fn frames_are_put_in_their_phase() {
        assert_eq!(phase_of(Some(0), 0), Phase::Blast);
        assert_eq!(phase_of(Some(BLAST_TICKS), 500), Phase::Blast);
        assert_eq!(phase_of(Some(BLAST_TICKS + 1), 500), Phase::Hot);
        assert_eq!(phase_of(None, 1), Phase::Hot);
        assert_eq!(phase_of(None, 0), Phase::Calm);
    }

    #[test]
    fn the_playfield_is_the_games() {
        assert_eq!(playfield_area((1920, 1080)), [0, 0, 1920, 864], "16:9, the bar's fifth below");
        let [x, _, w, _] = playfield_area((3440, 1440));
        assert_eq!(w, 2560, "the 16:9 frame between the side columns");
        assert_eq!(x, (3440 - 2560) / 2);
        let hud = HudLayout::new(Rect::from_min_size(Pos2::ZERO, vec2(3440.0, 1440.0)), HudArrangement::Auto);
        assert_eq!(playfield_area((3440, 1440))[3], hud.playfield.height().round() as u32);
    }

    #[test]
    fn a_stat_has_a_mean_and_percentiles() {
        let s = Stat::of(&[1.0, 2.0, 6.0]);
        assert_eq!((s.mean, s.p50, s.p99, s.max), (3.0, 2.0, 6.0, 6.0));
        assert_eq!(Stat::of(&[]), Stat::default());
    }
}
