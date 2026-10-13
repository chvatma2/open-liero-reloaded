//! Game announcements (decision N39): the summary a host publishes as a
//! Nostr note, how a note is built and checked, and when to publish.
//! Sans-IO: the caller supplies the time and the randomness.

use std::net::IpAddr;
use std::time::{Duration, Instant};

use rl_lobby::Identity;
use rl_proto::{
    FINGERPRINT, LanAnnounce, LobbyState, MAX_ID, MAX_NAME, MAX_PLAYERS, MAX_SPECTATORS, NO_HASH,
    PROTOCOL_VERSION, Phase, SeatWire, valid_hash, valid_name, valid_pack_id,
};
use serde::{Deserialize, Serialize};

use crate::nostr::{Event, KIND_ANNOUNCE, MAX_CONTENT};

/// The `d` and `t` tag value of every announcement.
pub const APP_TAG: &str = "rust-liero";
/// How long an announcement lives (its NIP-40 expiration), in seconds.
pub const ANNOUNCE_TTL: u64 = 180;
/// How long a closing note lives, in seconds.
pub const CLOSE_TTL: u64 = 10;
/// The announcer's keep-alive period.
pub const REPUBLISH: Duration = Duration::from_secs(60);
/// The least time between two notes of a changing game: above the list's
/// 10 s rate limit, so clock skew does not get a note refused.
pub const MIN_GAP: Duration = Duration::from_secs(12);
/// Longest `mode`, in characters.
pub const MAX_MODE: usize = 16;
/// Longest `map_name`, in characters.
pub const MAX_MAP_NAME: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    Europe,
    NorthAmericaEast,
    NorthAmericaWest,
    SouthAmerica,
    Asia,
    Oceania,
    Africa,
    /// Also what a region this version does not know reads as.
    #[serde(other)]
    Unknown,
}

/// What a game announces about itself. The wire form is JSON with the mod's
/// id under `mod`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameSummary {
    /// The protocol version.
    pub v: u16,
    pub name: String,
    /// A free string of at most `MAX_MODE` characters, such as "deathmatch".
    pub mode: String,
    /// The level's pack id.
    pub map: String,
    /// The level's display name.
    pub map_name: String,
    #[serde(rename = "mod")]
    pub mod_id: String,
    pub mod_hash: String,
    pub players: u8,
    pub max_players: u8,
    pub spectators: u8,
    pub max_spectators: u8,
    pub password: bool,
    pub region: Region,
    pub playing: bool,
    /// The host's public address, when known.
    pub addr: Option<IpAddr>,
    pub port: u16,
    #[serde(default)]
    pub closed: bool,
}

/// What a host knows of its game beside its lobby: the summary's fields a
/// `LobbyState` lacks.
#[derive(Clone, Debug, PartialEq)]
pub struct Hosting {
    /// The port it listens on (TCP and UDP).
    pub port: u16,
    pub max_players: u8,
    pub max_spectators: u8,
    pub password: bool,
    pub region: Region,
    /// The public address (N40), when known.
    pub addr: Option<IpAddr>,
    /// The level's display name (the content's; the lobby has only the
    /// id). A blank or invalid one reads as the level's id.
    pub map_name: String,
}

impl GameSummary {
    /// The one summary of a hosted lobby, for Nostr and (through
    /// [`GameSummary::lan`]) the LAN, so both count alike: players are the
    /// seated members and the bots, spectators the other members, each held
    /// to its maximum; playing from the start to the results. Its notes are
    /// valid when the lobby's name and level id are (the lobby's own are).
    pub fn of_lobby(state: &LobbyState, host: &Hosting) -> GameSummary {
        let seated = state.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).count();
        let count = |n: usize, max: u8| u8::try_from(n).unwrap_or(u8::MAX).min(max);
        let setup = &state.setup;
        let mod_hash = setup.packs.iter().find(|(id, _)| *id == setup.mod_id).map(|p| p.1.as_str());
        GameSummary {
            v: PROTOCOL_VERSION,
            name: state.name.clone(),
            mode: "deathmatch".into(),
            map: setup.level_id.clone(),
            map_name: display_name(&host.map_name, &setup.level_id),
            mod_id: setup.mod_id.clone(),
            mod_hash: mod_hash.unwrap_or(NO_HASH).into(),
            players: count(seated + setup.bots.len(), host.max_players),
            max_players: host.max_players,
            spectators: count(state.members.len() - seated, host.max_spectators),
            max_spectators: host.max_spectators,
            password: host.password,
            region: host.region,
            playing: matches!(state.phase, Phase::Starting | Phase::Playing { .. }),
            addr: host.addr,
            port: host.port,
            closed: false,
        }
    }

    /// The LAN announcement of the same game (this build's fingerprint).
    pub fn lan(&self) -> LanAnnounce {
        LanAnnounce {
            fingerprint: FINGERPRINT,
            name: self.name.clone(),
            port: self.port,
            players: self.players,
            max_players: self.max_players,
            spectators: self.spectators,
            mod_id: self.mod_id.clone(),
            level_id: self.map.clone(),
            password: self.password,
            playing: self.playing,
        }
    }
}

/// `name` cut to [`MAX_MAP_NAME`] characters, else (blank or invalid) `id`'s.
fn display_name(name: &str, id: &str) -> String {
    let cut = |s: &str| s.chars().take(MAX_MAP_NAME).collect::<String>().trim().to_owned();
    let name = cut(name);
    if valid_name(&name, MAX_MAP_NAME) { name } else { cut(id) }
}

/// Why a note is not a valid announcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AnnounceError {
    #[error("not an announcement kind")]
    WrongKind,
    #[error("missing or wrong tag `{0}`")]
    Tag(&'static str),
    #[error("content too large")]
    TooLarge,
    #[error("content is not a game summary")]
    Json,
    #[error("invalid field `{0}`")]
    Invalid(&'static str),
}

/// The first invalid field of `s`, if any.
fn check(s: &GameSummary) -> Result<(), AnnounceError> {
    let bad = |f: &'static str| Err(AnnounceError::Invalid(f));
    if !valid_name(&s.name, MAX_NAME) {
        return bad("name");
    }
    if s.mode.chars().count() > MAX_MODE || s.mode.chars().any(char::is_control) {
        return bad("mode");
    }
    if !valid_pack_id(&s.map) || s.map.len() > MAX_ID {
        return bad("map");
    }
    if !valid_name(&s.map_name, MAX_MAP_NAME) {
        return bad("map_name");
    }
    if !valid_pack_id(&s.mod_id) {
        return bad("mod");
    }
    if !valid_hash(&s.mod_hash) {
        return bad("mod_hash");
    }
    if s.players > s.max_players || usize::from(s.max_players) > MAX_PLAYERS {
        return bad("players");
    }
    if s.spectators > s.max_spectators || usize::from(s.max_spectators) > MAX_SPECTATORS {
        return bad("spectators");
    }
    Ok(())
}

/// Builds and signs the note for `summary`: tags `d`, `t`, `f` (the
/// fingerprint as 16 lowercase hex) and `expiration` (180 s ahead, 10 s for
/// a closing note). `aux` is the signature's randomness.
///
/// The summary must be valid (the caller builds it from its own state); an
/// invalid one still yields a signed note that readers refuse.
pub fn announcement(
    identity: &Identity,
    summary: &GameSummary,
    fingerprint: u64,
    now_unix: u64,
    aux: [u8; 32],
) -> Event {
    let content = serde_json::to_string(summary).unwrap_or_default();
    debug_assert!(content.len() <= MAX_CONTENT);
    let ttl = if summary.closed { CLOSE_TTL } else { ANNOUNCE_TTL };
    let s = |a: &str, b: String| vec![a.to_owned(), b];
    let tags = vec![
        s("d", APP_TAG.into()),
        s("t", APP_TAG.into()),
        s("f", format!("{fingerprint:016x}")),
        s("expiration", now_unix.saturating_add(ttl).to_string()),
    ];
    Event::sign(identity, now_unix, KIND_ANNOUNCE, tags, content, aux)
}

/// Reads a note: the summary and the fingerprint. Does not verify the
/// signature (`Event::verify` does) nor the time.
pub fn parse_announcement(e: &Event) -> Result<(GameSummary, u64), AnnounceError> {
    if e.kind != KIND_ANNOUNCE {
        return Err(AnnounceError::WrongKind);
    }
    if e.tag("d") != Some(APP_TAG) {
        return Err(AnnounceError::Tag("d"));
    }
    let f = e.tag("f").ok_or(AnnounceError::Tag("f"))?;
    if f.len() != 16 || !f.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(AnnounceError::Tag("f"));
    }
    let fingerprint = u64::from_str_radix(f, 16).map_err(|_| AnnounceError::Tag("f"))?;
    if e.content.len() > MAX_CONTENT {
        return Err(AnnounceError::TooLarge);
    }
    let s: GameSummary = serde_json::from_str(&e.content).map_err(|_| AnnounceError::Json)?;
    check(&s)?;
    Ok((s, fingerprint))
}

/// When to publish: every minute, at a change (at most every 10 s) and once
/// when closing. The caller publishes `announcement(..)` when told to.
#[derive(Debug, Default)]
pub struct Announcer {
    last: Option<(Instant, GameSummary)>,
    closed: bool,
}

impl Announcer {
    pub fn new() -> Announcer {
        Announcer::default()
    }

    /// True when a note of `summary` should be published now.
    pub fn tick(&mut self, now: Instant, summary: &GameSummary) -> bool {
        if self.closed {
            return false;
        }
        let due = match &self.last {
            None => true,
            Some((at, last)) => {
                let since = now.saturating_duration_since(*at);
                since >= REPUBLISH || (since >= MIN_GAP && last != summary)
            }
        };
        if due {
            self.last = Some((now, summary.clone()));
        }
        due
    }

    /// True once, when a closing note is due: a note was published and not
    /// yet closed. The caller publishes the summary with `closed: true`.
    pub fn closing(&mut self) -> bool {
        let due = !self.closed && self.last.is_some();
        self.closed = true;
        due
    }
}
