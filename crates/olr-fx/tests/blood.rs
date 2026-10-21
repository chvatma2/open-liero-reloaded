//! The player's blood amount (issue #6, `Fx::set_blood`): blood events
//! scaled, droppers never multiplied, drops never thinned, extra blood
//! below its own cap, the same entities from the same events.

use crate::common;

use common::{Setup, cosmetic};
use olr_fx::{
    BLOOD_DEFAULT, BLOOD_MAX, DEATH_RESERVE, EXTRA_BLOOD_HEADROOM, Entity, Fx, Kind, PLAY_ENTITIES,
};
use olr_sim::{CosmeticKind, Event};

/// `n` blood events in the open air, falling onto the rock floor (row 60).
fn blood(n: usize) -> Vec<Event> {
    (0..n)
        .map(|i| {
            let red = if i % 2 == 0 { 200 } else { 180 };
            cosmetic(0, CosmeticKind::Blood { red }, (10.0 + (i % 60) as f32, 40.0), (3.0, -20.0))
        })
        .collect()
}

/// `n` blood droppers high in the air, falling slowly.
fn droppers(n: usize) -> Vec<Event> {
    (0..n).map(|i| cosmetic(0, CosmeticKind::BloodDropper, (5.0 + i as f32, 2.0), (0.0, 0.0))).collect()
}

fn count(s: &Setup, kind: Kind) -> usize {
    s.fx.entities().filter(|(_, e)| e.kind == kind).count()
}

/// A play pool (extra blood has room) at `percent`.
fn play(percent: u16) -> Setup {
    let mut s = Setup::new();
    s.fx = Fx::for_match(7, false);
    s.fx.set_blood(percent);
    s
}

#[test]
fn the_default_is_olxs_and_the_amount_is_bounded() {
    assert_eq!(Fx::new(1).blood(), BLOOD_DEFAULT);
    assert_eq!(Fx::for_match(1, false).blood(), 100);
    let mut fx = Fx::new(1);
    fx.set_blood(9000);
    assert_eq!(fx.blood(), BLOOD_MAX);
}

#[test]
fn at_zero_no_blood_droppers_or_stains() {
    let mut s = play(0);
    let unstained = s.map.pixels().to_vec();
    let death: Vec<Event> =
        blood(10).into_iter().chain(droppers(4)).map(|e| Event { death: true, ..e }).collect();
    s.tick(&blood(100), 1);
    s.tick(&droppers(20), 1);
    s.tick(&death, 1);
    assert!(s.fx.is_empty(), "{:?}", s.kinds());
    s.passes(200);
    assert!(s.map.pixels() == unstained.as_slice(), "nothing stained");

    // Droppers made before the change drip nothing after it.
    let mut s = play(100);
    s.tick(&droppers(5), 0);
    s.fx.set_blood(0);
    s.passes(30);
    assert_eq!((count(&s, Kind::BloodDropper), count(&s, Kind::Blood)), (5, 0));
}

#[test]
fn at_half_every_other_one_shows() {
    let mut s = play(50);
    s.tick(&blood(100), 0);
    assert_eq!(count(&s, Kind::Blood), 50);
    s.tick(&droppers(10), 0);
    assert_eq!(count(&s, Kind::BloodDropper), 5);
    s.tick(&blood(1), 0);
    s.tick(&blood(1), 0);
    assert_eq!(count(&s, Kind::Blood), 51, "spread over the events, not per tick");

    // The droppers shown drip as at 100 %: they are fewer already.
    let mut s = play(50);
    s.tick(&droppers(10), 0);
    s.passes(11);
    assert_eq!(count(&s, Kind::Blood), 5);
}

#[test]
fn at_five_hundred_five_times_bleeding_included() {
    let mut s = play(500);
    s.tick(&blood(100), 0);
    assert_eq!(count(&s, Kind::Blood), 500);
    s.fx.clear();

    // Droppers are never multiplied, their drops are: 5×, not 25×.
    s.tick(&droppers(10), 0);
    assert_eq!(count(&s, Kind::BloodDropper), 10);
    s.passes(11);
    assert_eq!(count(&s, Kind::Blood), 50);

    // And so on in between: 130 % of 100 events and drops.
    let mut s = play(130);
    s.tick(&blood(100), 0);
    assert_eq!(count(&s, Kind::Blood), 130);
    s.fx.clear();
    s.tick(&droppers(10), 0);
    assert_eq!(count(&s, Kind::BloodDropper), 10);
    s.passes(11);
    assert_eq!(count(&s, Kind::Blood), 13);
}

#[test]
fn extra_blood_parts_and_stains_more() {
    let mut s = play(500);
    s.tick(&blood(1), 0);
    let vels: Vec<_> = s.fx.entities().map(|(_, e)| e.vel).collect();
    assert_eq!(vels.len(), 5);
    assert_eq!(vels[0], olr_math::Vec2::new(3.0, -20.0), "the event's own entity is as sent");
    for (i, v) in vels.iter().enumerate().skip(1) {
        assert!(*v != vels[0] && v.x.is_finite(), "copy {i} jittered: {v:?}");
    }

    let stains = |percent| {
        let mut s = play(percent);
        let clean = s.map.pixels().to_vec();
        s.tick(&blood(60), 1);
        s.passes(300);
        assert!(s.fx.is_empty(), "every drop landed");
        s.map.pixels().chunks(4).zip(clean.chunks(4)).filter(|(a, b)| a != b).count()
    };
    let (one, five) = (stains(100), stains(500));
    assert!(five > one, "more blood, more stains: {one} at 100 %, {five} at 500 %");
}

#[test]
fn extra_blood_is_refused_first_and_never_takes_the_death_reserve() {
    let mut s = play(500);
    let extra = s.fx.extra_blood_limit();
    let ordinary = PLAY_ENTITIES - DEATH_RESERVE;
    assert_eq!(extra, ordinary - EXTRA_BLOOD_HEADROOM);

    // Smoke up to the extra blood's cap: a blood event is its own entity alone.
    let smoke = cosmetic(0, CosmeticKind::Smoke, (20.0, 20.0), (0.0, 0.0));
    s.tick(&vec![smoke; extra], 0);
    s.tick(&blood(1), 0);
    assert_eq!(s.fx.len(), extra + 1, "the copies were refused");
    s.tick(&vec![smoke; EXTRA_BLOOD_HEADROOM - 1], 0);
    assert_eq!(s.fx.len(), ordinary, "ordinary effects still had room");

    // A death's blood takes the reserve, its copies never do.
    let death: Vec<Event> = blood(100).into_iter().map(|e| Event { death: true, ..e }).collect();
    s.tick(&death, 0);
    assert_eq!(s.fx.len(), ordinary + 100);
    assert_eq!(s.fx.death_spawns(), 100);

    // A flood of blood never reaches into the reserve, so a death's gibs
    // still show.
    let mut s = play(500);
    s.tick(&blood(4 * PLAY_ENTITIES), 0);
    assert_eq!(s.fx.len(), ordinary, "blood alone stops short of the reserve");
    let gibs: Vec<Event> = (0..7)
        .map(|_| Event {
            death: true,
            ..cosmetic(0, CosmeticKind::Gib { worm: 0 }, (20.0, 20.0), (0.0, 0.0))
        })
        .collect();
    s.tick(&gibs, 0);
    assert_eq!(count(&s, Kind::Gib { worm: 0 }), 7, "the death shows");
    assert_eq!(s.fx.death_drops(), 0);
}

#[test]
fn with_olx_quirks_no_extra_blood() {
    let mut s = Setup::new();
    s.fx.set_blood(500);
    assert_eq!(s.fx.extra_blood_limit(), 0);
    s.tick(&blood(100), 0);
    assert_eq!(count(&s, Kind::Blood), 100, "OLX's 1024 pool has no reserve to keep: no copies");
    s.fx.set_blood(50);
    s.tick(&blood(100), 0);
    assert_eq!(count(&s, Kind::Blood), 150, "fewer still works");
}

#[test]
fn the_same_events_and_amount_give_the_same_entities() {
    let run = |percent| {
        let mut s = play(percent);
        let death: Vec<Event> = blood(30).into_iter().map(|e| Event { death: true, ..e }).collect();
        for _ in 0..20 {
            s.tick(&blood(17), 2);
            s.tick(&droppers(3), 1);
            s.tick(&death, 3);
        }
        (s.fx.entities().map(|(i, e)| (i, *e)).collect::<Vec<(usize, Entity)>>(), s.map.pixels().to_vec())
    };
    for percent in [0, 30, 100, 130, 500] {
        assert!(run(percent) == run(percent), "{percent} %");
    }
    assert!(run(100) != run(500));
}
