//! The canonical formatter (`olr-cli pack fmt`): a pack's format files
//! rewritten as their canonical writers produce them. Foreign files are
//! never listed nor touched.

use std::path::Path;

use olr_pack::PackError;
use olr_vfs::{ContentSource, DirFs, VPath};

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

/// The format files of `pack` (loaded from `dir`) that differ on disk.
fn changed(dir: &Path, pack: &Pack) -> Result<Vec<VPath>, PackError> {
    let disk = DirFs::new(dir);
    let mut out = Vec::new();
    for (path, bytes) in files(pack)? {
        if pack.other.contains_key(&path) {
            continue;
        }
        let same = disk.read_max(&path, olr_pack::MAX_FILE_BYTES).is_ok_and(|b| b == bytes);
        if !same {
            out.push(path);
        }
    }
    Ok(out)
}
