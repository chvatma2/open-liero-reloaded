//! Host migration over real connections (networking spec §3.7, plan 12c):
//! how a client takes part ([`MigrationConfig`]), what it reports while the
//! match moves ([`MigrationStep`]) and the texts of its endings. The
//! drivers are `peer/migrate.rs`'s; every rule (timing, the successor
//! tried, the merge) is `rl_netcode`'s.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use rl_discovery::{PoolConfig, Region};
use rl_net::endpoint::{EndpointConfig, IceConfig};
use rl_netcode::migration::{MAX_MIGRATIONS, Migration, RETRY_US, Step};
use rl_proto::SuccessorWire;

use crate::join::{Join, NostrJoin, join_relays};

/// A direct join to a successor gives up after this long (N73).
pub const MIGRATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// A join to a successor whose answer came but whose link did not open
/// within this is lost contact: a successor that died or froze after
/// answering does not hold its joiners until their setup's timeout (past
/// the give-up), they move on with the others. One for every route
/// (ruling SB-1; Task 7 had 2 s for a direct link): directly or through
/// the relays, after the answer ICE checks its candidate pairs, whose lost
/// checks are sent again 0.25, 0.75, 1.75 and 3.75 s after the first
/// (str0m's backoff), then DTLS and SCTP open over the pair, each a round
/// trip or two, and rl-net reports the link `Connected` once its stats
/// name the pair (up to 1.5 s after it opened). A slow but healthy
/// successor is waited for (a direct link on `bad` opened 3.3 s after its
/// answer in net-soak's `soak-transport-125`); a dead one holds its
/// joiners this long, within the successor's window and the give-up.
pub const ANSWER_OPEN_TIMEOUT: Duration = Duration::from_secs(6);

/// A new host awaits the reports of the members back at most this long
/// past its window's end (N68, C1: one round trip and more); a rejoined
/// member that never reports holds nobody longer, and its report, should
/// it come, is merged late (N67).
pub const REPORT_WAIT: Duration = Duration::from_secs(1);

/// Why a match ends for a peer that found no successor (spec §3.7: the
/// "host lost" screen).
pub const HOST_LOST: &str = "The host left and no other player could take over the game.";

/// The log line of a migration called off: the old host was heard again
/// before anyone took over (N69).
pub const HOST_BACK: &str = "The host is back: the match goes on";

/// Why a host that heard none of its players for `HOST_SILENT_US` during a
/// match left it (ruling SB-2): its own network is down, and its peers,
/// finding it silent at the same time, play on under a successor.
pub const CUT_OFF: &str = "Connection lost: the others play on under a new host.";

/// Why a host that stalled `secs` seconds stepped down (N70).
pub fn stepped_down(secs: f32) -> String {
    format!("Your game stopped for {secs:.1} s, so the match moved on without you.")
}

/// Why a match ended for a peer without it (spec §3.7's "host lost"
/// screen; ruling FR12c-4: each case its own text, so a failed migration
/// on real PCs can be told apart).
#[derive(Clone, Debug, PartialEq)]
pub enum LostWhy {
    /// N74: the match moved hosts [`MAX_MIGRATIONS`] times already.
    TooManyMigrations,
    /// Ruling SK-8: the host went while this spectator was still joining
    /// (no world to follow the match with).
    StillJoining,
    /// No successor took over within `GIVE_UP_US`: the last attempt to
    /// reach one, if any.
    NobodyReachable(Option<Attempted>),
    /// N70: this host's game stopped this long (seconds), so it stepped
    /// down.
    SteppedDown { secs: f32 },
    /// Ruling SB-2: this host heard none of its members (its own network).
    CutOff,
}

impl std::fmt::Display for LostWhy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LostWhy::TooManyMigrations => write!(
                f,
                "The host left, and this match has moved to a new host {MAX_MIGRATIONS} times already: it \
                 cannot move again."
            ),
            LostWhy::StillJoining => {
                write!(f, "The host left while you were still joining the match, so you could not follow it.")
            }
            LostWhy::NobodyReachable(None) => f.write_str(HOST_LOST),
            LostWhy::NobodyReachable(Some(last)) => write!(f, "{HOST_LOST} Last try: {last}."),
            LostWhy::SteppedDown { secs } => f.write_str(&stepped_down(*secs)),
            LostWhy::CutOff => f.write_str(CUT_OFF),
        }
    }
}

/// One attempt to reach a successor and how it ended (ruling FR12c-4):
/// each is logged once (`OnlineEvent::MigrationLog`), the last kept and
/// named by [`LostWhy::NobodyReachable`]. The address reaches only this
/// PC's console and log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempted {
    /// The successor's name.
    pub successor: String,
    /// Directly at this address; `None`: through the relays.
    pub direct: Option<SocketAddr>,
    pub outcome: AttemptOutcome,
}

/// How an attempt to reach a successor ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptOutcome {
    /// Its route did not reach it: why (a connect refused or timed out, no
    /// reply through the relays, a link closed before it opened).
    Unreached(String),
    /// Not taking the match over (`NotHost`): not its turn, or its own
    /// game still hears the host.
    NotHosting,
    /// It refused the join: why.
    Refused(String),
    /// Its link closed after it opened: why.
    Closed(String),
    /// It answered, but the link did not open within this many seconds.
    Lapsed { secs: u64 },
    /// Its window ended (or the give-up came) with the attempt still under
    /// way: unanswered, or `answered` but not taken back.
    TimedOut { answered: bool },
}

impl std::fmt::Display for Attempted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = &self.successor;
        let route = match self.direct {
            Some(addr) => format!("directly at {addr}"),
            None => "through the relays".into(),
        };
        match &self.outcome {
            AttemptOutcome::Unreached(why) => write!(f, "could not reach {name} {route}: {why}"),
            AttemptOutcome::NotHosting => write!(f, "{name} ({route}) was not taking the game over"),
            AttemptOutcome::Refused(why) => write!(f, "{name} ({route}) refused: {why}"),
            AttemptOutcome::Closed(why) => write!(f, "the link to {name} ({route}) closed: {why}"),
            AttemptOutcome::Lapsed { secs } => {
                write!(f, "{name} ({route}) answered, but the link did not open within {secs} s")
            }
            AttemptOutcome::TimedOut { answered: false } => {
                write!(f, "{name} ({route}) did not answer before its turn ended")
            }
            AttemptOutcome::TimedOut { answered: true } => {
                write!(f, "{name} ({route}) answered, but did not take this player back in time")
            }
        }
    }
}

/// How a client takes part in a host migration (spec §3.7).
#[derive(Clone, Debug)]
pub struct MigrationConfig {
    /// Its standby endpoint (N63): `port` is forced to 0 and
    /// `ice.port_mapping` off. `None`: it never hosts (N79).
    pub standby: Option<EndpointConfig>,
    /// The relays it listens on once it hosts, and the first 2 it reports;
    /// `None`: no Nostr (direct joins only).
    pub relays: Option<PoolConfig>,
    /// Its joins to a successor through Nostr: their ICE (STUN, TURN),
    /// region and TURN notes (as a `NostrJoin`'s).
    pub ice: IceConfig,
    pub region: Region,
    pub turn_from_nostr: bool,
    pub allow_local: bool,
    /// Off: the host's loss ends this peer as before (tests that need the
    /// old ending).
    pub enabled: bool,
}

impl MigrationConfig {
    /// No migration (the old ending).
    pub fn off() -> MigrationConfig {
        MigrationConfig {
            standby: None,
            relays: None,
            ice: IceConfig::default(),
            region: Region::Unknown,
            turn_from_nostr: false,
            allow_local: false,
            enabled: false,
        }
    }

    /// A standby on port 0 without STUN, no relays: LAN and loopback
    /// games, tests.
    pub fn direct() -> MigrationConfig {
        MigrationConfig {
            standby: Some(EndpointConfig { port: 0, ..EndpointConfig::default() }),
            enabled: true,
            ..MigrationConfig::off()
        }
    }

    /// The relays it reports with its standby: the first 2 of `relays`.
    pub(crate) fn reported_relays(&self) -> Vec<String> {
        self.relays
            .as_ref()
            .map(|p| p.relays.iter().take(rl_proto::MAX_STANDBY_RELAYS).cloned().collect())
            .unwrap_or_default()
    }
}

/// N64 on the receiving side: a succession's direct address is one the
/// host saw on its own network (not a global address) with a standby port
/// (at least `MIN_STANDBY_PORT`). An honest host lists no other; a hostile
/// one cannot aim its members' connects at an internet address.
#[doc(hidden)]
pub fn n64_direct(ip: [u8; 4], port: u16) -> bool {
    !rl_proto::is_global(std::net::IpAddr::from(ip)) && port >= rl_proto::MIN_STANDBY_PORT
}

/// The route of a join to a successor (N73): its direct address, or its
/// key through the relays.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Route {
    Direct,
    Relays,
}

/// How a join to a successor ended without a welcome.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Refused `NotHost` (not its turn yet): asked again `RETRY_US` later.
    NotHost,
    /// Its route did not reach it: a connect refused or timed out, no
    /// reply through the relays, an answered link that never opened. A
    /// direct route is not tried again in its window (N73): the relays are
    /// next, when there are any.
    Unreached,
    /// Anything else (a refusal, a link closed, another host's key): not
    /// tried again in its window.
    Refused,
}

/// What starts and ends a [`Planner`]'s joins: the real one makes a
/// `Joiner`, its endpoint and lobby client (`peer/migrate.rs`); a test's
/// counts them.
#[doc(hidden)]
pub trait Connector {
    type Attempt;
    /// A join to successor `entry`: `join` is direct only, or through the
    /// relays only. `None`: it could not start (as unreached).
    fn connect(&mut self, entry: &SuccessorWire, join: Join) -> Option<Self::Attempt>;
    /// An attempt given up (the real one drops its endpoint on a thread).
    fn discard(&mut self, attempt: Self::Attempt);
}

/// The join under way.
struct Live<A> {
    to: usize,
    route: Route,
    attempt: A,
    /// When the successor answered (N65's contact).
    answered_us: Option<u64>,
    opened: bool,
}

/// A client's migration, its joins planned (N65, N73; Review Focus 3): which
/// successor it tries (`rl_netcode`'s [`Migration`]), by which route, and
/// when not.
/// - At most one attempt at a time; a step moving to another entry, to its
///   own turn or past the windows discards it; none starts past the
///   give-up.
/// - Direct first when the entry has an address N64 allows ([`n64_direct`]:
///   not global, a port of at least 1024; loopback only from the host's own
///   PC, B-M1; and the test hook does not skip it), with
///   `MIGRATION_CONNECT_TIMEOUT`; a direct route that failed is not tried
///   again in its window.
/// - Then through the relays: the entry's (not local ones unless
///   `allow_local`, B-M3), then this peer's own, cut to
///   [`MAX_JOIN_RELAYS`](crate::join::MAX_JOIN_RELAYS) (`join_relays`),
///   expecting the entry's key. Without relays on either side: direct
///   only.
/// - `NotHost` directly is asked again `RETRY_US` later (not its turn
///   yet); through the relays, and any other failure, is not tried again
///   in its window; an entry with no route left waits for the next window.
/// - An answered join whose link does not open within
///   [`ANSWER_OPEN_TIMEOUT`] lapses (lost contact).
#[doc(hidden)]
pub struct Planner<A> {
    m: Migration,
    order: Vec<SuccessorWire>,
    /// This peer's own relays and its joins' Nostr settings.
    own: Option<PoolConfig>,
    region: Region,
    turn_from_nostr: bool,
    allow_local: bool,
    ignore_direct: bool,
    /// A loopback direct address is tried ([`Planner::loopback_host`]).
    loopback_ok: bool,
    attempt: Option<Live<A>>,
    /// Entries not tried again before this time (their window's end).
    failed: BTreeMap<usize, u64>,
    /// Entries whose direct route is not tried again before this time.
    direct_failed: BTreeMap<usize, u64>,
    /// An entry that refused `NotHost`, asked again at this time.
    retry: Option<(usize, u64)>,
    /// The old host was heard while in contact with a successor (N69
    /// ignores it then): should that contact be lost, the migration is
    /// called off, as the successor itself called it off.
    host_heard: bool,
}

impl<A> Planner<A> {
    /// A migration begun at `now_us` over the old host's `order`;
    /// `can_host[i]` as the driver decides it, `me` this peer's entry; the
    /// relays and Nostr settings from `cfg`.
    pub fn new(
        order: Vec<SuccessorWire>,
        can_host: Vec<bool>,
        me: Option<usize>,
        cfg: &MigrationConfig,
        now_us: u64,
    ) -> Planner<A> {
        Planner {
            m: Migration::new(can_host, me, now_us),
            order,
            own: cfg.relays.clone(),
            region: cfg.region,
            turn_from_nostr: cfg.turn_from_nostr,
            allow_local: cfg.allow_local,
            ignore_direct: false,
            loopback_ok: false,
            attempt: None,
            failed: BTreeMap::new(),
            direct_failed: BTreeMap::new(),
            retry: None,
            host_heard: false,
        }
    }

    /// Joins skip direct addresses (a test hook: a loopback test then
    /// exercises the relays).
    pub fn ignore_direct(&mut self) {
        self.ignore_direct = true;
    }

    /// This peer's own link to the host is loopback (it runs on the host's
    /// PC): only then is a loopback direct address tried (final review
    /// B-M1: the host lists a member on its own PC at 127.0.0.1, which
    /// from another PC is that PC itself).
    pub fn loopback_host(&mut self, same_pc: bool) {
        self.loopback_ok = same_pc;
    }

    /// The old host's succession order.
    pub fn order(&self) -> &[SuccessorWire] {
        &self.order
    }

    /// When the current window ends.
    pub fn window_end(&self, now_us: u64) -> u64 {
        self.m.window_end(now_us)
    }

    /// The step at `now_us`, nothing started or discarded.
    pub fn peek(&mut self, now_us: u64) -> Step {
        self.m.step(now_us)
    }

    /// The step at `now_us`: an attempt to `Try`'s entry goes on, or one
    /// starts (unless it waits for a retry, failed in this window, or has
    /// no route); any other attempt is discarded.
    pub fn step(&mut self, now_us: u64, c: &mut impl Connector<Attempt = A>) -> Step {
        let step = self.m.step(now_us);
        let keep = match step {
            Step::Try(i) => Some(i),
            Step::Host | Step::Wait | Step::GiveUp => None,
        };
        if self.attempt.as_ref().is_some_and(|l| Some(l.to) != keep)
            && let Some(l) = self.attempt.take()
        {
            c.discard(l.attempt);
        }
        if let Some(i) = keep {
            self.start(i, now_us, c);
        }
        step
    }

    fn start(&mut self, i: usize, now_us: u64, c: &mut impl Connector<Attempt = A>) {
        let until = |m: &BTreeMap<usize, u64>| m.get(&i).is_some_and(|&t| now_us < t);
        if self.attempt.is_some()
            || self.retry.is_some_and(|(r, at)| r == i && now_us < at)
            || until(&self.failed)
        {
            return;
        }
        self.retry = None;
        let Some(entry) = self.order.get(i) else { return };
        let window_end = self.m.window_end(now_us);
        let direct = entry
            .direct
            .filter(|&(ip, port)| n64_direct(ip, port))
            .filter(|&(ip, _)| self.loopback_ok || !std::net::Ipv4Addr::from(ip).is_loopback())
            .filter(|_| !self.ignore_direct && !until(&self.direct_failed))
            .map(|(ip, port)| SocketAddr::from((ip, port)));
        let (route, join) = match (direct, self.nostr(entry)) {
            (Some(addr), _) => (Route::Direct, Join::direct(addr)),
            (None, Some(nostr)) => (Route::Relays, Join { direct: None, nostr: Some(nostr) }),
            (None, None) => {
                self.failed.insert(i, window_end);
                return;
            }
        };
        match c.connect(entry, join) {
            Some(attempt) => {
                self.attempt = Some(Live { to: i, route, attempt, answered_us: None, opened: false })
            }
            None if route == Route::Direct => {
                self.direct_failed.insert(i, window_end);
            }
            None => {
                self.failed.insert(i, window_end);
            }
        }
    }

    /// The join through the relays to `entry`: its relays first (but a
    /// local or private one unless `allow_local`: final review B-M3, a
    /// hostile member cannot aim every peer's WebSocket at a LAN host),
    /// then this peer's own, at most 8; `None` without any.
    fn nostr(&self, entry: &SuccessorWire) -> Option<NostrJoin> {
        let own = self.own.as_ref().map_or(&[][..], |p| p.relays.as_slice());
        let theirs: Vec<String> = entry
            .relays
            .iter()
            .filter(|r| self.allow_local || !rl_discovery::hosting::local_relays(std::slice::from_ref(r)))
            .cloned()
            .collect();
        let relays = join_relays(&theirs, own);
        if relays.is_empty() {
            return None;
        }
        let pool = PoolConfig { relays, ..self.own.clone().unwrap_or_default() };
        Some(NostrJoin {
            host: entry.key,
            pool,
            region: self.region,
            turn_from_nostr: self.turn_from_nostr,
            allow_local: self.allow_local,
        })
    }

    /// The attempt under way: its entry and route.
    pub fn attempt(&self) -> Option<(usize, Route)> {
        self.attempt.as_ref().map(|l| (l.to, l.route))
    }

    pub fn attempt_mut(&mut self) -> Option<&mut A> {
        self.attempt.as_mut().map(|l| &mut l.attempt)
    }

    /// The welcomed attempt, for the driver to keep.
    pub fn take_attempt(&mut self) -> Option<A> {
        self.attempt.take().map(|l| l.attempt)
    }

    /// The attempt's successor answered as a host (contact, N65).
    pub fn answered(&mut self, now_us: u64) {
        if let Some(l) = &mut self.attempt {
            l.answered_us.get_or_insert(now_us);
            self.m.contact(l.to);
        }
    }

    /// The attempt's link opened.
    pub fn opened(&mut self) {
        if let Some(l) = &mut self.attempt {
            l.opened = true;
        }
    }

    /// The attempt has contact.
    pub fn in_contact(&self) -> bool {
        self.attempt.as_ref().is_some_and(|l| l.answered_us.is_some())
    }

    /// The attempt's answer came [`ANSWER_OPEN_TIMEOUT`] ago and its link
    /// has not opened: the successor died or froze after answering.
    pub fn lapsed(&self, now_us: u64) -> bool {
        self.attempt.as_ref().is_some_and(|l| {
            let limit = ANSWER_OPEN_TIMEOUT.as_micros() as u64;
            !l.opened && l.answered_us.is_some_and(|t| now_us >= t.saturating_add(limit))
        })
    }

    /// The attempt failed (`why`): discarded, its contact lost; `NotHost`
    /// asked again `RETRY_US` later, an unreached direct route not tried
    /// again in its window (the relays next), anything else not that entry
    /// in its window. `true`: call the migration off (N69: the old host
    /// was heard while this contact held it).
    pub fn failed(&mut self, now_us: u64, why: Failure, c: &mut impl Connector<Attempt = A>) -> bool {
        let Some(l) = self.attempt.take() else { return false };
        c.discard(l.attempt);
        let contact = l.answered_us.is_some();
        if contact {
            self.m.lost_contact(l.to);
        }
        let until = self.m.window_end(now_us);
        match (why, l.route) {
            // A successor listens on its relays only in its turn: `NotHost`
            // through them is no honest answer (a hostile key's), so not
            // asked again in the window.
            (Failure::NotHost, Route::Direct) => self.retry = Some((l.to, now_us.saturating_add(RETRY_US))),
            (Failure::Unreached, Route::Direct) => {
                self.direct_failed.insert(l.to, until);
            }
            _ => {
                self.failed.insert(l.to, until);
            }
        }
        contact && self.host_heard
    }

    /// N69: the old host was heard. `true`: call the migration off now (no
    /// contact); in contact it is remembered ([`Planner::failed`]).
    pub fn host_heard(&mut self) -> bool {
        if self.in_contact() {
            self.host_heard = true;
            return false;
        }
        true
    }
}

/// Where a client's migration stands, for its HUD (N81).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationStep {
    /// The host is gone: no successor's turn yet, or none answering.
    Finding,
    /// Joining the successor, by its name.
    Moving(String),
    /// This peer's turn: it takes the match over.
    TakingOver,
}

impl std::fmt::Display for MigrationStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationStep::Finding => write!(f, "The host is gone: finding who takes over…"),
            MigrationStep::Moving(name) => write!(f, "The host is gone: moving the game to {name}…"),
            MigrationStep::TakingOver => write!(f, "The host is gone: you are taking over the game…"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texts() {
        assert_eq!(MigrationStep::Finding.to_string(), "The host is gone: finding who takes over…");
        assert_eq!(
            MigrationStep::Moving("Anna".into()).to_string(),
            "The host is gone: moving the game to Anna…"
        );
        assert_eq!(MigrationStep::TakingOver.to_string(), "The host is gone: you are taking over the game…");
        assert_eq!(stepped_down(1.73), "Your game stopped for 1.7 s, so the match moved on without you.");
        assert_eq!(HOST_LOST, "The host left and no other player could take over the game.");
    }

    /// Ruling FR12c-4: each host lost and each attempt has its own text.
    #[test]
    fn the_host_lost_texts_tell_the_cases_apart() {
        let at: SocketAddr = "192.168.1.5:40123".parse().unwrap();
        let attempt = |direct, outcome| Attempted { successor: "Anna".into(), direct, outcome };
        let texts = [
            (
                attempt(Some(at), AttemptOutcome::Unreached("no answer".into())),
                "could not reach Anna directly at 192.168.1.5:40123: no answer",
            ),
            (
                attempt(None, AttemptOutcome::Unreached("no reply".into())),
                "could not reach Anna through the relays: no reply",
            ),
            (
                attempt(Some(at), AttemptOutcome::NotHosting),
                "Anna (directly at 192.168.1.5:40123) was not taking the game over",
            ),
            (
                attempt(None, AttemptOutcome::Refused("full".into())),
                "Anna (through the relays) refused: full",
            ),
            (
                attempt(Some(at), AttemptOutcome::Closed("it left".into())),
                "the link to Anna (directly at 192.168.1.5:40123) closed: it left",
            ),
            (
                attempt(None, AttemptOutcome::Lapsed { secs: 6 }),
                "Anna (through the relays) answered, but the link did not open within 6 s",
            ),
            (
                attempt(Some(at), AttemptOutcome::TimedOut { answered: false }),
                "Anna (directly at 192.168.1.5:40123) did not answer before its turn ended",
            ),
            (
                attempt(None, AttemptOutcome::TimedOut { answered: true }),
                "Anna (through the relays) answered, but did not take this player back in time",
            ),
        ];
        for (a, text) in &texts {
            assert_eq!(a.to_string(), *text);
        }
        let lost = [
            LostWhy::TooManyMigrations,
            LostWhy::StillJoining,
            LostWhy::NobodyReachable(None),
            LostWhy::NobodyReachable(Some(texts[0].0.clone())),
            LostWhy::SteppedDown { secs: 1.73 },
            LostWhy::CutOff,
        ]
        .map(|w| w.to_string());
        assert_eq!(
            lost,
            [
                "The host left, and this match has moved to a new host 4 times already: it cannot move again.",
                "The host left while you were still joining the match, so you could not follow it.",
                HOST_LOST,
                "The host left and no other player could take over the game. Last try: could not reach Anna \
                 directly at 192.168.1.5:40123: no answer.",
                "Your game stopped for 1.7 s, so the match moved on without you.",
                CUT_OFF,
            ]
        );
    }

    const S: u64 = 1_000_000;

    /// Every join started: direct, or the relays it asks.
    #[derive(Default)]
    struct Joins(Vec<(Option<SocketAddr>, Vec<String>)>);

    impl Connector for Joins {
        type Attempt = ();

        fn connect(&mut self, _: &SuccessorWire, join: Join) -> Option<()> {
            self.0.push((join.direct, join.nostr.map(|n| n.pool.relays).unwrap_or_default()));
            Some(())
        }

        fn discard(&mut self, _: ()) {}
    }

    fn entry(direct: bool, relays: &[&str]) -> SuccessorWire {
        SuccessorWire {
            member: 1,
            key: [9; 32],
            direct: direct.then_some(([192, 168, 1, 5], 4000)),
            relays: relays.iter().map(|r| r.to_string()).collect(),
            can_host: true,
        }
    }

    fn planner(e: SuccessorWire, own: &[&str]) -> Planner<()> {
        let relays = (!own.is_empty()).then(|| PoolConfig {
            relays: own.iter().map(|r| r.to_string()).collect(),
            ..Default::default()
        });
        Planner::new(vec![e], vec![true], None, &MigrationConfig { relays, ..MigrationConfig::direct() }, 0)
    }

    #[test]
    fn a_join_goes_direct_then_through_the_relays() {
        let mut p = planner(entry(true, &["ws://a"]), &["ws://own", "ws://a/"]);
        let mut c = Joins::default();
        assert_eq!(p.step(0, &mut c), Step::Try(0));
        assert_eq!(p.attempt(), Some((0, Route::Direct)));
        assert!(!p.failed(S, Failure::Unreached, &mut c));
        p.step(S, &mut c);
        assert_eq!(p.attempt(), Some((0, Route::Relays)));
        // The entry's relays first, then its own, each once.
        assert_eq!(c.0[1], (None, vec!["ws://a".to_string(), "ws://own".to_string()]));
        p.failed(2 * S, Failure::Unreached, &mut c);
        p.step(2 * S, &mut c);
        assert_eq!(c.0.len(), 2, "neither route again in its window");
        p.step(5 * S, &mut c);
        assert_eq!(c.0.len(), 2, "its window is over: nobody's turn");
    }

    #[test]
    fn without_relays_on_either_side_a_join_is_direct_only() {
        let mut p = planner(entry(true, &[]), &[]);
        let mut c = Joins::default();
        p.step(0, &mut c);
        p.failed(1, Failure::Unreached, &mut c);
        p.step(2, &mut c);
        assert_eq!(c.0.len(), 1);
        assert!(p.attempt().is_none());
        // Without an address: the relays alone; without either, nothing.
        let mut p = planner(entry(false, &["ws://a"]), &[]);
        p.step(0, &mut c);
        assert_eq!(p.attempt(), Some((0, Route::Relays)));
        let mut p = planner(entry(false, &[]), &[]);
        p.step(0, &mut c);
        assert!(p.attempt().is_none());
        assert_eq!(c.0.len(), 2);
    }

    #[test]
    fn a_direct_address_n64_forbids_is_never_connected() {
        let mut c = Joins::default();
        for (ip, port) in [([93, 184, 216, 34], 4000), ([8, 8, 8, 8], 4000), ([192, 168, 1, 5], 1023)] {
            let mut e = entry(false, &["ws://a"]);
            e.direct = Some((ip, port));
            let mut p = planner(e, &[]);
            p.step(0, &mut c);
            assert_eq!(p.attempt(), Some((0, Route::Relays)), "{ip:?}:{port}: the relays alone");
        }
        assert!(c.0.iter().all(|(direct, _)| direct.is_none()));
        assert!(n64_direct([192, 168, 1, 5], 1024) && n64_direct([10, 0, 0, 1], 4000));
        assert!(n64_direct([127, 0, 0, 1], 4000), "loopback is no global address");
        assert!(!n64_direct([1, 1, 1, 1], 4000) && !n64_direct([10, 0, 0, 1], 80));
    }

    /// Final review B-M1: a member the host saw on its own PC (127.0.0.1)
    /// is listed at loopback; another PC's connect there reaches itself.
    /// Its direct address is tried only by a peer whose own link to the
    /// host is loopback (the same PC); the others go through the relays.
    #[test]
    fn a_loopback_address_is_tried_only_from_the_same_pc() {
        let mut c = Joins::default();
        for (same_pc, route) in [(false, Route::Relays), (true, Route::Direct)] {
            let mut e = entry(false, &["ws://a"]);
            e.direct = Some(([127, 0, 0, 1], 4000));
            let mut p = planner(e, &[]);
            p.loopback_host(same_pc);
            p.step(0, &mut c);
            assert_eq!(p.attempt(), Some((0, route)), "same PC: {same_pc}");
        }
    }

    /// Final review B-M3: a member's standby relays naming a local or
    /// private address (or `localhost`) are not asked (a hostile member
    /// would aim every peer's WebSocket at a LAN host), unless local
    /// relays are allowed (`allow_local`: a LAN game's own relay).
    #[test]
    fn local_standby_relays_are_asked_only_when_allowed() {
        let local = ["ws://192.168.1.1/", "ws://10.0.0.5:22", "ws://localhost:7777", "wss://127.0.0.1"];
        let mut c = Joins::default();
        let mut e = entry(false, &local);
        e.relays.push("wss://relay.example".into());
        let mut p = planner(e.clone(), &["wss://own.example"]);
        p.step(0, &mut c);
        assert_eq!(c.0[0].1, ["wss://relay.example", "wss://own.example"]);
        let cfg = MigrationConfig {
            relays: Some(PoolConfig { relays: vec!["wss://own.example".into()], ..Default::default() }),
            allow_local: true,
            ..MigrationConfig::direct()
        };
        let mut p = Planner::new(vec![e], vec![true], None, &cfg, 0);
        p.step(0, &mut c);
        assert_eq!(c.0[1].1.len(), 6, "allowed: {:?}", c.0[1].1);
    }

    #[test]
    fn not_host_through_the_relays_is_not_asked_again() {
        let mut p = planner(entry(false, &["ws://a"]), &[]);
        let mut c = Joins::default();
        p.step(0, &mut c);
        p.failed(S, Failure::NotHost, &mut c);
        p.step(S + 2 * RETRY_US, &mut c);
        p.step(4 * S, &mut c);
        assert_eq!(c.0.len(), 1, "once in its window");
    }

    #[test]
    fn not_host_is_asked_again_after_its_retry() {
        let mut p = planner(entry(true, &[]), &[]);
        let mut c = Joins::default();
        p.step(0, &mut c);
        p.failed(S, Failure::NotHost, &mut c);
        p.step(S + RETRY_US - 1, &mut c);
        assert_eq!(c.0.len(), 1);
        p.step(S + RETRY_US, &mut c);
        assert_eq!(c.0.len(), 2, "asked again, directly");
        assert_eq!(p.attempt(), Some((0, Route::Direct)));
    }

    #[test]
    fn an_answered_join_lapses_alike_on_every_route() {
        let mut c = Joins::default();
        for (e, route) in [(entry(true, &[]), Route::Direct), (entry(false, &["ws://a"]), Route::Relays)] {
            let mut p = planner(e, &[]);
            p.step(0, &mut c);
            p.answered(S);
            assert!(p.in_contact());
            let limit = ANSWER_OPEN_TIMEOUT.as_micros() as u64;
            assert!(!p.lapsed(S + limit - 1) && p.lapsed(S + limit), "{route:?}");
            p.opened();
            assert!(!p.lapsed(S + 2 * limit), "{route:?}: open");
        }
        assert_eq!(ANSWER_OPEN_TIMEOUT, Duration::from_secs(6), "ruling SB-1");
    }

    #[test]
    fn the_old_host_heard_in_contact_calls_off_once_it_is_lost() {
        let mut p = planner(entry(true, &[]), &[]);
        let mut c = Joins::default();
        assert!(p.host_heard(), "no contact: called off at once");
        p.step(0, &mut c);
        p.answered(1);
        assert!(!p.host_heard(), "in contact: remembered");
        assert!(p.failed(2, Failure::Refused, &mut c), "the contact lost: called off");
    }

    #[test]
    fn off_and_direct() {
        let off = MigrationConfig::off();
        assert!(!off.enabled && off.standby.is_none() && off.relays.is_none());
        let direct = MigrationConfig::direct();
        assert!(direct.enabled && direct.relays.is_none());
        let standby = direct.standby.unwrap();
        assert_eq!(standby.port, 0);
        assert!(standby.ice.stun.is_empty() && !standby.ice.port_mapping);
        let relays = rl_discovery::PoolConfig {
            relays: vec!["ws://a".into(), "ws://b".into(), "ws://c".into()],
            ..Default::default()
        };
        let cfg = MigrationConfig { relays: Some(relays), ..MigrationConfig::direct() };
        assert_eq!(cfg.reported_relays(), ["ws://a", "ws://b"]);
    }
}
