//! `rl-server --config <file.ron> [--content <dir>] [--data-dir <dir>]`:
//! the dedicated server (networking spec §7.4). Logs every event to the
//! terminal and to `<data>/server.log` (rotated past 10 MiB, two old files
//! kept), each line stamped with the UTC
//! time. Ctrl+C, SIGTERM, closing its console (Windows) or typing `quit`
//! stops it cleanly: everyone gets `Bye`.

use std::fs;
use std::io::{self, BufRead as _, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use clap::Parser;
use rl_content::Content;
use rl_server::config::Config;
use rl_server::files::{LOG_KEEP, LOG_MAX_BYTES, RotatingLog};

/// The log file, in the data directory.
const LOG_FILE: &str = "server.log";

static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Parser)]
#[command(name = "rl-server", about = "The rust-liero dedicated server")]
struct Args {
    /// The server's config (RON; see fixtures/server/example.ron).
    #[arg(long)]
    config: PathBuf,
    /// The content directory (mods, levels, skins): `content/packs` beside
    /// this program when there is one, else in the working directory.
    #[arg(long, default_value_os_t = default_content())]
    content: PathBuf,
    /// Where the server keeps its identity, bans, log and desync bundles
    /// (default: the config file's directory).
    #[arg(long)]
    data_dir: Option<PathBuf>,
}

/// `content/packs` beside the executable (an unpacked release), else in
/// the working directory (the repository).
fn default_content() -> PathBuf {
    rl_content::default_content_dir(std::env::current_exe().ok().as_deref())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let text =
        fs::read_to_string(&args.config).with_context(|| format!("reading {}", args.config.display()))?;
    let config = Config::parse(&text).with_context(|| format!("{}", args.config.display()))?;
    let data_dir = args.data_dir.unwrap_or_else(|| {
        args.config.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| ".".into(), PathBuf::from)
    });
    fs::create_dir_all(&data_dir).with_context(|| format!("making {}", data_dir.display()))?;
    let content =
        Content::open(&args.content).with_context(|| format!("content {}", args.content.display()))?;
    for w in content.warnings() {
        eprintln!("warning: {w}");
    }
    let file = RotatingLog::open(&data_dir.join(LOG_FILE), LOG_MAX_BYTES, LOG_KEEP)
        .with_context(|| format!("opening {}", data_dir.join(LOG_FILE).display()))?;
    let mut log = Stamped { file, line_start: true };
    // Ruling T13-A: the handler only sets the flag the loop reads.
    ctrlc::set_handler(|| STOP.store(true, Ordering::Relaxed)).context("the Ctrl+C handler")?;
    std::thread::Builder::new().name("rl-server-console".into()).spawn(console)?;
    println!("Ctrl+C or quit stops the server");
    rl_server::run(config, content, &data_dir, &STOP, &mut log)
}

/// Reads the terminal: `quit` (or `stop`, `exit`) stops the server. A
/// closed input (a service) changes nothing.
fn console() {
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { return };
        match line.trim() {
            "quit" | "stop" | "exit" => {
                STOP.store(true, Ordering::Relaxed);
                return;
            }
            "" => {}
            other => println!("unknown command {other:?}: type quit to stop the server"),
        }
    }
}

/// The log: each line, stamped with the UTC time, to the terminal and the
/// log file (rotated past 10 MiB, two old files kept).
struct Stamped {
    file: RotatingLog,
    line_start: bool,
}

impl Write for Stamped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut out = Vec::with_capacity(buf.len() + 24);
        for &b in buf {
            if self.line_start {
                out.extend_from_slice(stamp().as_bytes());
                self.line_start = false;
            }
            out.push(b);
            self.line_start = b == b'\n';
        }
        let _ = io::stdout().write_all(&out);
        // A full disk must not stop the server: the terminal still has it.
        let _ = self.file.write_all(&out);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stdout().flush();
        let _ = self.file.flush();
        Ok(())
    }
}

/// `YYYY-MM-DD HH:MM:SS ` in UTC.
fn stamp() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} ", rest / 3600, rest / 60 % 60, rest % 60)
}

/// The proleptic Gregorian date of a day count since 1970-01-01 (Howard
/// Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_gregorian() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(59), (1970, 3, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_739), (2026, 10, 13));
    }
}
