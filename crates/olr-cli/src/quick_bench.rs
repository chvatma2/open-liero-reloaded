//! `olr-cli quick-bench`: a one-minute benchmark for one change (the issue
//! runner's sessions; `cargo xtask bench-quick` gates it). The match's
//! ticks and draw lists are counted in instructions (`perfcount`), stable
//! on a busy machine; the offscreen frame is the minimum wall time of a few.
//! `cargo xtask bench` stays the real measurement.

use std::time::Instant;

use anyhow::Context;
use olr_bots::Bot;
use olr_content::Content;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{OUTPUT_FORMAT, Offscreen, RenderSettings};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use olr_sim::{Keys, TickEvents, World};
use serde::{Deserialize, Serialize};

use crate::perfcount::Instructions;
use crate::spec::MatchSpec;

/// Ticks played before measuring (20 s: the bots spread out, fight, dig).
pub const WARM_TICKS: u32 = 2000;
/// Ticks counted (bots think, then the world ticks).
pub const MATCH_TICKS: u32 = 300;
/// Draw lists counted.
pub const DRAW_LISTS: u32 = 60;
/// Offscreen 1080p frames timed; the fastest counts.
pub const FRAMES: u32 = 7;

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
    let mut spec = MatchSpec::new("olx-classic", "olx-level-liero-factory", 8, 5, WARM_TICKS);
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
    match Offscreen::new() {
        Ok(gpu) => {
            let mut renderer = gpu.renderer(RenderSettings::default());
            let size = (1920, 1080);
            let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("quick-bench frame"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OUTPUT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
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
        }
        Err(e) => notice(format!("no graphics adapter ({e}): the frame not measured")),
    }
    Ok(report)
}
