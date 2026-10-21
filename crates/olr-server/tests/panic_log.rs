//! The server's panic hook (issue #2): a panic on any thread lands in
//! `server.log`, stamped, with its message, location and thread (the hook
//! is process-global: a test binary of its own, one test).

use olr_server::files::{log_panics, panic_lines};

fn stamp() -> String {
    "2026-10-21 07:20:05 ".into()
}

#[test]
fn a_panic_lands_in_the_server_log() {
    assert_eq!(
        panic_lines("thread 'main' panicked at a.rs:1:2: boom\nmore\nbacktrace:", "T "),
        "T panic: thread 'main' panicked at a.rs:1:2: boom\nT     more\nT     backtrace:\n"
    );
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("server.log");
    std::fs::write(&log, "2026-10-21 07:20:00 an earlier line\n").unwrap();
    log_panics(log.clone(), stamp);
    let joined = std::thread::Builder::new()
        .name("olr-server-test".into())
        .spawn(|| panic!("the match broke"))
        .unwrap()
        .join();
    assert!(joined.is_err());
    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "2026-10-21 07:20:00 an earlier line", "{text}");
    let first = format!("2026-10-21 07:20:05 panic: thread 'olr-server-test' panicked at {}:", file!());
    assert!(lines[1].starts_with(&first) && lines[1].ends_with(": the match broke"), "{text}");
    assert!(lines[2..].iter().all(|l| l.starts_with("2026-10-21 07:20:05     ")), "{text}");
}
