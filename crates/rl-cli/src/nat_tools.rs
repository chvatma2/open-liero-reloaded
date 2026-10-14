//! The Docker NAT tests' dev tools (feature `nat-tools`, plan 12b Task
//! 10): `rl-cli nostr-relay` and `rl-cli stun-server`, rl-discovery's test
//! servers as processes. Each prints where it listens on its first line,
//! then serves until killed. Never in a release build (`xtask dist` does
//! not build rl-cli).

use std::io::Write as _;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context as _;
use rl_discovery::{StunResponder, TestRelay};

/// A Nostr relay (rl-discovery's `TestRelay`, honest) until killed. Its
/// first line is `nostr relay: ws://<address>:<port>` (loopback for an
/// unspecified address).
#[derive(clap::Args)]
pub struct RelayArgs {
    /// Where it listens (port 0: a free one); loopback by default.
    #[arg(long, default_value = "127.0.0.1:0")]
    listen: SocketAddr,
}

/// A STUN server answering each binding request with its observed source
/// (rl-discovery's `StunResponder::reflecting`) until killed. Its first
/// line is `stun server: <address>:<port>`.
#[derive(clap::Args)]
pub struct StunArgs {
    /// Where it listens (UDP; port 0: a free one); loopback by default.
    #[arg(long, default_value = "127.0.0.1:0")]
    listen: SocketAddr,
}

/// Runs the relay; returns only on an error.
pub fn run_relay(args: &RelayArgs) -> anyhow::Result<bool> {
    let relay = TestRelay::start_on(args.listen).with_context(|| format!("listening on {}", args.listen))?;
    announce(&format!("nostr relay: {}", relay.url()))?;
    serve()
}

/// Runs the STUN server; returns only on an error.
pub fn run_stun(args: &StunArgs) -> anyhow::Result<bool> {
    let stun =
        StunResponder::reflecting(args.listen).with_context(|| format!("listening on {}", args.listen))?;
    announce(&format!("stun server: {}", stun.local_addr()))?;
    serve()
}

/// Prints `line` and flushes it: the caller reads it at once.
fn announce(line: &str) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}")?;
    out.flush()?;
    Ok(())
}

/// Serves until the process is killed.
fn serve() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
