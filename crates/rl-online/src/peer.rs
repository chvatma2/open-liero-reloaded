//! One end of online play over real connections (networking spec §4): the
//! host's lobby and its peers, or a client and its host; the match each
//! start builds, its advance every poll, its end, and back to the lobby.
//!
//! Every reliable payload carries `rl_proto`'s tag byte: `0` the netcode's
//! `Control` (the session sends it untagged: tagged and untagged here), `1`
//! `ToHost`, `2` `ToClient`, `3` `ContentMsg` (the content side,
//! `content::peer_side`). Time is the caller's (`poll(now, ..)`); the
//! sockets are `rl-net`'s endpoint.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use rl_content::Content;
use rl_lobby::{
    ClientEvent, HostConfig, HostEvent, Identity, LobbyClient, LobbyError, LobbyHost, NotStartable,
    sanitize_chat,
};
use rl_net::endpoint::{Endpoint, EndpointConfig, Gate, HOST, NetEvent, Why};
use rl_net::random_bytes;
use rl_netcode::world_sim::TickRecorder;
use rl_netcode::{Channel, Config, Driver, Event, PeerId, Role, Seat, Session, TickInfo, WorldSim};
use rl_proto::{
    AdminCommand, Control, LobbyState, Look, MAX_INPUT_DELAY, MatchEndWire, MatchSetupWire, MatchStartWire,
    Notice, Path, Phase, PublicKey, Refusal, SeatWire, TAG_CONTENT, TAG_NETCODE, TAG_TO_CLIENT, TAG_TO_HOST,
    Tagged, ToClient, ToHost, decode_content, decode_control, decode_to_client, decode_to_host,
    encode_content, encode_control, encode_to_client, encode_to_host, tag,
};
use rl_sim::{EndReason, Keys, MatchResult, Score, TickEvents, World};

use crate::bundle::{GameBundle, MAX_BUNDLES_PER_MATCH};
use crate::content::peer_side::ContentSide;
use crate::content::{Download, PeerContent};
use crate::events::{NetStats, OnlineEvent};
use crate::join::{BUSY, Join, Joiner, Lost};
use crate::matches::{self, HostBots};
use crate::migration::MigrationConfig;
use crate::outbox::{Outbox, Part};
use crate::reach::{HostListen, HostReach, Reachability, RelayJoin};
use crate::replay::{MatchRecord, PendingReplay};

mod lobby_handle;
mod migrate;
pub use lobby_handle::LobbyHandle;

/// The host sends its lobby's messages at most this often: changes in
/// between are coalesced into one `Lobby` (a start, an end, a kick and a
/// mid-match join go out at once).
pub const LOBBY_DRAIN: Duration = Duration::from_millis(250);
/// The match goes on this many confirmed ticks after the tick it ended on
/// (the results in game, local play's `RECORD_AFTER_END`), then ends.
pub const RECORD_AFTER_END: u32 = 300;
/// After the end, the host keeps sending the match this long, so every
/// client confirms its last ticks.
pub const HOST_DRAIN: Duration = Duration::from_secs(2);
/// After the host's end, a client waits this long at most to confirm the
/// match's last ticks before it makes its replay.
pub const CLIENT_FINISH: Duration = Duration::from_secs(3);
/// A peer being closed gets this long to receive what was sent to it.
pub const CLOSE_LINGER: Duration = Duration::from_secs(1);
/// A client lets its host send this many reliable bytes per second (whole
/// lobbies, state transfers), far above the limit for other peers.
pub const HOST_RELIABLE_BYTES_PER_S: u32 = 4 * 1024 * 1024;
/// The host updates its members' pings this often.
pub const PING_EVERY: Duration = Duration::from_secs(2);
/// The start of the notice an admin gets for a setup the host refuses.
pub const SETUP_REFUSED: &str = "the host cannot use that setup";
/// Said when the host starts a match while one is running (ignored).
pub const START_IGNORED: &str = "the host started a match during the match: ignored";
/// Said when a match ends while the last one's replay is still being made.
pub const REPLAY_SKIPPED: &str = "the last replay is still being made: this match has none";
/// Why a client that cannot play the host's start leaves.
pub const MISMATCH: &str = "your simulation differs from the host's: version or content";
/// Why a client leaves a host that is not the one expected, or that does
/// not prove its key over the connection's DTLS fingerprints (N37).
pub const WRONG_HOST: &str = "this is not the game you were invited to";
/// Why a client leaves a host that does not prove its key when no key was
/// expected (a LAN or direct join: nobody was invited).
pub const UNPROVEN_HOST: &str = "this game could not prove who it is";

/// Why a client left a host that failed its proof: [`WRONG_HOST`] when a
/// key was `expected` (an invite's or a listed game's), else
/// [`UNPROVEN_HOST`].
/// A client's far end as its lines name it: the TURN relay a relayed link
/// goes through (the pair's remote end is then the host, ruling FR12b-4),
/// else the host's end `remote`.
fn far_end(ep: &Endpoint, path: Path, remote: SocketAddr) -> SocketAddr {
    match path {
        Path::Relayed => ep.relay_server(HOST).unwrap_or(remote),
        _ => remote,
    }
}

fn wrong_host_text(expected: bool) -> &'static str {
    if expected { WRONG_HOST } else { UNPROVEN_HOST }
}
/// A client gives up a host that has not welcomed it this long after the
/// link opened (a host that never sends `Challenge`, `HostProof` or
/// `Welcome`: the link's keepalives alone never time it out).
pub const JOIN_DEADLINE: Duration = Duration::from_secs(10);
/// Why a client gave up after [`JOIN_DEADLINE`].
pub const NO_ANSWER: &str = "the game did not answer";

/// Which end of online play a peer is. Made once per peer, so a large
/// `Client` (its migration's endpoint configurations) costs nothing.
#[allow(clippy::large_enum_variant)]
pub enum PeerRole {
    /// Hosts a lobby on the endpoint's port, listening on Nostr as
    /// `listen` says (N54).
    Host { lobby: HostConfig, listen: HostListen },
    /// Hosts a lobby with no local player (a dedicated server): the look
    /// given to [`OnlinePeer::start`] is not used, and the host plays only
    /// its bots.
    Server { lobby: HostConfig, listen: HostListen },
    /// Joins a host as `join` says (N46: its address, then Nostr); with
    /// `expected_host` (an invite's key), only a host proving that key
    /// (`WRONG_HOST` otherwise). A join through Nostr expects its host's
    /// key without it. Every host proves the key it names. `migration`:
    /// how it takes part when its host is lost during a match (spec §3.7;
    /// `MigrationConfig::off()`, the old ending).
    Client {
        join: Join,
        password: Option<String>,
        expected_host: Option<PublicKey>,
        migration: MigrationConfig,
    },
}

/// A peer shutting down (ruling NT-2): its endpoint's mapping is being
/// deleted on a thread.
#[must_use = "wait on it before the process exits, or the router may keep the mapping"]
pub struct Shutdown {
    endpoint: Option<rl_net::endpoint::Shutdown>,
}

impl Shutdown {
    /// Waits until the port mapping is deleted (at most
    /// `rl_net::portmap::DROP_WAIT`, 1 s) or `until`, whichever comes first.
    /// `true` when it was deleted, or there was none.
    pub fn wait(self, until: Instant) -> bool {
        self.endpoint.is_none_or(|s| s.wait(until))
    }

    /// The verdict if the deletion is over, never waiting (`Some(true)` at
    /// once without a mapping); `None` while the router is still asked.
    pub fn poll(&mut self) -> Option<bool> {
        self.endpoint.as_mut().map_or(Some(true), rl_net::endpoint::Shutdown::poll)
    }
}

/// Why the host could not start a match.
#[derive(Clone, Debug, PartialEq)]
pub enum StartError {
    /// Only the host starts.
    NotHost,
    /// A match is still running.
    Running,
    Lobby(NotStartable),
    /// The world could not be built (content).
    Build(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::NotHost => write!(f, "only the host starts the match"),
            StartError::Running => write!(f, "a match is running"),
            StartError::Lobby(n) => write!(f, "the lobby cannot start: {n}"),
            StartError::Build(e) => write!(f, "the match cannot be built: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

struct HostSide {
    lobby: LobbyHost,
    /// The endpoint's gate refuses these addresses (the lobby's bans).
    banned: Arc<Mutex<BTreeSet<IpAddr>>>,
    addrs: BTreeMap<PeerId, SocketAddr>,
    /// Measured round trips: the join handshake's, then the match's.
    rtts: BTreeMap<PeerId, u32>,
    /// When each pending peer's challenge left (the first round trip).
    challenged: BTreeMap<PeerId, Instant>,
    last_drain: Option<Instant>,
    last_ping: Option<Instant>,
    /// The lobby's revision last reported to the caller.
    reported: Option<u64>,
    /// Its listener on Nostr and its reachability (N47, N54).
    reach: HostReach,
}

impl HostSide {
    /// Forgets what the host kept of a closed peer's connection.
    fn forget(&mut self, peer: PeerId) {
        self.addrs.remove(&peer);
        self.rtts.remove(&peer);
        self.challenged.remove(&peer);
    }
}

struct ClientSide {
    lobby: LobbyClient,
    /// A host key was expected (an invite's or a listed game's).
    expected: bool,
    /// When the link opened, until the host's `Welcome`
    /// ([`JOIN_DEADLINE`]).
    waiting_since: Option<Instant>,
    /// The join, until the link opens.
    joiner: Option<Box<Joiner>>,
    /// How the link reaches the host, once open.
    connection: Option<(Path, SocketAddr)>,
    /// Its identity: a successor's joins and lobby (plan 12c).
    me: Identity,
    /// The endpoint configuration it joined with: a successor's joins'.
    cfg: EndpointConfig,
    migration: MigrationConfig,
    /// Its standby endpoint (N63), from its first welcome with migration on.
    standby: Option<Box<migrate::Standby>>,
    /// The migration under way, from the host's loss until a new host
    /// welcomed it (A9), it took over, or the old host was back (N69).
    migrating: Option<Box<migrate::Migrating>>,
    /// A new host welcomed it: that host's name, until its `Resume`.
    rejoined: Option<String>,
    /// The match's migrations so far, carried to the session of a member
    /// back after its new host's resume (N72, N74: its mid-match `Start`;
    /// cleared at the match's end, so the next match counts from 0).
    migrations: u32,
}

enum Side {
    Host(Box<HostSide>),
    Client(Box<ClientSide>),
}

/// The counters a second ago, for [`NetStats`].
#[derive(Default)]
struct Window {
    at: Option<Instant>,
    tick: u32,
    datagrams_in: u64,
    corrections: u64,
    depth_hist: Vec<u64>,
}

/// A running match (decision N18: one session per match).
struct Game {
    session: Session<WorldSim>,
    recorder: TickRecorder,
    /// The host's bots.
    bots: Option<HostBots>,
    start: MatchStartWire,
    my_slot: Option<u8>,
    /// The lobby heard of the first tick.
    playing: bool,
    /// The last tick the match records: its end plus [`RECORD_AFTER_END`].
    end_at: Option<u32>,
    /// The latest confirmed scores and result.
    last: Option<(Vec<Score>, Option<MatchResult>)>,
    /// When the match ended (the host's `Ended`).
    ended: Option<Instant>,
    window: Window,
    /// Desync bundles made this match ([`MAX_BUNDLES_PER_MATCH`] at most).
    bundles: u32,
    /// Migrations this peer went through in this match (N74).
    migrations: u32,
    /// A host that took the match over: its window's end (the wait for
    /// members not yet back, N68), until its resume.
    collecting: Option<u64>,
}

/// One end of online play: the lobby, the match, the replay.
pub struct OnlinePeer {
    /// The endpoint behind the reliable backlog (`outbox`).
    endpoint: Outbox,
    side: Side,
    /// The content: what this peer has, downloads and serves
    /// (`content::peer_side`).
    content: ContentSide,
    my_key: PublicKey,
    epoch: Option<Instant>,
    now: Option<Instant>,
    now_us: u64,
    game: Option<Game>,
    /// The replay being made (one at a time).
    replay: Option<PendingReplay>,
    last_record: Option<MatchRecord>,
    /// The match's world as last presented, kept when it ended (until
    /// taken: [`OnlinePeer::take_ended_world`]).
    ended_world: Option<World>,
    /// Member names by id, as last seen (members who left keep theirs).
    names: BTreeMap<u8, String>,
    /// Peers to close once what was sent to them is delivered (or at the deadline).
    closing: BTreeMap<PeerId, Instant>,
    events: Vec<OnlineEvent>,
    stats: NetStats,
    /// Left, refused, kicked or disconnected: nothing more happens.
    closed: bool,
    test_seed: Option<u32>,
    /// The last poll's time, monotonic and wall-clock (a host's step-down,
    /// N70).
    last_poll: Option<(Instant, SystemTime)>,
    /// Tests only (`test-hooks`): every endpoint drops every datagram until
    /// then (`test_outage`).
    #[cfg(feature = "test-hooks")]
    outage: Option<Instant>,
    /// Tests only (`test-hooks`): `test_delay_rejoin`'s delay.
    #[cfg(feature = "test-hooks")]
    rejoin_delay: Duration,
    /// Tests only (`test-hooks`): `test_withhold_reports`.
    #[cfg(feature = "test-hooks")]
    withhold_reports: bool,
    /// Test hook: joins to successors skip direct addresses.
    #[cfg(feature = "test-hooks")]
    ignore_direct: bool,
    /// Test hook: the joins to successors started (`test_joins`).
    #[cfg(feature = "test-hooks")]
    joins: Vec<(u8, bool)>,
}

impl OnlinePeer {
    /// Opens the endpoint: a host's lobby for `setup` (`me` plays with
    /// `look`; a server's lobby has no local player), or a connection to
    /// the host. A client's endpoint lets its host send
    /// [`HOST_RELIABLE_BYTES_PER_S`]. `content` is this peer's own packs;
    /// `content_cfg` its download cache (or why there is none) and a
    /// host's upload rate.
    pub fn start(
        role: PeerRole,
        endpoint: EndpointConfig,
        me: Identity,
        look: Look,
        content: Content,
        setup: Option<MatchSetupWire>,
        content_cfg: PeerContent,
    ) -> io::Result<OnlinePeer> {
        let invalid = |what: String| io::Error::new(io::ErrorKind::InvalidInput, what);
        let my_key = me.public();
        let content = match &role {
            PeerRole::Host { .. } => ContentSide::new(content, content_cfg, true, Some(look.clone())),
            PeerRole::Server { .. } => ContentSide::new(content, content_cfg, true, None),
            PeerRole::Client { .. } => ContentSide::new(content, content_cfg, false, Some(look.clone())),
        };
        let hosting = match role {
            PeerRole::Host { lobby, listen } => Ok((lobby, listen, Some(look))),
            PeerRole::Server { lobby, listen } => Ok((lobby, listen, None)),
            PeerRole::Client { join, password, expected_host, migration } => {
                Err((join, password, expected_host, look, migration))
            }
        };
        let mut events = Vec::new();
        let (endpoint, side) = match hosting {
            Ok((cfg, listen, look)) => {
                let setup = setup.ok_or_else(|| invalid("a host needs a match setup".into()))?;
                let reach = HostReach::start(listen, &me)?;
                let lobby = LobbyHost::new(cfg, me, look, setup)
                    .map_err(|e: LobbyError| invalid(format!("the match setup: {e}")))?;
                let banned = Arc::new(Mutex::new(BTreeSet::new()));
                let gate_banned = Arc::clone(&banned);
                let gate: Gate = Box::new(move |addr, _| {
                    let banned = gate_banned.lock().unwrap_or_else(|e| e.into_inner());
                    if banned.contains(&addr.ip()) { Err(Refusal::Banned) } else { Ok(()) }
                });
                let side = HostSide {
                    lobby,
                    banned,
                    addrs: BTreeMap::new(),
                    rtts: BTreeMap::new(),
                    challenged: BTreeMap::new(),
                    last_drain: None,
                    last_ping: None,
                    reported: None,
                    reach,
                };
                (Some(Endpoint::host(endpoint, gate)?), Side::Host(Box::new(side)))
            }
            Err((join, password, expected_host, look, migration)) => {
                let mut cfg = endpoint;
                cfg.limits.max_reliable_bytes_per_s =
                    cfg.limits.max_reliable_bytes_per_s.max(HOST_RELIABLE_BYTES_PER_S);
                // A join through Nostr knows its host's key (N37's proof and
                // the encrypted answer both bind it).
                let expected_host = expected_host.or(join.nostr.as_ref().map(|n| n.host));
                let expected = expected_host.is_some();
                let (joiner, endpoint, step) = Joiner::start(join, cfg.clone())?;
                events.push(OnlineEvent::Joining(step));
                let lobby = LobbyClient::new(me.clone(), look, password, expected_host);
                let client = ClientSide {
                    lobby,
                    expected,
                    waiting_since: None,
                    joiner: Some(Box::new(joiner)),
                    connection: None,
                    me,
                    cfg,
                    migration,
                    standby: None,
                    migrating: None,
                    rejoined: None,
                    migrations: 0,
                };
                (endpoint, Side::Client(Box::new(client)))
            }
        };
        Ok(OnlinePeer {
            endpoint: endpoint.map_or_else(Outbox::empty, Outbox::new),
            side,
            content,
            my_key,
            epoch: None,
            now: None,
            now_us: 0,
            game: None,
            replay: None,
            last_record: None,
            ended_world: None,
            names: BTreeMap::new(),
            closing: BTreeMap::new(),
            events,
            stats: NetStats::default(),
            closed: false,
            test_seed: None,
            last_poll: None,
            #[cfg(feature = "test-hooks")]
            outage: None,
            #[cfg(feature = "test-hooks")]
            rejoin_delay: Duration::ZERO,
            #[cfg(feature = "test-hooks")]
            withhold_reports: false,
            #[cfg(feature = "test-hooks")]
            ignore_direct: false,
            #[cfg(feature = "test-hooks")]
            joins: Vec::new(),
        })
    }

    /// The endpoint's UDP port (a host's TCP port too); 0 while a join
    /// through Nostr has no endpoint yet.
    pub fn local_port(&self) -> u16 {
        self.endpoint.endpoint().map_or(0, Endpoint::local_port)
    }

    /// A host's reachability now (its router, STUN, the configured address,
    /// its listener); `None` on a client.
    pub fn reachability(&self) -> Option<Reachability> {
        let Side::Host(h) = &self.side else { return None };
        Some(h.reach.now(self.endpoint.endpoint()?))
    }

    /// A host listens on `pool`'s relays from now on (N54; the relays
    /// edited while hosting: the game's Options), or on none: the old
    /// listener's thread drops its pool, a new one starts. Never blocks;
    /// nothing on a client, a closed peer, or the same relays.
    pub fn listen_on(&mut self, pool: Option<rl_discovery::PoolConfig>) -> io::Result<()> {
        match &mut self.side {
            Side::Host(h) if !self.closed => h.reach.listen_on(pool),
            _ => Ok(()),
        }
    }

    /// A client's path to its host and the host's end (relayed: the TURN
    /// relay's address, ruling FR12b-4), once its link opened; the path
    /// follows ICE.
    pub fn connection(&self) -> Option<(Path, SocketAddr)> {
        let Side::Client(c) = &self.side else { return None };
        let (path, remote) = c.connection?;
        let ep = self.endpoint.endpoint();
        let now = ep.and_then(|ep| ep.path(HOST)).unwrap_or(path);
        let remote = ep.and_then(|ep| ep.peers().next()).map_or(remote, |(_, addr)| addr);
        Some((now, ep.map_or(remote, |ep| far_end(ep, now, remote))))
    }

    /// Leaves (as [`OnlinePeer::leave`]), stops listening and shuts the
    /// endpoint down (ruling NT-2): never blocks. The rest of the peer is
    /// dropped on a thread (`rl-online drop`: its content worker waits up to
    /// 2 s for a running job). Wait on the [`Shutdown`] (at most 1 s)
    /// before the process exits, so the router deletes the port mapping
    /// (N51). Take the replay first ([`OnlinePeer::take_replay`]).
    pub fn shutdown(mut self) -> Shutdown {
        self.leave();
        let endpoint = self.endpoint.take_endpoint().map(Endpoint::shutdown);
        // If no thread can start, the closure (and the peer) is dropped here.
        let _ = std::thread::Builder::new().name("rl-online drop".into()).spawn(move || drop(self));
        Shutdown { endpoint }
    }

    pub fn is_host(&self) -> bool {
        matches!(self.side, Side::Host(_))
    }

    /// Left, refused, kicked or disconnected.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Does what the network, the lobby and the match have for `now`:
    /// receives, runs the lobby, advances the match (the local player's
    /// keys from `local_keys`; `on_tick` sees the world after each simulated
    /// tick, re-simulated ones included), sends.
    pub fn poll(
        &mut self,
        now: Instant,
        local_keys: &mut dyn FnMut() -> Keys,
        on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo),
    ) -> Vec<OnlineEvent> {
        let epoch = *self.epoch.get_or_insert(now);
        self.now = Some(now);
        self.now_us = now.saturating_duration_since(epoch).as_micros() as u64;
        if !self.closed {
            // N70: before anything is received or sent.
            self.step_down_check(now);
            self.join_tasks(now);
            self.listen_tasks(now);
            let mut net = Vec::new();
            self.endpoint.poll(now, &mut net);
            for e in net {
                self.on_net(e, now);
            }
            self.session_events();
            self.migration_tasks(now);
            self.sync_paths();
            if let Side::Host(h) = &mut self.side {
                h.lobby.tick(self.now_us);
            }
            self.join_deadline(now);
            self.host_lobby_events(now);
            self.content_tasks();
            self.advance(now, local_keys, on_tick);
            // SB-2: after the advance, which keeps the silence clock.
            self.cut_off_check(now);
            self.host_tasks(now);
            self.overflowed();
            self.close_lingering(now);
            self.update_stats(now);
            self.report_reachability();
        }
        self.collect_replay();
        std::mem::take(&mut self.events)
    }

    /// The lobby's actions: the host's or the client's.
    pub fn lobby(&mut self) -> LobbyHandle<'_> {
        LobbyHandle { peer: self }
    }

    pub fn match_session(&self) -> Option<&Session<WorldSim>> {
        self.game.as_ref().filter(|g| g.ended.is_none()).map(|g| &g.session)
    }

    pub fn match_session_mut(&mut self) -> Option<&mut Session<WorldSim>> {
        self.game.as_mut().filter(|g| g.ended.is_none()).map(|g| &mut g.session)
    }

    /// How far the running match's pace is into its next tick at `now`
    /// (`rl_netcode::Session::tick_phase`): what the game interpolates by.
    /// `None` without a match, while it stalls or catches up, and for a
    /// spectator: then the newest tick is drawn as it is.
    pub fn tick_phase(&self, now: Instant) -> Option<f32> {
        let epoch = self.epoch?;
        let now_us = now.saturating_duration_since(epoch).as_micros() as u64;
        self.match_session()?.tick_phase(now_us)
    }

    /// The worm this peer plays in the running match.
    pub fn my_slot(&self) -> Option<u8> {
        self.game.as_ref().filter(|g| g.ended.is_none()).and_then(|g| g.my_slot)
    }

    /// The connection's numbers over the last second of the match.
    pub fn stats(&self) -> NetStats {
        self.stats
    }

    /// The last match this peer finished, as its replay is made of it (a
    /// spectator that joined mid-match has none).
    pub fn last_record(&self) -> Option<&MatchRecord> {
        self.last_record.as_ref()
    }

    /// The world of the match that just ended, as last presented (the
    /// last `on_tick` world): once `MatchEnded` is out the session goes on
    /// unpresented ([`OnlinePeer::match_session`] is `None`) to confirm and
    /// drain, so this is what a game draws until it leaves the match.
    /// `None` once taken, or before a match ended.
    pub fn take_ended_world(&mut self) -> Option<World> {
        self.ended_world.take()
    }

    /// Leaves: the match's `Bye` (the netcode's) and the lobby's to everyone,
    /// sent at once, then the connections close.
    pub fn leave(&mut self) {
        if self.closed {
            return;
        }
        if let Some(g) = &mut self.game {
            g.session.leave();
            let ep = &mut self.endpoint;
            g.session.transmit(&mut |peer, ch, bytes| send(ep, peer, ch, &bytes));
        }
        match &mut self.side {
            Side::Host(h) => {
                for (peer, msg) in h.lobby.outgoing() {
                    send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&msg));
                }
                let peers: Vec<PeerId> = self.endpoint.peers().map(|(p, _)| p).collect();
                for peer in peers {
                    send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&ToClient::Bye));
                }
            }
            Side::Client(c) => {
                c.lobby.leave();
                for msg in c.lobby.outgoing() {
                    send_tagged(&mut self.endpoint, HOST, TAG_TO_HOST, &encode_to_host(&msg));
                }
            }
        }
        self.shut();
    }

    /// The content this peer's matches and replays load from: its own
    /// with the list's downloaded packs laid over.
    pub fn content(&self) -> Arc<Content> {
        self.content.content()
    }

    /// The content downloads: a client's from its host, a host's skins
    /// from its members.
    pub fn downloads(&self) -> Vec<Download> {
        self.content.downloads()
    }

    /// The missing bytes waiting for [`OnlinePeer::approve_downloads`] (a
    /// client's content over the prompt, [`OnlineEvent::DownloadPrompt`]),
    /// as of the last poll; `None` once approved or below the prompt.
    pub fn download_prompt(&self) -> Option<u64> {
        self.content.prompt()
    }

    /// The player lets the content over the prompt download
    /// ([`OnlineEvent::DownloadPrompt`]).
    pub fn approve_downloads(&mut self) {
        self.content.approve();
    }

    /// Tests only (`test-hooks`): a host serves pack `other`'s archive for
    /// pack `id` (listed with that archive's size; it fails to verify).
    #[cfg(feature = "test-hooks")]
    pub fn test_serve_instead(&mut self, id: &str, other: &str) {
        self.content.serve_instead(id, other);
    }

    /// Tests only (`test-hooks`): a client's prompt comes above `bytes`
    /// instead of `content::PROMPT_BYTES`.
    #[cfg(feature = "test-hooks")]
    pub fn test_prompt_limit(&mut self, bytes: u64) {
        self.content.set_prompt_bytes(bytes);
    }

    /// Tests only (`test-hooks`): a client joining through Nostr dates its
    /// handshake `secs` off the system's clock (a PC whose clock is wrong).
    #[cfg(feature = "test-hooks")]
    pub fn test_clock_offset(&mut self, secs: i64) {
        if let Side::Client(c) = &mut self.side
            && let Some(j) = &mut c.joiner
        {
            j.set_clock(secs);
        }
    }

    /// Tests only (`test-hooks`): a client builds the host's start with
    /// this seed instead (a world that differs from the host's).
    #[cfg(feature = "test-hooks")]
    pub fn test_seed_override(&mut self, seed: Option<u32>) {
        self.test_seed = seed;
    }

    // ---- the network ----

    fn on_net(&mut self, e: NetEvent, now: Instant) {
        let now_us = self.now_us;
        // A client's open link lost during a match: the migration begins
        // (N76), or goes on.
        if matches!(e, NetEvent::Disconnected { .. })
            && self.client_link_open()
            && self.begin_migration(false)
        {
            return;
        }
        match &mut self.side {
            Side::Host(h) => match e {
                NetEvent::Connected { peer, addr, path } => {
                    h.addrs.insert(peer, addr);
                    let fps = self.endpoint.endpoint().and_then(|ep| ep.dtls_fingerprints(peer));
                    match (random_bytes::<32>(), fps) {
                        (Ok(nonce), Some(fps)) => {
                            h.lobby.connected(peer, addr.ip(), path, now_us, nonce, fps)
                        }
                        _ => self.drop_peer(peer),
                    }
                }
                NetEvent::Message { peer, channel: Channel::Unreliable, bytes } => {
                    if let Some(g) = &mut self.game {
                        g.session.receive(peer, Channel::Unreliable, &bytes, now_us);
                    }
                }
                NetEvent::Message { peer, channel: Channel::Reliable, bytes } => match tag(&bytes) {
                    Some(Tagged::Netcode(b)) => {
                        if let Some(g) = &mut self.game {
                            g.session.receive(peer, Channel::Reliable, b, now_us);
                        }
                    }
                    Some(Tagged::ToHost(b)) => match decode_to_host(b) {
                        Ok(msg) => {
                            if let Some(sent) = h.challenged.remove(&peer) {
                                let rtt = now.saturating_duration_since(sent).as_millis();
                                h.rtts.entry(peer).or_insert(u32::try_from(rtt).unwrap_or(u32::MAX));
                            }
                            let hello = matches!(msg, ToHost::Hello { .. });
                            h.lobby.receive(peer, msg, now_us);
                            if hello {
                                // The host's proof of its key leaves at
                                // once: the joiner waits for it to prove
                                // itself.
                                self.drain_lobby(now);
                            }
                        }
                        Err(_) => self.drop_peer(peer),
                    },
                    Some(Tagged::Content(b)) => match decode_content(b) {
                        // Only members fetch and offer content.
                        Ok(msg) => {
                            let look = h.lobby.member_of(peer).and_then(|id| {
                                h.lobby.state().members.iter().find(|m| m.id == id).map(|m| m.look.clone())
                            });
                            if look.is_some() {
                                self.content.receive(peer, msg, look.as_ref());
                            }
                        }
                        Err(_) => self.drop_peer(peer),
                    },
                    _ => self.drop_peer(peer),
                },
                NetEvent::Disconnected { peer, .. } => self.forget_peer(peer),
                NetEvent::Refused { .. } | NetEvent::Answered => {}
                NetEvent::JoinFailed { .. } => {
                    let mut notes = Vec::new();
                    h.reach.note(RelayJoin::NotConnected, now, &mut notes);
                    self.events
                        .extend(notes.into_iter().map(|(join, more)| OnlineEvent::RelayJoin { join, more }));
                }
            },
            Side::Client(c) => match e {
                NetEvent::Connected { addr, path, .. } => {
                    let ep = self.endpoint.endpoint();
                    let addr = ep.map_or(addr, |ep| far_end(ep, path, addr));
                    match ep.and_then(|ep| ep.dtls_fingerprints(HOST)) {
                        Some(fps) => {
                            c.waiting_since = Some(now);
                            c.lobby.connected(fps);
                            // The join is done: its pool goes (on a thread).
                            c.joiner = None;
                            if c.connection.is_none() {
                                self.events.push(OnlineEvent::Connected { path, remote: addr });
                            }
                            c.connection = Some((path, addr));
                        }
                        // An open link always has them.
                        None => {
                            self.end_with(OnlineEvent::Disconnected("the connection is not secured".into()))
                        }
                    }
                }
                NetEvent::Message { channel: Channel::Unreliable, bytes, .. } => {
                    if let Some(g) = &mut self.game {
                        g.session.receive(HOST, Channel::Unreliable, &bytes, now_us);
                    }
                }
                NetEvent::Message { channel: Channel::Reliable, bytes, .. } => match tag(&bytes) {
                    Some(Tagged::Netcode(b)) => {
                        #[cfg(feature = "test-hooks")]
                        if self.withhold_reports && matches!(decode_control(b), Ok(Control::Collect { .. })) {
                            return;
                        }
                        if let Some(g) = &mut self.game {
                            if let Ok(Control::StateBegin { len, .. }) = decode_control(b)
                                && let Some(ep) = self.endpoint.endpoint_mut()
                            {
                                ep.allow_burst(HOST, len as usize);
                            }
                            g.session.receive(HOST, Channel::Reliable, b, now_us);
                        }
                    }
                    Some(Tagged::ToClient(b)) => match decode_to_client(b) {
                        Ok(msg) => self.client_receive(msg, now),
                        Err(_) => self
                            .end_with(OnlineEvent::Disconnected("the host sent a malformed message".into())),
                    },
                    Some(Tagged::Content(b)) => match decode_content(b) {
                        Ok(msg) => self.content.receive(HOST, msg, None),
                        Err(_) => self
                            .end_with(OnlineEvent::Disconnected("the host sent a malformed message".into())),
                    },
                    _ => self.end_with(OnlineEvent::Disconnected("the host sent a malformed message".into())),
                },
                NetEvent::Disconnected { why, .. } => {
                    // Before the link opened: the join may go on (N46) or
                    // say what failed.
                    let lost = match &mut c.joiner {
                        Some(j) if c.connection.is_none() => {
                            j.lost(why, &mut self.endpoint, &mut self.events)
                        }
                        _ => Lost::Usual,
                    };
                    let text = match lost {
                        Lost::FellBack => return,
                        Lost::Text(text) => text,
                        Lost::Usual => why_text(why).into(),
                    };
                    c.lobby.disconnected();
                    let _ = c.lobby.events();
                    self.end_with(OnlineEvent::Disconnected(text));
                }
                NetEvent::Refused { why } => {
                    // Through Nostr, `Busy` is the host's pending setups
                    // (PF12b-2), not this key in its lobby.
                    let nostr = c.joiner.as_ref().is_some_and(|j| j.through_nostr());
                    let text = if nostr && why == Refusal::Busy { BUSY } else { refusal_text(why) };
                    self.end_with(OnlineEvent::Refused(text.into()));
                }
                // A host's only; the answer matters to a successor's join
                // alone (`migrate`).
                NetEvent::JoinFailed { .. } | NetEvent::Answered => {}
            },
        }
    }

    /// Closes a host's peer at once: the lobby and the match forget it.
    fn drop_peer(&mut self, peer: PeerId) {
        self.endpoint.close(peer);
        self.forget_peer(peer);
    }

    /// A host's peer is gone.
    fn forget_peer(&mut self, peer: PeerId) {
        if let Side::Host(h) = &mut self.side {
            h.lobby.disconnected(peer);
            h.forget(peer);
        }
        self.closing.remove(&peer);
        self.content.peer_left(peer);
        if let Some(g) = &mut self.game {
            g.session.peer_lost(peer);
        }
    }

    /// Peers the outbox closed: they did not read what was sent to them
    /// (a backlog past `outbox::MAX_BACKLOG`, or a second state while one
    /// was outstanding).
    fn overflowed(&mut self) {
        for peer in self.endpoint.take_overflowed() {
            if self.is_host() {
                let name = match &self.side {
                    Side::Host(h) => h.lobby.member_of(peer).map(|id| self.name(id)),
                    Side::Client(_) => None,
                };
                let who = name.unwrap_or_else(|| format!("connection {peer}"));
                self.events.push(OnlineEvent::Notice(format!("{who} did not keep up and was disconnected")));
                self.forget_peer(peer);
            } else {
                self.end_with(OnlineEvent::Disconnected("the host did not take what this peer sent".into()));
            }
        }
    }

    /// Closes a host's peer once what was sent to it is delivered.
    fn close_later(&mut self, peer: PeerId, now: Instant) {
        self.closing.entry(peer).or_insert(now + CLOSE_LINGER);
        if let Some(g) = &mut self.game {
            g.session.peer_lost(peer);
        }
    }

    fn close_lingering(&mut self, now: Instant) {
        let done: Vec<PeerId> = self
            .closing
            .iter()
            .filter(|&(&peer, &until)| now >= until || self.endpoint.buffered(peer) == 0)
            .map(|(&peer, _)| peer)
            .collect();
        for peer in done {
            self.closing.remove(&peer);
            self.endpoint.close(peer);
            self.content.peer_left(peer);
            if let Side::Host(h) = &mut self.side {
                h.forget(peer);
            }
        }
    }

    /// Closes every connection; nothing more happens (a replay being made
    /// still arrives).
    fn shut(&mut self) {
        // A client closing once its match has a result (the end confirmed,
        // or the host's `Ended`): the replay from what it confirmed (else it
        // would be lost with the session).
        if !self.is_host() && self.game.as_ref().is_some_and(|g| g.ended.is_some() || g.end_at.is_some()) {
            self.finish_client();
        }
        // What was queued leaves with this poll, before the close (before
        // the first poll no connection is open).
        if let Some(now) = self.now {
            self.endpoint.poll(now, &mut Vec::new());
        }
        let peers: Vec<PeerId> = self.endpoint.peers().map(|(p, _)| p).collect();
        for peer in peers {
            self.endpoint.close(peer);
        }
        // The listener's and the joiner's pools go on threads of their own,
        // a successor's endpoints too.
        match &mut self.side {
            Side::Host(h) => h.reach.stop(),
            Side::Client(c) => {
                c.joiner = None;
                migrate::drop_migration(c);
            }
        }
        self.closing.clear();
        self.game = None;
        self.closed = true;
        self.stats = NetStats::default();
    }

    /// A client's join through Nostr (before its link opens): its steps,
    /// and its end when one fails.
    fn join_tasks(&mut self, now: Instant) {
        let Side::Client(c) = &mut self.side else { return };
        let Some(j) = &mut c.joiner else { return };
        if let Err(why) = j.poll(now, &mut self.endpoint, &mut self.events) {
            self.end_with(OnlineEvent::Disconnected(why));
        }
    }

    /// A host's members' paths follow their links as ICE moves (final
    /// review B-M1: the tags, and N59's ban decision, use the path now).
    pub(crate) fn sync_paths(&mut self) {
        let (Side::Host(h), Some(ep)) = (&mut self.side, self.endpoint.endpoint()) else { return };
        for &peer in h.addrs.keys() {
            if let Some(path) = ep.path(peer) {
                h.lobby.set_path(peer, path);
            }
        }
    }

    /// A host's listener's offers, answered by the endpoint (N54), and
    /// what it says of them (FR12b-3).
    fn listen_tasks(&mut self, now: Instant) {
        let (Side::Host(h), Some(ep)) = (&mut self.side, self.endpoint.endpoint_mut()) else { return };
        let mut notes = Vec::new();
        h.reach.answer(ep, now, &mut notes);
        self.events.extend(notes.into_iter().map(|(join, more)| OnlineEvent::RelayJoin { join, more }));
        let news = h.reach.relay_news();
        self.events.extend(news.into_iter().map(|(url, status)| OnlineEvent::ListenRelay { url, status }));
    }

    /// A host's reachability, when its lines, whether it settled or its
    /// direct address changed.
    fn report_reachability(&mut self) {
        let (Side::Host(h), Some(ep)) = (&mut self.side, self.endpoint.endpoint()) else { return };
        if let Some(r) = h.reach.changed(ep) {
            self.events.push(OnlineEvent::Reachability(r));
        }
    }

    /// A client not welcomed [`JOIN_DEADLINE`] after its link opened gives
    /// up ([`NO_ANSWER`]).
    fn join_deadline(&mut self, now: Instant) {
        let Side::Client(c) = &self.side else { return };
        if c.waiting_since.is_some_and(|t| now.saturating_duration_since(t) >= JOIN_DEADLINE) {
            self.end_with(OnlineEvent::Disconnected(NO_ANSWER.into()));
        }
    }

    /// A client's end: the event, then every connection closed.
    fn end_with(&mut self, event: OnlineEvent) {
        if !self.closed {
            self.events.push(event);
            self.shut();
        }
    }

    // ---- the lobby ----

    fn name(&self, id: u8) -> String {
        self.names.get(&id).cloned().unwrap_or_else(|| format!("member {id}"))
    }

    fn learn_names(&mut self, state: &LobbyState) {
        for m in &state.members {
            self.names.insert(m.id, m.look.name.clone());
        }
    }

    /// The host's lobby messages, now.
    fn drain_lobby(&mut self, now: Instant) {
        let Side::Host(h) = &mut self.side else { return };
        for (peer, msg) in h.lobby.outgoing() {
            if matches!(msg, ToClient::Challenge { .. }) {
                h.challenged.insert(peer, now);
            }
            send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&msg));
        }
        h.last_drain = Some(now);
    }

    /// The content side's poll: the list, downloads, `Have`, serving; its
    /// messages sent on `TAG_CONTENT`. Chunks go to a peer while its
    /// outbox (backlog and endpoint) holds less than `content::QUEUED`.
    fn content_tasks(&mut self) {
        let now_us = self.now_us;
        let ep = &self.endpoint;
        let buffered = |peer: PeerId| ep.buffered(peer);
        match &mut self.side {
            Side::Host(h) => self.content.poll_host(now_us, &mut h.lobby, &buffered),
            Side::Client(c) => self.content.poll_client(now_us, &mut c.lobby, &buffered),
        }
        self.flush_client();
        for (peer, msg) in self.content.take_out() {
            send_tagged(&mut self.endpoint, peer, TAG_CONTENT, &encode_content(&msg)[1..]);
        }
        self.events.extend(self.content.take_events());
    }

    fn host_lobby_events(&mut self, now: Instant) {
        let Side::Host(h) = &mut self.side else { return };
        let events = h.lobby.events();
        self.on_host_events(events, now);
    }

    /// The host's lobby events, in order.
    fn on_host_events(&mut self, events: Vec<HostEvent>, now: Instant) {
        let Side::Host(h) = &mut self.side else { return };
        if events.is_empty() {
            return;
        }
        let state = h.lobby.state().clone();
        self.learn_names(&state);
        for e in events {
            match e {
                HostEvent::Joined(id) => {
                    self.events.push(OnlineEvent::Notice(format!("{} joined", self.name(id))));
                    // The content list follows its welcome.
                    self.drain_lobby(now);
                    if let Side::Host(h) = &self.side
                        && let Some(peer) = h.lobby.peer_of(id)
                    {
                        self.content.joined(peer);
                    }
                }
                // N33: a member waiting during a match (a joiner, or one
                // that lacked content at its start) watches once its
                // content is complete (its `Have`, after every list).
                HostEvent::ContentComplete(id) => self.mid_match_join(id, now),
                HostEvent::Left(id) => self.events.push(OnlineEvent::Left(self.name(id))),
                HostEvent::Kick { peer, ban } => {
                    // A ban by key leaves the address open (spec §5.2);
                    // one with the address closes the gate to it.
                    if let Side::Host(h) = &mut self.side
                        && ban
                        && let Some(addr) = h.addrs.get(&peer)
                        && h.lobby.banned_ip(addr.ip())
                    {
                        h.banned.lock().unwrap_or_else(|e| e.into_inner()).insert(addr.ip());
                    }
                    // The refusal or `Kicked` leaves before the close.
                    self.drain_lobby(now);
                    self.close_later(peer, now);
                }
                HostEvent::Close { peer } => self.close_later(peer, now),
                HostEvent::Chat { from, text } => {
                    self.events.push(OnlineEvent::Chat { from: self.name(from), member: from, text });
                }
                HostEvent::Admin { from, cmd: AdminCommand::Settings(setup) } => {
                    self.admin_setup(from, setup)
                }
                HostEvent::Notice(text) => self.events.push(OnlineEvent::Notice(text)),
                // Plan 12c: a migrated lobby's rejoin is the online layer's to
                // act on (tasks 6 and 7).
                HostEvent::Rejoined(id) => self.host_rejoined(id, now),
                HostEvent::Admin { .. } | HostEvent::StartRequested => {}
            }
        }
    }

    /// An admin's setup: taken when the host has its packs and the lobby
    /// takes it, else a notice to that admin says why not.
    fn admin_setup(&mut self, from: u8, setup: MatchSetupWire) {
        let content = self.content.content();
        let Side::Host(h) = &mut self.side else { return };
        let have = |id: &str| content.pack_kind(id).is_some();
        let refused = if !have(&setup.mod_id) || !have(&setup.level_id) {
            Some("the host does not have that mod or level".to_string())
        } else if let Err(e) = matches::check_packs(&content, &setup) {
            Some(format!("{e:#}"))
        } else {
            h.lobby.set_setup(setup).err().map(|e| e.to_string())
        };
        let (Some(why), Some(peer)) = (refused, h.lobby.peer_of(from)) else { return };
        let text = sanitize_chat(&format!("{SETUP_REFUSED}: {why}")).unwrap_or_else(|| SETUP_REFUSED.into());
        let notice = ToClient::Notice(Notice::Message(text));
        send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&notice));
    }

    /// A member welcomed during a match watches it (spec §3.6): it gets the
    /// start (after its welcome), and the session owes it the state.
    fn mid_match_join(&mut self, id: u8, now: Instant) {
        let Side::Host(h) = &self.side else { return };
        let Some(peer) = h.lobby.peer_of(id) else { return };
        let Some(start) = self.game.as_ref().filter(|g| g.ended.is_none()).map(|g| g.start.clone()) else {
            return;
        };
        self.drain_lobby(now);
        send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&ToClient::Start(start)));
        if let Some(g) = &mut self.game {
            g.session.add_spectator(peer, self.now_us);
        }
    }

    /// The host's lobby drain (every [`LOBBY_DRAIN`]), its pings and the
    /// lobby reported to the caller.
    fn host_tasks(&mut self, now: Instant) {
        let Side::Host(h) = &mut self.side else { return };
        if h.last_ping.is_none_or(|t| now.saturating_duration_since(t) >= PING_EVERY) {
            h.last_ping = Some(now);
            if let Some(g) = &self.game {
                for &peer in h.addrs.keys() {
                    if let Some(rtt) = g.session.peer_rtt_ms(peer) {
                        h.rtts.insert(peer, rtt);
                    }
                }
            }
            for (&peer, &rtt) in &h.rtts {
                h.lobby.set_ping(peer, rtt);
            }
        }
        if h.last_drain.is_none_or(|t| now.saturating_duration_since(t) >= LOBBY_DRAIN) {
            self.drain_lobby(now);
        }
        let Side::Host(h) = &mut self.side else { return };
        if h.reported != Some(h.lobby.revision()) {
            let state = h.lobby.state().clone();
            h.reported = Some(h.lobby.revision());
            self.learn_names(&state);
            self.events.push(OnlineEvent::Lobby(state));
        }
    }

    fn client_receive(&mut self, msg: ToClient, now: Instant) {
        let Side::Client(c) = &mut self.side else { return };
        c.lobby.receive(msg, random_bytes::<32>().unwrap_or([0; 32]));
        let events = c.lobby.events();
        self.flush_client();
        for e in events {
            self.on_client_event(e, now);
        }
    }

    fn flush_client(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        for msg in c.lobby.outgoing() {
            send_tagged(&mut self.endpoint, HOST, TAG_TO_HOST, &encode_to_host(&msg));
        }
    }

    fn on_client_event(&mut self, e: ClientEvent, now: Instant) {
        match e {
            ClientEvent::Welcome { .. } => {
                if let Side::Client(c) = &mut self.side {
                    c.waiting_since = None;
                }
                self.open_standby();
                if let Side::Client(c) = &self.side
                    && let Some(state) = c.lobby.state().cloned()
                {
                    self.learn_names(&state);
                    self.events.push(OnlineEvent::Lobby(state));
                }
            }
            ClientEvent::Lobby(state) => {
                self.learn_names(&state);
                self.events.push(OnlineEvent::Lobby(state));
            }
            ClientEvent::Chat { from, text } => {
                self.events.push(OnlineEvent::Chat { from: self.name(from), member: from, text })
            }
            ClientEvent::Notice(n) => {
                let e = self.notice(n);
                self.events.push(e);
            }
            ClientEvent::Start(start) => self.client_start(start),
            ClientEvent::Ended(end) => {
                // A late rejoiner's carried count belongs to this match.
                if let Side::Client(c) = &mut self.side {
                    c.migrations = 0;
                }
                if let Some(g) = &mut self.game
                    && g.ended.is_none()
                {
                    // Received before this poll's advance: the world is
                    // the last poll's, the one presented.
                    self.ended_world = Some(g.session.sim().world().clone());
                    g.ended = Some(now);
                    // The host's last tick ends every record (ruling T14-A;
                    // a `/next` end has no result tick to find it by): the
                    // client finishes once it confirmed and simulated it,
                    // or after `CLIENT_FINISH` with what it has.
                    g.end_at = Some(end.last_tick);
                }
                self.events.push(OnlineEvent::MatchEnded(end));
            }
            ClientEvent::Refused(why) => self.end_with(OnlineEvent::Refused(refusal_text(why).into())),
            ClientEvent::Kicked { banned } => self.end_with(OnlineEvent::Kicked { banned }),
            ClientEvent::WrongHost { .. } => {
                let expected = matches!(&self.side, Side::Client(c) if c.expected);
                self.end_with(OnlineEvent::Refused(wrong_host_text(expected).into()));
            }
            // A7: during a match the host's `Bye` (it left or stepped down)
            // starts the migration.
            ClientEvent::Closed => {
                if !self.begin_migration(true) {
                    self.end_with(OnlineEvent::Disconnected("the host closed the lobby".into()))
                }
            }
        }
    }

    fn notice(&self, n: Notice) -> OnlineEvent {
        match n {
            Notice::Joined(id) => OnlineEvent::Notice(format!("{} joined", self.name(id))),
            Notice::Left(id) => OnlineEvent::Left(self.name(id)),
            Notice::Kicked(id) => OnlineEvent::Notice(format!("{} was kicked", self.name(id))),
            Notice::Banned(id) => OnlineEvent::Notice(format!("{} was banned", self.name(id))),
            Notice::NotReady(ids) => {
                let names: Vec<String> = ids.iter().map(|&id| self.name(id)).collect();
                OnlineEvent::Notice(format!("not ready: {}", names.join(", ")))
            }
            Notice::Started => OnlineEvent::Notice("the match starts".into()),
            Notice::Message(text) => OnlineEvent::Notice(text),
        }
    }

    // ---- the match ----

    /// The host's start (spec §4): the lobby's start, the world built from
    /// it and its start checksum filled in, `Start` to everyone at once,
    /// the session.
    fn host_start(&mut self, seed: u32, force: bool) -> Result<(), StartError> {
        if self.game.is_some() {
            return Err(StartError::Running);
        }
        let now_us = self.now_us;
        let Side::Host(h) = &mut self.side else { return Err(StartError::NotHost) };
        let started = if force { h.lobby.force_start(seed, &h.rtts) } else { h.lobby.start(seed, &h.rtts) };
        let mut start = started.map_err(StartError::Lobby)?;
        let world = matches::build(&self.content.content(), &start)
            .map_err(|e| StartError::Build(format!("{e:#}")))?;
        start.start_checksum = world.sim.world().checksum();
        h.lobby.announce_start(start.clone()).map_err(StartError::Lobby)?;
        let me = h.lobby.state().members.iter().find(|m| m.key == self.my_key).map(|m| m.id);
        let slot_of = |member: u8| start.slots.iter().position(|s| s.member == Some(member)).map(|i| i as u8);
        let peers: Vec<(PeerId, Seat)> = h
            .lobby
            .state()
            .members
            .iter()
            // N33: a member waiting for its content is not in the match (it
            // gets no start, would send nothing and time out); it joins as
            // a spectator once complete (`ContentComplete`).
            .filter(|m| m.seat != SeatWire::Waiting)
            .filter_map(|m| {
                let peer = h.lobby.peer_of(m.id)?;
                Some((peer, slot_of(m.id).map_or(Seat::Spectator, Seat::Player)))
            })
            .collect();
        let my_slot = me.and_then(slot_of);
        let slots = start.slots.len() as u8;
        let session = Session::new(Config::new(slots, 0), Role::Host { peers }, world.sim, now_us);
        if let Some(now) = self.now {
            self.drain_lobby(now);
        }
        // The world's build is the start's own pause, not a stall (N70):
        // the peers hear the start only now.
        self.last_poll = None;
        self.ended_world = None;
        self.game = Some(Game::new(session, Some(HostBots::new(world.bots)), start.clone(), my_slot));
        self.events.push(OnlineEvent::MatchStarting { start, my_slot });
        Ok(())
    }

    /// A client's start: the world built and checked against the host's
    /// start checksum (else it leaves), then a player's, a spectator's or,
    /// arriving mid-match (the lobby already starting or playing), a
    /// joining spectator's session.
    fn client_start(&mut self, start: MatchStartWire) {
        match &self.game {
            // A host bug or a hostile one: one match at a time.
            Some(g) if g.ended.is_none() => {
                self.events.push(OnlineEvent::Notice(START_IGNORED.into()));
                return;
            }
            Some(_) => self.finish_client(),
            None => {}
        }
        let Side::Client(c) = &self.side else { return };
        let joining =
            matches!(c.lobby.state().map(|s| s.phase), Some(Phase::Starting | Phase::Playing { .. }));
        let me = c.lobby.me();
        let mut built = start.clone();
        if let Some(seed) = self.test_seed {
            built.seed = seed;
        }
        let world = match matches::build(&self.content.content(), &built) {
            Ok(w) => w,
            Err(e) => {
                self.leave();
                self.events.push(OnlineEvent::Refused(format!("cannot build the host's match: {e:#}")));
                return;
            }
        };
        if world.sim.world().checksum() != start.start_checksum {
            self.leave();
            self.events.push(OnlineEvent::Refused(MISMATCH.into()));
            return;
        }
        let slots = start.slots.len() as u8;
        let slot = me.and_then(|m| start.slots.iter().position(|s| s.member == Some(m))).map(|i| i as u8);
        let delay = me
            .and_then(|m| start.input_delay.iter().find(|d| d.0 == m))
            .map_or(MAX_INPUT_DELAY, |d| d.1.min(MAX_INPUT_DELAY));
        let (role, cfg, my_slot) = match (joining, slot) {
            (true, _) => (Role::Joining { host: HOST }, Config::new(slots, 0), None),
            (false, Some(slot)) => (Role::Client { host: HOST, slot }, Config::new(slots, delay), Some(slot)),
            (false, None) => (Role::Spectator { host: HOST }, Config::new(slots, 0), None),
        };
        let session = Session::new(cfg, role, world.sim, self.now_us);
        self.ended_world = None;
        let mut game = Game::new(session, None, start.clone(), my_slot);
        if let Side::Client(c) = &mut self.side {
            game.migrations = std::mem::take(&mut c.migrations);
        }
        self.game = Some(game);
        self.events.push(OnlineEvent::MatchStarting { start, my_slot });
    }

    /// The session's events since the last look.
    fn session_events(&mut self) {
        let Some(g) = &mut self.game else { return };
        let events = g.session.drain_events();
        for e in events {
            self.on_session_event(e);
        }
    }

    fn on_session_event(&mut self, e: Event) {
        let host = self.is_host();
        match e {
            Event::StateLoaded { tick } => {
                if let Some(g) = &mut self.game {
                    g.recorder.state_loaded(tick);
                    if g.ended.is_none() {
                        self.events.push(OnlineEvent::StateLoaded { tick });
                    }
                }
            }
            Event::StateSent { peer, bytes, .. } if host => {
                if let Some(ep) = self.endpoint.endpoint_mut() {
                    ep.allow_burst(peer, bytes)
                }
            }
            Event::PeerTimedOut { peer } | Event::RepairFailed { peer } | Event::JoinRefused { peer }
                if host =>
            {
                self.drop_peer(peer);
            }
            Event::Lagging(on) if !host => self.events.push(OnlineEvent::Lagging(on)),
            Event::Desync { tick, local, remote, peer } => self.desync_bundle(tick, local, remote, peer),
            Event::PeerLeft { .. } if !host => {
                if !self.begin_migration(true) {
                    self.end_with(OnlineEvent::Disconnected("the host left".into()));
                }
            }
            Event::PeerTimedOut { .. } if !host => {
                if !self.begin_migration(false) {
                    self.end_with(OnlineEvent::Disconnected("the host stopped answering".into()));
                }
            }
            Event::HostSilent if !host => {
                self.begin_migration(false);
            }
            Event::HostHeard if !host => self.host_heard(),
            Event::Resumed { .. } if !host => self.resumed(),
            _ => {}
        }
    }

    /// A desync's bundle (spec §3.5, §9.10): the start, the final inputs
    /// this peer confirmed from tick 0, its checksums and the mismatch; at
    /// most [`MAX_BUNDLES_PER_MATCH`] a match.
    fn desync_bundle(&mut self, tick: u32, local: u64, remote: u64, peer: PeerId) {
        let host = self.is_host();
        let Some(g) = &mut self.game else { return };
        if g.bundles >= MAX_BUNDLES_PER_MATCH {
            return;
        }
        g.bundles += 1;
        let record = record_of(&g.session, &g.start, g.session.confirmed()).unwrap_or_else(|| MatchRecord {
            start: g.start.clone(),
            rows: Vec::new(),
            checksums: g.session.checksums().to_vec(),
        });
        let what = if host {
            format!("peer {peer}'s checksum of tick {tick} differs from the host's")
        } else {
            format!("the host's checksum of tick {tick} differs from this peer's")
        };
        let mut bundle = GameBundle::of(record, what, host, g.my_slot);
        bundle.mismatch = Some((tick, local, remote));
        bundle.other_checksums = if host { g.session.peer_checksums(peer) } else { vec![(tick, remote)] };
        self.events.push(OnlineEvent::DesyncBundle(bundle.encode()));
    }

    /// This peer's member id in its lobby.
    fn me(&self) -> Option<u8> {
        match &self.side {
            Side::Host(h) => h.lobby.state().members.iter().find(|m| m.key == self.my_key).map(|m| m.id),
            Side::Client(c) => c.lobby.me(),
        }
    }

    /// Advances the match, sends its messages, reports its ticks, and ends it.
    fn advance(
        &mut self,
        now: Instant,
        local_keys: &mut dyn FnMut() -> Keys,
        on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo),
    ) {
        let now_us = self.now_us;
        let Some(g) = &mut self.game else { return };
        let ended = g.ended.is_some();
        let mut no_keys = || Keys(0);
        let mut no_tick = |_: &World, _: &TickEvents, _: TickInfo| {};
        let keys: &mut dyn FnMut() -> Keys = if ended { &mut no_keys } else { local_keys };
        let on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo) =
            if ended { &mut no_tick } else { on_tick };
        let mut driver = PeerDriver {
            my_slot: g.my_slot,
            keys,
            bots: g.bots.as_mut(),
            recorder: &mut g.recorder,
            on_tick,
        };
        g.session.advance(now_us, &mut driver);
        let ep = &mut self.endpoint;
        g.session.transmit(&mut |peer, ch, bytes| send(ep, peer, ch, &bytes));
        let restored = g.session.take_restored_areas();
        let ticked = g.recorder.take_ticked();
        let confirmed = g.recorder.take_confirmed(g.session.confirmed());
        for c in &confirmed {
            if g.end_at.is_none() && c.result.is_some() {
                g.end_at = Some(c.tick.saturating_add(RECORD_AFTER_END));
            }
        }
        if let Some(c) = confirmed.last() {
            g.last = Some((c.scores.clone(), c.result));
        }
        if !ended {
            if !restored.is_empty() {
                self.events.push(OnlineEvent::Restored(restored));
            }
            if !ticked.is_empty() {
                self.events.push(OnlineEvent::Ticked(ticked));
            }
            if !confirmed.is_empty() {
                self.events.push(OnlineEvent::Confirmed(confirmed));
            }
        }
        if !g.playing && g.session.current_tick() > 0 {
            g.playing = true;
            if let Side::Host(h) = &mut self.side {
                h.lobby.playing(0);
            }
        }
        self.session_events();
        let Some(g) = &self.game else { return };
        // The last tick is reached once it is confirmed and simulated (its
        // checksum recorded): a dedicated server, with no slot of its own,
        // confirms inputs its clients sent ahead before it simulates them.
        let reached = g.end_at.is_some_and(|e| final_upto(&g.session) > e);
        match (&self.side, g.ended) {
            (Side::Host(_), None) if reached => self.host_end(now),
            (Side::Host(_), Some(t)) if now.saturating_duration_since(t) >= HOST_DRAIN => self.game = None,
            (Side::Client(_), Some(t)) if reached || now.saturating_duration_since(t) >= CLIENT_FINISH => {
                self.finish_client();
            }
            _ => {}
        }
    }

    /// The host's end (spec §4): the results to the lobby and `Ended` to
    /// everyone, the replay made; the session keeps sending for
    /// [`HOST_DRAIN`].
    fn host_end(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Some(g) = &mut self.game else { return };
        // After this poll's presented advance: the world presented.
        self.ended_world = Some(g.session.sim().world().clone());
        g.ended = Some(now);
        // The record's last tick, for every peer's record (ruling T14-A).
        let last_tick = *g.end_at.get_or_insert_with(|| final_upto(&g.session).saturating_sub(1));
        let end = end_wire(g.last.as_ref(), last_tick.min(u32::MAX - 1));
        let record = record_of(&g.session, &g.start, last_tick.saturating_add(1));
        if let Side::Host(h) = &mut self.side {
            h.lobby.match_ended(end.clone(), now_us);
        }
        self.drain_lobby(now);
        self.events.push(OnlineEvent::MatchEnded(end));
        self.make_replay(record);
    }

    /// The host ends its running match now: as at a match's end, with the
    /// confirmed ticks so far as the match.
    fn host_end_now(&mut self) -> bool {
        let Some(now) = self.now else { return false };
        if !self.is_host() || self.closed {
            return false;
        }
        let Some(g) = self.game.as_mut().filter(|g| g.ended.is_none()) else { return false };
        if g.end_at.is_none() {
            // The record is ticks `0..=end_at`: every tick confirmed and
            // simulated so far.
            g.end_at = Some(final_upto(&g.session).saturating_sub(1));
        }
        self.host_end(now);
        true
    }

    /// A client's match is over: its replay, from the ticks it confirmed
    /// up to the match's last.
    fn finish_client(&mut self) {
        let Some(g) = self.game.take() else { return };
        let confirmed = g.session.confirmed();
        let upto = g.end_at.map_or(confirmed, |e| (e + 1).min(confirmed));
        self.make_replay(record_of(&g.session, &g.start, upto));
    }

    /// Starts making the match's replay, unless one is still being made:
    /// one worker thread at a time (a host starting matches back to back
    /// cannot pile them up); that match's replay is skipped then.
    fn make_replay(&mut self, record: Option<MatchRecord>) {
        let Some(record) = record else { return };
        self.last_record = Some(record.clone());
        if self.replay.is_some() {
            self.events.push(OnlineEvent::Notice(REPLAY_SKIPPED.into()));
            return;
        }
        let my_slot = record.start.slots.iter().position(|s| s.member == self.me()).map(|i| i as u8);
        let host = self.is_host();
        self.replay = Some(PendingReplay::spawn(self.content.content(), record, host, my_slot));
    }

    fn collect_replay(&mut self) {
        let Some(events) = self.replay.as_ref().and_then(PendingReplay::poll) else { return };
        self.replay = None;
        self.events.extend(events);
    }

    /// The replay being made, handed out so it outlives this peer (the
    /// game leaves right after a match: its replay is still saved). A
    /// client that leaves once its match has a result makes it first, from
    /// what it confirmed (`leave`).
    pub fn take_replay(&mut self) -> Option<PendingReplay> {
        self.replay.take()
    }

    fn update_stats(&mut self, now: Instant) {
        let host = self.is_host();
        let Some(g) = self.game.as_mut().filter(|g| g.ended.is_none()) else {
            self.stats = NetStats::default();
            return;
        };
        let s = g.session.stats();
        let corrections = if host { s.rollbacks } else { s.own_corrections };
        let w = &mut g.window;
        let Some(at) = w.at else {
            *w = Window {
                at: Some(now),
                tick: g.session.current_tick(),
                datagrams_in: s.datagrams_in,
                corrections,
                depth_hist: s.depth_hist.to_vec(),
            };
            return;
        };
        let secs = now.saturating_duration_since(at).as_secs_f32();
        if secs < 1.0 {
            return;
        }
        let ticks = g.session.current_tick().saturating_sub(w.tick);
        let datagrams = s.datagrams_in.saturating_sub(w.datagrams_in);
        let loss = (!host && ticks > 0).then(|| (1.0 - datagrams as f32 / ticks as f32).clamp(0.0, 1.0));
        let depth =
            s.depth_hist.iter().zip(&w.depth_hist).rposition(|(now, before)| now > before).unwrap_or(0);
        self.stats = NetStats {
            rtt_ms: s.rtt_ms,
            input_delay: g.session.config().input_delay,
            rollback_depth: depth as u32,
            loss,
            corrections_per_s: corrections.saturating_sub(w.corrections) as f32 / secs,
            stalls: s.stalls,
        };
        *w = Window {
            at: Some(now),
            tick: g.session.current_tick(),
            datagrams_in: s.datagrams_in,
            corrections,
            depth_hist: s.depth_hist.to_vec(),
        };
    }
}

impl Game {
    fn new(
        session: Session<WorldSim>,
        bots: Option<HostBots>,
        start: MatchStartWire,
        my_slot: Option<u8>,
    ) -> Game {
        Game {
            session,
            recorder: TickRecorder::default(),
            bots,
            start,
            my_slot,
            playing: false,
            end_at: None,
            last: None,
            ended: None,
            window: Window::default(),
            bundles: 0,
            migrations: 0,
            collecting: None,
        }
    }
}

/// Ticks below it are confirmed and simulated with their final inputs:
/// their checksums are recorded. A host without local slots (a dedicated
/// server) may confirm ticks it has not simulated yet.
fn final_upto(s: &Session<WorldSim>) -> u32 {
    s.confirmed().min(s.current_tick())
}

/// The record of ticks `0..upto` (fewer when a row is missing); `None` for
/// a spectator that joined mid-match (its inputs start at its state).
fn record_of(s: &Session<WorldSim>, start: &MatchStartWire, upto: u32) -> Option<MatchRecord> {
    let log = s.input_log();
    if log.origin() > 0 {
        return None;
    }
    let rows: Vec<[u8; 32]> = (0..upto).map_while(|t| log.row(t)).collect();
    let n = rows.len() as u32;
    let checksums = s.checksums().iter().copied().filter(|&(t, _)| t < n).collect();
    Some(MatchRecord { start: start.clone(), rows, checksums })
}

/// The results of the last confirmed tick: why it ended, and each worm's
/// slot, score, kills, deaths and lives; the match's last recorded tick.
fn end_wire(last: Option<&(Vec<Score>, Option<MatchResult>)>, last_tick: u32) -> MatchEndWire {
    let reason = match last.and_then(|l| l.1).map(|r| r.reason) {
        Some(EndReason::KillLimit) => "kill limit",
        Some(EndReason::TimeLimit) => "time limit",
        Some(EndReason::LastStanding) => "last worm standing",
        Some(EndReason::Other(_)) | None => "the match ended",
    };
    let rows = last
        .map(|l| {
            l.0.iter()
                .take(rl_proto::MAX_SLOTS)
                .map(|s| (u8::try_from(s.worm).unwrap_or(u8::MAX), s.score, s.kills, s.deaths, s.lives))
                .collect()
        })
        .unwrap_or_default();
    MatchEndWire { reason: reason.into(), rows, last_tick }
}

/// The game's side of the session: the local player's keys, the host's
/// bots, the tick records and the caller's view of each tick.
struct PeerDriver<'a> {
    my_slot: Option<u8>,
    keys: &'a mut dyn FnMut() -> Keys,
    bots: Option<&'a mut HostBots>,
    recorder: &'a mut TickRecorder,
    on_tick: &'a mut dyn FnMut(&World, &TickEvents, TickInfo),
}

impl Driver<WorldSim> for PeerDriver<'_> {
    fn local_inputs(&mut self, sim: &WorldSim, _tick: u32, slots: &[u8], out: &mut [Keys]) {
        let human = self.my_slot.map(|s| (s, &mut *self.keys as &mut dyn FnMut() -> Keys));
        match &mut self.bots {
            Some(bots) => bots.press(sim, slots, out, human),
            None => {
                for (&slot, key) in slots.iter().zip(out.iter_mut()) {
                    *key = if Some(slot) == self.my_slot { (self.keys)() } else { Keys(0) };
                }
            }
        }
    }

    fn simulated(&mut self, sim: &WorldSim, info: TickInfo) {
        self.recorder.simulated(sim, info);
        (self.on_tick)(sim.world(), sim.events(), info);
    }
}

fn send(ep: &mut Outbox, peer: PeerId, ch: Channel, bytes: &[u8]) {
    match ch {
        Channel::Unreliable => ep.send(peer, ch, bytes),
        Channel::Reliable => ep.send_part(peer, TAG_NETCODE, bytes, state_part(bytes)),
    }
}

/// What the session's reliable message `control` is to the outbox's bound:
/// a state transfer's `StateBegin` or `StateChunk`, else an ordinary
/// message. postcard encodes a variant's index first (one byte below 128),
/// so a chunk is known without decoding its 32 KiB.
fn state_part(control: &[u8]) -> Part {
    let variant = |c: &Control| encode_control(c).first().copied();
    let first = control.first().copied();
    if first == variant(&Control::StateChunk { index: 0, bytes: Vec::new() }) {
        Part::StateChunk
    } else if first == variant(&Control::StateBegin { tick: 0, len: 0, chunks: 0 }) {
        match decode_control(control) {
            Ok(Control::StateBegin { chunks, .. }) => Part::StateBegin { chunks: usize::from(chunks) },
            _ => Part::Message,
        }
    } else {
        Part::Message
    }
}

fn send_tagged(ep: &mut Outbox, peer: PeerId, tag: u8, bytes: &[u8]) {
    ep.send_tagged(peer, tag, bytes);
}

fn why_text(why: Why) -> &'static str {
    match why {
        Why::Closed => "the connection closed",
        Why::Timeout => "the host stopped answering",
        Why::Limits => "the host sent too much",
        Why::Malformed => "the host sent a malformed message",
        Why::Refused => "the host refused us",
        Why::Unsent => "this game could not send to the host",
    }
}

fn refusal_text(r: Refusal) -> &'static str {
    match r {
        Refusal::Version => "the host plays another version",
        Refusal::Full => "the lobby is full",
        Refusal::Banned => "you are banned from this lobby",
        Refusal::Busy => "you are in this lobby already",
        Refusal::Malformed => "the host refused the handshake",
        Refusal::Password => "wrong password",
        Refusal::Exhausted => "the host ran out of connection ids and must restart",
        Refusal::Throttled => "too many wrong passwords: try again in a minute",
        Refusal::NotHost => "this player is not hosting the game",
        Refusal::Closed => "this game takes back its own players only",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matches::{bot_wire, rules_wire, setup_packs};
    use rl_proto::{BotWire, SlotWire};

    fn content() -> Option<Content> {
        let dir = std::env::var("RL_CONTENT_DIR").ok().filter(|d| std::path::Path::new(d).is_dir());
        let Some(dir) = dir else {
            eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
            return None;
        };
        Some(Content::open(std::path::Path::new(&dir)).unwrap())
    }

    fn look() -> Look {
        Look {
            name: "Bot".into(),
            colour: [1, 2, 3],
            skin: "olx-skin-default".into(),
            skin_hash: rl_proto::NO_HASH.into(),
        }
    }

    /// A record of 50 ticks of two bots on Dirt Level.
    fn record(c: &Content) -> MatchRecord {
        let (m, l) = ("olx-classic", "olx-level-dirt-level");
        let bot: BotWire = bot_wire(&rl_bots::BotConfig::default(), look());
        let setup = MatchSetupWire {
            mod_id: m.into(),
            level_id: l.into(),
            packs: setup_packs(c, m, l).unwrap(),
            rules: rules_wire(&rl_modes::DeathmatchSettings::default()),
            bonuses: false,
            loading_time: 100,
            restrictions: String::new(),
            bots: vec![bot.clone(), bot],
        };
        let slots =
            (0..2).map(|i| SlotWire { member: None, bot: Some(i), look: look(), loadout: None }).collect();
        let start = MatchStartWire { seed: 1, setup, slots, input_delay: vec![], start_checksum: 0 };
        MatchRecord { start, rows: vec![[0; 32]; 50], checksums: vec![] }
    }

    /// A client whose host is not there: nothing but its replays happens.
    fn peer(c: Content) -> OnlinePeer {
        let me = Identity::from_secret([7; 32]).unwrap();
        let role = PeerRole::Client {
            join: Join::direct(SocketAddr::from(([127, 0, 0, 1], 9))),
            password: None,
            expected_host: None,
            migration: MigrationConfig::off(),
        };
        let cfg = EndpointConfig { port: 0, ..EndpointConfig::default() };
        OnlinePeer::start(role, cfg, me, look(), c, None, PeerContent::without_downloads("a test")).unwrap()
    }

    #[test]
    fn one_replay_is_made_at_a_time() {
        let Some(c) = content() else { return };
        let r = record(&c);
        let mut p = peer(c);
        assert_eq!(p.events, vec![OnlineEvent::Joining(crate::join::JoinStep::Direct)]);
        p.events.clear();
        p.make_replay(Some(r.clone()));
        p.make_replay(Some(r.clone()));
        p.make_replay(Some(r.clone()));
        assert_eq!(p.events, vec![OnlineEvent::Notice(REPLAY_SKIPPED.into()); 2], "the others skipped");
        p.events.clear();
        let begun = std::time::Instant::now();
        while p.replay.is_some() {
            assert!(begun.elapsed() < Duration::from_secs(10), "the replay is made");
            p.collect_replay();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(p.events[..], [OnlineEvent::ReplayReady(_)]), "{:?}", p.events.len());
        // The worker is free again.
        p.make_replay(Some(r));
        assert!(p.replay.is_some());
    }

    /// A join through Nostr without `expected_host` expects the host's key
    /// it asks (N37's proof binds it: `WRONG_HOST`, not `UNPROVEN_HOST`).
    #[test]
    fn a_nostr_join_expects_its_hosts_key() {
        let Some(c) = content() else { return };
        let host = Identity::from_secret([9; 32]).unwrap().public();
        let nostr = crate::join::NostrJoin {
            host,
            pool: rl_discovery::PoolConfig::default(),
            region: rl_discovery::Region::Europe,
            turn_from_nostr: false,
            allow_local: false,
        };
        let role = PeerRole::Client {
            join: Join { direct: None, nostr: Some(nostr) },
            password: None,
            expected_host: None,
            migration: MigrationConfig::off(),
        };
        let cfg = EndpointConfig { port: 0, ..EndpointConfig::default() };
        let me = Identity::from_secret([7; 32]).unwrap();
        let p = OnlinePeer::start(role, cfg, me, look(), c, None, PeerContent::without_downloads("a test"))
            .unwrap();
        assert!(matches!(&p.side, Side::Client(c) if c.expected), "a key is expected");
        assert_eq!(p.events, vec![OnlineEvent::Joining(crate::join::JoinStep::Relays)]);
        assert_eq!(p.local_port(), 0, "no endpoint before gathering");
    }

    /// The session's state messages are told apart for the outbox's bound.
    #[test]
    fn state_messages_are_told_apart() {
        let begin = encode_control(&Control::StateBegin { tick: 9, len: 70_000, chunks: 3 });
        assert_eq!(state_part(&begin), Part::StateBegin { chunks: 3 });
        let chunk = encode_control(&Control::StateChunk { index: 2, bytes: vec![7; rl_proto::STATE_CHUNK] });
        assert_eq!(state_part(&chunk), Part::StateChunk);
        for other in [
            Control::Checksum { tick: 1, hash: 2 },
            Control::StateLoaded { tick: 3 },
            Control::Bye,
            Control::Desync { tick: 4, host_hash: 5 },
        ] {
            assert_eq!(state_part(&encode_control(&other)), Part::Message, "{other:?}");
        }
        assert_eq!(state_part(&[]), Part::Message);
    }

    /// A host that fails its proof is "not the game you were invited to"
    /// only when there was an invitation (a key expected); otherwise (a LAN
    /// or direct join) it could not prove who it is.
    #[test]
    fn a_failed_host_proof_names_the_invitation_only_when_there_was_one() {
        assert_eq!(wrong_host_text(true), WRONG_HOST);
        assert_eq!(wrong_host_text(false), UNPROVEN_HOST);
        assert_eq!(UNPROVEN_HOST, "this game could not prove who it is");
    }

    /// A client's own failed send is not blamed on the host.
    #[test]
    fn a_failed_send_is_not_the_hosts_fault() {
        assert_eq!(why_text(Why::Limits), "the host sent too much");
        assert_eq!(why_text(Why::Unsent), "this game could not send to the host");
    }
}
