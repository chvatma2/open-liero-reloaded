//! Joining through routers (plan 12b, N45-N50, N54, PF12b-2): a client
//! without the host's address reaches it through Nostr relays (a
//! `TestRelay` on 127.0.0.1), the host's listener answers, the link opens
//! directly or through a local `TurnServer` found on Nostr, and the joins'
//! failures say what failed. Every endpoint's base is 127.0.0.1
//! (`base_override`), and every datagram stays on 127.0.0.0/8 (test rule
//! NT-1): a fake router's external address is 127.0.0.2. Classic on Dirt
//! Level from `OLR_CONTENT_DIR` (skipped with a notice without it); every
//! wait is bounded, every test under 20 s.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use olr_bots::BotConfig;
use olr_content::Content;
use olr_discovery::announce::ANNOUNCE_TTL;
use olr_discovery::handshake::Asker;
use olr_discovery::nip44::{conversation_key, encrypt};
use olr_discovery::turn_note::{TURN_NOTE_V, TurnNote, turn_note};
use olr_discovery::{
    Event, Filter, KIND_HANDSHAKE, Misbehaviour as RelayMisbehaviour, Pool, PoolConfig, PoolEvent, Region,
    TestRelay,
};
use olr_lobby::{HostConfig, Identity};
use olr_modes::DeathmatchSettings;
use olr_net::endpoint::{CONNECT_TIMEOUT, EndpointConfig, IceConfig, MAX_SIGNALLED_PENDING};
use olr_net::link::{Link, LinkCandidates};
use olr_net::portmap::DROP_WAIT;
use olr_net::portmap::fake::{FakeGateway, Misbehaviour as GatewayMisbehaviour};
use olr_net::turn::{RelayPorts, TurnLimits, TurnServer, TurnServerConfig};
use olr_online::join::{
    Join, JoinStep, NO_RELAYS, NO_REPLY, NostrJoin, RELAYS_INVALID, RELAYS_REFUSED, no_path,
};
use olr_online::matches::{bot_wire, rules_wire, setup_packs};
use olr_online::reach::{HostListen, Listening};
use olr_online::{MigrationConfig, OnlineEvent, OnlinePeer, PeerContent, PeerRole, RelayJoin};
use olr_proto::{FINGERPRINT, Look, MatchSetupWire, PROTOCOL_VERSION, Path, PublicKey, SeatWire, Signal};
use olr_sim::Keys;

const MOD: &str = "olx-classic";
const LEVEL: &str = "olx-level-dirt-level";
const LOCALHOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
/// The fake router's external address: a loopback alias (NT-1).
const EXTERNAL: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 2);
const TURN_USER: &str = "relay-user";
const TURN_PASS: &str = "relay-pass";

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(std::path::Path::new(&dir)).unwrap())
}

fn identity() -> Identity {
    loop {
        if let Some(id) = Identity::from_secret(olr_net::random_bytes::<32>().unwrap()) {
            return id;
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn look(name: &str) -> Look {
    Look {
        name: name.into(),
        colour: [200, 80, 40],
        skin: "olx-skin-default".into(),
        skin_hash: olr_proto::NO_HASH.into(),
    }
}

/// Classic on Dirt Level, one bot, ending after `minutes`.
fn setup(content: &Content, minutes: f32) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        packs: setup_packs(content, MOD, LEVEL).unwrap(),
        rules: rules_wire(&DeathmatchSettings { time_limit: minutes, ..DeathmatchSettings::default() }),
        loadout: olr_proto::LoadoutWire::default(),
        bots: vec![bot_wire(&BotConfig::default(), look("Bot"))],
    }
}

/// Every endpoint's ICE: the base on 127.0.0.1, local relays allowed.
fn ice() -> IceConfig {
    IceConfig { base_override: Some(LOCALHOST), allow_local: true, ..IceConfig::default() }
}

fn endpoint(ice: IceConfig) -> EndpointConfig {
    EndpointConfig { port: 0, ice, ..EndpointConfig::default() }
}

fn pool(relays: &[&TestRelay]) -> PoolConfig {
    PoolConfig { relays: relays.iter().map(|r| r.url()).collect(), ..PoolConfig::default() }
}

fn lobby() -> HostConfig {
    HostConfig { name: "Through".into(), password: None, max_players: 8, max_spectators: 4, admins: vec![] }
}

/// A join through Nostr to `host` on `relays` (no TURN notes asked).
fn nostr(host: PublicKey, relays: PoolConfig) -> NostrJoin {
    NostrJoin { host, pool: relays, region: Region::Europe, turn_from_nostr: false, allow_local: true }
}

/// A peer of the test and what it reported, each event with when.
struct Peer {
    p: OnlinePeer,
    key: PublicKey,
    events: Vec<(Instant, OnlineEvent)>,
}

impl Peer {
    fn poll(&mut self) {
        let now = Instant::now();
        let key = self.key[0];
        let mut n = 0u32;
        let mut keys = |_: &olr_online::KeysFor| {
            n += 1;
            Keys(u16::from((n / 25) as u8 ^ key) & !Keys::CHANGE)
        };
        for e in self.p.poll(now, &mut keys, &mut |_, _, _| {}) {
            self.events.push((now, e));
        }
    }

    /// When the first event `f` holds for came.
    fn when(&self, f: impl Fn(&OnlineEvent) -> bool) -> Option<Instant> {
        self.events.iter().find(|(_, e)| f(e)).map(|(t, _)| *t)
    }

    fn has(&self, f: impl Fn(&OnlineEvent) -> bool) -> bool {
        self.when(f).is_some()
    }

    fn steps(&self) -> Vec<JoinStep> {
        self.events
            .iter()
            .filter_map(|(_, e)| match e {
                OnlineEvent::Joining(s) => Some(*s),
                _ => None,
            })
            .collect()
    }

    fn connected(&self) -> Option<(Path, SocketAddr)> {
        self.events.iter().find_map(|(_, e)| match e {
            OnlineEvent::Connected { path, remote } => Some((*path, *remote)),
            _ => None,
        })
    }

    fn ended_with(&self) -> Option<String> {
        self.events.iter().find_map(|(_, e)| match e {
            OnlineEvent::Disconnected(why) | OnlineEvent::Refused(why) => Some(why.clone()),
            _ => None,
        })
    }

    fn in_lobby(&mut self, members: usize) -> bool {
        self.p.lobby().state().is_some_and(|s| s.members.len() == members)
    }
}

/// A host listening on `relays` and its clients, polled together.
struct Net {
    peers: Vec<Peer>,
}

impl Net {
    fn new() -> Net {
        Net { peers: Vec::new() }
    }

    /// A host of `setup` with `ice`, listening as `listen`; its index.
    fn host(&mut self, content: Content, setup: MatchSetupWire, ice: IceConfig, listen: HostListen) -> usize {
        let me = identity();
        let key = me.public();
        let role = PeerRole::Host { lobby: lobby(), listen };
        let p = OnlinePeer::start(
            role,
            endpoint(ice),
            me,
            look("Host"),
            content,
            Some(setup),
            PeerContent::without_downloads("a test"),
        )
        .unwrap();
        self.peers.push(Peer { p, key, events: Vec::new() });
        self.peers.len() - 1
    }

    /// A client joining by `join` with `ice`; its index.
    fn client(
        &mut self,
        content: Content,
        join: Join,
        ice: IceConfig,
        expected_host: Option<PublicKey>,
    ) -> usize {
        self.client_with(content, join, endpoint(ice), expected_host)
    }

    /// A client joining by `join` with the endpoint `cfg`; its index.
    fn client_with(
        &mut self,
        content: Content,
        join: Join,
        cfg: EndpointConfig,
        expected_host: Option<PublicKey>,
    ) -> usize {
        let me = identity();
        let key = me.public();
        let role =
            PeerRole::Client { join, password: None, expected_host, migration: MigrationConfig::off() };
        let p = OnlinePeer::start(
            role,
            cfg,
            me,
            look("Anna"),
            content,
            None,
            PeerContent::without_downloads("a test"),
        )
        .unwrap();
        self.peers.push(Peer { p, key, events: Vec::new() });
        self.peers.len() - 1
    }

    fn poll(&mut self) {
        for p in &mut self.peers {
            p.poll();
        }
    }

    /// Polls until `done` holds; panics after `limit` with `what`.
    fn run_until(&mut self, limit: Duration, what: &str, mut done: impl FnMut(&mut Net) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < limit, "{what} within {limit:?}: {:?}", self.summary());
            self.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Each peer's events but the match's (for failure messages).
    fn summary(&self) -> Vec<Vec<String>> {
        self.peers
            .iter()
            .map(|p| {
                p.events
                    .iter()
                    .filter(|(_, e)| {
                        !matches!(
                            e,
                            OnlineEvent::Ticked(_) | OnlineEvent::Confirmed(_) | OnlineEvent::Restored(_)
                        )
                    })
                    .map(|(_, e)| format!("{e:?}").chars().take(200).collect())
                    .collect()
            })
            .collect()
    }
}

/// A host on `relay` (no port mapping, no STUN) and a client joining it
/// through Nostr with `join`'s other fields; returns the net, the host's
/// index and its key.
fn host_on(content: Content, relays: PoolConfig) -> (Net, usize, PublicKey) {
    let mut net = Net::new();
    let setup = setup(&content, 0.1);
    let h = net.host(content, setup, ice(), HostListen { pool: Some(relays), public_address: None });
    let key = net.peers[h].key;
    (net, h, key)
}

/// Waits until the client `c` is in the host's lobby (both see two members).
fn joined(net: &mut Net, h: usize, c: usize, limit: Duration) {
    net.run_until(limit, "the client joins the lobby", |n| {
        n.peers[c].connected().is_some() && n.peers[h].in_lobby(2) && n.peers[c].in_lobby(2)
    });
}

/// A closed 127.0.0.1 port: bound, then released.
fn closed_port() -> SocketAddr {
    let l = TcpListener::bind((LOCALHOST, 0)).unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    addr
}

#[test]
fn a_client_joins_through_nostr_without_an_address() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&relay]))) };
    let c = net.client(self::content().unwrap(), join, ice(), Some(key));
    joined(&mut net, h, c, Duration::from_secs(15));
    let steps = net.peers[c].steps();
    let relays = steps.iter().position(|s| *s == JoinStep::Relays).expect("Relays");
    let asking = steps.iter().position(|s| *s == JoinStep::Asking).expect("Asking");
    assert!(relays < asking, "{steps:?}");
    assert!(!steps.contains(&JoinStep::Direct), "no direct attempt: {steps:?}");
    let (path, remote) = net.peers[c].connected().unwrap();
    assert_eq!(path, Path::Direct);
    assert_eq!(remote, SocketAddr::from((LOCALHOST, net.peers[h].p.local_port())));
    assert_eq!(net.peers[c].p.connection(), Some((Path::Direct, remote)));
    // The host's member for it says how it came.
    let ckey = net.peers[c].key;
    let state = net.peers[h].p.lobby().state().unwrap().clone();
    let member = state.members.iter().find(|m| m.key == ckey).expect("the client's member");
    assert_eq!(member.path, Some(Path::Direct));
    assert!(net.peers[c].ended_with().is_none());
    // The host said it answered (FR12b-3).
    assert_eq!(relay_joins(&net.peers[h]), [RelayJoin::Answered]);
}

/// A host whose relays were edited listens on the new ones
/// (`OnlinePeer::listen_on`, plan 12b Task 9): its reachability says so
/// and a join through the new relay alone reaches it; the same relays
/// again change nothing.
#[test]
fn a_host_listens_on_edited_relays() {
    let Some(content) = content() else { return };
    let (old, new) = (TestRelay::start(), TestRelay::start());
    let (mut net, h, key) = host_on(content, pool(&[&old]));
    net.peers[h].p.listen_on(Some(pool(&[&new]))).unwrap();
    net.peers[h].p.listen_on(Some(pool(&[&new]))).unwrap();
    net.run_until(Duration::from_secs(10), "listening on the new relay", |n| {
        n.peers[h].p.reachability().is_some_and(|r| r.listening == Listening::On { relays: 1, of: 1 })
    });
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&new]))) };
    let c = net.client(self::content().unwrap(), join, ice(), Some(key));
    joined(&mut net, h, c, Duration::from_secs(15));
    // No relay: no longer listening.
    net.peers[h].p.listen_on(None).unwrap();
    assert_eq!(net.peers[h].p.reachability().unwrap().listening, Listening::Off);
}

/// The TURN relay's own loopback address: not the host's (127.0.0.1), so
/// the relayed line can name the wrong one and fail (ruling FR12b-4).
const TURN_AT: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 3);

#[test]
fn a_client_joins_through_a_turn_relay_found_on_nostr() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let turn = TurnServer::start(TurnServerConfig {
        listen: (TURN_AT, 0).into(),
        public: TURN_AT.into(),
        ports: RelayPorts::Ephemeral,
        user: TURN_USER.into(),
        pass: TURN_PASS.into(),
        limits: TurnLimits::generous(),
        allow_local: true,
    })
    .unwrap();
    // The relay's operator announces it.
    let SocketAddr::V4(at) = turn.local_addr() else { panic!("IPv4") };
    let note = TurnNote {
        v: TURN_NOTE_V,
        addr: *at.ip(),
        port: at.port(),
        region: Region::Europe,
        capacity: 32,
        load: 0,
        user: TURN_USER.into(),
        pass: TURN_PASS.into(),
    };
    let note = turn_note(&identity(), &note, unix_now(), ANNOUNCE_TTL, [3; 32]);
    let mut announcer = Pool::start(pool(&[&relay]));
    announcer.publish(note.clone());
    let begun = Instant::now();
    while !relay.events().iter().any(|e| e.id == note.id) {
        assert!(begun.elapsed() < Duration::from_secs(5), "the note is stored");
        std::thread::sleep(Duration::from_millis(5));
    }

    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    let mut join = nostr(key, pool(&[&relay]));
    join.turn_from_nostr = true;
    let c = net.client(
        self::content().unwrap(),
        Join { direct: None, nostr: Some(join) },
        IceConfig { relay_only: true, ..ice() },
        Some(key),
    );
    joined(&mut net, h, c, Duration::from_secs(15));
    let (path, remote) = net.peers[c].connected().unwrap();
    assert_eq!(path, Path::Relayed);
    assert_eq!(remote, turn.local_addr(), "the TURN relay's address, not the host's");
    assert!(
        net.peers[c].has(|e| *e == OnlineEvent::Connected { path, remote: turn.local_addr() }),
        "the event names the relay too"
    );
    assert!(turn.stats().allocations >= 1, "{:?}", turn.stats());

    // A 6 s match over the relay, to equal confirmed checksums.
    net.peers[c].p.lobby().set_ready(true);
    net.run_until(Duration::from_secs(5), "the client is ready", |n| {
        n.peers[h].p.lobby().state().is_some_and(|s| {
            s.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).all(|m| m.ready)
        })
    });
    net.peers[h].p.lobby().start(3).unwrap();
    net.run_until(Duration::from_secs(16), "both keep the match's record", |n| {
        n.peers.iter_mut().all(|p| p.p.last_record().is_some())
    });
    let host = net.peers[h].p.last_record().cloned().unwrap();
    let client = net.peers[c].p.last_record().cloned().unwrap();
    assert!(host.rows.len() > 600, "{} ticks", host.rows.len());
    assert!(!host.checksums.is_empty());
    assert_eq!(client.rows, host.rows);
    assert_eq!(client.checksums, host.checksums);
    assert!(turn.stats().relayed_bytes > 0);
}

#[test]
fn a_lost_offer_is_sent_again() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    // The host's listener is up before the losses are counted.
    net.run_until(Duration::from_secs(5), "the host listens", |n| {
        n.peers[h].p.reachability().is_some_and(|r| r.listening == Listening::On { relays: 1, of: 1 })
    });
    relay.set_misbehaviour(RelayMisbehaviour::DropEphemeral(2));
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&relay]))) };
    let started = Instant::now();
    let c = net.client(self::content().unwrap(), join, ice(), Some(key));
    net.run_until(Duration::from_secs(8), "connected despite two lost offers", |n| {
        n.peers[c].connected().is_some()
    });
    assert!(
        started.elapsed() >= Duration::from_secs(4),
        "the third offer got through: {:?}",
        started.elapsed()
    );
    joined(&mut net, h, c, Duration::from_secs(5));
}

#[test]
fn a_relay_that_never_forwards_is_survived() {
    let Some(content) = content() else { return };
    let (deaf, honest) = (TestRelay::start(), TestRelay::start());
    deaf.set_misbehaviour(RelayMisbehaviour::NoForward);
    let both = pool(&[&deaf, &honest]);
    let (mut net, h, key) = host_on(content, both.clone());
    let join = Join { direct: None, nostr: Some(nostr(key, both)) };
    // No `expected_host`: a join through Nostr expects its host's key.
    let c = net.client(self::content().unwrap(), join, ice(), None);
    joined(&mut net, h, c, Duration::from_secs(15));
    assert_eq!(net.peers[c].connected().unwrap().0, Path::Direct);
}

/// The host answered, but no path opens (a client offering only relayed
/// candidates, with no TURN server): `no_path` names the host's port, once
/// the setup's time (a 3 s test hook instead of 15 s) is up.
#[test]
fn a_join_without_a_path_says_what_to_do() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let mut net = Net::new();
    let setup = setup(&content, 0.1);
    let three = IceConfig { setup_timeout: Some(Duration::from_secs(3)), ..ice() };
    let h = net.host(content, setup, three, HostListen { pool: Some(pool(&[&relay])), public_address: None });
    let key = net.peers[h].key;
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&relay]))) };
    let pathless = IceConfig { relay_only: true, setup_timeout: Some(Duration::from_secs(3)), ..ice() };
    let c = net.client(self::content().unwrap(), join, pathless, Some(key));
    net.run_until(Duration::from_secs(12), "the join gives up", |n| n.peers[c].ended_with().is_some());
    let port = net.peers[h].p.local_port();
    assert_eq!(net.peers[c].ended_with(), Some(no_path(port, None)));
    assert!(net.peers[c].steps().contains(&JoinStep::Connecting), "the host answered");
    assert!(net.peers[c].connected().is_none());
    // The host said so too (FR12b-3).
    net.run_until(Duration::from_secs(2), "the host's line", |n| {
        relay_joins(&n.peers[h]).contains(&RelayJoin::NotConnected)
    });
    assert_eq!(relay_joins(&net.peers[h]), [RelayJoin::Answered, RelayJoin::NotConnected]);
}

/// A host refusing the offer (another simulation fingerprint): the join
/// says why, and never reports `Connecting`.
#[test]
fn a_refused_join_says_why() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&relay]))) };
    let other = EndpointConfig { fingerprint_override: Some(FINGERPRINT ^ 1), ..endpoint(ice()) };
    let c = net.client_with(self::content().unwrap(), join, other, Some(key));
    net.run_until(Duration::from_secs(10), "the join is refused", |n| n.peers[c].ended_with().is_some());
    let p = &net.peers[c];
    assert!(p.has(|e| matches!(e, OnlineEvent::Refused(_))), "{:?}", p.ended_with());
    assert_eq!(p.ended_with().as_deref(), Some("the host plays another version"));
    assert!(!p.steps().contains(&JoinStep::Connecting), "{:?}", p.steps());
    assert_eq!(relay_joins(&net.peers[h]), [RelayJoin::Refused(olr_proto::Refusal::Version)]);
}

/// The lines the host said about joins through the relays (FR12b-3).
fn relay_joins(p: &Peer) -> Vec<RelayJoin> {
    p.events
        .iter()
        .filter_map(|(_, e)| match e {
            OnlineEvent::RelayJoin { join, .. } => Some(join.clone()),
            _ => None,
        })
        .collect()
}

/// Ruling FR12b-3 (final review B-I1): a joiner whose clock is 2 minutes
/// ahead (its handshake dated so) is never answered; the host says why,
/// naming the clocks, and the joiner's text names them too.
#[test]
fn a_joiner_whose_clock_is_off_is_told_on_both_sides() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&relay]))) };
    let c = net.client(self::content().unwrap(), join, ice(), Some(key));
    net.peers[c].p.test_clock_offset(120);
    net.run_until(Duration::from_secs(16), "the join gives up", |n| n.peers[c].ended_with().is_some());
    assert_eq!(net.peers[c].ended_with().as_deref(), Some(NO_REPLY));
    assert!(NO_REPLY.contains("clock"), "{NO_REPLY}");
    let said = relay_joins(&net.peers[h]);
    let skew = said.iter().find_map(|j| match j {
        RelayJoin::Skewed { by } => Some(*by),
        _ => None,
    });
    assert!(skew.is_some_and(|by| (115..=125).contains(&by)), "{said:?}");
    let line = RelayJoin::Skewed { by: skew.unwrap() }.line(0);
    assert!(line.contains("ahead of this PC's clock: check both PCs' clocks"), "{line}");
    assert!(!said.contains(&RelayJoin::Answered), "{said:?}");
}

#[test]
fn a_game_that_never_answers_says_so() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let mut net = Net::new();
    let nobody = identity().public();
    let join = Join { direct: None, nostr: Some(nostr(nobody, pool(&[&relay]))) };
    let c = net.client(content, join, ice(), Some(nobody));
    net.run_until(Duration::from_secs(16), "the join gives up", |n| n.peers[c].ended_with().is_some());
    let p = &net.peers[c];
    assert_eq!(p.ended_with().as_deref(), Some(NO_REPLY));
    let asking = p.when(|e| *e == OnlineEvent::Joining(JoinStep::Asking)).expect("asked");
    let gave_up = p.when(|e| matches!(e, OnlineEvent::Disconnected(_))).unwrap();
    let after = gave_up - asking;
    assert!(
        (Duration::from_secs(9)..=Duration::from_secs(11)).contains(&after),
        "NO_REPLY {after:?} after asking"
    );
}

#[test]
fn relays_refusing_handshakes_are_named() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    relay.set_misbehaviour(RelayMisbehaviour::RefuseKind(KIND_HANDSHAKE));
    let mut net = Net::new();
    let host = identity().public();
    let join = Join { direct: None, nostr: Some(nostr(host, pool(&[&relay]))) };
    let c = net.client(content, join, ice(), Some(host));
    net.run_until(Duration::from_secs(8), "the join gives up", |n| n.peers[c].ended_with().is_some());
    assert_eq!(net.peers[c].ended_with().as_deref(), Some(RELAYS_REFUSED));
}

/// Final review B-M2: a relay refusing the join's events as invalid (here
/// expired: the joiner's clock is 2 minutes behind, so its events expire
/// at once) is not blamed on web of trust: the text asks after the clock.
#[test]
fn invalid_handshakes_ask_after_the_clock() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let mut net = Net::new();
    let host = identity().public();
    let join = Join { direct: None, nostr: Some(nostr(host, pool(&[&relay]))) };
    let c = net.client(content, join, ice(), Some(host));
    net.peers[c].p.test_clock_offset(-120);
    net.run_until(Duration::from_secs(8), "the join gives up", |n| n.peers[c].ended_with().is_some());
    assert_eq!(net.peers[c].ended_with().as_deref(), Some(RELAYS_INVALID));
    assert!(RELAYS_INVALID.contains("clock"), "{RELAYS_INVALID}");
}

#[test]
fn no_relay_reachable_says_so() {
    let Some(content) = content() else { return };
    let mut net = Net::new();
    let host = identity().public();
    let closed = PoolConfig { relays: vec![format!("ws://{}", closed_port())], ..PoolConfig::default() };
    let join = Join { direct: None, nostr: Some(nostr(host, closed)) };
    let c = net.client(content, join, ice(), Some(host));
    net.run_until(Duration::from_secs(6), "the join gives up", |n| n.peers[c].ended_with().is_some());
    assert_eq!(net.peers[c].ended_with().as_deref(), Some(NO_RELAYS));
}

#[test]
fn direct_failure_falls_back_to_nostr() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    let join = Join { direct: Some(closed_port()), nostr: Some(nostr(key, pool(&[&relay]))) };
    let c = net.client(self::content().unwrap(), join, ice(), Some(key));
    joined(&mut net, h, c, Duration::from_secs(15));
    let steps = net.peers[c].steps();
    assert_eq!(steps.first(), Some(&JoinStep::Direct), "{steps:?}");
    assert!(steps.contains(&JoinStep::Relays) && steps.contains(&JoinStep::Asking), "{steps:?}");
    assert_eq!(net.peers[c].connected().unwrap().0, Path::Direct);
}

/// Review Focus 5: a joiner behind the host's router dials the host's
/// public address, which the router does not hairpin (the connect never
/// completes: olr-net's `connect_stall` hook, so nothing leaves 127.0.0.1,
/// NT-1). After the direct attempt's `CONNECT_TIMEOUT` it joins through
/// Nostr, and ICE finds the host's LAN address: a direct path.
#[test]
fn a_same_lan_joiner_without_hairpinning_falls_back_and_connects_directly() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    let join = Join { direct: Some(closed_port()), nostr: Some(nostr(key, pool(&[&relay]))) };
    let started = Instant::now();
    let stalled = EndpointConfig { connect_stall: true, ..endpoint(ice()) };
    let c = net.client_with(self::content().unwrap(), join, stalled, Some(key));
    joined(&mut net, h, c, Duration::from_secs(16));
    let p = &net.peers[c];
    assert_eq!(p.steps().first(), Some(&JoinStep::Direct));
    let fell_back = p.when(|e| *e == OnlineEvent::Joining(JoinStep::Relays)).unwrap();
    assert!(fell_back - started >= CONNECT_TIMEOUT, "Nostr only after the direct attempt timed out");
    assert_eq!(
        p.connected().unwrap(),
        (Path::Direct, SocketAddr::from((LOCALHOST, net.peers[h].p.local_port())))
    );
}

/// The SDP of an offerer `Link` that is never pumped: a joiner that never
/// sends a connectivity check (its candidate a silent 127.0.0.1 socket).
fn silent_offer(at: SocketAddr, links: &mut Vec<Link>) -> Signal {
    let (link, sdp) =
        Link::offerer_with(&LinkCandidates { host: vec![at], ..LinkCandidates::default() }, Instant::now());
    links.push(link);
    Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: FINGERPRINT, sdp }
}

/// A handshake event addressed to `host` from a fresh key, with `content`.
fn addressed(host: &PublicKey, content: String, unix: u64) -> Event {
    let tags =
        vec![vec!["p".to_owned(), hex::encode(host)], vec!["expiration".to_owned(), (unix + 60).to_string()]];
    Event::sign(&identity(), unix, KIND_HANDSHAKE, tags, content, olr_net::random_bytes::<32>().unwrap())
}

/// One undecryptable handshake addressed to `host`, of kind `i % 3`: a
/// forged signature (a relay refuses it), a valid offer's content tampered
/// and signed again (its MAC fails), or content encrypted to another key.
fn junk(host: &PublicKey, i: u64, at: SocketAddr, links: &mut Vec<Link>) -> Event {
    let unix = unix_now();
    let mut rand = || olr_net::random_bytes::<32>().unwrap();
    match i % 3 {
        0 => {
            let mut forged = addressed(host, format!("forged {i}"), unix);
            forged.sig[0] ^= 1;
            forged
        }
        1 => {
            let mut a = Asker::new(*host, silent_offer(at, links), Instant::now(), unix, &mut rand);
            let mut tampered = a.poll(Instant::now(), unix, &mut rand).unwrap().content;
            let flipped = if &tampered[10..11] == "A" { "B" } else { "A" };
            tampered.replace_range(10..11, flipped);
            addressed(host, tampered, unix)
        }
        _ => {
            let ck = conversation_key(&identity().secret(), &identity().public()).unwrap();
            addressed(host, encrypt(&ck, "{\"v\":1}", rand()).unwrap(), unix)
        }
    }
}

/// What a flood sent and heard: the host's replies to its offers.
struct Flooded {
    replies: Vec<Signal>,
    junk: u64,
    /// How many of the host's links sent checks to the offers' candidates.
    checked: usize,
    /// How many of the host's answers to valid checks from the forged
    /// source (made with its own answer's credentials) reached it.
    forged_answered: usize,
}

/// For `length`: 100 undecryptable handshakes a second to `host` on
/// `relay`, and after every 20 of them (junk first) a valid offer from a
/// fresh key (5 a second). Its joiner never answers the host's checks, but
/// once the host's answer arrives it sends valid connectivity checks (it
/// holds the answer's credentials) from a forged source: another socket
/// than its candidate, which never answers either (FR12b-1).
fn flood(relay: PoolConfig, host: PublicKey, length: Duration) -> std::thread::JoinHandle<Flooded> {
    std::thread::spawn(move || {
        let quiet = UdpSocket::bind((LOCALHOST, 0)).unwrap();
        let forged = UdpSocket::bind((LOCALHOST, 0)).unwrap();
        forged.set_nonblocking(true).unwrap();
        let at = quiet.local_addr().unwrap();
        let mut links = Vec::new();
        let mut rand = || olr_net::random_bytes::<32>().unwrap();
        let silent = (length.as_secs() as usize + 1) * 5;
        let (t0, unix) = (Instant::now(), unix_now());
        let mut offers: Vec<(Asker, Link, bool)> = (0..silent)
            .map(|_| {
                let mut one = Vec::new();
                let asker = Asker::new(host, silent_offer(at, &mut one), t0, unix, &mut rand);
                (asker, one.pop().unwrap(), false)
            })
            .collect();
        let mut pool = Pool::start(relay);
        let replies = Filter {
            kinds: vec![KIND_HANDSHAKE],
            authors: vec![host],
            tags: vec![('p', offers.iter().map(|(a, ..)| hex::encode(a.key())).collect())],
            ..Filter::default()
        };
        pool.subscribe("replies", vec![replies]);
        let mut heard = Vec::new();
        while !heard.iter().any(|e| matches!(e, PoolEvent::Eose { .. })) {
            assert!(t0.elapsed() < Duration::from_secs(5), "the flood's pool subscribes");
            pool.poll(&mut heard);
            std::thread::sleep(Duration::from_millis(1));
        }
        let (mut answers, mut seen) = (Vec::new(), heard.len());
        let begun = Instant::now();
        let (mut sent, mut next) = (0u64, 0usize);
        let mut transmits = Vec::new();
        while begun.elapsed() < length {
            // On schedule: 100 a second.
            while sent < (begun.elapsed().as_millis() / 10) as u64 {
                pool.publish(junk(&host, sent, at, &mut links));
                sent += 1;
                if sent % 20 == 0
                    && let Some((a, ..)) = offers.get_mut(next)
                {
                    next += 1;
                    pool.publish(a.poll(Instant::now(), unix_now(), &mut rand).unwrap());
                }
            }
            pool.poll(&mut heard);
            for e in &heard[seen..] {
                let PoolEvent::Event { event, .. } = e else { continue };
                for (a, link, answered) in &mut offers {
                    if let Some(reply) = a.take(event) {
                        if let Signal::Answer { sdp } = &reply.signal {
                            *answered = link.accept_answer(sdp).is_ok();
                        }
                        answers.push(reply.signal);
                    }
                }
            }
            seen = heard.len();
            // The answered offers' checks leave from the forged source.
            for (_, link, _) in offers.iter_mut().filter(|(.., answered)| *answered) {
                link.poll(Instant::now(), &mut transmits, &mut Vec::new());
                for t in transmits.drain(..) {
                    let _ = forged.send_to(&t.bytes, t.to);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let quiet_end = Instant::now();
        while quiet_end.elapsed() < Duration::from_millis(500) {
            pool.poll(&mut heard);
            std::thread::sleep(Duration::from_millis(5));
        }
        for e in &heard[seen..] {
            let PoolEvent::Event { event, .. } = e else { continue };
            answers.extend(offers.iter_mut().find_map(|(a, ..)| a.take(event)).map(|r| r.signal));
        }
        let forged_answered = binding_successes(&forged);
        drop((links, offers));
        Flooded { replies: answers, junk: sent, checked: check_usernames(&quiet).len(), forged_answered }
    })
}

/// PF12b-2, FR12b-1 and Review Focus 1: for 8 s, 100 forged or
/// undecryptable handshakes a second and, among them (junk first), 5 valid
/// offers a second from fresh keys (enough to fill `MAX_SIGNALLED_PENDING`
/// many times over), each sending valid connectivity checks from a forged
/// source once answered but never answering the host's, reach the host's
/// relay. An honest client starting 3 s into the flood (the pending table
/// full of checked offers) connects during it.
#[test]
fn a_flood_of_offers_leaves_room_for_an_honest_joiner() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let (mut net, h, key) = host_on(content, pool(&[&relay]));
    net.run_until(Duration::from_secs(5), "the host listens", |n| {
        n.peers[h].p.reachability().is_some_and(|r| r.listening == Listening::On { relays: 1, of: 1 })
    });
    let length = Duration::from_secs(8);
    let flooding = flood(pool(&[&relay]), key, length);
    let begun = Instant::now();
    net.run_until(Duration::from_secs(5), "3 s of flood", |_| begun.elapsed() >= Duration::from_secs(3));
    let join = Join { direct: None, nostr: Some(nostr(key, pool(&[&relay]))) };
    let c = net.client(self::content().unwrap(), join, ice(), Some(key));
    joined(&mut net, h, c, Duration::from_secs(5));
    let at = net.peers[c].when(|e| matches!(e, OnlineEvent::Connected { .. })).unwrap();
    assert!(at < begun + length, "connected during the flood, {:?} in", at - begun);
    assert_eq!(net.peers[c].connected().unwrap().0, Path::Direct);
    // The flood ends; the host stays well.
    net.run_until(Duration::from_secs(12), "the flood ends", |_| flooding.is_finished());
    let flooded = flooding.join().unwrap();
    assert!(flooded.junk >= 700, "{} junk events", flooded.junk);
    // The host answered more offers than its pending table holds (PF12b-2
    // evicted the oldest unproven ones; without FR12b-1, the forged checks
    // would protect them and the honest joiner would be refused `Busy`).
    let answers = flooded.replies;
    assert!(answers.len() > MAX_SIGNALLED_PENDING, "{} flood offers answered", answers.len());
    assert!(answers.iter().all(|s| matches!(s, Signal::Answer { .. })), "{answers:?}");
    assert!(flooded.checked >= MAX_SIGNALLED_PENDING, "{} links checked their joiners", flooded.checked);
    assert!(
        flooded.forged_answered >= MAX_SIGNALLED_PENDING,
        "{} forged checks taken by the host",
        flooded.forged_answered
    );
    assert_eq!(
        net.peers[h].p.lobby().state().unwrap().members.len(),
        2,
        "only the honest joiner is a member"
    );
}

/// How many STUN binding success responses reached `socket` (the host's
/// answers to checks sent from it).
fn binding_successes(socket: &UdpSocket) -> usize {
    socket.set_nonblocking(true).unwrap();
    let (mut n, mut buf) = (0, [0u8; 2048]);
    while let Ok((len, _)) = socket.recv_from(&mut buf) {
        if len >= 20 && buf[..2] == [1, 1] && buf[4..8] == [0x21, 0x12, 0xa4, 0x42] {
            n += 1;
        }
    }
    n
}

/// The ICE usernames of the STUN binding requests that reached `socket`
/// (each answering link's own).
fn check_usernames(socket: &UdpSocket) -> BTreeSet<Vec<u8>> {
    socket.set_nonblocking(true).unwrap();
    let mut names = BTreeSet::new();
    let mut buf = [0u8; 2048];
    while let Ok((n, _)) = socket.recv_from(&mut buf) {
        let msg = &buf[..n];
        // A binding request: type 0x0001, the magic cookie, then attributes.
        if n < 20 || msg[..2] != [0, 1] || msg[4..8] != [0x21, 0x12, 0xa4, 0x42] {
            continue;
        }
        let mut at = 20;
        while at + 4 <= n {
            let kind = u16::from_be_bytes([msg[at], msg[at + 1]]);
            let len = usize::from(u16::from_be_bytes([msg[at + 2], msg[at + 3]]));
            let Some(value) = msg.get(at + 4..at + 4 + len) else { break };
            if kind == 0x0006 {
                names.insert(value.to_vec());
            }
            at += 4 + len.div_ceil(4) * 4;
        }
    }
    names
}

/// The fake's PCP/NAT-PMP address, for the `gateway` hook.
fn pmp(fake: &FakeGateway) -> SocketAddrV4 {
    match fake.pmp() {
        SocketAddr::V4(a) => a,
        other => panic!("the fake listens on IPv4: {other}"),
    }
}

/// A host mapping its port on `fake` (UPnP through the hooks) and listening
/// on `relay`.
fn mapping_host(content: Content, fake: &FakeGateway, relay: &TestRelay) -> (Net, usize) {
    let mut net = Net::new();
    let setup = setup(&content, 0.1);
    let ice = IceConfig { port_mapping: true, gateway: Some(pmp(fake)), ssdp: Some(fake.ssdp()), ..ice() };
    let h = net.host(content, setup, ice, HostListen { pool: Some(pool(&[relay])), public_address: None });
    (net, h)
}

#[test]
fn the_host_reports_its_reachability() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let fake = FakeGateway::start(true, false, false, EXTERNAL, GatewayMisbehaviour::Honest);
    let (mut net, h) = mapping_host(content, &fake, &relay);
    let port = net.peers[h].p.local_port();
    let open = format!("Port {port} is open on your router (UPnP)");
    let listening = "Taking joins through 1 of 1 relays";
    net.run_until(Duration::from_secs(8), "the reachability lines", |n| {
        n.peers[h].has(|e| {
            matches!(e, OnlineEvent::Reachability(r)
                if r.lines().iter().any(|l| l.starts_with(&open)) && r.lines().iter().any(|l| l == listening))
        })
    });
    let reach = net.peers[h].p.reachability().unwrap();
    assert_eq!(reach.port, port);
    assert_eq!(reach.direct(), Some(SocketAddr::from((EXTERNAL, port))));
    assert!(reach.settled());
    assert_eq!(
        reach.lines(),
        vec![format!("{open}: players join directly at {EXTERNAL}:{port}"), listening.to_string()]
    );
    // Reported when the lines change, not every poll.
    let reports =
        net.peers[h].events.iter().filter(|(_, e)| matches!(e, OnlineEvent::Reachability(_))).count();
    assert!(reports <= 6, "{reports} reports");
    // A client has none.
    assert!(net.peers[h].p.connection().is_none());
}

/// NT-2: a host's shutdown returns at once, and waiting on it returns once
/// the router deleted the mapping, within a second.
#[test]
fn a_host_shuts_down_and_its_mapping_goes() {
    let Some(content) = content() else { return };
    let relay = TestRelay::start();
    let fake = FakeGateway::start(true, false, false, EXTERNAL, GatewayMisbehaviour::SlowDelete);
    let (mut net, h) = mapping_host(content, &fake, &relay);
    net.run_until(Duration::from_secs(8), "the mapping", |n| {
        n.peers[h].p.reachability().is_some_and(|r| r.direct().is_some())
    });
    assert_eq!(fake.mappings().len(), 2, "TCP and UDP");
    let host = net.peers.remove(h);
    let started = Instant::now();
    let shutdown = host.p.shutdown();
    assert!(started.elapsed() < Duration::from_millis(200), "shutdown blocked for {:?}", started.elapsed());
    assert!(shutdown.wait(Instant::now() + DROP_WAIT + Duration::from_millis(200)), "deleted in time");
    assert_eq!(fake.mappings(), vec![]);
}
