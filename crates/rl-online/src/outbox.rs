//! Reliable sends never dropped: everything for the reliable channel waits
//! in a per-peer FIFO backlog, fed into `rl-net`'s endpoint while it holds
//! less than [`FEED_BELOW`] for that peer, so the endpoint's own limit
//! (`MAX_QUEUED`, fatal for the link) is never reached. A peer whose
//! backlog would pass [`MAX_BACKLOG`] (it does not read) is closed and
//! reported ([`Outbox::take_overflowed`]).

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::time::Instant;

use rl_net::endpoint::{Endpoint, NetEvent};
use rl_netcode::{Channel, PeerId};

/// The backlog is fed into the endpoint while it holds less than this for
/// the peer (reliable bytes queued and unacknowledged).
pub const FEED_BELOW: usize = 1024 * 1024;
/// Room in a peer's backlog beyond one whole state transfer: lobby states
/// (60 KiB at most each) and the match's control messages meanwhile.
pub const LOBBY_HEADROOM: usize = 8 * 1024 * 1024;
/// The most a peer's backlog holds: one state transfer of
/// [`rl_proto::MAX_STATE`] plus [`LOBBY_HEADROOM`]. Beyond it the peer
/// does not read: it is closed.
pub const MAX_BACKLOG: usize = rl_proto::MAX_STATE + LOBBY_HEADROOM;

/// The endpoint and the reliable messages waiting to enter it.
pub struct Outbox {
    endpoint: Endpoint,
    backlog: BTreeMap<PeerId, Backlog>,
    overflowed: Vec<PeerId>,
}

/// One peer's reliable messages not yet in the endpoint, oldest first.
#[derive(Default)]
struct Backlog {
    queue: VecDeque<Vec<u8>>,
    bytes: usize,
}

impl Outbox {
    pub fn new(endpoint: Endpoint) -> Outbox {
        Outbox { endpoint, backlog: BTreeMap::new(), overflowed: Vec::new() }
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn endpoint_mut(&mut self) -> &mut Endpoint {
        &mut self.endpoint
    }

    /// One message to `peer` (ignored for a peer not open): unreliable ones
    /// go to the endpoint (lost when it is full, as on the network),
    /// reliable ones through the backlog.
    pub fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        match channel {
            Channel::Unreliable => self.endpoint.send(peer, channel, bytes),
            Channel::Reliable => self.push(peer, bytes.to_vec()),
        }
    }

    /// One reliable message, `tag` first.
    pub fn send_tagged(&mut self, peer: PeerId, tag: u8, bytes: &[u8]) {
        let mut v = Vec::with_capacity(bytes.len() + 1);
        v.push(tag);
        v.extend_from_slice(bytes);
        self.push(peer, v);
    }

    fn push(&mut self, peer: PeerId, message: Vec<u8>) {
        if self.endpoint.stats(peer).is_none() {
            return;
        }
        let b = self.backlog.entry(peer).or_default();
        if b.bytes + message.len() > MAX_BACKLOG {
            self.backlog.remove(&peer);
            self.endpoint.close(peer);
            self.overflowed.push(peer);
            return;
        }
        b.bytes += message.len();
        b.queue.push_back(message);
        self.feed(peer);
    }

    /// Moves `peer`'s backlog into the endpoint while it holds less than
    /// [`FEED_BELOW`] for it.
    fn feed(&mut self, peer: PeerId) {
        let Some(b) = self.backlog.get_mut(&peer) else { return };
        while self.endpoint.buffered(peer, Channel::Reliable) < FEED_BELOW
            && let Some(message) = b.queue.pop_front()
        {
            b.bytes -= message.len();
            self.endpoint.send(peer, Channel::Reliable, &message);
        }
        if b.queue.is_empty() {
            self.backlog.remove(&peer);
        }
    }

    fn feed_all(&mut self) {
        let peers: Vec<PeerId> = self.backlog.keys().copied().collect();
        for peer in peers {
            self.feed(peer);
        }
    }

    /// Feeds the backlogs, polls the endpoint, feeds again (what the poll
    /// delivered made room). A disconnected peer's backlog is dropped.
    pub fn poll(&mut self, now: Instant, events: &mut Vec<NetEvent>) {
        self.feed_all();
        self.endpoint.poll(now, events);
        for e in events.iter() {
            if let NetEvent::Disconnected { peer, .. } = e {
                self.backlog.remove(peer);
            }
        }
        self.feed_all();
    }

    /// The reliable bytes waiting for `peer`: the backlog and the endpoint's.
    pub fn buffered(&self, peer: PeerId) -> usize {
        self.backlog.get(&peer).map_or(0, |b| b.bytes) + self.endpoint.buffered(peer, Channel::Reliable)
    }

    /// Closes the link to `peer` and drops its backlog.
    pub fn close(&mut self, peer: PeerId) {
        self.backlog.remove(&peer);
        self.endpoint.close(peer);
    }

    /// Peers closed because their backlog would have passed
    /// [`MAX_BACKLOG`], since the last call.
    pub fn take_overflowed(&mut self) -> Vec<PeerId> {
        std::mem::take(&mut self.overflowed)
    }

    pub fn peers(&self) -> impl Iterator<Item = (PeerId, SocketAddr)> + '_ {
        self.endpoint.peers()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rl_net::endpoint::{EndpointConfig, HOST};

    use super::*;

    /// A host's outbox and one client endpoint, connected over 127.0.0.1.
    fn pair() -> (Outbox, PeerId, Endpoint) {
        let cfg = EndpointConfig { port: 0, ..EndpointConfig::default() };
        let host = Endpoint::host(cfg.clone(), Box::new(|_, _| Ok(()))).unwrap();
        let addr = SocketAddr::from(([127, 0, 0, 1], host.local_port()));
        let mut client = Endpoint::connect(cfg, addr).unwrap();
        let mut host = Outbox::new(host);
        let (mut he, mut ce) = (Vec::new(), Vec::new());
        let begun = Instant::now();
        loop {
            assert!(begun.elapsed() < Duration::from_secs(10), "connected within 10 s");
            host.poll(Instant::now(), &mut he);
            client.poll(Instant::now(), &mut ce);
            let peer = he.iter().find_map(|e| match e {
                NetEvent::Connected { peer, .. } => Some(*peer),
                _ => None,
            });
            if let Some(peer) = peer
                && ce.iter().any(|e| matches!(e, NetEvent::Connected { .. }))
            {
                return (host, peer, client);
            }
            std::thread::sleep(Duration::from_micros(300));
        }
    }

    /// A 10 MB state transfer (well past the endpoint's 4 MiB) arrives
    /// whole and in order.
    #[test]
    fn a_10_mb_transfer_arrives_whole() {
        const BYTES: usize = 10 * 1000 * 1000;
        let (mut host, peer, mut client) = pair();
        client.allow_burst(HOST, 2 * BYTES);
        host.endpoint_mut().allow_burst(peer, 2 * BYTES);
        let state: Vec<u8> = (0..BYTES as u32).map(|i| (i * 7 % 251) as u8).collect();
        for chunk in state.chunks(rl_proto::STATE_CHUNK) {
            host.send_tagged(peer, 0, chunk);
        }
        let (mut he, mut ce) = (Vec::new(), Vec::new());
        let mut got = Vec::with_capacity(BYTES);
        let begun = Instant::now();
        while got.len() < BYTES {
            assert!(begun.elapsed() < Duration::from_secs(30), "10 MB within 30 s: {} bytes", got.len());
            host.poll(Instant::now(), &mut he);
            client.poll(Instant::now(), &mut ce);
            for e in ce.drain(..) {
                match e {
                    NetEvent::Message { channel: Channel::Reliable, bytes, .. } => {
                        assert_eq!(bytes[0], 0);
                        got.extend_from_slice(&bytes[1..]);
                    }
                    other => panic!("client: {other:?}"),
                }
            }
            assert!(he.is_empty(), "host: {he:?}");
            std::thread::sleep(Duration::from_micros(200));
        }
        assert!(got == state, "the state arrives whole and in order");
        assert_eq!(host.endpoint().stats(peer).unwrap().sends_dropped, 0);
        while host.buffered(peer) > 0 {
            assert!(begun.elapsed() < Duration::from_secs(30), "acknowledged within 30 s");
            host.poll(Instant::now(), &mut he);
            client.poll(Instant::now(), &mut ce);
            std::thread::sleep(Duration::from_micros(200));
        }
        assert!(he.is_empty() && ce.is_empty(), "{he:?} {ce:?}");
    }

    /// A peer that never reads is closed once its backlog passes
    /// [`MAX_BACKLOG`], and reported once.
    #[test]
    fn a_peer_that_never_reads_is_closed() {
        let (mut host, peer, _client) = pair();
        let message = vec![9u8; 60_000];
        let mut events = Vec::new();
        // The endpoint takes up to FEED_BELOW (plus a message) first.
        for _ in 0..(MAX_BACKLOG + FEED_BELOW) / message.len() + 3 {
            host.send(peer, Channel::Reliable, &message);
        }
        host.poll(Instant::now(), &mut events);
        assert_eq!(host.take_overflowed(), [peer]);
        assert!(host.take_overflowed().is_empty(), "reported once");
        assert!(host.peers().next().is_none(), "the link is closed");
        assert_eq!(host.buffered(peer), 0, "its backlog is gone");
        assert!(events.is_empty(), "a local close reports nothing: {events:?}");
        // Sends to it are ignored now.
        host.send(peer, Channel::Reliable, &message);
        assert_eq!(host.buffered(peer), 0);
        assert!(host.take_overflowed().is_empty());
    }
}
