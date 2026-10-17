//! The canonical formatter (`olr-cli pack fmt`): a pack's format files
//! rewritten as their canonical writers produce them. Foreign files are
//! never listed nor touched.

use std::path::Path;

use olr_pack::PackError;
use olr_vfs::{ContentSource, DirFs, VPath, walk};

use crate::pack::{Pack, SaveError, files, load, save};

/// The files of the pack at `dir` whose bytes differ from their
/// canonical form, sorted. Foreign files are never listed.
pub fn unformatted(dir: &Path) -> Result<Vec<VPath>, PackError> {
    changed(dir, &load(dir)?)
}

/// Rewrites the pack in canonical form (through `pack::save`) when
/// anything differs; returns the paths rewritten.
pub fn format(dir: &Path) -> Result<Vec<VPath>, SaveError> {
    let pack = load(dir)?;
    let changed = changed(dir, &pack)?;
    if !changed.is_empty() {
        save(dir, &pack)?;
    }
    Ok(changed)
}

/// The format files of `pack` (loaded from `dir`) that differ on disk,
/// and those on disk the canonical writer leaves out (an empty
/// `restrictions.ron`), which a save removes.
fn changed(dir: &Path, pack: &Pack) -> Result<Vec<VPath>, PackError> {
    let disk = DirFs::new(dir);
    let files = files(pack)?;
    let mut out: Vec<VPath> = walk(&disk, &VPath::root())?
        .into_iter()
        .filter(|p| p.as_str() != olr_pack::MANIFEST && !pack.other.contains_key(p) && !files.contains_key(p))
        .collect();
    for (path, bytes) in files {
        if pack.other.contains_key(&path) {
            continue;
        }
        let same = disk.read_max(&path, olr_pack::MAX_FILE_BYTES).is_ok_and(|b| b == bytes);
        if !same {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
