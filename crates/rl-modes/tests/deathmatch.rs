//! Deathmatch in real worlds: scoring, game over, winner, lives and
//! messages, against OLX's `CGameMode` (`src/server/CGameMode.cpp`) and
//! `GameServer::SimulateGame` (`src/server/CServer_Game.cpp:167-261`).

use crate::common;

use common::*;
use rl_modes::text::*;
use rl_modes::{Deathmatch, DeathmatchError, DeathmatchSettings};
use rl_sim::{EndReason, EventKind, Keys, MatchResult, World};

fn dm() -> DeathmatchSettings {
    DeathmatchSettings::default()
}

/// The settled arena: worms stand on the floor after 150 ticks.
fn settled(dm: DeathmatchSettings, xs: &[(i32, i32)]) -> Run {
    let mut r = Run::new(world(dm, xs));
    r.idle(150);
    assert!(r.kills().is_empty() && r.game_overs().is_empty());
    r
}

fn stats(w: &World, i: usize) -> (i32, i32, i32, i32) {
    let w = &w.worms()[i];
    (w.kills, w.deaths, w.suicides, w.lives)
}

/// Review Focus 2: suicides at score 0 without `AllowNegativeScore`,
/// simultaneous kills in one tick, the kill limit reached by two worms in
/// one tick, a time-limit tie; plus last standing and lives first.
#[test]
fn deathmatch_game_over_and_winner_like_olx() {
    // A suicide at score 0 counts nothing without AllowNegativeScore
    // (`CWorm::addSuicide`/`addDeath`, `CWorm.cpp:1430-1442`); the message
    // is sent anyway.
    let mut r = settled(dm(), &[(50, BOMB), (300, LASER)]);
    r.fire(&[0], 10);
    assert_eq!(r.kills().iter().map(|k| (k.1, k.2)).collect::<Vec<_>>(), [(0, 0)]);
    assert_eq!(stats(&r.w, 0), (0, 0, 0, -2));
    assert_eq!(r.messages(), [msg(SUICIDE, [0, 0, 0])]);
    assert!(r.game_overs().is_empty());
    // With AllowNegativeScore and SuicideDecreasesScore it costs a point.
    let neg = DeathmatchSettings { allow_negative_score: true, suicide_decreases_score: true, ..dm() };
    let mut r = settled(neg, &[(50, BOMB), (300, LASER)]);
    r.fire(&[0], 10);
    assert_eq!(stats(&r.w, 0), (0, 1, 1, -2));
    assert_eq!(r.board(), [(1, 0), (0, -1)]);

    // Simultaneous kills: worm 0 kills worm 2 and worm 1 kills worm 3 in
    // one tick (the beams are delivered together). Worm 0's shot is first.
    let four = [(50, LASER), (200, LASER), (80, LASER), (230, LASER)];
    let mut r = settled(DeathmatchSettings { kill_limit: 2, ..dm() }, &four);
    r.fire(&[0, 1], 10);
    let kills = r.kills();
    assert_eq!(kills.iter().map(|k| (k.1, k.2)).collect::<Vec<_>>(), [(2, 0), (3, 1)]);
    assert_eq!(kills[0].0, kills[1].0, "one tick");
    assert_eq!(r.messages(), [msg(KILLED, [0, 2, -1]), msg(FIRST_BLOOD, [0, 0, 0]), msg(KILLED, [1, 3, -1])]);
    assert!(r.game_overs().is_empty());
    assert_eq!(r.board()[..2], [(0, 1), (1, 1)]);

    // The kill limit reached by two worms in that tick: the match ends in
    // the kill's own frame (`recheckGame` stays set from `BeginMatch` on, so
    // `CheckGameOver` runs every frame). The message names the first worm
    // by id at the limit; with unlimited lives the tie (score, damage)
    // keeps the lower id.
    let mut r = settled(DeathmatchSettings { kill_limit: 1, ..dm() }, &four);
    r.fire(&[0, 1], 10);
    let kill_tick = r.kills()[0].0;
    let over = MatchResult { winner: 0, reason: EndReason::KillLimit };
    assert_eq!(r.game_overs(), [(kill_tick, over)]);
    assert_eq!(r.w.result(), Some(over));
    assert_eq!(r.messages()[3..], [msg(KILL_LIMIT, [1, 0, 0]), msg(PLAYER_HAS_WON, [0, 0, 0])]);
    // With limited lives `CompareWormsScore` ranks any worm at the limit
    // first, whichever it is compared with, so the last one at the limit
    // (by id) replaces the leader and wins (`CGameMode.cpp:398-418`).
    let mut r = settled(DeathmatchSettings { kill_limit: 1, lives: 3, ..dm() }, &four);
    r.fire(&[0, 1], 10);
    assert_eq!(r.w.result(), Some(MatchResult { winner: 1, reason: EndReason::KillLimit }));
    assert_eq!(r.w.scores()[0].worm, 1, "the scoreboard agrees");

    // A time-limit tie: 0.05 min is 3000 ms, and the match ends in the
    // first frame with `serverFrame * 10 > 3000` (`CGameMode.cpp:305-314`).
    // Nobody scored and nobody was hurt: the lower id wins.
    let mut r =
        Run::new(world(DeathmatchSettings { time_limit: 0.05, ..dm() }, &[(50, LASER), (300, LASER)]));
    r.run_to_frame(300);
    assert!(r.game_overs().is_empty());
    r.tick(&[]);
    assert_eq!(r.w.server_frame(), 301);
    let over = MatchResult { winner: 0, reason: EndReason::TimeLimit };
    assert_eq!(r.game_overs(), [(300, over)]);
    assert_eq!(r.messages(), [msg(TIME_LIMIT, [0, 0, 0]), msg(PLAYER_HAS_WON, [0, 0, 0])]);
    // The damage score breaks a score tie: worm 1 grazed worm 0.
    let mut r = settled(DeathmatchSettings { time_limit: 0.05, ..dm() }, &[(200, LASER), (50, GRAZE)]);
    r.fire(&[1], 10);
    assert_eq!(r.w.worms()[1].damage, 30.0);
    r.run_to_frame(301);
    assert_eq!(r.w.result(), Some(MatchResult { winner: 1, reason: EndReason::TimeLimit }));

    // Last standing: with Lives 0 every worm dies once; two out in one tick
    // leave worm 1, which wins whatever the scores (`CGameMode::Winner`).
    let mut r = settled(DeathmatchSettings { lives: 0, ..dm() }, &[(50, LASER), (200, LASER), (300, LASER)]);
    r.w.set_health(0, 0.0);
    r.w.set_health(2, 0.0);
    r.tick(&[]);
    let over = MatchResult { winner: 1, reason: EndReason::LastStanding };
    assert_eq!(r.game_overs(), [(150, over)]);
    assert_eq!(
        r.messages(),
        [msg(PLAYER_OUT, [0, 0, 0]), msg(PLAYER_OUT, [2, 0, 0]), msg(PLAYER_HAS_WON, [1, 0, 0])]
    );

    // With limited lives, lives rank before the score: worm 0 has a kill but
    // one life fewer than worm 1 at the time limit.
    let lives = DeathmatchSettings { lives: 2, time_limit: 0.05, ..dm() };
    let mut r = settled(lives, &[(50, LASER), (300, LASER), (80, LASER)]);
    r.fire(&[0], 10);
    r.w.set_health(0, 0.0);
    r.run_to_frame(301);
    assert_eq!((stats(&r.w, 0), stats(&r.w, 1)), ((1, 1, 0, 1), (0, 0, 0, 2)));
    assert_eq!(r.w.result(), Some(MatchResult { winner: 1, reason: EndReason::TimeLimit }));
    assert_eq!(r.board(), [(1, 0), (0, 1), (2, 0)]);
}

/// Lives `N` let a worm die `N + 1` times; then it is out
/// (`WRM_OUT`), the server's respawn loop skips it
/// (`CServer_Game.cpp:204`) and the match goes on while two are left.
#[test]
fn lives_out_removes_worm_from_respawn() {
    let xs = [(50, LASER), (200, LASER), (300, LASER)];
    let mut r = settled(DeathmatchSettings { lives: 0, ..dm() }, &xs);
    r.w.set_health(2, 0.0);
    r.tick(&[]);
    assert_eq!(stats(&r.w, 2), (0, 0, 0, -1));
    assert_eq!(r.messages(), [msg(PLAYER_OUT, [2, 0, 0])]);
    r.idle(600);
    assert!(!r.w.worms()[2].alive, "an out worm never respawns");
    assert!(r.w.worms()[0].alive && r.w.worms()[1].alive);
    assert!(r.game_overs().is_empty(), "two worms are left");
    assert_eq!(r.w.scores().last().unwrap().worm, 2, "out ranks last");

    // With a life left, the worm respawns after RespawnTime (2.5 s).
    let mut r = settled(DeathmatchSettings { lives: 1, ..dm() }, &xs);
    r.w.set_health(2, 0.0);
    r.tick(&[]);
    assert!(!r.w.worms()[2].alive);
    r.idle(300);
    assert!(r.w.worms()[2].alive, "respawned");
    assert_eq!(stats(&r.w, 2), (0, 0, 0, 0));
    assert!(r.messages().is_empty());
}

/// The board counts every death and suicide, also at a score of 0, where
/// OLX's `iDeaths`/`iSuicides` (the worm's, which the score reads) count
/// nothing without `AllowNegativeScore` (`CWorm.cpp:1430-1442`); OLX never
/// shows them (`CClient::UpdateScore`). A user decision, in STATUS.md.
#[test]
fn board_counts_every_death_and_suicide() {
    let row = |r: &Run, worm: i32| {
        let s = r.w.scores().iter().find(|s| s.worm == worm).copied().expect("a row");
        (s.score, s.lives, s.kills, s.deaths, s.suicides)
    };
    // Lives 1: worm 2 dies twice at a score of 0, then it is out.
    let xs = [(50, LASER), (200, LASER), (300, LASER)];
    let mut r = settled(DeathmatchSettings { lives: 1, ..dm() }, &xs);
    r.w.set_health(2, 0.0);
    r.tick(&[]);
    r.idle_until(400, |w| w.worms()[2].alive);
    r.w.set_health(2, 0.0);
    r.tick(&[]);
    assert_eq!(stats(&r.w, 2), (0, 0, 0, -1), "OLX's gated counters stay");
    assert_eq!(row(&r, 2), (0, -1, 0, 2, 0), "both deaths shown");

    // A suicide at a score of 0: one death and one suicide on the board,
    // none on the worm, and the score stays 0.
    let mut r = settled(dm(), &[(50, BOMB), (300, LASER)]);
    r.fire(&[0], 10);
    assert_eq!(stats(&r.w, 0), (0, 0, 0, -2));
    assert_eq!(row(&r, 0), (0, -2, 0, 1, 1));
}

/// `CGameMode::Kill`'s messages (`CGameMode.cpp:75-206`): the kill, first
/// blood once, killing sprees at 3, 5, 7, 9 and 10 kills in a row (the
/// killer), dying sprees at as many deaths in a row (the victim, whoever
/// killed it), each threshold once per run.
#[test]
fn spree_messages() {
    // Worm 0 at the left kills worms 1..=10 lined up to its right, one at a
    // time; worm 11, behind it, dies ten times on its own.
    let mut xs = vec![(20, LASER)];
    xs.extend((1..=10).map(|i| (30 + 25 * i, LASER)));
    xs.push((5, LASER));
    let mut r = settled(dm(), &xs);
    // A beam shot sets `LastFire` to 0.3 s: one shot every 35 ticks. Early
    // victims respawn meanwhile, possibly into the line of fire.
    while r.kills().len() < 10 {
        r.fire(&[0], 34);
        assert!(r.ticks < 2000);
    }
    let kills = r.kills();
    assert!(kills.iter().all(|k| k.2 == 0 && k.1 != 0 && k.1 != 11), "{kills:?}");
    let mut want = vec![];
    for (n, k) in kills.iter().enumerate() {
        want.push(msg(KILLED, [0, k.1, -1]));
        if n == 0 {
            want.push(msg(FIRST_BLOOD, [0, 0, 0]));
        }
        if let Some(i) = [3, 5, 7, 9, 10].iter().position(|&t| t == n + 1) {
            want.push(msg(SPREE[i], [0, 0, 0]));
        }
    }
    assert_eq!(r.messages(), want);

    // Ten uncredited deaths in a row: dying sprees, no kill messages.
    r.log.clear();
    let mut deaths = 0;
    while deaths < 10 {
        if r.w.worms()[11].alive {
            r.w.set_health(11, 0.0);
            deaths += 1;
        }
        r.tick(&[]);
    }
    let want: Vec<_> = (0..5).map(|i| msg(DYING_SPREE[i], [11, 0, 0])).collect();
    assert_eq!(r.messages(), want);
    assert_eq!(r.w.worms()[11].deaths, 0, "no deaths count at score 0");
}

/// F5: a Deathmatch needs two worms; `prepare` applies the rules to the
/// match's settings and the worms' lives (`Game.cpp:851`).
#[test]
fn prepare_applies_rules_and_needs_two_worms() {
    let mut s = setup(&[(50, LASER)]);
    assert_eq!(Deathmatch::prepare(dm(), &mut s).err(), Some(DeathmatchError::TooFewWorms(1)));
    let mut s = setup(&[(50, LASER), (100, LASER)]);
    let rules = DeathmatchSettings {
        lives: 4,
        kill_limit: 7,
        allow_negative_score: true,
        death_decreases_score: 0.5,
        suicide_decreases_score: true,
        ..dm()
    };
    Deathmatch::prepare(rules, &mut s).unwrap();
    let st = &s.settings;
    assert_eq!((st.lives, st.kill_limit, st.allow_negative_score), (4, 7, true));
    assert_eq!((st.death_decreases_score, st.suicide_decreases_score), (0.5, true));
    assert!(s.worms.iter().all(|w| w.loadout.lives == 4));
    let mut s = setup(&[(50, LASER), (100, LASER)]);
    let bad = DeathmatchSettings { time_limit: f32::NAN, ..dm() };
    assert!(matches!(Deathmatch::prepare(bad, &mut s), Err(DeathmatchError::TimeLimit(_))));
}

/// The mode's state is in the checksum, a clone plays on identically, and
/// a whole match with kills, messages and the end does not allocate.
#[test]
fn deathmatch_is_deterministic_and_allocation_free() {
    let four = [(50, LASER), (200, LASER), (80, LASER), (230, LASER)];
    let mut r = settled(DeathmatchSettings { kill_limit: 2, time_limit: 0.1, ..dm() }, &four);
    let mut c = Run::new(r.w.clone());
    c.ticks = r.ticks;
    let fire = [Keys(Keys::FIRE), Keys(Keys::FIRE), Keys(0), Keys(0)];
    let (w, out) = (&mut r.w, &mut r.out);
    let (mut messages, mut ends) = (0, 0);
    let info = allocation_counter::measure(|| {
        w.tick(&fire, out);
        for _ in 0..610 {
            w.tick(&[], out);
            for e in out.iter() {
                match e.kind {
                    EventKind::Message(_) => messages += 1,
                    EventKind::GameOver(_) => ends += 1,
                    _ => {}
                }
            }
        }
    });
    assert_eq!(info.count_total, 0, "a Deathmatch tick allocated");
    assert_eq!((messages, ends), (5, 1), "two kills, first blood, time limit, winner");
    assert_eq!(r.w.result().map(|m| m.reason), Some(EndReason::TimeLimit));
    c.fire(&[0, 1], 0);
    c.idle(610);
    assert_eq!(c.w.checksum(), r.w.checksum());
    assert_eq!(c.messages().len(), 5);
    // The mode's state and settings are in the checksum.
    assert_eq!(world(dm(), &four).checksum(), world(dm(), &four).checksum());
    let other = DeathmatchSettings { kill_limit: 3, ..dm() };
    assert_ne!(world(dm(), &four).checksum(), world(other, &four).checksum());
    let mode = r.w.mode_state().as_any().downcast_ref::<Deathmatch>().unwrap();
    assert_eq!(mode.result(), r.w.result());
    assert_eq!(mode.time_left_ms(), Some(0));
}

/// Plan 6's final review: the clock stops at the end. `time_left_ms` is
/// the time left in the game-over frame from then on, so a HUD or a
/// results screen reads one number however long the end is shown.
#[test]
fn time_left_freezes_at_the_game_over_frame() {
    let four = [(50, LASER), (200, LASER), (80, LASER), (230, LASER)];
    let mut r = settled(DeathmatchSettings { kill_limit: 1, time_limit: 1.0, ..dm() }, &four);
    r.fire(&[0], 10);
    assert_eq!(r.game_overs().len(), 1, "the kill limit ended the match");
    let left = |r: &Run| r.w.mode_state().as_any().downcast_ref::<Deathmatch>().unwrap().time_left_ms();
    let at_end = left(&r).unwrap();
    let end_frame = r.w.server_frame();
    assert!(at_end > 0 && at_end < 60_000, "{at_end} ms left at the end");
    r.idle(500);
    assert!(r.w.server_frame() > end_frame);
    assert_eq!(left(&r), Some(at_end), "the clock stopped at the game over");
}

/// Plan 6's final review: one source of rules. A Deathmatch is made by
/// `prepare` (which writes its rules into the match's settings) or, for a
/// setup made that way before (a replay, an oracle trace), by `resume`,
/// which refuses settings that disagree with the rules.
#[test]
fn resume_refuses_rules_that_disagree_with_the_settings() {
    let mut s = setup(&[(50, LASER), (100, LASER)]);
    let rules = DeathmatchSettings { lives: 4, kill_limit: 7, death_decreases_score: 0.5, ..dm() };
    Deathmatch::prepare(rules.clone(), &mut s).unwrap();
    let again = Deathmatch::resume(rules.clone(), &s.settings).unwrap();
    assert_eq!(again.settings(), &rules);
    for other in [
        DeathmatchSettings { lives: 3, ..rules.clone() },
        DeathmatchSettings { kill_limit: 8, ..rules.clone() },
        DeathmatchSettings { allow_negative_score: true, ..rules.clone() },
        DeathmatchSettings { death_decreases_score: 0.25, ..rules.clone() },
        DeathmatchSettings { suicide_decreases_score: true, ..rules.clone() },
    ] {
        assert_eq!(Deathmatch::resume(other, &s.settings).err(), Some(DeathmatchError::Disagrees));
    }
    let nan = DeathmatchSettings { time_limit: f32::NAN, ..rules };
    assert!(matches!(Deathmatch::resume(nan, &s.settings), Err(DeathmatchError::TimeLimit(_))));
}

/// N84, N85, user decision 3: a withdrawn worm's row is a left one; with
/// fewer than two worms not out the match ends at once and the last wins
/// (`CGameMode::CheckGameOver`, `src/server/CGameMode.cpp:316-336`).
#[test]
fn a_withdrawn_worm_leaves_the_board_and_the_last_one_wins() {
    let mut r = settled(dm(), &[(50, LASER), (200, LASER), (300, LASER)]);
    r.w.withdraw(1);
    r.idle(1);
    let rows: Vec<(i32, bool)> = r.w.scores().iter().map(|s| (s.worm, s.left)).collect();
    assert_eq!(rows.iter().filter(|r| r.1).collect::<Vec<_>>(), [&(1, true)]);
    assert!(r.w.result().is_none(), "two worms play on");
    r.w.withdraw(2);
    r.idle(1);
    assert_eq!(r.w.result(), Some(MatchResult { winner: 0, reason: EndReason::LastStanding }));
}

/// A worm that left never wins (`CGameMode::HighestScoredWorm`,
/// `src/server/CGameMode.cpp:398-407`, iterates the worms still in the
/// game): here the last two go out in one tick, and the one that left has
/// the best damage score.
#[test]
fn a_withdrawn_worm_never_wins() {
    let lives = DeathmatchSettings { lives: 0, ..dm() };
    let mut r = settled(lives, &[(200, LASER), (50, GRAZE), (300, LASER)]);
    r.fire(&[1], 10);
    assert!(r.w.worms()[1].damage > 0.0, "worm 1 grazed worm 0");
    r.w.withdraw(1);
    r.idle(1);
    assert!(r.w.result().is_none(), "two worms play on");
    r.w.set_health(0, 0.0);
    r.w.set_health(2, 0.0);
    r.idle(2);
    let result = r.w.result().expect("both went out: the match is over");
    assert_eq!(result.reason, EndReason::LastStanding);
    assert_ne!(result.winner, 1, "the worm that left does not win");
    assert_eq!(result.winner, 0, "a tie among the worms still in: the first");
}

/// N84 (ruling PF12d-2): a projectile in flight when its worm leaves still
/// explodes and kills, crediting nobody. A beam cannot show it: its shot is
/// dropped when its worm is dead at delivery.
#[test]
fn a_projectile_of_a_worm_that_left_credits_nobody() {
    // Worm 0 throws a bomb at its feet (the suicide layout of
    // `deathmatch_game_over_and_winner_like_olx`); worm 1 stands in the
    // blast (within 5 px: `CClient::Explosion`'s reach); worm 2 far away
    // keeps two worms in the match.
    let mut r = settled(dm(), &[(50, BOMB), (52, LASER), (300, LASER)]);
    r.fire(&[0], 0);
    r.idle_until(20, |w| w.projectiles().next().is_some()); // delivered: in flight
    r.w.withdraw(0);
    r.idle(200);
    let kills: Vec<(i32, i32)> = r.kills().iter().map(|k| (k.1, k.2)).collect();
    assert!(kills.contains(&(1, -1)), "the bomb killed worm 1, credited to nobody: {kills:?}");
    assert_eq!(r.w.worms()[0].kills, 0);
}
