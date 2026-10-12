//! The rollback session (networking spec §3.1-§3.3, §3.6): one struct for the
//! host, a client and a spectator, the role deciding the branches. The host
//! orders inputs (what it forwards is final), every player peer predicts the
//! inputs it lacks and rolls back when a final input differs, spectators
//! simulate confirmed ticks only.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::mem;

use rl_proto::{
    ClientInputs, Datagram, HostInputs, MAX_DATAGRAM, MAX_FRAMES, MAX_KEYS, OVERHEAD, Stamp, decode_control,
    decode_datagram, decode_frames, encode_datagram, encode_frames,
};
use rl_sim::Keys;

use crate::inputs::{InputLog, SetResult};
use crate::{Channel, PeerId, Simulation};

/// One tick lasts 10 ms (100 ticks per second, OLX's `FRAME_MS`).
pub const TICK_US: u64 = 10_000;
/// The host's fill deadline in ticks (spec §3.2's rollback window).
pub const FILL_WINDOW: u32 = 12;
/// A player peer predicts at most this many ticks past its frontier (decision N1).
pub const MAX_PREDICTION: u32 = 20;
/// Peers compare checksums every this many confirmed ticks (spec §3.5).
pub const CHECKSUM_EVERY: u32 = 10;
/// At most this many new ticks per [`Session::advance`].
pub const MAX_CATCHUP: u32 = 10;
/// Received inputs this many ticks or more past the current tick are
/// ignored: a bound on what a hostile peer can make the log hold. An honest
/// peer is never more than a few hundred ticks ahead.
const MAX_AHEAD: u32 = 6_000;

/// The input delay for a round trip (spec §3.2): `min(3, rtt_ms / 40)` ticks.
pub fn input_delay(rtt_ms: u32) -> u8 {
    (rtt_ms / 40).min(3) as u8
}

/// What a peer is in the match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    /// Plays the worm of this slot.
    Player(u8),
    /// Watches.
    Spectator,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Worms in the match; slot `i` is worm `i`.
    pub slots: u8,
    /// A client's input delay in ticks (decision N2: fixed per match); the
    /// host's local slots use none.
    pub input_delay: u8,
    pub fill_window: u32,
    pub max_prediction: u32,
    pub checksum_every: u32,
}

impl Config {
    /// The spec's numbers for `slots` worms (1 to 32) and an input delay.
    pub fn new(slots: u8, input_delay: u8) -> Config {
        assert!((1..=32).contains(&slots), "a match has 1 to 32 slots, not {slots}");
        Config {
            slots,
            input_delay,
            fill_window: FILL_WINDOW,
            max_prediction: MAX_PREDICTION,
            checksum_every: CHECKSUM_EVERY,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Role {
    /// Orders the inputs. Its local slots (its human and the bots) are those
    /// no peer holds.
    Host { peers: Vec<(PeerId, Seat)> },
    /// Plays `slot`.
    Client { host: PeerId, slot: u8 },
    /// Watches from tick 0.
    Spectator { host: PeerId },
}

/// One simulated tick, as [`Driver::simulated`] sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickInfo {
    pub tick: u32,
    /// Re-simulated by a rollback.
    pub resim: bool,
    /// Every slot's final input was known when it was simulated.
    pub all_final: bool,
}

/// The game around a session: it presses the local slots' keys and sees
/// every tick simulated.
pub trait Driver<S> {
    /// The keys of the local `slots` (`out[i]` for `slots[i]`) for `tick`,
    /// from the state before the tick being simulated. Called once per new tick.
    fn local_inputs(&mut self, sim: &S, tick: u32, slots: &[u8], out: &mut [Keys]);
    fn simulated(&mut self, _sim: &S, _info: TickInfo) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Ticks `from..to` were re-simulated.
    Rollback { from: u32, to: u32 },
    /// The prediction limit stopped the session before `tick`.
    Stalled { tick: u32 },
    /// An undecodable message from `peer`.
    Malformed { peer: PeerId },
    /// The host sent a final input that differs from an earlier one.
    Inconsistent { peer: PeerId, tick: u32, slot: u8 },
}

#[derive(Clone, Debug)]
pub struct Stats {
    pub rollbacks: u64,
    /// Rollbacks by depth in ticks (63 and more in the last).
    pub depth_hist: [u64; 64],
    pub resim_ticks: u64,
    pub stalls: u64,
    /// A client's own inputs the host's final values replaced.
    pub own_corrections: u64,
    pub malformed: u64,
    /// The round trip to the host (a client or spectator), or the largest
    /// over the peers (the host).
    pub rtt_ms: Option<u32>,
    /// Datagrams and bytes, each datagram counting [`OVERHEAD`] more.
    pub datagrams_in: u64,
    pub bytes_in: u64,
    pub datagrams_out: u64,
    pub bytes_out: u64,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            rollbacks: 0,
            depth_hist: [0; 64],
            resim_ticks: 0,
            stalls: 0,
            own_corrections: 0,
            malformed: 0,
            rtt_ms: None,
            datagrams_in: 0,
            bytes_in: 0,
            datagrams_out: 0,
            bytes_out: 0,
        }
    }
}

/// What the session knows of a remote peer.
struct PeerState {
    /// The host's view of a peer; clients use `Seat::Spectator` for the host.
    seat: Seat,
    /// Host: the peer's frontier. Client: the host's `your_ack`.
    ack: u32,
    /// Their newest `sent_ms` and when it arrived (µs).
    echo: Option<(u32, u64)>,
    /// Our first `sent_ms` to them: older echoes are no echo.
    first_sent_ms: Option<u32>,
    /// The round trip's moving average in eighths of a millisecond.
    rtt_x8: Option<i64>,
}

impl PeerState {
    fn new(seat: Seat) -> Self {
        Self { seat, ack: 0, echo: None, first_sent_ms: None, rtt_x8: None }
    }
}

pub struct Session<S: Simulation> {
    cfg: Config,
    role: Role,
    sim: S,
    log: InputLog,
    /// The slots this peer presses (`Driver::local_inputs`).
    local: Vec<u8>,
    local_keys: Vec<Keys>,
    /// A client's own slot's provisional inputs, by tick from 0.
    own: Vec<u8>,
    /// The keys used for each tick in `base..current`.
    used: VecDeque<[u8; 32]>,
    /// The state before each tick in `base..current`.
    snaps: VecDeque<S::Snapshot>,
    base: u32,
    /// The next tick to simulate.
    current: u32,
    /// New ticks run by the pace (Task 7 replaces the pacing).
    ticks_run: u32,
    rollback_from: Option<u32>,
    start_us: u64,
    /// Per remote peer: the host keeps one per client, others one for the host.
    peers: BTreeMap<PeerId, PeerState>,
    out: Vec<(PeerId, Channel, Vec<u8>)>,
    events: Vec<Event>,
    stats: Stats,
    /// Decoded frame values before they are applied.
    scratch: Vec<(u32, u8, u8)>,
}

impl<S: Simulation> Session<S> {
    pub fn new(cfg: Config, role: Role, sim: S, now_us: u64) -> Self {
        let slots = cfg.slots;
        let mut peers = BTreeMap::new();
        let mut own = Vec::new();
        let local: Vec<u8> = match &role {
            Role::Host { peers: list } => {
                for &(id, seat) in list {
                    if let Seat::Player(s) = seat {
                        assert!(s < slots, "peer {id} plays slot {s} of {slots}");
                    }
                    peers.insert(id, PeerState::new(seat));
                }
                (0..slots).filter(|s| !list.iter().any(|&(_, seat)| seat == Seat::Player(*s))).collect()
            }
            Role::Client { host, slot } => {
                assert!(*slot < slots, "the client plays slot {slot} of {slots}");
                peers.insert(*host, PeerState::new(Seat::Spectator));
                own = vec![0; cfg.input_delay as usize];
                vec![*slot]
            }
            Role::Spectator { host } => {
                peers.insert(*host, PeerState::new(Seat::Spectator));
                Vec::new()
            }
        };
        Session {
            log: InputLog::new(slots),
            local_keys: vec![Keys(0); local.len()],
            local,
            own,
            cfg,
            role,
            sim,
            used: VecDeque::new(),
            snaps: VecDeque::new(),
            base: 0,
            current: 0,
            ticks_run: 0,
            rollback_from: None,
            start_us: now_us,
            peers,
            out: Vec::new(),
            events: Vec::new(),
            stats: Stats::default(),
            scratch: Vec::new(),
        }
    }

    pub fn sim(&self) -> &S {
        &self.sim
    }

    pub fn sim_mut(&mut self) -> &mut S {
        &mut self.sim
    }

    /// The next tick to simulate.
    pub fn current_tick(&self) -> u32 {
        self.current
    }

    /// Ticks below it have every slot's final input.
    pub fn frontier(&self) -> u32 {
        self.log.frontier()
    }

    pub fn final_input(&self, tick: u32, slot: u8) -> Option<Keys> {
        self.log.get(tick, slot).map(Keys)
    }

    pub fn input_log(&self) -> &InputLog {
        &self.log
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    /// Hands every queued datagram to `out`.
    pub fn transmit(&mut self, out: &mut dyn FnMut(PeerId, Channel, Vec<u8>)) {
        for (peer, ch, bytes) in self.out.drain(..) {
            out(peer, ch, bytes);
        }
    }

    fn is_host(&self) -> bool {
        matches!(self.role, Role::Host { .. })
    }

    fn is_spectator(&self) -> bool {
        matches!(self.role, Role::Spectator { .. })
    }

    fn own_slot(&self) -> Option<u8> {
        match self.role {
            Role::Client { slot, .. } => Some(slot),
            _ => None,
        }
    }

    /// The delay between pressing and applying a local input.
    fn delay(&self) -> u32 {
        match self.role {
            Role::Client { .. } => self.cfg.input_delay as u32,
            _ => 0,
        }
    }

    /// Runs the ticks that are due: first a pending rollback, then new ticks.
    pub fn advance(&mut self, now_us: u64, driver: &mut dyn Driver<S>) {
        if let Some(from) = self.rollback_from.take() {
            self.resimulate(from, driver);
        }
        let due = self.due_ticks(now_us);
        let mut ran = 0;
        for _ in 0..due {
            let frontier = self.log.frontier();
            if self.is_spectator() {
                if self.current >= frontier {
                    break;
                }
            } else if self.current.saturating_sub(frontier) >= self.cfg.max_prediction {
                self.events.push(Event::Stalled { tick: self.current });
                self.stats.stalls += 1;
                break;
            }
            self.new_tick(driver);
            ran += 1;
        }
        self.ticks_run += ran;
        self.trim_snapshots();
        // The host also sends while it stalls: a client waiting for frames
        // the host already has would otherwise wait for ever.
        if ran > 0 || due > 0 {
            self.queue_datagrams(now_us);
        }
    }

    /// The pace (Task 6): ticks elapsed since the start minus those run, at
    /// most [`MAX_CATCHUP`].
    fn due_ticks(&self, now_us: u64) -> u32 {
        let elapsed = now_us.saturating_sub(self.start_us) / TICK_US;
        let elapsed = u32::try_from(elapsed).unwrap_or(u32::MAX);
        elapsed.saturating_sub(self.ticks_run).min(MAX_CATCHUP)
    }

    /// The keys used for `slot` at `tick` (spec §3.3): the final value, a
    /// client's own provisional input, or the slot's last final value.
    fn keys_for(&self, tick: u32, slot: u8) -> u8 {
        if let Some(v) = self.log.get(tick, slot) {
            return v;
        }
        if self.own_slot() == Some(slot)
            && let Some(&v) = self.own.get(tick as usize)
        {
            return v;
        }
        self.log.predicted(tick, slot)
    }

    /// Saves the state before `current`, simulates it and reports it.
    fn simulate(&mut self, driver: &mut dyn Driver<S>, resim: bool) {
        let t = self.current;
        if self.snaps.is_empty() {
            self.base = t;
        }
        self.snaps.push_back(self.sim.save());
        let slots = self.cfg.slots;
        let mut row = [0u8; 32];
        let mut keys = [Keys(0); 32];
        for s in 0..slots {
            row[s as usize] = self.keys_for(t, s);
            keys[s as usize] = Keys(row[s as usize]);
        }
        let all_final = self.log.complete(t);
        self.sim.tick(&keys[..slots as usize]);
        self.used.push_back(row);
        driver.simulated(&self.sim, TickInfo { tick: t, resim, all_final });
        self.current = t + 1;
    }

    /// A new tick: the local inputs, then the tick itself.
    fn new_tick(&mut self, driver: &mut dyn Driver<S>) {
        let t = self.current;
        let at = t + self.delay();
        self.local_keys.iter_mut().for_each(|k| *k = Keys(0));
        driver.local_inputs(&self.sim, at, &self.local, &mut self.local_keys);
        match self.role {
            Role::Host { .. } => {
                for (&s, k) in self.local.iter().zip(&self.local_keys) {
                    self.log.set(t, s, k.0);
                }
            }
            Role::Client { .. } => {
                if self.own.len() == at as usize {
                    self.own.push(self.local_keys[0].0);
                }
            }
            Role::Spectator { .. } => {}
        }
        self.simulate(driver, false);
    }

    /// Restores the state before `from` and re-simulates to the present.
    fn resimulate(&mut self, from: u32, driver: &mut dyn Driver<S>) {
        let to = self.current;
        let from = from.max(self.base);
        let i = (from - self.base) as usize;
        if from >= to || i >= self.snaps.len() {
            return;
        }
        self.sim.restore(&self.snaps[i]);
        self.snaps.truncate(i);
        self.used.truncate(i);
        self.current = from;
        while self.current < to {
            self.simulate(driver, true);
        }
        let depth = to - from;
        self.stats.rollbacks += 1;
        self.stats.depth_hist[(depth as usize).min(63)] += 1;
        self.stats.resim_ticks += depth as u64;
        self.events.push(Event::Rollback { from, to });
    }

    /// Keeps the snapshots from the frontier on (at least one) and lets the
    /// simulation forget older ones.
    fn trim_snapshots(&mut self) {
        let frontier = self.log.frontier();
        while self.base < frontier && self.snaps.len() > 1 {
            self.snaps.pop_front();
            self.used.pop_front();
            self.base += 1;
        }
        if let Some(s) = self.snaps.front() {
            self.sim.forget_before(s);
        }
    }

    /// A final value arrived: roll back if the tick ran on other keys.
    fn on_final(&mut self, tick: u32, slot: u8, v: u8) {
        if self.own_slot() == Some(slot) && self.own.get(tick as usize).is_some_and(|&o| o != v) {
            self.stats.own_corrections += 1;
        }
        if tick < self.current
            && tick >= self.base
            && let Some(row) = self.used.get((tick - self.base) as usize)
            && row[slot as usize] != v
        {
            self.rollback_from = Some(self.rollback_from.map_or(tick, |r| r.min(tick)));
        }
    }

    fn stamp(&mut self, peer: PeerId, now_us: u64) -> Stamp {
        let sent_ms = (now_us / 1000) as u32;
        let Some(p) = self.peers.get_mut(&peer) else { return Stamp::default() };
        p.first_sent_ms.get_or_insert(sent_ms);
        match p.echo {
            Some((echo_ms, at)) => {
                let held = (now_us.saturating_sub(at) / 1000).min(u16::MAX as u64) as u16;
                Stamp { sent_ms, echo_ms, held_ms: held }
            }
            None => Stamp { sent_ms, echo_ms: 0, held_ms: 0 },
        }
    }

    fn on_stamp(&mut self, peer: PeerId, st: Stamp, now_us: u64) {
        let Some(p) = self.peers.get_mut(&peer) else { return };
        if p.echo.is_none_or(|(e, _)| st.sent_ms >= e) {
            p.echo = Some((st.sent_ms, now_us));
        }
        // An echo older than our first stamp to them is no echo.
        if p.first_sent_ms.is_some_and(|f| st.echo_ms >= f) {
            let sample = (now_us / 1000) as i64 - st.echo_ms as i64 - st.held_ms as i64;
            if sample >= 0 {
                // `rtt += (sample - rtt) / 8`, kept in eighths so that it
                // settles on the sample instead of up to 7 ms off.
                p.rtt_x8 = Some(match p.rtt_x8 {
                    None => sample * 8,
                    Some(r) => r + sample - r / 8,
                });
                let rtt = |p: &PeerState| p.rtt_x8.map(|r| (r / 8).clamp(0, u32::MAX as i64) as u32);
                self.stats.rtt_ms = self.peers.values().filter_map(rtt).max();
            }
        }
    }

    fn push_out(&mut self, peer: PeerId, ch: Channel, bytes: Vec<u8>) {
        self.stats.datagrams_out += 1;
        self.stats.bytes_out += (bytes.len() + OVERHEAD) as u64;
        self.out.push((peer, ch, bytes));
    }

    /// The datagrams after an advance: the host's frames to every peer, or a
    /// client's (spectator's) inputs and ack to the host.
    fn queue_datagrams(&mut self, now_us: u64) {
        let slots = self.cfg.slots as usize;
        match self.role {
            Role::Host { .. } => {
                let ids: Vec<PeerId> = self.peers.keys().copied().collect();
                for id in ids {
                    let frontier = self.log.frontier();
                    let (ack, seat) = {
                        let p = &self.peers[&id];
                        (p.ack, p.seat)
                    };
                    let first = ack.min(frontier);
                    let your_ack = match seat {
                        Seat::Player(s) => self.log.known_upto(s),
                        Seat::Spectator => frontier,
                    };
                    let stamp = self.stamp(id, now_us);
                    let log = &self.log;
                    let base = |s: usize| if first == 0 { Some(0) } else { log.get(first - 1, s as u8) };
                    let mut n = log.horizon().saturating_sub(first).min(MAX_FRAMES as u32);
                    let bytes = loop {
                        let frames = encode_frames(first, n, slots, &|t, s| log.get(t, s as u8), &base);
                        let d = Datagram::Host(HostInputs {
                            stamp,
                            tick: self.current,
                            your_ack,
                            margin: 0,
                            first,
                            frames,
                        });
                        let bytes = encode_datagram(&d);
                        if bytes.len() <= MAX_DATAGRAM || n == 0 {
                            break bytes;
                        }
                        n /= 2;
                    };
                    self.push_out(id, Channel::Unreliable, bytes);
                }
            }
            Role::Client { host, .. } | Role::Spectator { host } => {
                let first = self.peers.get(&host).map_or(0, |p| p.ack);
                let from = (first as usize).min(self.own.len());
                let keys: Vec<u8> = self.own[from..].iter().copied().take(MAX_KEYS).collect();
                let stamp = self.stamp(host, now_us);
                let d = Datagram::Client(ClientInputs {
                    stamp,
                    tick: self.current,
                    ack: self.log.frontier(),
                    first,
                    keys,
                });
                self.push_out(host, Channel::Unreliable, encode_datagram(&d));
            }
        }
    }

    fn malformed(&mut self, peer: PeerId) {
        self.stats.malformed += 1;
        self.events.push(Event::Malformed { peer });
    }

    /// Takes one message from `peer`. Never panics: anything undecodable is
    /// counted ([`Event::Malformed`]) and changes nothing else.
    pub fn receive(&mut self, from: PeerId, ch: Channel, bytes: &[u8], now_us: u64) {
        self.stats.datagrams_in += 1;
        self.stats.bytes_in += (bytes.len() + OVERHEAD) as u64;
        if !self.peers.contains_key(&from) {
            return;
        }
        match ch {
            // Control messages (late inputs, checksums, states) come with
            // Tasks 7 and 8; until then a valid one changes nothing.
            Channel::Reliable => {
                if decode_control(bytes).is_err() {
                    self.malformed(from);
                }
            }
            Channel::Unreliable => match decode_datagram(bytes, self.cfg.slots as usize) {
                Ok(Datagram::Client(c)) if self.is_host() => self.on_client_inputs(from, c, now_us),
                Ok(Datagram::Host(h)) if !self.is_host() => self.on_host_inputs(from, h, now_us),
                _ => self.malformed(from),
            },
        }
    }

    /// Host: a client's inputs become final in arrival order.
    fn on_client_inputs(&mut self, from: PeerId, c: ClientInputs, now_us: u64) {
        self.on_stamp(from, c.stamp, now_us);
        let Some(p) = self.peers.get_mut(&from) else { return };
        p.ack = p.ack.max(c.ack);
        let Seat::Player(slot) = p.seat else { return };
        let limit = self.current.saturating_add(MAX_AHEAD);
        for (k, &v) in c.keys.iter().enumerate() {
            let Some(t) = c.first.checked_add(k as u32) else { break };
            if t >= limit {
                break;
            }
            match self.log.set(t, slot, v) {
                SetResult::New => self.on_final(t, slot, v),
                // The first value stays final.
                SetResult::Same | SetResult::Conflict => {}
                SetResult::Gap => break,
            }
        }
    }

    /// Client or spectator: the host's final inputs.
    fn on_host_inputs(&mut self, from: PeerId, h: HostInputs, now_us: u64) {
        let slots = self.cfg.slots as usize;
        let first = h.first;
        let mut scratch = mem::take(&mut self.scratch);
        scratch.clear();
        let log = &self.log;
        let base = |s: usize| if first == 0 { Some(0) } else { log.get(first - 1, s as u8) };
        let decoded =
            decode_frames(first, &h.frames, slots, &base, &mut |t, s, v| scratch.push((t, s as u8, v)));
        if decoded.is_err() {
            self.scratch = scratch;
            self.malformed(from);
            return;
        }
        self.on_stamp(from, h.stamp, now_us);
        let limit = self.current.saturating_add(MAX_AHEAD);
        for &(t, s, v) in &scratch {
            if t >= limit {
                continue;
            }
            match self.log.set(t, s, v) {
                SetResult::New => self.on_final(t, s, v),
                SetResult::Conflict => self.events.push(Event::Inconsistent { peer: from, tick: t, slot: s }),
                SetResult::Same | SetResult::Gap => {}
            }
        }
        self.scratch = scratch;
        if let Some(p) = self.peers.get_mut(&from) {
            p.ack = p.ack.max(h.your_ack);
        }
    }
}
