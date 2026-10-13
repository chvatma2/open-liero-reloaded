//! The Nostr core: published events verify, ours round-trip, frames never panic.

use proptest::prelude::*;
use rl_discovery::*;
use rl_lobby::Identity;
use serde_json::Value;

/// Three kind-1 events a public relay served on 2026-10-13, unmodified
/// (fetched once by hand; no test touches the network): unicode and newlines,
/// escaped quotes in the content, and a leading space with a run of newlines.
const FIXTURE: &str = include_str!("fixtures/relay-events.json");

fn alice() -> Identity {
    Identity::from_secret([7; 32]).unwrap()
}

fn tags(t: &[&[&str]]) -> Vec<Vec<String>> {
    t.iter().map(|t| t.iter().map(|s| s.to_string()).collect()).collect()
}

fn sample() -> Event {
    Event::sign(
        &alice(),
        1_790_000_000,
        KIND_ANNOUNCE,
        tags(&[&["d", "rust-liero"], &["expiration", "1790000180"]]),
        "hello \"world\"\n".into(),
        [3; 32],
    )
}

#[test]
fn a_published_event_verifies() {
    let events: Vec<Value> = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(events.len(), 3);
    for v in events {
        // Through the relay-frame decoder, as it would arrive.
        let frame = serde_json::to_string(&serde_json::json!(["EVENT", "s", v])).unwrap();
        let FromRelay::Event { event, .. } = decode_from_relay(&frame).unwrap() else {
            panic!("not an event")
        };
        assert_eq!(hex::encode(event.id), v["id"].as_str().unwrap());
        assert!(event.verify(), "event {} does not verify", v["id"]);
        let mut bad = event.clone();
        bad.content.push('x');
        assert!(!bad.verify());
    }
}

#[test]
fn signing_round_trips() {
    let e = sample();
    assert!(e.verify());
    assert_eq!(e.pubkey, alice().public());
    assert_eq!(e.tag("d"), Some("rust-liero"));
    assert_eq!(e.tag("nope"), None);
    assert_eq!(e.expiration(), Some(1_790_000_180));
    let wire = encode_to_relay(&ToRelay::Event(e.clone()));
    assert_eq!(decode_to_relay(&wire), Ok(ToRelay::Event(e.clone())));
    let wire = encode_from_relay(&FromRelay::Event { sub: "s".into(), event: e.clone() });
    let FromRelay::Event { event, .. } = decode_from_relay(&wire).unwrap() else { panic!() };
    assert_eq!(event, e);
    assert!(event.verify());
}

#[test]
fn expiration_needs_a_number() {
    let mut e = sample();
    e.tags = tags(&[&["expiration", "soon"]]);
    assert_eq!(e.expiration(), None);
    e.tags = tags(&[&["expiration"]]);
    assert_eq!(e.expiration(), None);
    e.tags.clear();
    assert_eq!(e.expiration(), None);
}

#[test]
fn canonical_escapes_like_nip01() {
    let key = [0xab; 32];
    let s = canonical(
        &key,
        5,
        1,
        &tags(&[&["t", "a\"b"], &["e"], &[]]),
        "q\" b\\ n\n r\r t\t b\u{8} f\u{c} nul\u{0} del\u{7f} é €  😀 / <",
    );
    let expected = format!(
        "[0,\"{}\",5,1,[[\"t\",\"a\\\"b\"],[\"e\"],[]],\
         \"q\\\" b\\\\ n\\n r\\r t\\t b\\b f\\f nul\u{0} del\u{7f} é €  😀 / <\"]",
        "ab".repeat(32)
    );
    assert_eq!(s, expected);
}

#[test]
fn tampered_events_fail() {
    let e = sample();
    let mut x = e.clone();
    x.content.push('!');
    assert!(!x.verify());
    let mut x = e.clone();
    x.tags[0][1] = "other".into();
    assert!(!x.verify());
    let mut x = e.clone();
    x.created_at += 1;
    assert!(!x.verify());
    let mut x = e.clone();
    x.kind += 1;
    assert!(!x.verify());
    let mut x = e.clone();
    x.sig[10] ^= 1;
    assert!(!x.verify());
    let mut x = e.clone();
    x.id[0] ^= 1;
    assert!(!x.verify());
    let mut x = e.clone();
    x.pubkey = Identity::from_secret([8; 32]).unwrap().public();
    assert!(!x.verify());
    // An event re-id'd after tampering still lacks a valid signature.
    let mut x = e;
    x.content.push('!');
    x.id = Event::sign(&alice(), x.created_at, x.kind, x.tags.clone(), x.content.clone(), [0; 32]).id;
    assert!(!x.verify());
}

#[test]
fn frames_round_trip() {
    let e = sample();
    let req = ToRelay::Req {
        sub: "list".into(),
        filters: vec![
            Filter {
                kinds: vec![KIND_ANNOUNCE],
                authors: vec![e.pubkey],
                tags: vec![('t', vec!["rust-liero".into()])],
                since: Some(100),
                limit: Some(50),
            },
            Filter::default(),
        ],
    };
    for m in [
        ToRelay::Event(e.clone()),
        req,
        ToRelay::Req { sub: "x".into(), filters: vec![] },
        ToRelay::Close { sub: "list".into() },
    ] {
        assert_eq!(decode_to_relay(&encode_to_relay(&m)), Ok(m));
    }
    for m in [
        FromRelay::Event { sub: "s".into(), event: e.clone() },
        FromRelay::Eose { sub: "s".into() },
        FromRelay::Ok { id: e.id, accepted: true, message: String::new() },
        FromRelay::Ok { id: e.id, accepted: false, message: "blocked: no".into() },
        FromRelay::Notice("hi \"there\"".into()),
        FromRelay::Closed { sub: "s".into(), message: "auth-required: x".into() },
    ] {
        assert_eq!(decode_from_relay(&encode_from_relay(&m)), Ok(m));
    }
    // Wild frames: OK without a message, unknown filter keys, CLOSED without one.
    let id = hex::encode(e.id);
    assert_eq!(
        decode_from_relay(&format!("[\"OK\",\"{id}\",true]")),
        Ok(FromRelay::Ok { id: e.id, accepted: true, message: String::new() })
    );
    assert_eq!(
        decode_to_relay("[\"REQ\",\"a\",{\"until\":5,\"#d\":[\"x\"],\"#long\":[1]}]"),
        Ok(ToRelay::Req {
            sub: "a".into(),
            filters: vec![Filter { tags: vec![('d', vec!["x".into()])], ..Filter::default() }]
        })
    );
}

#[test]
fn bad_frames_are_errors() {
    assert_eq!(decode_from_relay("nope"), Err(NostrError::Json));
    assert_eq!(decode_from_relay("{}"), Err(NostrError::NotAFrame));
    assert_eq!(decode_from_relay("[]"), Err(NostrError::NotAFrame));
    assert_eq!(decode_from_relay("[1,\"x\"]"), Err(NostrError::NotAFrame));
    // Not a relay frame at all, as opposed to a frame we skip.
    for e in [NostrError::Json, NostrError::TooLarge, NostrError::NotAFrame] {
        assert!(e.is_frame_error(), "{e:?}");
    }
    for e in [NostrError::Unknown, NostrError::Malformed("event")] {
        assert!(!e.is_frame_error(), "{e:?}");
    }
    assert_eq!(decode_from_relay("[\"AUTH\",\"x\"]"), Err(NostrError::Unknown));
    assert_eq!(decode_to_relay("[\"REQ\"]"), Err(NostrError::Malformed("subscription")));
    let big = format!("[\"NOTICE\",\"{}\"]", "a".repeat(MAX_FRAME));
    assert_eq!(decode_from_relay(&big), Err(NostrError::TooLarge));
    let deep = format!("[\"NOTICE\",{}{}]", "[".repeat(100_000), "]".repeat(100_000));
    assert!(decode_from_relay(&deep).is_err());
    // A short id or a non-hex pubkey is refused.
    let e = sample();
    let wire = encode_from_relay(&FromRelay::Event { sub: "s".into(), event: e.clone() });
    let bad = wire.replace(&hex::encode(e.pubkey), "zz");
    assert!(decode_from_relay(&bad).is_err());
}

#[test]
fn out_of_range_numbers_are_errors() {
    let e = sample();
    let wire = encode_from_relay(&FromRelay::Event { sub: "s".into(), event: e.clone() });
    let kind = format!("\"kind\":{}", e.kind);
    let at = format!("\"created_at\":{}", e.created_at);
    assert!(wire.contains(&kind) && wire.contains(&at));
    for (from, to) in [
        (&kind, "\"kind\":4294967296"),
        (&kind, "\"kind\":-1"),
        (&at, "\"created_at\":18446744073709551616"),
        (&at, "\"created_at\":-5"),
        (&at, "\"created_at\":1.5"),
    ] {
        let bad = wire.replace(from.as_str(), to);
        assert!(decode_from_relay(&bad).is_err(), "{to}");
    }
    assert_eq!(decode_from_relay(&wire), Ok(FromRelay::Event { sub: "s".into(), event: e }));
}

fn mutate(s: &str, edits: &[(usize, u8)]) -> String {
    let mut b = s.as_bytes().to_vec();
    for &(i, v) in edits {
        let n = b.len();
        b[i % n] = v;
    }
    String::from_utf8_lossy(&b).into_owned()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_frames_never_panic(s in ".{0,300}", prefix in 0usize..5) {
        let p = ["", "[", "[\"EVENT\",", "[\"REQ\",\"a\",", "[\"OK\","][prefix];
        let text = format!("{p}{s}");
        let _ = decode_from_relay(&text);
        let _ = decode_to_relay(&text);
    }

    #[test]
    fn mutated_frames_never_panic(edits in prop::collection::vec((any::<usize>(), any::<u8>()), 1..8)) {
        let e = sample();
        let frames = [
            encode_from_relay(&FromRelay::Event { sub: "s".into(), event: e.clone() }),
            encode_to_relay(&ToRelay::Event(e.clone())),
            encode_to_relay(&ToRelay::Req { sub: "s".into(), filters: vec![Filter {
                kinds: vec![1], authors: vec![e.pubkey], tags: vec![('t', vec!["x".into()])],
                since: Some(1), limit: Some(1) }] }),
            encode_from_relay(&FromRelay::Ok { id: e.id, accepted: true, message: "m".into() }),
        ];
        for f in &frames {
            let m = mutate(f, &edits);
            if let Ok(FromRelay::Event { event, .. }) = decode_from_relay(&m) {
                let _ = event.verify();
            }
            if let Ok(ToRelay::Event(event)) = decode_to_relay(&m) {
                let _ = event.verify();
            }
        }
    }
}
