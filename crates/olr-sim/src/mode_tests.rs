//! Unit tests of the match API: setup, game modes and their commands.
use super::*;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_terrain::{PX_EMPTY, Stamp, Terrain};

use crate::damage::kill_worm;
use crate::olx_kill_credit;
use crate::{
    Event, EventKind, Keys, Loadout, MatchSetup, SetupError, SimSettings, Slot, TerrainOp, TickEvents, World,
    Worm, WormSetup,
};

fn holes() -> Vec<Stamp> {
    [5, 7, 9, 11, 15]
        .into_iter()
        .map(|size| {
            let mut img = RgbaImage::new(size, size);
            for y in 0..size {
                for x in 0..size {
                    img.set(x, y, Rgba::MAGENTA);
                }
            }
            Stamp::from_image(&img)
        })
        .collect()
}

fn setup(settings: SimSettings, fill: Material) -> MatchSetup {
    let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, fill));
    MatchSetup {
        holes: holes(),
        seed: 1,
        ..MatchSetup::new(settings, crate::mod_rt::tests::test_mod(), terrain)
    }
}

fn worm_at(x: i32, y: i32) -> WormSetup {
    let loadout = Loadout::new(vec![Slot::ready(0); 5], &SimSettings::lx56());
    WormSetup { team: 0, loadout, spawn: Some((x, y)) }
}

/// The crediting `olr-sim` hard-coded before plan 6 (`damage.rs`
/// `kill_worm`, `score`, `counts`), kept here as the reference.
fn hard_coded_kill(worms: &mut [Worm], s: &SimSettings, victim: i32, killer: i32) {
    fn score(w: &Worm, s: &SimSettings) -> i32 {
        let mut score = w.kills;
        if s.death_decreases_score > 0.0 {
            score = score
                .wrapping_sub(crate::projectile::c_int(w.deaths as f32 * s.death_decreases_score + 0.01));
        }
        if s.suicide_decreases_score {
            score = score.wrapping_sub(w.suicides);
        }
        score
    }
    let counts = |w: &Worm| s.allow_negative_score || score(w, s) > 0;
    let index = |id: i32| worms.iter().position(|w| w.id == id && (0..32).contains(&id));
    let Some(vi) = index(victim) else { return };
    let ki = index(killer);
    if worms[vi].lives < 0 && s.lives >= 0 {
        worms[vi].lives = -1;
        return;
    }
    if let Some(k) = ki {
        if k == vi {
            if counts(&worms[k]) {
                worms[k].suicides += 1;
            }
        } else {
            worms[k].kills += 1;
        }
    }
    let w = &mut worms[vi];
    w.alive = false;
    w.can_respawn_now = false;
    w.respawn_requested = false;
    if counts(w) {
        w.deaths += 1;
    }
    if w.lives != -2 {
        w.lives -= 1;
    }
}

#[test]
fn oracle_mode_matches_the_hard_coded_credit() {
    let mut cases = 0;
    for allow_negative_score in [false, true] {
        for death_decreases_score in [0.0, 0.5, 1.0] {
            for suicide_decreases_score in [false, true] {
                for lives in [-2, 3] {
                    let s = SimSettings {
                        allow_negative_score,
                        death_decreases_score,
                        suicide_decreases_score,
                        lives,
                        ..SimSettings::lx56()
                    };
                    let mut m = setup(s.clone(), Material::Background);
                    m.worms = vec![worm_at(50, 100), worm_at(150, 100)];
                    let base = World::new(m, Box::new(OracleMode)).unwrap();
                    for (kills, deaths, suicides) in [(0, 0, 0), (1, 0, 0), (1, 2, 0), (2, 1, 1), (3, 3, 2)] {
                        for victim_lives in [-2, -1, 0, 2] {
                            for killer in [0, 1, -1, 40] {
                                let mut w = base.clone();
                                for worm in &mut w.worms[..] {
                                    (worm.kills, worm.deaths, worm.suicides) = (kills, deaths, suicides);
                                }
                                w.worms[0].lives = victim_lives;
                                let mut expected: Vec<Worm> = w.worms.to_vec();
                                hard_coded_kill(&mut expected, &s, 0, killer);
                                kill_worm(&mut w.harm(), 0, killer);
                                for e in &mut expected {
                                    // `CWorm::Kill` sets the time of death.
                                    if !e.alive {
                                        e.time_of_death = w.time();
                                    }
                                }
                                assert_eq!(
                                    &w.worms[..],
                                    &expected[..],
                                    "{s:?} {kills} {deaths} {suicides} {killer}"
                                );
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases, 2 * 3 * 2 * 2 * 5 * 4 * 4);
}

#[test]
fn world_new_rejects_invalid_settings() {
    let ok = |m: MatchSetup| World::new(m, Box::new(OracleMode));
    let mut s = SimSettings::lx56();
    s.rope_speed = f32::NAN;
    assert!(matches!(ok(setup(s, Material::Background)), Err(SetupError::Settings(_))));
    let mut s = SimSettings::lx56();
    s.lx56_physics_fps = 0;
    assert!(matches!(ok(setup(s, Material::Background)), Err(SetupError::Settings(_))));

    let mut m = setup(SimSettings::lx56(), Material::Background);
    m.worms = vec![worm_at(10, 10); 33];
    assert_eq!(ok(m).unwrap_err(), SetupError::TooManyWorms(33));
    for capacity in [0, MatchSetup::MAX_POOL_CAPACITY + 1] {
        let mut m = setup(SimSettings::lx56(), Material::Background);
        m.pool_capacity = capacity;
        assert_eq!(ok(m).unwrap_err(), SetupError::PoolCapacity(capacity));
    }
    let mut m = setup(SimSettings::lx56(), Material::Background);
    let mut six = worm_at(10, 10);
    six.loadout.slots.push(Slot::ready(0));
    m.worms = vec![worm_at(20, 20), six];
    assert_eq!(ok(m).unwrap_err(), SetupError::TooManySlots { worm: 1, slots: 6 });
    let mut m = setup(SimSettings::lx56(), Material::Background);
    m.worms = vec![worm_at(10, 10); 32];
    assert_eq!(ok(m).unwrap().worms().len(), 32);
}

/// A mode that runs a script of commands by tick, and remembers what it saw.
#[derive(Clone, Debug, Default)]
struct Script {
    ticks: u32,
    spawned: Vec<i32>,
    errors: Vec<CommandError>,
}

const THE_END: MatchResult = MatchResult { winner: 1, reason: EndReason::Other(9) };
const HELLO: Message = Message { text: TextId(7), args: [1, 2, 3] };

impl GameMode for Script {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(self.clone())
    }

    fn checksum(&self, h: &mut Fnv) {
        h.u32(self.ticks);
        h.usize(self.spawned.len());
        self.spawned.iter().for_each(|&id| h.i32(id));
    }

    fn on_spawn(&mut self, worm: i32, _ctx: &mut ModeCtx) {
        self.spawned.push(worm);
    }

    /// Tries every command: `on_drop` may give none.
    fn on_drop(&mut self, _worm: i32, ctx: &mut ModeCtx) {
        let results = [
            ctx.spawn_worm(&worm_at(150, 100)),
            ctx.give_weapon(0, 1, 0),
            ctx.stamp(StampOp::Carve { hole: 2 }, 100, 50),
            ctx.message(HELLO),
            ctx.end(THE_END),
        ];
        self.errors.extend(results.into_iter().filter_map(Result::err));
    }

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        olx_kill_credit(ctx.worms(), ctx.settings(), victim, killer)
    }

    fn on_tick(&mut self, ctx: &mut ModeCtx) {
        self.ticks += 1;
        let mut run = |r: Result<(), CommandError>| {
            if let Err(e) = r {
                self.errors.push(e);
            }
        };
        match self.ticks {
            3 => {
                let mut new = worm_at(150, 100);
                new.team = 1;
                run(ctx.spawn_worm(&new));
                run(ctx.give_weapon(0, 1, 0));
                run(ctx.stamp(StampOp::Carve { hole: 2 }, 100, 50));
                run(ctx.message(HELLO));
                run(ctx.give_weapon(0, 1, 5));
            }
            5 => run(ctx.end(THE_END)),
            _ => {}
        }
    }
}

fn scripted() -> World {
    let mut m = setup(SimSettings::lx56(), Material::Dirt);
    let mut first = worm_at(50, 100);
    first.loadout.slots[1] = Slot { weapon: 0, charge: 0.25, reloading: true, last_fire: 0.5 };
    m.worms = vec![first];
    World::new(m, Box::new(Script::default())).unwrap()
}

fn run(mut w: World, ticks: usize) -> (World, Vec<(u64, Vec<Event>)>) {
    let mut out = TickEvents::new();
    let mut log = Vec::new();
    for _ in 0..ticks {
        w.tick(&[Keys(Keys::RIGHT)], &mut out);
        log.push((w.checksum(), out.as_slice().to_vec()));
    }
    (w, log)
}

#[test]
fn mode_commands_apply_inside_tick_deterministically() {
    let (w, log) = run(scripted(), 7);
    // Tick 3 (index 2): the commands after the physics and the server, in queue order.
    let last = w.substeps_last_tick();
    let tail: Vec<EventKind> =
        log[2].1.iter().skip_while(|e| e.kind != EventKind::Spawned { worm: 1 }).map(|e| e.kind).collect();
    assert!(
        matches!(tail[1], EventKind::TerrainEdit { op: TerrainOp::Carve, x: 150, y: 100, .. }),
        "{tail:?}"
    );
    assert!(
        matches!(tail[3], EventKind::TerrainEdit { op: TerrainOp::Carve, x: 100, y: 50, .. }),
        "{tail:?}"
    );
    assert_eq!(tail[4], EventKind::Message(HELLO));
    assert_eq!(tail.len(), 5, "{tail:?}");
    assert!(log[2].1.iter().rev().take(5).all(|e| e.substep == last));
    assert_eq!(w.worms().len(), 2);
    assert_eq!(w.worms()[1].team, 1);
    assert_eq!(w.worms()[0].slots[1], Slot::ready(0));
    assert_eq!(w.terrain().flags(100, 50) & PX_EMPTY, PX_EMPTY, "the stamp carved");
    // Tick 5: game over, once.
    let over = |i: usize| log[i].1.iter().filter(|e| e.kind == EventKind::GameOver(THE_END)).count();
    assert_eq!((over(3), over(4), over(5), over(6)), (0, 1, 0, 0));
    assert_eq!(w.result(), Some(THE_END));
    // The mode saw both spawns, and the bad weapon was refused.
    let mut h = Fnv::new();
    w.mode_state().checksum(&mut h);
    let mut expected = Fnv::new();
    Script { ticks: 7, spawned: vec![0, 1], errors: vec![] }.checksum(&mut expected);
    assert_eq!(h.finish(), expected.finish());

    // The same setup gives the same run, and so does a clone from tick 2.
    let (_, again) = run(scripted(), 7);
    assert_eq!(log, again);
    let (mid, first) = run(scripted(), 2);
    let (_, rest) = run(mid.clone(), 5);
    assert_eq!([first, rest].concat(), log);
    // The mode's state is part of the checksum.
    let mut other = mid.clone();
    other.referee.mode = Box::new(Script { ticks: 99, ..Script::default() });
    assert_ne!(other.checksum(), mid.checksum());
}

#[test]
fn commands_are_checked_when_queued() {
    let w = scripted();
    let mut r = w.referee.clone();
    let mut ctx = r.ctx(&w.worms, &w.settings, w.time);
    assert_eq!(ctx.give_weapon(0, 0, 1), Err(CommandError::NoSuchWeapon(1)));
    assert_eq!(ctx.give_weapon(0, MAX_SLOTS, 0), Err(CommandError::NoSuchSlot(MAX_SLOTS)));
    assert_eq!(ctx.stamp(StampOp::Dirt { hole: 5 }, 0, 0), Err(CommandError::NoSuchHole(5)));
    let mut six = worm_at(1, 1);
    six.loadout.slots.push(Slot::ready(0));
    assert_eq!(ctx.spawn_worm(&six), Err(CommandError::TooManySlots(6)));
    for _ in 0..COMMAND_CAPACITY {
        ctx.message(HELLO).unwrap();
    }
    assert_eq!(ctx.message(HELLO), Err(CommandError::Full));
}

#[test]
fn game_over_stops_kills_spawns_input_and_bonuses() {
    // OLX: `killWorm` and `SpawnWorm` return while `gameOver`, input is
    // cleared, shooting stops, no bonus spawns, and the client clears its
    // projectiles when it parses `S2C_GAMEOVER`.
    let s = SimSettings { bonuses: true, bonus_frequency: 1.0, ..SimSettings::lx56() };
    let mut m = setup(s, Material::Background);
    m.worms = vec![worm_at(50, 100), worm_at(150, 100)];
    let mut w = World::new(m, Box::new(Script::default())).unwrap();
    let mut out = TickEvents::new();
    for _ in 0..5 {
        w.tick(&[Keys(Keys::FIRE | Keys::UP)], &mut out);
    }
    assert_eq!(w.result(), Some(THE_END), "tick 5: the end");
    let before: Vec<u32> = w.projectiles().map(|(_, p)| p.serial).collect();
    assert!(!before.is_empty());
    w.tick(&[Keys(Keys::FIRE | Keys::UP)], &mut out);
    // `ParseGameOver` clears them; the shots of tick 5 are processed after it.
    assert!(w.projectiles().all(|(_, p)| !before.contains(&p.serial)), "cleared at the next delivery");
    let (angle, kills) = (w.worms()[0].angle, w.worms()[1].kills);
    kill_worm(&mut w.harm(), 0, 1);
    assert!(w.worms()[0].alive && w.worms()[1].kills == kills, "kills are ignored");
    for i in 0..300 {
        w.tick(&[Keys(Keys::FIRE | Keys::UP | Keys::LEFT)], &mut out);
        // Model R delivers the shots of two frames at once: the last are tick 5's.
        let fired = out.iter().any(|e| matches!(e.kind, EventKind::WeaponFired { .. }));
        assert!(i == 0 || !fired, "tick {}: no shots after the end", i + 7);
    }
    assert_eq!(w.worms()[0].angle, angle, "no input");
    assert!(w.server_bonuses().iter().all(|b| !b.used), "no bonus spawns");
    // Three seconds after the end (`GAMEOVER_WAIT`), the physics stops: its
    // clock is set to the frame's.
    for _ in 0..3 {
        w.tick(&[], &mut out);
        assert_eq!(w.physics_time() + w.frame_dt(), w.time());
    }
    w.worms[1].alive = false;
    w.worms[1].time_of_death = olr_math::AbsTime(0);
    for _ in 0..10 {
        w.tick(&[], &mut out);
    }
    assert!(!w.worms()[1].alive, "no respawn");
}

#[test]
fn checksum_canonicalises_nan() {
    let mut m = setup(SimSettings::lx56(), Material::Background);
    m.worms = vec![worm_at(50, 100)];
    let w = World::new(m, Box::new(OracleMode)).unwrap();
    let with = |bits: u32| {
        let mut v = w.clone();
        v.worms[0].health = f32::from_bits(bits);
        v.checksum()
    };
    assert_eq!(with(0x7fc0_0000), with(0xffc0_1234), "every NaN hashes alike");
    assert_eq!(with(0x7fc0_0000), with(0x7f80_0001));
    assert_ne!(with(0x7fc0_0000), with(0x0000_0000));
    assert_ne!(with(0x0000_0000), with(0x8000_0000), "-0 and +0 still differ");
}

#[test]
fn pool_capacity_is_a_setting() {
    let m = setup(SimSettings::lx56(), Material::Background);
    assert_eq!(m.pool_capacity, 3000);
    assert_eq!(m.start, MatchSetup::T0);
    assert_eq!(World::new(m, Box::new(OracleMode)).unwrap().projectile_capacity(), 3000);
    let mut m = setup(SimSettings::lx56(), Material::Background);
    m.pool_capacity = 4;
    m.worms = vec![worm_at(50, 100)];
    let mut w = World::new(m, Box::new(OracleMode)).unwrap();
    assert_eq!(w.projectile_capacity(), 4);
    let mut out = TickEvents::new();
    let mut most = 0;
    for _ in 0..200 {
        w.tick(&[Keys(Keys::FIRE)], &mut out);
        most = most.max(w.projectiles().count());
    }
    assert_eq!(most, 4);
}

#[test]
fn place_bonus_reports_what_it_cannot_place() {
    let mut w = World::new(setup(SimSettings::lx56(), Material::Background), Box::new(OracleMode)).unwrap();
    assert_eq!(
        w.place_bonus(10, 10, crate::BonusItem::Weapon(3)),
        Err(crate::PlaceBonusError::NoSuchWeapon(3))
    );
    for _ in 0..crate::MAX_BONUSES {
        w.place_bonus(10, 10, crate::BonusItem::Health).unwrap();
    }
    assert_eq!(w.place_bonus(10, 10, crate::BonusItem::Health), Err(crate::PlaceBonusError::Full));
}

/// A mode with a scoreboard: each worm's kills, kept by `on_tick`; it ends
/// the match through `is_over` when a score reaches 1, reading `World::scores`.
#[derive(Clone, Debug, Default)]
struct FirstBlood {
    ticks: u32,
    table: Vec<Score>,
}

impl GameMode for FirstBlood {
    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn checksum(&self, h: &mut Fnv) {
        h.u32(self.ticks);
        self.table.iter().for_each(|s| h.i32(s.score));
    }

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        olx_kill_credit(ctx.worms(), ctx.settings(), victim, killer)
    }

    fn on_tick(&mut self, ctx: &mut ModeCtx) {
        self.ticks += 1;
        self.table.clear();
        for w in ctx.worms() {
            let s = Score {
                worm: w.id,
                score: w.kills,
                lives: w.lives,
                kills: w.kills,
                deaths: w.deaths,
                suicides: w.suicides,
                damage: w.damage,
                left: false,
            };
            self.table.push(s);
        }
    }

    fn is_over(&self, world: &World) -> Option<MatchResult> {
        let best = world.scores().iter().find(|s| s.score >= 1)?;
        Some(MatchResult { winner: best.worm, reason: EndReason::KillLimit })
    }

    fn scores(&self, _: &World) -> &[Score] {
        &self.table
    }
}

#[test]
fn is_over_sees_the_world_with_its_mode() {
    let mut m = setup(SimSettings::lx56(), Material::Background);
    m.worms = vec![worm_at(50, 100), worm_at(150, 100)];
    let mode = FirstBlood { ticks: 0, table: Vec::with_capacity(crate::MAX_WORMS) };
    let mut w = World::new(m, Box::new(mode)).unwrap();
    let mut out = TickEvents::new();
    w.tick(&[], &mut out);
    assert_eq!(w.scores().len(), 2);
    assert_eq!(w.result(), None);
    kill_worm(&mut w.harm(), 1, 0);
    w.tick(&[], &mut out); // `on_tick` records the kill
    w.tick(&[], &mut out); // `is_over` reads it through `World::scores`
    let end = MatchResult { winner: 0, reason: EndReason::KillLimit };
    assert_eq!(w.result(), Some(end));
    assert!(out.iter().any(|e| e.kind == EventKind::GameOver(end)));
    // The mode's own state, read back through `as_any`.
    let mode = w.mode_state().as_any().downcast_ref::<FirstBlood>().unwrap();
    assert_eq!(mode.ticks, 3);
    assert!(w.mode_state().as_any().downcast_ref::<OracleMode>().is_none());
}

#[test]
fn no_spawn_after_the_end() {
    // OLX: `GameServer::SpawnWorm` returns while `gameOver` (`CServer_Game.cpp:40`).
    let (mut w, _) = run(scripted(), 5);
    assert_eq!((w.result(), w.worms().len()), (Some(THE_END), 2));
    let mut ctx = w.referee.ctx(&w.worms, &w.settings, w.time);
    ctx.spawn_worm(&worm_at(20, 20)).unwrap();
    w.apply_commands();
    assert_eq!(w.worms().len(), 2);
}

/// `gameOverTime().seconds() > GAMEOVER_WAIT` compares `ms * 0.001f` with 3:
/// 3000 ms is 3.0000002 s, so the physics stops exactly 300 frames after
/// the end (`dm-kill-limit` records it).
#[test]
fn physics_stops_at_exactly_three_seconds_like_olx() {
    let mut r = crate::referee::Referee::oracle();
    r.over = Some(MatchResult { winner: 0, reason: EndReason::KillLimit });
    r.over_frame = 62;
    assert!(!r.frozen(361), "2990 ms");
    assert!(r.frozen(362), "3000 ms is past 3 s in float");
}

/// `on_drop` gives no commands (they would wait between ticks, where a
/// state transfer does not carry them): each is refused, nothing is
/// queued, and commands work again afterwards.
#[test]
fn on_drop_cannot_queue_commands() {
    let mut w = scripted();
    w.withdraw(0);
    let script = w.mode_state().as_any().downcast_ref::<Script>().unwrap();
    assert_eq!(script.errors, [CommandError::Closed; 5]);
    assert!(w.referee.commands.list.is_empty(), "nothing waits between ticks");
    let (w, _) = run(w, 4);
    assert_eq!(w.worms().len(), 2, "the tick-3 script's spawn ran");
}
