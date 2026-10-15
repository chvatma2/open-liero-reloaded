//! `rl-cli net-soak`: seeded online cases for as long as it is asked
//! (plan 12c Task 11, `rl_cli::soak`).

use std::path::PathBuf;

use rl_cli::soak::{MAX_FAILURES, SoakConfig, run};

/// Plays seeded online cases (host crashes, departures and freezes, outages,
/// profile changes, spectators; every few a real-transport match on
/// 127.0.0.1) until the time is up, keeping every failure as a case in
/// `--failures` (at most 32, then it stops); exits with status 1 if any
/// failed. Not part of `cargo xtask full`: `cargo xtask net-soak`.
#[derive(clap::Args)]
pub struct Args {
    /// How long to run.
    #[arg(long, default_value_t = 10.0)]
    minutes: f64,
    /// The first case's seed; case `n` is seeded `seed + n`.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Every this many runs one is a real-transport case (0: none).
    #[arg(long, default_value_t = 5)]
    transport_every: u32,
    /// Stop after this many runs instead of by the time.
    #[arg(long)]
    runs: Option<u32>,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Where failing cases are kept.
    #[arg(long, default_value = "target/failures")]
    failures: PathBuf,
}

/// Returns `Ok(false)` if any case failed.
pub fn run_cmd(args: Args) -> anyhow::Result<bool> {
    anyhow::ensure!(
        args.minutes.is_finite() && args.minutes >= 0.0,
        "--minutes {} is not a duration",
        args.minutes
    );
    let cfg = SoakConfig {
        minutes: args.minutes,
        seed: args.seed,
        transport_every: args.transport_every,
        runs: args.runs,
        max_failures: MAX_FAILURES,
        failures: args.failures,
        content_dir: args.content,
    };
    let summary = run(&cfg, &mut std::io::stdout())?;
    Ok(summary.failures.is_empty())
}
