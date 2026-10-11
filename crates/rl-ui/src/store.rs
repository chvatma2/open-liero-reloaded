//! `settings.ron` and `profiles.ron` in the platform data directory (spec
//! §9.4, §21 G2).
//!
//! - **Format:** RON, `(version: N, settings: (...))` (or `profiles:`).
//!   The version is read first; any other than this game's is refused.
//! - **Writes are atomic:** a temporary file in the same directory, synced,
//!   then renamed over the old one, so a crash leaves the old file or the
//!   new one, never half of either.
//! - **A bad file never stops the game** (plan 7 Review Focus 1): one that
//!   is not UTF-8, does not parse, has another version or holds values out
//!   of range gives defaults (or the values fixed) and a warning, and is
//!   moved aside to `<name>.bak` (then `<name>.1.bak`, ... : earlier
//!   backups are kept). A file that cannot be moved aside is kept where it
//!   is and never overwritten by this store.
//! - **No data directory** (no home): [`Store::in_memory`] loads defaults
//!   and saves nothing.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};

use crate::profiles::{PROFILES_VERSION, Profiles};
use crate::settings::{SETTINGS_VERSION, Settings};

/// The settings file's name in the data directory.
pub const SETTINGS_FILE: &str = "settings.ron";
/// The profiles file's name in the data directory.
pub const PROFILES_FILE: &str = "profiles.ron";
/// Auto-saved replays of the last matches (§21 G2).
pub const REPLAYS_DIR: &str = "replays";
/// A settings or profiles file larger than this is bad (they are a few KB).
pub const MAX_FILE_BYTES: usize = 1 << 20;
/// Backups `<name>.bak`, `<name>.1.bak` .. `<name>.<MAX_BACKUPS - 1>.bak`.
pub const MAX_BACKUPS: usize = 100;

/// What a load gave: the value to use, and what was wrong (each to be
/// logged once).
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded<T> {
    pub value: T,
    pub warnings: Vec<String>,
}

/// The user's files.
#[derive(Clone, Debug, Default)]
pub struct Store {
    /// `None`: nothing is read or written.
    dir: Option<PathBuf>,
    /// Bad files that could not be moved aside: never overwritten.
    kept: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct Header {
    version: u32,
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    settings: Settings,
}

#[derive(Serialize, Deserialize)]
struct ProfilesFile {
    version: u32,
    profiles: Profiles,
}

impl Store {
    /// The files in `dir`, which is made when the first file is saved.
    pub fn open(dir: impl Into<PathBuf>) -> Store {
        Store { dir: Some(dir.into()), kept: Vec::new() }
    }

    /// The platform data directory (`directories`' `ProjectDirs` for
    /// "rust-liero": `~/.local/share/rust-liero` on Linux), or, when the
    /// platform has none, [`Store::in_memory`] and a warning.
    pub fn platform() -> (Store, Option<String>) {
        match directories::ProjectDirs::from("", "", "rust-liero") {
            Some(d) => (Store::open(d.data_dir()), None),
            None => {
                (Store::in_memory(), Some("no data directory: settings and profiles are not saved".into()))
            }
        }
    }

    /// No directory: loads give defaults, saves do nothing.
    pub fn in_memory() -> Store {
        Store::default()
    }

    /// The data directory, if there is one.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Where replays are saved (§21 G2), if there is a data directory.
    pub fn replays_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(REPLAYS_DIR))
    }

    /// `settings.ron`: defaults when there is none (no warning) or it is bad.
    pub fn load_settings(&mut self) -> Loaded<Settings> {
        self.load(
            SETTINGS_FILE,
            SETTINGS_VERSION,
            |t| ron::from_str::<SettingsFile>(t).map(|f| f.settings),
            |s| s.sanitize(),
        )
    }

    /// `profiles.ron`: the default profile when there is none or it is bad.
    pub fn load_profiles(&mut self) -> Loaded<Profiles> {
        self.load(
            PROFILES_FILE,
            PROFILES_VERSION,
            |t| ron::from_str::<ProfilesFile>(t).map(|f| f.profiles),
            |p| p.sanitize(),
        )
    }

    /// Writes `settings.ron` atomically.
    pub fn save_settings(&mut self, settings: &Settings) -> anyhow::Result<()> {
        let file = SettingsFile { version: SETTINGS_VERSION, settings: settings.clone() };
        self.save(SETTINGS_FILE, &file)
    }

    /// Writes `profiles.ron` atomically.
    pub fn save_profiles(&mut self, profiles: &Profiles) -> anyhow::Result<()> {
        let file = ProfilesFile { version: PROFILES_VERSION, profiles: profiles.clone() };
        self.save(PROFILES_FILE, &file)
    }

    fn load<T: Default>(
        &mut self,
        name: &str,
        version: u32,
        parse: impl FnOnce(&str) -> Result<T, ron::error::SpannedError>,
        sanitize: impl FnOnce(&mut T) -> Vec<String>,
    ) -> Loaded<T> {
        let Some(path) = self.dir.as_ref().map(|d| d.join(name)) else {
            return Loaded { value: T::default(), warnings: Vec::new() };
        };
        let bytes = match read_capped(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Loaded { value: T::default(), warnings: Vec::new() };
            }
            Err(e) => {
                // Unreadable: it stays, and is never overwritten.
                self.keep(&path);
                let w = format!("cannot read {}: {e}; using defaults (the file is kept)", path.display());
                return Loaded { value: T::default(), warnings: vec![w] };
            }
        };
        let (value, problems) = match decode(&bytes, version, parse) {
            Ok(mut v) => {
                let fixes = sanitize(&mut v);
                (v, fixes)
            }
            Err(why) => (T::default(), vec![format!("{why}; using defaults")]),
        };
        if problems.is_empty() {
            return Loaded { value, warnings: problems };
        }
        let saved = self.back_up(&path);
        let warnings = problems.into_iter().map(|p| format!("{}: {p} ({saved})", path.display())).collect();
        Loaded { value, warnings }
    }

    /// Moves a bad file aside; says where it went.
    fn back_up(&mut self, path: &Path) -> String {
        let free = (0..MAX_BACKUPS).map(|n| backup_path(path, n)).find(|p| !p.exists());
        let moved = match free {
            Some(bak) => fs::rename(path, &bak).map(|()| bak).map_err(|e| e.to_string()),
            None => Err(format!("{MAX_BACKUPS} backups exist")),
        };
        match moved {
            Ok(bak) => format!("the file was moved to {}", bak.display()),
            Err(e) => {
                self.keep(path);
                format!("the file is kept where it is and will not be overwritten: {e}")
            }
        }
    }

    fn keep(&mut self, path: &Path) {
        if !self.kept.iter().any(|p| p == path) {
            self.kept.push(path.to_path_buf());
        }
    }

    fn save<T: Serialize>(&mut self, name: &str, file: &T) -> anyhow::Result<()> {
        let Some(dir) = &self.dir else { return Ok(()) };
        let path = dir.join(name);
        if self.kept.contains(&path) {
            if path.exists() {
                bail!("{} could not be read or backed up: move it away to save", path.display());
            }
            self.kept.retain(|p| *p != path);
        }
        let text = ron::ser::to_string_pretty(file, ron::ser::PrettyConfig::default())
            .with_context(|| format!("encoding {name}"))?;
        write_atomic(&path, text.as_bytes()).with_context(|| format!("saving {}", path.display()))
    }
}

/// The file's bytes, at most [`MAX_FILE_BYTES`] + 1 of them (more is bad:
/// the decoder refuses it), so a huge file costs no huge allocation.
fn read_capped(path: &Path) -> io::Result<Vec<u8>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// `<name>.bak` for `n` 0, else `<name>.<n>.bak`.
fn backup_path(path: &Path, n: usize) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bak = if n == 0 { format!("{name}.bak") } else { format!("{name}.{n}.bak") };
    path.with_file_name(bak)
}

/// The file's value, or why it is bad.
fn decode<T>(
    bytes: &[u8],
    version: u32,
    parse: impl FnOnce(&str) -> Result<T, ron::error::SpannedError>,
) -> Result<T, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!("larger than {MAX_FILE_BYTES} bytes"));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| format!("not UTF-8 text ({e})"))?;
    let header: Header = ron::from_str(text).map_err(|e| format!("unreadable ({e})"))?;
    if header.version != version {
        let whose = if header.version > version { "a newer game's" } else { "an older game's" };
        return Err(format!("version {} is {whose}; this game reads version {version}", header.version));
    }
    parse(text).map_err(|e| format!("unreadable ({e})"))
}

/// Writes `bytes` to a temporary file next to `path`, syncs it and renames
/// it over `path` (making the directory first).
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no directory"))?;
    fs::create_dir_all(dir)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = dir.join(tmp_name);
    let written = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}
