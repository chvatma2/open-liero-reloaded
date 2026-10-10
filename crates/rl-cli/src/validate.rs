use std::path::{Path, PathBuf};

use rl_pack::Severity;
use rl_vfs::DirFs;

/// Pack directories under `path`: `path` itself if it has `pack.ron`, else its
/// immediate subdirectories that do.
fn packs(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.join(rl_pack::MANIFEST).is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let p = entry?.path();
        if p.join(rl_pack::MANIFEST).is_file() {
            out.push(p);
        }
    }
    out.sort();
    if out.is_empty() {
        anyhow::bail!("{}: no pack.ron here or in any subdirectory", path.display());
    }
    Ok(out)
}

pub fn run(paths: &[PathBuf]) -> anyhow::Result<bool> {
    let (mut checked, mut errors, mut warnings) = (0, 0, 0);
    for path in paths {
        for pack in packs(path)? {
            checked += 1;
            for d in rl_pack::validate(&DirFs::new(&pack)) {
                match d.severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                }
                println!("{}: {d}", pack.display());
            }
        }
    }
    println!("validated {checked} packs: {errors} errors, {warnings} warnings");
    Ok(errors == 0)
}
