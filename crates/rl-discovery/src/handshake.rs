//! Handshake events (decision N45): a joiner's WebRTC offer and the host's
//! answer, as ephemeral kind-21470 events addressed to the other's key
//! (`p` tag, expiration 60 s ahead), their content NIP-44 encrypted
//! (N44). Sans-IO: the caller supplies time and randomness.
//!
//! - [`Asker`] (the joiner) signs and encrypts with a fresh key per join,
//!   never the player's identity, so relays cannot link joins to players.
//!   It publishes its offer at once and again every [`RESEND`] (a new event,
//!   the same session) until the host's reply, and gives up after
//!   [`ANSWER_DEADLINE`].
//! - [`Inbox`] (the host) takes offers addressed to it: cheap checks
//!   first (size, kind, tag, age), a rate bucket before any decryption (10
//!   new offers a second, a burst of 20), and remembers each session for
//!   [`SESSION_MEMORY`] (at most 4096), so a resent offer is answered with
//!   the same reply event. A hostile or over-limit event is ignored and
//!   counted: its sender cannot be dropped, its key being ephemeral.
//!
//! Cost: `Inbox::offer` checks every signature again, although the pool's
//! drivers verified each event already (a forgery never reaches it from a
//! `Pool`). What it spends before the rate bucket is thus bounded only by
//! the pool's per-relay limits (200 frames/s): call it from the host's
//! network thread, never the game's UI thread.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_lobby::Identity;
use rl_proto::{MAX_SDP, PublicKey, Refusal, Signal};
use serde::{Deserialize, Serialize};

use crate::nip44::{conversation_key, decrypt, encrypt};
use crate::nostr::{Event, Filter, KIND_HANDSHAKE};

/// The largest handshake content (encrypted), in bytes.
pub const MAX_HANDSHAKE_CONTENT: usize = 16 * 1024;
/// The joiner re-sends its offer this often until a reply.
pub const RESEND: Duration = Duration::from_secs(2);
/// The joiner gives up this long after it started asking.
pub const ANSWER_DEADLINE: Duration = Duration::from_secs(10);
/// Handshake events dated further than this from now (either way) are
/// ignored, in seconds; also their expiration, ahead of `created_at`.
pub const MAX_SKEW: u64 = 60;
/// How long the host remembers a session and its reply.
pub const SESSION_MEMORY: Duration = Duration::from_secs(120);
/// Most sessions remembered; when full the oldest is dropped.
pub const MAX_SESSIONS: usize = 4096;
/// New offers a second, after the burst.
pub const OFFER_RATE: u64 = 10;
/// New offers taken at once.
pub const OFFER_BURST: u64 = 20;
/// Most signed events of a session after its offer (resends, answered
/// with its cached reply or not yet): the joiner sends at most
/// `ANSWER_DEADLINE / RESEND` events in all.
const MAX_REPEATS: u8 = 5;
/// Most event ids remembered per session, to tell a relay's duplicate (or
/// a replay) from a resend.
const MAX_IDS: usize = 8;
/// The content's format version.
const WIRE_V: u16 = 1;

/// A joiner's offer, decrypted and checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The joiner's key for this join (ephemeral).
    pub from: PublicKey,
    pub session: [u8; 16],
    /// Always a `Signal::Offer` whose SDP is within `MAX_SDP`.
    pub signal: Signal,
    /// The joiner's router-mapped address, when it has one (IPv4).
    pub mapped: Option<SocketAddr>,
}

/// What an event in the host's inbox amounts to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inbound {
    /// A new session: answer it with [`Inbox::reply`].
    Offer(Request),
    /// a repeated session: publish this cached reply again
    Repeat(Event),
    /// Dropped, and why.
    Ignored(&'static str),
}

/// The decrypted content, both ways.
#[derive(Serialize, Deserialize)]
struct Wire {
    v: u16,
    session: String,
    signal: Signal,
    mapped: Option<String>,
}

/// The encrypted content, or `None` when it would exceed
/// [`MAX_HANDSHAKE_CONTENT`] (an SDP of `MAX_SDP` bytes fits unless JSON
/// escapes most of it).
fn seal(
    ck: &[u8; 32],
    session: &[u8; 16],
    signal: &Signal,
    mapped: Option<SocketAddr>,
    nonce: [u8; 32],
) -> Option<String> {
    let wire = Wire {
        v: WIRE_V,
        session: hex::encode(session),
        signal: signal.clone(),
        mapped: mapped.map(|a| a.to_string()),
    };
    let json = serde_json::to_string(&wire).ok()?;
    encrypt(ck, &json, nonce).ok().filter(|c| c.len() <= MAX_HANDSHAKE_CONTENT)
}

/// Ignored for honest redundancy, not counted: every relay's copy of an
/// event, and a resend before the reply.
const DUPLICATE: &str = "duplicate";
const BEING_ANSWERED: &str = "being answered";

/// Decrypts and parses a content: the session, the signal and an IPv4
/// mapped address; the signal's own checks are the caller's.
fn open(ck: &[u8; 32], content: &str) -> Result<([u8; 16], Signal, Option<SocketAddr>), &'static str> {
    let json = decrypt(ck, content).map_err(|_| "undecryptable")?;
    let wire: Wire = serde_json::from_str(&json).map_err(|_| "not a handshake")?;
    if wire.v != WIRE_V {
        return Err("unknown version");
    }
    let mut session = [0u8; 16];
    hex::decode_to_slice(&wire.session, &mut session).map_err(|_| "bad session")?;
    let mapped = match wire.mapped {
        None => None,
        Some(s) => match s.parse::<SocketAddr>() {
            Ok(a @ SocketAddr::V4(_)) => Some(a),
            _ => return Err("bad mapped address"),
        },
    };
    Ok((session, wire.signal, mapped))
}

fn tags_to(to: &PublicKey, now_unix: u64) -> Vec<Vec<String>> {
    vec![
        vec!["p".to_owned(), hex::encode(to)],
        vec!["expiration".to_owned(), now_unix.saturating_add(MAX_SKEW).to_string()],
    ]
}

/// The checks before any signature or decryption: size, kind, the `p` tag
/// and the date.
fn cheap(e: &Event, me: &str, now_unix: u64) -> Result<(), &'static str> {
    if e.content.len() > MAX_HANDSHAKE_CONTENT {
        return Err("too large");
    }
    if e.kind != KIND_HANDSHAKE {
        return Err("not a handshake");
    }
    if e.tag("p") != Some(me) {
        return Err("not for us");
    }
    if e.created_at.saturating_add(MAX_SKEW) < now_unix {
        return Err("too old");
    }
    if e.created_at > now_unix.saturating_add(MAX_SKEW) {
        return Err("in the future");
    }
    Ok(())
}

struct Session {
    session: [u8; 16],
    conversation: [u8; 32],
    /// The first offer, or the reply: memory counts from the later.
    at: Instant,
    reply: Option<Event>,
    ids: Vec<[u8; 32]>,
    repeats: u8,
}

/// The host's side: offers addressed to its key.
pub struct Inbox {
    me: Identity,
    me_hex: String,
    /// By the joiner's (ephemeral) key: one session per key.
    sessions: BTreeMap<PublicKey, Session>,
    /// The rate bucket, in microseconds of refill: a token is
    /// `1 s / OFFER_RATE`.
    tokens: u64,
    filled: Option<Instant>,
    ignored: u64,
}

const TOKEN: u64 = 1_000_000 / OFFER_RATE;

impl Inbox {
    pub fn new(me: Identity) -> Inbox {
        let me_hex = hex::encode(me.public());
        Inbox { me, me_hex, sessions: BTreeMap::new(), tokens: OFFER_BURST * TOKEN, filled: None, ignored: 0 }
    }

    /// The subscription: handshakes tagged with our key, since a minute ago.
    pub fn filter(&self, now_unix: u64) -> Filter {
        Filter {
            kinds: vec![KIND_HANDSHAKE],
            tags: vec![('p', vec![self.me_hex.clone()])],
            since: Some(now_unix.saturating_sub(MAX_SKEW)),
            ..Filter::default()
        }
    }

    /// How many sessions are remembered.
    pub fn sessions(&self) -> usize {
        self.sessions.len()
    }

    /// How many hostile or over-limit events were ignored (not the honest
    /// duplicates from several relays nor resends before the reply).
    pub fn ignored(&self) -> u64 {
        self.ignored
    }

    /// Takes an event from a relay (see the module docs for the order of
    /// the checks).
    pub fn offer(&mut self, e: &Event, now: Instant, now_unix: u64) -> Inbound {
        let got = self.check(e, now, now_unix);
        if let Inbound::Ignored(why) = got
            && why != DUPLICATE
            && why != BEING_ANSWERED
        {
            self.ignored += 1;
        }
        got
    }

    fn check(&mut self, e: &Event, now: Instant, now_unix: u64) -> Inbound {
        if let Err(why) = cheap(e, &self.me_hex, now_unix) {
            return Inbound::Ignored(why);
        }
        if let Some(s) = self.sessions.get(&e.pubkey)
            && now.saturating_duration_since(s.at) >= SESSION_MEMORY
        {
            self.sessions.remove(&e.pubkey);
        }
        if let Some(s) = self.sessions.get_mut(&e.pubkey) {
            // A known key: a duplicate (another relay, a replay) costs a
            // lookup; a resend a signature check and a decryption with the
            // remembered key.
            if s.ids.contains(&e.id) {
                return Inbound::Ignored(DUPLICATE);
            }
            if s.repeats >= MAX_REPEATS {
                return Inbound::Ignored("too many repeats");
            }
            if !e.verify() {
                return Inbound::Ignored("bad signature");
            }
            // Counted once signed by the joiner's key: a forgery naming the
            // key cannot use up the joiner's repeats.
            s.repeats += 1;
            let (session, _, _) = match open(&s.conversation, &e.content) {
                Ok(v) => v,
                Err(why) => return Inbound::Ignored(why),
            };
            if session != s.session {
                return Inbound::Ignored("another session");
            }
            if s.ids.len() >= MAX_IDS {
                s.ids.remove(0);
            }
            s.ids.push(e.id);
            let Some(reply) = &s.reply else { return Inbound::Ignored(BEING_ANSWERED) };
            return Inbound::Repeat(reply.clone());
        }
        if !e.verify() {
            return Inbound::Ignored("bad signature");
        }
        if !self.take_token(now) {
            return Inbound::Ignored("too many offers");
        }
        let Some(conversation) = conversation_key(&self.me.secret(), &e.pubkey) else {
            return Inbound::Ignored("bad key");
        };
        let (session, signal, mapped) = match open(&conversation, &e.content) {
            Ok(v) => v,
            Err(why) => return Inbound::Ignored(why),
        };
        match &signal {
            Signal::Offer { sdp, .. } if sdp.len() <= MAX_SDP => {}
            Signal::Offer { .. } => return Inbound::Ignored("SDP too large"),
            _ => return Inbound::Ignored("not an offer"),
        }
        self.remember(e, session, conversation, now);
        Inbound::Offer(Request { from: e.pubkey, session, signal, mapped })
    }

    /// One token of the bucket, refilled at `OFFER_RATE` a second up to
    /// `OFFER_BURST`.
    fn take_token(&mut self, now: Instant) -> bool {
        if let Some(at) = self.filled {
            let us = u64::try_from(now.saturating_duration_since(at).as_micros()).unwrap_or(u64::MAX);
            self.tokens = self.tokens.saturating_add(us).min(OFFER_BURST * TOKEN);
        }
        self.filled = Some(self.filled.map_or(now, |at| at.max(now)));
        if self.tokens < TOKEN {
            return false;
        }
        self.tokens -= TOKEN;
        true
    }

    /// Records a new session: forgets those past `SESSION_MEMORY`, then,
    /// when still full, the oldest.
    fn remember(&mut self, e: &Event, session: [u8; 16], conversation: [u8; 32], now: Instant) {
        self.sessions.retain(|_, s| now.saturating_duration_since(s.at) < SESSION_MEMORY);
        if self.sessions.len() >= MAX_SESSIONS
            && let Some(oldest) = self.sessions.iter().min_by_key(|(k, s)| (s.at, **k)).map(|(k, _)| *k)
        {
            self.sessions.remove(&oldest);
        }
        let s = Session { session, conversation, at: now, reply: None, ids: vec![e.id], repeats: 0 };
        self.sessions.insert(e.pubkey, s);
    }

    /// The reply to `req` (an `Answer` or `Refused`), signed with our key,
    /// and remembered for the session's repeats from `now` on. `rand`
    /// gives the nonce and the signature's randomness. A signal whose
    /// content would exceed [`MAX_HANDSHAKE_CONTENT`] is replaced by
    /// `Refused { reason: Malformed }`, so the joiner learns at once
    /// instead of waiting out its deadline.
    pub fn reply(
        &mut self,
        req: &Request,
        signal: Signal,
        mapped: Option<SocketAddr>,
        now: Instant,
        now_unix: u64,
        rand: &mut dyn FnMut() -> [u8; 32],
    ) -> Event {
        let conversation = match self.sessions.get(&req.from) {
            Some(s) if s.session == req.session => Some(s.conversation),
            _ => conversation_key(&self.me.secret(), &req.from),
        };
        let refused = Signal::Refused { reason: Refusal::Malformed };
        let content = conversation
            .and_then(|ck| {
                seal(&ck, &req.session, &signal, mapped, rand())
                    .or_else(|| seal(&ck, &req.session, &refused, None, rand()))
            })
            .unwrap_or_default();
        let e =
            Event::sign(&self.me, now_unix, KIND_HANDSHAKE, tags_to(&req.from, now_unix), content, rand());
        if let Some(s) = self.sessions.get_mut(&req.from)
            && s.session == req.session
        {
            s.reply = Some(e.clone());
            s.at = s.at.max(now);
        }
        e
    }
}

/// What the host replied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// `Signal::Answer` (SDP within `MAX_SDP`) or `Signal::Refused`.
    pub signal: Signal,
    /// The host's router-mapped address, when it has one (IPv4).
    pub mapped: Option<SocketAddr>,
}

/// A secret drawn from `rand`: a draw that is no valid secret (zero, or
/// not below the curve order) is drawn again, up to 16 draws. `None` after
/// that: the source is plainly broken, and a key derived from its output
/// would be the same for every join.
fn fresh_identity(rand: &mut dyn FnMut() -> [u8; 32]) -> Option<Identity> {
    (0..16).find_map(|_| Identity::from_secret(rand()))
}

/// The joiner's side: one offer to one host.
pub struct Asker {
    host: PublicKey,
    /// `None` when `rand` gave no valid secret in 16 draws.
    key: Option<Identity>,
    /// `None` when `host` is no valid key or there is no key of ours:
    /// nothing is sent.
    conversation: Option<[u8; 32]>,
    session: [u8; 16],
    offer: Signal,
    started: Instant,
    next: Option<Instant>,
    answered: bool,
}

impl Asker {
    /// A fresh ephemeral key from `rand` (a draw that is no valid secret, such as all zeros or ≥ n, is drawn again); `offer` is a Signal::Offer.
    ///
    /// After 16 invalid draws (a broken source) the asker sends nothing and
    /// expires at its deadline. An offer whose content would exceed
    /// [`MAX_HANDSHAKE_CONTENT`] is never sent either: callers keep SDPs
    /// within `MAX_SDP` and mostly unescaped (an SDP of `MAX_SDP` bytes in
    /// CRLF-ended lines fits). The joiner sends no `mapped` address: only
    /// the host's is useful to the other side (N47).
    pub fn new(
        host: PublicKey,
        offer: Signal,
        now: Instant,
        now_unix: u64,
        rand: &mut dyn FnMut() -> [u8; 32],
    ) -> Asker {
        let _ = now_unix;
        let key = fresh_identity(rand);
        let conversation = key.as_ref().and_then(|k| conversation_key(&k.secret(), &host));
        let mut session = [0u8; 16];
        session.copy_from_slice(&rand()[..16]);
        Asker { host, key, conversation, session, offer, started: now, next: None, answered: false }
    }

    /// The ephemeral key of this join (all zeros when there is none: a
    /// broken randomness source).
    pub fn key(&self) -> PublicKey {
        self.key.as_ref().map_or([0; 32], Identity::public)
    }

    /// The subscription: the host's handshakes tagged with our key.
    pub fn filter(&self, now_unix: u64) -> Filter {
        Filter {
            kinds: vec![KIND_HANDSHAKE],
            authors: vec![self.host],
            tags: vec![('p', vec![hex::encode(self.key())])],
            since: Some(now_unix.saturating_sub(MAX_SKEW)),
            limit: None,
        }
    }

    /// The offer event to publish now: at once, then every RESEND (a new event, the same session).
    pub fn poll(&mut self, now: Instant, now_unix: u64, rand: &mut dyn FnMut() -> [u8; 32]) -> Option<Event> {
        let (ck, key) = (self.conversation?, self.key.as_ref()?);
        if self.answered || self.expired(now) || self.next.is_some_and(|n| now < n) {
            return None;
        }
        self.next = Some(now + RESEND);
        let content = seal(&ck, &self.session, &self.offer, None, rand())?;
        Some(Event::sign(key, now_unix, KIND_HANDSHAKE, tags_to(&self.host, now_unix), content, rand()))
    }

    /// The host's reply to our session, decrypted: by the host, to our
    /// key, within the size limit, signed, and an `Answer` (SDP within
    /// `MAX_SDP`) or `Refused`. Taken once.
    pub fn take(&mut self, e: &Event) -> Option<Reply> {
        let ck = self.conversation?;
        if self.answered
            || e.pubkey != self.host
            || e.kind != KIND_HANDSHAKE
            || e.content.len() > MAX_HANDSHAKE_CONTENT
            || e.tag("p") != Some(hex::encode(self.key()).as_str())
            || !e.verify()
        {
            return None;
        }
        let (session, signal, mapped) = open(&ck, &e.content).ok()?;
        if session != self.session {
            return None;
        }
        match &signal {
            Signal::Answer { sdp } if sdp.len() <= MAX_SDP => {}
            Signal::Refused { .. } => {}
            _ => return None,
        }
        self.answered = true;
        Some(Reply { signal, mapped })
    }

    /// ANSWER_DEADLINE passed without a reply.
    pub fn expired(&self, now: Instant) -> bool {
        !self.answered && now.saturating_duration_since(self.started) >= ANSWER_DEADLINE
    }
}
