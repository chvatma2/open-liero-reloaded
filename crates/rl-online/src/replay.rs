//! The match's replay (decision N18): a rollback session keeps the world
//! only at confirmed ticks, so after the match every peer re-simulates the
//! confirmed inputs from the start world on a worker thread, recording an
//! `rl_replay` replay; the netcode's confirmed checksums check the way.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use anyhow::{Context as _, anyhow};
use rl_content::Content;
use rl_proto::MatchStartWire;
use rl_sim::{Keys, TickEvents};

use crate::matches;

/// A finished match as one peer confirmed it: what its replay is made of.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchRecord {
    pub start: MatchStartWire,
    /// The final inputs of ticks `0..rows.len()`, one byte per slot.
    pub rows: Vec<[u8; 32]>,
    /// The session's confirmed sync checksums (`Session::checksums`) of
    /// those ticks.
    pub checksums: Vec<(u32, u64)>,
}

/// A replay made: its bytes, and a warning when the re-simulation missed a
/// confirmed checksum (a desync worth a bundle; the replay is still kept).
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub bytes: Vec<u8>,
    pub warning: Option<String>,
}

/// Re-simulates `record` from its start world and returns the encoded
/// `.rlrp`.
pub fn make(content: &Content, record: &MatchRecord) -> anyhow::Result<Made> {
    let options = matches::resolve(content, &record.start)?;
    let built = content.build_match(&options)?;
    let (mut world, mut recorder) = (built.world, built.recorder);
    let slots = record.start.slots.len();
    let expected: BTreeMap<u32, u64> = record.checksums.iter().copied().collect();
    let mut events = TickEvents::new();
    let mut keys = vec![Keys(0); slots];
    let mut warning = None;
    for (t, row) in record.rows.iter().enumerate() {
        for (k, &v) in keys.iter_mut().zip(row.iter()) {
            *k = Keys(v);
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

/// A replay being made on its own thread.
pub struct ReplayJob {
    rx: Receiver<Result<Made, String>>,
}

impl ReplayJob {
    /// Starts making `record`'s replay on a worker thread.
    pub fn spawn(content: Arc<Content>, record: MatchRecord) -> ReplayJob {
        let (tx, rx) = mpsc::channel();
        let work = move || {
            let made = make(&content, &record).map_err(|e| format!("{e:#}"));
            let _ = tx.send(made);
        };
        // A thread that cannot start drops its sender: `poll` reports it.
        let _ = std::thread::Builder::new().name("rl-online-replay".into()).spawn(work);
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
