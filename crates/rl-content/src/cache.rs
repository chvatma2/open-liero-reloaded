//! The content-addressed download cache (plan 11, ruling N30): packs a
//! match's host sent, each in `<cache>/packs/<content hash>/`, found by
//! content hash, with resumable partial downloads and least-recently-used
//! eviction. Everything that comes from the network is verified before it
//! becomes a pack: the archive's SHA-256, the archive's bounds
//! (`rl_pack::archive::decode`), the pack's content hash, its id and kind,
//! and `rl_pack::validate`.
//!
//! Layout: `index.ron` (entries and a use counter, written atomically; a
//! missing or unreadable one is rebuilt from `packs/*/pack.ron`),
//! `packs/<hash>/`, `<hash>.part` with `<hash>.part.ron` (a partial
//! download: its archive hash and size) and `tmp/` (an install in progress,
//! emptied when the cache opens). The LRU order is a stored use counter,
//! never the wall clock; `get`'s touches are saved with the next install,
//! eviction or clear, or when the cache closes.
//!
//! One directory belongs to one open cache (ruling PF11-3): `open` holds an
//! exclusive advisory lock on `<dir>/lock` while the `DownloadCache` lives,
//! and a second `open` fails with `InstallError::Locked`. Threads of one
//! process share one `DownloadCache` behind a mutex.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use rl_data::manifest::PackKind;
use rl_pack::archive::{self, ArchiveError};
use rl_pack::{Severity, content_hash, read_manifest, validate};
use rl_vfs::DirFs;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The cache's size above which the least recently used packs are evicted.
pub const CACHE_BYTES: u64 = 1 << 30;
/// The most bytes a skin's archive may have.
pub const MAX_SKIN_ARCHIVE: u64 = 64 * 1024;

const INDEX: &str = "index.ron";

/// Why a download was not installed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Archive(#[from] ArchiveError),
    #[error("the download's content differs from the host's ({0})")]
    Hash(String),
    #[error("the download is pack {found}, not {wanted}")]
    Id { wanted: String, found: String },
    #[error("the download is a {found:?}, not a {wanted:?}")]
    Kind { wanted: PackKind, found: PackKind },
    #[error("the downloaded pack is invalid: {0}")]
    Invalid(String),
    #[error("the download cache: {0}")]
    Io(String),
    #[error("the download cache is in use by another game")]
    Locked,
}

fn io(what: &str, e: impl std::fmt::Display) -> InstallError {
    InstallError::Io(format!("{what}: {e}"))
}

/// A partial download: the archive it is of (its SHA-256, hex), its full
/// size and the bytes held so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partial {
    pub archive: String,
    pub size: u64,
    pub have: u64,
}

#[derive(Serialize, Deserialize)]
struct PartialMeta {
    archive: String,
    size: u64,
    /// The use counter when the partial started: older ones are discarded
    /// first (`trim_partials`).
    #[serde(default)]
    started: u64,
}

/// What `evict` did: the packs it removed, and those it could not remove
/// whole (dropped from the index all the same).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Eviction {
    pub evicted: Vec<String>,
    pub stuck: Vec<String>,
}

/// A complete partial download being installed: `DownloadCache::begin_install`
/// makes it, `verify` checks it without the cache (so a worker thread need
/// not hold the cache's lock while it hashes and unpacks), and
/// `DownloadCache::finish_install` moves it into the cache.
#[derive(Debug)]
pub struct PendingInstall {
    hash: String,
    id: String,
    kind: PackKind,
    limit: u64,
    partial: Partial,
    part: PathBuf,
    tmp: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    id: String,
    kind: PackKind,
    bytes: u64,
    used: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    counter: u64,
    entries: BTreeMap<String, Entry>,
}

/// A content hash is 64 lowercase hex digits; anything else never names a
/// file in the cache.
fn valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The bytes of the regular files under `dir` (symlinks are not followed).
fn dir_bytes(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    rd.filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_bytes(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// The download cache. See the module documentation.
pub struct DownloadCache {
    dir: PathBuf,
    index: Index,
    /// The index has changes `save` has not written (`get`'s touches).
    dirty: bool,
    /// Installs begun, for unique `tmp/` names.
    installs: u64,
    /// Holds the directory's lock while the cache lives.
    _lock: fs::File,
}

impl Drop for DownloadCache {
    fn drop(&mut self) {
        if self.dirty {
            let _ = self.save(); // best effort: only the LRU order is at stake
        }
    }
}

impl DownloadCache {
    /// Opens the cache in `dir`, creating it, and locks it: `Locked` while
    /// another `DownloadCache` (another process) has it open. An install
    /// that was cut off (`tmp/`) is removed; a missing or unreadable index is
    /// rebuilt by scanning `packs/` (use counters 0), and entries whose
    /// directory is gone are dropped.
    pub fn open(dir: &Path) -> Result<DownloadCache, InstallError> {
        fs::create_dir_all(dir.join("packs")).map_err(|e| io("creating", e))?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("lock"))
            .map_err(|e| io("lock", e))?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(InstallError::Locked),
            Err(fs::TryLockError::Error(e)) => return Err(io("lock", e)),
        }
        let _ = fs::remove_dir_all(dir.join("tmp"));
        let parsed = fs::read_to_string(dir.join(INDEX)).ok().and_then(|s| ron::from_str::<Index>(&s).ok());
        let mut cache = DownloadCache {
            dir: dir.to_path_buf(),
            index: parsed.unwrap_or_default(),
            dirty: false,
            installs: 0,
            _lock: lock,
        };
        // The index only says what was recorded: a pack installed but not
        // recorded (a crash, a failed write) is adopted, a record without
        // its pack and anything else in `packs/` is dropped.
        cache.reconcile();
        cache.save()?;
        Ok(cache)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn pack_path(&self, hash: &str) -> PathBuf {
        self.dir.join("packs").join(hash)
    }

    fn part_path(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.part"))
    }

    fn meta_path(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.part.ron"))
    }

    /// Makes the index match `packs/`: entries without a pack directory are
    /// dropped, valid packs without an entry adopted (use counter 0), and
    /// everything else in `packs/` removed.
    fn reconcile(&mut self) {
        let packs = self.dir.join("packs");
        self.index.entries.retain(|h, _| valid_hash(h) && packs.join(h).is_dir());
        let Ok(rd) = fs::read_dir(self.dir.join("packs")) else { return };
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if self.index.entries.contains_key(&name) {
                continue;
            }
            let path = e.path();
            let manifest = if valid_hash(&name) { read_manifest(&DirFs::new(&path)).ok() } else { None };
            match manifest {
                Some(m) => {
                    let bytes = dir_bytes(&path);
                    self.index.entries.insert(name, Entry { id: m.id, kind: m.kind, bytes, used: 0 });
                }
                None => {
                    let _ = fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path));
                }
            }
        }
    }

    /// Writes the index atomically (a temporary file, then a rename).
    fn save(&mut self) -> Result<(), InstallError> {
        self.dirty = false;
        let text = ron::ser::to_string_pretty(&self.index, ron::ser::PrettyConfig::default())
            .map_err(|e| io("index", e))?;
        let tmp = self.dir.join(format!("{INDEX}.tmp"));
        fs::write(&tmp, text).map_err(|e| io("index", e))?;
        fs::rename(&tmp, self.dir.join(INDEX)).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            io("index", e)
        })
    }

    /// The directory of the pack with content hash `hash`, marking it
    /// recently used (saved later: see the module documentation).
    pub fn get(&mut self, hash: &str) -> Option<PathBuf> {
        let path = self.pack_path(hash);
        if !valid_hash(hash) || !self.index.entries.contains_key(hash) {
            return None;
        }
        if !path.is_dir() {
            self.index.entries.remove(hash);
            self.dirty = true; // `open` reconciles in any case
            return None;
        }
        self.index.counter += 1;
        let used = self.index.counter;
        if let Some(e) = self.index.entries.get_mut(hash) {
            e.used = used;
        }
        self.dirty = true;
        Some(path)
    }

    pub fn contains(&self, hash: &str) -> bool {
        valid_hash(hash) && self.index.entries.contains_key(hash)
    }

    fn read_meta(&self, hash: &str) -> Option<PartialMeta> {
        ron::from_str(&fs::read_to_string(self.meta_path(hash)).ok()?).ok()
    }

    /// The partial download of the pack with content hash `hash`, if any.
    pub fn partial(&self, hash: &str) -> Option<Partial> {
        if !valid_hash(hash) {
            return None;
        }
        let meta = self.read_meta(hash)?;
        let have = fs::metadata(self.part_path(hash)).map_or(0, |m| m.len());
        Some(Partial { archive: meta.archive, size: meta.size, have })
    }

    /// Appends `bytes` at `offset` to the partial download of `hash`, whose
    /// archive has SHA-256 `archive` and `size` bytes; returns the bytes held
    /// then. Only `offset == have` is accepted (or 0, which restarts a
    /// partial of another archive); any other offset is an error and the
    /// caller asks the host again from `have`.
    pub fn append(
        &mut self,
        hash: &str,
        archive: &str,
        size: u64,
        offset: u64,
        bytes: &[u8],
    ) -> Result<u64, InstallError> {
        if !valid_hash(hash) {
            return Err(io("download", "not a content hash"));
        }
        if size > archive::MAX_ARCHIVE {
            return Err(ArchiveError::TooLarge(archive::MAX_ARCHIVE).into());
        }
        let same = self.read_meta(hash).is_some_and(|m| m.archive == archive && m.size == size);
        let part = self.part_path(hash);
        let have = if same { fs::metadata(&part).map_or(0, |m| m.len()) } else { 0 };
        if offset != have {
            return Err(io("download", format!("write at {offset}, but {have} bytes are held")));
        }
        let end = have.saturating_add(bytes.len() as u64);
        if end > size {
            return Err(io("download", "more bytes than the archive has"));
        }
        if !same {
            self.index.counter += 1;
            self.dirty = true;
            let meta =
                ron::to_string(&PartialMeta { archive: archive.into(), size, started: self.index.counter })
                    .map_err(|e| io("meta", e))?;
            fs::write(&part, []).map_err(|e| io("partial", e))?;
            fs::write(self.meta_path(hash), meta).map_err(|e| io("meta", e))?;
        }
        let mut f = fs::OpenOptions::new().append(true).open(&part).map_err(|e| io("partial", e))?;
        f.write_all(bytes).map_err(|e| io("partial", e))?;
        Ok(end)
    }

    /// Forgets the partial download of `hash`.
    pub fn discard_partial(&mut self, hash: &str) {
        if valid_hash(hash) {
            let _ = fs::remove_file(self.part_path(hash));
            let _ = fs::remove_file(self.meta_path(hash));
        }
    }

    /// Verifies the complete partial download of `hash` and moves it into
    /// the cache: its archive's SHA-256 and size (`limit`, the archive's
    /// bytes) first, then the archive's bounds while it is unpacked into
    /// `tmp/`, then the content hash, the manifest's `id` and `kind`, and
    /// `rl_pack::validate`. A failure removes the unpacked files and the
    /// partial. A partial that is not yet complete stays (and is an error).
    /// It is `begin_install`, `PendingInstall::verify` and `finish_install`
    /// in one.
    pub fn install(
        &mut self,
        hash: &str,
        id: &str,
        kind: PackKind,
        limit: u64,
    ) -> Result<PathBuf, InstallError> {
        let pending = self.begin_install(hash, id, kind, limit)?;
        let verified = pending.verify();
        self.finish_install(pending, verified)
    }

    /// The first step of `install`: the partial of `hash` must be complete.
    pub fn begin_install(
        &mut self,
        hash: &str,
        id: &str,
        kind: PackKind,
        limit: u64,
    ) -> Result<PendingInstall, InstallError> {
        if !valid_hash(hash) {
            return Err(io("install", "not a content hash"));
        }
        let Some(p) = self.partial(hash) else {
            return Err(io("install", "no such download"));
        };
        if p.have < p.size {
            return Err(io("install", format!("incomplete: {} of {} bytes", p.have, p.size)));
        }
        self.installs += 1;
        Ok(PendingInstall {
            hash: hash.into(),
            id: id.into(),
            kind,
            limit,
            partial: p,
            part: self.part_path(hash),
            tmp: self.dir.join("tmp").join(format!("{hash}-{}", self.installs)),
        })
    }

    /// The last step of `install`: moves the verified pack into the cache,
    /// or cleans up after a failed verification. Either way the partial is
    /// gone.
    pub fn finish_install(
        &mut self,
        pending: PendingInstall,
        verified: Result<u64, InstallError>,
    ) -> Result<PathBuf, InstallError> {
        let PendingInstall { hash, id, kind, tmp, .. } = pending;
        let result = verified.and_then(|bytes| {
            let dest = self.pack_path(&hash);
            if !dest.is_dir() {
                fs::rename(&tmp, &dest).map_err(|e| io("installing", e))?;
            }
            Ok((dest, bytes))
        });
        // What is left of `tmp` (a failure, or a pack that was already here).
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir(self.dir.join("tmp"));
        self.discard_partial(&hash);
        let (dest, bytes) = result?;
        self.index.counter += 1;
        let used = self.index.counter;
        self.index.entries.insert(hash, Entry { id, kind, bytes, used });
        self.save()?;
        Ok(dest)
    }

    /// The bytes the partial downloads hold.
    pub fn partial_bytes(&self) -> u64 {
        self.partials().iter().map(|(_, _, have)| have).sum()
    }

    /// The partial downloads, oldest first: hash, start (use counter) and
    /// bytes held.
    fn partials(&self) -> Vec<(String, u64, u64)> {
        let Ok(rd) = fs::read_dir(&self.dir) else { return Vec::new() };
        let mut out: Vec<(String, u64, u64)> = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let hash = name.strip_suffix(".part.ron").filter(|h| valid_hash(h))?.to_string();
                let started = self.read_meta(&hash).map_or(0, |m| m.started);
                let have = fs::metadata(self.part_path(&hash)).map_or(0, |m| m.len());
                Some((hash, started, have))
            })
            .collect();
        out.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        out
    }

    /// Discards partial downloads not in `keep`, oldest first, while the
    /// packs and the partials together take more than `limit` bytes;
    /// returns the discarded hashes.
    pub fn trim_partials(&mut self, keep: &[&str], limit: u64) -> Vec<String> {
        let mut total = self.total_bytes().saturating_add(self.partial_bytes());
        let mut out = Vec::new();
        for (hash, _, have) in self.partials() {
            if total <= limit {
                break;
            }
            if keep.contains(&hash.as_str()) {
                continue;
            }
            self.discard_partial(&hash);
            total = total.saturating_sub(have);
            out.push(hash);
        }
        out
    }

    /// Removes least recently used packs, except those in `keep` (the
    /// content hashes the lobby lists), until at most `limit` bytes remain
    /// or only kept packs are left. A pack that cannot be removed whole is
    /// `stuck`: it leaves the index all the same, so it is never offered
    /// half-removed (`open` adopts it again if its manifest survived).
    pub fn evict(&mut self, keep: &[&str], limit: u64) -> Eviction {
        let mut out = Eviction::default();
        while self.total_bytes() > limit {
            let victim = self
                .index
                .entries
                .iter()
                .filter(|(h, _)| !keep.contains(&h.as_str()))
                .min_by_key(|(h, e)| (e.used, (*h).clone()))
                .map(|(h, _)| h.clone());
            let Some(h) = victim else { break };
            let _ = fs::remove_dir_all(self.pack_path(&h));
            self.index.entries.remove(&h);
            if self.pack_path(&h).exists() { out.stuck.push(h) } else { out.evicted.push(h) }
        }
        if out != Eviction::default() {
            let _ = self.save(); // best effort: `open` reconciles
        }
        out
    }

    /// The bytes the cached packs take.
    pub fn total_bytes(&self) -> u64 {
        self.index.entries.values().map(|e| e.bytes).sum()
    }

    /// Removes every cached pack and partial download except those in
    /// `keep`, including packs the index does not list.
    pub fn clear(&mut self, keep: &[&str]) -> Result<(), InstallError> {
        let mut failed = None;
        if let Ok(rd) = fs::read_dir(self.dir.join("packs")) {
            for e in rd.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                if keep.contains(&name.as_str()) {
                    continue;
                }
                let path = e.path();
                match fs::remove_dir_all(&path).or_else(|_| fs::remove_file(&path)) {
                    Ok(()) => {
                        self.index.entries.remove(&name);
                    }
                    Err(_) if !path.exists() => {
                        self.index.entries.remove(&name);
                    }
                    Err(e) => failed = Some(io("clearing", e)),
                }
            }
        }
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for e in rd.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let hash = name.split('.').next().unwrap_or_default();
                if name.contains(".part") && !keep.contains(&hash) {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        self.save()?;
        failed.map_or(Ok(()), Err)
    }
}

impl PendingInstall {
    /// Unpacks the partial into its `tmp/` directory and verifies it (see
    /// `DownloadCache::install`); returns the unpacked pack's size.
    pub fn verify(&self) -> Result<u64, InstallError> {
        let (p, limit, tmp, hash) = (&self.partial, self.limit, self.tmp.as_path(), self.hash.as_str());
        if p.have > p.size || p.have > limit {
            return Err(ArchiveError::TooLarge(limit).into());
        }
        let file = fs::File::open(&self.part).map_err(|e| io("partial", e))?;
        let mut sha = Sha256::new();
        let mut r = BufReader::new(&file);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = r.read(&mut buf).map_err(|e| io("partial", e))?;
            if n == 0 {
                break;
            }
            sha.update(&buf[..n]);
        }
        let got = hex::encode(sha.finalize());
        if got != p.archive {
            return Err(InstallError::Hash(format!("archive {got}, expected {}", p.archive)));
        }
        fs::create_dir_all(tmp).map_err(|e| io("tmp", e))?;
        let file = fs::File::open(&self.part).map_err(|e| io("partial", e))?;
        let mut sink = DirFs::new(tmp);
        archive::decode(&mut BufReader::new(file), &mut sink, limit)?;
        let src = DirFs::new(tmp);
        let content = content_hash(&src).map_err(|e| InstallError::Invalid(e.to_string()))?;
        if content != hash {
            return Err(InstallError::Hash(format!("content {content}, expected {hash}")));
        }
        let m = read_manifest(&src).map_err(|e| InstallError::Invalid(e.to_string()))?;
        if m.id != self.id {
            return Err(InstallError::Id { wanted: self.id.clone(), found: m.id });
        }
        if m.kind != self.kind {
            return Err(InstallError::Kind { wanted: self.kind, found: m.kind });
        }
        let errors: Vec<String> =
            validate(&src).into_iter().filter(|d| d.severity == Severity::Error).map(|d| d.message).collect();
        if !errors.is_empty() {
            return Err(InstallError::Invalid(errors.join("; ")));
        }
        Ok(dir_bytes(tmp))
    }
}
