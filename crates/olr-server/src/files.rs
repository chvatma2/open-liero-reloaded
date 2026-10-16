//! The server's files in its data directory: its identity
//! (`server-identity.ron`, owner-only like the game's `identity.ron`), the
//! bans file and the rotating log (`server.log`).

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write as _};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use olr_lobby::Identity;
use olr_proto::PublicKey;
use serde::{Deserialize, Serialize};

/// The server's secret key, in the data directory.
pub const IDENTITY_FILE: &str = "server-identity.ron";
/// The identity file's format (the game's `identity.ron` format).
pub const IDENTITY_VERSION: u32 = 1;
/// A bans file larger than this is refused (it holds keys and addresses).
pub const MAX_BANS_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityFile {
    version: u32,
    /// 64 hex characters.
    secret: String,
}

/// The bans file: keys (64 hex characters), addresses, and who each key
/// was (by its hex), for `/unban <name>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct BansFile {
    keys: Vec<String>,
    ips: Vec<IpAddr>,
    who: BTreeMap<String, Who>,
}

/// Who a banned key was: its name, and the address banned with it
/// (`/banip`; lifted with the key).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Who {
    pub name: String,
    #[serde(default)]
    pub ip: Option<IpAddr>,
}

/// The bans kept across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BanList {
    pub keys: Vec<PublicKey>,
    pub ips: Vec<IpAddr>,
    /// Only for banned keys.
    pub who: BTreeMap<PublicKey, Who>,
}

/// 32 bytes as 64 lowercase hex characters.
pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 64 hex characters (either case) as 32 bytes.
pub fn parse_key(text: &str) -> anyhow::Result<PublicKey> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        bail!("not 64 hex characters");
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).context("not hex")?;
    }
    Ok(out)
}

/// The server's identity from `<dir>/server-identity.ron`, made (from the
/// operating system's randomness) and saved owner-only when there is no
/// file; `true` when it was made. A damaged file is an error: the server
/// does not start with another key behind its operator's back.
pub fn load_identity(dir: &Path) -> anyhow::Result<(Identity, bool)> {
    let path = dir.join(IDENTITY_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let file: IdentityFile = ron::from_str(&text).with_context(|| format!("{}", path.display()))?;
            if file.version != IDENTITY_VERSION {
                bail!("{}: version {} (this server reads {IDENTITY_VERSION})", path.display(), file.version);
            }
            let secret =
                parse_key(&file.secret).with_context(|| format!("{}: the secret", path.display()))?;
            let id = Identity::from_secret(secret)
                .with_context(|| format!("{}: the secret is no valid key", path.display()))?;
            Ok((id, false))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let id = fresh_identity()?;
            let file = IdentityFile { version: IDENTITY_VERSION, secret: hex(&id.secret()) };
            write_atomic(&path, to_ron(&file)?.as_bytes(), true)
                .with_context(|| format!("saving {}", path.display()))?;
            Ok((id, true))
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn fresh_identity() -> anyhow::Result<Identity> {
    // A random scalar is valid but for a chance of about 2^-128 a try.
    for _ in 0..16 {
        if let Some(id) = Identity::from_secret(olr_net::random_bytes::<32>()?) {
            return Ok(id);
        }
    }
    bail!("no valid key from the system's randomness")
}

/// The bans in `path` (none when there is no file); its keys checked.
pub fn load_bans(path: &Path) -> anyhow::Result<BanList> {
    let text = match fs::metadata(path) {
        Ok(m) if m.len() > MAX_BANS_BYTES => bail!("{}: larger than {MAX_BANS_BYTES} bytes", path.display()),
        Ok(_) => fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BanList::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let bans: BansFile = ron::from_str(&text).with_context(|| format!("{}", path.display()))?;
    let key = |k: &String| parse_key(k).with_context(|| format!("{}: key {k:?}", path.display()));
    let keys = bans.keys.iter().map(key).collect::<anyhow::Result<Vec<_>>>()?;
    let mut who = BTreeMap::new();
    for (k, w) in bans.who {
        let k = key(&k)?;
        if keys.contains(&k) {
            who.insert(k, w);
        }
    }
    Ok(BanList { keys, ips: bans.ips, who })
}

/// Writes the bans to `path` atomically.
pub fn save_bans(path: &Path, bans: &BanList) -> anyhow::Result<()> {
    let bans = BansFile {
        keys: bans.keys.iter().map(hex).collect(),
        ips: bans.ips.clone(),
        who: bans
            .who
            .iter()
            .filter(|(k, _)| bans.keys.contains(k))
            .map(|(k, w)| (hex(k), w.clone()))
            .collect(),
    };
    write_atomic(path, to_ron(&bans)?.as_bytes(), false).with_context(|| format!("saving {}", path.display()))
}

fn to_ron<T: Serialize>(value: &T) -> anyhow::Result<String> {
    Ok(ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default())? + "\n")
}

/// Writes `bytes` to a temporary file next to `path`, syncs it and renames
/// it over `path` (making the directory first); `private` makes it
/// readable by its owner only (unix mode 0600).
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

/// The log file rotates past this size.
pub const LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Old log files kept: `server.log.1` (the newer), `server.log.2`.
pub const LOG_KEEP: usize = 2;

/// An appending log file that rotates past `max` bytes (at a line start),
/// keeping `keep` old files (`<path>.1` the newest). Writing never fails:
/// a full disk must not stop the server (the terminal still has the log).
pub struct RotatingLog {
    path: PathBuf,
    file: Option<fs::File>,
    size: u64,
    max: u64,
    keep: usize,
    line_start: bool,
    /// A rotation failed: the log goes on in the current file for the rest
    /// of the run, and no rotation is tried again.
    stuck: bool,
    /// `fs::rename` (a test injects failures).
    rename: fn(&Path, &Path) -> io::Result<()>,
}

impl RotatingLog {
    pub fn open(path: &Path, max: u64, keep: usize) -> io::Result<RotatingLog> {
        let file = fs::OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata()?.len();
        Ok(RotatingLog {
            path: path.to_path_buf(),
            file: Some(file),
            size,
            max,
            keep,
            line_start: true,
            stuck: false,
            rename: |from, to| fs::rename(from, to),
        })
    }

    /// `<path>.<n>` (`n` from 1).
    fn old(&self, n: usize) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    /// `<path>` becomes `<path>.1`, `.1` becomes `.2`, … the oldest beyond
    /// `keep` is removed; a fresh `<path>` is opened. On a failure (Windows:
    /// a viewer holding the log open) the old files are kept, the log goes
    /// on in `<path>`, and that is said once on stderr.
    fn rotate(&mut self) {
        self.file = None;
        if let Err(e) = self.shift() {
            self.stuck = true;
            eprintln!("warning: cannot rotate {}: {e}; it grows from now on", self.path.display());
        }
        self.file = fs::OpenOptions::new().create(true).append(true).open(&self.path).ok();
        self.size = self.file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
    }

    /// Moves the files along: `<path>` first, out of the way to `<path>.0`
    /// (what fails while it is held open), so nothing old is touched until
    /// that worked; on a later failure `<path>.0` goes back in place.
    fn shift(&self) -> io::Result<()> {
        if self.keep == 0 {
            return fs::remove_file(&self.path);
        }
        let fresh = self.old(0);
        // A stale one (a crash mid-rotation) would block the rename on Windows.
        let _ = fs::remove_file(&fresh);
        (self.rename)(&self.path, &fresh)?;
        let shifted = (|| {
            match fs::remove_file(self.old(self.keep)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            for n in (1..self.keep).rev() {
                if self.old(n).exists() {
                    (self.rename)(&self.old(n), &self.old(n + 1))?;
                }
            }
            (self.rename)(&fresh, &self.old(1))
        })();
        if shifted.is_err() {
            let _ = (self.rename)(&fresh, &self.path);
        }
        shifted
    }
}

impl io::Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.line_start && !self.stuck && self.size > 0 && self.size + buf.len() as u64 > self.max {
            self.rotate();
        }
        if let Some(f) = &mut self.file
            && f.write_all(buf).is_ok()
        {
            self.size += buf.len() as u64;
        }
        if let Some(&last) = buf.last() {
            self.line_start = last == b'\n';
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = &mut self.file {
            let _ = f.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_through_hex() {
        let key: [u8; 32] = core::array::from_fn(|i| (i * 37 + 5) as u8);
        assert_eq!(parse_key(&hex(&key)).unwrap(), key);
        assert_eq!(parse_key(&hex(&key).to_uppercase()).unwrap(), key);
        for bad in ["", "ab", &"g".repeat(64), &"é".repeat(32), &"a".repeat(65)] {
            assert!(parse_key(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn bans_round_trip_and_a_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bans.ron");
        assert_eq!(load_bans(&path).unwrap(), BanList::default());
        let ips: Vec<IpAddr> = vec!["10.0.0.1".parse().unwrap(), "::1".parse().unwrap()];
        let who = Who { name: "Bob".into(), ip: Some(ips[0]) };
        let mut bans = BanList { keys: vec![[7; 32]], ips, who: BTreeMap::from([([7; 32], who)]) };
        save_bans(&path, &bans).unwrap();
        assert_eq!(load_bans(&path).unwrap(), bans);
        // Who an unbanned key was is not kept.
        bans.who.insert([8; 32], Who { name: "Old".into(), ip: None });
        save_bans(&path, &bans).unwrap();
        assert_eq!(load_bans(&path).unwrap().who.len(), 1);
        // A hand-written file: keys alone.
        fs::write(&path, format!("(keys: [{:?}])", hex(&[9; 32]))).unwrap();
        assert_eq!(load_bans(&path).unwrap().keys, vec![[9; 32]]);
        fs::write(&path, "(keys: [\"zz\"])").unwrap();
        assert!(load_bans(&path).is_err(), "a bad key is an error, not an empty list");
    }

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (a, made) = load_identity(dir.path()).unwrap();
        assert!(made);
        let (b, made) = load_identity(dir.path()).unwrap();
        assert!(!made);
        assert_eq!(a.public(), b.public());
        fs::write(dir.path().join(IDENTITY_FILE), "(version: 1, secret: \"00\")").unwrap();
        assert!(load_identity(dir.path()).is_err(), "a damaged identity stops the server");
    }

    #[test]
    fn the_log_rotates_keeping_two_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.log");
        let read = |name: &str| fs::read_to_string(dir.path().join(name)).unwrap_or_default();
        let mut log = RotatingLog::open(&path, 100, 2).unwrap();
        let lines: Vec<String> = (0..12).map(|i| format!("line {i:02} {}\n", "x".repeat(23))).collect();
        for l in &lines {
            // A line in two writes is never split between files.
            let (a, b) = l.split_at(5);
            log.write_all(a.as_bytes()).unwrap();
            log.write_all(b.as_bytes()).unwrap();
        }
        drop(log);
        // 32-byte lines, 100 bytes a file: 3 lines each, the oldest gone.
        assert_eq!(read("server.log"), lines[9..].concat());
        assert_eq!(read("server.log.1"), lines[6..9].concat());
        assert_eq!(read("server.log.2"), lines[3..6].concat());
        assert!(!dir.path().join("server.log.3").exists());
        // Reopened, it counts what the file holds.
        let mut log = RotatingLog::open(&path, 100, 2).unwrap();
        log.write_all(lines[0].as_bytes()).unwrap();
        assert_eq!(read("server.log"), lines[0].clone());
        assert_eq!(read("server.log.1"), lines[9..].concat());
    }

    thread_local! {
        static RENAMES_OF_THE_LOG: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// A rename that fails for `server.log` itself (Windows, with a viewer
    /// holding it open), counting those attempts.
    fn held_open(from: &Path, to: &Path) -> io::Result<()> {
        if from.file_name().is_some_and(|n| n == "server.log") {
            RENAMES_OF_THE_LOG.with(|c| c.set(c.get() + 1));
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "held open"));
        }
        fs::rename(from, to)
    }

    /// When the log cannot be renamed, the old files stay as they are, the
    /// log goes on in the current file and no rotation is tried again.
    #[test]
    fn a_log_that_cannot_rotate_keeps_its_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.log");
        let read = |name: &str| fs::read_to_string(dir.path().join(name)).unwrap_or_default();
        let mut log = RotatingLog::open(&path, 100, 2).unwrap();
        let lines: Vec<String> = (0..15).map(|i| format!("line {i:02} {}\n", "x".repeat(23))).collect();
        for l in &lines[..9] {
            log.write_all(l.as_bytes()).unwrap();
        }
        log.rename = held_open;
        for l in &lines[9..] {
            log.write_all(l.as_bytes()).unwrap();
        }
        drop(log);
        assert_eq!(read("server.log.2"), lines[..3].concat(), "the oldest file kept");
        assert_eq!(read("server.log.1"), lines[3..6].concat(), "the newer old file kept");
        assert_eq!(read("server.log"), lines[6..].concat(), "the log goes on in the current file");
        assert_eq!(RENAMES_OF_THE_LOG.with(|c| c.get()), 1, "tried once");
    }
}
