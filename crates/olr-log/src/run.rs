//! One run's log file: made in the logs directory with a lock its
//! instance holds, the old runs pruned, the previous run's end checked,
//! and lines written buffered under the size cap.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::Level;

use crate::format::{self, Clock};
use crate::redact;

/// Runs kept, this one included.
pub const KEEP: usize = 10;
/// A run's file stops taking lines past this (errors aside).
pub const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// What errors and the crash record may add past the cap.
pub const ERROR_ALLOWANCE: u64 = 256 * 1024;
/// The last line of a run that ended by itself.
pub const CLEAN_END: &str = "the run ended cleanly";
/// The line the next run adds to a run that ended without [`CLEAN_END`],
/// so it is reported once.
pub const UNCLEAN_NOTED: &str = "this run ended unexpectedly (noticed by the next run)";
/// The lines are written to the disk at least this often (and at once on
/// a warning, an error, a panic and the end).
pub const FLUSH_EVERY: Duration = Duration::from_secs(1);

/// The lock file of `log` (`<stem>.lock`), held while its run lives.
pub fn lock_path(log: &Path) -> PathBuf {
    log.with_extension("lock")
}

/// Whether the instance that writes `log` still runs: its lock file is
/// held. No lock file is a run that ended; a lock that cannot be checked
/// counts as live (never deleted, never reported).
pub fn is_live(log: &Path) -> bool {
    let file = match File::open(lock_path(log)) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    match file.try_lock() {
        Ok(()) => false,
        Err(fs::TryLockError::WouldBlock) => true,
        Err(fs::TryLockError::Error(_)) => true,
    }
}

/// The run logs in `dir`, oldest first (their names sort by start).
pub fn runs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|e| e == "log")
                && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.as_bytes()[0].is_ascii_digit())
        })
        .collect();
    out.sort();
    out
}

/// Deletes all but the newest `keep` runs of `dir` (and lock files left
/// without their log), skipping live ones; returns what it deleted.
pub fn prune(dir: &Path, keep: usize) -> Vec<PathBuf> {
    let all = runs(dir);
    let mut gone = Vec::new();
    for log in &all[..all.len().saturating_sub(keep)] {
        if !is_live(log) && fs::remove_file(log).is_ok() {
            let _ = fs::remove_file(lock_path(log));
            gone.push(log.clone());
        }
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for lock in entries.filter_map(|e| e.ok().map(|e| e.path())) {
            let log = lock.with_extension("log");
            if lock.extension().is_some_and(|e| e == "lock") && !log.exists() && !is_live(&log) {
                let _ = fs::remove_file(&lock);
            }
        }
    }
    gone
}

/// The last line of `path` that has text (from its last 4 KiB).
fn last_line(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(4096)))?;
    let mut tail = Vec::new();
    f.read_to_end(&mut tail)?;
    let text = String::from_utf8_lossy(&tail);
    Ok(text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string())
}

/// The previous run, if it ended without its clean last line and was not
/// reported yet: the newest log of `dir` but `ours` whose instance is not
/// running. It is marked reported with `noted`, a line ending in
/// [`UNCLEAN_NOTED`].
pub fn previous_unclean(dir: &Path, ours: &Path, noted: &str) -> Option<PathBuf> {
    let previous = runs(dir).into_iter().rev().filter(|l| l != ours).find(|l| !is_live(l))?;
    let ended = last_line(&previous)
        .is_ok_and(|l| l.trim_end().ends_with(CLEAN_END) || l.trim_end().ends_with(UNCLEAN_NOTED));
    if ended {
        return None;
    }
    if let Ok(mut f) = fs::OpenOptions::new().append(true).open(&previous) {
        let _ = f.write_all(noted.as_bytes());
    }
    Some(previous)
}

/// What opening a run's log found.
#[derive(Debug, Default)]
pub struct Opened {
    /// The previous run's log, when it ended unexpectedly.
    pub previous_unclean: Option<PathBuf>,
    /// The old runs deleted.
    pub pruned: Vec<PathBuf>,
}

/// One run's log file.
pub struct RunLog {
    path: PathBuf,
    file: BufWriter<File>,
    /// Held until the process ends: other instances see this run live.
    _lock: File,
    clock: Box<dyn Clock>,
    home: Option<String>,
    written: u64,
    cap: u64,
    truncated: bool,
    last_flush: Instant,
}

impl RunLog {
    /// A new run's file in `dir` (made if needed), named by the clock's
    /// time and `pid`, its lock taken; then the previous run's end is
    /// checked and the runs beyond `keep` are pruned. `home` is shown as
    /// `~`; the file stops taking lines past `cap` bytes.
    pub fn open(
        dir: &Path,
        clock: Box<dyn Clock>,
        pid: u32,
        home: Option<&Path>,
        cap: u64,
        keep: usize,
    ) -> io::Result<(RunLog, Opened)> {
        fs::create_dir_all(dir)?;
        let name = format::file_name(clock.now(), pid);
        let (path, file) = (0..100)
            .find_map(|n| {
                let path = match n {
                    0 => dir.join(&name),
                    n => dir.join(name.replace(".log", &format!("-{n}.log"))),
                };
                match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(f) => Some(Ok((path, f))),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                    Err(e) => Some(Err(e)),
                }
            })
            .unwrap_or_else(|| Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free log name")))?;
        let lock = fs::OpenOptions::new().write(true).create(true).truncate(true).open(lock_path(&path))?;
        lock.lock()?;
        let noted = format::line(clock.now(), Level::Warn, "log", UNCLEAN_NOTED);
        let opened =
            Opened { previous_unclean: previous_unclean(dir, &path, &noted), pruned: prune(dir, keep) };
        let log = RunLog {
            path,
            file: BufWriter::with_capacity(64 * 1024, file),
            _lock: lock,
            clock,
            home: redact::home_text(home),
            written: 0,
            cap,
            truncated: false,
            last_flush: Instant::now(),
        };
        Ok((log, opened))
    }

    /// This run's file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes one event: the home directory hidden, and at `info` and
    /// above the addresses and keys too. Past the cap one line says so,
    /// then only errors get in (up to [`ERROR_ALLOWANCE`] more). Flushed on
    /// a warning or an error, else at most every [`FLUSH_EVERY`].
    pub fn write(&mut self, level: Level, source: &str, message: &str) {
        let message = redact::home(message, self.home.as_deref());
        let message = if level <= Level::Info { redact::network(&message) } else { message };
        let line = format::line(self.clock.now(), level, source, &message);
        let size = line.len() as u64;
        if self.truncated || self.written + size > self.cap {
            if !self.truncated {
                self.truncated = true;
                let note = format!(
                    "the log reached its {} KiB cap: only errors are written from here on",
                    self.cap / 1024
                );
                let note = format::line(self.clock.now(), Level::Warn, "log", &note);
                self.put(&note);
            }
            if level != Level::Error || self.written + size > self.cap + ERROR_ALLOWANCE {
                return;
            }
        }
        self.put(&line);
        if level <= Level::Warn || self.last_flush.elapsed() >= FLUSH_EVERY {
            self.flush();
        }
    }

    fn put(&mut self, line: &str) {
        if self.file.write_all(line.as_bytes()).is_ok() {
            self.written += line.len() as u64;
        }
    }

    /// Writes the buffered lines to the disk.
    pub fn flush(&mut self) {
        let _ = self.file.flush();
        self.last_flush = Instant::now();
    }

    /// The run's last line, [`CLEAN_END`] (past the cap too), flushed.
    pub fn end(&mut self) {
        let line = format::line(self.clock.now(), Level::Info, "olr", CLEAN_END);
        self.put(&line);
        self.flush();
    }
}
