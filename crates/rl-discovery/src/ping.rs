//! Timing a listed game (decision N41): a TCP connect to its address,
//! timed, on a thread, at most [`PING_TIMEOUT`]. A driver: the threads are
//! here, the caller only polls.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use rl_proto::PublicKey;

use crate::list::Ping;

/// A ping gives up after this long ([`Ping::NoAnswer`]).
pub const PING_TIMEOUT: Duration = Duration::from_secs(2);
/// At most this many pings run at once.
pub const MAX_PINGS: usize = 4;

/// Pings on threads of their own; [`Pinger::poll`] collects the answers.
pub struct Pinger {
    tx: Sender<(PublicKey, Ping)>,
    rx: Receiver<(PublicKey, Ping)>,
    running: BTreeSet<PublicKey>,
}

impl Default for Pinger {
    fn default() -> Pinger {
        Pinger::new()
    }
}

impl Pinger {
    pub fn new() -> Pinger {
        let (tx, rx) = mpsc::channel();
        Pinger { tx, rx, running: BTreeSet::new() }
    }

    /// Times a TCP connect to `addr` for the game of `key`, unless one runs
    /// for it already or [`MAX_PINGS`] run. Whether it started.
    pub fn ping(&mut self, key: PublicKey, addr: SocketAddr) -> bool {
        if self.running.contains(&key) || self.running.len() >= MAX_PINGS {
            return false;
        }
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new().name("ping".into()).spawn(move || {
            let start = Instant::now();
            let ping = match TcpStream::connect_timeout(&addr, PING_TIMEOUT) {
                Ok(_) => Ping::Ms(u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX)),
                Err(_) => Ping::NoAnswer,
            };
            let _ = tx.send((key, ping));
        });
        if spawned.is_ok() {
            self.running.insert(key);
        }
        spawned.is_ok()
    }

    /// Whether a ping for `key` runs.
    pub fn running(&self, key: &PublicKey) -> bool {
        self.running.contains(key)
    }

    /// The answers since the last poll.
    pub fn poll(&mut self) -> Vec<(PublicKey, Ping)> {
        let answers: Vec<_> = self.rx.try_iter().collect();
        for (key, _) in &answers {
            self.running.remove(key);
        }
        answers
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    fn wait(p: &mut Pinger) -> Vec<(PublicKey, Ping)> {
        let until = Instant::now() + PING_TIMEOUT + Duration::from_secs(3);
        loop {
            let got = p.poll();
            if !got.is_empty() || Instant::now() > until {
                return got;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_listening_game_answers_and_a_closed_port_does_not() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let open = listener.local_addr().unwrap();
        let mut p = Pinger::new();
        let started = Instant::now();
        assert!(p.ping([1; 32], open));
        assert!(!p.ping([1; 32], open), "one at a time per game");
        assert!(p.running(&[1; 32]));
        assert!(started.elapsed() < Duration::from_millis(100), "never blocks the caller");
        let got = wait(&mut p);
        assert!(matches!(got[..], [([1, ..], Ping::Ms(ms))] if ms < 1000), "{got:?}");
        assert!(!p.running(&[1; 32]));
        // A port nobody listens on: refused at once.
        drop(listener);
        assert!(p.ping([2; 32], open));
        assert_eq!(wait(&mut p), vec![([2; 32], Ping::NoAnswer)]);
    }

    #[test]
    fn pings_are_bounded() {
        let mut p = Pinger::new();
        // TEST-NET-1 is never routed: each ping waits out its timeout.
        let addr: SocketAddr = "192.0.2.1:9".parse().unwrap();
        for k in 0..MAX_PINGS as u8 {
            assert!(p.ping([k; 32], addr));
        }
        assert!(!p.ping([200; 32], addr), "at most MAX_PINGS at once");
    }
}
