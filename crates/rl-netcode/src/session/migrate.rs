//! The session's side of a host migration (networking spec §3.7, decisions
//! N67, N69): suspension, the take-over, rejoins, the collection of what
//! each peer knows, the merge and the resume. Who takes over and who
//! rejoins whom is the drivers' part (the harness, rl-online); time is an
//! argument, as everywhere in the session.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;

use rl_proto::{
    ClientInputs, Control, Datagram, Frame, HostInputs, InputsReport, MAX_KEYS, MAX_REPORT_OWN,
    decode_frames, encode_control, encode_datagram,
};
use rl_sim::Keys;

use super::{MAX_AHEAD, PeerState, Resuming, Role, Seat, Session, TICK_US, TimeSync, frames_base};
use crate::clock::{Pace, TARGET_MARGIN};
use crate::inputs::SetResult;
use crate::migration::{
    HOST_SILENT_US, KEEPALIVE_US, MAX_RESUME_HOLD_US, RESUME_REPEAT_US, fit_report, fit_resume,
};
use crate::{Channel, Event, PeerId, Simulation};

/// A resume's `at_ms` is taken at most this far from the receipt (a broken
/// or hostile host cannot move a pace by more; an honest one is within a
/// hold and a lost reliable message's retransmission of it).
pub(super) const RESUME_AT_BOUND_MS: i64 = 2_000;

/// A rehosted peer's keepalive period, faster than [`KEEPALIVE_US`]: each
/// one's stamp is a sample of its clock at the new host (ruling RS-1).
pub(super) const REHOSTED_KEEPALIVE_US: u64 = 20_000;

/// A rejoined peer's report, as the new host has it.
enum Rejoin {
    /// `Control::Collect` sent at `collect_us`, no `Control::Report` yet.
    Awaiting { collect_us: u64 },
    /// Reported: its own provisional inputs from `own_first`, kept until the
    /// resume (then taken, and emptied).
    Reported { own_first: u32, own: Vec<u8> },
}

/// New host: the migration it collects, from [`Session::take_over`] on.
pub(super) struct Collection {
    /// Player slots none of its local slots is: each awaited from a
    /// rejoining peer, else abandoned at the resume.
    awaited: BTreeSet<u8>,
    rejoined: BTreeMap<PeerId, Rejoin>,
    /// [`Session::resume`] ran: no more rejoins; late reports are still taken.
    resumed: bool,
}

impl<S: Simulation> Session<S> {
    /// Stops advancing (the migration began): no new tick and no input
    /// frames. A frame-less keepalive datagram every [`KEEPALIVE_US`] and
    /// every reliable control still go; an owed rollback or a loaded state
    /// still re-simulates up to the current tick; states arriving are still
    /// taken. The frames kept ahead of the frontier go (ruling FR12c-2:
    /// they are the old host's, and a new host may have its peer id).
    pub fn suspend(&mut self) {
        if !self.suspended {
            self.suspended = true;
            self.keepalive_us = None;
        }
        self.ahead.clear();
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    /// Host (rulings SB-2, FR12c-1): at least two remote members (players
    /// and spectators) were heard in this match (a stamped datagram) and
    /// are not abandoned, and every one has sent nothing for
    /// [`HOST_SILENT_US`], counted only over time this host advanced (as a
    /// client's `HostSilent`). Its own network is down (or they all crashed):
    /// its peers find it silent as it finds them, and move to a successor;
    /// with migration on the driver ends its match view rather than play on
    /// alone. One member still heard shows its network is up. With one
    /// remote member it is never so: neither side can tell its own drop
    /// from the other's, so a blip stays a stall (FR12c-1). Members gone by
    /// `Bye` are abandoned and count for nothing.
    pub fn members_silent(&self, now_us: u64) -> bool {
        if !self.is_host() {
            return false;
        }
        let from = self.silence_from_us;
        let mut members = self.peers.values().filter(|p| !p.lost && p.echo.is_some());
        let silent = |p: &PeerState| now_us.saturating_sub(p.heard_us.max(from)) >= HOST_SILENT_US;
        let mut n = 0;
        members.all(|p| {
            n += 1;
            silent(p)
        }) && n >= 2
    }

    /// The old host is back before any rehost (N69): advancing again. Its
    /// host ran on meanwhile (a host stopped for `STEP_DOWN_US` steps down,
    /// one cut off from its players leaves), so the pause is caught up, as
    /// after an outage without migration: the pace ran on while suspended,
    /// and the ticks it is behind run at `MAX_CATCHUP` an advance (ruling
    /// SK-4: restarted at our own stale tick, a peer whose link blipped
    /// played on seconds behind until a fresh median of margins jumped it,
    /// and on `bad` its slot's fills stalled every other player). Nothing
    /// for a session that rehosted or took over (its resume ends the wait).
    pub fn unsuspend(&mut self) {
        let collecting = self.collection.as_ref().is_some_and(|c| !c.resumed);
        if !self.suspended || self.rehosted || collecting {
            return;
        }
        self.suspended = false;
    }

    /// Client or spectator: its host is now `host` (a new link). The host's
    /// state (acks, round trip, silence) starts afresh, and nothing queued
    /// or kept ahead for the old host stays (ruling FR12c-2); the session stays
    /// suspended until `Control::Resume`, answers `Control::Collect` with
    /// `Control::Report`, and takes nothing from any other peer.
    pub fn rehost(&mut self, host: PeerId, now_us: u64) {
        match &mut self.role {
            Role::Client { host: h, .. } | Role::Spectator { host: h } => *h = host,
            Role::Host { .. } | Role::Joining { .. } => return,
        }
        self.peers.clear();
        self.peers.insert(host, PeerState::new(Seat::Spectator, now_us));
        // Everything queued, and every frame kept ahead, was the old host's,
        // whatever its id (rl-online's new host has the old one's: ruling
        // FR12c-2).
        self.out.clear();
        self.ahead.clear();
        self.rehosted = true;
        self.suspend();
        // Our clock's first sample at the new host goes at once.
        self.keepalive_us = None;
        self.incoming = None;
        self.forget_time_sync();
    }

    /// This peer becomes the host (spec §3.7). Its local slots are its own
    /// slot (if a player) and `bots`; every other slot is awaited from a
    /// rejoining peer or abandoned at the resume. Suspended until
    /// [`Session::resume`]. Nothing is filled here (N67: a later report may
    /// carry final values for any slot). Nothing for a host or a joining
    /// spectator (it has no state to host from).
    /// (`_now_us`: the pace restarts at the resume.)
    pub fn take_over(&mut self, bots: &[u8], _now_us: u64) {
        let own = match self.role {
            Role::Client { slot, .. } => Some(slot),
            Role::Spectator { .. } => None,
            Role::Host { .. } | Role::Joining { .. } => return,
        };
        let slots = self.cfg.slots;
        let mut local: Vec<u8> = bots.iter().copied().filter(|&s| s < slots && Some(s) != own).collect();
        local.sort_unstable();
        local.dedup();
        let awaited = (0..slots).filter(|s| Some(*s) != own && !local.contains(s)).collect();
        self.role = Role::Host { peers: Vec::new() };
        self.own_slot = own;
        self.local_keys = vec![Keys(0); local.len()];
        self.local = local;
        self.peers.clear();
        self.out.clear();
        self.ahead.clear();
        self.collection = Some(Collection { awaited, rejoined: BTreeMap::new(), resumed: false });
        self.abandoned.clear();
        self.rehosted = false;
        self.suspend();
        self.incoming = None;
        self.dirty = u32::MAX;
        self.lates.clear();
        self.forget_time_sync();
    }

    /// New host: `peer` rejoined with `seat` (its key matched the
    /// succession). Sends it `Control::Collect` with our known prefixes.
    /// `false` (nothing done) after the resume, for a seat out of range,
    /// for a slot already held (ours, a bot's, another rejoined peer's), for
    /// a peer already rejoined, or when not a suspended host.
    pub fn rejoined(&mut self, peer: PeerId, seat: Seat, now_us: u64) -> bool {
        let collecting = self.collection.as_ref().is_some_and(|c| !c.resumed);
        if !self.is_host() || !self.suspended || !collecting || self.peers.contains_key(&peer) {
            return false;
        }
        if let Seat::Player(s) = seat
            && (s >= self.cfg.slots
                || self.own_slot == Some(s)
                || self.local.contains(&s)
                || self.peers.values().any(|p| p.seat == seat))
        {
            return false;
        }
        self.peers.insert(peer, PeerState::new(seat, now_us));
        if let Role::Host { peers } = &mut self.role {
            peers.push((peer, seat));
        }
        if let Some(c) = self.collection.as_mut() {
            c.rejoined.insert(peer, Rejoin::Awaiting { collect_us: now_us });
        }
        let known = (0..self.cfg.slots).map(|s| self.log.known_upto(s)).collect();
        self.push_out(peer, Channel::Reliable, encode_control(&Control::Collect { known }));
        true
    }

    /// New host: rejoined peers whose `Control::Report` has not arrived
    /// (abandoned ones aside), in id order.
    pub fn awaiting_reports(&self) -> Vec<PeerId> {
        let Some(c) = &self.collection else { return Vec::new() };
        c.rejoined
            .iter()
            .filter(|(id, r)| {
                matches!(r, Rejoin::Awaiting { .. }) && self.peers.get(id).is_some_and(|p| !p.lost)
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// New host: the merge ends (N67; the order matters). Reported own
    /// inputs extend their slots; bot slots' missing ticks below R get their
    /// previous final value, its own slot its pressed keys; awaited slots
    /// nobody took become abandoned (`Keys(0)`, decision N6). Returns the
    /// current tick, R. Once only: later calls, and calls on a session that
    /// did not take over, change nothing and return the current tick.
    ///
    /// The time sync (ruling RS-1): every input in flight before the
    /// migration is gone, so tick R waits for the rejoined players' first
    /// inputs: it runs after a hold of their longest round trip (at most
    /// [`MAX_RESUME_HOLD_US`], in whole ticks; our pace restarts at
    /// `now_us`, that far behind R). Each rejoined peer gets
    /// `Control::Resume` now, with the final inputs it lacks from its
    /// report's `ack`, and `at_ms`: the moment, on its own clock, from which
    /// its inputs reach us as R runs. It is our clock at R's start less the
    /// median of its datagrams' arrival here minus their `sent_ms` (its
    /// clock's offset plus one trip): the trip's own length cancels, and a
    /// `Resume` delivered late (a reliable message lost once) is no later
    /// for it.
    pub fn resume(&mut self, now_us: u64) -> u32 {
        let r = self.current;
        let host = self.is_host();
        let Some(c) = self.collection.as_mut() else { return r };
        if c.resumed || !host {
            return r;
        }
        c.resumed = true;
        // Reports first: each rejoined player's own inputs.
        let mut owns = Vec::new();
        for (id, rejoin) in c.rejoined.iter_mut() {
            if let Rejoin::Reported { own_first, own } = rejoin
                && let Some(Seat::Player(s)) = self.peers.get(id).map(|p| p.seat)
            {
                owns.push((s, *own_first, core::mem::take(own)));
            }
        }
        let awaited: Vec<u8> = c.awaited.iter().copied().collect();
        for (s, own_first, own) in owns {
            self.extend_own(s, own_first, &own);
        }
        // Then the fills: the bots' previous final values (what every peer
        // predicted), our own pressed keys.
        for i in 0..self.local.len() {
            let s = self.local[i];
            while self.log.known_upto(s) < r {
                let k = self.log.known_upto(s);
                let v = self.log.predicted(k, s);
                self.log.set(k, s, v);
                self.on_final(k, s, v);
            }
        }
        if let Some(o) = self.own_slot {
            while self.log.known_upto(o) < r {
                let k = self.log.known_upto(o);
                let v = self.own.get(k as usize).copied().unwrap_or_else(|| self.log.predicted(k, o));
                self.log.set(k, o, v);
                self.on_final(k, o, v);
            }
        }
        // Players not back: `Keys(0)` from their first unknown tick (N6).
        for s in awaited {
            if !self.peers.values().any(|p| p.seat == Seat::Player(s)) {
                self.abandoned.insert(s);
            }
        }
        // The lagging rule for R now, as the tick's own fill would: our
        // current tick can be `MAX_PREDICTION` past an absent player's
        // prefix, and a host stalled there would never run that fill.
        self.fill(r);
        // The hold: the rejoined players' longest round trip.
        let rtt_us = |p: &PeerState| p.rtt_x8.map(|r| (r.max(0) as u64 / 8).saturating_mul(1000));
        let players = self.peers.values().filter(|p| !p.lost && matches!(p.seat, Seat::Player(_)));
        let longest = players.filter_map(rtt_us).max().unwrap_or(0).min(MAX_RESUME_HOLD_US);
        let hold = longest.div_ceil(TICK_US);
        let start_ms = ((now_us + hold * TICK_US) / 1000) as u32;
        let (frontier, horizon, slots) = (self.log.frontier(), self.log.horizon(), self.cfg.slots as usize);
        let ids: Vec<PeerId> = self.peers.iter().filter(|(_, p)| !p.lost).map(|(&id, _)| id).collect();
        for id in ids {
            let Some(p) = self.peers.get(&id) else { continue };
            // 0 is "unknown" on the wire: a real moment is at least 1.
            let at_ms = p.offsets.median_i32().map_or(0, |o| start_ms.wrapping_sub(o as u32).max(1));
            // It holds every slot below its ack, and so do we below our
            // frontier; nothing below its log's start (SK-9).
            let first = p.ack.min(frontier).max(self.log.origin()).max(p.origin);
            let (log, origin) = (&self.log, p.origin);
            let base = |s: usize| frames_base(log, first, origin, s);
            let resume = fit_resume(r, at_ms, first, horizon, slots, &|t, s| log.get(t, s as u8), &base);
            self.push_out(id, Channel::Reliable, encode_control(&resume));
            // Its unreliable twin in every datagram until its inputs come.
            if let Some(p) = self.peers.get_mut(&id)
                && matches!(p.seat, Seat::Player(_))
            {
                p.resuming = Some(Resuming { until_us: now_us + RESUME_REPEAT_US, tick: r, at_ms });
            }
        }
        self.suspended = false;
        self.restart_pace(now_us, -(hold as i64));
        self.events.push(Event::Resumed { tick: r });
        self.stats.migrations += 1;
        r
    }

    /// Client or spectator: the new host asks what we know (`known[s]`: the
    /// first tick of slot `s` it lacks). Answered with every final input
    /// from the least of them to our horizon, and our own provisional
    /// inputs from our slot's known prefix ([`fit_report`]). Only from a
    /// host we rehosted to, before its resume; a `known` for another number
    /// of slots is malformed.
    pub(super) fn on_collect(&mut self, from: PeerId, known: &[u32]) {
        let (Role::Client { host, .. } | Role::Spectator { host }) = self.role else { return };
        if from != host || !self.rehosted {
            return;
        }
        if known.len() != self.cfg.slots as usize {
            self.malformed(from);
            return;
        }
        let log = &self.log;
        // Below a joined spectator's origin it knows nothing.
        let first = known.iter().copied().min().unwrap_or(0).max(log.origin());
        let value = |t: u32, s: usize| log.get(t, s as u8);
        let base = |s: usize| first.checked_sub(1).and_then(|p| log.get(p, s as u8));
        let (own_first, own) = match self.own_slot {
            Some(o) => {
                let f = log.known_upto(o);
                let own = self.own.iter().skip(f as usize).take(MAX_REPORT_OWN).copied().collect();
                (f, own)
            }
            None => (0, Vec::new()),
        };
        let slots = self.cfg.slots as usize;
        let report = fit_report(
            log.frontier(),
            log.origin(),
            first,
            log.horizon(),
            slots,
            &value,
            &base,
            own_first,
            own,
        );
        self.push_out(host, Channel::Reliable, encode_control(&Control::Report(report)));
    }

    /// New host: a rejoined peer's report (N67). Its final inputs are taken
    /// in tick order where they extend a slot's prefix (`Same` is fine; a
    /// `Conflict` is ignored and counted); its own provisional inputs wait
    /// for the resume (after it, they are taken at once); its `ack` is where
    /// our frames to it start. A report from a peer that did not rejoin, or
    /// a second one, changes nothing. One that does not decode for the
    /// match's slots (a frame naming a slot beyond them: the decoder cannot
    /// know the count) is malformed, takes nothing and ends the wait for it.
    pub(super) fn on_report(&mut self, from: PeerId, r: InputsReport, now_us: u64) {
        let Some(c) = self.collection.as_mut() else { return };
        let Some(&Rejoin::Awaiting { collect_us }) = c.rejoined.get(&from) else { return };
        let resumed = c.resumed;
        c.rejoined.insert(from, Rejoin::Reported { own_first: 0, own: Vec::new() });
        let slots = self.cfg.slots as usize;
        let first = r.first;
        let mut scratch = core::mem::take(&mut self.scratch);
        scratch.clear();
        let log = &self.log;
        // An honest reporter starts at the least of our known prefixes, so
        // we hold every slot at `first - 1`; a slot we lack there takes none
        // of these values (`Gap`), so any base decodes it.
        let base = |s: usize| match first {
            0 => Some(0),
            f => log.get(f - 1, s as u8).or_else(|| (log.known_upto(s as u8) < f).then_some(0)),
        };
        let decoded = (first == 0 || first >= log.origin())
            && decode_frames(first, &r.frames, slots, &base, &mut |t, s, v| scratch.push((t, s as u8, v)))
                .is_ok();
        if !decoded {
            self.scratch = scratch;
            self.malformed(from);
            return;
        }
        let limit = self.current.saturating_add(MAX_AHEAD);
        for &(t, s, v) in &scratch {
            if t >= limit {
                continue;
            }
            match self.log.set(t, s, v) {
                SetResult::New => self.on_final(t, s, v),
                SetResult::Conflict => self.stats.report_conflicts += 1,
                SetResult::Same | SetResult::Gap => {}
            }
        }
        self.scratch = scratch;
        let Some(p) = self.peers.get_mut(&from) else { return };
        p.ack = r.ack;
        // Its log's start (a spectator's state; at most its ack, the
        // decoder's bound): our first frames to it there carry no base.
        p.origin = r.origin;
        // Our `Collect` and its answer are a round trip (a reliable one: a
        // retransmission lengthens it), the measure until stamps give one.
        if p.rtt_x8.is_none() {
            p.rtt_x8 = Some((now_us.saturating_sub(collect_us) / 1000) as i64 * 8);
        }
        let Seat::Player(s) = p.seat else { return };
        if resumed {
            self.extend_own(s, r.own_first, &r.own);
        } else if let Some(c) = self.collection.as_mut() {
            c.rejoined.insert(from, Rejoin::Reported { own_first: r.own_first, own: r.own });
        }
    }

    /// Client or spectator: the new host resumes at `tick` (R), sending
    /// the final inputs we lack from `first`. They are taken as its
    /// datagrams' are (a frame that does not decode makes the whole message
    /// malformed: nothing changes). A client's pace is at `R +
    /// TARGET_MARGIN - input_delay` at `at_ms` on our clock (ruling RS-1:
    /// from then our inputs reach the new host one tick early, as time sync
    /// keeps them; the receipt when `at_ms` is 0), and runs on from there:
    /// catching up when `at_ms` has passed, waiting when ahead (the pause
    /// is not caught up); a spectator's at `max(current, R)` now. `at_ms` is
    /// taken within [`RESUME_AT_BOUND_MS`] of now. Time sync starts as
    /// after a jump (margins from inputs made after a catch-up). Only from
    /// our host, and not more than `MAX_AHEAD` ticks past our current tick
    /// (a hostile or broken host cannot push our timeline to `u32::MAX`);
    /// once resumed (by the first of `Control::Resume` and its
    /// `Datagram::Resume` twin, or never rehosted), its finals alone.
    pub(super) fn on_resume(
        &mut self,
        from: PeerId,
        tick: u32,
        at_ms: u32,
        first: u32,
        frames: &[Frame],
        now_us: u64,
    ) {
        let (Role::Client { host, .. } | Role::Spectator { host }) = self.role else { return };
        if from != host || tick > self.current.saturating_add(MAX_AHEAD) {
            return;
        }
        // An honest host starts them at our report's ack, within our log.
        let within = first == 0 || first >= self.log.origin();
        if !within || !self.take_frames(from, first, frames) {
            self.malformed(from);
            return;
        }
        // Resumed already (its twin came first): the final inputs alone.
        if !self.rehosted {
            return;
        }
        self.rehosted = false;
        self.suspended = false;
        let current = i64::from(self.current);
        let (target, origin_us) = match self.role {
            Role::Client { .. } => {
                // How long ago `at_ms` was on our clock (negative: to come).
                let now_ms = (now_us / 1000) as u32;
                let ago_ms = match at_ms {
                    0 => 0,
                    at => i64::from(now_ms.wrapping_sub(at) as i32)
                        .clamp(-RESUME_AT_BOUND_MS, RESUME_AT_BOUND_MS),
                };
                let origin_us = (now_us as i64 - ago_ms * 1000).max(0) as u64;
                (i64::from(tick) + i64::from(TARGET_MARGIN) - i64::from(self.cfg.input_delay), origin_us)
            }
            _ => (current.max(i64::from(tick)), now_us),
        };
        self.restart_pace(origin_us, target - current);
        if matches!(self.role, Role::Client { .. }) {
            self.sync = TimeSync::Jumped;
        }
        self.events.push(Event::Resumed { tick });
        self.stats.migrations += 1;
    }

    /// Suspended: a frame-less datagram every [`KEEPALIVE_US`] (a client's
    /// `ClientInputs` without keys to its host, a host's `HostInputs`
    /// without frames to every peer), so nobody takes us for silent.
    pub(super) fn keepalive(&mut self, now_us: u64) {
        let period = if self.rehosted { REHOSTED_KEEPALIVE_US } else { KEEPALIVE_US };
        if self.keepalive_us.is_some_and(|k| now_us.saturating_sub(k) < period) {
            return;
        }
        self.keepalive_us = Some(now_us);
        let frontier = self.log.frontier();
        match self.role {
            Role::Host { .. } => {
                let ids: Vec<PeerId> =
                    self.peers.iter().filter(|(_, p)| !p.lost).map(|(&id, _)| id).collect();
                for id in ids {
                    let Some(p) = self.peers.get(&id) else { continue };
                    let your_ack = match p.seat {
                        Seat::Player(s) => self.log.known_upto(s),
                        Seat::Spectator => frontier,
                    };
                    let margin = p.margins.median().unwrap_or(0);
                    let stamp = self.stamp(id, now_us);
                    let d = Datagram::Host(HostInputs {
                        stamp,
                        tick: self.current,
                        your_ack,
                        margin,
                        first: frontier,
                        frames: Vec::new(),
                    });
                    self.push_out(id, Channel::Unreliable, encode_datagram(&d));
                }
            }
            Role::Client { host, .. } | Role::Spectator { host } | Role::Joining { host } => {
                let Some(p) = self.peers.get(&host) else { return };
                if p.lost {
                    return;
                }
                // Where our next inputs start, as `queue_datagrams` has it.
                let len = self.own.len();
                let first =
                    (p.ack as usize).min(len.saturating_sub(1)).max(len.saturating_sub(MAX_KEYS)) as u32;
                let stamp = self.stamp(host, now_us);
                let d = Datagram::Client(ClientInputs {
                    stamp,
                    tick: self.current,
                    ack: frontier,
                    first,
                    keys: Vec::new(),
                });
                self.push_out(host, Channel::Unreliable, encode_datagram(&d));
            }
        }
    }

    /// `own` (inputs of `slot` from tick `own_first`) where it extends the
    /// slot's known prefix, and nothing beyond a gap or `MAX_AHEAD`.
    fn extend_own(&mut self, slot: u8, own_first: u32, own: &[u8]) {
        let limit = self.current.saturating_add(MAX_AHEAD);
        for (k, &v) in own.iter().enumerate() {
            let Some(t) = u32::try_from(k).ok().and_then(|k| own_first.checked_add(k)) else { break };
            let upto = self.log.known_upto(slot);
            if t >= limit || t > upto {
                break;
            }
            if t == upto && self.log.set(t, slot, v) == SetResult::New {
                self.on_final(t, slot, v);
            }
        }
    }

    /// The pace starts again at `now_us`, `ahead` ticks past the current
    /// tick (negative: that many ticks of time before the next runs; the
    /// pause is not caught up), keeping its speed.
    fn restart_pace(&mut self, now_us: u64, ahead: i64) {
        let adjust = self.pace.adjust_ppm();
        self.pace = Pace::new(now_us);
        self.pace.set_adjust_ppm(adjust);
        let bound = i64::from(MAX_AHEAD);
        self.pace.shift(ahead.clamp(-bound, bound) as i32);
        self.ticks_run = 0;
        self.sent_at = None;
        self.behind = false;
    }

    /// Another host: its margins, tick, speed and round trip are not ours
    /// to keep.
    fn forget_time_sync(&mut self) {
        self.stats.rtt_ms = None;
        self.sync = TimeSync::Free;
        self.sync_i = 0;
        self.sync_tick = None;
        self.pace.set_adjust_ppm(0);
        self.stats.adjust_ppm = 0;
        self.stats.margin = None;
    }
}
