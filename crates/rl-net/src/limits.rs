//! Per-peer limits (spec §5.4): every message type has a size limit and
//! every peer a rate limit; a peer breaking either is dropped.

use std::time::Instant;

use rl_proto::{MAX_CONTROL, MAX_DATAGRAM, MAX_LOBBY, MAX_SIGNAL, STATE_CHUNK};

use crate::endpoint::Why;

/// The largest reliable message: a netcode `Control` or a lobby message,
/// plus the tag byte in front (`rl_proto::tag`). Below `Link`'s
/// `MAX_MESSAGE`.
pub const MAX_RELIABLE: usize = if MAX_CONTROL > MAX_LOBBY { MAX_CONTROL } else { MAX_LOBBY } + 1;
const _: () = assert!(MAX_RELIABLE <= crate::link::MAX_MESSAGE);

/// What one peer may send us.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Messages on the unreliable channel per second (one netcode datagram
    /// each; the netcode sends at most one per tick). Default 400.
    pub max_datagrams_per_s: u32,
    /// Bytes of reliable messages per second, each message counting
    /// [`MESSAGE_COST`] more than its length; a state transfer is exempt
    /// (`Endpoint::allow_burst`). Default 256 KiB.
    pub max_reliable_bytes_per_s: u32,
    /// The largest signalling message (one WebSocket frame). Default
    /// [`rl_proto::MAX_SIGNAL`].
    pub max_signal_bytes: usize,
    /// Raw UDP datagrams per second from the peer's address, whatever they
    /// carry (STUN, DTLS, SCTP control, junk). Generous: 10 000, about 4×
    /// what a 3 MB transfer per second takes, SACKs included.
    pub max_raw_datagrams_per_s: u32,
    /// Raw UDP bytes per second from the peer's address. Default 12 MiB,
    /// about 4× a 3 MB transfer per second.
    pub max_raw_bytes_per_s: u32,
}

/// [`Limits::max_datagrams_per_s`]'s default.
pub(crate) const DEFAULT_DATAGRAMS_PER_S: u32 = 400;

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_datagrams_per_s: DEFAULT_DATAGRAMS_PER_S,
            max_reliable_bytes_per_s: 256 * 1024,
            max_signal_bytes: MAX_SIGNAL,
            max_raw_datagrams_per_s: 10_000,
            max_raw_bytes_per_s: 12 * 1024 * 1024,
        }
    }
}

/// What every reliable message costs beyond its length, so a flood of tiny
/// messages also meets the byte limit.
pub const MESSAGE_COST: u32 = 64;

/// A token bucket that holds one second of its rate, full at the start.
#[derive(Clone, Debug)]
struct Bucket {
    rate: f64,
    level: f64,
    last: Instant,
}

impl Bucket {
    fn new(rate: u32, now: Instant) -> Bucket {
        Bucket { rate: rate as f64, level: rate as f64, last: now }
    }

    /// Takes `amount` tokens; false when there are not enough.
    fn take(&mut self, now: Instant, amount: f64) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = self.last.max(now);
        self.level = (self.level + elapsed * self.rate).min(self.rate);
        if amount > self.level {
            return false;
        }
        self.level -= amount;
        true
    }
}

/// One peer's limits as it sends.
#[derive(Clone, Debug)]
pub(crate) struct PeerLimits {
    datagrams: Bucket,
    bytes: Bucket,
    /// Reliable bytes still exempt from the byte limit (a state transfer).
    exempt: u64,
    raw_datagrams: Bucket,
    raw_bytes: Bucket,
    /// Raw datagrams and bytes still exempt from the raw limits.
    raw_exempt: (u64, u64),
}

impl PeerLimits {
    pub(crate) fn new(limits: &Limits, now: Instant) -> PeerLimits {
        PeerLimits {
            datagrams: Bucket::new(limits.max_datagrams_per_s, now),
            bytes: Bucket::new(limits.max_reliable_bytes_per_s, now),
            exempt: 0,
            raw_datagrams: Bucket::new(limits.max_raw_datagrams_per_s, now),
            raw_bytes: Bucket::new(limits.max_raw_bytes_per_s, now),
            raw_exempt: (0, 0),
        }
    }

    /// Lets the next `bytes` of state through the byte limit (plus each
    /// chunk's [`MESSAGE_COST`]), and twice their datagrams and bytes
    /// through the raw limits (a sender's SACKs, packet overheads).
    pub(crate) fn allow_burst(&mut self, bytes: usize) {
        let chunks = bytes.div_ceil(STATE_CHUNK) as u64 + 1;
        self.exempt = self.exempt.saturating_add(bytes as u64 + chunks * MESSAGE_COST as u64);
        let (datagrams, raw) = &mut self.raw_exempt;
        *datagrams = datagrams.saturating_add(2 * (bytes as u64 / 1000 + 16));
        *raw = raw.saturating_add(2 * bytes as u64 + 64 * 1024);
    }

    /// Counts a raw datagram of `len` bytes from the peer's address at
    /// `now`: `Err(Why::Limits)` over either raw rate.
    pub(crate) fn on_datagram(&mut self, now: Instant, len: usize) -> Result<(), Why> {
        let (datagrams, raw) = &mut self.raw_exempt;
        let one = u64::from(*datagrams > 0);
        *datagrams -= one;
        let free = (len as u64).min(*raw);
        *raw -= free;
        let ok = self.raw_datagrams.take(now, (1 - one) as f64)
            & self.raw_bytes.take(now, (len as u64 - free) as f64);
        if ok { Ok(()) } else { Err(Why::Limits) }
    }

    /// Counts a message of `len` bytes from the peer at `now`: `Err` when it
    /// is too large ([`Why::Malformed`]) or over the rate ([`Why::Limits`]).
    pub(crate) fn on_message(&mut self, now: Instant, reliable: bool, len: usize) -> Result<(), Why> {
        if !reliable {
            if len > MAX_DATAGRAM {
                return Err(Why::Malformed);
            }
            return if self.datagrams.take(now, 1.0) { Ok(()) } else { Err(Why::Limits) };
        }
        if len > MAX_RELIABLE {
            return Err(Why::Malformed);
        }
        let cost = len as u64 + MESSAGE_COST as u64;
        let exempt = cost.min(self.exempt);
        self.exempt -= exempt;
        if self.bytes.take(now, (cost - exempt) as f64) { Ok(()) } else { Err(Why::Limits) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_bucket_holds_one_second_and_refills_at_its_rate() {
        let t0 = Instant::now();
        let mut b = Bucket::new(400, t0);
        for _ in 0..400 {
            assert!(b.take(t0, 1.0));
        }
        assert!(!b.take(t0, 1.0), "empty after a second's worth");
        assert!(b.take(t0 + Duration::from_millis(10), 4.0), "4 tokens back after 10 ms");
        assert!(!b.take(t0 + Duration::from_millis(10), 1.0));
        assert!(b.take(t0 + Duration::from_secs(60), 400.0), "refills to one second");
        assert!(!b.take(t0 + Duration::from_secs(60), 1.0), "never above one second");
    }

    #[test]
    fn steady_traffic_within_the_rate_passes() {
        let t0 = Instant::now();
        let mut p = PeerLimits::new(&Limits::default(), t0);
        // 120 datagrams and 120 reliable 1 KB messages per second for 10 s.
        for i in 0..1200u64 {
            let now = t0 + Duration::from_micros(i * 8_333);
            assert_eq!(p.on_message(now, false, 200), Ok(()));
            assert_eq!(p.on_message(now, true, 1000), Ok(()));
        }
    }

    #[test]
    fn floods_and_oversized_messages_are_caught() {
        let t0 = Instant::now();
        let mut p = PeerLimits::new(&Limits::default(), t0);
        let caught =
            (0..2000u64).position(|i| p.on_message(t0 + Duration::from_micros(i * 500), false, 8).is_err());
        assert!(caught.is_some_and(|i| i < 900), "{caught:?}");
        let mut p = PeerLimits::new(&Limits::default(), t0);
        assert_eq!(p.on_message(t0, false, MAX_DATAGRAM + 1), Err(Why::Malformed));
        assert_eq!(p.on_message(t0, true, MAX_RELIABLE + 1), Err(Why::Malformed));
        assert_eq!(p.on_message(t0, true, MAX_RELIABLE), Ok(()));
        assert_eq!(MAX_RELIABLE, rl_proto::MAX_LOBBY + 1, "a full lobby and its tag byte");
        const { assert!(rl_proto::MAX_CONTENT < MAX_RELIABLE, "a full content message and its tag byte") };
        // Tiny reliable messages count MESSAGE_COST each.
        let mut p = PeerLimits::new(&Limits::default(), t0);
        let n = (256 * 1024 / MESSAGE_COST as usize) + 1;
        assert!((0..n).any(|_| p.on_message(t0, true, 0).is_err()));
    }

    #[test]
    fn raw_floods_are_caught_and_a_burst_lifts_the_raw_limits() {
        let t0 = Instant::now();
        let mut p = PeerLimits::new(&Limits::default(), t0);
        // A 3 MB transfer's datagrams (and as many SACKs) within a second pass.
        for _ in 0..2 * 3_000_000 / 1100 {
            assert_eq!(p.on_datagram(t0, 1100), Ok(()));
        }
        let mut p = PeerLimits::new(&Limits::default(), t0);
        let caught = (0..20_000).position(|_| p.on_datagram(t0, 100).is_err());
        assert_eq!(caught, Some(10_000), "the datagram rate");
        let mut p = PeerLimits::new(&Limits::default(), t0);
        let caught = (0..20_000).position(|_| p.on_datagram(t0, 1500).is_err());
        assert_eq!(caught, Some(12 * 1024 * 1024 / 1500), "the byte rate");
        let mut p = PeerLimits::new(&Limits::default(), t0);
        p.allow_burst(20_000_000);
        for _ in 0..20_000_000 / 1100 {
            assert_eq!(p.on_datagram(t0, 1100), Ok(()), "a 20 MB state at once");
        }
    }

    #[test]
    fn a_burst_lets_one_state_through() {
        let t0 = Instant::now();
        let state = 3 * 1024 * 1024;
        let mut p = PeerLimits::new(&Limits::default(), t0);
        p.allow_burst(state);
        for _ in 0..state / STATE_CHUNK {
            assert_eq!(p.on_message(t0, true, STATE_CHUNK), Ok(()));
        }
        // The burst is spent: the bucket's second of bytes remains, then no more.
        let rest = (0..100).position(|_| p.on_message(t0, true, STATE_CHUNK).is_err());
        // 64 bytes of the burst are left: the first chunk takes 32 KiB of the
        // bucket, the next six 32 KiB + 64 each, the eighth does not fit.
        assert_eq!(rest, Some(7));
    }
}
