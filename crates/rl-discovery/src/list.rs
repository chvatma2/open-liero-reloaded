//! The game list (decision N41): what the announcements on the relays add
//! up to. Sans-IO: time is an argument.

use std::collections::BTreeMap;

use rl_proto::PublicKey;

use crate::announce::{ANNOUNCE_TTL, GameSummary, parse_announcement};
use crate::nostr::{Event, Filter, KIND_ANNOUNCE, MAX_CONTENT};

/// Most games listed.
pub const MAX_GAMES: usize = 500;
/// A key's notes are accepted at most this often, in seconds.
pub const MIN_NOTE_GAP: u64 = 10;
/// Most keys whose last note is remembered for the rate limit.
const MAX_SEEN: usize = 4096;
/// A note dated further ahead than this is refused, in seconds.
const MAX_FUTURE: u64 = 60;

/// The result of timing a game's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ping {
    Unknown,
    Measuring,
    Ms(u32),
    NoAnswer,
}

impl Ping {
    /// Sort rank: measured, then not yet measured, then no answer.
    fn rank(self) -> (u8, u32) {
        match self {
            Ping::Ms(ms) => (0, ms),
            Ping::Unknown | Ping::Measuring => (1, 0),
            Ping::NoAnswer => (2, 0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListedGame {
    pub key: PublicKey,
    pub summary: GameSummary,
    /// The game runs this build's protocol fingerprint.
    pub compatible: bool,
    /// For an incompatible game, "needs version <protocol>-<6 hex digits>";
    /// empty otherwise.
    pub version_hint: String,
    pub ping: Ping,
    /// `now_unix` when the note was accepted.
    pub seen_unix: u64,
}

struct Entry {
    summary: GameSummary,
    fingerprint: u64,
    created_at: u64,
    expiration: Option<u64>,
    ping: Ping,
    seen_unix: u64,
}

/// What is remembered of a key's last accepted note (listed or closed).
#[derive(Clone, Copy)]
struct Seen {
    accepted_unix: u64,
    created_at: u64,
}

pub struct GameList {
    fingerprint: u64,
    games: BTreeMap<PublicKey, Entry>,
    seen: BTreeMap<PublicKey, Seen>,
    blocked: Vec<PublicKey>,
}

impl GameList {
    pub fn new(fingerprint: u64) -> GameList {
        GameList { fingerprint, games: BTreeMap::new(), seen: BTreeMap::new(), blocked: Vec::new() }
    }

    /// The relay subscription: announcements tagged `rust-liero`.
    pub fn filter() -> Filter {
        Filter {
            kinds: vec![KIND_ANNOUNCE],
            authors: Vec::new(),
            tags: vec![('t', vec![crate::announce::APP_TAG.to_owned()])],
            since: None,
            limit: Some(MAX_GAMES as u32),
        }
    }

    /// `filter()` limited to notes since `now_unix - 180`.
    pub fn filter_since(now_unix: u64) -> Filter {
        Filter { since: Some(now_unix.saturating_sub(ANNOUNCE_TTL)), ..GameList::filter() }
    }

    /// Takes a note from a relay. It is dropped unless it is a valid, signed,
    /// current announcement of a key not blocked, newer than the key's last
    /// one and at least 10 s after it (a closing note of a listed game is
    /// exempt from the gap), and the list has room.
    pub fn offer(&mut self, e: &Event, now_unix: u64) {
        // Cheap checks first.
        if e.kind != KIND_ANNOUNCE || e.content.len() > MAX_CONTENT || self.blocked.contains(&e.pubkey) {
            return;
        }
        if e.created_at > now_unix.saturating_add(MAX_FUTURE)
            || now_unix.saturating_sub(e.created_at) > ANNOUNCE_TTL
            || e.expiration().is_some_and(|x| x <= now_unix)
        {
            return;
        }
        let last = self.seen.get(&e.pubkey).copied();
        let listed = self.games.contains_key(&e.pubkey);
        let gap_ok = last.is_none_or(|l| now_unix.saturating_sub(l.accepted_unix) >= MIN_NOTE_GAP);
        // A closing note may follow at once; any other must wait.
        if !gap_ok && !listed {
            return;
        }
        if !e.verify() {
            return;
        }
        let Ok((summary, fingerprint)) = parse_announcement(e) else { return };
        if !gap_ok && !summary.closed {
            return;
        }
        // Newer than the key's last note (a closing note may share its second).
        if last
            .is_some_and(|l| e.created_at < l.created_at || (e.created_at == l.created_at && !summary.closed))
        {
            return;
        }
        if summary.closed {
            // Only a listed game has anything to close.
            if self.games.remove(&e.pubkey).is_some() {
                self.remember(e, now_unix);
            }
            return;
        }
        if !listed && self.games.len() >= MAX_GAMES {
            return;
        }
        if !self.remember(e, now_unix) {
            return;
        }
        let ping = match self.games.get(&e.pubkey) {
            Some(old) if old.summary.addr == summary.addr && old.summary.port == summary.port => old.ping,
            _ => Ping::Unknown,
        };
        self.games.insert(
            e.pubkey,
            Entry {
                summary,
                fingerprint,
                created_at: e.created_at,
                expiration: e.expiration(),
                ping,
                seen_unix: now_unix,
            },
        );
    }

    /// Records the accepted note; false when the table is full.
    fn remember(&mut self, e: &Event, now_unix: u64) -> bool {
        if self.seen.len() >= MAX_SEEN && !self.seen.contains_key(&e.pubkey) {
            self.prune_seen(now_unix);
            if self.seen.len() >= MAX_SEEN {
                return false;
            }
        }
        self.seen.insert(e.pubkey, Seen { accepted_unix: now_unix, created_at: e.created_at });
        true
    }

    /// Forgets keys whose last note no longer constrains anything: the gap
    /// has passed and the note is too old to be accepted again.
    fn prune_seen(&mut self, now_unix: u64) {
        self.seen.retain(|k, s| {
            self.games.contains_key(k)
                || now_unix.saturating_sub(s.accepted_unix) < MIN_NOTE_GAP
                || now_unix.saturating_sub(s.created_at) <= ANNOUNCE_TTL
        });
    }

    /// Drops games whose note expired or is older than 180 s.
    pub fn expire(&mut self, now_unix: u64) {
        self.games.retain(|_, g| {
            now_unix.saturating_sub(g.created_at) <= ANNOUNCE_TTL && g.expiration.is_none_or(|x| x > now_unix)
        });
        self.prune_seen(now_unix);
    }

    /// Hides a key's games from now on.
    pub fn block(&mut self, key: PublicKey) {
        if !self.blocked.contains(&key) {
            self.blocked.push(key);
        }
        self.games.remove(&key);
    }

    pub fn blocked(&self) -> &[PublicKey] {
        &self.blocked
    }

    /// Sets a listed game's ping; unlisted keys are ignored.
    pub fn set_ping(&mut self, key: &PublicKey, ping: Ping) {
        if let Some(g) = self.games.get_mut(key) {
            g.ping = ping;
        }
    }

    /// Compatible games first, then by measured ping, not yet measured,
    /// no answer; ties by name, then key.
    pub fn games(&self) -> Vec<ListedGame> {
        let mut v: Vec<ListedGame> = self
            .games
            .iter()
            .map(|(key, g)| {
                let compatible = g.fingerprint == self.fingerprint;
                let version_hint = if compatible {
                    String::new()
                } else {
                    format!("needs version {}-{}", g.summary.v, &format!("{:016x}", g.fingerprint)[..6])
                };
                ListedGame {
                    key: *key,
                    summary: g.summary.clone(),
                    compatible,
                    version_hint,
                    ping: g.ping,
                    seen_unix: g.seen_unix,
                }
            })
            .collect();
        v.sort_by(|a, b| {
            (!a.compatible, a.ping.rank(), &a.summary.name, a.key).cmp(&(
                !b.compatible,
                b.ping.rank(),
                &b.summary.name,
                b.key,
            ))
        });
        v
    }
}
