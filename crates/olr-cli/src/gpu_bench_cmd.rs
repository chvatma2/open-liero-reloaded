//! `olr-cli gpu-bench`: a replay drawn offscreen at several sizes on the
//! chosen adapter, timed on the CPU and the GPU, with the shader probe
//! (`olr_cli::gpu_bench`); the report printed and, with `--out`, written as
//! RON (`cargo xtask envelope` reads it).

use std::path::PathBuf;

use anyhow::Context as _;
use olr_cli::gpu_bench::{self, BenchConfig, parse_sizes};
use olr_render::{AdapterPreference, OffscreenOptions};

#[derive(Clone, Copy, clap::ValueEnum)]
enum Adapter {
    Hardware,
    /// An integrated GPU first: a laptop's GPU on a desktop (plan 15a Task 20).
    Integrated,
    Software,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Backend {
    Auto,
    Gl,
    Vulkan,
    Dx12,
}

/// Draws a recorded match offscreen and times every frame.
#[derive(clap::Args)]
pub struct Args {
    /// The replay to draw (`olr-cli stress --record`).
    #[arg(long, required_unless_present = "list_adapters")]
    replay: Option<PathBuf>,
    /// Output sizes, `WIDTHxHEIGHT` comma-separated.
    #[arg(long, default_value = "1920x1080,2560x1440,3440x1440,3840x2160")]
    sizes: String,
    #[arg(long, default_value_t = 1000)]
    start_tick: u32,
    #[arg(long, default_value_t = 600)]
    frames: u32,
    /// Probe pass counts (0, the baseline, is added first when missing).
    #[arg(long, value_delimiter = ',', default_value = "0,1,2,4,8,16,32")]
    probe_passes: Vec<u32>,
    #[arg(long, default_value_t = 9)]
    probe_taps: u32,
    #[arg(long, default_value_t = 120)]
    probe_frames: u32,
    #[arg(long, value_enum, default_value = "hardware")]
    adapter: Adapter,
    #[arg(long, value_enum, default_value = "auto")]
    backend: Backend,
    /// Imported content, OLR's own packs (the `reloaded` directory beside
    /// it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    #[arg(long)]
    out: Option<PathBuf>,
    /// Print every adapter wgpu finds and stop.
    #[arg(long)]
    list_adapters: bool,
    /// Draw the level lit: a dark ambient, a placed light every 96 world
    /// pixels, glowing strips, soft shadows, and the engine's lights of the
    /// replay's shots (plan 15a Task 20; `olr_cli::gpu_bench::bench_look`).
    #[arg(long)]
    look: bool,
    /// With `--look`: ambient occlusion in the bench look, STRENGTH[,REACH]
    /// (percent 1-100, world pixels 2-32, default 12; issue #10).
    #[arg(long, requires = "look", value_parser = parse_occlusion)]
    occlusion: Option<olr_look::Occlusion>,
    /// Views drawn each frame, 1 to 4 (issue #26, split screen): the
    /// replay's camera and views spread over the level, in the target's
    /// halves (2) or quarters (3, 4).
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=4))]
    views: u32,
}

fn parse_occlusion(s: &str) -> Result<olr_look::Occlusion, String> {
    let (strength, reach) = s.split_once(',').unwrap_or((s, "12"));
    let strength: u8 = strength.trim().parse().map_err(|_| format!("{strength:?} is not a percent"))?;
    let reach: u8 = reach.trim().parse().map_err(|_| format!("{reach:?} is not world pixels"))?;
    if !(1..=100).contains(&strength) {
        return Err(format!("strength {strength}, not within 1 to 100"));
    }
    let (lo, hi) = (olr_look::MIN_OCCLUSION_REACH, olr_look::MAX_OCCLUSION_REACH);
    if !(lo..=hi).contains(&reach) {
        return Err(format!("reach {reach}, not within {lo} to {hi}"));
    }
    Ok(olr_look::Occlusion { strength, reach })
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let backends = match args.backend {
        Backend::Auto => None,
        Backend::Gl => Some(wgpu::Backends::GL),
        Backend::Vulkan => Some(wgpu::Backends::VULKAN),
        Backend::Dx12 => Some(wgpu::Backends::DX12),
    };
    if args.list_adapters {
        for a in olr_render::list_adapters(backends) {
            println!("{} ({:?}, {:?})", a.name, a.backend, a.device_type);
        }
        return Ok(true);
    }
    let path = args.replay.as_ref().context("--replay")?;
    let bytes = std::fs::read(path).with_context(|| format!("{}", path.display()))?;
    let replay =
        olr_replay::Replay::decode(&bytes).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut probe_passes = args.probe_passes.clone();
    if probe_passes.first() != Some(&0) {
        probe_passes.insert(0, 0);
    }
    let cfg = BenchConfig {
        sizes: parse_sizes(&args.sizes)?,
        start_tick: args.start_tick,
        frames: args.frames,
        probe_passes,
        probe_taps: args.probe_taps,
        probe_frames: args.probe_frames,
        options: OffscreenOptions {
            prefer: match args.adapter {
                Adapter::Hardware => AdapterPreference::Hardware,
                Adapter::Integrated => AdapterPreference::Integrated,
                Adapter::Software => AdapterPreference::Software,
            },
            backends,
            timestamps: true,
            max_texture_side: None,
        },
        look: args.look,
        occlusion: args.occlusion,
        views: args.views,
    };
    let content = olr_content::Content::open_bundled(&args.content)?;
    let report = gpu_bench::run(&content, &replay, &cfg, &mut |l| eprintln!("  {l}"))?;
    println!(
        "{} ({}, {}); GPU times: {}",
        report.adapter,
        report.backend,
        report.device_type,
        if report.timestamps { "timestamps" } else { "none (CPU-side frame times only)" }
    );
    for s in &report.sizes {
        let gpu = s.gpu_ms.map_or("-".into(), |g| format!("{:.2}/{:.2}", g.p50, g.p99));
        println!(
            "  {}x{}: tick+frame p50/p99 {:.2}/{:.2} ms, frame {:.2}/{:.2}, gpu {gpu}; probe ms {:?}",
            s.size.0,
            s.size.1,
            s.total_ms.p50,
            s.total_ms.p99,
            s.frame_ms.p50,
            s.frame_ms.p99,
            s.probe.iter().map(|p| (p.passes, (p.ms.p50 * 1000.0).round() / 1000.0)).collect::<Vec<_>>()
        );
        let probe_dropped: u32 = s.probe.iter().map(|p| p.gpu_dropped).sum();
        if s.gpu_dropped + probe_dropped > 0 {
            println!(
                "    timestamps without a real span (left out): {} of {} frames, {probe_dropped} probe frames",
                s.gpu_dropped, s.frames
            );
        }
    }
    if let Some(out) = &args.out {
        let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())?;
        std::fs::write(out, text + "\n").with_context(|| format!("{}", out.display()))?;
    }
    Ok(true)
}
