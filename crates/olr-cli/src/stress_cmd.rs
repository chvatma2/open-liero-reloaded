//! `olr-cli stress`: one configuration of the engine stress test
//! (`olr_cli::stress`), its report printed and, with `--out`, written as RON
//! (`cargo xtask stress` and `full` read it).

use std::path::PathBuf;

use olr_cli::spec::Personality;
use olr_cli::stress::{self, Config};

/// Plays the stress match and prints its report.
#[derive(clap::Args)]
pub struct Args {
    /// The mod pack, such as `olx-mw-1-0`.
    #[arg(long = "mod", default_value = "olx-mw-1-0")]
    mod_id: String,
    /// The level pack (the largest: Castle Strike).
    #[arg(long = "level", default_value = "olx-level-castlestrike")]
    level_id: String,
    /// Chaos bots (2 to 32).
    #[arg(long, default_value_t = 32)]
    worms: usize,
    /// The projectile pool's size (OLX's is 3000).
    #[arg(long, default_value_t = 3000)]
    pool: usize,
    /// Seconds of match time.
    #[arg(long, default_value_t = 180.0)]
    seconds: f32,
    #[arg(long, default_value_t = 1)]
    seed: u32,
    /// Render a frame offscreen every this many ticks (0: never).
    #[arg(long, default_value_t = 100)]
    render_every: u32,
    /// The rendered frame's width and height.
    #[arg(long, default_value_t = 2560)]
    render_width: u32,
    #[arg(long, default_value_t = 1440)]
    render_height: u32,
    /// Do not play it again nor replay it.
    #[arg(long)]
    no_verify: bool,
    /// The cosmetic entities' pool (play's 16384; the envelope sweeps it).
    #[arg(long, default_value_t = olr_fx::PLAY_ENTITIES)]
    fx_cap: usize,
    /// Build the draw list every this many ticks, timed (0: only for rendered frames).
    #[arg(long, default_value_t = 0)]
    build_every: u32,
    /// Fail when peak memory passes this many MiB.
    #[arg(long, default_value_t = stress::MEMORY_CEILING_MIB)]
    memory_ceiling_mib: f64,
    /// Write the match's replay here.
    #[arg(long)]
    record: Option<PathBuf>,
    /// How every bot plays: `chaos` fires everything, `dm` (OLX's bot)
    /// finds routes through the area cache.
    #[arg(long, value_enum, default_value_t = Personality::Chaos)]
    personality: Personality,
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Write the report here as RON.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let content = olr_content::Content::open_bundled(&args.content)?;
    let spec = stress::spec(&args.mod_id, &args.level_id, args.worms, args.pool, args.seconds, args.seed);
    let cfg = Config {
        render_every: args.render_every,
        render_size: (args.render_width.max(1), args.render_height.max(1)),
        verify: !args.no_verify,
        fx_capacity: args.fx_cap.max(1),
        build_every: args.build_every,
        memory_ceiling_mib: args.memory_ceiling_mib,
        record: args.record.clone(),
        personality: args.personality,
        ..Config::new(spec)
    };
    let report = stress::run(&content, &cfg, &mut |line| eprintln!("  {line}"))?;
    print!("{}", report.text());
    if let Some(path) = &args.out {
        let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n")?;
    }
    Ok(true)
}
