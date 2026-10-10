//! `rl-cli`: headless tools for rust-liero. Run `rl-cli --help`.

mod import;
mod scenario;
mod validate;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rl-cli", about = "Headless tools for rust-liero")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Import OpenLieroX content into native packs (one directory per pack).
    Import(import::Args),
    /// Replay oracle scenarios in the simulation and compare them with OLX's traces;
    /// exits with status 1 if any diverges.
    Scenario(scenario::Args),
    /// Validate packs; exits with status 1 if any pack has errors.
    Validate {
        /// Pack directories, or directories containing pack directories.
        paths: Vec<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Import(args) => import::run(&args),
        Command::Scenario(args) => scenario::run(&args),
        Command::Validate { paths } => validate::run(&paths),
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
