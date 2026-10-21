//! The run log against a temporary directory and a clock the test sets:
//! names, lines, pruning, the cap, redaction and the previous run's end.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use olr_log::format::{self, Clock, Stamp};
use olr_log::run::{self, CLEAN_END, RunLog};
use olr_log::{Level, redact};

/// 2026-10-21 12:03:07.045 UTC.
const T0: i64 = 1_792_584_187_045;

/// A clock at `ms`, two hours east of UTC.
#[derive(Clone)]
struct TestClock(Arc<AtomicI64>);

impl TestClock {
    fn at(ms: i64) -> TestClock {
        TestClock(Arc::new(AtomicI64::new(ms)))
    }
}

impl Clock for TestClock {
    fn now(&self) -> Stamp {
        Stamp { unix_ms: self.0.load(Ordering::Relaxed), offset_secs: 7200 }
    }
}

fn open(dir: &Path, ms: i64, cap: u64) -> (RunLog, run::Opened) {
    RunLog::open(dir, Box::new(TestClock::at(ms)), 4242, Some(Path::new("/home/ann")), cap, 10).unwrap()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn lines_have_local_time_level_source_and_indented_lines() {
    let s = Stamp { unix_ms: T0, offset_secs: 7200 };
    assert_eq!(format::local_time(s), "2026-10-21 14:03:07.045");
    assert_eq!(
        format::line(s, Level::Info, "shell", "hello"),
        "2026-10-21 14:03:07.045 info  shell: hello\n"
    );
    assert_eq!(
        format::line(s, Level::Error, "panic", "first\n  second\nthird\n"),
        "2026-10-21 14:03:07.045 error panic: first\n      second\n    third\n"
    );
    assert_eq!(format::source("olr_game::shell::online"), "shell::online");
    assert_eq!(format::source("wgpu_core::device"), "wgpu_core::device");
    // A negative offset across midnight.
    let west = Stamp { unix_ms: T0, offset_secs: -13 * 3600 };
    assert_eq!(format::local_time(west), "2026-10-20 23:03:07.045");
}

#[test]
fn file_names_are_utc_and_sort_by_start() {
    let a = format::file_name(Stamp { unix_ms: T0, offset_secs: 7200 }, 99_999);
    assert_eq!(a, "2026-10-21_12-03-07.045Z_99999.log");
    let b = format::file_name(Stamp { unix_ms: T0 + 1, offset_secs: -36_000 }, 1);
    let c = format::file_name(Stamp { unix_ms: T0 + 86_400_000 * 400, offset_secs: 0 }, 1);
    assert!(a < b && b < c, "{a} {b} {c}");
}

#[test]
fn a_run_writes_its_file_and_ends_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("logs");
    let (mut log, opened) = open(&dir, T0, 1 << 20);
    assert!(opened.previous_unclean.is_none() && opened.pruned.is_empty());
    assert_eq!(log.path(), dir.join("2026-10-21_12-03-07.045Z_4242.log"));
    assert!(run::is_live(log.path()), "its lock is held");
    log.write(Level::Info, "olr", "olr 1.0");
    log.write(Level::Warn, "shell", "a warning");
    assert_eq!(
        read(log.path()),
        "2026-10-21 14:03:07.045 info  olr: olr 1.0\n2026-10-21 14:03:07.045 warn  shell: a warning\n",
        "a warning is flushed"
    );
    log.end();
    assert!(read(log.path()).ends_with("info  olr: the run ended cleanly\n"));
}

#[test]
fn the_last_ten_runs_are_kept_but_live_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let mut held = Vec::new();
    for i in 0..12 {
        let (mut log, _) = open(dir, T0 + i * 1000, 1 << 20);
        log.end();
        // The two oldest stay running.
        if i < 2 {
            held.push(log);
        }
    }
    // Twelve runs, two of them live: nothing deletable yet beyond them.
    assert_eq!(run::runs(dir).len(), 12);
    drop(held);
    let (log, opened) = open(dir, T0 + 100_000, 1 << 20);
    assert_eq!(opened.pruned.len(), 3, "{:?}", opened.pruned);
    let left = run::runs(dir);
    assert_eq!(left.len(), 10);
    assert_eq!(left.last().map(PathBuf::as_path), Some(log.path()));
    // Their lock files went with them.
    let locks =
        fs::read_dir(dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().unwrap() == "lock");
    assert_eq!(locks.count(), 10);
    // Another directory's files are left alone.
    fs::write(dir.join("notes.txt"), "x").unwrap();
    run::prune(dir, 1);
    assert!(dir.join("notes.txt").exists());
    assert_eq!(run::runs(dir), vec![log.path().to_path_buf()], "a live run survives any pruning");
}

#[test]
fn the_cap_keeps_errors_and_the_end() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut log, _) = open(tmp.path(), T0, 400);
    for i in 0..20 {
        log.write(Level::Info, "olr", &format!("line {i}"));
    }
    log.write(Level::Warn, "olr", "a late warning");
    log.write(Level::Error, "panic", "thread 'main' panicked at src/x.rs:1:2: boom");
    log.end();
    let text = read(log.path());
    let lines: Vec<&str> = text.lines().collect();
    let notes = lines.iter().filter(|l| l.contains("the log reached its")).count();
    assert_eq!(notes, 1, "{text}");
    assert!(!text.contains("line 19") && !text.contains("a late warning"), "{text}");
    assert!(text.contains("error panic: thread 'main' panicked at src/x.rs:1:2: boom"), "{text}");
    assert!(lines.last().unwrap().ends_with(CLEAN_END));
    assert!(text.len() < 400 + 300, "{}", text.len());
}

#[test]
fn the_file_hides_home_addresses_and_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut log, _) = open(tmp.path(), T0, 1 << 20);
    log.write(Level::Info, "olr", "saved replay /home/ann/.local/share/olr/replays/a.olrp, not /home/anna/x");
    log.write(Level::Info, "olr", "joining 192.168.1.20:23400");
    log.write(Level::Debug, "olr", "joining 192.168.1.20:23400 (debug keeps it)");
    log.end();
    let text = read(log.path());
    assert!(text.contains("saved replay ~/.local/share/olr/replays/a.olrp, not /home/anna/x"), "{text}");
    assert!(text.contains("joining <ip>:23400\n"), "{text}");
    assert!(text.contains("joining 192.168.1.20:23400 (debug keeps it)"), "{text}");
}

#[test]
fn redaction_hides_what_identifies_and_keeps_the_rest() {
    let n = |s: &str| redact::network(s).into_owned();
    assert_eq!(n("from 10.0.0.1, then 8.8.8.8."), "from <ip>, then <ip>.");
    assert_eq!(n("turn:1.2.3.4:3478?transport=udp"), "turn:<ip>:3478?transport=udp");
    assert_eq!(n("peer [2001:db8::1]:23400 and fe80::1 and ::1"), "peer [<ip>]:23400 and <ip> and <ip>");
    assert_eq!(n("::ffff:1.2.3.4 left"), "<ip> left");
    let key = "ab".repeat(32);
    assert_eq!(n(&format!("key {key}.")), "key <key>.");
    assert_eq!(n("npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqq joined"), "<key> joined");
    assert_eq!(n("code olr1qpzry9x8gf2tvdw0s3jn54khce6mua7l"), "code <invite>");
    for kept in [
        "olr_game::shell::online: 0.2.0-dev.192.24b2ac4",
        "at 14:03:07 on Mesa 24.0.5, driver 31.0.101.5186",
        "the hash 0123456789abcdef and 1.2.3",
        "v1.2.3.4 and 300.1.1.1 and a::b in std::fs",
        "crates/olr-game/src/app.rs:592:9",
        "Résumé 1.2.3.4é",
    ] {
        let out = n(kept);
        if kept.starts_with("Résumé") {
            assert_eq!(out, "Résumé <ip>é");
        } else {
            assert_eq!(out, kept);
        }
    }
    let h = |s: &str| redact::home(s, Some("/home/ann")).into_owned();
    assert_eq!(h("/home/ann"), "~");
    assert_eq!(h("in /home/ann/x and /home/ann."), "in ~/x and ~.");
    assert_eq!(h("/home/anna/x /home/ann_b"), "/home/anna/x /home/ann_b");
    let win = redact::home_text(Some(Path::new("C:\\Users\\Ann\\")));
    assert_eq!(redact::home("C:\\Users\\Ann\\AppData", win.as_deref()), "~\\AppData");
    assert_eq!(redact::home_text(Some(Path::new("/"))), None);
}

#[test]
fn a_run_without_its_clean_end_is_reported_once_ended() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    // A run that crashed: its lock released, no clean line.
    {
        let (mut log, _) = open(dir, T0, 1 << 20);
        log.write(Level::Info, "olr", "playing");
        log.flush();
    }
    let (mut second, opened) = open(dir, T0 + 1000, 1 << 20);
    let crashed = run::runs(dir)[0].clone();
    assert_eq!(opened.previous_unclean, Some(crashed));
    // A run still going is not reported.
    let (_third, opened) = open(dir, T0 + 2000, 1 << 20);
    assert_eq!(opened.previous_unclean, None, "the live second run is skipped, the first was reported");
    second.end();
    drop(second);
    let (_fourth, opened) = open(dir, T0 + 3000, 1 << 20);
    assert_eq!(opened.previous_unclean, None, "the third is live and the second ended cleanly");
}

#[test]
fn an_unwritable_directory_fails_to_open() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a file");
    fs::write(&file, "x").unwrap();
    assert!(RunLog::open(&file.join("logs"), Box::new(TestClock::at(T0)), 1, None, 1 << 20, 10).is_err());
}
