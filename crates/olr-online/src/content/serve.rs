//! Serving pack archives (plan 11, N32): one transfer at a time to each
//! receiving peer, its other wants queued; paced by a token bucket per
//! receiver (`PEER_RATE`) and one shared by all (the server's rate), in
//! round-robin over the receivers.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use olr_netcode::PeerId;
use olr_proto::{CONTENT_CHUNK, ContentMsg, MAX_ITEMS};

use super::{PEER_RATE, PackArchive, QUEUED};

/// The room a chunk needs in a peer's outbox.
const CHUNK_ROOM: usize = CONTENT_CHUNK + 64;
/// A chunk's encoded size beyond its bytes, at most: the tag, the variant,
/// the hash (length and 64 bytes), the offset and the bytes' length.
const CHUNK_OVERHEAD: usize = 1 + 1 + 65 + 10 + 3;
const MICROS: u64 = 1_000_000;

/// A token bucket in byte-microseconds, so slow rates lose no fractions.
#[derive(Debug)]
struct Bucket {
    rate: u64,
    tokens: u64,
}

impl Bucket {
    fn new(rate: u64) -> Bucket {
        Bucket { rate, tokens: 0 }
    }

    /// At most an eighth of a second's bytes (and a whole chunk) wait.
    fn cap(&self) -> u64 {
        (self.rate / 8).max(CONTENT_CHUNK as u64).saturating_mul(MICROS)
    }

    fn refill(&mut self, dt_us: u64) {
        self.tokens = self.tokens.saturating_add(self.rate.saturating_mul(dt_us)).min(self.cap());
    }

    fn has(&self, bytes: u64) -> bool {
        self.tokens >= bytes.saturating_mul(MICROS)
    }

    fn take(&mut self, bytes: u64) {
        self.tokens = self.tokens.saturating_sub(bytes.saturating_mul(MICROS));
    }
}

#[derive(Debug)]
struct Transfer {
    hash: String,
    archive: Arc<PackArchive>,
    offset: u64,
    /// `Begin` was sent for this offset.
    begun: bool,
}

#[derive(Debug)]
struct Receiver {
    wants: VecDeque<(String, u64)>,
    current: Option<Transfer>,
    /// `Unavailable` answers not yet sent.
    refusals: VecDeque<String>,
    bucket: Bucket,
}

impl Receiver {
    fn refuse(&mut self, hash: &str) {
        if self.refusals.len() < MAX_ITEMS {
            self.refusals.push_back(hash.to_string());
        }
    }
}

/// Serves archives to peers: sans-IO, time injected.
#[derive(Debug)]
pub struct Server {
    shared: Bucket,
    offered: BTreeMap<String, Arc<PackArchive>>,
    peers: BTreeMap<PeerId, Receiver>,
    last_us: Option<u64>,
    /// The receiver served last: the next round starts after it.
    last_served: Option<PeerId>,
}

impl Server {
    /// A server sending at most `rate` bytes per second in all.
    pub fn new(rate: u64) -> Server {
        Server {
            shared: Bucket::new(rate),
            offered: BTreeMap::new(),
            peers: BTreeMap::new(),
            last_us: None,
            last_served: None,
        }
    }

    pub fn set_rate(&mut self, rate: u64) {
        self.shared.rate = rate;
        self.shared.tokens = self.shared.tokens.min(self.shared.cap());
    }

    /// Offers `archive` under content hash `hash`.
    pub fn offer(&mut self, hash: &str, archive: Arc<PackArchive>) {
        self.offered.insert(hash.to_string(), archive);
    }

    /// Withdraws `hash`: a transfer of it in progress ends with `Unavailable`.
    pub fn withdraw(&mut self, hash: &str) {
        self.offered.remove(hash);
        for r in self.peers.values_mut() {
            if r.current.as_ref().is_some_and(|t| t.hash == hash) {
                r.current = None;
                r.refuse(hash);
            }
        }
    }

    pub fn offered(&self, hash: &str) -> bool {
        self.offered.contains_key(hash)
    }

    /// A message from `peer`: `Want` and `Cancel`; the others are not a
    /// server's.
    pub fn receive(&mut self, peer: PeerId, msg: &ContentMsg) {
        match msg {
            ContentMsg::Want { hash, offset } => {
                let r = self.peers.entry(peer).or_insert_with(|| Receiver {
                    wants: VecDeque::new(),
                    current: None,
                    refusals: VecDeque::new(),
                    bucket: Bucket::new(PEER_RATE),
                });
                let fits = self.offered.get(hash).is_some_and(|a| *offset <= a.bytes.len() as u64);
                if !fits {
                    r.refuse(hash);
                } else if let Some(t) = r.current.as_mut().filter(|t| t.hash == *hash) {
                    // Asked again (a resume, or the receiver heard nothing).
                    t.offset = *offset;
                    t.begun = false;
                } else if let Some(w) = r.wants.iter_mut().find(|(h, _)| h == hash) {
                    w.1 = *offset;
                } else if r.wants.len() < MAX_ITEMS {
                    r.wants.push_back((hash.clone(), *offset));
                }
            }
            ContentMsg::Cancel { hash } => {
                if let Some(r) = self.peers.get_mut(&peer) {
                    r.wants.retain(|(h, _)| h != hash);
                    if r.current.as_ref().is_some_and(|t| t.hash == *hash) {
                        r.current = None;
                    }
                }
            }
            _ => {}
        }
    }

    pub fn peer_left(&mut self, peer: PeerId) {
        self.peers.remove(&peer);
    }

    /// Messages to send now; `room(peer)` is how many bytes the peer's
    /// outbox may take (`QUEUED` minus what it holds, as the caller counts).
    /// A chunk goes only when the room is at least `CONTENT_CHUNK + 64`.
    /// `Unavailable` and a `Begin` without a chunk (a want at the archive's
    /// end) pass neither the room nor the buckets: each answers one `Want`,
    /// so they are bounded by what the peer sends (about 200 bytes out per
    /// want in), as olr-net bounds that.
    pub fn poll(&mut self, now_us: u64, room: &dyn Fn(PeerId) -> usize, out: &mut Vec<(PeerId, ContentMsg)>) {
        let dt = self.last_us.map_or(0, |last| now_us.saturating_sub(last));
        self.last_us = Some(now_us);
        self.shared.refill(dt);
        for (&id, r) in &mut self.peers {
            r.bucket.refill(dt);
            while let Some(hash) = r.refusals.pop_front() {
                out.push((id, ContentMsg::Unavailable { hash }));
            }
        }
        for id in self.peers.keys().copied().collect::<Vec<_>>() {
            self.next_transfer(id, out);
        }
        // Round-robin, starting after the receiver served last.
        let mut order: Vec<PeerId> =
            self.peers.iter().filter(|(_, r)| r.current.is_some()).map(|(id, _)| *id).collect();
        let first = order.iter().position(|id| Some(*id) > self.last_served).unwrap_or(0);
        order.rotate_left(first);
        let mut rooms: BTreeMap<PeerId, usize> =
            order.iter().map(|id| (*id, room(*id).min(QUEUED))).collect();
        loop {
            let mut sent = false;
            for &id in &order {
                let Some(left) = rooms.get_mut(&id) else { continue };
                if self.send_chunk(id, left, out) {
                    self.last_served = Some(id);
                    sent = true;
                }
            }
            if !sent {
                break;
            }
        }
    }

    /// Starts `peer`'s next queued want, if it has no transfer: wants for
    /// what is not (or no longer) offered are refused.
    fn next_transfer(&mut self, peer: PeerId, out: &mut Vec<(PeerId, ContentMsg)>) {
        let Some(r) = self.peers.get_mut(&peer) else { return };
        while r.current.is_none() {
            let Some((hash, offset)) = r.wants.pop_front() else { return };
            match self.offered.get(&hash).filter(|a| offset <= a.bytes.len() as u64) {
                Some(a) => r.current = Some(Transfer { hash, archive: a.clone(), offset, begun: false }),
                None => out.push((peer, ContentMsg::Unavailable { hash })),
            }
        }
    }

    /// Sends `peer`'s next chunk (after its `Begin`) if the outbox room and
    /// both buckets allow; returns whether it did.
    fn send_chunk(&mut self, peer: PeerId, room: &mut usize, out: &mut Vec<(PeerId, ContentMsg)>) -> bool {
        let Some(r) = self.peers.get_mut(&peer) else { return false };
        let Some(t) = r.current.as_mut() else { return false };
        let size = t.archive.bytes.len() as u64;
        let len = size.saturating_sub(t.offset).min(CONTENT_CHUNK as u64);
        if len > 0 && (*room < CHUNK_ROOM || !r.bucket.has(len) || !self.shared.has(len)) {
            return false;
        }
        if !t.begun {
            t.begun = true;
            let begin = ContentMsg::Begin {
                hash: t.hash.clone(),
                archive: t.archive.sha.clone(),
                size,
                offset: t.offset,
            };
            out.push((peer, begin));
        }
        if len > 0 {
            // `offset + len <= size`, which is the archive's length.
            let (start, end) = (t.offset as usize, (t.offset + len) as usize);
            let bytes = t.archive.bytes[start..end].to_vec();
            out.push((peer, ContentMsg::Chunk { hash: t.hash.clone(), offset: t.offset, bytes }));
            t.offset += len;
            r.bucket.take(len);
            self.shared.take(len);
            *room = room.saturating_sub(len as usize + CHUNK_OVERHEAD);
        }
        if t.offset >= size {
            r.current = None;
            self.next_transfer(peer, out);
        }
        len > 0
    }
}

#[cfg(test)]
mod tests {
    use super::super::HOST_RATE;
    use super::*;

    fn hash(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn archive(len: usize, seed: u8) -> Arc<PackArchive> {
        Arc::new(PackArchive::new((0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()))
    }

    fn want(c: char, offset: u64) -> ContentMsg {
        ContentMsg::Want { hash: hash(c), offset }
    }

    /// Polls every `step_us` until `until_us`; the outboxes always have
    /// `room`. Returns everything sent.
    fn run(
        server: &mut Server,
        from_us: u64,
        until_us: u64,
        step_us: u64,
        room: usize,
    ) -> Vec<(PeerId, ContentMsg)> {
        let mut out = vec![];
        let mut now = from_us;
        while now <= until_us {
            server.poll(now, &|_| room, &mut out);
            now += step_us;
        }
        out
    }

    fn chunk_bytes(out: &[(PeerId, ContentMsg)], peer: PeerId) -> u64 {
        out.iter()
            .filter(|(p, _)| *p == peer)
            .map(|(_, m)| match m {
                ContentMsg::Chunk { bytes, .. } => bytes.len() as u64,
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn the_server_paces_to_its_rates() {
        let mut server = Server::new(1 << 20);
        for (i, c) in ['a', 'b', 'c'].into_iter().enumerate() {
            server.offer(&hash(c), archive(8 << 20, i as u8));
            server.receive(i as PeerId + 1, &want(c, 0));
        }
        let out = run(&mut server, 0, 10_000_000, 10_000, 1 << 20);
        let total: u64 = (1..=3).map(|p| chunk_bytes(&out, p)).sum();
        let mib = total as f64 / (1 << 20) as f64;
        assert!((9.5..=10.5).contains(&mib), "{mib} MiB in 10 s at 1 MiB/s");
        for p in 1..=3 {
            // Round-robin: each gets about a third (without it, the first
            // receiver would take the whole shared rate).
            let each = chunk_bytes(&out, p);
            assert!(
                (each as f64 - total as f64 / 3.0).abs() <= 2.0 * CONTENT_CHUNK as f64,
                "{each} of {total}"
            );
        }
        // A server rate above three receivers' `PEER_RATE`: each is held to
        // `PEER_RATE` (and gets it).
        let mut server = Server::new(8 << 20);
        for (i, c) in ['a', 'b', 'c'].into_iter().enumerate() {
            server.offer(&hash(c), archive(32 << 20, i as u8));
            server.receive(i as PeerId + 1, &want(c, 0));
        }
        let out = run(&mut server, 0, 10_000_000, 10_000, 1 << 20);
        for p in 1..=3 {
            let each = chunk_bytes(&out, p);
            assert!((95 * PEER_RATE / 10..=10 * PEER_RATE).contains(&each), "{each} in 10 s");
        }
    }

    #[test]
    fn one_receiver_is_held_to_its_rate() {
        let mut server = Server::new(HOST_RATE);
        server.offer(&hash('a'), archive(16 << 20, 0));
        server.receive(1, &want('a', 0));
        let out = run(&mut server, 0, 4_000_000, 10_000, 1 << 20);
        let mib = chunk_bytes(&out, 1) as f64 / (1 << 20) as f64;
        assert!((7.5..=8.25).contains(&mib), "{mib} MiB in 4 s at 2 MiB/s");
    }

    #[test]
    fn unoffered_wants_are_refused() {
        let mut server = Server::new(HOST_RATE);
        server.offer(&hash('a'), archive(100, 0));
        server.receive(1, &want('b', 0));
        server.receive(1, &want('b', 0));
        // An offset beyond the archive is a violation for that transfer.
        server.receive(1, &want('a', 101));
        server.receive(1, &want('a', u64::MAX));
        let out = run(&mut server, 0, 1_000_000, 10_000, 1 << 20);
        let refused = |c: char| (1, ContentMsg::Unavailable { hash: hash(c) });
        assert_eq!(out, vec![refused('b'), refused('b'), refused('a'), refused('a')]);
        // Withdrawn while queued, then while sending.
        server.offer(&hash('c'), archive(3 * CONTENT_CHUNK, 1));
        server.receive(1, &want('a', 0));
        server.receive(1, &want('c', 0));
        server.withdraw(&hash('c'));
        let out = run(&mut server, 1_100_000, 2_000_000, 10_000, 1 << 20);
        assert!(matches!(out[0], (1, ContentMsg::Begin { .. })));
        assert_eq!(out.last(), Some(&refused('c')));
        server.offer(&hash('c'), archive(64 * CONTENT_CHUNK, 1));
        server.receive(1, &want('c', 0));
        let out = run(&mut server, 2_100_000, 2_120_000, 10_000, 1 << 20);
        assert!(matches!(out[..], [(1, ContentMsg::Begin { .. }), (1, ContentMsg::Chunk { .. }), ..]));
        server.withdraw(&hash('c'));
        let out = run(&mut server, 2_130_000, 2_200_000, 10_000, 1 << 20);
        assert_eq!(out, vec![refused('c')]);
        // A refused peer is not refused without bound: one answer a want.
        for _ in 0..1000 {
            server.receive(2, &want('z', 0));
        }
        let out = run(&mut server, 2_300_000, 2_300_000, 10_000, 1 << 20);
        assert_eq!(out.len(), MAX_ITEMS);
    }

    #[test]
    fn a_begin_precedes_every_transfer_and_resume() {
        let mut server = Server::new(HOST_RATE);
        let a = archive(2 * CONTENT_CHUNK - 5, 0);
        server.offer(&hash('a'), a.clone());
        server.receive(1, &want('a', CONTENT_CHUNK as u64 + 1));
        let out = run(&mut server, 0, 1_000_000, 10_000, 1 << 20);
        let begin = ContentMsg::Begin {
            hash: hash('a'),
            archive: a.sha.clone(),
            size: a.bytes.len() as u64,
            offset: CONTENT_CHUNK as u64 + 1,
        };
        let chunk = ContentMsg::Chunk {
            hash: hash('a'),
            offset: CONTENT_CHUNK as u64 + 1,
            bytes: a.bytes[CONTENT_CHUNK + 1..].to_vec(),
        };
        assert_eq!(out, vec![(1, begin), (1, chunk)]);
        // A want at the very end: a begin and no chunk.
        server.receive(1, &want('a', a.bytes.len() as u64));
        let out = run(&mut server, 1_010_000, 1_100_000, 10_000, 1 << 20);
        assert!(
            matches!(&out[..], [(1, ContentMsg::Begin { offset, .. })] if *offset == a.bytes.len() as u64)
        );
        // Asked again mid-transfer (one chunk a poll): a new begin at the
        // asked offset.
        server.offer(&hash('b'), archive(64 * CONTENT_CHUNK, 1));
        server.receive(1, &want('b', 0));
        let out = run(&mut server, 1_110_000, 1_110_000, 10_000, CHUNK_ROOM);
        assert!(matches!(
            &out[..],
            [(_, ContentMsg::Begin { offset: 0, .. }), (_, ContentMsg::Chunk { offset: 0, .. })]
        ));
        server.receive(1, &want('b', 7));
        let out = run(&mut server, 1_120_000, 1_120_000, 10_000, CHUNK_ROOM);
        assert!(matches!(
            &out[..],
            [(_, ContentMsg::Begin { offset: 7, .. }), (_, ContentMsg::Chunk { offset: 7, .. })]
        ));
    }

    #[test]
    fn one_transfer_at_a_time_and_cancel() {
        let mut server = Server::new(HOST_RATE);
        server.offer(&hash('a'), archive(4 * CONTENT_CHUNK, 0));
        server.offer(&hash('b'), archive(CONTENT_CHUNK, 1));
        server.receive(1, &want('a', 0));
        server.receive(1, &want('b', 0));
        let out = run(&mut server, 0, 20_000, 10_000, 1 << 20);
        let a = hash('a');
        assert!(!out.is_empty());
        assert!(
            out.iter()
                .all(|(_, m)| matches!(m, ContentMsg::Begin { hash: h, .. } | ContentMsg::Chunk { hash: h, .. } if *h == a))
        );
        server.receive(1, &ContentMsg::Cancel { hash: hash('a') });
        let out = run(&mut server, 30_000, 1_000_000, 10_000, 1 << 20);
        assert!(
            matches!(&out[..], [(1, ContentMsg::Begin { hash: h, .. }), (1, ContentMsg::Chunk { .. })] if *h == hash('b'))
        );
        // Wants beyond `MAX_ITEMS` are ignored.
        for i in 0..2 * MAX_ITEMS {
            server.offer(&format!("{i:064x}"), archive(10, 0));
            server.receive(2, &ContentMsg::Want { hash: format!("{i:064x}"), offset: 0 });
        }
        assert_eq!(server.peers[&2].wants.len(), MAX_ITEMS);
    }

    #[test]
    fn a_full_outbox_gets_no_chunks() {
        let mut server = Server::new(HOST_RATE);
        server.offer(&hash('a'), archive(4 * CONTENT_CHUNK, 0));
        server.receive(1, &want('a', 0));
        assert!(run(&mut server, 0, 1_000_000, 10_000, CHUNK_ROOM - 1).is_empty());
        // With room for one chunk, one chunk a poll.
        let out = run(&mut server, 1_010_000, 1_010_000, 10_000, CHUNK_ROOM);
        assert_eq!(chunk_bytes(&out, 1), CONTENT_CHUNK as u64);
        // Never more than `QUEUED` encoded bytes a poll, whatever the room
        // (the buckets, full, would allow `QUEUED` bytes of chunks).
        let mut server = Server::new(u64::MAX);
        server.offer(&hash('a'), archive(64 * CONTENT_CHUNK, 0));
        server.receive(1, &want('a', 0));
        let out = run(&mut server, 0, 10_000_000, 10_000_000, usize::MAX);
        let chunks = out.iter().filter(|(_, m)| matches!(m, ContentMsg::Chunk { .. })).count();
        let encoded = chunk_bytes(&out, 1) as usize + chunks * CHUNK_OVERHEAD;
        assert!(encoded <= QUEUED, "{encoded}");
        assert_eq!(chunks, QUEUED / (CONTENT_CHUNK + CHUNK_OVERHEAD));
    }
}
