//! Online play over localhost (networking spec §4): a host peer and its
//! clients join, start, play, end and make their replays over real
//! connections. Classic on Dirt Level from `RL_CONTENT_DIR` (skipped with a
//! notice without it); every wait is bounded, every test under 20 s.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rl_bots::BotConfig;
use rl_content::{Content, DEFAULT_PRESET, DownloadCache};
use rl_lobby::{HostConfig, Identity};
use rl_modes::DeathmatchSettings;
use rl_net::endpoint::EndpointConfig;
use rl_netcode::{Driver, WorldSim};
use rl_online::bundle::GameBundle;
use rl_online::content::{Download, DownloadState};
use rl_online::matches::{bot_wire, rules_wire, setup_packs};
use rl_online::peer::{MISMATCH, SETUP_REFUSED, WRONG_HOST};
use rl_online::{OnlineEvent, OnlinePeer, PeerContent, PeerRole};
use rl_proto::{Look, MatchSetupWire, Phase, PublicKey, SeatWire};
use rl_replay::Replay;
use rl_sim::Keys;
use tempfile::TempDir;

use crate::common::{TEST_SKIN, cache, partial_content, peer_content, with_skin, with_test_skin};

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
    Look {
        name: name.into(),
        colour: [200, 80, 40],
        skin: "olx-skin-default".into(),
        skin_hash: rl_proto::NO_HASH.into(),
    }
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
    /// Its download cache.
    cache: Option<Arc<Mutex<DownloadCache>>>,
}

impl Peer {
    fn new(p: OnlinePeer, key: PublicKey) -> Peer {
        Peer { p: Some(p), key, events: Vec::new(), pressed: 0, cache: None }
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
    /// Caches and partial contents, removed at the end.
    dirs: Vec<TempDir>,
}

impl Net {
    fn new(content: Content, minutes: f32) -> Net {
        Net::with_admins(content, minutes, Vec::new())
    }

    fn with_admins(content: Content, minutes: f32, admins: Vec<PublicKey>) -> Net {
        let cfg = HostConfig {
            name: "Test lobby".into(),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins,
        };
        Net::hosted(content, minutes, PeerRole::Host(cfg))
    }

    /// A dedicated server's lobby: no local player (`PeerRole::Server`).
    fn server(content: Content, minutes: f32) -> Net {
        let cfg = HostConfig {
            name: "Server".into(),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins: vec![],
        };
        Net::hosted(content, minutes, PeerRole::Server(cfg))
    }

    fn hosted(content: Content, minutes: f32, role: PeerRole) -> Net {
        let setup = setup(&content, minutes);
        Net::hosted_with(content, setup, role)
    }

    fn hosted_with(content: Content, setup: MatchSetupWire, role: PeerRole) -> Net {
        let content_dir = std::env::var("RL_CONTENT_DIR").unwrap();
        let me = identity();
        let key = me.public();
        let (dir, cache) = cache();
        let host =
            OnlinePeer::start(role, endpoint(), me, look("Host"), content, Some(setup), peer_content(&cache))
                .unwrap();
        let port = host.local_port();
        let mut peer = Peer::new(host, key);
        peer.cache = Some(cache);
        Net { content_dir, port, peers: vec![peer], dirs: vec![dir] }
    }

    fn content(&self) -> Content {
        Content::open(std::path::Path::new(&self.content_dir)).unwrap()
    }

    /// A client joining as `me`, with every pack and a cache of its own;
    /// its index.
    fn join_as(&mut self, me: Identity, name: &str) -> usize {
        let content = self.content();
        self.join_with(me, look(name), content, None)
    }

    /// A client joining as `me` with `look` and `content`, and `cfg` or a
    /// cache of its own; its index.
    fn join_with(&mut self, me: Identity, look: Look, content: Content, cfg: Option<PeerContent>) -> usize {
        self.join_expecting_with(me, look, content, cfg, None)
    }

    /// A client expecting the host's key `expected` (an invite's), with
    /// every pack and a cache of its own; its index.
    fn join_expecting(&mut self, name: &str, expected: PublicKey) -> usize {
        let content = self.content();
        self.join_expecting_with(identity(), look(name), content, None, Some(expected))
    }

    fn join_expecting_with(
        &mut self,
        me: Identity,
        look: Look,
        content: Content,
        cfg: Option<PeerContent>,
        expected_host: Option<PublicKey>,
    ) -> usize {
        let key = me.public();
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let role = PeerRole::Client { addr, password: None, expected_host };
        let mut own = None;
        let cfg = cfg.unwrap_or_else(|| {
            let (dir, cache) = cache();
            self.dirs.push(dir);
            own = Some(Arc::clone(&cache));
            peer_content(&cache)
        });
        let p = OnlinePeer::start(role, endpoint(), me, look, content, None, cfg).unwrap();
        let mut peer = Peer::new(p, key);
        peer.cache = own;
        self.peers.push(peer);
        self.peers.len() - 1
    }

    /// A client whose content lacks the packs `leave_out` (a cache of
    /// its own); its index.
    fn join_lacking(&mut self, name: &str, leave_out: &[&str]) -> usize {
        let (dir, content) = partial_content(leave_out);
        self.dirs.push(dir);
        self.join_with(identity(), look(name), content, None)
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
        self.run_until(Duration::from_secs(10), "the clients join", |n| {
            n.members() == want && ids.iter().all(|&c| n.peers[c].has(|e| matches!(e, OnlineEvent::Lobby(_))))
        });
        ids
    }

    /// Readies the clients and waits until the host's lobby can start.
    fn ready(&mut self, clients: &[usize]) {
        for &c in clients {
            self.peers[c].peer().lobby().set_ready(true);
        }
        self.run_until(Duration::from_secs(10), "everyone is ready", |n| {
            n.host().peer().lobby().state().is_some_and(|s| {
                s.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).all(|m| m.ready)
            })
        });
    }

    fn host_tick(&mut self) -> u32 {
        self.host().peer().match_session().map_or(0, |s| s.current_tick())
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
    net.run_until(Duration::from_secs(32), "every peer ends the match and makes its replay", |n| {
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
    net.run_until(Duration::from_secs(10), "the lobby shows the results", |n| {
        n.peers[1].peer().lobby().state().is_some_and(|s| s.phase == Phase::Results)
    });
}

/// A dedicated server without bots has no slot of its own: its clients'
/// inputs reach it before it simulates their ticks, so it confirms ticks it
/// has not simulated. Its match still ends only once the last tick is
/// simulated: its record holds the last tick's checksum, as the clients'.
#[test]
fn a_server_without_bots_records_the_last_checksum() {
    let Some(content) = content() else { return };
    // 2.997 s: the result at tick 299, the last tick 599, a checksum tick.
    let mut setup = setup(&content, 0.04995);
    setup.bots.clear();
    let cfg = HostConfig {
        name: "Server".into(),
        password: None,
        max_players: 8,
        max_spectators: 4,
        admins: vec![],
    };
    let mut net = Net::hosted_with(content, setup, PeerRole::Server(cfg));
    let clients = net.join_all(&["Anna", "Bert"]);
    net.ready(&clients);
    net.host().peer().lobby().start(5).unwrap();
    net.run_until(Duration::from_secs(30), "every peer keeps its record", |n| {
        n.peers.iter_mut().all(|p| p.peer().last_record().is_some())
    });
    let records: Vec<_> = net.peers.iter_mut().map(|p| p.peer().last_record().cloned().unwrap()).collect();
    let r = &records[0];
    // A checksum of every checksum tick of the record, up to its last.
    let n = r.rows.len() as u32;
    assert_eq!(n, 600, "the result at tick 299 and 300 more");
    let want: Vec<u32> = (0..n).filter(|t| (t + 1) % rl_netcode::CHECKSUM_EVERY == 0).collect();
    let ticks: Vec<u32> = r.checksums.iter().map(|c| c.0).collect();
    assert_eq!(ticks, want, "the server's checksums of {n} ticks");
    for other in &records[1..] {
        assert_eq!(other.rows, r.rows);
        assert_eq!(other.checksums, r.checksums);
    }
}

/// `/next` on a dedicated server without bots (`end_match`) at a moment it
/// has confirmed a checksum tick it has not simulated yet: the match ends at
/// the last tick it simulated, and its record has the checksum of every
/// checksum tick up to there. `MatchEndWire::last_tick` ends every
/// client's record there too (ruling T14-A): equal records and replays.
#[test]
fn ending_a_botless_servers_match_keeps_its_last_checksum() {
    let Some(content) = content() else { return };
    let mut setup = setup(&content, 1.0);
    setup.bots.clear();
    let cfg = HostConfig {
        name: "Server".into(),
        password: None,
        max_players: 8,
        max_spectators: 4,
        admins: vec![],
    };
    let mut net = Net::hosted_with(content, setup, PeerRole::Server(cfg));
    let clients = net.join_all(&["Anna", "Bert"]);
    net.ready(&clients);
    net.host().peer().lobby().start(6).unwrap();
    // Played a while, then a poll whose inputs confirm a checksum tick the
    // server has not simulated (its clients' inputs run ahead of it).
    let ahead = |n: &mut Net| {
        n.host().peer().match_session().is_some_and(|s| {
            let (current, confirmed) = (s.current_tick(), s.confirmed());
            current > 100 && (current..confirmed).any(|t| (t + 1) % rl_netcode::CHECKSUM_EVERY == 0)
        })
    };
    net.run_until(
        Duration::from_secs(15),
        "the server confirms a checksum tick ahead of its simulation",
        ahead,
    );
    let simulated = net.host().peer().match_session().unwrap().current_tick();
    assert!(net.host().peer().lobby().end_match(), "the running match ends");
    net.run_until(Duration::from_secs(15), "every peer keeps its record", |n| {
        n.peers.iter_mut().all(|p| p.peer().last_record().is_some())
    });
    let records: Vec<_> = net.peers.iter_mut().map(|p| p.peer().last_record().cloned().unwrap()).collect();
    let r = &records[0];
    let n = r.rows.len() as u32;
    assert_eq!(n, simulated, "the record ends at the last tick the server simulated");
    let want: Vec<u32> = (0..n).filter(|t| (t + 1) % rl_netcode::CHECKSUM_EVERY == 0).collect();
    let ticks: Vec<u32> = r.checksums.iter().map(|c| c.0).collect();
    assert_eq!(ticks, want, "the server's checksums of {n} ticks, the last one's too");
    for other in &records[1..] {
        assert_eq!(other.rows, r.rows);
        assert_eq!(other.checksums, r.checksums);
    }
    // Every peer said the same last tick, and made the same replay.
    for p in &net.peers {
        let ends: Vec<u32> = p
            .events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::MatchEnded(end) => Some(end.last_tick),
                _ => None,
            })
            .collect();
        assert_eq!(ends, [n - 1]);
    }
    net.run_until(Duration::from_secs(15), "every peer makes its replay", |n| {
        n.peers.iter().all(|p| p.has(replay_ready))
    });
    let bytes: Vec<&Vec<u8>> = net.peers.iter().map(|p| p.replays()[0]).collect();
    assert!(bytes.iter().all(|b| *b == bytes[0]), "equal replays");
}

#[test]
fn a_late_joiner_watches_then_plays_the_next_match() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.04);
    let a = net.join_all(&["Anna"])[0];
    net.ready(&[a]);
    net.host().peer().lobby().start(3).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 50);
    let b = net.join("Bert");
    net.run_until(Duration::from_secs(10), "the joiner watches confirmed ticks", |n| {
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
    net.run_until(Duration::from_secs(16), "the match ends everywhere", |n| {
        n.peers.iter().all(|p| p.has(ended))
    });
    // Gathering again after the results: the joiner takes a free seat.
    let key = net.peers[b].key;
    net.run_until(Duration::from_secs(14), "the joiner is seated", |n| {
        n.host().peer().lobby().state().is_some_and(|s| {
            s.phase == Phase::Gathering
                && s.members.iter().any(|m| m.key == key && matches!(m.seat, SeatWire::Player { .. }))
        })
    });
    net.ready(&[a, b]);
    net.host().peer().lobby().start(4).unwrap();
    net.run_until(Duration::from_secs(10), "the second match starts for the joiner", |n| {
        n.peers[b].started().len() == 2
    });
    assert_eq!(net.peers[b].started()[1], Some(2), "it plays");
    net.run_until(Duration::from_secs(10), "it plays ticks", |n| {
        n.peers[b].peer().match_session().is_some_and(|s| s.current_tick() > 20)
    });
    assert!(net.peers[a].replays().len() == 1, "the first match's replay");
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
    net.run_until(Duration::from_secs(10), "the odd client is refused and the host hears it left", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Refused(r) if r == MISMATCH))
            && n.peers[0].has(|e| matches!(e, OnlineEvent::Left(name) if name == "Bert"))
    });
    assert!(net.peers[b].peer().is_closed());
    net.run_until(Duration::from_secs(10), "the other client starts", |n| !n.peers[a].started().is_empty());
    assert!(matches!(net.peers[a].started()[..], [Some(1 | 2)]), "the other client plays");
    net.run_until(Duration::from_secs(10), "the match goes on", |n| n.host_tick() > 100);
    assert!(!net.peers[a].has(|e| matches!(e, OnlineEvent::Refused(_) | OnlineEvent::Disconnected(_))));
}

#[test]
fn a_client_vanishes_mid_match() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let clients = net.join_all(&["Anna", "Bert"]);
    let (a, b) = (clients[0], clients[1]);
    net.ready(&clients);
    net.host().peer().lobby().start(9).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 200);
    let gone_at = net.host_tick();
    let (Some(sa), Some(sb)) = (net.peers[a].started()[0], net.peers[b].started()[0]) else {
        panic!("both play")
    };
    let (sa, sb) = (usize::from(sa), usize::from(sb));
    net.peers[b].p = None;
    net.run_until(Duration::from_secs(28), "the host and the other client finish", |n| {
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
}

/// Presses nothing: a planted change's re-simulation.
struct Quiet;

impl Driver<WorldSim> for Quiet {
    fn local_inputs(&mut self, _: &WorldSim, _: u32, _: &[u8], _: &mut [Keys]) {}
}

fn bundles(p: &Peer) -> Vec<GameBundle> {
    p.events
        .iter()
        .filter_map(|e| match e {
            OnlineEvent::DesyncBundle(b) => Some(GameBundle::decode(b).expect("a bundle")),
            _ => None,
        })
        .collect()
}

/// Spec §3.5, §9.10: a desync in a real game saves a bundle on both
/// sides: the start, the final inputs from tick 0, the checksums, the
/// mismatch, the build and the platform. A health planted in the client's
/// world alone makes one.
#[test]
fn a_desync_makes_a_bundle_on_both_sides() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let a = net.join_all(&["Anna"])[0];
    net.ready(&[a]);
    net.host().peer().lobby().start(5).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 150);
    assert!(bundles(&net.peers[0]).is_empty() && bundles(&net.peers[a]).is_empty(), "no desync yet");
    let s = net.peers[a].peer().match_session_mut().expect("Anna plays");
    s.change_confirmed(&mut Quiet, |sim| sim.world_mut().set_health(0, 1.0));
    net.run_until(Duration::from_secs(10), "both sides make a bundle", |n| {
        !bundles(&n.peers[0]).is_empty() && !bundles(&n.peers[a]).is_empty()
    });
    for (i, host) in [(0, true), (a, false)] {
        let b = &bundles(&net.peers[i])[0];
        assert_eq!(b.host, host);
        assert_eq!(b.my_slot, Some(i as u8), "the host plays slot 0, Anna slot 1");
        assert_eq!(b.start.seed, 5);
        assert_eq!(b.fingerprint, rl_proto::FINGERPRINT);
        assert!(!b.platform.is_empty());
        let (tick, local, remote) = b.mismatch.expect("the mismatch");
        assert_ne!(local, remote);
        assert!(
            b.rows.len() > tick as usize,
            "the inputs reach the mismatch: {} rows, tick {tick}",
            b.rows.len()
        );
        assert!(b.checksums.iter().any(|&(t, c)| t == tick && c == local), "its own checksum of the tick");
    }
    let (h, c) = (&bundles(&net.peers[0])[0], &bundles(&net.peers[a])[0]);
    let (ht, hl, hr) = h.mismatch.unwrap();
    assert_eq!(c.mismatch, Some((ht, hr, hl)), "the same mismatch from both sides");
    // Both streams (spec §3.5): the host has the client's reports up to the
    // mismatch, which agree with the client's own before it.
    assert!(h.other_checksums.len() > 5, "the client's stream: {:?}", h.other_checksums);
    assert_eq!(h.other_checksums.last(), Some(&(ht, hr)));
    for &(t, sum) in h.other_checksums.iter().filter(|&&(t, _)| t < ht) {
        assert!(c.checksums.contains(&(t, sum)), "tick {t}: the client's own");
        assert!(h.checksums.contains(&(t, sum)), "tick {t}: in sync before the mismatch");
    }
    assert_eq!(c.other_checksums, [(ht, hl)], "a client knows the host's of the mismatch");
}

/// A joiner expecting another key than the host's (an invite naming
/// another game) refuses the host, in words, and is never admitted; one
/// expecting the host's key joins (carried ruling F-B, N37).
#[test]
fn a_host_with_another_key_is_refused() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let a = net.join_expecting("Anna", identity().public());
    net.run_until(Duration::from_secs(10), "the client refuses the host", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Refused(_)))
    });
    let refusal = net.peers[a].events.iter().find_map(|e| match e {
        OnlineEvent::Refused(r) => Some(r.clone()),
        _ => None,
    });
    assert_eq!(refusal.as_deref(), Some(WRONG_HOST));
    assert_eq!(WRONG_HOST, "this is not the game you were invited to");
    assert!(net.peers[a].peer().is_closed());
    for _ in 0..50 {
        net.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(net.members(), 1, "never admitted");
    let host_key = net.host().key;
    let b = net.join_expecting("Bert", host_key);
    net.run_until(Duration::from_secs(10), "the client expecting the host's key joins", |n| {
        n.members() == 2 && n.peers[b].has(|e| matches!(e, OnlineEvent::Lobby(_)))
    });
}

#[test]
fn kicked_players_cannot_rejoin() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let me = identity();
    let secret = me.secret();
    let a = net.join_as(me, "Anna");
    net.run_until(Duration::from_secs(10), "the client joins", |n| n.members() == 2);
    let key = net.peers[a].key;
    let id = net.host().member_id(key).unwrap();
    net.host().peer().lobby().kick(id, true);
    net.run_until(Duration::from_secs(10), "the client is kicked", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Kicked { banned: true }))
    });
    assert!(net.peers[a].peer().is_closed());
    net.run_until(Duration::from_secs(10), "the host's lobby lost it", |n| n.members() == 1);
    // The same key from the same address: refused.
    let again = net.join_as(Identity::from_secret(secret).unwrap(), "Anna");
    net.run_until(Duration::from_secs(10), "the rejoin is refused", |n| {
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
    // A ban is by key (spec §5.2): another key from the same address joins.
    let other = net.join("Bert");
    net.run_until(Duration::from_secs(10), "another key joins", |n| {
        n.members() == 2 && n.peers[other].has(|e| matches!(e, OnlineEvent::Lobby(_)))
    });
}

#[test]
fn an_admins_setup_is_taken_or_refused_with_a_notice() {
    let Some(content) = content() else { return };
    let admin = identity();
    let mut net = Net::with_admins(content, 0.1, vec![admin.public()]);
    let a = net.join_as(admin, "Anna");
    net.run_until(Duration::from_secs(10), "the admin joins", |n| {
        n.members() == 2 && n.peers[a].has(|e| matches!(e, OnlineEvent::Lobby(_)))
    });
    // A mod the host does not have: refused, the admin is told.
    let mut odd = setup(&net.content(), 0.1);
    odd.mod_id = "olx-no-such-mod".into();
    net.peers[a].peer().lobby().set_setup(odd).unwrap();
    net.run_until(Duration::from_secs(10), "the admin hears the refusal", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Notice(t) if t.starts_with(SETUP_REFUSED)))
    });
    assert!(net.host().peer().lobby().state().is_some_and(|s| s.setup.mod_id == MOD), "unchanged");
    // A setup the host can play: taken.
    let longer = setup(&net.content(), 0.5);
    net.peers[a].peer().lobby().set_setup(longer.clone()).unwrap();
    net.run_until(Duration::from_secs(10), "the host takes it", |n| {
        n.peers[a].peer().lobby().state().is_some_and(|s| s.setup == longer)
    });
    assert_eq!(
        net.peers[a].count(|e| matches!(e, OnlineEvent::Notice(t) if t.starts_with(SETUP_REFUSED))),
        1
    );
}

/// A dedicated server (`PeerRole::Server`): no member of its own, the chat
/// by member id, a notice to one member, its members' addresses, bans by
/// key and address, a match its bots and members play, ended early.
#[test]
fn a_dedicated_server_hosts_without_a_player() {
    let Some(content) = content() else { return };
    let mut net = Net::server(content, 1.0);
    assert_eq!(net.members(), 0, "the server is no member");
    let clients = net.join_all(&["Anna", "Bert"]);
    let (a, b) = (clients[0], clients[1]);
    let bert_key = net.peers[b].key;
    let bert = net.host().member_id(bert_key).unwrap();
    net.peers[b].peer().lobby().chat("/players");
    net.run_until(Duration::from_secs(10), "the server hears Bert by id", |n| {
        n.host().has(|e| {
            matches!(e, OnlineEvent::Chat { member, from, text }
            if *member == bert && from == "Bert" && text == "/players")
        })
    });
    net.host().peer().lobby().tell(bert, "just you");
    net.run_until(Duration::from_secs(10), "Bert hears the notice", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::Notice(t) if t == "just you"))
    });
    assert!(!net.peers[a].has(|e| matches!(e, OnlineEvent::Notice(t) if t == "just you")), "only Bert");
    let addr = net.host().peer().lobby().addr_of(bert).expect("Bert's address");
    assert!(addr.ip().is_loopback());
    // Bans from a list: a key, an address.
    let banned = identity();
    let odd: std::net::IpAddr = "192.0.2.7".parse().unwrap();
    net.host().peer().lobby().ban_key(banned.public());
    net.host().peer().lobby().ban_ip(odd);
    assert_eq!(net.host().peer().lobby().bans(), (vec![banned.public()], vec![odd]));
    let banned_secret = banned.secret();
    let c = net.join_as(banned, "Cid");
    net.run_until(Duration::from_secs(10), "the banned key is refused", |n| {
        n.peers[c].has(|e| matches!(e, OnlineEvent::Refused(r) if r == "you are banned from this lobby"))
    });
    // Lifted, it joins.
    let banned_key = net.peers[c].key;
    assert!(net.host().peer().lobby().unban_key(&banned_key));
    assert!(net.host().peer().lobby().unban_ip(odd));
    assert_eq!(net.host().peer().lobby().bans(), (vec![], vec![]));
    let c = net.join_as(Identity::from_secret(banned_secret).unwrap(), "Cid");
    net.run_until(Duration::from_secs(10), "the unbanned key joins", |n| {
        n.peers[c].has(|e| matches!(e, OnlineEvent::Lobby(_)))
    });
    net.peers[c].peer().leave();
    net.run_until(Duration::from_secs(10), "it leaves", |n| n.members() == 2);
    // The members and the bot play; the server plays no slot.
    net.ready(&clients);
    net.host().peer().lobby().start(11).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 100);
    assert_eq!(net.peers[0].started(), vec![None]);
    let mut slots: Vec<Option<u8>> = [a, b].iter().flat_map(|&i| net.peers[i].started()).collect();
    slots.sort();
    assert_eq!(slots, vec![Some(0), Some(1)]);
    assert!(net.host().peer().lobby().end_match(), "a match to end");
    assert!(!net.host().peer().lobby().end_match(), "once");
    net.run_until(Duration::from_secs(10), "every peer ends the match at once", |n| {
        [0, a, b].iter().all(|&i| n.peers[i].has(ended))
    });
    let record = net.peers[0].peer().last_record().cloned().expect("the host's record");
    assert!((90..600).contains(&record.rows.len()), "{} rows: ended early", record.rows.len());
}

// ---- content downloads (plan 11, N30-N33) ----

/// `key`'s member in the host's lobby.
fn member_of(net: &mut Net, key: PublicKey) -> Option<rl_proto::Member> {
    net.host().peer().lobby().state()?.members.iter().find(|m| m.key == key).cloned()
}

/// The last downloads a peer reported (`OnlineEvent::Downloads`).
fn reported(p: &Peer) -> Vec<Download> {
    p.events
        .iter()
        .rev()
        .find_map(|e| match e {
            OnlineEvent::Downloads(d) => Some(d.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Pack `id`'s download state, as last reported.
fn reported_state(p: &Peer, id: &str) -> Option<DownloadState> {
    reported(p).into_iter().find(|d| d.item.id == id).map(|d| d.state)
}

/// Pack `id`'s content hash in the full content.
fn hash_of(net: &Net, id: &str) -> String {
    let c = net.content();
    c.pack_hash(id, c.pack_kind(id).unwrap()).unwrap()
}

fn in_cache(p: &Peer, hash: &str) -> bool {
    p.cache.as_ref().is_some_and(|c| c.lock().unwrap().contains(hash))
}

/// A client lacking `pack` downloads it from the host in the lobby, is
/// ready, plays, and makes the replay every peer makes.
fn downloads_and_plays(pack: &str) {
    let mut net = Net::new(content().unwrap(), 0.05);
    let a = net.join_lacking("Anna", &[pack]);
    let key = net.peers[a].key;
    net.run_until(Duration::from_secs(15), "the download is done and reported", |n| {
        reported_state(&n.peers[a], pack) == Some(DownloadState::Done)
    });
    let states: Vec<DownloadState> = net.peers[a]
        .events
        .iter()
        .filter_map(|e| match e {
            OnlineEvent::Downloads(d) => d.iter().find(|d| d.item.id == pack).map(|d| d.state.clone()),
            _ => None,
        })
        .collect();
    assert!(!states.is_empty() && states[..states.len() - 1].iter().all(|s| *s != DownloadState::Done));
    assert!(states.len() <= 20, "coalesced: {} reports", states.len());
    net.run_until(Duration::from_secs(10), "the host sees its content complete", |n| {
        member_of(n, key).is_some_and(|m| m.missing.is_empty() && m.download.is_none())
    });
    assert!(in_cache(&net.peers[a], &hash_of(&net, pack)), "the cache holds the pack");
    assert!(net.peers[a].peer().content().pack_dir(pack).is_some(), "the match's content has it");
    net.ready(&[a]);
    net.host().peer().lobby().start(8).unwrap();
    net.run_until(Duration::from_secs(25), "both end the match and make their replay", |n| {
        n.peers.iter().all(|p| p.has(ended) && p.has(replay_ready))
    });
    assert_eq!(net.peers[a].started(), vec![Some(1)], "it played");
    let bytes: Vec<&Vec<u8>> = net.peers.iter().map(|p| p.replays()[0]).collect();
    assert_eq!(bytes[0], bytes[1], "equal replays");
}

#[test]
fn a_client_without_the_level_downloads_it_and_plays() {
    if content().is_none() {
        return;
    }
    downloads_and_plays(LEVEL);
}

#[test]
fn a_client_without_the_mod_downloads_it() {
    if content().is_none() {
        return;
    }
    downloads_and_plays(MOD);
}

/// N31: a member's skin the host lacks is fetched from that member, then
/// served to the others (never required).
#[test]
fn a_skin_travels_through_the_host() {
    let Some(content) = content() else { return };
    let (dir, _) = partial_content(&[]);
    let anna_content = with_test_skin(dir.path());
    let skin_hash = anna_content.pack_hash(TEST_SKIN, rl_data::manifest::PackKind::Skin).unwrap();
    let mut net = Net::new(content, 0.1);
    net.dirs.push(dir);
    let mut anna = look("Anna");
    anna.skin = TEST_SKIN.into();
    let a = net.join_with(identity(), anna, anna_content, None);
    let b = net.join("Bert");
    let a_key = net.peers[a].key;
    net.run_until(Duration::from_secs(15), "Bert has Anna's skin", |n| {
        reported_state(&n.peers[b], TEST_SKIN) == Some(DownloadState::Done)
    });
    assert!(in_cache(&net.peers[0], &skin_hash), "the host fetched it from Anna");
    assert!(in_cache(&net.peers[b], &skin_hash), "Bert's cache holds it");
    assert!(
        net.host()
            .peer()
            .downloads()
            .iter()
            .any(|d| d.item.hash == skin_hash && d.state == DownloadState::Done)
    );
    let look = member_of(&mut net, a_key).unwrap().look;
    assert_eq!((look.skin.as_str(), look.skin_hash.as_str()), (TEST_SKIN, skin_hash.as_str()), "Anna's look");
    // A skin never makes anyone lack content.
    let host_state = net.host().peer().lobby().state().unwrap().clone();
    assert!(host_state.members.iter().all(|m| m.missing.is_empty()));
    assert!(net.peers[b].peer().content().pack_dir(TEST_SKIN).is_some(), "Bert draws it");
}

/// A change to the list's skins alone (a member joins with a skin the
/// host fetches, then lists) asks no one to answer again: a ready player
/// stays ready all along, and the match starts.
#[test]
fn a_skin_change_keeps_everyone_ready() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let a = net.join_all(&["Anna"])[0];
    net.ready(&[a]);
    let a_key = net.peers[a].key;
    let ready_from = net.host().events.len();
    let (dir, _) = partial_content(&[]);
    let skinned = with_test_skin(dir.path());
    net.dirs.push(dir);
    let mut bert = look("Bert");
    bert.skin = TEST_SKIN.into();
    let b = net.join_with(identity(), bert, skinned, None);
    net.run_until(Duration::from_secs(15), "the host lists Bert's skin", |n| {
        reported_state(&n.peers[a], TEST_SKIN) == Some(DownloadState::Done)
    });
    let lobbies: Vec<bool> = net.host().events[ready_from..]
        .iter()
        .filter_map(|e| match e {
            OnlineEvent::Lobby(s) => s.members.iter().find(|m| m.key == a_key).map(|m| m.ready),
            _ => None,
        })
        .collect();
    assert!(!lobbies.is_empty() && lobbies.iter().all(|&r| r), "Anna stayed ready: {lobbies:?}");
    net.peers[b].peer().lobby().set_spectate(true);
    let b_key = net.peers[b].key;
    net.run_until(Duration::from_secs(10), "Bert watches", |n| {
        member_of(n, b_key).is_some_and(|m| m.seat == SeatWire::Spectator)
    });
    net.host().peer().lobby().start(15).expect("everyone ready");
}

/// A member's "skin" under the level's id (a skin pack whose manifest
/// says `olx-level-dirt-level`): the host never fetches it, never lists it,
/// and its matches still build; a complete client still plays.
#[test]
fn a_skin_under_a_required_id_is_refused() {
    let Some(content) = content() else { return };
    let (dir, _) = partial_content(&[LEVEL]);
    let fake_content = with_skin(dir.path(), LEVEL);
    let fake_hash = fake_content.pack_hash(LEVEL, rl_data::manifest::PackKind::Skin).unwrap();
    let mut net = Net::new(content, 0.1);
    net.dirs.push(dir);
    let mut fake = look("Mallory");
    fake.skin = LEVEL.into();
    let m = net.join_with(identity(), fake, fake_content, None);
    let b = net.join("Bert");
    let m_key = net.peers[m].key;
    net.run_until(Duration::from_secs(10), "the host knows Mallory's look", |n| {
        member_of(n, m_key).is_some_and(|m| m.look.skin_hash == fake_hash)
    });
    // Long enough for a fetch of a few KB, had it started.
    let until = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < until {
        net.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!in_cache(&net.peers[0], &fake_hash), "the host did not fetch it");
    assert!(net.host().peer().downloads().is_empty());
    let host_content = net.host().peer().content();
    assert_eq!(host_content.pack_kind(LEVEL), Some(rl_data::manifest::PackKind::Level), "the host's level");
    assert!(reported(&net.peers[b]).iter().all(|d| d.item.hash != fake_hash), "never listed");
    let b_key = net.peers[b].key;
    net.peers[b].peer().lobby().set_ready(true);
    net.run_until(Duration::from_secs(10), "Bert is ready", |n| member_of(n, b_key).is_some_and(|m| m.ready));
    net.host().peer().lobby().force_start(13).expect("the host's match still builds");
    net.run_until(Duration::from_secs(10), "Bert plays", |n| {
        n.peers[b].peer().match_session().is_some_and(|s| s.current_tick() > 20)
    });
    assert_eq!(net.peers[b].peer().content().pack_kind(LEVEL), Some(rl_data::manifest::PackKind::Level));
}

/// Another pack's archive served for the level: it fails to verify, is
/// fetched once more, then fails; the client stays, not ready, lacking it.
#[test]
fn a_tampered_archive_is_refused_and_retried_once() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    net.host().peer().test_serve_instead(LEVEL, "olx-theme-dirt");
    let a = net.join_lacking("Anna", &[LEVEL]);
    let key = net.peers[a].key;
    net.run_until(Duration::from_secs(15), "the level fails", |n| {
        matches!(reported_state(&n.peers[a], LEVEL), Some(DownloadState::Failed(_)))
    });
    let Some(DownloadState::Failed(why)) = reported_state(&net.peers[a], LEVEL) else { unreachable!() };
    assert!(why.contains("differs"), "{why}");
    net.peers[a].peer().lobby().set_ready(true);
    for _ in 0..100 {
        net.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!net.peers[a].peer().is_closed(), "it stays");
    let m = member_of(&mut net, key).unwrap();
    assert!(!m.ready, "not ready");
    assert_eq!(m.missing, vec![LEVEL.to_string()], "the lobby shows what it lacks");
    assert!(!in_cache(&net.peers[a], &hash_of(&net, LEVEL)));
}

/// N33: a spectator lacking the level at a start waits, outside the
/// match's session: more than the session's 10 s of silence later it is
/// still there, and once its content is complete it watches.
#[test]
fn a_member_waiting_at_the_start_is_not_timed_out() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 1.0);
    let a = net.join_all(&["Anna"])[0];
    let (dir, partial) = partial_content(&[LEVEL]);
    net.dirs.push(dir);
    let b = net.join_with(identity(), look("Bert"), partial, None);
    let b_key = net.peers[b].key;
    // Its download waits for the player (the prompt).
    net.peers[b].peer().test_prompt_limit(1000);
    net.run_until(Duration::from_secs(10), "Bert is told what it lacks", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::DownloadPrompt { .. }))
            && member_of(n, b_key).is_some_and(|m| m.missing == [LEVEL.to_string()])
    });
    net.peers[b].peer().lobby().set_spectate(true);
    net.ready(&[a]);
    net.host().peer().lobby().start(14).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs, Bert waits", |n| {
        n.host_tick() > 10 && member_of(n, b_key).is_some_and(|m| m.seat == SeatWire::Waiting)
    });
    let until = Instant::now() + Duration::from_millis(10_500);
    while Instant::now() < until {
        net.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!net.peers[b].peer().is_closed(), "Bert stays");
    assert!(member_of(&mut net, b_key).is_some(), "a member still");
    assert!(!net.host().has(|e| matches!(e, OnlineEvent::Left(_))), "nobody left");
    net.peers[b].peer().approve_downloads();
    net.run_until(Duration::from_secs(10), "Bert watches from a state", |n| {
        n.peers[b].has(|e| matches!(e, OnlineEvent::StateLoaded { .. }))
    });
    assert_eq!(net.peers[b].started(), vec![None]);
    assert!(net.host().peer().lobby().end_match());
}

/// A spectator joining mid-match without the level waits, downloads at
/// the match's rate, then watches; the match goes on to its end.
/// Returns the net, the spectator, and how long it waited from its
/// `Waiting` seat to its start.
fn late_spectator_lacking_the_level() -> (Net, usize, Duration) {
    let mut net = Net::new(content().unwrap(), 0.1);
    let a = net.join_all(&["Anna"])[0];
    net.ready(&[a]);
    net.host().peer().lobby().start(12).unwrap();
    net.run_until(Duration::from_secs(10), "the match runs", |n| n.host_tick() >= 50);
    let b = net.join_lacking("Bert", &[LEVEL]);
    let key = net.peers[b].key;
    net.run_until(Duration::from_secs(10), "it waits", |n| {
        member_of(n, key).is_some_and(|m| m.seat == SeatWire::Waiting)
    });
    let waiting = Instant::now();
    net.run_until(Duration::from_secs(10), "it starts watching", |n| !n.peers[b].started().is_empty());
    let waited = waiting.elapsed();
    net.run_until(Duration::from_secs(16), "the match ends everywhere", |n| {
        n.peers.iter().all(|p| p.has(ended))
    });
    (net, b, waited)
}

#[test]
fn a_spectator_lacking_content_waits_then_spectates() {
    if content().is_none() {
        return;
    }
    let (net, b, _) = late_spectator_lacking_the_level();
    let events = &net.peers[b].events;
    // Reports are coalesced: `Done` may come after the start, but the
    // download was under way before it.
    let downloading = events.iter().position(|e| {
        matches!(e, OnlineEvent::Downloads(d) if d.iter().any(|d| d.item.id == LEVEL && d.state != DownloadState::Done))
    });
    let started = events.iter().position(|e| matches!(e, OnlineEvent::MatchStarting { .. }));
    let (Some(downloading), Some(started)) = (downloading, started) else {
        panic!("downloading, then started: {downloading:?} {started:?}")
    };
    assert!(downloading < started, "it downloaded before it watched");
    assert_eq!(reported_state(&net.peers[b], LEVEL), Some(DownloadState::Done));
    assert_eq!(net.peers[b].started(), vec![None], "it watches");
    assert!(events.iter().any(|e| matches!(e, OnlineEvent::StateLoaded { .. })), "from a state");
    assert!(
        events.iter().any(|e| matches!(e, OnlineEvent::Confirmed(c) if !c.is_empty())),
        "confirmed ticks"
    );
}

#[test]
fn downloads_during_a_match_keep_it_alive() {
    if content().is_none() {
        return;
    }
    let (net, b, waited) = late_spectator_lacking_the_level();
    assert_eq!(reported_state(&net.peers[b], LEVEL), Some(DownloadState::Done));
    // Sent at the match's rate (`MATCH_RATE`), not the lobby's: the
    // host's bucket holds at most a chunk to start with.
    let size = reported(&net.peers[b]).iter().find(|d| d.item.id == LEVEL).unwrap().item.size;
    let least =
        size.saturating_sub(rl_proto::CONTENT_CHUNK as u64) as f64 / rl_online::content::MATCH_RATE as f64;
    assert!(waited.as_secs_f64() >= least, "{size} bytes in {waited:?}: faster than the match's rate");
    for p in &net.peers {
        assert!(
            !p.has(|e| matches!(
                e,
                OnlineEvent::Disconnected(_) | OnlineEvent::Refused(_) | OnlineEvent::Left(_)
            )),
            "nobody was dropped (a peer over its limits is)"
        );
    }
    let ends: Vec<_> = net.peers[..2].iter().map(|p| p.count(ended)).collect();
    assert_eq!(ends, [1, 1], "the match reached its end");
}

/// Ruling PF11-4: without a cache (`Err`) the lobby says why and the client
/// stays, not ready; a cache that cannot install fails the download with
/// the I/O reason, no panic.
#[test]
fn an_unwritable_cache_is_reported() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let (dir, partial) = partial_content(&[LEVEL]);
    net.dirs.push(dir);
    let cfg = PeerContent::without_downloads("the cache is a regular file");
    let a = net.join_with(identity(), look("Anna"), partial, Some(cfg));
    let a_key = net.peers[a].key;
    net.run_until(Duration::from_secs(10), "Anna hears why", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::Notice(t) if t.contains("the cache is a regular file") && t.contains(LEVEL)))
    });
    net.run_until(Duration::from_secs(10), "the host sees what Anna lacks", |n| {
        member_of(n, a_key).is_some_and(|m| m.missing == [LEVEL.to_string()])
    });
    // A cache whose packs cannot be written.
    let (dir, partial) = partial_content(&[LEVEL]);
    net.dirs.push(dir);
    let (cache_dir, cache) = cache();
    let locked = [cache_dir.path().join("packs"), cache_dir.path().join("tmp")];
    std::fs::create_dir_all(&locked[1]).unwrap();
    // The cache removes an empty `tmp/`: only what is there is changed.
    fn set_readonly(dirs: &[PathBuf], on: bool) {
        for d in dirs.iter().filter(|d| d.is_dir()) {
            let Ok(meta) = std::fs::metadata(d) else { continue };
            let mut perm = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(on);
            let _ = std::fs::set_permissions(d, perm);
        }
    }
    /// Writable again when the test ends, even by a panic (else the
    /// temporary directory cannot be removed).
    struct Unlock(Vec<PathBuf>);
    impl Drop for Unlock {
        fn drop(&mut self) {
            set_readonly(&self.0, false);
        }
    }
    set_readonly(&locked, true);
    let _unlock = Unlock(locked.to_vec());
    if std::fs::write(locked[0].join("probe"), b"").is_ok() {
        eprintln!("this user writes to read-only directories (root?): skipping the unwritable cache");
        return;
    }
    let b = net.join_with(identity(), look("Bert"), partial, Some(peer_content(&cache)));
    net.dirs.push(cache_dir);
    net.run_until(Duration::from_secs(15), "Bert's download fails", |n| {
        matches!(reported_state(&n.peers[b], LEVEL), Some(DownloadState::Failed(_)))
    });
    let Some(DownloadState::Failed(why)) = reported_state(&net.peers[b], LEVEL) else { unreachable!() };
    assert!(why.contains("the download cache"), "the I/O reason: {why}");
    for i in [a, b] {
        net.peers[i].peer().lobby().set_ready(true);
    }
    for _ in 0..100 {
        net.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    for i in [a, b] {
        assert!(!net.peers[i].peer().is_closed(), "it stays");
        let key = net.peers[i].key;
        let m = member_of(&mut net, key).unwrap();
        assert!(!m.ready && m.missing == [LEVEL.to_string()], "not ready, lacking the level");
    }
}

/// Content above the prompt's bytes waits for the player.
#[test]
fn big_downloads_wait_for_the_prompt() {
    let Some(content) = content() else { return };
    let mut net = Net::new(content, 0.1);
    let (dir, partial) = partial_content(&[LEVEL]);
    net.dirs.push(dir);
    let a = net.join_with(identity(), look("Anna"), partial, None);
    net.peers[a].peer().test_prompt_limit(1000);
    net.run_until(Duration::from_secs(10), "the prompt", |n| {
        n.peers[a].has(|e| matches!(e, OnlineEvent::DownloadPrompt { .. }))
    });
    let level = net.peers[a].peer().downloads().into_iter().find(|d| d.item.id == LEVEL).unwrap();
    let prompts: Vec<u64> = net.peers[a]
        .events
        .iter()
        .filter_map(|e| match e {
            OnlineEvent::DownloadPrompt { bytes } => Some(*bytes),
            _ => None,
        })
        .collect();
    assert_eq!(prompts, [level.item.size], "once, for the level's bytes");
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        net.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    let waiting = net.peers[a].peer().downloads();
    assert!(waiting.iter().all(|d| d.state == DownloadState::Queued && d.have == 0), "{waiting:?}");
    net.peers[a].peer().approve_downloads();
    net.run_until(Duration::from_secs(15), "the level downloads", |n| {
        reported_state(&n.peers[a], LEVEL) == Some(DownloadState::Done)
    });
}
