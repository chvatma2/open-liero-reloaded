//! `net-soak` keeps nothing between its cases (plan 12c Task 11): the live
//! heap after 9 runs is that after 3 (within 256 KiB), counted by
//! `allocation_counter` (its global allocator), and the process's resident memory is within 16 MiB (a coarse
//! backstop, Linux only). Its own test binary: a global allocator and
//! process memory are process-global state.

mod common;

use rl_cli::soak::{MAX_FAILURES, SoakCase, SoakConfig, case_short, run_cases};

/// The process's resident set in bytes (Linux), `None` elsewhere.
fn rss() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// The seeds the runs cycle through, the same in both phases.
const SEEDS: [u64; 3] = [3, 4, 5];

#[test]
fn memory_stays_flat_over_cases() {
    let Some(dir) = common::content_dir() else { return };
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
        let cases = &mut |n: u32, _| SoakCase::Net(case_short(SEEDS[n as usize % SEEDS.len()]));
        let summary = run_cases(&cfg, cases, &mut Vec::new()).unwrap();
        assert_eq!((summary.runs, summary.failures.len()), (runs, 0));
    };
    // What each phase leaves allocated (`bytes_current`: net of what it freed),
    // a leak of the soak's or of anything it builds. The cases cycle the same
    // seeds in both phases.
    let first = allocation_counter::measure(|| soak(3));
    let rss_3 = rss();
    let second = allocation_counter::measure(|| soak(9));
    let rss_9 = rss();
    let grew = second.bytes_current - first.bytes_current;
    assert!(
        grew <= 256 << 10,
        "{} B live after 3 runs, {} B after 9 runs",
        first.bytes_current,
        second.bytes_current
    );
    if let (Some(a), Some(b)) = (rss_3, rss_9) {
        assert!(b.saturating_sub(a) <= 16 << 20, "RSS {} MiB after 3 runs, {} MiB after 9", a >> 20, b >> 20);
    }
}
