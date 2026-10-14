//! TURN relay notes (decisions N52, N53): a volunteer relay (`rl-server
//! --relay`) announces itself as an addressable kind-31471 note, and
//! joiners keep a [`TurnList`] of them and pick the nearest few. Notes are
//! checked like game announcements (`list`): cheap checks first, one note
//! per key per 10 s, the newest per key, a bounded list. Sans-IO: time is
//! an argument.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use rl_lobby::Identity;
use rl_proto::PublicKey;
use serde::{Deserialize, Serialize};

use crate::announce::{ANNOUNCE_TTL, AnnounceError, Region};
use crate::list::{MIN_NOTE_GAP, is_global};
use crate::nostr::{Event, Filter, MAX_CONTENT};

/// TURN relay notes: addressable, tag `d` = [`TURN_TAG`] (N53).
pub const KIND_TURN: u32 = 31471;
/// The `d` and `t` tag value of every TURN note.
pub const TURN_TAG: &str = "rust-liero-turn";
/// The note format this build writes.
pub const TURN_NOTE_V: u16 = 1;
/// Most relays listed.
pub const MAX_RELAYS: usize = 64;
/// The largest `capacity` a note may claim (allocations).
pub const MAX_CAPACITY: u16 = 4096;
/// Longest user name and password, in bytes.
pub const MAX_CREDENTIAL: usize = 64;
/// Most keys whose last note is remembered for the rate limit.
const MAX_SEEN: usize = 4096;
/// A note dated further ahead than this is refused, in seconds.
const MAX_FUTURE: u64 = 60;

/// What a relay announces: where it listens, its region, how many
/// allocations it takes and holds, and its public long-term credentials
/// (realm `rust-liero`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnNote {
    /// The note format, [`TURN_NOTE_V`].
    pub v: u16,
    pub addr: Ipv4Addr,
    pub port: u16,
    pub region: Region,
    /// Allocations it takes; 0 in its closing note.
    pub capacity: u16,
    /// Allocations it holds now.
    pub load: u16,
    pub user: String,
    pub pass: String,
}

/// 1..=64 bytes of printable ASCII (space included).
fn credential(s: &str) -> bool {
    (1..=MAX_CREDENTIAL).contains(&s.len()) && s.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// The first invalid field of `n`, if any. The address policy (global or
/// local) is the list's.
fn check(n: &TurnNote) -> Result<(), AnnounceError> {
    let bad = |f: &'static str| Err(AnnounceError::Invalid(f));
    if n.addr.is_unspecified() || n.addr.is_broadcast() || n.addr.is_multicast() {
        return bad("addr");
    }
    if n.port == 0 {
        return bad("port");
    }
    if n.capacity > MAX_CAPACITY {
        return bad("capacity");
    }
    if !credential(&n.user) {
        return bad("user");
    }
    if !credential(&n.pass) {
        return bad("pass");
    }
    Ok(())
}

/// Tags d, t = TURN_TAG, expiration now + ttl (ANNOUNCE_TTL, 180, for a live relay; CLOSE_TTL, 10, with capacity 0 for its closing note).
/// `aux` is the signature's randomness.
pub fn turn_note(identity: &Identity, note: &TurnNote, now_unix: u64, ttl: u64, aux: [u8; 32]) -> Event {
    let content = serde_json::to_string(note).unwrap_or_default();
    let s = |a: &str, b: String| vec![a.to_owned(), b];
    let tags = vec![
        s("d", TURN_TAG.into()),
        s("t", TURN_TAG.into()),
        s("expiration", now_unix.saturating_add(ttl).to_string()),
    ];
    Event::sign(identity, now_unix, KIND_TURN, tags, content, aux)
}

/// Reads a note. Does not verify the signature (`Event::verify` does), the
/// time, nor the address policy.
pub fn parse_turn_note(e: &Event) -> Result<TurnNote, AnnounceError> {
    if e.kind != KIND_TURN {
        return Err(AnnounceError::WrongKind);
    }
    if e.tag("d") != Some(TURN_TAG) {
        return Err(AnnounceError::Tag("d"));
    }
    if e.content.len() > MAX_CONTENT {
        return Err(AnnounceError::TooLarge);
    }
    let n: TurnNote = serde_json::from_str(&e.content).map_err(|_| AnnounceError::Json)?;
    check(&n)?;
    Ok(n)
}

/// A relay to allocate on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnPick {
    pub key: PublicKey,
    pub addr: SocketAddr,
    pub user: String,
    pub pass: String,
}

struct Entry {
    note: TurnNote,
    created_at: u64,
    expiration: Option<u64>,
}

/// What is remembered of a key's last accepted note (listed or closed).
#[derive(Clone, Copy)]
struct Seen {
    accepted_unix: u64,
    created_at: u64,
}

/// The announced relays.
#[derive(Default)]
pub struct TurnList {
    relays: BTreeMap<PublicKey, Entry>,
    seen: BTreeMap<PublicKey, Seen>,
    allow_local: bool,
}

impl TurnList {
    pub fn new() -> TurnList {
        TurnList::default()
    }

    /// Lists relays at local, private or reserved addresses too (local
    /// setups and tests); by default only public addresses are listed.
    pub fn allow_local(&mut self, allow: bool) {
        self.allow_local = allow;
    }

    /// The relay subscription: TURN notes since `now_unix - 180`.
    pub fn filter(now_unix: u64) -> Filter {
        Filter {
            kinds: vec![KIND_TURN],
            tags: vec![('t', vec![TURN_TAG.to_owned()])],
            since: Some(now_unix.saturating_sub(ANNOUNCE_TTL)),
            ..Filter::default()
        }
    }

    /// Takes a note from a relay, as `GameList::offer` takes announcements:
    /// a valid, signed, current note, newer than the key's last and at
    /// least 10 s after it (by arrival or by date), while the list has room.
    /// A closing note (capacity 0) is exempt from the 10 s per-key gap, as GameList exempts closes, and removes the relay.
    /// Every accepted closing note leaves a tombstone, so an older open
    /// note from a relay that missed the close is refused. Whether the list
    /// changed.
    pub fn offer(&mut self, e: &Event, now_unix: u64) -> bool {
        // Cheap checks first.
        if e.kind != KIND_TURN || e.content.len() > MAX_CONTENT {
            return false;
        }
        if e.created_at > now_unix.saturating_add(MAX_FUTURE)
            || now_unix.saturating_sub(e.created_at) > ANNOUNCE_TTL
            || e.expiration().is_some_and(|x| x <= now_unix)
        {
            return false;
        }
        let last = self.seen.get(&e.pubkey).copied();
        let listed = self.relays.contains_key(&e.pubkey);
        let gap_ok = last.is_none_or(|l| {
            now_unix.saturating_sub(l.accepted_unix) >= MIN_NOTE_GAP
                || e.created_at >= l.created_at.saturating_add(MIN_NOTE_GAP)
        });
        // A closing note is recognised by our compact encoding before the
        // signature check.
        let closing = e.content.contains("\"capacity\":0,");
        if !gap_ok && (!listed || !closing) {
            return false;
        }
        if last.is_some_and(|l| e.created_at < l.created_at) {
            return false;
        }
        if !listed && !closing && self.relays.len() >= MAX_RELAYS {
            return false;
        }
        if !e.verify() {
            return false;
        }
        let Ok(note) = parse_turn_note(e) else { return false };
        let closing = note.capacity == 0;
        if !gap_ok && !closing {
            return false;
        }
        if !self.allow_local && !is_global(IpAddr::V4(note.addr)) {
            return false;
        }
        // Newer than the key's last note (a closing note may share its second).
        if last.is_some_and(|l| e.created_at < l.created_at || (e.created_at == l.created_at && !closing)) {
            return false;
        }
        if closing {
            let listed = self.relays.remove(&e.pubkey).is_some();
            self.remember(e, now_unix);
            return listed;
        }
        if !listed && self.relays.len() >= MAX_RELAYS {
            return false;
        }
        self.remember(e, now_unix);
        self.relays.insert(e.pubkey, Entry { note, created_at: e.created_at, expiration: e.expiration() });
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
                    .filter(|(k, _)| !self.relays.contains_key(*k))
                    .min_by_key(|(k, s)| (s.accepted_unix, **k))
                    .map(|(k, _)| *k);
                if let Some(k) = oldest {
                    self.seen.remove(&k);
                }
            }
        }
        self.seen.insert(e.pubkey, Seen { accepted_unix: now_unix, created_at: e.created_at });
    }

    /// Forgets keys whose last note no longer constrains anything.
    fn prune_seen(&mut self, now_unix: u64) {
        self.seen.retain(|k, s| {
            self.relays.contains_key(k)
                || now_unix.saturating_sub(s.accepted_unix) < MIN_NOTE_GAP
                || now_unix.saturating_sub(s.created_at) <= ANNOUNCE_TTL
        });
    }

    /// Drops relays whose note expired or is older than 180 s.
    pub fn expire(&mut self, now_unix: u64) {
        self.relays.retain(|_, r| {
            now_unix.saturating_sub(r.created_at) <= ANNOUNCE_TTL && r.expiration.is_none_or(|x| x > now_unix)
        });
        self.prune_seen(now_unix);
    }

    /// Region first, then load/capacity ascending, then key: at most `n`; full relays (load >= capacity) never.
    pub fn pick(&self, region: Region, n: usize) -> Vec<TurnPick> {
        let mut open: Vec<(&PublicKey, &TurnNote)> =
            self.relays.iter().map(|(k, r)| (k, &r.note)).filter(|(_, r)| r.load < r.capacity).collect();
        // Shares compared exactly: a/b < c/d as a·d < c·b (capacities are
        // above the loads, so never zero).
        open.sort_by(|(ka, a), (kb, b)| {
            let share_a = u32::from(a.load) * u32::from(b.capacity);
            let share_b = u32::from(b.load) * u32::from(a.capacity);
            (a.region != region).cmp(&(b.region != region)).then(share_a.cmp(&share_b)).then(ka.cmp(kb))
        });
        open.into_iter()
            .take(n)
            .map(|(k, r)| TurnPick {
                key: *k,
                addr: SocketAddr::from((r.addr, r.port)),
                user: r.user.clone(),
                pass: r.pass.clone(),
            })
            .collect()
    }

    /// How many relays are listed (full ones included).
    pub fn len(&self) -> usize {
        self.relays.len()
    }

    pub fn is_empty(&self) -> bool {
        self.relays.is_empty()
    }
}
