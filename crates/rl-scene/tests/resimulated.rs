//! A re-simulated tick (online play's rollback, spec §3.4) moves the
//! scene's history on like a new one, without shaking the camera again.

use crate::common;

use rl_content::olx_mod_id;
use rl_scene::{Camera, DrawList, Scene, SceneAssets, SceneOptions, Viewport};

#[test]
fn resimulated_ticks_keep_the_history_without_shaking() {
    let Some(content) = common::content() else { return };
    let (sc, trace) =
        rl_scenario::load_fixture(&common::root().join("fixtures/oracle"), "weapons/mw-1-0/bazooka").unwrap();
    let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    let assets = SceneAssets::new(content.core(), &m);
    let scene = || {
        Scene::new(
            Camera::new(Viewport::from_aspect(4.0 / 3.0, 16.0 / 9.0), 0, 1),
            SceneOptions::default(),
            2,
        )
    };
    let (mut new, mut resim) = (scene(), scene());
    let mut list = DrawList::new();
    let mut shook = false;
    rl_scenario::play(&content, &sc, &trace, |world, _, events| {
        new.after_tick(world, events);
        resim.after_tick_resimulated(world, events);
        assert_eq!(new.prev(), resim.prev(), "the same history");
        new.build(world, 1.0, &assets, &mut list);
        resim.build(world, 1.0, &assets, &mut list);
        shook |= new.camera.shake_amount() > 0;
        assert_eq!(resim.camera.shake_amount(), 0, "a re-simulated explosion shakes nothing");
    })
    .unwrap();
    assert!(shook, "the scenario's explosions shake a new tick's camera");
}
