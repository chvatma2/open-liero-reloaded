//! `rl-cli match`: a headless bot match (spec §7.1, §7.3).

use std::path::PathBuf;

use rl_cli::failure;
use rl_cli::runner::{Options, play};
use rl_cli::spec::MatchSpec;

/// Plays a headless bot match: checks the invariants every tick, reports
/// how it ended and the tick times, then (unless `--no-verify`) plays it
/// again and replays its recording. A failure writes the replay and the
/// seed to `--failures`.
#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    spec: MatchSpec,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Write the match's replay here.
    #[arg(long)]
    record: Option<PathBuf>,
    /// List the weapons fired, actions triggered, bonuses, rope attachments and kills.
    #[arg(long)]
    coverage: bool,
    /// Do not play the match again (determinism) nor replay it (round trip).
    #[arg(long)]
    no_verify: bool,
    /// Where a failing match writes its replay and record.
    #[arg(long, default_value = "target/failures")]
    failures: PathBuf,
    /// Fail when the 99th-percentile tick time is over this many ms.
    #[arg(long)]
    p99_budget_ms: Option<f64>,
    /// Panic inside this tick (tests the failure capture).
    #[arg(long, hide = true)]
    fail_at: Option<u32>,
}

/// Returns `Ok(false)` if the match failed.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let content = rl_content::Content::open(&args.content)?;
    let run = play(&content, &args.spec, &Options { fail_at: args.fail_at })?;
    println!("{}", run.summary());
    if let Some(path) = &args.record {
        std::fs::write(path, run.replay.encode())?;
        println!("replay: {} ({} ticks)", path.display(), run.ticks);
    }
    if args.coverage {
        let sim = content.load_mod(&args.spec.mod_id)?.sim;
        print!("{}", run.coverage.report(&sim));
    }
    let failure = match run.failure() {
        Some(f) => Some(f.clone()),
        None if args.no_verify => None,
        None => rl_cli::verify(&content, &run)?,
    };
    let mut ok = true;
    if let Some(f) = failure {
        ok = false;
        let record = failure::write(&args.failures, &args.spec, &f, &run.replay)?;
        println!("FAIL {}: failed at tick {}: {:?}: {}", args.spec.stem(), f.tick, f.kind, f.message);
        println!("     wrote {} and its replay", record.display());
        println!("     see the world before it: rl-cli replay <replay> --until {} --dump", f.tick);
    } else if !args.no_verify {
        println!("ok   deterministic, and the replay reproduces every checksum");
    }
    if let Some(budget) = args.p99_budget_ms
        && run.p99_us() > budget * 1000.0
    {
        println!("FAIL p99 tick time {:.0} µs is over {budget} ms", run.p99_us());
        ok = false;
    }
    Ok(ok)
}
