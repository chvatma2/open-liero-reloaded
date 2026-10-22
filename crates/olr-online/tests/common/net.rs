//! The test network of `online.rs` and `migration.rs`: a host and its
//! clients over 127.0.0.1 (port 0), polled together on the real clock.
#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use olr_bots::BotConfig;
use olr_content::{Content, DownloadCache};
use olr_lobby::{HostConfig, Identity};
use olr_modes::DeathmatchSettings;
use olr_net::endpoint::EndpointConfig;
use olr_online::matches::{bot_wire, rules_wire, setup_packs};
use olr_online::{HostListen, Join, MigrationConfig, OnlineEvent, OnlinePeer, PeerContent, PeerRole};
use olr_proto::{Look, MatchSetupWire, PublicKey, SeatWire};
use olr_sim::Keys;
use tempfile::TempDir;

use super::{cache, partial_content, peer_content};

pub const MOD: &str = "olx-classic";
pub const LEVEL: &str = "olx-level-dirt-level";

pub fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).unwrap())
}

pub fn identity() -> Identity {
    loop {
        if let Some(id) = Identity::from_secret(olr_net::random_bytes::<32>().unwrap()) {
            return id;
        }
    }
}

pub fn look(name: &str) -> Look {
    Look {
        name: name.into(),
        colour: [200, 80, 40],
        skin: "olx-skin-default".into(),
        skin_hash: olr_proto::NO_HASH.into(),
    }
}

/// Classic on Dirt Level, one bot, ending after `minutes`.
pub fn setup(content: &Content, minutes: f32) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        packs: setup_packs(content, MOD, LEVEL).unwrap(),
        rules: rules_wire(&DeathmatchSettings { time_limit: minutes, ..DeathmatchSettings::default() }),
        loadout: olr_proto::LoadoutWire::default(),
        bots: vec![bot_wire(&BotConfig::default(), look("Bot"))],
    }
}

pub fn endpoint() -> EndpointConfig {
    EndpointConfig { port: 0, ..EndpointConfig::default() }
}

/// One peer of the test, with everything it reported.
pub struct Peer {
    pub p: Option<OnlinePeer>,
    pub key: PublicKey,
    pub events: Vec<OnlineEvent>,
    /// Keys pressed so far: they change every 25 ticks.
    pub pressed: u32,
    /// Its download cache.
    pub cache: Option<Arc<Mutex<DownloadCache>>>,
    /// Not polled (`Net::poll` skips it): alive, its sockets open, silent
    /// (a hung machine).
    pub frozen: bool,
}

impl Peer {
    pub fn new(p: OnlinePeer, key: PublicKey) -> Peer {
        Peer { p: Some(p), key, events: Vec::new(), pressed: 0, cache: None, frozen: false }
    }

    pub fn peer(&mut self) -> &mut OnlinePeer {
        self.p.as_mut().expect("the peer is there")
    }

    pub fn poll(&mut self, now: Instant) {
        let Some(p) = &mut self.p else { return };
        let pressed = &mut self.pressed;
        let seed = self.key[0] as u32;
        let mut keys = || {
            *pressed += 1;
            let k = (*pressed / 25).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 24;
            Keys(u16::from(k as u8) & !Keys::CHANGE)
        };
        self.events.extend(p.poll(now, &mut keys, &mut |_, _, _| {}));
    }

    pub fn count(&self, f: impl Fn(&OnlineEvent) -> bool) -> usize {
        self.events.iter().filter(|e| f(e)).count()
    }

    pub fn has(&self, f: impl Fn(&OnlineEvent) -> bool) -> bool {
        self.count(f) > 0
    }

    pub fn started(&self) -> Vec<Option<u8>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::MatchStarting { my_slot, .. } => Some(*my_slot),
                _ => None,
            })
            .collect()
    }

    pub fn replays(&self) -> Vec<&Vec<u8>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::ReplayReady(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    pub fn member_id(&mut self, key: PublicKey) -> Option<u8> {
        let lobby = self.peer().lobby();
        lobby.state()?.members.iter().find(|m| m.key == key).map(|m| m.id)
    }
}

/// The host and its clients, polled together on the real clock.
pub struct Net {
    pub content_dir: String,
    pub port: u16,
    pub peers: Vec<Peer>,
    /// Caches and partial contents, removed at the end.
    pub dirs: Vec<TempDir>,
}

impl Net {
    pub fn new(content: Content, minutes: f32) -> Net {
        Net::with_admins(content, minutes, Vec::new())
    }

    pub fn with_admins(content: Content, minutes: f32, admins: Vec<PublicKey>) -> Net {
        let cfg = HostConfig {
            name: "Test lobby".into(),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins,
        };
        Net::hosted(content, minutes, PeerRole::Host { lobby: cfg, listen: HostListen::off() })
    }

    /// A dedicated server's lobby: no local player (`PeerRole::Server`).
    pub fn server(content: Content, minutes: f32) -> Net {
        let cfg = HostConfig {
            name: "Server".into(),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins: vec![],
        };
        Net::hosted(content, minutes, PeerRole::Server { lobby: cfg, listen: HostListen::off() })
    }

    pub fn hosted(content: Content, minutes: f32, role: PeerRole) -> Net {
        let setup = setup(&content, minutes);
        Net::hosted_with(content, setup, role)
    }

    pub fn hosted_with(content: Content, setup: MatchSetupWire, role: PeerRole) -> Net {
        let content_dir = std::env::var("OLR_CONTENT_DIR").unwrap();
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

    pub fn content(&self) -> Content {
        Content::open(std::path::Path::new(&self.content_dir)).unwrap()
    }

    /// A client joining as `me`, with every pack and a cache of its own;
    /// its index.
    pub fn join_as(&mut self, me: Identity, name: &str) -> usize {
        let content = self.content();
        self.join_with(me, look(name), content, None)
    }

    /// A client joining as `me` with `look` and `content`, and `cfg` or a
    /// cache of its own; its index.
    pub fn join_with(
        &mut self,
        me: Identity,
        look: Look,
        content: Content,
        cfg: Option<PeerContent>,
    ) -> usize {
        self.join_expecting_with(me, look, content, cfg, None)
    }

    /// A client expecting the host's key `expected` (an invite's), with
    /// every pack and a cache of its own; its index.
    pub fn join_expecting(&mut self, name: &str, expected: PublicKey) -> usize {
        let content = self.content();
        self.join_expecting_with(identity(), look(name), content, None, Some(expected))
    }

    pub fn join_expecting_with(
        &mut self,
        me: Identity,
        look: Look,
        content: Content,
        cfg: Option<PeerContent>,
        expected_host: Option<PublicKey>,
    ) -> usize {
        self.join_full(me, look, content, cfg, expected_host, MigrationConfig::off())
    }

    /// A client taking part in host migrations as `migration` says, with
    /// every pack and a cache of its own; its index once the host welcomed
    /// it (so join order, and the succession, is the order of the calls).
    pub fn join_with_migration(&mut self, name: &str, migration: MigrationConfig) -> usize {
        let content = self.content();
        let before = self.members();
        let i = self.join_full(identity(), look(name), content, None, None, migration);
        self.run_until(Duration::from_secs(10), "the client is welcomed", |n| {
            n.members() == before + 1 && n.peers[i].has(|e| matches!(e, OnlineEvent::Lobby(_)))
        });
        i
    }

    pub fn join_full(
        &mut self,
        me: Identity,
        look: Look,
        content: Content,
        cfg: Option<PeerContent>,
        expected_host: Option<PublicKey>,
        migration: MigrationConfig,
    ) -> usize {
        let key = me.public();
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let role = PeerRole::Client { join: Join::direct(addr), password: None, expected_host, migration };
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
    pub fn join_lacking(&mut self, name: &str, leave_out: &[&str]) -> usize {
        let (dir, content) = partial_content(leave_out);
        self.dirs.push(dir);
        self.join_with(identity(), look(name), content, None)
    }

    pub fn join(&mut self, name: &str) -> usize {
        self.join_as(identity(), name)
    }

    pub fn poll(&mut self) {
        let now = Instant::now();
        for p in self.peers.iter_mut().filter(|p| !p.frozen) {
            p.poll(now);
        }
    }

    /// Polls until `done` holds; panics after `limit` with `what`.
    pub fn run_until(&mut self, limit: Duration, what: &str, mut done: impl FnMut(&mut Net) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < limit, "{what} within {limit:?}");
            self.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn host(&mut self) -> &mut Peer {
        &mut self.peers[0]
    }

    /// The peer hosting now (`is_host`), the first if several think so.
    pub fn host_index(&self) -> Option<usize> {
        self.peers.iter().position(|p| p.p.as_ref().is_some_and(OnlinePeer::is_host))
    }

    /// Peer `i`'s every endpoint drops every datagram both ways for `d`
    /// (`OnlinePeer::test_outage`).
    pub fn outage(&mut self, i: usize, d: Duration) {
        self.peers[i].peer().test_outage(Instant::now() + d);
    }

    /// Members in the host's lobby.
    pub fn members(&mut self) -> usize {
        self.host().peer().lobby().state().map_or(0, |s| s.members.len())
    }

    /// Joins clients and waits until the host's lobby lists them.
    pub fn join_all(&mut self, names: &[&str]) -> Vec<usize> {
        let before = self.members();
        let ids: Vec<usize> = names.iter().map(|n| self.join(n)).collect();
        let want = before + names.len();
        self.run_until(Duration::from_secs(10), "the clients join", |n| {
            n.members() == want && ids.iter().all(|&c| n.peers[c].has(|e| matches!(e, OnlineEvent::Lobby(_))))
        });
        ids
    }

    /// Readies the clients and waits until the host's lobby can start.
    pub fn ready(&mut self, clients: &[usize]) {
        for &c in clients {
            self.peers[c].peer().lobby().set_ready(true);
        }
        self.run_until(Duration::from_secs(10), "everyone is ready", |n| {
            n.host().peer().lobby().state().is_some_and(|s| {
                s.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).all(|m| m.ready)
            })
        });
    }

    pub fn host_tick(&mut self) -> u32 {
        self.host().peer().match_session().map_or(0, |s| s.current_tick())
    }
}

pub fn ended(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::MatchEnded(_))
}

pub fn replay_ready(e: &OnlineEvent) -> bool {
    matches!(e, OnlineEvent::ReplayReady(_))
}
