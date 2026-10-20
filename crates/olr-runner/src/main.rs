//! `olr-runner`: the issue runner's commands.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use olr_runner::github::GhCli;
use olr_runner::run::{self, RunResult};
use olr_runner::settings::Settings;
use olr_runner::state::State;

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

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("olr-runner: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> anyhow::Result<()> {
    let command = Cli::parse().command;
    let s = Settings::load()?;
    let st = State::new(&s.state_dir)?;
    let gh = GhCli { repo: s.repo.clone() };
    match command {
        Command::Install => olr_runner::install::run(&s, &gh)?,
        Command::Status => print!("{}", run::status(&gh, &st)?),
        Command::Pause => {
            st.set_paused(true)?;
            println!("paused: no new issues will be taken");
        }
        Command::Resume => {
            st.set_paused(false)?;
            println!("resumed");
        }
        Command::Tick => run::tick(&gh, &s, &st)?,
        Command::Run { issue } => match run::run_now(&gh, &s, &st, issue)? {
            RunResult::Pushed { first, last } => println!("#{issue} pushed {first}..{last}"),
            RunResult::HandedBack(h) => println!("#{issue} handed back: {}", h.describe()),
        },
        Command::DryRun => println!("{}", run::dry_run(&gh, &s)?),
    }
    Ok(())
}
