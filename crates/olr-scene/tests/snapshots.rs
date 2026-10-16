//! Scene snapshots: the draw list of frame 120 of oracle scenarios, drawn
//! like OLX (the latest state at whole pixels), in `DrawList::describe`'s
//! compact form. Review a change with `cargo insta review` or by reading
//! the `.snap.new` files; accept an intended one with
//! `INSTA_UPDATE=always`.

use crate::common;

use olr_scene::{DrawList, Interpolation, SceneOptions};

/// Frame 120 of scenario `name`.
fn frame_120(name: &str) -> Option<String> {
    let content = common::content()?;
    let latest = SceneOptions { interpolation: Interpolation::Latest, ..SceneOptions::default() };
    let mut text = None;
    let mut list = DrawList::new();
    common::play(&content, name, latest, |world, frame, _, scene, assets| {
        scene.build(world, 1.0, assets, &mut list);
        if frame == 120 {
            text = Some(list.describe(assets));
        }
    });
    text
}

fn snapshot(name: &str) {
    let Some(text) = frame_120(name) else { return };
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(name.replace('/', "__"));
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| insta::assert_snapshot!("frame_120", text));
}

#[test]
fn scene_rope() {
    snapshot("rope");
}

#[test]
fn scene_fire_minigun() {
    snapshot("fire-minigun");
}

/// Classic: the Rifle, with its laser sight.
#[test]
fn scene_classic_weapon() {
    snapshot("weapons/classic/rifle");
}

/// MW 1.0: an animated rocket with its trail.
#[test]
fn scene_mw_weapon() {
    snapshot("weapons/mw-1-0/bazooka");
}
