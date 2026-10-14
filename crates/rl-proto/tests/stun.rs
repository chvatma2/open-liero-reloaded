//! The STUN binding codec (RFC 5389): requests, responses, `is_stun`.

use std::net::SocketAddr;

use proptest::prelude::*;
use rl_proto::stun::{binding_request, binding_response, is_stun, parse_binding_response};

/// RFC 5769 §2.2: the sample IPv4 response (transaction b7e7a701bc34d686fa87dfae).
const RFC5769_IPV4: [u8; 80] = [
    0x01, 0x01, 0x00, 0x3c, 0x21, 0x12, 0xa4, 0x42, 0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa,
    0x87, 0xdf, 0xae, 0x80, 0x22, 0x00, 0x0b, 0x74, 0x65, 0x73, 0x74, 0x20, 0x76, 0x65, 0x63, 0x74, 0x6f,
    0x72, 0x20, 0x00, 0x20, 0x00, 0x08, 0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43, 0x00, 0x08, 0x00,
    0x14, 0x2b, 0x91, 0xf5, 0x99, 0xfd, 0x9e, 0x90, 0xc3, 0x8c, 0x74, 0x89, 0xf9, 0x2a, 0xf9, 0xba, 0x53,
    0xf0, 0x6b, 0xe7, 0xd7, 0x80, 0x28, 0x00, 0x04, 0xc0, 0x7d, 0x4c, 0x96,
];
const RFC_TXN: [u8; 12] = [0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae];

fn sa(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

#[test]
fn parses_a_real_response() {
    assert_eq!(parse_binding_response(&RFC5769_IPV4, &RFC_TXN), Some(sa("192.0.2.1:32853")));
    let req = binding_request(RFC_TXN);
    assert_eq!(req.len(), 20);
    assert_eq!(&req[..8], &[0, 1, 0, 0, 0x21, 0x12, 0xa4, 0x42]);
    assert_eq!(&req[8..], &RFC_TXN);
}

#[test]
fn mapped_address_only_without_xor() {
    let t = [7u8; 12];
    let plain = binding_response(&t, sa("203.0.113.9:4000"), false);
    assert_eq!(parse_binding_response(&plain, &t), Some(sa("203.0.113.9:4000")));
    let xor = binding_response(&t, sa("198.51.100.7:5000"), true);
    assert_eq!(parse_binding_response(&xor, &t), Some(sa("198.51.100.7:5000")));
    // Both present: the XOR one wins, whatever the order.
    let (mut a, b) = (plain.clone(), &xor[20..]);
    a.extend_from_slice(b);
    let body_len = (a.len() - 20) as u16;
    a[2..4].copy_from_slice(&body_len.to_be_bytes());
    assert_eq!(parse_binding_response(&a, &t), Some(sa("198.51.100.7:5000")));
    // IPv6 round-trips.
    let v6 = binding_response(&t, sa("[2001:db8::1]:9"), true);
    assert_eq!(parse_binding_response(&v6, &t), Some(sa("[2001:db8::1]:9")));
}

#[test]
fn refuses_wrong_transactions_and_garbage() {
    let mut other = RFC_TXN;
    other[0] ^= 1;
    assert_eq!(parse_binding_response(&RFC5769_IPV4, &other), None);
    assert_eq!(parse_binding_response(&[], &RFC_TXN), None);
    assert_eq!(parse_binding_response(&RFC5769_IPV4[..19], &RFC_TXN), None);
    assert_eq!(parse_binding_response(&RFC5769_IPV4[..60], &RFC_TXN), None);
    let mut bad = RFC5769_IPV4;
    bad[0] = 0x01;
    bad[1] = 0x11; // an indication, not a success
    assert_eq!(parse_binding_response(&bad, &RFC_TXN), None);
    let mut bad = RFC5769_IPV4;
    bad[4] ^= 1; // cookie
    assert_eq!(parse_binding_response(&bad, &RFC_TXN), None);
    let mut bad = RFC5769_IPV4;
    bad[41] = 0x03; // an unknown address family
    assert_eq!(parse_binding_response(&bad, &RFC_TXN), None);
    let mut bad = RFC5769_IPV4;
    bad[22] = 0xff; // an attribute longer than the message
    assert_eq!(parse_binding_response(&bad, &RFC_TXN), None);
    assert_eq!(parse_binding_response(&[0xff; 1000], &RFC_TXN), None);
}

/// STUN is told from DTLS, SCTP and the game's bytes by its first two bits,
/// the magic cookie and a length that matches.
#[test]
fn stun_messages_are_recognised() {
    assert!(is_stun(&RFC5769_IPV4));
    assert!(is_stun(&binding_request(RFC_TXN)));
    assert!(is_stun(&binding_response(&RFC_TXN, sa("203.0.113.9:4000"), true)));
    assert!(!is_stun(&[]));
    assert!(!is_stun(&RFC5769_IPV4[..19]));
    assert!(!is_stun(&RFC5769_IPV4[..76]), "the length no longer matches");
    let mut long = RFC5769_IPV4.to_vec();
    long.extend_from_slice(&[0; 4]);
    assert!(!is_stun(&long), "the length no longer matches");
    // A DTLS 1.2 handshake record: content type, version, epoch, sequence, length.
    let mut dtls = vec![0x16, 0xfe, 0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 40];
    dtls.extend_from_slice(&[0; 40]);
    assert!(!is_stun(&dtls));
    let mut top = RFC5769_IPV4;
    top[0] |= 0x40;
    assert!(!is_stun(&top), "the first two bits are 0");
    let mut cookie = RFC5769_IPV4;
    cookie[7] ^= 1;
    assert!(!is_stun(&cookie));
    let mut odd = binding_request(RFC_TXN);
    odd.push(0);
    odd[3] = 1; // a length that is no multiple of 4
    assert!(!is_stun(&odd));
}

proptest! {
    #[test]
    fn responses_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..700), flip in 0usize..80, val in any::<u8>()) {
        let _ = parse_binding_response(&bytes, &RFC_TXN);
        let _ = is_stun(&bytes);
        let mut v = RFC5769_IPV4;
        v[flip] = val;
        let _ = parse_binding_response(&v, &RFC_TXN);
        let _ = is_stun(&v);
        let mut w = RFC5769_IPV4.to_vec();
        w.extend_from_slice(&bytes);
        let _ = parse_binding_response(&w, &RFC_TXN);
    }
}
