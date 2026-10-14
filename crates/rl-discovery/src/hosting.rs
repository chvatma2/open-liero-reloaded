//! Hosting a game others can find (plan 12a), shared by the dedicated
//! server and the game: the invite code (N42) through [`invite`], and the
//! announcements' signing and timing (N39) through [`Publisher`]. A host's
//! address (plan 12b, N47) is its reachability's (`rl-online`), never one
//! STUN learnt. Sans-IO (time is an argument).

use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use rl_lobby::Identity;
use rl_proto::{FINGERPRINT, PublicKey};

use crate::announce::{Announcer, GameSummary, announcement};
use crate::defaults::DEFAULT_RELAYS;
use crate::invite::{Invite, MAX_URL_BYTES, RelayHint};
use crate::list::is_global;
use crate::nostr::Event;

/// Whether every relay is on this machine or a private network (a relay
/// of one's own, or the tests'): lists then take games announcing local
/// addresses too (ruling PF12-5's `allow_local`, PF12-8), and a host may
/// announce a private address on them. A public relay among them keeps
/// both to public addresses. A relay named by a host name other than
/// `localhost` counts as public (its address is not known without a
/// lookup).
pub fn local_relays(relays: &[String]) -> bool {
    let local = |url: &str| {
        let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://")).unwrap_or(url);
        let authority = rest.split('/').next().unwrap_or("");
        let host = match authority.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or(""),
            None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
        };
        host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().is_ok_and(|ip| !is_global(ip))
    };
    !relays.is_empty() && relays.iter().all(|r| local(r))
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
/// [`Announcer`]'s timing, signed by the host's identity, `created_at` the
/// wall-clock time the caller reads at each note (a monotonic clock
/// anchored once falls behind after a suspend or an NTP step, and every
/// note would be born expired: final review A-I2), and the closing note
/// newer than the last note (a tie in the second would let relays and
/// lists keep the open one, as ties go by id).
pub struct Publisher {
    identity: Identity,
    announcer: Announcer,
    /// The earliest `created_at` of the first note (after a closing note
    /// of the same key).
    floor: u64,
    /// The last note's `created_at`.
    last_unix: Option<u64>,
}

impl Publisher {
    /// Notes signed by `identity`. `after` is the earliest date of the
    /// first note: a game hosted again right after its closing note dates
    /// its first note after it (relays keep the newest note of a slot, and
    /// lists refuse a note older than the tombstone; final review A-M6).
    pub fn new(identity: Identity, after: Option<u64>) -> Publisher {
        Publisher { identity, announcer: Announcer::new(), floor: after.unwrap_or(0), last_unix: None }
    }

    /// The note of `summary` to publish now, if one is due (every minute,
    /// at a change after 12 s); `now_unix` is the wall-clock time now,
    /// `aux` gives the signature's randomness.
    pub fn tick(
        &mut self,
        now: Instant,
        now_unix: u64,
        summary: &GameSummary,
        aux: impl FnOnce() -> [u8; 32],
    ) -> Option<Event> {
        if !self.announcer.tick(now, summary) {
            return None;
        }
        let unix = if self.last_unix.is_none() { now_unix.max(self.floor) } else { now_unix };
        self.last_unix = Some(unix);
        Some(announcement(&self.identity, summary, FINGERPRINT, unix, aux()))
    }

    /// Once, when a note was published: the closing note of `summary`
    /// (`closed: true`), dated `now_unix` but at least a second newer than
    /// the last note.
    pub fn closing(
        &mut self,
        now_unix: u64,
        summary: &GameSummary,
        aux: impl FnOnce() -> [u8; 32],
    ) -> Option<Event> {
        if !self.announcer.closing() {
            return None;
        }
        let unix = now_unix.max(self.last_unix.unwrap_or(0).saturating_add(1));
        let summary = GameSummary { closed: true, ..summary.clone() };
        Some(announcement(&self.identity, &summary, FINGERPRINT, unix, aux()))
    }

    /// Whether a note was published, so a closing note is due at the end
    /// (it stays true once the closing note went out too).
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
    use std::time::Duration;

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
    fn local_relays_allow_local_games() {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(local_relays(&v(&["ws://127.0.0.1:7000"])));
        assert!(local_relays(&v(&["wss://localhost:7000/x", "ws://192.168.1.4", "ws://[::1]:80"])));
        assert!(!local_relays(&v(&["ws://127.0.0.1:7000", "wss://relay.damus.io"])));
        assert!(!local_relays(&v(&["wss://81.2.69.142"])));
        assert!(!local_relays(&[]), "no relays: nothing to allow");
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
        let mut p = Publisher::new(id.clone(), None);
        assert_eq!(p.key(), id.public());
        assert!(!p.published());
        let s = summary();
        let first = p.tick(t0, 1_000_000, &s, || [1; 32]).expect("the first note at once");
        assert!(first.verify() && first.pubkey == id.public());
        assert_eq!(first.created_at, 1_000_000);
        assert!(p.published());
        assert!(p.tick(t0 + Duration::from_secs(5), 1_000_005, &s, || unreachable!("not due")).is_none());
        let later =
            p.tick(t0 + Duration::from_secs(60), 1_000_060, &s, || [2; 32]).expect("the minute's note");
        assert_eq!(later.created_at, 1_000_060, "on the wall clock given");
        // Closing in the same second: one newer.
        let close = p.closing(1_000_060, &s, || [3; 32]).expect("a closing note");
        assert_eq!(close.created_at, 1_000_061);
        let (closed, _) = crate::parse_announcement(&close).unwrap();
        assert!(closed.closed);
        assert!(p.closing(1_000_061, &s, || [4; 32]).is_none(), "once");
        assert!(
            p.tick(t0 + Duration::from_secs(200), 1_000_200, &s, || [5; 32]).is_none(),
            "nothing after closing"
        );
        // Nothing published: nothing to close.
        let mut q = Publisher::new(id, None);
        assert!(q.closing(5, &s, || [6; 32]).is_none());
    }

    /// The notes are dated by the wall clock the caller reads at each
    /// note, not by a monotonic clock anchored once: after a suspend (the
    /// monotonic clock stood still) or an NTP step, the next note is
    /// current, never born expired (final review A-I2).
    #[test]
    fn notes_follow_the_wall_clock_across_a_suspend() {
        let id = Identity::from_secret([7; 32]).unwrap();
        let t0 = Instant::now();
        let mut p = Publisher::new(id, None);
        let s = summary();
        p.tick(t0, 1_000_000, &s, || [1; 32]).unwrap();
        // Ten minutes asleep: the monotonic clock moved 60 s, the wall 660 s.
        let woke = p.tick(t0 + Duration::from_secs(60), 1_000_660, &s, || [2; 32]).unwrap();
        assert_eq!(woke.created_at, 1_000_660);
        assert!(woke.expiration().is_some_and(|x| x > 1_000_660), "not born expired");
        // The wall clock stepped back a little: the closing note still beats
        // the last note.
        let close = p.closing(1_000_650, &s, || [3; 32]).unwrap();
        assert_eq!(close.created_at, 1_000_661);
    }

    /// A game hosted again right after its closing note dates its first
    /// note `floor` at the earliest (final review A-M6): relays keep the
    /// newest note of a slot, and lists refuse one older than the
    /// tombstone.
    #[test]
    fn a_game_hosted_again_dates_its_first_note_after_the_floor() {
        let id = Identity::from_secret([7; 32]).unwrap();
        let t0 = Instant::now();
        let s = summary();
        let mut p = Publisher::new(id.clone(), Some(1_000_011));
        let first = p.tick(t0, 1_000_001, &s, || [1; 32]).unwrap();
        assert_eq!(first.created_at, 1_000_011);
        let later = p.tick(t0 + Duration::from_secs(60), 1_000_061, &s, || [2; 32]).unwrap();
        assert_eq!(later.created_at, 1_000_061, "the wall clock again once past the floor");
        assert_eq!(Publisher::new(id, Some(5)).tick(t0, 9, &s, || [3; 32]).unwrap().created_at, 9);
    }
}
