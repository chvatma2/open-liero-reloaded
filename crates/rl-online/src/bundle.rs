//! A real game's desync bundle (networking spec §3.5, §9.10: every desync
//! in a real game saves one): what this peer knew when its simulation and
//! the host's differed, so the developer can make a test of it.
//!
//! The file is the magic [`GAME_BUNDLE_MAGIC`] (`RLDG`), the version (u16
//! little-endian) and a postcard body ([`GameBundle`]). It is not
//! `rl-cli`'s `.rlds` (a `net-sim` run's, which rebuilds its world from a
//! `NetSpec`): this one carries the lobby's match start instead. A
//! `net-repro` for game bundles comes later; until then the bundle holds
//! everything one needs (the start, the final inputs from tick 0, both
//! checksums, the build and the platform).

use anyhow::{Context as _, ensure};
use rl_proto::MatchStartWire;
use serde::{Deserialize, Serialize};

use crate::replay::MatchRecord;

/// The first bytes of a game desync bundle.
pub const GAME_BUNDLE_MAGIC: [u8; 4] = *b"RLDG";
/// The format's version.
pub const GAME_BUNDLE_VERSION: u16 = 1;
/// The extension of a game desync bundle's file.
pub const GAME_BUNDLE_EXTENSION: &str = "rldg";
/// At most this many bundles a match, per peer (a desync the host repairs
/// may come back; a lobby of many peers could make one per peer).
pub const MAX_BUNDLES_PER_MATCH: u32 = 3;

/// One desync, as one peer saw it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameBundle {
    /// What happened, in words: the desync reported, or the replay's
    /// checksum warning.
    pub what: String,
    /// The host recorded it.
    pub host: bool,
    /// The slot this peer played (`None`: watching).
    pub my_slot: Option<u8>,
    /// `rl_proto::FINGERPRINT` of the build that recorded it.
    pub fingerprint: u64,
    /// The recording machine, such as `x86_64-linux`.
    pub platform: String,
    /// The match's start: every peer built its world from it.
    pub start: MatchStartWire,
    /// The final input rows from tick 0 that this peer confirmed (empty
    /// for a spectator that joined mid-match).
    pub rows: Vec<[u8; 32]>,
    /// This peer's confirmed sync checksums (`(tick, checksum)`).
    pub checksums: Vec<(u32, u64)>,
    /// The other side's stream as this peer knows it: on the host, the
    /// peer's reports it compared (`Session::peer_checksums`, the last
    /// `PEER_STREAM_KEEP`); on a client, the host's checksum of the
    /// mismatch (the one the host sends).
    pub other_checksums: Vec<(u32, u64)>,
    /// The mismatch: the tick, this peer's checksum and the other's (the
    /// host's on a client; the peer's on the host). `None` for a replay's
    /// checksum warning (in `what`).
    pub mismatch: Option<(u32, u64, u64)>,
}

impl GameBundle {
    /// The bundle of `record` (its start, rows and checksums).
    pub fn of(record: MatchRecord, what: String, host: bool, my_slot: Option<u8>) -> GameBundle {
        GameBundle {
            what,
            host,
            my_slot,
            fingerprint: rl_proto::FINGERPRINT,
            platform: platform(),
            start: record.start,
            rows: record.rows,
            checksums: record.checksums,
            other_checksums: Vec::new(),
            mismatch: None,
        }
    }

    /// The magic, the version and the postcard body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = GAME_BUNDLE_MAGIC.to_vec();
        out.extend_from_slice(&GAME_BUNDLE_VERSION.to_le_bytes());
        out.extend(postcard::to_allocvec(self).expect("a bundle serializes"));
        out
    }

    pub fn decode(b: &[u8]) -> anyhow::Result<GameBundle> {
        ensure!(b.len() >= 6 && b[..4] == GAME_BUNDLE_MAGIC, "not a game desync bundle (bad magic)");
        let version = u16::from_le_bytes([b[4], b[5]]);
        ensure!(
            version == GAME_BUNDLE_VERSION,
            "bundle version {version}, this build reads {GAME_BUNDLE_VERSION}"
        );
        postcard::from_bytes(&b[6..]).context("decoding the bundle's body")
    }
}

/// This machine, as `<arch>-<os>`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_round_trips_and_refuses_other_files() {
        let start = MatchStartWire {
            seed: 3,
            setup: rl_proto::MatchSetupWire {
                mod_id: "m".into(),
                level_id: "l".into(),
                packs: vec![],
                rules: crate::matches::rules_wire(&rl_modes::DeathmatchSettings::default()),
                bonuses: false,
                loading_time: 100,
                restrictions: String::new(),
                bots: vec![],
            },
            slots: vec![],
            input_delay: vec![],
            start_checksum: 9,
        };
        let record = MatchRecord { start, rows: vec![[1; 32], [2; 32]], checksums: vec![(9, 77)] };
        let mut b = GameBundle::of(record, "desync".into(), false, Some(1));
        b.mismatch = Some((9, 77, 78));
        let bytes = b.encode();
        assert_eq!(&bytes[..4], b"RLDG");
        assert_eq!(GameBundle::decode(&bytes).unwrap(), b);
        assert!(GameBundle::decode(b"RLDS\x01\x00").is_err(), "rl-cli's bundles are another format");
        assert!(GameBundle::decode(&bytes[..5]).is_err());
        let mut newer = bytes.clone();
        newer[4] = 2;
        assert!(GameBundle::decode(&newer).is_err());
    }
}
