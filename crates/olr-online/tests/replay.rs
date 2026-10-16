//! The match's replay from its confirmed inputs: a confirmed checksum the
//! re-simulation misses is a warning, and the replay is still made.

use crate::common;

use common::{content, setup, start};
use olr_netcode::Simulation;
use olr_online::matches;
use olr_online::replay::{MatchRecord, make};
use olr_replay::Replay;
use olr_sim::Keys;

/// 300 ticks of a start with changing keys, and the sync checksums a
/// session would confirm (after ticks `t` with `(t + 1) % 10 == 0`).
fn record(c: &olr_content::Content) -> MatchRecord {
    record_gone(c, None)
}

/// [`record`], slot 1's worm withdrawn before tick `gone` when given.
fn record_gone(c: &olr_content::Content, gone: Option<u32>) -> MatchRecord {
    let start = start(setup(c, 0.1), 3);
    let mut sim = matches::build(c, &start).unwrap().sim;
    let (mut rows, mut checksums) = (Vec::new(), Vec::new());
    for t in 0..300u32 {
        let mut row = [0u8; 32];
        row[0] = ((t / 20).wrapping_mul(97) as u8) & !Keys::CHANGE;
        row[1] = ((t / 30).wrapping_mul(31) as u8) & !Keys::CHANGE;
        if gone == Some(t) {
            sim.withdraw(1);
        }
        sim.tick(&[Keys(row[0]), Keys(row[1])]);
        if (t + 1) % 10 == 0 {
            checksums.push((t, sim.checksum()));
        }
        rows.push(row);
    }
    MatchRecord { start, rows, checksums, gone: gone.map(|t| (t, 1)).into_iter().collect() }
}

/// N83: a record's withdrawals replay at their ticks; without them the
/// re-simulation misses the match's checksums.
#[test]
fn a_withdrawal_replays_at_its_tick() {
    let Some(c) = content() else { return };
    let r = record_gone(&c, Some(150));
    let made = make(&c, &r, None).unwrap();
    assert_eq!(made.warning, None);
    let replay = Replay::decode(&made.bytes).unwrap();
    assert_eq!(replay.withdrawals, [(150, 1)]);
    c.play_replay(&replay).unwrap();
    let lost = MatchRecord { gone: vec![], ..r };
    let warning = make(&c, &lost, None).unwrap().warning.expect("a warning");
    assert!(warning.contains("after tick 159"), "{warning}");
}

#[test]
fn a_replay_of_the_confirmed_inputs_plays() {
    let Some(c) = content() else { return };
    let r = record(&c);
    let made = make(&c, &r, None).unwrap();
    assert_eq!(made.warning, None);
    let replay = Replay::decode(&made.bytes).unwrap();
    assert_eq!(replay.ticks(), 300);
    c.play_replay(&replay).unwrap();
}

#[test]
fn a_missed_checksum_warns_and_the_replay_is_kept() {
    let Some(c) = content() else { return };
    let good = record(&c);
    let mut bad = good.clone();
    bad.checksums[12].1 ^= 1;
    let (made, kept) = (make(&c, &bad, None).unwrap(), make(&c, &good, None).unwrap());
    let warning = made.warning.expect("a warning");
    assert!(warning.contains("after tick 129"), "{warning}");
    assert_eq!(made.bytes, kept.bytes, "the same replay");
}

/// The replay's viewer is the slot of the peer that made it: a player's
/// own, none for a spectator.
#[test]
fn the_replay_starts_on_the_peers_own_slot() {
    let Some(c) = content() else { return };
    let r = record(&c);
    let viewer = |slot| Replay::decode(&make(&c, &r, slot).unwrap().bytes).unwrap().setup.viewer;
    assert_eq!(viewer(Some(1)), Some(1));
    assert_eq!(viewer(None), None, "a spectator's");
}
