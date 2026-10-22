//! `olr-cli frame-hashes`: a bot match's last frames drawn lit on the
//! software adapter, a hash of each frame's light grid and lit target
//! (`olr_cli::frame_hash`, issue #34), printed and, with `--out`, written as
//! RON (`fixtures/render/lit-hashes.ron` is one).

use std::path::PathBuf;

use anyhow::Context as _;
use olr_cli::frame_hash;
use olr_cli::spec::MatchSpec;

/// Plays a headless bot match and hashes the exact bytes of its last frames, drawn lit.
#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    spec: MatchSpec,
    /// The last ticks drawn and hashed, one frame each.
    #[arg(long, default_value_t = 30)]
    frames: u32,
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Write the hashes as RON.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let content = olr_content::Content::open_bundled(&args.content)?;
    let gpu = olr_render::Offscreen::new()?;
    let hashes = frame_hash::record(&content, &args.spec, args.frames, &gpu)?;
    println!(
        "{} frames on {}: light grid {}x{}, lit target {}x{}",
        hashes.frames.len(),
        hashes.adapter,
        hashes.light_size.0,
        hashes.light_size.1,
        hashes.lit_size.0,
        hashes.lit_size.1
    );
    for f in &hashes.frames {
        println!("  tick {}: world {} light {} lit {} ({} discs)", f.tick, f.world, f.light, f.lit, f.discs);
    }
    if let Some(out) = &args.out {
        let text = ron::ser::to_string_pretty(&hashes, ron::ser::PrettyConfig::default())?;
        std::fs::write(out, text + "\n").with_context(|| format!("{}", out.display()))?;
    }
    Ok(true)
}
