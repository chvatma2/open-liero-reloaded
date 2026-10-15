//! A host migration over real connections (networking spec §3.7, plan
//! 12c): a client's standby endpoint (N63), its migration from the host's
//! loss (`rl_netcode::migration::Migration`: which successor it tries,
//! whether it hosts, when it gives up), its joins to the successors (direct
//! links, N73), its promotion when its turn comes (the migrated lobby, the
//! take-over, the bots), the new host's collection and resume (N68), the
//! cancel when the old host speaks again (N69), the host lost; and a
//! host's step-down after a stall of its own (N70). Every rule is
//! `rl_netcode`'s and `rl_lobby`'s; this drives them with sockets.

use std::sync::atomic::{AtomicBool, Ordering};

use rl_net::endpoint::IceConfig;
use rl_netcode::migration::{HOST_SILENT_US, MAX_MIGRATIONS, Migration, RETRY_US, STEP_DOWN_US, Step};
use rl_proto::{MAX_MEMBERS, MAX_SPECTATORS, SuccessorWire};

use super::*;
use crate::migration::{
    ANSWER_OPEN_TIMEOUT, HOST_BACK, MIGRATION_CONNECT_TIMEOUT, MigrationStep, REPORT_WAIT,
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
    /// the first rejoin commits it (or its window ends, D8).
    taking: Option<Box<HostSide>>,
    /// Links refused before the commit, closed once their refusal left.
    closing: BTreeMap<PeerId, Instant>,
}

/// One client's migration, from the host's loss until a new host welcomed
/// it (A9), it took over, or the old host was back (N69).
pub(super) struct Migrating {
    m: Migration,
    /// The old host's succession order.
    order: Vec<SuccessorWire>,
    /// The join to a successor under way (one at a time).
    attempt: Option<Box<Attempt>>,
    /// Entries whose join failed: not tried again before this time (their
    /// window's end, N73).
    failed: BTreeMap<usize, u64>,
    /// An entry that refused (`NotHost`: not its turn yet), asked again at
    /// this time (`RETRY_US`).
    retry: Option<(usize, u64)>,
    /// The step last reported.
    reported: Option<MigrationStep>,
    /// The old host was heard while in contact with a successor (N69
    /// ignores it then): should that contact be lost, the migration is
    /// called off, as the successor itself called it off.
    host_heard: bool,
}

/// A join to successor `order[to]`: its own endpoint and lobby client,
/// until its welcome (the commit) or its failure.
struct Attempt {
    to: usize,
    joiner: Joiner,
    out: Outbox,
    lobby: LobbyClient,
    /// The successor answered as a host (N65's contact).
    contact: bool,
    /// How the new link goes, once open.
    connection: Option<(Path, SocketAddr)>,
    /// When the successor answered, and when the link opened.
    answered_at: Option<Instant>,
    opened_at: Option<Instant>,
    /// The lobby's messages wait this long after the link opened (the
    /// test hook `test_delay_rejoin`; zero otherwise), held meanwhile.
    hello_delay: Duration,
    held: Vec<ToHost>,
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
    /// It failed; `retry`: refused `NotHost` (asked again `RETRY_US` later).
    Failed { retry: bool },
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
    if let Some(a) = c.migrating.take().and_then(|mg| mg.attempt) {
        drop_off_thread(a.out);
    }
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
fn my_look(c: &ClientSide, key: &PublicKey) -> Look {
    c.lobby.state().and_then(|s| s.members.iter().find(|m| m.key == *key)).map_or_else(
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
            let attempt = c.migrating.as_mut().and_then(|m| m.attempt.as_mut());
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

    /// The test hook's delay of a rejoin's `Hello` (zero without it).
    fn hello_delay(&self) -> Duration {
        #[cfg(feature = "test-hooks")]
        return self.rejoin_delay;
        #[cfg(not(feature = "test-hooks"))]
        Duration::ZERO
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
                c.standby = Some(Box::new(Standby { out, open, banned, port, taking: None, closing }));
            }
            Err(e) => self.events.push(OnlineEvent::Notice(format!(
                "this game cannot take the match over if the host leaves: {e}"
            ))),
        }
    }

    /// The host is gone during a match (its silence, its `Bye`, its link
    /// closed: N71, N76, A7): the session is suspended where it is and the
    /// migration begins from the host's succession. `true` when one is
    /// under way (begun now or before, or the host lost past
    /// `MAX_MIGRATIONS`); `false` when there is none to begin (migration
    /// off, outside a match, no succession): the caller ends as before.
    pub(super) fn begin_migration(&mut self) -> bool {
        let now_us = self.now_us;
        let my_key = self.my_key;
        let Side::Client(c) = &mut self.side else { return false };
        if c.migrating.is_some() {
            return true;
        }
        let Some(g) = self.game.as_mut().filter(|g| g.ended.is_none()) else { return false };
        if !c.migration.enabled || c.lobby.succession().is_empty() {
            return false;
        }
        if g.migrations >= MAX_MIGRATIONS {
            self.host_lost(HOST_LOST.into());
            return true;
        }
        let order = c.lobby.succession().to_vec();
        let me = order.iter().position(|e| e.key == my_key);
        let mut can_host: Vec<bool> = order.iter().map(|e| e.can_host).collect();
        if let Some(i) = me {
            // Its own turn needs its standby.
            can_host[i] &= c.standby.is_some();
        }
        g.session.suspend();
        c.rejoined = None;
        c.migrating = Some(Box::new(Migrating {
            m: Migration::new(can_host, me, now_us),
            order,
            attempt: None,
            failed: BTreeMap::new(),
            retry: None,
            reported: None,
            host_heard: false,
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
        self.standby_tasks(now);
        self.attempt_tasks(now);
        self.step_migration(now);
        self.resume_check();
    }

    /// The standby, polled every poll: closed, no link stays; on its turn
    /// the migrated lobby takes members back (their proofs, at once), and
    /// the first rejoin commits (N68: [`OnlinePeer::take_over`]).
    fn standby_tasks(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        // A rejoin commits only within this peer's turn: one handled in the
        // poll its window ended is not (the gate closes after it, D8).
        let its_turn = c.migrating.as_mut().is_some_and(|mg| mg.m.step(now_us) == Step::Host);
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
                NetEvent::Disconnected { peer, .. } => {
                    t.lobby.disconnected(peer);
                    t.forget(peer);
                    sb.closing.remove(&peer);
                }
                _ => {}
            }
        }
        t.lobby.tick(now_us);
        // The handshake's answers leave at once (the host's proof, a
        // rejoiner's welcome before anything the commit sends it).
        for (peer, msg) in t.lobby.outgoing() {
            if matches!(msg, ToClient::Challenge { .. }) {
                t.challenged.insert(peer, now);
            }
            send_tagged(&mut sb.out, peer, TAG_TO_CLIENT, &encode_to_client(&msg));
        }
        t.last_drain = Some(now);
        let mut events = t.lobby.events().into_iter();
        while let Some(e) = events.next() {
            match e {
                HostEvent::Rejoined(_) => {
                    // The first rejoin commits: this peer hosts from now
                    // on, and its lobby's events are a host's.
                    self.take_over(now);
                    let rest: Vec<HostEvent> = std::iter::once(e).chain(events).collect();
                    self.on_host_events(rest, now);
                    return;
                }
                HostEvent::Kick { peer, .. } | HostEvent::Close { peer } => {
                    sb.closing.entry(peer).or_insert(now + CLOSE_LINGER);
                }
                _ => {}
            }
        }
    }

    /// The attempt's endpoint and lobby: the host's answer is contact
    /// (N65), its welcome commits ([`OnlinePeer::rejoin`]); a refusal
    /// (`NotHost`: asked again `RETRY_US` later), a closed link or a
    /// refused join fails it (not tried again in its window, N73), and a
    /// contact it had is lost (D8: an uncommitted successor closes at its
    /// window's end).
    fn attempt_tasks(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        let Some(mg) = c.migrating.as_deref_mut() else { return };
        let Some(a) = mg.attempt.as_deref_mut() else { return };
        let mut steps = Vec::new();
        let mut outcome =
            a.joiner.poll(now, &mut a.out, &mut steps).err().map(|_| Outcome::Failed { retry: false });
        let mut net = Vec::new();
        a.out.poll(now, &mut net);
        let mut events = net.into_iter();
        while outcome.is_none()
            && let Some(e) = events.next()
        {
            match e {
                NetEvent::Answered => {
                    a.contact = true;
                    a.answered_at.get_or_insert(now);
                    mg.m.contact(a.to);
                }
                NetEvent::Connected { addr, path, .. } => {
                    let ep = a.out.endpoint();
                    let remote = ep.map_or(addr, |ep| far_end(ep, path, addr));
                    match ep.and_then(|ep| ep.dtls_fingerprints(HOST)) {
                        Some(fps) => {
                            a.lobby.connected(fps);
                            a.connection = Some((path, remote));
                            a.opened_at.get_or_insert(now);
                        }
                        None => outcome = Some(Outcome::Failed { retry: false }),
                    }
                }
                NetEvent::Message { channel: Channel::Reliable, bytes, .. } => {
                    let Some(Tagged::ToClient(b)) = tag(&bytes) else { continue };
                    let Ok(msg) = decode_to_client(b) else {
                        outcome = Some(Outcome::Failed { retry: false });
                        continue;
                    };
                    a.lobby.receive(msg, random_bytes::<32>().unwrap_or([0; 32]));
                    a.flush(now);
                    let lobby_events = a.lobby.events();
                    let welcome = lobby_events.iter().any(|e| matches!(e, ClientEvent::Welcome { .. }));
                    let refused = lobby_events.iter().any(|e| {
                        matches!(
                            e,
                            ClientEvent::Refused(_)
                                | ClientEvent::WrongHost { .. }
                                | ClientEvent::Closed
                                | ClientEvent::Kicked { .. }
                        )
                    });
                    if welcome {
                        outcome = Some(Outcome::Welcome(lobby_events));
                    } else if refused {
                        outcome = Some(Outcome::Failed { retry: false });
                    }
                }
                NetEvent::Refused { why } => {
                    outcome = Some(Outcome::Failed { retry: why == Refusal::NotHost })
                }
                NetEvent::Disconnected { .. } => outcome = Some(Outcome::Failed { retry: false }),
                _ => {}
            }
        }
        if outcome.is_none() {
            a.flush(now);
            // A successor that answered but whose link does not open (it
            // died or froze after its answer): contact lapses, so this peer
            // moves on with the others instead of waiting for its setup's
            // timeout, past the give-up.
            if a.opened_at.is_none() && a.answered_at.is_some_and(|t| now >= t + ANSWER_OPEN_TIMEOUT) {
                outcome = Some(Outcome::Failed { retry: false });
            }
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
            Some(Outcome::Failed { retry }) => {
                let Some(a) = mg.attempt.take() else { return };
                if a.contact {
                    mg.m.lost_contact(a.to);
                }
                if retry {
                    mg.retry = Some((a.to, now_us.saturating_add(RETRY_US)));
                } else {
                    let until = mg.m.window_end(now_us);
                    mg.failed.insert(a.to, until);
                }
                drop_off_thread(a.out);
                // N69: the old host was heard while this contact held it
                // here; the successor that answered has gone back to it (or
                // is gone): so does this peer.
                if a.contact && mg.host_heard {
                    self.call_off();
                }
            }
        }
    }

    /// The migration's step now: give up (host lost), host (the gate open;
    /// committed at once when it expects nobody, N68), try a successor, or
    /// wait. A gate open uncommitted closes once the step is no longer
    /// `Host` (D8).
    fn step_migration(&mut self, now: Instant) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        let Some(mg) = c.migrating.as_deref_mut() else { return };
        let step = mg.m.step(now_us);
        if step != Step::Host {
            self.close_gate();
        }
        match step {
            Step::GiveUp => self.host_lost(HOST_LOST.into()),
            Step::Wait => {
                self.drop_attempt();
                self.report_step(MigrationStep::Finding);
            }
            Step::Host => {
                self.drop_attempt();
                self.report_step(MigrationStep::TakingOver);
                if self.open_gate() {
                    self.take_over(now);
                }
            }
            Step::Try(i) => {
                self.try_successor(i);
                let Side::Client(c) = &self.side else { return };
                let member = c.migrating.as_ref().and_then(|mg| mg.order.get(i)).map(|e| e.member);
                if let Some(member) = member {
                    let name = self.name(member);
                    self.report_step(MigrationStep::Moving(name));
                }
            }
        }
    }

    /// The attempt goes (its endpoint on a thread).
    fn drop_attempt(&mut self) {
        let Side::Client(c) = &mut self.side else { return };
        if let Some(a) = c.migrating.as_mut().and_then(|mg| mg.attempt.take()) {
            drop_off_thread(a.out);
        }
    }

    /// `Try(i)`: an attempt to entry `i` goes on, or one starts unless it
    /// failed in this window or waits for its retry. Direct only (an entry
    /// without a direct address cannot be joined here).
    fn try_successor(&mut self, i: usize) {
        let now_us = self.now_us;
        let outage = self.outage();
        let hello_delay = self.hello_delay();
        let Side::Client(c) = &mut self.side else { return };
        let look = my_look(c, &self.my_key);
        let Some(mg) = c.migrating.as_deref_mut() else { return };
        if mg.attempt.as_ref().is_some_and(|a| a.to != i)
            && let Some(a) = mg.attempt.take()
        {
            drop_off_thread(a.out);
        }
        if mg.attempt.is_some()
            || mg.retry.is_some_and(|(r, at)| r == i && now_us < at)
            || mg.failed.get(&i).is_some_and(|&until| now_us < until)
        {
            return;
        }
        mg.retry = None;
        let Some(entry) = mg.order.get(i) else { return };
        let Some((ip, port)) = entry.direct else {
            let until = mg.m.window_end(now_us);
            mg.failed.insert(i, until);
            return;
        };
        let cfg = EndpointConfig {
            port: 0,
            connect_timeout: MIGRATION_CONNECT_TIMEOUT,
            ice: c.migration.ice.clone(),
            ..c.cfg.clone()
        };
        let join = Join { direct: Some(SocketAddr::from((ip, port))), nostr: None };
        match Joiner::start(join, cfg) {
            Ok((joiner, ep, _)) => {
                let mut lobby = LobbyClient::new(c.me.clone(), look, None, Some(entry.key));
                if let Some(sb) = &c.standby {
                    lobby.standby(sb.port, c.migration.reported_relays());
                }
                let mut out = ep.map_or_else(Outbox::empty, Outbox::new);
                darken(outage, &mut out);
                let a = Attempt {
                    to: i,
                    joiner,
                    out,
                    lobby,
                    contact: false,
                    connection: None,
                    answered_at: None,
                    opened_at: None,
                    hello_delay,
                    held: Vec::new(),
                };
                mg.attempt = Some(Box::new(a));
            }
            Err(_) => {
                let until = mg.m.window_end(now_us);
                mg.failed.insert(i, until);
            }
        }
    }

    /// Its turn (`Step::Host`): the gate opens and the migrated lobby (A13:
    /// the old lobby's state and succession, the match's member ids, the
    /// state's name, its player slots, `MAX_SPECTATORS`, the admins from
    /// the members' marks, no password) waits for the members; with
    /// `migration.relays` it listens on them (Task 7). `true` when it
    /// expects nobody (no member holding a slot or a seat from the start).
    fn open_gate(&mut self) -> bool {
        let Side::Client(c) = &mut self.side else { return false };
        let Some(sb) = c.standby.as_deref_mut() else { return false };
        if let Some(t) = &sb.taking {
            return t.lobby.away_in_match().is_empty();
        }
        let (Some(state), Some(g)) = (c.lobby.state(), self.game.as_ref()) else { return false };
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
            return false;
        };
        let listen = HostListen { pool: c.migration.relays.clone(), public_address: None };
        let Ok(reach) =
            HostReach::start(listen, &c.me).or_else(|_| HostReach::start(HostListen::off(), &c.me))
        else {
            return false;
        };
        let nobody = lobby.away_in_match().is_empty();
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
        nobody
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

    /// The successor commits (N68: its first rejoin, or nobody expected):
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
        let window_end = c.migrating.as_ref().map_or(now_us, |mg| mg.m.window_end(now_us));
        if let Some(a) = c.migrating.take().and_then(|mg| mg.attempt) {
            drop_off_thread(a.out);
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
    fn resume_check(&mut self) {
        let now_us = self.now_us;
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
    /// attempt's is the peer's, its lobby client too; the session rehosts
    /// (suspended until the new host's `Resume`), the content follows the
    /// new host. `Migrated` and `Connected` come at the resume.
    fn rejoin(&mut self) {
        let now_us = self.now_us;
        let Side::Client(c) = &mut self.side else { return };
        let Some(mut mg) = c.migrating.take() else { return };
        let Some(a) = mg.attempt.take() else { return };
        let Attempt { to, out, lobby, connection, .. } = *a;
        let member = mg.order.get(to).map(|e| e.member);
        let name = member.map_or_else(
            || "the new host".into(),
            |id| self.names.get(&id).cloned().unwrap_or_else(|| format!("member {id}")),
        );
        let old = std::mem::replace(&mut self.endpoint, out);
        drop_off_thread(old);
        c.lobby = lobby;
        c.joiner = None;
        c.waiting_since = None;
        c.connection = connection;
        c.expected = true;
        c.rejoined = Some(name);
        self.closing.clear();
        self.content.host_changed();
        if let Some(g) = &mut self.game {
            g.session.rehost(HOST, now_us);
            g.migrations += 1;
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
        if mg.attempt.as_ref().is_some_and(|a| a.contact) {
            // Remembered: should that contact be lost, it is called off then.
            mg.host_heard = true;
            return;
        }
        self.call_off();
    }

    /// The migration is called off (N69): back to the old host, which the
    /// session still follows (nothing rehosted).
    fn call_off(&mut self) {
        let now_us = self.now_us;
        self.drop_attempt();
        self.close_gate();
        let Side::Client(c) = &mut self.side else { return };
        c.migrating = None;
        if let Some(g) = &mut self.game {
            g.session.unsuspend(now_us);
        }
        self.events.push(OnlineEvent::Notice(HOST_BACK.into()));
    }

    /// No successor took over (`GiveUp`, or past `MAX_MIGRATIONS`): the
    /// match is over for this peer. Its replay of the rows it confirmed
    /// (`ReplayReady` later; none for a spectator that joined mid-match),
    /// `HostLost(why)`, and every connection closed.
    pub(super) fn host_lost(&mut self, why: String) {
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
        self.step_down(gap, now);
    }

    /// The step-down (N70): `Bye` to every peer (the session's and the
    /// lobby's, sent with this poll), each link closed after
    /// `CLOSE_LINGER`, the lobby back to gathering without them (its
    /// results first, as at an end), the replay of what it confirmed, and
    /// `HostLost(stepped_down(secs))`. It does not rejoin the match; a
    /// dedicated server's lobby stays open.
    fn step_down(&mut self, gap: Duration, now: Instant) {
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
        self.events.push(OnlineEvent::HostLost(stepped_down(gap.as_secs_f32())));
        self.make_replay(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    /// N70, pre-flight C3: the monotonic gap counts; a wall-clock one only
    /// beyond it plus `HOST_SILENT_US` (a suspend); a backward step never.
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
