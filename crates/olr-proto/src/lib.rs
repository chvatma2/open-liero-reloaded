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

pub mod addr;
mod content;
mod frames;
mod lan;
mod lobby;
mod signal;
pub mod stun;
pub use addr::is_global;
pub use content::*;
pub use frames::{decode_frames, encode_frames};
pub use lan::*;
pub use lobby::*;
pub use signal::*;

#[cfg(test)]
#[path = "fingerprint_impl.rs"]
mod fingerprint_impl;

/// Wire protocol version, checked in the handshake. 6: the keys' top five
/// bits are Weapon 1-5 (issue #48), and the state transfer's latches
/// gained theirs. 7: the state transfer's live copies (`olr_sim::LiveWire`,
/// issue #86); later changes to them bump it again.
pub const PROTOCOL_VERSION: u16 = 7;
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
/// Most ticks of final inputs one `Control::Report` or `Control::Resume` carries (30 s).
pub const MAX_REPORT_FRAMES: usize = 3_000;
/// Most provisional inputs of its own slot a report carries.
pub const MAX_REPORT_OWN: usize = 256;
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
    /// Plan 12c (ruling RS-1): a new host's resume, the unreliable twin of
    /// `Control::Resume`.
    Resume(HostResume),
}

/// `keys[k]` is the sender's own slot's input for tick `first + k` (the
/// simulation's key bits, `olr_sim::Keys`); `ack` is the first tick whose
/// frame the sender lacks; `tick` its next tick to simulate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInputs {
    pub stamp: Stamp,
    pub tick: u32,
    pub ack: u32,
    pub first: u32,
    pub keys: Vec<u16>,
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
    /// Withdrawals (decision N83) at ticks within `frames`, as `(slot,
    /// tick)`: at most `MAX_SLOTS`, each slot once.
    pub gone: Vec<(u8, u32)>,
}

/// A new host's resume to a rejoined player, sent every tick until that
/// player's inputs arrive: `Control::Resume`'s fields (`tick`, `at_ms`, the
/// final inputs from `first`), with at most `MAX_FRAMES` frames, so that a
/// reliable message lost once (a ping later) does not make a player late.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostResume {
    pub tick: u32,
    pub at_ms: u32,
    pub first: u32,
    pub frames: Vec<Frame>,
    /// Withdrawals (decision N83) at ticks within `frames`, as `(slot,
    /// tick)`: at most `MAX_SLOTS`, each slot once.
    pub gone: Vec<(u8, u32)>,
}

/// Slots with a final input (`known`), those differing from the previous
/// tick (`changed`, a subset), and one key value per `changed` bit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub known: u32,
    pub changed: u32,
    pub keys: Vec<u16>,
}

/// What a peer knows, for the new host after a migration (spec §3.7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputsReport {
    /// The reporter's frontier: the new host's frames to it start there.
    pub ack: u32,
    /// Where the reporter's log begins: 0, or a spectator's first tick
    /// after the state it joined from (it knows nothing before it, so the
    /// new host's frames starting there carry no base, as the first host's
    /// did; ruling SK-9). At most `ack`.
    pub origin: u32,
    /// `frames[k]` holds tick `first + k`'s final inputs as the reporter
    /// knows them (the frame delta rule; `base` is the new host's value at
    /// `first - 1`).
    pub first: u32,
    pub frames: Vec<Frame>,
    /// Withdrawals (decision N83) at ticks within `frames`, as `(slot,
    /// tick)`: at most `MAX_SLOTS`, each slot once.
    pub gone: Vec<(u8, u32)>,
    /// The reporter's own provisional inputs for its slot, ticks
    /// `own_first..` (empty for a spectator).
    pub own_first: u32,
    pub own: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    Checksum {
        tick: u32,
        hash: u64,
    },
    /// The host filled the receiver's inputs it got too late, within ticks
    /// `from..to` (one per datagram; `from < to`). The span covers every
    /// filled tick the datagram hit, so it can include ticks between two
    /// fill episodes whose inputs were on time.
    Late {
        from: u32,
        to: u32,
    },
    Desync {
        tick: u32,
        host_hash: u64,
    },
    StateBegin {
        tick: u32,
        len: u32,
        chunks: u16,
    },
    StateChunk {
        index: u16,
        bytes: Vec<u8>,
    },
    /// The sender leaves the match.
    Bye,
    /// A client or spectator loaded the host's state after `tick`.
    StateLoaded {
        tick: u32,
    },
    /// A client or spectator refused the host's state after `tick` (its
    /// transfer, or the bytes).
    StateRefused {
        tick: u32,
    },
    /// New host to a rejoined peer: per slot, the first tick it lacks
    /// (`known.len()` = the match's slots, at most `MAX_SLOTS`).
    Collect {
        known: Vec<u32>,
    },
    /// A rejoined peer to its new host: the answer to `Collect`.
    Report(InputsReport),
    /// New host to a rejoined peer: the match resumes at `tick`. `at_ms`
    /// is the moment, on the receiver's own clock (its stamps' `sent_ms`),
    /// from which an input it sends reaches the new host as it runs `tick`
    /// (`0`: unknown, the receipt). `frames` are the final inputs from
    /// `first` the peer lacks (the frame delta rule, `base` the peer's
    /// values at `first - 1`), so it never waits for them after the resume.
    /// `gone`: the withdrawals (decision N83) at ticks within `frames`, as
    /// `(slot, tick)`: at most `MAX_SLOTS`, each slot once.
    Resume {
        tick: u32,
        at_ms: u32,
        first: u32,
        frames: Vec<Frame>,
        gone: Vec<(u8, u32)>,
    },
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

/// Whether `marks` fit a message whose frames are ticks `first..first +
/// len`: at most `MAX_SLOTS` entries, each slot below `slots` and listed
/// once, each tick within the frames (decision N83, ruling PF12d-4).
pub fn valid_marks(first: u32, len: usize, marks: &[(u8, u32)], slots: usize) -> bool {
    let end = u64::from(first) + len as u64;
    let mut seen = 0u32;
    marks.len() <= MAX_SLOTS
        && marks.iter().all(|&(s, t)| {
            let ok = (s as usize) < slots.min(MAX_SLOTS)
                && seen & (1 << s) == 0
                && t >= first
                && u64::from(t) < end;
            if ok {
                seen |= 1 << s;
            }
            ok
        })
}

fn validate_datagram(d: &Datagram, slots: usize) -> Result<(), ProtoError> {
    let ok = match d {
        Datagram::Client(c) => c.keys.len() <= MAX_KEYS,
        Datagram::Host(h) => {
            h.frames.len() <= MAX_FRAMES
                && h.frames.iter().all(|f| validate_frame(f, slots))
                && valid_marks(h.first, h.frames.len(), &h.gone, slots)
        }
        Datagram::Resume(r) => {
            r.frames.len() <= MAX_FRAMES
                && r.frames.iter().all(|f| validate_frame(f, slots))
                && valid_marks(r.first, r.frames.len(), &r.gone, slots)
        }
    };
    if ok { Ok(()) } else { Err(ProtoError::Invalid) }
}

pub fn encode_control(c: &Control) -> Vec<u8> {
    postcard::to_allocvec(c).expect("serialising into a Vec cannot fail")
}

/// A migration message's frames (`Report`, `Resume`): at most
/// `MAX_REPORT_FRAMES`, no `u32` overflow of `first + len`, each frame
/// `changed ⊆ known` with a key per changed bit (the slots' count is the
/// receiver's to check).
fn valid_report_frames(first: u32, frames: &[Frame]) -> bool {
    frames.len() <= MAX_REPORT_FRAMES
        && first.checked_add(frames.len() as u32).is_some()
        && frames.iter().all(|f| f.changed & !f.known == 0 && f.keys.len() == f.changed.count_ones() as usize)
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
        Control::Late { from, to } => from < to,
        Control::Collect { known } => (1..=MAX_SLOTS).contains(&known.len()),
        // The match's slots are the receiver's to check (a mark for a slot
        // beyond them is a `Gap` in its log).
        Control::Report(r) => {
            valid_report_frames(r.first, &r.frames)
                && valid_marks(r.first, r.frames.len(), &r.gone, MAX_SLOTS)
                && r.origin <= r.ack
                && r.own.len() <= MAX_REPORT_OWN
                && r.own_first.checked_add(r.own.len() as u32).is_some()
        }
        Control::Resume { first, frames, gone, .. } => {
            valid_report_frames(*first, frames) && valid_marks(*first, frames.len(), gone, MAX_SLOTS)
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
