//! The host's public address through STUN (RFC 5389, decision N40).
//!
//! Only the IP is wanted, so the binding request goes from an ephemeral UDP
//! socket, not the game socket (plan 12b's ICE uses that one). The socket is
//! bound to `0.0.0.0:0`, so only IPv4 servers are asked.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_MAPPED: u16 = 0x0001;
const ATTR_XOR_MAPPED: u16 = 0x0020;
/// The largest datagram read; a binding response is well under 100 bytes.
const MAX_DATAGRAM: usize = 548;
/// Time and tries per server.
const TRY_TIMEOUT: Duration = Duration::from_millis(500);
const TRIES: u32 = 2;

/// A binding request (RFC 5389 §6, §7.2.1): the 20-byte header, no attributes.
pub fn binding_request(transaction: [u8; 12]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    v.extend_from_slice(&0u16.to_be_bytes());
    v.extend_from_slice(&COOKIE);
    v.extend_from_slice(&transaction);
    v
}

/// The mapped address of a binding success response to `transaction`:
/// XOR-MAPPED-ADDRESS, or MAPPED-ADDRESS when there is no XOR one. The
/// address is returned as given (the caller judges whether it is usable).
/// Bounded and never panics: wrong type, cookie, transaction, a truncated
/// or overlong message or a malformed attribute yield `None`. The integrity
/// and fingerprint attributes are not checked.
pub fn parse_binding_response(bytes: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    if bytes.len() < 20 || bytes.len() > MAX_DATAGRAM {
        return None;
    }
    if u16::from_be_bytes([bytes[0], bytes[1]]) != BINDING_SUCCESS {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
    if len % 4 != 0 || bytes.len() != 20 + len || bytes[4..8] != COOKIE || bytes[8..20] != transaction[..] {
        return None;
    }
    let mut body = &bytes[20..];
    let mut plain = None;
    while !body.is_empty() {
        if body.len() < 4 {
            return None;
        }
        let ty = u16::from_be_bytes([body[0], body[1]]);
        let alen = usize::from(u16::from_be_bytes([body[2], body[3]]));
        let padded = alen.checked_add(3)? & !3;
        let value = body.get(4..4 + alen)?;
        body = body.get(4 + padded..)?;
        match ty {
            ATTR_XOR_MAPPED => return decode_address(value, true, transaction),
            ATTR_MAPPED if plain.is_none() => plain = Some(decode_address(value, false, transaction)),
            _ => {}
        }
    }
    plain.flatten()
}

/// An address attribute value: reserved byte, family, port, address.
fn decode_address(v: &[u8], xor: bool, transaction: &[u8; 12]) -> Option<SocketAddr> {
    let mask = |i: usize| -> u8 { if i < 4 { COOKIE[i] } else { transaction[i - 4] } };
    let mut port = u16::from_be_bytes([*v.get(2)?, *v.get(3)?]);
    if xor {
        port ^= u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
    }
    let addr = match *v.get(1)? {
        1 if v.len() == 8 => {
            let mut o = [0u8; 4];
            for (i, b) in o.iter_mut().enumerate() {
                *b = v[4 + i] ^ if xor { mask(i) } else { 0 };
            }
            IpAddr::V4(Ipv4Addr::from(o))
        }
        2 if v.len() == 20 => {
            let mut o = [0u8; 16];
            for (i, b) in o.iter_mut().enumerate() {
                *b = v[4 + i] ^ if xor { mask(i) } else { 0 };
            }
            IpAddr::V6(Ipv6Addr::from(o))
        }
        _ => return None,
    };
    Some(SocketAddr::new(addr, port))
}

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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread::{self, JoinHandle};

    /// A STUN server for tests (feature `test-relay`): answers binding
    /// requests on `127.0.0.1` with a configured mapped address, in an
    /// XOR-MAPPED-ADDRESS (or a MAPPED-ADDRESS, see [`Self::start_plain`]).
    /// Dropping it stops the thread.
    pub struct StunResponder {
        addr: SocketAddr,
        answered: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl StunResponder {
        /// Answers with XOR-MAPPED-ADDRESS `mapped`.
        pub fn start(mapped: SocketAddr) -> Self {
            Self::spawn(mapped, true)
        }

        /// Answers with the plain MAPPED-ADDRESS `mapped`.
        pub fn start_plain(mapped: SocketAddr) -> Self {
            Self::spawn(mapped, false)
        }

        fn spawn(mapped: SocketAddr, xor: bool) -> Self {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind the STUN responder");
            let _ = socket.set_read_timeout(Some(Duration::from_millis(20)));
            let addr = socket.local_addr().expect("responder address");
            let answered = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (a, s) = (answered.clone(), stop.clone());
            let thread = thread::spawn(move || {
                let mut buf = [0u8; 1500];
                while !s.load(Ordering::Relaxed) {
                    let Ok((len, from)) = socket.recv_from(&mut buf) else { continue };
                    let req = &buf[..len];
                    if len != 20 || req[..2] != BINDING_REQUEST.to_be_bytes() || req[4..8] != COOKIE {
                        continue;
                    }
                    let mut t = [0u8; 12];
                    t.copy_from_slice(&req[8..20]);
                    let _ = socket.send_to(&response(&t, mapped, xor), from);
                    a.fetch_add(1, Ordering::Relaxed);
                }
            });
            Self { addr, answered, stop, thread: Some(thread) }
        }

        /// `127.0.0.1:<port>`, as a `host:port` server string.
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

    /// A binding success response carrying `mapped` (also for tests of the parser).
    pub fn response(t: &[u8; 12], mapped: SocketAddr, xor: bool) -> Vec<u8> {
        let mask = |i: usize| -> u8 {
            if !xor {
                0
            } else if i < 4 {
                COOKIE[i]
            } else {
                t[i - 4]
            }
        };
        let (family, ip): (u8, Vec<u8>) = match mapped.ip() {
            IpAddr::V4(a) => (1, a.octets().to_vec()),
            IpAddr::V6(a) => (2, a.octets().to_vec()),
        };
        let port = if xor { mapped.port() ^ 0x2112 } else { mapped.port() };
        let mut attr = vec![0, family];
        attr.extend_from_slice(&port.to_be_bytes());
        attr.extend(ip.iter().enumerate().map(|(i, b)| b ^ mask(i)));
        let ty = if xor { ATTR_XOR_MAPPED } else { ATTR_MAPPED };
        let mut v = Vec::new();
        v.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
        v.extend_from_slice(&((attr.len() + 4) as u16).to_be_bytes());
        v.extend_from_slice(&COOKIE);
        v.extend_from_slice(t);
        v.extend_from_slice(&ty.to_be_bytes());
        v.extend_from_slice(&(attr.len() as u16).to_be_bytes());
        v.extend_from_slice(&attr);
        v
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
