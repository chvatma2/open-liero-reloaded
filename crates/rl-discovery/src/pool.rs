//! The relay pool: one driver thread per relay ([`crate::relay`]) behind a
//! caller-facing [`Pool`] that never blocks. Publishing and subscribing
//! queue work for the drivers; [`Pool::poll`] drains what they received,
//! relay by relay, with events deduplicated by id across relays.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rustls::pki_types::CertificateDer;

use crate::nostr::{Event, Filter, MAX_FRAME, ToRelay, encode_to_relay};
use crate::relay::{self, DriverConfig, Link, Outgoing, TICK};

/// How long dropping a pool waits for its threads.
const JOIN_BOUND: Duration = Duration::from_secs(2);
/// The event ids remembered for deduplication.
const SEEN_IDS: usize = 4096;

/// The relays and limits of a [`Pool`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolConfig {
    /// `ws://` or `wss://` URLs; a relay's index in [`PoolEvent`] is its
    /// position here.
    pub relays: Vec<String>,
    /// DER certificates trusted besides Mozilla's roots (webpki-roots), for
    /// tests. The platform's own roots are never used, so every OS trusts
    /// the same set.
    pub extra_roots: Vec<Vec<u8>>,
    /// The largest frame accepted from a relay, at most
    /// [`MAX_FRAME`](crate::MAX_FRAME); a larger one drops the connection.
    pub max_frame: usize,
}

impl Default for PoolConfig {
    fn default() -> PoolConfig {
        PoolConfig { relays: Vec::new(), extra_roots: Vec::new(), max_frame: MAX_FRAME }
    }
}

/// A relay connection's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayStatus {
    Connecting,
    Connected,
    /// The connection could not be made; retried after a backoff (unless
    /// the URL itself is bad).
    Failed(String),
    /// An established connection was lost or dropped for breaking a
    /// limit; retried after a backoff.
    Dropped(String),
}

/// What [`Pool::poll`] reports; `relay` indexes [`PoolConfig::relays`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolEvent {
    /// A verified event of one of our subscriptions, the first time any
    /// relay delivered it (lately).
    Event {
        relay: usize,
        event: Event,
    },
    /// The relay sent all stored events of `sub` (after every
    /// (re)subscription).
    Eose {
        relay: usize,
        sub: String,
    },
    /// The relay's verdict on an event we published.
    Ok {
        relay: usize,
        id: [u8; 32],
        accepted: bool,
        message: String,
    },
    Status {
        relay: usize,
        status: RelayStatus,
    },
}

/// The last [`SEEN_IDS`] event ids, least recently seen evicted first.
struct Seen {
    clock: u64,
    by_id: HashMap<[u8; 32], u64>,
    by_age: BTreeMap<u64, [u8; 32]>,
}

impl Seen {
    fn new() -> Seen {
        Seen { clock: 0, by_id: HashMap::new(), by_age: BTreeMap::new() }
    }

    /// Records `id` as just seen; true if it was not remembered.
    fn insert(&mut self, id: [u8; 32]) -> bool {
        self.clock += 1;
        let fresh = match self.by_id.insert(id, self.clock) {
            Some(old) => {
                self.by_age.remove(&old);
                false
            }
            None => true,
        };
        self.by_age.insert(self.clock, id);
        if self.by_id.len() > SEEN_IDS
            && let Some((_, oldest)) = self.by_age.pop_first()
        {
            self.by_id.remove(&oldest);
        }
        fresh
    }
}

/// Connections to a set of relays. Every method returns at once: the
/// sockets live on one driver thread per relay. Dropping the pool sends
/// what is queued, closes every connection and waits at most 2 s for the
/// threads (a thread stuck in a connect is left to finish on its own).
pub struct Pool {
    links: Vec<Arc<Link>>,
    threads: Vec<Option<JoinHandle<()>>>,
    seen: Seen,
}

fn tls_config(extra_roots: &[Vec<u8>]) -> Option<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for der in extra_roots {
        // An unusable extra root is ignored: its relay fails verification.
        let _ = roots.add(CertificateDer::from(der.clone()));
    }
    let config =
        rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .ok()?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Some(Arc::new(config))
}

/// The slot of a replaceable (0, 3, 10000-19999) or addressable
/// (30000-39999) event: queued, a newer one replaces it.
fn slot(e: &Event) -> Option<relay::Slot> {
    let replaceable = e.kind == 0 || e.kind == 3 || (10_000..20_000).contains(&e.kind);
    let addressable = (30_000..40_000).contains(&e.kind);
    (replaceable || addressable).then(|| (e.pubkey, e.kind, e.tag("d").unwrap_or("").to_owned()))
}

impl Pool {
    /// Starts one driver per relay; they connect in the background.
    pub fn start(cfg: PoolConfig) -> Pool {
        let driver =
            DriverConfig { tls: tls_config(&cfg.extra_roots), max_frame: cfg.max_frame.min(MAX_FRAME) };
        let mut links = Vec::with_capacity(cfg.relays.len());
        let mut threads = Vec::with_capacity(cfg.relays.len());
        for (index, url) in cfg.relays.into_iter().enumerate() {
            let link = Arc::new(Link::new(index, url));
            let (l, d) = (Arc::clone(&link), driver.clone());
            let spawned =
                thread::Builder::new().name(format!("relay-{index}")).spawn(move || relay::run(&l, &d));
            match spawned {
                Ok(h) => threads.push(Some(h)),
                Err(e) => {
                    link.set_status(RelayStatus::Failed(format!("no thread: {e}")));
                    threads.push(None);
                }
            }
            links.push(link);
        }
        Pool { links, threads, seen: Seen::new() }
    }

    /// Publishes `event` on every relay: sent at once to connected ones,
    /// queued for the others (at most 256 per relay, the oldest dropped;
    /// a queued replaceable or addressable event is replaced by a newer
    /// one for the same slot).
    pub fn publish(&mut self, event: Event) {
        let frame = encode_to_relay(&ToRelay::Event(event.clone()));
        for link in &self.links {
            link.queue(Outgoing { slot: slot(&event), id: event.id, frame: frame.clone() });
        }
    }

    /// Subscribes (or replaces subscription `sub`) on every relay; sent
    /// again after every reconnect.
    pub fn subscribe(&mut self, sub: &str, filters: Vec<Filter>) {
        for link in &self.links {
            link.set_sub(sub, Some(filters.clone()));
        }
    }

    /// Ends subscription `sub` on every relay.
    pub fn unsubscribe(&mut self, sub: &str) {
        for link in &self.links {
            link.set_sub(sub, None);
        }
    }

    /// Appends what the relays reported since the last poll to `out`:
    /// relay by relay in index order, each relay's reports in the order
    /// they arrived. An event already reported (by any relay, among the
    /// last 4096 ids) is left out.
    pub fn poll(&mut self, out: &mut Vec<PoolEvent>) {
        let mut batch = Vec::new();
        for link in &self.links {
            link.drain(&mut batch);
        }
        for ev in batch {
            if let PoolEvent::Event { event, .. } = &ev
                && !self.seen.insert(event.id)
            {
                continue;
            }
            out.push(ev);
        }
    }

    /// Each relay's URL and current state.
    pub fn statuses(&self) -> Vec<(String, RelayStatus)> {
        self.links.iter().map(|l| (l.url.clone(), l.lock().status.clone())).collect()
    }

    /// Events dropped because a relay delivered more than 4096 (or 8 MiB)
    /// between two polls.
    pub fn dropped_events(&self) -> u64 {
        self.links.iter().map(|l| l.lock().dropped).sum()
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for link in &self.links {
            link.stop();
        }
        let deadline = Instant::now() + JOIN_BOUND;
        for h in self.threads.iter_mut().filter_map(Option::take) {
            while !h.is_finished() && Instant::now() < deadline {
                thread::sleep(TICK / 10);
            }
            if h.is_finished() {
                let _ = h.join();
            }
            // A thread past the bound (in a connect) is detached.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> [u8; 32] {
        let mut id = [0; 32];
        id[..4].copy_from_slice(&n.to_le_bytes());
        id
    }

    #[test]
    fn seen_ids_are_a_bounded_lru() {
        let mut s = Seen::new();
        assert!(s.insert(id(0)));
        assert!(!s.insert(id(0)));
        for n in 1..SEEN_IDS as u32 {
            assert!(s.insert(id(n)));
        }
        // Full: 0 is the least recently seen; seeing it again saves it.
        assert!(!s.insert(id(0)));
        assert!(s.insert(id(100_000)));
        assert_eq!(s.by_id.len(), SEEN_IDS);
        assert_eq!(s.by_age.len(), SEEN_IDS);
        assert!(!s.insert(id(0)));
        // 1 was evicted instead.
        assert!(s.insert(id(1)));
    }
}
