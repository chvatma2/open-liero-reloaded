//! Finding the server (plan 12a): its public address (decision N40) and
//! its invite code (N42).

use std::net::{IpAddr, SocketAddr};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use rl_discovery::defaults::DEFAULT_RELAYS;
use rl_discovery::list::is_global;
use rl_discovery::{Invite, MAX_URL_BYTES, RelayHint};
use rl_proto::PublicKey;

/// The public address is asked again this often (a home server's address
/// changes).
pub const ADDRESS_EVERY: Duration = Duration::from_secs(10 * 60);
/// How long one round of STUN questions may take.
pub const STUN_DEADLINE: Duration = Duration::from_secs(3);

/// The server's public address: the configured one, or what STUN answers
/// (on a thread, at the start and every [`ADDRESS_EVERY`]).
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
            let _ = tx.send(rl_discovery::public_ip(&servers, STUN_DEADLINE));
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

/// The server's invite: its key, its relays, and its public address on
/// `port` (codes hold IPv4 only).
pub fn invite(key: PublicKey, relays: &[String], addr: Option<IpAddr>, port: u16) -> Invite {
    let addr = match addr {
        Some(ip @ IpAddr::V4(_)) => Some(SocketAddr::new(ip, port)),
        _ => None,
    };
    Invite { key, relays: relay_hints(relays), addr }
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
        assert_eq!(Invite::decode(&code.encode()).unwrap().relays.len(), rl_discovery::MAX_DEFAULT_HINTS);
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
}
