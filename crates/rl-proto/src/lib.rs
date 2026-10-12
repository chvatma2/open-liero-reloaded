//! The network wire format (networking spec §2, §3.1, §5.4): datagrams on
//! the unreliable inputs channel, messages on the reliable control channel,
//! the frame delta codec and the simulation fingerprint. Every decoder
//! bounds its input and never panics.
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

mod frames;
mod lan;
mod lobby;
mod signal;
pub use frames::{decode_frames, encode_frames};
pub use lan::*;
pub use lobby::*;
pub use signal::*;

#[cfg(test)]
#[path = "fingerprint_impl.rs"]
mod fingerprint_impl;

/// Wire protocol version, checked in the handshake.
pub const PROTOCOL_VERSION: u16 = 1;
/// At most this many slots in a match.
pub const MAX_SLOTS: usize = 32;
/// Largest accepted inputs datagram.
pub const MAX_DATAGRAM: usize = 1200;
/// Largest accepted control message.
pub const MAX_CONTROL: usize = 40 * 1024;
/// Most frames in one host datagram.
pub const MAX_FRAMES: usize = 64;
/// Most keys in one client datagram.
pub const MAX_KEYS: usize = 64;
/// Bytes of world state per chunk.
pub const STATE_CHUNK: usize = 32 * 1024;
/// Largest accepted world state.
pub const MAX_STATE: usize = 64 * 1024 * 1024;
/// Per-datagram bytes on the wire (DTLS + SCTP + UDP + IPv4), for budgets.
pub const OVERHEAD: usize = 48;
/// Hash of the simulation's source: peers only play with an equal one (spec §2.2).
pub const FINGERPRINT: u64 = include!(concat!(env!("OUT_DIR"), "/fingerprint.rs"));

/// Round-trip measurement: `sent_ms` is the sender's clock, `echo_ms` the
/// last `sent_ms` it received from this peer, `held_ms` how long ago.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub sent_ms: u32,
    pub echo_ms: u32,
    pub held_ms: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Datagram {
    Client(ClientInputs),
    Host(HostInputs),
}

/// `keys[k]` is the sender's own slot's input for tick `first + k`; `ack` is
/// the first tick whose frame the sender lacks; `tick` its next tick to simulate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInputs {
    pub stamp: Stamp,
    pub tick: u32,
    pub ack: u32,
    pub first: u32,
    pub keys: Vec<u8>,
}

/// `frames[k]` describes tick `first + k`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostInputs {
    pub stamp: Stamp,
    pub tick: u32,
    pub your_ack: u32,
    pub margin: i16,
    pub first: u32,
    pub frames: Vec<Frame>,
}

/// Slots with a final input (`known`), those differing from the previous
/// tick (`changed`, a subset), and one key byte per `changed` bit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub known: u32,
    pub changed: u32,
    pub keys: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    Checksum { tick: u32, hash: u64 },
    Late { tick: u32 },
    Desync { tick: u32, host_hash: u64 },
    StateBegin { tick: u32, len: u32, chunks: u16 },
    StateChunk { index: u16, bytes: Vec<u8> },
    Bye,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtoError {
    TooLong,
    Decode,
    Invalid,
}

/// Decodes one value and refuses trailing bytes.
fn decode_exact<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Result<T, ProtoError> {
    let (v, rest) = postcard::take_from_bytes(b).map_err(|_| ProtoError::Decode)?;
    if rest.is_empty() { Ok(v) } else { Err(ProtoError::Decode) }
}

pub fn encode_datagram(d: &Datagram) -> Vec<u8> {
    postcard::to_allocvec(d).expect("serialising into a Vec cannot fail")
}

pub fn decode_datagram(b: &[u8], slots: usize) -> Result<Datagram, ProtoError> {
    if b.len() > MAX_DATAGRAM {
        return Err(ProtoError::TooLong);
    }
    let d: Datagram = decode_exact(b)?;
    validate_datagram(&d, slots)?;
    Ok(d)
}

fn validate_frame(f: &Frame, slots: usize) -> bool {
    f.changed & !f.known == 0
        && (slots >= 32 || f.known >> slots == 0)
        && f.keys.len() == f.changed.count_ones() as usize
}

fn validate_datagram(d: &Datagram, slots: usize) -> Result<(), ProtoError> {
    let ok = match d {
        Datagram::Client(c) => c.keys.len() <= MAX_KEYS,
        Datagram::Host(h) => {
            h.frames.len() <= MAX_FRAMES && h.frames.iter().all(|f| validate_frame(f, slots))
        }
    };
    if ok { Ok(()) } else { Err(ProtoError::Invalid) }
}

pub fn encode_control(c: &Control) -> Vec<u8> {
    postcard::to_allocvec(c).expect("serialising into a Vec cannot fail")
}

pub fn decode_control(b: &[u8]) -> Result<Control, ProtoError> {
    if b.len() > MAX_CONTROL {
        return Err(ProtoError::TooLong);
    }
    let c: Control = decode_exact(b)?;
    let ok = match &c {
        Control::StateChunk { bytes, .. } => bytes.len() <= STATE_CHUNK,
        Control::StateBegin { len, chunks, .. } => {
            *len as usize <= MAX_STATE && *chunks as usize == (*len as usize).div_ceil(STATE_CHUNK)
        }
        _ => true,
    };
    if ok { Ok(c) } else { Err(ProtoError::Invalid) }
}

#[cfg(test)]
mod tests {
    use super::fingerprint_impl::fingerprint;
    use super::*;

    #[test]
    fn fingerprint_depends_on_contents_and_names() {
        let a: &[(&str, &[u8])] = &[("a.rs", b"fn a() {}"), ("b.rs", b"fn b() {}")];
        let changed: &[(&str, &[u8])] = &[("a.rs", b"fn a() {}"), ("b.rs", b"fn c() {}")];
        let swapped: &[(&str, &[u8])] = &[("b.rs", b"fn a() {}"), ("a.rs", b"fn b() {}")];
        assert_eq!(fingerprint(a), fingerprint(a));
        assert_ne!(fingerprint(a), fingerprint(changed));
        assert_ne!(fingerprint(a), fingerprint(swapped));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut d = encode_datagram(&Datagram::Client(ClientInputs {
            stamp: Stamp::default(),
            tick: 0,
            ack: 0,
            first: 0,
            keys: alloc::vec![1],
        }));
        d.push(0);
        assert_eq!(decode_datagram(&d, 8), Err(ProtoError::Decode));
        let mut c = encode_control(&Control::Bye);
        c.push(0);
        assert_eq!(decode_control(&c), Err(ProtoError::Decode));
    }

    #[test]
    fn state_begin_chunks_must_match() {
        let ok = Control::StateBegin { tick: 1, len: STATE_CHUNK as u32 + 1, chunks: 2 };
        assert_eq!(decode_control(&encode_control(&ok)), Ok(ok));
        let bad = Control::StateBegin { tick: 1, len: 10, chunks: 2 };
        assert_eq!(decode_control(&encode_control(&bad)), Err(ProtoError::Invalid));
    }
}
