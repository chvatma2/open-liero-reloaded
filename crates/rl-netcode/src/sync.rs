//! Desync detection and repair (networking spec §3.5, §3.6): the sync
//! checksums of confirmed ticks, the host's view of each peer's, and a world
//! state's transfer in chunks on the reliable channel. Everything taken from
//! the network is bounded here: received ticks, lengths and chunk indices
//! only ever key maps or are compared, never index.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use rl_proto::{Control, MAX_STATE, STATE_CHUNK};

/// The host keeps a peer's reported checksums for at most this many ticks
/// past its own confirmed ones (spec: the last 1024 ticks' worth).
pub(crate) const REPORT_WINDOW: u32 = 1024;

/// One peer's sync checksums: those of ticks simulated but not yet
/// confirmed (`pending`), then confirmed ones in tick order.
#[derive(Default)]
pub(crate) struct Checks {
    pending: BTreeMap<u32, u64>,
    confirmed: Vec<(u32, u64)>,
}

impl Checks {
    /// The checksum after simulating `tick` (again: a re-simulation
    /// replaces it; a confirmed tick keeps its checksum).
    pub(crate) fn record(&mut self, tick: u32, hash: u64) {
        if tick >= self.confirmed_upto() {
            self.pending.insert(tick, hash);
        }
    }

    /// A rollback to `from`: the ticks from it on run again.
    pub(crate) fn rewind(&mut self, from: u32) {
        self.pending.split_off(&from);
    }

    /// Pending entries below `below` are confirmed; returns where the newly
    /// confirmed ones start in [`Checks::confirmed`].
    pub(crate) fn confirm(&mut self, below: u32) -> usize {
        let start = self.confirmed.len();
        let rest = self.pending.split_off(&below);
        let done = core::mem::replace(&mut self.pending, rest);
        // Ascending: `record` keeps nothing below the confirmed ones.
        self.confirmed.extend(done);
        start
    }

    /// A new timeline from the state after `tick` (a loaded state, or the
    /// simulation changed outside the timeline): nothing pending, nothing
    /// confirmed from `tick` on; `hash` is the state's checksum when `tick`
    /// is a checksum tick.
    pub(crate) fn restart(&mut self, tick: u32, hash: Option<u64>) {
        self.pending.clear();
        let keep = self.confirmed.partition_point(|&(t, _)| t < tick);
        self.confirmed.truncate(keep);
        if let Some(h) = hash {
            self.confirmed.push((tick, h));
        }
    }

    pub(crate) fn confirmed(&self) -> &[(u32, u64)] {
        &self.confirmed
    }

    /// The confirmed checksum of `tick`.
    pub(crate) fn get(&self, tick: u32) -> Option<u64> {
        let i = self.confirmed.binary_search_by_key(&tick, |&(t, _)| t).ok()?;
        Some(self.confirmed[i].1)
    }

    /// The first tick without a confirmed checksum (`0` before any).
    pub(crate) fn confirmed_upto(&self) -> u32 {
        self.confirmed.last().map_or(0, |&(t, _)| t.saturating_add(1))
    }
}

/// A loaded state that has not repaired a peer by the host's tick when the
/// peer said it loaded it (`Control::StateLoaded`) plus this many ticks (a
/// round trip and the prediction window, with room) is taken to have
/// failed: a later mismatch re-arms.
pub(crate) const REPAIR_MARGIN: u32 = 300;
/// A peer that has not said it loaded or refused a state this long after
/// it was sent (silent, or still downloading over a hopeless link) is
/// given up (`Event::RepairFailed`).
pub(crate) const LOAD_TIMEOUT_US: u64 = 60_000_000;
/// States sent to repair one peer before the host gives up on it
/// (`Event::RepairFailed`).
pub(crate) const MAX_REPAIRS: u8 = 3;

/// The host's repair of one peer (at most one in flight).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repair {
    /// In sync as far as known.
    Clean,
    /// A state is owed: sent at the end of the next advance that can.
    Owed,
    /// The state after `tick` was sent: the peer's reports differ until it
    /// loads it, so mismatches are ignored until one after `tick` matches
    /// (repaired) or one for a tick from `until` on differs (the peer
    /// diverged again: a new desync). `until` is set when the peer says it
    /// loaded the state ([`REPAIR_MARGIN`] past our tick then); without
    /// that by `deadline_us` (our time), the repair fails.
    Sent { tick: u32, until: Option<u32>, deadline_us: u64 },
    /// Given up ([`MAX_REPAIRS`], or a state too large to send): reports
    /// are ignored.
    Failed,
}

/// The host keeps the last this many of a peer's compared checksums (its
/// stream for a desync bundle: about 7 minutes of match at one per 10
/// ticks).
pub const PEER_STREAM_KEEP: usize = 4096;

/// The host's view of a peer's checksums.
pub(crate) struct Remote {
    /// Reported for ticks the host has not confirmed yet.
    pub(crate) reported: BTreeMap<u32, u64>,
    /// The peer's checksums the host compared with its own, oldest first
    /// (the last [`PEER_STREAM_KEEP`]).
    pub(crate) compared: VecDeque<(u32, u64)>,
    pub(crate) repair: Repair,
    /// States sent to repair it (a joining spectator's first is not one).
    pub(crate) repairs: u8,
}

impl Default for Remote {
    fn default() -> Self {
        Remote { reported: BTreeMap::new(), compared: VecDeque::new(), repair: Repair::Clean, repairs: 0 }
    }
}

/// A world state arriving in chunks.
pub(crate) struct Incoming {
    pub(crate) tick: u32,
    len: usize,
    chunks: u16,
    parts: BTreeMap<u16, Vec<u8>>,
    total: usize,
}

impl Incoming {
    /// A transfer announced by `StateBegin`: refused unless its length is
    /// at most [`MAX_STATE`] and it is cut into exactly the chunks
    /// [`state_controls`] makes.
    pub(crate) fn begin(tick: u32, len: u32, chunks: u16) -> Option<Incoming> {
        let len = len as usize;
        (len <= MAX_STATE && chunks as usize == len.div_ceil(STATE_CHUNK)).then(|| Incoming {
            tick,
            len,
            chunks,
            parts: BTreeMap::new(),
            total: 0,
        })
    }

    /// The announced number of chunks (`0`: an empty state, complete at once).
    pub(crate) fn chunks(&self) -> u16 {
        self.chunks
    }

    /// Takes chunk `index`: `Err` for an index out of range, a repeated
    /// chunk, one longer than [`STATE_CHUNK`] or more bytes than announced;
    /// `Ok(Some(state))` once complete.
    pub(crate) fn add(&mut self, index: u16, bytes: Vec<u8>) -> Result<Option<Vec<u8>>, ()> {
        if index >= self.chunks || self.parts.contains_key(&index) || bytes.len() > STATE_CHUNK {
            return Err(());
        }
        self.total = self.total.saturating_add(bytes.len());
        if self.total > self.len {
            return Err(());
        }
        self.parts.insert(index, bytes);
        if self.parts.len() < self.chunks as usize {
            return Ok(None);
        }
        if self.total != self.len {
            return Err(());
        }
        let mut out = Vec::with_capacity(self.len);
        for part in core::mem::take(&mut self.parts).into_values() {
            out.extend(part);
        }
        Ok(Some(out))
    }
}

/// The reliable messages that carry `state`, the state after `tick`
/// (`None` past [`MAX_STATE`]).
pub(crate) fn state_controls(tick: u32, state: &[u8]) -> Option<Vec<Control>> {
    if state.len() > MAX_STATE {
        return None;
    }
    let chunks = state.len().div_ceil(STATE_CHUNK);
    let mut out = Vec::with_capacity(chunks + 1);
    out.push(Control::StateBegin { tick, len: state.len() as u32, chunks: chunks as u16 });
    for (i, c) in state.chunks(STATE_CHUNK).enumerate() {
        out.push(Control::StateChunk { index: i as u16, bytes: c.to_vec() });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn checks_confirm_in_order_and_restart() {
        let mut c = Checks::default();
        c.record(9, 1);
        c.record(19, 2);
        c.rewind(19);
        c.record(19, 3);
        assert_eq!(c.confirm(15), 0);
        assert_eq!(c.confirmed(), &[(9, 1)]);
        c.record(29, 4);
        assert_eq!(c.confirm(30), 1);
        assert_eq!(c.confirmed(), &[(9, 1), (19, 3), (29, 4)]);
        assert_eq!(c.get(19), Some(3));
        assert_eq!(c.get(20), None);
        c.restart(19, Some(7));
        assert_eq!(c.confirmed(), &[(9, 1), (19, 7)]);
        assert_eq!(c.confirmed_upto(), 20);
    }

    #[test]
    fn a_state_round_trips_through_its_chunks() {
        let state: Vec<u8> = (0..2 * STATE_CHUNK + 5).map(|i| i as u8).collect();
        let controls = state_controls(41, &state).unwrap();
        let Control::StateBegin { tick, len, chunks } = controls[0] else { panic!() };
        let mut inc = Incoming::begin(tick, len, chunks).unwrap();
        let mut got = None;
        for c in controls[1..].iter().rev() {
            let Control::StateChunk { index, bytes } = c else { panic!() };
            got = inc.add(*index, bytes.clone()).unwrap();
        }
        assert_eq!(got, Some(state));
        assert!(Incoming::begin(0, 0, 0).is_some());
        assert!(Incoming::begin(0, 10, 2).is_none());
        assert!(Incoming::begin(0, MAX_STATE as u32 + 1, 2049).is_none());
        assert_eq!(Incoming::begin(0, 3, 1).unwrap().add(0, vec![0; 4]), Err(()));
    }
}
