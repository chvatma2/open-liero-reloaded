//! Real transport on one machine (networking spec §9.4): headless bot
//! clients over real connections (`rl-cli net-client --bot`), and whole
//! matches of `rl-server` with such clients on localhost, through
//! `rl-net`'s impairment layer, judged by the verdict of §9.1.
//!
//! [`run_bot_client`] is one client: it joins, readies (or watches), plays
//! its matches with a bot thinking on its own predicted world, and keeps
//! each match's confirmed checksums and replay. [`run_transport`] runs
//! `rl_server::run_with` in a thread and the clients in threads, and
//! [`TransportRun::verdict`] judges what they kept.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow};
use rl_bots::{Bot, BotConfig, Difficulty};
use rl_content::{Content, DownloadCache};
use rl_lobby::Identity;
use rl_net::endpoint::EndpointConfig;
use rl_netcode::TickInfo;
use rl_netcode::netsim::Profile;
use rl_online::content::HOST_RATE;
use rl_online::peer::RECORD_AFTER_END;
use rl_online::replay::MatchRecord;
use rl_online::{OnlineEvent, OnlinePeer, PeerContent, PeerRole};
use rl_proto::{Look, MatchStartWire, Phase, SeatWire};
use rl_replay::Replay;
use rl_server::config::{Config, Entry, Visibility};
use rl_sim::{Keys, TickEvents, World};

/// A client polls this often (the server polls every `rl_server::POLL_EVERY`).
pub const CLIENT_POLL: Duration = Duration::from_millis(1);
/// A lobby request (ready, spectate) is repeated at most this often while
/// the lobby does not show it yet.
const REQUEST_EVERY: Duration = Duration::from_millis(500);

/// One headless bot client.
#[derive(Clone, Debug)]
pub struct ClientSpec {
    /// The server's (TCP signalling) address.
    pub addr: SocketAddr,
    pub name: String,
    pub difficulty: Difficulty,
    /// Matches to play (or watch) before leaving.
    pub matches: u32,
    /// Every datagram this client sends passes this network (spec §9.4).
    pub impair: Option<Profile>,
    pub impair_seed: u64,
    /// Watch instead of playing.
    pub spectate: bool,
    /// Gives up (and reports itself dropped) at this time.
    pub deadline: Instant,
    /// Its download cache's directory (plan 11): what its content lacks
    /// of a match it downloads from the server into it. `None`: no
    /// downloads (content lacking a match's packs sits it out).
    pub cache: Option<PathBuf>,
}

/// One match as a client confirmed it.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerMatch {
    pub start: MatchStartWire,
    /// The worm it played (`None`: it watched).
    pub slot: Option<u8>,
    /// Its confirmed sync checksums: the whole match's when it was there
    /// from tick 0, else from the state it joined with (the last seen
    /// before its session ended).
    pub checksums: Vec<(u32, u64)>,
    /// Its record of the match (`None` for a spectator that joined
    /// mid-match: its inputs start at its state, rl-online makes no replay).
    pub record: Option<MatchRecord>,
    /// Its replay's bytes (with `record`).
    pub replay: Option<Vec<u8>>,
}

/// What a client did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientRun {
    pub name: String,
    /// Its finished matches, in order.
    pub matches: Vec<PeerMatch>,
    /// Why it left early: refused, kicked, disconnected, timed out or
    /// stopped. `None`: it finished its matches.
    pub dropped: Option<String>,
    /// Desync bundles it made (spec §3.5).
    pub desync_bundles: u32,
}

/// What a client tells its caller as it goes.
pub enum Progress<'a> {
    /// A match starts: the slot it plays (`None`: it watches).
    Started(&'a MatchStartWire, Option<u8>),
    Finished(&'a PeerMatch),
}

/// The bot of a client's slot, thinking on the world each newest tick
/// left (a re-simulation of an older tick changes nothing: the next tick's
/// keys come from the world before it).
struct Brain {
    cfg: BotConfig,
    /// The slot and seed of the running match.
    playing: Option<(u8, u32)>,
    bot: Option<Bot>,
    /// The newest tick simulated.
    newest: Option<u32>,
    keys: Keys,
}

impl Brain {
    fn start(&mut self, slot: Option<u8>, seed: u32) {
        self.playing = slot.map(|s| (s, seed));
        self.stop();
    }

    fn stop(&mut self) {
        self.bot = None;
        self.newest = None;
        self.keys = Keys(0);
    }

    fn on_tick(&mut self, world: &World, events: &TickEvents, info: TickInfo) {
        let Some((slot, seed)) = self.playing else { return };
        if self.newest.is_some_and(|n| info.tick < n) {
            return;
        }
        self.newest = Some(info.tick);
        let cfg = &self.cfg;
        let bot = self.bot.get_or_insert_with(|| Bot::new(usize::from(slot), *cfg, seed, world));
        self.keys = bot.think(world, events);
    }
}

/// A client's match being played or finished.
struct Current {
    start: MatchStartWire,
    slot: Option<u8>,
    /// Its session's confirmed checksums as last seen (rl-online hides the
    /// session once the match ended).
    checksums: Vec<(u32, u64)>,
    /// It joined mid-match: its inputs start at a state (no record, no replay).
    joined_late: bool,
    ended: bool,
    /// Finished, its replay awaited.
    finished: Option<PeerMatch>,
}

/// A fresh identity (a random valid secret).
pub fn new_identity() -> anyhow::Result<Identity> {
    for _ in 0..64 {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>()?) {
            return Ok(id);
        }
    }
    Err(anyhow!("no valid identity from 64 random secrets"))
}

/// One bot client (spec §9.4, `rl-cli net-client --bot`): joins `spec.addr`,
/// readies whenever the lobby gathers (or watches), plays `spec.matches`
/// matches with a bot on its slot, keeps each match's confirmed checksums
/// and replay, then leaves. Returns early, `dropped`, when the server
/// refuses, kicks or loses it, at `spec.deadline` or when `stop` is set.
/// An error only when the client cannot start.
pub fn run_bot_client(
    content: Content,
    spec: &ClientSpec,
    stop: &AtomicBool,
    progress: &mut dyn FnMut(Progress<'_>),
) -> anyhow::Result<ClientRun> {
    let endpoint = EndpointConfig {
        port: 0,
        impair: spec.impair,
        impair_seed: spec.impair_seed,
        ..EndpointConfig::default()
    };
    let look = Look {
        name: spec.name.clone(),
        colour: [120, 200, 80],
        skin: rl_lobby::DEFAULT_SKIN.into(),
        skin_hash: rl_proto::NO_HASH.into(),
    };
    let role = PeerRole::Client { addr: spec.addr, password: None, expected_host: None };
    let downloads = match &spec.cache {
        Some(dir) => {
            let cache =
                DownloadCache::open(dir).with_context(|| format!("the download cache {}", dir.display()))?;
            PeerContent { cache: Ok(Arc::new(Mutex::new(cache))), upload_rate: HOST_RATE }
        }
        None => PeerContent::without_downloads("this client has no download cache"),
    };
    let mut peer = OnlinePeer::start(role, endpoint, new_identity()?, look, content, None, downloads)
        .with_context(|| format!("joining {}", spec.addr))?;
    let brain = RefCell::new(Brain {
        cfg: BotConfig { difficulty: spec.difficulty, ..BotConfig::default() },
        playing: None,
        bot: None,
        newest: None,
        keys: Keys(0),
    });
    let mut run = ClientRun { name: spec.name.clone(), ..ClientRun::default() };
    let mut current: Option<Current> = None;
    let mut replays: Vec<Vec<u8>> = Vec::new();
    let mut asked: Option<Instant> = None;
    loop {
        let now = Instant::now();
        if stop.load(Ordering::Relaxed) {
            run.dropped = Some("stopped".into());
        } else if now >= spec.deadline {
            run.dropped = Some(format!("timed out ({})", waiting_for(&run, current.as_ref())));
        }
        if run.dropped.is_some() {
            break;
        }
        let events = peer
            .poll(now, &mut || brain.borrow().keys, &mut |w, e, info| brain.borrow_mut().on_tick(w, e, info));
        for e in events {
            match e {
                OnlineEvent::MatchStarting { start, my_slot } => {
                    brain.borrow_mut().start(my_slot, start.seed);
                    progress(Progress::Started(&start, my_slot));
                    current = Some(Current {
                        start,
                        slot: my_slot,
                        checksums: Vec::new(),
                        joined_late: false,
                        ended: false,
                        finished: None,
                    });
                }
                OnlineEvent::MatchEnded(_) => {
                    brain.borrow_mut().stop();
                    if let Some(c) = &mut current {
                        c.ended = true;
                    }
                }
                OnlineEvent::ReplayReady(bytes) => replays.push(bytes),
                OnlineEvent::DesyncBundle(_) => run.desync_bundles += 1,
                OnlineEvent::Refused(why) => run.dropped = Some(format!("refused: {why}")),
                OnlineEvent::Kicked { banned } => {
                    run.dropped = Some(if banned { "banned" } else { "kicked" }.into());
                }
                OnlineEvent::Disconnected(why) => run.dropped = Some(format!("disconnected: {why}")),
                _ => {}
            }
        }
        if run.dropped.is_none() && peer.is_closed() {
            run.dropped = Some("the connection closed".into());
        }
        if let Some(c) = &mut current {
            if let Some(s) = peer.match_session() {
                c.joined_late |= s.input_log().origin() > 0;
                if s.checksums().len() != c.checksums.len() {
                    c.checksums = s.checksums().to_vec();
                }
            }
            // Once ended, a peer there from tick 0 keeps its record when it
            // confirmed the last ticks (or gave up); a late joiner has its
            // checksums up to the end.
            if c.ended && c.finished.is_none() {
                let record = peer.last_record().filter(|r| r.start == c.start).cloned();
                if record.is_some() || c.joined_late {
                    let checksums =
                        record.as_ref().map_or_else(|| c.checksums.clone(), |r| r.checksums.clone());
                    c.finished = Some(PeerMatch {
                        start: c.start.clone(),
                        slot: c.slot,
                        checksums,
                        record,
                        replay: None,
                    });
                }
            }
            if let Some(f) = &mut c.finished {
                if f.record.is_some() && f.replay.is_none() && !replays.is_empty() {
                    f.replay = Some(replays.remove(0));
                }
                if f.record.is_none() || f.replay.is_some() {
                    let f = current.take().and_then(|c| c.finished).expect("a finished match");
                    progress(Progress::Finished(&f));
                    run.matches.push(f);
                }
            }
        }
        if run.dropped.is_some() || run.matches.len() >= spec.matches as usize {
            break;
        }
        let more = run.matches.len() + usize::from(current.is_some()) < spec.matches as usize;
        if want_request(&mut peer, spec.spectate, more)
            && asked.is_none_or(|t| now.duration_since(t) >= REQUEST_EVERY)
        {
            asked = Some(now);
            if spec.spectate {
                peer.lobby().set_spectate(true);
            } else {
                peer.lobby().set_ready(true);
            }
        }
        std::thread::sleep(CLIENT_POLL);
    }
    peer.leave();
    Ok(run)
}

/// Whether the lobby does not show this client's wish yet: a player not
/// ready while it gathers and the client wants `more` matches, or a
/// spectator seated.
fn want_request(peer: &mut OnlinePeer, spectate: bool, more: bool) -> bool {
    let lobby = peer.lobby();
    let Some(me) = lobby.me() else { return false };
    let Some(state) = lobby.state() else { return false };
    let Some(m) = state.members.iter().find(|m| m.id == me) else { return false };
    match (spectate, m.seat) {
        (true, SeatWire::Player { .. }) => true,
        (false, SeatWire::Player { .. }) => more && state.phase == Phase::Gathering && !m.ready,
        _ => false,
    }
}

fn waiting_for(run: &ClientRun, current: Option<&Current>) -> String {
    let n = run.matches.len() + 1;
    match current {
        None => format!("match {n} did not start"),
        Some(c) if !c.ended => format!("match {n} did not end"),
        Some(c) if c.finished.is_none() => format!("match {n} ended but left no record"),
        Some(_) => format!("match {n}'s replay was not made"),
    }
}

/// A server and its bot clients over localhost (spec §9.4).
#[derive(Clone, Debug)]
pub struct TransportSpec {
    pub mod_id: String,
    pub level_id: String,
    /// The match's time limit, in minutes.
    pub minutes: f32,
    /// Bot clients seated as players (the match starts when all are ready).
    pub players: u8,
    /// The server fills the seats up to this many worms with its own bots
    /// (a match needs two worms).
    pub fill_bots: u8,
    /// Spectators joining this long after the first player's match started.
    pub spectators: Vec<Duration>,
    /// Matches every client plays.
    pub matches: u32,
    /// The clients' network (every datagram they send), by name: `lan`,
    /// `good`, `typical`, `bad` or `awful`; `None`: localhost as it is.
    pub profile: Option<String>,
    /// The clients' bots; `None`: client `i` Easy, Medium, Hard, Xtreme in turn.
    pub difficulty: Option<Difficulty>,
    /// Client `i`'s impairment seed is `net_seed + i`.
    pub net_seed: u64,
    /// The server's data directory (its identity, desync bundles).
    pub data_dir: PathBuf,
    /// Everything gives up this long after the start ([`TransportSpec::new`]:
    /// the match's length and 40 s; raise it for more matches).
    pub timeout: Duration,
    /// Clients (by name) that may be dropped without failing the verdict.
    pub may_drop: Vec<String>,
    /// Each of the server's records holds at least this many checksums
    /// (never fewer than one: an empty record proves nothing).
    pub min_checksums: usize,
    /// Content directories of their own for some clients (by index:
    /// players, then spectators), such as a copy lacking the level: they
    /// download the rest from the server (plan 11). The others open the
    /// run's content directory.
    pub client_content: BTreeMap<usize, PathBuf>,
}

impl TransportSpec {
    /// Classic on Dirt Level, `players` bot clients, one match of
    /// `minutes`, the clients through `profile`.
    pub fn new(players: u8, minutes: f32, profile: Option<&str>, data_dir: &Path) -> TransportSpec {
        TransportSpec {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-dirt-level".into(),
            minutes,
            players,
            fill_bots: 0,
            spectators: Vec::new(),
            matches: 1,
            profile: profile.map(String::from),
            difficulty: None,
            net_seed: 1,
            data_dir: data_dir.to_path_buf(),
            timeout: Duration::from_secs_f32(minutes.max(0.0) * 60.0) + Duration::from_secs(40),
            may_drop: Vec::new(),
            min_checksums: 1,
            client_content: BTreeMap::new(),
        }
    }

    /// Client `i`'s download cache (players, then spectators; 0-based), in
    /// the data directory.
    pub fn client_cache(&self, i: usize) -> PathBuf {
        self.data_dir.join(format!("client-{}-downloads", i + 1))
    }

    /// The name of player client `i` (0-based).
    pub fn player_name(i: usize) -> String {
        format!("Client {}", i + 1)
    }

    /// The name of spectator `i` (0-based).
    pub fn spectator_name(i: usize) -> String {
        format!("Watcher {}", i + 1)
    }

    /// The server's config: one rotation entry, time-limited, `fill_bots`,
    /// starting when every player is ready.
    pub fn server_config(&self) -> Config {
        Config {
            name: "Transport".into(),
            visibility: Visibility::DirectOnly,
            password: None,
            port: 0,
            max_players: self.players.max(self.fill_bots).max(1),
            max_spectators: u8::try_from(self.spectators.len()).unwrap_or(u8::MAX).max(1),
            admins: Vec::new(),
            fill_bots: self.fill_bots,
            rotation: vec![Entry {
                mod_id: self.mod_id.clone(),
                level: self.level_id.clone(),
                time_limit: self.minutes,
                kill_limit: -1,
                lives: -1,
                bonuses: true,
            }],
            start_when_ready: true,
            min_players: self.players.max(1),
            bans_file: None,
            upload_kib_s: rl_server::config::DEFAULT_UPLOAD_KIB_S,
        }
    }
}

/// What a transport run gave.
#[derive(Clone, Debug)]
pub struct TransportRun {
    /// The server's finished matches, in order.
    pub server: Vec<MatchRecord>,
    /// The players, then the spectators.
    pub clients: Vec<ClientRun>,
    /// The server's log.
    pub server_log: String,
    pub elapsed: Duration,
}

/// A `Write` into a shared buffer (the server's log, read by the harness).
#[derive(Clone, Default)]
pub struct SharedLog(Arc<Mutex<Vec<u8>>>);

impl Write for SharedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().map_err(|_| std::io::Error::other("poisoned"))?.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedLog {
    /// What was written so far.
    pub fn text(&self) -> String {
        self.0.lock().map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
    }
}

/// Runs `rl_server::run_with` in a thread on port 0 and `spec`'s bot
/// clients in threads (each opening `content_dir`, or its own in
/// `spec.client_content`, with its download cache `spec.client_cache(i)`),
/// until every client
/// finished (or dropped) and the server finished as many matches (all
/// bounded by `spec.timeout`); then stops the server.
pub fn run_transport(content_dir: &Path, spec: &TransportSpec) -> anyhow::Result<TransportRun> {
    let begun = Instant::now();
    let deadline = begun + spec.timeout;
    let impair = match &spec.profile {
        Some(p) => Some(Profile::by_name(p).ok_or_else(|| anyhow!("no network profile {p:?}"))?),
        None => None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let log = SharedLog::default();
    let records: Arc<Mutex<Vec<MatchRecord>>> = Arc::default();
    let (port_tx, port_rx) = mpsc::channel();
    let server = {
        let (config, data_dir) = (spec.server_config(), spec.data_dir.clone());
        let content = Content::open(content_dir)?;
        let (stop, mut log, records) = (Arc::clone(&stop), log.clone(), Arc::clone(&records));
        std::thread::Builder::new().name("rl-server".into()).spawn(move || {
            let mut last = None;
            rl_server::run_with(config, content, &data_dir, &stop, &mut log, &mut |s| {
                let _ = port_tx.send(s.local_port());
                if let Some(r) = s.last_record() {
                    // A record is made once, at its match's end, and replaced
                    // only by the next match's: a cheap key that names the
                    // match (its random seed and start world, its length) tells
                    // a new one, instead of comparing whole records every poll
                    // (2 ms). Two matches alike in all four would be one
                    // match's start played twice, which the server never does.
                    let key = (r.start.seed, r.start.start_checksum, r.rows.len(), r.checksums.len());
                    if last != Some(key) {
                        last = Some(key);
                        records.lock().expect("records").push(r.clone());
                    }
                }
            })
        })?
    };
    let Ok(port) = port_rx.recv_timeout(Duration::from_secs(20)) else {
        stop.store(true, Ordering::Relaxed);
        let why = match server.join() {
            Ok(Err(e)) => format!("{e:#}"),
            Ok(Ok(())) => "it stopped".into(),
            Err(_) => "it panicked".into(),
        };
        return Err(anyhow!("the server did not start: {why}\n{}", log.text()));
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let first_start: Arc<Mutex<Option<Instant>>> = Arc::default();
    let mut threads = Vec::new();
    let levels = [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard, Difficulty::Xtreme];
    let clients = (0..usize::from(spec.players))
        .map(|i| (TransportSpec::player_name(i), None))
        .chain(spec.spectators.iter().enumerate().map(|(i, &d)| (TransportSpec::spectator_name(i), Some(d))));
    for (i, (name, join_after)) in clients.enumerate() {
        let client = ClientSpec {
            addr,
            name: name.clone(),
            difficulty: spec.difficulty.unwrap_or(levels[i % levels.len()]),
            matches: spec.matches,
            impair,
            impair_seed: spec.net_seed.wrapping_add(i as u64),
            spectate: join_after.is_some(),
            deadline,
            cache: Some(spec.client_cache(i)),
        };
        let content_dir = spec.client_content.get(&i).map_or(content_dir, PathBuf::as_path).to_path_buf();
        let (stop, first_start) = (Arc::clone(&stop), Arc::clone(&first_start));
        let thread = std::thread::Builder::new().name(name.clone()).spawn(move || {
            if let Some(after) = join_after {
                // Mid-match: once a player's match started, `after` later.
                loop {
                    let started = *first_start.lock().expect("first start");
                    if started.is_some_and(|t| t.elapsed() >= after) {
                        break;
                    }
                    if Instant::now() >= client.deadline || stop.load(Ordering::Relaxed) {
                        let why = "timed out (no match started for it to watch)".to_string();
                        return Ok(ClientRun {
                            name: client.name,
                            dropped: Some(why),
                            ..ClientRun::default()
                        });
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            let content = Content::open(&content_dir)?;
            run_bot_client(content, &client, &stop, &mut |p| {
                if let Progress::Started(..) = p {
                    first_start.lock().expect("first start").get_or_insert_with(Instant::now);
                }
            })
        })?;
        threads.push((name, thread));
    }
    let mut runs = Vec::new();
    for (name, t) in threads {
        let run = match t.join() {
            Ok(Ok(run)) => run,
            Ok(Err(e)) => {
                ClientRun { name, dropped: Some(format!("could not start: {e:#}")), ..ClientRun::default() }
            }
            Err(_) => ClientRun { name, dropped: Some("panicked".into()), ..ClientRun::default() },
        };
        runs.push(run);
    }
    // The server keeps its record once its drain is over (after the clients').
    while records.lock().expect("records").len() < spec.matches as usize && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.store(true, Ordering::Relaxed);
    match server.join() {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e.context("the server")),
        Err(_) => return Err(anyhow!("the server panicked\n{}", log.text())),
    }
    let server = std::mem::take(&mut *records.lock().expect("records"));
    Ok(TransportRun { server, clients: runs, server_log: log.text(), elapsed: begun.elapsed() })
}

impl TransportRun {
    /// The verdict (spec §9.4): the server finished `spec.matches` matches,
    /// each record with at least `spec.min_checksums` checksums (at least one);
    /// every client did too (unless dropped and `spec.may_drop` names it: a
    /// dropped client is a failure otherwise); every client's confirmed
    /// checksums of each match equal the server's (a spectator that joined
    /// mid-match: from its state on, at least to the match's end); every
    /// replay's bytes equal the server's (made here from its record, as
    /// rl-online makes it); and that replay played back reproduces the
    /// server's checksums. `Err` lists every failure.
    pub fn verdict(&self, content: &Content, spec: &TransportSpec) -> Result<(), String> {
        let mut fails = Vec::new();
        if self.server.len() != spec.matches as usize {
            fails.push(format!("the server finished {} matches, not {}", self.server.len(), spec.matches));
        }
        let mut server_replays = Vec::new();
        for (i, r) in self.server.iter().enumerate() {
            let least = spec.min_checksums.max(1);
            if r.checksums.len() < least {
                fails.push(format!(
                    "the server's match {}: {} checksums, fewer than {least} ({} ticks)",
                    i + 1,
                    r.checksums.len(),
                    r.rows.len()
                ));
            }
            match rl_online::replay::make(content, r) {
                Ok(made) => {
                    if let Some(w) = made.warning {
                        fails.push(format!("the server's match {}: {w}", i + 1));
                    }
                    if let Err(e) = replay_checksums(content, &made.bytes, &r.checksums) {
                        fails.push(format!("the server's match {}'s replay: {e}", i + 1));
                    }
                    server_replays.push(Some(made.bytes));
                }
                Err(e) => {
                    fails.push(format!("the server's match {}: no replay: {e:#}", i + 1));
                    server_replays.push(None);
                }
            }
        }
        for c in &self.clients {
            if let Some(why) = &c.dropped {
                if !spec.may_drop.contains(&c.name) {
                    fails.push(format!("{} dropped: {why}", c.name));
                }
                continue;
            }
            if c.matches.len() != spec.matches as usize {
                fails.push(format!("{} finished {} matches, not {}", c.name, c.matches.len(), spec.matches));
            }
            if c.desync_bundles > 0 {
                fails.push(format!("{} made {} desync bundles", c.name, c.desync_bundles));
            }
            for (k, m) in c.matches.iter().enumerate() {
                let what = format!("{}'s match {}", c.name, k + 1);
                let Some(i) = self.server.iter().position(|r| r.start == m.start) else {
                    fails.push(format!(
                        "{what}: the server has no match with its start (seed {})",
                        m.start.seed
                    ));
                    continue;
                };
                let r = &self.server[i];
                match &m.record {
                    Some(rec) => {
                        if rec.checksums != r.checksums {
                            fails.push(format!("{what}: {}", differ(&rec.checksums, &r.checksums)));
                        }
                        if rec.rows != r.rows {
                            let n = rec.rows.iter().zip(&r.rows).take_while(|(a, b)| a == b).count();
                            fails.push(format!(
                                "{what}: its inputs differ from the server's from tick {n} ({} and {} ticks)",
                                rec.rows.len(),
                                r.rows.len()
                            ));
                        }
                        match (&m.replay, &server_replays[i]) {
                            (Some(a), Some(b)) if a == b => {}
                            (Some(_), Some(_)) => {
                                fails.push(format!("{what}: its replay differs from the server's"))
                            }
                            (None, _) => fails.push(format!("{what}: no replay")),
                            (Some(_), None) => {}
                        }
                    }
                    None => {
                        let all: BTreeMap<u32, u64> = r.checksums.iter().copied().collect();
                        let wrong = m.checksums.iter().find(|(t, c)| all.get(t) != Some(c));
                        if let Some((t, c)) = wrong {
                            let s = all.get(t).map_or("none".into(), |h| format!("{h:016x}"));
                            fails.push(format!("{what}: tick {t}: {c:016x}, the server's {s}"));
                        }
                        let end = (r.rows.len() as u32).saturating_sub(RECORD_AFTER_END);
                        let last = m.checksums.last().map(|&(t, _)| t);
                        if last.is_none_or(|t| t < end) {
                            fails.push(format!(
                                "{what}: its confirmed checksums end at tick {last:?}, before the match's end {end}"
                            ));
                        }
                    }
                }
            }
        }
        if fails.is_empty() { Ok(()) } else { Err(fails.join("\n")) }
    }
}

/// Where two checksum streams first differ.
fn differ(a: &[(u32, u64)], b: &[(u32, u64)]) -> String {
    let n = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    match (a.get(n), b.get(n)) {
        (Some(x), Some(y)) => format!("checksum {n} differs: {x:?}, the server's {y:?}"),
        _ => format!("{} checksums, the server's {} (equal up to there)", a.len(), b.len()),
    }
}

/// Plays replay `bytes` on `content` and checks the world's sync checksum
/// after every tick in `expected` (the server's confirmed checksums).
pub fn replay_checksums(content: &Content, bytes: &[u8], expected: &[(u32, u64)]) -> anyhow::Result<()> {
    let replay = Replay::decode(bytes).map_err(|e| anyhow!("{e}"))?;
    let mut world = content.world_for_replay(&replay)?;
    let want: BTreeMap<u32, u64> = expected.iter().copied().collect();
    let mut events = TickEvents::new();
    let mut checked = 0;
    for t in 0..replay.ticks() {
        world.tick(replay.row(t), &mut events);
        if let Some(&c) = want.get(&t) {
            let got = world.sync_checksum();
            if got != c {
                return Err(anyhow!("tick {t}: {got:016x}, the server's {c:016x}"));
            }
            checked += 1;
        }
    }
    if checked != want.len() {
        return Err(anyhow!(
            "{} ticks played, {checked} of the server's {} checksums met",
            replay.ticks(),
            want.len()
        ));
    }
    Ok(())
}
