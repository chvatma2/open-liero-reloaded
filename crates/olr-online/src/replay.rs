//! The match's replay (decision N18): a rollback session keeps the world
//! only at confirmed ticks, so after the match every peer re-simulates the
//! confirmed inputs from the start world on a worker thread, recording an
//! `olr_replay` replay; the netcode's confirmed checksums check the way.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use anyhow::{Context as _, anyhow};
use olr_content::Content;
use olr_proto::MatchStartWire;
use olr_sim::{Keys, TickEvents};

use crate::bundle::GameBundle;
use crate::events::OnlineEvent;
use crate::matches;

/// Whether two encoded replays are of the same match: equal but for the
/// viewer, which is each peer's own slot.
pub fn same_match(a: &[u8], b: &[u8]) -> bool {
    match (olr_replay::Replay::decode(a), olr_replay::Replay::decode(b)) {
        (Ok(mut a), Ok(mut b)) => {
            a.setup.viewer = None;
            b.setup.viewer = None;
            a == b
        }
        _ => false,
    }
}

/// A finished match as one peer confirmed it: what its replay is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchRecord {
    pub start: MatchStartWire,
    /// The final inputs of ticks `0..rows.len()`, one byte per slot.
    pub rows: Vec<[u8; 32]>,
    /// The session's confirmed sync checksums (`Session::checksums`) of
    /// those ticks.
    pub checksums: Vec<(u32, u64)>,
    /// The withdrawals at ticks below `rows.len()`, `(tick, slot)` (decision N83).
    pub gone: Vec<(u32, u8)>,
}

/// A replay made: its bytes, and a warning when the re-simulation missed a
/// confirmed checksum (a desync worth a bundle; the replay is still kept).
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub bytes: Vec<u8>,
    pub warning: Option<String>,
}

/// Re-simulates `record` from its start world and returns the encoded
/// `.rlrp`, whose viewer is `viewer`: the slot of the peer that made it
/// (`None`: a spectator, or nobody).
pub fn make(content: &Content, record: &MatchRecord, viewer: Option<u8>) -> anyhow::Result<Made> {
    let options = matches::resolve(content, &record.start)?;
    let built = content.build_match(&options)?;
    let (mut world, mut recorder) = (built.world, built.recorder);
    recorder.set_viewer(viewer);
    let slots = record.start.slots.len();
    let expected: BTreeMap<u32, u64> = record.checksums.iter().copied().collect();
    let mut events = TickEvents::new();
    let mut keys = vec![Keys(0); slots];
    let mut warning = None;
    for (t, row) in record.rows.iter().enumerate() {
        for (k, &v) in keys.iter_mut().zip(row.iter()) {
            *k = Keys(v);
        }
        // N83: each withdrawal before its tick, as every peer did.
        for &(_, s) in record.gone.iter().filter(|g| g.0 as usize == t) {
            recorder.withdraw(&mut world, s).map_err(|e| anyhow!("{e}")).context("recording a withdrawal")?;
        }
        world.tick(&keys, &mut events);
        recorder.push(&keys, &world).map_err(|e| anyhow!("{e}")).context("recording")?;
        let t = t as u32;
        if warning.is_none()
            && let Some(&hash) = expected.get(&t)
            && hash != world.sync_checksum()
        {
            warning = Some(format!(
                "the replay differs from the match after tick {t}: {:016x}, the match {hash:016x}",
                world.sync_checksum()
            ));
        }
    }
    let replay = recorder.finish(&world).map_err(|e| anyhow!("{e}")).context("finishing the replay")?;
    Ok(Made { bytes: replay.encode(), warning })
}

/// A finished match's replay being made (decision N18), as the peer hands
/// it out ([`crate::OnlinePeer::take_replay`]) so it survives the peer: the
/// job, what it is made of, and who made it (for a desync bundle when a
/// confirmed checksum is missed).
pub struct PendingReplay {
    job: ReplayJob,
    record: MatchRecord,
    host: bool,
    my_slot: Option<u8>,
}

impl PendingReplay {
    pub fn spawn(
        content: Arc<Content>,
        record: MatchRecord,
        host: bool,
        my_slot: Option<u8>,
    ) -> PendingReplay {
        let job = ReplayJob::spawn(content, record.clone(), my_slot);
        PendingReplay { job, record, host, my_slot }
    }

    /// `None` while it is being made; then what it gave, in order: a
    /// confirmed checksum the re-simulation missed (a desync too, spec
    /// §9.10: `DesyncBundle` and a `Notice`), then `ReplayReady`; or the
    /// failure's `Notice`. Once it gave them, the job is done: drop it.
    pub fn poll(&self) -> Option<Vec<OnlineEvent>> {
        let mut events = Vec::new();
        match self.job.poll()? {
            Ok(made) => {
                if let Some(w) = made.warning {
                    let bundle = GameBundle::of(self.record.clone(), w.clone(), self.host, self.my_slot);
                    events.push(OnlineEvent::DesyncBundle(bundle.encode()));
                    events.push(OnlineEvent::Notice(w));
                }
                events.push(OnlineEvent::ReplayReady(made.bytes));
            }
            Err(e) => events.push(OnlineEvent::Notice(format!("the replay failed: {e}"))),
        }
        Some(events)
    }
}

/// A replay being made on its own thread.
pub struct ReplayJob {
    rx: Receiver<Result<Made, String>>,
}

impl ReplayJob {
    /// Starts making `record`'s replay (its viewer `viewer`) on a worker thread.
    pub fn spawn(content: Arc<Content>, record: MatchRecord, viewer: Option<u8>) -> ReplayJob {
        let (tx, rx) = mpsc::channel();
        let work = move || {
            let made = make(&content, &record, viewer).map_err(|e| format!("{e:#}"));
            let _ = tx.send(made);
        };
        // A thread that cannot start drops its sender: `poll` reports it.
        let _ = std::thread::Builder::new().name("olr-online-replay".into()).spawn(work);
        ReplayJob { rx }
    }

    /// The result once there is one (a thread that died is an error).
    pub fn poll(&self) -> Option<Result<Made, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the replay's thread stopped".into())),
        }
    }
}
