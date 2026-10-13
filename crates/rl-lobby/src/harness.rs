//! An in-process lobby: a [`LobbyHost`] and its clients exchanging tagged,
//! encoded messages (rl-proto's `encode_to_host`/`decode_to_host` and
//! `encode_to_client`/`decode_to_client` behind the reliable channel's tag
//! byte) through an ordered queue with a fixed one-way delay, in virtual
//! time. For the lobby's tests and the match layer's.
//!
//! Everything arriving at one instant is delivered before the sides are
//! drained (one poll). The host's `HostEvent::Kick` and `HostEvent::Close`
//! close the peer's link after the messages already queued to it arrive; `disconnect` drops a link at once (what is
//! in flight is lost). A message that fails to decode is recorded in
//! `errors` and dropped: the tests require none.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::net::IpAddr;

use rl_netcode::PeerId;
use rl_proto::{
    MatchStartWire, TAG_TO_CLIENT, TAG_TO_HOST, Tagged, ToClient, ToHost, decode_to_client, decode_to_host,
    encode_to_client, encode_to_host, tag,
};

use crate::client::LobbyClient;
use crate::host::LobbyHost;
use crate::identity::DtlsFingerprints;
use crate::model::{ClientEvent, HostEvent, NotStartable};

/// One client's end.
pub struct NetPeer {
    /// Its id at the host (index + 1).
    pub peer: PeerId,
    pub addr: IpAddr,
    /// `None`: a raw peer the test drives by hand (`send_raw`).
    pub client: Option<LobbyClient>,
    /// Every message it received, decoded.
    pub received: Vec<ToClient>,
    /// Its client's events.
    pub events: Vec<ClientEvent>,
    /// Whether its link is open.
    pub open: bool,
}

enum What {
    ToHost(usize, Vec<u8>),
    ToClient(usize, Vec<u8>),
    Close(usize),
}

struct Flight {
    at: u64,
    what: What,
}

/// The host, its clients and the links between them.
pub struct Net {
    pub host: LobbyHost,
    pub peers: Vec<NetPeer>,
    /// Every host event so far.
    pub host_events: Vec<HostEvent>,
    pub now_us: u64,
    /// One-way delay of every message.
    pub delay_us: u64,
    /// Messages that failed to decode.
    pub errors: Vec<String>,
    flights: VecDeque<Flight>,
    serial: u64,
}

/// The DTLS fingerprints of every link, as both ends see them (unless a
/// test makes a client see others: `join_seeing`).
pub const FINGERPRINTS: DtlsFingerprints = ([0xf1; 32], [0xf2; 32]);

/// Most deliveries one `settle` makes (a loop between host and clients
/// would be a lobby bug; the bound turns it into a test failure).
const SETTLE_LIMIT: usize = 100_000;

impl Net {
    pub fn new(host: LobbyHost, delay_us: u64) -> Net {
        Net {
            host,
            peers: Vec::new(),
            host_events: Vec::new(),
            now_us: 0,
            delay_us,
            errors: Vec::new(),
            flights: VecDeque::new(),
            serial: 0,
        }
    }

    /// Fresh deterministic bytes (nonces, signing randomness).
    fn fresh(&mut self) -> [u8; 32] {
        self.serial += 1;
        let mut b = [0x5a; 32];
        b[..8].copy_from_slice(&self.serial.to_le_bytes());
        b
    }

    fn open(&mut self, client: Option<LobbyClient>, addr: IpAddr, seen: DtlsFingerprints) -> usize {
        let i = self.peers.len();
        let peer = PeerId::try_from(i + 1).unwrap_or(PeerId::MAX);
        self.peers.push(NetPeer { peer, addr, client, received: Vec::new(), events: Vec::new(), open: true });
        let nonce = self.fresh();
        self.host.connected(peer, addr, self.now_us, nonce, FINGERPRINTS);
        if let Some(c) = &mut self.peers[i].client {
            c.connected(seen);
        }
        self.pump();
        i
    }

    /// Opens a link from `addr` for `client`; the handshake runs as time
    /// passes (`settle`, `run`).
    pub fn join(&mut self, client: LobbyClient, addr: IpAddr) -> usize {
        self.open(Some(client), addr, FINGERPRINTS)
    }

    /// As `join`, the client seeing the DTLS fingerprints `seen` while the
    /// host sees `FINGERPRINTS` (a man in the middle).
    pub fn join_seeing(&mut self, client: LobbyClient, addr: IpAddr, seen: DtlsFingerprints) -> usize {
        self.open(Some(client), addr, seen)
    }

    /// Opens a link without a client: the test sends with `send_raw` and
    /// reads `received`.
    pub fn join_raw(&mut self, addr: IpAddr) -> usize {
        self.open(None, addr, FINGERPRINTS)
    }

    /// Sends `msg` from peer `c` as is.
    pub fn send_raw(&mut self, c: usize, msg: ToHost) {
        self.push_to_host(c, &msg);
    }

    /// Peer `c`'s client, to act on (then `settle` or `run`).
    ///
    /// # Panics
    /// For a raw peer (a test bug).
    pub fn client(&mut self, c: usize) -> &mut LobbyClient {
        self.peers[c].client.as_mut().expect("not a raw peer")
    }

    /// Drops peer `c`'s link at once: both ends see it close, messages in
    /// flight are lost.
    pub fn disconnect(&mut self, c: usize) {
        self.pump();
        self.flights.retain(
            |f| !matches!(f.what, What::ToHost(i, _) | What::ToClient(i, _) | What::Close(i) if i == c),
        );
        self.close(c);
        self.pump();
    }

    /// The round trips the host would measure: twice the delay, in ms.
    pub fn rtts(&self) -> BTreeMap<PeerId, u32> {
        let ms = u32::try_from(2 * self.delay_us / 1000).unwrap_or(u32::MAX);
        self.peers.iter().filter(|p| p.open).map(|p| (p.peer, ms)).collect()
    }

    /// The match layer's start: `start`, the checksum filled in,
    /// `announce_start`, then settles. Returns the announced start.
    pub fn start(&mut self, seed: u32, checksum: u64) -> Result<MatchStartWire, NotStartable> {
        let rtts = self.rtts();
        let started = self.host.start(seed, &rtts).map(|mut wire| {
            wire.start_checksum = checksum;
            wire
        });
        if let Ok(wire) = &started {
            self.host.announce_start(wire.clone())?;
        }
        self.settle();
        started
    }

    /// Runs until nothing is in flight.
    pub fn settle(&mut self) {
        self.pump();
        for _ in 0..SETTLE_LIMIT {
            let Some(at) = self.flights.front().map(|f| f.at) else { return };
            self.arrive(at);
        }
        self.errors.push(format!("not settled after {SETTLE_LIMIT} deliveries"));
    }

    /// Advances time by `us`, delivering what arrives meanwhile.
    pub fn run(&mut self, us: u64) {
        self.pump();
        let end = self.now_us + us;
        while let Some(at) = self.flights.front().map(|f| f.at).filter(|&at| at <= end) {
            self.arrive(at);
        }
        self.now_us = end;
        self.host.tick(self.now_us);
        self.pump();
    }

    /// Delivers everything arriving at `at` (one poll of a real transport),
    /// then lets the host tick and drains every side once.
    fn arrive(&mut self, at: u64) {
        self.now_us = self.now_us.max(at);
        while let Some(f) = self.flights.pop_front_if(|f| f.at == at) {
            self.deliver(f.what);
        }
        self.host.tick(self.now_us);
        self.pump();
    }

    fn deliver(&mut self, what: What) {
        match what {
            What::ToHost(c, bytes) => {
                if !self.peers[c].open {
                    return;
                }
                match tag(&bytes) {
                    Some(Tagged::ToHost(b)) => match decode_to_host(b) {
                        Ok(msg) => self.host.receive(self.peers[c].peer, msg, self.now_us),
                        Err(e) => self.errors.push(format!("to host from {c}: {e:?}")),
                    },
                    _ => self.errors.push(format!("to host from {c}: bad tag")),
                }
            }
            What::ToClient(c, bytes) => {
                if !self.peers[c].open {
                    return;
                }
                let msg = match tag(&bytes) {
                    Some(Tagged::ToClient(b)) => decode_to_client(b),
                    _ => return self.errors.push(format!("to client {c}: bad tag")),
                };
                match msg {
                    Ok(msg) => {
                        self.peers[c].received.push(msg.clone());
                        let aux = self.fresh();
                        if let Some(client) = &mut self.peers[c].client {
                            client.receive(msg, aux);
                        }
                    }
                    Err(e) => self.errors.push(format!("to client {c}: {e:?}")),
                }
            }
            What::Close(c) => self.close(c),
        }
    }

    fn close(&mut self, c: usize) {
        if !core::mem::replace(&mut self.peers[c].open, false) {
            return;
        }
        if let Some(client) = &mut self.peers[c].client {
            client.disconnected();
        }
        self.host.disconnected(self.peers[c].peer);
    }

    fn push_to_host(&mut self, c: usize, msg: &ToHost) {
        let mut bytes = Vec::from([TAG_TO_HOST]);
        bytes.extend(encode_to_host(msg));
        self.flights.push_back(Flight { at: self.now_us + self.delay_us, what: What::ToHost(c, bytes) });
    }

    /// Moves everything the host and the clients produced into the links.
    fn pump(&mut self) {
        let at = self.now_us + self.delay_us;
        for (peer, msg) in self.host.outgoing() {
            let Some(c) = self.peers.iter().position(|p| p.peer == peer) else {
                self.errors.push(format!("message to unknown peer {peer}"));
                continue;
            };
            let mut bytes = Vec::from([TAG_TO_CLIENT]);
            bytes.extend(encode_to_client(&msg));
            self.flights.push_back(Flight { at, what: What::ToClient(c, bytes) });
        }
        for e in self.host.events() {
            if let HostEvent::Kick { peer, .. } | HostEvent::Close { peer } = e
                && let Some(c) = self.peers.iter().position(|p| p.peer == peer)
            {
                self.flights.push_back(Flight { at, what: What::Close(c) });
            }
            self.host_events.push(e);
        }
        for c in 0..self.peers.len() {
            let Some(client) = &mut self.peers[c].client else { continue };
            let (out, events) = (client.outgoing(), client.events());
            self.peers[c].events.extend(events);
            if self.peers[c].open {
                for msg in &out {
                    self.push_to_host(c, msg);
                }
            }
        }
    }
}
