//! `rl-cli net-repro`: replays a desync bundle (`.rlds`) and names the first
//! diverging tick and world field.

use std::path::PathBuf;

use anyhow::Context as _;
use rl_cli::bundle::{DesyncBundle, reproduce};

/// Replays a desync bundle written by `net-sim` (or a real game); exits with
/// status 1 if anything differs from the recording.
#[derive(clap::Args)]
pub struct Args {
    bundle: PathBuf,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
}

/// Returns `Ok(false)` if the replay differs from the bundle.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let bytes = std::fs::read(&args.bundle).with_context(|| format!("reading {}", args.bundle.display()))?;
    let b = DesyncBundle::decode(&bytes).with_context(|| format!("decoding {}", args.bundle.display()))?;
    let content = rl_content::Content::open(&args.content)?;
    println!(
        "peer {} desynced at tick {} (host checksum {:#x}); recorded on {}, fingerprint {:#x}{}",
        b.peer,
        b.host.0,
        b.host.1,
        b.platform,
        b.fingerprint,
        if b.fingerprint == rl_proto::FINGERPRINT { "" } else { " (differs from this build's)" },
    );
    let r = reproduce(&content, &b)?;
    match r.first_local_divergence {
        None => println!("local stream: matches the replay"),
        Some(t) => println!(
            "local stream: differs from tick {t} (this machine's simulation differs from the recording peer's)"
        ),
    }
    for (what, state, diff) in
        [("local", &b.local_state, &r.local_diff), ("host", &b.host_state, &r.host_diff)]
    {
        match (state, diff) {
            (None, _) => println!("{what} state: not recorded"),
            (Some((t, _)), None) => println!("{what} state at {t}: equal"),
            (Some((t, _)), Some(path)) => println!("{what} state at {t} differs first at {path}"),
        }
    }
    Ok(r.is_clean())
}
