//! Announcement notes and the announcer's schedule.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use olr_discovery::*;
use olr_lobby::Identity;

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
        bots: 3,
        spectators: 0,
        max_spectators: 4,
        password: false,
        region: Region::Europe,
        playing: false,
        addr: Some(IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4))),
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
        bad_content("\"bots\":3", "\"bots\":33"),
        bad_content("\"max_spectators\":4", "\"max_spectators\":17"),
        bad_content("Office", ""),
        bad_content("Office", "an unreasonably long game name"),
        bad_content("deathmatch", "a mode name far too long"),
        bad_content("olx-classic", "Not A Pack"),
        bad_content(&"ab".repeat(32), "xyz"),
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
    // A change within 12 s waits for them to pass.
    s.players = 3;
    assert!(!a.tick(at(65), &s));
    assert!(!a.tick(at(71), &s));
    assert!(a.tick(at(72), &s));
    assert!(!a.tick(at(73), &s));
    assert!(!a.tick(at(100), &s));
    // A change after the gap is immediate.
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

fn member(id: u8, seat: olr_proto::SeatWire) -> olr_proto::Member {
    olr_proto::Member {
        id,
        key: [id; 32],
        look: olr_proto::Look {
            name: format!("Worm {id}"),
            colour: [1, 2, 3],
            skin: "olx-skin-default".into(),
            skin_hash: olr_proto::NO_HASH.into(),
        },
        seat,
        ready: false,
        missing: Vec::new(),
        ping_ms: None,
        admin: false,
        download: None,
        path: None,
    }
}

fn lobby(phase: olr_proto::Phase, bots: usize) -> olr_proto::LobbyState {
    use olr_proto::SeatWire::*;
    let bot =
        olr_proto::BotWire { difficulty: 1, personality: 0, loadout: 0, look: member(99, Waiting).look };
    olr_proto::LobbyState {
        name: "Office".into(),
        phase,
        setup: olr_proto::MatchSetupWire {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-dirt-level".into(),
            packs: vec![
                ("olx-level-dirt-level".into(), "cd".repeat(32)),
                ("olx-classic".into(), "ab".repeat(32)),
            ],
            rules: olr_proto::RulesWire {
                lives: -2,
                kill_limit: 15,
                time_limit_min: 6.0,
                allow_negative_score: true,
                death_decreases_score: 0.0,
                suicide_decreases_score: false,
                sprees: [0; 5],
                dying_sprees: [0; 5],
            },
            bonuses: true,
            loading_time: 100,
            restrictions: String::new(),
            bots: vec![bot; bots],
        },
        members: vec![
            member(1, Player { slot: 0 }),
            member(2, Player { slot: 1 }),
            member(3, Spectator),
            member(4, Waiting),
        ],
    }
}

fn hosting() -> Hosting {
    Hosting {
        port: 23400,
        max_players: 8,
        max_spectators: 4,
        password: true,
        region: Region::Oceania,
        addr: Some(IpAddr::V4(Ipv4Addr::new(81, 2, 69, 142))),
        public_port: None,
        map_name: "Dirt Level".into(),
    }
}

/// The user's report of 2026-10-15 (N39 amended): one player who added
/// three bots is "1/8 +3 bots", not "4/8": bots take no seat a joiner
/// could have, so `players` counts seated members alone and `bots` the
/// bots, on Nostr and on the LAN.
#[test]
fn bots_take_no_seat_in_the_count() {
    let mut state = lobby(olr_proto::Phase::Gathering, 3);
    state.members.truncate(1);
    let s = GameSummary::of_lobby(&state, &hosting());
    assert_eq!((s.players, s.max_players, s.bots, s.spectators), (1, 8, 3, 0));
    let lan = hosting().lan(&s);
    assert_eq!((lan.players, lan.max_players, lan.bots), (1, 8, 3));
    let e = announcement(&host(), &s, 1, 1000, [1; 32]);
    assert!(e.content.contains("\"bots\":3"), "{}", e.content);
    assert_eq!(parse_announcement(&e).unwrap().0, s);
}

/// A note written before `bots` existed reads as one without bots.
#[test]
fn a_note_without_bots_reads_as_none() {
    let base = announcement(&host(), &summary(), 7, 1000, [1; 32]);
    let old = resign(&base, |_, c| *c = c.replace("\"bots\":3,", ""));
    assert!(!old.content.contains("bots"), "{}", old.content);
    let s = parse_announcement(&old).unwrap().0;
    assert_eq!((s.players, s.bots), (2, 0));
}

/// Plan 12b Task 8 (N47): a note names the router mapping's external port
/// (`public_port`) when the host has one; its LAN announcement keeps the
/// local port.
#[test]
fn a_note_names_the_public_port() {
    let mapped = Hosting { public_port: Some(40001), ..hosting() };
    let s = GameSummary::of_lobby(&lobby(olr_proto::Phase::Gathering, 0), &mapped);
    assert_eq!(s.port, 40001);
    assert_eq!(mapped.lan(&s).port, 23400, "the LAN joins the local port");
    let s = GameSummary::of_lobby(&lobby(olr_proto::Phase::Gathering, 0), &hosting());
    assert_eq!((s.port, hosting().lan(&s)), (23400, s.lan()));
}

#[test]
fn one_builder_summarizes_a_lobby_for_nostr_and_the_lan() {
    let s = GameSummary::of_lobby(&lobby(olr_proto::Phase::Gathering, 3), &hosting());
    assert_eq!(
        s,
        GameSummary {
            v: olr_proto::PROTOCOL_VERSION,
            name: "Office".into(),
            mode: "deathmatch".into(),
            map: "olx-level-dirt-level".into(),
            map_name: "Dirt Level".into(),
            mod_id: "olx-classic".into(),
            mod_hash: "ab".repeat(32),
            // Two seated members (bots take no seat); the waiting member watches.
            players: 2,
            max_players: 8,
            bots: 3,
            spectators: 2,
            max_spectators: 4,
            password: true,
            region: Region::Oceania,
            playing: false,
            addr: Some(IpAddr::V4(Ipv4Addr::new(81, 2, 69, 142))),
            port: 23400,
            closed: false,
        }
    );
    // A note of it is valid.
    let e = announcement(&host(), &s, olr_proto::FINGERPRINT, 1000, [1; 32]);
    assert_eq!(parse_announcement(&e).unwrap().0, s);
    // The LAN announcement counts the same.
    assert_eq!(
        s.lan(),
        olr_proto::LanAnnounce {
            fingerprint: olr_proto::FINGERPRINT,
            name: "Office".into(),
            port: 23400,
            players: 2,
            max_players: 8,
            spectators: 2,
            mod_id: "olx-classic".into(),
            level_id: "olx-level-dirt-level".into(),
            password: true,
            playing: false,
            bots: 3,
        }
    );
    let playing = GameSummary::of_lobby(&lobby(olr_proto::Phase::Playing { tick: 9 }, 0), &hosting());
    assert!(playing.playing && playing.lan().playing);
    assert!(GameSummary::of_lobby(&lobby(olr_proto::Phase::Starting, 0), &hosting()).playing);
    assert!(!GameSummary::of_lobby(&lobby(olr_proto::Phase::Results, 0), &hosting()).playing);
}

#[test]
fn the_builder_keeps_its_notes_valid() {
    // More seated than seats, more watchers than places: held to the
    // maximums; bots take no seat, so they are never cut to it.
    let small = Hosting { max_players: 1, max_spectators: 1, ..hosting() };
    let s = GameSummary::of_lobby(&lobby(olr_proto::Phase::Gathering, 3), &small);
    assert_eq!((s.players, s.bots, s.spectators), (1, 3, 1));
    // A blank or bad display name falls back to the level's id; a long one is cut.
    for bad in ["", "  ", "Dirt\u{0}Level"] {
        let s = GameSummary::of_lobby(
            &lobby(olr_proto::Phase::Gathering, 0),
            &Hosting { map_name: bad.into(), ..hosting() },
        );
        assert_eq!(s.map_name, "olx-level-dirt-level", "{bad:?}");
    }
    let long = "A".repeat(100);
    let s = GameSummary::of_lobby(
        &lobby(olr_proto::Phase::Gathering, 0),
        &Hosting { map_name: long, ..hosting() },
    );
    assert_eq!(s.map_name.chars().count(), announce::MAX_MAP_NAME);
    // No hash for the mod: the "unknown" hash, still a valid note.
    let mut state = lobby(olr_proto::Phase::Gathering, 0);
    state.setup.packs.clear();
    let s = GameSummary::of_lobby(&state, &hosting());
    assert_eq!(s.mod_hash, olr_proto::NO_HASH);
    let e = announcement(&host(), &s, 1, 1000, [1; 32]);
    assert!(parse_announcement(&e).is_ok());
}
