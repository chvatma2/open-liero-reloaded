//! The online screens' data (plan 10 task 11): what the Multiplayer, Host
//! and Lobby screens show and send, as plain types. This crate stays free
//! of networking (it is layer 5 and `rl-online` is its neighbour): the app
//! (`rl-game`) converts the lobby's own types into these views and the
//! [`LobbyCommand`]s back.

use std::net::SocketAddr;

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

/// How a game is announced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// Announced on the local network (and joinable by address).
    #[default]
    Lan,
    /// Joinable only by address.
    DirectOnly,
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
        w
    }
}

/// A game announced on the local network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanEntry {
    pub name: String,
    pub map: String,
    pub players: u8,
    pub max_players: u8,
    /// A password is needed.
    pub password: bool,
    pub addr: SocketAddr,
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
    pub ping_ms: Option<u16>,
    pub admin: bool,
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
}
