//! The file browser's state (plan 14c R9, spec §4 "Files and safety"): a
//! small egui browser instead of a native dialog, the same on WSL and
//! Windows and tested headless. The host lists directories
//! (`EditorCommand::List`, answered by `EditorAction::Listed`) and reads the
//! file chosen (`EditorCommand::ReadImage`); nothing here touches the disk.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use olr_data::image::RgbaImage;

/// What the browser is choosing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A PNG or JPEG to import as an image element.
    ImportImage,
    /// A level pack's folder to open.
    OpenPack,
    /// A PNG or JPEG to add to the user's textures (plan 14e R2).
    ImportTexture,
    /// An asset folder (holding `asset.ron`) to add to the user's assets
    /// (issue #9).
    ImportAsset,
    /// A WAV or Ogg sound to add to the asset tab's asset (issue #80).
    ImportSound,
    /// Where to write the level's export (issue #84): the browser's one
    /// save purpose, with a File name field; folders and `.olrlevel` files
    /// shown.
    ExportLevel,
    /// A `.olrlevel` file to import (issue #84).
    ImportLevel,
}

impl Purpose {
    /// Whether it picks an image file (shown, and previewed).
    pub fn images(self) -> bool {
        matches!(self, Purpose::ImportImage | Purpose::ImportTexture)
    }
}

/// One entry of a listed directory, as the host found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    /// A folder holding `pack.ron`.
    pub is_pack: bool,
    /// A folder holding `asset.ron` (issue #9).
    pub is_asset: bool,
}

/// The most entries a listing holds: the host lists a larger directory's
/// first ones in [`sort_entries`]' order, one more to say there were more,
/// and the browser shows the first and says so.
pub const MAX_ENTRIES: usize = 10_000;

/// The file extensions an import offers (case aside).
const IMAGE_EXTENSIONS: [&str; 3] = ["png", "jpg", "jpeg"];
/// The file extensions a sound's import offers (case aside, issue #80).
const SOUND_EXTENSIONS: [&str; 2] = ["wav", "ogg"];
/// A level export's (issue #84).
const LEVEL_EXTENSIONS: [&str; 1] = [crate::notice::LEVEL_EXPORT_EXTENSION];

/// The browser's state: its purpose, its directory, the entries of the
/// last listing that succeeded (folders first, then by name, case aside)
/// and what the last listing said when it failed.
#[derive(Clone, Debug, PartialEq)]
pub struct Browser {
    purpose: Purpose,
    dir: PathBuf,
    entries: Vec<DirEntry>,
    message: Option<String>,
    /// The selected image's preview, and the file it was last asked for.
    preview: Option<Preview>,
    asked: Option<String>,
    /// The save purpose's File name to start from (issue #84), and the
    /// existing file whose replacing it asks about.
    file_name: Option<String>,
    replacing: Option<String>,
}

/// The selected image's preview for an import (the host made it small:
/// `EditorCommand::ReadPreview`), or why there is none.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    /// The entry it shows.
    pub name: String,
    pub image: Result<RgbaImage, String>,
    /// The image's own size, when it was read.
    pub size: Option<(u32, u32)>,
}

impl Browser {
    /// A browser at `dir`, empty until its first listing.
    pub(crate) fn new(purpose: Purpose, dir: PathBuf) -> Browser {
        Browser {
            purpose,
            dir,
            entries: Vec::new(),
            message: None,
            preview: None,
            asked: None,
            file_name: None,
            replacing: None,
        }
    }

    /// A browser choosing where to save, its File name starting at `name`
    /// (issue #84).
    pub(crate) fn new_save(purpose: Purpose, dir: PathBuf, name: String) -> Browser {
        Browser { file_name: Some(name), ..Browser::new(purpose, dir) }
    }

    /// The File name a save purpose starts from.
    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    /// The file whose replacing the save asks about ("Replace <name>?").
    pub fn replacing(&self) -> Option<&str> {
        self.replacing.as_deref()
    }

    /// Whether the folder shown holds an entry `name` (case aside: a
    /// Windows folder would take it for the same file).
    pub(crate) fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name.eq_ignore_ascii_case(name))
    }

    pub(crate) fn ask_replace(&mut self, name: String) {
        self.file_name = Some(name.clone());
        self.replacing = Some(name);
    }

    pub(crate) fn cancel_replace(&mut self) {
        self.replacing = None;
    }

    /// Says `message` where a listing's failure shows.
    pub(crate) fn say(&mut self, message: String) {
        self.message = Some(message);
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// The directory shown.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The selected image's preview, once the host answered.
    pub fn preview(&self) -> Option<&Preview> {
        self.preview.as_ref()
    }

    /// Asks for entry `name`'s preview: the path to read, for an import's
    /// file not asked for already.
    pub(crate) fn ask_preview(&mut self, name: &str) -> Option<PathBuf> {
        let e = self.entry(name).filter(|e| self.purpose.images() && !e.is_dir)?;
        if self.asked.as_deref() == Some(e.name.as_str()) {
            return None;
        }
        self.asked = Some(name.to_string());
        Some(self.dir.join(name))
    }

    /// The host's preview of `path`: kept when it is the file asked for last.
    pub(crate) fn previewed(&mut self, path: &Path, image: Result<(RgbaImage, (u32, u32)), String>) {
        let Some(name) = self.asked.clone().filter(|n| self.dir.join(n) == path) else { return };
        let (image, size) = match image {
            Ok((img, size)) => (Ok(img), Some(size)),
            Err(why) => (Err(why), None),
        };
        self.preview = Some(Preview { name, image, size });
    }

    /// Why the last listing failed (the old one stays shown).
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The entries shown: folders, and for an image's purpose the `.png`, `.jpg`
    /// and `.jpeg` files, for a sound's the `.wav` and `.ogg` files (case
    /// aside); hidden (`.`) entries left out.
    pub fn shown(&self) -> Vec<&DirEntry> {
        let files: &[&str] = match self.purpose {
            p if p.images() => &IMAGE_EXTENSIONS,
            Purpose::ImportSound => &SOUND_EXTENSIONS,
            Purpose::ExportLevel | Purpose::ImportLevel => &LEVEL_EXTENSIONS,
            _ => &[],
        };
        self.entries
            .iter()
            .filter(|e| !e.name.starts_with('.'))
            .filter(|e| {
                e.is_dir
                    || Path::new(&e.name)
                        .extension()
                        .and_then(|x| x.to_str())
                        .is_some_and(|x| files.iter().any(|i| x.eq_ignore_ascii_case(i)))
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
                sort_entries(&mut entries);
                self.message = (entries.len() > MAX_ENTRIES)
                    .then(|| format!("only the first {MAX_ENTRIES} entries are shown"));
                entries.truncate(MAX_ENTRIES);
                self.dir = dir;
                self.entries = entries;
                self.preview = None;
                self.asked = None;
                self.replacing = None;
            }
            Err(why) => self.message = Some(why),
        }
    }

    /// `path` from this browser: as typed when absolute, else under its
    /// directory; `.` and `..` resolved by name (never above the root), so
    /// the directory shown and Up stay plain.
    pub(crate) fn resolve(&self, path: &Path) -> PathBuf {
        use std::path::Component;
        let joined = if path.is_absolute() { path.to_path_buf() } else { self.dir.join(path) };
        let mut out = PathBuf::new();
        for c in joined.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    if out.file_name().is_some() {
                        out.pop();
                    }
                }
                c => out.push(c),
            }
        }
        out
    }
}

/// Sorts a listing as the browser shows it ([`order`]), each name
/// lowercased once.
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by_cached_key(sort_key);
}

/// [`order`] as a key: folders first, then the name ignoring case, then
/// exactly.
pub fn sort_key(e: &DirEntry) -> (bool, String, String) {
    (!e.is_dir, e.name.to_lowercase(), e.name.clone())
}

/// Folders first, then by name ignoring case, then by the exact name (a
/// total order: the listing is the same whatever the disk's order).
pub fn order(a: &DirEntry, b: &DirEntry) -> Ordering {
    b.is_dir
        .cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.name.cmp(&b.name))
}
