//! The online screens' data (plan 10 task 11): what the Multiplayer, Host
//! and Lobby screens show and send, as plain types. This crate stays free
//! of networking (it is layer 5 and `rl-online` is its neighbour): the app
//! (`rl-game`) converts the lobby's own types into these views and the
//! [`LobbyCommand`]s back.

use std::net::{IpAddr, SocketAddr};

use rl_input::KeyCode;
use rl_sim::MAX_SLOTS;
use serde::{Deserialize, Serialize};

use crate::match_prefs::MatchPrefs;

/// The default port a game is hosted on and joined at (spec §5.1).
pub const DEFAULT_PORT: u16 = 23400;

/// The players a lobby holds (spec §7.2); the Host screen's maximum.
pub const MAX_PLAYERS: u8 = 32;

/// The fewest players a hosted game can be for.
pub const MIN_PLAYERS: u8 = 2;

/// A chat line's longest length in characters (spec §7.2).
pub const CHAT_CHARS: usize = 200;

/// A lobby's [`LobbyView::phase`] while the match is still being set up;
/// any other phase is a match under way, which Back asks about first.
pub const PHASE_LOBBY: &str = "Lobby";

/// The [`LobbyView::listing`] of a lobby whose host took the match over
/// from a lost host (plan 12c, N72): only the match's members (players,
/// spectators, those waiting) may come back, for the lobby's life.
pub const MIGRATED_LISTING: &str = "Taken over from the host: closed to new players";

/// A [`MemberView::seat`] that watches instead of playing.
pub const SEAT_SPECTATOR: &str = "Spectator";

/// The longest address typed (a DNS name's limit) and password.
pub const ADDRESS_CHARS: usize = 253;
pub const PASSWORD_CHARS: usize = 64;

/// A password: `Debug` never shows it (commands and settings get logged).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Password(pub String);

impl Password {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Password {
    fn from(s: &str) -> Password {
        Password(s.to_string())
    }
}

impl std::fmt::Debug for Password {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Password(..)")
    }
}

/// Who can join a hosted game (decision N43).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// Announced on the Internet (Nostr) and on the local network.
    Public,
    /// Announced on the local network (and joinable by address).
    #[default]
    Lan,
    /// Announced nowhere: joined by its invite code or address (plan 10's
    /// `DirectOnly` reads as this).
    #[serde(alias = "DirectOnly")]
    Invite,
}

impl Visibility {
    pub const ALL: [Visibility; 3] = [Visibility::Public, Visibility::Lan, Visibility::Invite];

    /// The Host screen's words.
    pub fn label(self) -> &'static str {
        match self {
            Visibility::Public => "Anyone (Internet)",
            Visibility::Lan => "This network",
            Visibility::Invite => "Invite only",
        }
    }
}

/// The region a host says it is in (`rl_discovery::Region`, mirrored: this
/// crate stays free of networking; `rl-game` converts, naming every
/// variant).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    Europe,
    NorthAmericaEast,
    NorthAmericaWest,
    SouthAmerica,
    Asia,
    Oceania,
    Africa,
    /// Also what a region this version does not know reads as.
    #[default]
    #[serde(other)]
    Unknown,
}

impl Region {
    pub const ALL: [Region; 8] = [
        Region::Unknown,
        Region::Europe,
        Region::NorthAmericaEast,
        Region::NorthAmericaWest,
        Region::SouthAmerica,
        Region::Asia,
        Region::Oceania,
        Region::Africa,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Region::Europe => "Europe",
            Region::NorthAmericaEast => "North America (east)",
            Region::NorthAmericaWest => "North America (west)",
            Region::SouthAmerica => "South America",
            Region::Asia => "Asia",
            Region::Oceania => "Oceania",
            Region::Africa => "Africa",
            Region::Unknown => "Not said",
        }
    }
}

/// The most relays, or STUN servers, the options keep.
pub const MAX_SERVERS: usize = 32;
/// The longest relay URL or STUN server kept, in bytes.
pub const SERVER_BYTES: usize = 200;
/// The most blocked hosts kept.
pub const MAX_BLOCKED: usize = 1000;

/// Whether `url` can be a relay: `wss://` or `ws://` and a host, at most
/// [`SERVER_BYTES`], no whitespace or control character.
pub fn valid_relay(url: &str) -> bool {
    let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://"));
    rest.is_some_and(|r| !r.is_empty() && !r.starts_with('/'))
        && url.len() <= SERVER_BYTES
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Whether `server` can be a STUN server: `host:port`, at most
/// [`SERVER_BYTES`], no whitespace or control character.
pub fn valid_stun(server: &str) -> bool {
    let port =
        server.rsplit_once(':').filter(|(h, _)| !h.is_empty()).and_then(|(_, p)| p.parse::<u16>().ok());
    port.is_some_and(|p| p != 0)
        && server.len() <= SERVER_BYTES
        && !server.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The most TURN servers of the player's own the options keep (N52; a
/// join uses the first 2).
pub const MAX_TURN: usize = 8;

/// A TURN server entry's parts, `user:pass@host:port`: the user (before the
/// first `:`), the password (up to the last `@`) and `host:port`, each not
/// empty, the port not 0; at most [`SERVER_BYTES`], no whitespace or
/// control character. `None`: not one.
pub fn turn_parts(entry: &str) -> Option<(&str, &str, &str)> {
    if entry.len() > SERVER_BYTES || entry.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (credentials, server) = entry.rsplit_once('@')?;
    let (user, pass) = credentials.split_once(':')?;
    (!user.is_empty() && !pass.is_empty() && valid_stun(server)).then_some((user, pass, server))
}

/// Whether `entry` can be a TURN server: `user:pass@host:port` ([`turn_parts`]).
pub fn valid_turn(entry: &str) -> bool {
    turn_parts(entry).is_some()
}

/// A TURN server entry as the options list it: its password hidden. An
/// entry that is not one shows nothing of itself (where its password
/// ends cannot be told): "•••".
pub fn turn_shown(entry: &str) -> String {
    match turn_parts(entry) {
        Some((user, _, server)) => format!("{user}:•••@{server}"),
        None => "•••".into(),
    }
}

/// A host's key as the options keep it: 64 lowercase hex characters.
pub fn key_hex(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

/// `hex` as a key (64 hex characters, any case).
pub fn parse_key_hex(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(key)
}

/// Finding games on the Internet and reaching them (plans 12a and 12b; the
/// options' Online tab). `Debug` hides the TURN servers' passwords
/// ([`turn_shown`]): settings, actions and commands may be printed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindingSettings {
    /// The region this player's hosted games say.
    pub region: Region,
    /// The Nostr relays (`wss://` or `ws://`); `None`: the game's
    /// defaults (`MenuData::default_relays`), which may grow.
    pub relays: Option<Vec<String>>,
    /// The STUN servers (`host:port`) the game's socket asks for its
    /// public address and port (ICE's candidates); `None`: the defaults.
    pub stun: Option<Vec<String>>,
    /// The address a hosted game says players join it at directly (a
    /// manual port forward); empty: none (N47: the router's mapping, else
    /// joins through the relays).
    pub public_address: String,
    /// Hosts whose games the Internet tab hides: their keys, 64 lowercase
    /// hex characters.
    pub blocked: Vec<String>,
    /// A hosted Public or Invite game asks the router to open its port
    /// (UPnP, NAT-PMP, PCP; N51). On by default; an older file without it
    /// reads as on.
    pub port_mapping: bool,
    /// The player's own TURN servers for joining (N52), `user:pass@host:port`
    /// ([`valid_turn`]), at most [`MAX_TURN`]; a join uses the first 2.
    /// None by default: the project ships no TURN list.
    pub turn: Vec<String>,
}

impl std::fmt::Debug for FindingSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let turn: Vec<String> = self.turn.iter().map(|t| turn_shown(t)).collect();
        f.debug_struct("FindingSettings")
            .field("region", &self.region)
            .field("relays", &self.relays)
            .field("stun", &self.stun)
            .field("public_address", &self.public_address)
            .field("blocked", &self.blocked)
            .field("port_mapping", &self.port_mapping)
            .field("turn", &turn)
            .finish()
    }
}

impl Default for FindingSettings {
    fn default() -> Self {
        FindingSettings {
            region: Region::default(),
            relays: None,
            stun: None,
            public_address: String::new(),
            blocked: Vec::new(),
            port_mapping: true,
            turn: Vec::new(),
        }
    }
}

impl FindingSettings {
    /// Fixes what a hand-edited file may hold: bad entries dropped (one
    /// warning each), duplicates and the excess beyond the limits dropped.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        let mut list = |name: &str, list: &mut Option<Vec<String>>, ok: fn(&str) -> bool| {
            let Some(l) = list else { return };
            let mut kept: Vec<String> = Vec::new();
            for s in l.drain(..) {
                let s = s.trim().to_string();
                if !ok(&s) {
                    w.push(format!("online.finding.{name}: {s:?} is not one: dropped"));
                } else if !kept.contains(&s) {
                    kept.push(s);
                }
            }
            if kept.len() > MAX_SERVERS {
                w.push(format!("online.finding.{name}: over {MAX_SERVERS}: the rest dropped"));
                kept.truncate(MAX_SERVERS);
            }
            *l = kept;
        };
        list("relays", &mut self.relays, valid_relay);
        list("stun", &mut self.stun, valid_stun);
        let mut turn: Vec<String> = Vec::new();
        for t in self.turn.drain(..) {
            let t = t.trim().to_string();
            if !valid_turn(&t) {
                // The entry may hold a password: never in a warning.
                w.push("online.finding.turn: an entry is not user:pass@host:port: dropped".into());
            } else if !turn.contains(&t) {
                turn.push(t);
            }
        }
        if turn.len() > MAX_TURN {
            w.push(format!("online.finding.turn: over {MAX_TURN}: the rest dropped"));
            turn.truncate(MAX_TURN);
        }
        self.turn = turn;
        let address = self.public_address.trim();
        if !address.is_empty() && address.parse::<IpAddr>().is_err() {
            w.push(format!("online.finding.public_address {address:?} is not an IP address: none"));
            self.public_address.clear();
        } else {
            self.public_address = address.to_string();
        }
        let mut blocked: Vec<String> = Vec::new();
        for k in self.blocked.drain(..) {
            match parse_key_hex(k.trim()) {
                Some(key) => {
                    let hex = key_hex(&key);
                    if !blocked.contains(&hex) {
                        blocked.push(hex);
                    }
                }
                None => w.push(format!("online.finding.blocked: {k:?} is not a key: dropped")),
            }
        }
        if blocked.len() > MAX_BLOCKED {
            w.push(format!("online.finding.blocked: over {MAX_BLOCKED}: the oldest dropped"));
            blocked.drain(..blocked.len() - MAX_BLOCKED);
        }
        self.blocked = blocked;
        w
    }
}

/// The Host screen's choices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostPrefs {
    /// The host's name in the lobby (empty: the profile playing's).
    pub name: String,
    pub visibility: Visibility,
    /// Empty: no password. Never saved: the form starts empty.
    #[serde(skip)]
    pub password: Password,
    /// [`MIN_PLAYERS`] to [`MAX_PLAYERS`].
    pub max_players: u8,
    pub port: u16,
}

impl Default for HostPrefs {
    fn default() -> Self {
        HostPrefs {
            name: String::new(),
            visibility: Visibility::Lan,
            password: Password::default(),
            max_players: 8,
            port: DEFAULT_PORT,
        }
    }
}

impl HostPrefs {
    /// Fixes out-of-range values (a hand-edited file): one warning each.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&self.max_players) {
            let fixed = self.max_players.clamp(MIN_PLAYERS, MAX_PLAYERS);
            w.push(format!(
                "online.host.max_players {} is not {MIN_PLAYERS} to {MAX_PLAYERS}: {fixed}",
                self.max_players
            ));
            self.max_players = fixed;
        }
        if self.password.0.chars().count() > PASSWORD_CHARS {
            self.password.0 = self.password.0.chars().take(PASSWORD_CHARS).collect();
            w.push(format!("online.host.password is over {PASSWORD_CHARS} characters: cut"));
        }
        if self.port == 0 {
            w.push(format!("online.host.port is 0: {DEFAULT_PORT}"));
            self.port = DEFAULT_PORT;
        }
        let name: String =
            self.name.trim().chars().filter(|c| !c.is_control()).take(crate::MAX_NAME_CHARS).collect();
        self.name = name;
        w
    }
}

/// What the online screens remember (`settings.ron`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnlineSettings {
    /// The Multiplayer screen's address field.
    pub last_address: String,
    /// The Host screen's choices.
    pub host: HostPrefs,
    /// The port an address typed without one is joined at.
    pub port: u16,
    /// The key that opens the chat in an online match (spec §7.2: T,
    /// configurable). A key the game keeps (`GAME_KEYS`) is not one.
    pub chat_key: KeyCode,
    /// Finding games on the Internet: region, relays, STUN, blocked hosts.
    pub finding: FindingSettings,
}

/// The chat key by default (spec §7.2).
pub const CHAT_KEY: KeyCode = KeyCode::KeyT;

impl Default for OnlineSettings {
    fn default() -> Self {
        OnlineSettings {
            last_address: String::new(),
            host: HostPrefs::default(),
            port: DEFAULT_PORT,
            chat_key: CHAT_KEY,
            finding: FindingSettings::default(),
        }
    }
}

impl OnlineSettings {
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = self.host.sanitize();
        if self.last_address.chars().count() > ADDRESS_CHARS {
            self.last_address = self.last_address.chars().take(ADDRESS_CHARS).collect();
            w.push(format!("online.last_address is over {ADDRESS_CHARS} characters: cut"));
        }
        if self.port == 0 {
            w.push(format!("online.port is 0: {DEFAULT_PORT}"));
            self.port = DEFAULT_PORT;
        }
        if let Some((_, what)) = crate::settings::GAME_KEYS.iter().find(|(k, _)| *k == self.chat_key) {
            w.push(format!("online.chat_key {:?} is the game's ({what}): {CHAT_KEY:?}", self.chat_key));
            self.chat_key = CHAT_KEY;
        }
        w.extend(self.finding.sanitize());
        w
    }
}

/// A game announced on the local network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanEntry {
    pub name: String,
    pub map: String,
    /// The seated members: the seats taken of `max_players`.
    pub players: u8,
    pub max_players: u8,
    /// The bots, which take no seat.
    pub bots: u8,
    /// A password is needed.
    pub password: bool,
    pub addr: SocketAddr,
}

/// A listed game's players as the lists show them: the seats taken of
/// the seats, then its bots apart ("1/8 +3 bots"; "1/8" without bots).
/// Bots take no seat a joiner could have (the user's report of
/// 2026-10-15).
fn players_text(players: u8, max_players: u8, bots: u8) -> String {
    match bots {
        0 => format!("{players}/{max_players}"),
        1 => format!("{players}/{max_players} +1 bot"),
        n => format!("{players}/{max_players} +{n} bots"),
    }
}

impl LanEntry {
    /// [`players_text`] of this game.
    pub fn players_text(&self) -> String {
        players_text(self.players, self.max_players, self.bots)
    }

    /// Every seat is taken (bots take none).
    pub fn is_full(&self) -> bool {
        self.players >= self.max_players
    }
}

/// A ping as the Internet tab shows it (`rl_discovery::Ping`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PingView {
    #[default]
    Unknown,
    Measuring,
    Ms(u32),
    NoAnswer,
}

impl PingView {
    pub fn text(self) -> String {
        match self {
            PingView::Unknown => String::new(),
            PingView::Measuring => "…".into(),
            PingView::Ms(ms) => format!("{ms} ms"),
            PingView::NoAnswer => "no answer".into(),
        }
    }
}

/// A game announced on the Internet (the app converts the game list's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternetEntry {
    /// The host's key (what Join expects, and what Block hides).
    pub key: [u8; 32],
    pub name: String,
    /// The level's name.
    pub map: String,
    /// The mod's name (its id when this game does not have it).
    pub mod_name: String,
    /// The seated members: the seats taken of `max_players`.
    pub players: u8,
    pub max_players: u8,
    /// The bots, which take no seat.
    pub bots: u8,
    pub ping: PingView,
    pub region: Region,
    /// A password is needed.
    pub password: bool,
    /// Where it is joined directly (N47: its router's open port, or its
    /// public address setting); `None`: joined through the relays alone.
    pub addr: Option<SocketAddr>,
    /// For a game of another version, "needs version …" (greyed out).
    pub incompatible: Option<String>,
    /// A match is under way (joiners watch it first).
    pub playing: bool,
}

/// The Internet tab's state (the app keeps it current while the
/// Multiplayer screen shows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InternetStatus {
    /// No relay is connected and each has failed (or none is set).
    NoRelay,
    /// Waiting for a relay's stored games (its first `EOSE`).
    #[default]
    Searching,
    /// The games are listed.
    Listed,
}

impl InternetEntry {
    /// [`players_text`] of this game.
    pub fn players_text(&self) -> String {
        players_text(self.players, self.max_players, self.bots)
    }

    /// Every seat is taken (bots take none).
    pub fn is_full(&self) -> bool {
        self.players >= self.max_players
    }
}

/// What the Internet tab lists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InternetView {
    pub status: InternetStatus,
    pub games: Vec<InternetEntry>,
}

/// The shortest invite code: `rlx1`, 35 bytes of payload and the checksum.
const MIN_CODE: usize = 60;

/// The text of the Direct tab's field is an invite code: `rlx1`, any case,
/// or at least [`MIN_CODE`] letters and digits (a code with a typo in its
/// prefix, which the game then calls damaged; no host name is one label
/// that long), never an address to look up.
pub fn is_invite(text: &str) -> bool {
    let text = text.trim();
    text.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("rlx1"))
        || (text.len() >= MIN_CODE && text.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The lobby as the player in it sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct LobbyView {
    /// The lobby's (the host's) name.
    pub name: String,
    /// This player's member id.
    pub me: u8,
    /// This player hosts it.
    pub host: bool,
    /// [`PHASE_LOBBY`], else what is going on (a match playing).
    pub phase: String,
    pub members: Vec<MemberView>,
    pub setup: MatchPrefs,
    /// Whether the host may start now, else why not.
    pub can_start: Result<(), String>,
    /// The content downloads: a client's from its host (a host's: its
    /// members' skins), one row each.
    pub downloads: Vec<DownloadView>,
    /// The content this game lacks is this many bytes, over the prompt:
    /// nothing downloads until the player says Download (or leaves).
    pub prompt: Option<u64>,
    /// The hosted game's invite code (Copy puts it on the clipboard).
    pub invite: Option<String>,
    /// Where the hosted game stands: a Public game's listing on the
    /// Internet (or why not), an Invite game's code without an address.
    pub listing: Option<String>,
    /// A client's way to its host (`rl_online::path_text`: "connected
    /// directly to …", "… through the TURN relay …").
    pub connection: Option<String>,
    /// A hosted Public or Invite game's reachability, in order: its
    /// router's port, its public address setting, a symmetric NAT, its
    /// relays (`rl_online::Reachability::lines`), under `listing`.
    pub reach: Vec<String>,
}

/// One download of the lobby's list (plan 11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadView {
    /// The pack's id.
    pub name: String,
    /// "mod", "level", "theme" or "skin".
    pub kind: String,
    /// Bytes received of `size`.
    pub have: u64,
    pub size: u64,
    /// `queued`, `downloading`, `checking`, `done`, `failed: <why>` or
    /// `not available`.
    pub state: String,
}

/// The options' download cache row (the app keeps it current).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadCacheView {
    /// The bytes it holds.
    pub bytes: u64,
    /// Whether Clear may run now, else why not (an online game is on).
    pub clear: Result<(), String>,
}

/// `bytes` in megabytes for the player: MiB (2^20 bytes, the unit of the
/// download prompt's 50 MB threshold), shown as "MB"; one decimal below
/// 100 MB ("73.4 MB"), whole above ("312 MB").
pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1u64 << 20) as f64;
    if mb < 100.0 { format!("{mb:.1} MB") } else { format!("{mb:.0} MB") }
}

/// One member of a lobby.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberView {
    pub id: u8,
    pub name: String,
    /// The key's short fingerprint (the first 8 characters show).
    pub fingerprint: String,
    pub colour: [u8; 3],
    /// "Player 1", ..., or [`SEAT_SPECTATOR`].
    pub seat: String,
    pub ready: bool,
    /// Content (mods, levels, skins) this member lacks.
    pub missing: Vec<String>,
    /// Percent of the missing content received, while it downloads.
    pub download: Option<u8>,
    pub ping_ms: Option<u16>,
    pub admin: bool,
    /// How the host reaches this member, in a word or two
    /// (`rl_online::path_tag`: "direct", "open port", "punched",
    /// "relayed"); `None` for the host itself.
    pub path: Option<String>,
}

impl MemberView {
    pub fn is_spectator(&self) -> bool {
        self.seat == SEAT_SPECTATOR
    }
}

/// What a lobby's player asks of the lobby ([`crate::AppCommand::Lobby`]).
#[derive(Clone, Debug, PartialEq)]
pub enum LobbyCommand {
    Ready(bool),
    Spectate(bool),
    Chat(String),
    Setup(MatchPrefs),
    Kick {
        member: u8,
        ban: bool,
    },
    /// Moves a member to the spectators or back to the players (the host).
    Move {
        member: u8,
        spectator: bool,
    },
    Start,
    /// Starts although players are not ready or lack content: they watch.
    ForceStart,
    /// This player's weapons by mod id, or random.
    Loadout(Option<[i32; MAX_SLOTS]>),
    /// The player lets the content over the prompt download.
    ApproveDownloads,
}
