//! Review Focus 1: interpolation never blends a projectile with another
//! that reused its slot, nor a worm across a death or a respawn.

mod common;

use rl_data::image::Rgba;
use rl_math::Vec2;
use rl_scene::{Blend, DrawList, Interpolation, Item, Layer, SceneOptions, Sprite};

fn lerp(a: Vec2, b: Vec2) -> Vec2 {
    a + (b - a) * 0.5
}

/// The skin sprite of worm `i` (its tint is white; the crosshair's image is
/// 6 px high, the skin's 18).
fn skin(list: &DrawList, assets: &rl_scene::SceneAssets, i: usize) -> Option<Sprite> {
    let id = assets.worm(i).skin;
    list.layer(Layer::Worms).iter().find_map(|it| match it {
        Item::Sprite(s) if s.atlas == id && s.tint == Rgba::rgb(255, 255, 255) => Some(*s),
        _ => None,
    })
}

#[test]
fn interpolation_never_blends_reused_slots_or_respawns() {
    let Some(content) = common::content() else { return };
    let linear = SceneOptions { interpolation: Interpolation::Linear, ..SceneOptions::default() };

    // Projectiles: blended only with the same slot's same projectile.
    let (mut reused, mut blended) = (0, 0);
    for name in ["fire-minigun", "weapons/mw-1-0/bazooka"] {
        common::play(&content, name, linear, |world, _, _, scene, _| {
            let prev = scene.prev().projectiles().to_vec();
            let blend = Blend::new(scene.prev(), 0.5, Interpolation::Linear);
            let mut cursor = blend.projectiles();
            for (slot, p) in world.projectiles() {
                let drawn = cursor.pos(slot, p);
                match prev.iter().find(|q| q.slot as usize == slot) {
                    Some(q) if q.serial == p.serial() => {
                        assert_eq!(drawn, lerp(q.pos, p.pos));
                        blended += 1;
                    }
                    Some(_) => {
                        assert_eq!(drawn, p.pos, "slot {slot} reused: drawn where the new projectile is");
                        reused += 1;
                    }
                    None => assert_eq!(drawn, p.pos),
                }
            }
            // Drawing the latest state ignores the snapshot.
            let latest = Blend::new(scene.prev(), 0.5, Interpolation::Latest);
            let mut cursor = latest.projectiles();
            for (slot, p) in world.projectiles() {
                assert_eq!(cursor.pos(slot, p), p.pos);
            }
        });
    }
    assert!(reused > 0, "the scenarios reuse no slot within a tick");
    assert!(blended > 0);

    // Worms: worm 1 dies, then respawns elsewhere; the camera follows it.
    let (mut deaths, mut respawns) = (0, 0);
    let mut list = DrawList::new();
    common::play(&content, "explode-kill-respawn", linear, |world, frame, _, scene, assets| {
        if frame < 0 {
            scene.camera.set_target(1);
        }
        let prev = scene.prev().worms().to_vec();
        let blend = Blend::new(scene.prev(), 0.5, Interpolation::Linear);
        for (i, w) in world.worms().iter().enumerate() {
            let Some(p) = prev.get(i) else { continue };
            let same_life = p.alive == w.alive && p.time_of_death == w.time_of_death;
            if same_life {
                assert_eq!(blend.worm(i, w), lerp(p.pos, w.pos));
            } else {
                assert_eq!(blend.worm(i, w), w.pos, "worm {i} at frame {frame}");
                deaths += usize::from(p.alive && !w.alive);
                respawns += usize::from(!p.alive && w.alive);
            }
        }
        scene.build(world, 0.5, assets, &mut list);
        let w = &world.worms()[1];
        if w.alive && prev.get(1).is_some_and(|p| !p.alive) {
            // The respawned worm is drawn where it is, not on the way.
            let s = skin(&list, assets, 1).expect("the respawned worm is drawn");
            assert_eq!(s.pos, [w.pos.x - 5.0, w.pos.y - 4.5]);
            // And the camera snapped to it in one frame.
            let v = list.view;
            let (mw, mh) = (world.terrain().width() as f32, world.terrain().height() as f32);
            let x = (w.pos.x - v.w.div_euclid(2.0)).clamp(0.0, mw - v.w);
            let y = (w.pos.y - v.h.div_euclid(2.0)).clamp(0.0, mh - v.h);
            assert_eq!((v.x, v.y), (x, y));
        }
        if !w.alive {
            // A dead worm is not drawn; the camera stays where it died
            // (once the explosion's shake is over).
            assert!(skin(&list, assets, 1).is_none());
            if scene.camera.shake_amount() != 0 {
                return;
            }
            let (mw, mh) = (world.terrain().width() as f32, world.terrain().height() as f32);
            let v = list.view;
            assert_eq!(v.x, (w.pos.x - v.w.div_euclid(2.0)).clamp(0.0, mw - v.w));
            assert_eq!(v.y, (w.pos.y - v.h.div_euclid(2.0)).clamp(0.0, mh - v.h));
        }
    });
    assert_eq!((deaths, respawns), (1, 1));
}

/// Review Focus 1, the rope: a rope thrown again while it is out (a rope
/// press, or jump and rope in one tick; OLX `CWormHuman.cpp:565-572`) is
/// released in both snapshots, but the new hook starts at the worm and must
/// not be drawn sliding back from the old anchor.
#[test]
fn rethrown_rope_is_not_blended() {
    use rl_scene::Snapshot;
    use rl_sim::{Keys, TickEvents};
    let Some(content) = common::content() else { return };
    let (sc, trace) = rl_scenario::load_fixture(&common::root().join("fixtures/oracle"), "rope").unwrap();
    let mut world = rl_scenario::load_world(&content, &sc, &trace).unwrap();
    let mut events = TickEvents::new();
    let mut prev = Snapshot::new();
    let keys = |t: u32| match t {
        40..70 => Keys::UP,
        80 | 130 => Keys::ROPE,
        180 => Keys::JUMP | Keys::ROPE,
        _ => 0,
    };
    let (mut rethrows, mut flights) = (0, 0);
    for t in 0..230 {
        prev.capture_into(&world);
        world.tick(&[Keys(keys(t))], &mut events);
        let (p, w) = (prev.worms()[0], &world.worms()[0]);
        let drawn = Blend::new(&prev, 0.5, Interpolation::Linear).hook(0, w);
        if !(p.rope_released && w.rope.released) {
            continue;
        }
        if keys(t) & Keys::ROPE != 0 {
            assert!((p.hook - w.rope.pos).length() > 20.0, "tick {t}: the old hook is far away");
            assert_eq!(drawn, w.rope.pos, "tick {t}: the re-thrown hook is drawn where it is");
            rethrows += 1;
        } else {
            assert_eq!(drawn, lerp(p.hook, w.rope.pos), "tick {t}: one throw blends");
            flights += 1;
        }
    }
    assert_eq!(rethrows, 2);
    assert!(flights > 100);
}
