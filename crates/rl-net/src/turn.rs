//! TURN relays (decision N52, spec §5.3; ruling TR-2): our own sans-IO
//! relay core ([`TurnCore`], the UDP subset of RFC 8656 that our client
//! and browsers' WebRTC use) with its limits per client IP, and its socket
//! driver ([`TurnServer`], `driver.rs`).
//!
//! A volunteer relay must not become a free general proxy, nor a way into
//! its operator's network:
//! - every client IP gets a few allocations and about 32 KiB/s each way;
//!   over a limit the datagram is dropped and counted, never the client
//!   (a relay has no session to drop);
//! - peers must be global addresses, not the relay's own (TR-1), unless
//!   `allow_local` (N62: office and LAN setups, tests);
//! - every table is bounded and indexed; an unauthenticated source costs
//!   no memory (a stateless nonce), and only request sources get an IP
//!   entry.
//!
//! Supported: Binding; Allocate (REQUESTED-TRANSPORT UDP, LIFETIME,
//! DONT-FRAGMENT accepted and ignored); Refresh (0 deletes); CreatePermission;
//! ChannelBind; Send and Data indications; ChannelData. Long-term
//! credentials (realm [`REALM`], MESSAGE-INTEGRITY with HMAC-SHA1; our nonce
//! does not offer RFC 8489's SHA-256 upgrade, so clients use SHA-1).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use rl_proto::is_global;
use sha1::Sha1;

use self::msg::{Builder, Class, Msg};

mod driver;
mod msg;

pub use driver::{TurnServer, TurnServerConfig, TurnStats};

/// The realm of every rust-liero relay (N52: the credentials are public).
pub const REALM: &str = "rust-liero";

/// At most this many client IPs are tracked; a new one evicts the one
/// idle longest among those holding no allocation.
const MAX_IPS: usize = 4096;
/// A token bucket holds this many seconds of its rate.
const BURST_SECS: u128 = 2;
const NANOS: u128 = 1_000_000_000;
/// A request (anything but data) above this is dropped unread.
const MAX_REQUEST: usize = 1500;
/// Per allocation.
const MAX_PERMISSIONS: usize = 16;
const MAX_CHANNELS: usize = 16;
/// XOR-PEER-ADDRESS attributes one CreatePermission may carry.
const MAX_PEERS_PER_REQUEST: usize = 8;
/// RFC 8656 §2, §9, §12: lifetimes.
const PERMISSION_LIFETIME: Duration = Duration::from_secs(300);
const CHANNEL_LIFETIME: Duration = Duration::from_secs(600);
const DEFAULT_LIFETIME: u32 = 600;
const MAX_LIFETIME: u32 = 3600;
/// A nonce is valid in the period it was made in and the next.
const NONCE_PERIOD: u64 = 600;
/// The channel numbers RFC 5766 allows (RFC 8656 narrows them to 0x4fff;
/// clients built on 5766 use the whole range).
const CHANNELS: std::ops::RangeInclusive<u16> = 0x4000..=0x7fff;
/// Attributes every authenticated request may carry (USERNAME, MESSAGE-
/// INTEGRITY, REALM, NONCE, MESSAGE-INTEGRITY-SHA256, PASSWORD-ALGORITHM,
/// USERHASH).
const AUTH_ATTRIBUTES: [u16; 7] = [0x0006, 0x0008, 0x0014, 0x0015, 0x001c, 0x001d, 0x001e];

/// What a relay allows (N52).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnLimits {
    /// Allocations in all.
    pub capacity: usize,
    /// Allocations per client IP.
    pub per_ip_allocations: usize,
    /// Relayed payload bytes per second per client IP, each direction
    /// (a bucket holding 2 s).
    pub per_ip_bytes_per_s: u32,
    /// STUN/TURN requests per second per client IP (a bucket holding 2 s).
    pub per_ip_requests_per_s: u32,
}

impl Default for TurnLimits {
    /// 32 allocations, 4 per IP, 32 KiB/s each way, 20 requests/s (N52).
    fn default() -> TurnLimits {
        TurnLimits {
            capacity: 32,
            per_ip_allocations: 4,
            per_ip_bytes_per_s: 32 * 1024,
            per_ip_requests_per_s: 20,
        }
    }
}

impl TurnLimits {
    /// For tests that share 127.0.0.1: 256 allocations, 64 per IP, 64 MiB/s, 10 000 requests/s.
    pub fn generous() -> TurnLimits {
        TurnLimits {
            capacity: 256,
            per_ip_allocations: 64,
            per_ip_bytes_per_s: 64 * 1024 * 1024,
            per_ip_requests_per_s: 10_000,
        }
    }
}

/// One datagram for the driver to send from its port `from_port` (the
/// listening port or a relay port).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnOut {
    pub from_port: u16,
    pub to: SocketAddr,
    pub bytes: Vec<u8>,
}

/// The library asked for a relay socket for `client`: the driver picks the port and calls `allocated`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllocationRequest {
    pub client: SocketAddr,
    pub(crate) id: u64,
}

/// A token bucket of `rate` per second holding [`BURST_SECS`], in
/// billionths of a token; it starts full.
#[derive(Clone, Copy, Debug)]
struct Bucket {
    nanos: u128,
    at: Instant,
}

impl Bucket {
    fn full(rate: u32, now: Instant) -> Bucket {
        Bucket { nanos: BURST_SECS * u128::from(rate) * NANOS, at: now }
    }

    /// Takes `cost` tokens if there are that many.
    fn take(&mut self, rate: u32, cost: u64, now: Instant) -> bool {
        let cap = BURST_SECS * u128::from(rate) * NANOS;
        let elapsed = now.saturating_duration_since(self.at).as_nanos();
        self.at = self.at.max(now);
        self.nanos = self.nanos.saturating_add(elapsed.saturating_mul(u128::from(rate))).min(cap);
        let need = u128::from(cost) * NANOS;
        if self.nanos >= need {
            self.nanos -= need;
            true
        } else {
            false
        }
    }
}

/// What one client IP used.
struct IpState {
    /// Allocations, waiting for a port or bound.
    allocations: usize,
    /// Relayed payload: client to peers, and peers to the client.
    up: Bucket,
    down: Bucket,
    requests: Bucket,
    last: Instant,
}

/// The client IPs, and those holding no allocation by when they were last
/// seen (the eviction order).
#[derive(Default)]
struct Ips {
    states: BTreeMap<IpAddr, IpState>,
    idle: BTreeSet<(Instant, IpAddr)>,
}

impl Ips {
    /// The state of request source `ip`, seen `now`: made (evicting the
    /// longest idle one when full) if new; `None` when every tracked IP
    /// holds an allocation.
    fn touch(&mut self, ip: IpAddr, limits: &TurnLimits, now: Instant) -> Option<&mut IpState> {
        if !self.states.contains_key(&ip) {
            if self.states.len() >= MAX_IPS {
                let (_, oldest) = self.idle.pop_first()?;
                self.states.remove(&oldest);
            }
            self.states.insert(
                ip,
                IpState {
                    allocations: 0,
                    up: Bucket::full(limits.per_ip_bytes_per_s, now),
                    down: Bucket::full(limits.per_ip_bytes_per_s, now),
                    requests: Bucket::full(limits.per_ip_requests_per_s, now),
                    last: now,
                },
            );
            self.idle.insert((now, ip));
        }
        let state = self.states.get_mut(&ip)?;
        if state.last < now {
            if state.allocations == 0 {
                self.idle.remove(&(state.last, ip));
                self.idle.insert((now, ip));
            }
            state.last = now;
        }
        Some(state)
    }

    /// One allocation more (or less) for `ip`.
    fn count(&mut self, ip: IpAddr, more: bool) {
        let Some(state) = self.states.get_mut(&ip) else { return };
        if more {
            if state.allocations == 0 {
                self.idle.remove(&(state.last, ip));
            }
            state.allocations += 1;
        } else if state.allocations > 0 {
            state.allocations -= 1;
            if state.allocations == 0 {
                self.idle.insert((state.last, ip));
            }
        }
    }
}

/// An allocation waiting for the driver's port.
struct Pending {
    client: SocketAddr,
    transaction: [u8; 12],
    lifetime: u32,
}

/// A bound allocation of one client 5-tuple.
struct Allocation {
    port: u16,
    /// The Allocate that made it (a retransmission gets the same answer).
    transaction: [u8; 12],
    lifetime: u32,
    expires: Instant,
    /// Peer IPs and when their permission ends.
    permissions: BTreeMap<IpAddr, Instant>,
    /// Channel numbers, their peers and when they end; and back.
    channels: BTreeMap<u16, (SocketAddr, Instant)>,
    peers: BTreeMap<SocketAddr, u16>,
}

impl Allocation {
    fn prune(&mut self, now: Instant) {
        self.permissions.retain(|_, until| *until > now);
        let peers = &mut self.peers;
        self.channels.retain(|_, (peer, until)| {
            let keep = *until > now;
            if !keep {
                peers.remove(peer);
            }
            keep
        });
    }

    fn permitted(&self, ip: IpAddr, now: Instant) -> bool {
        self.permissions.get(&ip).is_some_and(|until| *until > now)
    }
}

/// An error answer: code and reason (RFC 8489 §14.8, RFC 8656 §18).
type Refusal = (u16, &'static str);
const BAD_REQUEST: Refusal = (400, "Bad Request");
const UNAUTHORIZED: Refusal = (401, "Unauthorized");
const FORBIDDEN: Refusal = (403, "Forbidden");
const ALLOCATION_MISMATCH: Refusal = (437, "Allocation Mismatch");
const STALE_NONCE: Refusal = (438, "Stale Nonce");
const FAMILY_NOT_SUPPORTED: Refusal = (440, "Address Family not Supported");
const UNSUPPORTED_TRANSPORT: Refusal = (442, "Unsupported Transport Protocol");
const PEER_FAMILY_MISMATCH: Refusal = (443, "Peer Address Family Mismatch");
const QUOTA_REACHED: Refusal = (486, "Allocation Quota Reached");
const INSUFFICIENT_CAPACITY: Refusal = (508, "Insufficient Capacity");

/// The relay without sockets (RFC 8656 over UDP): allocations, their
/// permissions and channels, and the limits per client IP (N52).
pub struct TurnCore {
    listen: SocketAddr,
    public: IpAddr,
    user: String,
    /// The long-term key, MD5(user ":" realm ":" pass) (RFC 8489 §9.2.2).
    key: [u8; 16],
    limits: TurnLimits,
    allow_local: bool,
    /// Signs nonces (stateless: an HMAC of the source and the period).
    secret: [u8; 32],
    epoch: Instant,
    ips: Ips,
    allocations: BTreeMap<SocketAddr, Allocation>,
    by_port: BTreeMap<u16, SocketAddr>,
    expiries: BTreeSet<(Instant, SocketAddr)>,
    pending: BTreeMap<u64, Pending>,
    pending_clients: BTreeMap<SocketAddr, u64>,
    requests: Vec<AllocationRequest>,
    next_id: u64,
    /// Data indications' transaction ids.
    next_indication: u64,
    released: Vec<u16>,
    out: Vec<TurnOut>,
    dropped: u64,
    relayed_bytes: u64,
}

impl TurnCore {
    /// Listens on `listen`; relayed addresses are `public` with the port the driver allocated.
    /// Peers must be global and not the relay's own addresses
    /// ([`TurnCore::set_allow_local`] lifts it); the nonce key comes from
    /// the system's randomness ([`TurnCore::set_secret`] sets it).
    pub fn new(
        listen: SocketAddr,
        public: IpAddr,
        user: &str,
        pass: &str,
        limits: TurnLimits,
        now: Instant,
    ) -> TurnCore {
        let mut key = [0u8; 16];
        key.copy_from_slice(&Md5::digest(format!("{user}:{REALM}:{pass}").as_bytes()));
        let secret = crate::random_bytes::<32>().unwrap_or_else(|_| {
            // Without system randomness: the clock, unpredictable enough
            // for nonces that only spare the relay a table.
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            let mut s = [0u8; 32];
            s[..16].copy_from_slice(&t.as_nanos().to_le_bytes());
            s
        });
        TurnCore {
            listen,
            public,
            user: user.to_owned(),
            key,
            limits,
            allow_local: false,
            secret,
            epoch: now,
            ips: Ips::default(),
            allocations: BTreeMap::new(),
            by_port: BTreeMap::new(),
            expiries: BTreeSet::new(),
            pending: BTreeMap::new(),
            pending_clients: BTreeMap::new(),
            requests: Vec::new(),
            next_id: 0,
            next_indication: 0,
            released: Vec::new(),
            out: Vec::new(),
            dropped: 0,
            relayed_bytes: 0,
        }
    }

    /// N62: relay to private, loopback and the relay's own addresses too
    /// (office and LAN setups, tests).
    pub fn set_allow_local(&mut self, allow: bool) {
        self.allow_local = allow;
    }

    /// The nonce key (a seeded one keeps a simulation's bytes repeatable).
    pub fn set_secret(&mut self, secret: [u8; 32]) {
        self.secret = secret;
    }

    /// Whether a peer at `ip` is refused (TR-1): IPv4 only, global and not
    /// ours, unless `allow_local`.
    fn forbidden(&self, ip: IpAddr) -> bool {
        if ip.is_unspecified() {
            return true;
        }
        !self.allow_local && (!is_global(ip) || ip == self.public || ip == self.listen.ip())
    }

    fn push(&mut self, from_port: u16, to: SocketAddr, bytes: Vec<u8>) {
        // Never to ourselves.
        if to != self.listen && !to.ip().is_unspecified() {
            self.out.push(TurnOut { from_port, to, bytes });
        }
    }

    /// The nonce for `from` in period `period`: the period and an HMAC of
    /// the source and the period, in hex.
    fn nonce(&self, from: SocketAddr, period: u64) -> String {
        let mut mac = Hmac::<Sha1>::new_from_slice(&self.secret).expect("HMAC takes any key");
        match from.ip() {
            IpAddr::V4(a) => mac.update(&a.octets()),
            IpAddr::V6(a) => mac.update(&a.octets()),
        }
        mac.update(&from.port().to_be_bytes());
        mac.update(&period.to_be_bytes());
        let tag = mac.finalize().into_bytes();
        let mut s = format!("{period:016x}");
        for b in &tag[..10] {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    fn period(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.epoch).as_secs() / NONCE_PERIOD
    }

    fn nonce_ok(&self, nonce: &[u8], from: SocketAddr, now: Instant) -> bool {
        let current = self.period(now);
        let Some(period) = nonce
            .get(..16)
            .and_then(|p| std::str::from_utf8(p).ok())
            .and_then(|p| u64::from_str_radix(p, 16).ok())
        else {
            return false;
        };
        (period == current || period + 1 == current) && self.nonce(from, period).as_bytes() == nonce
    }

    /// An error answer to request `m` from `from`: signed when the request
    /// was authenticated; 401 and 438 carry REALM and a fresh NONCE.
    fn refuse(&mut self, m: &Msg<'_>, from: SocketAddr, (code, reason): Refusal, signed: bool, now: Instant) {
        let mut b = Builder::new(Class::Error, m.method, m.transaction).error(code, reason);
        if code == 401 || code == 438 {
            let nonce = self.nonce(from, self.period(now));
            b = b.attr(msg::REALM, REALM.as_bytes()).attr(msg::NONCE, nonce.as_bytes());
        }
        if signed {
            b = b.integrity(&self.key);
        }
        let bytes = b.fingerprint().finish();
        self.push(self.listen.port(), from, bytes);
    }

    /// A signed success answer to `m` with `attrs` added.
    fn answer(&mut self, m: &Msg<'_>, from: SocketAddr, attrs: impl FnOnce(Builder) -> Builder) {
        let b = attrs(Builder::new(Class::Success, m.method, m.transaction));
        let bytes = b.integrity(&self.key).fingerprint().finish();
        self.push(self.listen.port(), from, bytes);
    }

    /// A datagram to the listening port.
    pub fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) {
        if bytes.first().is_some_and(|b| b & 0xc0 == 0x40) {
            return self.client_data(from, bytes, now);
        }
        let send = bytes.get(..2) == Some(&[0x00, 0x16]);
        if !send && bytes.len() > MAX_REQUEST {
            self.dropped += 1;
            return;
        }
        let Some(m) = Msg::parse(bytes) else { return };
        match m.class {
            Class::Indication if m.method == msg::SEND => self.send_indication(from, &m, now),
            Class::Request => {
                let limits = self.limits;
                let admitted = self
                    .ips
                    .touch(from.ip(), &limits, now)
                    .is_some_and(|s| s.requests.take(limits.per_ip_requests_per_s, 1, now));
                if !admitted {
                    self.dropped += 1;
                    return;
                }
                self.request(from, &m, now);
            }
            _ => {}
        }
    }

    fn request(&mut self, from: SocketAddr, m: &Msg<'_>, now: Instant) {
        let known: &[u16] = match m.method {
            msg::BINDING => {
                let bytes = Builder::new(Class::Success, msg::BINDING, m.transaction)
                    .xor_addr(msg::XOR_MAPPED_ADDRESS, from)
                    .fingerprint()
                    .finish();
                return self.push(self.listen.port(), from, bytes);
            }
            msg::ALLOCATE => {
                &[msg::REQUESTED_TRANSPORT, msg::LIFETIME, msg::DONT_FRAGMENT, msg::REQUESTED_ADDRESS_FAMILY]
            }
            msg::REFRESH => &[msg::LIFETIME, msg::REQUESTED_ADDRESS_FAMILY],
            msg::CREATE_PERMISSION => &[msg::XOR_PEER_ADDRESS],
            msg::CHANNEL_BIND => &[msg::CHANNEL_NUMBER, msg::XOR_PEER_ADDRESS],
            _ => return,
        };
        // Long-term credentials (RFC 8489 §9.2.4).
        if !m.has_integrity() {
            return self.refuse(m, from, UNAUTHORIZED, false, now);
        }
        let (Some(user), Some(realm), Some(nonce)) =
            (m.attr(msg::USERNAME), m.attr(msg::REALM), m.attr(msg::NONCE))
        else {
            return self.refuse(m, from, BAD_REQUEST, false, now);
        };
        if user != self.user.as_bytes() || realm != REALM.as_bytes() || !m.integrity_ok(&self.key) {
            return self.refuse(m, from, UNAUTHORIZED, false, now);
        }
        if !self.nonce_ok(nonce, from, now) {
            return self.refuse(m, from, STALE_NONCE, false, now);
        }
        let unknown: Vec<u16> =
            m.types().filter(|t| *t < 0x8000 && !known.contains(t) && !AUTH_ATTRIBUTES.contains(t)).collect();
        if !unknown.is_empty() {
            let list: Vec<u8> = unknown.iter().flat_map(|t| t.to_be_bytes()).collect();
            let bytes = Builder::new(Class::Error, m.method, m.transaction)
                .error(420, "Unknown Attribute")
                .attr(msg::UNKNOWN_ATTRIBUTES, &list)
                .integrity(&self.key)
                .fingerprint()
                .finish();
            return self.push(self.listen.port(), from, bytes);
        }
        let result = match m.method {
            msg::ALLOCATE => self.allocate(from, m, now),
            msg::REFRESH => self.refresh(from, m, now),
            msg::CREATE_PERMISSION => self.create_permission(from, m, now),
            _ => self.channel_bind(from, m, now),
        };
        if let Err(refusal) = result {
            self.refuse(m, from, refusal, true, now);
        }
    }

    fn allocate(&mut self, from: SocketAddr, m: &Msg<'_>, now: Instant) -> Result<(), Refusal> {
        if let Some(a) = self.allocations.get(&from) {
            if a.transaction != m.transaction {
                return Err(ALLOCATION_MISMATCH);
            }
            // A retransmission: the same answer.
            let (port, lifetime) = (a.port, a.lifetime);
            self.allocated_answer(m.transaction, from, port, lifetime);
            return Ok(());
        }
        if self.pending_clients.contains_key(&from) {
            return Ok(()); // Answered once the port comes.
        }
        let transport = m.attr(msg::REQUESTED_TRANSPORT).ok_or(BAD_REQUEST)?;
        if transport.first() != Some(&17) {
            return Err(UNSUPPORTED_TRANSPORT);
        }
        if m.attr(msg::REQUESTED_ADDRESS_FAMILY).is_some_and(|f| f.first() != Some(&0x01))
            || !self.public.is_ipv4()
        {
            return Err(FAMILY_NOT_SUPPORTED);
        }
        if self.allocations.len() + self.pending.len() >= self.limits.capacity {
            return Err(INSUFFICIENT_CAPACITY);
        }
        let limits = self.limits;
        let state = self.ips.touch(from.ip(), &limits, now).ok_or(INSUFFICIENT_CAPACITY)?;
        if state.allocations >= limits.per_ip_allocations {
            return Err(QUOTA_REACHED);
        }
        self.ips.count(from.ip(), true);
        let lifetime = lifetime(m).unwrap_or(DEFAULT_LIFETIME).clamp(DEFAULT_LIFETIME, MAX_LIFETIME);
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, Pending { client: from, transaction: m.transaction, lifetime });
        self.pending_clients.insert(from, id);
        self.requests.push(AllocationRequest { client: from, id });
        Ok(())
    }

    fn allocated_answer(&mut self, transaction: [u8; 12], client: SocketAddr, port: u16, lifetime: u32) {
        let relayed = SocketAddr::new(self.public, port);
        let bytes = Builder::new(Class::Success, msg::ALLOCATE, transaction)
            .xor_addr(msg::XOR_RELAYED_ADDRESS, relayed)
            .u32(msg::LIFETIME, lifetime)
            .xor_addr(msg::XOR_MAPPED_ADDRESS, client)
            .integrity(&self.key)
            .fingerprint()
            .finish();
        self.push(self.listen.port(), client, bytes);
    }

    fn refresh(&mut self, from: SocketAddr, m: &Msg<'_>, now: Instant) -> Result<(), Refusal> {
        if !self.allocations.contains_key(&from) {
            return Err(ALLOCATION_MISMATCH);
        }
        let requested = lifetime(m).unwrap_or(DEFAULT_LIFETIME);
        if requested == 0 {
            self.release(from);
            self.answer(m, from, |b| b.u32(msg::LIFETIME, 0));
            return Ok(());
        }
        let lifetime = requested.clamp(DEFAULT_LIFETIME, MAX_LIFETIME);
        if let Some(a) = self.allocations.get_mut(&from) {
            self.expiries.remove(&(a.expires, from));
            a.expires = now + Duration::from_secs(u64::from(lifetime));
            a.lifetime = lifetime;
            self.expiries.insert((a.expires, from));
        }
        self.answer(m, from, |b| b.u32(msg::LIFETIME, lifetime));
        Ok(())
    }

    /// The peer address of `m`, judged (RFC 8656 §9.2, §11.2).
    fn peer(&self, value: &[u8], m: &Msg<'_>) -> Result<SocketAddr, Refusal> {
        let peer = msg::decode_xor(value, &m.transaction).ok_or(BAD_REQUEST)?;
        if !peer.is_ipv4() {
            return Err(PEER_FAMILY_MISMATCH);
        }
        if self.forbidden(peer.ip()) {
            return Err(FORBIDDEN);
        }
        Ok(peer)
    }

    fn create_permission(&mut self, from: SocketAddr, m: &Msg<'_>, now: Instant) -> Result<(), Refusal> {
        let mut ips = BTreeSet::new();
        for value in m.attrs(msg::XOR_PEER_ADDRESS) {
            ips.insert(self.peer(value, m)?.ip());
            if ips.len() > MAX_PEERS_PER_REQUEST {
                return Err(BAD_REQUEST);
            }
        }
        if ips.is_empty() {
            return Err(BAD_REQUEST);
        }
        let a = self.allocations.get_mut(&from).ok_or(ALLOCATION_MISMATCH)?;
        a.prune(now);
        let new = ips.iter().filter(|ip| !a.permissions.contains_key(ip)).count();
        if a.permissions.len() + new > MAX_PERMISSIONS {
            return Err(INSUFFICIENT_CAPACITY);
        }
        for ip in ips {
            a.permissions.insert(ip, now + PERMISSION_LIFETIME);
        }
        self.answer(m, from, |b| b);
        Ok(())
    }

    fn channel_bind(&mut self, from: SocketAddr, m: &Msg<'_>, now: Instant) -> Result<(), Refusal> {
        let number = m.attr(msg::CHANNEL_NUMBER).filter(|v| v.len() == 4).ok_or(BAD_REQUEST)?;
        let number = u16::from_be_bytes([number[0], number[1]]);
        let peer = self.peer(m.attr(msg::XOR_PEER_ADDRESS).ok_or(BAD_REQUEST)?, m)?;
        if !CHANNELS.contains(&number) || peer.port() == 0 {
            return Err(BAD_REQUEST);
        }
        let a = self.allocations.get_mut(&from).ok_or(ALLOCATION_MISMATCH)?;
        a.prune(now);
        let bound = a.channels.get(&number).map(|(p, _)| *p);
        if bound.is_some_and(|p| p != peer) || a.peers.get(&peer).is_some_and(|n| *n != number) {
            return Err(BAD_REQUEST);
        }
        if bound.is_none() && a.channels.len() >= MAX_CHANNELS {
            return Err(INSUFFICIENT_CAPACITY);
        }
        if !a.permissions.contains_key(&peer.ip()) && a.permissions.len() >= MAX_PERMISSIONS {
            return Err(INSUFFICIENT_CAPACITY);
        }
        a.channels.insert(number, (peer, now + CHANNEL_LIFETIME));
        a.peers.insert(peer, number);
        a.permissions.insert(peer.ip(), now + PERMISSION_LIFETIME);
        self.answer(m, from, |b| b);
        Ok(())
    }

    /// Takes `bytes` from `ip`'s bucket, up (`true`) or down; counts a drop.
    fn charge(&mut self, ip: IpAddr, up: bool, bytes: usize, now: Instant) -> bool {
        let rate = self.limits.per_ip_bytes_per_s;
        let ok = self.ips.states.get_mut(&ip).is_some_and(|s| {
            let bucket = if up { &mut s.up } else { &mut s.down };
            bucket.take(rate, bytes as u64, now)
        });
        if !ok {
            self.dropped += 1;
        }
        ok
    }

    /// A Send indication: to a permitted, allowed peer from the allocation's port.
    fn send_indication(&mut self, from: SocketAddr, m: &Msg<'_>, now: Instant) {
        let (Some(peer), Some(data)) = (m.xor_addr(msg::XOR_PEER_ADDRESS), m.attr(msg::DATA_ATTR)) else {
            return;
        };
        self.relay_up(from, peer, data, now);
    }

    /// ChannelData from a client: to its channel's peer.
    fn client_data(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) {
        let Some((number, payload)) = msg::parse_channel_data(bytes) else { return };
        let Some(a) = self.allocations.get(&from) else { return };
        let Some(&(peer, until)) = a.channels.get(&number) else { return };
        if until > now {
            self.relay_up(from, peer, payload, now);
        }
    }

    fn relay_up(&mut self, from: SocketAddr, peer: SocketAddr, data: &[u8], now: Instant) {
        let Some(a) = self.allocations.get(&from) else { return };
        if !peer.is_ipv4() || peer.port() == 0 || self.forbidden(peer.ip()) || !a.permitted(peer.ip(), now) {
            return;
        }
        let port = a.port;
        if self.charge(from.ip(), true, data.len(), now) {
            self.relayed_bytes += data.len() as u64;
            self.push(port, peer, data.to_vec());
        }
    }

    /// A datagram to relay port `port` from a peer.
    pub fn receive_relayed(&mut self, port: u16, from: SocketAddr, bytes: &[u8], now: Instant) {
        let Some(&client) = self.by_port.get(&port) else { return };
        let Some(a) = self.allocations.get(&client) else { return };
        if !from.is_ipv4() || self.forbidden(from.ip()) || !a.permitted(from.ip(), now) {
            return;
        }
        let channel =
            a.peers.get(&from).copied().filter(|n| a.channels.get(n).is_some_and(|(_, t)| *t > now));
        // Charged only once the permission passed (review I2).
        if !self.charge(client.ip(), false, bytes.len(), now) {
            return;
        }
        let datagram = match channel {
            Some(n) => msg::channel_data(n, bytes),
            None => {
                self.next_indication += 1;
                let mut transaction = [0u8; 12];
                transaction[..8].copy_from_slice(&self.next_indication.to_be_bytes());
                Builder::new(Class::Indication, msg::DATA, transaction)
                    .xor_addr(msg::XOR_PEER_ADDRESS, from)
                    .attr(msg::DATA_ATTR, bytes)
                    .finish()
            }
        };
        self.relayed_bytes += bytes.len() as u64;
        self.push(self.listen.port(), client, datagram);
    }

    /// Ends allocations past their lifetime and hands out the datagrams to send.
    pub fn poll(&mut self, now: Instant, out: &mut Vec<TurnOut>) {
        while let Some(&(at, client)) = self.expiries.first() {
            if at > now {
                break;
            }
            self.release(client);
        }
        out.append(&mut self.out);
    }

    /// Ends `client`'s allocation: its port goes back to the driver.
    fn release(&mut self, client: SocketAddr) {
        let Some(a) = self.allocations.remove(&client) else { return };
        self.by_port.remove(&a.port);
        self.expiries.remove(&(a.expires, client));
        self.ips.count(client.ip(), false);
        self.released.push(a.port);
    }

    /// Allocations the limits admitted, waiting for a port (refused ones never appear: TurnCore answers them).
    pub fn take_allocation_requests(&mut self) -> Vec<AllocationRequest> {
        std::mem::take(&mut self.requests)
    }

    /// The driver's answer: `Some(port)` bound, `None` no port free (the client gets 508).
    pub fn allocated(&mut self, req: AllocationRequest, port: Option<u16>, now: Instant) {
        let Some(p) = self.pending.remove(&req.id) else { return };
        self.pending_clients.remove(&p.client);
        // A port already relaying is no answer (a driver's mistake).
        let port =
            port.filter(|port| !self.by_port.contains_key(port) && *port != self.listen.port() && *port != 0);
        let Some(port) = port else {
            self.ips.count(p.client.ip(), false);
            let bytes = Builder::new(Class::Error, msg::ALLOCATE, p.transaction)
                .error(INSUFFICIENT_CAPACITY.0, INSUFFICIENT_CAPACITY.1)
                .integrity(&self.key)
                .fingerprint()
                .finish();
            return self.push(self.listen.port(), p.client, bytes);
        };
        let expires = now + Duration::from_secs(u64::from(p.lifetime));
        self.allocations.insert(
            p.client,
            Allocation {
                port,
                transaction: p.transaction,
                lifetime: p.lifetime,
                expires,
                permissions: BTreeMap::new(),
                channels: BTreeMap::new(),
                peers: BTreeMap::new(),
            },
        );
        self.by_port.insert(port, p.client);
        self.expiries.insert((expires, p.client));
        self.allocated_answer(p.transaction, p.client, port, p.lifetime);
    }

    /// Relay ports whose allocation ended since the last call (the driver closes them).
    pub fn released(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.released)
    }

    /// Allocations bound to a port.
    pub fn allocations(&self) -> usize {
        self.allocations.len()
    }

    /// Client IPs tracked (at most 4096).
    pub fn tracked_ips(&self) -> usize {
        self.ips.states.len()
    }

    /// Datagrams over a limit (and requests too large to read).
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Payload bytes relayed, both directions.
    pub fn relayed_bytes(&self) -> u64 {
        self.relayed_bytes
    }
}

/// A request's LIFETIME in seconds.
fn lifetime(m: &Msg<'_>) -> Option<u32> {
    let v = m.attr(msg::LIFETIME)?;
    Some(u32::from_be_bytes(v.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn a_bucket_holds_two_seconds_and_refills() {
        let t0 = Instant::now();
        let mut b = Bucket::full(10, t0);
        assert!(b.take(10, 20, t0), "two seconds' worth at once");
        assert!(!b.take(10, 1, t0));
        assert!(!b.take(10, 6, t0 + Duration::from_millis(500)), "five refilled");
        assert!(b.take(10, 5, t0 + Duration::from_millis(500)));
        // Never above two seconds' worth.
        assert!(!b.take(10, 21, t0 + Duration::from_secs(100)));
        assert!(b.take(10, 20, t0 + Duration::from_secs(100)));
    }

    #[test]
    fn the_ip_table_is_bounded_and_keeps_allocating_ips() {
        let t0 = Instant::now();
        let limits = TurnLimits::default();
        let mut ips = Ips::default();
        let holder: IpAddr = Ipv4Addr::new(10, 0, 0, 1).into();
        ips.touch(holder, &limits, t0);
        ips.count(holder, true);
        for i in 0..MAX_IPS as u32 + 10 {
            let ip = IpAddr::V4(Ipv4Addr::from(0x0b00_0000 + i));
            ips.touch(ip, &limits, t0 + Duration::from_millis(u64::from(i) + 1));
        }
        assert_eq!(ips.states.len(), MAX_IPS);
        assert_eq!(ips.idle.len(), MAX_IPS - 1, "the index holds every idle IP");
        assert!(ips.states.contains_key(&holder), "an IP holding an allocation stays");
        assert!(
            !ips.states.contains_key(&IpAddr::V4(Ipv4Addr::from(0x0b00_0000))),
            "the oldest idle one went"
        );
        ips.count(holder, false);
        assert!(ips.idle.contains(&(t0, holder)), "idle again");
    }

    #[test]
    fn nonces_are_bound_to_the_source_and_expire() {
        let t0 = Instant::now();
        let a: SocketAddr = "11.0.0.20:50000".parse().unwrap();
        let core =
            TurnCore::new("11.0.0.9:3478".parse().unwrap(), a.ip(), "u", "p", TurnLimits::default(), t0);
        let n = core.nonce(a, 0);
        assert!(core.nonce_ok(n.as_bytes(), a, t0));
        assert!(
            core.nonce_ok(n.as_bytes(), a, t0 + Duration::from_secs(NONCE_PERIOD + 1)),
            "the next period"
        );
        assert!(!core.nonce_ok(n.as_bytes(), a, t0 + Duration::from_secs(2 * NONCE_PERIOD)), "stale");
        assert!(!core.nonce_ok(n.as_bytes(), "11.0.0.20:50001".parse().unwrap(), t0), "another source");
        assert!(!core.nonce_ok(b"zz", a, t0));
    }
}
