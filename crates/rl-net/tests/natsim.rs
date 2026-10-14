//! Hole punching through simulated routers (N56), on a virtual clock.

use std::net::SocketAddr;
use std::time::Duration;

use rl_net::natsim::{NatKind, Net, NodeId};
use rl_net::probe::NatBehaviour;
use rl_proto::Path;

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

/// A host behind `host_nat` and a client behind `client_nat` (None: public), gathered and signalled.
fn join(host_nat: Option<NatKind>, client_nat: Option<NatKind>) -> (Net, NodeId, Option<Path>) {
    let mut net = Net::new(7);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let s2 = net.stun_server(a("198.51.100.2:3478"));
    let nh = host_nat.map(|k| net.nat(k, "203.0.113.10".parse().unwrap()));
    let nc = client_nat.map(|k| net.nat(k, "203.0.113.20".parse().unwrap()));
    let host = net.host(nh, if nh.is_some() { a("192.168.1.10:23400") } else { a("203.0.113.10:23400") });
    let client = net.client(nc, if nc.is_some() { a("192.168.7.20:50000") } else { a("203.0.113.20:50000") });
    net.gather(host, &[s1, s2], &[]);
    net.gather(client, &[s1, s2], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    // Past SIGNALLED_SETUP_TIMEOUT (15 s), so a failed setup has reported Disconnected.
    net.run_until(Duration::from_secs(20), |n| n.connected(client).is_some());
    let path = net.connected(client);
    (net, client, path)
}

#[test]
fn public_peers_connect_directly() {
    assert_eq!(join(None, None).2, Some(Path::Direct));
}

#[test]
fn cone_nats_hole_punch() {
    assert_eq!(join(Some(NatKind::FullCone), Some(NatKind::FullCone)).2, Some(Path::HolePunched));
}

#[test]
fn port_restricted_nats_hole_punch() {
    assert_eq!(join(Some(NatKind::PortRestricted), Some(NatKind::PortRestricted)).2, Some(Path::HolePunched));
}

#[test]
fn a_full_cone_host_takes_a_symmetric_client() {
    assert_eq!(join(Some(NatKind::FullCone), Some(NatKind::Symmetric)).2, Some(Path::HolePunched));
}

#[test]
fn symmetric_against_port_restricted_fails_without_a_relay() {
    let (mut net, client, path) = join(Some(NatKind::PortRestricted), Some(NatKind::Symmetric));
    assert_eq!(path, None);
    assert!(net.events(client).iter().any(|e| matches!(e, rl_net::endpoint::NetEvent::Disconnected { .. })));
}

#[test]
fn a_mapped_port_is_used_and_named() {
    let mut net = Net::new(7);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let nh = net.nat(NatKind::PortRestricted, "203.0.113.10".parse().unwrap());
    let nc = net.nat(NatKind::Symmetric, "203.0.113.20".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    net.forward(nh, 23400, a("192.168.1.10:23400"));
    net.core(host).set_mapped(Some(a("203.0.113.10:23400")));
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(host, &[s1], &[]);
    net.gather(client, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    assert!(net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some()));
    assert_eq!(net.connected(client), Some(Path::MappedPort));
}

#[test]
fn the_same_lan_connects_directly() {
    let mut net = Net::new(7);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let n = net.nat(NatKind::PortRestricted, "203.0.113.10".parse().unwrap());
    let host = net.host(Some(n), a("192.168.1.10:23400"));
    let client = net.client(Some(n), a("192.168.1.11:50000"));
    net.gather(host, &[s1], &[]);
    net.gather(client, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    assert!(net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some()));
    assert_eq!(net.connected(client), Some(Path::Direct));
}

#[test]
fn nat_behaviour_is_detected() {
    for (kind, expected) in [
        (None, NatBehaviour::Open),
        (Some(NatKind::PortRestricted), NatBehaviour::Preserving),
        (Some(NatKind::Symmetric), NatBehaviour::Symmetric),
    ] {
        let mut net = Net::new(3);
        let s1 = net.stun_server(a("198.51.100.1:3478"));
        let s2 = net.stun_server(a("198.51.100.2:3478"));
        let nat = kind.map(|k| net.nat(k, "203.0.113.20".parse().unwrap()));
        let at = if nat.is_some() { a("192.168.7.20:50000") } else { a("203.0.113.20:50000") };
        let c = net.client(nat, at);
        net.gather(c, &[s1, s2], &[]);
        assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(c)));
        assert_eq!(net.core(c).nat(), expected, "{kind:?}");
    }
}

#[test]
fn stun_keeps_the_mapping_alive() {
    let mut net = Net::new(5);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let nh = net.nat(NatKind::Symmetric, "203.0.113.10".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    net.gather(host, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host)));
    let before = net.core(host).local().srflx;
    assert!(before.is_some(), "STUN answered");
    // Past the NAT's 120 s idle lifetime: STUN_EVERY (25 s) keeps the mapping, so the
    // (seeded, random) symmetric port does not change.
    net.run_until(Duration::from_secs(130), |_| false);
    assert_eq!(net.core(host).local().srflx, before);
}

#[test]
fn a_changed_reflexive_address_is_offered() {
    let mut net = Net::new(5);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let nh = net.nat(NatKind::PortRestricted, "203.0.113.10".parse().unwrap());
    let nc = net.nat(NatKind::PortRestricted, "203.0.113.20".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    net.gather(host, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host)));
    net.renumber(nh, "203.0.113.11".parse().unwrap());
    // The next STUN round (at most STUN_EVERY later) learns the new address.
    assert!(net.run_until(Duration::from_secs(30), |n| {
        n.core(host)
            .local()
            .srflx
            .is_some_and(|s| s.ip() == "203.0.113.11".parse::<std::net::IpAddr>().unwrap())
    }));
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(client, &[s1], &[]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(client)));
    net.signal(client, host).expect("answered");
    assert!(net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some()));
    assert_eq!(net.connected(client), Some(Path::HolePunched));
}

fn join_relayed(host_nat: NatKind, client_nat: NatKind) -> (Net, NodeId, Option<Path>) {
    let mut net = Net::new(11);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let turn = net.turn_server(a("198.51.100.9:3478"));
    let nh = net.nat(host_nat, "203.0.113.10".parse().unwrap());
    let nc = net.nat(client_nat, "203.0.113.20".parse().unwrap());
    let host = net.host(Some(nh), a("192.168.1.10:23400"));
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(host, &[s1], &[]);
    net.gather(client, &[s1], &[turn]);
    assert!(net.run_until(Duration::from_secs(3), |n| n.gathered(host) && n.gathered(client)));
    net.signal(client, host).expect("answered");
    net.run_until(Duration::from_secs(15), |n| n.connected(client).is_some());
    let path = net.connected(client);
    (net, client, path)
}

#[test]
fn symmetric_against_port_restricted_goes_through_the_relay() {
    assert_eq!(join_relayed(NatKind::PortRestricted, NatKind::Symmetric).2, Some(Path::Relayed));
}

#[test]
fn symmetric_against_symmetric_goes_through_the_relay() {
    assert_eq!(join_relayed(NatKind::Symmetric, NatKind::Symmetric).2, Some(Path::Relayed));
}

#[test]
fn a_hole_punched_link_releases_its_allocation() {
    let (mut net, client, path) = join_relayed(NatKind::PortRestricted, NatKind::PortRestricted);
    assert_eq!(path, Some(Path::HolePunched));
    net.run_until(Duration::from_secs(6), |_| false);
    assert!(!net.core(client).relays().is_empty(), "the client allocated");
    assert!(net.core(client).relays().iter().all(|(_, s)| *s == rl_net::relayed::RelayState::Released));
}

#[test]
fn a_dead_relay_ends_the_link() {
    let (mut net, client, path) = join_relayed(NatKind::PortRestricted, NatKind::Symmetric);
    assert_eq!(path, Some(Path::Relayed));
    net.kill(a("198.51.100.9:3478"));
    assert!(net.run_until(Duration::from_secs(30), |n| {
        n.events(client).iter().any(|e| matches!(e, rl_net::endpoint::NetEvent::Disconnected { .. }))
    }));
}

#[test]
fn gathering_waits_for_a_silent_relay_at_most_turn_wait() {
    let mut net = Net::new(11);
    let s1 = net.stun_server(a("198.51.100.1:3478"));
    let turn = net.turn_server(a("198.51.100.9:3478"));
    net.kill(turn.addr);
    let nc = net.nat(NatKind::PortRestricted, "203.0.113.20".parse().unwrap());
    let client = net.client(Some(nc), a("192.168.7.20:50000"));
    net.gather(client, &[s1], &[turn]);
    // STUN answers within STUN_WAIT; the relay never does.
    net.run_until(rl_net::probe::STUN_WAIT, |_| false);
    assert!(!net.gathered(client), "waiting for the relay");
    assert!(net.run_until(rl_net::relayed::TURN_WAIT, |n| n.gathered(client)));
    assert_eq!(net.core(client).relays()[0].1, rl_net::relayed::RelayState::Allocating);
    assert!(net.core(client).local().relayed.is_empty());
}
