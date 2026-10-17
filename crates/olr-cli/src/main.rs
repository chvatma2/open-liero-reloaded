//! `olr-cli`: headless tools for OLR. Run `olr-cli --help`.

mod element_cmd;
mod gpu_bench_cmd;
mod import;
mod level_cmd;
mod match_cmd;
#[cfg(feature = "nat-tools")]
mod nat_tools;
mod net_client_cmd;
mod net_repro_cmd;
mod net_sim_cmd;
mod net_soak_cmd;
mod pack_cmd;
mod render;
mod replay_cmd;
mod rollback_cost_cmd;
mod scenario;
mod stress_cmd;

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "olr-cli", about = "Headless tools for Open Liero Reloaded")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import OpenLieroX content into native packs (one directory per pack).
    Import(import::Args),
    /// Author native packs: check, format and make them.
    Pack(pack_cmd::Args),
    /// Edit a level pack: carve, fill, paint, stamp, resize, its objects; preview it.
    Level(level_cmd::Args),
    /// Replay oracle scenarios in the simulation and compare them with OLX's traces;
    /// exits with status 1 if any diverges.
    Scenario(scenario::Args),
    /// Render one frame of an oracle scenario to a PNG, offscreen with wgpu.
    Render(render::Args),
    /// Play a headless bot match: invariants every tick, tick times, determinism,
    /// replay round trip and coverage; a failure writes its replay to target/failures/.
    Match(match_cmd::Args),
    /// Plays an online match in one process over a simulated network (bots on every peer)
    /// and checks that every peer agrees; exits with status 1 on a failure.
    NetSim(net_sim_cmd::Args),
    /// Seeded online cases (host crashes, departures, freezes, outages; some over real
    /// connections on 127.0.0.1) for as long as asked; keeps failures; exits with status 1 on any.
    NetSoak(net_soak_cmd::Args),
    /// A headless bot client over real connections: joins a server (by address, invite
    /// code, key or listed name), plays its matches, prints its path and each match's
    /// confirmed checksums; exits with status 1 if it was dropped.
    NetClient(net_client_cmd::Args),
    /// Replays a desync bundle (`.olds`) and names the first diverging tick and world field;
    /// exits with status 1 if anything differs from the recording.
    NetRepro(net_repro_cmd::Args),
    /// Play a recorded match, checking its checksums; exits with status 1 on a mismatch.
    Replay(replay_cmd::Args),
    /// The engine stress test: up to 32 chaos bots, near-instant reloads, bonuses,
    /// OLX's projectile pool or a larger one; prints tick, bot, pool, entity,
    /// event, render and memory figures.
    Stress(stress_cmd::Args),
    /// Times a rollback: restoring the world and re-simulating ticks (plan 9, spec §3.3).
    RollbackCost(rollback_cost_cmd::Args),
    /// Draw a recorded match offscreen at several sizes on the GPU, timing the CPU
    /// and (with timestamps) the GPU, plus the shader-headroom probe (plan 8).
    GpuBench(gpu_bench_cmd::Args),
    /// Measures Nostr relays for the default list (by hand only: it publishes probe events
    /// from a fresh random key); exits with status 1 unless every relay passes.
    NostrProbe(olr_cli::nostr_probe::RelayArgs),
    /// Measures STUN servers for the default list; exits with status 1 unless every one answers.
    StunProbe(olr_cli::nostr_probe::StunArgs),
    /// A Nostr relay for the NAT tests (dev only), until killed; prints its URL first.
    #[cfg(feature = "nat-tools")]
    NostrRelay(nat_tools::RelayArgs),
    /// A STUN server answering with the observed source, for the NAT tests (dev only),
    /// until killed; prints its address first.
    #[cfg(feature = "nat-tools")]
    StunServer(nat_tools::StunArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Import(args) => import::run(&args),
        Command::Pack(args) => pack_cmd::run(&args),
        Command::Level(args) => level_cmd::run(&args),
        Command::Scenario(args) => scenario::run(&args),
        Command::Render(args) => render::run(&args),
        Command::Match(args) => match_cmd::run(&args),
        Command::NetSim(args) => net_sim_cmd::run_cmd(args),
        Command::NetSoak(args) => net_soak_cmd::run_cmd(args),
        Command::NetClient(args) => net_client_cmd::run(&args),
        Command::NetRepro(args) => net_repro_cmd::run(&args),
        Command::Replay(args) => replay_cmd::run(&args),
        Command::Stress(args) => stress_cmd::run(&args),
        Command::RollbackCost(args) => rollback_cost_cmd::run(&args),
        Command::GpuBench(args) => gpu_bench_cmd::run(&args),
        Command::NostrProbe(args) => olr_cli::nostr_probe::run_relays(&args),
        Command::StunProbe(args) => olr_cli::nostr_probe::run_stun(&args),
        #[cfg(feature = "nat-tools")]
        Command::NostrRelay(args) => nat_tools::run_relay(&args),
        #[cfg(feature = "nat-tools")]
        Command::StunServer(args) => nat_tools::run_stun(&args),
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
