//! A match's determinism and replay round trip (spec §7.1): the same
//! match played again gives the same keys and checksums, and its replay,
//! encoded, decoded and replayed from the content, reproduces every
//! recorded checksum.

use olr_content::Content;
use olr_replay::{Mismatch, Replay};

use crate::runner::{Failure, FailureKind, MatchRun, Options, play};

/// The first tick at which `a` and `b` differ: keys, a checksum or length.
pub fn first_divergence(a: &Replay, b: &Replay) -> Option<u32> {
    let ticks = a.ticks().min(b.ticks());
    let keys = (0..ticks).find(|&t| a.row(t) != b.row(t));
    let sums = a.checksums.iter().zip(&b.checksums).find(|(x, y)| x != y).map(|(x, y)| x.0.min(y.0));
    let len = (a.ticks() != b.ticks() || a.checksums.len() != b.checksums.len()).then_some(ticks);
    [keys, sums, len].into_iter().flatten().min()
}

/// Plays `run`'s match again and replays its recording: the first
/// difference is the failure. Errors are the content's (a missing pack).
pub fn verify(content: &Content, run: &MatchRun) -> anyhow::Result<Option<Failure>> {
    if let Some(f) = run.failure() {
        return Ok(Some(f.clone()));
    }
    let again = play(content, &run.spec, &Options::default())?;
    if let Some(tick) = first_divergence(&run.replay, &again.replay) {
        let message = format!("played again with the same seed, it went differently ({})", again.end);
        return Ok(Some(Failure { tick, kind: FailureKind::Determinism, message }));
    }
    let fail = |tick, message: String| Ok(Some(Failure { tick, kind: FailureKind::Replay, message }));
    let decoded = match Replay::decode(&run.replay.encode()) {
        Ok(r) if r == run.replay => r,
        Ok(_) => return fail(0, "the replay's encoding does not round-trip".into()),
        Err(e) => return fail(0, format!("the replay does not decode: {e}")),
    };
    let played = crate::assets::LevelAssets::load(content, &run.spec.mod_id, &run.spec.level_id)
        .and_then(|a| a.replay_world(content, &decoded))
        .and_then(|world| decoded.play(world).map_err(anyhow::Error::new));
    match played {
        Ok(r) if r.ticks == run.ticks && r.checked == decoded.checksums.len() => Ok(None),
        Ok(r) => fail(r.ticks, format!("the replay played {} of {} ticks", r.ticks, run.ticks)),
        Err(e) => match e.downcast_ref::<Mismatch>() {
            Some(m) => fail(m.tick, format!("the replay diverged: {m}")),
            None => Err(e),
        },
    }
}
