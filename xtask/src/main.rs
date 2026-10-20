//! Developer workflows. Run from anywhere in the workspace with `cargo xtask <command>`.

mod api;
mod bench;
mod bless;
mod bless_regression;
mod check;
mod determinism;
mod dist;
mod e2e;
mod envelope;
mod fetch;
mod flatpak;
mod format_docs;
mod layers;
mod nat;
mod net;
mod olx_timing;
mod oracle;
mod oracle_gen;
mod tidy;
mod version;
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
        /// Also the render goldens, HUD snapshots and the renderer's GPU tests: what a
        /// rendering change needs, without the rest of `full`.
        #[arg(long)]
        render: bool,
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
    /// Write the schema pages in docs/formats/ from olr-format's authoring types.
    FormatDocs {
        /// Only compare: exit 1 naming each page that differs from the code.
        #[arg(long)]
        check: bool,
    },
    /// Write public API summaries to target/api/<crate>.txt.
    Api,
    /// Download the pinned OLX content and import it into content/packs.
    FetchOlxContent {
        /// Only download; print the game directory path and skip importing.
        #[arg(long)]
        no_import: bool,
    },
    /// Reclaim disk: remove superseded builds and incremental sessions from
    /// target/ and superseded Docker images (the oracle's and the NAT tests').
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
    /// (flagged, never failed). About 7 minutes warm (--quick about 2). Not in `full`.
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
    /// The feel numbers of online play (plan 9): every network profile's rollback depth,
    /// corrections, stalls and bytes from seeded `olr-cli net-sim --feel` runs, gated against
    /// fixtures/net/feel.ron (also a step of `full`).
    NetFeel {
        /// Measure and write the baseline instead, logged in fixtures/blessings.log.
        #[arg(long, requires = "reason")]
        bless: bool,
        /// Why the baseline changes (with --bless).
        #[arg(long)]
        reason: Option<String>,
    },
    /// Seeded online cases for as long as asked (plan 12c, spec §9.10): host crashes,
    /// departures and freezes, outages, profile changes and spectators in `net-sim`, every
    /// 5th case a real-transport match on 127.0.0.1. Failures are kept in target/failures.
    /// Not a step of `full`.
    NetSoak {
        /// How long to run.
        #[arg(long, default_value_t = 10.0)]
        minutes: f64,
        /// The first case's seed (default: from the clock, printed).
        #[arg(long)]
        seed: Option<u64>,
        /// Build without the network.
        #[arg(long)]
        offline: bool,
    },
    /// The Docker NAT tests (plan 12b, spec §9.7; also a step of `full`): home routers
    /// (cone, symmetric, UPnP, PCP, NAT-PMP) on isolated networks, a Nostr relay, STUN and a TURN
    /// relay; direct joins, port mapping, hole punching, relays, finding and --nat-check.
    /// Skipped with a notice without Docker.
    NetNat {
        /// Leave the networks and the last scenario's containers (for an hour) for inspection.
        #[arg(long)]
        keep: bool,
        /// Do not download: without the image already built, the step is skipped.
        #[arg(long)]
        offline: bool,
        /// Only these scenarios (repeatable): direct, upnp, pcp, nat-pmp, hole-punching,
        /// relay, no-relay, finding, nat-check.
        #[arg(long)]
        only: Vec<String>,
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
    /// Build olr-cli and olr-game for Windows (x86_64-pc-windows-gnu), stage them with the
    /// content in Windows' temp directory and list the adapters each backend sees (plan 8).
    Windows,
    /// Portable release zips: olr-game and olr-server (the `dist` profile) with the content,
    /// docs and an example server config, for Linux and Windows, in
    /// target/release-zips/open-liero-reloaded-<version>-<platform>.zip; then each is unpacked and run.
    Dist {
        /// Only build the zips: do not unpack and run them.
        #[arg(long)]
        no_verify: bool,
        /// The final release: named `<version>` (not `<version>-dev.<n>.<commit>`),
        /// refused unless the commit is tagged v<version> and nothing is uncommitted.
        #[arg(long = "final")]
        final_release: bool,
    },
    /// The Linux release build repackaged as a Flatpak bundle (runtime
    /// org.freedesktop.Platform 26.08), in
    /// target/release-zips/open-liero-reloaded-<version>-linux-x64.flatpak. Needs flatpak and
    /// flatpak-builder; nothing is run.
    Flatpak,
    /// Cross-platform determinism (plan 9, spec §9.5): golden bot matches' replays are played
    /// by `olr-detcheck` natively and on another platform; the checksum streams must be equal.
    /// With no leg flag the WASM leg (Node.js WASI) runs; a flag runs that leg only.
    Determinism {
        /// The Windows leg (plan 8's interop runner); every golden match.
        #[arg(long)]
        windows: bool,
        /// The arm64 Linux leg under QEMU; every golden match.
        #[arg(long)]
        arm64: bool,
        /// The WASM leg (the default when no leg is named).
        #[arg(long)]
        wasm: bool,
        /// Every golden match, not only the one `check` replays.
        #[arg(long)]
        all: bool,
    },
}

/// The workspace root (the parent of this crate's directory).
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_path_buf()
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Check { offline, render } => check::run(offline, render),
        Command::Full { offline } => check::full(offline),
        Command::Layers => layers::run().map(|()| true),
        Command::FormatDocs { check } => format_docs::run(check),
        Command::Api => api::run().map(|()| true),
        Command::FetchOlxContent { no_import } => fetch::run(!no_import).map(|p| {
            println!("OLR_OLX_GAMEDIR={}", p.gamedir.display());
            if !no_import {
                println!("OLR_CONTENT_DIR={}", p.content.display());
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
        Command::NetFeel { bless, reason } => net::run(reason.as_deref().filter(|_| bless)),
        Command::NetSoak { minutes, seed, offline } => net::soak(minutes, seed, offline),
        Command::NetNat { keep, offline, only } => nat::command(&only, keep, offline),
        Command::InstallHooks => install_hooks().map(|()| true),
        Command::OracleRecord { check, names } => oracle::run(&names, check),
        Command::OracleGenWeapons => oracle_gen::run(),
        Command::BlessRender { reason } => bless::run(&reason),
        Command::BlessRegression { reason, from } => bless_regression::run(&reason, from.as_deref()),
        Command::Windows => winbuild::command(),
        Command::Dist { no_verify, final_release } => dist::command(!no_verify, final_release),
        Command::Flatpak => flatpak::command(),
        Command::Determinism { windows, arm64, wasm, all } => {
            determinism::command(determinism::Opts { wasm, windows, arm64, all })
        }
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
