//! Time stamps and the log's lines.

use std::fmt::Write as _;

use log::Level;

/// A moment: milliseconds since the Unix epoch and the local UTC offset
/// then.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub unix_ms: i64,
    pub offset_secs: i32,
}

/// Where the log's times come from (tests inject one).
pub trait Clock: Send {
    fn now(&self) -> Stamp;
}

/// The system clock, with the local offset read once when it is made:
/// `time` reads it only while the process has one thread on Unix (the
/// game makes its clock first thing in `main`), else UTC. A daylight
/// saving change during a run is not followed.
pub struct SystemClock {
    offset_secs: i32,
}

impl SystemClock {
    pub fn new() -> SystemClock {
        let offset_secs = time::UtcOffset::current_local_offset().map_or(0, |o| o.whole_seconds());
        SystemClock { offset_secs }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Stamp {
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        Stamp { unix_ms, offset_secs: self.offset_secs }
    }
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `(y, m, d, hh, mm, ss, ms)` of `ms` milliseconds since the epoch.
fn fields(ms: i64) -> (i64, u32, u32, i64, i64, i64, i64) {
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (y, m, d) = civil_from_days(days);
    (y, m, d, rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

/// `YYYY-MM-DD HH:MM:SS.mmm`, the local wall-clock time.
pub fn local_time(s: Stamp) -> String {
    let (y, m, d, hh, mm, ss, ms) = fields(s.unix_ms + i64::from(s.offset_secs) * 1000);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}.{ms:03}")
}

/// A run's file name, `YYYY-MM-DD_HH-MM-SS.mmmZ_<pid>.log`: the start in
/// UTC, so the newest sorts last whatever the time zone did.
pub fn file_name(s: Stamp, pid: u32) -> String {
    let (y, m, d, hh, mm, ss, ms) = fields(s.unix_ms);
    format!("{y:04}-{m:02}-{d:02}_{hh:02}-{mm:02}-{ss:02}.{ms:03}Z_{pid}.log")
}

/// A level's column: the issue's names, padded to one width.
pub fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warn => "warn ",
        Level::Info => "info ",
        Level::Debug | Level::Trace => "debug",
    }
}

/// The source column: a record's target, the game's own modules without
/// `olr_game::` (`shell::online`), other crates' in full.
pub fn source(target: &str) -> &str {
    target.strip_prefix("olr_game::").unwrap_or(target)
}

/// What continuation lines start with.
pub const INDENT: &str = "    ";

/// One event: `<time> <level> <source>: <message>`, its message's further
/// lines indented under it, ending in a newline.
pub fn line(s: Stamp, level: Level, source: &str, message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 48);
    let _ = write!(out, "{} {} {source}: ", local_time(s), level_name(level));
    let mut lines = message.trim_end().lines();
    out.push_str(lines.next().unwrap_or(""));
    out.push('\n');
    for l in lines {
        out.push_str(INDENT);
        out.push_str(l);
        out.push('\n');
    }
    out
}
