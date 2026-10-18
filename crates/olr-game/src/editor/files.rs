//! The editor host's disk work (plan 14c Task 5): the level packs of the
//! data directory's `packs/`, a directory listed for the file browser, and
//! an image read for an import, bounded before it is read whole.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use olr_data::manifest::PackKind;
use olr_editor::browser::DirEntry;

/// The largest image an import reads (from its metadata, before reading).
pub const MAX_IMPORT_BYTES: u64 = 64 << 20;

/// The most entries a listing shows (`olr-editor`'s).
pub use olr_editor::browser::MAX_ENTRIES;

/// The header an import looks at before reading the rest.
const HEADER_BYTES: u64 = 16;

/// The level packs in `packs_dir` by manifest id (a pack directory's name is
/// its id when the editor made it; one someone put there may differ), in
/// directory name order: the first of two with one id wins. Unreadable or
/// hidden (`.`) entries are skipped.
pub fn level_dirs(packs_dir: &Path) -> BTreeMap<String, PathBuf> {
    let Ok(rd) = std::fs::read_dir(packs_dir) else { return BTreeMap::new() };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut out = BTreeMap::new();
    for dir in dirs {
        let Ok(m) = olr_pack::read_manifest(&olr_vfs::DirFs::new(&dir)) else { continue };
        if m.kind == PackKind::Level {
            out.entry(m.id).or_insert(dir);
        }
    }
    out
}

/// The level pack ids in `packs_dir` (by manifest), for
/// `MenuData::set_editor_packs`.
pub fn editable_levels(packs_dir: &Path) -> BTreeSet<String> {
    level_dirs(packs_dir).into_keys().collect()
}

/// `dir`'s entries for the browser (`EditorCommand::List`), a pack being a
/// folder holding `pack.ron`: every name read, sorted as the browser shows
/// them (each name lowercased once), then the first `MAX_ENTRIES` and one
/// more kept (the browser shows the first and says the rest were cut). An
/// entry's kind comes from the directory listing itself (no `stat`, but
/// for a symbolic link); only a folder's `pack.ron` is looked for.
pub fn list(dir: &Path) -> Result<Vec<DirEntry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
    let mut entries: Vec<(DirEntry, PathBuf)> = rd
        .filter_map(Result::ok)
        .map(|e| {
            let path = e.path();
            let is_dir = match e.file_type() {
                Ok(t) if t.is_symlink() => path.is_dir(),
                Ok(t) => t.is_dir(),
                Err(_) => false,
            };
            let name = e.file_name().to_string_lossy().into_owned();
            (DirEntry { name, is_dir, is_pack: false }, path)
        })
        .collect();
    entries.sort_by_cached_key(|e| olr_editor::browser::sort_key(&e.0));
    // One more than shown: the browser says the rest were cut.
    entries.truncate(MAX_ENTRIES + 1);
    Ok(entries
        .into_iter()
        .map(|(mut e, path)| {
            e.is_pack = e.is_dir && path.join(olr_pack::MANIFEST).is_file();
            e
        })
        .collect())
}

/// The image at `path` for an import (`EditorCommand::ReadImage`): its file
/// name and bytes. A file above [`MAX_IMPORT_BYTES`] by its metadata, or
/// not a regular file (a FIFO never blocks), is refused before it is
/// opened; one whose first bytes are no PNG or JPEG signature before the
/// rest is read; one whose header gives no size after.
pub fn read_image(path: &Path) -> Result<(String, Vec<u8>), String> {
    let name =
        path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let not_image = || format!("{name} is not a PNG or JPEG");
    let meta = std::fs::metadata(path).map_err(|e| format!("{name}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{name} is not a file"));
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(format!(
            "{name} is {} MiB: an imported image may be at most {} MiB",
            meta.len().div_ceil(1 << 20),
            MAX_IMPORT_BYTES >> 20
        ));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("{name}: {e}"))?;
    let mut head = Vec::new();
    let mut r = file.take(MAX_IMPORT_BYTES + 1);
    (&mut r).take(HEADER_BYTES).read_to_end(&mut head).map_err(|e| format!("{name}: {e}"))?;
    let png = head.starts_with(b"\x89PNG\r\n\x1a\n");
    let jpeg = head.starts_with(&[0xFF, 0xD8, 0xFF]);
    if !png && !jpeg {
        return Err(not_image());
    }
    let mut bytes = head;
    r.read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
    if bytes.len() as u64 > MAX_IMPORT_BYTES {
        return Err(format!("{name} grew above {} MiB while it was read", MAX_IMPORT_BYTES >> 20));
    }
    olr_format::image_io::image_size(&bytes).ok_or_else(not_image)?;
    Ok((name, bytes))
}

/// Whether `dir` lies inside one of `roots` (each compared canonically,
/// symbolic links resolved; a directory that does not exist yet, `dir` or
/// a root, by its nearest existing ancestor). A `dir` that resolves
/// nowhere counts as inside.
pub fn inside(dir: &Path, roots: &[PathBuf]) -> bool {
    let canon = |p: &Path| -> Option<PathBuf> {
        let mut at = p.to_path_buf();
        let mut rest = Vec::new();
        loop {
            if let Ok(c) = at.canonicalize() {
                return Some(rest.iter().rev().fold(c, |c: PathBuf, n: &std::ffi::OsString| c.join(n)));
            }
            rest.push(at.file_name()?.to_os_string());
            at = at.parent()?.to_path_buf();
        }
    };
    // A path that resolves nowhere is treated as inside: refused, never
    // written to (fail closed).
    let Some(dir) = canon(dir) else { return true };
    roots.iter().filter_map(|r| canon(r)).any(|r| dir.starts_with(r))
}
