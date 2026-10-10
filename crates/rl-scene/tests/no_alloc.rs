//! A frame allocates nothing (plan 5, Global Constraints): reading a tick
//! and building a draw list reuse buffers sized up front.

mod common;

use rl_content::olx_mod_id;
use rl_data::image::Rgba;
use rl_scene::{Camera, DrawList, Scene, SceneAssets, SceneOptions, Viewport};

#[test]
fn frames_allocate_nothing() {
    let Some(content) = common::content() else { return };
    let (mut ticks, mut frames) = (0, 0);
    for name in ["two-worms-fire", "weapons/classic/rifle", "beam-burst", "rope", "weapons/mw-1-0/bazooka"] {
        let (sc, trace) = rl_scenario::load_fixture(&common::root().join("fixtures/oracle"), name).unwrap();
        let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
        let mut assets = SceneAssets::new(content.core(), &m);
        let skin = content.skin_or_default("olx-skin-default");
        for _ in &sc.worms {
            assets.add_worm(&skin, Rgba::rgb(255, 0, 0), true, false);
        }
        let options = SceneOptions { show_health_bars: true, ..SceneOptions::default() };
        let mut scene =
            Scene::new(Camera::new(Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0), 0, 1), options, 2);
        let mut list = DrawList::new();
        rl_scenario::play(&content, &sc, &trace, |world, frame, events| {
            let tick = allocation_counter::measure(|| scene.after_tick(world, events));
            if frame >= 0 {
                assert_eq!(tick.count_total, 0, "{name} frame {frame}: after_tick allocated");
                ticks += 1;
            }
            let built = allocation_counter::measure(|| scene.build(world, 0.7, &assets, &mut list));
            assert_eq!(built.count_total, 0, "{name} frame {frame}: build allocated");
            frames += 1;
        })
        .unwrap();
    }
    assert!(ticks > 0 && frames > 0);
}
