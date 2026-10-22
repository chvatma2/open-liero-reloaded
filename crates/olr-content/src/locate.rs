//! Where `olr-game` and `olr-server` find the content when `--content` is
//! not given: beside the executable first (an unpacked release zip started
//! from anywhere), else in the working directory (the repository). OLR's
//! own packs (`content/reloaded`, issue #7: the Reloaded levels) sit beside
//! the imported ones and are laid over them ([`bundled_roots`]).

use std::path::{Path, PathBuf};

/// The content directory, relative to the executable's directory or to the
/// working directory.
pub const CONTENT_DIR: &str = "content/packs";

/// The directory of OLR's own packs, beside a content directory (so
/// `content/reloaded` beside `content/packs`): hand-made, unlike the
/// imported packs, and shipped with them.
pub const RELOADED_DIR: &str = "reloaded";

/// The directory of the built-in level editor assets, beside a content
/// directory (so `content/assets` beside `content/packs`, issue #9):
/// read-only, shipped with the packs.
pub const ASSETS_DIR: &str = "assets";

/// The built-in assets' directory of content directory `content`
/// ([`ASSETS_DIR`] beside it; it may not exist).
pub fn assets_dir(content: &Path) -> Option<PathBuf> {
    content.parent().map(|dir| dir.join(ASSETS_DIR))
}

/// The directory of the built-in surface materials, beside a content
/// directory (so `content/materials` beside `content/packs`, issue #37):
/// read-only, shipped with the packs.
pub const MATERIALS_DIR: &str = "materials";

/// The built-in materials' directory of content directory `content`
/// ([`MATERIALS_DIR`] beside it; it may not exist).
pub fn materials_dir(content: &Path) -> Option<PathBuf> {
    content.parent().map(|dir| dir.join(MATERIALS_DIR))
}

/// `<exe dir>/content/packs` when that directory exists (`exe` is the
/// running executable's path, `std::env::current_exe()`), else
/// `content/packs` relative to the working directory.
pub fn default_content_dir(exe: Option<&Path>) -> PathBuf {
    // By components: Windows shows `<exe dir>\content\packs`.
    exe.and_then(Path::parent)
        .map(|dir| CONTENT_DIR.split('/').fold(dir.to_path_buf(), |d, c| d.join(c)))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from(CONTENT_DIR))
}

/// The bundled content roots of content directory `content`, highest
/// priority first (spec §9.4): OLR's own packs, [`RELOADED_DIR`] beside
/// `content` (skipped by `Content::open_layers` when it does not exist),
/// then `content`. The two share no pack id (a test of `olr-cli`'s), so
/// neither hides the other's; a user's packs go in front of both.
pub fn bundled_roots(content: &Path) -> Vec<PathBuf> {
    let reloaded = content.parent().map(|dir| dir.join(RELOADED_DIR));
    reloaded.into_iter().chain([content.to_path_buf()]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_beside_the_executable_wins() {
        let dir = tempfile::tempdir().unwrap();
        let packs = dir.path().join("content").join("packs");
        std::fs::create_dir_all(&packs).unwrap();
        let exe = dir.path().join("olr-server");
        assert_eq!(default_content_dir(Some(&exe)), packs);
    }

    #[test]
    fn without_it_the_working_directory_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("olr");
        assert_eq!(default_content_dir(Some(&exe)), PathBuf::from(CONTENT_DIR));
        assert_eq!(default_content_dir(None), PathBuf::from(CONTENT_DIR));
        // A file named like the directory is not content.
        std::fs::create_dir_all(dir.path().join("content")).unwrap();
        std::fs::write(dir.path().join("content").join("packs"), "x").unwrap();
        assert_eq!(default_content_dir(Some(&exe)), PathBuf::from(CONTENT_DIR));
        // An executable path without a directory (never current_exe's).
        assert_eq!(default_content_dir(Some(Path::new("olr"))), PathBuf::from(CONTENT_DIR));
    }

    #[test]
    fn the_reloaded_packs_lie_beside_the_content() {
        let content = Path::new("content").join("packs");
        assert_eq!(bundled_roots(&content), [Path::new("content").join("reloaded"), content.clone()]);
        assert_eq!(bundled_roots(Path::new("packs")), [PathBuf::from("reloaded"), PathBuf::from("packs")]);
        // A root has nothing beside it.
        assert_eq!(bundled_roots(Path::new("/")), [PathBuf::from("/")]);
    }
}
