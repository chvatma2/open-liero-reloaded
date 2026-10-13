# Plan 11: Content Download Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A player who joins an online game without its mod, level, theme or someone's skin downloads them from the host in the lobby: verified, cached and loaded like any other pack, then plays.

**Architecture:**
- **Archives:** a pack travels as a `.rlpk` archive, an uncompressed, sorted, size-checked listing of its files (`rl-pack`).
- **Protocol:** a new reliable-channel message family (`rl-proto`, tag 3) lists the match's content by hash and moves archives in 32 KiB chunks, resumably.
- **Cache:** a content-addressed download cache with an LRU index (`rl-content`) installs only archives whose files hash to the requested content hash and that pass the pack validator. Downloaded packs join a match's content as an overlay by id, and the player's own packs stay untouched.
- **Transfers:** sans-IO state machines in `rl-online` serve and fetch archives, paced under the receivers' rate limits. Hashing, archiving and verifying run on a worker thread.
- **Game:** the game shows the list, the progress and a prompt above 50 MB.

**Tech Stack:** Rust 1.99 (edition 2024); existing workspace crates and dependencies only (`sha2`, `hex`, `serde`, `postcard`, `ron`, `directories`, `tempfile` for tests). No new dependency.

**Spec:** `docs/superpowers/specs/2026-10-12-networking-design.md` (approved; plans 9 and 10 implemented). This plan covers spec §11's plan 11 row: §8 (content download) and the content-download part of §9.9 (the lobby's download progress). Not here: the browser's content fetching (§10, plan 13), Nostr and NAT (plan 12).

## Global Constraints

- **Spec §8:**
  - The content list is the match's mod pack, level pack (and its theme) and every player's skin, each by the hash of its content.
  - The client looks each one up in its own content and its download cache, and downloads what is missing from the host over the control channel in resumable chunks.
  - Every download is verified against its hash, then loaded with the existing loaders (untrusted content).
  - The player is in the lobby while downloads run; Start waits for their ready mark.
  - Packs travel in the native format (`content/packs`), never as OLX originals.
- **Limits** (spec §8):
  - a prompt above `PROMPT_BYTES = 50 * 1024 * 1024` missing bytes per game;
  - a download cache of `CACHE_BYTES = 1 << 30` bytes, dropping the least recently used items;
  - a skin archive of at most `MAX_SKIN_ARCHIVE = 64 * 1024` bytes.
- **Never panic on network input or content** (spec §5.4): every new decoder is fuzzed with proptest (`cases: 256, failure_persistence: None`), and every message type has a size limit. Archive reading is bounded:
  - `MAX_ARCHIVE = 512 MiB`;
  - `MAX_ARCHIVE_FILES = 4096`;
  - `MAX_ARCHIVE_PATH = 255` bytes;
  - each file at most `rl_pack`'s `MAX_FILE_BYTES` (64 MiB).
- **The simulation's behaviour must not change.** Oracle traces, match goldens and render goldens stay byte-identical.
- **Layering** (`xtask/layers.ron`), unchanged:
  - `rl-pack` 2 (archive format);
  - `rl-proto` 4 (`no_std`, messages);
  - `rl-content` 4 (cache, overlay);
  - `rl-lobby` 4 (`no_std`);
  - `rl-online` 5 (transfers, worker thread);
  - `rl-ui` 5;
  - `rl-game` and `rl-server` 6.

  No new crate and no new `same_layer` pair.
- **Determinism and testability:**
  - Transfer logic takes time as an argument; the LRU order is a stored use counter, never the wall clock.
  - Real sockets and threads appear only in `rl-online`'s worker and the existing endpoint.
- **Rate limits stay as they are** (`rl-net`): a host receives at most 256 KiB/s of reliable bytes from each client, and a client at most 4 MiB/s from its host.
  - Content transfers stay well under them: at most 2 MiB/s per receiving client, at most 4 MiB/s host-wide (configurable on the server), 256 KiB/s host-wide during `Starting`/`Playing`, and 128 KiB/s for a client serving its own skin.
- **Commits:** run `cargo xtask check` before every commit; commit on `main`, never push. Commit trailers:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_019821i4KFSbhZ4mHYULrhEV
  ```

## Decisions this plan makes (recorded as STATUS.md N-rulings in Task 8)

- **N29 – pack archives.** A pack travels as a `.rlpk` archive:
  - magic `RLPK1\n`, then a `u32` file count, then per file, sorted by path: a `u16` path length, the UTF-8 `VPath`, a `u64` length and the bytes;
  - no compression: packs are mostly PNG and OGG;
  - deterministic: the same pack always gives the same bytes, so a resumed download continues byte-exactly;
  - the archive's own SHA-256 (the *archive hash*) identifies a partial download;
  - the *content hash* (`rl_pack::content_hash`, unchanged) identifies the pack.
- **N30 – a content-addressed cache.**
  - Downloads live in `<cache>/packs/<content hash>/`, one pack directory each.
  - `<cache>/index.ron` lists each entry's id, kind, bytes and a use counter (LRU).
  - Above `CACHE_BYTES`, entries are evicted in LRU order, but never one the current lobby lists.
  - A match's content is the player's own content with the needed cached packs laid over it by id (`Content::with_packs`). A local pack with the same id but another hash is hidden for that match only, never changed.
  - Paths:
    - the game's cache is `ProjectDirs::cache_dir()/downloads`;
    - the server's is `<data dir>/downloads`.
- **N31 – skins by hash.**
  - `Look` gains `skin_hash` (64 lowercase hex).
  - The host fetches a member's skin from that member, if the host lacks it, and serves it to the others.
  - Skins never block the ready mark: a skin that cannot be fetched draws as the default skin, as today.
  - Mod, level and theme are *required*.
- **N32 – transfers.**
  - Reliable-channel tag `TAG_CONTENT = 3` carries `ContentMsg` both ways: `List`, `Want`, `Begin`, `Chunk`, `Unavailable`, `Cancel`.
  - Chunks are 32 KiB.
  - A peer serves one archive at a time to each peer and queues the other wants.
  - Pacing uses the rates of the Global Constraints, with at most 256 KiB queued per peer.
  - Resume: the fetcher keeps `<hash>.part` and `<hash>.part.ron` (archive hash and size), and asks `Want { hash, offset }`. A different archive hash restarts from 0.
- **N33 – lacking content waits, never drops.** At a start, a member whose required content is incomplete sits the match out as `SeatWire::Waiting`. It is not in the session, gets no state, and stays connected. A joiner during a match waits the same way until its content is complete, then spectates. This replaces plan 10's "a spectator lacking content is dropped at every start".
- **N34 – protocol version 2.** `PROTOCOL_VERSION` becomes 2, because `Look`, `Member` and the reliable tags change.

## Review Focus

1. **A hostile or broken host's archive** (spec §5.4, §8): path traversal (`..`, absolute, backslashes), huge or many files, a truncated or padded archive, wrong content hash, a pack of another id or kind, a pack the loaders refuse.
   - Expected: refused with a message, nothing written outside the cache's temporary directory, no panic.
   - Pinned by:
     - Task 1's `hostile_archives_are_refused` and the fuzz proptests;
     - Task 3's `install_refuses_what_does_not_verify`;
     - Task 5's `a_tampered_archive_is_refused_and_retried_once`.
2. **A dropped connection mid-download, then a rejoin**: the download resumes at the partial's offset. If the host's level changes mid-download, the old transfer is cancelled and the new one starts. Pinned by Task 4's `a_download_resumes_after_a_reconnect` and `a_changed_list_cancels_the_transfer`.
3. **A local pack with the match's id but another hash** (an edited level): the match uses the downloaded version, the player's own pack is unchanged, and the replay plays back afterwards from the cache. Pinned by Task 3's `an_overlay_hides_a_local_pack_of_the_same_id` and Task 7's `a_replay_of_a_downloaded_level_plays_back`.
4. **Downloads during a match** (a waiting member fetching while others play): no peer trips a rate limit, and the match runs to its end. Pinned by Task 6's `downloads_during_a_match_keep_it_alive`.
5. **A full or unwritable cache**: eviction never removes a pack the lobby needs. An unwritable cache gives a clear lobby message and the player stays connected and not ready, with no panic. Pinned by Task 3's `eviction_keeps_what_the_lobby_needs` and Task 6's `an_unwritable_cache_is_reported`.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/rl-pack/src/archive.rs` (new) | `.rlpk` encode, bounded streaming decode into a `ContentSink`. |
| `crates/rl-proto/src/content.rs` (new) | `ContentMsg`, `Item`, `ItemKind`, validators, `TAG_CONTENT`. |
| `crates/rl-proto/src/lobby.rs` | `Look::skin_hash`, `Member::download`, the tag split gains `Tagged::Content`. |
| `crates/rl-content/src/cache.rs` (new) | `DownloadCache`: partials, install with verification, the LRU index, eviction. |
| `crates/rl-content/src/lib.rs` | `Content::with_packs`, a memoised `pack_hash`, `Content::has(id, hash)`. |
| `crates/rl-lobby/src/host.rs`, `model.rs` | N33's waiting seats, `set_download`, the skin hash in looks. |
| `crates/rl-online/src/content/{mod.rs, serve.rs, fetch.rs, worker.rs}` (new) | Content list, server, fetcher, worker thread. |
| `crates/rl-online/src/peer.rs` | Wiring: lists, wants, tag 3, overlays for match builds, events. |
| `crates/rl-online/src/events.rs` | `OnlineEvent::Downloads`, `OnlineEvent::DownloadPrompt`. |
| `crates/rl-ui/src/{store.rs, online.rs, screens/lobby.rs}` | Cache dir, download rows and prompt, "Clear download cache" in Options. |
| `crates/rl-game/src/{net.rs, setup.rs, shell/online.rs, replay*}` | Drop the game-side `missing_packs`, load a match from the peer's content, replays from the cache. |
| `crates/rl-server/src/{config.rs, lib.rs, main.rs}` | `upload_kib_s`, the server's cache. |
| `crates/rl-cli/src/net_client_cmd.rs`, `transport_test.rs` | A bot client with partial content downloads. |
| `docs/porting/STATUS.md`, `CHANGELOG.md`, `docs/online.md` | Records. |

---

### Task 1: `rl-pack::archive`: the `.rlpk` format

**Files:**
- Create: `crates/rl-pack/src/archive.rs`, `crates/rl-pack/tests/archive.rs` (a `mod archive;` of `tests/integration.rs`)
- Modify: `crates/rl-pack/src/lib.rs` (export), `crates/rl-pack/CLAUDE.md`
- Test: `tests/archive.rs`

**Interfaces:**
- Produces (Tasks 3, 4):
  ```rust
  pub const MAGIC: &[u8; 6] = b"RLPK1\n";
  pub const MAX_ARCHIVE: u64 = 512 << 20;
  pub const MAX_ARCHIVE_FILES: u32 = 4096;
  pub const MAX_ARCHIVE_PATH: usize = 255;
  /// The archive of every file under `src`'s root (sorted by path, `pack.ron`
  /// and presentation files included), deterministic.
  pub fn encode(src: &dyn ContentSource) -> Result<Vec<u8>, PackError>;
  /// Reads an archive from `r`, writing each file into `sink`; stops at the
  /// first violation. Returns the files written.
  pub fn decode(r: &mut dyn std::io::Read, sink: &mut dyn ContentSink, limit: u64) -> Result<u32, ArchiveError>;
  #[derive(Debug, thiserror::Error)] pub enum ArchiveError {
      #[error("not a pack archive")] Magic,
      #[error("the archive is cut short")] Truncated,
      #[error("the archive has bytes after its last file")] Trailing,
      #[error("the archive is larger than {0} bytes")] TooLarge(u64),
      #[error("the archive has too many files")] TooManyFiles,
      #[error("bad path in the archive: {0}")] BadPath(String),
      #[error("the archive lists {0} twice or out of order")] Order(String),
      #[error("{0}")] Io(String),
  }
  ```

Context: N29. `VPath::new` already refuses `..`, absolute paths and empty components. The decoder also refuses backslashes, paths over 255 bytes, names starting with `.`, duplicates and unsorted order (strictly increasing byte order). Sorting makes duplicates detectable in one pass. Reading is streaming: the decoder never holds more than one file (at most `MAX_FILE_BYTES`) in memory. `limit` caps the archive's total bytes; the caller passes `MAX_SKIN_ARCHIVE` for skins and `MAX_ARCHIVE` otherwise. `encode` reads through `walk` and `read_max(MAX_FILE_BYTES)`.

- [ ] **Step 1: Write the failing tests**

```rust
//! The `.rlpk` pack archive (plan 11 decision N29).

use rl_pack::archive::{self, ArchiveError};
use rl_vfs::{ContentSource, MemFs, VPath};

fn pack() -> MemFs {
    MemFs::new()
        .with("pack.ron", "(format_version: 4, id: \"x\", name: \"X\", version: \"1\", kind: Skin, authors: [])")
        .with("skin.ron", "(name: \"x\")")
        .with("gfx/a.png", vec![1u8, 2, 3])
}

#[test]
fn round_trip_is_exact_and_deterministic() {
    let a = archive::encode(&pack()).unwrap();
    assert_eq!(a, archive::encode(&pack()).unwrap());
    let mut out = MemFs::new();
    assert_eq!(archive::decode(&mut a.as_slice(), &mut out, archive::MAX_ARCHIVE).unwrap(), 3);
    for (path, bytes) in pack().files() {
        assert_eq!(out.read(path).unwrap(), bytes);
    }
}

fn with_entry(path: &str) -> Vec<u8> {
    let mut a = archive::MAGIC.to_vec();
    a.extend(1u32.to_le_bytes());
    a.extend((path.len() as u16).to_le_bytes());
    a.extend(path.as_bytes());
    a.extend(1u64.to_le_bytes());
    a.push(0);
    a
}

#[test]
fn hostile_archives_are_refused() {
    for bad in ["../x", "/x", "a\\b", ".hidden", "a//b", ""] {
        let r = archive::decode(&mut with_entry(bad).as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
        assert!(matches!(r, Err(ArchiveError::BadPath(_))), "{bad}: {r:?}");
    }
    let long = "a".repeat(archive::MAX_ARCHIVE_PATH + 1);
    assert!(matches!(archive::decode(&mut with_entry(&long).as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE), Err(ArchiveError::BadPath(_))));
    let good = archive::encode(&pack()).unwrap();
    assert!(matches!(archive::decode(&mut &good[..good.len() - 1], &mut MemFs::new(), archive::MAX_ARCHIVE), Err(ArchiveError::Truncated)));
    let mut padded = good.clone();
    padded.push(0);
    assert!(matches!(archive::decode(&mut padded.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE), Err(ArchiveError::Trailing)));
    assert!(matches!(archive::decode(&mut good.as_slice(), &mut MemFs::new(), 16), Err(ArchiveError::TooLarge(16))));
    let mut many = archive::MAGIC.to_vec();
    many.extend((archive::MAX_ARCHIVE_FILES + 1).to_le_bytes());
    assert!(matches!(archive::decode(&mut many.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE), Err(ArchiveError::TooManyFiles)));
}

#[test]
fn duplicates_and_disorder_are_refused() {
    // Two entries "b" then "a": out of order.
    let mut a = archive::MAGIC.to_vec();
    a.extend(2u32.to_le_bytes());
    for p in ["b", "a"] {
        a.extend(1u16.to_le_bytes());
        a.extend(p.as_bytes());
        a.extend(0u64.to_le_bytes());
    }
    assert!(matches!(archive::decode(&mut a.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE), Err(ArchiveError::Order(_))));
}

#[test]
fn a_file_length_beyond_the_limit_is_refused_before_reading() {
    let mut a = archive::MAGIC.to_vec();
    a.extend(1u32.to_le_bytes());
    a.extend(1u16.to_le_bytes());
    a.extend(b"a");
    a.extend(u64::MAX.to_le_bytes());
    assert!(matches!(archive::decode(&mut a.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE), Err(ArchiveError::TooLarge(_))));
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 256, failure_persistence: None, ..Default::default() })]
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
        let _ = archive::decode(&mut bytes.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
    }
    #[test]
    fn mutated_archives_never_panic(edits in proptest::collection::vec((0usize..200, proptest::prelude::any::<u8>()), 1..8), cut in 0usize..200) {
        let mut a = archive::encode(&pack()).unwrap();
        for (i, b) in edits { if i < a.len() { a[i] = b; } }
        a.truncate(a.len().saturating_sub(cut));
        let _ = archive::decode(&mut a.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
    }
}
```

Also add `every_committed_pack_round_trips` (skipped with a notice when `content/packs` is absent): encode `olx-classic`, `olx-level-dirt-level` and `olx-skin-default` through `DirFs`, decode each into a `MemFs`, and check that `content_hash` is equal on both sides. It also asserts that `olx-skin-default`'s archive is at most `64 * 1024` bytes. If it is not, stop and report: N31's skin limit would then be wrong.

- [ ] **Step 2: Run them, expect a compile failure.** `cargo test -p rl-pack --test integration archive`.
- [ ] **Step 3: Implement `archive.rs`.** Add `proptest` as a dev-dependency if `rl-pack` lacks it, and `MemFs::files` if it is not public. Document the format in the module docs, with the N29 reference.
- [ ] **Step 4: Run the tests and expect PASS.** Also run `cargo build -p rl-pack --target wasm32-unknown-unknown`: rl-pack is a WASM crate, so `decode` reads through `std::io::Read`, which exists there.
- [ ] **Step 5: Commit** `rl-pack: the .rlpk pack archive`.

---

### Task 2: `rl-proto`: content messages, skin hashes, protocol 2

**Files:**
- Create: `crates/rl-proto/src/content.rs`
- Modify: `crates/rl-proto/src/{lib.rs, lobby.rs}`, `crates/rl-proto/tests/fuzz_lobby.rs`, every constructor of `Look`/`Member` in the workspace (compile fixes only: `skin_hash: String::new()` is invalid, so use `rl_proto::NO_HASH`, defined below)
- Test: `crates/rl-proto/tests/fuzz_lobby.rs`, unit tests in `content.rs`

**Interfaces:**
- Produces (Tasks 4-7):
  ```rust
  pub const PROTOCOL_VERSION: u16 = 2;
  pub const TAG_CONTENT: u8 = 3;
  pub const CONTENT_CHUNK: usize = 32 * 1024;
  pub const MAX_CONTENT: usize = CONTENT_CHUNK + 256;      // one message's bound
  pub const MAX_ITEMS: usize = 64;
  pub const NO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum ItemKind { Mod, Level, Theme, Skin }
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Item { pub kind: ItemKind, pub id: String, pub hash: String, pub size: u64, pub required: bool }
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub enum ContentMsg {
      /// The match's content (host → client), whole, on every change.
      List(Vec<Item>),
      /// Send the archive of `hash` from byte `offset`.
      Want { hash: String, offset: u64 },
      /// The archive follows: its size and its SHA-256 (hex), from `offset`.
      Begin { hash: String, archive: String, size: u64, offset: u64 },
      /// Archive bytes at `offset` (at most `CONTENT_CHUNK`).
      Chunk { hash: String, offset: u64, bytes: Vec<u8> },
      /// This peer will not send `hash` (not listed, failed, too large).
      Unavailable { hash: String },
      /// Stop sending `hash`.
      Cancel { hash: String },
  }
  pub fn encode_content(m: &ContentMsg) -> Vec<u8>;            // tag byte included
  pub fn decode_content(bytes: &[u8]) -> Result<ContentMsg, ProtoError>; // without the tag
  pub fn valid_hash(h: &str) -> bool;                          // 64 lowercase hex
  pub fn valid_content(m: &ContentMsg) -> bool;
  // lobby.rs
  pub struct Look { pub name: String, pub colour: [u8; 3], pub skin: String, pub skin_hash: String }
  pub struct Member { /* existing fields */ pub download: Option<u8> /* percent of required content received, while downloading */ }
  pub enum Tagged<'a> { Netcode(&'a [u8]), ToHost(&'a [u8]), ToClient(&'a [u8]), Content(&'a [u8]) }
  ```

Context:
- **Validity.**
  - `valid_content` requires valid hashes, `size <= rl_pack`'s `MAX_ARCHIVE` (restate the constant as `MAX_ARCHIVE_BYTES: u64 = 512 << 20` in rl-proto, since rl-proto may not depend on rl-pack), `offset <= size`, `bytes.len() <= CONTENT_CHUNK`, and a `List` of at most `MAX_ITEMS` items with valid pack ids.
  - In a `List`, no hash appears twice, and a `Skin` item has `size <= 64 * 1024` and `required == false`.
- **Look.** `valid_look` also requires `valid_hash(skin_hash)`. `NO_HASH` is valid and means "unknown". A peer never fetches `NO_HASH`.
- **Wire order.**
  - `Member::download` is appended last. `Look::skin_hash` is appended last.
  - Postcard encodes field order, so this changes the encoding, which is fine at protocol 2.
- **Fuzzing.** The existing `worst_case_messages_fit` test gains a worst-case `List` (64 items, longest ids) and a full `Chunk`, both at most `MAX_CONTENT`; `MAX_CONTENT` is the bound only if the worst-case `List` fits. If it does not, raise `MAX_CONTENT` to fit and note it.
- **Reliable cap.** `rl-net`'s `MAX_RELIABLE` stays `max(MAX_CONTROL, MAX_LOBBY) + 1`. Add an assertion test in `rl-net` that `MAX_CONTENT + 1 <= MAX_RELIABLE`.

- [ ] **Step 1: Write the failing tests.**
  - Unit tests in `content.rs`:
    - `valid_messages_round_trip` (each variant);
    - `invalid_messages_are_refused` (bad hash, oversized chunk, offset > size, duplicate hashes in a list, a required skin, a 65-item list, a skin over 64 KiB).
  - Add one sample of each `ContentMsg` to `fuzz_lobby.rs`'s `samples()`, and `decode_content` to `decode_all`.
  - Extend `worst_case_messages_fit`.
- [ ] **Step 2: Run `cargo test -p rl-proto`, expect FAIL.**
- [ ] **Step 3: Implement.** `tag()` splits off `TAG_CONTENT` as `Tagged::Content`. Fix every compile error in the workspace:
  - looks built by the game and the server take `NO_HASH` until Task 6 fills them;
  - `Member { download: None, .. }`.
- [ ] **Step 4: Run `cargo test -p rl-proto -p rl-lobby -p rl-online -p rl-net`, then `cargo xtask check`, and expect PASS.**
- [ ] **Step 5: Commit** `rl-proto: content messages, skin hashes, protocol 2`.

---

### Task 3: `rl-content`: the download cache and overlays

**Files:**
- Create: `crates/rl-content/src/cache.rs`, `crates/rl-content/tests/cache.rs` (a `mod cache;` of `tests/integration.rs`)
- Modify: `crates/rl-content/src/lib.rs`, `crates/rl-content/CLAUDE.md`
- Test: `tests/cache.rs`

**Interfaces:**
- Consumes: `rl_pack::archive::{decode, MAX_ARCHIVE}`, `rl_pack::{content_hash, validate, read_manifest}`, `rl_proto` is **not** used (same layer); the item kind is `rl_data::PackKind`.
- Produces (Tasks 4-7):
  ```rust
  pub const CACHE_BYTES: u64 = 1 << 30;
  pub const MAX_SKIN_ARCHIVE: u64 = 64 * 1024;
  pub struct DownloadCache { .. }
  pub struct Partial { pub archive: String, pub size: u64, pub have: u64 }
  #[derive(Debug, thiserror::Error)] pub enum InstallError {
      #[error("{0}")] Archive(#[from] rl_pack::archive::ArchiveError),
      #[error("the download's content differs from the host's ({0})")] Hash(String),
      #[error("the download is pack {found}, not {wanted}")] Id { wanted: String, found: String },
      #[error("the download is a {found:?}, not a {wanted:?}")] Kind { wanted: PackKind, found: PackKind },
      #[error("the downloaded pack is invalid: {0}")] Invalid(String),
      #[error("the download cache: {0}")] Io(String),
  }
  impl DownloadCache {
      pub fn open(dir: &Path) -> Result<DownloadCache, InstallError>;   // creates it; rebuilds a bad index by scanning
      pub fn dir(&self) -> &Path;
      pub fn get(&mut self, hash: &str) -> Option<PathBuf>;            // touches it (LRU)
      pub fn contains(&self, hash: &str) -> bool;
      pub fn partial(&self, hash: &str) -> Option<Partial>;
      pub fn append(&mut self, hash: &str, archive: &str, size: u64, offset: u64, bytes: &[u8]) -> Result<u64 /* have */, InstallError>;
      pub fn discard_partial(&mut self, hash: &str);
      /// Verifies the complete partial and moves it into the cache.
      pub fn install(&mut self, hash: &str, id: &str, kind: PackKind, limit: u64) -> Result<PathBuf, InstallError>;
      pub fn evict(&mut self, keep: &[&str], limit: u64) -> Vec<String>; // evicted hashes
      pub fn total_bytes(&self) -> u64;
      pub fn clear(&mut self, keep: &[&str]) -> Result<(), InstallError>;
  }
  impl Content {
      /// A copy whose packs `packs` (id → pack directory) hide this content's packs of the same id.
      pub fn with_packs(&self, packs: &[(String, PathBuf)]) -> anyhow::Result<Content>;
      /// Whether this content has pack `id` with content hash `hash` (memoised per pack directory).
      pub fn has(&self, id: &str, hash: &str) -> bool;
  }
  ```

Context: N30.
- **Install.**
  1. Read `<hash>.part` through `archive::decode` into a fresh `<dir>/tmp/<hash>-<n>/` directory (a `DirFs` sink).
  2. Check that `content_hash(DirFs)` equals `hash`, that the manifest's `id` and `kind` are the wanted ones, and that `validate` reports no `Severity::Error`.
  3. Rename into `<dir>/packs/<hash>/`.
  4. Any failure removes the temporary directory and the partial. A partial whose archive hash is wrong is discarded at `install` (the archive SHA-256 is checked first).
- **Archive writes.** `append` writes at `offset == have` only. Any other offset returns an error, and the caller restarts with `Want { offset: have }`.
- **Index.** The index is RON, written atomically (temp file plus rename). A missing or unreadable index is rebuilt from `packs/*/pack.ron` with use counter 0.
- **Memoisation.** `Content::pack_hash` memoises in a `Mutex<BTreeMap<PathBuf, String>>`, keyed by the pack directory. Content is opened per session, so staleness within a session is acceptable; say so in the doc comment.
- **Overlay.** `with_packs` reads each directory's manifest and inserts it as a new top layer. Nothing on disk changes.

- [ ] **Step 1: Write the failing tests**, using `tempfile::tempdir()` and archives built with `rl_pack::archive::encode` from `MemFs` packs (a valid skin pack, as in Task 1's `pack()`, plus a level pack copied from `content/packs/olx-level-dirt-level` when present):
  - `a_complete_download_installs_and_is_found` (append in 3 chunks, install, `get` returns `packs/<hash>`, `contains`).
  - `a_partial_survives_reopening` (append half, drop, reopen, `partial` reports `have`; append the rest; install).
  - `install_refuses_what_does_not_verify`:
    - a flipped byte, giving the `Hash` error;
    - another id, giving `Id`;
    - a level archived as a skin, giving `Kind`;
    - an invalid pack whose `skin.ron` is garbage, giving `Invalid`;
    - a skin archive over `MAX_SKIN_ARCHIVE` with `limit` `MAX_SKIN_ARCHIVE`, giving `Archive(TooLarge)`.

    After each, assert that `dir/tmp` is empty and nothing exists outside `dir`.
  - `eviction_keeps_what_the_lobby_needs` (three entries; touch order; `evict(&[oldest], small_limit)` evicts the middle one, never `oldest`).
  - `a_damaged_index_is_rebuilt` (write garbage to `index.ron`, reopen, entries found).
  - `an_overlay_hides_a_local_pack_of_the_same_id` (a local skin `x` with hash A; a cached `x` with hash B; `content.with_packs(&[("x", cached)])` has `x` at B, the original content still at A, and the files on disk are unchanged).
  - `has_compares_the_hash`.
- [ ] **Step 2: Run `cargo test -p rl-content --test integration cache`, expect FAIL.**
- [ ] **Step 3: Implement** `cache.rs` and the `Content` additions.
- [ ] **Step 4: Run, expect PASS; then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-content: a content-addressed download cache and overlays`.

---

### Task 4: `rl-online::content`: serving and fetching, sans-IO

**Files:**
- Create: `crates/rl-online/src/content/{mod.rs, serve.rs, fetch.rs, worker.rs}`, `crates/rl-online/tests/content.rs` (a `mod content;` of `tests/integration.rs`)
- Modify: `crates/rl-online/src/lib.rs`, `crates/rl-online/CLAUDE.md`
- Test: `tests/content.rs`, unit tests in `serve.rs` and `fetch.rs`

**Interfaces:**
- Consumes: Task 2's `ContentMsg`, `Item`, `CONTENT_CHUNK`; Task 3's `DownloadCache`, `Content::{has, pack_hash}`; Task 1's `archive::encode`.
- Produces (Tasks 5, 6):
  ```rust
  pub mod content {
      pub const PROMPT_BYTES: u64 = 50 * 1024 * 1024;
      pub const PEER_RATE: u64 = 2 << 20;        // bytes/s to one receiver
      pub const HOST_RATE: u64 = 4 << 20;        // bytes/s host-wide (default)
      pub const MATCH_RATE: u64 = 256 << 10;     // host-wide during Starting/Playing
      pub const SKIN_RATE: u64 = 128 << 10;      // a client serving its own skin
      pub const QUEUED: usize = 256 << 10;       // per-peer bytes in the outbox before more chunks

      /// Serves archives to peers: sans-IO, time injected.
      pub struct Server { .. }
      impl Server {
          pub fn new(rate: u64) -> Server;
          pub fn set_rate(&mut self, rate: u64);
          /// The archives this server may send, by content hash (built by the worker).
          pub fn offer(&mut self, hash: &str, archive: std::sync::Arc<Vec<u8>>);
          pub fn withdraw(&mut self, hash: &str);
          pub fn receive(&mut self, peer: PeerId, msg: &ContentMsg);       // Want, Cancel
          pub fn peer_left(&mut self, peer: PeerId);
          /// Messages to send now; `room(peer)` is how many bytes the peer's outbox may take.
          pub fn poll(&mut self, now_us: u64, room: &dyn Fn(PeerId) -> usize, out: &mut Vec<(PeerId, ContentMsg)>);
      }

      #[derive(Clone, Debug, PartialEq)] pub struct Download { pub item: Item, pub have: u64, pub state: DownloadState }
      #[derive(Clone, Debug, PartialEq)] pub enum DownloadState { Queued, Receiving, Verifying, Done, Failed(String), Unavailable }

      /// Fetches what a content list lacks from one source peer.
      pub struct Fetcher { .. }
      impl Fetcher {
          pub fn new(cache: std::sync::Arc<std::sync::Mutex<DownloadCache>>) -> Fetcher; // shared with the Worker
          /// A new list from `source`: returns the missing items (local content and cache checked).
          pub fn list(&mut self, source: PeerId, items: &[Item], local: &Content) -> Vec<Item>;
          pub fn approve(&mut self);                     // after the prompt, or at once below PROMPT_BYTES
          pub fn receive(&mut self, from: PeerId, msg: &ContentMsg, worker: &Worker);
          pub fn poll(&mut self, now_us: u64, worker: &Worker, out: &mut Vec<(PeerId, ContentMsg)>);
          pub fn downloads(&self) -> &[Download];
          pub fn missing_required(&self) -> Vec<String>;  // ids, for `ToHost::Have`
          pub fn installed(&self) -> Vec<(String, PathBuf)>; // (id, dir) of this list's cached packs, for `Content::with_packs`
          pub fn source_left(&mut self);                 // keep partials; resume on the next list
      }

      /// One background thread for hashing, archiving and verifying.
      pub struct Worker { .. }
      pub enum Job { Archive { hash: String, dir: PathBuf }, Install { hash: String, id: String, kind: PackKind, limit: u64 }, Hash { id: String, dir: PathBuf } }
      pub enum Done { Archived { hash: String, archive: Result<std::sync::Arc<Vec<u8>>, String> }, Installed { hash: String, dir: Result<PathBuf, String> }, Hashed { id: String, hash: Result<String, String> } }
      impl Worker { pub fn start(cache: std::sync::Arc<std::sync::Mutex<DownloadCache>>) -> Worker; pub fn submit(&self, job: Job); pub fn take(&self) -> Vec<Done>; }
  }
  ```
  The exact split between `Fetcher` and the worker may be adjusted (rule on it and record it in the report). The behaviour below is the requirement.

Context:
- **Server.**
  - One transfer per receiving peer: the first queued `Want` whose hash is offered. A `Want` for an un-offered hash gets `Unavailable` (bounded: one per `Want`).
  - Pacing: a token bucket per receiver at `PEER_RATE` and one shared bucket at the server's `rate`, with round-robin over receivers. A chunk goes only when `room(peer) >= CONTENT_CHUNK + 64` and the outbox holds less than `QUEUED`.
  - `Begin` precedes the first chunk of each transfer, including a resumed one: it carries the archive's SHA-256 and the `offset`.
  - At most `MAX_ITEMS` queued wants per peer: extra ones are ignored.
- **Fetcher.**
  - Downloads one item at a time, in list order: required items first, skins after.
  - Received chunks are written to the cache (`append`). At the end it submits `Install` to the worker.
  - On `Done::Installed(Err)` it discards the partial, retries once from 0, then marks `Failed`.
  - `Unavailable` marks the item `Unavailable`.
  - A new `List` without a current item sends `Cancel` for it and moves on. Its partial is kept: a future list may want it again.
  - A `Begin` whose archive hash differs from the partial's discards the partial and restarts at the begin's offset only if that offset is 0. Otherwise it sends `Want { offset: 0 }`.
  - Items above `PROMPT_BYTES` in total wait for `approve`.
  - **Time:** time arrives as `now_us`. A silent source (no chunk for 10 s while receiving) is re-asked once, then the item is `Failed("the host stopped sending")`.

**Tests**, in memory: a `Server` and a `Fetcher` wired directly with a simulated outbox room of 1 MiB, on a virtual clock. The worker is real: it is a thread, so wait on `take` with a bounded condition loop.
- `a_level_downloads_installs_and_verifies` (with real `olx-level-dirt-level` when the content dir exists, else a generated pack): all states pass through to `Done`, and `installed()` lists it.
- `a_download_resumes_after_a_reconnect`: stop after 3 chunks, call `source_left`, then a new `Fetcher` on the same cache dir gets the list again. Its first message is `Want { offset: 3 * CONTENT_CHUNK }`.
- `a_changed_list_cancels_the_transfer`.
- `a_tampered_archive_fails_after_one_retry`: the server's archive is a valid archive of another pack, so the hash differs.
- `the_server_paces_to_its_rates`: on a virtual clock, 10 s of serving 3 receivers at a server `rate` of 1 MiB/s sends between 9.5 and 10.5 MiB in total, and no receiver more than `PEER_RATE`.
- `unoffered_wants_are_refused`.
- `big_lists_wait_for_approval`: total over `PROMPT_BYTES`, so no `Want` before `approve`.

- [ ] **Step 1: Write the failing tests** as listed (in `tests/content.rs`; pacing and state-machine unit tests in `serve.rs`/`fetch.rs`).
- [ ] **Step 2: Run `cargo test -p rl-online content`, expect FAIL.**
- [ ] **Step 3: Implement.** The worker thread exits when `Worker` drops (channel closed); `Drop` joins it with a bound.
- [ ] **Step 4: Run, expect PASS. Then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-online: serve and fetch pack archives`.

---

### Task 5: `rl-lobby`: waiting seats, download progress, skin hashes

**Files:**
- Modify: `crates/rl-lobby/src/{host.rs, model.rs, client.rs}`, `crates/rl-lobby/CLAUDE.md`
- Test: unit tests in `host.rs` (and `harness` tests)

**Interfaces:**
- Produces (Task 6):
  ```rust
  impl LobbyHost {
      /// The share of `member`'s required content received, while it downloads (`None`: complete or not started).
      pub fn set_download(&mut self, member: u8, percent: Option<u8>);
      /// Members whose required content is incomplete (`Have` with ids).
      pub fn lacking(&self) -> Vec<u8>;
  }
  // `LobbyHost::start`/`force_start`: a seated member in `lacking()` sits the match out as `SeatWire::Waiting` (N33).
  // A member who joins during a match stays `Waiting` while it lacks content; `HostEvent::Joined`/the existing mid-match path fires only once it is complete.
  ```

Context: N33. Today a member whose `missing` is non-empty is already not `ready()`, so `start` refuses while a seated player lacks content, and `force_start` makes them spectators.
- **Starts.** In both starts, spectators lacking content now become `Waiting` instead of being sent the start. `force_start` makes lacking *players* `Waiting` too, not spectators.
- **Mid-match.** The mid-match spectator path (plan 10, `rl-online`'s `mid_match_join`) must only run for a member with complete content. `LobbyHost` reports completion through the existing event, or through a new `HostEvent::ContentComplete(u8)` (rule on whichever fits `host.rs`).
- **Progress.** `set_download` publishes `Member::download`, coalesced like other lobby changes, at most one change per member per 250 ms of `now`.

- [ ] **Step 1: Write the failing tests:**
  - `a_spectator_lacking_content_waits_at_the_start`;
  - `force_start_makes_lacking_players_wait`;
  - `a_mid_match_joiner_spectates_once_complete`;
  - `download_progress_is_published_and_coalesced`.
- [ ] **Step 2: Run `cargo test -p rl-lobby`, expect FAIL.**
- [ ] **Step 3: Implement.** The lobby stays `no_std`.
- [ ] **Step 4: Run, expect PASS. Then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-lobby: members lacking content wait instead of being dropped`.

---

### Task 6: `rl-online`: content in the online peer

**Files:**
- Modify: `crates/rl-online/src/{peer.rs, events.rs, matches.rs, lib.rs}`, `crates/rl-online/tests/{online.rs, common/mod.rs}`
- Test: `crates/rl-online/tests/online.rs`

**Interfaces:**
- Consumes: Tasks 2-5.
- Produces (Task 7):
  ```rust
  pub struct PeerContent { pub cache_dir: Option<PathBuf>, pub upload_rate: u64 /* host-wide, bytes/s */ }
  // OnlinePeer::start gains `content_cfg: PeerContent` (None cache: downloads disabled, the lobby says so).
  impl OnlinePeer {
      /// The content this peer's matches load from: its own with the list's downloaded packs laid over it.
      pub fn content(&self) -> std::sync::Arc<Content>;
      pub fn downloads(&self) -> Vec<content::Download>;
      pub fn approve_downloads(&mut self);
  }
  // OnlineEvent gains:
  //   Downloads(Vec<content::Download>)      — progress changed (coalesced, ≤ 4 per second)
  //   DownloadPrompt { bytes: u64 }          — the missing content exceeds PROMPT_BYTES; approve or leave
  ```

Context:
- **Host.**
  - On every setup or member-look change, the host builds the content list: the setup's `packs` (mod, level, theme: `required: true`) plus every distinct skin of members and bots (`required: false`).
  - Sizes come from the worker's archives: the host archives everything it has, and items not yet archived are listed once archived.
  - It sends `ContentMsg::List` to every client on change and to each joiner after its `Welcome`.
  - It serves archives through `content::Server` at its `upload_rate`, or `MATCH_RATE` while the phase is `Starting`/`Playing`.
  - **Skins it lacks.** For a member's skin the host lacks (by `skin_hash`, not `NO_HASH`), the host acts as a fetcher toward that member: `Want`, with `MAX_SKIN_ARCHIVE` as the limit. Once installed, the skin is offered to the others.
  - **Look hashes.** It fills its own and its bots' `Look::skin_hash` from its content (`Content::pack_hash` through the worker).
- **Client.**
  - On `List`, it runs `Fetcher::list` against its content and cache, emits `DownloadPrompt` when over the limit, and otherwise approves at once.
  - It sends `ToHost::Have { missing: fetcher.missing_required() }` whenever that changes; this replaces the game-side `missing_packs`.
  - It reports progress to the host through `Have` as it is today. The host derives `Member::download` from the bytes it has sent that member, so the client sends nothing extra.
  - It serves its own skin's archive (only that hash) at `SKIN_RATE`.
  - It fills its own `Look::skin_hash` from its content before `Hello`.
- **Matches.**
  - `matches::build` runs on `self.content()` (the overlay).
  - A client that still lacks required content when `Start` arrives is a `Waiting` member, so it gets no start (Task 5).
  - The existing `check_packs` failure path stays as the last guard.
- **Tag 3.** Reliable messages tagged `TAG_CONTENT` route to the server or fetcher. Decode errors drop the peer, as for other tags.
- **Leaving.** Leaving or a disconnect calls `Fetcher::source_left` and `Server::peer_left`.

**Tests** (over localhost, extending `tests/online.rs`; helpers in `common/mod.rs`):
- A `partial_content(&[&str] /* pack ids to leave out */) -> (TempDir, Content)` helper copies only `olx-core`, `olx-classic`, `olx-theme-dirt`, `olx-level-dirt-level` and `olx-skin-default` from `RL_CONTENT_DIR`, minus the listed ones.

The tests:
- `a_client_without_the_level_downloads_it_and_plays`: a host plus a client lacking `olx-level-dirt-level`. The client gets `Downloads` events ending in `Done`, becomes ready, and the match starts and ends with equal replays. The cache holds the level.
- `a_client_without_the_mod_downloads_it`: the same, with `olx-classic`.
- `a_skin_travels_through_the_host`: client A has a skin pack `test-skin` (generated into its content); the host and client B lack it. B ends with it in its cache, and the host fetched it from A.
- `a_tampered_archive_is_refused_and_retried_once`: a test hook on the host serves a valid archive of another pack for the level's hash. The client shows `Failed`, stays connected and not ready, and the lobby shows the member's `missing`.
- `a_spectator_lacking_content_waits_then_spectates`: a spectator without the level joins mid-match. It is `Waiting`, downloads during the match, then gets the state and spectates.
- `downloads_during_a_match_keep_it_alive`: the same run asserts that no peer saw `Why::Limits` and that the match reached its end.
- `an_unwritable_cache_is_reported`: the cache dir is a file path, so `PeerContent { cache_dir }` points at a regular file. The client gets a `Notice` naming the cache, stays connected and is not ready.
- `big_downloads_wait_for_the_prompt`: hook the prompt limit down with a `test-hooks` setter. The client gets `DownloadPrompt`, nothing downloads until `approve_downloads`, then the download completes.

- [ ] **Step 1: Write the failing tests.**
- [ ] **Step 2: Run `cargo test -p rl-online --features test-hooks`, expect FAIL.**
- [ ] **Step 3: Implement.** Keep `peer.rs` from growing further: content wiring lives in `content/peer_side.rs` (or similar), called from `peer.rs`'s poll.
- [ ] **Step 4: Run, expect PASS. Run the whole `rl-online` suite three times to make sure it is not flaky, then `cargo xtask check`.**
- [ ] **Step 5: Commit** `rl-online: download a match's missing content from the host`.

---

### Task 7: the game and the menus

**Files:**
- Modify:
  - `crates/rl-ui/src/{store.rs, online.rs, screens/lobby.rs, screens/options.rs}`;
  - `crates/rl-game/src/{net.rs, setup.rs, shell/online.rs, shell.rs}`;
  - the game's replay loading (`crates/rl-game/src/...replay...`; find where `check_replay_packs` is called);
  - `crates/rl-game/tests/online.rs`;
  - kittest tests in `crates/rl-ui/tests/`.
- Test: the above.

**Interfaces:**
- Consumes: Task 6's `OnlinePeer::{content, downloads, approve_downloads}`, `OnlineEvent::{Downloads, DownloadPrompt}`, Task 3's `DownloadCache`.
- Produces:
  ```rust
  impl Store { pub fn cache_dir(&self) -> Option<PathBuf>; }        // ProjectDirs::cache_dir()/downloads; None in memory
  pub struct DownloadView { pub name: String, pub kind: String, pub have: u64, pub size: u64, pub state: String }
  // LobbyView gains `downloads: Vec<DownloadView>` and `prompt: Option<u64>` (bytes).
  // UiAction gains `LobbyDownload(bool)` (approve / leave) and `ClearDownloadCache`.
  ```

Context:
- **Removed.** The game-side `missing_packs`/`checked_packs` go: `rl-online` reports `Have` itself.
- **Matches.** `setup::load_online` loads the match's packs, including downloaded skins, from `peer.content()` instead of re-opening the roots, so rendering uses the same packs the match was built from. A skin still missing falls back to the default skin, as today.
- **Replays.** Replay playback resolves each `PackRef` from local content, or else from the download cache by hash (`Content::with_packs`), before `check_replay_packs`. Pin this with `a_replay_of_a_downloaded_level_plays_back`.
- **Lobby screen.**
  - **Download list.** A "Downloads" list under the members, one row per item: name, kind, a progress bar (`have/size`), and a state (`queued`, `downloading`, `checking`, `done`, `failed: <why>`, `not available`).
  - **Member rows.** Each row shows `downloading 45%` when `Member::download` is `Some`; otherwise `missing: …` as now.
  - **Prompt.** A prompt frame: "This game needs 73.4 MB of content from the host. [Download] [Leave]".
  - **Ready.** The ready checkbox is disabled, with the hover text "get the missing content first", while required content is missing.
- **Options.** A "Download cache: 312 MB [Clear]" row (Clear keeps nothing in use: the game is in the menus).
- **Paths.** `Shell` passes `PeerContent { cache_dir: store.cache_dir(), upload_rate: HOST_RATE }`.

- [ ] **Step 1: Write the failing tests:**
  - kittest `lobby_shows_downloads_and_the_prompt` (a `LobbyView` with downloads and a prompt; clicking Download emits `LobbyDownload(true)`);
  - `ready_is_disabled_while_content_is_missing`;
  - `options_clears_the_download_cache`;
  - in `rl-game/tests/online.rs`, `a_joiner_without_the_level_downloads_and_plays` (the game's online test harness, the client with partial content and a temporary cache);
  - `a_replay_of_a_downloaded_level_plays_back`.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run `cargo test -p rl-ui -p rl-game`, expect PASS. Then `cargo xtask check`. If the UI changes a committed HUD snapshot (`fixtures/ui/`), run `cargo xtask bless-render --reason "plan 11: download rows in the lobby"` only when the change is intended.**
- [ ] **Step 5: Commit** `rl-game: download missing content in the lobby`.

---

### Task 8: the server, real-transport tests and records

**Files:**
- Modify:
  - `crates/rl-server/src/{config.rs, lib.rs, main.rs}`, `crates/rl-server/tests/server.rs`;
  - `crates/rl-cli/src/{net_client_cmd.rs, transport_test.rs}` (or wherever the real-transport tests live);
  - `docs/porting/STATUS.md` (N29-N34), `CHANGELOG.md`, `docs/online.md`, `crates/*/CLAUDE.md` touched.
- Test: `rl-server` tests, `rl-cli` real-transport tests.

Context:
- **Config.** `server.ron` gains `upload_kib_s: u32` (default 4096, minimum 64, maximum 65536). The server's cache is `<data dir>/downloads`.
- **net-client.** `rl-cli net-client` gains `--cache <dir>` (default: a temporary directory removed at exit), so bot clients can join with partial content (`--content` pointing at a partial copy).
- **Real-transport test.** The check-tier real-transport test gains one client with partial content (the level left out). It downloads, plays, and the replays are equal.
- **Records.**
  - **STATUS:** N29-N34 as in this plan's Decisions.
  - **docs/online.md:** what downloads, where the cache lives, the 50 MB prompt, how to clear the cache, and that skins are fetched through the host.
  - **CHANGELOG:** an "Unreleased — content download" entry.

- [ ] **Step 1: Write the failing tests:** `server_config_reads_upload_rate`, and the extended real-transport test.
- [ ] **Step 2: Run, expect FAIL.**
- [ ] **Step 3: Implement**, and write the records.
- [ ] **Step 4: Run `cargo test -p rl-server -p rl-cli`, `cargo build --release --target x86_64-pc-windows-gnu -p rl-server -p rl-game`, and `cargo xtask check`. Expect PASS.**
- [ ] **Step 5: Commit** `rl-server: serve content; plan 11's records`.
