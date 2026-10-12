//! Impairment over real sockets (spec §9.4): every outgoing datagram of a
//! link passes through `rl_netcode::netsim`'s link model — loss, delay,
//! jitter, duplication — before it is sent. Loss applies below WebRTC, so
//! the reliable channel retransmits as it would on a real network.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

use rl_netcode::Rng;
use rl_netcode::netsim::{LinkModel, Profile};

use crate::link::Transmit;

struct Due {
    at: Instant,
    seq: u64,
    transmit: Transmit,
}

impl PartialEq for Due {
    fn eq(&self, o: &Self) -> bool {
        (self.at, self.seq) == (o.at, o.seq)
    }
}
impl Eq for Due {}
impl PartialOrd for Due {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Due {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.at, self.seq).cmp(&(o.at, o.seq))
    }
}

/// One direction of one link: what we send to that peer.
pub(crate) struct Impair {
    model: LinkModel,
    queue: BinaryHeap<Reverse<Due>>,
    seq: u64,
    /// Datagrams the model lost.
    pub(crate) lost: u64,
}

impl Impair {
    pub(crate) fn new(profile: Profile, rng: Rng) -> Impair {
        Impair { model: LinkModel::new(profile, rng), queue: BinaryHeap::new(), seq: 0, lost: 0 }
    }

    /// Takes a datagram sent at `now`: lost, or held for its delay (and
    /// perhaps a second copy for another). The draws are `Network`'s for an
    /// unreliable datagram.
    pub(crate) fn push(&mut self, now: Instant, transmit: Transmit) {
        let lost = self.model.lost();
        let first = Duration::from_micros(self.model.delay_us());
        let second = self.model.duplicated().then(|| Duration::from_micros(self.model.delay_us()));
        if lost {
            self.lost += 1;
            return;
        }
        if let Some(d) = second {
            self.hold(now + d, transmit.clone());
        }
        self.hold(now + first, transmit);
    }

    fn hold(&mut self, at: Instant, transmit: Transmit) {
        self.seq += 1;
        self.queue.push(Reverse(Due { at, seq: self.seq, transmit }));
    }

    /// Appends the datagrams due by `now` to `out`, in order of their time.
    pub(crate) fn due(&mut self, now: Instant, out: &mut Vec<Transmit>) {
        while self.queue.peek().is_some_and(|d| d.0.at <= now) {
            let Some(Reverse(d)) = self.queue.pop() else { break };
            out.push(d.transmit);
        }
    }

    /// When the next held datagram is due.
    #[cfg(test)]
    fn next(&self) -> Option<Instant> {
        self.queue.peek().map(|d| d.0.at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transmit(i: u32) -> Transmit {
        Transmit {
            from: "127.0.0.1:1".parse().unwrap(),
            to: "127.0.0.1:2".parse().unwrap(),
            bytes: i.to_le_bytes().to_vec(),
        }
    }

    #[test]
    fn a_profile_delays_loses_and_duplicates_like_the_model() {
        let t0 = Instant::now();
        let mut imp = Impair::new(Profile::AWFUL, Rng::new(3));
        for i in 0..10_000u32 {
            imp.push(t0, transmit(i));
        }
        assert_eq!(
            imp.next().map(|t| t >= t0 + Duration::from_millis(100)),
            Some(true),
            "at least ping/2 - jitter/2"
        );
        let mut out = Vec::new();
        imp.due(t0 + Duration::from_millis(99), &mut out);
        assert!(out.is_empty());
        imp.due(t0 + Duration::from_millis(201), &mut out);
        assert!(imp.next().is_none(), "at most ping/2 + jitter/2");
        let lost = imp.lost as f64 / 10_000.0;
        assert!((0.12..0.18).contains(&lost), "about 15 % lost: {lost}");
        let mut ids: Vec<u32> =
            out.iter().map(|t| u32::from_le_bytes(t.bytes[..].try_into().unwrap())).collect();
        let copies = ids.len() as u64 + imp.lost - 10_000;
        assert!((100..300).contains(&copies), "about 2 % duplicated: {copies}");
        assert!(ids.windows(2).any(|w| w[0] > w[1]), "jitter reorders");
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len() as u64 + imp.lost, 10_000);
    }

    #[test]
    fn the_same_seed_gives_the_same_network() {
        let t0 = Instant::now();
        let run = || {
            let mut imp = Impair::new(Profile::BAD, Rng::new(9));
            for i in 0..500 {
                imp.push(t0 + Duration::from_millis(i as u64), transmit(i));
            }
            let mut out = Vec::new();
            imp.due(t0 + Duration::from_secs(5), &mut out);
            out
        };
        assert_eq!(run(), run());
    }
}
