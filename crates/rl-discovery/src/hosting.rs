//! Hosting a game others can find (plan 12a), shared by the dedicated
//! server and the game: the public address (decision N40) through
//! [`AddressWatch`], the invite code (N42) through [`invite`], and the
//! announcements' signing and timing (N39) through [`Publisher`].
//!
//! `AddressWatch` is a driver: it asks STUN on a thread of its own. The
//! rest is sans-IO (time is an argument).

use std::net::{IpAddr, SocketAddr};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use rl_lobby::Identity;
use rl_proto::{FINGERPRINT, PublicKey};

use crate::announce::{Announcer, GameSummary, announcement};
use crate::defaults::DEFAULT_RELAYS;
use crate::invite::{Invite, MAX_URL_BYTES, RelayHint};
use crate::list::is_global;
use crate::nostr::Event;

/// The public address is asked again this often (a home server's address
/// changes).
pub const ADDRESS_EVERY: Duration = Duration::from_secs(10 * 60);
/// How long one round of STUN questions may take.
pub const STUN_DEADLINE: Duration = Duration::from_secs(3);

/// A host's public address: the configured one, or what STUN answers (on a
/// thread, at the start and every [`ADDRESS_EVERY`]).
pub struct AddressWatch {
    servers: Vec<String>,
    current: Option<IpAddr>,
    asked: Option<Instant>,
    pending: Option<Receiver<Option<IpAddr>>>,
    /// The first question was answered (or none is asked).
    settled: bool,
}

impl AddressWatch {
    /// The configured address, or STUN's from `servers` (the first question
    /// asked at once). Also the line saying so for the log.
    pub fn start(configured: Option<IpAddr>, servers: &[String], now: Instant) -> (AddressWatch, String) {
        let mut w = AddressWatch {
            servers: servers.to_vec(),
            current: configured,
            asked: None,
            pending: None,
            settled: true,
        };
        let said = if let Some(ip) = configured {
            w.servers.clear();
            format!("public address {ip} (configured)")
        } else if servers.is_empty() {
            "no public address (neither public_address nor STUN servers configured)".into()
        } else {
            w.settled = false;
            w.ask(now);
            format!("asking STUN servers for the public address ({})", servers.join(", "))
        };
        (w, said)
    }

    /// The public address, when known.
    pub fn current(&self) -> Option<IpAddr> {
        self.current
    }

    /// The first question was answered (or none was asked).
    pub fn settled(&self) -> bool {
        self.settled
    }

    /// Takes STUN's answer, and asks again when it is time. Returns a line
    /// for the log when the address is first settled or changed (the invite
    /// changes with it). A later round that gets no answer keeps the
    /// address; one answering a non-public address drops it.
    pub fn poll(&mut self, now: Instant) -> Option<String> {
        if let Some(rx) = &self.pending {
            let answer = match rx.try_recv() {
                Ok(a) => a,
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => None,
            };
            self.pending = None;
            let first = !std::mem::replace(&mut self.settled, true);
            return self.learnt(answer, first);
        }
        if !self.servers.is_empty()
            && self.asked.is_none_or(|t| now.saturating_duration_since(t) >= ADDRESS_EVERY)
        {
            self.ask(now);
        }
        None
    }

    fn learnt(&mut self, answer: Option<IpAddr>, first: bool) -> Option<String> {
        let said = match answer {
            Some(ip) if is_global(ip) => {
                if self.current == Some(ip) {
                    return None;
                }
                self.current = Some(ip);
                format!("public address {ip} (STUN)")
            }
            Some(ip) => {
                if !first && self.current.is_none() {
                    return None;
                }
                self.current = None;
                format!("no public address (STUN answered {ip}, not a public address)")
            }
            None if first => "no public address (no STUN server answered)".into(),
            None => return None,
        };
        Some(said)
    }

    /// One round of questions on a thread of its own (bounded by
    /// [`STUN_DEADLINE`], but for one name lookup).
    fn ask(&mut self, now: Instant) {
        self.asked = Some(now);
        let (tx, rx) = mpsc::channel();
        let servers = self.servers.clone();
        // No thread: the receiver reports a disconnect, read as no answer.
        let _ = std::thread::Builder::new().name("stun".into()).spawn(move || {
            let _ = tx.send(crate::stun::public_ip(&servers, STUN_DEADLINE));
        });
        self.pending = Some(rx);
    }
}

/// The relay hints of an invite: the default relays among `relays` by
/// index, and the first other one short enough to hint. `Invite::encode`
/// keeps what fits (ruling PF12-6).
pub fn relay_hints(relays: &[String]) -> Vec<RelayHint> {
    let mut hints = Vec::new();
    let mut url = false;
    for r in relays {
        let r = r.trim_end_matches('/');
        match DEFAULT_RELAYS.iter().position(|d| *d == r).and_then(|i| u8::try_from(i).ok()) {
            Some(i) => hints.push(RelayHint::Default(i)),
            None if !url && r.len() <= MAX_URL_BYTES => {
                url = true;
                hints.push(RelayHint::Url(r.to_owned()));
            }
            None => {}
        }
    }
    hints
}

/// A host's invite: its key, its relays, and its public address on `port`
/// (codes hold IPv4 only).
pub fn invite(key: PublicKey, relays: &[String], addr: Option<IpAddr>, port: u16) -> Invite {
    let addr = match addr {
        Some(ip @ IpAddr::V4(_)) => Some(SocketAddr::new(ip, port)),
        _ => None,
    };
    Invite { key, relays: relay_hints(relays), addr }
}

/// When and how a hosted game's notes are published (N39): the
/// [`Announcer`]'s timing, signed by the host's identity, `created_at` on
/// the caller's clock from a unix time taken once (no wall clock in the
/// poll), and the closing note newer than the last note (a tie in the
/// second would let relays and lists keep the open one, as ties go by id).
pub struct Publisher {
    identity: Identity,
    announcer: Announcer,
    /// The unix time at an instant: `created_at` follows the caller's clock
    /// from there.
    clock: (Instant, u64),
    /// The last note's `created_at`.
    last_unix: Option<u64>,
}

impl Publisher {
    /// Notes signed by `identity`; `now_unix` is the unix time at `now`.
    pub fn new(identity: Identity, now: Instant, now_unix: u64) -> Publisher {
        Publisher { identity, announcer: Announcer::new(), clock: (now, now_unix), last_unix: None }
    }

    /// The unix time at `now`, on the caller's clock.
    pub fn unix(&self, now: Instant) -> u64 {
        self.clock.1.saturating_add(now.saturating_duration_since(self.clock.0).as_secs())
    }

    /// The note of `summary` to publish now, if one is due (every minute,
    /// at a change after 12 s); `aux` gives the signature's randomness.
    pub fn tick(
        &mut self,
        now: Instant,
        summary: &GameSummary,
        aux: impl FnOnce() -> [u8; 32],
    ) -> Option<Event> {
        if !self.announcer.tick(now, summary) {
            return None;
        }
        let unix = self.unix(now);
        self.last_unix = Some(unix);
        Some(announcement(&self.identity, summary, FINGERPRINT, unix, aux()))
    }

    /// Once, when a note was published: the closing note of `summary`
    /// (`closed: true`), at least a second newer than the last note.
    pub fn closing(
        &mut self,
        now: Instant,
        summary: &GameSummary,
        aux: impl FnOnce() -> [u8; 32],
    ) -> Option<Event> {
        if !self.announcer.closing() {
            return None;
        }
        let unix = self.unix(now).max(self.last_unix.unwrap_or(0).saturating_add(1));
        let summary = GameSummary { closed: true, ..summary.clone() };
        Some(announcement(&self.identity, &summary, FINGERPRINT, unix, aux()))
    }

    /// A note was published (and no closing note yet).
    pub fn published(&self) -> bool {
        self.last_unix.is_some()
    }

    /// The key the notes are signed with.
    pub fn key(&self) -> PublicKey {
        self.identity.public()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_name_default_relays_by_index_and_one_other() {
        let relays: Vec<String> = [
            "wss://relay.example.org",
            DEFAULT_RELAYS[3],
            "wss://relay.nos.lol.example",
            &format!("{}/", DEFAULT_RELAYS[0]),
        ]
        .iter()
        .map(|r| r.to_string())
        .collect();
        assert_eq!(
            relay_hints(&relays),
            vec![
                RelayHint::Url("wss://relay.example.org".into()),
                RelayHint::Default(3),
                RelayHint::Default(0)
            ]
        );
        // A URL too long to hint leaves the place to the next one.
        let long = format!("wss://{}.example.org", "a".repeat(80));
        let relays = vec![long, "ws://127.0.0.1:7000".to_string()];
        assert_eq!(relay_hints(&relays), vec![RelayHint::Url("ws://127.0.0.1:7000".into())]);
    }

    #[test]
    fn invites_carry_ipv4_addresses_only() {
        let v4: IpAddr = "81.2.69.142".parse().unwrap();
        let v6: IpAddr = "2a00:1450::1".parse().unwrap();
        assert_eq!(invite([1; 32], &[], Some(v4), 7).addr, Some(SocketAddr::new(v4, 7)));
        assert_eq!(invite([1; 32], &[], Some(v6), 7).addr, None);
        let code =
            invite([1; 32], &DEFAULT_RELAYS.iter().map(|r| r.to_string()).collect::<Vec<_>>(), Some(v4), 7);
        assert_eq!(Invite::decode(&code.encode()).unwrap().relays.len(), crate::invite::MAX_DEFAULT_HINTS);
    }

    #[test]
    fn the_watch_keeps_public_answers() {
        let now = Instant::now();
        let (mut w, said) = AddressWatch::start(None, &[], now);
        assert!(w.settled() && w.current().is_none() && said.starts_with("no public address"));
        assert_eq!(w.poll(now), None);
        let ip: IpAddr = "81.2.69.142".parse().unwrap();
        let (w, said) = AddressWatch::start(Some(ip), &["127.0.0.1:1".into()], now);
        assert_eq!((w.settled(), w.current()), (true, Some(ip)));
        assert_eq!(said, "public address 81.2.69.142 (configured)");

        // Answers as STUN's thread gives them.
        let mut w =
            AddressWatch { servers: vec![], current: None, asked: None, pending: None, settled: false };
        let private: IpAddr = "192.168.1.9".parse().unwrap();
        assert!(w.learnt(None, true).unwrap().contains("no STUN server answered"));
        assert_eq!(w.learnt(Some(ip), false).as_deref(), Some("public address 81.2.69.142 (STUN)"));
        assert_eq!(w.learnt(Some(ip), false), None, "the same address: nothing to say");
        assert_eq!(w.learnt(None, false), None, "no answer keeps it");
        assert_eq!(w.current(), Some(ip));
        assert!(w.learnt(Some(private), false).unwrap().contains("not a public address"));
        assert_eq!(w.current(), None);
        assert_eq!(w.learnt(Some(private), false), None);
    }

    fn summary() -> GameSummary {
        GameSummary {
            v: rl_proto::PROTOCOL_VERSION,
            name: "Den".into(),
            mode: "deathmatch".into(),
            map: "olx-level-dirt-level".into(),
            map_name: "Dirt Level".into(),
            mod_id: "olx-classic".into(),
            mod_hash: rl_proto::NO_HASH.into(),
            players: 1,
            max_players: 8,
            spectators: 0,
            max_spectators: 4,
            password: false,
            region: crate::Region::Europe,
            playing: false,
            addr: None,
            port: 23400,
            closed: false,
        }
    }

    #[test]
    fn the_publisher_signs_due_notes_and_closes_once_newer() {
        let id = Identity::from_secret([7; 32]).unwrap();
        let t0 = Instant::now();
        let mut p = Publisher::new(id.clone(), t0, 1_000_000);
        assert_eq!(p.key(), id.public());
        assert!(!p.published());
        let s = summary();
        let first = p.tick(t0, &s, || [1; 32]).expect("the first note at once");
        assert!(first.verify() && first.pubkey == id.public());
        assert_eq!(first.created_at, 1_000_000);
        assert!(p.published());
        assert!(p.tick(t0 + Duration::from_secs(5), &s, || unreachable!("not due")).is_none());
        let later = p.tick(t0 + Duration::from_secs(60), &s, || [2; 32]).expect("the minute's note");
        assert_eq!(later.created_at, 1_000_060, "on the caller's clock");
        // Closing in the same second: one newer.
        let close = p.closing(t0 + Duration::from_secs(60), &s, || [3; 32]).expect("a closing note");
        assert_eq!(close.created_at, 1_000_061);
        let (closed, _) = crate::parse_announcement(&close).unwrap();
        assert!(closed.closed);
        assert!(p.closing(t0 + Duration::from_secs(61), &s, || [4; 32]).is_none(), "once");
        assert!(p.tick(t0 + Duration::from_secs(200), &s, || [5; 32]).is_none(), "nothing after closing");
        // Nothing published: nothing to close.
        let mut q = Publisher::new(id, t0, 5);
        assert!(q.closing(t0, &s, || [6; 32]).is_none());
    }
}
