//! `olr-cli quick-bench`: a one-minute benchmark for one change (the issue
//! runner's sessions; `cargo xtask bench-quick` gates it). The match's
//! ticks and draw lists are counted in instructions (`perfcount`), stable
//! on a busy machine; the offscreen frame is the minimum wall time of a few.
//! Issue #34: a lit frame of Castle Strike Reloaded too ([`warm_lit`]: its
//! look, its glow groups and a fight's discs), its uploads and encoding
//! counted in instructions and its fastest wall time on the software
//! adapter, a relative signal of the look passes' shader work (never real
//! milliseconds: `olr-cli gpu-bench --look` on a GPU measures those).
//! Issue #35: the lit frame's uploads, passes and each pass group's
//! fastest software-adapter time, and the terrain's uploads over
//! [`TERRAIN_TICKS`] ticks of its match, so each optimisation shows its gain.
//! `cargo xtask bench` stays the real measurement.

use std::time::Instant;

use anyhow::Context;
use olr_bots::Bot;
use olr_content::Content;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{OUTPUT_FORMAT, Offscreen, OffscreenOptions, PASS_GROUPS, PassGroup, RenderSettings};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use olr_sim::{Keys, TickEvents, World};
use serde::{Deserialize, Serialize};

use crate::perfcount::Instructions;
use crate::spec::{MatchSpec, Personality};

/// Ticks played before measuring (20 s: the bots spread out, fight, dig).
pub const WARM_TICKS: u32 = 2000;
/// Ticks counted (bots think, then the world ticks).
pub const MATCH_TICKS: u32 = 300;
/// Draw lists counted.
pub const DRAW_LISTS: u32 = 60;
/// Offscreen 1080p frames timed; the fastest counts.
pub const FRAMES: u32 = 7;
/// The lit frame's level (issue #34): the flames' flicker and the sky's
/// unlimited glow over the whole level, the most a level's look asks.
pub const LIT_LEVEL: &str = "olr-level-castlestrike-reloaded";
/// Lit 1080p frames counted and timed (about 0.3 s each on llvmpipe); the
/// fewest instructions and the fastest wall time count.
pub const LIT_FRAMES: u32 = 5;
/// Ticks of the lit match whose terrain uploads are counted (issue #35:
/// digs and blood stains, each tick's dirty rectangles uploaded).
pub const TERRAIN_TICKS: u32 = 100;

/// A Classic Deathmatch of 8 bots on Liero Factory after [`WARM_TICKS`]:
/// the world, its bots, the last tick's events and its presentation (which
/// saw every tick).
pub struct Warm {
    pub world: World,
    pub bots: Vec<Bot>,
    pub events: TickEvents,
    pub present: Presentation,
}

pub fn warm(content: &Content) -> anyhow::Result<Warm> {
    warm_on(content, MatchSpec::new("olx-classic", "olx-level-liero-factory", 8, 5, WARM_TICKS))
}

/// The lit frame's match (issue #34): 8 chaos bots (they fire everything,
/// so the frame has a fight's glowing discs) in Classic on [`LIT_LEVEL`]
/// after [`WARM_TICKS`], drawn with the level's look.
pub fn warm_lit(content: &Content) -> anyhow::Result<Warm> {
    let mut spec = MatchSpec::new("olx-classic", LIT_LEVEL, 8, 5, WARM_TICKS);
    spec.personality = Personality::Chaos;
    warm_on(content, spec)
}

/// `spec`'s match, without limits, after [`WARM_TICKS`].
fn warm_on(content: &Content, mut spec: MatchSpec) -> anyhow::Result<Warm> {
    spec.time_limit = 0.0;
    spec.kill_limit = 0;
    let built = content.build_match(&spec.match_options())?;
    let (m, level) = (content.load_mod(&spec.mod_id)?, content.load_level(&spec.level_id)?);
    let theme = content.load_theme(&level.theme)?;
    let skin = content.skin_or_default("olx-skin-default");
    let setup = PresentSetup {
        core: content.core(),
        m: &m,
        level: &level,
        theme: &theme,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
        options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: false },
        seed: 5,
    };
    let (mut world, mut bots) = (built.world, built.bots);
    let mut present = Presentation::new(&setup, &world);
    for i in 0..world.worms().len() {
        present.add_worm(&skin, default_colour(i), i == 0, false);
    }
    let mut events = TickEvents::new();
    let mut keys = vec![Keys(0); bots.len()];
    for _ in 0..WARM_TICKS {
        for (i, b) in bots.iter_mut().enumerate() {
            keys[i] = b.think(&world, &events);
        }
        world.tick(&keys, &mut events);
        present.on_tick(&world, &events);
    }
    Ok(Warm { world, bots, events, present })
}

/// What `quick-bench` measured; `None` where it could not measure (no
/// instruction counter, no graphics adapter).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QuickReport {
    pub match_ticks_instructions: Option<u64>,
    pub draw_lists_instructions: Option<u64>,
    pub render_frame_ms_min: Option<f64>,
    /// The lit Castle Strike frame's uploads and encoding (issue #34),
    /// the fewest of [`LIT_FRAMES`].
    #[serde(default)]
    pub lit_frame_instructions: Option<u64>,
    /// Its fastest wall time, the device waited for.
    #[serde(default)]
    pub lit_frame_ms_min: Option<f64>,
    /// The glowing discs (play's lights) in the lit frame: said, not gated.
    #[serde(default)]
    pub lit_discs: Option<usize>,
    /// Issue #35, said, not gated: the bytes a lit frame writes to the
    /// device (textures and buffers; no terrain changes between them).
    #[serde(default)]
    pub lit_frame_upload_bytes: Option<u64>,
    /// The render passes of a lit frame (GPU timestamps; `None` without them).
    #[serde(default)]
    pub lit_passes: Option<u32>,
    /// Each pass group a lit frame runs, by name, and its fastest time of
    /// [`LIT_FRAMES`] on the software adapter (ms; relative only).
    #[serde(default)]
    pub lit_pass_ms: Vec<(String, f64)>,
    /// The instructions of [`TERRAIN_TICKS`] ticks' uploads of the lit
    /// match (the dirty rectangles of its digs and stains; gated).
    #[serde(default)]
    pub terrain_upload_instructions: Option<u64>,
    /// Their texture writes and bytes (said).
    #[serde(default)]
    pub terrain_upload_writes: Option<u64>,
    #[serde(default)]
    pub terrain_upload_bytes: Option<u64>,
}

/// A 1080p target of `gpu` to draw into.
fn frame_target(gpu: &Offscreen) -> wgpu::TextureView {
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("quick-bench frame"),
        size: wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Measures the warm match; `notice` hears why a number is missing.
pub fn run(content: &Content, notice: &mut dyn FnMut(String)) -> anyhow::Result<QuickReport> {
    let mut w = warm(content)?;
    let mut report = QuickReport::default();
    match Instructions::open() {
        Ok(mut counter) => {
            let mut keys = vec![Keys(0); w.bots.len()];
            let (_, ticks) = counter.measure(|| {
                for _ in 0..MATCH_TICKS {
                    for (i, b) in w.bots.iter_mut().enumerate() {
                        keys[i] = b.think(&w.world, &w.events);
                    }
                    w.world.tick(&keys, &mut w.events);
                    w.present.on_tick(&w.world, &w.events);
                }
            });
            report.match_ticks_instructions = Some(ticks);
            let mut list = DrawList::new();
            let (_, lists) = counter.measure(|| {
                for _ in 0..DRAW_LISTS {
                    w.present.build(&w.world, 0.5, &mut list);
                }
            });
            report.draw_lists_instructions = Some(lists);
        }
        Err(e) => notice(format!("no instruction counter ({e}): instructions not measured")),
    }
    // Timestamps for the lit frame's passes (issue #35); timing is off
    // for every counted and timed frame.
    let gpu = match Offscreen::open(&OffscreenOptions { timestamps: true, ..OffscreenOptions::default() }) {
        Ok(gpu) => gpu,
        Err(e) => {
            notice(format!("no graphics adapter ({e}): the frames not measured"));
            return Ok(report);
        }
    };
    let mut renderer = gpu.renderer(RenderSettings::default());
    let size = (1920, 1080);
    let view = frame_target(&gpu);
    let mut list = DrawList::new();
    let mut best = f64::INFINITY;
    for _ in 0..FRAMES {
        let t0 = Instant::now();
        w.present.build(&w.world, 1.0, &mut list);
        w.present.upload(&mut renderer);
        renderer.render(&list, &view, size);
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
        best = best.min(t0.elapsed().as_secs_f64() * 1000.0);
    }
    report.render_frame_ms_min = Some(best);
    // The unlit match's world and renderer go before the lit one's.
    drop((w, renderer));
    lit_frame(content, &gpu, &mut report, notice)?;
    Ok(report)
}

/// The lit Castle Strike frame (issue #34) into `report`: a first frame
/// makes the targets and uploads the level, then each of [`LIT_FRAMES`]
/// builds the list (not counted), uploads and encodes it (counted) and
/// waits for the device (timed with the rest).
fn lit_frame(
    content: &Content,
    gpu: &Offscreen,
    report: &mut QuickReport,
    notice: &mut dyn FnMut(String),
) -> anyhow::Result<()> {
    let mut w = warm_lit(content)?;
    let mut counter = Instructions::open().ok();
    let mut renderer = gpu.renderer(RenderSettings::default());
    let view = frame_target(gpu);
    let area = [0, 0, 1920, 1080];
    let mut list = DrawList::new();
    w.present.build(&w.world, 1.0, &mut list);
    w.present.upload(&mut renderer);
    anyhow::ensure!(w.present.look().is_some(), "{LIT_LEVEL} has no look");
    renderer.render_lit(&list, w.present.look(), &view, area);
    gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
    let (mut fewest, mut best) = (u64::MAX, f64::INFINITY);
    let mut uploads = 0;
    for _ in 0..LIT_FRAMES {
        let t0 = Instant::now();
        w.present.build(&w.world, 1.0, &mut list);
        let before = renderer.uploads();
        let mut frame = || {
            w.present.upload(&mut renderer);
            renderer.render_lit(&list, w.present.look(), &view, area);
        };
        match counter.as_mut() {
            Some(c) => fewest = fewest.min(c.measure(frame).1),
            None => frame(),
        }
        let u = renderer.uploads().since(before);
        uploads = u.texture_bytes + u.buffer_bytes;
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
        best = best.min(t0.elapsed().as_secs_f64() * 1000.0);
    }
    if counter.is_none() {
        notice("no instruction counter: the lit frame's instructions not measured".into());
    }
    report.lit_frame_instructions = counter.as_ref().and(Some(fewest));
    report.lit_frame_ms_min = Some(best);
    report.lit_discs = w.present.look().map(|l| l.emitters.len());
    report.lit_frame_upload_bytes = Some(uploads);
    // Each pass group's fastest time, timed apart from the frames above.
    if renderer.enable_gpu_timing() {
        let mut fastest = [f64::INFINITY; PASS_GROUPS];
        let mut passes = None;
        for _ in 0..LIT_FRAMES {
            w.present.build(&w.world, 1.0, &mut list);
            w.present.upload(&mut renderer);
            renderer.render_lit(&list, w.present.look(), &view, area);
            if let Some(t) = renderer.gpu_times() {
                passes = Some(t.pass_count);
                for (f, ms) in fastest.iter_mut().zip(t.passes) {
                    *f = f.min(ms);
                }
            }
        }
        report.lit_passes = passes;
        report.lit_pass_ms = PassGroup::ALL
            .iter()
            .zip(fastest)
            .filter(|&(_, ms)| ms.is_finite() && ms > 0.0)
            .map(|(g, ms)| (g.name().to_string(), ms))
            .collect();
    } else {
        notice("no GPU timestamps: the lit frame's passes not measured".into());
    }
    terrain_uploads(&mut w, &mut renderer, counter.as_mut(), report);
    Ok(())
}

/// The lit match's terrain uploads (issue #35) into `report`: each of
/// [`TERRAIN_TICKS`] ticks played (not counted), then its dirty
/// rectangles uploaded (counted, in instructions and in writes).
fn terrain_uploads(
    w: &mut Warm,
    renderer: &mut olr_render::Renderer,
    mut counter: Option<&mut Instructions>,
    report: &mut QuickReport,
) {
    let mut keys = vec![Keys(0); w.bots.len()];
    let before = renderer.uploads();
    let mut instructions = 0;
    for _ in 0..TERRAIN_TICKS {
        for (i, b) in w.bots.iter_mut().enumerate() {
            keys[i] = b.think(&w.world, &w.events);
        }
        w.world.tick(&keys, &mut w.events);
        w.present.on_tick(&w.world, &w.events);
        match counter.as_deref_mut() {
            Some(c) => instructions += c.measure(|| w.present.upload(renderer)).1,
            None => w.present.upload(renderer),
        }
        // The writes' staging goes to the device each tick, as a frame's would.
        renderer.queue().submit([]);
    }
    let u = renderer.uploads().since(before);
    report.terrain_upload_instructions = counter.and(Some(instructions));
    report.terrain_upload_writes = Some(u.texture_writes);
    report.terrain_upload_bytes = Some(u.texture_bytes);
}
