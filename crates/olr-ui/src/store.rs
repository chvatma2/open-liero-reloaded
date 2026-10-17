//! `settings.ron`, `profiles.ron` and `identity.ron` in the platform data directory (spec
//! §9.4, §21 G2).
//!
//! - **Format:** RON, `(version: N, settings: (...))` (or `profiles:`).
//!   The version is read first; any other than this game's is refused.
//! - **Writes are atomic:** a temporary file in the same directory, synced,
//!   then renamed over the old one, so a crash leaves the old file or the
//!   new one, never half of either.
//! - **A bad file never stops the game** (plan 7 Review Focus 1): one that
//!   is not UTF-8, does not parse, has another version or holds values out
//!   of range gives defaults (or the values fixed) and a warning. A file
//!   that cannot be decoded is moved aside to `<name>.bak` (then
//!   `<name>.1.bak`, ... : earlier backups are kept, up to
//!   [`MAX_BACKUPS`], after which the oldest goes). One that decodes but
//!   needs values fixed is copied there and the fixed values are written
//!   in its place, so its other values survive the next start. A file
//!   that cannot be backed up is kept as it is and never overwritten by
//!   this store.
//! - **A newer game's file** (a version above this game's) is left where
//!   it is, untouched: the session uses defaults and saves nothing to it
//!   (unless it is removed meanwhile), and [`Store::notices`] says so for
//!   the main menu, so going back to the newer game finds it intact.
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
/// The install's identity key (plan 10, spec §5.2).
pub const IDENTITY_FILE: &str = "identity.ron";
/// The identity file's format version.
pub const IDENTITY_VERSION: u32 = 1;
/// Auto-saved replays of the last matches (§21 G2).
pub const REPLAYS_DIR: &str = "replays";
/// The user's packs (spec §9.4: written by `olr-cli import`), layered over
/// the bundled ones.
pub const PACKS_DIR: &str = "packs";
/// Downloaded content in the cache directory (plan 11, ruling N30).
pub const DOWNLOADS_DIR: &str = "downloads";
/// A data directory's own cache directory ([`Store::open`]).
pub const CACHE_DIR: &str = "cache";
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

/// A secret key's bytes; `Debug` never prints them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Secret(pub [u8; 32]);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// The user's files.
#[derive(Clone, Debug, Default)]
pub struct Store {
    /// `None`: nothing is read or written.
    dir: Option<PathBuf>,
    /// The cache directory (the platform's, or `<dir>/cache`); `None` in
    /// memory.
    cache: Option<PathBuf>,
    /// Bad files that could not be moved aside: never overwritten.
    kept: Vec<PathBuf>,
    /// Files of a newer game: left untouched, nothing saved to them.
    newer: Vec<PathBuf>,
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

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    version: u32,
    #[serde(with = "hex_secret")]
    secret: [u8; 32],
}

/// A secret as 64 hex digits; all zeros is no key.
mod hex_secret {
    use serde::de::Error as _;
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(secret: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        let text: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        s.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let text = String::deserialize(d)?;
        let digit = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
        let mut out = [0u8; 32];
        if text.len() != 64 {
            return Err(D::Error::custom("the secret is not 64 hex digits"));
        }
        for (o, pair) in out.iter_mut().zip(text.as_bytes().chunks(2)) {
            match (digit(pair[0]), digit(pair[1])) {
                (Some(h), Some(l)) => *o = h << 4 | l,
                _ => return Err(D::Error::custom("the secret is not 64 hex digits")),
            }
        }
        if out == [0; 32] {
            return Err(D::Error::custom("the secret is zero"));
        }
        Ok(out)
    }
}

impl Store {
    /// The files in `dir`, which is made when the first file is saved.
    /// Its cache directory is `<dir>/cache` (a data directory of its own:
    /// `--data-dir`, the tests).
    pub fn open(dir: impl Into<PathBuf>) -> Store {
        let dir = dir.into();
        Store { cache: Some(dir.join(CACHE_DIR)), dir: Some(dir), kept: Vec::new(), newer: Vec::new() }
    }

    /// The platform data directory (`directories`' `ProjectDirs` for
    /// "open-liero-reloaded": `~/.local/share/open-liero-reloaded` on Linux; its cache
    /// directory `~/.cache/open-liero-reloaded`), or, when the
    /// platform has none, [`Store::in_memory`] and a warning.
    pub fn platform() -> (Store, Option<String>) {
        match directories::ProjectDirs::from("", "", "open-liero-reloaded") {
            Some(d) => {
                (Store { cache: Some(d.cache_dir().to_path_buf()), ..Store::open(d.data_dir()) }, None)
            }
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

    /// Where downloaded content is kept (plan 11, ruling N30): the cache
    /// directory's `downloads/` (`ProjectDirs::cache_dir()/downloads`);
    /// `None` in memory.
    pub fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.as_ref().map(|d| d.join(DOWNLOADS_DIR))
    }

    /// The user packs layer (spec §9.4), if there is a data directory.
    pub fn packs_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(PACKS_DIR))
    }

    /// What the player should know of the files, one line each, for the
    /// main menu: a newer game's file left untouched.
    pub fn notices(&self) -> Vec<String> {
        self.newer
            .iter()
            .map(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                format!(
                    "{name} is from a newer version; using defaults and leaving it untouched \
                     (changes are not saved)"
                )
            })
            .collect()
    }

    /// `settings.ron`: defaults when there is none (no warning) or it is bad.
    pub fn load_settings(&mut self) -> Loaded<Settings> {
        self.load(
            SETTINGS_FILE,
            SETTINGS_VERSION,
            |t| ron::from_str::<SettingsFile>(t).map(|f| f.settings),
            |s| s.sanitize(),
            |store, s| store.save_settings(s),
        )
    }

    /// `profiles.ron`: the default profile when there is none or it is bad.
    pub fn load_profiles(&mut self) -> Loaded<Profiles> {
        self.load(
            PROFILES_FILE,
            PROFILES_VERSION,
            |t| ron::from_str::<ProfilesFile>(t).map(|f| f.profiles),
            |p| p.sanitize(),
            |store, p| store.save_profiles(p),
        )
    }

    /// `identity.ron`: the install's secret key (spec §5.2). With no file,
    /// `fresh` (the caller's randomness) makes one, which is saved; a
    /// damaged file is moved to `.bak` and replaced the same way, with a
    /// warning (the player's fingerprint changes). Whether the scalar is
    /// valid is `olr-lobby`'s `Identity::from_secret`'s to say; all zeros
    /// is refused here. Without a data directory the secret is not kept,
    /// and neither is one when the file is a newer game's (left
    /// untouched): each launch then gets a fresh identity. The file and
    /// its backups are private (unix mode 0600).
    pub fn load_identity(&mut self, fresh: impl FnOnce() -> [u8; 32]) -> Loaded<Secret> {
        let loaded = self.load(
            IDENTITY_FILE,
            IDENTITY_VERSION,
            |t| ron::from_str::<IdentityFile>(t).map(|f| Some(f.secret)),
            |_| Vec::new(),
            |_, _| Ok(()),
        );
        let mut warnings = loaded.warnings;
        let value = match loaded.value {
            Some(secret) => secret,
            None => {
                let secret = fresh();
                // A newer game's file stays untouched (the save skips it).
                if let Err(e) = self.save(IDENTITY_FILE, &IdentityFile { version: IDENTITY_VERSION, secret })
                {
                    warnings.push(format!("cannot save the identity: {e:#}"));
                }
                secret
            }
        };
        Loaded { value: Secret(value), warnings }
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
        save: impl FnOnce(&mut Self, &T) -> anyhow::Result<()>,
    ) -> Loaded<T> {
        let Some(path) = self.dir.as_ref().map(|d| d.join(name)) else {
            return Loaded { value: T::default(), warnings: Vec::new() };
        };
        self.newer.retain(|p| *p != path);
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
        let (value, problems, saved) = match decode(&bytes, version, parse) {
            Ok(mut v) => {
                let fixes = sanitize(&mut v);
                if fixes.is_empty() {
                    return Loaded { value: v, warnings: fixes };
                }
                // Readable, with values fixed: the original is copied
                // aside and the fixed values written in its place, so the
                // rest of the file survives the next start.
                let saved = self.rewrite(&path, &v, save);
                (v, fixes, saved)
            }
            Err(Bad::Newer(why)) => {
                // Left for the newer game: never moved, never overwritten.
                self.newer.push(path.clone());
                let w = format!(
                    "{}: {why}; using defaults (the file is left untouched and this session saves \
                     nothing to it)",
                    path.display()
                );
                return Loaded { value: T::default(), warnings: vec![w] };
            }
            Err(Bad::Other(why)) => {
                (T::default(), vec![format!("{why}; using defaults")], self.move_aside(&path))
            }
        };
        let warnings = problems.into_iter().map(|p| format!("{}: {p} ({saved})", path.display())).collect();
        Loaded { value, warnings }
    }

    /// Moves a bad file to a backup; says where it went.
    fn move_aside(&mut self, path: &Path) -> String {
        match backup_slot(path).and_then(|bak| fs::rename(path, &bak).map(|()| bak)) {
            Ok(bak) => {
                harden_backup(path, &bak);
                format!("the file was moved to {}", bak.display())
            }
            Err(e) => {
                self.keep(path);
                format!("the file is kept where it is and will not be overwritten: {e}")
            }
        }
    }

    /// Copies a file to a backup, then saves `value` over it; says what
    /// happened. Without the copy, the file is kept and not overwritten.
    fn rewrite<T>(
        &mut self,
        path: &Path,
        value: &T,
        save: impl FnOnce(&mut Self, &T) -> anyhow::Result<()>,
    ) -> String {
        let bak = match backup_slot(path).and_then(|bak| fs::copy(path, &bak).map(|_| bak)) {
            Ok(bak) => bak,
            Err(e) => {
                self.keep(path);
                return format!("the file is kept as it is and will not be overwritten: {e}");
            }
        };
        match save(self, value) {
            Ok(()) => format!("the original was copied to {} and the fixed values saved", bak.display()),
            Err(e) => {
                format!("the original was copied to {}; saving the fixed values failed: {e:#}", bak.display())
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
        if self.newer.contains(&path) {
            if path.exists() {
                return Ok(());
            }
            // Gone: nothing of the newer game's to keep.
            self.newer.retain(|p| *p != path);
        }
        if self.kept.contains(&path) {
            if path.exists() {
                bail!("{} could not be read or backed up: move it away to save", path.display());
            }
            self.kept.retain(|p| *p != path);
        }
        let text = ron::ser::to_string_pretty(file, ron::ser::PrettyConfig::default())
            .with_context(|| format!("encoding {name}"))?;
        let private = name == IDENTITY_FILE;
        write_atomic(&path, text.as_bytes(), private).with_context(|| format!("saving {}", path.display()))
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

/// The backup to write next: the first free of [`backup_path`]'s
/// [`MAX_BACKUPS`] slots; when all are taken, the oldest (`.bak`) is
/// removed and every other moves down one, freeing the last.
fn backup_slot(path: &Path) -> io::Result<PathBuf> {
    if let Some(free) = (0..MAX_BACKUPS).map(|n| backup_path(path, n)).find(|p| !p.exists()) {
        return Ok(free);
    }
    fs::remove_file(backup_path(path, 0))?;
    for n in 1..MAX_BACKUPS {
        fs::rename(backup_path(path, n), backup_path(path, n - 1))?;
    }
    Ok(backup_path(path, MAX_BACKUPS - 1))
}

/// Why a file is not read.
enum Bad {
    /// A newer game's: left as it is.
    Newer(String),
    Other(String),
}

/// The file's value, or why it is bad.
fn decode<T>(
    bytes: &[u8],
    version: u32,
    parse: impl FnOnce(&str) -> Result<T, ron::error::SpannedError>,
) -> Result<T, Bad> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(Bad::Other(format!("larger than {MAX_FILE_BYTES} bytes")));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| Bad::Other(format!("not UTF-8 text ({e})")))?;
    let header: Header = ron::from_str(text).map_err(|e| Bad::Other(format!("unreadable ({e})")))?;
    if header.version > version {
        return Err(Bad::Newer(format!(
            "version {} is a newer game's; this game reads version {version}",
            header.version
        )));
    }
    if header.version != version {
        return Err(Bad::Other(format!(
            "version {} is an older game's; this game reads version {version}",
            header.version
        )));
    }
    parse(text).map_err(|e| Bad::Other(format!("unreadable ({e})")))
}

/// A new file; `private` makes it readable by its owner only (unix).
fn create_file(path: &Path, private: bool) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt as _;
        // A stale temporary file is removed first so the mode applies.
        let _ = fs::remove_file(path);
        return options.create_new(true).mode(0o600).open(path);
    }
    let _ = private;
    options.create(true).truncate(true).open(path)
}

/// Makes the identity file's backup owner-only (unix); other files are
/// left as they are.
fn harden_backup(original: &Path, bak: &Path) {
    #[cfg(unix)]
    if original.file_name().is_some_and(|n| n == IDENTITY_FILE) {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(bak, fs::Permissions::from_mode(0o600));
    }
    let _ = (original, bak);
}

/// Writes `bytes` to a temporary file next to `path`, syncs it and renames
/// it over `path` (making the directory first).
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::other("no directory"))?;
    fs::create_dir_all(dir)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = dir.join(tmp_name);
    let written = (|| {
        let mut f = create_file(&tmp, private)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}
