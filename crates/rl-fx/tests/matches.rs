//! Oracle scenarios played with entities attached (need the imported OLX
//! content, `RL_CONTENT_DIR`; they skip with a notice without it).

use crate::common;

use rl_content::{olx_level_id, olx_mod_id};
use rl_data::image::Rgba;
use rl_fx::{Fx, FxAssets, FxImages, FxTick, default_dirt_colour};
use rl_scene::{DrawList, SceneAssets, View};

/// Busy scenarios: shots, explosions, kills, gibs, blood, digging and ropes.
const BUSY: [&str; 4] = ["two-worms-fire", "suicide-score", "explode-kill-respawn", "rope-dirt"];

#[test]
fn fx_never_touches_sim_randomness() {
    let Some(content) = common::content() else { return };
    let dir = common::root().join("fixtures/oracle");
    let (mut spawned, mut stained) = (0, 0);
    for name in BUSY {
        let (sc, trace) = rl_scenario::load_fixture(&dir, name).unwrap();
        let mut plain = Vec::new();
        rl_scenario::play(&content, &sc, &trace, |w, _, _| plain.push((w.checksum(), w.rand_calls())))
            .unwrap();

        let level = content.load_level(&olx_level_id(&sc.level)).unwrap();
        let theme = content.load_theme(&level.theme).unwrap();
        let mut map = level.color_map(&theme);
        let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
        let mut scene = SceneAssets::new(content.core(), &m);
        let mut assets =
            FxAssets::new(FxImages::from_core(content.core()), default_dirt_colour(&theme), &mut scene);
        for _ in &sc.worms {
            assets.add_worm(Rgba::rgb(255, 0, 0), &mut scene);
        }
        let (mut fx, mut list, mut with_fx) = (Fx::new(3), DrawList::new(), Vec::new());
        rl_scenario::play(&content, &sc, &trace, |w, frame, events| {
            if frame >= 0 {
                fx.step_tick(&FxTick::new(w, events), &mut map, &assets);
                spawned = spawned.max(fx.len());
                list.clear();
                list.view = View { x: 0.0, y: 0.0, w: 640.0, h: 480.0 };
                fx.draw(&assets, &mut list);
            }
            with_fx.push((w.checksum(), w.rand_calls()));
        })
        .unwrap();
        assert_eq!(plain, with_fx, "{name}");
        let unstained = rl_scenario::color_after(&content, &sc, &trace).unwrap();
        stained += map.pixels().chunks(4).zip(unstained.pixels().chunks(4)).filter(|(a, b)| a != b).count();
    }
    assert!(spawned > 100, "the matches made entities: {spawned}");
    assert!(stained > 0, "blood and gibs stained the map");
}

#[test]
fn steps_and_draws_allocate_nothing() {
    let Some(content) = common::content() else { return };
    let dir = common::root().join("fixtures/oracle");
    let mut ticks = 0;
    for name in BUSY {
        let (sc, trace) = rl_scenario::load_fixture(&dir, name).unwrap();
        let level = content.load_level(&olx_level_id(&sc.level)).unwrap();
        let theme = content.load_theme(&level.theme).unwrap();
        let mut map = level.color_map(&theme);
        let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
        let mut scene = SceneAssets::new(content.core(), &m);
        let mut assets =
            FxAssets::new(FxImages::from_core(content.core()), default_dirt_colour(&theme), &mut scene);
        for _ in &sc.worms {
            assets.add_worm(Rgba::rgb(0, 0, 255), &mut scene);
        }
        let (mut fx, mut list) = (Fx::new(3), DrawList::new());
        rl_scenario::play(&content, &sc, &trace, |w, frame, events| {
            if frame < 0 {
                return;
            }
            let step = allocation_counter::measure(|| {
                fx.step_tick(&FxTick::new(w, events), &mut map, &assets);
                map.take_dirty().for_each(drop);
            });
            assert_eq!(step.count_total, 0, "{name} frame {frame}: step_tick allocated");
            list.clear();
            list.view = View { x: 0.0, y: 0.0, w: 640.0, h: 480.0 };
            let drawn = allocation_counter::measure(|| fx.draw(&assets, &mut list));
            assert_eq!(drawn.count_total, 0, "{name} frame {frame}: draw allocated");
            ticks += 1;
        })
        .unwrap();
    }
    assert!(ticks > 0);
}

#[test]
fn colour_edits_follow_the_sim_like_apply_edits() {
    let Some(content) = common::content() else { return };
    let dir = common::root().join("fixtures/oracle");
    for name in ["two-worms-fire", "rope-dirt", "dig"] {
        let (sc, trace) = rl_scenario::load_fixture(&dir, name).unwrap();
        let expected = rl_scenario::color_after(&content, &sc, &trace).unwrap();
        let level = content.load_level(&olx_level_id(&sc.level)).unwrap();
        let theme = content.load_theme(&level.theme).unwrap();
        let mut map = level.color_map(&theme);
        let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
        let mut scene = SceneAssets::new(content.core(), &m);
        let assets =
            FxAssets::new(FxImages::from_core(content.core()), default_dirt_colour(&theme), &mut scene);
        let mut fx = Fx::new(3);
        rl_scenario::play(&content, &sc, &trace, |w, frame, events| {
            if frame < 0 {
                return;
            }
            // Without blood, droppers and gibs nothing stains: the edits alone must agree.
            let edits: Vec<rl_sim::Event> = events
                .iter()
                .filter(|e| {
                    !matches!(
                        e.kind,
                        rl_sim::EventKind::SpawnCosmetic {
                            kind: rl_sim::CosmeticKind::Blood { .. }
                                | rl_sim::CosmeticKind::BloodDropper
                                | rl_sim::CosmeticKind::Gib { .. },
                            ..
                        }
                    )
                })
                .copied()
                .collect();
            let tick = FxTick { events: &edits, ..FxTick::new(w, events) };
            fx.step_tick(&tick, &mut map, &assets);
        })
        .unwrap();
        assert!(map.pixels() == expected.pixels(), "{name}: the colour maps differ");
    }
}

#[test]
fn fx_assets_share_the_scene_white_pixel() {
    let Some(content) = common::content() else { return };
    let m = content.load_mod(&olx_mod_id("Classic")).unwrap();
    let mut scene = SceneAssets::new(content.core(), &m);
    let before = scene.images().len();
    let assets = FxAssets::new(FxImages::from_core(content.core()), Rgba::BLACK, &mut scene);
    assert_eq!(assets.white, scene.white);
    assert_eq!(scene.images().len(), before + 6, "five sprites and the plain gibs, no second white");
}
