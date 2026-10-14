//! The Docker NAT tests' dev tools (feature `nat-tools`, plan 12b Task
//! 10): `rl-cli nostr-relay` and `rl-cli stun-server`, rl-discovery's test
//! servers as processes. Each prints where it listens on its first line,
//! then serves until killed. Never in a release build (`xtask dist` does
//! not build rl-cli).

use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use anyhow::Context as _;
use rl_discovery::{StunResponder, TestRelay};

/// A Nostr relay (rl-discovery's `TestRelay`, honest) until killed. Its
/// first line is `nostr relay: ws://<address>:<port>` (loopback for an
/// unspecified address: a caller listening on every interface names the
/// reachable URL itself).
#[derive(clap::Args)]
pub struct RelayArgs {
    /// Where it listens (port 0: a free one); loopback by default.
    #[arg(long, default_value = "127.0.0.1:0")]
    listen: SocketAddr,
}

/// A STUN server answering each binding request with its observed source
/// (rl-discovery's `StunResponder::reflecting`) until killed. Its first
/// line is `stun server: <address>:<port>` (loopback for an unspecified
/// address, as the relay's URL: a caller listening on every interface
/// names the reachable address itself, such as 203.0.113.10 in Docker).
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
    announce(&format!("stun server: {}", local(stun.local_addr())))?;
    serve()
}

/// `addr` as reached on this machine: loopback for an unspecified address
/// (as the relay's URL; the caller names a reachable one, such as the
/// Docker WAN address).
fn local(mut addr: SocketAddr) -> SocketAddr {
    if addr.ip().is_unspecified() {
        addr.set_ip(match addr.ip() {
            IpAddr::V4(_) => Ipv4Addr::LOCALHOST.into(),
            IpAddr::V6(_) => Ipv6Addr::LOCALHOST.into(),
        });
    }
    addr
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unspecified_address_is_printed_as_loopback() {
        let sa = |s: &str| s.parse::<SocketAddr>().unwrap();
        assert_eq!(local(sa("0.0.0.0:3479")), sa("127.0.0.1:3479"));
        assert_eq!(local(sa("[::]:3479")), sa("[::1]:3479"));
        assert_eq!(local(sa("203.0.113.10:3479")), sa("203.0.113.10:3479"));
    }
}
