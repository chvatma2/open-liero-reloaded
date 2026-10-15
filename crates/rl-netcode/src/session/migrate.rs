//! The session's side of a host migration (networking spec §3.7, decisions
//! N67, N69): suspension, the take-over, rejoins, the collection of what
//! each peer knows, the merge and the resume. Who takes over and who
//! rejoins whom is the drivers' part (the harness, rl-online); time is an
//! argument, as everywhere in the session.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;

use rl_proto::{
    ClientInputs, Control, Datagram, HostInputs, InputsReport, MAX_KEYS, MAX_REPORT_OWN, decode_frames,
    encode_control, encode_datagram,
};
use rl_sim::Keys;

use super::{MAX_AHEAD, PeerState, Role, Seat, Session, TimeSync};
use crate::clock::Pace;
use crate::inputs::SetResult;
use crate::migration::{KEEPALIVE_US, fit_report};
use crate::{Channel, Event, PeerId, Simulation};

/// A rejoined peer's report, as the new host has it.
enum Rejoin {
    /// `Control::Collect` sent, no `Control::Report` yet.
    Awaiting,
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
    /// taken.
    pub fn suspend(&mut self) {
        if !self.suspended {
            self.suspended = true;
            self.keepalive_us = None;
        }
    }

    pub fn suspended(&self) -> bool {
        self.suspended
    }

    /// The old host is back before any rehost (N69): advancing again, the
    /// pace restarting at `now_us` (the pause is not caught up). Nothing
    /// for a session that rehosted or took over (its resume ends the wait).
    pub fn unsuspend(&mut self, now_us: u64) {
        let collecting = self.collection.as_ref().is_some_and(|c| !c.resumed);
        if !self.suspended || self.rehosted || collecting {
            return;
        }
        self.suspended = false;
        self.restart_pace(now_us, 0);
    }

    /// Client or spectator: its host is now `host` (a new link). The host's
    /// state (acks, round trip, silence) starts afresh; the session stays
    /// suspended until `Control::Resume`, answers `Control::Collect` with
    /// `Control::Report`, and takes nothing from any other peer.
    pub fn rehost(&mut self, host: PeerId, now_us: u64) {
        match &mut self.role {
            Role::Client { host: h, .. } | Role::Spectator { host: h } => *h = host,
            Role::Host { .. } | Role::Joining { .. } => return,
        }
        self.peers.clear();
        self.peers.insert(host, PeerState::new(Seat::Spectator, now_us));
        self.out.retain(|&(to, _, _)| to == host);
        self.rehosted = true;
        self.suspend();
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
            c.rejoined.insert(peer, Rejoin::Awaiting);
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
            .filter(|(id, r)| matches!(r, Rejoin::Awaiting) && self.peers.get(id).is_some_and(|p| !p.lost))
            .map(|(&id, _)| id)
            .collect()
    }

    /// New host: the merge ends (N67; the order matters). Reported own
    /// inputs extend their slots; bot slots' missing ticks below R get their
    /// previous final value, its own slot its pressed keys; awaited slots
    /// nobody took become abandoned (`Keys(0)`, decision N6).
    /// `Control::Resume { tick }` to every rejoined peer; the pace restarts
    /// at the current tick and `now_us`. Returns that tick (R). Once only:
    /// later calls, and calls on a session that did not take over, change
    /// nothing and return the current tick.
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
        let ids: Vec<PeerId> = self.peers.iter().filter(|(_, p)| !p.lost).map(|(&id, _)| id).collect();
        for id in ids {
            // Our next datagram starts at its report's ack: what it lacks.
            if let Some(p) = self.peers.get_mut(&id) {
                p.resend = true;
            }
            self.push_out(id, Channel::Reliable, encode_control(&Control::Resume { tick: r }));
        }
        self.suspended = false;
        self.restart_pace(now_us, 0);
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
        let report = fit_report(log.frontier(), first, log.horizon(), slots, &value, &base, own_first, own);
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
    pub(super) fn on_report(&mut self, from: PeerId, r: InputsReport) {
        let Some(c) = self.collection.as_mut() else { return };
        if !matches!(c.rejoined.get(&from), Some(Rejoin::Awaiting)) {
            return;
        }
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
        let Seat::Player(s) = p.seat else { return };
        if resumed {
            self.extend_own(s, r.own_first, &r.own);
        } else if let Some(c) = self.collection.as_mut() {
            c.rejoined.insert(from, Rejoin::Reported { own_first: r.own_first, own: r.own });
        }
    }

    /// Client or spectator: the new host resumes at `tick`. The pace
    /// restarts at `max(current, tick)` and `now_us` (the pause is not
    /// caught up); time sync starts as after a jump. Only from a host we
    /// rehosted to, before its resume, and not more than `MAX_AHEAD` ticks
    /// past our current tick (a hostile or broken host cannot push our
    /// timeline to `u32::MAX`).
    pub(super) fn on_resume(&mut self, from: PeerId, tick: u32, now_us: u64) {
        let (Role::Client { host, .. } | Role::Spectator { host }) = self.role else { return };
        if from != host || !self.rehosted || tick > self.current.saturating_add(MAX_AHEAD) {
            return;
        }
        self.rehosted = false;
        self.suspended = false;
        self.restart_pace(now_us, tick.saturating_sub(self.current));
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
        if self.keepalive_us.is_some_and(|k| now_us.saturating_sub(k) < KEEPALIVE_US) {
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
    /// tick (the pause is not caught up), keeping its speed.
    fn restart_pace(&mut self, now_us: u64, ahead: u32) {
        let adjust = self.pace.adjust_ppm();
        self.pace = Pace::new(now_us);
        self.pace.set_adjust_ppm(adjust);
        self.pace.shift(ahead.min(MAX_AHEAD) as i32);
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
