//! Developer workflows. Run from anywhere in the workspace with `cargo xtask <command>`.

mod api;
mod check;
mod fetch;
mod layers;

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
    /// Check crate layering rules from xtask/layers.ron.
    Layers,
    /// Write public API summaries to target/api/<crate>.txt.
    Api,
    /// Download the pinned OLX content and import it into target/content.
    FetchOlxContent {
        /// Only download; print the game directory path and skip importing.
        #[arg(long)]
        no_import: bool,
    },
    /// Install a git pre-push hook that runs `cargo xtask check`.
    InstallHooks,
}

/// The workspace root (the parent of this crate's directory).
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").to_path_buf()
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Check { offline } => check::run(offline),
        Command::Layers => layers::run().map(|()| true),
        Command::Api => api::run().map(|()| true),
        Command::FetchOlxContent { no_import } => fetch::run(!no_import).map(|p| {
            println!("RL_OLX_GAMEDIR={}", p.gamedir.display());
            if !no_import {
                println!("RL_CONTENT_DIR={}", p.content.display());
            }
            true
        }),
        Command::InstallHooks => install_hooks().map(|()| true),
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
