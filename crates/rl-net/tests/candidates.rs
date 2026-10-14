use std::net::SocketAddr;

use rl_net::candidates::{
    LocalCandidates, MAX_CANDIDATES, RemoteCandidates, classify, remote_candidates, sanitize,
};
use rl_proto::Path;

fn a(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

const SDP: &str = "v=0\r\na=ice-ufrag:x\r\n\
    a=candidate:1 1 udp 2130706431 192.168.1.5 50000 typ host\r\n\
    a=candidate:2 1 udp 1694498815 198.51.100.7 61000 typ srflx raddr 0.0.0.0 rport 0\r\n\
    a=candidate:3 1 udp 16777215 203.0.113.9 49160 typ relay raddr 0.0.0.0 rport 0\r\n\
    a=candidate:4 1 tcp 2130706431 192.168.1.5 9 typ host tcptype active\r\n\
    a=candidate:5 1 udp 2130706431 x.local 50000 typ host\r\n\
    a=candidate:6 1 udp 2130706431 0.0.0.0 50000 typ host\r\n\
    a=candidate:7 1 udp 2130706431 224.0.0.1 50000 typ host\r\n\
    a=candidate:8 1 udp 2130706431 fe80::1 50000 typ host\r\n";

#[test]
fn remote_candidates_are_read_by_type() {
    let r = remote_candidates(SDP, Some(a("198.51.100.7:23400")));
    assert_eq!(r.host, vec![a("192.168.1.5:50000")]);
    assert_eq!(r.srflx, vec![a("198.51.100.7:61000")]);
    assert_eq!(r.relay, vec![a("203.0.113.9:49160")]);
    assert_eq!(r.mapped, Some(a("198.51.100.7:23400")));
}

#[test]
fn signalled_offers_keep_at_most_eight_unicast_candidates() {
    let kept = sanitize(SDP);
    assert_eq!(kept.matches("a=candidate:").count(), 3, "{kept}");
    let mut many = String::from("v=0\r\n");
    for i in 0..100 {
        many.push_str(&format!(
            "a=candidate:{i} 1 udp 2130706431 10.0.{}.{} 5000 typ host\r\n",
            i / 200,
            i % 200 + 1
        ));
    }
    assert_eq!(sanitize(&many).matches("a=candidate:").count(), MAX_CANDIDATES);
    assert!(sanitize(&many).starts_with("v=0\r\n"));
}

#[test]
fn paths_are_classified() {
    let ours = LocalCandidates {
        base: Some(a("192.168.7.20:50000")),
        host: vec![a("192.168.7.20:50000")],
        relayed: vec![a("203.0.113.50:49170")],
        ..LocalCandidates::default()
    };
    let theirs = RemoteCandidates {
        host: vec![a("192.168.1.5:23400")],
        srflx: vec![a("198.51.100.7:23400")],
        relay: vec![a("203.0.113.9:49160")],
        mapped: Some(a("198.51.100.7:23401")),
    };
    let base = a("192.168.7.20:50000");
    assert_eq!(classify(base, a("192.168.1.5:23400"), &ours, &theirs), Path::Direct);
    assert_eq!(classify(base, a("198.51.100.7:23400"), &ours, &theirs), Path::HolePunched);
    assert_eq!(classify(base, a("198.51.100.7:40000"), &ours, &theirs), Path::HolePunched); // peer-reflexive
    assert_eq!(classify(base, a("198.51.100.7:23401"), &ours, &theirs), Path::MappedPort);
    assert_eq!(classify(base, a("203.0.113.9:49160"), &ours, &theirs), Path::Relayed);
    assert_eq!(classify(a("203.0.113.50:49170"), a("198.51.100.7:23400"), &ours, &theirs), Path::Relayed);
}

/// What `remote_candidates` reads of `sdp`, every address in one list.
fn read(sdp: &str) -> Vec<SocketAddr> {
    let r = remote_candidates(sdp, None);
    r.host.into_iter().chain(r.srflx).chain(r.relay).collect()
}

/// `SDP` with candidate line `line`'s space-separated field `field` replaced.
fn mutate(line: usize, field: usize, with: &str) -> String {
    let mut n = 0;
    SDP.split_inclusive('\n')
        .map(|l| match l.strip_prefix("a=candidate:") {
            Some(rest) => {
                n += 1;
                if n - 1 != line % 8 {
                    return l.to_owned();
                }
                let mut fields: Vec<&str> = rest.trim_end().split(' ').collect();
                let i = field % (fields.len() + 1);
                if i == fields.len() {
                    fields.push(with);
                } else {
                    fields[i] = with;
                }
                format!("a=candidate:{}\r\n", fields.join(" "))
            }
            None => l.to_owned(),
        })
        .collect()
}

fn check(sdp: &str) -> Result<(), proptest::test_runner::TestCaseError> {
    let kept = sanitize(sdp);
    let lines = kept.split('\n').filter(|l| l.starts_with("a=candidate:")).count();
    prop_assert!(lines <= MAX_CANDIDATES, "{kept}");
    prop_assert_eq!(sanitize(&kept), kept.clone());
    // Only what `sanitize` keeps is read, the same from the kept SDP.
    let read_all = read(sdp);
    prop_assert_eq!(&read_all, &read(&kept));
    for a in read_all {
        let SocketAddr::V4(v4) = a else { return Err(proptest::test_runner::TestCaseError::fail("IPv6")) };
        let ip = v4.ip();
        prop_assert!(!ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast(), "{a}");
    }
    Ok(())
}

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn sanitize_and_remote_candidates_never_panic(
        junk in any::<String>(),
        printable in "[ -~\r\n]{0,400}",
        line in 0usize..8,
        field in 0usize..12,
        with in "[a-z0-9.:\\- \r\n]{0,16}",
        extra in prop::collection::vec("a=candidate:[0-9] 1 (udp|UDP|tcp) [0-9]{1,10} [0-9.]{1,15} [0-9]{1,5} typ (host|srflx|prflx|relay|x)( raddr 0.0.0.0 rport 0)?\r?\n", 0..20),
    ) {
        check(&junk)?;
        check(&printable)?;
        check(&mutate(line, field, &with))?;
        let mut many = mutate(line, field, &with);
        many.extend(extra);
        check(&many)?;
    }
}
