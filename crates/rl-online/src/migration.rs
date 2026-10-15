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
use rl_netcode::migration::{Migration, RETRY_US, Step};
use rl_proto::SuccessorWire;

use crate::join::{Join, NostrJoin, join_relays};

/// A direct join to a successor gives up after this long (N73).
pub const MIGRATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// A direct join to a successor whose answer came but whose link did not
/// open within this is lost contact: a successor that died or froze after
/// answering does not hold its joiners until their setup's timeout (past
/// the give-up), they move on with the others. On a direct link (one
/// network, TCP signalling to the address) the link opens within
/// milliseconds of the answer.
pub const ANSWER_OPEN_TIMEOUT: Duration = Duration::from_secs(2);

/// The same for a join through the relays (plan 12c Task 7): after the
/// answer, ICE checks its candidate pairs (host, server-reflexive, a TURN
/// relay's), whose lost checks are sent again 250 ms, then 0.5, 1 and 2 s
/// later (str0m's backoff), then DTLS and SCTP open over the pair, each a
/// round trip or two of a relayed path. A slow but healthy successor is
/// waited for one window.
pub const RELAYS_OPEN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a join to a successor through `route` may take from the
/// successor's answer to its link's opening ([`ANSWER_OPEN_TIMEOUT`],
/// [`RELAYS_OPEN_TIMEOUT`]).
pub fn answer_open_timeout(route: Route) -> Duration {
    match route {
        Route::Direct => ANSWER_OPEN_TIMEOUT,
        Route::Relays => RELAYS_OPEN_TIMEOUT,
    }
}

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

/// Why a host that stalled `secs` seconds stepped down (N70).
pub fn stepped_down(secs: f32) -> String {
    format!("Your game stopped for {secs:.1} s, so the match moved on without you.")
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

/// The route of a join to a successor (N73): its direct address, or its
/// key through the relays.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
/// - Direct first when the entry has an address (and the test hook does
///   not skip it), with `MIGRATION_CONNECT_TIMEOUT`; a direct route that
///   failed is not tried again in its window.
/// - Then through the relays: the entry's, then this peer's own, cut to
///   [`MAX_JOIN_RELAYS`](crate::join::MAX_JOIN_RELAYS) (`join_relays`),
///   expecting the entry's key. Without relays on either side: direct
///   only.
/// - `NotHost` is asked again `RETRY_US` later; any other failure is not
///   tried again in its window; an entry with no route left waits for the
///   next window.
/// - An answered join whose link does not open within
///   [`answer_open_timeout`] of its route lapses (lost contact).
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

    /// The join through the relays to `entry`: its relays first, then this
    /// peer's own, at most 8; `None` without any.
    fn nostr(&self, entry: &SuccessorWire) -> Option<NostrJoin> {
        let own = self.own.as_ref().map_or(&[][..], |p| p.relays.as_slice());
        let relays = join_relays(&entry.relays, own);
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

    /// The attempt's answer came [`answer_open_timeout`] ago and its link
    /// has not opened: the successor died or froze after answering.
    pub fn lapsed(&self, now_us: u64) -> bool {
        self.attempt.as_ref().is_some_and(|l| {
            let limit = answer_open_timeout(l.route).as_micros() as u64;
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
            (Failure::NotHost, _) => self.retry = Some((l.to, now_us.saturating_add(RETRY_US))),
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
    fn an_answered_join_lapses_in_its_routes_time() {
        let mut c = Joins::default();
        for (e, route) in [(entry(true, &[]), Route::Direct), (entry(false, &["ws://a"]), Route::Relays)] {
            let mut p = planner(e, &[]);
            p.step(0, &mut c);
            p.answered(S);
            assert!(p.in_contact());
            let limit = answer_open_timeout(route).as_micros() as u64;
            assert!(!p.lapsed(S + limit - 1) && p.lapsed(S + limit), "{route:?}");
            p.opened();
            assert!(!p.lapsed(S + 2 * limit), "{route:?}: open");
        }
        assert_eq!(answer_open_timeout(Route::Direct), ANSWER_OPEN_TIMEOUT);
        assert_eq!(answer_open_timeout(Route::Relays), RELAYS_OPEN_TIMEOUT);
        assert!(RELAYS_OPEN_TIMEOUT > ANSWER_OPEN_TIMEOUT);
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
