//! The STUN/TURN message codec of our relay (ruling TR-2; RFC 8489 §5,
//! §14, RFC 8656 §12, §18): parsing a request or indication (header,
//! attributes, MESSAGE-INTEGRITY and FINGERPRINT checks) and building
//! answers. Bounded and never panics on input: at most `MAX_ATTRIBUTES`
//! attributes, every length checked.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ops::Range;

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;

pub(crate) const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const HEADER: usize = 20;
/// More attributes than this and a message is not ours to parse.
const MAX_ATTRIBUTES: usize = 32;
const FINGERPRINT_XOR: u32 = 0x5354_554e;

// Methods (RFC 8489 §18.2, RFC 8656 §17).
pub(crate) const BINDING: u16 = 0x001;
pub(crate) const ALLOCATE: u16 = 0x003;
pub(crate) const REFRESH: u16 = 0x004;
pub(crate) const SEND: u16 = 0x006;
pub(crate) const DATA: u16 = 0x007;
pub(crate) const CREATE_PERMISSION: u16 = 0x008;
pub(crate) const CHANNEL_BIND: u16 = 0x009;

// Attributes (RFC 8489 §18.3, RFC 8656 §18).
pub(crate) const USERNAME: u16 = 0x0006;
pub(crate) const MESSAGE_INTEGRITY: u16 = 0x0008;
pub(crate) const ERROR_CODE: u16 = 0x0009;
pub(crate) const UNKNOWN_ATTRIBUTES: u16 = 0x000a;
pub(crate) const CHANNEL_NUMBER: u16 = 0x000c;
pub(crate) const LIFETIME: u16 = 0x000d;
pub(crate) const XOR_PEER_ADDRESS: u16 = 0x0012;
pub(crate) const DATA_ATTR: u16 = 0x0013;
pub(crate) const REALM: u16 = 0x0014;
pub(crate) const NONCE: u16 = 0x0015;
pub(crate) const XOR_RELAYED_ADDRESS: u16 = 0x0016;
pub(crate) const REQUESTED_ADDRESS_FAMILY: u16 = 0x0017;
pub(crate) const REQUESTED_TRANSPORT: u16 = 0x0019;
pub(crate) const DONT_FRAGMENT: u16 = 0x001a;
pub(crate) const XOR_MAPPED_ADDRESS: u16 = 0x0020;
pub(crate) const FINGERPRINT: u16 = 0x8028;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Request,
    Indication,
    Success,
    Error,
}

impl Class {
    fn bits(self) -> u16 {
        match self {
            Class::Request => 0b00,
            Class::Indication => 0b01,
            Class::Success => 0b10,
            Class::Error => 0b11,
        }
    }
}

/// The message type of `class` and `method` (RFC 8489 §5: the class bits
/// sit at 4 and 8 between the method's).
fn message_type(class: Class, method: u16) -> u16 {
    let c = class.bits();
    (method & 0x000f) | ((method & 0x0070) << 1) | ((method & 0x0f80) << 2) | ((c & 1) << 4) | ((c & 2) << 7)
}

/// The class and method of a message type.
fn split_type(t: u16) -> (Class, u16) {
    let method = (t & 0x000f) | ((t & 0x00e0) >> 1) | ((t & 0x3e00) >> 2);
    let class = match ((t >> 4) & 1) | ((t >> 7) & 2) {
        0 => Class::Request,
        1 => Class::Indication,
        2 => Class::Success,
        _ => Class::Error,
    };
    (class, method)
}

/// A parsed message: its header and its attributes' value ranges in `bytes`.
pub(crate) struct Msg<'a> {
    pub(crate) class: Class,
    pub(crate) method: u16,
    pub(crate) transaction: [u8; 12],
    bytes: &'a [u8],
    attrs: Vec<(u16, Range<usize>)>,
    /// Where MESSAGE-INTEGRITY's attribute header starts.
    integrity: Option<usize>,
}

impl<'a> Msg<'a> {
    /// Parses a STUN message: the header (first bits 0, the cookie, a
    /// length matching the datagram), attributes up to MESSAGE-INTEGRITY
    /// (only FINGERPRINT may follow it), and FINGERPRINT, if present, last
    /// and correct. `None` for anything else.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Msg<'a>> {
        if bytes.len() < HEADER || bytes[0] & 0xc0 != 0 || bytes[4..8] != COOKIE {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
        if len % 4 != 0 || bytes.len() != HEADER + len {
            return None;
        }
        let (class, method) = split_type(u16::from_be_bytes([bytes[0], bytes[1]]));
        let transaction: [u8; 12] = bytes[8..20].try_into().ok()?;
        let mut attrs = Vec::new();
        let mut integrity = None;
        let mut at = HEADER;
        while at < bytes.len() {
            if attrs.len() >= MAX_ATTRIBUTES || at + 4 > bytes.len() {
                return None;
            }
            let ty = u16::from_be_bytes([bytes[at], bytes[at + 1]]);
            let vlen = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
            let value = at + 4..at + 4 + vlen;
            let next = value.start + vlen.div_ceil(4) * 4;
            if next > bytes.len() {
                return None;
            }
            if ty == FINGERPRINT {
                if next != bytes.len() || vlen != 4 {
                    return None;
                }
                let want = u32::from_be_bytes(bytes[value.clone()].try_into().ok()?);
                if crc32fast::hash(&bytes[..at]) ^ FINGERPRINT_XOR != want {
                    return None;
                }
            } else if integrity.is_some() {
                // After MESSAGE-INTEGRITY only FINGERPRINT counts.
                at = next;
                continue;
            } else if ty == MESSAGE_INTEGRITY {
                integrity = Some(at);
            }
            attrs.push((ty, value));
            at = next;
        }
        Some(Msg { class, method, transaction, bytes, attrs, integrity })
    }

    /// The first attribute of type `ty`.
    pub(crate) fn attr(&self, ty: u16) -> Option<&'a [u8]> {
        self.attrs.iter().find(|(t, _)| *t == ty).map(|(_, r)| &self.bytes[r.clone()])
    }

    /// Every attribute of type `ty`, in order.
    pub(crate) fn attrs(&self, ty: u16) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.attrs.iter().filter(move |(t, _)| *t == ty).map(|(_, r)| &self.bytes[r.clone()])
    }

    /// The attribute types present, in order.
    pub(crate) fn types(&self) -> impl Iterator<Item = u16> + '_ {
        self.attrs.iter().map(|(t, _)| *t)
    }

    /// The address in an XOR-…-ADDRESS attribute of type `ty`.
    pub(crate) fn xor_addr(&self, ty: u16) -> Option<SocketAddr> {
        decode_xor(self.attr(ty)?, &self.transaction)
    }

    /// Whether MESSAGE-INTEGRITY is present and is the HMAC-SHA1 under
    /// `key` (RFC 8489 §14.5).
    pub(crate) fn has_integrity(&self) -> bool {
        self.integrity.is_some()
    }

    pub(crate) fn integrity_ok(&self, key: &[u8]) -> bool {
        let (Some(at), Some(tag)) = (self.integrity, self.attr(MESSAGE_INTEGRITY)) else { return false };
        if tag.len() != 20 {
            return false;
        }
        let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(key) else { return false };
        // The length as if the message ended with MESSAGE-INTEGRITY.
        let len = (at + 24 - HEADER) as u16;
        mac.update(&self.bytes[..2]);
        mac.update(&len.to_be_bytes());
        mac.update(&self.bytes[4..at]);
        mac.verify_slice(tag).is_ok()
    }
}

/// An XOR-…-ADDRESS value (RFC 8489 §14.2).
pub(crate) fn decode_xor(v: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    if v.len() < 4 || v[0] != 0 {
        return None;
    }
    let port = u16::from_be_bytes([v[2], v[3]]) ^ 0x2112;
    let ip = match (v[1], v.len()) {
        (0x01, 8) => {
            let mut o = [0u8; 4];
            for (i, b) in o.iter_mut().enumerate() {
                *b = v[4 + i] ^ COOKIE[i];
            }
            IpAddr::V4(Ipv4Addr::from(o))
        }
        (0x02, 20) => {
            let mut o = [0u8; 16];
            for (i, b) in o.iter_mut().enumerate() {
                *b = v[4 + i] ^ if i < 4 { COOKIE[i] } else { transaction[i - 4] };
            }
            IpAddr::V6(Ipv6Addr::from(o))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// An XOR-…-ADDRESS value for `addr`.
fn encode_xor(addr: SocketAddr, transaction: &[u8; 12]) -> Vec<u8> {
    let mut v = vec![0u8];
    let ip: Vec<u8> = match addr.ip() {
        IpAddr::V4(a) => {
            v.push(0x01);
            a.octets().iter().enumerate().map(|(i, b)| b ^ COOKIE[i]).collect()
        }
        IpAddr::V6(a) => {
            v.push(0x02);
            a.octets()
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ if i < 4 { COOKIE[i] } else { transaction[i - 4] })
                .collect()
        }
    };
    v.extend_from_slice(&(addr.port() ^ 0x2112).to_be_bytes());
    v.extend_from_slice(&ip);
    v
}

/// Builds one message.
pub(crate) struct Builder {
    buf: Vec<u8>,
    transaction: [u8; 12],
}

impl Builder {
    pub(crate) fn new(class: Class, method: u16, transaction: [u8; 12]) -> Builder {
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(&message_type(class, method).to_be_bytes());
        buf.extend_from_slice(&[0, 0]);
        buf.extend_from_slice(&COOKIE);
        buf.extend_from_slice(&transaction);
        Builder { buf, transaction }
    }

    fn set_len(&mut self, len: usize) {
        let len = (len - HEADER) as u16;
        self.buf[2..4].copy_from_slice(&len.to_be_bytes());
    }

    pub(crate) fn attr(mut self, ty: u16, value: &[u8]) -> Builder {
        self.buf.extend_from_slice(&ty.to_be_bytes());
        self.buf.extend_from_slice(&(value.len() as u16).to_be_bytes());
        self.buf.extend_from_slice(value);
        self.buf.resize(self.buf.len().div_ceil(4) * 4, 0);
        let len = self.buf.len();
        self.set_len(len);
        self
    }

    pub(crate) fn xor_addr(self, ty: u16, addr: SocketAddr) -> Builder {
        let v = encode_xor(addr, &self.transaction);
        self.attr(ty, &v)
    }

    pub(crate) fn u32(self, ty: u16, value: u32) -> Builder {
        self.attr(ty, &value.to_be_bytes())
    }

    /// ERROR-CODE (RFC 8489 §14.8).
    pub(crate) fn error(self, code: u16, reason: &str) -> Builder {
        let mut v = vec![0, 0, (code / 100) as u8, (code % 100) as u8];
        v.extend_from_slice(reason.as_bytes());
        self.attr(ERROR_CODE, &v)
    }

    /// MESSAGE-INTEGRITY under `key` (RFC 8489 §14.5).
    pub(crate) fn integrity(mut self, key: &[u8]) -> Builder {
        let len = self.buf.len() + 24;
        self.set_len(len);
        let tag: [u8; 20] = match Hmac::<Sha1>::new_from_slice(key) {
            Ok(mut mac) => {
                mac.update(&self.buf);
                mac.finalize().into_bytes().into()
            }
            Err(_) => [0; 20],
        };
        self.attr(MESSAGE_INTEGRITY, &tag)
    }

    /// FINGERPRINT (RFC 8489 §14.7), last.
    pub(crate) fn fingerprint(mut self) -> Builder {
        let len = self.buf.len() + 8;
        self.set_len(len);
        let crc = crc32fast::hash(&self.buf) ^ FINGERPRINT_XOR;
        self.attr(FINGERPRINT, &crc.to_be_bytes())
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// A ChannelData message (RFC 8656 §12.4): the channel number and the
/// payload's length, the payload; over UDP without padding.
pub(crate) fn channel_data(channel: u16, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(4 + payload.len());
    v.extend_from_slice(&channel.to_be_bytes());
    v.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

/// A ChannelData message's channel and payload (`None` unless the first
/// two bits are 01 and the length fits the datagram; trailing padding is
/// allowed).
pub(crate) fn parse_channel_data(bytes: &[u8]) -> Option<(u16, &[u8])> {
    if bytes.len() < 4 || bytes[0] & 0xc0 != 0x40 {
        return None;
    }
    let channel = u16::from_be_bytes([bytes[0], bytes[1]]);
    let len = usize::from(u16::from_be_bytes([bytes[2], bytes[3]]));
    bytes.get(4..4 + len).map(|p| (channel, p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::Config;

    #[test]
    fn a_built_message_parses_back_with_integrity_and_fingerprint() {
        let t = [7u8; 12];
        let peer: SocketAddr = "203.0.113.9:4000".parse().unwrap();
        let bytes = Builder::new(Class::Request, CREATE_PERMISSION, t)
            .xor_addr(XOR_PEER_ADDRESS, peer)
            .attr(USERNAME, b"user")
            .integrity(b"key")
            .fingerprint()
            .finish();
        let m = Msg::parse(&bytes).unwrap();
        assert_eq!((m.class, m.method, m.transaction), (Class::Request, CREATE_PERMISSION, t));
        assert_eq!(m.xor_addr(XOR_PEER_ADDRESS), Some(peer));
        assert_eq!(m.attr(USERNAME), Some(&b"user"[..]));
        assert!(m.integrity_ok(b"key"));
        assert!(!m.integrity_ok(b"other"));
        let mut bad = bytes.clone();
        bad[30] ^= 1;
        assert!(Msg::parse(&bad).is_none(), "the fingerprint catches damage");
        let v6: SocketAddr = "[2001:db8::1]:9".parse().unwrap();
        let b = Builder::new(Class::Success, ALLOCATE, t).xor_addr(XOR_RELAYED_ADDRESS, v6).finish();
        assert_eq!(Msg::parse(&b).unwrap().xor_addr(XOR_RELAYED_ADDRESS), Some(v6));
    }

    #[test]
    fn types_split_and_join() {
        for (class, method) in
            [(Class::Request, ALLOCATE), (Class::Error, CHANNEL_BIND), (Class::Indication, DATA)]
        {
            assert_eq!(split_type(message_type(class, method)), (class, method));
        }
        assert_eq!(message_type(Class::Success, BINDING), 0x0101);
        assert_eq!(message_type(Class::Indication, SEND), 0x0016);
        assert_eq!(message_type(Class::Error, ALLOCATE), 0x0113);
    }

    #[test]
    fn channel_data_round_trips() {
        let v = channel_data(0x4001, b"abc");
        assert_eq!(parse_channel_data(&v), Some((0x4001, &b"abc"[..])));
        assert_eq!(parse_channel_data(&v[..6]), None);
        assert_eq!(parse_channel_data(&[0x80, 0, 0, 0]), None);
    }

    proptest! {
        #![proptest_config(Config { cases: 256, failure_persistence: None, ..Config::default() })]

        /// Any bytes, and a real message damaged, parse or not without a panic.
        #[test]
        fn any_bytes_parse_without_panic(junk in proptest::collection::vec(any::<u8>(), 0..600), at in any::<prop::sample::Index>(), byte in any::<u8>()) {
            if let Some(m) = Msg::parse(&junk) {
                let _ = (m.xor_addr(XOR_PEER_ADDRESS), m.integrity_ok(b"k"), m.types().count());
            }
            let _ = parse_channel_data(&junk);
            let mut real = Builder::new(Class::Request, CHANNEL_BIND, [1; 12])
                .u32(CHANNEL_NUMBER, 0x4000_0000)
                .xor_addr(XOR_PEER_ADDRESS, "198.51.100.1:5".parse().unwrap())
                .integrity(b"k")
                .finish();
            let i = at.index(real.len());
            real[i] = byte;
            if let Some(m) = Msg::parse(&real) {
                let _ = (m.xor_addr(XOR_PEER_ADDRESS), m.integrity_ok(b"k"), m.attr(CHANNEL_NUMBER));
            }
            let mut cut = junk.clone();
            cut.truncate(at.index(junk.len() + 1));
            let _ = Msg::parse(&cut);
        }
    }
}
