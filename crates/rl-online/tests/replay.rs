//! The match's replay from its confirmed inputs: a confirmed checksum the
//! re-simulation misses is a warning, and the replay is still made.

use crate::common;

use common::{content, setup, start};
use rl_netcode::Simulation;
use rl_online::matches;
use rl_online::replay::{MatchRecord, make};
use rl_replay::Replay;
use rl_sim::Keys;

/// 300 ticks of a start with changing keys, and the sync checksums a
/// session would confirm (after ticks `t` with `(t + 1) % 10 == 0`).
fn record(c: &rl_content::Content) -> MatchRecord {
    let start = start(setup(c, 0.1), 3);
    let mut sim = matches::build(c, &start).unwrap().sim;
    let (mut rows, mut checksums) = (Vec::new(), Vec::new());
    for t in 0..300u32 {
        let mut row = [0u8; 32];
        row[0] = ((t / 20).wrapping_mul(97) as u8) & !Keys::CHANGE;
        row[1] = ((t / 30).wrapping_mul(31) as u8) & !Keys::CHANGE;
        sim.tick(&[Keys(row[0]), Keys(row[1])]);
        if (t + 1) % 10 == 0 {
            checksums.push((t, sim.checksum()));
        }
        rows.push(row);
    }
    MatchRecord { start, rows, checksums }
}

#[test]
fn a_replay_of_the_confirmed_inputs_plays() {
    let Some(c) = content() else { return };
    let r = record(&c);
    let made = make(&c, &r).unwrap();
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
    let (made, kept) = (make(&c, &bad).unwrap(), make(&c, &good).unwrap());
    let warning = made.warning.expect("a warning");
    assert!(warning.contains("after tick 129"), "{warning}");
    assert_eq!(made.bytes, kept.bytes, "the same replay");
}
