//! `olr-cli rollback-cost`: the cost of a rollback (`olr_cli::rollback_cost`),
//! printed and, with `--out`, written as RON (`cargo xtask envelope` reads it).

use std::path::PathBuf;

use olr_cli::rollback_cost;

/// Times restoring the world and re-simulating `depth` ticks, over samples
/// taken every few ticks of a bot match.
#[derive(clap::Args)]
pub struct Args {
    /// The mod pack.
    #[arg(long = "mod", default_value = "olx-classic")]
    mod_id: String,
    /// The level pack, or a generated level's id (`gen-4096x4096-a1-s1`).
    #[arg(long = "level", default_value = "olx-level-castlestrike")]
    level_id: String,
    /// Chaos bots (2 to 32).
    #[arg(long, default_value_t = 16)]
    worms: usize,
    /// Ticks restored and re-simulated.
    #[arg(long, default_value_t = 12)]
    depth: u32,
    /// How many times it is measured.
    #[arg(long, default_value_t = 300)]
    samples: usize,
    /// Imported content (packs written by `olr-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Write the report here as RON.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let content = olr_content::Content::open(&args.content)?;
    let r =
        rollback_cost::measure(&content, &args.mod_id, &args.level_id, args.worms, args.depth, args.samples)?;
    print!("{}", r.text());
    if let Some(path) = &args.out {
        let text = ron::ser::to_string_pretty(&r, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n")?;
    }
    Ok(true)
}
