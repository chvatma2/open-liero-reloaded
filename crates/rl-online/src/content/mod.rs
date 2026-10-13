//! Content downloads (plan 11, N32): a peer serves pack archives to the
//! peers that lack them (`Server`) and fetches what a content list lacks
//! from its source (`Fetcher`), in `CONTENT_CHUNK` chunks on the reliable
//! channel; one background thread (`Worker`) archives, hashes and installs.
//! `Server` and `Fetcher` are sans-IO: messages in and out, time passed in
//! as `now_us`.

pub mod fetch;
pub mod serve;
pub mod worker;

pub use fetch::{Download, DownloadState, Fetcher};
pub use serve::Server;
pub use worker::{Done, Job, PackArchive, Worker};

use rl_content::MAX_SKIN_ARCHIVE;
use rl_data::manifest::PackKind;
use rl_proto::ItemKind;

/// Missing bytes above which the player is asked before downloading.
pub const PROMPT_BYTES: u64 = 50 * 1024 * 1024;
/// Bytes per second sent to one receiver.
pub const PEER_RATE: u64 = 2 << 20;
/// Bytes per second a host sends in all (the default; configurable).
pub const HOST_RATE: u64 = 4 << 20;
/// Bytes per second a host sends in all while a match starts or runs.
pub const MATCH_RATE: u64 = 256 << 10;
/// Bytes per second a client sends serving its own skin.
pub const SKIN_RATE: u64 = 128 << 10;
/// Bytes a peer's outbox may hold before it gets more chunks.
pub const QUEUED: usize = 256 << 10;
/// A source that sends no chunk for this long is asked again once, then
/// the download fails.
pub const SILENCE_US: u64 = 10_000_000;

/// The pack kind a content item must be.
pub fn pack_kind(kind: ItemKind) -> PackKind {
    match kind {
        ItemKind::Mod => PackKind::Mod,
        ItemKind::Level => PackKind::Level,
        ItemKind::Theme => PackKind::Theme,
        ItemKind::Skin => PackKind::Skin,
    }
}

/// The largest archive of a content item: `MAX_SKIN_ARCHIVE` for a skin,
/// `rl_pack::archive::MAX_ARCHIVE` otherwise.
pub fn archive_limit(kind: ItemKind) -> u64 {
    match kind {
        ItemKind::Skin => MAX_SKIN_ARCHIVE,
        _ => rl_pack::archive::MAX_ARCHIVE,
    }
}
