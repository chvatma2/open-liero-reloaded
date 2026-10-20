//! `olr-runner`: the issue runner's commands.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "olr-runner")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install the binary, the systemd timer and the sessions' config; create the labels.
    Install,
    /// The lock holder, the queue of the open milestone and the last results.
    Status,
    /// Take no new issues (a running session finishes).
    Pause,
    /// Take issues again.
    Resume,
    /// One poll-and-work pass (what the timer runs).
    Tick,
    /// Work one approved issue now, regardless of order.
    Run {
        /// The issue number.
        issue: u64,
    },
    /// Show what the next run would send, with no side effects.
    DryRun,
}

fn main() {
    match Cli::parse().command {
        Command::Install
        | Command::Status
        | Command::Pause
        | Command::Resume
        | Command::Tick
        | Command::Run { .. }
        | Command::DryRun => todo!(),
    }
}
