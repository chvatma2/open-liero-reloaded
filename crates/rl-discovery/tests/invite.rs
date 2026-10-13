//! Invite codes (N42): round trips, damage, versions, fuzz.

use std::net::{Ipv4Addr, SocketAddr};

use proptest::prelude::*;
use rl_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
use rl_discovery::{Invite, InviteError, RelayHint};

fn sample(addr: bool, relays: Vec<RelayHint>) -> Invite {
    Invite {
        key: [7; 32],
        relays,
        addr: addr.then(|| SocketAddr::from((Ipv4Addr::new(203, 0, 113, 9), 7777))),
    }
}

#[test]
fn invites_round_trip() {
    let cases = [
        sample(false, vec![]),
        sample(true, vec![]),
        sample(true, vec![RelayHint::Default(0), RelayHint::Default(2)]),
        sample(false, vec![RelayHint::Url("wss://relay.example.org".into())]),
        sample(true, vec![RelayHint::Default(1), RelayHint::Url("ws://10.0.0.1:7777".into())]),
    ];
    for inv in cases {
        let text = inv.encode();
        assert!(text.starts_with("rlx1"), "{text}");
        assert!(text.len() <= 200, "{} chars", text.len());
        assert_eq!(Invite::decode(&text).unwrap(), inv, "{text}");
    }
}

#[test]
fn relay_urls_resolve_defaults() {
    let inv = sample(false, vec![RelayHint::Default(1), RelayHint::Url("wss://x.example".into())]);
    assert_eq!(inv.relay_urls(), vec![DEFAULT_RELAYS[1].to_string(), "wss://x.example".to_string()]);
    assert!(!DEFAULT_STUN.is_empty());
}

#[test]
fn damaged_codes_are_refused_clearly() {
    let good = sample(true, vec![RelayHint::Default(0), RelayHint::Default(1)]).encode();
    // Every single-character change after the prefix is damage.
    let alphabet = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
    for i in 4..good.len() {
        for &c in alphabet {
            if c == good.as_bytes()[i] {
                continue;
            }
            let mut b = good.clone().into_bytes();
            b[i] = c;
            let s = String::from_utf8(b).unwrap();
            assert!(matches!(Invite::decode(&s), Err(InviteError::Damaged)), "{s}");
        }
    }
    // Truncation.
    for n in 4..good.len() {
        assert!(matches!(Invite::decode(&good[..n]), Err(InviteError::Damaged)), "len {n}");
    }
    // Whitespace is trimmed, upper case accepted, mixed case refused.
    let inv = Invite::decode(&good).unwrap();
    assert_eq!(Invite::decode(&format!("  \n{good}\t ")).unwrap(), inv);
    assert_eq!(Invite::decode(&good.to_uppercase()).unwrap(), inv);
    let mut mixed = good.clone().into_bytes();
    mixed[6] = mixed[6].to_ascii_uppercase();
    if mixed[6] == good.as_bytes()[6] {
        mixed[7] = mixed[7].to_ascii_uppercase();
    }
    let mixed = String::from_utf8(mixed).unwrap();
    assert_ne!(mixed, good);
    assert_ne!(mixed, good.to_uppercase());
    assert!(matches!(Invite::decode(&mixed), Err(InviteError::Damaged)));
}

#[test]
fn other_text_is_not_an_invite() {
    for s in
        ["", "hello", "https://example.org", "npub1qqqq", "rlx", "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"]
    {
        assert!(matches!(Invite::decode(s), Err(InviteError::NotInvite)), "{s}");
    }
    assert_eq!(InviteError::NotInvite.to_string(), "this is not an invite code");
    assert_eq!(InviteError::Damaged.to_string(), "this invite code is damaged");
    assert_eq!(InviteError::Newer.to_string(), "this invite code is from a newer version of the game");
    assert_eq!(InviteError::Invalid.to_string(), "this invite code is invalid");
}

fn code(payload: &[u8]) -> String {
    use bech32::{Bech32m, Hrp};
    bech32::encode::<Bech32m>(Hrp::parse("rlx").unwrap(), payload).unwrap()
}

#[test]
fn newer_versions_are_named() {
    // Version 2 with a payload we could not parse as version 1.
    assert!(matches!(Invite::decode(&code(&[2, 1, 2, 3])), Err(InviteError::Newer)));
    assert!(matches!(Invite::decode(&code(&[255])), Err(InviteError::Newer)));
    // Version 0 and nothing at all are invalid.
    assert!(matches!(Invite::decode(&code(&[0; 40])), Err(InviteError::Invalid)));
    assert!(matches!(Invite::decode(&code(&[])), Err(InviteError::Invalid)));
    // Version 1, truncated or with trailing bytes.
    let mut p = vec![1u8];
    p.extend_from_slice(&[7; 32]);
    assert!(matches!(Invite::decode(&code(&p)), Err(InviteError::Invalid)));
    p.extend_from_slice(&[0, 0]); // no relays, no address
    assert!(Invite::decode(&code(&p)).is_ok());
    p.push(0);
    assert!(matches!(Invite::decode(&code(&p)), Err(InviteError::Invalid)));
}

/// A version-1 payload with the given raw relay bytes (postcard: count, then
/// per hint the variant, and for a URL its length and bytes), no address.
fn raw(relays: &[u8]) -> String {
    let mut p = vec![1u8];
    p.extend_from_slice(&[7; 32]);
    p.extend_from_slice(relays);
    p.push(0);
    code(&p)
}

fn url_bytes(u: &str) -> Vec<u8> {
    let mut v = vec![1, u.len() as u8];
    v.extend_from_slice(u.as_bytes());
    v
}

#[test]
fn hints_are_validated() {
    let two_urls = [vec![2], url_bytes("wss://a"), url_bytes("wss://b")].concat();
    let long = [vec![1], url_bytes(&format!("wss://{}", "a".repeat(59)))].concat(); // 65 bytes
    let bad: Vec<Vec<u8>> = vec![
        vec![4, 0, 0, 0, 1, 0, 2],
        [vec![1], url_bytes("https://relay.example")].concat(),
        long,
        two_urls,
    ];
    for r in bad {
        assert!(matches!(Invite::decode(&raw(&r)), Err(InviteError::Invalid)), "{r:?}");
    }
    let ok64 = [vec![1], url_bytes(&format!("wss://{}", "a".repeat(58)))].concat();
    assert!(Invite::decode(&raw(&ok64)).is_ok());
    assert!(Invite::decode(&raw(&[vec![4, 0, 0, 0, 1, 0, 1, 0, 2], url_bytes("ws://a")].concat())).is_err()); // 4 defaults
    assert!(Invite::decode(&raw(&[vec![4, 0, 0, 0, 1, 0, 2], url_bytes("ws://a")].concat())).is_ok());
}

#[test]
fn the_hrp_is_exactly_rlx() {
    // `rlx1foo1…`: bech32's separator is the last `1`, so its HRP is
    // `rlx1foo`; a valid checksum for that HRP is still not an invite.
    let mut p = vec![1u8];
    p.extend_from_slice(&[7; 32]);
    p.extend_from_slice(&[0, 0]);
    use bech32::{Bech32m, Hrp};
    let other = bech32::encode::<Bech32m>(Hrp::parse("rlx1foo").unwrap(), &p).unwrap();
    assert!(other.starts_with("rlx1foo1"), "{other}");
    assert!(matches!(Invite::decode(&other), Err(InviteError::Damaged)), "{other}");
    assert!(matches!(Invite::decode(&other.to_uppercase()), Err(InviteError::Damaged)));
    assert!(Invite::decode(&code(&p)).is_ok());
}

#[test]
fn a_plain_bech32_checksum_is_damage() {
    let mut p = vec![1u8];
    p.extend_from_slice(&[7; 32]);
    p.extend_from_slice(&[0, 0]);
    use bech32::{Bech32, Hrp};
    let plain = bech32::encode::<Bech32>(Hrp::parse("rlx").unwrap(), &p).unwrap();
    assert!(matches!(Invite::decode(&plain), Err(InviteError::Damaged)), "{plain}");
}

#[test]
fn unknown_default_indices_are_ignored() {
    // A newer build's longer list: an index beyond ours is skipped, so the
    // code's other hints still serve.
    let n = DEFAULT_RELAYS.len() as u8;
    let got = Invite::decode(&raw(&[vec![4, 0, n, 0, 255, 0, 1], url_bytes("wss://a")].concat())).unwrap();
    assert_eq!(got.relays, vec![RelayHint::Default(1), RelayHint::Url("wss://a".into())]);
    assert_eq!(got.relay_urls(), vec![DEFAULT_RELAYS[1].to_string(), "wss://a".to_string()]);
    // They still count towards the limits.
    let four = [vec![4, 0, n, 0, n, 0, n, 0, 1]].concat();
    assert!(matches!(Invite::decode(&raw(&four)), Err(InviteError::Invalid)));
}

#[test]
fn the_worst_case_fits() {
    let inv = sample(
        true,
        vec![
            RelayHint::Default(0),
            RelayHint::Default(1),
            RelayHint::Default(2),
            RelayHint::Url(format!("wss://{}", "a".repeat(58))),
        ],
    );
    let text = inv.encode();
    assert!(text.len() <= 200, "{} chars", text.len());
    assert_eq!(Invite::decode(&text).unwrap(), inv);
}

#[test]
fn encode_keeps_hints_within_limits() {
    let n = DEFAULT_RELAYS.len() as u8;
    let inv = sample(
        true,
        vec![
            RelayHint::Url("https://bad".into()),
            RelayHint::Default(n),
            RelayHint::Url(format!("wss://{}", "a".repeat(99))),
            RelayHint::Default(0),
            RelayHint::Url("wss://first.example".into()),
            RelayHint::Url("wss://second.example".into()),
            RelayHint::Default(1),
            RelayHint::Default(2),
            RelayHint::Default(3),
        ],
    );
    let got = Invite::decode(&inv.encode()).unwrap();
    assert_eq!(
        got.relays,
        vec![
            RelayHint::Default(0),
            RelayHint::Url("wss://first.example".into()),
            RelayHint::Default(1),
            RelayHint::Default(2)
        ]
    );
}

#[test]
fn decode_refuses_over_200_characters() {
    let mut p = vec![1u8];
    p.extend_from_slice(&[7; 32]);
    p.push(60);
    p.extend_from_slice(&[0, 0].repeat(60));
    p.push(0);
    let text = code(&p);
    assert!(text.len() > 200);
    assert!(matches!(Invite::decode(&text), Err(InviteError::Invalid)));
}

proptest! {
    #[test]
    fn encode_always_decodes(
        hints in proptest::collection::vec(
            prop_oneof![any::<u8>().prop_map(RelayHint::Default), ".{0,120}".prop_map(RelayHint::Url)], 0..8),
        addr in any::<bool>(),
    ) {
        let inv = sample(addr, hints);
        let text = inv.encode();
        prop_assert!(text.len() <= 200);
        prop_assert!(Invite::decode(&text).is_ok());
    }

    #[test]
    fn arbitrary_text_never_panics(s in ".{0,300}", bytes in proptest::collection::vec(any::<u8>(), 0..150)) {
        let _ = Invite::decode(&s);
        let _ = Invite::decode(&format!("rlx1{s}"));
        let _ = Invite::decode(&code(&bytes));
        let mut v = vec![1u8];
        v.extend_from_slice(&bytes);
        let _ = Invite::decode(&code(&v));
    }
}
