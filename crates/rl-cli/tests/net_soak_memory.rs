//! `net-soak` keeps nothing between its cases (plan 12c Task 11): the
//! process's resident memory after 8 runs is that after 2 (within 16 MiB).
//! Its own test binary: it reads process-global state.

mod common;

use rl_cli::soak::{MAX_FAILURES, SoakCase, SoakConfig, case_short, run_cases};

/// The process's resident set in bytes (Linux), `None` elsewhere.
fn rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

#[test]
fn memory_stays_flat_over_cases() {
    let Some(dir) = common::content_dir() else { return };
    let Some(_) = rss() else {
        eprintln!("no /proc/self/status: skipping");
        return;
    };
    let failures = tempfile::tempdir().unwrap();
    let soak = |runs: u32| {
        let cfg = SoakConfig {
            minutes: 60.0,
            seed: 1,
            transport_every: 0,
            runs: Some(runs),
            max_failures: MAX_FAILURES,
            failures: failures.path().to_path_buf(),
            content_dir: dir.clone().into(),
        };
        // The same case every time: any growth is the soak's.
        let summary = run_cases(&cfg, &mut |_, _| SoakCase::Net(case_short(3)), &mut Vec::new()).unwrap();
        assert_eq!((summary.runs, summary.failures.len()), (runs, 0));
    };
    soak(2);
    let after_2 = rss().unwrap();
    soak(6);
    let after_8 = rss().unwrap();
    let grew = after_8.saturating_sub(after_2);
    assert!(grew <= 16 << 20, "RSS {} MiB after 2 runs, {} MiB after 8", after_2 >> 20, after_8 >> 20);
}
