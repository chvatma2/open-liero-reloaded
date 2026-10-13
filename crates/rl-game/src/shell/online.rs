//! The shell's online play (plan 10 task 12; see `shell.rs`'s module docs):
//! hosting, joining, the lobby's commands, the peer polled every frame and
//! its events.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Instant;

use rl_audio::Audio;
use rl_content::Content;
use rl_lobby::{HostConfig, Identity};
use rl_net::endpoint::EndpointConfig;
use rl_net::lan::{Announcer, Listener, broadcast_targets};
use rl_netcode::netsim::Profile;
use rl_online::matches::setup_packs;
use rl_online::{OnlineEvent, OnlinePeer, PeerContent, PeerRole};
use rl_proto::{LAN_PORT, LobbyState, Look, MatchStartWire};
use rl_sim::Keys;
use rl_ui::hud::{LineKind, LogLine};
use rl_ui::{HostPrefs, LobbyCommand, Password, Screen, UiAction, Visibility};

use super::{MAX_SPECTATORS, NET_SIM_BOTS, Outcome, Playing, Shell};
use crate::net::{self, Net, NetView};
use crate::online::OnlineDriver;
use crate::session::{Driver, Session};
use crate::setup::{self, DEFAULT_SKIN};

impl Shell {
    /// LAN announcements and the Multiplayer screen's LAN list (on by
    /// default; tests turn them off: they use the fixed LAN port).
    pub fn set_lan(&mut self, on: bool) {
        self.lan = on;
    }

    /// Hosts on, and joins an address without a port at, `port` instead of
    /// the options' (`--port`; 0 hosts on a free port).
    pub fn set_port(&mut self, port: Option<u16>) {
        self.port = port;
    }

    /// An online game (a lobby, its match, or `--net-sim`'s) exists: the
    /// app polls it every frame, and keeps doing so while the window is
    /// hidden (decision N21).
    pub fn online_active(&self) -> bool {
        self.net.is_some() || self.sim_host.is_some() || !self.replays.is_empty()
    }

    /// An online match plays (its session's `Driver::Online`): the app
    /// runs no local tick.
    pub fn plays_online(&self) -> bool {
        self.session().is_some_and(|s| matches!(s.driver, Driver::Online(_)))
    }

    /// The online game's port (a host's: the one to join).
    pub fn local_port(&self) -> Option<u16> {
        self.peer().map(OnlinePeer::local_port)
    }

    /// The network overlay's numbers while an online match plays (F3;
    /// spec §7.3).
    pub fn net_view(&self) -> Option<NetView> {
        let net = self.net.as_ref()?;
        let peer = self.peer()?;
        let s = peer.match_session()?;
        Some(NetView {
            stats: peer.stats(),
            rollback_last: net.meter.last_depth,
            rollback_p99: net.meter.p99(),
            frontier_lag: s.current_tick().saturating_sub(s.frontier()),
            lagging: net.lagging,
        })
    }

    /// The chat key (`settings.online.chat_key`) in an online match: opens
    /// the chat's field (spec §7.2), unless the match's menu or another
    /// screen shows or it is open. Whether it opened.
    pub fn open_chat(&mut self) -> bool {
        let can = self.plays_online()
            && self.ui.screen() == Screen::InGame
            && !self.ui.is_paused()
            && self.data.ingame_chat.is_none();
        if can {
            self.data.ingame_chat = Some(String::new());
        }
        can
    }

    /// The host says this client's inputs are late (spec §3.2): the HUD
    /// warns "Connection problem".
    pub fn lagging(&self) -> bool {
        self.net.as_ref().is_some_and(|n| n.lagging)
    }

    fn peer(&self) -> Option<&OnlinePeer> {
        if let Some(p) = self.net.as_ref().and_then(|n| n.peer.as_ref()) {
            return Some(p);
        }
        match &self.play.as_ref()?.session.driver {
            Driver::Online(d) => Some(d.peer()),
            _ => None,
        }
    }

    fn peer_mut(&mut self) -> Option<&mut OnlinePeer> {
        if self.net.as_ref().is_some_and(|n| n.peer.is_some()) {
            return self.net.as_mut().and_then(|n| n.peer.as_mut());
        }
        match &mut self.play.as_mut()?.session.driver {
            Driver::Online(d) => Some(d.peer_mut()),
            _ => None,
        }
    }

    /// `--host [name]`: hosts at once with the profile's look and the saved
    /// match (the Lobby opens). Nothing the flags change is saved.
    pub fn host_from_flag(&mut self, name: Option<String>) -> Vec<Outcome> {
        let mut prefs = self.ui.settings().online.host.clone();
        if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
            prefs.name = name;
        }
        self.without_saving(|sh| {
            let mut out = sh.act(UiAction::Open(Screen::Multiplayer));
            out.extend(sh.act(UiAction::Open(Screen::Host)));
            out.extend(sh.act(UiAction::Host(prefs)));
            out
        })
    }

    /// `--join <addr>`: joins at once (the Lobby opens once the host
    /// welcomes this player). The address is not remembered.
    pub fn join_from_flag(&mut self, addr: &str) -> Vec<Outcome> {
        let addr = addr.to_string();
        self.without_saving(|sh| {
            let mut out = sh.act(UiAction::Open(Screen::Multiplayer));
            out.extend(sh.act(UiAction::Join { addr, password: None }));
            out
        })
    }

    /// `--net-sim <profile>` (spec §9.10): a host in this process with
    /// [`NET_SIM_BOTS`] bots on the saved match (the host watches), joined
    /// as a client; every datagram both ways passes `profile`'s impaired
    /// network (`EndpointConfig::impair`), so one player feels it. The
    /// client readies itself and the host starts whenever every player is
    /// ready.
    pub fn start_net_sim(&mut self, profile: Profile) -> anyhow::Result<()> {
        self.leave_online();
        let content = self.open_content()?;
        let mut m = self.ui.settings().gameplay.match_prefs.clone();
        m.bots = NET_SIM_BOTS;
        m.per_bot = None;
        let (_, me) = self.me()?;
        let setup = net::setup_of(&m, setup_packs(&content, &m.mod_id, &m.level_id)?, me.colour);
        let cfg = HostConfig {
            name: format!("net-sim {}", profile.name),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins: Vec::new(),
        };
        let endpoint =
            EndpointConfig { port: 0, impair: Some(profile), impair_seed: 1, ..EndpointConfig::default() };
        let look = Look {
            name: "net-sim host".into(),
            colour: [128, 128, 128],
            skin: DEFAULT_SKIN.into(),
            skin_hash: rl_proto::NO_HASH.into(),
        };
        let mut host = OnlinePeer::start(
            PeerRole::Host(cfg),
            endpoint,
            fresh_identity()?,
            look,
            content,
            Some(setup),
            no_downloads(),
        )?;
        host.lobby().set_spectate(true);
        let port = host.local_port();
        println!("rl-game: net-sim {}: a host on port {port} with {NET_SIM_BOTS} bots", profile.name);
        self.sim_host = Some(host);
        self.impair = Some(profile);
        self.join_from_flag(&format!("127.0.0.1:{port}"));
        let net = self
            .net
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("{}", self.ui.message().unwrap_or("not joined")))?;
        net.auto_ready = true;
        Ok(())
    }

    /// `f` without the settings it changes being saved (the command
    /// line's online flags).
    fn without_saving(&mut self, f: impl FnOnce(&mut Shell) -> Vec<Outcome>) -> Vec<Outcome> {
        let (unsaved, online) = (self.unsaved_settings.take(), self.ui.settings().online.clone());
        let out = f(self);
        self.unsaved_settings = unsaved;
        self.ui.restore_online(online);
        out
    }

    /// The content for a peer: the menus' roots, opened again (the peer
    /// keeps its own).
    fn open_content(&self) -> anyhow::Result<Content> {
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        Content::open_layers(&roots)
    }

    /// This install's identity (loaded once; `identity.ron`) and the
    /// profile playing's look (its skin by pack id).
    fn me(&mut self) -> anyhow::Result<(Identity, Look)> {
        let id = match &self.identity {
            Some(id) => id.clone(),
            None => {
                let id = self.load_identity()?;
                self.identity = Some(id.clone());
                id
            }
        };
        let p = self.ui.profiles().selected().cloned().unwrap_or_default();
        let skin =
            self.content.as_ref().and_then(|c| c.skin_id(&p.skin_id)).unwrap_or_else(|| DEFAULT_SKIN.into());
        Ok((id, Look { name: p.name, colour: p.colour, skin, skin_hash: rl_proto::NO_HASH.into() }))
    }

    /// The identity file's key; a key the file holds that is no valid one
    /// gives a fresh key for this run (with a warning).
    fn load_identity(&mut self) -> anyhow::Result<Identity> {
        let fresh = rl_net::random_bytes::<32>()?;
        let loaded = self.store.load_identity(|| fresh);
        self.log(loaded.warnings);
        if let Some(id) = Identity::from_secret(loaded.value.0) {
            return Ok(id);
        }
        self.log(["the identity file's key is not a valid key: another one for this run".to_string()]);
        fresh_identity()
    }

    /// Hosting from the Host screen: the lobby for the saved match (the
    /// setup's bots may be none: the host adds them in the lobby).
    pub(super) fn start_host(&mut self, prefs: &HostPrefs) -> anyhow::Result<()> {
        self.leave_net();
        let (id, look) = self.me()?;
        let content = self.open_content()?;
        let m = &self.ui.settings().gameplay.match_prefs;
        let packs = setup_packs(&content, &m.mod_id, &m.level_id)?;
        let setup = net::setup_of(m, packs.clone(), look.colour);
        let key = (m.mod_id.clone(), m.level_id.clone());
        let name = if prefs.name.is_empty() { look.name.clone() } else { prefs.name.clone() };
        let cfg = HostConfig {
            name,
            password: (!prefs.password.is_empty()).then(|| prefs.password.as_str().to_string()),
            max_players: prefs.max_players,
            max_spectators: MAX_SPECTATORS,
            admins: Vec::new(),
        };
        let port = self.port.unwrap_or(prefs.port);
        let endpoint = EndpointConfig { port, impair: self.impair, ..EndpointConfig::default() };
        let peer =
            OnlinePeer::start(PeerRole::Host(cfg), endpoint, id, look, content, Some(setup), no_downloads())
                .map_err(|e| anyhow::anyhow!("cannot host on port {port}: {e}"))?;
        println!("rl-game: hosting on port {}", peer.local_port());
        let mut n = Net::new(peer, true);
        n.setup_packs = Some((key, packs));
        n.password = !prefs.password.is_empty();
        n.max_players = prefs.max_players;
        if self.lan && prefs.visibility == Visibility::Lan {
            match broadcast_targets().first().map(|t| Announcer::new(*t)) {
                Some(Ok(a)) => n.announcer = Some(a),
                Some(Err(e)) => self.log([format!("cannot announce the game on the LAN: {e}")]),
                None => {}
            }
        }
        self.net = Some(n);
        Ok(())
    }

    /// Joining from the Multiplayer screen: `addr` is `host[:port]` as
    /// typed (the port defaults to the options' or `--port`).
    pub(super) fn start_client(&mut self, addr: &str, password: Option<Password>) -> anyhow::Result<()> {
        self.leave_net();
        let default_port = self.port.filter(|&p| p != 0).unwrap_or(self.ui.settings().online.port);
        let to = resolve(addr, default_port)?;
        let (id, look) = self.me()?;
        let content = self.open_content()?;
        let role = PeerRole::Client { addr: to, password: password.map(|p| p.0) };
        let endpoint = EndpointConfig { port: 0, impair: self.impair, ..EndpointConfig::default() };
        let peer = OnlinePeer::start(role, endpoint, id, look, content, None, no_downloads())?;
        println!("rl-game: joining {to}");
        self.net = Some(Net::new(peer, false));
        Ok(())
    }

    /// The menus told: a host's lobby is up at once (`LobbyJoined`); a
    /// client waits for the host's welcome (`net_frame`); a failure
    /// returns with the reason (`OnlineFailed`).
    pub(super) fn connected(&mut self, started: anyhow::Result<bool>, out: &mut Vec<Outcome>) {
        let action = match started {
            Ok(false) => return,
            Ok(true) => {
                if let Some(n) = &mut self.net {
                    n.joined = true;
                }
                self.data.chat.clear();
                UiAction::LobbyJoined
            }
            Err(e) => {
                let why = format!("{e:#}");
                eprintln!("warning: online: {why}");
                UiAction::OnlineFailed(why)
            }
        };
        for c in self.ui.update(action) {
            self.carry(c, out);
        }
    }

    /// What the lobby's player asked (`LobbyCommand`); a refusal shows as
    /// the lobby's status.
    pub(super) fn lobby_command(&mut self, c: LobbyCommand) {
        let seed = match c {
            LobbyCommand::Start | LobbyCommand::ForceStart => self.ui.take_seed(),
            _ => 0,
        };
        let setup = match &c {
            LobbyCommand::Setup(p) => {
                let colour = self.ui.profiles().selected().map_or([255, 255, 255], |p| p.colour);
                Some(match (&self.content, &mut self.net) {
                    (Some(content), Some(n)) => {
                        n.packs(content, &p.mod_id, &p.level_id).map(|packs| net::setup_of(p, packs, colour))
                    }
                    _ => Err(anyhow::anyhow!("there is no content")),
                })
            }
            _ => None,
        };
        let Some(peer) = self.peer_mut() else { return };
        let mut lobby = peer.lobby();
        let state = lobby.state().cloned();
        let why = |e: rl_online::StartError| match &state {
            Some(s) => net::start_reason(&e, s),
            None => e.to_string(),
        };
        let result = match c {
            LobbyCommand::Ready(on) => {
                lobby.set_ready(on);
                Ok(())
            }
            LobbyCommand::Spectate(on) => {
                lobby.set_spectate(on);
                Ok(())
            }
            LobbyCommand::Chat(text) => {
                lobby.chat(&text);
                Ok(())
            }
            LobbyCommand::Setup(_) => match setup {
                Some(Ok(s)) => lobby.set_setup(s).map_err(|e| format!("the lobby refuses that setup: {e}")),
                Some(Err(e)) => Err(format!("{e:#}")),
                None => Ok(()),
            },
            LobbyCommand::Kick { member, ban } => {
                lobby.kick(member, ban);
                Ok(())
            }
            LobbyCommand::Move { member, spectator } => {
                lobby.move_seat(member, spectator);
                Ok(())
            }
            LobbyCommand::Start => lobby.start(seed).map_err(why),
            LobbyCommand::ForceStart => lobby.force_start(seed).map_err(why),
            LobbyCommand::Loadout(w) => {
                lobby.set_loadout(w);
                Ok(())
            }
        };
        self.data.online_status = result.err();
    }

    /// Leaves the online game (the lobby's Back, the match's Leave):
    /// [`Shell::leave_net`], and `--net-sim`'s host goes too.
    pub(super) fn leave_online(&mut self) {
        self.leave_net();
        if let Some(mut h) = self.sim_host.take() {
            h.leave();
        }
        self.impair = None;
    }

    /// Leaves the lobby this player is in (also before hosting or joining
    /// another): the match ends, the peer says goodbye.
    pub(super) fn leave_net(&mut self) {
        if self.plays_online() {
            self.end();
        }
        if let Some(p) = self.net.take().and_then(|n| n.peer) {
            self.release(p);
        }
        self.data.lobby = None;
        self.data.online_status = None;
        self.data.weapons = None;
    }

    /// Lets `peer` go: its goodbye, and the replay it is making kept (it is
    /// saved when done: [`Shell::poll_replays`]).
    fn release(&mut self, mut peer: OnlinePeer) {
        peer.leave();
        if let Some(r) = peer.take_replay() {
            self.replays.push(r);
        }
    }

    /// The replays of matches whose peer is gone, saved as they are done
    /// (with their desync bundles and notices).
    fn poll_replays(&mut self) {
        let mut done = Vec::new();
        self.replays.retain(|r| match r.poll() {
            Some(events) => {
                done.extend(events);
                false
            }
            None => true,
        });
        for e in done {
            match e {
                OnlineEvent::ReplayReady(bytes) => self.save_online_replay(&bytes),
                OnlineEvent::DesyncBundle(bytes) => self.save_bundle(&bytes),
                OnlineEvent::Notice(text) => self.log([text]),
                _ => {}
            }
        }
    }

    /// At exit: the replays still being made, waited for at most `limit`.
    pub(super) fn wait_for_replays(&mut self, limit: std::time::Duration) {
        let until = Instant::now() + limit;
        self.poll_replays();
        while !self.replays.is_empty() && Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.poll_replays();
        }
        if !self.replays.is_empty() {
            eprintln!("warning: quitting while the last match's replay is made: it is not saved");
        }
    }

    /// The connection is gone (`Disconnected`, `Kicked`, `Refused`): the
    /// match view goes at once (the peer dropped its match: the session
    /// would draw its start world), then the menus return to Multiplayer
    /// with `why`.
    fn lost(&mut self, why: String, out: &mut Vec<Outcome>) {
        eprintln!("rl-game: online: {why}");
        self.end();
        if let Some(p) = self.net.take().and_then(|n| n.peer) {
            self.release(p);
        }
        if let Some(mut h) = self.sim_host.take() {
            h.leave();
        }
        self.impair = None;
        self.data.lobby = None;
        self.data.weapons = None;
        for c in self.ui.update(UiAction::OnlineFailed(why)) {
            self.carry(c, out);
        }
    }

    /// One frame of online play at `now` (the app's loop, and its hidden
    /// window's frames: decision N21): the LAN list while the Multiplayer
    /// screen shows, `--net-sim`'s host, then the peer polled once (through
    /// the match's session while one plays, `Session::frame`, the player's
    /// keys from `keys`: none while the match's menu, the chat or another
    /// screen has them) and its events carried out.
    pub fn net_frame(&mut self, now: Instant, keys: &mut dyn FnMut() -> Keys) -> Vec<Outcome> {
        let mut out = Vec::new();
        self.poll_replays();
        self.poll_lan(now);
        self.poll_sim_host(now);
        if self.net.is_none() {
            return out;
        }
        let held = self.paused || self.ui.screen() != Screen::InGame || self.data.ingame_chat.is_some();
        let mut none = || Keys(0);
        let keys: &mut dyn FnMut() -> Keys = if held { &mut none } else { keys };
        let events = match &mut self.play {
            Some(p) if matches!(p.session.driver, Driver::Online(_)) => p.session.frame(now, keys),
            _ => match self.net.as_mut().and_then(|n| n.peer.as_mut()) {
                Some(peer) => peer.poll(now, &mut || Keys(0), &mut |_, _, _| {}),
                None => Vec::new(),
            },
        };
        self.on_net_events(events, &mut out);
        let Some(n) = &mut self.net else { return out };
        if let Some((a, info)) = n.announcer.as_mut().zip(n.announce.as_ref()) {
            a.tick(now, info);
        }
        let hist = match &self.play {
            Some(p) => match &p.session.driver {
                Driver::Online(d) => d.peer().match_session().map(|s| s.stats().depth_hist),
                _ => None,
            },
            None => None,
        };
        if let Some(h) = hist {
            n.meter.sample(now, &h);
        }
        out
    }

    /// The poll's events, in order.
    fn on_net_events(&mut self, events: Vec<OnlineEvent>, out: &mut Vec<Outcome>) {
        let mut queue: VecDeque<OnlineEvent> = events.into();
        while let Some(e) = queue.pop_front() {
            match e {
                OnlineEvent::Lobby(state) => self.show_lobby(&state, out),
                OnlineEvent::Chat { from, text, .. } => {
                    self.chat_line(LogLine { text: format!("{from}: {text}"), kind: LineKind::Chat });
                }
                OnlineEvent::Notice(text) => self.chat_line(LogLine::event(text)),
                OnlineEvent::Left(name) => self.chat_line(LogLine::event(format!("{name} left"))),
                OnlineEvent::MatchStarting { start, my_slot } => {
                    let after: Vec<OnlineEvent> = queue.drain(..).collect();
                    let rest = self.start_online(start, my_slot, after, out);
                    queue.extend(rest);
                }
                OnlineEvent::MatchEnded(_) => {
                    // The host's end: the results show now if they did not
                    // already (the confirmed end's).
                    if self.plays_online() && !self.over_reported && self.ui.screen() == Screen::InGame {
                        let o = self.report_over();
                        out.extend(o);
                    }
                }
                OnlineEvent::Lagging(on) => {
                    if let Some(n) = &mut self.net {
                        n.lagging = on;
                    }
                }
                OnlineEvent::Kicked { banned } => {
                    let why = if banned { "The host banned you." } else { "The host kicked you." };
                    return self.lost(why.into(), out);
                }
                OnlineEvent::Refused(why) | OnlineEvent::Disconnected(why) => return self.lost(why, out),
                OnlineEvent::ReplayReady(bytes) => self.save_online_replay(&bytes),
                OnlineEvent::DesyncBundle(bytes) => self.save_bundle(&bytes),
                // The lobby's download rows and prompt come with Task 7 (this
                // game has no download cache yet).
                OnlineEvent::Downloads(_) | OnlineEvent::DownloadPrompt { .. } => {}
                // The match's presentation, polled without a match view (a
                // view that failed to start): nothing to show it on.
                OnlineEvent::StateLoaded { .. }
                | OnlineEvent::Restored(_)
                | OnlineEvent::Ticked(_)
                | OnlineEvent::Confirmed(_) => {}
            }
        }
    }

    /// The lobby changed: its view (and the client's welcome: the Lobby
    /// opens), the packs a client lacks told to the host, the weapon
    /// screen's menu for its mod, the LAN announcement.
    fn show_lobby(&mut self, state: &LobbyState, out: &mut Vec<Outcome>) {
        let Some(peer) = self.peer_mut() else { return };
        let port = peer.local_port();
        let lobby = peer.lobby();
        let (me, can) = (lobby.me(), lobby.can_start());
        let Some(n) = &mut self.net else { return };
        let missing = match &self.content {
            Some(c) if !n.host && n.checked_packs.as_ref() != Some(&state.setup.packs) => {
                n.checked_packs = Some(state.setup.packs.clone());
                Some(net::missing_packs(c, &state.setup))
            }
            _ => None,
        };
        let key = (state.setup.mod_id.clone(), state.setup.restrictions.clone());
        if n.weapons.as_ref().is_none_or(|w| w.0 != key) {
            let menu = self.content.as_ref().and_then(|c| net::weapon_menu(c, &key.0, &key.1));
            n.weapons = Some((key, menu));
        }
        let host = n.host;
        if host {
            n.announce = Some(net::announcement(state, port, n.max_players, n.password));
        }
        let can_start = if host {
            can.map_err(|e| net::start_reason(&e, state))
        } else {
            Err("Waiting for the host".into())
        };
        let weapons = n.weapons.as_ref().and_then(|w| w.1.clone());
        let welcomed = !n.joined && me.is_some();
        n.joined |= welcomed;
        // `--net-sim`: ready whenever the lobby gathers (after each match
        // everyone readies again).
        let mine = state.members.iter().find(|m| Some(m.id) == me);
        let auto_ready = n.auto_ready
            && state.phase == rl_proto::Phase::Gathering
            && mine.is_some_and(|m| !m.ready && matches!(m.seat, rl_proto::SeatWire::Player { .. }));
        if let Some(missing) = missing
            && let Some(peer) = self.peer_mut()
        {
            peer.lobby().have(missing);
        }
        if auto_ready && let Some(peer) = self.peer_mut() {
            peer.lobby().set_ready(true);
        }
        if self.play.is_none() {
            self.data.weapons = weapons;
        }
        self.data.lobby = me.map(|me| net::lobby_view(state, me, host, can_start));
        if welcomed {
            self.data.chat.clear();
            for c in self.ui.update(UiAction::LobbyJoined) {
                self.carry(c, out);
            }
        }
    }

    /// A line of the lobby's chat: in the lobby's log and, in game, the
    /// HUD's (spec §7.2).
    fn chat_line(&mut self, line: LogLine) {
        if let Some(s) = self.session_mut() {
            s.log.push(line.clone());
        }
        self.data.chat.push(line);
        let excess = self.data.chat.len().saturating_sub(net::CHAT_KEEP);
        self.data.chat.drain(..excess);
    }

    /// The host started a match (`MatchStarting`; the events after it in
    /// that poll are its): the last match's view goes, the match's assets
    /// load (mod, level, theme, every slot's skin) and its session starts
    /// with `Driver::Online`; the menus go in game. Returns the events
    /// that are not the match's presentation.
    fn start_online(
        &mut self,
        start: MatchStartWire,
        my_slot: Option<u8>,
        after: Vec<OnlineEvent>,
        out: &mut Vec<Outcome>,
    ) -> Vec<OnlineEvent> {
        if self.play.is_some() {
            self.end();
        }
        let Some(peer) = self.net.as_mut().and_then(|n| n.peer.take()) else { return after };
        self.launches += 1;
        let roots = self.roots();
        let root: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let mut assets = match setup::load_online(&root, &start, my_slot) {
            Ok(a) => a,
            Err(e) => {
                self.release(peer);
                self.lost(format!("cannot show the match: {e:#}"), out);
                return Vec::new();
            }
        };
        self.log(std::mem::take(&mut assets.warnings));
        let driver = OnlineDriver::new(peer, &start);
        let made =
            Session::try_online(&assets, driver, self.cfg.viewport, self.cfg.interpolation, Audio::silent());
        let mut session = match made {
            Ok(s) => s,
            Err((e, driver)) => {
                self.release(driver.into_peer());
                self.lost(format!("cannot show the match: {e:#}"), out);
                return Vec::new();
            }
        };
        let mut audio = self.audio.take().unwrap_or_else(Audio::silent);
        audio.stop_all();
        audio.load(&assets.m, assets.content.core());
        self.log(audio.take_warnings());
        session.audio = audio;
        let rest = session.present_events(after);
        self.play = Some(Playing { assets, session });
        self.paused = false;
        self.over_reported = false;
        self.data.results = None;
        if let Some(n) = &mut self.net {
            n.lagging = false;
            n.meter.clear();
        }
        let watch = if my_slot.is_some() { "plays" } else { "watches" };
        println!("rl-game: online match seed {}: this player {watch}", start.seed);
        out.extend(self.act(UiAction::MatchStarted));
        out.push(Outcome::Started);
        rest
    }

    /// A finished online match's replay, with the automatic ones.
    fn save_online_replay(&mut self, bytes: &[u8]) {
        let Some(dir) = self.store.replays_dir() else { return };
        match rl_ui::replays::auto_save(&dir, bytes) {
            Ok(path) => println!("rl-game: saved replay {}", path.display()),
            Err(e) => eprintln!("warning: saving the replay in {}: {e}", dir.display()),
        }
    }

    /// A desync's bundle (spec §9.10), in the data directory's `desyncs/`.
    fn save_bundle(&mut self, bytes: &[u8]) {
        let Some(dir) = self.store.dir().map(|d| d.join(net::DESYNCS_DIR)) else {
            eprintln!("warning: a desync happened; without a data directory its bundle is not saved");
            return;
        };
        let secs =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        match net::save_bundle(&dir, bytes, secs) {
            Ok(path) => eprintln!("warning: a desync happened: its bundle is {}", path.display()),
            Err(e) => eprintln!("warning: saving a desync bundle in {}: {e}", dir.display()),
        }
    }

    /// The Multiplayer screen's LAN list, while it shows.
    fn poll_lan(&mut self, now: Instant) {
        if !self.lan || self.ui.screen() != Screen::Multiplayer {
            self.lan_listener = None;
            self.lan_failed = false;
            return;
        }
        if self.lan_listener.is_none() && !self.lan_failed {
            match Listener::new(LAN_PORT) {
                Ok(l) => self.lan_listener = Some(l),
                Err(e) => {
                    self.lan_failed = true;
                    self.log([format!("cannot listen for LAN games on port {LAN_PORT}: {e}")]);
                }
            }
        }
        let Some(l) = &mut self.lan_listener else { return };
        let catalog = &self.data.catalog;
        let level = |id: &str| catalog.level(id).map(|l| l.name.clone());
        self.data.lan_games = l
            .poll(now)
            .iter()
            .filter(|g| g.info.fingerprint == rl_proto::FINGERPRINT)
            .map(|g| net::lan_entry(g, level))
            .collect();
    }

    /// `--net-sim`'s host: polled; it starts whenever every player is ready.
    fn poll_sim_host(&mut self, now: Instant) {
        let Some(h) = &mut self.sim_host else { return };
        for e in h.poll(now, &mut || Keys(0), &mut |_, _, _| {}) {
            if let OnlineEvent::DesyncBundle(b) = e {
                self.save_bundle(&b);
            }
        }
        if self.sim_host.as_mut().is_some_and(|h| h.lobby().can_start().is_ok()) {
            let seed = self.ui.take_seed();
            if let Some(Err(e)) = self.sim_host.as_mut().map(|h| h.lobby().start(seed)) {
                self.log([format!("net-sim: the host cannot start: {e}")]);
            }
        }
    }
}

/// A fresh identity (a key the store does not keep).
/// Downloads are wired into the game by plan 11's Task 7 (the cache in
/// the store's cache directory); until then a peer has none.
fn no_downloads() -> PeerContent {
    PeerContent::without_downloads("this game has no download cache yet")
}

fn fresh_identity() -> anyhow::Result<Identity> {
    for _ in 0..8 {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>()?) {
            return Ok(id);
        }
    }
    anyhow::bail!("no valid key from the system's randomness")
}

/// Why an IPv6 address is not joined: the host listens on IPv4 only and
/// the client's UDP socket is IPv4.
pub const IPV6_UNSUPPORTED: &str = "IPv6 is not supported yet";

/// The address `typed` (`host[:port]`, `default_port` when it has none); a
/// name is looked up and its first IPv4 address taken. An IPv6 literal
/// (`::1`, `[::1]:23400`) or a name with IPv6 addresses only is refused
/// ([`IPV6_UNSUPPORTED`]).
pub fn resolve(typed: &str, default_port: u16) -> anyhow::Result<SocketAddr> {
    let typed = typed.trim();
    let literal = typed.parse::<SocketAddr>().ok().or_else(|| {
        typed
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .ok()
            .map(|ip| SocketAddr::new(ip, default_port))
    });
    if let Some(a) = literal {
        return choose_address(typed, [a]);
    }
    let with_port = if typed.contains(':') { typed.to_string() } else { format!("{typed}:{default_port}") };
    let addrs = with_port.to_socket_addrs().map_err(|e| anyhow::anyhow!("{typed}: {e}"))?;
    choose_address(typed, addrs)
}

/// The address to join among those `typed` resolved to: the first IPv4
/// one; IPv6 alone is refused ([`IPV6_UNSUPPORTED`]).
pub fn choose_address(
    typed: &str,
    addrs: impl IntoIterator<Item = SocketAddr>,
) -> anyhow::Result<SocketAddr> {
    let mut ipv6 = false;
    for a in addrs {
        match a {
            SocketAddr::V4(_) => return Ok(a),
            SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
                Some(v4) => return Ok(SocketAddr::new(v4.into(), a.port())),
                None => ipv6 = true,
            },
        }
    }
    if ipv6 {
        anyhow::bail!("{typed}: {IPV6_UNSUPPORTED}")
    }
    anyhow::bail!("{typed}: no address")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addrs(list: &[&str]) -> Vec<SocketAddr> {
        list.iter().map(|a| a.parse().unwrap()).collect()
    }

    #[test]
    fn a_name_joins_its_ipv4_address_first() {
        let got =
            choose_address("host", addrs(&["[2001:db8::1]:23400", "192.0.2.7:23400", "192.0.2.8:23400"]));
        assert_eq!(got.unwrap(), "192.0.2.7:23400".parse().unwrap());
        let mapped = choose_address("host", addrs(&["[::ffff:192.0.2.9]:5"]));
        assert_eq!(mapped.unwrap(), "192.0.2.9:5".parse().unwrap(), "an IPv4-mapped address is IPv4");
    }

    #[test]
    fn an_ipv6_only_name_is_refused_clearly() {
        let err = choose_address("host", addrs(&["[2001:db8::1]:23400"])).unwrap_err();
        assert_eq!(err.to_string(), "host: IPv6 is not supported yet");
        let err = choose_address("host", addrs(&[])).unwrap_err();
        assert_eq!(err.to_string(), "host: no address");
    }

    #[test]
    fn an_ipv6_literal_is_refused_clearly() {
        for typed in ["::1", "[::1]", "[2001:db8::1]:23400"] {
            let err = resolve(typed, 23400).unwrap_err();
            assert_eq!(err.to_string(), format!("{typed}: IPv6 is not supported yet"));
        }
        assert_eq!(resolve("127.0.0.1", 23400).unwrap(), "127.0.0.1:23400".parse().unwrap());
        assert_eq!(resolve(" 127.0.0.1:99 ", 23400).unwrap(), "127.0.0.1:99".parse().unwrap());
        assert_eq!(resolve("localhost:5", 23400).unwrap(), "127.0.0.1:5".parse().unwrap());
    }
}
