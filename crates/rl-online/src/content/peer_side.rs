//! The online peer's content (plan 11, N30-N33), called from `peer.rs`'s
//! poll: what the host lists, serves and fetches from its members, what a
//! client fetches from its host and serves of its own skin, and the content
//! the matches load from (this peer's packs with the downloaded ones laid
//! over). Sans-IO: messages leave through `take_out`, time is `now_us`, and
//! every hash and archive is made on the worker thread, never here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rl_content::{Content, DownloadCache, MAX_SKIN_ARCHIVE};
use rl_data::manifest::PackKind;
use rl_lobby::{LobbyClient, LobbyHost};
use rl_net::endpoint::HOST;
use rl_netcode::PeerId;
use rl_proto::{ContentMsg, Item, ItemKind, Look, MAX_ITEMS, NO_HASH, Phase};

use super::{
    Done, Download, Fetcher, HOST_RATE, Job, MATCH_RATE, PackArchive, QUEUED, SKIN_RATE, Server, Worker,
};
use crate::events::OnlineEvent;

/// `OnlineEvent::Downloads` comes at most this often (4 a second).
pub const REPORT_EVERY_US: u64 = 250_000;

/// How an online peer gets and serves content.
pub struct PeerContent {
    /// The download cache, one per directory, shared by the peers of one
    /// process (ruling PF11-3). `Err(why)`: downloads are off, and the
    /// lobby says why when content is missing.
    pub cache: Result<Arc<Mutex<DownloadCache>>, String>,
    /// Bytes per second a host sends in all outside matches (`HOST_RATE`;
    /// `MATCH_RATE` while a match starts or runs).
    pub upload_rate: u64,
}

impl PeerContent {
    /// No downloads (`why` says so); the default upload rate.
    pub fn without_downloads(why: impl Into<String>) -> PeerContent {
        PeerContent { cache: Err(why.into()), upload_rate: HOST_RATE }
    }
}

/// The content list's kind of a pack (the core pack never travels).
fn item_kind(kind: PackKind) -> Option<ItemKind> {
    match kind {
        PackKind::Mod => Some(ItemKind::Mod),
        PackKind::Level => Some(ItemKind::Level),
        PackKind::Theme => Some(ItemKind::Theme),
        PackKind::Skin => Some(ItemKind::Skin),
        PackKind::Core => None,
    }
}

/// Whether an item may be fetched and laid over `base`: a required one
/// always; an optional one (a skin) only if no required item names its id
/// and `base` has no pack of another kind under it. A member or a host
/// cannot pass a skin off as a mod, level or theme.
fn may_take(base: &Content, required: &[&str], item: &Item) -> bool {
    item.required
        || (!required.contains(&item.id.as_str())
            && base.pack_kind(&item.id).is_none_or(|k| k == super::pack_kind(item.kind)))
}

/// This peer's own packs and the worker that hashes and archives them.
struct Local {
    base: Arc<Content>,
    worker: Worker,
    /// Pack ids being hashed, with the directory hashed.
    hashing: BTreeMap<String, PathBuf>,
    /// Pack ids whose hashing failed: not asked again.
    unhashable: BTreeSet<String>,
    /// Archives by content hash, made or failed.
    archives: BTreeMap<String, Result<Arc<PackArchive>, String>>,
    /// Content hashes being archived.
    archiving: BTreeSet<String>,
}

impl Local {
    /// Pack `id`'s content hash if known; otherwise the worker is asked
    /// (once) and it is `None` for now. Never hashes on this thread.
    fn hash_of(&mut self, id: &str) -> Option<String> {
        if let Some(h) = self.base.known_hash(id) {
            return Some(h);
        }
        if !self.unhashable.contains(id)
            && !self.hashing.contains_key(id)
            && let Some(dir) = self.base.pack_dir(id)
        {
            self.hashing.insert(id.to_string(), dir.to_path_buf());
            self.worker.submit(Job::Hash { id: id.to_string(), dir: dir.to_path_buf() });
        }
        None
    }

    /// Whether this peer's hash of pack `id` is settled: known, failed, or
    /// no such pack here (so `Content::has` will not hash it here).
    fn hash_settled(&mut self, id: &str) -> bool {
        self.hash_of(id).is_some() || self.base.pack_dir(id).is_none() || self.unhashable.contains(id)
    }

    /// The archive of the pack in `dir` whose content hash is `hash`: made,
    /// failed, or `None` while the worker makes it.
    fn archive(&mut self, hash: &str, dir: &Path) -> Option<Result<Arc<PackArchive>, String>> {
        if let Some(a) = self.archives.get(hash) {
            return Some(a.clone());
        }
        if self.archiving.insert(hash.to_string()) {
            self.worker.submit(Job::Archive { hash: hash.to_string(), dir: dir.to_path_buf() });
        }
        None
    }
}

/// A member's skin the host fetches from that member (N31).
struct SkinFetch {
    /// What the member offered (its one-item `List`).
    item: Item,
    fetcher: Option<Fetcher>,
    /// `item` was given to the fetcher.
    listed: bool,
}

struct HostContent {
    /// The list last sent (`None` before the first).
    list: Option<Vec<Item>>,
    /// The list's revision (the lobby's `set_content_revision`): 1 for the
    /// first, one more for each change.
    revision: u32,
    /// Members' skins this host lacks, by the member's peer.
    skins: BTreeMap<PeerId, SkinFetch>,
    /// Skins fetched from members: (id, directory) by content hash.
    fetched: BTreeMap<String, (String, PathBuf)>,
    /// Archive bytes sent to each peer, by content hash (the end of the
    /// last chunk): the members' `Member::download`.
    sent: BTreeMap<PeerId, BTreeMap<String, u64>>,
    /// The host player's look (none on a dedicated server), and the one
    /// last given to the lobby.
    look: Option<Look>,
    look_set: Option<Look>,
    /// Hashes whose archive failed, said once.
    said: BTreeSet<String>,
    /// Counts what changes the list besides the lobby (worker results,
    /// offers, fetched skins, peers leaving); with the lobby's revision,
    /// when the list was built last.
    changes: u64,
    built: Option<(u64, u64)>,
    /// Tests: pack id → the other pack whose archive is served for it.
    #[cfg(feature = "test-hooks")]
    tamper: BTreeMap<String, String>,
    upload_rate: u64,
}

impl HostContent {
    /// The pack whose archive is served for pack `id`: none but in tests
    /// (`serve_instead`).
    fn served_for(&self, id: &str) -> Option<&String> {
        #[cfg(feature = "test-hooks")]
        return self.tamper.get(id);
        #[cfg(not(feature = "test-hooks"))]
        {
            let _ = id;
            None
        }
    }
}

struct ClientContent {
    fetcher: Option<Fetcher>,
    /// The host's list (revision, items), waiting for this peer's hashes
    /// of its packs.
    pending: Option<(u32, Vec<Item>)>,
    /// The list processed last, and its revision.
    items: Vec<Item>,
    revision: u32,
    /// Without downloads: the required ids missing from the last list.
    lacking: Vec<String>,
    /// The missing ids last told the host (`None` before the first list).
    told: Option<Vec<String>>,
    /// A list was processed: tell the host (even if nothing changed).
    have_due: bool,
    /// The player's look, and the one last given to the lobby.
    look: Look,
    look_set: Option<Look>,
    /// This peer's skin offered to the host (its archive served), and the
    /// offer last sent.
    offer: Option<Item>,
    offer_sent: Option<Item>,
    /// The notice that downloads are off was shown.
    said_off: bool,
}

enum Role {
    Host(Box<HostContent>),
    Client(Box<ClientContent>),
}

/// The content side of an online peer.
pub(crate) struct ContentSide {
    local: Local,
    /// `local.base` with the downloaded packs laid over.
    overlay: Arc<Content>,
    /// The packs laid over: (id, directory).
    laid: Vec<(String, PathBuf)>,
    cache: Option<Arc<Mutex<DownloadCache>>>,
    /// Why downloads are off.
    off: Option<String>,
    server: Server,
    role: Role,
    out: Vec<(PeerId, ContentMsg)>,
    events: Vec<OnlineEvent>,
    /// The downloads last reported, and when.
    reported: Vec<Download>,
    reported_at: Option<u64>,
    /// The missing bytes last prompted for.
    prompted: Option<u64>,
    /// Downloaded packs refused to be laid over (said once).
    refused: BTreeSet<String>,
}

impl ContentSide {
    /// A host's (`host`, `look` its player's, none for a dedicated server)
    /// or a client's (`look` its player's) content over `base`.
    pub(crate) fn new(base: Content, cfg: PeerContent, host: bool, look: Option<Look>) -> ContentSide {
        let (cache, off) = match cfg.cache {
            Ok(c) => (Some(c), None),
            Err(why) => (None, Some(why)),
        };
        let base = Arc::new(base);
        let role = if host {
            Role::Host(Box::new(HostContent {
                list: None,
                revision: 1,
                skins: BTreeMap::new(),
                fetched: BTreeMap::new(),
                sent: BTreeMap::new(),
                look,
                look_set: None,
                said: BTreeSet::new(),
                changes: 0,
                built: None,
                #[cfg(feature = "test-hooks")]
                tamper: BTreeMap::new(),
                upload_rate: cfg.upload_rate,
            }))
        } else {
            let look = look.unwrap_or_else(|| Look {
                name: String::new(),
                colour: [0; 3],
                skin: rl_lobby::DEFAULT_SKIN.into(),
                skin_hash: NO_HASH.into(),
            });
            Role::Client(Box::new(ClientContent {
                fetcher: cache.clone().map(Fetcher::new),
                pending: None,
                items: Vec::new(),
                revision: 0,
                lacking: Vec::new(),
                told: None,
                have_due: false,
                look,
                look_set: None,
                offer: None,
                offer_sent: None,
                said_off: false,
            }))
        };
        let mut side = ContentSide {
            local: Local {
                base: Arc::clone(&base),
                worker: Worker::start(cache.clone()),
                hashing: BTreeMap::new(),
                unhashable: BTreeSet::new(),
                archives: BTreeMap::new(),
                archiving: BTreeSet::new(),
            },
            overlay: base,
            laid: Vec::new(),
            cache,
            off,
            server: Server::new(if host { cfg.upload_rate } else { SKIN_RATE }),
            role,
            out: Vec::new(),
            events: Vec::new(),
            reported: Vec::new(),
            reported_at: None,
            prompted: None,
            refused: BTreeSet::new(),
        };
        // The player's skin is hashed from the start (its `Hello` carries it
        // when the hash comes first).
        if let Some(skin) = side.my_look().map(|l| l.skin.clone()) {
            side.local.hash_of(&skin);
        }
        side
    }

    fn my_look(&self) -> Option<&Look> {
        match &self.role {
            Role::Host(h) => h.look.as_ref(),
            Role::Client(c) => Some(&c.look),
        }
    }

    /// The content matches and replays load from: this peer's packs with
    /// the downloaded ones laid over.
    pub(crate) fn content(&self) -> Arc<Content> {
        Arc::clone(&self.overlay)
    }

    /// The downloads: a client's from its host, a host's skins from its
    /// members.
    pub(crate) fn downloads(&self) -> Vec<Download> {
        match &self.role {
            Role::Client(c) => c.fetcher.as_ref().map(|f| f.downloads().to_vec()).unwrap_or_default(),
            Role::Host(h) => h
                .skins
                .values()
                .filter_map(|s| s.fetcher.as_ref())
                .flat_map(|f| f.downloads().iter().cloned())
                .collect(),
        }
    }

    /// The player lets the content over the prompt download.
    pub(crate) fn approve(&mut self) {
        if let Role::Client(c) = &mut self.role
            && let Some(f) = &mut c.fetcher
        {
            f.approve();
        }
    }

    /// The look the player chose, its skin's hash filled in when known
    /// (otherwise once the worker has it, by the poll).
    pub(crate) fn set_look(&mut self, look: Look) -> Look {
        let filled = match self.local.hash_of(&look.skin) {
            Some(hash) => Look { skin_hash: hash, ..look.clone() },
            None => look.clone(),
        };
        match &mut self.role {
            Role::Host(h) => {
                h.look = Some(look);
                h.look_set = Some(filled.clone());
            }
            Role::Client(c) => {
                c.look = look;
                c.look_set = Some(filled.clone());
            }
        }
        filled
    }

    /// Tests: the host serves pack `other`'s archive for pack `id`.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn serve_instead(&mut self, id: &str, other: &str) {
        if let Role::Host(h) = &mut self.role {
            h.tamper.insert(id.into(), other.into());
            h.changes += 1;
        }
    }

    /// Tests: lists over `bytes` wait for the player's approval.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn set_prompt_bytes(&mut self, bytes: u64) {
        if let Role::Client(c) = &mut self.role
            && let Some(f) = &mut c.fetcher
        {
            f.set_prompt_bytes(bytes);
        }
    }

    pub(crate) fn take_out(&mut self) -> Vec<(PeerId, ContentMsg)> {
        std::mem::take(&mut self.out)
    }

    pub(crate) fn take_events(&mut self) -> Vec<OnlineEvent> {
        std::mem::take(&mut self.events)
    }

    /// A content message from `from`. On a host `look` is the sending
    /// member's (messages of other peers are not passed in).
    pub(crate) fn receive(&mut self, from: PeerId, msg: ContentMsg, look: Option<&Look>) {
        match (&mut self.role, msg) {
            (role, msg @ (ContentMsg::Want { .. } | ContentMsg::Cancel { .. })) => {
                // A transfer asked for from `offset` has sent that much.
                if let (Role::Host(h), ContentMsg::Want { hash, offset }) = (role, &msg) {
                    h.sent.entry(from).or_default().insert(hash.clone(), *offset);
                }
                self.server.receive(from, &msg)
            }
            (Role::Client(c), ContentMsg::List { revision, items }) if from == HOST => {
                c.pending = Some((revision, items))
            }
            (Role::Client(c), msg) => {
                if let Some(f) = &mut c.fetcher {
                    f.receive(from, &msg, &self.local.worker);
                }
            }
            // A member offers its own skin (ruling R6-1): the one item must
            // be the skin its look names.
            (Role::Host(h), ContentMsg::List { items, .. }) => {
                let Some(look) = look else { return };
                let [item] = items.as_slice() else { return };
                if item.kind != ItemKind::Skin
                    || item.id != look.skin
                    || item.hash != look.skin_hash
                    || item.hash == NO_HASH
                    || !may_take(&self.local.base, &[], item)
                {
                    return;
                }
                let same = h.skins.get(&from).is_some_and(|s| s.item == *item);
                if !same {
                    h.changes += 1;
                    let fetcher = h.skins.remove(&from).and_then(|s| s.fetcher);
                    h.skins.insert(from, SkinFetch { item: item.clone(), fetcher, listed: false });
                }
            }
            (Role::Host(h), msg) => {
                if let Some(f) = h.skins.get_mut(&from).and_then(|s| s.fetcher.as_mut()) {
                    f.receive(from, &msg, &self.local.worker);
                }
            }
        }
    }

    /// A host's peer is gone: its transfers stop, its skin fetch too.
    pub(crate) fn peer_left(&mut self, peer: PeerId) {
        self.server.peer_left(peer);
        if let Role::Host(h) = &mut self.role {
            if let Some(mut s) = h.skins.remove(&peer)
                && let Some(f) = &mut s.fetcher
            {
                f.source_left();
            }
            h.sent.remove(&peer);
            h.changes += 1;
        }
    }

    /// A member was welcomed (its `Welcome` sent): the list follows.
    pub(crate) fn joined(&mut self, peer: PeerId) {
        if let Role::Host(h) = &self.role
            && let Some(list) = &h.list
        {
            self.out.push((peer, ContentMsg::List { revision: h.revision, items: list.clone() }));
        }
    }

    /// The worker's results: hashes remembered, archives kept, installs
    /// routed to their fetcher.
    fn results(&mut self) {
        let done = self.local.worker.take();
        if let Role::Host(h) = &mut self.role
            && !done.is_empty()
        {
            h.changes += 1;
        }
        for done in done {
            match &done {
                Done::Hashed { id, hash } => {
                    let dir = self.local.hashing.remove(id);
                    match (hash, dir) {
                        (Ok(hash), Some(dir)) => {
                            self.local.base.remember_hash(id, &dir, hash);
                            self.overlay.remember_hash(id, &dir, hash);
                        }
                        (Err(_), _) => {
                            self.local.unhashable.insert(id.clone());
                        }
                        // Not asked for by this side.
                        (Ok(_), None) => {}
                    }
                }
                Done::Archived { hash, archive } => {
                    self.local.archiving.remove(hash);
                    self.local.archives.insert(hash.clone(), archive.clone());
                }
                Done::Installed { .. } => match &mut self.role {
                    Role::Client(c) => {
                        if let Some(f) = &mut c.fetcher {
                            f.finished(&done);
                        }
                    }
                    Role::Host(h) => {
                        for f in h.skins.values_mut().filter_map(|s| s.fetcher.as_mut()) {
                            if f.finished(&done) {
                                break;
                            }
                        }
                    }
                },
            }
        }
    }

    /// Lays `packs` (their item and directory) over this peer's own: the
    /// content of the next match. One that may not be taken
    /// ([`may_take`] against `required`, the ids the list requires) is
    /// refused with a notice, once.
    fn lay_over(&mut self, packs: Vec<(Item, PathBuf)>, required: &[&str]) {
        let mut laid: Vec<(String, PathBuf)> = Vec::new();
        let mut hashes: Vec<(String, PathBuf, String)> = Vec::new();
        for (item, dir) in &packs {
            if !may_take(&self.local.base, required, item) {
                if self.refused.insert(item.hash.clone()) {
                    self.events.push(OnlineEvent::Notice(format!(
                        "the downloaded {} is not used: it would replace another pack",
                        item.id
                    )));
                }
                continue;
            }
            // One pack per id: the first listed.
            if !laid.iter().any(|(i, _)| *i == item.id) {
                laid.push((item.id.clone(), dir.clone()));
                hashes.push((item.id.clone(), dir.clone(), item.hash.clone()));
            }
        }
        if laid == self.laid {
            return;
        }
        match self.local.base.with_packs(&laid) {
            Ok(content) => {
                // Verified by the install: never hashed here.
                for (id, dir, hash) in &hashes {
                    content.remember_hash(id, dir, hash);
                }
                self.overlay = Arc::new(content);
            }
            Err(e) => {
                self.events.push(OnlineEvent::Notice(format!("the downloaded content cannot be used: {e:#}")))
            }
        }
        self.laid = laid;
    }

    /// `OnlineEvent::Downloads` when they changed, at most every
    /// [`REPORT_EVERY_US`].
    fn report(&mut self, now_us: u64) {
        let downloads = self.downloads();
        if downloads != self.reported
            && self.reported_at.is_none_or(|t| now_us.saturating_sub(t) >= REPORT_EVERY_US)
        {
            self.events.push(OnlineEvent::Downloads(downloads.clone()));
            self.reported = downloads;
            self.reported_at = Some(now_us);
        }
    }

    /// Serves what was asked for while the peer's outbox holds less than
    /// `QUEUED` (`buffered(peer)`: the reliable bytes it holds for that
    /// peer), so content never queues more than that behind the match.
    fn serve(&mut self, now_us: u64, buffered: &dyn Fn(PeerId) -> usize) {
        let mut out = Vec::new();
        let room = |peer: PeerId| QUEUED.saturating_sub(buffered(peer));
        self.server.poll(now_us, &room, &mut out);
        if let Role::Host(h) = &mut self.role {
            for (peer, msg) in &out {
                if let ContentMsg::Chunk { hash, offset, bytes } = msg {
                    let end = offset.saturating_add(bytes.len() as u64);
                    let sent = h.sent.entry(*peer).or_default().entry(hash.clone()).or_insert(0);
                    *sent = (*sent).max(end);
                }
            }
        }
        self.out.append(&mut out);
    }

    // ---- the host ----

    /// The host's part of a poll: its look's skin hash, its members' skins
    /// it lacks fetched from them, the list built and sent when it changed,
    /// what members asked for served, and their progress in the lobby.
    pub(crate) fn poll_host(
        &mut self,
        now_us: u64,
        lobby: &mut LobbyHost,
        buffered: &dyn Fn(PeerId) -> usize,
    ) {
        self.results();
        self.host_look(lobby);
        let required: Vec<String> = lobby.state().setup.packs.iter().map(|(id, _)| id.clone()).collect();
        let required: Vec<&str> = required.iter().map(String::as_str).collect();
        self.host_skins(now_us, &required);
        self.host_list(lobby);
        // Members are lacking until their `Have` answers this list (from
        // the first poll: before the first list too).
        if let Role::Host(h) = &self.role {
            lobby.set_content_revision(h.revision);
        }
        let fetched: Vec<(Item, PathBuf)> = match &self.role {
            Role::Host(h) => h
                .fetched
                .iter()
                .map(|(hash, (id, dir))| {
                    let item = Item {
                        kind: ItemKind::Skin,
                        id: id.clone(),
                        hash: hash.clone(),
                        size: 0,
                        required: false,
                    };
                    (item, dir.clone())
                })
                .collect(),
            Role::Client(_) => return,
        };
        self.lay_over(fetched, &required);
        let Role::Host(h) = &self.role else { return };
        let playing = matches!(lobby.state().phase, Phase::Starting | Phase::Playing { .. });
        self.server.set_rate(if playing { MATCH_RATE } else { h.upload_rate });
        self.serve(now_us, buffered);
        self.progress(lobby);
        self.report(now_us);
    }

    fn host_look(&mut self, lobby: &mut LobbyHost) {
        let Some(look) = self.my_look().cloned() else { return };
        let Some(hash) = self.local.hash_of(&look.skin) else { return };
        let filled = Look { skin_hash: hash, ..look };
        if let Role::Host(h) = &mut self.role
            && h.look_set.as_ref() != Some(&filled)
        {
            lobby.set_my_look(filled.clone());
            h.look_set = Some(filled);
        }
    }

    /// Fetches the members' skins this host lacks, one fetch per hash; an
    /// offer under an id the setup requires is dropped ([`may_take`]).
    fn host_skins(&mut self, now_us: u64, required: &[&str]) {
        let Role::Host(h) = &mut self.role else { return };
        let base = &self.local.base;
        h.skins.retain(|_, s| may_take(base, required, &s.item));
        let peers: Vec<PeerId> = h.skins.keys().copied().collect();
        for peer in peers {
            let Role::Host(h) = &mut self.role else { return };
            let Some(s) = h.skins.get(&peer) else { continue };
            let item = s.item.clone();
            if !self.local.hash_settled(&item.id) {
                continue;
            }
            let Role::Host(h) = &mut self.role else { return };
            let ours = self.local.base.known_hash(&item.id).as_deref() == Some(item.hash.as_str());
            let busy =
                h.skins.iter().any(|(p, o)| *p != peer && o.item.hash == item.hash && o.fetcher.is_some());
            let Some(s) = h.skins.get_mut(&peer) else { continue };
            if ours || h.fetched.contains_key(&item.hash) {
                continue;
            }
            if s.fetcher.is_none() && !busy {
                s.fetcher = self.cache.clone().map(Fetcher::new);
            }
            let Some(f) = &mut s.fetcher else { continue };
            if !s.listed {
                s.listed = true;
                f.list(peer, std::slice::from_ref(&item), &self.local.base);
            }
            f.poll(now_us, &mut self.out);
            for (id, dir) in f.installed() {
                if id == item.id && dir.ends_with(&item.hash) && !h.fetched.contains_key(&item.hash) {
                    h.fetched.insert(item.hash.clone(), (id, dir));
                    h.changes += 1;
                }
            }
        }
    }

    /// The content list (N31): the setup's packs (required), then every
    /// member's and bot's skin this host has (a bot's as this host has
    /// it), each once by hash; an item is listed once its archive is made.
    /// No list goes out until every required item's archive is made (one
    /// the worker could not make is listed with size 0: refused when
    /// asked for). A changed list goes to every member.
    fn host_list(&mut self, lobby: &LobbyHost) {
        // Built again only when the lobby or what the list is made of changed.
        if let Role::Host(h) = &mut self.role {
            let now = (lobby.revision(), h.changes);
            if h.built == Some(now) {
                return;
            }
            h.built = Some(now);
        }
        let state = lobby.state();
        let mut wanted: Vec<(ItemKind, String, String, bool)> = Vec::new();
        for (id, hash) in &state.setup.packs {
            if let Some(kind) = self.local.base.pack_kind(id).and_then(item_kind) {
                wanted.push((kind, id.clone(), hash.clone(), true));
            }
        }
        let members = state.members.iter().map(|m| (&m.look, false));
        let bots = state.setup.bots.iter().map(|b| (&b.look, true));
        for (look, bot) in members.chain(bots) {
            let ours = self.local.hash_of(&look.skin);
            let hash = match (bot, ours) {
                // A bot's skin as this host has it (ruling R6-2).
                (true, Some(ours)) if look.skin_hash == NO_HASH => ours,
                _ => look.skin_hash.clone(),
            };
            let required: Vec<&str> = state.setup.packs.iter().map(|(id, _)| id.as_str()).collect();
            let item = Item {
                kind: ItemKind::Skin,
                id: look.skin.clone(),
                hash: hash.clone(),
                size: 0,
                required: false,
            };
            if hash != NO_HASH
                && !wanted.iter().any(|w| w.2 == hash)
                && may_take(&self.local.base, &required, &item)
            {
                wanted.push((ItemKind::Skin, look.skin.clone(), hash, false));
            }
        }
        let Role::Host(h) = &mut self.role else { return };
        // Skins fetched from members stay only while the list names them.
        h.fetched.retain(|hash, _| wanted.iter().any(|w| !w.3 && w.2 == *hash));
        let mut items = Vec::new();
        let mut ready = true;
        let mut needed = BTreeSet::new();
        for (kind, id, hash, required) in wanted {
            // Where this host has it, and what it serves for it.
            let (src_id, src_hash) = match h.served_for(&id) {
                Some(other) => match state.setup.packs.iter().find(|(i, _)| i == other) {
                    Some((i, hh)) => (i.clone(), hh.clone()),
                    None => (id.clone(), hash.clone()),
                },
                None => (id.clone(), hash.clone()),
            };
            let dir = if self.local.base.known_hash(&src_id).as_deref() == Some(src_hash.as_str())
                || (required && self.local.base.pack_dir(&src_id).is_some())
            {
                self.local.base.pack_dir(&src_id).map(Path::to_path_buf)
            } else {
                h.fetched.get(&src_hash).map(|(_, d)| d.clone())
            };
            let Some(dir) = dir else { continue };
            needed.insert(src_hash.clone());
            match self.local.archive(&src_hash, &dir) {
                Some(Ok(a)) => {
                    if !self.server.offered(&hash) {
                        self.server.offer(&hash, Arc::clone(&a));
                    }
                    items.push(Item { kind, id, hash, size: a.bytes.len() as u64, required });
                }
                Some(Err(why)) => {
                    if h.said.insert(hash.clone()) {
                        self.events.push(OnlineEvent::Notice(format!("this host cannot send {id}: {why}")));
                    }
                    if required {
                        items.push(Item { kind, id, hash, size: 0, required });
                    }
                }
                None => ready &= !required,
            }
        }
        items.truncate(MAX_ITEMS);
        if !ready || h.list.as_ref() == Some(&items) {
            return;
        }
        // What the new list no longer names is no longer served.
        for old in h.list.iter().flatten() {
            if !items.iter().any(|i| i.hash == old.hash) {
                self.server.withdraw(&old.hash);
            }
        }
        self.local.archives.retain(|hash, _| needed.contains(hash));
        if h.list.is_some() {
            h.revision = h.revision.wrapping_add(1).max(1);
        }
        for m in &state.members {
            if let Some(peer) = lobby.peer_of(m.id) {
                self.out.push((peer, ContentMsg::List { revision: h.revision, items: items.clone() }));
            }
        }
        h.list = Some(items);
    }

    /// Each member's share of its missing required content sent so far,
    /// as `Member::download`.
    fn progress(&self, lobby: &mut LobbyHost) {
        let Role::Host(h) = &self.role else { return };
        let Some(list) = &h.list else { return };
        let members: Vec<(u8, Vec<String>)> = lobby
            .state()
            .members
            .iter()
            .filter(|m| !m.missing.is_empty())
            .map(|m| (m.id, m.missing.clone()))
            .collect();
        for (id, missing) in members {
            let Some(peer) = lobby.peer_of(id) else { continue };
            let sent = h.sent.get(&peer);
            let (mut total, mut done) = (0u64, 0u64);
            for item in list.iter().filter(|i| i.required && missing.contains(&i.id)) {
                total = total.saturating_add(item.size);
                let got = sent.and_then(|s| s.get(&item.hash)).copied().unwrap_or(0);
                done = done.saturating_add(got.min(item.size));
            }
            if let Some(percent) = done.saturating_mul(100).checked_div(total) {
                lobby.set_download(id, Some(percent.min(100) as u8));
            }
        }
    }

    // ---- the client ----

    /// A client's part of a poll: its look's skin hash, its skin offered
    /// to the host, the host's list processed (once this peer's hashes of
    /// its packs are in), the downloads, `Have` after every list and on
    /// every change, the prompt, and its skin served.
    pub(crate) fn poll_client(
        &mut self,
        now_us: u64,
        lobby: &mut LobbyClient,
        buffered: &dyn Fn(PeerId) -> usize,
    ) {
        self.results();
        self.client_look(lobby);
        self.process_list();
        let Role::Client(c) = &mut self.role else { return };
        let mut installed = Vec::new();
        if let Some(f) = &mut c.fetcher {
            f.poll(now_us, &mut self.out);
            for (id, dir) in f.installed() {
                if let Some(item) = c.items.iter().find(|i| i.id == id && dir.ends_with(&i.hash)) {
                    installed.push((item.clone(), dir));
                }
            }
        }
        let required: Vec<String> = c.items.iter().filter(|i| i.required).map(|i| i.id.clone()).collect();
        let required: Vec<&str> = required.iter().map(String::as_str).collect();
        self.lay_over(installed, &required);
        self.have(lobby);
        let Role::Client(c) = &mut self.role else { return };
        let waiting = c.fetcher.as_ref().and_then(Fetcher::waiting_approval);
        if waiting != self.prompted {
            if let Some(bytes) = waiting {
                self.events.push(OnlineEvent::DownloadPrompt { bytes });
            }
            self.prompted = waiting;
        }
        self.report(now_us);
        self.serve(now_us, buffered);
    }

    /// The look's skin hash once known (before `Hello` when it comes
    /// first, else a `SetLook`), and the skin's archive offered to the
    /// host with a one-item `List` (ruling R6-1) and served, only it.
    fn client_look(&mut self, lobby: &mut LobbyClient) {
        let Role::Client(c) = &self.role else { return };
        let look = c.look.clone();
        let Some(hash) = self.local.hash_of(&look.skin) else { return };
        let filled = Look { skin_hash: hash.clone(), ..look.clone() };
        let is_skin = self.local.base.pack_kind(&look.skin) == Some(PackKind::Skin);
        let dir = self.local.base.pack_dir(&look.skin).map(Path::to_path_buf);
        let offer = match dir.filter(|_| is_skin).and_then(|d| self.local.archive(&hash, &d)) {
            Some(Ok(a)) if a.bytes.len() as u64 <= MAX_SKIN_ARCHIVE => {
                if !self.server.offered(&hash) {
                    self.server.offer(&hash, a.clone());
                }
                Some(Item {
                    kind: ItemKind::Skin,
                    id: look.skin.clone(),
                    hash,
                    size: a.bytes.len() as u64,
                    required: false,
                })
            }
            _ => None,
        };
        let Role::Client(c) = &mut self.role else { return };
        if c.look_set.as_ref() != Some(&filled) {
            lobby.set_look(filled.clone());
            c.look_set = Some(filled);
        }
        if let Some(old) = &c.offer
            && offer.as_ref().is_none_or(|o| o.hash != old.hash)
        {
            self.server.withdraw(&old.hash);
        }
        c.offer = offer;
        if lobby.joined() && c.offer.is_some() && c.offer_sent != c.offer {
            c.offer_sent = c.offer.clone();
            self.out.push((HOST, ContentMsg::List { revision: 0, items: c.offer.iter().cloned().collect() }));
        }
    }

    /// The host's list, once this peer's hashes of the listed packs it has
    /// are in (made on the worker: a list naming a pack not hashed yet
    /// waits for it).
    fn process_list(&mut self) {
        let Role::Client(c) = &self.role else { return };
        let Some((revision, items)) = c.pending.clone() else { return };
        let mut settled = true;
        for it in items.iter().filter(|i| i.hash != NO_HASH) {
            settled &= self.local.hash_settled(&it.id);
        }
        if !settled {
            return;
        }
        let base = &self.local.base;
        // Optional items under a required id, or a local pack's id of
        // another kind, are not taken (a host cannot pass a skin off as a
        // level).
        let required: Vec<&str> = items.iter().filter(|i| i.required).map(|i| i.id.as_str()).collect();
        let items: Vec<Item> = items.iter().filter(|i| may_take(base, &required, i)).cloned().collect();
        let Role::Client(c) = &mut self.role else { return };
        c.pending = None;
        match &mut c.fetcher {
            Some(f) => {
                f.list(HOST, &items, base);
            }
            None => {
                c.lacking = items
                    .iter()
                    .filter(|i| i.required && !base.has(&i.id, &i.hash))
                    .map(|i| i.id.clone())
                    .collect();
                if !c.lacking.is_empty() && !c.said_off {
                    c.said_off = true;
                    let why = self.off.as_deref().unwrap_or("no download cache");
                    self.events.push(OnlineEvent::Notice(format!(
                        "content downloads are off ({why}): missing {}",
                        c.lacking.join(", ")
                    )));
                }
            }
        }
        c.items = items;
        c.revision = revision;
        c.have_due = true;
    }

    /// `Have` after every list processed, and whenever what is missing
    /// changes (sent once joined).
    fn have(&mut self, lobby: &mut LobbyClient) {
        let Role::Client(c) = &mut self.role else { return };
        let missing = match &c.fetcher {
            Some(f) => f.missing_required(),
            None => c.lacking.clone(),
        };
        let changed = c.told.as_ref().is_some_and(|t| *t != missing);
        if (c.have_due || changed) && lobby.joined() {
            lobby.have(c.revision, missing.clone());
            c.told = Some(missing);
            c.have_due = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_lobby::Identity;

    fn content_dir() -> Option<PathBuf> {
        let dir = std::env::var("RL_CONTENT_DIR").ok().map(PathBuf::from).filter(|d| d.is_dir());
        if dir.is_none() {
            eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        }
        dir
    }

    fn look() -> Look {
        Look {
            name: "Anna".into(),
            colour: [1, 2, 3],
            skin: "olx-skin-default".into(),
            skin_hash: NO_HASH.into(),
        }
    }

    /// Carried from Task 4: a list naming this peer's own packs waits for
    /// their hashes from the worker; nothing is hashed on the caller's
    /// thread (the memo stays empty until the worker's result is in).
    #[test]
    fn a_list_waits_for_the_workers_hashes() {
        let Some(dir) = content_dir() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let cache = Arc::new(Mutex::new(DownloadCache::open(tmp.path()).unwrap()));
        let cfg = PeerContent { cache: Ok(cache), upload_rate: HOST_RATE };
        let mut side = ContentSide::new(Content::open(&dir).unwrap(), cfg, false, Some(look()));
        let hash = Content::open(&dir).unwrap().pack_hash("olx-classic", PackKind::Mod).unwrap();
        let item = Item {
            kind: ItemKind::Mod,
            id: "olx-classic".into(),
            hash: hash.clone(),
            size: 10,
            required: true,
        };
        side.receive(HOST, ContentMsg::List { revision: 1, items: vec![item] }, None);
        let id = Identity::from_secret([3; 32]).unwrap();
        let mut lobby = LobbyClient::new(id, look(), None, None);
        let room = |_| 0;
        side.poll_client(0, &mut lobby, &room);
        let Role::Client(c) = &side.role else { panic!("a client") };
        assert!(c.pending.is_some(), "the list waits");
        assert_eq!(side.local.base.known_hash("olx-classic"), None, "not hashed on this thread");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            side.poll_client(0, &mut lobby, &room);
            let Role::Client(c) = &side.role else { panic!("a client") };
            if c.pending.is_none() {
                assert!(c.have_due, "a `Have` is due once joined");
                break;
            }
            assert!(std::time::Instant::now() < until, "the worker hashes the mod");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(side.local.base.known_hash("olx-classic"), Some(hash), "the worker's hash");
        assert!(side.downloads().is_empty(), "nothing to fetch");
    }

    /// A host's list whose optional items reuse a required item's id, or
    /// name a pack this client has under another kind: they are refused,
    /// and the client keeps its own packs.
    #[test]
    fn optional_items_never_shadow_required_or_other_kinds() {
        let Some(dir) = content_dir() else { return };
        let tmp = tempfile::tempdir().unwrap();
        let cache = Arc::new(Mutex::new(DownloadCache::open(tmp.path()).unwrap()));
        let cfg = PeerContent { cache: Ok(cache), upload_rate: HOST_RATE };
        let mut side = ContentSide::new(Content::open(&dir).unwrap(), cfg, false, Some(look()));
        let full = Content::open(&dir).unwrap();
        let level = "olx-level-dirt-level";
        let level_hash = full.pack_hash(level, PackKind::Level).unwrap();
        let skin = |id: &str, c: char| Item {
            kind: ItemKind::Skin,
            id: id.into(),
            hash: c.to_string().repeat(64),
            size: 1000,
            required: false,
        };
        let items = vec![
            Item { kind: ItemKind::Level, id: level.into(), hash: level_hash, size: 1000, required: true },
            skin(level, 'a'),
            skin("olx-classic", 'b'),
            skin("not-here", 'c'),
        ];
        side.receive(HOST, ContentMsg::List { revision: 1, items }, None);
        let id = Identity::from_secret([5; 32]).unwrap();
        let mut lobby = LobbyClient::new(id, look(), None, None);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while matches!(&side.role, Role::Client(c) if c.pending.is_some()) {
            assert!(std::time::Instant::now() < until, "the list is processed");
            side.poll_client(0, &mut lobby, &|_| 0);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let ids: Vec<String> = side.downloads().into_iter().map(|d| d.item.id).collect();
        assert_eq!(ids, ["not-here"], "only the harmless skin is fetched");
        assert_eq!(side.content().pack_kind(level), Some(PackKind::Level));
        assert_eq!(side.content().pack_kind("olx-classic"), Some(PackKind::Mod));
    }

    /// Carried from Task 4: chunks go to a peer only while its outbox holds
    /// less than `QUEUED`. An outbox that never drains ends up holding
    /// about `QUEUED` of content, never more, however long it waits.
    #[test]
    fn an_outbox_holds_at_most_queued_content() {
        let Some(dir) = content_dir() else { return };
        let content = Content::open(&dir).unwrap();
        let (m, l) = ("olx-classic", "olx-level-dirt-level");
        let setup = rl_proto::MatchSetupWire {
            mod_id: m.into(),
            level_id: l.into(),
            packs: crate::matches::setup_packs(&content, m, l).unwrap(),
            rules: crate::matches::rules_wire(&rl_modes::DeathmatchSettings::default()),
            bonuses: false,
            loading_time: 100,
            restrictions: String::new(),
            bots: vec![],
        };
        let cfg = rl_lobby::HostConfig {
            name: "Office".into(),
            password: None,
            max_players: 4,
            max_spectators: 2,
            admins: vec![],
        };
        let me = Identity::from_secret([4; 32]).unwrap();
        let mut lobby = LobbyHost::new(cfg, me, Some(look()), setup).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let cache = Arc::new(Mutex::new(DownloadCache::open(tmp.path()).unwrap()));
        let cfg = PeerContent { cache: Ok(cache), upload_rate: HOST_RATE };
        let mut side = ContentSide::new(content, cfg, true, Some(look()));
        let held = std::cell::Cell::new(0usize);
        let buffered = |_| held.get();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let list = loop {
            side.poll_host(0, &mut lobby, &buffered);
            if let Role::Host(h) = &side.role
                && let Some(list) = &h.list
            {
                break list.clone();
            }
            assert!(std::time::Instant::now() < until, "the archives are made");
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        // Everything listed: more than the bound.
        let total: u64 = list.iter().map(|i| i.size).sum();
        assert!(
            total > (QUEUED + rl_proto::CONTENT_CHUNK) as u64,
            "the list is larger than the bound: {total}"
        );
        for item in &list {
            side.receive(7, ContentMsg::Want { hash: item.hash.clone(), offset: 0 }, None);
        }
        for step in 1..=300u64 {
            side.poll_host(step * 10_000, &mut lobby, &buffered);
            for (peer, msg) in side.take_out() {
                assert_eq!(peer, 7);
                held.set(held.get() + rl_proto::encode_content(&msg).len());
            }
            assert!(held.get() <= QUEUED, "{} bytes held at {step}", held.get());
        }
        assert!(held.get() > QUEUED - 2 * rl_proto::CONTENT_CHUNK, "it filled the outbox: {}", held.get());
        // A transfer asked for again from 0 (a failed verification): its
        // progress (`Member::download`) starts again too.
        let first = &list[0].hash;
        let sent = |side: &ContentSide| match &side.role {
            Role::Host(h) => h.sent.get(&7).and_then(|s| s.get(first)).copied(),
            Role::Client(_) => None,
        };
        assert!(sent(&side).is_some_and(|b| b > 0));
        side.receive(7, ContentMsg::Want { hash: first.clone(), offset: 0 }, None);
        assert_eq!(sent(&side), Some(0));
    }
}
