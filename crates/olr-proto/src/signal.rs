//! Signalling: the WebSocket messages that set up a WebRTC connection
//! (networking spec §5.1). One postcard message per binary frame.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::{ProtoError, decode_exact};

/// Largest accepted signalling message.
pub const MAX_SIGNAL: usize = 16 * 1024;
/// Largest accepted SDP, in bytes.
pub const MAX_SDP: usize = 8 * 1024;

/// Why a host refuses a joiner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    Version,
    Full,
    Banned,
    Busy,
    Malformed,
    /// The lobby's password proof was missing or wrong.
    Password,
    /// The host has no connection ids left (it never reuses one); it must
    /// restart.
    Exhausted,
    /// Too many wrong passwords from this address: try again in a minute.
    Throttled,
    /// A standby was dialled before its peer took over hosting.
    NotHost,
    /// A migrated lobby takes back its own players only (N72).
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Signal {
    /// Client to host.
    Offer { protocol: u16, fingerprint: u64, sdp: String },
    /// Host to client.
    Answer { sdp: String },
    /// Host to client, then the host closes.
    Refused { reason: Refusal },
}

pub fn encode_signal(s: &Signal) -> Vec<u8> {
    postcard::to_allocvec(s).expect("serialising into a Vec cannot fail")
}

pub fn decode_signal(b: &[u8]) -> Result<Signal, ProtoError> {
    if b.len() > MAX_SIGNAL {
        return Err(ProtoError::TooLong);
    }
    let s: Signal = decode_exact(b)?;
    let ok = match &s {
        Signal::Offer { sdp, .. } | Signal::Answer { sdp } => sdp.len() <= MAX_SDP,
        Signal::Refused { .. } => true,
    };
    if ok { Ok(s) } else { Err(ProtoError::Invalid) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn round_trips() {
        for s in [
            Signal::Offer { protocol: 1, fingerprint: u64::MAX, sdp: "v=0\r\n".to_string() },
            Signal::Answer { sdp: "v=0".to_string() },
            Signal::Refused { reason: Refusal::Banned },
            Signal::Refused { reason: Refusal::Password },
            Signal::Refused { reason: Refusal::Exhausted },
            Signal::Refused { reason: Refusal::Throttled },
        ] {
            assert_eq!(decode_signal(&encode_signal(&s)), Ok(s));
        }
    }

    #[test]
    fn sdp_is_bounded() {
        let ok = Signal::Answer { sdp: "a".repeat(MAX_SDP) };
        assert_eq!(decode_signal(&encode_signal(&ok)), Ok(ok));
        let big = Signal::Answer { sdp: "a".repeat(MAX_SDP + 1) };
        assert_eq!(decode_signal(&encode_signal(&big)), Err(ProtoError::Invalid));
    }

    #[test]
    fn oversized_and_trailing_are_refused() {
        assert_eq!(decode_signal(&alloc::vec![0u8; MAX_SIGNAL + 1]), Err(ProtoError::TooLong));
        let mut b = encode_signal(&Signal::Refused { reason: Refusal::Full });
        b.push(0);
        assert_eq!(decode_signal(&b), Err(ProtoError::Decode));
    }

    #[test]
    fn invalid_utf8_is_refused() {
        // Answer (variant 1), length 1, byte 0xff.
        assert_eq!(decode_signal(&[1, 1, 0xff]), Err(ProtoError::Decode));
    }
}
