//! Each entity type moves, animates, stains and dies as OLX's
//! `SimulateEntities` makes it (presentation note §7.2).

mod common;

use common::{DEFAULT_DIRT, DIRT, ROCK, SKY, Setup, art, cosmetic, dt};
use rl_data::image::Rgba;
use rl_fx::{Fx, Kind, MAX_ENTITIES};
use rl_math::Vec2;
use rl_scene::Rect;
use rl_sim::CosmeticKind;

#[test]
fn blood_falls_and_stains_like_olx() {
    let mut s = Setup::new();
    s.tick(&[cosmetic(0, CosmeticKind::Blood { red: 200 }, (50.25, 50.0), (0.0, 0.0))], 1);
    // One pass: gravity 100 px/s², then the move, both with dt 0.011.
    let e = s.only();
    assert_eq!((e.kind, e.colour), (Kind::Blood, Rgba::rgb(200, 0, 0)));
    assert_eq!(e.vel, Vec2::new(0.0, 100.0 * dt()));
    assert_eq!(e.pos, Vec2::new(50.25, 50.0 + e.vel.y * dt()));
    // It reaches the rock (row 60) in its 41st pass.
    s.passes(39);
    let e = s.only();
    assert!(e.pos.y < 60.0, "{e:?}");
    assert_eq!(art(&s.map, 50, 60), [ROCK; 4]);
    // A 2×2 screen-pixel block centred on OLX's half-pixel position.
    let drawn = s.draw();
    assert_eq!(drawn.len(), 1);
    let y = (e.pos.y * 2.0) as i32 as f32 / 2.0;
    assert_eq!((drawn[0].pos, drawn[0].size), ([49.5, y - 0.5], [1.0, 1.0]));
    assert_eq!(drawn[0].tint, Rgba::rgb(200, 0, 0));
    s.passes(1);
    assert!(s.fx.is_empty());
    assert_eq!(art(&s.map, 50, 60), [Rgba::rgb(200, 0, 0); 4], "the stain is one world pixel");
    assert_eq!((art(&s.map, 50, 59), art(&s.map, 51, 60)), ([SKY; 4], [ROCK; 4]));

    // Blood leaving the map is removed without a stain; blood in dirt stains it.
    let mut s = Setup::new();
    let ev = [
        cosmetic(0, CosmeticKind::Blood { red: 180 }, (0.5, 10.0), (-100.0, 0.0)),
        cosmetic(0, CosmeticKind::Blood { red: 128 }, (85.5, 30.0), (0.0, 0.0)),
    ];
    s.tick(&ev, 1);
    assert!(s.fx.is_empty());
    assert_eq!((art(&s.map, 0, 10), art(&s.map, 85, 30)), ([SKY; 4], [Rgba::rgb(128, 0, 0); 4]));
}

#[test]
fn explosion_frames_and_invisible_start() {
    let mut s = Setup::new();
    let ev = [
        cosmetic(0, CosmeticKind::Explosion { size: 8 }, (20.0, 20.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Explosion { size: 20 }, (40.0, 20.0), (0.0, 0.0)),
    ];
    s.tick(&ev, 0);
    // Frame `15 - size`: a small explosion starts big; a large one starts
    // before the image (frame -5), which SDL clips away.
    let frame = |x: u32| Rect { x, y: 0, w: 32, h: 32 };
    let drawn = s.draw();
    assert_eq!(drawn.len(), 1);
    assert_eq!((drawn[0].src, drawn[0].pos, drawn[0].size), (frame(7 * 32), [12.0, 12.0], [16.0, 16.0]));
    // +0.4 a pass: the large one shows frame 0 after 10 passes (-0.9999991
    // truncates to 0), about 0.11 s.
    s.passes(9);
    assert_eq!(s.draw().iter().map(|d| d.src).collect::<Vec<_>>(), [frame(10 * 32)]);
    s.passes(1);
    // 7 + 10 × 0.4 is 10.999997 in floats: still frame 10.
    assert_eq!(s.draw().iter().map(|d| d.src).collect::<Vec<_>>(), [frame(10 * 32), frame(0)]);
    assert_eq!(s.draw()[1].pos, [32.0, 12.0]);
    // Removed once past frame 15: after 21 passes and 51.
    s.passes(10);
    assert_eq!(s.fx.len(), 2);
    s.passes(1);
    assert_eq!(s.kinds(), [(1, Kind::Explosion)]);
    s.passes(29);
    assert_eq!(s.fx.len(), 1);
    s.passes(1);
    assert!(s.fx.is_empty());
}

/// The passes `kind` lives, spawned alone in open air.
fn lifetime(kind: CosmeticKind) -> usize {
    let mut s = Setup::new();
    s.tick(&[cosmetic(0, kind, (20.0, 5.0), (0.0, 0.0))], 0);
    let mut n = 0;
    while !s.fx.is_empty() {
        s.passes(1);
        n += 1;
        assert!(n < 1000, "{kind:?} never dies");
    }
    n
}

#[test]
fn animations_live_as_long_as_olx() {
    // `fFrame += 0.01 * rate` a pass, in floats (presentation note §7.2).
    assert_eq!(lifetime(CosmeticKind::Smoke), 34);
    assert_eq!(lifetime(CosmeticKind::ChemSmoke), 34);
    assert_eq!(lifetime(CosmeticKind::Doomsday), 27);
    assert_eq!(lifetime(CosmeticKind::Spawn), 40);
    assert_eq!(lifetime(CosmeticKind::Sparkle), 31, "an air jump's sparkle fades at 10");
    assert_eq!(lifetime(CosmeticKind::JetpackSpray), 76);
    assert_eq!(lifetime(CosmeticKind::LaserSight { worm: 0, len: 30 }), 2);
    assert_eq!(
        lifetime(CosmeticKind::Beam { worm: 0, colour: Rgba::rgb(255, 0, 0) }),
        0,
        "beams are rl-scene's"
    );

    // Frames past the image draw nothing: chemsmoke's 5th of 4, spawn's 6th of 5.
    let mut s = Setup::new();
    let ev = [
        cosmetic(0, CosmeticKind::ChemSmoke, (20.0, 5.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Spawn, (40.0, 5.0), (0.0, 0.0)),
    ];
    s.tick(&ev, 0);
    s.passes(27);
    assert_eq!(s.fx.len(), 2);
    let spawn_only = s.draw().iter().map(|d| d.src.x).collect::<Vec<_>>();
    assert_eq!(spawn_only, [4 * 32], "both at frame 4: the chemsmoke draws nothing");
    s.passes(7);
    assert_eq!(s.kinds(), [(1, Kind::Spawn)]);
    assert!(s.draw().is_empty(), "the spawn at frame 5 draws nothing");
}

#[test]
fn gibs_use_the_victims_colour() {
    let mut s = Setup::new();
    s.assets.add_worm(Rgba::rgb(255, 0, 0), &mut s.atlas);
    s.assets.add_worm(Rgba::rgb(48, 78, 126), &mut s.atlas);
    s.tick(&[cosmetic(0, CosmeticKind::Gib { worm: 1 }, (40.5, 55.0), (0.0, 10.0))], 0);
    let gib = s.only();
    assert!((0..=3).contains(&gib.rotation));
    let drawn = s.draw();
    let (id, image) = s.assets.gibs(1);
    let image = image.clone();
    assert_eq!(drawn[0].atlas, id);
    assert_ne!(id, s.assets.gibs(0).0);
    let src = Rect { x: gib.rotation as u32 * 8, y: 0, w: 8, h: 8 };
    assert_eq!((drawn[0].src, drawn[0].pos, drawn[0].size), (src, [39.5, 54.0], [4.0, 4.0]));
    // `CWorm::ColorizeImage`: `min(255, colour * c / (96, 156, 252))` in
    // floats, truncated; the gun greys and the colour key are kept.
    let tinted = |c: Rgba| {
        let f = |c: u8, k: u8, d: f32| (k as f32 * (c as f32 / d)).min(255.0) as u8;
        Rgba::rgb(f(c.r, 48, 96.0), f(c.g, 78, 156.0), f(c.b, 126, 252.0))
    };
    let raw = common::giblets();
    assert_eq!(image.get(5, 3), tinted(raw.get(5, 3)));
    assert_eq!(image.get(5, 3), Rgba::rgb(52, 40, 15));
    assert_eq!(image.get(1, 3), Rgba::rgb(180, 180, 180));
    let drawn_image = s.atlas.image(id);
    assert_eq!(drawn_image.get(5, 3), tinted(raw.get(5, 3)));
    assert_eq!(drawn_image.get(0, 3).a, 0, "magenta is transparent in the atlas");

    // A slow gib sticks to the rock and stamps its 4 × 4 frame at `(int)pos - 1`.
    s.passes(100);
    assert!(s.fx.is_empty());
    let r = gib.rotation as u32 * 4;
    for j in 0..4 {
        for i in 0..4 {
            let c = image.get(r + i, 8 + j);
            if !c.same_rgb(Rgba::MAGENTA) {
                assert_eq!(art(&s.map, 39 + i, 59 + j), [tinted(raw.get(r + i, 8 + j)); 4], "{i},{j}");
            }
        }
    }

    // A worm without gibs of its own gets the plain ones, never a panic.
    s.tick(&[cosmetic(0, CosmeticKind::Gib { worm: 9 }, (20.0, 20.0), (0.0, 0.0))], 0);
    assert_eq!(s.draw()[0].atlas, s.assets.gibs(-1).0);
}

#[test]
fn a_fast_gib_bounces_off_rock() {
    let mut s = Setup::new();
    s.tick(&[cosmetic(0, CosmeticKind::Gib { worm: 0 }, (30.5, 59.0), (0.0, 200.0))], 1);
    // `(int)|vel|² > 25600`: the velocity is reversed and scaled by 0.4.
    let e = s.only();
    let vy = 200.0 + 100.0 * dt();
    assert_eq!(e.vel, Vec2::new(-0.0, vy * -0.4));
    assert!(e.pos.y >= 60.0, "it stays inside the rock: {e:?}");
    s.passes(1);
    assert!(s.fx.is_empty(), "slow now, it sticks");
}

#[test]
fn a_blood_dropper_drips_every_ten_passes() {
    let mut s = Setup::new();
    s.tick(&[cosmetic(0, CosmeticKind::BloodDropper, (20.0, 5.0), (0.0, 0.0))], 0);
    let drops = |s: &Setup| s.kinds().iter().filter(|(_, k)| *k == Kind::Blood).count();
    s.passes(10);
    assert_eq!(drops(&s), 0);
    // `fExtra > 0.1` s, counted in whole milliseconds: the 11th pass.
    s.passes(1);
    assert_eq!(drops(&s), 1);
    assert_eq!(s.draw()[0].tint, Rgba::rgb(128, 0, 0), "the dropper itself");
    s.passes(9);
    assert_eq!(drops(&s), 1);
    s.passes(1);
    assert_eq!(drops(&s), 2);
}

#[test]
fn entity_cap_drops_new_ones() {
    let mut s = Setup::new();
    let smoke = cosmetic(0, CosmeticKind::Smoke, (20.0, 20.0), (0.0, 0.0));
    s.tick(&vec![smoke; MAX_ENTITIES + 6], 0);
    assert_eq!(s.fx.len(), MAX_ENTITIES);
    assert!(s.fx.entities().map(|(i, _)| i).eq(0..MAX_ENTITIES));
    s.passes(33);
    s.tick(&[cosmetic(0, CosmeticKind::Explosion { size: 8 }, (20.0, 20.0), (0.0, 0.0))], 0);
    assert!(s.kinds().iter().all(|(_, k)| *k == Kind::Smoke), "dropped while full");
    s.passes(1);
    assert!(s.fx.is_empty());

    // Free slots are reused lowest first.
    let ev =
        [smoke, cosmetic(0, CosmeticKind::LaserSight { worm: 0, len: 9 }, (20.0, 20.0), (1.0, 0.0)), smoke];
    s.tick(&ev, 0);
    assert_eq!(s.kinds(), [(0, Kind::Smoke), (1, Kind::LaserSight), (2, Kind::Smoke)]);
    s.passes(2);
    s.tick(&[cosmetic(0, CosmeticKind::Explosion { size: 8 }, (20.0, 20.0), (0.0, 0.0))], 0);
    assert_eq!(s.kinds(), [(0, Kind::Smoke), (1, Kind::Explosion), (2, Kind::Smoke)]);

    // A restart forgets them.
    s.fx.clear();
    assert!(s.fx.is_empty());
    s.tick(&[smoke], 0);
    assert_eq!(s.kinds(), [(0, Kind::Smoke)]);
}

/// The pool's size is a bench-only knob (plan 8): a larger pool keeps
/// entities OLX's 1024 would drop; the default stays OLX's.
#[test]
fn a_larger_bench_pool_keeps_more_entities() {
    let mut s = Setup::new();
    s.fx = Fx::with_capacity(7, 4 * MAX_ENTITIES);
    let smoke = cosmetic(0, CosmeticKind::Smoke, (20.0, 20.0), (0.0, 0.0));
    s.tick(&vec![smoke; MAX_ENTITIES + 6], 0);
    assert_eq!(s.fx.len(), MAX_ENTITIES + 6);
    assert_eq!(s.fx.capacity(), 4 * MAX_ENTITIES);
    assert_eq!(Fx::new(7).capacity(), MAX_ENTITIES);
    assert_eq!(Fx::with_capacity(7, 0).capacity(), 1, "never an empty pool");
}

#[test]
fn dirt_particles_keep_their_colour() {
    let mut s = Setup::new();
    s.tick(&[cosmetic(0, CosmeticKind::Dirt { probe: (85, 30) }, (78.0, 30.0), (-30.0, -10.0))], 1);
    let e = s.only();
    assert_eq!((e.kind, e.colour), (Kind::Particle, DIRT));
    assert_eq!(s.draw()[0].tint, DIRT);
    assert_ne!(DIRT, DEFAULT_DIRT);
}
