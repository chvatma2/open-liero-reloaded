//! The headless dedicated server (networking spec §7.4): a lobby without a
//! local player (`rl_online`'s `PeerRole::Server`), a map rotation, bots
//! filling empty seats, admins' chat commands, bans kept in a file, LAN
//! announcements and one log line per event. No window, no GPU, no audio:
//! it simulates only to run the bots and check checksums.
//!
//! [`Server`] does one poll at a time on the caller's clock (tests drive it
//! in-process); [`run`] polls it on the real clock until a stop flag.

pub mod admin;
pub mod config;
pub mod files;
pub mod rotation;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use rl_content::Content;
use rl_lobby::{NOT_ADMIN, fingerprint};
use rl_net::endpoint::EndpointConfig;
use rl_net::lan::{Announcer, broadcast_targets};
use rl_online::replay::MatchRecord;
use rl_online::{OnlineEvent, OnlinePeer, PeerRole, StartError};
use rl_proto::{
    LanAnnounce, LobbyState, Look, MAX_CHAT, MatchEndWire, MatchStartWire, Phase, PublicKey, SeatWire,
};
use rl_sim::Keys;

use crate::admin::{Command, Setting, Unban};
use crate::config::{Config, Visibility};
use crate::files::{BanList, Who, hex};
use crate::rotation::Rotation;

/// A match starts this long after the lobby gathered, forced (players not
/// ready watch it), when at least `min_players` are seated.
pub const AUTO_START: Duration = Duration::from_secs(60);
/// [`run`] polls this often.
pub const POLL_EVERY: Duration = Duration::from_millis(2);
/// Desync bundles go here, in the data directory (rl-online's bounded
/// `bundle::save`).
pub const DESYNCS_DIR: &str = "desyncs";
/// The bans file is compared with the lobby's bans this often (an admin's
/// ban from the game's lobby; the server's own commands save at once).
pub const BANS_EVERY: Duration = Duration::from_secs(1);
/// A bans file that cannot be written is said at most this often.
pub const BANS_WARN_EVERY: Duration = Duration::from_secs(60);
/// Said to an admin who tries to kick or ban an admin (or themselves).
pub const NOT_AN_ADMIN_TARGET: &str = "admins cannot kick or ban admins (or themselves)";

/// What the server keeps of a member for its log.
struct Known {
    name: String,
    key: PublicKey,
}

/// The running match's counters last logged.
#[derive(Default)]
struct MatchCounters {
    desyncs: u64,
    states_sent: u64,
}

/// A dedicated server: its lobby and matches, its rotation, its bans.
pub struct Server {
    peer: OnlinePeer,
    config: Config,
    key: PublicKey,
    data_dir: PathBuf,
    rotation: Rotation,
    bans_path: Option<PathBuf>,
    /// The bans as last written to the bans file.
    bans_saved: (Vec<PublicKey>, Vec<IpAddr>),
    /// Who each banned key was (by `/ban`, `/banip`, or the file).
    who: BTreeMap<PublicKey, Who>,
    /// `who` changed since the file was written.
    who_changed: bool,
    /// When the lobby's bans were last compared with the file's
    /// ([`BANS_EVERY`]; the lists may be long).
    bans_checked: Option<Instant>,
    /// When a failure to write the bans file was last said.
    bans_warned: Option<Instant>,
    /// The last poll's time.
    now: Option<Instant>,
    /// Rotation entries the content could not build since the last start.
    failed: BTreeSet<usize>,
    /// Every entry failed in a row: no start until the server restarts.
    halted: bool,
    announcer: Option<Announcer>,
    members: BTreeMap<u8, Known>,
    /// The setup the server last gave the lobby; another (an admin's
    /// `Settings` from the game's lobby) is kept for the next match.
    set_by_us: Option<rl_proto::MatchSetupWire>,
    /// An admin's own setup holds the lobby until the next match starts.
    admin_setup: bool,
    /// When the lobby last began gathering ([`AUTO_START`]).
    gathering_since: Option<Instant>,
    counters: MatchCounters,
    /// The running (or last) match's worm names by slot.
    start_names: Vec<String>,
    seeds: u32,
    /// The lobby as of its revision (`LobbyHandle::revision`): copied only
    /// when it changed, not every poll.
    lobby: Option<(u64, LobbyState)>,
}

impl Server {
    /// Opens the server: its identity (`server-identity.ron` in
    /// `data_dir`, made at the first start), its bans, its rotation checked
    /// against `content`, the lobby on the config's port. Logs where it
    /// listens.
    pub fn start(
        config: Config,
        content: Content,
        data_dir: &Path,
        log: &mut dyn Write,
    ) -> anyhow::Result<Server> {
        std::fs::create_dir_all(data_dir).with_context(|| format!("making {}", data_dir.display()))?;
        let (me, made) = files::load_identity(data_dir)?;
        let key = me.public();
        let rotation = Rotation::new(&content, &config.rotation, config.fill_bots)?;
        let bans_path = config.bans_file.as_ref().map(|f| data_dir.join(f));
        let bans = match &bans_path {
            Some(p) => files::load_bans(p)?,
            None => BanList::default(),
        };
        let setup = rotation.setup(config.fill_bots);
        let endpoint = EndpointConfig { port: config.port, ..EndpointConfig::default() };
        let role = PeerRole::Server(config.host_config()?);
        let unused = Look { name: config.name.clone(), colour: [0; 3], skin: rl_lobby::DEFAULT_SKIN.into() };
        let mut peer = OnlinePeer::start(role, endpoint, me, unused, content, Some(setup))
            .with_context(|| format!("hosting on port {}", config.port))?;
        for &k in &bans.keys {
            peer.lobby().ban_key(k);
        }
        for &ip in &bans.ips {
            peer.lobby().ban_ip(ip);
        }
        let set_by_us = peer.lobby().state().map(|s| s.setup.clone());
        let announcer = match config.visibility {
            Visibility::Lan => match broadcast_targets().first().map(|t| Announcer::new(*t)) {
                Some(Ok(a)) => Some(a),
                Some(Err(e)) => {
                    line(log, format_args!("warning: no LAN announcements: {e}"));
                    None
                }
                None => None,
            },
            Visibility::DirectOnly => None,
        };
        let fp = fingerprint(&key);
        let identity = if made { "a new identity" } else { "its identity" };
        line(log, format_args!("server {:?} with {identity} {fp} (key {})", config.name, hex(&key)));
        line(log, format_args!("bans: {} keys, {} addresses", bans.keys.len(), bans.ips.len()));
        line(log, format_args!("listening on port {} (TCP and UDP)", peer.local_port()));
        let next = rotation.next();
        line(log, format_args!("next map: {} on {}", next.mod_id, next.level));
        Ok(Server {
            lobby: None,
            peer,
            config,
            key,
            data_dir: data_dir.to_path_buf(),
            rotation,
            bans_path,
            bans_saved: (bans.keys, bans.ips),
            who: bans.who,
            who_changed: false,
            bans_checked: None,
            bans_warned: None,
            now: None,
            failed: BTreeSet::new(),
            halted: false,
            announcer,
            members: BTreeMap::new(),
            set_by_us,
            admin_setup: false,
            gathering_since: None,
            counters: MatchCounters::default(),
            start_names: Vec::new(),
            seeds: 0,
        })
    }

    /// The TCP and UDP port it listens on.
    pub fn local_port(&self) -> u16 {
        self.peer.local_port()
    }

    /// The server's public key (its identity).
    pub fn key(&self) -> PublicKey {
        self.key
    }

    /// The last match the server finished, as its confirmed inputs and
    /// checksums (rl-online's `last_record`; `rl-cli`'s real-transport
    /// verdict compares every client's with it).
    pub fn last_record(&self) -> Option<&MatchRecord> {
        self.peer.last_record()
    }

    /// Does what the network, the lobby and the match have for `now`, and
    /// what the server decides: commands, bots, starts, bans, the log.
    pub fn poll(&mut self, now: Instant, log: &mut dyn Write) {
        self.now = Some(now);
        let events = self.peer.poll(now, &mut || Keys(0), &mut |_, _, _| {});
        for e in events {
            self.on_event(e, log);
        }
        self.track_match(log);
        if self.bans_checked.is_none_or(|t| now.saturating_duration_since(t) >= BANS_EVERY) {
            self.bans_checked = Some(now);
            self.save_bans(log);
        }
        // The lobby copied only when its revision changed (a setup `fill`
        // changes shows in the next poll).
        let Some(revision) = self.peer.lobby().revision() else { return };
        let cached = self.lobby.take().filter(|(r, _)| *r == revision);
        let Some((revision, state)) = cached.or_else(|| self.state().map(|s| (revision, s))) else { return };
        self.track_members(&state, log);
        self.fill(&state, log);
        self.maybe_start(&state, now, log);
        self.announce(&state, now);
        self.lobby = Some((revision, state));
    }

    /// Says goodbye to everyone (`Bye`) and closes every connection.
    pub fn shutdown(&mut self, log: &mut dyn Write) {
        self.peer.leave();
        self.save_bans(log);
        line(log, format_args!("server stopped"));
    }

    fn state(&mut self) -> Option<LobbyState> {
        self.peer.lobby().state().cloned()
    }

    fn on_event(&mut self, e: OnlineEvent, log: &mut dyn Write) {
        match e {
            OnlineEvent::Chat { from, member, text } => {
                line(log, format_args!("chat {from}: {text}"));
                if let Some(cmd) = admin::parse(&text) {
                    self.command(member, &from, cmd, log);
                }
            }
            // Joins are logged from the lobby with their key and address.
            OnlineEvent::Notice(text) if !text.ends_with(" joined") => {
                line(log, format_args!("notice: {text}"));
            }
            OnlineEvent::MatchStarting { start, .. } => self.started(&start, log),
            OnlineEvent::MatchEnded(end) => self.ended(&end, log),
            OnlineEvent::DesyncBundle(bytes) => self.save_bundle(&bytes, log),
            _ => {}
        }
    }

    // ---- members ----

    /// Joins and leaves, from the lobby's members.
    fn track_members(&mut self, state: &LobbyState, log: &mut dyn Write) {
        let gone: Vec<u8> =
            self.members.keys().copied().filter(|id| !state.members.iter().any(|m| m.id == *id)).collect();
        for id in gone {
            if let Some(k) = self.members.remove(&id) {
                line(log, format_args!("leave {} [{}]", k.name, fingerprint(&k.key)));
            }
        }
        for m in &state.members {
            if let Some(k) = self.members.get_mut(&m.id) {
                k.name.clone_from(&m.look.name);
                continue;
            }
            let ip = self.peer.lobby().addr_of(m.id).map_or_else(|| "?".into(), |a| a.ip().to_string());
            let admin = if m.admin { " (admin)" } else { "" };
            line(
                log,
                format_args!(
                    "join {} [{}] from {ip}{admin} key {}",
                    m.look.name,
                    fingerprint(&m.key),
                    hex(&m.key)
                ),
            );
            self.members.insert(m.id, Known { name: m.look.name.clone(), key: m.key });
        }
    }

    // ---- commands ----

    fn command(&mut self, member: u8, name: &str, cmd: Result<Command, String>, log: &mut dyn Write) {
        let Some(state) = self.state() else { return };
        // The member may have left in the same poll: then nothing.
        let Some(m) = state.members.iter().find(|m| m.id == member) else { return };
        let cmd = match cmd {
            Ok(c) => c,
            Err(why) => return self.peer.lobby().tell(member, &why),
        };
        if cmd.admin_only() && !m.admin {
            line(log, format_args!("refused {} from {name} (not an admin)", command_word(&cmd)));
            return self.peer.lobby().tell(member, NOT_ADMIN);
        }
        let how = match cmd {
            Command::Ban(_) => "ban",
            Command::BanIp(_) => "banip",
            _ => "kick",
        };
        match cmd {
            Command::Help => {
                for l in admin::help(m.admin) {
                    self.peer.lobby().tell(member, l);
                }
            }
            Command::Players => {
                let names: Vec<String> = state
                    .members
                    .iter()
                    .map(|m| {
                        let seat = if matches!(m.seat, SeatWire::Spectator) { ", watching" } else { "" };
                        format!("{} [{}{seat}]", m.look.name, fingerprint(&m.key))
                    })
                    .collect();
                for l in chunks("players: ", &names) {
                    self.peer.lobby().tell(member, &l);
                }
            }
            Command::Kick(target) | Command::Ban(target) | Command::BanIp(target) => {
                let wanted = target.to_lowercase();
                let Some(t) = state.members.iter().find(|t| t.look.name.to_lowercase() == wanted) else {
                    return self.peer.lobby().tell(member, &format!("no member named {target}"));
                };
                if t.admin || t.id == member {
                    line(log, format_args!("refused /{how} {} from {name} (an admin)", t.look.name));
                    return self.peer.lobby().tell(member, NOT_AN_ADMIN_TARGET);
                }
                let addr = self.peer.lobby().addr_of(t.id).map(|a| a.ip());
                let ip = addr.map_or_else(|| "?".into(), |a| a.to_string());
                line(
                    log,
                    format_args!(
                        "{how} {} [{}] from {ip} by {name} key {}",
                        t.look.name,
                        fingerprint(&t.key),
                        hex(&t.key)
                    ),
                );
                match how {
                    "kick" => self.peer.lobby().kick(t.id, false),
                    "ban" => self.peer.lobby().kick(t.id, true),
                    _ => self.peer.lobby().ban_with_address(t.id),
                }
                if how != "kick" {
                    let ip = if how == "banip" { addr } else { None };
                    self.who.insert(t.key, Who { name: t.look.name.clone(), ip });
                    self.who_changed = true;
                    self.save_bans(log);
                }
            }
            Command::Unban(arg) => self.unban(member, name, &arg, log),
            Command::Next => {
                if self.peer.lobby().end_match() {
                    line(log, format_args!("{name} ended the match (/next)"));
                } else {
                    self.rotation.advance();
                    self.admin_setup = false;
                    let next = self.rotation.next();
                    let text = format!("next map: {} on {}", next.mod_id, next.level);
                    line(log, format_args!("{name} skipped the map; {text}"));
                    self.peer.lobby().chat(&text);
                }
            }
            Command::Settings(s) => match self.rotation.set(s) {
                Ok(()) => {
                    let text =
                        format!("{} for the next match on {}", setting_text(s), self.rotation.next().level);
                    line(log, format_args!("{name} set {text}"));
                    self.admin_setup = false;
                    self.peer.lobby().chat(&text);
                }
                Err(why) => self.peer.lobby().tell(member, &why),
            },
        }
    }

    /// `/unban <name|key prefix|address>`: a key's ban (and the address
    /// banned with it), or an address's.
    fn unban(&mut self, member: u8, name: &str, arg: &str, log: &mut dyn Write) {
        let (keys, _) = self.peer.lobby().bans();
        let banned: Vec<(PublicKey, Option<&str>)> =
            keys.iter().map(|k| (*k, self.who.get(k).map(|w| w.name.as_str()))).collect();
        let lifted = match admin::unban_target(arg, &banned) {
            Err(why) => return self.peer.lobby().tell(member, &why),
            Ok(Unban::Address(ip)) => {
                if !self.peer.lobby().unban_ip(ip) {
                    return self.peer.lobby().tell(member, &format!("{ip} is not banned"));
                }
                ip.to_string()
            }
            Ok(Unban::Key(key)) => {
                self.peer.lobby().unban_key(&key);
                let who = self.who.remove(&key);
                self.who_changed = true;
                let mut text =
                    format!("{} [{}]", who.as_ref().map_or("a key", |w| w.name.as_str()), fingerprint(&key));
                if let Some(ip) = who.and_then(|w| w.ip)
                    && self.peer.lobby().unban_ip(ip)
                {
                    text.push_str(&format!(" and {ip}"));
                }
                text
            }
        };
        line(log, format_args!("unban {lifted} by {name}"));
        self.peer.lobby().tell(member, &format!("unbanned {lifted}"));
        self.save_bans(log);
    }

    // ---- the lobby's setup and the starts ----

    /// While the lobby gathers: the rotation's next setup with bots for the
    /// seats players leave free (an admin's own setup holds until the next
    /// match).
    fn fill(&mut self, state: &LobbyState, log: &mut dyn Write) {
        if state.phase != Phase::Gathering
            || matches!(self.peer.lobby().can_start(), Err(StartError::Running))
        {
            return;
        }
        if self.set_by_us.as_ref() != Some(&state.setup) && !self.admin_setup {
            self.admin_setup = true;
            let s = &state.setup;
            line(log, format_args!("an admin set the next match: {} on {}", s.mod_id, s.level_id));
        }
        if self.admin_setup {
            return;
        }
        let players = seated(state);
        let bots = self.config.fill_bots.saturating_sub(players);
        let wanted = self.rotation.setup(bots);
        if wanted != state.setup {
            match self.peer.lobby().set_setup(wanted) {
                Ok(()) => self.set_by_us = self.state().map(|s| s.setup),
                Err(e) => line(log, format_args!("warning: the lobby refuses the rotation's setup: {e}")),
            }
        }
    }

    fn maybe_start(&mut self, state: &LobbyState, now: Instant, log: &mut dyn Write) {
        if state.phase != Phase::Gathering
            || matches!(self.peer.lobby().can_start(), Err(StartError::Running))
        {
            self.gathering_since = None;
            return;
        }
        let since = *self.gathering_since.get_or_insert(now);
        if self.halted || seated(state) < self.config.min_players {
            return;
        }
        let ready = self.peer.lobby().can_start().is_ok();
        let forced = now.saturating_duration_since(since) >= AUTO_START;
        if !(ready && self.config.start_when_ready) && !forced {
            return;
        }
        let seed = self.seed();
        let started = if ready { self.peer.lobby().start(seed) } else { self.peer.lobby().force_start(seed) };
        match started {
            Ok(()) => {}
            // The content cannot build it: an admin's setup is dropped, the
            // rotation's entry skipped (said once a cycle); after a whole
            // cycle of failures no start is tried again.
            Err(StartError::Build(e)) => {
                let s = &state.setup;
                if std::mem::take(&mut self.admin_setup) {
                    line(log, format_args!("cannot start {} on {}: {e}; dropped", s.mod_id, s.level_id));
                    return;
                }
                if self.failed.insert(self.rotation.next_index()) {
                    line(log, format_args!("cannot start {} on {}: {e}; skipping it", s.mod_id, s.level_id));
                }
                self.rotation.advance();
                if self.failed.len() == self.rotation.len() {
                    self.halted = true;
                    line(
                        log,
                        format_args!(
                            "error: no map of the rotation can be built: no match starts until the server \
                             restarts (the lobby stays open)"
                        ),
                    );
                }
            }
            // Not startable even forced (no ready player): try again later.
            Err(e) => {
                if forced {
                    line(log, format_args!("no forced start: {e}"));
                    self.gathering_since = Some(now);
                }
            }
        }
    }

    fn seed(&mut self) -> u32 {
        self.seeds = self.seeds.wrapping_add(1);
        rl_net::random_bytes::<4>().map_or(self.seeds, u32::from_le_bytes)
    }

    fn started(&mut self, start: &MatchStartWire, log: &mut dyn Write) {
        let players: Vec<&str> =
            start.slots.iter().filter(|s| s.member.is_some()).map(|s| s.look.name.as_str()).collect();
        line(
            log,
            format_args!(
                "match start: {} on {}, seed {}, players {}, bots {}",
                start.setup.mod_id,
                start.setup.level_id,
                start.seed,
                players.join(", "),
                start.setup.bots.len()
            ),
        );
        self.failed.clear();
        // The lobby now offers the map after it (unless an admin's own setup played).
        if self.admin_setup {
            self.admin_setup = false;
        } else {
            self.rotation.advance();
        }
        self.counters = MatchCounters::default();
        self.start_names = start.slots.iter().map(|s| s.look.name.clone()).collect();
    }

    fn ended(&mut self, end: &MatchEndWire, log: &mut dyn Write) {
        let name = |slot: u8| {
            self.start_names.get(usize::from(slot)).cloned().unwrap_or_else(|| format!("slot {slot}"))
        };
        let winner = match end.rows.first() {
            Some(&(slot, score, kills, deaths, _)) => {
                format!("winner {} (score {score}, kills {kills}, deaths {deaths})", name(slot))
            }
            None => "no winner".into(),
        };
        line(log, format_args!("match end ({}): {winner}", end.reason));
    }

    /// Desyncs and repairs (states sent) of the running match.
    fn track_match(&mut self, log: &mut dyn Write) {
        let Some(s) = self.peer.match_session() else { return };
        let stats = s.stats();
        if stats.desyncs > self.counters.desyncs {
            line(log, format_args!("desync: {} peer checksums differed this match", stats.desyncs));
        }
        if stats.states_sent > self.counters.states_sent {
            let n = stats.states_sent - self.counters.states_sent;
            line(log, format_args!("repair: {n} state(s) sent (a repair or a joining spectator)"));
        }
        self.counters = MatchCounters { desyncs: stats.desyncs, states_sent: stats.states_sent };
    }

    fn save_bundle(&mut self, bytes: &[u8], log: &mut dyn Write) {
        let dir = self.data_dir.join(DESYNCS_DIR);
        let secs =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        // Bounded: 4 MiB a bundle, 64 MiB of bundles (the oldest go).
        match rl_online::bundle::save(&dir, bytes, secs) {
            Ok(path) => line(log, format_args!("desync bundle saved: {}", path.display())),
            Err(e) => line(log, format_args!("warning: cannot save a desync bundle: {e}")),
        }
    }

    /// Writes the bans file when the lobby's bans (or who they were)
    /// changed. A failure is tried again at the next check and said at most
    /// every [`BANS_WARN_EVERY`].
    fn save_bans(&mut self, log: &mut dyn Write) {
        let bans = self.peer.lobby().bans();
        if bans == self.bans_saved && !self.who_changed {
            return;
        }
        self.who.retain(|k, _| bans.0.contains(k));
        if let Some(path) = &self.bans_path {
            let list = BanList { keys: bans.0.clone(), ips: bans.1.clone(), who: self.who.clone() };
            if let Err(e) = files::save_bans(path, &list) {
                let now = self.now.unwrap_or_else(Instant::now);
                if self.bans_warned.is_none_or(|t| now.saturating_duration_since(t) >= BANS_WARN_EVERY) {
                    self.bans_warned = Some(now);
                    line(log, format_args!("warning: {e:#} (tried again every second)"));
                }
                return;
            }
            line(log, format_args!("bans saved: {} keys, {} addresses", bans.0.len(), bans.1.len()));
        }
        self.bans_saved = bans;
        self.who_changed = false;
    }

    fn announce(&mut self, state: &LobbyState, now: Instant) {
        let port = self.local_port();
        if let Some(a) = &mut self.announcer {
            a.tick(now, &announcement(state, port, self.config.max_players, self.config.password.is_some()));
        }
    }
}

/// Runs a server until `stop` is set: [`Server::start`], a poll every
/// [`POLL_EVERY`] on the real clock, then [`Server::shutdown`] (`Bye` to
/// everyone). The log's lines say where it listens ("listening on port
/// N") and what happens.
pub fn run(
    config: Config,
    content: Content,
    data_dir: &Path,
    stop: &AtomicBool,
    log: &mut dyn Write,
) -> anyhow::Result<()> {
    run_with(config, content, data_dir, stop, log, &mut |_| {})
}

/// [`run`], showing the server to `observe` after each poll (its port, its
/// finished matches: `rl-cli`'s real-transport tests).
pub fn run_with(
    config: Config,
    content: Content,
    data_dir: &Path,
    stop: &AtomicBool,
    log: &mut dyn Write,
    observe: &mut dyn FnMut(&Server),
) -> anyhow::Result<()> {
    let mut server = Server::start(config, content, data_dir, log)?;
    while !stop.load(Ordering::Relaxed) {
        server.poll(Instant::now(), log);
        observe(&server);
        std::thread::sleep(POLL_EVERY);
    }
    server.shutdown(log);
    Ok(())
}

/// One log line (a failing log never stops the server).
fn line(log: &mut dyn Write, args: std::fmt::Arguments<'_>) {
    let _ = writeln!(log, "{args}");
    let _ = log.flush();
}

/// Members seated as players.
fn seated(state: &LobbyState) -> u8 {
    let n = state.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).count();
    u8::try_from(n).unwrap_or(u8::MAX)
}

fn command_word(c: &Command) -> &'static str {
    match c {
        Command::Kick(_) => "/kick",
        Command::Ban(_) => "/ban",
        Command::BanIp(_) => "/banip",
        Command::Unban(_) => "/unban",
        Command::Next => "/next",
        Command::Settings(_) => "/settings",
        Command::Players => "/players",
        Command::Help => "/help",
    }
}

fn setting_text(s: Setting) -> String {
    match s {
        Setting::TimeLimit(m) => format!("time_limit {m} min"),
        Setting::KillLimit(k) => format!("kill_limit {k}"),
        Setting::Lives(l) => format!("lives {l}"),
        Setting::Bonuses(b) => format!("bonuses {}", if b { "on" } else { "off" }),
    }
}

/// `items` after `head`, comma-separated, in lines of at most `MAX_CHAT`
/// characters (a notice's limit).
fn chunks(head: &str, items: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = head.to_string();
    for item in items {
        let sep = if cur == head { "" } else { ", " };
        if cur.chars().count() + sep.len() + item.chars().count() > MAX_CHAT && cur != head {
            lines.push(std::mem::replace(&mut cur, head.to_string()));
            cur.push_str(item);
        } else {
            cur.push_str(sep);
            cur.push_str(item);
        }
    }
    lines.push(cur);
    lines
}

/// The LAN announcement of the lobby on `port` (as the game's).
fn announcement(state: &LobbyState, port: u16, max_players: u8, password: bool) -> LanAnnounce {
    let players = state.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).count();
    let spectators = state.members.len() - players;
    LanAnnounce {
        fingerprint: rl_proto::FINGERPRINT,
        name: state.name.clone(),
        port,
        players: u8::try_from(players + state.setup.bots.len()).unwrap_or(u8::MAX),
        max_players,
        spectators: u8::try_from(spectators).unwrap_or(u8::MAX),
        mod_id: state.setup.mod_id.clone(),
        level_id: state.setup.level_id.clone(),
        password,
        playing: matches!(state.phase, Phase::Starting | Phase::Playing { .. }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_lists_split_into_notices() {
        let items: Vec<String> = (0..48).map(|i| format!("Player number {i:02} [abcdefgh]")).collect();
        let lines = chunks("players: ", &items);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.starts_with("players: ") && l.chars().count() <= MAX_CHAT));
        let joined: Vec<String> =
            lines.iter().flat_map(|l| l["players: ".len()..].split(", ").map(String::from)).collect();
        assert_eq!(joined, items);
        assert_eq!(chunks("players: ", &[]), vec!["players: ".to_string()]);
    }
}
