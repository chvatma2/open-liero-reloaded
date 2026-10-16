//! A host migration over real connections (networking spec §3.7, plan
//! 12c): a client's standby endpoint (N63), its migration from the host's
//! loss (`rl_netcode::migration::Migration`: which successor it tries,
//! whether it hosts, when it gives up), its joins to the successors
//! (directly, then through the relays: `migration::Planner`, N73), its
//! promotion when its turn comes (the migrated lobby, its relays, the
//! take-over, the bots), the new host's collection and resume (N68), a
//! member back after that resume (N72), the
//! cancel when the old host speaks again (N69), the host lost; and a
//! host's step-down after a stall of its own (N70). Every rule is
//! `rl_netcode`'s and `rl_lobby`'s; this drives them with sockets.

use std::sync::atomic::{AtomicBool, Ordering};

use rl_net::endpoint::IceConfig;
use rl_netcode::migration::{HOST_SILENT_US, MAX_MIGRATIONS, STEP_DOWN_US, Step, quorum};
use rl_proto::{MAX_MEMBERS, MAX_SPECTATORS, SuccessorWire};

use super::relays::MigrationRelays;
use super::*;
use crate::join::{JoinStep, drop_pool};
use crate::migration::{
    ANSWER_OPEN_TIMEOUT, AttemptOutcome, Attempted, Connector, Failure, HOST_BACK, LostWhy,
    MIGRATION_CONNECT_TIMEOUT, MigrationStep, Planner, REPORT_WAIT, Route, n64_direct,
};

/// A client's standby endpoint (N63): a host endpoint on its own port whose
/// gate refuses `NotHost` until this peer's turn comes.
pub(super) struct Standby {
    out: Outbox,
    /// The gate admits offers (its turn, then for good once it hosts).
    open: Arc<AtomicBool>,
    /// The gate refuses these addresses once it hosts (the lobby's bans).
    banned: Arc<Mutex<BTreeSet<IpAddr>>>,
    port: u16,
    /// Its turn: the migrated lobby taking the match's members back, until
    /// a quorum commits it (ruling FR12d-Q), or its window ends (D8).
    taking: Option<Box<HostSide>>,
    /// Links refused before the commit, closed once their refusal left.
    closing: BTreeMap<PeerId, Instant>,
    /// Ruling FR12d-Q, while its turn's lobby waits for a quorum: the
    /// other members of the succession (`away` as its turn opened:
    /// followers too, FR12d-Q1)...
    members: usize,
    /// ...the members it took back (their `Welcome` and everything after
    /// it held: they follow no host yet, so a gate closing uncommitted
    /// leaves them free to follow the next window, D8)...
    welcomed: BTreeSet<PeerId>,
    held: Vec<(PeerId, ToClient)>,
    /// ...and the lobby's events from the first rejoin on, a host's once it
    /// commits.
    held_events: Vec<HostEvent>,
}

/// One client's migration, from the host's loss until a new host welcomed
/// it (A9), it took over, or the old host was back (N69).
pub(super) struct Migrating {
    /// Which successor it tries, by which route, and when not.
    plan: Planner<Box<Attempt>>,
    /// The step last reported.
    reported: Option<MigrationStep>,
    /// The joins' step last reported (a `NotHost` retry every `RETRY_US`
    /// starts its join again: its step is not said again), and the entry
    /// it was for (another entry's steps are said afresh).
    joining: Option<JoinStep>,
    joining_to: Option<usize>,
    /// The old host is known gone: its `Bye` (the session's or the
    /// lobby's) came. Ruling FR12c-1: a successor expecting nobody commits
    /// only then; its own drop would look the same.
    gone: bool,
    /// The last attempt to reach a successor, how it ended (ruling
    /// FR12c-4): a host lost names it.
    last: Option<Attempted>,
    /// The attempts' outcomes said: (entry, route, window end, kind), each
    /// once (a `NotHost` asked again every `RETRY_US` is said once).
    said: BTreeSet<(usize, Route, u64, u8)>,
}

impl Migrating {
    /// Ruling FR12c-4: how an attempt to entry `to` by `route` ended, kept
    /// as the last, and said once per entry, route, window and kind, with
    /// the successor's name and address.
    fn note(
        &mut self,
        to: usize,
        route: Route,
        outcome: AttemptOutcome,
        now_us: u64,
        names: &BTreeMap<u8, String>,
        out: &mut Vec<OnlineEvent>,
    ) {
        let entry = self.plan.order().get(to);
        let successor = entry.map_or_else(
            || "the successor".into(),
            |e| names.get(&e.member).cloned().unwrap_or_else(|| format!("member {}", e.member)),
        );
        let direct = entry
            .and_then(|e| e.direct)
            .filter(|_| route == Route::Direct)
            .filter(|&(ip, port)| n64_direct(ip, port))
            .map(|(ip, port)| SocketAddr::from((ip, port)));
        let kind = match outcome {
            AttemptOutcome::Unreached(_) => 0,
            AttemptOutcome::NotHosting => 1,
            AttemptOutcome::Refused(_) => 2,
            AttemptOutcome::Closed(_) => 3,
            AttemptOutcome::Lapsed { .. } => 4,
            AttemptOutcome::TimedOut { .. } => 5,
        };
        let attempted = Attempted { successor, direct, outcome };
        if self.said.insert((to, route, self.plan.window_end(now_us), kind)) {
            out.push(OnlineEvent::MigrationLog(attempted.to_string()));
        }
        self.last = Some(attempted);
    }

    /// The joins' events to `out`, each step once until another comes.
    fn report_joins(&mut self, steps: Vec<OnlineEvent>, out: &mut Vec<OnlineEvent>) {
        for e in steps {
            if let OnlineEvent::Joining(step) = e {
                if self.joining == Some(step) {
                    continue;
                }
                self.joining = Some(step);
            }
            out.push(e);
        }
    }
}

/// A join to a successor: its own endpoint and lobby client, until its
/// welcome (the commit) or its failure.
pub(super) struct Attempt {
    joiner: Joiner,
    out: Outbox,
    lobby: LobbyClient,
    /// How the new link goes, once open.
    connection: Option<(Path, SocketAddr)>,
    /// When the link opened.
    opened_at: Option<Instant>,
    /// The lobby's messages wait this long after the link opened (the
    /// test hook `test_delay_rejoin`; zero otherwise), held meanwhile.
    hello_delay: Duration,
    held: Vec<ToHost>,
}

/// The real [`Connector`]: a `Joiner` (direct, or through the relays) with
/// the client's endpoint config, `migration.ice` and
/// `MIGRATION_CONNECT_TIMEOUT`, and a fresh `LobbyClient` expecting the
/// entry's key (reporting the standby); the join's first step reported.
struct Connect<'a> {
    base: &'a EndpointConfig,
    migration: &'a MigrationConfig,
    me: &'a Identity,
    my_key: &'a PublicKey,
    /// The client's lobby (its look there).
    lobby: &'a LobbyClient,
    /// The standby's port, if any.
    standby: Option<u16>,
    outage: Option<Instant>,
    hello_delay: Duration,
    events: &'a mut Vec<OnlineEvent>,
    /// The joins started: the entry's member, direct or not.
    started: Vec<(u8, bool)>,
    /// The standing relays (FR12c-5): a join through them borrows their
    /// pool.
    relays: Option<&'a mut MigrationRelays>,
    now: Instant,
}

impl Connector for Connect<'_> {
    type Attempt = Box<Attempt>;

    fn connect(&mut self, entry: &SuccessorWire, join: Join) -> Option<Box<Attempt>> {
        let cfg = EndpointConfig {
            port: 0,
            connect_timeout: MIGRATION_CONNECT_TIMEOUT,
            ice: self.migration.ice.clone(),
            ..self.base.clone()
        };
        let direct = join.direct.is_some();
        // Through the relays, on the standing pool when it has them (FR12c-5).
        let lent = match (&join.nostr, self.relays.as_deref_mut()) {
            (Some(n), Some(r)) if join.direct.is_none() => r.lend(&n.pool.relays, self.now),
            _ => None,
        };
        let (joiner, ep, step) = match Joiner::start_lent(join, cfg, lent) {
            Ok(started) => started,
            // A lent pool went with the failed start: the next join makes
            // its own (never on the caller's thread: `drop_pool`).
            Err(_) => return None,
        };
        let look = my_look(self.lobby, self.my_key);
        let mut lobby = LobbyClient::new(self.me.clone(), look, None, Some(entry.key));
        if let Some(port) = self.standby {
            lobby.standby(port, self.migration.reported_relays());
        }
        let mut out = ep.map_or_else(Outbox::empty, Outbox::new);
        darken(self.outage, &mut out);
        self.events.push(OnlineEvent::Joining(step));
        self.started.push((entry.member, direct));
        Some(Box::new(Attempt {
            joiner,
            out,
            lobby,
            connection: None,
            opened_at: None,
            hello_delay: self.hello_delay,
            held: Vec::new(),
        }))
    }

    fn discard(&mut self, attempt: Box<Attempt>) {
        end_attempt(attempt, self.relays.as_deref_mut());
    }
}

/// Attempts given up, their endpoints dropped on threads, a lent pool
/// back to the standing relays.
struct OffThread<'a>(Option<&'a mut MigrationRelays>);

impl Connector for OffThread<'_> {
    type Attempt = Box<Attempt>;

    fn connect(&mut self, _: &SuccessorWire, _: Join) -> Option<Box<Attempt>> {
        None
    }

    fn discard(&mut self, attempt: Box<Attempt>) {
        end_attempt(attempt, self.0.as_deref_mut());
    }
}

/// An attempt ends: a pool it borrowed goes back to the standing relays
/// (FR12c-5; without them, dropped on a thread), its endpoint dropped on a
/// thread.
fn end_attempt(mut a: Box<Attempt>, relays: Option<&mut MigrationRelays>) {
    give_back(&mut a, relays);
    drop_off_thread(a.out);
}

/// The pool the attempt borrowed, back to the standing relays.
fn give_back(a: &mut Attempt, relays: Option<&mut MigrationRelays>) {
    if let Some(pool) = a.joiner.give_back() {
        match relays {
            Some(r) => r.give_back(pool),
            None => drop_pool(pool),
        }
    }
}

impl Attempt {
    /// The lobby's messages to the successor: sent once the link has been
    /// open `hello_delay`, held until then.
    fn flush(&mut self, now: Instant) {
        self.held.extend(self.lobby.outgoing());
        if self.opened_at.is_some_and(|t| now >= t + self.hello_delay) {
            for m in self.held.drain(..) {
                send_tagged(&mut self.out, HOST, TAG_TO_HOST, &encode_to_host(&m));
            }
        }
    }
}

/// How an attempt's poll ended.
enum Outcome {
    /// Welcomed: these lobby events (the poll's other events follow).
    Welcome(Vec<ClientEvent>),
    /// It failed: what the planner makes of it, and why in words.
    Failed(Failure, AttemptOutcome),
}

/// A successor's link lost, in words (not the host's: `why_text`'s).
fn link_text(why: rl_net::endpoint::Why) -> &'static str {
    use rl_net::endpoint::Why;
    match why {
        Why::Closed => "the connection was refused or closed",
        Why::Timeout => "no answer in time",
        Why::Limits => "it sent too much",
        Why::Malformed => "it sent a malformed message",
        Why::Refused => "it refused the link",
        Why::Unsent => "this game could not send to it",
    }
}

/// What a successor's lobby said in refusing the join, in words.
fn lobby_refusal(e: &ClientEvent) -> Option<String> {
    Some(match e {
        ClientEvent::Refused(why) => refusal_text(*why).into(),
        ClientEvent::WrongHost { .. } => "another game answered at its address".into(),
        ClientEvent::Closed => "it closed the join".into(),
        ClientEvent::Kicked { .. } => "it kicked this player".into(),
        _ => return None,
    })
}

/// Drops an outbox's endpoint on a thread (`rl-online drop`): never on the
/// caller's (the game's UI thread). If no thread can start, here.
fn drop_off_thread(out: Outbox) {
    if out.endpoint().is_some() {
        let _ = std::thread::Builder::new().name("rl-online drop".into()).spawn(move || drop(out));
    }
}

/// A client's migration parts go (its end): the attempt's and the
/// standby's endpoints on threads, the successor's listener stopped.
pub(super) fn drop_migration(c: &mut ClientSide) {
    if let Some(a) = c.migrating.take().and_then(|mut mg| mg.plan.take_attempt()) {
        end_attempt(a, None);
    }
    c.relays = None;
    if let Some(sb) = c.standby.take() {
        let Standby { out, taking, .. } = *sb;
        if let Some(mut t) = taking {
            t.reach.stop();
        }
        drop_off_thread(out);
    }
}

/// N70, pre-flight C3: the stall between two polls. The monotonic `mono`
/// counts; the wall clock's `wall` only when it exceeds it by more than
/// `HOST_SILENT_US` (a suspend whose monotonic clock stood still, long
/// enough for the peers to have moved); a backward step (`None`) never.
pub(super) fn stall_gap(mono: Duration, wall: Option<Duration>) -> Duration {
    match wall {
        Some(w) if w > mono + Duration::from_micros(HOST_SILENT_US) => w,
        _ => mono,
    }
}

/// The succession a migration begins from: the lobby's, else (final
/// review B-M4) the one kept at a rejoin, the old one without the new host,
/// until the new host's first `Succession` comes (a rejoin that changed
/// the member list empties the lobby's until the new host's next drain,
/// `LOBBY_DRAIN`: a new host lost then ended the client as before plan 12c).
fn migration_order(lobby: &[SuccessorWire], kept: &[SuccessorWire]) -> Vec<SuccessorWire> {
    if lobby.is_empty() { kept.to_vec() } else { lobby.to_vec() }
}

/// A new endpoint of this peer in the test hook's outage too.
fn darken(outage: Option<Instant>, out: &mut Outbox) {
    #[cfg(feature = "test-hooks")]
    if let (Some(until), Some(ep)) = (outage, out.endpoint_mut()) {
        ep.test_blackout(until);
    }
    #[cfg(not(feature = "test-hooks"))]
    let _ = (outage, out);
}

/// The look a client last had in its lobby (its attempts join with it).
fn my_look(lobby: &LobbyClient, key: &PublicKey) -> Look {
    lobby.state().and_then(|s| s.members.iter().find(|m| m.key == *key)).map_or_else(
        || Look {
            name: "Player".into(),
            colour: [0; 3],
            skin: rl_lobby::DEFAULT_SKIN.into(),
            skin_hash: rl_proto::NO_HASH.into(),
        },
        |m| m.look.clone(),
    )
}

impl OnlinePeer {
    /// A client's standby endpoint's port (N63), once open.
    pub fn standby_port(&self) -> Option<u16> {
        match &self.side {
            Side::Client(c) => c.standby.as_ref().map(|s| s.port),
            Side::Host(_) => None,
        }
    }

    /// Tests only (`test-hooks`): every endpoint of this peer (its link, its
    /// standby, an attempt's, and those it makes before then) drops every
    /// datagram both ways until `until` (rl-net's `test_blackout`; the
    /// signalling's TCP is untouched).
    #[cfg(feature = "test-hooks")]
    pub fn test_outage(&mut self, until: Instant) {
        self.outage = Some(until);
        if let Some(ep) = self.endpoint.endpoint_mut() {
            ep.test_blackout(until);
        }
        if let Side::Client(c) = &mut self.side {
            if let Some(ep) = c.standby.as_mut().and_then(|s| s.out.endpoint_mut()) {
                ep.test_blackout(until);
            }
            let attempt = c.migrating.as_mut().and_then(|m| m.plan.attempt_mut());
            if let Some(ep) = attempt.and_then(|a| a.out.endpoint_mut()) {
                ep.test_blackout(until);
            }
        }
    }

    /// Tests only (`test-hooks`): once an attempt's link to a successor
    /// opens, this peer sends its `Hello` only `d` later (a slow path: a
    /// peer late while it keeps contact).
    #[cfg(feature = "test-hooks")]
    pub fn test_delay_rejoin(&mut self, d: Duration) {
        self.rejoin_delay = d;
    }

    /// Tests only (`test-hooks`): this client never answers a new host's
    /// `Collect` (a rejoined peer that never reports).
    #[cfg(feature = "test-hooks")]
    pub fn test_withhold_reports(&mut self) {
        self.withhold_reports = true;
    }

    /// Tests only (`test-hooks`): the match session sends nothing from now
    /// on while the transport stays alive, so the host's session (not its
    /// transport) times this peer out.
    #[cfg(feature = "test-hooks")]
    pub fn test_mute_game(&mut self) {
        self.mute_game = true;
    }

    /// The test hook's delay of a rejoin's `Hello` (zero without it).
    fn hello_delay(&self) -> Duration {
        #[cfg(feature = "test-hooks")]
        return self.rejoin_delay;
        #[cfg(not(feature = "test-hooks"))]
        Duration::ZERO
    }

    /// The running match's migrations so far (N74: a rejoin and a take-over
    /// each count; past `MAX_MIGRATIONS` the next is host lost); `None`
    /// without a match.
    pub fn migrations(&self) -> Option<u32> {
        self.game.as_ref().filter(|g| g.ended.is_none()).map(|g| g.migrations)
    }

    /// Tests only (`test-hooks`): the running match counts `n` migrations
    /// so far (N74's cap without playing them all).
    #[cfg(feature = "test-hooks")]
    pub fn test_set_migrations(&mut self, n: u32) {
        if let Some(g) = &mut self.game {
            g.migrations = n;
        }
    }

    /// Tests only (`test-hooks`): every join to a successor this peer
    /// started, in order: the entry's member id, and whether it was direct.
    #[cfg(feature = "test-hooks")]
    pub fn test_joins(&self) -> &[(u8, bool)] {
        &self.joins
    }

    /// Tests only (`test-hooks`): this peer's joins to successors skip
    /// direct addresses (a loopback test then exercises the relays).
    #[cfg(feature = "test-hooks")]
    pub fn test_ignore_direct(&mut self) {
        self.ignore_direct = true;
    }

    /// The test hook's skipping of direct addresses (`false` without it).
    fn ignore_direct(&self) -> bool {
        #[cfg(feature = "test-hooks")]
        return self.ignore_direct;
        #[cfg(not(feature = "test-hooks"))]
        false
    }

    /// The test hook's outage (`None` without `test-hooks`).
    fn outage(&self) -> Option<Instant> {
        #[cfg(feature = "test-hooks")]
        return self.outage;
        #[cfg(not(feature = "test-hooks"))]
        None
    }

    /// A client whose link to its host opened.
    pub(super) fn client_link_open(&self) -> bool {
        matches!(&self.side, Side::Client(c) if c.connection.is_some())
    }

    /// N63: on a welcome, a client with migration on opens its standby
    /// endpoint (port 0, no port mapping, room for every member) and
    /// reports it with its first 2 relays (`LobbyClient::standby` says it
    /// again after every welcome). A bind failure is a notice, and no
    /// standby: it follows a migration but never hosts (N79).
    pub(super) fn open_standby(&mut self) {
        let Side::Client(c) = &self.side else { return };
        if !c.migration.enabled || c.standby.is_some() {
            return;
        }
        let Some(cfg) = c.migration.standby.clone() else { return };
        let cfg = EndpointConfig {
            port: 0,
            max_peers: MAX_MEMBERS,
            ice: IceConfig { port_mapping: false, ..cfg.ice },
            ..cfg
        };
        let open = Arc::new(AtomicBool::new(false));
        let banned = Arc::new(Mutex::new(BTreeSet::new()));
        let (gate_open, gate_banned) = (Arc::clone(&open), Arc::clone(&banned));
        let gate: Gate = Box::new(move |addr, _| {
            if !gate_open.load(Ordering::Relaxed) {
                return Err(Refusal::NotHost);
            }
            let banned = gate_banned.lock().unwrap_or_else(|e| e.into_inner());
            if banned.contains(&addr.ip()) { Err(Refusal::Banned) } else { Ok(()) }
        });
        match Endpoint::host(cfg, gate) {
            Ok(ep) => {
                let port = ep.local_port();
                let mut out = Outbox::new(ep);
                darken(self.outage(), &mut out);
                let Side::Client(c) = &mut self.side else { return };
                c.lobby.standby(port, c.migration.reported_relays());
                let closing = BTreeMap::new();
                c.standby = Some(Box::new(Standby {
                    out,
                    open,
                    banned,
                    port,
                    taking: None,
                    closing,
                    members: 0,
                    welcomed: BTreeSet::new(),
                    held: Vec::new(),
                    held_events: Vec::new(),
                }));
            }
            Err(e) => self.events.push(OnlineEvent::Notice(format!(
                "this game cannot take the match over if the host leaves: {e}"
            ))),
        }
    }

    /// The host is gone during a match (its silence, its `Bye`, its link
    /// closed: N71, N76, A7): the session is suspended where it is and the
    /// migration begins from the host's succession. `gone`: its `Bye` came
    /// (known gone, ruling FR12c-1; also when a migration is under way).
    /// `true` when one is under way (begun now or before, or the host lost
    /// past `MAX_MIGRATIONS`); `false` when there is none to begin
    /// (migration off, outside a match, no succession): the caller ends as
    /// before.
    pub(super) fn begin_migration(&mut self, gone: bool) -> bool {
        let now_us = self.now_us;
        let my_key = self.my_key;
        let ignore_direct = self.ignore_direct();
        let Side::Client(c) = &mut self.side else { return false };
        if let Some(mg) = c.migrating.as_deref_mut() {
            mg.gone |= gone;
            return true;
        }
        // In the match, or (final review B-M2) a member waiting for the
        // next one while it runs (welcomed then, or still downloading): it
        // follows the match to the new host, as a late rejoiner does.
        let has_game = self.game.is_some();
        let mut game = self.game.as_mut().filter(|g| g.ended.is_none());
        let running =
            matches!(c.lobby.state().map(|s| s.phase), Some(Phase::Starting | Phase::Playing { .. }));
        if game.is_none() && (has_game || !running) {
            return false;
        }
        let order = migration_order(c.lobby.succession(), &c.kept_order);
        if !c.migration.enabled || order.is_empty() {
            return false;
        }
        // A spectator still joining has no world to follow a migration
        // with (ruling SK-8): host lost, not stranded on its join.
        let joining = game.as_ref().is_some_and(|g| g.session.is_joining());
        if game.as_ref().map_or(c.migrations, |g| g.migrations) >= MAX_MIGRATIONS || joining {
            let why = if joining { LostWhy::StillJoining } else { LostWhy::TooManyMigrations };
            self.host_lost(why);
            return true;
        }
        let me = order.iter().position(|e| e.key == my_key);
        let mut can_host: Vec<bool> = order.iter().map(|e| e.can_host).collect();
        if let Some(i) = me {
            // Its own turn needs its standby.
            can_host[i] &= c.standby.is_some();
        }
        if let Some(g) = game.as_mut() {
            g.session.suspend();
        }
        c.rejoined = None;
        let mut plan = Planner::new(order, can_host, me, &c.migration, now_us);
        if ignore_direct {
            plan.ignore_direct();
        }
        // B-M1: a loopback successor address is this PC only from the
        // host's own PC.
        plan.loopback_host(c.connection.is_some_and(|(_, remote)| remote.ip().is_loopback()));
        c.migrating = Some(Box::new(Migrating {
            plan,
            reported: None,
            joining: None,
            joining_to: None,
            gone,
            last: None,
            said: BTreeSet::new(),
        }));
        self.report_step(MigrationStep::Finding);
        true
    }

    /// `Migrating(step)` when it differs from the last reported.
    fn report_step(&mut self, step: MigrationStep) {
        let Side::Client(c) = &mut self.side else { return };
        let Some(mg) = c.migrating.as_deref_mut() else { return };
        if mg.reported.as_ref() != Some(&step) {
            mg.reported = Some(step.clone());
            self.events.push(OnlineEvent::Migrating(step));
        }
    }

    /// Every poll: the standby, the attempt, the migration's step, and a
    /// new host's resume.
    pub(super) fn migration_tasks(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        if let Side::Client(c) = &mut self.side
            && let Some(r) = c.relays.as_deref_mut()
        {
            r.poll(now);
        }
        self.standby_tasks(now);
        self.attempt_tasks(now);
        self.step_migration(now);
        self.resume_check(now);
    }

    /// The standby, polled every poll: closed, no link stays; on its turn
    /// the migrated lobby takes members back (their proofs, at once, their
    /// welcomes held), and a quorum of them commits (N68, ruling FR12d-Q:
    /// [`OnlinePeer::commit_if_quorum`]).
    fn standby_tasks(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        // A rejoin counts only within this peer's turn: one handled in the
        // poll its window ended does not (the gate closes after it, D8).
        let its_turn = c.migrating.as_mut().is_some_and(|mg| mg.plan.peek(now_us) == Step::Host);
        let Some(sb) = c.standby.as_deref_mut() else { return };
        let mut net = Vec::new();
        sb.out.poll(now, &mut net);
        let gone: Vec<PeerId> = sb
            .closing
            .iter()
            .filter(|&(&peer, &until)| now >= until || sb.out.buffered(peer) == 0)
            .map(|(&peer, _)| peer)
            .collect();
        for peer in gone {
            sb.closing.remove(&peer);
            sb.out.close(peer);
        }
        // Past its turn the gate closes in this poll's step: nothing is
        // answered or welcomed meanwhile.
        let Some(t) = sb.taking.as_deref_mut().filter(|_| its_turn) else {
            // Not its turn: no link stays (one that opened as the gate closed).
            for e in net {
                if let NetEvent::Connected { peer, .. } = e {
                    sb.out.close(peer);
                }
            }
            return;
        };
        // N73: its turn's relays carry joins too; their offers are answered
        // on the standby (its gate open), and what became of them said.
        if let Some(ep) = sb.out.endpoint_mut() {
            let mut notes = Vec::new();
            t.reach.answer(ep, now, &mut notes);
            self.events.extend(notes.into_iter().map(|(join, more)| OnlineEvent::RelayJoin { join, more }));
        }
        let news = t.reach.relay_news();
        self.events.extend(news.into_iter().map(|(url, status)| OnlineEvent::ListenRelay { url, status }));
        for e in net {
            match e {
                NetEvent::Connected { peer, addr, path } => {
                    t.addrs.insert(peer, addr);
                    let fps = sb.out.endpoint().and_then(|ep| ep.dtls_fingerprints(peer));
                    match (random_bytes::<32>(), fps) {
                        (Ok(nonce), Some(fps)) => {
                            t.lobby.connected(peer, addr.ip(), path, now_us, nonce, fps)
                        }
                        _ => {
                            sb.out.close(peer);
                            t.forget(peer);
                        }
                    }
                }
                NetEvent::Message { peer, channel: Channel::Reliable, bytes } => {
                    // Nothing but the lobby's handshake comes before a
                    // member's welcome.
                    if let Some(Tagged::ToHost(b)) = tag(&bytes) {
                        match decode_to_host(b) {
                            Ok(msg) => {
                                if let Some(sent) = t.challenged.remove(&peer) {
                                    let rtt = now.saturating_duration_since(sent).as_millis();
                                    t.rtts.entry(peer).or_insert(u32::try_from(rtt).unwrap_or(u32::MAX));
                                }
                                t.lobby.receive(peer, msg, now_us);
                            }
                            Err(_) => {
                                sb.out.close(peer);
                                t.lobby.disconnected(peer);
                                t.forget(peer);
                            }
                        }
                    }
                }
                NetEvent::Disconnected { peer, why } => {
                    if why == rl_net::endpoint::Why::Timeout {
                        t.lobby.timed_out(peer);
                    } else {
                        t.lobby.disconnected(peer);
                    }
                    t.forget(peer);
                    sb.closing.remove(&peer);
                }
                _ => {}
            }
        }
        t.lobby.tick(now_us);
        // The handshake's answers leave at once (the host's proof); a
        // rejoiner's welcome, and everything after it, waits for the commit
        // (ruling FR12d-Q), so it comes before anything the commit sends.
        for (peer, msg) in t.lobby.outgoing() {
            if matches!(msg, ToClient::Challenge { .. }) {
                t.challenged.insert(peer, now);
            }
            // Ruling FR12c-4: a join its migrated lobby refuses is logged
            // here (the joiner's side says it too).
            if let ToClient::Refused(why) = &msg {
                let from = t.addrs.get(&peer).map_or_else(|| "a joiner".into(), |a| a.to_string());
                let text = format!("taking over: refused a join from {from}: {}", refusal_text(*why));
                self.events.push(OnlineEvent::MigrationLog(text));
            }
            if matches!(msg, ToClient::Welcome { .. }) {
                sb.welcomed.insert(peer);
            }
            if sb.welcomed.contains(&peer) {
                sb.held.push((peer, msg));
            } else {
                send_tagged(&mut sb.out, peer, TAG_TO_CLIENT, &encode_to_client(&msg));
            }
        }
        t.last_drain = Some(now);
        for e in t.lobby.events() {
            // From the first rejoin on its lobby's events are a host's, once
            // it commits.
            if matches!(e, HostEvent::Rejoined(_)) || !sb.held_events.is_empty() {
                sb.held_events.push(e);
                continue;
            }
            if let HostEvent::Kick { peer, .. } | HostEvent::Close { peer } = e {
                sb.closing.entry(peer).or_insert(now + CLOSE_LINGER);
            }
        }
        self.commit_if_quorum(now);
    }

    /// Ruling FR12d-Q: in its turn, the successor commits only once it and
    /// the members back with it are strictly more than half of the match's
    /// members not known gone (`rl_netcode::migration::quorum`: the other
    /// members of the succession as its turn opened, followers too, ruling
    /// FR12d-Q1; itself; and its old host unless its `Bye` or the lobby's
    /// `Closed` came); with nobody else, once its old host is known gone
    /// (FR12c-1). Then the welcomes it held leave
    /// and the lobby's events since the first rejoin are a host's. This
    /// replaces FR12c-1's commit at the first rejoin: two successors'
    /// quorums share a member, so a match continues on one side of a
    /// partition at most.
    fn commit_if_quorum(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        let Some(mg) = c.migrating.as_deref_mut() else { return };
        if mg.plan.peek(now_us) != Step::Host {
            return;
        }
        let gone = mg.gone;
        let Some(sb) = c.standby.as_deref_mut() else { return };
        let Some(t) = sb.taking.as_deref() else { return };
        let back = sb.members.saturating_sub(t.lobby.away().len());
        if !quorum(sb.members, back, gone) {
            return;
        }
        let held = std::mem::take(&mut sb.held);
        let events = std::mem::take(&mut sb.held_events);
        self.take_over(now);
        for (peer, msg) in held {
            send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&msg));
        }
        self.on_host_events(events, now);
    }

    /// The attempt's endpoint and lobby: the host's answer is contact
    /// (N65), its welcome commits ([`OnlinePeer::rejoin`]); a refusal, a
    /// closed link, a failed join through the relays or an answered link
    /// that does not open in time fails it ([`Planner::failed`] decides
    /// what next). The join's steps are reported.
    fn attempt_tasks(&mut self, now: Instant) {
        let now_us = self.now_us;
        let outage = self.outage();
        let Side::Client(c) = &mut self.side else { return };
        let ClientSide { migrating, relays, .. } = &mut **c;
        let Some(mg) = migrating.as_deref_mut() else { return };
        let Some((to, route)) = mg.plan.attempt() else { return };
        let Some(a) = mg.plan.attempt_mut() else { return };
        let mut steps = Vec::new();
        let mut outcome = a
            .joiner
            .poll(now, &mut a.out, &mut steps)
            .err()
            .map(|why| Outcome::Failed(Failure::Unreached, AttemptOutcome::Unreached(why)));
        // A signalled endpoint made just now is in the test hook's outage too.
        darken(outage, &mut a.out);
        let mut net = Vec::new();
        a.out.poll(now, &mut net);
        let (mut answered, mut opened) = (false, false);
        let mut events = net.into_iter();
        let refused = |why: String| Some(Outcome::Failed(Failure::Refused, AttemptOutcome::Refused(why)));
        while outcome.is_none()
            && let Some(e) = events.next()
        {
            match e {
                NetEvent::Answered => answered = true,
                NetEvent::Connected { addr, path, .. } => {
                    let ep = a.out.endpoint();
                    let remote = ep.map_or(addr, |ep| far_end(ep, path, addr));
                    match ep.and_then(|ep| ep.dtls_fingerprints(HOST)) {
                        Some(fps) => {
                            a.lobby.connected(fps);
                            a.connection = Some((path, remote));
                            a.opened_at.get_or_insert(now);
                            opened = true;
                        }
                        None => outcome = refused("its link gave no DTLS fingerprints".into()),
                    }
                }
                NetEvent::Message { channel: Channel::Reliable, bytes, .. } => {
                    let Some(Tagged::ToClient(b)) = tag(&bytes) else { continue };
                    let Ok(msg) = decode_to_client(b) else {
                        outcome = refused("it sent a message this game cannot read".into());
                        continue;
                    };
                    a.lobby.receive(msg, random_bytes::<32>().unwrap_or([0; 32]));
                    a.flush(now);
                    let lobby_events = a.lobby.events();
                    let welcome = lobby_events.iter().any(|e| matches!(e, ClientEvent::Welcome { .. }));
                    if welcome {
                        outcome = Some(Outcome::Welcome(lobby_events));
                    } else if let Some(why) = lobby_events.iter().find_map(lobby_refusal) {
                        outcome = refused(why);
                    }
                }
                NetEvent::Refused { why: Refusal::NotHost } => {
                    outcome = Some(Outcome::Failed(Failure::NotHost, AttemptOutcome::NotHosting))
                }
                NetEvent::Refused { why } => outcome = refused(refusal_text(why).into()),
                // Before its link opened its route did not reach the
                // successor; after, the successor closed it.
                NetEvent::Disconnected { why, .. } => {
                    let text = link_text(why).to_string();
                    outcome = Some(if a.opened_at.is_some() {
                        Outcome::Failed(Failure::Refused, AttemptOutcome::Closed(text))
                    } else {
                        Outcome::Failed(Failure::Unreached, AttemptOutcome::Unreached(text))
                    });
                }
                _ => {}
            }
        }
        if outcome.is_none() {
            a.flush(now);
        }
        mg.report_joins(steps, &mut self.events);
        if answered {
            mg.plan.answered(now_us);
        }
        if opened {
            mg.plan.opened();
        }
        // A successor that answered but whose link does not open in its
        // route's time (it died or froze after its answer): contact lapses,
        // so this peer moves on with the others instead of waiting for its
        // setup's timeout, past the give-up.
        if outcome.is_none() && mg.plan.lapsed(now_us) {
            let secs = ANSWER_OPEN_TIMEOUT.as_secs();
            outcome = Some(Outcome::Failed(Failure::Unreached, AttemptOutcome::Lapsed { secs }));
        }
        match outcome {
            None => {}
            Some(Outcome::Welcome(lobby_events)) => {
                let rest: Vec<NetEvent> = events.collect();
                self.rejoin();
                for e in lobby_events {
                    self.on_client_event(e, now);
                }
                // The new link's other news are the peer's now.
                for e in rest {
                    self.on_net(e, now);
                }
            }
            Some(Outcome::Failed(why, said)) => {
                mg.note(to, route, said, now_us, &self.names, &mut self.events);
                // N69: the old host was heard while this contact held it
                // here; the successor that answered has gone back to it (or
                // is gone): so does this peer.
                if mg.plan.failed(now_us, why, &mut OffThread(relays.as_deref_mut())) {
                    self.call_off();
                }
            }
        }
    }

    /// The migration's step now: give up (host lost), host (the gate open;
    /// committed with a quorum, ruling FR12d-Q: at once when it expects
    /// nobody and its old host is known gone, N68 and FR12c-1; else once
    /// enough members rejoined), try a successor, or wait. A gate open
    /// uncommitted closes once the step is no longer `Host` (D8).
    fn step_migration(&mut self, now: Instant) {
        let now_us = self.now_us;
        let outage = self.outage();
        let hello_delay = self.hello_delay();
        let Side::Client(c) = &mut self.side else { return };
        let ClientSide { migrating, me, cfg, migration, lobby, standby, relays, .. } = &mut **c;
        let Some(mg) = migrating.as_deref_mut() else { return };
        let mut steps = Vec::new();
        let mut conn = Connect {
            base: cfg,
            migration,
            me,
            my_key: &self.my_key,
            lobby,
            standby: standby.as_ref().map(|sb| sb.port),
            outage,
            hello_delay,
            events: &mut steps,
            started: Vec::new(),
            relays: relays.as_deref_mut(),
            now,
        };
        let before = mg.plan.attempt().map(|(to, route)| (to, route, mg.plan.in_contact()));
        let step = mg.plan.step(now_us, &mut conn);
        let started = std::mem::take(&mut conn.started);
        // An attempt the step moved past (its window ended, its own turn,
        // the give-up) ended unfinished: said and kept too (FR12c-4).
        if let Some((to, route, answered)) = before
            && mg.plan.attempt().map(|a| a.0) != Some(to)
        {
            mg.note(to, route, AttemptOutcome::TimedOut { answered }, now_us, &self.names, &mut self.events);
        }
        let to = match step {
            Step::Try(i) => Some(i),
            _ => None,
        };
        if mg.joining_to != to {
            mg.joining_to = to;
            mg.joining = None;
        }
        mg.report_joins(steps, &mut self.events);
        #[cfg(feature = "test-hooks")]
        self.joins.extend(started);
        #[cfg(not(feature = "test-hooks"))]
        let _ = started;
        let moving = match step {
            Step::Try(i) => mg.plan.order().get(i).map(|e| e.member),
            _ => None,
        };
        let last = mg.last.clone();
        if step != Step::Host {
            self.close_gate();
        }
        match step {
            Step::GiveUp => self.host_lost(LostWhy::NobodyReachable(last)),
            Step::Wait => self.report_step(MigrationStep::Finding),
            Step::Host => {
                self.report_step(MigrationStep::TakingOver);
                // Expecting nobody, its quorum is its own only when its old
                // host said `Bye`: in a two-peer match its own blip looks
                // like the host's loss (ruling FR12c-1).
                self.open_gate();
                self.commit_if_quorum(now);
            }
            Step::Try(_) => {
                if let Some(member) = moving {
                    let name = self.name(member);
                    self.report_step(MigrationStep::Moving(name));
                }
            }
        }
    }

    /// The attempt goes (its endpoint on a thread, a lent pool back).
    fn drop_attempt(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        if let Some(a) = c.migrating.as_mut().and_then(|mg| mg.plan.take_attempt()) {
            end_attempt(a, c.relays.as_deref_mut());
        }
    }

    /// Its turn (`Step::Host`): the gate opens and the migrated lobby (A13:
    /// the old lobby's state and succession, the match's member ids, the
    /// state's name, its player slots, `MAX_SPECTATORS`, the admins from
    /// the members' marks, no password) waits for the members; with
    /// `migration.relays` it listens on them from now (N73, A15; answered
    /// in `standby_tasks`). Every other member of the succession, away
    /// until it rejoins, is its quorum's (rulings FR12d-Q, FR12d-Q1).
    fn open_gate(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        let Some(sb) = c.standby.as_deref_mut() else { return };
        if sb.taking.is_some() {
            return;
        }
        let (Some(state), Some(g)) = (c.lobby.state(), self.game.as_ref()) else { return };
        let match_ids: Vec<u8> = g.start.slots.iter().filter_map(|s| s.member).collect();
        let cfg = HostConfig {
            name: state.name.clone(),
            password: None,
            max_players: match_ids.len().clamp(1, rl_proto::MAX_SLOTS) as u8,
            max_spectators: MAX_SPECTATORS as u8,
            admins: state.members.iter().filter(|m| m.admin).map(|m| m.key).collect(),
        };
        let Ok(lobby) = LobbyHost::migrated(c.me.clone(), state, c.lobby.succession(), &match_ids, cfg)
        else {
            // It cannot host: its window passes.
            return;
        };
        // Its listener on the standing relays' pool, connected since the
        // match's start (FR12c-5); without them, a pool of its own.
        let now = self.now.unwrap_or_else(Instant::now);
        let lent = c.relays.as_deref_mut().and_then(|r| Some((r.config().clone(), r.lend_to_listener(now)?)));
        let reach = match lent {
            Some((pool, lent)) => {
                HostReach::start_on(HostListen { pool: Some(pool), public_address: None }, &c.me, Some(lent))
            }
            None => {
                HostReach::start(HostListen { pool: c.migration.relays.clone(), public_address: None }, &c.me)
            }
        };
        let Ok(reach) = reach.or_else(|_| HostReach::start(HostListen::off(), &c.me)) else {
            return;
        };
        sb.members = lobby.away().len();
        sb.welcomed.clear();
        sb.held.clear();
        sb.held_events.clear();
        sb.taking = Some(Box::new(HostSide {
            lobby,
            banned: Arc::clone(&sb.banned),
            addrs: BTreeMap::new(),
            rtts: BTreeMap::new(),
            challenged: BTreeMap::new(),
            last_drain: None,
            last_ping: None,
            reported: None,
            reach,
        }));
        sb.open.store(true, Ordering::Relaxed);
    }

    /// The gate closes (an uncommitted successor's window ended, D8; the
    /// old host is back, N69): the joins it answered are refused `NotHost`
    /// (their joiners lose contact at once), the links not welcomed yet
    /// close, the migrated lobby and its listener go.
    fn close_gate(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        let Some(sb) = c.standby.as_deref_mut() else { return };
        sb.open.store(false, Ordering::Relaxed);
        let Some(mut t) = sb.taking.take() else { return };
        // The members it held follow no host: their links close below.
        sb.welcomed.clear();
        sb.held.clear();
        sb.held_events.clear();
        t.reach.stop();
        if let Some(ep) = sb.out.endpoint_mut() {
            ep.refuse_pending(Refusal::NotHost);
        }
        let peers: Vec<PeerId> = sb.out.peers().map(|(p, _)| p).collect();
        for peer in peers {
            sb.out.close(peer);
        }
        sb.closing.clear();
    }

    /// The successor commits (N68, ruling FR12d-Q: its quorum back):
    /// the old endpoint shuts down on a thread and the standby becomes the
    /// peer's endpoint, the migrated lobby a host's side, the content side
    /// a host's; the session takes over with the bot slots and the bots
    /// restart (N75); it collects until its resume (`resume_check`).
    pub(super) fn take_over(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        let Some(sb) = c.standby.take() else { return };
        let Standby { out, taking, closing, .. } = *sb;
        let Some(taking) = taking else {
            drop_off_thread(out);
            return;
        };
        let window_end = c.migrating.as_ref().map_or(now_us, |mg| mg.plan.window_end(now_us));
        if let Some(a) = c.migrating.take().and_then(|mut mg| mg.plan.take_attempt()) {
            end_attempt(a, c.relays.as_deref_mut());
        }
        let old = std::mem::replace(&mut self.endpoint, out);
        drop_off_thread(old);
        self.closing.clear();
        // Refusals before the commit still close after their linger.
        self.closing.extend(closing.into_iter().map(|(peer, until)| (peer, until.max(now))));
        self.side = Side::Host(taking);
        self.content.become_host();
        if let Some(g) = &mut self.game {
            let bots: Vec<u8> = g
                .start
                .slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s.bot.is_some())
                .map(|(i, _)| i as u8)
                .collect();
            g.session.take_over(&bots, now_us);
            g.bots = Some(match matches::restart_bots(&g.start, g.session.sim().world()) {
                Ok(bots) => bots,
                Err(e) => {
                    self.events.push(OnlineEvent::Notice(format!("the bots could not restart: {e:#}")));
                    HostBots::new(BTreeMap::new())
                }
            });
            g.migrations += 1;
            g.collecting = Some(window_end);
        }
    }

    /// A new host: member `id` rejoined (`HostEvent::Rejoined`): its
    /// welcome leaves first, then the content list and the session's
    /// `Collect` (`rejoined` with its slot of the match, else a spectator
    /// seat), in that order on the ordered channel.
    pub(super) fn host_rejoined(&mut self, id: u8, now: Instant) {
        let now_us = self.now_us;
        self.drain_lobby(now);
        let Side::Host(h) = &self.side else { return };
        let Some(peer) = h.lobby.peer_of(id) else { return };
        self.content.joined(peer);
        let Some(g) = self.game.as_mut().filter(|g| g.ended.is_none()) else { return };
        let seat = g
            .start
            .slots
            .iter()
            .position(|s| s.member == Some(id))
            .map_or(Seat::Spectator, |i| Seat::Player(i as u8));
        g.session.rejoined(peer, seat, now_us);
    }

    /// A new host resumes when every member it waits for is back
    /// (`away_in_match`, A10) and every member back reported; at its
    /// window's end it stops waiting for those not back, but not for the
    /// reports of those back (C1). Then the away members leave, the
    /// session resumes, the lobby plays on: `Migrated { me: true }`.
    fn resume_check(&mut self, now: Instant) {
        let now_us = self.now_us;
        if !self.game.as_ref().is_some_and(|g| g.collecting.is_some()) {
            return;
        }
        // A member the lobby took back in this poll is rejoined in the
        // session first (its `Collect`): it is back before the resume, not
        // left without one (`rejoined` is refused after it).
        self.host_lobby_events(now);
        let Side::Host(h) = &mut self.side else { return };
        let Some(g) = self.game.as_mut() else { return };
        let Some(window_end) = g.collecting else { return };
        let waiting = !h.lobby.away_in_match().is_empty() && now_us < window_end;
        // The reports of those back are awaited one round trip past the
        // window at most: a rejoined peer that never reports holds nobody
        // longer (a late report is merged all the same, N67).
        let reports_due = now_us < window_end.saturating_add(REPORT_WAIT.as_micros() as u64);
        if waiting || (reports_due && !g.session.awaiting_reports().is_empty()) {
            return;
        }
        h.lobby.drop_away();
        let tick = g.session.resume(now_us);
        h.lobby.playing(tick);
        g.collecting = None;
        let host = self.me().map_or_else(|| "this player".into(), |id| self.name(id));
        self.events.push(OnlineEvent::Migrated { host, me: true });
    }

    /// The client commits to a new host (its attempt's welcome): it drops
    /// its migration (A9), the old endpoint shuts down on a thread and the
    /// attempt's is the peer's, its lobby client too; the content follows
    /// the new host. In time (its old member id), the session rehosts
    /// (suspended until the new host's `Resume`; `Migrated` and `Connected`
    /// come then). Late (N72: after the new host's resume it is a new
    /// member, waiting): its old session goes, `Migrated` and `Connected`
    /// come now, and it watches as a mid-match joiner (`Start`,
    /// `Role::Joining`) once its content is complete; its slot stays
    /// `Keys(0)` at the new host.
    fn rejoin(&mut self) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        let Some(mut mg) = c.migrating.take() else { return };
        let to = mg.plan.attempt().map(|(to, _)| to);
        // B-M4: the old succession without the new host, until its own.
        let new_host = to.and_then(|to| mg.plan.order().get(to)).map(|e| e.key);
        c.kept_order = mg.plan.order().iter().filter(|e| Some(e.key) != new_host).cloned().collect();
        let Some(mut a) = mg.plan.take_attempt() else { return };
        // A pool it borrowed goes back: the match goes on, and a further
        // migration (N74) may need it (FR12c-5).
        give_back(&mut a, c.relays.as_deref_mut());
        let Attempt { out, lobby, connection, .. } = *a;
        let member = to.and_then(|to| mg.plan.order().get(to)).map(|e| e.member);
        let name = member.map_or_else(
            || "the new host".into(),
            |id| self.names.get(&id).cloned().unwrap_or_else(|| format!("member {id}")),
        );
        // A migrated lobby keeps a member's id until its resume; one back
        // after it is taken as a new member.
        let late = c.lobby.me().is_some_and(|old| lobby.me() != Some(old));
        // Late: the match it played up to here makes its replay (N18), as at
        // a host lost, before its member id changes, and its world is kept
        // for the game to draw until it watches (`take_ended_world`); the
        // match's migrations so far still count for the session it watches
        // from (N74).
        let carried = if late { self.game.take() } else { None }.map(|g| {
            self.ended_world = Some(g.session.sim().world().clone());
            self.make_replay(record_of(&g.session, &g.start, g.session.frontier()));
            g.migrations + 1
        });
        let Side::Client(c) = &mut self.side else { return };
        let old = std::mem::replace(&mut self.endpoint, out);
        drop_off_thread(old);
        c.lobby = lobby;
        c.joiner = None;
        c.waiting_since = None;
        c.connection = connection;
        c.expected = true;
        self.closing.clear();
        self.content.host_changed();
        // A member that waited outside the match (B-M2) has no session to
        // resume: it is through at once, its count carried on.
        let waited = self.game.is_none() && !late;
        if late || waited {
            c.migrations = carried.unwrap_or(c.migrations + 1);
            self.events.push(OnlineEvent::Migrated { host: name, me: false });
            if let Some((path, remote)) = connection {
                self.events.push(OnlineEvent::Connected { path, remote });
            }
        } else {
            c.rejoined = Some(name);
            if let Some(g) = &mut self.game {
                g.session.rehost(HOST, now_us);
                g.migrations += 1;
            }
        }
    }

    /// A client's session resumed under its new host: `Migrated`, then the
    /// new link's `Connected`.
    pub(super) fn resumed(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        let Some(host) = c.rejoined.take() else { return };
        let connection = c.connection;
        self.events.push(OnlineEvent::Migrated { host, me: false });
        if let Some((path, remote)) = connection {
            self.events.push(OnlineEvent::Connected { path, remote });
        }
    }

    /// N69: the old host is heard again before the commit. Without contact
    /// with a hosting successor, the migration is called off: the attempt
    /// closes (or the gate), the session goes on with its host; a log line.
    pub(super) fn host_heard(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        let Some(mg) = c.migrating.as_deref_mut() else { return };
        // In contact it is remembered: should that contact be lost, it is
        // called off then.
        if mg.plan.host_heard() {
            self.call_off();
        }
    }

    /// The migration is called off (N69): back to the old host, which the
    /// session still follows (nothing rehosted).
    fn call_off(&mut self) {
        self.drop_attempt();
        self.close_gate();
        let Side::Client(c) = &mut self.side else { return };
        c.migrating = None;
        if let Some(g) = &mut self.game {
            g.session.unsuspend();
        }
        self.events.push(OnlineEvent::Notice(HOST_BACK.into()));
    }

    /// No successor took over (`GiveUp`, or past `MAX_MIGRATIONS`): the
    /// match is over for this peer. Its replay of the rows it confirmed
    /// (`ReplayReady` later; none for a spectator that joined mid-match),
    /// `HostLost(why)`, and every connection closed.
    pub(super) fn host_lost(&mut self, why: LostWhy) {
        if let Some(g) = self.game.take() {
            self.ended_world = Some(g.session.sim().world().clone());
            self.make_replay(record_of(&g.session, &g.start, g.session.frontier()));
        }
        self.events.push(OnlineEvent::HostLost(why));
        self.shut();
    }

    /// N70, first in every poll: a host whose polls stopped for
    /// `STEP_DOWN_US` during a match ([`stall_gap`]), with an entry of its
    /// succession that can host, steps down.
    pub(super) fn step_down_check(&mut self, now: Instant) {
        let wall = SystemTime::now();
        let Some((then, then_wall)) = self.last_poll.replace((now, wall)) else { return };
        let Side::Host(h) = &self.side else { return };
        if !self.game.as_ref().is_some_and(|g| g.ended.is_none()) {
            return;
        }
        let gap = stall_gap(now.saturating_duration_since(then), wall.duration_since(then_wall).ok());
        if gap < Duration::from_micros(STEP_DOWN_US) || !h.lobby.succession().iter().any(|s| s.can_host) {
            return;
        }
        self.leave_match(LostWhy::SteppedDown { secs: gap.as_secs_f32() }, now);
    }

    /// Rulings SB-2 and FR12c-1, after each advance: a host during a match,
    /// with an entry of its succession that can host, that heard none of
    /// its two or more remote members for `HOST_SILENT_US`
    /// (`Session::members_silent`: its own network is down, so they are
    /// moving to a successor) leaves the match as a stepped-down host does,
    /// `HostLost(CUT_OFF)`, instead of playing on alone. With one remote
    /// member it plays on (a blip is a stall). Members that all left by
    /// `Bye` are not this case.
    pub(super) fn cut_off_check(&mut self, now: Instant) {
        let Side::Host(h) = &self.side else { return };
        let Some(g) = self.game.as_ref().filter(|g| g.ended.is_none()) else { return };
        if g.session.members_silent(self.now_us) && h.lobby.succession().iter().any(|s| s.can_host) {
            self.leave_match(LostWhy::CutOff, now);
        }
    }

    /// The step-down (N70), and a host cut off (SB-2): `Bye` to every peer
    /// (the session's and the lobby's, sent with this poll), each link
    /// closed after `CLOSE_LINGER`, the lobby back to gathering without
    /// them (its results first, as at an end), the replay of what it
    /// confirmed, and `HostLost(why)`. It does not rejoin the match; a
    /// dedicated server's lobby stays open.
    fn leave_match(&mut self, why: LostWhy, now: Instant) {
        let now_us = self.now_us;
        let Some(mut g) = self.game.take() else { return };
        g.session.leave();
        let ep = &mut self.endpoint;
        g.session.transmit(&mut |peer, ch, bytes| send(ep, peer, ch, &bytes));
        self.ended_world = Some(g.session.sim().world().clone());
        let upto = final_upto(&g.session);
        let end = end_wire(g.last.as_ref(), upto.saturating_sub(1));
        let record = record_of(&g.session, &g.start, upto);
        let peers: Vec<PeerId> = self.endpoint.peers().map(|(p, _)| p).collect();
        for &peer in &peers {
            send_tagged(&mut self.endpoint, peer, TAG_TO_CLIENT, &encode_to_client(&ToClient::Bye));
            self.closing.entry(peer).or_insert(now + CLOSE_LINGER);
        }
        if let Side::Host(h) = &mut self.side {
            for &peer in &peers {
                h.lobby.disconnected(peer);
            }
            h.lobby.match_ended(end, now_us);
            // Nobody is left to tell.
            let _ = h.lobby.outgoing();
        }
        self.events.push(OnlineEvent::HostLost(why));
        self.make_replay(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    /// N70, pre-flight C3: the monotonic gap counts; a wall-clock one only
    /// beyond it plus `HOST_SILENT_US` (a suspend); a backward step never.
    /// Final review B-M4: a rejoined client's lobby has no succession until
    /// its new host's next drain; a migration then begins from the one kept
    /// at the rejoin (the old without the new host), and from the lobby's
    /// once it came.
    #[test]
    fn a_rejoined_client_keeps_the_old_succession_until_the_new_one() {
        let entry = |member: u8| SuccessorWire {
            member,
            key: [member; 32],
            direct: None,
            relays: vec![],
            can_host: true,
        };
        let kept = [entry(2), entry(3)];
        assert_eq!(migration_order(&[], &kept), kept);
        assert_eq!(migration_order(&[entry(3)], &kept), [entry(3)]);
        assert!(migration_order(&[], &[]).is_empty());
    }

    #[test]
    fn a_stall_is_the_monotonic_gap_unless_the_machine_slept() {
        assert_eq!(stall_gap(2 * S, Some(2 * S)), 2 * S);
        // An NTP step forward of 1.9 s: not a stall.
        assert_eq!(
            stall_gap(Duration::from_millis(10), Some(Duration::from_millis(1_910))),
            Duration::from_millis(10)
        );
        // A step back: never.
        assert_eq!(stall_gap(Duration::from_millis(10), None), Duration::from_millis(10));
        // A suspend of 30 s whose monotonic clock stood still.
        assert_eq!(stall_gap(Duration::from_millis(5), Some(30 * S)), 30 * S);
        // Just at the bound: the monotonic gap.
        assert_eq!(stall_gap(S, Some(3 * S)), S);
    }
}
