//! The game's run log (issue #2): every run of `olr` writes
//! `<data>/logs/<start>_<pid>.log` through the [`log`] facade, mirrored to
//! the terminal as the game always printed (`olr: …` on stdout, `warning:
//! …` and `error: …` on stderr), and a panic hook writes every panic into
//! it before the process goes on or ends.
//!
//! - **Files** ([`run`]): the last [`KEEP`] runs kept, each capped at
//!   [`MAX_BYTES`]; a lock file per run, so another instance's live log is
//!   neither pruned nor reported as crashed. A run's last line is
//!   [`CLEAN_END`]; the next run warns when the previous one lacks it.
//! - **Lines** ([`format`]): `2026-10-21 14:03:07.123 info  shell: …`,
//!   local time, level, source (the module, `olr_game::` left off), the
//!   message; its further lines indented.
//! - **Privacy** ([`redact`]): the file shows the home directory as `~`,
//!   and hides addresses and keys at `info` and above.
//! - **Levels:** `info` by default, `debug` on request (`--log debug`,
//!   `OLR_LOG=debug`). Other crates' records (wgpu, winit, …) reach the
//!   file from `warn` up and never the terminal.

pub mod format;
pub mod redact;
pub mod run;

use std::io::{self, Write as _};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

pub use format::{Clock, Stamp, SystemClock};
pub use log::{Level, LevelFilter};
pub use run::{CLEAN_END, KEEP, MAX_BYTES, RunLog, UNCLEAN_NOTED};

/// The environment variable that sets the level (`--log` wins).
pub const LEVEL_VAR: &str = "OLR_LOG";
/// The logs directory under the data directory.
pub const LOGS_DIR: &str = "logs";
/// The most backtrace lines a panic's record keeps.
pub const MAX_BACKTRACE_LINES: usize = 80;

/// A level by name: `error`, `warn`, `info` or `debug` (`trace` is
/// `debug`; any case).
pub fn parse_level(name: &str) -> Option<LevelFilter> {
    match name.trim().to_ascii_lowercase().as_str() {
        "error" => Some(LevelFilter::Error),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" | "trace" => Some(LevelFilter::Debug),
        _ => None,
    }
}

/// How the log starts.
pub struct Options {
    /// The logs directory (`<data>/logs`); `None`: the terminal alone.
    pub dir: Option<PathBuf>,
    pub level: LevelFilter,
    /// Shown as `~` in the file.
    pub home: Option<PathBuf>,
    /// The file's first lines (the version, the system): not mirrored.
    pub header: Vec<String>,
}

struct Logger {
    level: LevelFilter,
    file: Mutex<Option<RunLog>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Whether `target` is this project's (mirrored, at the run's level).
fn ours(target: &str) -> bool {
    target == "olr" || target.starts_with("olr_") || target.starts_with("olr::")
}

impl Logger {
    /// The file, also after a panic while it was held; `None` when another
    /// thread holds it past `wait` (a panic hook must not hang).
    fn file(&self, wait: Option<Duration>) -> Option<MutexGuard<'_, Option<RunLog>>> {
        let Some(wait) = wait else {
            return Some(self.file.lock().unwrap_or_else(|e| e.into_inner()));
        };
        let start = std::time::Instant::now();
        loop {
            match self.file.try_lock() {
                Ok(g) => return Some(g),
                Err(std::sync::TryLockError::Poisoned(e)) => return Some(e.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) if start.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(std::sync::TryLockError::WouldBlock) => return None,
            }
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        if ours(m.target()) { m.level() <= self.level } else { m.level() <= Level::Warn }
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        if ours(record.target()) {
            mirror(record.level(), &message);
        }
        if let Some(mut file) = self.file(None)
            && let Some(f) = file.as_mut()
        {
            f.write(record.level(), format::source(record.target()), &message);
        }
    }

    fn flush(&self) {
        if let Some(mut file) = self.file(None)
            && let Some(f) = file.as_mut()
        {
            f.flush();
        }
    }
}

/// The terminal's line, as the game printed before the log: `olr: …` on
/// stdout, `warning: …`, `error: …` and `debug: …` on stderr.
pub fn terminal_line(level: Level, message: &str) -> (bool, String) {
    match level {
        Level::Info => (true, format!("olr: {message}")),
        Level::Warn => (false, format!("warning: {message}")),
        Level::Error => (false, format!("error: {message}")),
        Level::Debug | Level::Trace => (false, format!("debug: {message}")),
    }
}

fn mirror(level: Level, message: &str) {
    let (stdout, line) = terminal_line(level, message);
    // A closed terminal is no reason to fail.
    let _ = if stdout {
        writeln!(io::stdout().lock(), "{line}")
    } else {
        writeln!(io::stderr().lock(), "{line}")
    };
}

/// Starts the log: the run's file (or the terminal alone, with a warning,
/// when the directory cannot be written), the [`log`] facade, the panic
/// hook. Then warns of a previous run that ended unexpectedly. Returns
/// the run's file. A second call does nothing.
pub fn init(options: Options, clock: Box<dyn Clock>) -> Option<PathBuf> {
    if LOGGER.get().is_some() {
        return path();
    }
    let mut warning = None;
    let mut opened = run::Opened::default();
    let file = options.dir.as_ref().and_then(|dir| {
        match RunLog::open(dir, clock, std::process::id(), options.home.as_deref(), MAX_BYTES, KEEP) {
            Ok((mut f, o)) => {
                for h in &options.header {
                    f.write(Level::Info, "olr", h);
                }
                opened = o;
                Some(f)
            }
            Err(e) => {
                warning = Some(format!(
                    "cannot write the log in {}: {e}; the terminal alone has it",
                    dir.display()
                ));
                None
            }
        }
    });
    let path = file.as_ref().map(|f| f.path().to_path_buf());
    let logger = LOGGER.get_or_init(|| Logger { level: options.level, file: Mutex::new(file) });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(options.level.max(LevelFilter::Warn));
    }
    install_panic_hook();
    if let Some(w) = warning {
        log::warn!(target: "olr", "{w}");
    }
    if let Some(p) = &opened.previous_unclean {
        log::warn!(target: "olr", "the previous run ended unexpectedly (a crash or a kill): its log is {}", p.display());
    }
    for p in &opened.pruned {
        log::debug!(target: "olr", "deleted the old log {}", p.display());
    }
    path
}

/// The run's file, if the log writes one.
pub fn path() -> Option<PathBuf> {
    let logger = LOGGER.get()?;
    let file = logger.file(None)?;
    file.as_ref().map(|f| f.path().to_path_buf())
}

/// Writes a line to the file alone (the terminal shows it otherwise).
pub fn to_file(level: Level, source: &str, message: &str) {
    if let Some(logger) = LOGGER.get()
        && let Some(mut file) = logger.file(None)
        && let Some(f) = file.as_mut()
    {
        f.write(level, source, message);
    }
}

/// The run ends by itself: its last line, [`CLEAN_END`], flushed. Nothing
/// is written after it.
pub fn end() {
    if let Some(logger) = LOGGER.get()
        && let Some(mut file) = logger.file(None)
        && let Some(mut f) = file.take()
    {
        f.end();
    }
}

/// A panic as the log records it: `thread '<name>' panicked at
/// <file>:<line>:<col>: <message>`, then the backtrace when there is one
/// (at most [`MAX_BACKTRACE_LINES`]; a release without symbols shows
/// addresses).
pub fn panic_report(info: &PanicHookInfo<'_>) -> String {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    let message = info.payload_as_str().unwrap_or("Box<dyn Any>");
    let at = info.location().map_or("an unknown place".to_string(), |l| l.to_string());
    let mut out = format!("thread '{name}' panicked at {at}: {message}");
    let trace = std::backtrace::Backtrace::force_capture();
    if trace.status() == std::backtrace::BacktraceStatus::Captured {
        out.push_str("\nbacktrace:");
        let text = trace.to_string();
        let lines: Vec<&str> = text.lines().collect();
        for l in lines.iter().take(MAX_BACKTRACE_LINES) {
            out.push('\n');
            out.push_str(l);
        }
        if lines.len() > MAX_BACKTRACE_LINES {
            out.push_str(&format!("\n… {} more lines", lines.len() - MAX_BACKTRACE_LINES));
        }
    }
    out
}

/// Every panic, caught or not, goes into the file as an error (flushed);
/// then the hook that was there (the terminal's message) runs.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(logger) = LOGGER.get()
            && let Some(mut file) = logger.file(Some(Duration::from_millis(200)))
            && let Some(f) = file.as_mut()
        {
            f.write(Level::Error, "panic", &panic_report(info));
            f.flush();
        }
        previous(info);
    }));
}

/// The data directory's logs directory.
pub fn logs_dir(data: &Path) -> PathBuf {
    data.join(LOGS_DIR)
}
