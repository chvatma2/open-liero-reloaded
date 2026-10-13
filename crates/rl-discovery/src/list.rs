//! The game list (decision N41): what the announcements on the relays add
//! up to. Sans-IO: time is an argument.

use std::collections::BTreeMap;
use std::net::IpAddr;

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
    allow_local: bool,
    verifications: u64,
}

/// Whether `a` is a public address (no `is_global` on stable Rust): not
/// unspecified, loopback, private, shared, link-local, documentation,
/// benchmarking, multicast, reserved or broadcast. A v6 address that wraps a
/// v4 one (mapped or compatible) is judged as the v4 address.
pub fn is_global(a: IpAddr) -> bool {
    match a {
        IpAddr::V4(a) => {
            let [a0, a1, a2, _] = a.octets();
            !(a0 == 0
                || a0 == 10
                || (a0 == 100 && (64..128).contains(&a1))
                || a0 == 127
                || (a0 == 169 && a1 == 254)
                || (a0 == 172 && (16..32).contains(&a1))
                || (a0 == 192 && a1 == 0 && (a2 == 0 || a2 == 2))
                || (a0 == 192 && a1 == 168)
                || (a0 == 198 && (a1 == 18 || a1 == 19))
                || (a0 == 198 && a1 == 51 && a2 == 100)
                || (a0 == 203 && a1 == 0 && a2 == 113)
                || a0 >= 224)
        }
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4() {
                return is_global(IpAddr::V4(v4));
            }
            let s = a.segments();
            !(a.is_unspecified()
                || a.is_loopback()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || (s[0] == 0x2001 && s[1] == 0x0db8)
                || (s[0] & 0xff00) == 0xff00)
        }
    }
}

impl GameList {
    pub fn new(fingerprint: u64) -> GameList {
        GameList {
            fingerprint,
            games: BTreeMap::new(),
            seen: BTreeMap::new(),
            blocked: Vec::new(),
            allow_local: false,
            verifications: 0,
        }
    }

    /// Lists games announcing local, private or reserved addresses too (for
    /// local and test relays); by default only public addresses are listed.
    pub fn allow_local(&mut self, allow: bool) {
        self.allow_local = allow;
    }

    /// How many signatures were checked (cheap refusals come first).
    pub fn verifications(&self) -> u64 {
        self.verifications
    }

    /// How many keys' last notes are remembered for the rate limit.
    pub fn remembered_keys(&self) -> usize {
        self.seen.len()
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
    /// one and at least 10 s after it, by arrival or by date (a closing
    /// note of a listed game is exempt from the gap), and the list has
    /// room. Every accepted closing note leaves a tombstone, listed or not,
    /// so an older open note from a relay that missed the close is refused.
    /// Whether the list changed.
    pub fn offer(&mut self, e: &Event, now_unix: u64) -> bool {
        // Cheap checks first.
        if e.kind != KIND_ANNOUNCE || e.content.len() > MAX_CONTENT || self.blocked.contains(&e.pubkey) {
            return false;
        }
        if e.created_at > now_unix.saturating_add(MAX_FUTURE)
            || now_unix.saturating_sub(e.created_at) > ANNOUNCE_TTL
            || e.expiration().is_some_and(|x| x <= now_unix)
        {
            return false;
        }
        let last = self.seen.get(&e.pubkey).copied();
        let listed = self.games.contains_key(&e.pubkey);
        // The gap counts in arrival time, or in the notes' own dates: a
        // note dated a gap after the last one is no faster than its host
        // published (a key stays bounded: dates lie within the last 180 s
        // and the next 60 s).
        let gap_ok = last.is_none_or(|l| {
            now_unix.saturating_sub(l.accepted_unix) >= MIN_NOTE_GAP
                || e.created_at >= l.created_at.saturating_add(MIN_NOTE_GAP)
        });
        // A closing note of a listed game may follow at once; any other
        // note waits. A note older than the key's last is stale. A full
        // list takes no new key (a closing note still leaves its
        // tombstone). All are refused before the costly signature check (a
        // closing note is recognised by our compact encoding).
        let closing = e.content.contains("\"closed\":true");
        if !gap_ok && (!listed || !closing) {
            return false;
        }
        if last.is_some_and(|l| e.created_at < l.created_at) {
            return false;
        }
        if !listed && !closing && self.games.len() >= MAX_GAMES {
            return false;
        }
        self.verifications += 1;
        if !e.verify() {
            return false;
        }
        let Ok((summary, fingerprint)) = parse_announcement(e) else { return false };
        if !gap_ok && !summary.closed {
            return false;
        }
        if !self.allow_local && summary.addr.is_some_and(|a| !is_global(a)) {
            return false;
        }
        // Newer than the key's last note (a closing note may share its second).
        if last
            .is_some_and(|l| e.created_at < l.created_at || (e.created_at == l.created_at && !summary.closed))
        {
            return false;
        }
        if summary.closed {
            // A tombstone, listed or not: another relay may still hold the
            // older open note and deliver it later (final review A-I1).
            let listed = self.games.remove(&e.pubkey).is_some();
            self.remember(e, now_unix);
            return listed;
        }
        if !listed && self.games.len() >= MAX_GAMES {
            return false;
        }
        self.remember(e, now_unix);
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
        true
    }

    /// Records the accepted note. When the table is full, stale keys go,
    /// then the oldest key that is not listed.
    fn remember(&mut self, e: &Event, now_unix: u64) {
        if self.seen.len() >= MAX_SEEN && !self.seen.contains_key(&e.pubkey) {
            self.prune_seen(now_unix);
            if self.seen.len() >= MAX_SEEN {
                let oldest = self
                    .seen
                    .iter()
                    .filter(|(k, _)| !self.games.contains_key(*k))
                    .min_by_key(|(k, s)| (s.accepted_unix, **k))
                    .map(|(k, _)| *k);
                if let Some(k) = oldest {
                    self.seen.remove(&k);
                }
            }
        }
        self.seen.insert(e.pubkey, Seen { accepted_unix: now_unix, created_at: e.created_at });
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

    /// How many games are listed (compatible or not).
    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// Hides a key's games from now on.
    pub fn block(&mut self, key: PublicKey) {
        if !self.blocked.contains(&key) {
            self.blocked.push(key);
        }
        self.games.remove(&key);
    }

    /// Lists a key's games again from its next note (its notes were not
    /// kept while it was blocked).
    pub fn unblock(&mut self, key: &PublicKey) {
        self.blocked.retain(|k| k != key);
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
