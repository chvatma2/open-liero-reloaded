//! The headless dedicated server (networking spec §7.4): a lobby without a
//! local player (`olr_online`'s `PeerRole::Server`), a map rotation, bots
//! filling empty seats, admins' chat commands, bans kept in a file, LAN
//! and Nostr announcements, an invite code and one log line per event. No
//! window, no GPU, no audio: it simulates only to run the bots and check
//! checksums.
//!
//! [`Server`] does one poll at a time on the caller's clock (tests drive it
//! in-process); [`run`] polls it on the real clock until a stop flag.
//!
//! Plan 12b: the server maps its port at the router (N51), takes joins
//! through its Nostr relays (N54), advertises an address only when it is
//! directly reachable (N47), can run a volunteer TURN relay ([`relay`],
//! N52) and checks the network ([`nat_check`], N60).

pub mod admin;
pub mod config;
pub mod files;
pub mod nat_check;
pub mod relay;
pub mod rotation;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use olr_content::{Content, DownloadCache};
use olr_discovery::{GameSummary, Hosting, Pool, PoolConfig, PoolEvent, Publisher, RelayStatus};
use olr_lobby::{NOT_ADMIN, RELAYED_BAN, fingerprint};
use olr_net::endpoint::{EndpointConfig, IceConfig};
use olr_net::lan::{Announcer, broadcast_targets};
use olr_net::portmap::DROP_WAIT;
use olr_net::turn::TurnStats;
use olr_online::join::path_tag;
use olr_online::replay::MatchRecord;
use olr_online::{HostListen, LostWhy, OnlineEvent, OnlinePeer, PeerRole, Reachability, StartError};
use olr_proto::{
    LobbyState, Look, MAX_CHAT, MatchEndWire, MatchStartWire, Path as LinkPath, Phase, PublicKey, SeatWire,
};
use olr_sim::Keys;

use crate::admin::{Command, Setting, Unban};
use crate::config::{Config, Visibility};
use crate::files::{BanList, Who, hex};
use crate::relay::{MapTarget, Relay};
use crate::rotation::Rotation;

/// A match starts this long after the lobby gathered, forced (players not
/// ready watch it), when at least `min_players` are seated.
pub const AUTO_START: Duration = Duration::from_secs(60);
/// [`run`] polls this often.
pub const POLL_EVERY: Duration = Duration::from_millis(2);
/// Desync bundles go here, in the data directory (olr-online's bounded
/// `bundle::save`).
pub const DESYNCS_DIR: &str = "desyncs";
/// The bans file is compared with the lobby's bans this often (an admin's
/// ban from the game's lobby; the server's own commands save at once).
pub const BANS_EVERY: Duration = Duration::from_secs(1);
/// A bans file that cannot be written is said at most this often.
pub const BANS_WARN_EVERY: Duration = Duration::from_secs(60);
/// Said to an admin who tries to kick or ban an admin (or themselves).
pub const NOT_AN_ADMIN_TARGET: &str = "admins cannot kick or ban admins (or themselves)";
/// The download cache (plan 11 ruling N30), in the data directory.
pub const DOWNLOADS_DIR: &str = "downloads";
/// The first note and the printed invite wait this long at most for the
/// reachability to settle (`Reachability::settled`: the router answered or
/// gave up, within `UPNP_DEADLINE` 4 s, and STUN's first round).
pub const SETTLE_WAIT: Duration = Duration::from_secs(5);

/// What the server keeps of a member for its log.
struct Known {
    name: String,
    key: PublicKey,
}

/// Publishing on Nostr: a public server's notes, a relay's TURN notes; the
/// relays, and what each last said in the log.
struct Nostr {
    pool: Pool,
    /// Signs and times the game's notes (the server's identity); `None`:
    /// the game is not announced (only a relay's notes go out).
    publisher: Option<Publisher>,
    /// Each relay's last status or refusal logged (said once until it
    /// changes).
    said: Vec<Option<String>>,
    events: Vec<PoolEvent>,
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
    /// The time since the poll before this one: how long a stall that made
    /// the server step down lasted (N70).
    gap: Duration,
    /// Rotation entries the content could not build since the last start.
    failed: BTreeSet<usize>,
    /// Every entry failed in a row: no start until the server restarts.
    halted: bool,
    announcer: Option<Announcer>,
    nostr: Option<Nostr>,
    /// The host's reachability as last reported (N47), and the lines the
    /// log said of it.
    reach: Option<Reachability>,
    reach_said: Vec<String>,
    /// When the server started ([`SETTLE_WAIT`]).
    started: Instant,
    /// The direct address of the invite last printed.
    invite_said: Option<Option<SocketAddr>>,
    /// The volunteer TURN relay (`--relay`).
    relay: Option<Relay>,
    members: BTreeMap<u8, Known>,
    /// The setup the server last gave the lobby; another (an admin's
    /// `Settings` from the game's lobby) is kept for the next match.
    set_by_us: Option<olr_proto::MatchSetupWire>,
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
        let identity = me.clone();
        let endpoint = EndpointConfig { port: config.port, ice: ice(&config), ..EndpointConfig::default() };
        // N54: Public and Invite servers take joins through their relays.
        let listen = HostListen {
            pool: (config.visibility != Visibility::Lan && !config.relays.is_empty())
                .then(|| PoolConfig { relays: config.relays.clone(), ..PoolConfig::default() }),
            public_address: config.public_ip(),
        };
        let role = PeerRole::Server { lobby: config.host_config()?, listen };
        let unused = Look {
            name: config.name.clone(),
            colour: [0; 3],
            skin: olr_lobby::DEFAULT_SKIN.into(),
            skin_hash: olr_proto::NO_HASH.into(),
        };
        let downloads = open_downloads(&data_dir.join(DOWNLOADS_DIR), log);
        let content_cfg = olr_online::PeerContent { cache: downloads, upload_rate: config.upload_rate() };
        let mut peer = OnlinePeer::start(role, endpoint, me, unused, content, Some(setup), content_cfg)
            .with_context(|| format!("hosting on port {}", config.port))?;
        for &k in &bans.keys {
            peer.lobby().ban_key(k);
        }
        for &ip in &bans.ips {
            peer.lobby().ban_ip(ip);
        }
        let set_by_us = peer.lobby().state().map(|s| s.setup.clone());
        let announcer = match config.visibility {
            Visibility::Public | Visibility::Lan => {
                match broadcast_targets().first().map(|t| Announcer::new(*t)) {
                    Some(Ok(a)) => Some(a),
                    Some(Err(e)) => {
                        line(log, format_args!("warning: no LAN announcements: {e}"));
                        None
                    }
                    None => None,
                }
            }
            Visibility::Invite => None,
        };
        let fp = fingerprint(&key);
        let which = if made { "a new identity" } else { "its identity" };
        line(log, format_args!("server {:?} with {which} {fp} (key {})", config.name, hex(&key)));
        line(log, format_args!("bans: {} keys, {} addresses", bans.keys.len(), bans.ips.len()));
        line(log, format_args!("listening on port {} (TCP and UDP)", peer.local_port()));
        let announced = match config.visibility {
            Visibility::Public if config.relays.is_empty() => {
                line(log, format_args!("warning: public, but no relays configured: not announced on Nostr"));
                false
            }
            Visibility::Public if let Some(ip) = config.unreachable_public_address() => {
                line(
                    log,
                    format_args!(
                        "warning: public_address {ip} is not reachable from the Internet: not announced on Nostr \
                         (set the server's public IPv4 address, or remove public_address)"
                    ),
                );
                false
            }
            Visibility::Public => {
                line(
                    log,
                    format_args!(
                        "announcing on {} Nostr relays: {}",
                        config.relays.len(),
                        config.relays.join(", ")
                    ),
                );
                true
            }
            Visibility::Lan => {
                line(log, format_args!("announced on the LAN only"));
                false
            }
            Visibility::Invite => {
                line(log, format_args!("not announced: joined by its invite code or address"));
                false
            }
        };
        let relay = match &config.relay {
            Some(_) if config.relays.is_empty() => {
                line(log, format_args!("relay: not started: no Nostr relays to announce it on"));
                None
            }
            Some(rc) => Relay::new(
                rc.clone(),
                identity.clone(),
                config.region,
                config.allow_local,
                config.public_ip(),
                config.port_mapping.then(|| map_target(&config)),
                log,
            ),
            None => None,
        };
        let nostr = (announced || relay.is_some()).then(|| Nostr {
            pool: Pool::start(PoolConfig { relays: config.relays.clone(), ..PoolConfig::default() }),
            publisher: announced.then(|| Publisher::new(identity, None)),
            said: vec![None; config.relays.len()],
            events: Vec::new(),
        });
        let next = rotation.next();
        line(log, format_args!("next map: {} on {}", next.mod_id, next.level));
        let server = Server {
            nostr,
            reach: None,
            reach_said: Vec::new(),
            started: Instant::now(),
            invite_said: None,
            relay,
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
            gap: Duration::ZERO,
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
        };
        Ok(server)
    }

    /// The server's invite code (decision N42): its key, the relays it
    /// listens on (none for a LAN server) and, when it is directly
    /// reachable (N47), that address.
    pub fn invite(&self) -> olr_discovery::Invite {
        let direct = self.direct();
        let relays: &[String] =
            if self.config.visibility == Visibility::Lan { &[] } else { &self.config.relays };
        let port = direct.map_or(self.local_port(), |a| a.port());
        olr_discovery::invite(self.key, relays, direct.map(|a| a.ip()), port)
    }

    /// N47: the usable mapping's external address, else the configured
    /// one on the local port.
    fn direct(&self) -> Option<SocketAddr> {
        self.reach.as_ref().and_then(Reachability::direct)
    }

    /// The reachability settled, or [`SETTLE_WAIT`] passed.
    fn settled(&self, now: Instant) -> bool {
        self.reach.as_ref().is_some_and(Reachability::settled)
            || now.saturating_duration_since(self.started) >= SETTLE_WAIT
    }

    /// The invite once settled, and again whenever its address changes.
    fn say_invite(&mut self, now: Instant, log: &mut dyn Write) {
        if !self.settled(now) || self.invite_said == Some(self.direct()) {
            return;
        }
        self.invite_said = Some(self.direct());
        let invite = self.invite();
        let code = invite.encode();
        match (invite.addr, invite.relays.is_empty()) {
            (Some(_), _) => line(log, format_args!("invite code: {code}")),
            (None, false) => line(log, format_args!("invite code (joins through the relays): {code}")),
            (None, true) => {
                line(log, format_args!("invite code (no address and no relays: it joins nothing): {code}"))
            }
        }
    }

    /// N47 and N54 in the log: each new line of the reachability.
    fn reached(&mut self, r: Reachability, log: &mut dyn Write) {
        let lines = r.lines();
        for l in lines.iter().filter(|l| !self.reach_said.contains(l)) {
            line(log, format_args!("reach: {l}"));
        }
        self.reach_said = lines;
        self.reach = Some(r);
    }

    /// The TURN relay's listening address, once it runs (`--relay`).
    pub fn relay_addr(&self) -> Option<SocketAddr> {
        self.relay.as_ref().and_then(Relay::local_addr)
    }

    /// What the TURN relay did, once it runs.
    pub fn relay_stats(&self) -> Option<TurnStats> {
        self.relay.as_ref().and_then(Relay::stats)
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
    /// checksums (olr-online's `last_record`; `olr-cli`'s real-transport
    /// verdict compares every client's with it).
    pub fn last_record(&self) -> Option<&MatchRecord> {
        self.peer.last_record()
    }

    /// The running match's confirmed input rows (the ticks below the
    /// session's frontier; none without a match): what `olr-cli`'s
    /// real-transport verdict holds a server that freezes or leaves to.
    pub fn confirmed_rows(&self) -> Vec<[u8; 32]> {
        let Some(s) = self.peer.match_session() else { return Vec::new() };
        let log = s.input_log();
        (0..log.frontier()).map(|t| log.row(t).unwrap_or([0; 32])).collect()
    }

    /// Does what the network, the lobby and the match have for `now`, and
    /// what the server decides: commands, bots, starts, bans, the log.
    pub fn poll(&mut self, now: Instant, log: &mut dyn Write) {
        self.gap = self.now.map_or(Duration::ZERO, |t| now.saturating_duration_since(t));
        self.now = Some(now);
        self.relays(log);
        let events = self.peer.poll(now, &mut || Keys(0), &mut |_, _, _| {});
        for e in events {
            self.on_event(e, log);
        }
        self.say_invite(now, log);
        if let Some(relay) = &mut self.relay {
            let peer = &self.peer;
            if let Some(note) = relay.poll(now, &mut || peer.reachability(), log)
                && let Some(n) = &mut self.nostr
            {
                n.pool.publish(note);
            }
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

    /// Says goodbye to everyone (`Bye`), closes every connection, stops
    /// listening and, when it was announced on Nostr, publishes a closing
    /// note (and the relay's); the pool's drop sends them, bounded at 2 s.
    /// Then waits at most [`DROP_WAIT`] (1 s) for the router to delete the
    /// port mappings (N51, ruling NT-2).
    pub fn shutdown(mut self, log: &mut dyn Write) {
        self.save_bans(log);
        let mut nostr = self.nostr.take();
        if let Some(n) = &mut nostr
            && let Some(publisher) = &mut n.publisher
            && let Some((_, state)) = &self.lobby
        {
            let summary = GameSummary::of_lobby(state, &self.hosting(state));
            if let Some(note) = publisher.closing(unix_now(), &summary, aux) {
                n.pool.publish(note);
                line(log, format_args!("closing note published"));
            }
        }
        let relay = self.relay.take().map(|r| r.shutdown(log));
        // The endpoint's mapping and the relay's go at once, on threads.
        let peer = self.peer.shutdown();
        let until = Instant::now() + DROP_WAIT;
        let unmapping = relay.map(|(closing, unmapping)| {
            if let (Some(note), Some(n)) = (closing, &mut nostr) {
                n.pool.publish(note);
            }
            unmapping
        });
        drop(nostr);
        let deleted = peer.wait(until) & unmapping.is_none_or(|u| u.wait(until));
        if !deleted {
            line(
                log,
                format_args!("warning: the router did not confirm deleting the port mappings within 1 s"),
            );
        }
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
            // Ruling SB-2: the server heard none of its members for
            // `HOST_SILENT_US` during a match (its own network down);
            // olr-online left the match as at a step-down.
            OnlineEvent::HostLost(LostWhy::CutOff) => line(
                log,
                format_args!(
                    "heard none of the players: the match moved to the next player; the lobby is open again"
                ),
            ),
            // N70: the server stalled past `STEP_DOWN_US` during a match
            // with someone to take over; olr-online stepped down (its `Bye`,
            // its replay, the lobby gathering again), `secs` its gap.
            OnlineEvent::HostLost(LostWhy::SteppedDown { secs }) => line(
                log,
                format_args!(
                    "stalled for {secs:.1} s: the match moved to the next player; the lobby is open again"
                ),
            ),
            // A server is never a client, so the above are its only host
            // lost; any other is said as it is (final review B-M6).
            OnlineEvent::HostLost(why) => line(log, format_args!("host lost: {why}")),
            OnlineEvent::MigrationLog(text) => line(log, format_args!("migration: {text}")),
            OnlineEvent::DesyncBundle(bytes) => self.save_bundle(&bytes, log),
            OnlineEvent::Reachability(r) => self.reached(r, log),
            // FR12b-3: what became of a join through the relays.
            OnlineEvent::RelayJoin { join, more } => line(log, format_args!("relays: {}", join.line(more))),
            // B-M4: the listener's relays, when no pool of the server's own
            // says the same relays' news.
            OnlineEvent::ListenRelay { url, status } if self.nostr.is_none() => {
                line(log, format_args!("relay {url}: {status}"));
            }
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
            let via = m.path.map_or("an unknown path", path_tag);
            line(
                log,
                format_args!(
                    "join: {} ({}) from {ip} via {via}{admin} key {}",
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
        let public = self.config.visibility == Visibility::Public;
        if cmd.admin_only(public) && !m.admin {
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
                for l in admin::help(m.admin, public) {
                    self.peer.lobby().tell(member, l);
                }
            }
            Command::Invite => {
                let code = self.invite().encode();
                self.peer.lobby().tell(member, &format!("invite: {code}"));
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
                // N59: a relayed member's address is the relay's, which
                // banning would close to everyone: its key alone is banned
                // (live and in the bans file), and the admin is told.
                let relayed = t.path == Some(LinkPath::Relayed);
                match how {
                    "kick" => self.peer.lobby().kick(t.id, false),
                    "ban" => self.peer.lobby().kick(t.id, true),
                    _ => {
                        self.peer.lobby().ban_with_address_by(t.id, Some(member));
                        if relayed {
                            line(log, format_args!("banip {}: {RELAYED_BAN}", t.look.name));
                        }
                    }
                }
                if how != "kick" {
                    let ip = if how == "banip" && !relayed { addr } else { None };
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
        olr_net::random_bytes::<4>().map_or(self.seeds, u32::from_le_bytes)
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
        // Spec §3.7: the members with a standby and a world, who could host
        // this match should the server go: its players (a slot of the
        // start) and its spectators.
        let (mut players, mut spectators) = (0, 0);
        for e in self.peer.lobby().succession().iter().filter(|e| e.can_host) {
            if start.slots.iter().any(|s| s.member == Some(e.member)) {
                players += 1;
            } else {
                spectators += 1;
            }
        }
        line(log, format_args!("succession: {}", succession_line(players, spectators)));
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
        match olr_online::bundle::save(&dir, bytes, secs) {
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

    /// What the summary (one builder for the LAN and Nostr) takes beside
    /// the lobby: the direct address and its port (N47), when there is one.
    fn hosting(&self, state: &LobbyState) -> Hosting {
        let content = self.peer.content();
        let direct = self.direct();
        Hosting {
            port: self.local_port(),
            max_players: self.config.max_players,
            max_spectators: self.config.max_spectators,
            password: self.config.password.is_some(),
            region: self.config.region,
            addr: direct.map(|a| a.ip()),
            public_port: direct.map(|a| a.port()),
            map_name: content.pack_name(&state.setup.level_id).unwrap_or_default().into(),
        }
    }

    /// The LAN broadcast (on the local port), and on Nostr every minute and
    /// at a change, once the reachability settled (so the first note has
    /// the address), at most [`SETTLE_WAIT`] after the start.
    fn announce(&mut self, state: &LobbyState, now: Instant) {
        let publishing = self.nostr.as_ref().is_some_and(|n| n.publisher.is_some());
        if self.announcer.is_none() && !publishing {
            return;
        }
        let hosting = self.hosting(state);
        let summary = GameSummary::of_lobby(state, &hosting);
        if let Some(a) = &mut self.announcer {
            a.tick(now, &hosting.lan(&summary));
        }
        let settled = self.settled(now);
        if let Some(n) = &mut self.nostr
            && let Some(publisher) = &mut n.publisher
            && settled
            && let Some(note) = publisher.tick(now, unix_now(), &summary, aux)
        {
            n.pool.publish(note);
        }
    }

    /// The relays' news for the log: each status (but connecting) and
    /// refusal of a note, once until it changes.
    fn relays(&mut self, log: &mut dyn Write) {
        let Some(n) = &mut self.nostr else { return };
        n.pool.poll(&mut n.events);
        let urls = &self.config.relays;
        for e in n.events.drain(..) {
            let (relay, said) = match e {
                PoolEvent::Status { relay, status: RelayStatus::Connected } => {
                    (relay, "connected".to_string())
                }
                PoolEvent::Status { relay, status: RelayStatus::Failed(why) | RelayStatus::Dropped(why) } => {
                    (relay, format!("unreachable: {why}"))
                }
                PoolEvent::Ok { relay, accepted: false, message, .. } => {
                    (relay, format!("refused the announcement: {message}"))
                }
                _ => continue,
            };
            let Some(last) = n.said.get_mut(relay) else { continue };
            if last.as_deref() != Some(said.as_str()) {
                let url = urls.get(relay).map_or("?", String::as_str);
                line(log, format_args!("relay {url}: {said}"));
                *last = Some(said);
            }
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
/// finished matches: `olr-cli`'s real-transport tests).
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

/// The game endpoint's ICE settings (N48, N51): the config's STUN servers,
/// port mapping for Public and Invite servers, `allow_local`; under the
/// test hooks the fake gateway and the pinned base.
fn ice(config: &Config) -> IceConfig {
    #[allow(unused_mut)]
    let mut ice = IceConfig {
        stun: config.stun.clone(),
        port_mapping: config.port_mapping && config.visibility != Visibility::Lan,
        allow_local: config.allow_local,
        ..IceConfig::default()
    };
    #[cfg(feature = "test-hooks")]
    if let Some(g) = &config.test_gateway {
        ice.base_override = Some(g.base);
        if let SocketAddr::V4(pmp) = g.pmp {
            ice.gateway = Some(pmp);
        }
        ice.ssdp = Some(g.ssdp);
        ice.mapping_lifetime = g.lifetime;
    }
    ice
}

/// Where the relay's mappings ask: under the test hooks the fake gateway
/// and the pinned base; else `None` (the OS's base and gateway, found on a
/// thread).
#[allow(clippy::unnecessary_wraps)]
fn map_target(config: &Config) -> Option<MapTarget> {
    #[cfg(feature = "test-hooks")]
    if let Some(g) = &config.test_gateway {
        let gateway = match g.pmp.ip() {
            IpAddr::V4(ip) => Some(ip),
            IpAddr::V6(_) => None,
        };
        return Some(MapTarget {
            base: g.base,
            gateway,
            pmp_port: Some(g.pmp.port()),
            ssdp: Some(g.ssdp),
            lifetime: g.lifetime,
        });
    }
    let _ = config;
    None
}

/// The server's download cache in `dir` (`<data dir>/downloads`, ruling
/// N30). One the server cannot open (another server on the same data
/// directory holds it: `InstallError::Locked`; or it cannot be made) is
/// a warning: the server runs without, still serving its own packs.
fn open_downloads(dir: &Path, log: &mut dyn Write) -> Result<Arc<Mutex<DownloadCache>>, String> {
    match DownloadCache::open(dir) {
        Ok(cache) => {
            line(log, format_args!("download cache {}", dir.display()));
            Ok(Arc::new(Mutex::new(cache)))
        }
        Err(e) => {
            let why = format!("{}: {e}", dir.display());
            line(
                log,
                format_args!("warning: no download cache ({why}); the server still serves its own packs"),
            );
            Err(why)
        }
    }
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
        Command::Invite => "/invite",
    }
}

fn setting_text(s: Setting) -> String {
    match s {
        Setting::TimeLimit(m) => format!("time_limit {m} min"),
        Setting::KillLimit(k) => format!("kill_limit {k}"),
        Setting::Lives(l) => format!("lives {l}"),
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

/// The wall-clock unix time now, read at each note: its `created_at`
/// (never anchored to the monotonic clock, which stands still while the
/// machine sleeps and ignores NTP steps; final review A-I2).
fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A signature's randomness (BIP-340 `aux`; zeros, still safe, when the
/// system has none).
fn aux() -> [u8; 32] {
    olr_net::random_bytes::<32>().unwrap_or([0; 32])
}

/// Who can take a match over, in words: "2 players can take over", "1
/// player and 1 spectator can take over", "nobody can take over".
fn succession_line(players: usize, spectators: usize) -> String {
    let count = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    let who = match (players, spectators) {
        (0, 0) => return "nobody can take over".into(),
        (p, 0) => count(p, "player"),
        (0, s) => count(s, "spectator"),
        (p, s) => format!("{} and {}", count(p, "player"), count(s, "spectator")),
    };
    format!("{who} can take over")
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

    #[test]
    fn the_succession_line_counts_players_and_spectators() {
        assert_eq!(succession_line(2, 0), "2 players can take over");
        assert_eq!(succession_line(1, 0), "1 player can take over");
        assert_eq!(succession_line(1, 1), "1 player and 1 spectator can take over");
        assert_eq!(succession_line(3, 2), "3 players and 2 spectators can take over");
        assert_eq!(succession_line(0, 2), "2 spectators can take over");
        assert_eq!(succession_line(0, 0), "nobody can take over");
    }
}
