//! Developer workflows. Run from anywhere in the workspace with `cargo xtask <command>`.

mod api;
mod bench;
mod bless;
mod bless_regression;
mod check;
mod e2e;
mod envelope;
mod fetch;
mod layers;
mod olx_timing;
mod oracle;
mod oracle_gen;
mod tidy;
mod winbuild;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cargo xtask")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// The fast gate: fmt, clippy, layering, wasm build, OLX content, tests. Prints only failures.
    Check {
        /// Do not download OLX content; tests that need it are skipped.
        #[arg(long)]
        offline: bool,
    },
    /// `check`, then the bot-match grid and the 30-minute match, then the GPU tier:
    /// the renderer's GPU tests and the render goldens (skipped with a notice when
    /// no graphics adapter opens).
    Full {
        /// Do not download OLX content; tests that need it are skipped.
        #[arg(long)]
        offline: bool,
    },
    /// Check crate layering rules from xtask/layers.ron.
    Layers,
    /// Write public API summaries to target/api/<crate>.txt.
    Api,
    /// Download the pinned OLX content and import it into content/packs.
    FetchOlxContent {
        /// Only download; print the game directory path and skip importing.
        #[arg(long)]
        no_import: bool,
    },
    /// Reclaim disk: remove target incremental caches and superseded oracle Docker images.
    Tidy {
        /// Also bound Docker's build cache to 2 GB (`docker builder prune`): the
        /// cache is the whole machine's, so this prunes other projects' layers too.
        #[arg(long)]
        docker_cache: bool,
    },
    /// Play fixtures/autoplay/menus.ron in the game on Xvfb (software
    /// rendering, a fake clock) and check its log and screenshots.
    E2e {
        /// Do not download OLX content (the run is skipped without it).
        #[arg(long)]
        offline: bool,
    },
    /// The benchmarks (divan) and the short stress run against their
    /// baselines in fixtures/bench/ (medians of 3 runs, fail over +10 %).
    Bench {
        /// Measure and write the baselines instead, logged in fixtures/blessings.log.
        #[arg(long, requires = "reason")]
        bless: bool,
        /// Why the baselines change (with --bless).
        #[arg(long)]
        reason: Option<String>,
        /// Do not download OLX content (the content benchmarks and stress are skipped).
        #[arg(long)]
        offline: bool,
    },
    /// The engine stress test: 32 chaos bots on Castle Strike for minutes, both
    /// mods, OLX's projectile pool and a raised one; prints each report.
    Stress {
        /// Seconds of match time per configuration.
        #[arg(long, default_value_t = 180.0)]
        seconds: f32,
    },
    /// The performance envelope (plan 8): sweeps of worms, projectiles, effects, map
    /// size and art scale, the GPU on WSL and native Windows with the shader probe,
    /// and OLX's frame times; compared with fixtures/bench/envelope/<host>.ron
    /// (flagged, never failed). About 45 minutes (--quick about 10). Not in `full`.
    Envelope {
        /// One run per point and fewer points.
        #[arg(long)]
        quick: bool,
        /// Only these parts (repeatable): worms, pool, fx, map, art, gpu, olx.
        #[arg(long, value_enum)]
        only: Vec<envelope::Axis>,
        /// Which GPU paths run.
        #[arg(long, value_enum, default_value = "both")]
        gpu: envelope::GpuPaths,
        /// Write the results for this host, logged in fixtures/blessings.log.
        #[arg(long, requires = "reason")]
        bless: bool,
        /// Why the results change (with --bless).
        #[arg(long)]
        reason: Option<String>,
        /// Write docs/perf/envelope.md from this host's results file (or --from) and stop.
        #[arg(long, conflicts_with_all = ["bless", "quick"])]
        report: bool,
        /// The results file the report is made from.
        #[arg(long, requires = "report")]
        from: Option<PathBuf>,
    },
    /// Install a git pre-push hook that runs `cargo xtask check`.
    InstallHooks,
    /// Record OLX reference traces for fixtures/oracle/**/*.ron (needs Docker).
    OracleRecord {
        /// Re-record and compare with the committed traces instead of writing them.
        #[arg(long)]
        check: bool,
        /// Scenario names (paths in fixtures/oracle/ without `.ron`, such as
        /// `walk` or `weapons/classic/bazooka`); all if none.
        names: Vec<String>,
    },
    /// Write one scenario per weapon of the imported mods to fixtures/oracle/weapons/.
    OracleGenWeapons,
    /// Re-generate the render goldens in fixtures/render/ and record why in fixtures/blessings.log.
    BlessRender {
        /// Why the rendering changed (recorded with the date and the changed files).
        #[arg(long)]
        reason: String,
    },
    /// Re-record the golden match checksums in fixtures/matches/ and record why in
    /// fixtures/blessings.log.
    BlessRegression {
        /// Why the matches play differently (recorded with the date and the changed files).
        #[arg(long)]
        reason: String,
        /// A failure record (target/failures/<name>.ron) to add as a new golden match.
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Build rl-cli and rl-game for Windows (x86_64-pc-windows-gnu), stage them with the
    /// content in Windows' temp directory and list the adapters each backend sees (plan 8).
    Windows,
}

/// The workspace root (the parent of this crate's directory).
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_path_buf()
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Check { offline } => check::run(offline),
        Command::Full { offline } => check::full(offline),
        Command::Layers => layers::run().map(|()| true),
        Command::Api => api::run().map(|()| true),
        Command::FetchOlxContent { no_import } => fetch::run(!no_import).map(|p| {
            println!("RL_OLX_GAMEDIR={}", p.gamedir.display());
            if !no_import {
                println!("RL_CONTENT_DIR={}", p.content.display());
            }
            true
        }),
        Command::Tidy { docker_cache } => tidy::run(docker_cache),
        Command::E2e { offline } => e2e_command(offline),
        Command::Bench { bless, reason, offline } => bench::run(reason.as_deref().filter(|_| bless), offline),
        Command::Stress { seconds } => bench::stress_command(seconds),
        Command::Envelope { quick, only, gpu, bless, reason, report, from } => {
            envelope::run(&envelope::Opts { quick, only, gpu, bless: reason.filter(|_| bless), report, from })
        }
        Command::InstallHooks => install_hooks().map(|()| true),
        Command::OracleRecord { check, names } => oracle::run(&names, check),
        Command::OracleGenWeapons => oracle_gen::run(),
        Command::BlessRender { reason } => bless::run(&reason),
        Command::BlessRegression { reason, from } => bless_regression::run(&reason, from.as_deref()),
        Command::Windows => winbuild::command(),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `cargo xtask e2e`: the content first (as `check` fetches it), then the run.
fn e2e_command(offline: bool) -> anyhow::Result<bool> {
    let content = if offline { None } else { Some(fetch::verify()?.content) };
    match e2e::run(content.as_deref()) {
        Ok(()) => Ok(true),
        Err(why) => {
            println!("FAIL e2e\n{why}");
            Ok(false)
        }
    }
}

fn install_hooks() -> anyhow::Result<()> {
    let hook = root().join(".git/hooks/pre-push");
    std::fs::create_dir_all(hook.parent().expect("hooks dir"))?;
    std::fs::write(&hook, "#!/bin/sh\nexec cargo xtask check\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("installed {}", hook.display());
    Ok(())
}
