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
    let h = key(0);
    l.offer(&note(&h, &named("One"), FP, 1000), 1000);
    assert_eq!(l.games().len(), 1);
    // The closing note is accepted at once, despite the rate limit.
    let closed = GameSummary { closed: true, ..named("One") };
    l.offer(&note(&h, &closed, FP, 1001), 1001);
    assert!(l.games().is_empty());
    // A replayed older note does not bring it back.
    l.offer(&note(&h, &named("One"), FP, 1000), 1020);
    assert!(l.games().is_empty());
    // A closed note for an unknown key lists nothing.
    l.offer(&note(&key(1), &closed, FP, 1000), 1002);
    assert!(l.games().is_empty());
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
    let nested = Event::sign(
        &key(3),
        now,
        KIND_ANNOUNCE,
        vec![vec!["d".into(), "rust-liero".into()], vec!["f".into(), format!("{FP:016x}")]],
        format!("{}{}", "[".repeat(3000), "]".repeat(3000)),
        [1; 32],
    );
    l.offer(&nested, now);
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
