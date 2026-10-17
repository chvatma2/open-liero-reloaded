//! Fetching what a content list lacks (plan 11, N32): from one source peer,
//! one item at a time (required items first, skins after), resumable from
//! the download cache's partials, verified and installed by the worker.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use olr_content::{CACHE_BYTES, Content, DownloadCache, Keeper};
use olr_netcode::PeerId;
use olr_proto::{ContentMsg, Item, NO_HASH};

use super::{Done, Job, PROMPT_BYTES, SILENCE_US, Worker, archive_limit, pack_kind};

/// What `Fetcher::downloads` reports of one missing item.
#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    pub item: Item,
    /// Archive bytes held.
    pub have: u64,
    pub state: DownloadState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadState {
    Queued,
    Receiving,
    Verifying,
    Done,
    /// Why, in words.
    Failed(String),
    /// The source will not send it.
    Unavailable,
}

/// The item being fetched.
#[derive(Debug)]
struct Current {
    hash: String,
    /// The archive (SHA-256) of the source's accepted `Begin`: chunks are
    /// taken only after one, and only at the bytes held.
    archive: Option<String>,
    /// The install failed once and the item is fetched again from 0.
    retried: bool,
    /// The silent source was asked again.
    reasked: bool,
    /// When the bytes held last changed (or the item was asked for).
    since_us: u64,
    seen_have: u64,
}

/// Fetches what a content list lacks from one source peer.
pub struct Fetcher {
    cache: Arc<Mutex<DownloadCache>>,
    source: Option<PeerId>,
    /// Every hash the list names (kept by eviction).
    listed: Vec<String>,
    /// Keeps `listed` from every eviction of the shared cache, whoever
    /// evicts (review Minor 2).
    keeper: Keeper,
    /// The list's missing items, in download order.
    downloads: Vec<Download>,
    /// The list's packs in the cache: (id, directory).
    cached: Vec<(String, PathBuf)>,
    /// Hashes the player approved (or below `prompt_bytes` in all).
    approved: BTreeSet<String>,
    /// Missing bytes above which a list waits for `approve`
    /// (`PROMPT_BYTES`; lowered by the tests).
    prompt_bytes: u64,
    current: Option<Current>,
    /// Messages for the next `poll`.
    outbox: Vec<(PeerId, ContentMsg)>,
    now_us: u64,
}

impl Fetcher {
    /// A fetcher using `cache`, shared with the `Worker`.
    pub fn new(cache: Arc<Mutex<DownloadCache>>) -> Fetcher {
        Fetcher {
            keeper: Keeper::new(&cache),
            cache,
            source: None,
            listed: Vec::new(),
            downloads: Vec::new(),
            cached: Vec::new(),
            approved: BTreeSet::new(),
            prompt_bytes: PROMPT_BYTES,
            current: None,
            outbox: Vec::new(),
            now_us: 0,
        }
    }

    /// The cache, or `None` when a thread panicked holding it.
    fn cache(&self) -> Option<MutexGuard<'_, DownloadCache>> {
        self.cache.lock().ok()
    }

    /// A new list from `source`: returns the missing items (those neither
    /// in `local` nor in the cache; `NO_HASH` is never fetched). A transfer
    /// the list no longer wants is cancelled (its partial stays; one listed
    /// with another size starts afresh); a failed item is tried again; the
    /// missing items not approved before wait for `approve` when more than
    /// `PROMPT_BYTES` of them are still to download (their partials' bytes
    /// not counted, ruling PF11-6).
    pub fn list(&mut self, source: PeerId, items: &[Item], local: &Content) -> Vec<Item> {
        let old_source = self.source.replace(source);
        self.listed = items.iter().map(|i| i.hash.clone()).collect();
        self.keeper.keep(&self.listed);
        let mut missing = Vec::new();
        let mut cached = Vec::new();
        for it in items.iter().filter(|i| i.hash != NO_HASH) {
            if local.has(&it.id, &it.hash) {
                continue;
            }
            match self.cache().and_then(|mut c| c.get(&it.hash)) {
                Some(dir) => cached.push((it.id.clone(), dir)),
                None => missing.push(it.clone()),
            }
        }
        self.cached = cached;

        // The transfer goes on only for the same item from the same source.
        // The same hash listed as another item (another size) is fetched
        // afresh: the partial is of the other one.
        if let Some(cur) = &self.current {
            let item = self.downloads.iter().find(|d| d.item.hash == cur.hash).map(|d| &d.item);
            let listed = missing.iter().find(|i| i.hash == cur.hash);
            if old_source != Some(source) || listed.is_none() || listed != item {
                let hash = cur.hash.clone();
                if let Some(old) = old_source {
                    self.outbox.push((old, ContentMsg::Cancel { hash: hash.clone() }));
                }
                if listed.is_some()
                    && listed != item
                    && let Some(mut c) = self.cache()
                {
                    c.discard_partial(&hash);
                }
                self.current = None;
            }
        }
        let same_source = old_source == Some(source);
        let old = std::mem::take(&mut self.downloads);
        self.downloads = missing
            .iter()
            .map(|item| {
                let kept = old.iter().find(|d| same_source && d.item == *item);
                match kept {
                    Some(d) if self.current.as_ref().is_some_and(|c| c.hash == item.hash) => d.clone(),
                    // The source said it will not send it. A failed item is
                    // tried again, once a list.
                    Some(d) if d.state == DownloadState::Unavailable => d.clone(),
                    _ => Download { item: item.clone(), have: self.held(item), state: DownloadState::Queued },
                }
            })
            .collect();
        if self.to_download() <= self.prompt_bytes {
            self.approve();
        }
        missing
    }

    /// The bytes still to download of the items not approved yet (ruling
    /// PF11-6): each one's size less the bytes its partial holds. Items the
    /// player approved are not asked about again, and the sum does not
    /// change while they download.
    fn to_download(&self) -> u64 {
        self.downloads
            .iter()
            .filter(|d| !self.approved.contains(&d.item.hash))
            .map(|d| d.item.size.saturating_sub(d.have))
            .fold(0, u64::saturating_add)
    }

    /// The bytes of `item`'s partial download, if it is of the listed size.
    fn held(&self, item: &Item) -> u64 {
        self.cache()
            .and_then(|c| c.partial(&item.hash))
            .filter(|p| p.size == item.size)
            .map_or(0, |p| p.have.min(p.size))
    }

    /// The missing bytes above which a list waits for `approve`, instead of
    /// `PROMPT_BYTES` (the tests' prompt).
    pub fn set_prompt_bytes(&mut self, bytes: u64) {
        self.prompt_bytes = bytes;
    }

    /// Lets every listed missing item download (after the player's prompt).
    pub fn approve(&mut self) {
        self.approved.extend(self.downloads.iter().map(|d| d.item.hash.clone()));
    }

    /// The bytes still to download of the items waiting for `approve`
    /// (ruling PF11-6: less what their partials hold), while some wait.
    pub fn waiting_approval(&self) -> Option<u64> {
        let waiting = self.downloads.iter().any(|d| !self.approved.contains(&d.item.hash));
        waiting.then(|| self.to_download())
    }

    /// A message from `from`; only the source's `Begin`, `Chunk` and
    /// `Unavailable` matter. A complete download goes to `worker` to install.
    pub fn receive(&mut self, from: PeerId, msg: &ContentMsg, worker: &Worker) {
        if self.source != Some(from) {
            return;
        }
        match msg {
            ContentMsg::Begin { hash, archive, size, offset } => {
                self.begin(hash, archive, *size, *offset, worker)
            }
            ContentMsg::Chunk { hash, offset, bytes } => self.chunk(hash, *offset, bytes, worker),
            ContentMsg::Unavailable { hash } => {
                if let Some(i) = self.receiving(hash) {
                    self.current = None;
                    self.downloads[i].state = DownloadState::Unavailable;
                }
                for d in &mut self.downloads {
                    if d.item.hash == *hash && d.state == DownloadState::Queued {
                        d.state = DownloadState::Unavailable;
                    }
                }
            }
            _ => {}
        }
    }

    /// The download being received, with its current transfer.
    fn receiving(&mut self, hash: &str) -> Option<usize> {
        let cur = self.current.as_ref().filter(|c| c.hash == hash)?;
        self.downloads.iter().position(|d| d.item.hash == cur.hash && d.state == DownloadState::Receiving)
    }

    fn begin(&mut self, hash: &str, archive: &str, size: u64, offset: u64, worker: &Worker) {
        let Some(i) = self.receiving(hash) else { return };
        let item = self.downloads[i].item.clone();
        let limit = archive_limit(item.kind);
        if size != item.size || size > limit || offset > size {
            let why = format!(
                "the host sent a {size}-byte archive from byte {offset}, not the {}-byte one it listed",
                item.size
            );
            return self.violation(i, why);
        }
        let partial = self.cache().and_then(|c| c.partial(hash));
        let have = match partial {
            Some(p) if p.archive == archive && p.size == size => p.have,
            Some(_) => {
                // Another archive of the pack: its bytes are of no use.
                if let Some(mut c) = self.cache() {
                    c.discard_partial(hash);
                }
                0
            }
            None => 0,
        };
        self.downloads[i].have = have;
        let source = self.source;
        let Some(cur) = self.current.as_mut() else { return };
        if offset == have {
            cur.archive = Some(archive.to_string());
            if have == size {
                self.install(i, worker);
            }
        } else {
            // Not where this side stands: ask for it (from 0 when the
            // archive changed).
            cur.archive = None;
            if let Some(s) = source {
                self.outbox.push((s, ContentMsg::Want { hash: hash.to_string(), offset: have }));
            }
        }
    }

    fn chunk(&mut self, hash: &str, offset: u64, bytes: &[u8], worker: &Worker) {
        let Some(i) = self.receiving(hash) else { return };
        let Some(archive) = self.current.as_ref().and_then(|c| c.archive.clone()) else { return };
        let have = self.downloads[i].have;
        if offset != have {
            return; // from before the last `Begin`
        }
        let size = self.downloads[i].item.size;
        if offset.checked_add(bytes.len() as u64).is_none_or(|end| end > size) {
            return self.violation(i, format!("the host sent bytes beyond the archive's {size}"));
        }
        let written = match self.cache() {
            Some(mut c) => c.append(hash, &archive, size, offset, bytes).map_err(|e| e.to_string()),
            None => Err("the download cache failed".to_string()),
        };
        match written {
            Ok(have) => {
                self.downloads[i].have = have;
                if have == size {
                    self.install(i, worker);
                }
            }
            Err(e) => self.violation(i, e),
        }
    }

    fn install(&mut self, i: usize, worker: &Worker) {
        let d = &mut self.downloads[i];
        d.state = DownloadState::Verifying;
        let item = &d.item;
        worker.submit(Job::Install {
            hash: item.hash.clone(),
            id: item.id.clone(),
            kind: pack_kind(item.kind),
            limits: olr_content::archive_limits(pack_kind(item.kind)),
        });
    }

    /// Download `i` fails; the source is told to stop.
    fn violation(&mut self, i: usize, why: String) {
        let hash = self.downloads[i].item.hash.clone();
        self.fail(i, why);
        if let Some(s) = self.source {
            self.outbox.push((s, ContentMsg::Cancel { hash }));
        }
    }

    fn fail(&mut self, i: usize, why: String) {
        self.downloads[i].state = DownloadState::Failed(why);
        self.current = None;
    }

    /// A worker's result: an `Installed` for the item being verified
    /// completes it (or retries it once from 0); returns whether it was
    /// this fetcher's.
    pub fn finished(&mut self, done: &Done) -> bool {
        let Done::Installed { hash, dir } = done else { return false };
        let Some(cur) = self.current.as_mut().filter(|c| c.hash == *hash) else { return false };
        let Some(i) =
            self.downloads.iter().position(|d| d.item.hash == *hash && d.state == DownloadState::Verifying)
        else {
            return false;
        };
        match dir {
            Ok(dir) => {
                let d = &mut self.downloads[i];
                d.state = DownloadState::Done;
                d.have = d.item.size;
                self.cached.push((d.item.id.clone(), dir.clone()));
                self.current = None;
            }
            Err(_) if !cur.retried => {
                *cur = Current {
                    hash: hash.clone(),
                    archive: None,
                    retried: true,
                    reasked: false,
                    since_us: self.now_us,
                    seen_have: 0,
                };
                if let Some(mut c) = self.cache() {
                    c.discard_partial(hash);
                }
                self.downloads[i].have = 0;
                self.downloads[i].state = DownloadState::Receiving;
                if let Some(s) = self.source {
                    self.outbox.push((s, ContentMsg::Want { hash: hash.clone(), offset: 0 }));
                }
            }
            Err(e) => self.fail(i, e.clone()),
        }
        true
    }

    /// Messages to send now: asks for the next item, and again (once) when
    /// the source has been silent for `SILENCE_US`.
    pub fn poll(&mut self, now_us: u64, out: &mut Vec<(PeerId, ContentMsg)>) {
        self.now_us = now_us;
        out.append(&mut self.outbox);
        let Some(source) = self.source else { return };
        if let Some(cur) = self.current.as_mut() {
            let Some(i) = self.downloads.iter().position(|d| d.item.hash == cur.hash) else { return };
            let d = &self.downloads[i];
            if d.state != DownloadState::Receiving {
                return;
            }
            if d.have != cur.seen_have {
                cur.seen_have = d.have;
                cur.since_us = now_us;
            } else if now_us.saturating_sub(cur.since_us) >= SILENCE_US {
                if cur.reasked {
                    self.fail(i, "the host stopped sending".into());
                    out.push((source, ContentMsg::Cancel { hash: self.downloads[i].item.hash.clone() }));
                } else {
                    cur.reasked = true;
                    cur.since_us = now_us;
                    cur.archive = None;
                    out.push((source, ContentMsg::Want { hash: cur.hash.clone(), offset: d.have }));
                }
            }
            return;
        }
        self.start_next(source, out);
    }

    /// Asks for the first queued, approved item.
    fn start_next(&mut self, source: PeerId, out: &mut Vec<(PeerId, ContentMsg)>) {
        let next = self
            .downloads
            .iter()
            .position(|d| d.state == DownloadState::Queued && self.approved.contains(&d.item.hash));
        let Some(i) = next else { return };
        // Room for it: partials count toward the cache's budget too.
        let keep: Vec<&str> = self.listed.iter().map(String::as_str).collect();
        if let Some(mut c) = self.cache() {
            c.evict(&keep, CACHE_BYTES);
            c.trim_partials(&keep, CACHE_BYTES);
        }
        let item = self.downloads[i].item.clone();
        let have = self.held(&item);
        let d = &mut self.downloads[i];
        d.have = have;
        d.state = DownloadState::Receiving;
        self.current = Some(Current {
            hash: item.hash.clone(),
            archive: None,
            retried: false,
            reasked: false,
            since_us: self.now_us,
            seen_have: have,
        });
        out.push((source, ContentMsg::Want { hash: item.hash, offset: have }));
    }

    pub fn downloads(&self) -> &[Download] {
        &self.downloads
    }

    /// The ids of the list's required items that are not here yet.
    pub fn missing_required(&self) -> Vec<String> {
        self.downloads
            .iter()
            .filter(|d| d.item.required && d.state != DownloadState::Done)
            .map(|d| d.item.id.clone())
            .collect()
    }

    /// (id, directory) of the list's packs in the cache, for
    /// `Content::with_packs`.
    pub fn installed(&self) -> Vec<(String, PathBuf)> {
        self.cached.clone()
    }

    /// The source is gone: the transfer stops, its partial stays, and the
    /// next list resumes it.
    pub fn source_left(&mut self) {
        self.source = None;
        self.current = None;
        self.outbox.clear();
        for d in &mut self.downloads {
            if matches!(d.state, DownloadState::Receiving | DownloadState::Verifying) {
                d.state = DownloadState::Queued;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_proto::{CONTENT_CHUNK, ItemKind};

    const SRC: PeerId = 7;

    struct Setup {
        _dir: tempfile::TempDir,
        cache: Arc<Mutex<DownloadCache>>,
        worker: Worker,
        fetcher: Fetcher,
        local: Content,
    }

    fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("empty")).unwrap();
        let cache = Arc::new(Mutex::new(DownloadCache::open(&dir.path().join("cache")).unwrap()));
        Setup {
            local: Content::open_layers(&[dir.path().join("empty").as_path()]).unwrap(),
            worker: Worker::start(Some(cache.clone())),
            fetcher: Fetcher::new(cache.clone()),
            cache,
            _dir: dir,
        }
    }

    fn item(c: char, kind: ItemKind, size: u64, required: bool) -> Item {
        Item { kind, id: format!("pack-{c}"), hash: c.to_string().repeat(64), size, required }
    }

    fn sha(c: char) -> String {
        c.to_string().repeat(64)
    }

    impl Setup {
        fn poll(&mut self, now_us: u64) -> Vec<(PeerId, ContentMsg)> {
            let mut out = vec![];
            self.fetcher.poll(now_us, &mut out);
            out
        }

        fn send(&mut self, msg: ContentMsg) {
            self.fetcher.receive(SRC, &msg, &self.worker);
        }

        fn state(&self, c: char) -> DownloadState {
            self.fetcher.downloads().iter().find(|d| d.item.hash == sha(c)).unwrap().state.clone()
        }
    }

    fn want(c: char, offset: u64) -> (PeerId, ContentMsg) {
        (SRC, ContentMsg::Want { hash: sha(c), offset })
    }

    fn begin(c: char, archive: char, size: u64, offset: u64) -> ContentMsg {
        ContentMsg::Begin { hash: sha(c), archive: sha(archive), size, offset }
    }

    fn chunk(c: char, offset: u64, len: usize) -> ContentMsg {
        ContentMsg::Chunk { hash: sha(c), offset, bytes: vec![1; len] }
    }

    /// Review Minor 2: what any fetcher on a cache lists is kept by every
    /// eviction and trim of that cache (a client's, both `--net-sim` peers'), not only by the fetcher's own;
    /// a fetcher dropped keeps nothing.
    #[test]
    fn every_fetchers_list_is_kept() {
        let s = setup();
        let mut other = Fetcher::new(s.cache.clone());
        let mut first = Fetcher::new(s.cache.clone());
        first.list(SRC, &[item('a', ItemKind::Level, 10, true)], &s.local);
        other.list(SRC + 1, &[item('b', ItemKind::Mod, 10, true)], &s.local);
        {
            let mut c = s.cache.lock().unwrap();
            for h in ['a', 'b', 'c'] {
                c.append(&sha(h), &sha('f'), 10, 0, &[1; 5]).unwrap();
            }
            assert_eq!(c.trim_partials(&[], 0), [sha('c')], "only the unlisted partial goes");
        }
        drop(first);
        let mut c = s.cache.lock().unwrap();
        assert_eq!(c.trim_partials(&[], 0), [sha('a')], "a dropped fetcher keeps nothing");
        assert!(c.partial(&sha('b')).is_some());
    }

    #[test]
    fn items_are_fetched_in_list_order() {
        let mut s = setup();
        let mut unknown = item('d', ItemKind::Level, 10, true);
        unknown.hash = NO_HASH.into();
        let items = [item('b', ItemKind::Level, 10, true), unknown, item('c', ItemKind::Mod, 10, true)];
        let missing = s.fetcher.list(SRC, &items, &s.local);
        let order: Vec<&str> = missing.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(order, ["pack-b", "pack-c"]);
        assert_eq!(s.fetcher.missing_required(), ["pack-b", "pack-c"]);
        assert_eq!(s.poll(0), vec![want('b', 0)]);
        // Unavailable: that item is given up, the next one starts.
        s.send(ContentMsg::Unavailable { hash: sha('b') });
        assert_eq!(s.state('b'), DownloadState::Unavailable);
        assert_eq!(s.poll(10), vec![want('c', 0)]);
        // Only the source is heard.
        s.fetcher.receive(SRC + 1, &ContentMsg::Unavailable { hash: sha('c') }, &s.worker);
        assert_eq!(s.state('c'), DownloadState::Receiving);
    }

    #[test]
    fn a_begin_that_differs_from_the_list_fails_the_item() {
        for (size, offset) in [(11, 0), (u64::MAX, 0), (10, 11), (10, u64::MAX)] {
            let mut s = setup();
            s.fetcher.list(SRC, &[item('a', ItemKind::Level, 10, true)], &s.local);
            s.poll(0);
            s.send(begin('a', 'f', size, offset));
            assert!(matches!(s.state('a'), DownloadState::Failed(_)), "{size} {offset}");
            assert_eq!(s.poll(10), vec![(SRC, ContentMsg::Cancel { hash: sha('a') })]);
        }
    }

    #[test]
    fn hostile_chunks_never_overflow() {
        let mut s = setup();
        let size = 2 * CONTENT_CHUNK as u64;
        s.fetcher.list(SRC, &[item('a', ItemKind::Level, size, true)], &s.local);
        s.poll(0);
        // Chunks before a begin are not taken.
        s.send(chunk('a', 0, 10));
        assert_eq!(s.fetcher.downloads()[0].have, 0);
        s.send(begin('a', 'f', size, 0));
        s.send(chunk('a', 0, 10));
        // Not where it stands: ignored, whatever the offset.
        s.send(chunk('a', u64::MAX, 10));
        s.send(chunk('a', 5, 10));
        assert_eq!(s.fetcher.downloads()[0].have, 10);
        assert_eq!(s.state('a'), DownloadState::Receiving);
        // Past the archive's end: the item fails.
        s.send(chunk('a', 10, CONTENT_CHUNK));
        s.send(chunk('a', 10 + CONTENT_CHUNK as u64, CONTENT_CHUNK));
        assert!(matches!(s.state('a'), DownloadState::Failed(_)));
        assert_eq!(s.poll(10).last(), Some(&(SRC, ContentMsg::Cancel { hash: sha('a') })));
    }

    #[test]
    fn another_archive_restarts_only_from_zero() {
        let mut s = setup();
        let size = 100;
        s.cache.lock().unwrap().append(&sha('a'), &sha('e'), size, 0, &[1; 40]).unwrap();
        s.fetcher.list(SRC, &[item('a', ItemKind::Level, size, true)], &s.local);
        assert_eq!(s.fetcher.downloads()[0].have, 40);
        assert_eq!(s.poll(0), vec![want('a', 40)]);
        // The host's archive changed, and it sends from 40: the partial is
        // no use, ask from 0.
        s.send(begin('a', 'f', size, 40));
        assert!(s.cache.lock().unwrap().partial(&sha('a')).is_none());
        assert_eq!(s.poll(10), vec![want('a', 0)]);
        s.send(chunk('a', 40, 10));
        assert_eq!(s.fetcher.downloads()[0].have, 0);
        s.send(begin('a', 'f', size, 0));
        s.send(chunk('a', 0, 10));
        assert_eq!(s.fetcher.downloads()[0].have, 10);
        // Again another archive, from 0: restart there at once.
        s.send(begin('a', 'g', size, 0));
        s.send(chunk('a', 0, 20));
        let p = s.cache.lock().unwrap().partial(&sha('a')).unwrap();
        assert_eq!((p.archive, p.have), (sha('g'), 20));
        // The same archive from elsewhere: ask from what is held.
        s.send(begin('a', 'g', size, 5));
        assert_eq!(s.poll(20), vec![want('a', 20)]);
    }

    #[test]
    fn a_silent_source_is_asked_again_once() {
        let mut s = setup();
        let size = 100;
        s.fetcher.list(SRC, &[item('a', ItemKind::Level, size, true)], &s.local);
        assert_eq!(s.poll(1_000), vec![want('a', 0)]);
        s.send(begin('a', 'f', size, 0));
        s.send(chunk('a', 0, 10));
        assert!(s.poll(5_000_000).is_empty());
        // 10 s after the last chunk (seen at the poll above).
        assert!(s.poll(15_000_000 - 1).is_empty());
        assert_eq!(s.poll(15_000_000), vec![want('a', 10)]);
        assert!(s.poll(24_000_000).is_empty());
        assert_eq!(s.poll(25_000_000), vec![(SRC, ContentMsg::Cancel { hash: sha('a') })]);
        assert_eq!(s.state('a'), DownloadState::Failed("the host stopped sending".into()));
        assert!(s.poll(40_000_000).is_empty());
    }

    #[test]
    fn a_source_leaving_keeps_the_partial() {
        let mut s = setup();
        s.fetcher.list(SRC, &[item('a', ItemKind::Level, 100, true)], &s.local);
        s.poll(0);
        s.send(begin('a', 'f', 100, 0));
        s.send(chunk('a', 0, 30));
        s.fetcher.source_left();
        assert_eq!(s.state('a'), DownloadState::Queued);
        assert!(s.poll(10).is_empty());
        assert_eq!(s.fetcher.missing_required(), ["pack-a"]);
        // Another source lists it: resumed from 30.
        s.fetcher.list(SRC + 1, &[item('a', ItemKind::Level, 100, true)], &s.local);
        assert_eq!(s.poll(20), vec![(SRC + 1, ContentMsg::Want { hash: sha('a'), offset: 30 })]);
    }

    #[test]
    fn a_resent_list_with_another_size_restarts_the_item() {
        let mut s = setup();
        s.fetcher.list(SRC, &[item('a', ItemKind::Level, 100, true)], &s.local);
        s.poll(0);
        s.send(begin('a', 'f', 100, 0));
        s.send(chunk('a', 0, 30));
        // The same hash, another size: another item, fetched afresh.
        s.fetcher.list(SRC, &[item('a', ItemKind::Level, 120, true)], &s.local);
        assert!(s.cache.lock().unwrap().partial(&sha('a')).is_none());
        assert_eq!(s.poll(10), vec![(SRC, ContentMsg::Cancel { hash: sha('a') }), want('a', 0)]);
        assert_eq!(s.state('a'), DownloadState::Receiving);
        s.send(begin('a', 'f', 120, 0));
        s.send(chunk('a', 0, 50));
        assert_eq!(s.fetcher.downloads()[0].have, 50);
    }

    /// Ruling PF11-6: the prompt counts the bytes still to download, each
    /// item's size less what its partial holds.
    #[test]
    fn the_prompt_counts_only_the_bytes_still_to_download() {
        let mut s = setup();
        s.fetcher.set_prompt_bytes(100);
        let items = [item('a', ItemKind::Level, 120, true), item('b', ItemKind::Mod, 30, true)];
        // 150 bytes, 70 of them held: 80 to go, under the prompt.
        s.cache.lock().unwrap().append(&sha('a'), &sha('e'), 120, 0, &[1; 70]).unwrap();
        s.fetcher.list(SRC, &items, &s.local);
        assert_eq!(s.fetcher.waiting_approval(), None);
        assert_eq!(s.poll(0), vec![want('a', 70)]);
        // Over it: the prompt names what is still to come.
        let mut s = setup();
        s.fetcher.set_prompt_bytes(100);
        s.cache.lock().unwrap().append(&sha('a'), &sha('e'), 120, 0, &[1; 40]).unwrap();
        s.fetcher.list(SRC, &items, &s.local);
        assert_eq!(s.fetcher.waiting_approval(), Some(110));
        assert!(s.poll(0).is_empty());
        // A partial of another size is no use: it counts whole.
        let mut s = setup();
        s.fetcher.set_prompt_bytes(100);
        s.cache.lock().unwrap().append(&sha('a'), &sha('e'), 121, 0, &[1; 70]).unwrap();
        s.fetcher.list(SRC, &items, &s.local);
        assert_eq!(s.fetcher.waiting_approval(), Some(150));
        // Approved items are not asked about again: a list adding a small
        // one starts it at once, and one adding a big one asks for that
        // one alone, its number unchanged while the others download.
        s.fetcher.approve();
        let mut more = items.to_vec();
        more.push(item('c', ItemKind::Mod, 10, true));
        s.fetcher.list(SRC, &more, &s.local);
        assert_eq!(s.fetcher.waiting_approval(), None);
        more.push(item('d', ItemKind::Theme, 101, true));
        s.fetcher.list(SRC, &more, &s.local);
        assert_eq!(s.fetcher.waiting_approval(), Some(101));
        assert_eq!(s.poll(0), vec![want('a', 0)]);
        s.send(begin('a', 'f', 120, 0));
        s.send(chunk('a', 0, 50));
        assert_eq!((s.fetcher.downloads()[0].have, s.state('a')), (50, DownloadState::Receiving));
        assert_eq!(s.fetcher.waiting_approval(), Some(101));
    }

    #[test]
    fn a_new_list_retries_a_failed_item_once() {
        let mut s = setup();
        let items = [item('a', ItemKind::Level, 100, true)];
        s.fetcher.list(SRC, &items, &s.local);
        s.poll(0);
        s.send(begin('a', 'f', 99, 0));
        assert!(matches!(s.state('a'), DownloadState::Failed(_)));
        assert_eq!(s.poll(10), vec![(SRC, ContentMsg::Cancel { hash: sha('a') })]);
        for round in 0..2 {
            s.fetcher.list(SRC, &items, &s.local);
            assert_eq!(s.state('a'), DownloadState::Queued, "{round}");
            assert_eq!(s.poll(20), vec![want('a', 0)]);
            s.send(begin('a', 'f', 99, 0));
            assert!(matches!(s.state('a'), DownloadState::Failed(_)));
            // Once a list: no more until the next.
            assert_eq!(s.poll(30), vec![(SRC, ContentMsg::Cancel { hash: sha('a') })]);
            assert!(s.poll(40).is_empty());
        }
        // Unavailable stays so.
        s.fetcher.list(SRC, &items, &s.local);
        s.poll(50);
        s.send(ContentMsg::Unavailable { hash: sha('a') });
        s.fetcher.list(SRC, &items, &s.local);
        assert_eq!(s.state('a'), DownloadState::Unavailable);
    }
}
