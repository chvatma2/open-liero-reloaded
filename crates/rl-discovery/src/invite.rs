//! Invite codes (N42): `rlx1…`, bech32m with HRP `rlx`.
//!
//! The payload is postcard: `version: u8 = 1`, the host's 32-byte key, the
//! relay hints and an optional IPv4 address (4 bytes and a 2-byte port).
//!
//! Bech32m's 90-character limit is not applied: this crate's `bech32` (0.12)
//! checks the length against the checksum's own `CODE_LENGTH` (1023 for
//! Bech32 and Bech32m), not 90, so `CheckedHrpstring::new::<Bech32m>` takes
//! longer codes, and `Invite::decode` itself refuses text above
//! [`MAX_CODE`] (200) characters. Decoding uses `Bech32m` only (a
//! plain-bech32 checksum is damage), and `CheckedHrpstring` refuses mixed
//! case while accepting all upper case.

use std::net::{Ipv4Addr, SocketAddr};

use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32m, Hrp};
use rl_proto::PublicKey;
use serde::{Deserialize, Serialize};

use crate::defaults::DEFAULT_RELAYS;

/// The longest invite code `decode` takes, in characters.
pub const MAX_CODE: usize = 200;
const VERSION: u8 = 1;
const HRP: &str = "rlx";
/// At most this many `Url` hints.
pub const MAX_URL_HINTS: usize = 1;
/// A `Url` hint is at most this many bytes.
pub const MAX_URL_BYTES: usize = 64;
/// At most this many `Default` hints.
pub const MAX_DEFAULT_HINTS: usize = 3;
// Worst case: version 1 + key 32 + relays (count 1 + 3 x 2 + 1 x (2 + 64))
// + address (1 + 4 + 2) = 113 bytes = 181 base32 characters, plus the
// 4-character prefix and the 6-character checksum: 191 <= MAX_CODE.

/// A game's invite: its key, where to find it on Nostr, and where it listens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub key: PublicKey,
    pub relays: Vec<RelayHint>,
    /// IPv4 only.
    pub addr: Option<SocketAddr>,
}

/// A relay named in an invite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayHint {
    /// An index into [`DEFAULT_RELAYS`] (append-only). `decode` skips an
    /// index beyond this build's list (a newer build's code).
    Default(u8),
    /// `wss://` or `ws://`, at most 100 bytes.
    Url(String),
}

/// Why an invite code was refused (user-facing text).
#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    #[error("this is not an invite code")]
    NotInvite,
    #[error("this invite code is damaged")]
    Damaged,
    #[error("this invite code is from a newer version of the game")]
    Newer,
    #[error("this invite code is invalid")]
    Invalid,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    version: u8,
    key: [u8; 32],
    relays: Vec<RelayHint>,
    addr: Option<([u8; 4], u16)>,
}

fn hint_ok(h: &RelayHint) -> bool {
    match h {
        RelayHint::Default(i) => usize::from(*i) < DEFAULT_RELAYS.len(),
        RelayHint::Url(u) => u.len() <= MAX_URL_BYTES && (u.starts_with("wss://") || u.starts_with("ws://")),
    }
}

impl Invite {
    /// The code, which `decode` always takes. `encode` is infallible, so it
    /// keeps hints within the limits: it drops out-of-range `Default`s and
    /// bad `Url`s (wrong scheme or over [`MAX_URL_BYTES`]), then the hints
    /// beyond [`MAX_DEFAULT_HINTS`] / [`MAX_URL_HINTS`] (the first win). An
    /// IPv6 address is not representable and is left out.
    pub fn encode(&self) -> String {
        let (mut defaults, mut urls) = (0, 0);
        let relays: Vec<RelayHint> = self
            .relays
            .iter()
            .filter(|h| match h {
                RelayHint::Default(_) => {
                    hint_ok(h) && {
                        defaults += 1;
                        defaults <= MAX_DEFAULT_HINTS
                    }
                }
                RelayHint::Url(_) => {
                    hint_ok(h) && {
                        urls += 1;
                        urls <= MAX_URL_HINTS
                    }
                }
            })
            .cloned()
            .collect();
        let addr = match self.addr {
            Some(SocketAddr::V4(a)) => Some((a.ip().octets(), a.port())),
            _ => None,
        };
        let wire = Wire { version: VERSION, key: self.key, relays, addr };
        let bytes = postcard::to_allocvec(&wire).unwrap_or_default();
        match Hrp::parse(HRP) {
            Ok(hrp) => bech32::encode::<Bech32m>(hrp, &bytes).unwrap_or_default(),
            Err(_) => String::new(),
        }
    }

    pub fn decode(text: &str) -> Result<Invite, InviteError> {
        let text = text.trim();
        let prefix = text.get(..HRP.len() + 1).unwrap_or("");
        if !prefix.eq_ignore_ascii_case("rlx1") {
            return Err(InviteError::NotInvite);
        }
        if text.len() > MAX_CODE {
            return Err(InviteError::Invalid);
        }
        let checked = CheckedHrpstring::new::<Bech32m>(text).map_err(|_| InviteError::Damaged)?;
        // `rlx1foo1…` with a valid checksum for HRP `rlx1foo` (bech32's
        // separator is the last `1`) is not ours.
        if !checked.hrp().as_str().eq_ignore_ascii_case(HRP) {
            return Err(InviteError::Damaged);
        }
        let bytes: Vec<u8> = checked.byte_iter().collect();
        match bytes.first() {
            None | Some(0) => return Err(InviteError::Invalid),
            Some(&v) if v > VERSION => return Err(InviteError::Newer),
            Some(_) => {}
        }
        let (wire, rest) = postcard::take_from_bytes::<Wire>(&bytes).map_err(|_| InviteError::Invalid)?;
        if !rest.is_empty() {
            return Err(InviteError::Invalid);
        }
        let defaults = wire.relays.iter().filter(|h| matches!(h, RelayHint::Default(_))).count();
        let urls = wire.relays.len() - defaults;
        let urls_ok = wire.relays.iter().all(|h| matches!(h, RelayHint::Default(_)) || hint_ok(h));
        if !urls_ok || defaults > MAX_DEFAULT_HINTS || urls > MAX_URL_HINTS {
            return Err(InviteError::Invalid);
        }
        // A `Default` index beyond this build's list comes from a newer
        // build's longer one: it is skipped, the other hints still serve.
        let relays = wire.relays.into_iter().filter(hint_ok).collect();
        let addr = wire.addr.map(|(ip, port)| SocketAddr::from((Ipv4Addr::from(ip), port)));
        Ok(Invite { key: wire.key, relays, addr })
    }

    /// The relay URLs the hints name, in order (defaults resolved).
    pub fn relay_urls(&self) -> Vec<String> {
        self.relays
            .iter()
            .filter_map(|h| match h {
                RelayHint::Default(i) => DEFAULT_RELAYS.get(usize::from(*i)).map(|s| (*s).to_string()),
                RelayHint::Url(u) => Some(u.clone()),
            })
            .collect()
    }
}
