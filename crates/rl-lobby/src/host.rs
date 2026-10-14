//! The lobby's host (networking spec §4, §5.2, §5.4, §7.2): admits peers
//! that prove their key, seats players, spectators and late joiners, relays
//! chat, kicks and bans, and turns the lobby into a match start. Sans-IO:
//! the transport's events and the time come in, messages and events go out.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::net::IpAddr;

use rl_netcode::{PeerId, input_delay};
use rl_proto::{
    AdminCommand, LobbyState, Look, MAX_INPUT_DELAY, MAX_PLAYERS, MAX_SPECTATORS, MatchEndWire,
    MatchSetupWire, MatchStartWire, Member, Notice, Path, Phase, PublicKey, Refusal, SeatWire, SlotWire,
    ToClient, ToHost, valid_setup,
};

use crate::identity::{
    DtlsFingerprints, Identity, challenge_message, host_proof_message, password_proof, verify,
};
use crate::model::*;

/// A peer whose transport is open but who has not proven its key yet.
struct Pending {
    addr: IpAddr,
    /// How the transport reaches the joiner (`Member::path`).
    path: Path,
    nonce: [u8; 32],
    /// The link's DTLS fingerprints as the host saw them (both proofs sign them).
    fingerprints: DtlsFingerprints,
    since_us: u64,
    hello: Option<(PublicKey, Look)>,
}

enum Link {
    /// Boxed: a pending link carries its nonce and fingerprints.
    Pending(Box<Pending>),
    Member(u8),
}

/// A member as the host keeps it.
struct Rec {
    id: u8,
    /// `None`: the host's own player.
    peer: Option<PeerId>,
    addr: Option<IpAddr>,
    /// How its link reaches the host (`None`: the host's own player).
    path: Option<Path>,
    key: PublicKey,
    look: Look,
    seat: SeatWire,
    /// Wants a player seat (false: spectates by choice).
    wants_play: bool,
    /// Said it is ready; ready on the wire only with its content known
    /// complete.
    said_ready: bool,
    missing: Vec<String>,
    /// The content list its last `Have` answered.
    have_revision: Option<u32>,
    /// Its last `Have` answered the host's current content list (always,
    /// without one: `LobbyHost::set_content_revision` was never called;
    /// the host's own player always).
    known: bool,
    /// The download progress shown (`Member::download`).
    download: Option<u8>,
    /// A progress held back by the coalescing, shown at `download_at +
    /// DOWNLOAD_EVERY_US` (`LobbyHost::tick`).
    download_next: Option<Option<u8>>,
    /// When `download` last changed.
    download_at: Option<u64>,
    /// Sits a match out (waiting seat) and its content is not yet known to
    /// be complete: `HostEvent::ContentComplete` fires when it is.
    awaiting_content: bool,
    loadout: Option<[i32; 5]>,
    admin: bool,
    /// The round trip the transport measured (`set_ping`).
    ping_ms: Option<u16>,
    /// When its last chat lines were relayed (at most `CHAT_LINES`).
    chat: VecDeque<u64>,
    /// When its last look changes were taken (at most `LOOK_CHANGES`).
    looks: VecDeque<u64>,
    /// Its latest look beyond them, taken by `tick` once the window
    /// allows (the last of a dragged colour slider is the one chosen).
    look_held: Option<Look>,
}

impl Rec {
    /// Whether a look change may be taken at `now_us` (the window's
    /// changes are counted when it is).
    fn look_change_allowed(&mut self, now_us: u64) -> bool {
        while self.looks.front().is_some_and(|&t| now_us.saturating_sub(t) >= LOOK_WINDOW_US) {
            self.looks.pop_front();
        }
        if self.looks.len() >= LOOK_CHANGES {
            return false;
        }
        self.looks.push_back(now_us);
        true
    }
}

impl Rec {
    /// Its content is not known complete: its `Have` for the current list
    /// is missing, or named something.
    fn lacking(&self) -> bool {
        !self.known || !self.missing.is_empty()
    }

    fn ready(&self) -> bool {
        self.said_ready && !self.lacking()
    }

    fn wire(&self) -> Member {
        Member {
            id: self.id,
            key: self.key,
            look: self.look.clone(),
            seat: self.seat,
            ready: self.ready(),
            missing: self.missing.clone(),
            ping_ms: self.ping_ms,
            admin: self.admin,
            download: self.download,
            path: self.path,
        }
    }
}

/// How a member leaves.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gone {
    Left,
    Kicked { ban: Ban },
}

/// What a kick bans (spec §5.2: by key, optionally the address).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ban {
    No,
    Key,
    KeyAndAddress,
}

/// The lobby's host.
pub struct LobbyHost {
    cfg: HostConfig,
    key: Identity,
    /// The host's own member id (`None`: a dedicated server).
    me: Option<u8>,
    links: BTreeMap<PeerId, Link>,
    /// Members in join order.
    members: Vec<Rec>,
    phase: Phase,
    setup: MatchSetupWire,
    state: LobbyState,
    banned_keys: BTreeSet<PublicKey>,
    banned_ips: BTreeSet<IpAddr>,
    next_id: u8,
    /// Member ids in the announced match: never given to a new member
    /// while it runs.
    in_match: BTreeSet<u8>,
    results_since: u64,
    startable: bool,
    /// The state changed since the last `Lobby` went out: one goes out at
    /// the next `outgoing()` (or before a `Joined` notice).
    dirty: bool,
    /// Counts the changes a start depends on; `announce_start` takes only
    /// a start made at the current generation.
    generation: u64,
    started_at: Option<u64>,
    /// Counts the changes of `state` ([`LobbyHost::revision`]).
    revision: u64,
    /// Wrong password proofs by address ([`PASSWORD_TRIES`]).
    guesses: BTreeMap<IpAddr, Guesses>,
    out: Vec<(PeerId, ToClient)>,
    events: Vec<HostEvent>,
    /// The latest time the host was given.
    now_us: u64,
    /// The content list's revision (`set_content_revision`; `None`: no
    /// content layer, every `Have` counts).
    content_revision: Option<u32>,
}

/// One address's recent wrong password proofs.
#[derive(Default)]
struct Guesses {
    /// When the wrong proofs within the last [`PASSWORD_WINDOW_US`] came.
    at: Vec<u64>,
    /// Joins are refused until then.
    held_until: Option<u64>,
}

impl Guesses {
    fn held(&self, now_us: u64) -> bool {
        self.held_until.is_some_and(|t| now_us < t)
    }

    /// The last time this entry matters to.
    fn last(&self) -> u64 {
        self.at.last().copied().unwrap_or(0).max(self.held_until.unwrap_or(0))
    }
}

impl LobbyHost {
    /// A lobby gathering for `setup`. `key` is the host's identity (a
    /// dedicated server has its own); `me` is the host player's look, `None`
    /// for a dedicated server. The setup must be valid for the wire.
    pub fn new(
        cfg: HostConfig,
        key: Identity,
        me: Option<Look>,
        setup: MatchSetupWire,
    ) -> Result<LobbyHost, LobbyError> {
        if !valid_setup(&setup) {
            return Err(LobbyError::InvalidSetup);
        }
        let cfg = HostConfig {
            name: sanitize_name(&cfg.name, "Lobby"),
            max_players: cfg.max_players.clamp(1, MAX_PLAYERS as u8),
            max_spectators: cfg.max_spectators.min(MAX_SPECTATORS as u8),
            ..cfg
        };
        let state = LobbyState {
            name: cfg.name.clone(),
            phase: Phase::Gathering,
            setup: setup.clone(),
            members: Vec::new(),
        };
        let mut host = LobbyHost {
            cfg,
            key,
            me: None,
            links: BTreeMap::new(),
            members: Vec::new(),
            phase: Phase::Gathering,
            setup,
            state,
            banned_keys: BTreeSet::new(),
            banned_ips: BTreeSet::new(),
            next_id: 0,
            in_match: BTreeSet::new(),
            results_since: 0,
            startable: false,
            dirty: false,
            generation: 0,
            started_at: None,
            revision: 0,
            guesses: BTreeMap::new(),
            out: Vec::new(),
            events: Vec::new(),
            now_us: 0,
            content_revision: None,
        };
        if let Some(look) = me {
            let id = host.alloc_id().unwrap_or(0);
            host.members.push(Rec {
                id,
                peer: None,
                addr: None,
                path: None,
                key: host.key.public(),
                look: sanitize_look(&look),
                seat: SeatWire::Player { slot: 0 },
                wants_play: true,
                said_ready: true,
                missing: Vec::new(),
                have_revision: None,
                known: true,
                download: None,
                download_next: None,
                download_at: None,
                awaiting_content: false,
                loadout: None,
                admin: true,
                ping_ms: None,
                chat: VecDeque::new(),
                looks: VecDeque::new(),
                look_held: None,
            });
            host.me = Some(id);
        }
        host.refresh();
        Ok(host)
    }

    // ---- the transport ----

    /// The transport opened `peer` from `addr`, reaching it by `path`
    /// (shown as `Member::path`): send the challenge (or refuse a banned
    /// address). `nonce` must be fresh randomness;
    /// `fingerprints` are the link's DTLS fingerprints, which the joiner's
    /// proof and the host's (`HostProof`) sign.
    pub fn connected(
        &mut self,
        peer: PeerId,
        addr: IpAddr,
        path: Path,
        now_us: u64,
        nonce: [u8; 32],
        fingerprints: DtlsFingerprints,
    ) {
        self.now_us = self.now_us.max(now_us);
        if self.links.contains_key(&peer) {
            self.disconnected(peer);
        }
        if self.banned_ips.contains(&addr) {
            self.refuse(peer, Refusal::Banned);
            return;
        }
        if self.held(addr, now_us) {
            self.refuse(peer, Refusal::Throttled);
            return;
        }
        let pending = Pending { addr, path, nonce, fingerprints, since_us: now_us, hello: None };
        self.links.insert(peer, Link::Pending(Box::new(pending)));
        let challenge = ToClient::Challenge {
            nonce,
            host_key: self.key.public(),
            needs_password: self.cfg.password.is_some(),
        };
        self.out.push((peer, challenge));
    }

    /// A message from `peer` (decoded and validated by rl-proto).
    pub fn receive(&mut self, peer: PeerId, msg: ToHost, now_us: u64) {
        self.now_us = self.now_us.max(now_us);
        match self.links.get(&peer) {
            None => {}
            Some(Link::Pending(_)) => self.receive_pending(peer, msg, now_us),
            Some(&Link::Member(id)) => self.receive_member(peer, id, msg, now_us),
        }
    }

    /// The transport to `peer` closed. A no-op for a peer already gone.
    pub fn disconnected(&mut self, peer: PeerId) {
        if let Some(Link::Member(id)) = self.links.remove(&peer) {
            self.drop_member(id, Gone::Left);
        }
    }

    /// Timeouts: unproven peers after `PROOF_TIMEOUT_US`, the results after
    /// `RESULTS_US`.
    pub fn tick(&mut self, now_us: u64) {
        self.now_us = self.now_us.max(now_us);
        self.flush_downloads();
        let late: Vec<PeerId> = self
            .links
            .iter()
            .filter_map(|(&peer, link)| match link {
                Link::Pending(p) if now_us.saturating_sub(p.since_us) >= PROOF_TIMEOUT_US => Some(peer),
                _ => None,
            })
            .collect();
        for peer in late {
            self.links.remove(&peer);
            self.events.push(HostEvent::Kick { peer, ban: false });
        }
        if self.phase == Phase::Results && now_us.saturating_sub(self.results_since) >= RESULTS_US {
            self.gather();
        }
        // Held looks, once their member's window allows.
        let mut taken = Vec::new();
        for m in &mut self.members {
            if m.look_held.is_some() && m.look_change_allowed(now_us) {
                taken.extend(m.look_held.take().map(|look| (m.id, look)));
            }
        }
        for (id, look) in taken {
            self.set_look(id, &look);
            self.publish();
        }
    }

    /// Drains the messages to send, ending with one `Lobby` to every
    /// member if the state changed since the last one.
    pub fn outgoing(&mut self) -> Vec<(PeerId, ToClient)> {
        self.flush();
        core::mem::take(&mut self.out)
    }

    /// The round trip the transport measured to `peer`, shown in the lobby
    /// (call it every few seconds, not per packet: a change is a `Lobby`).
    pub fn set_ping(&mut self, peer: PeerId, ms: u32) {
        let Some(id) = self.member_of(peer) else { return };
        let Some(i) = self.index(id) else { return };
        let ping = Some(u16::try_from(ms).unwrap_or(u16::MAX));
        if self.members[i].ping_ms != ping {
            self.members[i].ping_ms = ping;
            // Not a change a start depends on: no new generation.
            self.refresh();
            self.dirty = true;
        }
    }

    /// The share of `member`'s required content received, while it
    /// downloads (`None`: complete or not started), shown as
    /// `Member::download` (cut to 100). Coalesced: a member's progress
    /// changes at most once per [`DOWNLOAD_EVERY_US`] of the time the host
    /// was last given; the latest value waits for a `tick`. Not a change a
    /// start depends on.
    pub fn set_download(&mut self, member: u8, percent: Option<u8>) {
        let Some(i) = self.index(member) else { return };
        let percent = percent.map(|p| p.min(100));
        let m = &mut self.members[i];
        if percent == m.download {
            m.download_next = None;
        } else if m.download_at.is_none_or(|t| self.now_us.saturating_sub(t) >= DOWNLOAD_EVERY_US) {
            m.download_next = None;
            m.download = percent;
            m.download_at = Some(self.now_us);
            self.refresh();
            self.dirty = true;
        } else {
            m.download_next = Some(percent);
        }
    }

    /// Shows the progress held back for [`DOWNLOAD_EVERY_US`] once due.
    fn flush_downloads(&mut self) {
        let now = self.now_us;
        let mut changed = false;
        for m in &mut self.members {
            let due = m.download_at.is_none_or(|t| now.saturating_sub(t) >= DOWNLOAD_EVERY_US);
            if due
                && let Some(next) = m.download_next.take()
                && next != m.download
            {
                m.download = next;
                m.download_at = Some(now);
                changed = true;
            }
        }
        if changed {
            self.refresh();
            self.dirty = true;
        }
    }

    /// The members whose required content is incomplete (their last
    /// `Have` named something missing), in join order.
    pub fn lacking(&self) -> Vec<u8> {
        self.members.iter().filter(|m| m.lacking()).map(|m| m.id).collect()
    }

    /// The host's content list is now `revision` (the online layer's
    /// `ContentMsg::List`): a member is lacking until its `Have` answers
    /// it, so it is not ready and sits a start out meanwhile. A change a
    /// start depends on.
    pub fn set_content_revision(&mut self, revision: u32) {
        if self.content_revision == Some(revision) {
            return;
        }
        self.content_revision = Some(revision);
        for m in self.members.iter_mut().filter(|m| m.peer.is_some()) {
            m.known = m.have_revision == Some(revision);
        }
        self.publish();
    }

    /// Changes whenever [`LobbyHost::state`] does: compare it instead of
    /// the state.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Drains the events.
    pub fn events(&mut self) -> Vec<HostEvent> {
        core::mem::take(&mut self.events)
    }

    // ---- the host player's own actions ----

    /// The next match's setup. A change clears every other member's ready
    /// (they re-confirm what they agreed to); the same setup changes nothing.
    /// Refused outside gathering (`NotGathering`): a running match's setup
    /// stays what the lobby shows.
    pub fn set_setup(&mut self, setup: MatchSetupWire) -> Result<(), LobbyError> {
        if self.phase != Phase::Gathering {
            return Err(LobbyError::NotGathering);
        }
        if !valid_setup(&setup) {
            return Err(LobbyError::InvalidSetup);
        }
        if setup != self.setup {
            // Only the bots changed (a server's fill bots, Add/Remove bot):
            // the players' ready marks stay.
            let bots_only = MatchSetupWire { bots: setup.bots.clone(), ..self.setup.clone() } == setup;
            self.setup = setup;
            if !bots_only {
                let me = self.me;
                for m in self.members.iter_mut().filter(|m| Some(m.id) != me) {
                    m.said_ready = false;
                }
            }
            self.publish();
        }
        Ok(())
    }

    pub fn set_my_look(&mut self, look: Look) {
        if let Some(me) = self.me {
            self.set_look(me, &look);
            self.publish();
        }
    }

    /// The host is ready from the start and stays so unless it says otherwise.
    pub fn set_my_ready(&mut self, ready: bool) {
        if let Some(i) = self.me.and_then(|me| self.index(me)) {
            self.members[i].said_ready = ready;
            self.publish();
        }
    }

    pub fn set_my_loadout(&mut self, loadout: Option<[i32; 5]>) {
        if let Some(i) = self.me.and_then(|me| self.index(me)) {
            self.members[i].loadout = loadout;
            self.generation += 1;
        }
    }

    /// The host's chat line (cut to 200 characters); a dedicated server's
    /// is a notice.
    pub fn chat(&mut self, text: &str) {
        let Some(text) = sanitize_chat(text) else { return };
        match self.me {
            Some(me) => self.relay(me, text),
            None => self.broadcast(&ToClient::Notice(Notice::Message(text))),
        }
    }

    /// Kicks a member (not the host's own); `ban` also bans its key for
    /// the lobby's life (spec §5.2: by key; [`LobbyHost::ban_with_address`]
    /// bans the address too).
    pub fn kick(&mut self, member: u8, ban: bool) {
        self.kick_as(member, if ban { Ban::Key } else { Ban::No });
    }

    /// Kicks a member (not the host's own) and bans its key and its
    /// address for the lobby's life. `by` is the admin who asked (`None`:
    /// the host's own action, or a dedicated server's command).
    ///
    /// N59: a member reached through a TURN relay (`Path::Relayed`) comes
    /// from the relay's address, and banning it would ban the relay for
    /// everyone: its key alone is banned, and [`RELAYED_BAN`] goes to `by`
    /// as a notice, or with `None` as `HostEvent::Notice`.
    pub fn ban_with_address(&mut self, member: u8, by: Option<u8>) {
        let Some(i) = self.index(member).filter(|_| Some(member) != self.me) else { return };
        if self.members[i].path != Some(Path::Relayed) {
            return self.kick_as(member, Ban::KeyAndAddress);
        }
        self.kick_as(member, Ban::Key);
        match by.and_then(|admin| self.index(admin)).and_then(|a| self.members[a].peer) {
            Some(peer) => self.out.push((peer, ToClient::Notice(Notice::Message(RELAYED_BAN.into())))),
            None => self.events.push(HostEvent::Notice(RELAYED_BAN.into())),
        }
    }

    fn kick_as(&mut self, member: u8, ban: Ban) {
        if Some(member) != self.me && self.index(member).is_some() {
            self.drop_member(member, Gone::Kicked { ban });
        }
    }

    /// Moves a member to the spectators or back to the players (a free seat
    /// at once while gathering, else at the next gathering).
    pub fn move_seat(&mut self, member: u8, spectate: bool) {
        if let Some(i) = self.index(member) {
            self.members[i].wants_play = !spectate;
            if Some(member) != self.me {
                self.members[i].said_ready = false;
            }
            self.reseat();
            self.publish();
        }
    }

    // ---- the match ----

    /// Whether a match can start: gathering, at least one player (a
    /// member or the host's), 2 to 32 worms (bots included), every player
    /// ready with its content complete.
    pub fn can_start(&self) -> Result<(), NotStartable> {
        if self.phase != Phase::Gathering {
            return Err(NotStartable::Phase);
        }
        let players = self.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. }));
        Self::worms(players.clone().count(), self.setup.bots.len())?;
        let not_ready: Vec<u8> = players.filter(|m| !m.ready()).map(|m| m.id).collect();
        if not_ready.is_empty() { Ok(()) } else { Err(NotStartable::NotReady(not_ready)) }
    }

    fn worms(players: usize, bots: usize) -> Result<(), NotStartable> {
        if players == 0 {
            Err(NotStartable::NoPlayers)
        } else if players + bots < 2 {
            Err(NotStartable::TooFewWorms)
        } else if players + bots > MAX_PLAYERS {
            Err(NotStartable::TooManyWorms)
        } else {
            Ok(())
        }
    }

    /// A start forced by the host (spec §7.2): players who are not ready
    /// (or lack content) watch this match as spectators, and get their seats
    /// back at the next gathering; then `start`. Nobody moves when the
    /// ready players alone cannot start (no player, under 2 or over 32 worms).
    pub fn force_start(
        &mut self,
        seed: u32,
        rtts_ms: &BTreeMap<PeerId, u32>,
    ) -> Result<MatchStartWire, NotStartable> {
        if self.phase != Phase::Gathering {
            return Err(NotStartable::Phase);
        }
        let seated = |m: &Rec| matches!(m.seat, SeatWire::Player { .. });
        let ready = self.members.iter().filter(|m| seated(m) && m.ready()).count();
        Self::worms(ready, self.setup.bots.len())?;
        let mut moved = false;
        for m in self.members.iter_mut().filter(|m| seated(m) && !m.ready()) {
            // Lacking content: sits the match out (N33), else watches.
            m.seat = if m.lacking() { SeatWire::Waiting } else { SeatWire::Spectator };
            moved = true;
        }
        if moved {
            self.publish();
        }
        self.start(seed, rtts_ms)
    }

    /// The match start: players by seat, then the setup's bots in order;
    /// each player's input delay from its round trip (`rtts_ms` by peer;
    /// the host's 0, an unmeasured one the largest, `MAX_INPUT_DELAY`). `start_checksum` is 0: the
    /// caller builds the world from this, fills it in and calls
    /// `announce_start`. Nothing is sent but, when players are not ready,
    /// `Notice::NotReady` to everyone.
    pub fn start(
        &mut self,
        seed: u32,
        rtts_ms: &BTreeMap<PeerId, u32>,
    ) -> Result<MatchStartWire, NotStartable> {
        if let Err(e) = self.can_start() {
            if let NotStartable::NotReady(ids) = &e {
                self.broadcast(&ToClient::Notice(Notice::NotReady(ids.clone())));
            }
            return Err(e);
        }
        let mut players: Vec<&Rec> =
            self.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).collect();
        players.sort_by_key(|m| match m.seat {
            SeatWire::Player { slot } => slot,
            _ => u8::MAX,
        });
        let mut slots: Vec<SlotWire> = players
            .iter()
            .map(|m| SlotWire { member: Some(m.id), bot: None, look: m.look.clone(), loadout: m.loadout })
            .collect();
        slots.extend(self.setup.bots.iter().enumerate().map(|(i, b)| SlotWire {
            member: None,
            bot: Some(i as u8),
            look: b.look.clone(),
            loadout: None,
        }));
        let delays = players
            .iter()
            .map(|m| {
                let delay = match m.peer {
                    None => 0,
                    Some(p) => rtts_ms.get(&p).map_or(MAX_INPUT_DELAY, |&rtt| input_delay(rtt)),
                };
                (m.id, delay)
            })
            .collect();
        self.started_at = Some(self.generation);
        Ok(MatchStartWire { seed, setup: self.setup.clone(), slots, input_delay: delays, start_checksum: 0 })
    }

    /// Sends `Start` to everyone and moves to `Phase::Starting`. Refused
    /// when not gathering, or when the lobby changed since `start` made
    /// `wire` (`Stale`: start again).
    pub fn announce_start(&mut self, wire: MatchStartWire) -> Result<(), NotStartable> {
        if self.phase != Phase::Gathering {
            return Err(NotStartable::Phase);
        }
        if self.started_at != Some(self.generation) {
            return Err(NotStartable::Stale);
        }
        self.started_at = None;
        self.phase = Phase::Starting;
        self.in_match = wire.slots.iter().filter_map(|s| s.member).collect();
        // N33: a member lacking content sits the match out: no start, no
        // state, still connected. It spectates once complete.
        for m in &mut self.members {
            m.awaiting_content = m.lacking();
            if m.awaiting_content {
                m.seat = SeatWire::Waiting;
            }
        }
        let start = ToClient::Start(wire);
        let peers: Vec<PeerId> =
            self.members.iter().filter(|m| m.seat != SeatWire::Waiting).filter_map(|m| m.peer).collect();
        self.out.extend(peers.into_iter().map(|p| (p, start.clone())));
        self.publish();
        Ok(())
    }

    /// The match layer simulated its first tick: `Phase::Playing`.
    pub fn playing(&mut self, tick: u32) {
        if self.phase == Phase::Starting {
            self.phase = Phase::Playing { tick };
            self.publish();
        }
    }

    /// The match ended: `Ended` to everyone (reason sanitized, at most 32
    /// rows) and `Phase::Results` for `RESULTS_US`, then gathering again:
    /// ready cleared, spectators and waiting members who want to play take
    /// free seats in join order.
    pub fn match_ended(&mut self, end: MatchEndWire, now_us: u64) {
        if !matches!(self.phase, Phase::Starting | Phase::Playing { .. }) {
            return;
        }
        let mut end = end;
        end.reason = sanitize_chat(&end.reason).unwrap_or_default();
        end.rows.truncate(MAX_PLAYERS);
        self.phase = Phase::Results;
        self.results_since = now_us;
        self.broadcast(&ToClient::Ended(end));
        self.publish();
    }

    // ---- queries ----

    pub fn state(&self) -> &LobbyState {
        &self.state
    }

    pub fn peer_of(&self, member: u8) -> Option<PeerId> {
        self.index(member).and_then(|i| self.members[i].peer)
    }

    /// The address member `member` joined from (none for the host's own).
    pub fn ip_of(&self, member: u8) -> Option<IpAddr> {
        self.index(member).and_then(|i| self.members[i].addr)
    }

    pub fn member_of(&self, peer: PeerId) -> Option<u8> {
        match self.links.get(&peer) {
            Some(&Link::Member(id)) => Some(id),
            _ => None,
        }
    }

    pub fn banned(&self, key: &PublicKey) -> bool {
        self.banned_keys.contains(key)
    }

    pub fn banned_ip(&self, ip: IpAddr) -> bool {
        self.banned_ips.contains(&ip)
    }

    /// Every banned key, in order (a dedicated server's bans file).
    pub fn banned_keys(&self) -> impl Iterator<Item = &PublicKey> {
        self.banned_keys.iter()
    }

    /// Every banned address, in order.
    pub fn banned_ips(&self) -> impl Iterator<Item = &IpAddr> {
        self.banned_ips.iter()
    }

    /// Bans a key for the lobby's life (a dedicated server's bans file): it
    /// is refused at its next join. A member holding it now stays until
    /// kicked.
    pub fn ban_key(&mut self, key: PublicKey) {
        self.banned_keys.insert(key);
    }

    /// Bans an address for the lobby's life: a peer opening from it is
    /// refused (also mirror it in the transport's gate).
    pub fn ban_ip(&mut self, ip: IpAddr) {
        self.banned_ips.insert(ip);
    }

    /// Lifts a key's ban; whether it was banned.
    pub fn unban_key(&mut self, key: &PublicKey) -> bool {
        self.banned_keys.remove(key)
    }

    /// Lifts an address's ban; whether it was banned.
    pub fn unban_ip(&mut self, ip: IpAddr) -> bool {
        self.banned_ips.remove(&ip)
    }

    // ---- joining ----

    fn receive_pending(&mut self, peer: PeerId, msg: ToHost, now_us: u64) {
        let Some(Link::Pending(p)) = self.links.get(&peer) else { return };
        let (addr, path, nonce, fingerprints, hello) =
            (p.addr, p.path, p.nonce, p.fingerprints, p.hello.clone());
        match (msg, hello) {
            (ToHost::Hello { key, look, nonce: client_nonce }, None) => {
                // Checked at each attempt, not only when the link opened:
                // links opened before the hold get no more guesses.
                if self.held(addr, now_us) {
                    return self.refuse(peer, Refusal::Throttled);
                }
                if let Err(r) = self.may_join(&key, addr) {
                    return self.refuse(peer, r);
                }
                if let Some(Link::Pending(p)) = self.links.get_mut(&peer) {
                    p.hello = Some((key, look));
                }
                // The host proves its key (N37) before the joiner proves
                // its own and its password. BIP-340's aux is the
                // challenge's nonce: fresh per link; the signature stays
                // safe with a public aux.
                let message = host_proof_message(&client_nonce, &key, &fingerprints);
                let signature = self.key.sign(&message, nonce);
                self.out.push((peer, ToClient::HostProof { signature }));
            }
            (ToHost::Prove { signature, password }, Some((key, look))) => {
                if !verify(&key, &challenge_message(&nonce, &self.key.public(), &fingerprints), &signature) {
                    return self.refuse(peer, Refusal::Malformed);
                }
                if self.held(addr, now_us) {
                    return self.refuse(peer, Refusal::Throttled);
                }
                // The proof is bound to this joiner's key and this link
                // (`password_proof`): one relayed from another link fails.
                if let Some(pw) = &self.cfg.password
                    && password != Some(password_proof(&nonce, &key, &fingerprints, pw))
                {
                    // A client without a password proves the empty one: that
                    // is no guess (the game asks for the password next).
                    if password.is_some_and(|p| p != password_proof(&nonce, &key, &fingerprints, "")) {
                        self.wrong_password(addr, now_us);
                    }
                    return self.refuse(peer, Refusal::Password);
                }
                if let Err(r) = self.may_join(&key, addr) {
                    return self.refuse(peer, r);
                }
                self.admit(peer, addr, path, key, &look);
            }
            (ToHost::Bye, _) => {
                self.links.remove(&peer);
                self.events.push(HostEvent::Close { peer });
            }
            _ => self.refuse(peer, Refusal::Malformed),
        }
    }

    /// `addr` is held off for guessing the password.
    fn held(&self, addr: IpAddr, now_us: u64) -> bool {
        self.guesses.get(&addr).is_some_and(|g| g.held(now_us))
    }

    fn may_join(&self, key: &PublicKey, addr: IpAddr) -> Result<(), Refusal> {
        if self.banned_keys.contains(key) || self.banned_ips.contains(&addr) {
            return Err(Refusal::Banned);
        }
        if *key == self.key.public() || self.members.iter().any(|m| m.key == *key) {
            return Err(Refusal::Busy);
        }
        self.admit_seat().map(|_| ()).ok_or(Refusal::Full)
    }

    /// A new member's seat: a free player seat (waiting for the next
    /// gathering during a match), else a spectator's, else none (full).
    /// Members never exceed `max_players + max_spectators` (≤ 48).
    fn admit_seat(&self) -> Option<SeatWire> {
        let cap = usize::from(self.cfg.max_players) + usize::from(self.cfg.max_spectators);
        if self.members.len() >= cap {
            return None;
        }
        // A waiting member holds a seat for the next match only if it
        // wants to play (a spectator made to wait for its content does not).
        let seated = self
            .members
            .iter()
            .filter(|m| match m.seat {
                SeatWire::Player { .. } => true,
                SeatWire::Waiting => m.wants_play,
                SeatWire::Spectator => false,
            })
            .count();
        if seated >= usize::from(self.cfg.max_players) {
            Some(SeatWire::Spectator)
        } else if self.phase == Phase::Gathering {
            Some(self.free_slot().map_or(SeatWire::Spectator, |slot| SeatWire::Player { slot }))
        } else {
            Some(SeatWire::Waiting)
        }
    }

    fn admit(&mut self, peer: PeerId, addr: IpAddr, path: Path, key: PublicKey, look: &Look) {
        let (Some(seat), Some(id)) = (self.admit_seat(), self.alloc_id()) else {
            return self.refuse(peer, Refusal::Full);
        };
        self.links.insert(peer, Link::Member(id));
        self.members.push(Rec {
            id,
            peer: Some(peer),
            addr: Some(addr),
            path: Some(path),
            key,
            look: Look { name: String::new(), ..look.clone() },
            seat,
            wants_play: true,
            said_ready: false,
            missing: Vec::new(),
            have_revision: None,
            known: self.content_revision.is_none(),
            download: None,
            download_next: None,
            download_at: None,
            // Welcomed during a match: it spectates once its content is
            // known to be complete (N33).
            awaiting_content: self.phase != Phase::Gathering,
            loadout: None,
            admin: self.cfg.admins.contains(&key),
            ping_ms: None,
            chat: VecDeque::new(),
            looks: VecDeque::new(),
            look_held: None,
        });
        self.set_look(id, look);
        self.publish();
        self.out.push((peer, ToClient::Welcome { you: id, lobby: self.state.clone() }));
        // The others learn of the member before the notice naming it.
        self.flush_except(Some(peer));
        self.broadcast(&ToClient::Notice(Notice::Joined(id)));
        self.events.push(HostEvent::Joined(id));
    }

    /// A wrong password proof from `addr`: the [`PASSWORD_TRIES`]th within
    /// [`PASSWORD_WINDOW_US`] holds the address off for
    /// [`PASSWORD_LOCKOUT_US`]. At most [`PASSWORD_ADDRESSES`] are
    /// remembered: a new one forgets the one heard from longest ago.
    fn wrong_password(&mut self, addr: IpAddr, now_us: u64) {
        if !self.guesses.contains_key(&addr) && self.guesses.len() >= PASSWORD_ADDRESSES {
            let oldest = self.guesses.iter().min_by_key(|(_, g)| g.last()).map(|(&ip, _)| ip);
            if let Some(ip) = oldest {
                self.guesses.remove(&ip);
            }
        }
        let g = self.guesses.entry(addr).or_default();
        g.at.retain(|&t| now_us.saturating_sub(t) < PASSWORD_WINDOW_US);
        g.at.push(now_us);
        if g.at.len() >= PASSWORD_TRIES {
            g.at.clear();
            g.held_until = Some(now_us.saturating_add(PASSWORD_LOCKOUT_US));
        }
    }

    fn refuse(&mut self, peer: PeerId, why: Refusal) {
        self.links.remove(&peer);
        self.out.push((peer, ToClient::Refused(why)));
        self.events.push(HostEvent::Kick { peer, ban: false });
    }

    /// The lowest member id not in use (by a member or the running match).
    fn alloc_id(&mut self) -> Option<u8> {
        for _ in 0..=u8::MAX {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);
            if self.index(id).is_none() && !self.in_match.contains(&id) {
                return Some(id);
            }
        }
        None
    }

    // ---- members ----

    fn receive_member(&mut self, peer: PeerId, id: u8, msg: ToHost, now_us: u64) {
        let Some(i) = self.index(id) else { return };
        match msg {
            ToHost::Hello { .. } | ToHost::Prove { .. } => {
                self.drop_member(id, Gone::Kicked { ban: Ban::No })
            }
            ToHost::SetLook(look) => {
                // Each change rebuilds the content list and may start a
                // skin fetch: beyond the window's changes, only the latest
                // is held, and `tick` takes it once the window allows.
                let m = &mut self.members[i];
                if m.look_change_allowed(now_us) {
                    m.look_held = None;
                    self.set_look(id, &look);
                    self.publish();
                } else {
                    m.look_held = Some(look);
                }
            }
            ToHost::Ready(ready) => {
                if self.phase != Phase::Gathering {
                    return;
                }
                let m = &mut self.members[i];
                if ready && !m.missing.is_empty() {
                    m.said_ready = false;
                    self.out.push((peer, ToClient::Notice(Notice::Message(MISSING_CONTENT.into()))));
                } else {
                    m.said_ready = ready;
                }
                self.publish();
            }
            ToHost::Spectate(spectate) => {
                let m = &mut self.members[i];
                m.wants_play = !spectate;
                m.said_ready = false;
                self.reseat();
                self.publish();
            }
            ToHost::Loadout(loadout) => {
                self.members[i].loadout = loadout;
                self.generation += 1;
            }
            ToHost::Chat(text) => {
                let window = &mut self.members[i].chat;
                while window.front().is_some_and(|&t| now_us.saturating_sub(t) >= CHAT_WINDOW_US) {
                    window.pop_front();
                }
                if window.len() >= CHAT_LINES {
                    self.out.push((peer, ToClient::Notice(Notice::Message(SLOW_DOWN.into()))));
                } else if let Some(text) = sanitize_chat(&text) {
                    window.push_back(now_us);
                    self.relay(id, text);
                }
            }
            ToHost::Have { revision, missing } => {
                // An answer to another list than the current one.
                if self.content_revision.is_some_and(|r| r != revision) {
                    return;
                }
                let m = &mut self.members[i];
                m.have_revision = Some(revision);
                m.known = true;
                if !missing.is_empty() {
                    m.said_ready = false;
                } else {
                    // Complete: no progress to show.
                    m.download = None;
                    m.download_next = None;
                }
                let complete = missing.is_empty() && core::mem::take(&mut m.awaiting_content);
                m.missing = missing;
                if complete {
                    self.events.push(HostEvent::ContentComplete(id));
                }
                self.publish();
            }
            ToHost::Admin(cmd) => self.admin(peer, i, cmd),
            ToHost::Bye => {
                self.links.remove(&peer);
                self.drop_member(id, Gone::Left);
                self.events.push(HostEvent::Close { peer });
            }
        }
    }

    fn admin(&mut self, peer: PeerId, i: usize, cmd: AdminCommand) {
        let from = self.members[i].id;
        if !self.members[i].admin {
            self.out.push((peer, ToClient::Notice(Notice::Message(NOT_ADMIN.into()))));
            return;
        }
        match cmd {
            AdminCommand::Kick(key) | AdminCommand::Ban(key) => {
                let ban = matches!(cmd, AdminCommand::Ban(_));
                match self.members.iter().find(|m| m.key == key).map(|m| m.id) {
                    Some(target) if Some(target) == self.me => {
                        self.out.push((peer, ToClient::Notice(Notice::Message(NOT_THE_HOST.into()))));
                    }
                    Some(target) => {
                        let ban = if ban { Ban::Key } else { Ban::No };
                        self.drop_member(target, Gone::Kicked { ban });
                    }
                    None if ban => {
                        self.banned_keys.insert(key);
                    }
                    None => {}
                }
            }
            AdminCommand::Next | AdminCommand::Settings(_) => {
                self.events.push(HostEvent::Admin { from, cmd })
            }
        }
    }

    /// Removes a member: a leaver is announced as `Left`, a kicked one gets
    /// `Kicked` (and with `ban` its key and address are banned).
    fn drop_member(&mut self, id: u8, how: Gone) {
        let Some(i) = self.index(id) else { return };
        let rec = self.members.remove(i);
        if let Some(peer) = rec.peer {
            self.links.remove(&peer);
        }
        match how {
            Gone::Left => {
                self.broadcast(&ToClient::Notice(Notice::Left(id)));
                self.events.push(HostEvent::Left(id));
            }
            Gone::Kicked { ban } => {
                if ban != Ban::No {
                    self.banned_keys.insert(rec.key);
                }
                if let (Ban::KeyAndAddress, Some(addr)) = (ban, rec.addr) {
                    self.banned_ips.insert(addr);
                }
                let ban = ban != Ban::No;
                if let Some(peer) = rec.peer {
                    self.out.push((peer, ToClient::Kicked { banned: ban }));
                    self.events.push(HostEvent::Kick { peer, ban });
                }
                let notice = if ban { Notice::Banned(id) } else { Notice::Kicked(id) };
                self.broadcast(&ToClient::Notice(notice));
            }
        }
        self.reseat();
        self.publish();
    }

    /// A member's look, sanitized, its name unique ignoring case.
    fn set_look(&mut self, id: u8, look: &Look) {
        let Some(i) = self.index(id) else { return };
        let mut look = sanitize_look(look);
        let taken = self.members.iter().filter(|m| m.id != id).map(|m| m.look.name.as_str());
        look.name = unique_name(&look.name, taken);
        self.members[i].look = look;
    }

    fn relay(&mut self, from: u8, text: String) {
        self.broadcast(&ToClient::Chat { from, text: text.clone() });
        self.events.push(HostEvent::Chat { from, text });
    }

    // ---- seats and phases ----

    fn free_slot(&self) -> Option<u8> {
        (0..self.cfg.max_players)
            .find(|&s| !self.members.iter().any(|m| m.seat == SeatWire::Player { slot: s }))
    }

    /// While gathering: players who want to spectate give up their seats,
    /// then members who want to play take free seats in join order.
    fn reseat(&mut self) {
        if self.phase != Phase::Gathering {
            return;
        }
        for m in &mut self.members {
            if !m.wants_play || m.seat == SeatWire::Waiting {
                m.seat = SeatWire::Spectator;
            }
        }
        for i in 0..self.members.len() {
            if self.members[i].wants_play && !matches!(self.members[i].seat, SeatWire::Player { .. }) {
                self.members[i].seat = match self.free_slot() {
                    Some(slot) => SeatWire::Player { slot },
                    None => SeatWire::Spectator,
                };
            }
        }
    }

    /// Back from the results: everyone re-readies, seats are filled.
    fn gather(&mut self) {
        self.phase = Phase::Gathering;
        self.in_match.clear();
        for m in &mut self.members {
            m.awaiting_content = false;
        }
        let me = self.me;
        for m in self.members.iter_mut().filter(|m| Some(m.id) != me) {
            m.said_ready = false;
        }
        self.reseat();
        self.publish();
    }

    // ---- state and sending ----

    fn index(&self, id: u8) -> Option<usize> {
        self.members.iter().position(|m| m.id == id)
    }

    fn member_peers(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.members.iter().filter_map(|m| m.peer)
    }

    fn broadcast(&mut self, msg: &ToClient) {
        let peers: Vec<PeerId> = self.member_peers().collect();
        self.out.extend(peers.into_iter().map(|p| (p, msg.clone())));
    }

    /// Rebuilds the state and notes the lobby becoming startable.
    fn refresh(&mut self) {
        let state = LobbyState {
            name: self.cfg.name.clone(),
            phase: self.phase,
            setup: self.setup.clone(),
            members: self.members.iter().map(Rec::wire).collect(),
        };
        if state != self.state {
            self.state = state;
            self.revision += 1;
        }
        let startable = self.can_start().is_ok();
        if startable && !self.startable {
            self.events.push(HostEvent::StartRequested);
        }
        self.startable = startable;
    }

    /// Rebuilds the state; the next drain sends it to every member.
    fn publish(&mut self) {
        self.generation += 1;
        self.refresh();
        self.dirty = true;
    }

    fn flush(&mut self) {
        self.flush_except(None);
    }

    /// Sends the changed state to every member but `except`.
    fn flush_except(&mut self, except: Option<PeerId>) {
        if core::mem::take(&mut self.dirty) {
            let lobby = ToClient::Lobby(self.state.clone());
            let peers: Vec<PeerId> = self.member_peers().filter(|&p| Some(p) != except).collect();
            self.out.extend(peers.into_iter().map(|p| (p, lobby.clone())));
        }
    }
}
