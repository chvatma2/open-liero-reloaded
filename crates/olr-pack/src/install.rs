//! The names an import leaves in a content directory while it replaces a
//! pack (plan 13c task 4): `olr-import` writes them and `olr-content`'s
//! `open_layers` lists a pack an import killed halfway left aside.
//! `replace_dir` is that swap, shared by the importer and `olr-edit`'s
//! save (plan 13d R2).

use std::io;
use std::path::{Path, PathBuf};

/// A pack being written.
pub const TMP_PREFIX: &str = ".import-";
/// A pack being replaced (`olr_content::Content::open_layers` lists one
/// whose id has no other pack: an import killed between its two renames).
pub const OLD_PREFIX: &str = ".replaced-";

/// Sorts `paths` so the packs moved aside (`.replaced-<id>`,
/// `.replaced-<id>~k`) come first, newest first: by their manifest's
/// modification time (written by the import that made the pack), then by
/// the highest `k` (a later import takes a higher slot while a lower one
/// cannot be removed), then by name. Other paths keep their order after them.
pub fn newest_aside_first(paths: &mut [PathBuf]) {
    let key = |p: &PathBuf| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let rest = name.strip_prefix(OLD_PREFIX)?;
        let k = rest.rsplit_once('~').and_then(|(_, k)| k.parse::<u32>().ok()).unwrap_or(1);
        let time = std::fs::metadata(p.join(crate::MANIFEST))
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        Some((std::cmp::Reverse(time), std::cmp::Reverse(k)))
    };
    // `None` (not moved aside) sorts after every `Some`.
    paths.sort_by_cached_key(|p| (key(p).is_none(), key(p), p.clone()));
}

/// Replaces the directory `dir` with what `write` puts into a fresh
/// hidden sibling (`<parent>/.import-<name>`): the old directory is
/// renamed aside (`.replaced-<name>`, `~k` when taken), the new one
/// renamed into place, the old one removed. `write` or a rename failing
/// removes the sibling and leaves `dir` the old directory (renamed back).
/// Returns warnings (an old copy that could not be removed).
pub fn replace_dir(dir: &Path, write: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<Vec<String>> {
    replace_dir_with(dir, write, |from, to| std::fs::rename(from, to))
}

/// [`replace_dir`] with its renames done by `rename` (tests fail them).
fn replace_dir_with(
    dir: &Path,
    write: impl FnOnce(&Path) -> io::Result<()>,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<Vec<String>> {
    let name = dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("{}: not a directory name", dir.display()))
    })?;
    let parent = dir.parent().unwrap_or(Path::new(""));
    let tmp = parent.join(format!("{TMP_PREFIX}{name}"));
    remove_dir(&tmp)?;
    let written = std::fs::create_dir_all(&tmp).and_then(|()| write(&tmp));
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(context(e, format!("writing {}", dir.display())));
    }
    // From here on a failure puts the old directory back and removes the
    // hidden copy: `dir` is the old pack or the new one, never neither.
    let mut old = None;
    if dir.exists() {
        // `.replaced-<name>`, or `.replaced-<name>~2`, ... when an earlier
        // one could not be removed.
        let aside = (1..)
            .map(|k| {
                parent.join(if k == 1 {
                    format!("{OLD_PREFIX}{name}")
                } else {
                    format!("{OLD_PREFIX}{name}~{k}")
                })
            })
            .take(100)
            .find(|p| remove_dir(p).is_ok());
        let moved = match aside {
            None => Err(io::Error::other(format!("no place to move the old {} aside", dir.display()))),
            Some(aside) => rename(dir, &aside)
                .map(|()| aside)
                .map_err(|e| context(e, format!("moving the old {} aside", dir.display()))),
        };
        match moved {
            Ok(aside) => old = Some(aside),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
        }
    }
    if let Err(e) = rename(&tmp, dir) {
        if let Some(aside) = &old {
            let _ = rename(aside, dir);
        }
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(context(e, format!("moving {} into place", dir.display())));
    }
    Ok(match old.map(|o| remove_dir(&o)) {
        Some(Err(e)) => vec![format!("{e}: the next save or import of this pack tries again")],
        _ => Vec::new(),
    })
}

/// Removes `dir` and everything in it; a directory that is not there is fine.
fn remove_dir(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(context(e, format!("removing {}", dir.display())))
        }
        _ => Ok(()),
    }
}

/// `e` with `what` in front of its message, as `anyhow`'s `{:#}` prints a context.
fn context(e: io::Error, what: String) -> io::Error {
    io::Error::new(e.kind(), format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_dir_swaps_in_the_new_contents() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("old.txt"), b"old").unwrap();
        let warnings = replace_dir(&dir, |d| std::fs::write(d.join("new.txt"), b"new")).unwrap();
        assert!(warnings.is_empty());
        assert!(!dir.join("old.txt").exists());
        assert_eq!(std::fs::read(dir.join("new.txt")).unwrap(), b"new");
        let names: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("p")], "no hidden sibling left");
    }

    /// Review A-2: the new pack cannot be renamed into place: the old one
    /// goes back, and nothing hidden is left.
    #[test]
    fn a_failed_swap_puts_the_old_pack_back() {
        for failing in [1, 2] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("p");
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(dir.join("old.txt"), b"old").unwrap();
            let mut calls = 0;
            let result = replace_dir_with(
                &dir,
                |d| std::fs::write(d.join("new.txt"), b"new"),
                |from, to| {
                    calls += 1;
                    if calls == failing { Err(io::Error::other("busy")) } else { std::fs::rename(from, to) }
                },
            );
            assert!(result.is_err(), "rename {failing}");
            assert_eq!(std::fs::read(dir.join("old.txt")).unwrap(), b"old", "rename {failing}");
            assert!(!dir.join("new.txt").exists());
            let names: Vec<_> =
                std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
            assert_eq!(names, vec![std::ffi::OsString::from("p")], "rename {failing}: nothing hidden left");
        }
    }

    /// An old copy that cannot be removed is a warning, worded for any
    /// caller (an import or an editor's save).
    #[cfg(unix)]
    #[test]
    fn an_old_copy_left_behind_is_a_warning() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        std::fs::create_dir_all(dir.join("locked")).unwrap();
        std::fs::write(dir.join("locked/x"), b"x").unwrap();
        std::fs::set_permissions(dir.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
        let warnings = replace_dir(&dir, |d| std::fs::write(d.join("new.txt"), b"new")).unwrap();
        let aside = tmp.path().join(".replaced-p");
        std::fs::set_permissions(aside.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].ends_with(": the next save or import of this pack tries again"), "{warnings:?}");
        assert_eq!(std::fs::read(dir.join("new.txt")).unwrap(), b"new");
    }

    #[test]
    fn a_failed_write_leaves_the_old_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("old.txt"), b"old").unwrap();
        let err = replace_dir(&dir, |d| {
            std::fs::write(d.join("half.txt"), b"x")?;
            Err(std::io::Error::other("disk full"))
        });
        assert!(err.is_err());
        assert_eq!(std::fs::read(dir.join("old.txt")).unwrap(), b"old");
        assert!(!dir.join("half.txt").exists());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }
}
