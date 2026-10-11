//! `rl-cli import`: `rl_content::import` (the pipeline the game's Import
//! screen shares, plan 7 ruling Q3) with the items the flags name, into a
//! directory this import owns (`prune`).

use std::path::PathBuf;

use rl_content::import::{ImportOptions, ImportPlan, Progress};

#[derive(clap::Args)]
pub struct Args {
    /// OLX game directory (`share/gamedir` of an OpenLieroX checkout).
    #[arg(long)]
    olx: PathBuf,
    /// Output directory; each pack is written to `<out>/<pack id>/`.
    #[arg(long)]
    out: PathBuf,
    /// Source revision recorded in each pack's origin, such as a git commit.
    #[arg(long, default_value = "unknown")]
    revision: String,
    /// Mod directories relative to the game directory, such as "MW 1.0".
    #[arg(long = "mod")]
    mods: Vec<String>,
    /// Level files relative to the game directory, such as "levels/CastleStrike.lxl".
    #[arg(long = "level")]
    levels: Vec<String>,
    /// Theme directories relative to the game directory, such as "data/themes/Dirt".
    #[arg(long = "theme")]
    themes: Vec<String>,
    /// Import every PNG in `skins/`.
    #[arg(long)]
    skins: bool,
    /// Files relative to the game directory for the core pack `olx-core`
    /// (shared sprites, game sounds, `data/frontend/colours.cfg`).
    #[arg(long = "core")]
    core: Vec<String>,
}

/// Returns `Ok(false)` if any item failed; the report lists every item either way.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let plan = ImportPlan {
        mods: args.mods.clone(),
        levels: args.levels.clone(),
        themes: args.themes.clone(),
        skins: args.skins,
        core: args.core.clone(),
    };
    let options = ImportOptions { revision: args.revision.clone(), prune: true };
    let report = rl_content::import::import(&args.olx, &plan, &args.out, &options, &mut |p| {
        if let Progress::Done { entry, .. } = p
            && let Some(e) = &entry.error
        {
            eprintln!("error: {}: {e}", entry.source);
        }
        true
    })?;
    let failed = report.failed();
    println!(
        "imported {} packs, {failed} failed, {} warnings (see import-report.ron)",
        report.imported(),
        report.warnings()
    );
    Ok(failed == 0)
}
