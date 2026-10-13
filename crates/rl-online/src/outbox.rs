//! Reliable sends never dropped: everything for the reliable channel waits
//! in a per-peer FIFO backlog, fed into `rl-net`'s endpoint while it holds
//! less than [`FEED_BELOW`] for that peer, so the endpoint's own limit
//! (`MAX_QUEUED`, fatal for the link) is never reached. A peer whose
//! backlog of ordinary messages would pass [`MAX_BACKLOG`] (it does not
//! read) is closed and reported ([`Outbox::take_overflowed`]). One state
//! transfer may pass it: its messages, announced by its `StateBegin`, are
//! counted apart while that transfer is outstanding (one at a time).

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::time::Instant;

use rl_net::endpoint::{Endpoint, NetEvent};
use rl_netcode::{Channel, PeerId};

/// The backlog is fed into the endpoint while it holds less than this for
/// the peer (reliable bytes queued and unacknowledged).
pub const FEED_BELOW: usize = 1024 * 1024;
/// The most a peer's backlog holds of ordinary messages (lobby states, 60
/// KiB at most each, and the match's control messages). Beyond it the peer
/// does not read: it is closed. With 48 peers that is 384 MiB at most, plus
/// the state transfers outstanding.
pub const MAX_BACKLOG: usize = 8 * 1024 * 1024;
/// The most a state transfer holds in a backlog: [`rl_proto::MAX_STATE`]
/// and each message's framing (tag, variant, index, length).
pub const MAX_STATE_BACKLOG: usize =
    rl_proto::MAX_STATE + (rl_proto::MAX_STATE / rl_proto::STATE_CHUNK + 1) * 16;

/// What a reliable message is to its peer's bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// An ordinary message: at most [`MAX_BACKLOG`] of them wait.
    Message,
    /// A state transfer's first message, announcing `chunks` more. Refused
    /// (the peer closed) while another transfer to the peer is outstanding.
    StateBegin { chunks: usize },
    /// One of the chunks a `StateBegin` announced (an ordinary message
    /// when none is announced).
    StateChunk,
}

/// The endpoint and the reliable messages waiting to enter it.
pub struct Outbox {
    endpoint: Endpoint,
    backlog: BTreeMap<PeerId, Backlog>,
    overflowed: Vec<PeerId>,
}

/// One peer's reliable messages not yet in the endpoint, oldest first (each
/// with whether it belongs to the state transfer).
#[derive(Default)]
struct Backlog {
    queue: VecDeque<(Vec<u8>, bool)>,
    /// Bytes of ordinary messages queued.
    ordinary: usize,
    /// Bytes of the state transfer queued.
    state: usize,
    /// Chunks of the state transfer not yet sent to the backlog.
    chunks_left: usize,
}

impl Backlog {
    /// A state transfer is outstanding: announced chunks to come, or its
    /// messages still queued.
    fn transfer(&self) -> bool {
        self.chunks_left > 0 || self.state > 0
    }
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
            Channel::Reliable => self.push(peer, bytes.to_vec(), Part::Message),
        }
    }

    /// One ordinary reliable message, `tag` first.
    pub fn send_tagged(&mut self, peer: PeerId, tag: u8, bytes: &[u8]) {
        self.send_part(peer, tag, bytes, Part::Message);
    }

    /// One reliable message, `tag` first, as `part` of what goes to `peer`.
    pub fn send_part(&mut self, peer: PeerId, tag: u8, bytes: &[u8], part: Part) {
        let mut v = Vec::with_capacity(bytes.len() + 1);
        v.push(tag);
        v.extend_from_slice(bytes);
        self.push(peer, v, part);
    }

    fn push(&mut self, peer: PeerId, message: Vec<u8>, part: Part) {
        if self.endpoint.stats(peer).is_none() {
            return;
        }
        let b = self.backlog.entry(peer).or_default();
        let len = message.len();
        let state = match part {
            Part::Message => false,
            Part::StateBegin { chunks } if !b.transfer() => {
                b.chunks_left = chunks;
                true
            }
            // One state at a time: the peer has not taken the last one.
            Part::StateBegin { .. } => return self.overflow(peer),
            Part::StateChunk if b.chunks_left > 0 => {
                b.chunks_left -= 1;
                true
            }
            Part::StateChunk => false,
        };
        let fits = if state { b.state + len <= MAX_STATE_BACKLOG } else { b.ordinary + len <= MAX_BACKLOG };
        if !fits {
            return self.overflow(peer);
        }
        if state {
            b.state += len;
        } else {
            b.ordinary += len;
        }
        b.queue.push_back((message, state));
        self.feed(peer);
    }

    /// `peer` does not read: closed, its backlog dropped, reported once.
    fn overflow(&mut self, peer: PeerId) {
        self.backlog.remove(&peer);
        self.endpoint.close(peer);
        self.overflowed.push(peer);
    }

    /// Moves `peer`'s backlog into the endpoint while it holds less than
    /// [`FEED_BELOW`] for it.
    fn feed(&mut self, peer: PeerId) {
        let Some(b) = self.backlog.get_mut(&peer) else { return };
        while self.endpoint.buffered(peer, Channel::Reliable) < FEED_BELOW
            && let Some((message, state)) = b.queue.pop_front()
        {
            if state {
                b.state -= message.len();
            } else {
                b.ordinary -= message.len();
            }
            self.endpoint.send(peer, Channel::Reliable, &message);
        }
        if b.queue.is_empty() && b.chunks_left == 0 {
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
        self.backlog.get(&peer).map_or(0, |b| b.ordinary + b.state)
            + self.endpoint.buffered(peer, Channel::Reliable)
    }

    /// Closes the link to `peer` and drops its backlog.
    pub fn close(&mut self, peer: PeerId) {
        self.backlog.remove(&peer);
        self.endpoint.close(peer);
    }

    /// Peers closed because their backlog would have passed
    /// [`MAX_BACKLOG`] (or a second state was sent while one was
    /// outstanding), since the last call.
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

    const MIB: usize = 1024 * 1024;

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

    /// `state` as the session sends it: `StateBegin`, then its chunks.
    fn send_state(host: &mut Outbox, peer: PeerId, state: &[u8]) {
        let chunks = state.len().div_ceil(rl_proto::STATE_CHUNK);
        host.send_part(peer, 0, &[0xbe], Part::StateBegin { chunks });
        for chunk in state.chunks(rl_proto::STATE_CHUNK) {
            host.send_part(peer, 0, chunk, Part::StateChunk);
        }
    }

    /// A 10 MB state transfer (past the endpoint's 4 MiB and the backlog's
    /// bound for ordinary messages) arrives whole and in order.
    #[test]
    fn a_10_mb_state_arrives_whole() {
        const BYTES: usize = 10 * 1000 * 1000;
        let (mut host, peer, mut client) = pair();
        client.allow_burst(HOST, 2 * BYTES);
        host.endpoint_mut().allow_burst(peer, 2 * BYTES);
        let state: Vec<u8> = (0..BYTES as u32).map(|i| (i * 7 % 251) as u8).collect();
        // Lobby states meanwhile, before and after it.
        host.send_tagged(peer, 1, &[1; 60_000]);
        send_state(&mut host, peer, &state);
        host.send_tagged(peer, 1, &[2; 60_000]);
        assert!(host.buffered(peer) > 8 * MIB, "the state passes the ordinary bound");
        let (mut he, mut ce) = (Vec::new(), Vec::new());
        let mut got = Vec::with_capacity(BYTES);
        let mut lobby = Vec::new();
        let begun = Instant::now();
        while lobby.len() < 2 {
            assert!(begun.elapsed() < Duration::from_secs(30), "10 MB within 30 s: {} bytes", got.len());
            host.poll(Instant::now(), &mut he);
            client.poll(Instant::now(), &mut ce);
            for e in ce.drain(..) {
                match e {
                    NetEvent::Message { channel: Channel::Reliable, bytes, .. } if bytes[0] == 0 => {
                        assert!(lobby.len() == 1, "the state between the lobby messages");
                        if bytes[1..] != [0xbe] {
                            got.extend_from_slice(&bytes[1..]);
                        }
                    }
                    NetEvent::Message { channel: Channel::Reliable, bytes, .. } => lobby.push(bytes[1]),
                    other => panic!("client: {other:?}"),
                }
            }
            assert!(he.is_empty(), "host: {he:?}");
            assert!(host.take_overflowed().is_empty(), "nothing overflows");
            std::thread::sleep(Duration::from_micros(200));
        }
        assert_eq!(lobby, [1, 2]);
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

    /// A peer that never reads, flooded with lobby states, is closed once
    /// its backlog passes [`MAX_BACKLOG`] (8 MiB), and reported once.
    #[test]
    fn a_peer_that_never_reads_is_closed_at_8_mib() {
        let (mut host, peer, _client) = pair();
        let message = vec![9u8; 60_000];
        let mut events = Vec::new();
        let mut sent = 0;
        // The endpoint takes up to FEED_BELOW (plus a message) first.
        while host.take_overflowed().is_empty() {
            assert!(sent <= 8 * MIB + FEED_BELOW + 2 * message.len(), "closed by {sent} bytes");
            host.send_tagged(peer, 1, &message);
            sent += message.len();
            host.poll(Instant::now(), &mut events);
        }
        assert!(sent > 8 * MIB, "not closed before its backlog is full: {sent} bytes");
        assert!(host.take_overflowed().is_empty(), "reported once");
        assert!(host.peers().next().is_none(), "the link is closed");
        assert_eq!(host.buffered(peer), 0, "its backlog is gone");
        assert!(events.is_empty(), "a local close reports nothing: {events:?}");
        // Sends to it are ignored now.
        host.send(peer, Channel::Reliable, &message);
        assert_eq!(host.buffered(peer), 0);
        assert!(host.take_overflowed().is_empty());
    }

    /// A state passes the bound only while it is the peer's one transfer:
    /// a second state while the first is still queued closes the peer, and
    /// chunks no `StateBegin` announced count as ordinary messages.
    #[test]
    fn one_state_at_a_time() {
        let (mut host, peer, _client) = pair();
        let state = vec![5u8; 6 * 1024 * 1024];
        send_state(&mut host, peer, &state);
        assert!(host.take_overflowed().is_empty(), "one state is taken");
        send_state(&mut host, peer, &state);
        assert_eq!(host.take_overflowed(), [peer], "a second one is not");

        let (mut host, peer, _client) = pair();
        for chunk in vec![5u8; 8 * MIB + 2 * FEED_BELOW].chunks(rl_proto::STATE_CHUNK) {
            host.send_part(peer, 0, chunk, Part::StateChunk);
        }
        assert_eq!(host.take_overflowed(), [peer], "unannounced chunks are ordinary");
    }
}
