//! The machine's own addresses (N48, N51): the base of the ICE candidates
//! and the default gateway the port mapper asks. Asks the operating system
//! (`netdev`: a netlink dump, the routing table); sends nothing.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// At most this many host candidates (N48).
const MAX_HOSTS: usize = 2;

/// The default interface's IPv4 addresses (≤ 2) and gateway; else the OS's
/// source address toward `toward`; else 127.0.0.1.
pub fn base_addresses(toward: Option<SocketAddr>) -> (Vec<Ipv4Addr>, Option<Ipv4Addr> /* gateway */) {
    if let Some(found) = default_interface() {
        return found;
    }
    match toward.and_then(source_toward) {
        Some(ip) => (vec![ip], None),
        None => (vec![Ipv4Addr::LOCALHOST], None),
    }
}

/// The default interface's usable IPv4 addresses (≤ 2) and its gateway;
/// `None` when the OS names no default interface or it has no IPv4 address.
pub(crate) fn default_interface() -> Option<(Vec<Ipv4Addr>, Option<Ipv4Addr>)> {
    let iface = netdev::get_default_interface().ok()?;
    let ips: Vec<Ipv4Addr> =
        iface.ipv4_addrs().into_iter().filter(|ip| usable(*ip)).take(MAX_HOSTS).collect();
    if ips.is_empty() {
        return None;
    }
    let gateway = iface.gateway.and_then(|g| g.ipv4.into_iter().find(|ip| usable(*ip)));
    Some((ips, gateway))
}

/// The source address the OS routes from toward the Internet, at once
/// (no netdev, nothing sent: a `connect`ed UDP socket toward 192.0.2.1,
/// TEST-NET-1, which only picks the default route); 127.0.0.1 without
/// one. The endpoint's fallback when a signalled setup is made before the
/// slow `default_interface` answered.
pub(crate) fn quick_base() -> Ipv4Addr {
    source_toward((Ipv4Addr::new(192, 0, 2, 1), 9).into()).unwrap_or(Ipv4Addr::LOCALHOST)
}

/// A unicast address a peer could send to.
fn usable(ip: Ipv4Addr) -> bool {
    !(ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast())
}

/// The source address the OS would pick toward `toward` (IPv4): a UDP
/// socket `connect`ed to it, which sends nothing.
fn source_toward(toward: SocketAddr) -> Option<Ipv4Addr> {
    if !toward.is_ipv4() {
        return None;
    }
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(toward).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if usable(ip) => Some(ip),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_always_a_base() {
        let (ips, _) = base_addresses(None);
        assert!(!ips.is_empty() && ips.len() <= MAX_HOSTS, "{ips:?}");
        assert!(ips.iter().all(|ip| usable(*ip)), "{ips:?}");
    }

    /// The quick base is a usable address, found at once.
    #[test]
    fn a_quick_base_is_instant() {
        let t = std::time::Instant::now();
        let ip = quick_base();
        assert!(usable(ip), "{ip}");
        assert!(t.elapsed() < std::time::Duration::from_millis(100), "{:?}", t.elapsed());
    }

    /// Toward 127.0.0.1 the OS picks 127.0.0.1 (nothing is sent).
    #[test]
    fn the_source_toward_loopback_is_loopback() {
        assert_eq!(source_toward((Ipv4Addr::LOCALHOST, 9).into()), Some(Ipv4Addr::LOCALHOST));
        assert_eq!(source_toward("[::1]:9".parse().unwrap()), None);
    }
}
