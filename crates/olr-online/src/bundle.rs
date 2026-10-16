//! A real game's desync bundle (networking spec §3.5, §9.10: every desync
//! in a real game saves one): what this peer knew when its simulation and
//! the host's differed, so the developer can make a test of it.
//!
//! The file is the magic [`GAME_BUNDLE_MAGIC`] (`OLDG`), the version (u16
//! little-endian) and a postcard body ([`GameBundle`]). It is not
//! `olr-cli`'s `.olds` (a `net-sim` run's, which rebuilds its world from a
//! `NetSpec`): this one carries the lobby's match start instead. A
//! `net-repro` for game bundles comes later; until then the bundle holds
//! everything one needs (the start, the final inputs from tick 0, both
//! checksums, the build and the platform).

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, ensure};
use olr_proto::MatchStartWire;
use serde::{Deserialize, Serialize};

use crate::replay::MatchRecord;

/// The first bytes of a game desync bundle.
pub const GAME_BUNDLE_MAGIC: [u8; 4] = *b"OLDG";
/// The format's version.
pub const GAME_BUNDLE_VERSION: u16 = 2;
/// The extension of a game desync bundle's file.
pub const GAME_BUNDLE_EXTENSION: &str = "oldg";
/// At most this many bundles a match, per peer (a desync the host repairs
/// may come back; a lobby of many peers could make one per peer).
pub const MAX_BUNDLES_PER_MATCH: u32 = 3;

/// A bundle larger than this is not saved (a 20-minute match's inputs fit).
pub const MAX_BUNDLE_BYTES: usize = 4 * 1024 * 1024;
/// The bundles in one directory keep below this many bytes: the oldest go.
pub const MAX_BUNDLES_DIR_BYTES: u64 = 64 * 1024 * 1024;

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
    /// `olr_proto::FINGERPRINT` of the build that recorded it.
    pub fingerprint: u64,
    /// The recording machine, such as `x86_64-linux`.
    pub platform: String,
    /// The match's start: every peer built its world from it.
    pub start: MatchStartWire,
    /// The final input rows from tick 0 that this peer confirmed (empty
    /// for a spectator that joined mid-match).
    pub rows: Vec<[u8; 32]>,
    /// The withdrawals at ticks below `rows.len()`, `(tick, slot)`
    /// (decision N83; version 2).
    pub gone: Vec<(u32, u8)>,
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
            fingerprint: olr_proto::FINGERPRINT,
            platform: platform(),
            start: record.start,
            rows: record.rows,
            gone: record.gone,
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

/// Saves bundle `bytes` in `dir` (made if missing) as
/// `desync-<unix_secs>-<n>.oldg`, the first `n` free, written to a
/// temporary file and renamed; returns the file. Bounded: a bundle above
/// [`MAX_BUNDLE_BYTES`] is refused, and the oldest bundles of this naming
/// (by their seconds, then `n`; other files are never touched) are removed
/// until the directory's bundles stay within [`MAX_BUNDLES_DIR_BYTES`].
/// The time is the caller's (no clock is read here).
pub fn save(dir: &Path, bytes: &[u8], unix_secs: u64) -> io::Result<PathBuf> {
    save_within(dir, bytes, unix_secs, MAX_BUNDLE_BYTES, MAX_BUNDLES_DIR_BYTES)
}

fn save_within(
    dir: &Path,
    bytes: &[u8],
    unix_secs: u64,
    max_one: usize,
    max_all: u64,
) -> io::Result<PathBuf> {
    if bytes.len() > max_one {
        return Err(io::Error::other(format!(
            "the bundle has {} bytes, more than {max_one}: not saved",
            bytes.len()
        )));
    }
    std::fs::create_dir_all(dir)?;
    let path = (1..=1000u32)
        .map(|n| dir.join(format!("desync-{unix_secs}-{n}.{GAME_BUNDLE_EXTENSION}")))
        .find(|p| !p.exists())
        .ok_or_else(|| io::Error::other("a thousand bundles this second"))?;
    // The oldest first, until the new one fits.
    let mut old: Vec<((u64, u32), PathBuf, u64)> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let order = bundle_order(e.file_name().to_str()?)?;
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some((order, e.path(), meta.len()))
        })
        .collect();
    old.sort();
    let mut total: u64 = old.iter().map(|o| o.2).sum::<u64>() + bytes.len() as u64;
    for (_, p, len) in &old {
        if total <= max_all {
            break;
        }
        std::fs::remove_file(p)?;
        total -= len;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

/// `(seconds, n)` of a `desync-<seconds>-<n>.oldg` name.
fn bundle_order(name: &str) -> Option<(u64, u32)> {
    let stem = name.strip_prefix("desync-")?.strip_suffix(&format!(".{GAME_BUNDLE_EXTENSION}"))?;
    let (secs, n) = stem.split_once('-')?;
    Some((secs.parse().ok()?, n.parse().ok()?))
}

/// This machine, as `<arch>-<os>`.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_bundles_are_bounded_and_the_oldest_go() {
        let tmp = std::env::temp_dir().join(format!("olr-online-bundles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let a = save(&tmp, b"one", 1_700_000_000).unwrap();
        let b = save(&tmp, b"two", 1_700_000_000).unwrap();
        assert_eq!(a.file_name().unwrap(), "desync-1700000000-1.oldg");
        assert_eq!(b.file_name().unwrap(), "desync-1700000000-2.oldg");
        assert!(save(&tmp, &vec![0; MAX_BUNDLE_BYTES + 1], 1).is_err(), "too large");
        // Another file stays whatever happens.
        std::fs::write(tmp.join("notes.txt"), b"mine").unwrap();
        // With 100 bytes a bundle and 1000 in all (the same rule as 4 and
        // 64 MiB): 11 more bundles do not fit, the oldest go, in order.
        let big = vec![7; 100];
        let saved: Vec<PathBuf> =
            (0..11).map(|i| save_within(&tmp, &big, 1_800_000_000 + i, 100, 1000).unwrap()).collect();
        assert!(save_within(&tmp, &[0; 101], 1, 100, 1000).is_err());
        assert!(!a.exists() && !b.exists() && !saved[0].exists());
        assert!(saved[1..].iter().all(|p| p.exists()));
        let total: u64 =
            std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum::<u64>() - 4;
        assert_eq!(total, 1000);
        assert!(tmp.join("notes.txt").exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_bundle_round_trips_and_refuses_other_files() {
        let start = MatchStartWire {
            seed: 3,
            setup: olr_proto::MatchSetupWire {
                mod_id: "m".into(),
                level_id: "l".into(),
                packs: vec![],
                rules: crate::matches::rules_wire(&olr_modes::DeathmatchSettings::default()),
                bonuses: false,
                loading_time: 100,
                restrictions: String::new(),
                bots: vec![],
            },
            slots: vec![],
            input_delay: vec![],
            start_checksum: 9,
        };
        let record =
            MatchRecord { start, rows: vec![[1; 32], [2; 32]], checksums: vec![(9, 77)], gone: vec![(1, 3)] };
        let mut b = GameBundle::of(record, "desync".into(), false, Some(1));
        b.mismatch = Some((9, 77, 78));
        let bytes = b.encode();
        assert_eq!(&bytes[..4], b"OLDG");
        assert_eq!(GameBundle::decode(&bytes).unwrap(), b);
        assert!(GameBundle::decode(b"OLDS\x01\x00").is_err(), "olr-cli's bundles are another format");
        assert!(GameBundle::decode(&bytes[..5]).is_err());
        let mut newer = bytes.clone();
        newer[4] = 3;
        assert!(GameBundle::decode(&newer).is_err());
    }
}
