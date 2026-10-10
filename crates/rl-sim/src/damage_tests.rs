//! Unit tests of damage, death and respawning.
use super::*;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_math::{GlibcRand, Mt19937, Vec2};
use rl_terrain::{Stamp, Terrain};

use crate::input::Keys;
use crate::worm::{Loadout, Slot};
use crate::{SimMod, World};

const T0: AbsTime = AbsTime(100_000);

fn test_mod() -> Arc<SimMod> {
    crate::mod_rt::tests::test_mod()
}

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

/// Two worms on an open 200 × 200 map, 100 px apart, at T0.
fn world(settings: SimSettings) -> World {
    let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, Material::Background));
    let mut w = World::test_new(settings, test_mod(), terrain, holes(), T0, 1);
    w.spawn_worm(50, 100);
    w.spawn_worm(150, 100);
    w
}

fn worms(n: i32, s: &SimSettings) -> Vec<Worm> {
    (0..n).map(|i| Worm::spawned(i, Vec2::new(10.0 * i as f32, 0.0), T0, s)).collect()
}

fn harm<'a>(s: &'a SimSettings, worms: &'a mut [Worm], draws: &'a mut Draws) -> Harm<'a> {
    let referee = alloc::boxed::Box::leak(alloc::boxed::Box::new(crate::referee::Referee::oracle()));
    Harm { settings: s, worms, draws, now: T0, referee }
}

fn draws() -> Draws {
    Draws::new(GlibcRand::new(1), Mt19937::new(1), 0)
}

#[test]
fn exact_zero_health_dies_next_frame_uncredited() {
    // OLX: `CGameObject::injure` kills only below 0; the Gusanos `CWorm::think`
    // (base_worm.cpp:365-381) kills a worm at exactly 0 at the start of the
    // next frame, with killer -1 (spec §18 D9).
    let mut s = SimSettings::lx56();
    s.allow_negative_score = true;
    let mut w = world(s);
    w.tick_new(&[]);
    let now = w.time();
    {
        let h = &mut w.harm();
        h.worms[1].health = 10.0;
        h.worms[1].rope.released = true;
        injure_worm(h, 1, 10.0, 0);
        assert!(h.worms[1].alive, "exactly 0 is alive");
        assert_eq!(h.worms[1].health, 0.0);
    }
    assert_eq!(w.rand_calls(), 70, "10 rounds of hit blood, no death draws");
    w.tick_new(&[]);
    let (killer, victim) = (&w.worms()[0], &w.worms()[1]);
    assert!(!victim.alive);
    assert_eq!(victim.time_of_death, now, "Kill() sets the frame clock");
    assert_eq!((killer.kills, victim.deaths, victim.suicides, victim.lives), (0, 1, 0, -2));
    assert!(!victim.rope.released, "base_die clears the rope");
    assert_eq!(w.rand_calls(), 70, "Gusanos death particles are not ported (D9)");
}

#[test]
fn no_death_count_at_score_zero() {
    // `CWorm::addDeath`/`addSuicide` (CWorm.cpp:1434-1449): without
    // AllowNegativeScore, nothing is counted at a score of 0 or less.
    let mut s = SimSettings::lx56();
    let mut ws = worms(3, &s);
    let mut d = draws();
    ws[2].lives = 5;
    {
        let h = &mut harm(&s, &mut ws, &mut d);
        injure_worm(h, 1, 200.0, 0);
        assert_eq!((h.worms[0].kills, h.worms[1].deaths, h.worms[1].alive), (1, 0, false));
        // A dead owner still gets the kill; worm 0 now has a score of 1.
        injure_worm(h, 0, 200.0, 1);
        assert_eq!((h.worms[1].kills, h.worms[0].deaths, h.worms[0].lives), (1, 1, -2));
        // A suicide at score 0: no suicide, no death, but a life.
        injure_worm(h, 2, 200.0, 2);
        assert_eq!(
            (h.worms[2].kills, h.worms[2].suicides, h.worms[2].deaths, h.worms[2].lives),
            (0, 0, 0, 4)
        );
    }
    s.allow_negative_score = true;
    let mut ws = worms(3, &s);
    let h = &mut harm(&s, &mut ws, &mut d);
    injure_worm(h, 2, 200.0, 2);
    injure_worm(h, 1, 200.0, 0);
    assert_eq!((h.worms[2].suicides, h.worms[2].deaths, h.worms[1].deaths), (1, 1, 1));
}

#[test]
fn respawn_keeps_weapons_and_refills_them() {
    let mut w = world(SimSettings::lx56());
    let slot = |weapon, charge| Slot { weapon, charge, reloading: true, last_fire: 0.5 };
    let loadout = Loadout {
        slots: vec![slot(0, 0.25), slot(0, 0.0)],
        current_weapon: 1,
        lives: -2,
        kills: 0,
        deaths: 0,
    };
    w.set_loadout(0, &loadout);
    for _ in 0..120 {
        w.tick_new(&[]); // fills the position recorder
    }
    let died = w.time();
    injure_worm(&mut w.harm(), 0, 500.0, 1);
    assert!(!w.worms()[0].alive);
    // `tLX->currentTime > fTimeofDeath + RespawnTime` with the clock pinned
    // to frame time + 10 ms: 250 frames later, in the server phase.
    for _ in 0..250 {
        w.tick_new(&[]);
        assert!(!w.worms()[0].alive);
    }
    w.tick_new(&[]);
    let r = &w.worms()[0];
    assert!(r.alive && r.spawned_once && !r.can_respawn_now);
    assert_eq!(r.time_of_death, died);
    let kept: Vec<_> = r.slots.iter().map(|s| (s.weapon, s.charge, s.reloading, s.last_fire)).collect();
    assert_eq!(kept, [(0, 1.0, false, 0.0), (0, 1.0, false, 0.0)]);
    assert_eq!((r.current_weapon, r.health, r.vel, r.kills), (1, 100.0, Vec2::ZERO, 0));
    // FindSpot: two `rnd()` draws per try, `mt() / 2^32` scaled by the map
    // size. The first try, (83.4, 199.4), is within 3 px of the bottom edge,
    // and outside the map counts as rock; the second one is good.
    let mut mt = Mt19937::new(1);
    let mut rnd = || mt.next_u32() as f64 * (1.0 / 4294967296.0) * 200.0;
    let first = Vec2::new(rnd() as f32, rnd() as f32);
    let second = Vec2::new(rnd() as f32, rnd() as f32);
    assert!(first.y > 197.0);
    assert_eq!((r.pos, w.mt_calls()), (second, 4));
    // The recorder keeps its old entries (`clear(vPos)`): get(0) is stale (D7).
    assert_eq!(r.pos_recorder.get(1), r.pos);
    assert_ne!(r.pos_recorder.get(0), Vec2::ZERO);
}

#[test]
fn entity_counter_saturates_and_gates_gib_draws() {
    // A kill: the death sound (1), 7 gibs (2, plus 2 inside SpawnEntity while
    // the never-freed pool has room), 50 rounds of blood (7 each); then the
    // hit's blood, 10 rounds of 7 for a damage of 5.
    let s = SimSettings::lx56();
    for (ents, draws_made, ents_after) in
        [(0, 1 + 28 + 350 + 70, 187), (1021, 1 + 12 + 8 + 350 + 70, 1024), (1024, 1 + 14 + 350 + 70, 1024)]
    {
        let mut ws = worms(2, &s);
        ws[1].health = 1.0;
        let mut d = draws();
        d.ents = ents;
        injure_worm(&mut harm(&s, &mut ws, &mut d), 1, 5.0, 0);
        assert_eq!((d.rand.calls(), d.ents), (draws_made, ents_after), "{ents} entities before");
    }
    let mut d = draws();
    d.ents = 1023;
    assert!(d.spawn_entity() && !d.spawn_entity());
    assert_eq!(d.ents, 1024);
}

#[test]
fn shots_of_a_dead_worm_are_dropped() {
    // Review Focus 3: `CClient::ProcessShot` drops the shots of a worm that
    // is dead at delivery; its projectiles already flying keep their owner
    // id, also once it has respawned. Both worms fire, so owners differ.
    let mut s = SimSettings::lx56();
    (s.respawn_time, s.max_respawn_time) = (0.05, 0.0);
    let mut w = world(s);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    let loadout = Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 };
    w.set_loadout(0, &loadout);
    w.set_loadout(1, &loadout);
    let fire = [Keys(Keys::FIRE), Keys(Keys::FIRE)];
    let owned = |w: &World, owner: i32| -> Vec<(usize, AbsTime)> {
        w.projectiles().filter(|(_, p)| p.owner == owner).map(|(i, p)| (i, p.spawn_time)).collect()
    };
    for _ in 0..50 {
        if !owned(&w, 0).is_empty() && !owned(&w, 1).is_empty() {
            break;
        }
        w.tick_new(&fire);
    }
    assert!(!owned(&w, 0).is_empty() && !owned(&w, 1).is_empty(), "both worms fired");
    w.tick_new(&fire); // more shots in the list, not delivered yet
    let before = owned(&w, 0);
    let worm1_before = owned(&w, 1).len();
    injure_worm(&mut w.harm(), 0, 500.0, 1);
    let mut delivered = 0;
    for _ in 0..3 {
        w.tick_new(&fire);
        assert!(!w.worms()[0].alive, "dead for RespawnTime");
        delivered += w.last_shots().iter().filter(|s| s.worm == 0).count();
        // Every projectile of worm 0 is one it had before it died.
        assert!(owned(&w, 0).iter().all(|p| before.contains(p)), "a dropped shot spawned nothing");
    }
    assert!(delivered > 0, "the pending shots were delivered");
    assert!(
        w.projectiles().filter(|(_, p)| p.owner == 1).count() > worm1_before,
        "worm 1's were not dropped"
    );
    for _ in 0..10 {
        if w.worms()[0].alive {
            break;
        }
        w.tick_new(&fire);
    }
    assert!(w.worms()[0].alive, "respawned");
    // The old projectiles still flying are worm 0's: same slot, spawn time and owner.
    let kept =
        before.iter().filter(|(i, t)| w.projectiles().any(|(j, p)| j == *i && p.spawn_time == *t)).count();
    assert!(kept > 0);
    assert!(
        before
            .iter()
            .all(|(i, t)| w.projectiles().all(|(j, p)| j != *i || p.spawn_time != *t || p.owner == 0))
    );
}

#[test]
fn injury_applies_the_factors_and_self_injure() {
    // `CClient::InjureWorm`: owner's damage factor, then the victim's shield
    // factor (0: every hit kills, every heal fills up); SelfInjure off stops
    // only positive damage to oneself. Healing draws no blood.
    let mut s = SimSettings::lx56();
    let mut ws = worms(2, &s);
    let mut d = draws();
    ws[0].damage_factor = 2.0;
    ws[1].shield_factor = 4.0;
    {
        let h = &mut harm(&s, &mut ws, &mut d);
        injure_worm(h, 1, 10.0, 0); // 10 * 2 / 4
        assert_eq!((h.worms[1].health, h.worms[0].damage), (95.0, 5.0));
        injure_worm(h, 1, -10.0, 0); // heals 5, not clamped at 100
        assert_eq!(h.worms[1].health, 100.0);
        injure_worm(h, 1, 1.0, -1); // no owner: no factor, no score
        assert_eq!((h.worms[1].health, h.worms[0].damage), (99.75, 5.0));
        h.worms[1].shield_factor = 0.0;
        injure_worm(h, 1, -1.0, 1);
        assert_eq!(h.worms[1].health, 100.0, "MIN(health - 100, 0) is 0 here");
    }
    s.self_injure = false;
    let h = &mut harm(&s, &mut ws, &mut d);
    let before = h.draws.rand.calls();
    injure_worm(h, 0, 50.0, 0);
    assert_eq!((h.worms[0].health, h.draws.rand.calls()), (100.0, before), "no self injury, no blood");
    injure_worm(h, 1, 0.5, 0);
    assert!(!h.worms[1].alive, "shield 0: health + 1");
    assert_eq!(h.worms[0].kills, 1);
}
