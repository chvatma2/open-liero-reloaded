//! ICE candidates (decisions N48-N50): ours, the peer's as its SDP names
//! them, the sanitizing of a signalled offer, and the path a selected
//! candidate pair is.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use rl_proto::Path;

/// A signalled offer keeps at most this many candidate lines (N50).
pub const MAX_CANDIDATES: usize = 8;

/// Our candidates: all share the one UDP socket.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalCandidates {
    /// Where every datagram is taken as arriving (the first host candidate).
    pub base: Option<SocketAddr>,
    /// The host candidates offered, `base` first.
    pub host: Vec<SocketAddr>,
    /// The server-reflexive address STUN reported (the latest, N48).
    pub srflx: Option<SocketAddr>,
    /// The router mapping's external address.
    pub mapped: Option<SocketAddr>,
    /// The TURN relayed addresses allocated (`Core::allocate`).
    pub relayed: Vec<SocketAddr>,
}

/// The peer's candidates, from its SDP (`typ host`, `srflx` or `prflx`,
/// `relay`) and its signalled mapped hint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteCandidates {
    pub host: Vec<SocketAddr>,
    /// Server- and peer-reflexive candidates.
    pub srflx: Vec<SocketAddr>,
    pub relay: Vec<SocketAddr>,
    pub mapped: Option<SocketAddr>,
}

/// A TURN server's address and long-term credentials (`Core::allocate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnSpec {
    pub addr: SocketAddr,
    pub user: String,
    pub pass: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Host,
    Reflexive,
    Relay,
}

/// The address and kind of an `a=candidate` line that a signalled SDP may
/// keep (N50): UDP, an IPv4 address (no mDNS name, no IPv6) that is
/// unicast (not unspecified, multicast or broadcast) with a port, and a
/// known type. Fields are split on single spaces, as str0m's parser reads
/// them; anything else is `None`.
fn kept(line: &str) -> Option<(SocketAddr, Kind)> {
    let body = line.strip_suffix('\n').unwrap_or(line);
    let body = body.strip_suffix('\r').unwrap_or(body);
    let mut fields = body.strip_prefix("a=candidate:")?.split(' ');
    let (_foundation, component, protocol, priority) =
        (fields.next()?, fields.next()?, fields.next()?, fields.next()?);
    component.parse::<u16>().ok()?;
    priority.parse::<u32>().ok()?;
    if !protocol.eq_ignore_ascii_case("udp") {
        return None;
    }
    let ip: Ipv4Addr = fields.next()?.parse().ok()?;
    let port: u16 = fields.next()?.parse().ok()?;
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() || port == 0 {
        return None;
    }
    if fields.next()? != "typ" {
        return None;
    }
    let kind = match fields.next()? {
        "host" => Kind::Host,
        "srflx" | "prflx" => Kind::Reflexive,
        "relay" => Kind::Relay,
        _ => return None,
    };
    Some((SocketAddr::new(IpAddr::V4(ip), port), kind))
}

/// The candidate lines of `sdp` that [`sanitize`] keeps, in order.
fn kept_lines(sdp: &str) -> impl Iterator<Item = (SocketAddr, Kind)> + '_ {
    sdp.split_inclusive('\n').filter_map(kept).take(MAX_CANDIDATES)
}

/// Only the candidate lines `sanitize` keeps (UDP, IPv4, unicast, not
/// unspecified; no mDNS name, TCP, IPv6 or multicast), by `typ`.
pub fn remote_candidates(sdp: &str, mapped_hint: Option<SocketAddr>) -> RemoteCandidates {
    let mut r = RemoteCandidates { mapped: mapped_hint, ..RemoteCandidates::default() };
    for (addr, kind) in kept_lines(sdp) {
        match kind {
            Kind::Host => r.host.push(addr),
            Kind::Reflexive => r.srflx.push(addr),
            Kind::Relay => r.relay.push(addr),
        }
    }
    r
}

/// A signalled offer's SDP with at most MAX_CANDIDATES UDP IPv4 unicast
/// candidate lines (N50): with no TCP connection, T4-A (only the TCP
/// peer's IP) cannot apply, so a hostile joiner can aim at most a few
/// checks at a third party. Other lines are kept as they are (str0m ends
/// a line only at `\n`).
pub fn sanitize(sdp: &str) -> String {
    sanitize_with(sdp, true)
}

/// [`sanitize`], and without `allow_local` no loopback candidate either
/// (final review A-M4: no honest remote peer offers one, and a hostile
/// one would aim checks at the machine's own 127.0.0.1 services).
pub fn sanitize_with(sdp: &str, allow_local: bool) -> String {
    let mut candidates = 0;
    sdp.split_inclusive('\n')
        .filter(|line| {
            if !line.starts_with("a=candidate:") {
                return true;
            }
            let keep = candidates < MAX_CANDIDATES
                && kept(line).is_some_and(|(addr, _)| allow_local || !addr.ip().is_loopback());
            candidates += usize::from(keep);
            keep
        })
        .collect()
}

/// The path of a selected candidate pair, our `local` address to the
/// peer's `remote` one (N49): `Relayed` when either is a TURN relay,
/// `MappedPort` when `remote` is the peer's mapped hint, `Direct` to one of
/// its host candidates, `HolePunched` otherwise (server- or
/// peer-reflexive).
pub fn classify(
    local: SocketAddr,
    remote: SocketAddr,
    ours: &LocalCandidates,
    theirs: &RemoteCandidates,
) -> Path {
    if ours.relayed.contains(&local) || theirs.relay.contains(&remote) {
        Path::Relayed
    } else if theirs.mapped == Some(remote) {
        Path::MappedPort
    } else if theirs.host.contains(&remote) {
        Path::Direct
    } else {
        Path::HolePunched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of N49, from either side's view.
    #[test]
    fn every_row_of_n49() {
        let a = |s: &str| -> SocketAddr { s.parse().unwrap() };
        let ours = LocalCandidates {
            base: Some(a("10.0.0.2:5000")),
            host: vec![a("10.0.0.2:5000")],
            relayed: vec![a("203.0.113.50:49170")],
            ..LocalCandidates::default()
        };
        let theirs = RemoteCandidates {
            host: vec![a("10.0.0.3:5000"), a("198.51.100.9:5000")],
            srflx: vec![a("198.51.100.9:6000")],
            relay: vec![a("203.0.113.9:49160")],
            mapped: Some(a("198.51.100.9:5000")),
        };
        let base = a("10.0.0.2:5000");
        // Relayed: either side's address is a relay, before anything else.
        assert_eq!(classify(a("203.0.113.50:49170"), a("10.0.0.3:5000"), &ours, &theirs), Path::Relayed);
        assert_eq!(classify(base, a("203.0.113.9:49160"), &ours, &theirs), Path::Relayed);
        // MappedPort: the remote's mapped hint, even when it is also a host candidate.
        assert_eq!(classify(base, a("198.51.100.9:5000"), &ours, &theirs), Path::MappedPort);
        // Direct: a remote host candidate.
        assert_eq!(classify(base, a("10.0.0.3:5000"), &ours, &theirs), Path::Direct);
        // HolePunched: server-reflexive, or peer-reflexive (in no list).
        assert_eq!(classify(base, a("198.51.100.9:6000"), &ours, &theirs), Path::HolePunched);
        assert_eq!(classify(base, a("192.0.2.1:7000"), &ours, &theirs), Path::HolePunched);
        // No hint: a mapped address is just reflexive.
        let no_hint = RemoteCandidates { mapped: None, host: vec![], ..theirs.clone() };
        assert_eq!(classify(base, a("198.51.100.9:5000"), &ours, &no_hint), Path::HolePunched);
    }

    #[test]
    fn candidate_lines_are_judged_field_by_field() {
        let ok = "a=candidate:1 1 UDP 2130706431 192.168.1.5 50000 typ host\r\n";
        assert_eq!(kept(ok).map(|k| k.0), Some("192.168.1.5:50000".parse().unwrap()));
        for bad in [
            "a=candidate:1 1 udp 2130706431 192.168.1.5 0 typ host\r\n",
            "a=candidate:1 1 udp 2130706431 255.255.255.255 5000 typ host\r\n",
            "a=candidate:1 1 udp 2130706431 192.168.1.5 5000 typ unknown\r\n",
            "a=candidate:1 1 udp 2130706431 192.168.1.5 5000 type host\r\n",
            "a=candidate:1 1 udp 2130706431  192.168.1.5 5000 typ host\r\n",
            "a=candidate:1 1 udp x 192.168.1.5 5000 typ host\r\n",
            "a=candidate:1 1 udp 2130706431 192.168.1.5 5000 typ host\rjunk\r\n",
            "a=candidate:1 1 udp 2130706431 192.168.1.5\r\n",
        ] {
            assert_eq!(kept(bad).map(|k| k.0), None, "{bad}");
        }
    }
}
