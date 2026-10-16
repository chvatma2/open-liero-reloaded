//! The STUN binding codec (RFC 5389 §6, §7.2.1, §15.1-15.2), shared by
//! olr-discovery's public-IP lookup and olr-net's candidate gathering on the
//! game socket (decision N57). Only what a binding needs: the request, the
//! success response's mapped address, and telling STUN from other traffic
//! on a shared socket. `core::net`, no allocation beyond the `Vec`s made.

use alloc::vec::Vec;
use core::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_MAPPED: u16 = 0x0001;
const ATTR_XOR_MAPPED: u16 = 0x0020;
/// The largest binding response parsed; one is well under 100 bytes.
const MAX_RESPONSE: usize = 548;

/// A binding request (RFC 5389 §6, §7.2.1): the 20-byte header, no attributes.
pub fn binding_request(transaction: [u8; 12]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    v.extend_from_slice(&0u16.to_be_bytes());
    v.extend_from_slice(&COOKIE);
    v.extend_from_slice(&transaction);
    v
}

/// Whether `bytes` is a STUN message (RFC 5389 §6): the first two bits 0,
/// the magic cookie at 4..8, and a length, a multiple of 4, matching the
/// bytes after the 20-byte header. DTLS (first byte 20..=63) and SCTP over
/// DTLS never pass.
pub fn is_stun(bytes: &[u8]) -> bool {
    bytes.len() >= 20 && bytes[0] & 0xc0 == 0 && bytes[4..8] == COOKIE && {
        let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        len % 4 == 0 && bytes.len() == 20 + len
    }
}

/// The transaction of a binding request (`None` for anything else, an ICE
/// connectivity check included: it is a binding request with attributes).
pub fn binding_request_transaction(bytes: &[u8]) -> Option<[u8; 12]> {
    if !is_stun(bytes) || u16::from_be_bytes([bytes[0], bytes[1]]) != BINDING_REQUEST {
        return None;
    }
    bytes[8..20].try_into().ok()
}

/// The mapped address of a binding success response to `transaction`:
/// XOR-MAPPED-ADDRESS, or MAPPED-ADDRESS when there is no XOR one. The
/// address is returned as given (the caller judges whether it is usable).
/// Bounded and never panics: wrong type, cookie, transaction, a truncated
/// or overlong message or a malformed attribute yield `None`. The integrity
/// and fingerprint attributes are not checked.
pub fn parse_binding_response(bytes: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    if bytes.len() < 20 || bytes.len() > MAX_RESPONSE {
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

/// The XOR mask of byte `i` of an address (RFC 5389 §15.2): the cookie,
/// then the transaction.
fn mask(i: usize, transaction: &[u8; 12]) -> u8 {
    if i < 4 { COOKIE[i] } else { transaction[i - 4] }
}

/// An address attribute value: reserved byte, family, port, address.
fn decode_address(v: &[u8], xor: bool, transaction: &[u8; 12]) -> Option<SocketAddr> {
    let mut port = u16::from_be_bytes([*v.get(2)?, *v.get(3)?]);
    if xor {
        port ^= u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
    }
    let addr = match *v.get(1)? {
        1 if v.len() == 8 => {
            let mut o = [0u8; 4];
            for (i, b) in o.iter_mut().enumerate() {
                *b = v[4 + i] ^ if xor { mask(i, transaction) } else { 0 };
            }
            IpAddr::V4(Ipv4Addr::from(o))
        }
        2 if v.len() == 20 => {
            let mut o = [0u8; 16];
            for (i, b) in o.iter_mut().enumerate() {
                *b = v[4 + i] ^ if xor { mask(i, transaction) } else { 0 };
            }
            IpAddr::V6(Ipv6Addr::from(o))
        }
        _ => return None,
    };
    Some(SocketAddr::new(addr, port))
}

/// A binding success response to `transaction` carrying `mapped`, in an
/// XOR-MAPPED-ADDRESS (`xor`) or a MAPPED-ADDRESS: what a STUN server
/// answers (the test servers of olr-discovery and olr-net's `natsim`).
pub fn binding_response(transaction: &[u8; 12], mapped: SocketAddr, xor: bool) -> Vec<u8> {
    let mut attr = Vec::with_capacity(20);
    attr.extend_from_slice(&[0, if mapped.is_ipv4() { 1 } else { 2 }]);
    let port = if xor { mapped.port() ^ 0x2112 } else { mapped.port() };
    attr.extend_from_slice(&port.to_be_bytes());
    let ip = match mapped.ip() {
        IpAddr::V4(a) => a.octets().to_vec(),
        IpAddr::V6(a) => a.octets().to_vec(),
    };
    attr.extend(ip.iter().enumerate().map(|(i, b)| b ^ if xor { mask(i, transaction) } else { 0 }));
    let ty = if xor { ATTR_XOR_MAPPED } else { ATTR_MAPPED };
    let mut v = Vec::with_capacity(24 + attr.len());
    v.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
    v.extend_from_slice(&((attr.len() + 4) as u16).to_be_bytes());
    v.extend_from_slice(&COOKIE);
    v.extend_from_slice(transaction);
    v.extend_from_slice(&ty.to_be_bytes());
    v.extend_from_slice(&(attr.len() as u16).to_be_bytes());
    v.extend_from_slice(&attr);
    v
}
