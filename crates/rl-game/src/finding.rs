//! Finding games on the Internet from the menus (plan 12a, spec §7.1,
//! decisions N39-N43): the one relay [`Pool`] the game owns while the
//! Multiplayer screen shows or a hosted game is public, the [`GameList`]
//! its announcements add up to, the pings of selected games, and what a
//! hosted game publishes ([`Hosted`]: its invite, its notes, its lobby
//! lines). Plan 12b (N47): a hosted game's address is its reachability's
//! (`rl_online::Reachability::direct`: the router's usable mapping, else
//! the public address setting), never one STUN learnt. Nothing here
//! blocks the caller: the pool and the pings run on threads of their own,
//! the reachability is the peer's.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_content::Catalog;
use rl_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
pub use rl_discovery::hosting::local_relays;
use rl_discovery::list::is_global;
use rl_discovery::{
    Event, GameList, GameSummary, ListedGame, Ping, Pinger, Pool, PoolConfig, PoolEvent, Publisher,
    RelayStatus,
};
use rl_lobby::Identity;
use rl_net::endpoint::TurnServerSpec;
use rl_online::Reachability;
use rl_proto::{FINGERPRINT, PublicKey};
use rl_ui::{
    FindingSettings, InternetEntry, InternetStatus, InternetView, PingView, Region, Visibility,
    parse_key_hex, turn_parts,
};

/// The game list's subscription.
pub const GAMES_SUB: &str = "games";
/// The list drops expired games this often.
const EXPIRE_EVERY: Duration = Duration::from_secs(1);
/// The ids of our own notes remembered (to tell their `OK`s apart).
const OURS: usize = 16;
/// The relays stay connected this long after the game stopped needing them
/// (the Multiplayer screen left): a visit within it reuses them instead of
/// reconnecting to every relay.
pub const RELAY_LINGER: Duration = Duration::from_secs(30);
/// A hosted game's first note and its invite's line wait this long at most
/// for its reachability to settle (`Reachability::settled`: the router
/// answered or gave up, and STUN's first round), as rl-server's do.
pub const SETTLE_WAIT: Duration = Duration::from_secs(5);
/// A Public game's lobby line when the configured public address cannot be
/// reached from the Internet (private, local, or IPv6, which invites and
/// joiners do not use).
pub const UNREACHABLE_ADDRESS: &str =
    "Not listed: your public address setting is not reachable from the Internet";
/// An Invite game's lobby line when its code carries no address (N47: no
/// usable mapping, no public address setting): it joins through the
/// relays (N46).
pub const INVITE_THROUGH_RELAYS: &str = "This code joins through the relays";
/// An Invite game's lobby line when its code has neither an address nor
/// relays.
pub const INVITE_JOINS_NOTHING: &str =
    "This code has no address and no relays: it joins nothing (set relays in Options)";
/// A hosted game's line while its reachability settles.
pub const FINDING_ADDRESS: &str = "Finding the public address…";

/// The relays `settings` name, else the game's defaults.
pub fn relays_of(settings: &FindingSettings) -> Vec<String> {
    settings.relays.clone().unwrap_or_else(|| DEFAULT_RELAYS.iter().map(|r| r.to_string()).collect())
}

/// The STUN servers `settings` name, else the game's defaults.
pub fn stun_of(settings: &FindingSettings) -> Vec<String> {
    settings.stun.clone().unwrap_or_else(|| DEFAULT_STUN.iter().map(|s| s.to_string()).collect())
}

/// The player's own TURN servers (`settings.turn`, `user:pass@host:port`)
/// for a joiner's endpoint (N52: rl-online uses the first 2); entries that
/// are not one are skipped (the settings are sanitized already).
pub fn turn_of(settings: &FindingSettings) -> Vec<TurnServerSpec> {
    settings
        .turn
        .iter()
        .filter_map(|t| turn_parts(t))
        .map(|(user, pass, addr)| TurnServerSpec { addr: addr.into(), user: user.into(), pass: pass.into() })
        .collect()
}

/// The region of the menus as announced.
pub fn region(r: Region) -> rl_discovery::Region {
    match r {
        Region::Europe => rl_discovery::Region::Europe,
        Region::NorthAmericaEast => rl_discovery::Region::NorthAmericaEast,
        Region::NorthAmericaWest => rl_discovery::Region::NorthAmericaWest,
        Region::SouthAmerica => rl_discovery::Region::SouthAmerica,
        Region::Asia => rl_discovery::Region::Asia,
        Region::Oceania => rl_discovery::Region::Oceania,
        Region::Africa => rl_discovery::Region::Africa,
        Region::Unknown => rl_discovery::Region::Unknown,
    }
}

/// An announced region as the menus show it.
pub fn ui_region(r: rl_discovery::Region) -> Region {
    match r {
        rl_discovery::Region::Europe => Region::Europe,
        rl_discovery::Region::NorthAmericaEast => Region::NorthAmericaEast,
        rl_discovery::Region::NorthAmericaWest => Region::NorthAmericaWest,
        rl_discovery::Region::SouthAmerica => Region::SouthAmerica,
        rl_discovery::Region::Asia => Region::Asia,
        rl_discovery::Region::Oceania => Region::Oceania,
        rl_discovery::Region::Africa => Region::Africa,
        rl_discovery::Region::Unknown => Region::Unknown,
    }
}

/// A listed game as the Internet tab shows it (the mod's name from the
/// catalog when this game has it).
pub fn internet_entry(g: &ListedGame, catalog: &Catalog) -> InternetEntry {
    let s = &g.summary;
    let mod_name =
        catalog.mods.iter().find(|m| m.id == s.mod_id).map_or_else(|| s.mod_id.clone(), |m| m.name.clone());
    InternetEntry {
        key: g.key,
        name: s.name.clone(),
        map: s.map_name.clone(),
        mod_name,
        players: s.players,
        max_players: s.max_players,
        ping: match g.ping {
            Ping::Unknown => PingView::Unknown,
            Ping::Measuring => PingView::Measuring,
            Ping::Ms(ms) => PingView::Ms(ms),
            Ping::NoAnswer => PingView::NoAnswer,
        },
        region: ui_region(s.region),
        password: s.password,
        addr: s.addr.map(|ip| SocketAddr::new(ip, s.port)),
        incompatible: (!g.compatible).then(|| g.version_hint.clone()),
        playing: s.playing,
    }
}

/// The relays, the game list and the pings, while the game needs them.
pub struct Internet {
    /// `None` without relays.
    pool: Option<Pool>,
    relays: Vec<String>,
    list: GameList,
    pinger: Pinger,
    /// Subscribed to the game list.
    browsing: bool,
    /// A relay sent its stored games since the subscription.
    eose: bool,
    /// Per relay: connected now; failed (or dropped) since the start.
    connected: Vec<bool>,
    failed: Vec<bool>,
    expired: Option<Instant>,
    /// Our notes' ids, and whether a relay accepted one.
    ours: Vec<[u8; 32]>,
    accepted: bool,
    /// The list or the status changed since the view was made.
    changed: bool,
    events: Vec<PoolEvent>,
}

impl Internet {
    /// Connects to `relays` (none: no pool), hiding `blocked` hosts.
    pub fn open(relays: Vec<String>, blocked: &[String]) -> Internet {
        let mut list = GameList::new(FINGERPRINT);
        list.allow_local(local_relays(&relays));
        for k in blocked.iter().filter_map(|k| parse_key_hex(k)) {
            list.block(k);
        }
        let pool = (!relays.is_empty())
            .then(|| Pool::start(PoolConfig { relays: relays.clone(), ..PoolConfig::default() }));
        let n = relays.len();
        Internet {
            pool,
            relays,
            list,
            pinger: Pinger::new(),
            browsing: false,
            eose: false,
            connected: vec![false; n],
            failed: vec![false; n],
            expired: None,
            ours: Vec::new(),
            accepted: false,
            changed: true,
            events: Vec::new(),
        }
    }

    /// The relays it was opened with.
    pub fn relays(&self) -> &[String] {
        &self.relays
    }

    /// Subscribes to the game list (the Multiplayer screen shows), or ends
    /// the subscription. `now_unix` is the wall-clock time now.
    pub fn browse(&mut self, on: bool, now_unix: u64) {
        if on == self.browsing {
            return;
        }
        self.browsing = on;
        self.eose = false;
        self.changed = true;
        if let Some(pool) = &mut self.pool {
            if on {
                pool.subscribe(GAMES_SUB, vec![GameList::filter_since(now_unix)]);
            } else {
                pool.unsubscribe(GAMES_SUB);
            }
        }
    }

    /// What the relays and the pings reported: games offered to the list,
    /// the list's first `EOSE`, the relays' states, our notes' verdicts.
    /// `now_unix` is the wall-clock time now, read by the caller at each
    /// poll: notes are judged by it (a monotonic clock anchored once falls
    /// behind after a suspend; final review B-M9).
    pub fn poll(&mut self, now: Instant, now_unix: u64) {
        let unix = now_unix;
        if let Some(pool) = &mut self.pool {
            pool.poll(&mut self.events);
        }
        for e in self.events.drain(..) {
            match e {
                PoolEvent::Event { event, .. } if self.browsing => {
                    // Only a note the list took makes the view again (a
                    // flood of refused ones costs no rebuild).
                    self.changed |= self.list.offer(&event, unix);
                }
                PoolEvent::Event { .. } => {}
                PoolEvent::Eose { sub, .. } if sub == GAMES_SUB && self.browsing => {
                    self.changed |= !self.eose;
                    self.eose = true;
                }
                PoolEvent::Eose { .. } => {}
                PoolEvent::Ok { id, accepted, .. } => {
                    if accepted && self.ours.contains(&id) && !self.accepted {
                        self.accepted = true;
                        self.changed = true;
                    }
                }
                PoolEvent::Status { relay, status } => {
                    if let (Some(c), Some(f)) = (self.connected.get_mut(relay), self.failed.get_mut(relay)) {
                        *c = status == RelayStatus::Connected;
                        *f |= matches!(status, RelayStatus::Failed(_) | RelayStatus::Dropped(_));
                        self.changed = true;
                    }
                }
            }
        }
        for (key, ping) in self.pinger.poll() {
            self.list.set_ping(&key, ping);
            self.changed = true;
        }
        if self.expired.is_none_or(|t| now.saturating_duration_since(t) >= EXPIRE_EVERY) {
            self.expired = Some(now);
            let before = self.list.len();
            self.list.expire(unix);
            self.changed |= self.list.len() != before;
        }
    }

    /// Whether a relay is connected now.
    pub fn reachable(&self) -> bool {
        self.connected.iter().any(|c| *c)
    }

    /// The Internet tab's state: no relay reachable (none set, or none
    /// connected and each failed since the start), searching until a
    /// relay sent its stored games, else listed.
    pub fn status(&self) -> InternetStatus {
        let down = self.relays.is_empty() || (!self.reachable() && self.failed.iter().all(|f| *f));
        if down {
            InternetStatus::NoRelay
        } else if !self.eose {
            InternetStatus::Searching
        } else {
            InternetStatus::Listed
        }
    }

    /// The listed games.
    pub fn games(&self) -> Vec<ListedGame> {
        self.list.games()
    }

    /// The Internet tab's view, when the list or the status changed since
    /// the last one.
    pub fn view(&mut self, catalog: &Catalog) -> Option<InternetView> {
        if !std::mem::replace(&mut self.changed, false) {
            return None;
        }
        let games = self.list.games().iter().map(|g| internet_entry(g, catalog)).collect();
        Some(InternetView { status: self.status(), games })
    }

    /// Measures the ping of the listed game of `key` (the player selected
    /// it), on a thread; a game the list did not accept, or without an
    /// address, is not pinged.
    pub fn ping(&mut self, key: PublicKey) {
        let Some(g) = self.list.games().into_iter().find(|g| g.key == key) else { return };
        let Some(ip) = g.summary.addr else { return };
        if self.pinger.ping(key, SocketAddr::new(ip, g.summary.port)) {
            self.list.set_ping(&key, Ping::Measuring);
            self.changed = true;
        }
    }

    /// Hides `key`'s games from now on.
    pub fn block(&mut self, key: PublicKey) {
        self.list.block(key);
        self.changed = true;
    }

    /// Keeps the list's blocked hosts those of the settings (hex keys):
    /// Options' blocks and unblocks apply to the open list at once (an
    /// unblocked host's game shows with its next note; final review B-I2).
    pub fn set_blocked(&mut self, blocked: &[String]) {
        let want: Vec<PublicKey> = blocked.iter().filter_map(|k| parse_key_hex(k)).collect();
        let gone: Vec<PublicKey> =
            self.list.blocked().iter().filter(|k| !want.contains(k)).copied().collect();
        for k in &gone {
            self.list.unblock(k);
        }
        let new: Vec<PublicKey> = want.into_iter().filter(|k| !self.list.blocked().contains(k)).collect();
        for k in &new {
            self.list.block(*k);
        }
        self.changed |= !gone.is_empty() || !new.is_empty();
    }

    /// Forgets which notes were ours and that a relay accepted one: a new
    /// hosted game is not "Listed" on its predecessor's notes (final
    /// review B-M1).
    pub fn forget_ours(&mut self) {
        self.ours.clear();
        self.accepted = false;
    }

    /// Publishes our note on every relay.
    pub fn publish(&mut self, note: Event) {
        if self.ours.len() >= OURS {
            self.ours.remove(0);
        }
        self.ours.push(note.id);
        if let Some(pool) = &mut self.pool {
            pool.publish(note);
        }
    }

    /// A relay accepted one of our notes.
    pub fn accepted(&self) -> bool {
        self.accepted
    }

    /// Lets the relays go: the pool sends what is queued (a closing note)
    /// and closes on a thread of its own (its drop waits up to 2 s), or
    /// here when `wait` (quitting).
    pub fn close(self, wait: bool) {
        let Some(pool) = self.pool else { return };
        if wait {
            drop(pool);
            return;
        }
        let mut pool = Some(pool);
        let spawned = std::thread::Builder::new().name("relays-closing".into()).spawn({
            let pool = pool.take();
            move || drop(pool)
        });
        if spawned.is_err() {
            drop(pool);
        }
    }
}

/// What a hosted game does to be found: its invite (N42; Public and
/// Invite), its notes (N39; Public), and its lobby lines (its listing, its
/// reachability), from the peer's reachability (N47).
pub struct Hosted {
    pub visibility: Visibility,
    /// Signs and times the notes (Public).
    pub publisher: Option<Publisher>,
    pub relays: Vec<String>,
    pub region: rl_discovery::Region,
    /// When hosting began ([`SETTLE_WAIT`] counts from it).
    pub started: Instant,
    /// The reachability as the peer last reported it
    /// (`OnlineEvent::Reachability`, [`Hosted::set_reach`]); `None` before
    /// its first report.
    pub reach: Option<Reachability>,
    /// Its lines ([`Hosted::reach_lines`]), made once a report.
    pub said: Vec<String>,
    /// The lobby's summary as last seen.
    pub summary: Option<GameSummary>,
    /// The invite, listing and reachability lines last shown.
    pub shown: Shown,
    /// The last note published, and the pool (`Shell::relay_pools_opened`)
    /// it went to: a pool opened on edited relays gets it again at once.
    pub last_note: Option<Event>,
    pub pool: u32,
}

/// A hosted game's lobby lines: the invite code, the listing line, the
/// reachability's lines.
pub type Shown = (Option<String>, Option<String>, Vec<String>);

impl Hosted {
    /// A hosted game of `visibility` on the relays `settings` name;
    /// `identity` signs a Public game's notes, the first dated `after` at
    /// the earliest (a closing note of the same key just left:
    /// [`Publisher::new`]).
    pub fn start(
        visibility: Visibility,
        settings: &FindingSettings,
        identity: Identity,
        now: Instant,
        after: Option<u64>,
    ) -> Hosted {
        Hosted {
            visibility,
            publisher: (visibility == Visibility::Public).then(|| Publisher::new(identity, after)),
            relays: relays_of(settings),
            region: region(settings.region),
            started: now,
            reach: None,
            said: Vec::new(),
            summary: None,
            shown: (None, None, Vec::new()),
            last_note: None,
            pool: 0,
        }
    }

    /// Found on the Internet or by its code: a Public or Invite game.
    fn findable(&self) -> bool {
        matches!(self.visibility, Visibility::Public | Visibility::Invite)
    }

    /// N47: the address players join directly (the router's usable
    /// mapping, else the public address setting), if any.
    pub fn direct(&self) -> Option<SocketAddr> {
        self.reach.as_ref().and_then(Reachability::direct).filter(|_| self.findable())
    }

    /// The reachability settled, or [`SETTLE_WAIT`] passed since hosting
    /// began (a slow router: the first note and the invite's line go
    /// without the address, which follows when it comes).
    pub fn settled(&self, now: Instant) -> bool {
        self.reach.as_ref().is_some_and(Reachability::settled)
            || now.saturating_duration_since(self.started) >= SETTLE_WAIT
    }

    /// The invite code (Public and Invite games) of the host `key`: its
    /// relays and its direct address (IPv4), else the local `port`.
    pub fn invite(&self, key: PublicKey, port: u16) -> Option<String> {
        let direct = self.direct();
        let port = direct.map_or(port, |a| a.port());
        self.findable()
            .then(|| rl_discovery::invite(key, &self.relays, direct.map(|a| a.ip()), port).encode())
    }

    /// The address a Public game's notes give: the direct one, IPv4, and
    /// none the Internet cannot reach unless every relay is local too, so a
    /// LAN address never goes to public relays (final review B-M2).
    pub fn announced(&self) -> Option<SocketAddr> {
        self.direct().filter(|a| a.is_ipv4() && (is_global(a.ip()) || local_relays(&self.relays)))
    }

    /// Whether a Public game publishes: not with a direct address the
    /// Internet cannot reach (a public address setting that is private,
    /// local or IPv6), on public relays (its lobby says so, and a note
    /// would put the game on lists it is "Not listed" on).
    pub fn publishes(&self) -> bool {
        !self.unreachable()
    }

    fn unreachable(&self) -> bool {
        !local_relays(&self.relays) && self.direct().is_some_and(|a| a.is_ipv6() || !is_global(a.ip()))
    }

    /// The reachability's lines (`Reachability::lines`: the router's port,
    /// the public address setting, a symmetric NAT, the relays) of a Public
    /// or Invite game; a LAN game has none.
    pub fn reach_lines(&self) -> Vec<String> {
        match &self.reach {
            Some(r) if self.findable() => r.lines(),
            _ => Vec::new(),
        }
    }

    /// The peer reported `r`: kept, its lines made once; the lines not said
    /// before (for the log).
    pub fn set_reach(&mut self, r: Reachability) -> Vec<String> {
        self.reach = Some(r);
        let lines = self.reach_lines();
        let new = lines.iter().filter(|l| !self.said.contains(l)).cloned().collect();
        self.said = lines;
        new
    }

    /// Where a hosted game stands: for a Public game whether it is listed
    /// on the Internet, or why not; for an Invite game how its code joins
    /// (final review B-I3). The line shows beside the invite's Copy: one
    /// copied before the reachability settled may lack the address.
    pub fn listing(&self, internet: Option<&Internet>, now: Instant) -> Option<String> {
        if !self.findable() {
            return None;
        }
        if !self.settled(now) {
            return Some(FINDING_ADDRESS.to_string());
        }
        if self.visibility == Visibility::Invite {
            return if self.direct().is_some_and(|a| a.is_ipv4()) {
                None
            } else if self.relays.is_empty() {
                Some(INVITE_JOINS_NOTHING.to_string())
            } else {
                Some(INVITE_THROUGH_RELAYS.to_string())
            };
        }
        let said = if self.unreachable() {
            UNREACHABLE_ADDRESS
        } else if internet.is_none_or(|i| i.relays().is_empty()) {
            "Not listed: no relays (set them in Options)"
        } else if internet.is_some_and(|i| i.status() == InternetStatus::NoRelay) {
            "Not listed: no relay reachable"
        } else if internet.is_some_and(Internet::accepted) {
            "Listed on the Internet"
        } else {
            "Announcing on the Internet…"
        };
        Some(said.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4};

    use rl_net::endpoint::Reach;
    use rl_net::portmap::{MappingState, Method};
    use rl_net::probe::NatBehaviour;
    use rl_online::Listening;

    use super::*;

    #[test]
    fn regions_convert_both_ways() {
        for r in Region::ALL {
            assert_eq!(ui_region(region(r)), r);
        }
    }

    /// A hosted game's reachability: `mapping` at the router, `configured`
    /// its public address setting, settled.
    fn reached(mapping: MappingState, configured: Option<&str>) -> Reachability {
        Reachability {
            port: 23400,
            configured: configured.map(|a| a.parse().unwrap()),
            reach: Reach { base: None, srflx: None, nat: NatBehaviour::Unknown, mapping, relays: Vec::new() },
            listening: Listening::Off,
            stun_settled: true,
        }
    }

    /// A hosted game of `visibility` on `relays`, reached as `reach`.
    fn hosted(visibility: Visibility, relays: &[&str], reach: Option<Reachability>) -> Hosted {
        let f = FindingSettings {
            relays: Some(relays.iter().map(|r| r.to_string()).collect()),
            ..FindingSettings::default()
        };
        let id = Identity::from_secret([7; 32]).unwrap();
        let mut h = Hosted::start(visibility, &f, id, Instant::now(), None);
        h.reach = reach;
        h
    }

    /// A configured public address that the Internet cannot reach (private,
    /// local, or IPv6, which neither invites nor joiners use) is said in the
    /// lobby, never "Listed on the Internet".
    #[test]
    fn an_unreachable_configured_address_is_said() {
        let line = |addr: &str| {
            let reach = Some(reached(MappingState::Off, Some(addr)));
            hosted(Visibility::Public, &["wss://relay.example"], reach).listing(None, Instant::now()).unwrap()
        };
        for bad in ["192.168.1.5", "127.0.0.1", "2a00:1450::1"] {
            assert_eq!(line(bad), UNREACHABLE_ADDRESS, "{bad}");
        }
        assert_eq!(
            UNREACHABLE_ADDRESS,
            "Not listed: your public address setting is not reachable from the Internet"
        );
        assert!(line("81.2.69.142").starts_with("Not listed: no relays"), "no pool open");
    }

    #[test]
    fn no_relays_is_no_relay_reachable() {
        let i = Internet::open(Vec::new(), &[]);
        assert_eq!(i.status(), InternetStatus::NoRelay);
        assert!(!i.reachable());
    }

    /// The list judges notes by the wall clock the caller reads at each
    /// poll, not by a monotonic clock anchored when the pool opened: after
    /// a suspend (the monotonic clock stood still) current notes are taken
    /// (final review B-M9).
    #[test]
    fn the_list_follows_the_wall_clock_across_a_suspend() {
        let host = Identity::from_secret([9; 32]).unwrap();
        let summary = |name: &str| GameSummary {
            v: rl_proto::PROTOCOL_VERSION,
            name: name.into(),
            mode: "deathmatch".into(),
            map: "olx-level-dirt-level".into(),
            map_name: "Dirt Level".into(),
            mod_id: "olx-classic".into(),
            mod_hash: rl_proto::NO_HASH.into(),
            players: 1,
            max_players: 8,
            spectators: 0,
            max_spectators: 4,
            password: false,
            region: rl_discovery::Region::Europe,
            playing: false,
            addr: Some("81.2.69.142".parse().unwrap()),
            port: 23400,
            closed: false,
        };
        let t0 = Instant::now();
        let mut i = Internet::open(Vec::new(), &[]);
        i.browse(true, 1_000_000);
        // Ten minutes asleep: the monotonic clock moved 60 s, the wall 660 s.
        let note = rl_discovery::announcement(&host, &summary("Den"), FINGERPRINT, 1_000_660, [1; 32]);
        i.events.push(PoolEvent::Event { relay: 0, event: note });
        i.poll(t0 + Duration::from_secs(60), 1_000_660);
        assert_eq!(i.games().len(), 1, "a current note is listed");
    }

    /// Final review B-M1: a pool whose relays all dropped says so even
    /// after one accepted a note, and a new hosted game is not "Listed" on
    /// its predecessor's notes.
    #[test]
    fn listed_is_neither_stale_nor_inherited() {
        let h = hosted(Visibility::Public, &["ws://127.0.0.1:1"], Some(reached(MappingState::Off, None)));
        let mut i = Internet::open(Vec::new(), &[]);
        i.relays = vec!["ws://127.0.0.1:1".into()];
        i.connected = vec![false];
        i.failed = vec![true];
        i.accepted = true;
        let now = Instant::now();
        assert_eq!(h.listing(Some(&i), now).as_deref(), Some("Not listed: no relay reachable"));
        i.failed = vec![false];
        assert_eq!(h.listing(Some(&i), now).as_deref(), Some("Listed on the Internet"));
        i.forget_ours();
        assert_eq!(h.listing(Some(&i), now).as_deref(), Some("Announcing on the Internet…"));
    }

    /// Final review B-M2: a configured address the Internet cannot reach
    /// is never announced on public relays (no note at all: the lobby says
    /// "Not listed"), but is on relays that are all local.
    #[test]
    fn a_private_configured_address_stays_off_public_relays() {
        let private = || Some(reached(MappingState::Off, Some("192.168.1.5")));
        let public = hosted(Visibility::Public, &["wss://relay.example"], private());
        assert!(!public.publishes());
        assert_eq!(public.announced(), None);
        let local = hosted(Visibility::Public, &["ws://192.168.1.2:7000"], private());
        assert!(local.publishes());
        assert_eq!(local.announced(), Some("192.168.1.5:23400".parse().unwrap()));
    }

    /// Plan 12b (N47): the direct address is the router's usable mapping
    /// (its external port), else the public address setting; a mapping
    /// whose address is not public is not one, and STUN never gives one.
    /// Without it no address is announced, and the game is still published.
    #[test]
    fn the_address_comes_from_the_reachability() {
        let external = SocketAddrV4::new(Ipv4Addr::new(81, 2, 69, 142), 23401);
        let mapped = |usable| MappingState::Mapped { method: Method::Upnp, external, tcp: true, usable };
        let relays = ["wss://relay.example"];
        let h = hosted(Visibility::Public, &relays, Some(reached(mapped(true), Some("81.2.69.200"))));
        assert_eq!(h.direct(), Some(external.into()));
        assert_eq!(h.announced(), Some(external.into()));
        let key = [1; 32];
        let code = rl_discovery::Invite::decode(&h.invite(key, 23400).unwrap()).unwrap();
        assert_eq!(code.addr, Some(external.into()), "the external port");
        let h = hosted(Visibility::Public, &relays, Some(reached(mapped(false), None)));
        assert_eq!((h.direct(), h.announced()), (None, None), "a double NAT's mapping");
        assert!(h.publishes(), "listed without an address (N46)");
        let code = rl_discovery::Invite::decode(&h.invite(key, 23400).unwrap()).unwrap();
        assert_eq!(code.addr, None);
        let mut stun = reached(MappingState::Failed("no router".into()), None);
        stun.reach.srflx = Some("81.2.69.7:40000".parse().unwrap());
        let h = hosted(Visibility::Public, &relays, Some(stun));
        assert_eq!(h.direct(), None, "STUN's address is a candidate only");
        // A LAN game has neither an address nor lines.
        let lan = hosted(Visibility::Lan, &relays, Some(reached(mapped(true), None)));
        assert_eq!((lan.direct(), lan.invite(key, 1), lan.listing(None, Instant::now())), (None, None, None));
        assert!(lan.reach_lines().is_empty());
    }

    /// Final review B-I3, plan 12b: an Invite game's line says while the
    /// reachability settles, then how its code joins: nothing once it has
    /// an address, else through the relays, or nothing without them.
    #[test]
    fn an_invite_game_says_how_its_code_joins() {
        let relays = ["wss://relay.example"];
        let now = Instant::now();
        let line = |h: Hosted, at: Instant| h.listing(None, at);
        assert_eq!(line(hosted(Visibility::Invite, &relays, None), now).as_deref(), Some(FINDING_ADDRESS));
        let trying = || hosted(Visibility::Invite, &relays, Some(reached(MappingState::Trying, None)));
        assert_eq!(line(trying(), now).as_deref(), Some(FINDING_ADDRESS));
        assert_eq!(
            line(trying(), now + SETTLE_WAIT + Duration::from_secs(1)).as_deref(),
            Some("This code joins through the relays"),
            "a slow router: said after SETTLE_WAIT"
        );
        let none = || Some(reached(MappingState::Off, None));
        assert_eq!(
            line(hosted(Visibility::Invite, &relays, none()), now).as_deref(),
            Some(INVITE_THROUGH_RELAYS)
        );
        assert_eq!(
            line(hosted(Visibility::Invite, &[], none()), now).as_deref(),
            Some("This code has no address and no relays: it joins nothing (set relays in Options)")
        );
        let v6 = Some(reached(MappingState::Off, Some("2a00:1450::1")));
        assert_eq!(
            line(hosted(Visibility::Invite, &relays, v6), now).as_deref(),
            Some(INVITE_THROUGH_RELAYS)
        );
        let set = Some(reached(MappingState::Off, Some("81.2.69.142")));
        assert_eq!(line(hosted(Visibility::Invite, &relays, set), now), None);
    }

    /// The player's TURN servers for a joiner's endpoint.
    #[test]
    fn turn_entries_become_turn_servers() {
        let f = FindingSettings {
            turn: vec!["anna:p@ss:w@turn.example:3478".into(), "broken".into()],
            ..FindingSettings::default()
        };
        assert_eq!(
            turn_of(&f),
            [TurnServerSpec { addr: "turn.example:3478".into(), user: "anna".into(), pass: "p@ss:w".into() }]
        );
    }

    /// Final review B-M6: a note the list refuses (a flood, a forgery, a
    /// blocked host) does not make the Internet tab's view again.
    #[test]
    fn a_refused_note_changes_no_view() {
        let catalog = Catalog::default();
        let mut i = Internet::open(Vec::new(), &[]);
        i.browse(true, 1_000_000);
        i.poll(Instant::now(), 1_000_000);
        assert!(i.view(&catalog).is_some(), "the first view");
        assert!(i.view(&catalog).is_none(), "nothing changed");
        let host = Identity::from_secret([9; 32]).unwrap();
        let mut forged = rl_discovery::Event::sign(
            &host,
            1_000_000,
            rl_discovery::KIND_ANNOUNCE,
            Vec::new(),
            "{}".into(),
            [1; 32],
        );
        forged.sig[0] ^= 1;
        i.events.push(PoolEvent::Event { relay: 0, event: forged });
        i.poll(Instant::now(), 1_000_000);
        assert!(i.view(&catalog).is_none(), "a refused note changes nothing");
    }
}
