//! The import browser (issue #19): Import PNG…, Import sound… and Replace
//! with PNG… pick a file the host reads. The host lists a folder
//! (`ModCommand::List`, answered by `ModAction::Listed`) and reads the
//! file picked (`ModCommand::ReadFile`, answered by `ModAction::FileRead`);
//! the browser shows folders and the files the import takes, hidden
//! entries left out, as the level editor's browser does (`olr-editor`'s
//! `browser.rs`, followed rather than shared: its purposes are a level's).

use std::path::{Component, Path, PathBuf};

/// What a file picked is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Import {
    /// A new image (`gfx/<name>.png`, named after the file).
    Image,
    /// A new sound (`sfx/<name>`, WAV or Ogg).
    Sound,
    /// Image `path` replaced by the file (its frame count asked about when
    /// it changes).
    Replace(String),
}

impl Import {
    /// Whether a file named `name` is one this import takes.
    pub fn takes(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        match self {
            Import::Image | Import::Replace(_) => lower.ends_with(".png"),
            Import::Sound => lower.ends_with(".wav") || lower.ends_with(".ogg"),
        }
    }

    /// The browser's title.
    pub fn title(&self) -> &'static str {
        match self {
            Import::Image => "Import PNG",
            Import::Sound => "Import sound",
            Import::Replace(_) => "Replace with PNG",
        }
    }
}

/// One entry of a folder, as the host lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
}

/// The most entries shown (the level editor's `MAX_ENTRIES`).
pub const MAX_ENTRIES: usize = 10_000;

/// The browser open over the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Browser {
    import: Import,
    dir: PathBuf,
    entries: Vec<FileEntry>,
    selected: Option<String>,
    message: Option<String>,
}

impl Browser {
    pub(crate) fn new(import: Import, dir: PathBuf) -> Browser {
        Browser { import, dir, entries: Vec::new(), selected: None, message: None }
    }

    pub fn import(&self) -> &Import {
        &self.import
    }
    /// The folder shown.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
    /// The entry clicked last.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    /// Why the listing is not what was asked (it failed, or was cut).
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The entries shown: folders, then the files the import takes, each by
    /// name (case aside, then exactly); no hidden ones; at most
    /// [`MAX_ENTRIES`].
    pub fn shown(&self) -> Vec<&FileEntry> {
        let mut out: Vec<&FileEntry> = self
            .entries
            .iter()
            .filter(|e| !e.name.starts_with('.'))
            .filter(|e| e.is_dir || self.import.takes(&e.name))
            .collect();
        out.sort_by(|a, b| {
            (!a.is_dir, a.name.to_lowercase(), &a.name).cmp(&(!b.is_dir, b.name.to_lowercase(), &b.name))
        });
        out.truncate(MAX_ENTRIES);
        out
    }

    pub(crate) fn entry(&self, name: &str) -> Option<&FileEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub(crate) fn select(&mut self, name: String) {
        self.selected = Some(name);
    }

    /// The host's listing of `dir`: a failure keeps the old listing and
    /// says why; more than [`MAX_ENTRIES`] says the rest were cut.
    pub(crate) fn listed(&mut self, dir: PathBuf, entries: Result<Vec<FileEntry>, String>) {
        match entries {
            Ok(e) => {
                self.message = (e.len() > MAX_ENTRIES)
                    .then(|| format!("more than {MAX_ENTRIES} entries: the first are shown"));
                self.dir = dir;
                self.entries = e;
                self.selected = None;
            }
            Err(why) => self.message = Some(why),
        }
    }

    /// `path` (typed: relative to the folder shown) as a folder, `.` and
    /// `..` resolved by name.
    pub(crate) fn resolve(&self, path: &Path) -> PathBuf {
        let mut out = if path.is_absolute() { PathBuf::new() } else { self.dir.clone() };
        for c in path.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                c => out.push(c.as_os_str()),
            }
        }
        out
    }
}

/// A new asset's file name from a file picked: its stem in lowercase
/// ASCII letters, digits, `-` and `_` (others become `-`), `fallback`
/// when none is left, and `ext`.
pub fn asset_name(file: &str, fallback: &str, ext: &str) -> String {
    let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = stem.strip_suffix(".glow").unwrap_or(&stem);
    let mut name = String::new();
    for c in stem.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            name.push(c);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    let name: String = name.trim_matches('-').chars().take(48).collect();
    format!("{}.{ext}", if name.is_empty() { fallback } else { &name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_shows_folders_and_the_files_an_import_takes() {
        let mut b = Browser::new(Import::Sound, PathBuf::from("/a"));
        let e = |n: &str, d| FileEntry { name: n.into(), is_dir: d };
        b.listed(
            PathBuf::from("/a/b"),
            Ok(vec![
                e("z.ogg", false),
                e("A.WAV", false),
                e("pic.png", false),
                e(".h.wav", false),
                e("d", true),
            ]),
        );
        let names: Vec<&str> = b.shown().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["d", "A.WAV", "z.ogg"]);
        b.listed(PathBuf::from("/x"), Err("cannot list /x".into()));
        assert_eq!((b.dir(), b.message()), (Path::new("/a/b"), Some("cannot list /x")));
        assert_eq!(b.resolve(Path::new("../c/./d")), PathBuf::from("/a/c/d"));
        assert!(Import::Replace("gfx/a.png".into()).takes("X.PNG"));
    }

    #[test]
    fn asset_names_are_plain() {
        assert_eq!(asset_name("My Rocket!.png", "image", "png"), "my-rocket.png");
        assert_eq!(asset_name("boom.WAV", "sound", "wav"), "boom.wav");
        assert_eq!(asset_name("tip.glow.png", "image", "png"), "tip.png");
        assert_eq!(asset_name("???.png", "image", "png"), "image.png");
    }
}
