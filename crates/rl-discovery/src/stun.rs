//! The host's public address through STUN (RFC 5389, decision N40).
//!
//! Only the IP is wanted, so the binding request goes from an ephemeral UDP
//! socket, not the game socket (rl-net's ICE gathering uses that one). The
//! socket is bound to `0.0.0.0:0`, so only IPv4 servers are asked. The
//! binding codec lives in `rl_proto::stun` (decision N57) and is
//! re-exported here.

use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

pub use rl_proto::stun::{binding_request, parse_binding_response};

/// The largest datagram read; a binding response is well under 100 bytes.
const MAX_DATAGRAM: usize = 548;
/// Time and tries per server.
const TRY_TIMEOUT: Duration = Duration::from_millis(500);
const TRIES: u32 = 2;

/// A transaction id that differs per call and process (std's randomly keyed
/// hasher; no randomness crate and no clock).
fn new_transaction(salt: u64) -> [u8; 12] {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut h = RandomState::new().build_hasher();
    h.write_u64(salt);
    let a = h.finish().to_be_bytes();
    h.write_u64(salt.wrapping_add(1));
    let b = h.finish().to_be_bytes();
    let mut t = [0u8; 12];
    t[..8].copy_from_slice(&a);
    t[8..].copy_from_slice(&b[..4]);
    t
}

/// Asks each server (`host:port`) in turn, 500 ms per try and 2 tries each,
/// from an ephemeral UDP socket on `0.0.0.0:0`; returns the IP of the first
/// answer. Blocks for at most `deadline` plus one name lookup (no lookup
/// starts past the deadline, but the system's resolution of a name may take
/// longer): call it on a thread. Only a reply from the server's own address
/// with our transaction counts. IPv4 servers only. The address is returned as the server gave
/// it, even a private one; callers decide with `list::is_global`.
pub fn public_ip(servers: &[String], deadline: Duration) -> Option<IpAddr> {
    let end = Instant::now().checked_add(deadline)?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    let mut buf = [0u8; MAX_DATAGRAM + 1];
    for (n, server) in servers.iter().enumerate() {
        // A lookup may block: none is started past the deadline.
        if Instant::now() >= end {
            return None;
        }
        let Some(target) = server.to_socket_addrs().ok().and_then(|mut a| a.find(SocketAddr::is_ipv4)) else {
            continue;
        };
        let transaction = new_transaction(n as u64);
        let request = binding_request(transaction);
        for _ in 0..TRIES {
            let try_end = end.min(Instant::now() + TRY_TIMEOUT);
            if socket.send_to(&request, target).is_err() {
                break;
            }
            loop {
                let left = try_end.saturating_duration_since(Instant::now());
                if left.is_zero() || socket.set_read_timeout(Some(left)).is_err() {
                    break;
                }
                match socket.recv_from(&mut buf) {
                    Ok((len, from)) if from == target => {
                        if let Some(a) = parse_binding_response(&buf[..len], &transaction) {
                            return Some(a.ip());
                        }
                    }
                    Ok(_) => {}
                    Err(e) if waits_on(&e) => {}
                    Err(_) => break,
                }
            }
            if Instant::now() >= end {
                return None;
            }
        }
    }
    None
}

/// Whether a failed `recv_from` still waits for the current server's
/// answer: on Windows an ICMP port-unreachable caused by an earlier
/// server's request fails the next read with `WSAECONNRESET`
/// (`ConnectionReset`; `ConnectionRefused` elsewhere), though this
/// server's answer may still come. Anything else (a timeout) ends the try.
fn waits_on(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused)
}

#[cfg(feature = "test-relay")]
pub use responder::{StunResponder, response};

#[cfg(feature = "test-relay")]
mod responder {
    use super::*;
    use rl_proto::stun::binding_request_transaction;
    pub use rl_proto::stun::binding_response as response;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread::{self, JoinHandle};

    /// A STUN server for tests (feature `test-relay`): answers binding
    /// requests on `127.0.0.1` with a configured mapped address, in an
    /// XOR-MAPPED-ADDRESS (or a MAPPED-ADDRESS, see [`Self::start_plain`]),
    /// or on a given address with each request's observed source
    /// ([`Self::reflecting`], `rl-cli stun-server`). Dropping it stops the
    /// thread.
    pub struct StunResponder {
        addr: SocketAddr,
        answered: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    /// What a responder answers.
    #[derive(Clone, Copy)]
    enum Answer {
        /// This address, XOR-mapped or plain.
        Fixed(SocketAddr, bool),
        /// The request's source, XOR-mapped (a real server's answer).
        Source,
    }

    impl StunResponder {
        /// Answers with XOR-MAPPED-ADDRESS `mapped`.
        pub fn start(mapped: SocketAddr) -> Self {
            Self::spawn(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Answer::Fixed(mapped, true))
                .expect("bind the STUN responder")
        }

        /// Answers with the plain MAPPED-ADDRESS `mapped`.
        pub fn start_plain(mapped: SocketAddr) -> Self {
            Self::spawn(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Answer::Fixed(mapped, false))
                .expect("bind the STUN responder")
        }

        /// Listens on `bind` (port 0: a free one) and answers each request
        /// with its observed source in an XOR-MAPPED-ADDRESS, as a public
        /// STUN server does (`rl-cli stun-server`, the Docker NAT tests).
        /// An error when `bind` cannot be bound.
        pub fn reflecting(bind: SocketAddr) -> io::Result<Self> {
            Self::spawn(bind, Answer::Source)
        }

        fn spawn(bind: SocketAddr, answer: Answer) -> io::Result<Self> {
            let socket = UdpSocket::bind(bind)?;
            socket.set_read_timeout(Some(Duration::from_millis(20)))?;
            let addr = socket.local_addr()?;
            let answered = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (a, s) = (answered.clone(), stop.clone());
            let thread = thread::spawn(move || {
                let mut buf = [0u8; 1500];
                while !s.load(Ordering::Relaxed) {
                    let Ok((len, from)) = socket.recv_from(&mut buf) else { continue };
                    let Some(t) = binding_request_transaction(&buf[..len]) else { continue };
                    let reply = match answer {
                        Answer::Fixed(mapped, xor) => response(&t, mapped, xor),
                        Answer::Source => response(&t, from, true),
                    };
                    let _ = socket.send_to(&reply, from);
                    a.fetch_add(1, Ordering::Relaxed);
                }
            });
            Ok(Self { addr, answered, stop, thread: Some(thread) })
        }

        /// The address it listens on (as bound: `0.0.0.0` for every
        /// interface).
        pub fn local_addr(&self) -> SocketAddr {
            self.addr
        }

        /// Its address as a `host:port` server string (`127.0.0.1:<port>`
        /// for [`Self::start`]).
        pub fn server(&self) -> String {
            self.addr.to_string()
        }

        /// How many requests were answered.
        pub fn answered(&self) -> usize {
            self.answered.load(Ordering::Relaxed)
        }
    }

    impl Drop for StunResponder {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On Windows an ICMP port-unreachable from an earlier server makes the
    /// next `recv_from` fail with `WSAECONNRESET`: the wait for the current
    /// server's answer goes on (final review A-M8). A timeout ends it.
    #[test]
    fn a_stale_connection_reset_keeps_waiting() {
        use std::io::ErrorKind;
        for kind in [ErrorKind::ConnectionReset, ErrorKind::ConnectionRefused] {
            assert!(waits_on(&io::Error::from(kind)), "{kind:?}");
        }
        for kind in [ErrorKind::WouldBlock, ErrorKind::TimedOut, ErrorKind::Other] {
            assert!(!waits_on(&io::Error::from(kind)), "{kind:?}");
        }
    }
}
