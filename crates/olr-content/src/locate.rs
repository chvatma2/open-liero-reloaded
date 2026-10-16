//! Where `olr-game` and `olr-server` find the content when `--content` is
//! not given: beside the executable first (an unpacked release zip started
//! from anywhere), else in the working directory (the repository).

use std::path::{Path, PathBuf};

/// The content directory, relative to the executable's directory or to the
/// working directory.
pub const CONTENT_DIR: &str = "content/packs";

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
}
