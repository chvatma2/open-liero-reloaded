//! The canonical formatter (`olr-cli pack fmt`): a pack's format files
//! rewritten as their canonical writers produce them. Foreign files are
//! never listed nor touched.

use std::collections::BTreeMap;
use std::path::Path;

use olr_pack::PackError;
use olr_vfs::{ContentSource, DirFs, VPath, walk};

use crate::pack::{Pack, SaveError, check, files, load, save};

/// The files of the pack at `dir` whose bytes differ from their
/// canonical form, and those on disk the canonical writer leaves out,
/// sorted. Foreign files are never listed. When any are, the pack must be
/// one `format` would save: otherwise its errors (`SaveError::Invalid`).
pub fn unformatted(dir: &Path) -> Result<Vec<VPath>, SaveError> {
    let pack = load(dir)?;
    let files = files(&pack)?;
    let changed = changed(dir, &pack, &files)?;
    if !changed.is_empty() {
        check(&files)?;
    }
    Ok(changed)
}

/// What [`format`] did.
#[derive(Debug, Default, PartialEq)]
pub struct Formatted {
    /// The paths rewritten or removed, sorted.
    pub files: Vec<VPath>,
    /// The save's warnings (an old copy that could not be removed).
    pub warnings: Vec<String>,
}

/// Rewrites the pack in canonical form (through `pack::save`) when
/// anything differs.
pub fn format(dir: &Path) -> Result<Formatted, SaveError> {
    let pack = load(dir)?;
    let files = changed(dir, &pack, &files(&pack)?)?;
    let warnings = if files.is_empty() { Vec::new() } else { save(dir, &pack)? };
    Ok(Formatted { files, warnings })
}

/// The format files of `pack` (loaded from `dir`, written as `files`) that
/// differ on disk, and those on disk the canonical writer leaves out (an
/// empty `restrictions.ron`), which a save removes.
fn changed(dir: &Path, pack: &Pack, files: &BTreeMap<VPath, Vec<u8>>) -> Result<Vec<VPath>, PackError> {
    let disk = DirFs::new(dir);
    let mut out: Vec<VPath> = walk(&disk, &VPath::root())?
        .into_iter()
        .filter(|p| p.as_str() != olr_pack::MANIFEST && !pack.other.contains_key(p) && !files.contains_key(p))
        .collect();
    for (path, bytes) in files {
        if pack.other.contains_key(path) {
            continue;
        }
        let same = disk.read_max(path, olr_pack::MAX_FILE_BYTES).is_ok_and(|b| b == *bytes);
        if !same {
            out.push(path.clone());
        }
    }
    out.sort();
    Ok(out)
}
