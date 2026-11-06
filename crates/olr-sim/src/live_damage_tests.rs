//! Unit tests of explosions damaging destructible live copies (issue #94).
use super::*;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_data::mod_def::{ActionDef, ActionKind, BeamDef, WeaponDef, WeaponKind};
use olr_math::TimeDiff;
use olr_terrain::{PX_ROCK, Stamp, Terrain};

use crate::effects::{Blast, explosion};
use crate::events::{Event, TickEvents};
use crate::input::Keys;
use crate::live::{LiveCopySetup, LiveHealth, LivePart, LivePartKind, LiveProp, LiveState};
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::mode::{GameMode, KillCredit, ModeCtx};
use crate::worm::Slot;
use crate::{Loadout, MatchSetup, OracleMode, SimMod, SimSettings, World};

/// The crate's place and size: 20 × 20 at (140, 160), on the floor.
const AT: (i32, i32) = (140, 160);

/// A crate of 40 points (its `Health` 0 to 100, so the points bound it),
/// Wrecked at 0. Whole: a 20 × 20 rock box. Wrecked: a 20 × 4 rubble at
/// its foot, rock but in columns 8 to 11 (a gap), under a background
/// picture covering only its left column (`cover`).
fn crate_copy(id: &str, at: (i32, i32)) -> LiveCopy {
    let part =
        |kind, at, picture| LivePart { kind, layer: None, at, picture, glow_strength: 100, clip: false };
    let state = |name: &str, parts| LiveState {
        name: name.into(),
        values: Vec::new(),
        parts,
        enter_sound: None,
        loop_sound: None,
    };
    LiveCopy::new(LiveCopySetup {
        id: id.into(),
        at,
        size: (20, 20),
        props: vec![LiveProp {
            name: Some("health".into()),
            target: LiveTarget::Health,
            min: 0,
            max: 100,
            default: 100,
        }],
        states: vec![
            state("Whole", vec![part(LivePartKind::Rock, (0, 0), 0)]),
            state(
                "Wrecked",
                vec![part(LivePartKind::Background, (0, 0), 2), part(LivePartKind::Rock, (0, 16), 1)],
            ),
        ],
        pictures: vec![
            LiveSolid::new((20, 20), true, |_, _| true),
            LiveSolid::new((20, 4), true, |x, _| !(8..12).contains(&x)),
            LiveSolid::new((20, 16), false, |_, _| false).with_cover(|x, _| x == 0),
        ],
        start_state: 0,
        start_values: Vec::new(),
        health: Some(LiveHealth { points: 40, at_zero: 1 }),
        scripts: Vec::new(),
        sounds: Vec::new(),
    })
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

/// A 200 × 200 cave, rock from row 180 down.
fn terrain() -> Terrain {
    let cells =
        (0..200 * 200).map(|i| if i / 200 >= 180 { Material::Rock } else { Material::Background }).collect();
    Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap())
}

fn setup(m: Arc<SimMod>, copies: Vec<LiveCopy>) -> MatchSetup {
    MatchSetup {
        holes: holes(),
        live: (!copies.is_empty()).then(|| Arc::new(LiveLevel::new(copies, Vec::new(), None, 0))),
        ..MatchSetup::new(SimSettings::lx56(), m, terrain())
    }
}

fn world_with(m: Arc<SimMod>, copies: Vec<LiveCopy>, mode: Box<dyn GameMode>) -> World {
    World::build(setup(m, copies), mode)
}

/// The crate alone.
fn world() -> World {
    world_with(crate::mod_rt::tests::test_mod(), vec![crate_copy("crate", AT)], Box::new(OracleMode))
}

/// [`World::explode_for_test`] with an owner: its events.
fn explode(w: &mut World, pos: Vec2, damage: f32, owner: i32) -> Vec<EventKind> {
    let mut out = TickEvents::new();
    core::mem::swap(&mut w.draws.events, &mut out);
    if owner < 0 {
        w.explode_for_test(pos, damage);
    } else {
        let now = w.time;
        let (_, hits) = w.live.as_mut().map_or((None, None), crate::live::LiveWorld::physics_parts);
        let mut blast = Blast {
            settings: &w.settings,
            terrain: &mut w.terrain,
            holes: &w.holes,
            worms: &mut w.worms,
            draws: &mut w.draws,
            bonuses: &mut w.bonuses.client,
            now,
            wall_clock: now + TimeDiff(crate::world::CLOCK_OFFSET_MS),
            physics_dt: w.settings.lx56_physics_dt(),
            referee: &mut w.referee,
            live: hits,
        };
        explosion(&mut blast, now, pos, damage, 5, owner, -1);
        if let Some(l) = &mut w.live {
            l.refresh_if_stale();
        }
    }
    core::mem::swap(&mut w.draws.events, &mut out);
    out.iter().map(|e| e.kind).collect()
}

fn health(w: &World) -> i64 {
    w.live().unwrap().copy(0).unwrap().value("health").unwrap()
}

fn state(w: &World) -> &str {
    w.live().unwrap().copy(0).unwrap().state_name()
}

fn hits(events: &[EventKind]) -> Vec<(i64, bool)> {
    events
        .iter()
        .filter_map(|e| match *e {
            EventKind::LiveDamaged { copy: 0, amount, wrecked } => Some((amount.into(), wrecked)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_explosion_near_a_destructible_copy_takes_its_damage() {
    let mut w = world();
    assert_eq!(health(&w), 40, "its points bound it");
    // Inside the box, and 5 px off its left edge (the pixel at x = 140
    // spans 140..141: 135 is 5 px from it).
    let e = explode(&mut w, Vec2::new(150.0, 170.0), 12.0, -1);
    assert_eq!(hits(&e), [(12, false)]);
    assert_eq!(health(&w), 28);
    let e = explode(&mut w, Vec2::new(135.0, 170.0), 3.0, -1);
    assert_eq!(hits(&e), [(3, false)]);
    // Diagonally off the top-left corner: (137, 157) is √18 < 5 px away.
    explode(&mut w, Vec2::new(137.0, 157.0), 5.0, -1);
    assert_eq!(health(&w), 20);
    // A healing explosion heals up to its points.
    let e = explode(&mut w, Vec2::new(150.0, 170.0), -100.0, -1);
    assert_eq!(hits(&e), [(-100, false)]);
    assert_eq!(health(&w), 40);
    // The explosion itself comes first: the worms' part as ever.
    let e = explode(&mut w, Vec2::new(150.0, 170.0), 1.0, -1);
    let at = |f: fn(&EventKind) -> bool| e.iter().position(f).unwrap();
    assert!(
        at(|k| matches!(k, EventKind::Explosion { .. })) < at(|k| matches!(k, EventKind::LiveDamaged { .. }))
    );
}

#[test]
fn an_explosion_beyond_five_pixels_of_its_pixels_does_not() {
    let mut w = world();
    for pos in [(134.9, 170.0), (150.0, 154.9), (165.1, 170.0), (136.0, 156.0)] {
        let e = explode(&mut w, Vec2::new(pos.0, pos.1), 10.0, -1);
        assert!(hits(&e).is_empty(), "{pos:?}");
    }
    assert_eq!(health(&w), 40);
    // Wrecked, it covers its rubble (rows 176..180) and its background's
    // left column only, not that picture's rectangle.
    w.set_live_value(0, 0, 1).unwrap();
    explode(&mut w, Vec2::new(150.0, 170.0), 5.0, -1);
    assert_eq!(state(&w), "Wrecked");
    w.set_live_value(0, 0, 40).unwrap();
    let e = explode(&mut w, Vec2::new(150.0, 165.0), 10.0, -1);
    assert!(hits(&e).is_empty(), "6 px above the rubble, 9 px from the left column");
    let e = explode(&mut w, Vec2::new(145.9, 165.0), 10.0, -1);
    assert_eq!(hits(&e), [(10, false)], "within 5 px of the covered column");
    let e = explode(&mut w, Vec2::new(150.0, 171.0), 10.0, -1);
    assert!(hits(&e).is_empty(), "5 px above the rubble's gap, farther from its rock");
    let e = explode(&mut w, Vec2::new(156.0, 171.0), 10.0, -1);
    assert_eq!(hits(&e), [(10, false)], "5 px above the rubble");
    assert_eq!(health(&w), 20);
    // A NaN or infinite place reaches nothing.
    for pos in [(f32::NAN, 170.0), (150.0, f32::INFINITY), (f32::NEG_INFINITY, f32::NAN)] {
        assert!(hits(&explode(&mut w, Vec2::new(pos.0, pos.1), 10.0, -1)).is_empty());
    }
}

#[test]
fn a_copy_at_zero_enters_its_at_zero_state_once() {
    let mut w = world();
    let e = explode(&mut w, Vec2::new(150.0, 170.0), 30.0, -1);
    assert_eq!(hits(&e), [(30, false)]);
    let e = explode(&mut w, Vec2::new(150.0, 170.0), 30.0, -1);
    assert_eq!(hits(&e), [(30, true)]);
    let entered: Vec<_> = e.iter().filter(|k| matches!(k, EventKind::LiveState { .. })).collect();
    assert_eq!(entered, [&EventKind::LiveState { copy: 0, state: 1 }]);
    let at = |f: fn(&EventKind) -> bool| e.iter().position(f).unwrap();
    assert!(
        at(|k| matches!(k, EventKind::LiveDamaged { .. })) < at(|k| matches!(k, EventKind::LiveState { .. }))
    );
    assert_eq!((health(&w), state(&w)), (0, "Wrecked"), "clamped at 0");
    let e = explode(&mut w, Vec2::new(150.0, 178.0), 30.0, -1);
    assert!(hits(&e).is_empty() && !e.iter().any(|k| matches!(k, EventKind::LiveState { .. })));
}

#[test]
fn a_copy_at_zero_takes_no_more_damage_until_repaired() {
    let mut w = world();
    explode(&mut w, Vec2::new(150.0, 170.0), 1e9, -1);
    assert_eq!((health(&w), state(&w)), (0, "Wrecked"));
    for damage in [10.0, -10.0, 1e9] {
        assert!(hits(&explode(&mut w, Vec2::new(150.0, 178.0), damage, -1)).is_empty(), "{damage}");
    }
    assert_eq!(health(&w), 0, "a healing explosion is no repair");
    // A script or a state repairs it; then it takes damage and wrecks again.
    w.set_live_value(0, 0, 25).unwrap();
    w.set_live_state(0, 0).unwrap();
    let e = explode(&mut w, Vec2::new(150.0, 170.0), 25.0, -1);
    assert_eq!(hits(&e), [(25, true)]);
    assert_eq!((health(&w), state(&w)), (0, "Wrecked"));
}

#[test]
fn the_wrecked_picture_decides_what_is_solid() {
    let mut w = world();
    let rock = |w: &World, x, y| w.solid().flags(x, y) & PX_ROCK != 0;
    assert!(rock(&w, 150, 165) && rock(&w, 149, 178), "the box");
    explode(&mut w, Vec2::new(150.0, 170.0), 40.0, -1);
    assert_eq!(state(&w), "Wrecked");
    assert!(!rock(&w, 150, 165), "the box is gone: worms pass");
    assert!(!rock(&w, 140, 165), "its background is no rock");
    assert!(rock(&w, 145, 178) && rock(&w, 155, 177), "the rubble");
    assert!(!rock(&w, 149, 178) && !rock(&w, 148, 176), "its gap");
    // In play, a worm falls through the box's place into the gap.
    w.spawn_worm(150, 150);
    for _ in 0..150 {
        w.tick_new(&[Keys(0)]);
    }
    assert!(w.worms()[0].pos.y > 174.0, "{:?}", w.worms()[0].pos);
}

/// A mode counting its damage callbacks.
#[derive(Clone, Debug, Default)]
struct Counting {
    damage: u32,
}

impl GameMode for Counting {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(self.clone())
    }

    fn checksum(&self, _: &mut crate::Fnv) {}

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        crate::damage::olx_kill_credit(ctx.worms(), ctx.settings(), victim, killer)
    }

    fn on_damage(&mut self, _: i32, _: i32, _: f32, _: &mut ModeCtx) {
        self.damage += 1;
    }
}

#[test]
fn asset_damage_credits_nobody() {
    let m = crate::mod_rt::tests::test_mod();
    let mut w = world_with(m, vec![crate_copy("crate", AT)], Box::new(Counting::default()));
    w.spawn_worm(30, 170);
    let before = w.worms()[0].clone();
    // Worm 0's explosion at the crate, far from it.
    let e = explode(&mut w, Vec2::new(150.0, 170.0), 40.0, 0);
    assert_eq!(hits(&e), [(40, true)]);
    assert_eq!(w.worms()[0], before, "no damage score, no kill");
    assert!(!e.iter().any(|k| matches!(k, EventKind::Damage { .. } | EventKind::Kill { .. })));
    assert_eq!(w.mode_state().as_any().downcast_ref::<Counting>().unwrap().damage, 0, "no on_damage");
}

#[test]
fn worms_take_explosion_damage_as_before_beside_a_copy() {
    let m = crate::mod_rt::tests::test_mod();
    // The crate beside the worms, and a world without it (no live copies).
    let make = |copies| {
        let mut w = world_with(m.clone(), copies, Box::new(OracleMode));
        w.spawn_worm(135, 170);
        w.spawn_worm(132, 172);
        w
    };
    let (mut a, mut b) = (make(vec![crate_copy("crate", AT)]), make(Vec::new()));
    for (pos, damage) in
        [((136.0, 170.0), 12.0), ((133.0, 171.0), 9.0), ((137.0, 171.0), 30.0), ((150.0, 170.0), 7.5)]
    {
        let pos = Vec2::new(pos.0, pos.1);
        let ea = explode(&mut a, pos, damage, 1);
        let eb = explode(&mut b, pos, damage, 1);
        let worms = |e: &[EventKind]| -> Vec<EventKind> {
            e.iter()
                .filter(|k| !matches!(k, EventKind::LiveDamaged { .. } | EventKind::LiveState { .. }))
                .copied()
                .collect()
        };
        assert_eq!(worms(&ea), worms(&eb), "the same events but the copy's");
        assert_eq!(a.rand_calls(), b.rand_calls(), "the same draws");
        let hp = |w: &World| -> Vec<f32> { w.worms().iter().map(|w| w.health).collect() };
        assert_eq!(hp(&a), hp(&b));
    }
    assert_eq!(health(&a), 0, "the copy took 12 and 30 (7 px off, 9 not), then nothing at 0");
}

/// A world whose worm 0, at (100, 175) on the floor facing the crate, holds
/// `weapon` (a mod of it and a gravity-free pellet doing `hit` on the map).
fn shooter(weapon: WeaponDef, hit: ActionDef) -> World {
    let mut pellet = pixel_projectile(None);
    pellet.gravity = Some(0);
    pellet.hit = hit;
    let def = mod_def(vec![weapon], vec![("pellet", pellet)]);
    let m = Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap());
    let mut w = world_with(m, vec![crate_copy("crate", AT)], Box::new(OracleMode));
    w.spawn_worm(100, 150);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(0, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    for _ in 0..100 {
        w.tick_new(&[Keys(0)]);
    }
    w
}

/// Fires for `ticks` frames; the copy's hits.
fn fire(w: &mut World, ticks: usize) -> Vec<(i64, bool)> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        let e = w.tick_new(&[Keys(Keys::FIRE)]);
        out.extend(hits(&e.iter().map(|e: &Event| e.kind).collect::<Vec<_>>()));
    }
    out
}

#[test]
fn beams_and_direct_hits_do_not_damage_copies() {
    let laser = WeaponDef {
        kind: WeaponKind::Beam {
            recoil: 0,
            beam: BeamDef {
                colour: Rgba::rgb(255, 0, 0),
                damage: 3,
                player_damage: 50,
                length: 999_999,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            },
        },
        ..projectile_weapon("laser", "pellet")
    };
    let mut w = shooter(laser, ActionDef::olx_default(ActionKind::Explode));
    let mut beams = 0;
    for _ in 0..100 {
        let e = w.tick_new(&[Keys(Keys::FIRE)]);
        beams += e
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::SpawnCosmetic { kind: crate::CosmeticKind::Beam { stopped: true, .. }, .. }
                )
            })
            .count();
        assert!(hits(&e.iter().map(|e| e.kind).collect::<Vec<_>>()).is_empty());
    }
    assert!(beams > 0, "the beam stops on the crate");
    assert_eq!(health(&w), 40);
    // A pellet that hits the map without exploding.
    let mut w = shooter(
        projectile_weapon("gun", "pellet"),
        ActionDef { damage: 50, ..ActionDef::olx_default(ActionKind::Disappear) },
    );
    assert!(fire(&mut w, 200).is_empty());
    assert_eq!(health(&w), 40);
    // The control: one that explodes takes it.
    let mut w = shooter(
        projectile_weapon("gun", "pellet"),
        ActionDef { damage: 5, ..ActionDef::olx_default(ActionKind::Explode) },
    );
    let got = fire(&mut w, 200);
    assert!(!got.is_empty() && got.iter().all(|&(a, _)| a == 5), "{got:?}");
    assert_eq!(state(&w), "Wrecked");
}

#[test]
fn explosion_damage_truncates_toward_zero_and_saturates() {
    let cases = [
        (4.9, 4),
        (-0.5, 0),
        (-4.9, -4),
        (f32::NAN, 0),
        (f32::INFINITY, i64::MAX),
        (f32::NEG_INFINITY, i64::MIN),
        (f32::MAX, i64::MAX),
        (f32::MIN, i64::MIN),
    ];
    for (damage, amount) in cases {
        assert_eq!(damage_amount(damage), amount, "{damage}");
    }
    // None panics in a world: each its hit or none.
    for (damage, want) in cases {
        let mut w = world();
        w.set_live_value(0, 0, 20).unwrap();
        let e = explode(&mut w, Vec2::new(150.0, 170.0), damage, -1);
        let left = health(&w);
        match want {
            0 => assert!(hits(&e).is_empty() && left == 20, "{damage}"),
            a if a > 0 => {
                assert_eq!((hits(&e)[0].0, left), (a.min(i32::MAX.into()), (20 - a).max(0)), "{damage}")
            }
            a => assert_eq!(
                (hits(&e)[0].0, left),
                (a.max(i32::MIN.into()), 20i64.saturating_sub(a).min(40)),
                "{damage}: healed up to its points, the event's amount saturated"
            ),
        }
    }
}

/// `LiveDamaged`'s amount is an `i32`, so an event keeps its size (an
/// `i64` would make every event 40 bytes).
#[test]
fn an_event_keeps_its_size() {
    assert_eq!(core::mem::size_of::<Event>(), 36);
}
