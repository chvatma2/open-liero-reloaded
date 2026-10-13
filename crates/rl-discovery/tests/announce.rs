//! Announcement notes and the announcer's schedule.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use rl_discovery::*;
use rl_lobby::Identity;

pub fn host() -> Identity {
    Identity::from_secret([9; 32]).unwrap()
}

pub fn summary() -> GameSummary {
    GameSummary {
        v: 1,
        name: "Office".into(),
        mode: "deathmatch".into(),
        map: "arena".into(),
        map_name: "The Arena".into(),
        mod_id: "olx-classic".into(),
        mod_hash: "ab".repeat(32),
        players: 2,
        max_players: 8,
        spectators: 0,
        max_spectators: 4,
        password: false,
        region: Region::Europe,
        playing: false,
        addr: Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
        port: 23400,
        closed: false,
    }
}

#[test]
fn announcements_round_trip() {
    let s = summary();
    let e = announcement(&host(), &s, 0xdead_beef_0123_4567, 1000, [1; 32]);
    assert!(e.verify());
    assert_eq!(e.kind, KIND_ANNOUNCE);
    assert_eq!(e.tag("d"), Some("rust-liero"));
    assert_eq!(e.tag("t"), Some("rust-liero"));
    assert_eq!(e.tag("f"), Some("deadbeef01234567"));
    assert_eq!(e.expiration(), Some(1180));
    assert!(e.content.len() <= MAX_CONTENT);
    assert_eq!(parse_announcement(&e).unwrap(), (s.clone(), 0xdead_beef_0123_4567));
    // The content's key for the mod is "mod".
    assert!(e.content.contains("\"mod\":\"olx-classic\""));

    let closed = GameSummary { closed: true, ..s };
    let e = announcement(&host(), &closed, 1, 1000, [1; 32]);
    assert_eq!(e.expiration(), Some(1010));
    assert!(parse_announcement(&e).unwrap().0.closed);
}

fn resign(e: &Event, f: impl FnOnce(&mut Vec<Vec<String>>, &mut String)) -> Event {
    let (mut tags, mut content) = (e.tags.clone(), e.content.clone());
    f(&mut tags, &mut content);
    Event::sign(&host(), e.created_at, e.kind, tags, content, [2; 32])
}

#[test]
fn malformed_announcements_are_refused() {
    let base = announcement(&host(), &summary(), 7, 1000, [1; 32]);
    let bad_content = |from: &str, to: &str| resign(&base, |_, c| *c = c.replace(from, to));
    for e in [
        resign(&base, |t, _| t.retain(|x| x[0] != "f")),
        resign(&base, |t, _| {
            t.iter_mut().filter(|x| x[0] == "f").for_each(|x| x[1] = "DEADBEEF01234567".into())
        }),
        resign(&base, |t, _| t.iter_mut().filter(|x| x[0] == "f").for_each(|x| x[1] = "abc".into())),
        resign(&base, |t, _| t.retain(|x| x[0] != "d")),
        resign(&base, |_, c| *c = "not json".into()),
        resign(&base, |_, c| *c = format!("{}{}", c, " ".repeat(MAX_CONTENT))),
        bad_content("\"players\":2", "\"players\":9"),
        bad_content("\"max_players\":8", "\"max_players\":33"),
        bad_content("\"max_spectators\":4", "\"max_spectators\":17"),
        bad_content("Office", ""),
        bad_content("Office", "an unreasonably long game name"),
        bad_content("deathmatch", "a mode name far too long"),
        bad_content("olx-classic", "Not A Pack"),
        bad_content(&"ab".repeat(32), "xyz"),
        bad_content("\"port\":23400", "\"port\":0"),
        bad_content("203.0.113.7", "0.0.0.0"),
    ] {
        assert!(parse_announcement(&e).is_err(), "{}", e.content);
    }
    let mut wrong_kind = base.clone();
    wrong_kind.kind = 1;
    assert!(parse_announcement(&wrong_kind).is_err());
    // An unknown region is `Unknown`, not an error.
    let e = bad_content("Europe", "Mars");
    assert_eq!(parse_announcement(&e).unwrap().0.region, Region::Unknown);
}

#[test]
fn the_announcer_republishes_every_minute_and_on_change() {
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut a = Announcer::new();
    let mut s = summary();
    assert!(a.tick(at(0), &s), "first tick publishes");
    assert!(!a.tick(at(1), &s));
    assert!(!a.tick(at(59), &s));
    assert!(a.tick(at(60), &s), "every minute");
    assert!(!a.tick(at(61), &s));
    // A change within 10 s waits for them to pass.
    s.players = 3;
    assert!(!a.tick(at(65), &s));
    assert!(!a.tick(at(69), &s));
    assert!(a.tick(at(70), &s));
    assert!(!a.tick(at(71), &s));
    assert!(!a.tick(at(100), &s));
    // A change after 10 s is immediate.
    s.players = 4;
    assert!(a.tick(at(101), &s));
    assert!(a.tick(at(161), &s));
    // Closing: one note, then nothing.
    assert!(a.closing());
    assert!(!a.closing());
    assert!(!a.tick(at(400), &s));
}

#[test]
fn closing_without_a_published_note_needs_no_note() {
    let mut a = Announcer::new();
    assert!(!a.closing());
}
