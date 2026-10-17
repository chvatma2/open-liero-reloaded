//! `olr-cli import`: `olr_import` (the OLX import pipeline, plan 7 ruling
//! Q3; only the CLI uses it since plan 13c) with the items the flags name,
//! into `--out`. Packs already there stay unless `--prune` says `--out` is
//! this import's alone.

use std::path::PathBuf;

use olr_import::{ImportOptions, ImportPlan, Progress, SkinSource};

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
    /// Import everything found in `--olx` (every mod, level, theme and skin,
    /// as the removed Import screen did). Cannot be combined with the item
    /// flags (`--mod`, `--level`, `--theme`, `--skins`, `--core`,
    /// `--extra-skins`).
    #[arg(
        long,
        conflicts_with_all = ["mods", "levels", "themes", "skins", "core", "extra_skins"]
    )]
    all: bool,
    /// Remove the packs in `--out` that this run did not produce, so `--out`
    /// is exactly this import (`cargo xtask fetch-olx-content` for
    /// `content/packs`). Without it, an import only adds or replaces packs.
    #[arg(long)]
    prune: bool,
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
    /// A directory of more skin PNGs from outside the game directory,
    /// imported with their own origin (`--extra-skins-source`,
    /// `--extra-skins-revision`).
    #[arg(long, requires_all = ["extra_skins_source", "extra_skins_revision"])]
    extra_skins: Option<PathBuf>,
    /// The origin source recorded for `--extra-skins`, such as "OpenLieroX 0.58 rc5".
    #[arg(long, requires = "extra_skins")]
    extra_skins_source: Option<String>,
    /// The origin revision recorded for `--extra-skins`.
    #[arg(long, requires = "extra_skins")]
    extra_skins_revision: Option<String>,
}

/// Returns `Ok(false)` if any item failed; the report lists every item either way.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let mut olx = args.olx.clone();
    let plan = if args.all {
        let found = olr_import::discover(&args.olx)?;
        for n in &found.notes {
            eprintln!("note: {n}");
        }
        olx = found.gamedir;
        found.plan
    } else {
        item_plan(args)
    };
    let options = ImportOptions { revision: args.revision.clone(), prune: args.prune };
    run_plan(&olx, &plan, args, &options)
}

/// The plan the item flags name.
fn item_plan(args: &Args) -> ImportPlan {
    ImportPlan {
        mods: args.mods.clone(),
        levels: args.levels.clone(),
        themes: args.themes.clone(),
        skins: args.skins,
        core: args.core.clone(),
        extra_skins: args
            .extra_skins
            .iter()
            .map(|dir| SkinSource {
                dir: dir.clone(),
                source: args.extra_skins_source.clone().unwrap_or_default(),
                revision: args.extra_skins_revision.clone().unwrap_or_default(),
            })
            .collect(),
    }
}

fn run_plan(
    olx: &std::path::Path,
    plan: &ImportPlan,
    args: &Args,
    options: &ImportOptions,
) -> anyhow::Result<bool> {
    let report = olr_import::import(olx, plan, &args.out, options, &mut |p| {
        if let Progress::Done { entry, .. } = p
            && let Some(e) = &entry.error
        {
            eprintln!("error: {}: {e}", entry.source);
        }
        true
    })?;
    for n in &report.notes {
        eprintln!("warning: {n}");
    }
    let failed = report.failed();
    println!(
        "imported {} packs, {failed} failed, {} warnings (see import-report.ron)",
        report.imported(),
        report.warnings()
    );
    Ok(failed == 0)
}
