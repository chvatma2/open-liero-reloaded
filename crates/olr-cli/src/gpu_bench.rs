//! The GPU half of the performance envelope (plan 8): a recorded match (a
//! replay: the heavy stress match, or a sweep's generated level) drawn
//! offscreen at each size on the chosen adapter, one tick and one frame at
//! a time; the CPU side timed with the clock, the GPU side with timestamps
//! when the device has them (`GpuReport::timestamps`; else only the wall
//! time of the frame waited for, said so). Then the shader-headroom probe
//! (`olr_render::Probe`) at each size. The scene is `Viewport::GAME` over
//! the whole target (no HUD; plan 8 ruling P11).

use std::time::Instant;

use anyhow::{Context as _, anyhow, bail, ensure};
use olr_content::{Content, World};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{
    AdapterPreference, GpuTimes, OUTPUT_FORMAT, Offscreen, OffscreenOptions, Probe, RenderSettings,
};
use olr_replay::{Cursor, Replay};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use olr_sim::TickEvents;
use serde::{Deserialize, Serialize};

use crate::assets::LevelAssets;
use crate::stress::Spread;

/// What to measure.
#[derive(Clone, Debug)]
pub struct BenchConfig {
    pub sizes: Vec<(u32, u32)>,
    /// Ticks played (unrendered) before the measured frames.
    pub start_tick: u32,
    /// Frames measured at each size (one tick each).
    pub frames: u32,
    /// Probe pass counts, the first 0 (the baseline), e.g. 0, 1, 2, 4, 8.
    pub probe_passes: Vec<u32>,
    pub probe_taps: u32,
    /// Frames drawn for each probe count.
    pub probe_frames: u32,
    pub options: OffscreenOptions,
    /// Draw the replay's level with [`bench_look`] (`--look`, plan 15a
    /// Task 20): the look system's passes, and the engine's lights of the
    /// replay's own shots and explosions.
    pub look: bool,
}

/// The probe at one pass count.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProbePoint {
    pub passes: u32,
    pub taps: u32,
    /// The probe passes' GPU ms (timestamps), else the frame's wall ms over
    /// the 0-pass baseline's median.
    pub ms: Spread,
    pub timed_on_gpu: bool,
    /// Frames whose timestamps gave no real span (unwritten, backwards):
    /// left out of `ms`, not counted as 0 ms.
    #[serde(default)]
    pub gpu_dropped: u32,
}

/// One size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SizeReport {
    pub size: (u32, u32),
    pub frames: u32,
    /// The replay's tick and the presentation's (`World::tick`, `on_tick`), ms.
    pub tick_ms: Spread,
    /// The draw list, the uploads and the render's encoding and submit, ms.
    pub cpu_ms: Spread,
    /// From the draw list to the device idle (CPU, then the GPU waited for), ms.
    pub frame_ms: Spread,
    /// A tick and a frame: what a 100 Hz game spends on a drawn frame at most, ms.
    pub total_ms: Spread,
    /// The world and upscale passes on the GPU (timestamps), ms.
    pub gpu_ms: Option<Spread>,
    /// Measured frames whose timestamps gave no real span (unwritten,
    /// backwards; a driver's fault): left out of `gpu_ms`, not counted as
    /// 0 ms, so they cannot pull it low.
    #[serde(default)]
    pub gpu_dropped: u32,
    pub probe: Vec<ProbePoint>,
}

/// What `olr-cli gpu-bench --out` writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GpuReport {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    /// The GPU times are timestamps (else CPU-side times only).
    pub timestamps: bool,
    /// The replay's mod and level.
    pub scene: String,
    pub start_tick: u32,
    /// Drawn with [`bench_look`].
    #[serde(default)]
    pub look: bool,
    pub sizes: Vec<SizeReport>,
}

/// A representative lit scene for a level of `world` pixels at art scale
/// `art` (plan 15a Task 20; plan 15c: glowing pixels are the only level
/// light, play's discs come from the match): a dark blue ambient at 20 %,
/// the solid floor and soak at their defaults, and one glow group (reach
/// 48, pulsing) lighting dashed strips 2 world pixels wide every 64.
pub fn bench_look(world: (u32, u32), art: u32) -> (olr_look::LevelLook, olr_terrain::GlowLayers) {
    use olr_look::{Ambient, Animation, GlowColour, GlowGroup, PulseShape, Reach};
    let art = art.max(1);
    let defaults = olr_format::look::LookSource::default();
    let group = GlowGroup {
        id: "bench".into(),
        colour: GlowColour::Own,
        intensity: 300,
        reach: Reach::Pixels(48),
        animation: Animation::Pulse { period_ms: 2000, low: 60, high: 100, shape: PulseShape::Sine },
        phase_ms: None,
    };
    let look = olr_look::LevelLook {
        ambient: Ambient { colour: [0x30, 0x40, 0x60], intensity: 20 },
        max_light: 100,
        light_steps: 0,
        softness: 1,
        light_soak: defaults.light_soak,
        solid_floor: Ambient { colour: [255, 255, 255], intensity: 30 },
        grade: Default::default(),
        groups: vec![group],
    };
    let art_size = (world.0.saturating_mul(art), world.1.saturating_mul(art));
    let mut glow = olr_terrain::GlowLayers::empty(art_size, world);
    let strip = |x: u32, y: u32| x % 64 < 2 && y % 32 < 8;
    for y in 0..world.1 {
        for x in 0..world.0 {
            if strip(x, y) {
                glow.groups[(y * world.0 + x) as usize] = 1;
                glow.groups_back[(y * world.0 + x) as usize] = 1;
            }
        }
    }
    for y in 0..art_size.1 {
        for x in 0..art_size.0 {
            if strip(x / art, y / art) {
                let i = (y * art_size.0 + x) as usize;
                (glow.front[i], glow.back[i]) = (255, 255);
            }
        }
    }
    (look, glow)
}

/// `1920x1080,2560x1440` as sizes.
pub fn parse_sizes(text: &str) -> anyhow::Result<Vec<(u32, u32)>> {
    let sizes = text
        .split(',')
        .map(|s| -> anyhow::Result<(u32, u32)> {
            let (w, h) = s.trim().split_once('x').ok_or_else(|| anyhow!("{s:?} is not WIDTHxHEIGHT"))?;
            let (w, h): (u32, u32) = (w.parse()?, h.parse()?);
            ensure!(w > 0 && h > 0 && w <= 16_384 && h <= 16_384, "{s:?}: sides from 1 to 16384");
            Ok((w, h))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    ensure!(!sizes.is_empty(), "no size");
    Ok(sizes)
}

/// Whether `name` is a software rasteriser's. wgpu's GL backend marks only
/// Mesa's offscreen, SwiftShader and llvmpipe as `Cpu`, so WARP behind
/// Mesa's d3d12 driver ("D3D12 (Microsoft Basic Render Driver)"), softpipe
/// and the like come back as `Other`: they are known by name. `xtask`'s
/// envelope keeps the same list (`envelope::software_name`).
pub fn software_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("basic render driver")
        || lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| matches!(w, "llvmpipe" | "lavapipe" | "softpipe" | "warp" | "swiftshader"))
}

/// A run that asked for the hardware adapter and got a software
/// rasteriser (a `Cpu` device, or one known by name: [`software_name`])
/// is an error, never a "GPU" result.
pub fn check_adapter(
    prefer: AdapterPreference,
    device_type: wgpu::DeviceType,
    name: &str,
) -> anyhow::Result<()> {
    if prefer != AdapterPreference::Software && (device_type == wgpu::DeviceType::Cpu || software_name(name))
    {
        bail!(
            "asked for the hardware adapter, got {name} (a software rasteriser); under WSL run with \
             GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl (and MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA)"
        );
    }
    Ok(())
}

/// A positive, finite span (`gpu_times` gives 0 for one not written or
/// running backwards).
fn real(ms: f64) -> Option<f64> {
    (ms.is_finite() && ms > 0.0).then_some(ms)
}

/// The frame's GPU ms, if the frame was timed and its span is real.
fn frame_sample(times: Option<GpuTimes>) -> Option<f64> {
    times.and_then(|g| real(g.frame_ms))
}

/// The probe passes' GPU ms of a frame drawn with `passes` (0, the
/// baseline: 0 ms), if the frame's span and the probe's are real.
fn probe_sample(times: Option<GpuTimes>, passes: u32) -> Option<f64> {
    let g = times?;
    real(g.frame_ms)?;
    if passes == 0 { Some(0.0) } else { real(g.probe_ms) }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

/// Plays on to the next recorded checksum and compares: the scene drawn is
/// the recorded match (a generated level made differently, on another
/// platform, would not be).
fn check_on(replay: &Replay, world: &mut World, out: &mut TickEvents, mut t: u32) -> anyhow::Result<()> {
    while t < replay.ticks() && !replay.checksums.iter().any(|&(ct, _)| ct == t) {
        world.tick(replay.row(t), out);
        t += 1;
    }
    if let Some(&(_, want)) = replay.checksums.iter().find(|&&(ct, _)| ct == t) {
        ensure!(world.checksum() == want, "the replay diverged by tick {t}: this is not the recorded match");
    }
    Ok(())
}

/// Measures `replay` at every size of `cfg`.
pub fn run(
    content: &Content,
    replay: &Replay,
    cfg: &BenchConfig,
    progress: &mut dyn FnMut(String),
) -> anyhow::Result<GpuReport> {
    ensure!(cfg.probe_passes.first().is_none_or(|&k| k == 0), "the probe counts start with 0 (the baseline)");
    ensure!(
        cfg.start_tick < replay.ticks(),
        "the replay has {} ticks, starting at {}",
        replay.ticks(),
        cfg.start_tick
    );
    let packs = &replay.setup.packs;
    let mut assets = LevelAssets::load(content, &packs.mod_pack.id, &packs.level.id)?;
    if cfg.look {
        let l = &mut assets.level;
        let (look, glow) = bench_look((l.material.width(), l.material.height()), l.art_scale);
        (l.look, l.glow) = (Some(look), Some(glow));
    }
    let gpu = Offscreen::open(&cfg.options)?;
    let info = gpu.adapter_info().clone();
    check_adapter(cfg.options.prefer, info.device_type, &info.name)?;
    let max_side = gpu.device().limits().max_texture_dimension_2d;
    if let Some(&(w, h)) = cfg.sizes.iter().find(|&&(w, h)| w > max_side || h > max_side) {
        bail!("{w}x{h}: {} draws at most {max_side} pixels a side", info.name);
    }
    let skin = content.skin_or_default("olx-skin-default");
    let mut report = GpuReport {
        adapter: info.name.clone(),
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        scene: format!("{} on {}", packs.mod_pack.id, packs.level.id),
        start_tick: cfg.start_tick,
        look: cfg.look,
        ..GpuReport::default()
    };
    let end = cfg.start_tick.saturating_add(cfg.frames).min(replay.ticks());
    for &size in &cfg.sizes {
        progress(format!("{}x{}: playing to tick {}", size.0, size.1, cfg.start_tick));
        let mut world = assets.replay_world(content, replay)?;
        let setup = PresentSetup {
            core: content.core(),
            m: &assets.m,
            level: &assets.level,
            theme: &assets.theme,
            viewport: Viewport::GAME,
            options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
            seed: replay.setup.seed,
        };
        let mut present = Presentation::new(&setup, &world);
        let mut renderer = gpu.renderer(RenderSettings::default());
        report.timestamps = renderer.enable_gpu_timing();
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("gpu-bench frame"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let (mut out, mut list, mut shown) = (TickEvents::new(), DrawList::new(), 0);
        let mut cursor = Cursor::default();
        while cursor.tick() < cfg.start_tick {
            if !cursor.step(replay, &mut world, &mut out).map_err(|m| anyhow!("the replay diverged: {m}"))? {
                break;
            }
            while shown < world.worms().len() {
                present.add_worm(&skin, default_colour(shown), shown == 0, false);
                shown += 1;
            }
            present.on_tick(&world, &out);
        }
        // A first frame makes the world target and uploads everything.
        present.build(&world, 1.0, &mut list);
        present.upload(&mut renderer);
        renderer.render_lit(&list, present.look(), &view, [0, 0, size.0, size.1]);
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
        let _ = renderer.gpu_times();
        let (mut tick, mut cpu, mut frame, mut total, mut gpu_ms) = (vec![], vec![], vec![], vec![], vec![]);
        let mut gpu_dropped = 0;
        for t in cfg.start_tick..end {
            let t0 = Instant::now();
            world.tick(replay.row(t), &mut out);
            while shown < world.worms().len() {
                present.add_worm(&skin, default_colour(shown), shown == 0, false);
                shown += 1;
            }
            present.on_tick(&world, &out);
            tick.push(ms(t0));
            let t1 = Instant::now();
            present.build(&world, 1.0, &mut list);
            present.upload(&mut renderer);
            renderer.render_lit(&list, present.look(), &view, [0, 0, size.0, size.1]);
            cpu.push(ms(t1));
            gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
            frame.push(ms(t1));
            total.push(ms(t0));
            match frame_sample(renderer.gpu_times()) {
                Some(g) => gpu_ms.push(g),
                None => gpu_dropped += u32::from(report.timestamps),
            }
        }
        check_on(replay, &mut world, &mut out, end)?;
        progress(format!("{}x{}: the probe", size.0, size.1));
        let mut probe = Vec::new();
        let mut base = 0.0;
        for &passes in &cfg.probe_passes {
            // The baseline too runs the probe's (untimed) seed, the drawn
            // frame copied into the first pass's source, so a frame's wall
            // time over the baseline is the passes alone.
            renderer.set_probe(Some(Probe { passes, taps: cfg.probe_taps }));
            let (mut wall, mut on_gpu, mut dropped) = (vec![], vec![], 0);
            for _ in 0..cfg.probe_frames {
                let t0 = Instant::now();
                renderer.render_lit(&list, present.look(), &view, [0, 0, size.0, size.1]);
                gpu.device().poll(wgpu::PollType::wait_indefinitely()).context("the device")?;
                wall.push(ms(t0));
                match probe_sample(renderer.gpu_times(), passes) {
                    Some(g) => on_gpu.push(g),
                    None => dropped += u32::from(report.timestamps),
                }
            }
            if passes == 0 {
                base = Spread::of(&wall).p50;
            }
            let timed_on_gpu = report.timestamps && !on_gpu.is_empty();
            let ms = if timed_on_gpu {
                Spread::of(&on_gpu)
            } else {
                Spread::of(&wall.iter().map(|w| (w - base).max(0.0)).collect::<Vec<_>>())
            };
            probe.push(ProbePoint { passes, taps: cfg.probe_taps, ms, timed_on_gpu, gpu_dropped: dropped });
        }
        renderer.set_probe(None);
        for w in renderer.warnings().drain(..) {
            progress(format!("warning: {w}"));
        }
        report.sizes.push(SizeReport {
            size,
            frames: end - cfg.start_tick,
            tick_ms: Spread::of(&tick),
            cpu_ms: Spread::of(&cpu),
            frame_ms: Spread::of(&frame),
            total_ms: Spread::of(&total),
            gpu_ms: (!gpu_ms.is_empty()).then(|| Spread::of(&gpu_ms)),
            gpu_dropped,
            probe,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--look`'s scene (plan 15a Task 20): a dark ambient and dashed
    /// glowing strips in group 1, sized to the level; the same for the same
    /// level.
    #[test]
    fn the_bench_look_fills_the_level() {
        let (look, glow) = bench_look((504, 350), 2);
        assert!(look.ambient.intensity < 50 && look.groups.len() == 1);
        assert_eq!((glow.world, glow.art), ((504, 350), (1008, 700)));
        assert!(glow.fits((1008, 700), (504, 350)));
        let lit = glow.front.iter().filter(|&&g| g > 0).count();
        assert!(lit > 0 && lit < glow.front.len() / 10, "{lit}");
        assert!(glow.groups.iter().all(|&g| g <= 1) && glow.groups.contains(&1));
        assert_eq!(bench_look((504, 350), 2), (look, glow));
    }

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_sizes("1920x1080,3440x1440").unwrap(), [(1920, 1080), (3440, 1440)]);
        assert!(parse_sizes("1920").is_err() && parse_sizes("0x10").is_err() && parse_sizes("").is_err());
    }

    /// An unwritten or backwards timestamp span (`gpu_times` gives 0) is no
    /// sample: counted as dropped, never a 0 ms frame.
    #[test]
    fn only_real_gpu_spans_are_samples() {
        let t = |frame_ms, probe_ms| Some(GpuTimes { frame_ms, probe_ms });
        assert_eq!(frame_sample(t(0.5, 0.0)), Some(0.5));
        assert_eq!(frame_sample(t(0.0, 0.0)), None);
        assert_eq!(frame_sample(t(f64::NAN, 0.0)), None);
        assert_eq!(frame_sample(None), None);
        assert_eq!(probe_sample(t(0.5, 0.2), 4), Some(0.2));
        assert_eq!(probe_sample(t(0.5, 0.0), 4), None, "the probe's span was not written");
        assert_eq!(probe_sample(t(0.0, 0.2), 4), None, "nor the frame's");
        assert_eq!(probe_sample(t(0.5, 0.0), 0), Some(0.0), "the baseline runs no probe");
        assert_eq!(probe_sample(t(0.0, 0.0), 0), None);
    }

    /// Review Focus 2: llvmpipe is never "the GPU".
    #[test]
    fn a_software_adapter_is_refused_for_hardware_runs() {
        use wgpu::DeviceType as D;
        assert!(check_adapter(AdapterPreference::Hardware, D::Cpu, "llvmpipe").is_err());
        assert!(check_adapter(AdapterPreference::Integrated, D::Cpu, "llvmpipe").is_err());
        assert!(check_adapter(AdapterPreference::Integrated, D::IntegratedGpu, "Radeon").is_ok());
        assert!(check_adapter(AdapterPreference::Hardware, D::DiscreteGpu, "RTX").is_ok());
        assert!(check_adapter(AdapterPreference::Hardware, D::Other, "D3D12 (RTX)").is_ok());
        assert!(check_adapter(AdapterPreference::Software, D::Cpu, "lavapipe").is_ok());
        let why = check_adapter(AdapterPreference::Hardware, D::Cpu, "llvmpipe").unwrap_err().to_string();
        assert!(why.contains("GALLIUM_DRIVER=d3d12"), "{why}");
    }

    /// Review Focus 2: wgpu's GL backend marks only some software
    /// rasterisers `Cpu`; the others are refused by name.
    #[test]
    fn a_software_adapter_reported_as_other_is_refused_by_name() {
        use wgpu::DeviceType as D;
        for name in [
            "D3D12 (Microsoft Basic Render Driver)",
            "Microsoft Basic Render Driver",
            "softpipe",
            "llvmpipe (LLVM 21.1.8, 256 bits)",
            "lavapipe",
            "WARP",
            "SwiftShader Device (Subzero)",
        ] {
            assert!(check_adapter(AdapterPreference::Hardware, D::Other, name).is_err(), "{name}");
            assert!(check_adapter(AdapterPreference::Software, D::Other, name).is_ok(), "{name}");
        }
        for name in
            ["D3D12 (NVIDIA GeForce RTX 5070 Ti)", "NVIDIA GeForce RTX 5070 Ti", "AMD Radeon(TM) Graphics"]
        {
            assert!(check_adapter(AdapterPreference::Hardware, D::Other, name).is_ok(), "{name}");
        }
    }
}
