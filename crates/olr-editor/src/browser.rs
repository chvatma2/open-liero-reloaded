//! The file browser's state (plan 14c R9, spec §4 "Files and safety"): a
//! small egui browser instead of a native dialog, the same on WSL and
//! Windows and tested headless. The host lists directories
//! (`EditorCommand::List`, answered by `EditorAction::Listed`) and reads the
//! file chosen (`EditorCommand::ReadImage`); nothing here touches the disk.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// What the browser is choosing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A PNG or JPEG to import as an image element.
    ImportImage,
    /// A level pack's folder to open.
    OpenPack,
}

/// One entry of a listed directory, as the host found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    /// A folder holding `pack.ron`.
    pub is_pack: bool,
}

/// The file extensions an import offers (case aside).
const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];

/// The browser's state: its purpose, its directory, the entries of the
/// last listing that succeeded (folders first, then by name, case aside)
/// and what the last listing said when it failed.
#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
    purpose: Purpose,
    dir: PathBuf,
    entries: Vec<DirEntry>,
    message: Option<String>,
}

impl Browser {
    /// A browser at `dir`, empty until its first listing.
    pub(crate) fn new(purpose: Purpose, dir: PathBuf) -> Browser {
        Browser { purpose, dir, entries: Vec::new(), message: None }
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// The directory shown.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Why the last listing failed (the old one stays shown).
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The entries shown: folders, and for `ImportImage` the `.png`, `.jpg`
    /// and `.jpeg` files (case aside); hidden (`.`) entries left out.
    pub fn shown(&self) -> Vec<&DirEntry> {
        self.entries
            .iter()
            .filter(|e| !e.name.starts_with('.'))
            .filter(|e| {
                e.is_dir
                    || self.purpose == Purpose::ImportImage
                        && Path::new(&e.name)
                            .extension()
                            .and_then(|x| x.to_str())
                            .is_some_and(|x| IMAGE_EXTENSIONS.iter().any(|i| x.eq_ignore_ascii_case(i)))
            })
            .collect()
    }

    /// A shown entry by name.
    pub(crate) fn entry(&self, name: &str) -> Option<&DirEntry> {
        self.shown().into_iter().find(|e| e.name == name)
    }

    /// A listing's answer: `Ok` shows `dir` with its entries, sorted; `Err`
    /// keeps the old listing and says why.
    pub(crate) fn listed(&mut self, dir: PathBuf, entries: Result<Vec<DirEntry>, String>) {
        match entries {
            Ok(mut entries) => {
                entries.sort_by(order);
                self.dir = dir;
                self.entries = entries;
                self.message = None;
            }
            Err(why) => self.message = Some(why),
        }
    }

    /// `path` from this browser: as typed when absolute, else under its
    /// directory.
    pub(crate) fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() { path.to_path_buf() } else { self.dir.join(path) }
    }
}

/// Folders first, then by name ignoring case, then by the exact name (a
/// total order: the listing is the same whatever the disk's order).
fn order(a: &DirEntry, b: &DirEntry) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}
