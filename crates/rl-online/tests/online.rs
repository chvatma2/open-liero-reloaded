//! Online play over localhost (networking spec §4): a host peer and its
//! clients join, start, play, end and make their replays over real
//! connections. Classic on Dirt Level from `RL_CONTENT_DIR` (skipped with a
//! notice without it); every wait is bounded, every test under 20 s.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_bots::BotConfig;
use rl_content::{Content, DEFAULT_PRESET};
use rl_lobby::{HostConfig, Identity};
use rl_modes::DeathmatchSettings;
use rl_net::endpoint::EndpointConfig;
use rl_online::matches::{bot_wire, rules_wire, setup_packs};
use rl_online::peer::{MISMATCH, SETUP_REFUSED};
use rl_online::{OnlineEvent, OnlinePeer, PeerRole};
use rl_proto::{Look, MatchSetupWire, Phase, PublicKey, SeatWire};
use rl_replay::Replay;
use rl_sim::Keys;

const MOD: &str = "olx-classic";
const LEVEL: &str = "olx-level-dirt-level";

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).unwrap())
}

fn identity() -> Identity {
    loop {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>().unwrap()) {
            return id;
        }
    }
}

fn look(name: &str) -> Look {
    Look { name: name.into(), colour: [200, 80, 40], skin: "olx-skin-default".into() }
}

/// Classic on Dirt Level, one bot, ending after `minutes`.
fn setup(content: &Content, minutes: f32) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        packs: setup_packs(content, MOD, LEVEL).unwrap(),
        rules: rules_wire(&DeathmatchSettings { time_limit: minutes, ..DeathmatchSettings::default() }),
        bonuses: false,
        loading_time: 100,
        restrictions: DEFAULT_PRESET.into(),
        bots: vec![bot_wire(&BotConfig::default(), look("Bot"))],
    }
}

fn endpoint() -> EndpointConfig {
    EndpointConfig { port: 0, ..EndpointConfig::default() }
}

/// One peer of the test, with everything it reported.
struct Peer {
    p: Option<OnlinePeer>,
    key: PublicKey,
    events: Vec<OnlineEvent>,
    /// Keys pressed so far: they change every 25 ticks.
    pressed: u32,
}

impl Peer {
    fn new(p: OnlinePeer, key: PublicKey) -> Peer {
        Peer { p: Some(p), key, events: Vec::new(), pressed: 0 }
    }

    fn peer(&mut self) -> &mut OnlinePeer {
        self.p.as_mut().expect("the peer is there")
    }

    fn poll(&mut self, now: Instant) {
        let Some(p) = &mut self.p else { return };
        let pressed = &mut self.pressed;
        let seed = self.key[0] as u32;
        let mut keys = || {
            *pressed += 1;
            let k = (*pressed / 25).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 24;
            Keys((k as u8) & !Keys::CHANGE)
        };
        self.events.extend(p.poll(now, &mut keys, &mut |_, _, _| {}));
    }

    fn count(&self, f: impl Fn(&OnlineEvent) -> bool) -> usize {
        self.events.iter().filter(|e| f(e)).count()
    }

    fn has(&self, f: impl Fn(&OnlineEvent) -> bool) -> bool {
        self.count(f) > 0
    }

    fn started(&self) -> Vec<Option<u8>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::MatchStarting { my_slot, .. } => Some(*my_slot),
                _ => None,
            })
            .collect()
    }

    fn replays(&self) -> Vec<&Vec<u8>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::ReplayReady(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    fn member_id(&mut self, key: PublicKey) -> Option<u8> {
        let lobby = self.peer().lobby();
        lobby.state()?.members.iter().find(|m| m.key == key).map(|m| m.id)
    }
}

/// The host and its clients, polled together on the real clock.
struct Net {
    content_dir: String,
    port: u16,
    peers: Vec<Peer>,
    begun: Instant,
}

impl Net {
    fn new(content: Content, minutes: f32) -> Net {
        Net::with_admins(content, minutes, Vec::new())
    }

    fn with_admins(content: Content, minutes: f32, admins: Vec<PublicKey>) -> Net {
        let content_dir = std::env::var("RL_CONTENT_DIR").unwrap();
        let me = identity();
        let key = me.public();
        let cfg = HostConfig {
            name: "Test lobby".into(),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins,
        };
        let setup = setup(&content, minutes);
        let host = OnlinePeer::start(PeerRole::Host(cfg), endpoint(), me, look("Host"), content, Some(setup))
            .unwrap();
        let port = host.local_port();
        Net { content_dir, port, peers: vec![Peer::new(host, key)], begun: Instant::now() }
    }

    fn content(&self) -> Content {
        Content::open(std::path::Path::new(&self.content_dir)).unwrap()
    }

    /// A client joining as `me`; its index.
    fn join_as(&mut self, me: Identity, name: &str) -> usize {
        let key = me.public();
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let role = PeerRole::Client { addr, password: None };
        let p = OnlinePeer::start(role, endpoint(), me, look(name), self.content(), None).unwrap();
        self.peers.push(Peer::new(p, key));
        self.peers.len() - 1
    }

    fn join(&mut self, name: &str) -> usize {
        self.join_as(identity(), name)
    }

    fn poll(&mut self) {
        let now = Instant::now();
        for p in &mut self.peers {
            p.poll(now);
        }
    }

    /// Polls until `done` holds; panics after `limit` with `what`.
    fn run_until(&mut self, limit: Duration, what: &str, mut done: impl FnMut(&mut Net) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < limit, "{what} within {limit:?}");
            self.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn host(&mut self) -> &mut Peer {
        &mut self.peers[0]
    }

    /// Members in the host's lobby.
    fn members(&mut self) -> usize {
        self.host().peer().lobby().state().map_or(0, |s| s.members.len())
    }

    /// Joins clients and waits until the host's lobby lists them.
    fn join_all(&mut self, names: &[&str]) -> Vec<usize> {
        let before = self.members();
        let ids: Vec<usize> = names.iter().map(|n| self.join(n)).collect();
        let want = before + names.len();
        self.run_until(Duration::from_secs(5), "the clients join", |n| {
            n.members() == want && ids.iter().all(|&c| n.peers[c].has(|e| matches!(e, OnlineEvent::Lobby(_))))
        });
        ids
    }

    /// Readies the clients and waits until the host's lobby can start.
    fn ready(&mut self, clients: &[usize]) {
        for &c in clients {
            self.peers[c].peer().lobby().set_ready(true);
        }
        self.run_until(Duration::from_secs(3), "everyone is ready", |n| {
            n.host().peer().lobby().state().is_some_and(|s| {
                s.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).all(|m| m.ready)
            })
        });
    }

    fn host_tick(&mut self) -> u32 {
        self.host().peer().match_session().map_or(0, |s| s.current_tick())
    }

    fn assert_quick(&self) {
        assert!(self.begun.elapsed() < Duration::from_secs(20), "the test took {:?}", self.begun.elapsed());
    }
}

fn ended(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::MatchEnded(_))
}

fn replay_ready(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::ReplayReady(_))
}

#[test]
fn a_host_and_two_clients_play_a_short_match() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let clients = net.join_all(&["Anna", "Bert"]);
    net.ready(&clients);
    net.host().peer().lobby().start(7).unwrap();
    net.run_until(Duration::from_secs(16), "every peer ends the match and makes its replay", |n| {
        n.peers.iter().all(|p| p.has(ended) && p.has(replay_ready))
    });
    // Players by seat (the clients' in the order they were welcomed), then the bot.
    assert_eq!(net.peers[0].started(), vec![Some(0)]);
    let mut slots: Vec<Option<u8>> = net.peers[1..].iter().flat_map(Peer::started).collect();
    slots.sort();
    assert_eq!(slots, vec![Some(1), Some(2)]);
    for p in &net.peers {
        assert_eq!(p.count(ended), 1);
        assert!(p.has(|e| matches!(e, OnlineEvent::Confirmed(_))), "confirmed ticks reported");
        assert!(p.has(|e| matches!(e, OnlineEvent::Ticked(_))), "simulated ticks reported");
        assert!(!p.has(|e| matches!(e, OnlineEvent::Notice(n) if n.contains("differs"))), "no desync");
    }
    // Every peer confirmed the same inputs and checksums up to the match's last tick.
    let records: Vec<_> = net.peers.iter_mut().map(|p| p.peer().last_record().cloned().unwrap()).collect();
    let r = &records[0];
    assert!(r.rows.len() > 600 + 300, "the match and its results: {} ticks", r.rows.len());
    assert!(r.checksums.len() >= 90, "{} checksums", r.checksums.len());
    assert!(
        r.rows.iter().any(|row| row[1] != 0) && r.rows.iter().any(|row| row[2] != 0),
        "the clients played"
    );
    for other in &records[1..] {
        assert_eq!(other.rows, r.rows);
        assert_eq!(other.checksums, r.checksums);
    }
    // The same replay everywhere, and it plays.
    let bytes: Vec<&Vec<u8>> = net.peers.iter().map(|p| p.replays()[0]).collect();
    assert!(bytes.iter().all(|b| *b == bytes[0]), "equal replays");
    let replay = Replay::decode(bytes[0]).unwrap();
    assert_eq!(replay.ticks() as usize, r.rows.len());
    net.content().play_replay(&replay).unwrap();
    // Back to the lobby.
    net.run_until(Duration::from_secs(1), "the lobby shows the results", |n| {
        n.peers[1].peer().lobby().state().is_some_and(|s| s.phase == Phase::Results)
    });
    net.assert_quick();
}

#[test]
fn a_late_joiner_watches_then_plays_the_next_match() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.04);
    let a = net.join_all(&["Anna"])[0];
    net.ready(&[a]);
    net.host().peer().lobby().start(3).unwrap();
    net.run_until(Duration::from_secs(3), "the match runs", |n| n.host_tick() >= 50);
    let b = net.join("Bert");
    net.run_until(Duration::from_secs(4), "the joiner watches confirmed ticks", |n| {
        n.peers[b].events.iter().any(|e| match e {
            OnlineEvent::Confirmed(c) => c.iter().any(|t| t.tick > 100),
            _ => false,
        })
    });
    assert_eq!(net.peers[b].started(), vec![None], "it watches");
    // The state it watches from is reported before the map it repaints.
    let events = &net.peers[b].events;
    let loaded = events.iter().position(|e| matches!(e, OnlineEvent::StateLoaded { .. }));
    let loaded = loaded.expect("the joiner reports the state it loaded");
    assert!(
        events[loaded..].iter().any(|e| matches!(e, OnlineEvent::Restored(a) if !a.is_empty())),
        "the loaded state's map comes after it"
    );
    assert!(net.peers[b].peer().match_session().is_some_and(|s| s.input_log().origin() > 0), "from a state");
    net.run_until(Duration::from_secs(8), "the match ends everywhere", |n| {
        n.peers.iter().all(|p| p.has(ended))
    });
    // Gathering again after the results: the joiner takes a free seat.
    let key = net.peers[b].key;
    net.run_until(Duration::from_secs(7), "the joiner is seated", |n| {
        n.host().peer().lobby().state().is_some_and(|s| {
            s.phase == Phase::Gathering
                && s.members.iter().any(|m| m.key == key && matches!(m.seat, SeatWire::Player { .. }))
        })
    });
    net.ready(&[a, b]);
    net.host().peer().lobby().start(4).unwrap();
    net.run_until(Duration::from_secs(2), "the second match starts for the joiner", |n| {
        n.peers[b].started().len() == 2
    });
    assert_eq!(net.peers[b].started()[1], Some(2), "it plays");
    net.run_until(Duration::from_secs(2), "it plays ticks", |n| {
        n.peers[b].peer().match_session().is_some_and(|s| s.current_tick() > 20)
    });
    assert!(net.peers[a].replays().len() == 1, "the first match's replay");
    net.assert_quick();
}

#[test]
fn a_client_with_other_content_is_refused_at_start() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let clients = net.join_all(&["Anna", "Bert"]);
    let (a, b) = (clients[0], clients[1]);
    net.peers[b].peer().test_seed_override(Some(99));
    net.ready(&clients);
    net.host().peer().lobby().start(5).unwrap();
    net.run_until(Duration::from_secs(4), "the odd client is refused and the host hears it left", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Refused(r) if r == MISMATCH))
            && n.peers[0].has(|e| matches!(e, OnlineEvent::Left(name) if name == "Bert"))
    });
    assert!(net.peers[b].peer().is_closed());
    net.run_until(Duration::from_secs(2), "the other client starts", |n| !n.peers[a].started().is_empty());
    assert!(matches!(net.peers[a].started()[..], [Some(1 | 2)]), "the other client plays");
    net.run_until(Duration::from_secs(2), "the match goes on", |n| n.host_tick() > 100);
    assert!(!net.peers[a].has(|e| matches!(e, OnlineEvent::Refused(_) | OnlineEvent::Disconnected(_))));
    net.assert_quick();
}

#[test]
fn a_client_vanishes_mid_match() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let clients = net.join_all(&["Anna", "Bert"]);
    let (a, b) = (clients[0], clients[1]);
    net.ready(&clients);
    net.host().peer().lobby().start(9).unwrap();
    net.run_until(Duration::from_secs(4), "the match runs", |n| n.host_tick() >= 200);
    let gone_at = net.host_tick();
    let (Some(sa), Some(sb)) = (net.peers[a].started()[0], net.peers[b].started()[0]) else {
        panic!("both play")
    };
    let (sa, sb) = (usize::from(sa), usize::from(sb));
    net.peers[b].p = None;
    net.run_until(Duration::from_secs(14), "the host and the other client finish", |n| {
        [0, a].iter().all(|&i| n.peers[i].has(ended) && n.peers[i].has(replay_ready))
    });
    assert!(net.peers[0].has(|e| matches!(e, OnlineEvent::Left(name) if name == "Bert")));
    let host = net.peers[0].peer().last_record().cloned().unwrap();
    let anna = net.peers[a].peer().last_record().cloned().unwrap();
    assert_eq!(host.rows, anna.rows);
    assert_eq!(host.checksums, anna.checksums);
    assert_eq!(net.peers[0].replays(), net.peers[a].replays());
    let rows = &host.rows;
    assert!(rows[..gone_at as usize].iter().any(|r| r[sb] != 0), "it played");
    // Within a second of leaving, its slot is `Keys(0)` to the end.
    let from = gone_at as usize + 100;
    assert!(rows.len() > from + 300, "{} rows, gone at {gone_at}", rows.len());
    assert!(rows[from..].iter().all(|r| r[sb] == 0), "the vanished slot presses nothing");
    assert!(rows[from..].iter().any(|r| r[sa] != 0), "the other client plays on");
    net.assert_quick();
}

#[test]
fn kicked_players_cannot_rejoin() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let me = identity();
    let secret = me.secret();
    let a = net.join_as(me, "Anna");
    net.run_until(Duration::from_secs(5), "the client joins", |n| n.members() == 2);
    let key = net.peers[a].key;
    let id = net.host().member_id(key).unwrap();
    net.host().peer().lobby().kick(id, true);
    net.run_until(Duration::from_secs(3), "the client is kicked", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Kicked { banned: true }))
    });
    assert!(net.peers[a].peer().is_closed());
    net.run_until(Duration::from_secs(2), "the host's lobby lost it", |n| n.members() == 1);
    // The same key from the same address: refused.
    let again = net.join_as(Identity::from_secret(secret).unwrap(), "Anna");
    net.run_until(Duration::from_secs(5), "the rejoin is refused", |n| {
        n.peers[again].has(|e| matches!(e, OnlineEvent::Refused(_)))
    });
    let refusal = net.peers[again].events.iter().find_map(|e| match e {
        OnlineEvent::Refused(r) => Some(r.clone()),
        _ => None,
    });
    assert_eq!(refusal.as_deref(), Some("you are banned from this lobby"));
    assert!(net.peers[again].peer().is_closed());
    for _ in 0..50 {
        net.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(net.members(), 1, "never admitted");
    net.assert_quick();
}

#[test]
fn an_admins_setup_is_taken_or_refused_with_a_notice() {
    let Some(content) = content() else { return };
    let admin = identity();
    let mut net = Net::with_admins(content, 0.1, vec![admin.public()]);
    let a = net.join_as(admin, "Anna");
    net.run_until(Duration::from_secs(5), "the admin joins", |n| {
        n.members() == 2 && n.peers[a].has(|e| matches!(e, OnlineEvent::Lobby(_)))
    });
    // A mod the host does not have: refused, the admin is told.
    let mut odd = setup(&net.content(), 0.1);
    odd.mod_id = "olx-no-such-mod".into();
    net.peers[a].peer().lobby().set_setup(odd).unwrap();
    net.run_until(Duration::from_secs(3), "the admin hears the refusal", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Notice(t) if t.starts_with(SETUP_REFUSED)))
    });
    assert!(net.host().peer().lobby().state().is_some_and(|s| s.setup.mod_id == MOD), "unchanged");
    // A setup the host can play: taken.
    let longer = setup(&net.content(), 0.5);
    net.peers[a].peer().lobby().set_setup(longer.clone()).unwrap();
    net.run_until(Duration::from_secs(3), "the host takes it", |n| {
        n.peers[a].peer().lobby().state().is_some_and(|s| s.setup == longer)
    });
    assert_eq!(
        net.peers[a].count(|e| matches!(e, OnlineEvent::Notice(t) if t.starts_with(SETUP_REFUSED))),
        1
    );
    net.assert_quick();
}
