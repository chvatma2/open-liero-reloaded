//! A Deathmatch world's state transfer (networking spec §3.5): the mode's
//! dynamic fields travel with the world's, and the mode refuses bytes it
//! cannot hold.

use crate::common;

use common::*;
use olr_modes::{Deathmatch, DeathmatchSettings};
use olr_sim::{
    EndReason, Fnv, GameMode, Keys, MAX_WORMS, MatchResult, ModeStateError, Score, World, WorldState,
};

const ARENA: [(i32, i32); 4] = [(50, LASER), (120, GRAZE), (200, BOMB), (300, LASER)];

/// Fire in turns, so worms kill, graze and bomb themselves (and respawn:
/// jump asks for it).
fn keys(t: u32) -> Vec<Keys> {
    (0..ARENA.len() as u32)
        .map(|i| match (t / 7 + i) % 4 {
            0 => Keys(Keys::FIRE),
            1 => Keys(Keys::JUMP),
            _ => Keys(0),
        })
        .collect()
}

fn dm() -> DeathmatchSettings {
    DeathmatchSettings { lives: 6, kill_limit: 40, ..DeathmatchSettings::default() }
}

#[test]
fn a_deathmatch_state_round_trips() {
    // Playing on, and over by the time limit (0.04 min: frame 241) before
    // the transfer, so the end travels too.
    for (dm, over) in [(dm(), false), (DeathmatchSettings { time_limit: 0.04, ..dm() }, true)] {
        let mut src = Run::new(world(dm.clone(), &ARENA));
        for t in 0..300 {
            src.tick(&keys(t));
        }
        assert!(src.kills().len() >= 2, "worms died: {:?}", src.kills());
        assert_eq!(src.w.result().is_some(), over);
        let bytes = src.w.state().encode();
        let mut dst = world(dm, &ARENA);
        dst.load_state(WorldState::decode(&bytes).unwrap()).unwrap();
        assert_eq!(dst, src.w);
        assert_eq!(dst.checksum(), src.w.checksum());
        assert_eq!(dst.scores(), src.w.scores());
        assert_eq!(mode(&dst).time_left_ms(), mode(&src.w).time_left_ms());
        let mut dst = Run::new(dst);
        dst.ticks = src.ticks;
        let seen = src.log.len();
        for t in 300..600 {
            src.tick(&keys(t));
            dst.tick(&keys(t));
        }
        assert_eq!(dst.w.checksum(), src.w.checksum());
        assert!(over || !dst.log.is_empty());
        assert_eq!(dst.log, src.log[seen..], "the same kills and messages");
    }
}

fn mode(w: &World) -> &Deathmatch {
    w.mode_state().as_any().downcast_ref().unwrap()
}

#[test]
fn deathmatch_refuses_bad_bytes() {
    let mut r = Run::new(world(dm(), &ARENA));
    for t in 0..100 {
        r.tick(&keys(t));
    }
    let bytes = mode(&r.w).save_state();
    let mut m = Deathmatch::resume(dm(), r.w.settings()).unwrap();
    m.load_state(&bytes).unwrap();
    assert_eq!(hash(&m), hash(mode(&r.w)));

    // The board is the last field: a fresh mode's ends in its empty length.
    let mut m = Deathmatch::resume(dm(), r.w.settings()).unwrap();
    let before = hash(&m);
    let empty = m.save_state();
    assert_eq!(empty.last(), Some(&0));
    let with_board = |rows: Vec<Score>| {
        let mut b = empty[..empty.len() - 1].to_vec();
        b.extend(postcard::to_allocvec(&rows).unwrap());
        b
    };
    let row =
        Score { worm: 0, score: 0, lives: 0, kills: 0, deaths: 0, suicides: 0, damage: 0.0, left: false };
    m.load_state(&with_board(vec![row; MAX_WORMS])).unwrap();
    let mut m = Deathmatch::resume(dm(), r.w.settings()).unwrap();
    assert_eq!(m.load_state(&with_board(vec![row; 40])), Err(ModeStateError::Invalid), "40 rows");
    let far = Score { worm: MAX_WORMS as i32, ..row };
    assert_eq!(m.load_state(&with_board(vec![far])), Err(ModeStateError::Invalid), "worm 32");
    assert_eq!(m.load_state(&with_board(vec![Score { worm: -1, ..row }])), Err(ModeStateError::Invalid));
    assert_eq!(m.load_state(&bytes[..bytes.len() / 2]), Err(ModeStateError::Decode), "truncated");
    // `over` comes right before `ended_at` and the board: a winner outside the worms.
    for (winner, ok) in [(-1, true), (31, true), (32, false), (-2, false)] {
        let over = MatchResult { winner, reason: EndReason::TimeLimit };
        let mut b = empty[..empty.len() - 3].to_vec(); // `over: None`, `ended_at: 0`, the empty board
        b.extend(postcard::to_allocvec(&(Some(over), 300u64, Vec::<Score>::new())).unwrap());
        let mut m = Deathmatch::resume(dm(), r.w.settings()).unwrap();
        let got = m.load_state(&b);
        assert_eq!(got.is_ok(), ok, "winner {winner}: {got:?}");
        if ok {
            assert_eq!(m.result(), Some(over));
        } else {
            assert_eq!(got, Err(ModeStateError::Invalid));
        }
    }
    assert_eq!(hash(&m), before, "a refused state changes nothing");
}

fn hash(m: &dyn GameMode) -> u64 {
    let mut h = Fnv::new();
    m.checksum(&mut h);
    h.finish()
}
