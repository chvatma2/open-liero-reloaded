//! Oracle scenarios played through a [`Scene`] (needs the imported OLX
//! content, `RL_CONTENT_DIR`; tests skip with a notice without it).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use rl_content::{Content, olx_mod_id};
use rl_data::image::Rgba;
use rl_scene::{Camera, Scene, SceneAssets, SceneOptions, Viewport};
use rl_sim::{TickEvents, World};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the workspace root").to_path_buf()
}

pub fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

/// OLX's default worm colours for the first worms (`Profiles.cpp`): red,
/// blue, green, yellow.
const COLOURS: [Rgba; 4] =
    [Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255), Rgba::rgb(0, 255, 0), Rgba::rgb(255, 255, 0)];

/// Plays oracle scenario `name` through a scene following worm 0 in a 4:3
/// viewport, every worm local (worm 0 human) with OLX's default skin:
/// `each(world, frame, events, scene, assets)` for the start (frame -1)
/// and after each frame's tick, once [`Scene::after_tick`] has read it.
pub fn play(
    content: &Content,
    name: &str,
    options: SceneOptions,
    mut each: impl FnMut(&World, i64, &TickEvents, &mut Scene, &SceneAssets),
) {
    let (sc, trace) = rl_scenario::load_fixture(&root().join("fixtures/oracle"), name).unwrap();
    let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    let mut assets = SceneAssets::new(content.core(), &m);
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..sc.worms.len() {
        assets.add_worm(&skin, COLOURS[i % COLOURS.len()], true, i == 0);
    }
    let camera = Camera::new(Viewport::from_aspect(4.0 / 3.0, 16.0 / 9.0), 0, 1);
    let mut scene = Scene::new(camera, options, 2);
    rl_scenario::play(content, &sc, &trace, |world, frame, events| {
        scene.after_tick(world, events);
        each(world, frame, events, &mut scene, &assets);
    })
    .unwrap();
}
