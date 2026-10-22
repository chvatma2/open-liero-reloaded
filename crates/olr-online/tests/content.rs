//! Content downloads in memory (plan 11, N32): a `content::Server` and a
//! `content::Fetcher` wired directly on a virtual clock, with a simulated
//! outbox room of 1 MiB; the worker is a real thread, so waits on it are
//! bounded loops.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use olr_content::{Content, DownloadCache};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Level, LevelDef, Material, MaterialGrid};
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_online::content::{Done, Download, DownloadState, Fetcher, Job, PROMPT_BYTES, Server, Worker};
use olr_proto::{CONTENT_CHUNK, ContentMsg, Item, ItemKind};
use olr_vfs::{ContentSink, DirFs, MemFs};

const HOST: u32 = 0;
const CLIENT: u32 = 1;
const STEP_US: u64 = 10_000;
const ROOM: usize = 1 << 20;

#[test]
fn archive_limits_agree() {
    // Ruling PF11-1: olr-proto restates olr-pack's limits.
    assert_eq!(olr_proto::MAX_ARCHIVE_BYTES, olr_pack::archive::MAX_ARCHIVE);
}

fn manifest(id: &str) -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: id.into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    }
}

/// A valid level pack `id` in `root/<id>`, with noisy art (PNG does not
/// compress it), so its archive is several chunks.
fn generated_level(root: &Path, id: &str, seed: u32) -> PathBuf {
    let (w, h) = (160, 120);
    let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    let mut noise = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    let mut art = RgbaImage::new(w, h);
    for py in 0..h {
        for px in 0..w {
            let v = noise().to_le_bytes();
            art.set(px, py, Rgba::rgb(v[0], v[1], v[2]));
        }
    }
    let level = Level {
        def: LevelDef {
            name: id.into(),
            width: w,
            height: h,
            art_scale: 1,
            theme: "olx-theme-dirt".into(),
            spawns: vec![],
        },
        material: MaterialGrid::new(w, h, Material::Dirt),
        front: art.clone(),
        back: art,
    };
    let mut fs = MemFs::new();
    olr_pack::write_level(
        &mut fs,
        &olr_pack::LevelPack {
            manifest: manifest(id),
            level,
            thumb: None,
            look: None,
            glow: None,
            warnings: Vec::new(),
            clear: None,
            distant: Vec::new(),
            surface: None,
            distant_surface: Vec::new(),
        },
    )
    .unwrap();
    let dir = root.join(id);
    let mut sink = DirFs::new(&dir);
    for (p, bytes) in fs.files() {
        sink.write(p, bytes).unwrap();
    }
    dir
}

/// Dirt Level from `OLR_CONTENT_DIR`, else a generated level.
fn a_level(root: &Path) -> (String, PathBuf) {
    let real = std::env::var("OLR_CONTENT_DIR").ok().map(|d| Path::new(&d).join("olx-level-dirt-level"));
    match real.filter(|d| d.is_dir()) {
        Some(dir) => ("olx-level-dirt-level".into(), dir),
        None => {
            eprintln!("OLR_CONTENT_DIR not set or missing: a generated level instead of Dirt Level");
            ("test-level".into(), generated_level(root, "test-level", 1))
        }
    }
}

fn wait_for(worker: &Worker, what: &str) -> Vec<Done> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        let done = worker.take();
        if !done.is_empty() {
            return done;
        }
        assert!(Instant::now() < until, "the worker did not {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The host side: a server with its worker, offering archives of pack directories.
struct Host {
    _dir: tempfile::TempDir,
    worker: Worker,
    server: Server,
}

impl Host {
    fn new(rate: u64) -> Host {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(Mutex::new(DownloadCache::open(dir.path()).unwrap()));
        Host { worker: Worker::start(Some(cache)), server: Server::new(rate), _dir: dir }
    }

    /// Archives the pack in `dir` and offers it under `hash` (its own
    /// content hash unless given); returns the listed item.
    fn offer(&mut self, id: &str, dir: &Path, hash: Option<&str>) -> Item {
        let own = olr_pack::content_hash(&DirFs::new(dir)).unwrap();
        self.worker.submit(Job::Archive { hash: own.clone(), dir: dir.to_path_buf() });
        let done = wait_for(&self.worker, "archive");
        let [Done::Archived { hash: archived, archive: Ok(archive) }] = &done[..] else { panic!("{done:?}") };
        assert_eq!(archived, &own);
        let hash = hash.unwrap_or(&own).to_string();
        self.server.offer(&hash, archive.clone());
        Item { kind: ItemKind::Level, id: id.into(), size: archive.bytes.len() as u64, hash, required: true }
    }
}

/// The client side: its cache, worker, fetcher and an empty local content.
struct Client {
    dir: PathBuf,
    cache: Arc<Mutex<DownloadCache>>,
    worker: Worker,
    fetcher: Fetcher,
    local: Content,
    /// Each download's states, in the order they were seen.
    seen: Vec<(String, DownloadState)>,
}

impl Client {
    fn open(dir: &Path, empty: &Path) -> Client {
        let cache = Arc::new(Mutex::new(DownloadCache::open(dir).unwrap()));
        Client {
            dir: dir.to_path_buf(),
            worker: Worker::start(Some(cache.clone())),
            fetcher: Fetcher::new(cache.clone()),
            cache,
            local: Content::open_layers(&[empty]).unwrap(),
            seen: vec![],
        }
    }

    fn list(&mut self, items: &[Item]) -> Vec<Item> {
        let missing = self.fetcher.list(HOST, items, &self.local);
        self.note();
        missing
    }

    fn note(&mut self) {
        for d in self.fetcher.downloads() {
            if !self.seen.iter().rev().find(|(h, _)| *h == d.item.hash).is_some_and(|(_, s)| *s == d.state) {
                self.seen.push((d.item.hash.clone(), d.state.clone()));
            }
        }
    }

    fn states(&self, hash: &str) -> Vec<DownloadState> {
        self.seen.iter().filter(|(h, _)| h == hash).map(|(_, s)| s.clone()).collect()
    }

    fn download(&self, hash: &str) -> Download {
        self.fetcher.downloads().iter().find(|d| d.item.hash == hash).unwrap().clone()
    }
}

/// The wire between them, on a virtual clock: everything sent arrives at
/// once, the outbox always has `ROOM`. `deliver` decides whether a message
/// from the host reaches the client (a cut connection drops the rest).
struct Wire {
    now: u64,
    to_host: Vec<ContentMsg>,
}

impl Wire {
    fn new() -> Wire {
        Wire { now: 0, to_host: vec![] }
    }

    /// One step; returns whether anything moved.
    fn step(
        &mut self,
        host: &mut Host,
        client: &mut Client,
        deliver: &mut dyn FnMut(&ContentMsg) -> bool,
    ) -> bool {
        self.now += STEP_US;
        let mut out = vec![];
        client.fetcher.poll(self.now, &mut out);
        client.note();
        let mut moved = !out.is_empty();
        for (to, m) in out {
            assert_eq!(to, HOST);
            self.to_host.push(m.clone());
            host.server.receive(CLIENT, &m);
        }
        let mut out = vec![];
        host.server.poll(self.now, &|_| ROOM, &mut out);
        moved |= !out.is_empty();
        for (to, m) in out {
            assert_eq!(to, CLIENT);
            if deliver(&m) {
                client.fetcher.receive(HOST, &m, &client.worker);
                client.note();
            }
        }
        for d in client.worker.take() {
            moved = true;
            assert!(client.fetcher.finished(&d), "{d:?}");
            client.note();
        }
        client.note();
        moved
    }

    /// Steps until `done` holds (bounded in steps and in real time).
    fn until(
        &mut self,
        host: &mut Host,
        client: &mut Client,
        deliver: &mut dyn FnMut(&ContentMsg) -> bool,
        done: &dyn Fn(&Client) -> bool,
    ) {
        let until = Instant::now() + Duration::from_secs(30);
        while !done(client) {
            assert!(Instant::now() < until, "stuck: {:?}", client.fetcher.downloads());
            if !self.step(host, client, deliver) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

fn all(_: &ContentMsg) -> bool {
    true
}

fn state_is(hash: &str, state: DownloadState) -> impl Fn(&Client) -> bool {
    let hash = hash.to_string();
    move |c: &Client| c.download(&hash).state == state
}

fn wants(wire: &Wire) -> Vec<(String, u64)> {
    wire.to_host
        .iter()
        .filter_map(|m| match m {
            ContentMsg::Want { hash, offset } => Some((hash.clone(), *offset)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_level_downloads_installs_and_verifies() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("empty")).unwrap();
    let (id, dir) = a_level(root.path());
    let mut host = Host::new(olr_online::content::HOST_RATE);
    let item = host.offer(&id, &dir, None);
    let mut client = Client::open(&root.path().join("cache"), &root.path().join("empty"));
    assert_eq!(client.list(std::slice::from_ref(&item)), vec![item.clone()]);
    assert_eq!(client.fetcher.missing_required(), vec![id.clone()]);
    assert!(client.fetcher.installed().is_empty());
    let mut wire = Wire::new();
    wire.until(&mut host, &mut client, &mut all, &state_is(&item.hash, DownloadState::Done));
    assert_eq!(
        client.states(&item.hash),
        vec![DownloadState::Queued, DownloadState::Receiving, DownloadState::Verifying, DownloadState::Done]
    );
    assert_eq!(client.download(&item.hash).have, item.size);
    assert!(client.fetcher.missing_required().is_empty());
    let installed = client.fetcher.installed();
    assert_eq!(installed, vec![(id.clone(), client.dir.join("packs").join(&item.hash))]);
    assert_eq!(olr_pack::content_hash(&DirFs::new(&installed[0].1)).unwrap(), item.hash);
    assert_eq!(wants(&wire), vec![(item.hash.clone(), 0)]);
    // The same list again: it is in the cache, nothing is missing.
    assert!(client.list(std::slice::from_ref(&item)).is_empty());
    assert_eq!(client.fetcher.installed(), installed);
    // And the overlay loads it.
    let over = client.local.with_packs(&installed).unwrap();
    assert!(over.has(&id, &item.hash));
}

#[test]
fn a_download_resumes_after_a_reconnect() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("empty")).unwrap();
    let (id, dir) = a_level(root.path());
    let mut host = Host::new(olr_online::content::HOST_RATE);
    let item = host.offer(&id, &dir, None);
    assert!(item.size > 3 * CONTENT_CHUNK as u64, "the level is too small for this test");
    let cache_dir = root.path().join("cache");
    let mut client = Client::open(&cache_dir, &root.path().join("empty"));
    client.list(std::slice::from_ref(&item));
    let mut wire = Wire::new();
    // The connection is cut after 3 chunks.
    let mut chunks = 0;
    let mut cut = |m: &ContentMsg| {
        if matches!(m, ContentMsg::Chunk { .. }) {
            chunks += 1;
        }
        chunks <= 3
    };
    let three = 3 * CONTENT_CHUNK as u64;
    let hash = item.hash.clone();
    wire.until(&mut host, &mut client, &mut cut, &move |c: &Client| c.download(&hash).have == three);
    client.fetcher.source_left();
    host.server.peer_left(CLIENT);
    drop(client);
    // The game again: a new fetcher on the same cache directory.
    let mut client = Client::open(&cache_dir, &root.path().join("empty"));
    client.list(std::slice::from_ref(&item));
    let mut wire = Wire::new();
    wire.until(&mut host, &mut client, &mut all, &state_is(&item.hash, DownloadState::Done));
    assert_eq!(wire.to_host[0], ContentMsg::Want { hash: item.hash.clone(), offset: three });
    let installed = client.fetcher.installed();
    assert_eq!(olr_pack::content_hash(&DirFs::new(&installed[0].1)).unwrap(), item.hash);
}

#[test]
fn a_changed_list_cancels_the_transfer() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("empty")).unwrap();
    let a_dir = generated_level(root.path(), "level-a", 2);
    let b_dir = generated_level(root.path(), "level-b", 3);
    let mut host = Host::new(olr_online::content::HOST_RATE);
    let a = host.offer("level-a", &a_dir, None);
    let b = host.offer("level-b", &b_dir, None);
    let mut client = Client::open(&root.path().join("cache"), &root.path().join("empty"));
    client.list(&[a.clone(), b.clone()]);
    let mut wire = Wire::new();
    let hash = a.hash.clone();
    wire.until(&mut host, &mut client, &mut all, &move |c: &Client| c.download(&hash).have > 0);
    // The host changed the level: A is no longer listed.
    assert_eq!(client.list(std::slice::from_ref(&b)), vec![b.clone()]);
    assert_eq!(client.fetcher.downloads().len(), 1);
    let sent = wire.to_host.len();
    wire.until(&mut host, &mut client, &mut all, &state_is(&b.hash, DownloadState::Done));
    assert_eq!(
        wire.to_host[sent..sent + 2],
        [ContentMsg::Cancel { hash: a.hash.clone() }, ContentMsg::Want { hash: b.hash.clone(), offset: 0 }]
    );
    // A's partial stays for a later list.
    let partial = client.cache.lock().unwrap().partial(&a.hash).unwrap();
    assert!(partial.have > 0 && partial.have < a.size);
    assert_eq!(client.fetcher.installed().len(), 1);
}

#[test]
fn a_tampered_archive_fails_after_one_retry() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("empty")).unwrap();
    let wanted = generated_level(root.path(), "level-a", 4);
    let other = generated_level(root.path(), "level-a-other", 5);
    let hash = olr_pack::content_hash(&DirFs::new(&wanted)).unwrap();
    let mut host = Host::new(olr_online::content::HOST_RATE);
    // A valid archive, of another pack, under the wanted pack's hash.
    let item = host.offer("level-a", &other, Some(&hash));
    let mut client = Client::open(&root.path().join("cache"), &root.path().join("empty"));
    client.list(std::slice::from_ref(&item));
    let mut wire = Wire::new();
    let failed = |c: &Client| matches!(c.download(&hash).state, DownloadState::Failed(_));
    wire.until(&mut host, &mut client, &mut all, &failed);
    let DownloadState::Failed(why) = client.download(&hash).state else { unreachable!() };
    assert!(why.contains("content"), "{why}");
    assert_eq!(wants(&wire), vec![(hash.clone(), 0), (hash.clone(), 0)]);
    let verifying = client.states(&hash).iter().filter(|s| **s == DownloadState::Verifying).count();
    assert_eq!(verifying, 2);
    assert_eq!(client.fetcher.missing_required(), vec!["level-a".to_string()]);
    assert!(client.fetcher.installed().is_empty());
    let cache = client.cache.lock().unwrap();
    assert!(cache.partial(&hash).is_none() && !cache.contains(&hash));
}

#[test]
fn big_lists_wait_for_approval() {
    let root = tempfile::tempdir().unwrap();
    let cache = Arc::new(Mutex::new(DownloadCache::open(root.path()).unwrap()));
    let mut fetcher = Fetcher::new(cache);
    std::fs::create_dir(root.path().join("empty")).unwrap();
    let local = Content::open_layers(&[root.path().join("empty").as_path()]).unwrap();
    let item = |c: char, size: u64| Item {
        kind: ItemKind::Mod,
        id: format!("pack-{c}"),
        hash: c.to_string().repeat(64),
        size,
        required: true,
    };
    let half = PROMPT_BYTES / 2 + 1;
    let items = [item('a', half), item('b', half)];
    assert_eq!(fetcher.list(HOST, &items, &local).len(), 2);
    assert_eq!(fetcher.waiting_approval(), Some(2 * half));
    let mut out = vec![];
    for t in 0..100 {
        fetcher.poll(t * STEP_US, &mut out);
    }
    assert!(out.is_empty(), "{out:?}");
    fetcher.approve();
    assert_eq!(fetcher.waiting_approval(), None);
    fetcher.poll(100 * STEP_US, &mut out);
    assert_eq!(out, vec![(HOST, ContentMsg::Want { hash: items[0].hash.clone(), offset: 0 })]);
    // At most `PROMPT_BYTES` starts at once.
    let mut fetcher =
        Fetcher::new(Arc::new(Mutex::new(DownloadCache::open(&root.path().join("other")).unwrap())));
    fetcher.list(HOST, &[item('c', PROMPT_BYTES)], &local);
    assert_eq!(fetcher.waiting_approval(), None);
    let mut out = vec![];
    fetcher.poll(0, &mut out);
    assert_eq!(out.len(), 1);
}

#[test]
fn the_worker_hashes_archives_and_stops() {
    let root = tempfile::tempdir().unwrap();
    let dir = generated_level(root.path(), "level-w", 6);
    let hash = olr_pack::content_hash(&DirFs::new(&dir)).unwrap();
    let cache = Arc::new(Mutex::new(DownloadCache::open(&root.path().join("cache")).unwrap()));
    let worker = Worker::start(Some(cache.clone()));
    worker.submit(Job::Hash { id: "level-w".into(), dir: dir.clone() });
    // An archive for another hash (the pack changed since): refused.
    worker.submit(Job::Archive { hash: "0".repeat(64), dir: dir.clone() });
    worker.submit(Job::Install {
        hash: hash.clone(),
        id: "level-w".into(),
        kind: PackKind::Level,
        limits: olr_pack::archive::Limits { bytes: 1 << 20, ..olr_pack::archive::Limits::PACK },
    });
    let mut done = vec![];
    while done.len() < 3 {
        done.extend(wait_for(&worker, "finish"));
    }
    assert!(matches!(&done[0], Done::Hashed { id, hash: Ok(h) } if id == "level-w" && *h == hash));
    assert!(matches!(&done[1], Done::Archived { archive: Err(e), .. } if e.contains("changed")), "{done:?}");
    assert!(
        matches!(&done[2], Done::Installed { dir: Err(e), .. } if e.contains("no such download")),
        "{done:?}"
    );
    let started = Instant::now();
    drop(worker);
    assert!(started.elapsed() < Duration::from_secs(1));
    // The worker's handle on the cache is gone with it.
    assert_eq!(Arc::strong_count(&cache), 1);
}
