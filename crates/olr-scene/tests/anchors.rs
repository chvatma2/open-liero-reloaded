//! Linear interpolation draws everything attached to a worm from the worm's
//! drawn position, and blends bonuses: laser sights and beams start at the
//! drawn worm, a hook on another worm sits on that worm as drawn, and a
//! falling bonus moves smoothly between ticks (the Task 3 review).

use crate::common;

use olr_content::{Content, SandboxSetup, SpawnSpec, olx_level_id, olx_mod_id};
use olr_data::image::Rgba;
use olr_math::Vec2;
use olr_scene::{
    Blend, Camera, DrawList, Interpolation, Item, Layer, Scene, SceneAssets, SceneOptions, Viewport,
    draw_bonus, perform_line,
};
use olr_sim::{BonusItem, CosmeticKind, EventKind, Keys, SimSettings, TickEvents, World};

const LINEAR: SceneOptions = SceneOptions { interpolation: Interpolation::Linear, show_health_bars: false };

/// Classic on Liero Factory's open floor: worm 0 at (190, 320) with five
/// of `weapon`, and the worms at `others`.
fn world(content: &Content, weapon: &str, others: &[(i32, i32)], bonuses: bool) -> (World, SceneAssets) {
    let m = content.load_mod(&olx_mod_id("Classic")).unwrap();
    let id = m.sim.weapons().iter().position(|w| w.name == weapon).expect("the weapon") as i32;
    let mut worms = vec![SpawnSpec { pos: Some((190, 320)), weapons: vec![id] }];
    worms.extend(others.iter().map(|&p| SpawnSpec { pos: Some(p), weapons: vec![] }));
    let settings = SimSettings { bonuses, ..SimSettings::lx56() };
    let setup = SandboxSetup {
        mod_id: olx_mod_id("Classic"),
        level_id: olx_level_id("Liero Factory.lxl"),
        seed: 5,
        settings,
        worms,
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    let world = content.new_world(&setup).unwrap();
    let mut assets = SceneAssets::new(content.core(), &m);
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..world.worms().len() {
        assets.add_worm(&skin, Rgba::rgb(255, 0, 0), true, i == 0);
    }
    (world, assets)
}

fn scene() -> Scene {
    Scene::new(Camera::new(Viewport::from_aspect(4.0 / 3.0, 16.0 / 9.0), 0, 1), LINEAR, 2)
}

/// Ticks `world` `n` times with `keys`, building a frame at alpha 0.5 after
/// each: `each(world, scene, list)`.
fn run(
    world: &mut World,
    assets: &SceneAssets,
    n: usize,
    keys: impl Fn(usize) -> Vec<Keys>,
    mut each: impl FnMut(&World, &TickEvents, &Scene, &DrawList),
) {
    let (mut scene, mut list, mut events) = (scene(), DrawList::new(), TickEvents::new());
    scene.after_tick(world, &events);
    for t in 0..n {
        world.tick(&keys(t), &mut events);
        scene.after_tick(world, &events);
        scene.build(world, 0.5, assets, &mut list);
        each(world, &events, &scene, &list);
    }
}

fn trunc(p: Vec2) -> (i32, i32) {
    (p.x as i32, p.y as i32)
}

#[test]
fn beams_start_at_the_drawn_worm() {
    let Some(content) = common::content() else { return };
    let (mut w, assets) = world(&content, "Laser", &[], false);
    let mut moved = 0;
    run(
        &mut w,
        &assets,
        150,
        |_| vec![Keys(Keys::FIRE | Keys::RIGHT)],
        |world, _, scene, list| {
            let worm = &world.worms()[0];
            let drawn = Blend::new(scene.prev(), 0.5, Interpolation::Linear).worm(0, worm);
            for item in list.layer(Layer::Beams) {
                if let Item::Line(l) = item {
                    assert_eq!(l.a, trunc(drawn), "the beam starts at the drawn worm");
                    moved += usize::from(trunc(drawn) != trunc(worm.pos));
                }
            }
        },
    );
    assert!(moved > 0, "the worm never moved while a beam showed");
}

#[test]
fn laser_sights_start_at_the_drawn_worm() {
    let Some(content) = common::content() else { return };
    let (mut w, assets) = world(&content, "Rifle", &[], false);
    let mut moved = 0;
    run(
        &mut w,
        &assets,
        120,
        |_| vec![Keys(Keys::RIGHT)],
        |world, events, scene, list| {
            let n = world.substeps_last_tick();
            let Some((dir, len)) =
                events.iter().filter(|e| n > 0 && e.substep + 1 == n).find_map(|e| match e.kind {
                    EventKind::SpawnCosmetic { kind: CosmeticKind::LaserSight { len, .. }, vel, .. } => {
                        Some((vel, len))
                    }
                    _ => None,
                })
            else {
                return;
            };
            let worm = &world.worms()[0];
            let drawn = Blend::new(scene.prev(), 0.5, Interpolation::Linear).worm(0, worm);
            // Every dot is on OLX's screen-pixel line from the drawn worm.
            let screen = |p: Vec2| ((p.x as i32) * 2, (p.y as i32) * 2);
            let mut on_line = Vec::new();
            perform_line(screen(drawn), screen(drawn + dir * len as f32), |x, y| {
                on_line.push(((x - x % 2) / 2, (y - y % 2) / 2));
            });
            for item in list.layer(Layer::Beams) {
                if let Item::Sprite(s) = item {
                    let dot = (s.pos[0] as i32, s.pos[1] as i32);
                    assert!(on_line.contains(&dot), "dot {dot:?} off the line from {drawn:?}");
                }
            }
            moved += usize::from(trunc(drawn) != trunc(worm.pos));
        },
    );
    assert!(moved > 0, "the worm never moved while its laser sight showed");
}

#[test]
fn a_hook_on_a_worm_sits_on_the_drawn_worm() {
    let Some(content) = common::content() else { return };
    // Worm 1 stands 40 px to the right; worm 0 throws its rope at it, then
    // worm 1 walks away dragging the hook.
    let (mut w, assets) = world(&content, "Laser", &[(230, 320)], false);
    let (mut attached, mut moved) = (0, 0);
    let keys = |t: usize| {
        let rope = if t < 3 { Keys::ROPE } else { 0 };
        vec![Keys(rope), Keys(if t > 20 { Keys::RIGHT } else { 0 })]
    };
    run(&mut w, &assets, 120, keys, |world, _, scene, list| {
        let rope = world.worms()[0].rope;
        if !(rope.attached && rope.player_attached == 1) {
            return;
        }
        attached += 1;
        let other = &world.worms()[1];
        let drawn = Blend::new(scene.prev(), 0.5, Interpolation::Linear).worm(1, other);
        let hook = list
            .layer(Layer::Worms)
            .iter()
            .find_map(|i| match i {
                Item::Sprite(s) if s.atlas == assets.hook => Some(s.pos),
                _ => None,
            })
            .expect("the hook is drawn");
        let (hx, hy) = trunc(drawn);
        assert_eq!(hook, [hx as f32 - 1.0, hy as f32 - 1.0], "the hook sits on the drawn worm");
        moved += usize::from(trunc(drawn) != trunc(other.pos));
    });
    assert!(attached > 0, "the rope never caught worm 1");
    assert!(moved > 0, "worm 1 never moved with the hook on it");
}

#[test]
fn bonuses_blend_between_ticks() {
    let Some(content) = common::content() else { return };
    let (mut w, assets) = world(&content, "Laser", &[], true);
    // A health bonus dropped in the open falls for a while.
    w.place_bonus(150, 250, BonusItem::Health).unwrap();
    let mut blended = 0;
    run(
        &mut w,
        &assets,
        100,
        |_| vec![Keys(0)],
        |world, _, scene, list| {
            let blend = Blend::new(scene.prev(), 0.5, Interpolation::Linear);
            let mut expected = DrawList::new();
            expected.view = list.view;
            for (slot, b) in world.bonuses().iter().enumerate().filter(|(_, b)| b.used) {
                let p = blend.bonus(slot, b);
                let prev = scene.prev().bonuses()[slot];
                if prev.used && prev.spawn_time == b.spawn_time {
                    assert_eq!(p, prev.pos + (b.pos - prev.pos) * 0.5);
                    blended += usize::from(prev.pos != b.pos);
                } else {
                    assert_eq!(p, b.pos, "a new bonus is drawn where it is");
                }
                draw_bonus(b, p, &assets, list.view, &mut expected);
            }
            assert_eq!(
                list.layer(Layer::Bonuses),
                expected.layer(Layer::Bonuses),
                "drawn at the blended position"
            );
        },
    );
    assert!(blended > 0, "the bonus never moved");
}
