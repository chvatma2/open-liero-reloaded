//! The game list: verification, rate limit, expiry, sorting, bounds.

use rl_discovery::*;
use rl_lobby::Identity;

use crate::announce::summary;

const FP: u64 = 0x1111_2222_3333_4444;

fn key(i: u32) -> Identity {
    let mut s = [0u8; 32];
    s[..4].copy_from_slice(&(i + 1).to_le_bytes());
    s[31] = 1;
    Identity::from_secret(s).unwrap()
}

fn note(id: &Identity, s: &GameSummary, fp: u64, now: u64) -> Event {
    announcement(id, s, fp, now, [3; 32])
}

fn named(n: &str) -> GameSummary {
    GameSummary { name: n.into(), ..summary() }
}

#[test]
fn a_listed_game_is_compatible_and_replaced_by_its_next_note() {
    let mut l = GameList::new(FP);
    let h = key(0);
    l.offer(&note(&h, &named("One"), FP, 1000), 1000);
    let g = l.games();
    assert_eq!(g.len(), 1);
    assert!(g[0].compatible);
    assert_eq!(g[0].version_hint, "");
    assert_eq!((g[0].key, g[0].seen_unix, g[0].ping), (h.public(), 1000, Ping::Unknown));
    l.set_ping(&h.public(), Ping::Ms(30));
    l.offer(&note(&h, &named("Two"), FP, 1010), 1010);
    let g = l.games();
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].summary.name, "Two");
    assert_eq!(g[0].ping, Ping::Ms(30), "the ping survives a replacement");
    // An older note does not replace a newer one.
    l.offer(&note(&h, &named("Old"), FP, 1005), 1030);
    assert_eq!(l.games()[0].summary.name, "Two");
}

#[test]
fn a_closed_game_leaves_the_list() {
    let mut l = GameList::new(FP);
    assert!(l.is_empty());
    let h = key(0);
    l.offer(&note(&h, &named("One"), FP, 1000), 1000);
    assert_eq!((l.games().len(), l.len()), (1, 1));
    // The closing note is accepted at once, despite the rate limit.
    let closed = GameSummary { closed: true, ..named("One") };
    l.offer(&note(&h, &closed, FP, 1001), 1001);
    assert!(l.games().is_empty() && l.is_empty());
    assert_eq!(l.len(), 0);
    // A replayed older note does not bring it back.
    l.offer(&note(&h, &named("One"), FP, 1000), 1020);
    assert!(l.games().is_empty());
    // A closed note for an unknown key lists nothing.
    l.offer(&note(&key(1), &closed, FP, 1000), 1002);
    assert!(l.games().is_empty());
    // Out of order: one relay's closing note of a key never listed, then
    // another relay's older open note (it missed the close). The closing
    // note's tombstone refuses it, at once and after the gap.
    let late = key(2);
    l.offer(&note(&late, &closed, FP, 2001), 2001);
    l.offer(&note(&late, &named("Late"), FP, 2000), 2001);
    assert!(l.games().is_empty(), "the closed game stays closed");
    l.offer(&note(&late, &named("Late"), FP, 2000), 2030);
    assert!(l.games().is_empty(), "after the gap too");
}

#[test]
fn expired_notes_leave_the_list() {
    let mut l = GameList::new(FP);
    l.offer(&note(&key(0), &named("One"), FP, 1000), 1000);
    l.expire(1179);
    assert_eq!(l.games().len(), 1);
    l.expire(1180);
    assert!(l.games().is_empty());

    // Already expired, too old or from the far future: refused.
    l.offer(&note(&key(0), &named("One"), FP, 1000), 1180);
    l.offer(&note(&key(1), &named("Two"), FP, 1000), 1181);
    l.offer(&note(&key(2), &named("Three"), FP, 5000), 1000);
    assert!(l.games().is_empty());

    // No expiration tag: still dropped after 180 s by `created_at`.
    let h = key(3);
    let mut e = note(&h, &named("Four"), FP, 2000);
    e = Event::sign(
        &h,
        e.created_at,
        e.kind,
        e.tags.into_iter().filter(|t| t[0] != "expiration").collect(),
        e.content,
        [1; 32],
    );
    l.offer(&e, 2000);
    assert_eq!(l.games().len(), 1);
    l.expire(2180);
    assert_eq!(l.games().len(), 1);
    l.expire(2181);
    assert!(l.games().is_empty());
    l.offer(&e, 2181);
    assert!(l.games().is_empty());
}

#[test]
fn other_fingerprints_are_greyed() {
    let mut l = GameList::new(FP);
    l.offer(&note(&key(0), &named("Same"), FP, 1000), 1000);
    l.offer(&note(&key(1), &named("Other"), 0xabcdef00_00000000, 1000), 1000);
    let g = l.games();
    assert_eq!(g[0].summary.name, "Same");
    assert!(g[0].compatible);
    assert!(!g[1].compatible);
    assert_eq!(g[1].version_hint, "needs version 1-abcdef");
}

#[test]
fn blocked_keys_are_hidden() {
    let mut l = GameList::new(FP);
    l.offer(&note(&key(0), &named("One"), FP, 1000), 1000);
    l.block(key(0).public());
    l.block(key(0).public());
    assert_eq!(l.blocked(), &[key(0).public()]);
    assert!(l.games().is_empty());
    l.offer(&note(&key(0), &named("One"), FP, 1020), 1020);
    assert!(l.games().is_empty());
    l.offer(&note(&key(1), &named("Two"), FP, 1020), 1020);
    assert_eq!(l.games().len(), 1);
}

#[test]
fn a_hostile_feed_is_filtered_and_bounded() {
    let mut l = GameList::new(FP);
    let now = 10_000;
    // Bad signatures.
    let h = key(0);
    let mut forged = note(&h, &named("Forged"), FP, now);
    forged.content = forged.content.replace("Forged", "Evil");
    l.offer(&forged, now);
    let mut badsig = note(&h, &named("Sig"), FP, now);
    badsig.sig[0] ^= 1;
    l.offer(&badsig, now);
    assert!(l.games().is_empty());
    // A forged note did not block the real key.
    l.offer(&note(&h, &named("Real"), FP, now), now);
    assert_eq!(l.games().len(), 1);

    // 100 notes per second from one key for 30 s: at most one per 10 s.
    let spammer = key(1);
    let mut accepted = 0;
    let mut last = String::new();
    for t in 0..30u64 {
        for i in 0..100u64 {
            let s = named(&format!("S{t}x{i}"));
            l.offer(&note(&spammer, &s, FP, now + t), now + t);
            let cur = l.games().into_iter().find(|g| g.key == spammer.public()).map(|g| g.summary.name);
            if let Some(n) = cur
                && n != last
            {
                accepted += 1;
                last = n;
            }
        }
    }
    assert!(accepted <= 3, "{accepted}");
    assert!(accepted >= 1);

    // 4 KiB+ content and nested JSON.
    let big = Event::sign(
        &key(2),
        now,
        KIND_ANNOUNCE,
        vec![vec!["d".into(), "rust-liero".into()], vec!["f".into(), format!("{FP:016x}")]],
        "x".repeat(MAX_CONTENT + 1),
        [1; 32],
    );
    l.offer(&big, now);
    // Nested within the content limit: it reaches the parser (its
    // signature is checked), whose recursion limit refuses it.
    let nested = Event::sign(
        &key(3),
        now,
        KIND_ANNOUNCE,
        vec![vec!["d".into(), "rust-liero".into()], vec!["f".into(), format!("{FP:016x}")]],
        format!("{}{}", "[".repeat(2000), "]".repeat(2000)),
        [1; 32],
    );
    assert!(nested.content.len() <= MAX_CONTENT);
    let checked = l.verifications();
    l.offer(&nested, now);
    assert_eq!(l.verifications(), checked + 1, "the nested note reached the parser");
    assert!(l.games().iter().all(|g| g.key != key(2).public() && g.key != key(3).public()));

    // 10 000 keys.
    for i in 100..10_100 {
        l.offer(&note(&key(i), &named("Flood"), FP, now), now);
    }
    assert!(l.games().len() <= 500);
    assert!(l.games().len() >= 500 - 1);
}

#[test]
fn sorting_sinks_no_answer() {
    let mut l = GameList::new(FP);
    let mk = |i: u32, n: &str, fp: u64, l: &mut GameList| l.offer(&note(&key(i), &named(n), fp, 1000), 1000);
    mk(0, "slow", FP, &mut l);
    mk(1, "fast", FP, &mut l);
    mk(2, "unknown-b", FP, &mut l);
    mk(3, "dead", FP, &mut l);
    mk(4, "measuring-a", FP, &mut l);
    mk(5, "tie-b", FP, &mut l);
    mk(6, "tie-a", FP, &mut l);
    mk(7, "other", 5, &mut l);
    l.set_ping(&key(0).public(), Ping::Ms(200));
    l.set_ping(&key(1).public(), Ping::Ms(20));
    l.set_ping(&key(3).public(), Ping::NoAnswer);
    l.set_ping(&key(4).public(), Ping::Measuring);
    l.set_ping(&key(5).public(), Ping::Ms(50));
    l.set_ping(&key(6).public(), Ping::Ms(50));
    l.set_ping(&key(7).public(), Ping::Ms(1));
    let names: Vec<String> = l.games().into_iter().map(|g| g.summary.name).collect();
    assert_eq!(names, ["fast", "tie-a", "tie-b", "slow", "measuring-a", "unknown-b", "dead", "other"]);
    // Pinging an unlisted key does nothing.
    l.set_ping(&key(99).public(), Ping::Ms(1));
    assert_eq!(l.games().len(), 8);
}

#[test]
fn the_subscription_filter_selects_announcements() {
    let f = GameList::filter();
    assert_eq!(f.kinds, [KIND_ANNOUNCE]);
    assert_eq!(f.tags, [('t', vec!["rust-liero".to_string()])]);
    assert_eq!(f.since, None);
    assert_eq!(GameList::filter_since(1000).since, Some(820));
}

fn at_addr(i: u32, addr: &str) -> Event {
    let s = GameSummary { addr: Some(addr.parse().unwrap()), ..named("Addr") };
    note(&key(i), &s, FP, 1000)
}

#[test]
fn only_global_addresses_are_listed_unless_local_is_allowed() {
    let refused = [
        "0.1.2.3",
        "10.1.2.3",
        "100.64.0.1",
        "100.127.255.255",
        "127.0.0.1",
        "169.254.1.1",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.8",
        "192.0.2.5",
        "192.168.1.1",
        "198.18.0.1",
        "198.19.255.255",
        "198.51.100.9",
        "203.0.113.7",
        "224.0.0.1",
        "239.1.2.3",
        "240.0.0.1",
        "255.255.255.255",
        "::1",
        "::",
        "fc00::1",
        "fdff::1",
        "fe80::1",
        "febf::1",
        "2001:db8::1",
        "ff02::1",
        "::ffff:10.0.0.1",
        "::ffff:127.0.0.1",
        "::ffff:192.168.0.1",
        "::10.0.0.1",
        "::ffff:224.0.0.1",
    ];
    let accepted = [
        "8.8.4.4",
        "100.63.255.255",
        "100.128.0.1",
        "172.15.255.255",
        "172.32.0.1",
        "198.17.255.255",
        "198.20.0.1",
        "223.255.255.255",
        "2606:4700::1111",
        "2001:db9::1",
        "::ffff:8.8.4.4",
        "fe00::1",
    ];
    let mut strict = GameList::new(FP);
    let mut local = GameList::new(FP);
    local.allow_local(true);
    for (i, a) in refused.iter().enumerate() {
        let e = at_addr(i as u32, a);
        strict.offer(&e, 1000);
        local.offer(&e, 1000);
    }
    assert!(
        strict.games().is_empty(),
        "{:?}",
        strict.games().iter().map(|g| g.summary.addr).collect::<Vec<_>>()
    );
    assert_eq!(local.games().len(), refused.len());
    let mut strict = GameList::new(FP);
    for (i, a) in accepted.iter().enumerate() {
        strict.offer(&at_addr(i as u32, a), 1000);
    }
    assert_eq!(strict.games().len(), accepted.len());
    // No address at all is fine; the host can build local notes.
    let none = GameSummary { addr: None, ..named("None") };
    strict.offer(&note(&key(500), &none, FP, 1000), 1000);
    assert_eq!(strict.games().len(), accepted.len() + 1);
    assert!(parse_announcement(&at_addr(0, "10.0.0.1")).is_ok());
}

#[test]
fn the_remembered_keys_never_lock_out_an_honest_key() {
    let mut l = GameList::new(FP);
    let closed = GameSummary { closed: true, ..named("x") };
    // Hostile keys open a game and close it at once, leaving a tombstone each.
    for i in 0..4200 {
        let t = 1000 + u64::from(i / 100);
        l.offer(&note(&key(1000 + i), &named("x"), FP, t), t);
        l.offer(&note(&key(1000 + i), &closed, FP, t + 1), t + 1);
    }
    assert!(l.remembered_keys() <= 4096);
    assert!(l.games().is_empty());
    let t = 1043;
    l.offer(&note(&key(0), &named("Honest"), FP, t), t);
    assert_eq!(l.games().len(), 1);
    assert!(l.remembered_keys() <= 4096);
}

#[test]
fn remembered_keys_are_pruned_when_they_cannot_matter() {
    let mut l = GameList::new(FP);
    l.offer(&note(&key(0), &named("One"), FP, 1000), 1000);
    l.offer(&note(&key(1), &named("Two"), FP, 1000), 1000);
    assert_eq!(l.remembered_keys(), 2);
    l.expire(1100);
    assert_eq!(l.remembered_keys(), 2, "listed keys stay");
    l.expire(1180);
    assert!(l.games().is_empty());
    l.expire(1181);
    assert_eq!(l.remembered_keys(), 0);
}

#[test]
fn the_future_limit_is_sixty_seconds() {
    let mut l = GameList::new(FP);
    l.offer(&note(&key(0), &named("Ok"), FP, 1060), 1000);
    l.offer(&note(&key(1), &named("Far"), FP, 1061), 1000);
    let names: Vec<String> = l.games().into_iter().map(|g| g.summary.name).collect();
    assert_eq!(names, ["Ok"]);
}

#[test]
fn a_closing_note_expires_after_ten_seconds() {
    let closed = GameSummary { closed: true, ..named("One") };
    let mut l = GameList::new(FP);
    l.offer(&note(&key(0), &named("One"), FP, 1000), 1000);
    // Dated 1001, it expires at 1011: seen at 1011 it is dropped.
    l.offer(&note(&key(0), &closed, FP, 1001), 1011);
    assert_eq!(l.games().len(), 1);
    l.offer(&note(&key(0), &closed, FP, 1001), 1010);
    assert!(l.games().is_empty());
}

#[test]
fn a_forged_note_under_a_listed_key_costs_no_verification() {
    let mut l = GameList::new(FP);
    let h = key(0);
    l.offer(&note(&h, &named("One"), FP, 1000), 1000);
    assert_eq!(l.verifications(), 1);
    // Inside the gap and not closing, or older than the last note.
    let mut forged = note(&h, &named("Two"), FP, 1003);
    forged.sig[0] ^= 1;
    l.offer(&forged, 1003);
    let mut older = note(&h, &named("Two"), FP, 999);
    older.sig[0] ^= 1;
    l.offer(&older, 1020);
    assert_eq!(l.verifications(), 1);
    // A forged closing note inside the gap is checked and refused.
    let closed = GameSummary { closed: true, ..named("One") };
    let mut forged = note(&h, &closed, FP, 1003);
    forged.sig[0] ^= 1;
    l.offer(&forged, 1003);
    assert_eq!(l.verifications(), 2);
    assert_eq!(l.games().len(), 1);
}

/// The gap counts in arrival time, but a note dated at least the gap after
/// the key's last one is no faster than the host published it: it is
/// taken at once (a subscription that caught the last minute's note just
/// before the next; a relay that delivered an older version first).
#[test]
fn a_note_dated_a_gap_later_is_taken_at_once() {
    let mut l = GameList::new(FP);
    let h = key(0);
    // Subscribed 55 s after the host's note, the next one 5 s later.
    l.offer(&note(&h, &named("Old"), FP, 1000), 1055);
    l.offer(&note(&h, &named("New"), FP, 1060), 1060);
    assert_eq!(l.games()[0].summary.name, "New");
    // Two relays at the subscription: the older version first.
    let k = key(1);
    l.offer(&note(&k, &named("Older"), FP, 1040), 1100);
    l.offer(&note(&k, &named("Newest"), FP, 1052), 1100);
    let g = l.games().into_iter().find(|g| g.key == k.public()).unwrap();
    assert_eq!(g.summary.name, "Newest");
    // Less than the gap apart, in both clocks: still refused.
    l.offer(&note(&k, &named("Soon"), FP, 1061), 1101);
    let g = l.games().into_iter().find(|g| g.key == k.public()).unwrap();
    assert_eq!(g.summary.name, "Newest");
}

/// A full list refuses a new key before checking its signature (a flood of
/// fresh keys costs no verification once the list is full).
#[test]
fn a_full_list_refuses_new_keys_without_verifying() {
    let mut l = GameList::new(FP);
    for i in 0..500 {
        l.offer(&note(&key(100 + i), &named("Flood"), FP, 1000), 1000);
    }
    assert_eq!(l.len(), 500);
    let before = l.verifications();
    l.offer(&note(&key(5000), &named("More"), FP, 1000), 1000);
    assert_eq!(l.verifications(), before, "no signature checked");
    assert_eq!(l.len(), 500);
    // A listed key's next note is still taken.
    l.offer(&note(&key(100), &named("Next"), FP, 1010), 1010);
    assert_eq!(l.verifications(), before + 1);
}
