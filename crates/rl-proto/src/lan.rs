//! LAN announcements: one UDP datagram per host every 2 s on `LAN_PORT`.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::lobby::{MAX_NAME, valid_name, valid_pack_id};
use crate::{ProtoError, decode_exact};

/// UDP port hosts announce on.
pub const LAN_PORT: u16 = 23401;
/// Largest accepted announcement.
pub const MAX_LAN: usize = 1024;

/// One host's announcement, postcard-encoded: its fields are positional,
/// so new ones are appended (`bots`, protocol 2) and an older build's
/// datagram does not decode (its fingerprint differs anyway).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanAnnounce {
    pub fingerprint: u64,
    pub name: String,
    pub port: u16,
    pub players: u8,
    pub max_players: u8,
    pub spectators: u8,
    pub mod_id: String,
    pub level_id: String,
    pub password: bool,
    pub playing: bool,
    /// The bots, apart from `players` (the seated members): bots take no
    /// seat (the user's report of 2026-10-15). At most `MAX_PLAYERS`.
    pub bots: u8,
}

pub fn encode_lan(a: &LanAnnounce) -> Vec<u8> {
    postcard::to_allocvec(a).expect("serialising into a Vec cannot fail")
}

pub fn decode_lan(b: &[u8]) -> Result<LanAnnounce, ProtoError> {
    if b.len() > MAX_LAN {
        return Err(ProtoError::TooLong);
    }
    let a: LanAnnounce = decode_exact(b)?;
    let ok = valid_name(&a.name, MAX_NAME)
        && valid_pack_id(&a.mod_id)
        && valid_pack_id(&a.level_id)
        && a.players <= a.max_players
        && a.max_players as usize <= crate::lobby::MAX_PLAYERS
        && a.bots as usize <= crate::lobby::MAX_PLAYERS
        && a.spectators as usize <= crate::lobby::MAX_SPECTATORS;
    if ok { Ok(a) } else { Err(ProtoError::Invalid) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn sample() -> LanAnnounce {
        LanAnnounce {
            fingerprint: 0xdead_beef,
            name: "Office".to_string(),
            port: 23400,
            players: 3,
            max_players: 8,
            spectators: 1,
            mod_id: "olx-classic".to_string(),
            level_id: "olx-level-dirt-level".to_string(),
            password: true,
            playing: false,
            bots: 3,
        }
    }

    #[test]
    fn round_trips() {
        let a = sample();
        assert_eq!(decode_lan(&encode_lan(&a)), Ok(a));
    }

    #[test]
    fn name_is_bounded_and_clean() {
        let mut a = sample();
        a.name = "n".repeat(MAX_NAME + 1);
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
        a.name = "a\nb".to_string();
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
    }

    #[test]
    fn ids_are_pack_ids() {
        let mut a = sample();
        a.mod_id = "../x".to_string();
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
        let mut a = sample();
        a.level_id = "a/b".to_string();
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
    }

    #[test]
    fn counts_are_bounded() {
        let mut a = sample();
        a.players = 9;
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
        let mut a = sample();
        a.max_players = 33;
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
        // Bots take no seat (the user's report of 2026-10-15): more bots
        // than seats is a game's right, more than the worms a match holds
        // is not.
        let mut a = sample();
        a.bots = 31;
        assert_eq!(decode_lan(&encode_lan(&a)), Ok(a.clone()));
        a.bots = 33;
        assert_eq!(decode_lan(&encode_lan(&a)), Err(ProtoError::Invalid));
    }

    #[test]
    fn oversized_and_trailing_are_refused() {
        assert_eq!(decode_lan(&alloc::vec![0u8; MAX_LAN + 1]), Err(ProtoError::TooLong));
        let mut b = encode_lan(&sample());
        b.push(0);
        assert_eq!(decode_lan(&b), Err(ProtoError::Decode));
    }
}
