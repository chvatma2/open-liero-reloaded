//! The lobby's rules (networking spec §4, §5.2, §5.4, §7.2), one test per
//! rule, played through the in-process harness: every message is encoded
//! and decoded with rl-proto's codec on the way.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};

use proptest::prelude::*;
use rl_lobby::harness::Net;
use rl_lobby::*;
use rl_proto::*;

const HOST: u8 = 100;
const DELAY_US: u64 = 20_000;

fn ident(n: u8) -> Identity {
    Identity::from_secret([n; 32]).unwrap()
}

fn key(n: u8) -> PublicKey {
    ident(n).public()
}

fn look(name: &str) -> Look {
    Look { name: name.into(), colour: [1, 2, 3], skin: "olx-skin-afro".into() }
}

fn setup(bots: usize) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-dirt-level".into(),
        packs: vec![("olx-classic".into(), "ab".repeat(32))],
        rules: RulesWire {
            lives: 10,
            kill_limit: -1,
            time_limit_min: 5.0,
            allow_negative_score: true,
            death_decreases_score: 0.5,
            suicide_decreases_score: false,
            sprees: [1, 2, 3, 4, 5],
            dying_sprees: [5, 4, 3, 2, 1],
        },
        bonuses: true,
        loading_time: 100,
        restrictions: String::new(),
        bots: (0..bots)
            .map(|i| BotWire { difficulty: 1, personality: 0, loadout: 0, look: look(&format!("Bot {i}")) })
            .collect(),
    }
}

fn cfg() -> HostConfig {
    HostConfig { name: "Office".into(), password: None, max_players: 8, max_spectators: 4, admins: vec![] }
}

fn ip(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(10, 0, 0, n))
}

fn lobby_with(cfg: HostConfig, bots: usize) -> Net {
    Net::new(LobbyHost::new(cfg, ident(HOST), Some(look("Host")), setup(bots)).unwrap(), DELAY_US)
}

fn lobby() -> Net {
    lobby_with(cfg(), 0)
}

/// A client with key `n` joins from `10.0.0.n` and the exchange settles.
fn join(net: &mut Net, n: u8, name: &str) -> usize {
    let c = net.join(LobbyClient::new(ident(n), look(name), None, None), ip(n));
    net.settle();
    c
}

fn id_of(net: &Net, c: usize) -> u8 {
    net.host.member_of(net.peers[c].peer).expect("a member")
}

fn member(net: &Net, c: usize) -> Member {
    member_by_id(net, id_of(net, c))
}

fn member_by_id(net: &Net, id: u8) -> Member {
    net.host.state().members.iter().find(|m| m.id == id).cloned().expect("in the lobby")
}

fn host_id(net: &Net) -> u8 {
    net.host.state().members.iter().find(|m| m.key == key(HOST)).unwrap().id
}

fn got(net: &Net, c: usize, e: &ClientEvent) -> bool {
    net.peers[c].events.contains(e)
}

fn chats(net: &Net, c: usize, from: u8) -> Vec<String> {
    net.peers[c]
        .events
        .iter()
        .filter_map(|e| match e {
            ClientEvent::Chat { from: f, text } if *f == from => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// No message failed to decode, and every joined client sees the host's state.
fn consistent(net: &Net) {
    assert!(net.errors.is_empty(), "{:?}", net.errors);
    let state = net.host.state();
    assert_eq!(
        decode_to_client(&encode_to_client(&ToClient::Lobby(state.clone()))),
        Ok(ToClient::Lobby(state.clone()))
    );
    for (i, p) in net.peers.iter().enumerate() {
        if let Some(client) = &p.client
            && p.open
            && client.joined()
        {
            assert_eq!(client.state(), Some(state), "client {i}");
            assert_eq!(client.me(), net.host.member_of(p.peer), "client {i}");
        }
    }
}

fn seat(net: &Net, c: usize) -> SeatWire {
    member(net, c).seat
}

#[test]
fn three_players_and_a_spectator_gather_ready_and_start() {
    let mut net = lobby_with(HostConfig { max_players: 4, max_spectators: 2, ..cfg() }, 0);
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    let s = join(&mut net, 4, "Sam");
    for x in [a, b, c, s] {
        assert!(matches!(net.peers[x].received[0], ToClient::Challenge { .. }));
        assert!(matches!(net.peers[x].events[0], ClientEvent::Welcome { .. }));
    }
    assert_eq!(member_by_id(&net, host_id(&net)).seat, SeatWire::Player { slot: 0 });
    assert_eq!(seat(&net, a), SeatWire::Player { slot: 1 });
    assert_eq!(seat(&net, b), SeatWire::Player { slot: 2 });
    assert_eq!(seat(&net, c), SeatWire::Player { slot: 3 });
    assert_eq!(seat(&net, s), SeatWire::Spectator, "the players' seats are full");
    assert!(got(&net, a, &ClientEvent::Notice(Notice::Joined(id_of(&net, s)))));
    let waiting = vec![id_of(&net, a), id_of(&net, b), id_of(&net, c)];
    assert_eq!(net.host.can_start(), Err(NotStartable::NotReady(waiting)));
    assert!(!net.host_events.contains(&HostEvent::StartRequested));

    for x in [a, b, c] {
        net.client(x).set_ready(true);
    }
    net.settle();
    assert_eq!(net.host.can_start(), Ok(()));
    assert!(net.host_events.contains(&HostEvent::StartRequested));
    consistent(&net);

    let wire = net.start(7, 0xfeed).unwrap();
    assert_eq!(wire.start_checksum, 0xfeed);
    assert_eq!(wire.slots.len(), 4);
    for x in [a, b, c, s] {
        assert!(got(&net, x, &ClientEvent::Start(wire.clone())), "client {x}");
    }
    assert_eq!(net.host.state().phase, Phase::Starting);
    net.host.playing(1);
    net.settle();
    assert_eq!(net.host.state().phase, Phase::Playing { tick: 1 });
    consistent(&net);
}

#[test]
fn a_forged_signature_is_refused() {
    let mut net = lobby();
    for case in 0..3 {
        let r = net.join_raw(ip(1));
        net.settle();
        let ToClient::Challenge { nonce, host_key, needs_password } = net.peers[r].received[0].clone() else {
            panic!("no challenge: {:?}", net.peers[r].received)
        };
        assert_eq!(host_key, key(HOST));
        assert!(!needs_password);
        let me = ident(1);
        let signature = match case {
            0 => ident(2).sign(&challenge_message(&nonce, &host_key), [0; 32]),
            1 => me.sign(&challenge_message(&[9; 32], &host_key), [0; 32]),
            _ => me.sign(&challenge_message(&nonce, &key(7)), [0; 32]),
        };
        net.send_raw(r, ToHost::Hello { key: me.public(), look: look("Eve"), password: None });
        net.send_raw(r, ToHost::Prove { signature });
        net.settle();
        assert_eq!(net.peers[r].received.last(), Some(&ToClient::Refused(Refusal::Malformed)), "case {case}");
        assert!(net.host_events.contains(&HostEvent::Kick { peer: net.peers[r].peer, ban: false }));
        assert!(!net.peers[r].open, "the transport closes");
        assert_eq!(net.host.state().members.len(), 1, "case {case}");
    }
    // The genuine signature joins.
    let r = net.join_raw(ip(1));
    net.settle();
    let ToClient::Challenge { nonce, host_key, .. } = net.peers[r].received[0].clone() else { panic!() };
    net.send_raw(r, ToHost::Hello { key: key(1), look: look("Eve"), password: None });
    net.send_raw(
        r,
        ToHost::Prove { signature: ident(1).sign(&challenge_message(&nonce, &host_key), [3; 32]) },
    );
    net.settle();
    assert!(matches!(net.peers[r].received.last(), Some(ToClient::Notice(Notice::Joined(_)))));
    assert!(net.peers[r].received.iter().any(|m| matches!(m, ToClient::Welcome { .. })));
    assert_eq!(net.host.state().members.len(), 2);
    consistent(&net);
}

#[test]
fn bans_given_by_key_or_address_are_refused_and_listed() {
    let mut net = lobby();
    // A dedicated server's bans file: a key and an address, before anyone joins.
    net.host.ban_key(key(1));
    net.host.ban_ip(ip(9));
    let a = net.join(LobbyClient::new(ident(1), look("Ann"), None, None), ip(1));
    net.settle();
    assert!(got(&net, a, &ClientEvent::Refused(Refusal::Banned)));
    let b = net.join(LobbyClient::new(ident(2), look("Bob"), None, None), ip(9));
    net.settle();
    assert_eq!(net.peers[b].received, vec![ToClient::Refused(Refusal::Banned)]);
    let c = join(&mut net, 3, "Cid");
    assert!(matches!(net.peers[c].events[0], ClientEvent::Welcome { .. }));
    // A ban with the address joins both lists, in order.
    net.host.ban_with_address(id_of(&net, c));
    net.settle();
    assert!(got(&net, c, &ClientEvent::Kicked { banned: true }));
    let mut keys = vec![key(1), key(3)];
    keys.sort();
    assert_eq!(net.host.banned_keys().copied().collect::<Vec<_>>(), keys);
    assert_eq!(net.host.banned_ips().copied().collect::<Vec<_>>(), vec![ip(3), ip(9)]);
    // Lifted: the key and the address join again.
    assert!(net.host.unban_key(&key(3)) && net.host.unban_ip(ip(3)));
    assert!(!net.host.unban_key(&key(3)) && !net.host.unban_ip(ip(3)), "once");
    let back = join(&mut net, 3, "Cid");
    assert!(matches!(net.peers[back].events[0], ClientEvent::Welcome { .. }));
    consistent(&net);
}

#[test]
fn a_wrong_password_is_refused() {
    let mut net = lobby_with(HostConfig { password: Some("secret".into()), ..cfg() }, 0);
    let wrong = net.join(LobbyClient::new(ident(1), look("Ann"), Some("wrong".into()), None), ip(1));
    let none = net.join(LobbyClient::new(ident(2), look("Bob"), None, None), ip(2));
    let right = net.join(LobbyClient::new(ident(3), look("Cid"), Some("secret".into()), None), ip(3));
    net.settle();
    for c in [wrong, none] {
        assert!(matches!(net.peers[c].received[0], ToClient::Challenge { needs_password: true, .. }));
        assert!(got(&net, c, &ClientEvent::Refused(Refusal::Password)), "client {c}");
        assert!(!net.peers[c].open);
        assert_eq!(net.host.member_of(net.peers[c].peer), None);
    }
    assert!(matches!(net.peers[right].events[0], ClientEvent::Welcome { .. }));
    assert_eq!(net.host.state().members.len(), 2);
    consistent(&net);
}

/// Guessing the password: after `PASSWORD_TRIES` (3) wrong proofs from an
/// IP within `PASSWORD_WINDOW_US` (a minute), that IP's joins are refused
/// (`Refusal::Throttled`) for `PASSWORD_LOCKOUT_US` (a minute); other
/// addresses are not touched, and no password (the empty one's proof) is
/// no guess.
#[test]
fn password_guessers_are_held_off_for_a_minute() {
    let mut net = lobby_with(HostConfig { password: Some("secret".into()), ..cfg() }, 0);
    let joiner = |net: &mut Net, i: u8, pw: Option<&str>, from: u8| {
        let c = net.join(LobbyClient::new(ident(i), look("Ann"), pw.map(Into::into), None), ip(from));
        net.settle();
        c
    };
    let none = joiner(&mut net, 1, None, 1);
    assert!(got(&net, none, &ClientEvent::Refused(Refusal::Password)));
    for _ in 0..PASSWORD_TRIES - 1 {
        let c = joiner(&mut net, 1, Some("guess"), 1);
        assert!(got(&net, c, &ClientEvent::Refused(Refusal::Password)));
    }
    // Two wrong guesses and a missing proof: the right one still works.
    let right = joiner(&mut net, 1, Some("secret"), 1);
    assert!(
        matches!(net.peers[right].events[0], ClientEvent::Welcome { .. }),
        "{:?}",
        net.peers[right].events
    );
    let c = joiner(&mut net, 2, Some("guess"), 1);
    assert!(got(&net, c, &ClientEvent::Refused(Refusal::Password)), "the third wrong guess");
    let held = joiner(&mut net, 3, Some("secret"), 1);
    assert!(got(&net, held, &ClientEvent::Refused(Refusal::Throttled)), "held off, even with the password");
    assert!(!net.peers[held].received.iter().any(|m| matches!(m, ToClient::Challenge { .. })));
    let other = joiner(&mut net, 4, Some("secret"), 2);
    assert!(matches!(net.peers[other].events[0], ClientEvent::Welcome { .. }), "another address joins");
    net.run(PASSWORD_LOCKOUT_US - 1_000_000);
    let still = joiner(&mut net, 3, Some("secret"), 1);
    assert!(got(&net, still, &ClientEvent::Refused(Refusal::Throttled)));
    net.run(1_000_000);
    let back = joiner(&mut net, 3, Some("secret"), 1);
    assert!(matches!(net.peers[back].events[0], ClientEvent::Welcome { .. }), "after a minute");
    consistent(&net);
}

/// Links opened before the guessing starts (all challenged, none held off
/// yet) get no more than `PASSWORD_TRIES` guesses between them: the hold
/// is checked at each join attempt (`Hello`), not only when a link opens.
#[test]
fn links_opened_early_get_no_more_guesses() {
    let mut net = lobby_with(HostConfig { password: Some("secret".into()), ..cfg() }, 0);
    let links: Vec<usize> = (0..10)
        .map(|i| net.join(LobbyClient::new(ident(10 + i), look("Ann"), Some("guess".into()), None), ip(1)))
        .collect();
    net.settle();
    let refused = |r: Refusal| links.iter().filter(|&&c| got(&net, c, &ClientEvent::Refused(r))).count();
    assert!(
        links.iter().all(|&c| net.peers[c].received.iter().any(|m| matches!(m, ToClient::Challenge { .. })))
    );
    assert_eq!(refused(Refusal::Password), PASSWORD_TRIES, "guesses answered");
    assert_eq!(refused(Refusal::Throttled), links.len() - PASSWORD_TRIES, "the rest held off");
    consistent(&net);
}

/// Guesses spread over more than a minute never hold anyone off.
#[test]
fn slow_password_mistakes_are_forgiven() {
    let mut net = lobby_with(HostConfig { password: Some("secret".into()), ..cfg() }, 0);
    for _ in 0..2 * PASSWORD_TRIES {
        let c = net.join(LobbyClient::new(ident(1), look("Ann"), Some("typo".into()), None), ip(1));
        net.settle();
        assert!(got(&net, c, &ClientEvent::Refused(Refusal::Password)));
        net.run(PASSWORD_WINDOW_US / 2 + 1_000);
    }
    let right = net.join(LobbyClient::new(ident(1), look("Ann"), Some("secret".into()), None), ip(1));
    net.settle();
    assert!(matches!(net.peers[right].events[0], ClientEvent::Welcome { .. }));
}

#[test]
fn banned_keys_and_ips_are_refused() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let a_id = id_of(&net, a);
    net.host.kick(a_id, true);
    net.settle();
    assert!(got(&net, a, &ClientEvent::Kicked { banned: true }));
    assert!(!net.peers[a].open);
    assert!(got(&net, b, &ClientEvent::Notice(Notice::Banned(a_id))));
    assert!(net.host_events.contains(&HostEvent::Kick { peer: net.peers[a].peer, ban: true }));
    // Spec §5.2: by key; the address stays open.
    assert!(net.host.banned(&key(1)));
    assert!(!net.host.banned_ip(ip(1)));
    assert!(!net.host.banned(&key(2)) && !net.host.banned_ip(ip(2)));

    // The same key from another address.
    let again = net.join(LobbyClient::new(ident(1), look("Ann"), None, None), ip(50));
    net.settle();
    assert!(got(&net, again, &ClientEvent::Refused(Refusal::Banned)));
    // Another key from the same address: welcome.
    let other = join(&mut net, 3, "Cid");
    assert!(matches!(net.peers[other].events[0], ClientEvent::Welcome { .. }));
    // Banned with the address: another key from it is refused at once.
    let c_id = id_of(&net, other);
    net.host.ban_with_address(c_id);
    net.settle();
    assert!(net.host.banned(&key(3)) && net.host.banned_ip(ip(3)));
    let from_there = net.join(LobbyClient::new(ident(4), look("Dan"), None, None), ip(3));
    net.settle();
    assert_eq!(net.peers[from_there].received, vec![ToClient::Refused(Refusal::Banned)]);
    assert!(!net.peers[from_there].open);

    // A plain kick does not ban.
    let b_id = id_of(&net, b);
    net.host.kick(b_id, false);
    net.settle();
    assert!(got(&net, b, &ClientEvent::Kicked { banned: false }));
    assert!(!net.host.banned(&key(2)));
    let back = join(&mut net, 2, "Bob");
    assert!(matches!(net.peers[back].events[0], ClientEvent::Welcome { .. }));
    // The host cannot kick itself.
    net.host.kick(host_id(&net), true);
    net.settle();
    assert_eq!(net.host.state().members.len(), 2);
    consistent(&net);
}

#[test]
fn duplicate_names_get_a_suffix() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Martin");
    let b = join(&mut net, 2, "martin");
    let c = join(&mut net, 3, "MARTIN");
    let d = join(&mut net, 4, "abcdefghijklmnopqrst");
    let e = join(&mut net, 5, "ABCDEFGHIJKLMNOPQRST");
    let f = join(&mut net, 6, "Émile");
    let g = join(&mut net, 7, "émile");
    let name = |net: &Net, c| member(net, c).look.name;
    assert_eq!(name(&net, a), "Martin");
    assert_eq!(name(&net, b), "martin (2)");
    assert_eq!(name(&net, c), "MARTIN (3)");
    assert_eq!(name(&net, d), "abcdefghijklmnopqrst");
    assert_eq!(name(&net, e), "ABCDEFGHIJKLMNOP (2)", "cut to fit 20 characters");
    assert_eq!(name(&net, f), "Émile");
    assert_eq!(name(&net, g), "émile (2)");
    // Re-setting one's own name keeps it; taking another's gets a suffix.
    net.client(a).set_look(look("Martin"));
    net.client(b).set_look(look("host"));
    net.settle();
    assert_eq!(name(&net, a), "Martin");
    assert_eq!(name(&net, b), "host (2)");
    // A name freed by a leaver can be taken.
    net.client(a).leave();
    net.settle();
    net.client(c).set_look(look("Martin"));
    net.settle();
    assert_eq!(name(&net, c), "Martin");
    consistent(&net);
}

#[test]
fn setup_changes_clear_ready() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    for x in [a, b] {
        net.client(x).set_ready(true);
    }
    net.settle();
    assert!(member(&net, a).ready && member(&net, b).ready);
    assert!(member_by_id(&net, host_id(&net)).ready, "the host is ready");

    let mut changed = setup(0);
    changed.rules.lives = 3;
    net.host.set_setup(changed.clone()).unwrap();
    net.settle();
    assert!(!member(&net, a).ready && !member(&net, b).ready);
    assert!(member_by_id(&net, host_id(&net)).ready, "the host's own ready stays");
    assert_eq!(net.peers[a].client.as_ref().unwrap().state().unwrap().setup, changed);

    // Only the bots changed (a server's fill bots, Add/Remove bot): the
    // ready marks stay.
    for x in [a, b] {
        net.client(x).set_ready(true);
    }
    net.settle();
    let mut more_bots = changed.clone();
    more_bots.bots = setup(2).bots;
    net.host.set_setup(more_bots.clone()).unwrap();
    net.settle();
    assert!(member(&net, a).ready && member(&net, b).ready, "bots alone keep ready");
    assert_eq!(net.host.state().setup, more_bots);
    net.host.set_setup(changed.clone()).unwrap();
    net.settle();
    assert!(member(&net, a).ready && member(&net, b).ready);

    // The same setup again changes nothing.
    for x in [a, b] {
        net.client(x).set_ready(true);
    }
    net.settle();
    net.host.set_setup(changed.clone()).unwrap();
    net.settle();
    assert!(member(&net, a).ready && member(&net, b).ready);
    // An invalid setup is refused and changes nothing.
    let mut bad = changed.clone();
    bad.mod_id = "../x".into();
    assert_eq!(net.host.set_setup(bad), Err(LobbyError::InvalidSetup));
    assert!(member(&net, a).ready);
    assert_eq!(net.host.state().setup, changed);
    consistent(&net);
}

/// The setup changes only while the lobby gathers: during a match (and its
/// results) the running match's setup stays what the lobby shows (an
/// admin's `/settings` on a server targets the next rotation entry).
#[test]
fn the_setup_cannot_change_outside_gathering() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    net.client(a).set_ready(true);
    net.settle();
    net.start(1, 2).unwrap();
    let mut changed = setup(0);
    changed.rules.lives = 3;
    assert_eq!(net.host.set_setup(changed.clone()), Err(LobbyError::NotGathering), "starting");
    net.host.playing(0);
    assert_eq!(net.host.set_setup(changed.clone()), Err(LobbyError::NotGathering), "playing");
    assert_eq!(net.host.state().setup, setup(0));
    net.settle();
    consistent(&net);
}

#[test]
fn chat_is_cut_and_rate_limited() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let a_id = id_of(&net, a);
    net.client(a).chat(&"x".repeat(250));
    net.client(a).chat("hi\u{7} \u{202E}there");
    net.settle();
    let expected = vec!["x".repeat(MAX_CHAT), "hi there".to_string()];
    assert_eq!(chats(&net, b, a_id), expected);
    assert_eq!(chats(&net, a, a_id), expected, "the sender sees its line");
    assert!(net.host_events.contains(&HostEvent::Chat { from: a_id, text: "hi there".into() }));

    net.run(CHAT_WINDOW_US + 1);
    for i in 0..7 {
        net.client(a).chat(&format!("line {i}"));
    }
    net.settle();
    let lines: Vec<String> = (0..CHAT_LINES).map(|i| format!("line {i}")).collect();
    assert_eq!(chats(&net, b, a_id)[2..], lines[..]);
    let slow = ClientEvent::Notice(Notice::Message(SLOW_DOWN.into()));
    assert_eq!(net.peers[a].events.iter().filter(|e| **e == slow).count(), 2);
    assert!(!got(&net, b, &slow), "only the sender is told");

    net.run(CHAT_WINDOW_US);
    net.client(a).chat("again");
    net.host.chat(&"y".repeat(300));
    net.settle();
    assert_eq!(chats(&net, b, a_id).last().unwrap(), "again");
    assert_eq!(chats(&net, b, host_id(&net)), vec!["y".repeat(MAX_CHAT)]);
    consistent(&net);
}

#[test]
fn only_admins_kick() {
    let mut net = lobby_with(HostConfig { admins: vec![key(1)], ..cfg() }, 0);
    let a = join(&mut net, 1, "Admin");
    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    assert!(member(&net, a).admin);
    assert!(!member(&net, b).admin);
    assert!(member_by_id(&net, host_id(&net)).admin, "the host is an admin");

    let not_admin = ClientEvent::Notice(Notice::Message(NOT_ADMIN.into()));
    for cmd in [AdminCommand::Kick(key(3)), AdminCommand::Ban(key(3)), AdminCommand::Next] {
        net.client(b).admin(cmd);
    }
    net.settle();
    assert_eq!(net.peers[b].events.iter().filter(|e| **e == not_admin).count(), 3);
    assert!(net.host.member_of(net.peers[c].peer).is_some());
    assert!(!net.host_events.iter().any(|e| matches!(e, HostEvent::Admin { .. } | HostEvent::Kick { .. })));

    // Nobody kicks the host.
    net.client(a).admin(AdminCommand::Kick(key(HOST)));
    net.settle();
    assert_eq!(net.host.state().members.len(), 4);

    net.client(a).admin(AdminCommand::Kick(key(3)));
    net.settle();
    assert!(got(&net, c, &ClientEvent::Kicked { banned: false }));
    assert!(got(&net, b, &ClientEvent::Notice(Notice::Kicked(member_id_from_events(&net, c)))));
    net.client(a).admin(AdminCommand::Ban(key(2)));
    net.settle();
    assert!(got(&net, b, &ClientEvent::Kicked { banned: true }));
    assert!(net.host.banned(&key(2)) && !net.host.banned_ip(ip(2)), "an admin's ban is by key");
    // A key not in the lobby is banned all the same.
    net.client(a).admin(AdminCommand::Ban(key(9)));
    net.client(a).admin(AdminCommand::Next);
    net.settle();
    assert!(net.host.banned(&key(9)));
    assert!(net.host_events.contains(&HostEvent::Admin { from: id_of(&net, a), cmd: AdminCommand::Next }));
    assert_eq!(net.host.state().members.len(), 2);
    consistent(&net);
}

/// The member id a client was welcomed with.
fn member_id_from_events(net: &Net, c: usize) -> u8 {
    net.peers[c]
        .events
        .iter()
        .find_map(|e| match e {
            ClientEvent::Welcome { you } => Some(*you),
            _ => None,
        })
        .unwrap()
}

#[test]
fn missing_content_prevents_ready() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let a_id = id_of(&net, a);
    net.client(a).have(vec!["olx-skin-x".into()]);
    net.client(a).set_ready(true);
    net.settle();
    assert_eq!(member(&net, a).missing, vec!["olx-skin-x".to_string()]);
    assert!(!member(&net, a).ready);
    assert!(got(&net, a, &ClientEvent::Notice(Notice::Message(MISSING_CONTENT.into()))));
    assert_eq!(net.host.can_start(), Err(NotStartable::NotReady(vec![a_id])));
    // Content arriving does not make a player ready by itself.
    net.client(a).have(vec![]);
    net.settle();
    assert!(!member(&net, a).ready);
    net.client(a).set_ready(true);
    net.settle();
    assert!(member(&net, a).ready);
    assert_eq!(net.host.can_start(), Ok(()));
    // Losing content clears ready.
    net.client(a).have(vec!["olx-skin-y".into()]);
    net.settle();
    assert!(!member(&net, a).ready);
    // Invalid ids are dropped by the client, the rest is bounded.
    net.client(a).have((0..20).map(|i| format!("p{i}")).chain(["Bad/Id".into()]).collect());
    net.settle();
    assert_eq!(member(&net, a).missing.len(), MAX_MISSING);
    consistent(&net);
}

#[test]
fn joiners_during_a_match_wait_and_get_seats_after() {
    let mut net = lobby_with(HostConfig { max_players: 3, max_spectators: 2, ..cfg() }, 0);
    let a = join(&mut net, 1, "Ann");
    net.client(a).set_ready(true);
    net.settle();
    net.start(1, 2).unwrap();
    net.host.playing(0);
    net.settle();

    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    let d = join(&mut net, 4, "Dan");
    let e = join(&mut net, 5, "Eve");
    assert_eq!(seat(&net, b), SeatWire::Waiting);
    assert_eq!(seat(&net, c), SeatWire::Spectator, "no seat left for the next match");
    assert_eq!(seat(&net, d), SeatWire::Spectator);
    assert!(got(&net, e, &ClientEvent::Refused(Refusal::Full)));
    for x in [b, c, d] {
        assert!(!net.peers[x].events.iter().any(|e| matches!(e, ClientEvent::Start(_))));
    }
    // A player leaves mid-match: the next match has a seat for one more.
    let a_id = id_of(&net, a);
    net.disconnect(a);
    net.settle();
    assert!(got(&net, b, &ClientEvent::Notice(Notice::Left(a_id))));

    let end = MatchEndWire { reason: "time".into(), rows: vec![(0, 1, 1, 0, 0)], last_tick: 900 };
    net.host.match_ended(end.clone(), net.now_us);
    net.settle();
    assert_eq!(net.host.state().phase, Phase::Results);
    for x in [b, c, d] {
        assert!(got(&net, x, &ClientEvent::Ended(end.clone())));
    }
    net.run(RESULTS_US - 2 * DELAY_US);
    assert_eq!(net.host.state().phase, Phase::Results);
    net.run(2 * DELAY_US);
    net.settle();
    assert_eq!(net.host.state().phase, Phase::Gathering);
    assert_eq!(member_by_id(&net, host_id(&net)).seat, SeatWire::Player { slot: 0 });
    assert_eq!(seat(&net, b), SeatWire::Player { slot: 1 }, "in join order");
    assert_eq!(seat(&net, c), SeatWire::Player { slot: 2 });
    assert_eq!(seat(&net, d), SeatWire::Spectator);
    assert!(net.host.state().members.iter().filter(|m| m.key != key(HOST)).all(|m| !m.ready));
    // A spectator by choice keeps watching when a seat frees up.
    net.client(d).set_spectate(true);
    net.client(c).set_spectate(true);
    net.settle();
    assert_eq!(seat(&net, c), SeatWire::Spectator);
    assert_eq!(seat(&net, d), SeatWire::Spectator);
    net.client(d).set_spectate(false);
    net.settle();
    assert_eq!(seat(&net, d), SeatWire::Player { slot: 2 });
    consistent(&net);
}

/// `watcher` was told `id` left, the host reported it, and it is gone.
fn left(net: &Net, watcher: usize, id: u8) {
    assert!(got(net, watcher, &ClientEvent::Notice(Notice::Left(id))), "{id} left");
    assert!(net.host_events.contains(&HostEvent::Left(id)));
    assert!(net.host.state().members.iter().all(|m| m.id != id));
    consistent(net);
}

#[test]
fn leaving_at_every_stage() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    let d = join(&mut net, 4, "Dan");
    let w = join(&mut net, 6, "Wes");
    let ids: Vec<u8> = [a, b, c, d].iter().map(|&x| id_of(&net, x)).collect();

    // Before the proof, saying goodbye: the link closes.
    let bye = net.join_raw(ip(8));
    net.send_raw(bye, ToHost::Bye);
    net.settle();
    assert!(!net.peers[bye].open);
    assert!(net.host_events.contains(&HostEvent::Close { peer: net.peers[bye].peer }));
    net.host_events.clear();

    // Before the proof: nobody hears of it, and no timeout fires later.
    let r = net.join_raw(ip(9));
    net.settle();
    net.disconnect(r);
    net.run(PROOF_TIMEOUT_US + 1);
    assert_eq!(net.host.state().members.len(), 6);
    assert!(!net.peers[w].events.iter().any(|e| matches!(e, ClientEvent::Notice(Notice::Left(_)))));
    assert!(!net.host_events.iter().any(|e| matches!(e, HostEvent::Kick { .. } | HostEvent::Left(_))));

    // After the welcome, by saying goodbye: the host closes the link.
    net.client(a).leave();
    net.settle();
    left(&net, w, ids[0]);
    assert!(!net.peers[a].open);
    assert!(net.host_events.contains(&HostEvent::Close { peer: net.peers[a].peer }));

    // While starting, by losing the connection.
    for x in [b, c, d, w] {
        net.client(x).set_ready(true);
    }
    net.settle();
    net.start(5, 6).unwrap();
    net.disconnect(b);
    net.settle();
    left(&net, w, ids[1]);
    assert_eq!(net.host.state().phase, Phase::Starting);

    // While playing.
    net.host.playing(3);
    net.client(c).leave();
    net.settle();
    left(&net, w, ids[2]);
    assert_eq!(net.host.state().phase, Phase::Playing { tick: 3 });

    // During the results.
    net.host.match_ended(MatchEndWire { reason: "kills".into(), rows: vec![], last_tick: 900 }, net.now_us);
    net.settle();
    net.disconnect(d);
    net.settle();
    left(&net, w, ids[3]);
    assert_eq!(net.host.state().phase, Phase::Results);

    net.run(RESULTS_US);
    net.settle();
    assert_eq!(net.host.state().phase, Phase::Gathering);
    assert_eq!(net.host.state().members.len(), 2);
    assert_eq!(seat(&net, w), SeatWire::Player { slot: 5 });
    assert!(net.host.peer_of(ids[0]).is_none());
    consistent(&net);
}

#[test]
fn unproven_peers_are_dropped() {
    let mut net = lobby();
    // No proof within 10 s.
    let r = net.join_raw(ip(1));
    net.settle();
    net.run(PROOF_TIMEOUT_US - 2 * DELAY_US);
    assert!(net.peers[r].open);
    net.run(2 * DELAY_US);
    net.settle();
    assert!(!net.peers[r].open);
    assert!(net.host_events.contains(&HostEvent::Kick { peer: net.peers[r].peer, ban: false }));

    // Anything but Hello and Prove before the proof.
    for msg in [
        ToHost::Chat("hi".into()),
        ToHost::Ready(true),
        ToHost::Prove { signature: [1; 64] },
        ToHost::Admin(AdminCommand::Next),
    ] {
        let r = net.join_raw(ip(2));
        net.send_raw(r, msg.clone());
        net.settle();
        assert_eq!(net.peers[r].received.last(), Some(&ToClient::Refused(Refusal::Malformed)), "{msg:?}");
        assert!(!net.peers[r].open);
    }
    // A second Hello.
    let r = net.join_raw(ip(3));
    let hello = ToHost::Hello { key: key(3), look: look("Cid"), password: None };
    net.send_raw(r, hello.clone());
    net.send_raw(r, hello.clone());
    net.settle();
    assert_eq!(net.peers[r].received.last(), Some(&ToClient::Refused(Refusal::Malformed)));

    // A proven member sending a Hello again is kicked.
    let a = join(&mut net, 4, "Ann");
    net.send_raw(a, ToHost::Hello { key: key(4), look: look("Ann"), password: None });
    net.settle();
    assert!(got(&net, a, &ClientEvent::Kicked { banned: false }));
    assert_eq!(net.host.state().members.len(), 1);
    // A second connection with a member's key is refused.
    let b = join(&mut net, 5, "Bob");
    let twin = net.join(LobbyClient::new(ident(5), look("Bob"), None, None), ip(6));
    net.settle();
    assert!(got(&net, twin, &ClientEvent::Refused(Refusal::Busy)));
    assert!(net.peers[b].open);
    consistent(&net);
}

#[test]
fn the_start_message_assigns_slots_and_delays() {
    let mut net = lobby_with(cfg(), 2);
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    let s = join(&mut net, 4, "Sam");
    net.client(s).set_spectate(true);
    net.client(b).leave();
    net.client(a).set_loadout(Some([1, 2, 3, 4, 5]));
    for x in [a, c] {
        net.client(x).set_ready(true);
    }
    net.settle();
    assert_eq!(seat(&net, c), SeatWire::Player { slot: 3 }, "slot 2 is free");
    let (host, a_id, c_id) = (host_id(&net), id_of(&net, a), id_of(&net, c));

    // Cid's round trip is not measured yet: the largest delay.
    let rtts = BTreeMap::from([(net.peers[a].peer, 50), (net.peers[s].peer, 10)]);
    let wire = net.host.start(9, &rtts).unwrap();
    assert_eq!(wire.seed, 9);
    assert_eq!(wire.start_checksum, 0, "filled in by the match layer");
    assert_eq!(wire.setup, setup(2));
    let members: Vec<Option<u8>> = wire.slots.iter().map(|x| x.member).collect();
    assert_eq!(members, [Some(host), Some(a_id), Some(c_id), None, None]);
    let bots: Vec<Option<u8>> = wire.slots.iter().map(|x| x.bot).collect();
    assert_eq!(bots, [None, None, None, Some(0), Some(1)]);
    let names: Vec<&str> = wire.slots.iter().map(|x| x.look.name.as_str()).collect();
    assert_eq!(names, ["Host", "Ann", "Cid", "Bot 0", "Bot 1"]);
    assert_eq!(wire.slots[1].loadout, Some([1, 2, 3, 4, 5]));
    assert_eq!(wire.slots[2].loadout, None);
    assert_eq!(wire.input_delay, vec![(host, 0), (a_id, 1), (c_id, MAX_INPUT_DELAY)]);
    assert_eq!(net.host.state().phase, Phase::Gathering, "nothing is sent yet");
    net.settle();
    assert!(!net.peers[a].events.iter().any(|e| matches!(e, ClientEvent::Start(_))));

    let mut announced = wire.clone();
    announced.start_checksum = 0xabc;
    net.host.announce_start(announced.clone()).unwrap();
    net.settle();
    for x in [a, c, s] {
        assert!(net.peers[x].received.contains(&ToClient::Start(announced.clone())));
    }
    assert_eq!(net.host.state().phase, Phase::Starting);
    assert_eq!(net.host.start(9, &rtts), Err(NotStartable::Phase));
    assert_eq!(net.host.announce_start(announced), Err(NotStartable::Phase));
    consistent(&net);
}

#[test]
fn member_ids_of_the_match_are_not_reused() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    net.client(a).set_ready(true);
    net.settle();
    let wire = net.start(1, 2).unwrap();
    let in_match: Vec<u8> = wire.slots.iter().filter_map(|s| s.member).collect();
    net.client(a).leave();
    net.settle();
    // Member ids wrap after 256: none of the running match's is handed out.
    for _ in 0..300 {
        let c = join(&mut net, 2, "Bob");
        assert!(!in_match.contains(&id_of(&net, c)));
        net.client(c).leave();
        net.settle();
    }
    consistent(&net);
}

#[test]
fn a_start_made_before_a_change_is_not_announced() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    net.client(a).set_ready(true);
    net.settle();
    let rtts = net.rtts();
    let wire = net.host.start(1, &rtts).unwrap();
    net.client(a).set_loadout(Some([5, 4, 3, 2, 1]));
    net.settle();
    assert_eq!(net.host.announce_start(wire), Err(NotStartable::Stale));
    let wire = net.host.start(1, &rtts).unwrap();
    assert_eq!(wire.slots[1].loadout, Some([5, 4, 3, 2, 1]));
    // A ping is not a change a start depends on.
    net.host.set_ping(net.peers[a].peer, 33);
    assert_eq!(net.host.announce_start(wire), Ok(()));
    net.settle();
    consistent(&net);
}

#[test]
fn a_forced_start_seats_only_the_ready() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let c = join(&mut net, 3, "Cid");
    net.client(a).set_ready(true);
    net.client(c).have(vec!["olx-skin-x".into()]);
    net.settle();
    let (a_id, b_id, c_id) = (id_of(&net, a), id_of(&net, b), id_of(&net, c));
    assert_eq!(net.host.can_start(), Err(NotStartable::NotReady(vec![b_id, c_id])));

    let rtts = net.rtts();
    let mut wire = net.host.force_start(4, &rtts).unwrap();
    let members: Vec<Option<u8>> = wire.slots.iter().map(|x| x.member).collect();
    assert_eq!(members, [Some(host_id(&net)), Some(a_id)]);
    assert_eq!(seat(&net, b), SeatWire::Spectator);
    assert_eq!(seat(&net, c), SeatWire::Spectator);
    wire.start_checksum = 1;
    net.host.announce_start(wire.clone()).unwrap();
    net.settle();
    assert!(got(&net, b, &ClientEvent::Start(wire)), "spectators watch");
    consistent(&net);

    // Back in their seats for the next match.
    net.host.playing(0);
    net.host.match_ended(MatchEndWire { reason: "time".into(), rows: vec![], last_tick: 900 }, net.now_us);
    net.run(RESULTS_US + 1);
    net.settle();
    assert_eq!(seat(&net, b), SeatWire::Player { slot: 2 });
    assert_eq!(seat(&net, c), SeatWire::Player { slot: 3 });
    consistent(&net);
}

#[test]
fn a_forced_start_needs_ready_worms() {
    // A dedicated server whose only player is not ready.
    let host = LobbyHost::new(cfg(), ident(HOST), None, setup(0)).unwrap();
    let mut net = Net::new(host, DELAY_US);
    let a = join(&mut net, 1, "Ann");
    assert_eq!(net.host.force_start(1, &net.rtts()), Err(NotStartable::NoPlayers));
    net.client(a).set_ready(true);
    net.settle();
    assert_eq!(net.host.force_start(1, &net.rtts()), Err(NotStartable::TooFewWorms));
    assert_eq!(seat(&net, a), SeatWire::Player { slot: 0 }, "nobody moved");
    net.host.set_setup(setup(1)).unwrap();
    net.client(a).set_ready(true);
    net.settle();
    assert!(net.host.force_start(1, &net.rtts()).is_ok());
}

#[test]
fn a_dedicated_server_with_bots_only_does_not_start() {
    let host = LobbyHost::new(cfg(), ident(HOST), None, setup(4)).unwrap();
    let mut net = Net::new(host, DELAY_US);
    assert_eq!(net.host.can_start(), Err(NotStartable::NoPlayers));
    let a = join(&mut net, 1, "Ann");
    net.client(a).set_ready(true);
    net.settle();
    assert_eq!(net.host.can_start(), Ok(()));
    assert_eq!(net.host_events.iter().filter(|e| **e == HostEvent::StartRequested).count(), 1);
    net.client(a).leave();
    net.settle();
    assert_eq!(net.host.can_start(), Err(NotStartable::NoPlayers));
    net.host.events();
    assert_eq!(net.host_events.iter().filter(|e| **e == HostEvent::StartRequested).count(), 1);
}

/// `revision` changes with every change of the state and only then (a
/// dedicated server compares it instead of cloning the state each poll).
#[test]
fn the_revision_follows_the_state() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    net.settle();
    let (rev, state) = (net.host.revision(), net.host.state().clone());
    net.run(1_000_000);
    net.host.tick(net.now_us);
    assert_eq!(net.host.revision(), rev, "nothing changed");
    net.host.set_ping(net.peers[a].peer, 77);
    assert_ne!(net.host.revision(), rev, "a ping is a change");
    let rev = net.host.revision();
    net.host.set_ping(net.peers[a].peer, 77);
    assert_eq!(net.host.revision(), rev, "the same ping is not");
    net.client(a).set_ready(true);
    net.settle();
    assert_ne!(net.host.revision(), rev, "ready is a change");
    assert_ne!(net.host.state(), &state);
}

#[test]
fn pings_show_in_the_lobby() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    net.host.set_ping(net.peers[a].peer, 42);
    net.host.set_ping(net.peers[b].peer, 1_000_000);
    net.settle();
    assert_eq!(member(&net, a).ping_ms, Some(42));
    assert_eq!(member(&net, b).ping_ms, Some(u16::MAX));
    assert_eq!(member_by_id(&net, host_id(&net)).ping_ms, None);
    consistent(&net);
}

#[test]
fn state_changes_are_coalesced() {
    let mut net = lobby();
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    let lobbies =
        |net: &Net| net.peers[a].received.iter().filter(|m| matches!(m, ToClient::Lobby(_))).count();
    let before = lobbies(&net);
    // A burst arriving together is one Lobby to everyone.
    for i in 0..5 {
        net.client(b).set_look(look(&format!("Bob {i}")));
    }
    net.client(b).set_ready(true);
    net.client(b).have(vec![]);
    net.client(b).set_spectate(false);
    net.settle();
    assert_eq!(lobbies(&net), before + 1);
    assert_eq!(member(&net, b).look.name, "Bob 4");
    consistent(&net);
}

#[test]
fn a_client_leaves_a_host_that_breaks_the_protocol() {
    let lobby_state = |ids: &[u8]| LobbyState {
        name: "Office".into(),
        phase: Phase::Gathering,
        setup: setup(0),
        members: ids
            .iter()
            .map(|&id| Member {
                id,
                key: key(id + 1),
                look: look("X"),
                seat: SeatWire::Spectator,
                ready: false,
                missing: vec![],
                ping_ms: None,
                admin: false,
            })
            .collect(),
    };
    let challenge = ToClient::Challenge { nonce: [1; 32], host_key: key(HOST), needs_password: false };
    let fresh = |expected| {
        let mut c = LobbyClient::new(ident(1), look("Ann"), None, expected);
        c.connected();
        c
    };
    // A welcome naming a member that is not in its lobby.
    let mut c = fresh(None);
    c.receive(challenge.clone(), [0; 32]);
    c.receive(ToClient::Welcome { you: 9, lobby: lobby_state(&[0, 1]) }, [0; 32]);
    assert_eq!(c.events(), vec![ClientEvent::Closed]);
    assert!(!c.joined());
    // A lobby without this member, with no kick.
    let mut c = fresh(Some(key(HOST)));
    c.receive(challenge.clone(), [0; 32]);
    assert_eq!(c.outgoing().len(), 2, "Hello and Prove to the expected host");
    c.receive(ToClient::Welcome { you: 1, lobby: lobby_state(&[0, 1]) }, [0; 32]);
    c.receive(ToClient::Lobby(lobby_state(&[0])), [0; 32]);
    assert_eq!(c.events(), vec![ClientEvent::Welcome { you: 1 }, ClientEvent::Closed]);
    assert!(!c.joined());
    // A challenge from another host than expected is not answered.
    let mut c = fresh(Some(key(7)));
    c.receive(challenge, [0; 32]);
    assert!(c.outgoing().is_empty());
    assert_eq!(c.events(), vec![ClientEvent::WrongHost { key: key(HOST) }]);
}

#[test]
fn worm_counts_bound_the_start() {
    let mut net = lobby_with(cfg(), 0);
    assert_eq!(net.host.can_start(), Err(NotStartable::TooFewWorms));
    net.host.set_setup(setup(1)).unwrap();
    assert_eq!(net.host.can_start(), Ok(()), "the host and a bot");
    net.host.set_setup(setup(MAX_BOTS)).unwrap();
    assert_eq!(net.host.can_start(), Ok(()), "32 worms");
    let a = join(&mut net, 1, "Ann");
    net.client(a).set_ready(true);
    net.settle();
    assert_eq!(net.host.can_start(), Err(NotStartable::TooManyWorms));
    // Not-ready players are named to everyone when a start fails (a
    // change of bots alone keeps Ann's ready mark: she takes it back).
    net.host.set_setup(setup(0)).unwrap();
    net.client(a).set_ready(false);
    net.settle();
    assert_eq!(net.host.start(1, &BTreeMap::new()), Err(NotStartable::NotReady(vec![id_of(&net, a)])));
    net.settle();
    assert!(got(&net, a, &ClientEvent::Notice(Notice::NotReady(vec![id_of(&net, a)]))));
    consistent(&net);
}

#[test]
fn a_dedicated_server_has_no_player_but_a_key() {
    let host = LobbyHost::new(cfg(), ident(HOST), None, setup(0)).unwrap();
    let mut net = Net::new(host, DELAY_US);
    let a = join(&mut net, 1, "Ann");
    let b = join(&mut net, 2, "Bob");
    assert!(
        matches!(net.peers[a].received[0], ToClient::Challenge { host_key, .. } if host_key == key(HOST))
    );
    assert_eq!(seat(&net, a), SeatWire::Player { slot: 0 });
    assert_eq!(seat(&net, b), SeatWire::Player { slot: 1 });
    net.host.chat("welcome");
    net.settle();
    assert!(got(&net, b, &ClientEvent::Notice(Notice::Message("welcome".into()))));
    for x in [a, b] {
        net.client(x).set_ready(true);
    }
    net.settle();
    let wire = net.start(3, 4).unwrap();
    assert_eq!(wire.input_delay.len(), 2);
    consistent(&net);
}

// ---- robustness: any sequence of messages and host actions ----

/// Peers in the proptest: more than a full lobby.
const PEERS: u8 = 56;

#[derive(Clone, Debug)]
enum Op {
    Join(u8, Look),
    /// Every peer joins with the longest look and missing list.
    Crowd,
    Connect(u8),
    Disconnect(u8),
    Send(u8, ToHost),
    Kick(u8, bool),
    Move(u8, bool),
    Setup(usize, bool),
    Start(bool),
    Playing(u32),
    Ended,
    Wait(u64),
}

fn to_host() -> impl Strategy<Value = ToHost> {
    let long_name = "\u{1F600}".repeat(MAX_NAME);
    let name = prop_oneof![
        Just("Ann".to_string()),
        Just("ann".to_string()),
        Just("Host".to_string()),
        Just(long_name),
        "[a-zA-Z]{1,20}",
    ];
    let skin = prop_oneof![Just("olx-skin-afro".to_string()), "[a-z0-9-]{64}"];
    let look = (name, skin).prop_map(|(name, skin)| Look { name, colour: [1, 2, 3], skin });
    prop_oneof![
        (0u8..PEERS, look.clone()).prop_map(|(k, l)| ToHost::Hello {
            key: key(k + 1),
            look: l,
            password: None
        }),
        any::<u8>().prop_map(|b| ToHost::Prove { signature: [b; 64] }),
        look.prop_map(ToHost::SetLook),
        any::<bool>().prop_map(ToHost::Ready),
        any::<bool>().prop_map(ToHost::Spectate),
        Just(ToHost::Loadout(Some([1, 2, 3, 4, 5]))),
        "[a-z ]{0,200}".prop_map(ToHost::Chat),
        prop::collection::vec("[a-z]{1,8}|[a-z0-9-]{64}", 0..=MAX_MISSING)
            .prop_map(|missing| ToHost::Have { missing }),
        (0u8..PEERS).prop_map(|k| ToHost::Admin(AdminCommand::Kick(key(k + 1)))),
        (0u8..PEERS).prop_map(|k| ToHost::Admin(AdminCommand::Ban(key(k + 1)))),
        Just(ToHost::Admin(AdminCommand::Next)),
        Just(ToHost::Bye),
    ]
}

/// A look as long as the wire allows.
fn long_look(p: u8) -> Look {
    let mut skin = format!("skin-{p}-");
    skin.extend(std::iter::repeat_n('x', MAX_ID - skin.len()));
    Look { name: format!("{p}{}", "\u{1F600}".repeat(MAX_NAME - 2)), colour: [9, 9, 9], skin }
}

/// A setup as large as the wire allows.
fn long_setup(bots: usize) -> MatchSetupWire {
    let mut s = setup(0);
    s.packs = (0..MAX_PACKS).map(|i| (format!("{i}{}", "p".repeat(MAX_ID - 1)), "ab".repeat(32))).collect();
    s.restrictions = "r".repeat(MAX_RESTRICTIONS);
    s.bots = (0..bots)
        .map(|i| BotWire { difficulty: 3, personality: 2, loadout: 1, look: long_look(i as u8) })
        .collect();
    s
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (0u8..PEERS, prop::bool::ANY).prop_map(|(p, long)| Op::Join(p, if long { long_look(p) } else { look("Ann") })),
        1 => (0u8..PEERS).prop_map(Op::Connect),
        1 => (0u8..PEERS).prop_map(Op::Disconnect),
        4 => ((0u8..PEERS), to_host()).prop_map(|(p, m)| Op::Send(p, m)),
        1 => (0u8..64, any::<bool>()).prop_map(|(m, b)| Op::Kick(m, b)),
        1 => (0u8..64, any::<bool>()).prop_map(|(m, b)| Op::Move(m, b)),
        1 => (0usize..=MAX_BOTS, any::<bool>()).prop_map(|(b, long)| Op::Setup(b, long)),
        1 => any::<bool>().prop_map(Op::Start),
        1 => any::<u32>().prop_map(Op::Playing),
        1 => Just(Op::Ended),
        1 => (0u64..12_000_000).prop_map(Op::Wait),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Whatever arrives, the host never panics and never sends a message
    /// that fails to decode: its state keeps every wire invariant.
    #[test]
    fn random_messages_keep_the_lobby_valid(
        max_players in 1u8..=40,
        max_spectators in 0u8..=20,
        ops in prop::collection::vec(op(), 1..120),
        crowd_at in prop::option::of(0usize..120),
    ) {
        let cfg = HostConfig { max_players, max_spectators, admins: vec![key(1)], ..cfg() };
        let mut host = LobbyHost::new(cfg, ident(HOST), Some(look("Host")), setup(0)).unwrap();
        let mut now = 0u64;
        let mut nonces: BTreeMap<u32, [u8; 32]> = BTreeMap::new();
        let mut serial = 0u8;
        for (i, op) in ops.into_iter().enumerate() {
            let op = if crowd_at == Some(i) { Op::Crowd } else { op };
            match op {
                Op::Join(p, _) | Op::Connect(p) => {
                    serial = serial.wrapping_add(1);
                    let nonce = [serial; 32];
                    let peer = u32::from(p);
                    host.connected(peer, ip(p), now, nonce);
                    nonces.insert(peer, nonce);
                    if let Op::Join(p, look) = op {
                        let me = ident(p + 1);
                        host.receive(peer, ToHost::Hello { key: me.public(), look, password: None }, now);
                        let signature = me.sign(&challenge_message(&nonce, &key(HOST)), [0; 32]);
                        host.receive(peer, ToHost::Prove { signature }, now);
                    }
                }
                Op::Crowd => {
                    for p in 0..PEERS {
                        serial = serial.wrapping_add(1);
                        let (peer, nonce, me) = (u32::from(p), [serial; 32], ident(p + 1));
                        host.connected(peer, ip(p), now, nonce);
                        host.receive(peer, ToHost::Hello { key: me.public(), look: long_look(p), password: None }, now);
                        let signature = me.sign(&challenge_message(&nonce, &key(HOST)), [0; 32]);
                        host.receive(peer, ToHost::Prove { signature }, now);
                        let missing = (0..MAX_MISSING).map(|i| format!("{i}{}", "m".repeat(MAX_ID - 1))).collect();
                        host.receive(peer, ToHost::Have { missing }, now);
                    }
                }
                Op::Disconnect(p) => host.disconnected(u32::from(p)),
                Op::Send(p, m) => host.receive(u32::from(p), m, now),
                Op::Kick(m, b) => host.kick(m, b),
                Op::Move(m, b) => host.move_seat(m, b),
                Op::Setup(bots, long) => {
                    let gathering = host.state().phase == Phase::Gathering;
                    let set = host.set_setup(if long { long_setup(bots) } else { setup(bots) });
                    assert_eq!(set.is_ok(), gathering, "{set:?}");
                }
                Op::Start(force) => {
                    let rtts = BTreeMap::from([(1u32, 900u32), (2, 0)]);
                    let started = if force { host.force_start(1, &rtts) } else { host.start(1, &rtts) };
                    if let Ok(mut w) = started {
                        w.start_checksum = 5;
                        let wire = encode_to_client(&ToClient::Start(w.clone()));
                        prop_assert!(decode_to_client(&wire).is_ok());
                        host.announce_start(w).unwrap();
                    }
                }
                Op::Playing(t) => host.playing(t),
                Op::Ended => host.match_ended(MatchEndWire { reason: "x\u{7}".repeat(150), rows: vec![(0, 0, 0, 0, 0); 40], last_tick: 900 }, now),
                Op::Wait(us) => {
                    now += us;
                    host.tick(now);
                }
            }
            // A broadcast is a run of one message to many peers: check it once.
            let mut last = None;
            for (_, m) in host.outgoing() {
                if last.as_ref() != Some(&m) {
                    prop_assert_eq!(decode_to_client(&encode_to_client(&m)), Ok(m.clone()));
                    last = Some(m);
                }
            }
            host.events();
            let state = host.state().clone();
            let players = state.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).count();
            prop_assert!(players <= usize::from(max_players.clamp(1, 32)));
            prop_assert!(state.members.len() <= usize::from(max_players.clamp(1, 32)) + usize::from(max_spectators.min(16)));
            let slots_in_range = state.members.iter().all(|m| match m.seat {
                SeatWire::Player { slot } => slot < max_players.clamp(1, 32),
                _ => true,
            });
            prop_assert!(slots_in_range);
            let msg = ToClient::Lobby(state);
            prop_assert_eq!(decode_to_client(&encode_to_client(&msg)), Ok(msg));
        }
    }
}
