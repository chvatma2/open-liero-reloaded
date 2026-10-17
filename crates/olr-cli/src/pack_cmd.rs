//! `olr-cli pack …`: checking packs (plan 13d).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use olr_pack::Severity;
use olr_vfs::DirFs;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: PackCommand,
}

#[derive(Subcommand)]
enum PackCommand {
    /// Parse, compile and validate packs; exits with status 1 if any pack has errors.
    Check {
        /// Pack directories, or directories containing pack directories.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        PackCommand::Check { paths } => check(paths),
    }
}

/// Pack directories under `path`: `path` itself if it has `pack.ron`, else its
/// immediate subdirectories that do.
fn packs(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.join(olr_pack::MANIFEST).is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let p = entry?.path();
        if p.join(olr_pack::MANIFEST).is_file() {
            out.push(p);
        }
    }
    out.sort();
    if out.is_empty() {
        anyhow::bail!("{}: no pack.ron here or in any subdirectory", path.display());
    }
    Ok(out)
}

fn check(paths: &[PathBuf]) -> anyhow::Result<bool> {
    let (mut checked, mut errors, mut warnings) = (0, 0, 0);
    for path in paths {
        for pack in packs(path)? {
            checked += 1;
            for d in olr_pack::validate(&DirFs::new(&pack)) {
                match d.severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                }
                println!("{}: {d}", pack.display());
            }
        }
    }
    println!("checked {checked} packs: {errors} errors, {warnings} warnings");
    Ok(errors == 0)
}
