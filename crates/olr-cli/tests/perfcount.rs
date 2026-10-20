//! The instruction counter behind `olr-cli quick-bench`. Where the kernel
//! offers no counter (a VM without one, a locked-down `perf_event_paranoid`)
//! the tests print a notice and pass, as `quick-bench` does.

use std::hint::black_box;

use olr_cli::perfcount::Instructions;

fn adds(n: u64) -> u64 {
    let mut sum = 0u64;
    for i in 0..n {
        sum = black_box(sum.wrapping_add(black_box(i)));
    }
    sum
}

#[test]
fn counts_a_loop() {
    let mut counter = match Instructions::open() {
        Ok(c) => c,
        Err(e) => return println!("skip: {e}"),
    };
    let (_, count) = counter.measure(|| adds(1_000_000));
    assert!(count >= 1_000_000, "{count} instructions for a million adds");
}

#[test]
fn repeatable() {
    let mut counter = match Instructions::open() {
        Ok(c) => c,
        Err(e) => return println!("skip: {e}"),
    };
    let (_, a) = counter.measure(|| adds(1_000_000));
    let (_, b) = counter.measure(|| adds(1_000_000));
    let spread = a.abs_diff(b) as f64 / a as f64;
    assert!(spread < 0.01, "{a} vs {b}");
}
