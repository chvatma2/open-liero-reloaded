//! LAN announcements (networking spec §5.1): a host sends one UDP datagram
//! every 2 s to the broadcast address on `LAN_PORT`; a [`Listener`] collects
//! them into a list of games. Both take the time as an argument.
//!
//! Limits: undecodable datagrams are ignored silently; one entry per source
//! address, at most [`MAX_PER_IP`] per source IP and [`MAX_GAMES`] in all (a
//! full list evicts the oldest entry of the IP holding most, so a hostile
//! host rotating ports cannot hide real games); a source IP sending more
//! than [`MAX_PER_SECOND`] datagrams in a second is ignored for [`PENALTY`].
//!
//! Known limits of std-only sockets: the listener binds without
//! `SO_REUSEADDR` (std cannot set it before `bind`), so two listeners on one
//! machine conflict (the game opens only one); directed per-interface
//! broadcasts need interface enumeration, which is deferred
//! ([`broadcast_targets`] returns the limited broadcast only).

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use olr_proto::{LAN_PORT, LanAnnounce, MAX_LAN, decode_lan, encode_lan};

/// Time between announcements.
pub const INTERVAL: Duration = Duration::from_secs(2);
/// A game unseen for this long is dropped.
pub const EXPIRY: Duration = Duration::from_secs(6);
/// Most games kept.
pub const MAX_GAMES: usize = 64;
/// Most games one source IP may hold.
pub const MAX_PER_IP: usize = 4;
/// Datagrams per second one source IP may send.
pub const MAX_PER_SECOND: u32 = 10;
/// How long a source over the rate is ignored.
pub const PENALTY: Duration = Duration::from_secs(10);
/// Datagrams read per `poll`.
const MAX_DRAIN: usize = 256;
/// Source IPs tracked for rate limiting (the oldest is evicted beyond).
const MAX_TRACKED: usize = 1024;

/// Sends the host's announcement at most every 2 s.
pub struct Announcer {
    socket: UdpSocket,
    target: SocketAddr,
    last: Option<Instant>,
}

impl Announcer {
    /// `target` is the broadcast address and `LAN_PORT` (see
    /// [`broadcast_targets`]).
    pub fn new(target: SocketAddr) -> io::Result<Announcer> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_nonblocking(true)?;
        socket.set_broadcast(true)?;
        Ok(Announcer { socket, target, last: None })
    }

    /// Sends `what` if 2 s have passed since the last send. Send errors
    /// (no route, no network) are ignored: the next tick tries again.
    pub fn tick(&mut self, now: Instant, what: &LanAnnounce) {
        if self.last.is_some_and(|t| now.saturating_duration_since(t) < INTERVAL) {
            return;
        }
        self.last = Some(now);
        let _ = self.socket.send_to(&encode_lan(what), self.target);
    }
}

/// A game seen on the LAN.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanGame {
    /// The datagram's source IP with the announced port.
    pub addr: SocketAddr,
    pub info: LanAnnounce,
    pub seen: Instant,
}

struct Source {
    window_start: Instant,
    count: u32,
    ignored_until: Option<Instant>,
}

/// Collects announcements.
pub struct Listener {
    socket: UdpSocket,
    /// Sorted by name then address.
    games: Vec<LanGame>,
    /// The source address each game came from, parallel to `games`.
    sources: Vec<SocketAddr>,
    rates: BTreeMap<IpAddr, Source>,
}

impl Listener {
    /// Binds `0.0.0.0:port` (0 picks a free port).
    pub fn new(port: u16) -> io::Result<Listener> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
        socket.set_nonblocking(true)?;
        Ok(Listener { socket, games: Vec::new(), sources: Vec::new(), rates: BTreeMap::new() })
    }

    /// The bound port.
    pub fn local_port(&self) -> u16 {
        self.socket.local_addr().map_or(0, |a| a.port())
    }

    /// Never blocks: reads at most 256 datagrams, drops games unseen for 6 s
    /// and returns the rest sorted by name, then address.
    pub fn poll(&mut self, now: Instant) -> &[LanGame] {
        let mut buf = [0u8; MAX_LAN + 1];
        for _ in 0..MAX_DRAIN {
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) => self.receive(now, from, &buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // e.g. ICMP-induced resets on some platforms: skip it.
                Err(_) => {}
            }
        }
        let mut i = 0;
        while i < self.games.len() {
            if now.saturating_duration_since(self.games[i].seen) >= EXPIRY {
                self.games.remove(i);
                self.sources.remove(i);
            } else {
                i += 1;
            }
        }
        self.rates.retain(|_, s| {
            s.ignored_until.is_some_and(|u| now < u)
                || now.saturating_duration_since(s.window_start) < Duration::from_secs(1)
        });
        &self.games
    }

    fn receive(&mut self, now: Instant, from: SocketAddr, bytes: &[u8]) {
        if !self.admit(now, from.ip()) {
            return;
        }
        let Ok(info) = decode_lan(bytes) else { return };
        let game = LanGame { addr: SocketAddr::new(from.ip(), info.port), info, seen: now };
        if let Some(i) = self.sources.iter().position(|s| *s == from) {
            self.remove(i);
        } else {
            let own: Vec<usize> =
                (0..self.sources.len()).filter(|&i| self.sources[i].ip() == from.ip()).collect();
            if own.len() >= MAX_PER_IP {
                let oldest = own.into_iter().min_by_key(|&i| self.games[i].seen);
                if let Some(i) = oldest {
                    self.remove(i);
                }
            } else if self.games.len() >= MAX_GAMES {
                // Evict from the IP holding most games, its oldest entry.
                let count = |ip: IpAddr| self.sources.iter().filter(|s| s.ip() == ip).count();
                let victim = (0..self.sources.len())
                    .max_by_key(|&i| (count(self.sources[i].ip()), std::cmp::Reverse(self.games[i].seen)));
                if let Some(i) = victim {
                    self.remove(i);
                }
            }
        }
        let at = self.games.partition_point(|g| (&g.info.name, g.addr) < (&game.info.name, game.addr));
        self.games.insert(at, game);
        self.sources.insert(at, from);
    }

    fn remove(&mut self, i: usize) {
        self.games.remove(i);
        self.sources.remove(i);
    }

    /// Rate limit per source IP; false while the source is ignored.
    fn admit(&mut self, now: Instant, from: IpAddr) -> bool {
        if !self.rates.contains_key(&from) && self.rates.len() >= MAX_TRACKED {
            let oldest = self.rates.iter().min_by_key(|(_, s)| s.window_start).map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.rates.remove(&k);
            }
        }
        let s = self.rates.entry(from).or_insert(Source { window_start: now, count: 0, ignored_until: None });
        if let Some(until) = s.ignored_until {
            if now < until {
                return false;
            }
            s.ignored_until = None;
            s.window_start = now;
            s.count = 0;
        }
        if now.saturating_duration_since(s.window_start) >= Duration::from_secs(1) {
            s.window_start = now;
            s.count = 0;
        }
        s.count += 1;
        if s.count > MAX_PER_SECOND {
            s.ignored_until = Some(now + PENALTY);
            return false;
        }
        true
    }
}

/// Where a host announces: the limited broadcast `255.255.255.255:LAN_PORT`.
/// Directed per-interface broadcasts need interface enumeration (no std
/// API; deferred), so a host on a machine with several interfaces reaches
/// the one the OS routes the limited broadcast to.
pub fn broadcast_targets() -> Vec<SocketAddr> {
    vec![SocketAddr::from((Ipv4Addr::BROADCAST, LAN_PORT))]
}
