//! Unit tests of the rules on hand-made worms.
use alloc::vec::Vec;

use rl_math::{AbsTime, Vec2};
use rl_sim::{KillCredit, Message, Score, SimSettings, Worm};

use super::*;
use crate::rules::{compare, highest, olx_round, sort};
use crate::text::*;

fn worms(n: i32) -> Vec<Worm> {
    let s = SimSettings::lx56();
    (0..n).map(|i| Worm::spawned(i, Vec2::new(10.0 * i as f32, 0.0), AbsTime(100_000), &s)).collect()
}

fn row(worm: i32, score: i32, lives: i32, damage: f32) -> Score {
    Score { worm, score, lives, kills: score, deaths: 0, suicides: 0, damage }
}

fn said(s: &Said) -> Vec<Message> {
    s.iter().collect()
}

fn m(text: rl_sim::TextId, args: [i32; 3]) -> Message {
    Message { text, args }
}

#[test]
fn round_halves_down_like_olx() {
    // `(int)ceil((double)x - 0.5)`.
    assert_eq!([0.5, -0.5, 0.49, 0.51, -0.51, 1.5, -1.5].map(olx_round), [0, -1, 0, 1, -1, 1, -2]);
    assert_eq!(olx_round(f32::NAN), 0);
}

#[test]
fn compare_like_olx() {
    let (unlim, kl) = (-2, 15);
    // Damage within a half is a tie, keeping the earlier worm; exactly
    // -0.5 ranks the first below.
    assert_eq!(compare(&row(1, 3, -2, 10.5), &row(0, 3, -2, 10.0), unlim, kl), 0);
    assert_eq!(compare(&row(1, 3, -2, 10.0), &row(0, 3, -2, 10.5), unlim, kl), -1);
    assert_eq!(highest([row(0, 3, -2, 10.0), row(1, 3, -2, 10.5)].into_iter(), unlim, kl), 0);
    assert_eq!(highest([row(0, 3, -2, 10.0), row(1, 3, -2, 10.6)].into_iter(), unlim, kl), 1);
    // Not out beats out, then the score.
    assert_eq!(compare(&row(0, 9, -1, 0.0), &row(1, 0, 2, 0.0), unlim, kl), -1);
    // Limited lives: the kill limit first (both ways at once), then lives.
    assert_eq!(compare(&row(0, 15, 0, 0.0), &row(1, 15, 3, 0.0), 3, kl), 1);
    assert_eq!(compare(&row(1, 15, 3, 0.0), &row(0, 15, 0, 0.0), 3, kl), 1);
    assert_eq!(compare(&row(0, 9, 1, 0.0), &row(1, 0, 2, 0.0), 3, kl), -1);
    assert_eq!(highest([row(0, 15, 0, 0.0), row(1, 15, 3, 0.0), row(2, 0, 3, 0.0)].into_iter(), 3, kl), 1);
    // No worms, no winner.
    assert_eq!(highest(core::iter::empty(), unlim, kl), -1);
    // The results keep ties in id order.
    let mut rows = [row(0, 1, -2, 0.0), row(1, 2, -2, 0.0), row(2, 1, -2, 0.0), row(3, 2, -1, 0.0)];
    sort(&mut rows, unlim, kl);
    assert_eq!(rows.map(|r| r.worm), [1, 0, 2, 3]);
}

#[test]
fn kill_rows_and_messages_like_olx() {
    let s = SimSettings::lx56();
    let w = worms(3);
    let mut dm = Deathmatch::new(DeathmatchSettings { sprees: [2, 3, 7, 9, 10], ..Default::default() });
    // A death without a killer: no kill text, no first blood, a death in a row.
    let (c, t) = dm.kill(&w, &s, 1, -1);
    assert_eq!((c, said(&t)), (KillCredit::default(), Vec::new()));
    // A suicide: its text, no first blood, and it ends the worm's kill run.
    let (_, t) = dm.kill(&w, &s, 0, 1);
    assert_eq!(said(&t), [m(KILLED, [1, 0, -1]), m(FIRST_BLOOD, [1, 0, 0])]);
    let (_, t) = dm.kill(&w, &s, 2, 2);
    assert_eq!(said(&t), [m(SUICIDE, [2, 0, 0])]);
    assert_eq!(dm.kills_in_row[..3], [0, 1, 0]);
    let (_, t) = dm.kill(&w, &s, 2, 1);
    assert_eq!(said(&t), [m(KILLED, [1, 2, -1]), m(SPREE[0], [1, 0, 0])]);
    // Dying, worm 1 starts over; its death row was reset by its kills.
    let (_, t) = dm.kill(&w, &s, 1, 0);
    assert_eq!(said(&t), [m(KILLED, [0, 1, -1])]);
    assert_eq!((&dm.kills_in_row[..3], &dm.deaths_in_row[..3]), (&[1, 0, 0][..], &[0, 1, 2][..]));
    // A worm on its last life is out after the kill.
    let mut w = worms(2);
    w[1].lives = 0;
    let (_, t) = dm.kill(&w, &s, 1, 0);
    assert_eq!(said(&t).last(), Some(&m(PLAYER_OUT, [1, 0, 0])));
    // An unknown victim counts nothing.
    let (c, t) = dm.kill(&w, &s, 7, 0);
    assert_eq!((c, said(&t)), (KillCredit::default(), Vec::new()));
}

#[test]
fn check_order_like_olx() {
    let s = SimSettings::lx56();
    let rules = DeathmatchSettings { kill_limit: 2, time_limit: 0.05, ..Default::default() };
    let dm = Deathmatch::new(rules);
    let mut w = worms(3);
    assert!(dm.check(&w, &s, 300).is_none());
    // The time limit: strictly after 3000 ms.
    let e = dm.check(&w, &s, 301).unwrap();
    assert_eq!((e.result.reason, e.result.winner), (EndReason::TimeLimit, 0));
    // The kill limit comes first, named by the first worm (by id) at it.
    w[2].kills = 2;
    w[1].kills = 3;
    let e = dm.check(&w, &s, 301).unwrap();
    assert_eq!((e.result.reason, e.hit_by, e.result.winner), (EndReason::KillLimit, 1, 1));
    // Fewer than two worms not out; one worm alone plays on (a local game).
    let mut w = worms(3);
    w[0].lives = WRM_OUT;
    w[1].lives = WRM_OUT;
    let e = dm.check(&w, &s, 1).unwrap();
    assert_eq!((e.result.reason, e.result.winner), (EndReason::LastStanding, 2));
    assert!(dm.check(&worms(1), &s, 1).is_none());
    let mut w = worms(1);
    w[0].lives = WRM_OUT;
    assert_eq!(dm.check(&w, &s, 1).unwrap().result.winner, 0, "nobody in: the highest scored");
    // Off: a time limit of 0 or less, a kill limit of 0 or less.
    let off = Deathmatch::new(DeathmatchSettings { kill_limit: 0, time_limit: -0.15, ..Default::default() });
    let mut w = worms(2);
    w[0].kills = 1000;
    assert!(off.check(&w, &s, u64::MAX).is_none());
}
