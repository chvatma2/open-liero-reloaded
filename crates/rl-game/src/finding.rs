//! Finding games on the Internet from the menus (plan 12a, spec §7.1,
//! decisions N39-N43): the one relay [`Pool`] the game owns while the
//! Multiplayer screen shows or a hosted game is public, the [`GameList`]
//! its announcements add up to, the pings of selected games, and what a
//! hosted game publishes ([`Hosted`]: its public address, its invite, its
//! notes). Nothing here blocks the caller: the pool, the pings and STUN
//! run on threads of their own.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use rl_content::Catalog;
use rl_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
use rl_discovery::list::is_global;
use rl_discovery::{
    AddressWatch, Event, GameList, GameSummary, ListedGame, Ping, Pinger, Pool, PoolConfig, PoolEvent,
    Publisher, RelayStatus,
};
use rl_lobby::Identity;
use rl_proto::{FINGERPRINT, PublicKey};
use rl_ui::{
    FindingSettings, InternetEntry, InternetStatus, InternetView, PingView, Region, Visibility, parse_key_hex,
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
/// A Public game's lobby line when the configured public address cannot be
/// reached from the Internet (private, local, or IPv6, which invites and
/// joiners do not use).
pub const UNREACHABLE_ADDRESS: &str =
    "Not listed: your public address setting is not reachable from the Internet";

/// The relays `settings` name, else the game's defaults.
pub fn relays_of(settings: &FindingSettings) -> Vec<String> {
    settings.relays.clone().unwrap_or_else(|| DEFAULT_RELAYS.iter().map(|r| r.to_string()).collect())
}

/// The STUN servers `settings` name, else the game's defaults.
pub fn stun_of(settings: &FindingSettings) -> Vec<String> {
    settings.stun.clone().unwrap_or_else(|| DEFAULT_STUN.iter().map(|s| s.to_string()).collect())
}

/// Whether every relay is on this machine or a private network (a relay
/// of one's own, or the tests'): the list then takes games announcing
/// local addresses too (ruling PF12-5's `allow_local`). A public relay
/// among them keeps the list to public addresses.
pub fn local_relays(relays: &[String]) -> bool {
    let local = |url: &str| {
        let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://")).unwrap_or(url);
        let authority = rest.split('/').next().unwrap_or("");
        let host = match authority.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or(""),
            None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
        };
        host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().is_ok_and(|ip| !is_global(ip))
    };
    !relays.is_empty() && relays.iter().all(|r| local(r))
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
                    self.list.offer(&event, unix);
                    self.changed = true;
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

/// What a hosted game does to be found: its public address (N40, Public
/// and Invite), its invite (N42), and for a Public game its notes (N39).
pub struct Hosted {
    pub visibility: Visibility,
    pub address: AddressWatch,
    /// Signs and times the notes (Public).
    pub publisher: Option<Publisher>,
    pub relays: Vec<String>,
    pub region: rl_discovery::Region,
    /// The address is the settings' `public_address`, not STUN's.
    pub configured: bool,
    /// The lobby's summary as last seen.
    pub summary: Option<GameSummary>,
    /// The invite and listing lines last shown.
    pub shown: (Option<String>, Option<String>),
}

impl Hosted {
    /// A hosted game of `visibility` (the address asked for a Public or
    /// Invite game only); `identity` signs a Public game's notes, the first
    /// dated `after` at the earliest (a closing note of the same key just
    /// left: [`Publisher::new`]). The line saying where the address comes
    /// from, if one is asked.
    pub fn start(
        visibility: Visibility,
        settings: &FindingSettings,
        identity: Identity,
        now: Instant,
        after: Option<u64>,
    ) -> (Hosted, Option<String>) {
        let configured = settings.public_address.trim().parse::<IpAddr>().ok();
        let asks = matches!(visibility, Visibility::Public | Visibility::Invite);
        let stun = if asks { stun_of(settings) } else { Vec::new() };
        let configured = configured.filter(|_| asks);
        let (address, said) = AddressWatch::start(configured, &stun, now);
        let publisher = (visibility == Visibility::Public).then(|| Publisher::new(identity, after));
        let hosted = Hosted {
            visibility,
            address,
            publisher,
            relays: relays_of(settings),
            region: region(settings.region),
            configured: configured.is_some(),
            summary: None,
            shown: (None, None),
        };
        (hosted, asks.then_some(said))
    }

    /// The invite code (Public and Invite games) of the host `key` on
    /// `port`.
    pub fn invite(&self, key: PublicKey, port: u16) -> Option<String> {
        matches!(self.visibility, Visibility::Public | Visibility::Invite)
            .then(|| rl_discovery::invite(key, &self.relays, self.address.current(), port).encode())
    }

    /// Whether a Public game is listed on the Internet, or why not.
    pub fn listing(&self, internet: Option<&Internet>, port: u16) -> Option<String> {
        if self.visibility != Visibility::Public {
            return None;
        }
        let said = if !self.address.settled() {
            "Finding the public address…".to_string()
        } else if self.address.current().is_none() {
            format!("Not listed: no public address (forward port {port} or set it in Options)")
        } else if self.configured && self.address.current().is_some_and(|ip| ip.is_ipv6() || !is_global(ip)) {
            UNREACHABLE_ADDRESS.to_string()
        } else if internet.is_none_or(|i| i.relays().is_empty()) {
            "Not listed: no relays (set them in Options)".to_string()
        } else if internet.is_some_and(Internet::accepted) {
            "Listed on the Internet".to_string()
        } else if internet.is_some_and(|i| i.status() == InternetStatus::NoRelay) {
            "Not listed: no relay reachable".to_string()
        } else {
            "Announcing on the Internet…".to_string()
        };
        Some(said)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_relays_allow_local_games() {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(local_relays(&v(&["ws://127.0.0.1:7000"])));
        assert!(local_relays(&v(&["wss://localhost:7000/x", "ws://192.168.1.4", "ws://[::1]:80"])));
        assert!(!local_relays(&v(&["ws://127.0.0.1:7000", "wss://relay.damus.io"])));
        assert!(!local_relays(&v(&["wss://81.2.69.142"])));
        assert!(!local_relays(&[]), "no relays: nothing to allow");
    }

    #[test]
    fn regions_convert_both_ways() {
        for r in Region::ALL {
            assert_eq!(ui_region(region(r)), r);
        }
    }

    /// A configured public address that the Internet cannot reach (private,
    /// local, or IPv6, which neither invites nor joiners use) is said in the
    /// lobby, never "Listed on the Internet".
    #[test]
    fn an_unreachable_configured_address_is_said() {
        let id = Identity::from_secret([7; 32]).unwrap();
        let now = Instant::now();
        let line = |addr: &str| {
            let f = FindingSettings {
                public_address: addr.into(),
                relays: Some(Vec::new()),
                stun: Some(Vec::new()),
                ..FindingSettings::default()
            };
            let (h, _) = Hosted::start(Visibility::Public, &f, id.clone(), now, None);
            h.listing(None, 23400).unwrap()
        };
        for bad in ["192.168.1.5", "127.0.0.1", "2a00:1450::1"] {
            assert_eq!(line(bad), UNREACHABLE_ADDRESS, "{bad}");
        }
        assert_eq!(
            UNREACHABLE_ADDRESS,
            "Not listed: your public address setting is not reachable from the Internet"
        );
        assert!(line("81.2.69.142").starts_with("Not listed: no relays"));
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
}
