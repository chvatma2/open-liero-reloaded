//! `olr-cli render-profile`: the staged nuke fight drawn offscreen on the
//! chosen adapter, its frames' costs ranked by phase
//! (`olr_cli::render_profile`), printed and, with `--out`, written as RON;
//! `--against` an earlier `--out` adds each cost's change.

use std::path::PathBuf;

use anyhow::Context as _;
use olr_cli::gpu_bench::parse_sizes;
use olr_cli::render_profile::{self, FRAMES_PER_TICK, ProfileConfig, ProfileReport};
use olr_render::OffscreenOptions;

use crate::gpu_bench_cmd::{Adapter, Backend};

/// Profiles the staged nuke fight's frames on the GPU.
#[derive(clap::Args)]
pub struct Args {
    /// Output sizes, `WIDTHxHEIGHT` comma-separated.
    #[arg(long, default_value = "1920x1080,3440x1440,3840x2160")]
    sizes: String,
    /// Frames drawn a tick (100 ticks a second).
    #[arg(long, default_value_t = FRAMES_PER_TICK)]
    frames_per_tick: u32,
    /// Only these ticks drawn, `FROM..TO` (the others played undrawn).
    #[arg(long, value_parser = parse_ticks)]
    ticks: Option<(u32, u32)>,
    #[arg(long, value_enum, default_value = "hardware")]
    adapter: Adapter,
    #[arg(long, value_enum, default_value = "auto")]
    backend: Backend,
    /// Imported content, OLR's own packs (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Write the report as RON.
    #[arg(long)]
    out: Option<PathBuf>,
    /// An earlier `--out` to compare with.
    #[arg(long)]
    against: Option<PathBuf>,
    /// Write the fight's replay (for the game's `--replay`).
    #[arg(long)]
    save_replay: Option<PathBuf>,
}

fn parse_ticks(s: &str) -> Result<(u32, u32), String> {
    let (a, b) = s.split_once("..").ok_or_else(|| format!("{s:?} is not FROM..TO"))?;
    let (a, b): (u32, u32) =
        (a.trim().parse().map_err(|_| format!("{a:?}"))?, b.trim().parse().map_err(|_| format!("{b:?}"))?);
    if a >= b {
        return Err(format!("{s:?}: FROM must be below TO"));
    }
    Ok((a, b))
}

/// The base to compare with, or why there is none (never an error).
fn base(path: &std::path::Path) -> Result<ProfileReport, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let content = olr_content::Content::open_bundled(&args.content)?;
    eprintln!("  staging the nuke fight");
    let fight = render_profile::stage_fight(&content)?;
    if let Some(path) = &args.save_replay {
        std::fs::write(path, fight.replay.encode()).with_context(|| format!("writing {}", path.display()))?;
    }
    let cfg = ProfileConfig {
        sizes: parse_sizes(&args.sizes)?,
        frames_per_tick: args.frames_per_tick.max(1),
        options: OffscreenOptions {
            prefer: args.adapter.preference(),
            backends: args.backend.backends(),
            timestamps: true,
            max_texture_side: None,
        },
        window: args.ticks,
    };
    let report = render_profile::run(&content, &fight, &cfg, &mut |l| eprintln!("  {l}"))?;
    let against = args.against.as_deref().and_then(|p| match base(p) {
        Ok(b) => Some(b),
        Err(why) => {
            println!("no base to compare against: {why}");
            None
        }
    });
    print!("{}", render_profile::table(&report, against.as_ref()));
    if let Some(path) = &args.out {
        let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ticks_parse() {
        assert_eq!(super::parse_ticks("180..340"), Ok((180, 340)));
        assert!(super::parse_ticks("340..180").is_err() && super::parse_ticks("12").is_err());
    }
}
