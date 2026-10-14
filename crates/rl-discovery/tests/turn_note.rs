//! TURN relay notes (N52, N53): building and reading them, the relay list,
//! its choice, its bounds and closing notes.

use std::net::Ipv4Addr;

use proptest::prelude::*;
use rl_discovery::announce::{ANNOUNCE_TTL, CLOSE_TTL};
use rl_discovery::turn_note::{KIND_TURN, TURN_TAG, TurnList, TurnNote, parse_turn_note, turn_note};
use rl_discovery::{AnnounceError, Event, Region};
use rl_lobby::Identity;

const NOW: u64 = 1_800_000_000;

fn key(i: u32) -> Identity {
    let mut s = [0u8; 32];
    s[28..].copy_from_slice(&(i + 1).to_be_bytes());
    Identity::from_secret(s).unwrap()
}

fn relay(region: Region, load: u16, capacity: u16) -> TurnNote {
    TurnNote {
        v: 1,
        addr: Ipv4Addr::new(198, 20, 0, 10),
        port: 3478,
        region,
        capacity,
        load,
        user: "rust-liero".into(),
        pass: "open sesame".into(),
    }
}

fn note(i: u32, n: &TurnNote, at: u64) -> Event {
    turn_note(&key(i), n, at, ANNOUNCE_TTL, [i as u8; 32])
}

#[test]
fn turn_notes_round_trip() {
    let n = relay(Region::Europe, 3, 32);
    let e = note(1, &n, NOW);
    assert!(e.verify());
    assert_eq!(e.kind, KIND_TURN);
    assert_eq!(KIND_TURN, 31471);
    assert_eq!(e.tag("d"), Some(TURN_TAG));
    assert_eq!(e.tag("t"), Some(TURN_TAG));
    assert_eq!(e.expiration(), Some(NOW + ANNOUNCE_TTL));
    assert_eq!(parse_turn_note(&e), Ok(n.clone()));
    let mut list = TurnList::new();
    assert!(list.offer(&e, NOW));
    assert_eq!(list.len(), 1);
    let picks = list.pick(Region::Europe, 2);
    assert_eq!(picks.len(), 1);
    assert_eq!(picks[0].key, key(1).public());
    assert_eq!(picks[0].addr, "198.20.0.10:3478".parse().unwrap());
    assert_eq!((picks[0].user.as_str(), picks[0].pass.as_str()), ("rust-liero", "open sesame"));
    assert!(TurnList::filter(NOW).matches(&e));
    assert_eq!(TurnList::filter(NOW).since, Some(NOW - ANNOUNCE_TTL));
}

#[test]
fn invalid_turn_notes_are_refused() {
    let good = relay(Region::Asia, 0, 32);
    let cases: [(&str, TurnNote, AnnounceError); 7] = [
        ("port", TurnNote { port: 0, ..good.clone() }, AnnounceError::Invalid("port")),
        ("empty user", TurnNote { user: String::new(), ..good.clone() }, AnnounceError::Invalid("user")),
        ("long user", TurnNote { user: "u".repeat(65), ..good.clone() }, AnnounceError::Invalid("user")),
        ("control in pass", TurnNote { pass: "a\nb".into(), ..good.clone() }, AnnounceError::Invalid("pass")),
        (
            "non-ASCII pass",
            TurnNote { pass: "heslo✓".into(), ..good.clone() },
            AnnounceError::Invalid("pass"),
        ),
        ("capacity", TurnNote { capacity: 4097, ..good.clone() }, AnnounceError::Invalid("capacity")),
        ("addr", TurnNote { addr: Ipv4Addr::UNSPECIFIED, ..good.clone() }, AnnounceError::Invalid("addr")),
    ];
    for (what, n, err) in cases {
        assert_eq!(parse_turn_note(&note(1, &n, NOW)), Err(err), "{what}");
    }
    let max = TurnNote { user: "u".repeat(64), pass: "~".repeat(64), capacity: 4096, ..good.clone() };
    assert_eq!(parse_turn_note(&note(1, &max, NOW)), Ok(max));
    // Wrong kind, wrong tag, not JSON.
    let mut e = note(1, &good, NOW);
    e.kind = 31470;
    assert_eq!(parse_turn_note(&e), Err(AnnounceError::WrongKind));
    let wrong_d = Event::sign(
        &key(1),
        NOW,
        KIND_TURN,
        vec![vec!["d".into(), "rust-liero".into()]],
        "{}".into(),
        [0; 32],
    );
    assert_eq!(parse_turn_note(&wrong_d), Err(AnnounceError::Tag("d")));
    let both = vec![vec!["d".to_owned(), TURN_TAG.to_owned()], vec!["t".to_owned(), TURN_TAG.to_owned()]];
    let junk = Event::sign(&key(1), NOW, KIND_TURN, both, "junk".into(), [0; 32]);
    assert_eq!(parse_turn_note(&junk), Err(AnnounceError::Json));
    // Without the `t` tag; another format version.
    let content = serde_json::to_string(&good).unwrap();
    let no_t =
        Event::sign(&key(1), NOW, KIND_TURN, vec![vec!["d".into(), TURN_TAG.into()]], content, [0; 32]);
    assert_eq!(parse_turn_note(&no_t), Err(AnnounceError::Tag("t")));
    let v2 = TurnNote { v: 2, ..good.clone() };
    assert_eq!(parse_turn_note(&note(1, &v2, NOW)), Err(AnnounceError::Invalid("v")));
}

#[test]
fn a_flood_of_closing_notes_keeps_a_closed_relays_tombstone() {
    let mut list = TurnList::new();
    assert!(list.offer(&note(0, &relay(Region::Europe, 4, 32), NOW), NOW));
    let closing = turn_note(&key(0), &relay(Region::Europe, 0, 0), NOW + 2, CLOSE_TTL, [1; 32]);
    assert!(list.offer(&closing, NOW + 2));
    // Signed closing notes from fresh keys, never listed, fill the
    // remembered keys past their bound.
    for i in 1..=4_200 {
        let c = turn_note(&key(i), &relay(Region::Europe, 0, 0), NOW + 3, CLOSE_TTL, [2; 32]);
        list.offer(&c, NOW + 3);
    }
    // The closed relay's older open note, from a relay that missed the close.
    assert!(!list.offer(&note(0, &relay(Region::Europe, 4, 32), NOW + 1), NOW + 20));
    assert!(list.is_empty());
}

#[test]
fn turn_notes_with_private_addresses_need_allow_local() {
    let mut list = TurnList::new();
    for (i, ip) in [[127, 0, 0, 1], [192, 168, 1, 2], [10, 0, 0, 1], [100, 64, 0, 1], [203, 0, 113, 10]]
        .into_iter()
        .enumerate()
    {
        let n = TurnNote { addr: Ipv4Addr::from(ip), ..relay(Region::Europe, 0, 32) };
        assert!(!list.offer(&note(i as u32, &n, NOW), NOW), "{ip:?}");
    }
    assert_eq!(list.len(), 0);
    let mut local = TurnList::new();
    local.allow_local(true);
    let n = TurnNote { addr: Ipv4Addr::LOCALHOST, ..relay(Region::Europe, 0, 32) };
    assert!(local.offer(&note(1, &n, NOW), NOW));
    assert_eq!(local.pick(Region::Europe, 1)[0].addr, "127.0.0.1:3478".parse().unwrap());
}

#[test]
fn pick_prefers_the_region_then_the_least_loaded_and_skips_full_relays() {
    let mut list = TurnList::new();
    let notes = [
        relay(Region::Europe, 10, 32),
        relay(Region::Europe, 30, 32),
        relay(Region::Asia, 0, 32),
        relay(Region::Europe, 32, 32),
    ];
    for (i, n) in notes.iter().enumerate() {
        assert!(list.offer(&note(i as u32, n, NOW), NOW));
    }
    let keys =
        |picks: Vec<rl_discovery::turn_note::TurnPick>| picks.into_iter().map(|p| p.key).collect::<Vec<_>>();
    assert_eq!(keys(list.pick(Region::Europe, 3)), vec![key(0).public(), key(1).public(), key(2).public()]);
    assert_eq!(keys(list.pick(Region::Europe, 10)).len(), 3, "the full relay never");
    assert_eq!(keys(list.pick(Region::Asia, 2)), vec![key(2).public(), key(0).public()]);
    assert!(list.pick(Region::Europe, 0).is_empty());
    // Load is compared as a share of capacity: 5/8 is fuller than 10/32.
    let mut list = TurnList::new();
    list.offer(&note(0, &relay(Region::Africa, 5, 8), NOW), NOW);
    list.offer(&note(1, &relay(Region::Africa, 10, 32), NOW), NOW);
    assert_eq!(keys(list.pick(Region::Africa, 2)), vec![key(1).public(), key(0).public()]);
}

#[test]
fn a_flood_of_turn_notes_is_bounded() {
    let mut list = TurnList::new();
    for i in 0..1_000 {
        list.offer(&note(i, &relay(Region::Europe, 0, 32), NOW), NOW);
    }
    assert_eq!(list.len(), 64);
    // One key: one note per 10 s, by arrival or by date.
    let mut list = TurnList::new();
    assert!(list.offer(&note(1, &relay(Region::Europe, 1, 32), NOW), NOW));
    assert!(!list.offer(&note(1, &relay(Region::Europe, 2, 32), NOW + 1), NOW + 1));
    assert!(!list.offer(&note(1, &relay(Region::Europe, 3, 32), NOW + 9), NOW + 9));
    assert!(list.offer(&note(1, &relay(Region::Europe, 4, 32), NOW + 10), NOW + 10));
    // Newest per key: an older note is refused.
    assert!(!list.offer(&note(1, &relay(Region::Europe, 5, 32), NOW - 5), NOW + 30));
    // Too old, in the future, expired.
    assert!(!list.offer(&note(2, &relay(Region::Europe, 0, 32), NOW - ANNOUNCE_TTL - 1), NOW));
    assert!(!list.offer(&note(3, &relay(Region::Europe, 0, 32), NOW + 61), NOW));
    let short = turn_note(&key(4), &relay(Region::Europe, 0, 32), NOW - 20, 10, [0; 32]);
    assert!(!list.offer(&short, NOW));
    // A forged note.
    let mut forged = note(5, &relay(Region::Europe, 0, 32), NOW);
    forged.content = forged.content.replace("3478", "3479");
    assert!(!list.offer(&forged, NOW));
    assert_eq!(list.len(), 1);
    // Notes expire.
    list.expire(NOW + 10 + ANNOUNCE_TTL + 1);
    assert_eq!(list.len(), 0);
}

#[test]
fn a_closing_turn_note_removes_the_relay_at_once() {
    let mut list = TurnList::new();
    assert!(list.offer(&note(1, &relay(Region::Europe, 4, 32), NOW), NOW));
    assert!(list.offer(&note(2, &relay(Region::Europe, 8, 32), NOW), NOW));
    let closing = turn_note(&key(1), &relay(Region::Europe, 0, 0), NOW + 2, CLOSE_TTL, [1; 32]);
    assert_eq!(closing.expiration(), Some(NOW + 2 + CLOSE_TTL));
    assert!(list.offer(&closing, NOW + 2), "accepted despite the gap");
    assert_eq!(list.len(), 1);
    assert!(list.pick(Region::Europe, 2).iter().all(|p| p.key != key(1).public()));
    // A late open note from a relay that missed the close is older: refused.
    assert!(!list.offer(&note(1, &relay(Region::Europe, 4, 32), NOW + 1), NOW + 20));
    assert_eq!(list.len(), 1);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_turn_notes_never_panic(
        content in ".{0,300}",
        tags in prop::collection::vec(prop::collection::vec("[a-z0-9-]{0,20}", 0..3), 0..4),
        edits in prop::collection::vec((any::<usize>(), any::<u8>()), 0..6),
        at in (NOW - 300)..(NOW + 300),
    ) {
        let mut list = TurnList::new();
        list.allow_local(true);
        let raw = Event::sign(&key(1), at, KIND_TURN, tags, content, [0; 32]);
        let _ = parse_turn_note(&raw);
        let _ = list.offer(&raw, NOW);
        // A real note's JSON with byte edits, signed as it stands.
        let mut json = note(2, &relay(Region::Oceania, 1, 2), at).content.into_bytes();
        for (i, b) in edits {
            let i = i % json.len();
            json[i] = b"{}[]\":,0129aZ- \\"[usize::from(b) % 16];
        }
        let tags = vec![vec!["d".to_owned(), TURN_TAG.to_owned()]];
        let edited = Event::sign(&key(3), at, KIND_TURN, tags, String::from_utf8_lossy(&json).into_owned(), [0; 32]);
        let _ = parse_turn_note(&edited);
        let _ = list.offer(&edited, NOW);
        let _ = list.pick(Region::Oceania, 4);
        list.expire(NOW + 1_000);
        prop_assert_eq!(list.len(), 0);
    }
}
