//! The presentation bundle against the wiring it replaces, plus its atlas
//! and drift bookkeeping. Plays oracle scenarios from the imported content
//! (`RL_CONTENT_DIR`); skips with a notice without it.

use std::path::{Path, PathBuf};

use rl_content::{Content, LoadedLevel, LoadedMod, LoadedTheme, olx_level_id, olx_mod_id};
use rl_fx::{Fx, FxAssets, FxImages, FxTick, default_dirt_colour};
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_scene::{Camera, DrawList, Interpolation, Scene, SceneAssets, SceneOptions, Viewport};
use rl_terrain::ColorEdit;

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("fixtures/oracle")
}

struct Packs {
    m: LoadedMod,
    level: LoadedLevel,
    theme: LoadedTheme,
}

fn packs(content: &Content, sc: &rl_oracle::Scenario) -> Packs {
    let level = content.load_level(&olx_level_id(&sc.level)).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    Packs { m: content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap(), level, theme }
}

const VIEWPORT: f32 = 4.0 / 3.0;

fn setup<'a>(content: &'a Content, p: &'a Packs) -> PresentSetup<'a> {
    PresentSetup {
        core: content.core(),
        m: &p.m,
        level: &p.level,
        theme: &p.theme,
        viewport: Viewport::from_aspect(VIEWPORT, VIEWPORT),
        options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: true },
        seed: 1,
    }
}

/// The bundle draws, frame for frame, what the scene and the entities
/// wired by hand draw (the order `rl-game` and `rl-cli render` used).
#[test]
fn presentation_draws_what_the_hand_wiring_draws() {
    let Some(content) = content() else { return };
    let skin = content.skin_or_default("olx-skin-default");
    for name in ["two-worms-fire", "weapons/mw-1-0/bazooka", "dig"] {
        let (sc, trace) = rl_scenario::load_fixture(&fixtures(), name).unwrap();
        let p = packs(&content, &sc);
        // By hand.
        let mut assets = SceneAssets::new(content.core(), &p.m);
        let mut fx_assets =
            FxAssets::new(FxImages::from_core(content.core()), default_dirt_colour(&p.theme), &mut assets);
        for i in 0..sc.worms.len() {
            assets.add_worm(&skin, default_colour(i), i == 0, i == 0);
            fx_assets.add_worm(default_colour(i), &mut assets);
        }
        let s = setup(&content, &p);
        let mut scene = Scene::new(Camera::new(s.viewport, 0, 1), s.options, 2);
        let mut fx = Fx::new(3);
        let mut map = p.level.color_map(&p.theme);
        // Bundled.
        let mut present = None;
        let (mut a, mut b) = (DrawList::new(), DrawList::new());
        let mut frames = 0;
        rl_scenario::play(&content, &sc, &trace, |world, f, events| {
            let present = present.get_or_insert_with(|| {
                let mut pr = Presentation::new(&s, world);
                for i in 0..sc.worms.len() {
                    pr.add_worm(&skin, default_colour(i), i == 0, i == 0);
                }
                pr
            });
            scene.after_tick(world, events);
            if f >= 0 {
                fx.step_tick(&FxTick::new(world, events), &mut map, &fx_assets);
                present.on_tick(world, events);
            }
            scene.build(world, 0.5, &assets, &mut a);
            fx.draw(&fx_assets, &mut a);
            present.build(world, 0.5, &mut b);
            assert_eq!(a.describe(&assets), b.describe(present.assets()), "{name} frame {f}");
            assert!(map.pixels() == present.color().pixels(), "{name} frame {f}: colour terrain");
            // Before the first tick, the spawn holes are setup events still to replay.
            assert!(f < 0 || !present.terrain_drift(world), "{name} frame {f}");
            frames += 1;
        })
        .unwrap();
        assert_eq!(frames, sc.frames as usize + 1);
        assert_eq!(assets.images().len(), present.as_ref().unwrap().assets().images().len());
    }
}

/// Every image is registered before the first upload; a worm added later
/// (a player joining mid-match) marks the atlas for the next frame's upload.
#[test]
fn a_worm_added_mid_match_marks_the_atlas_for_upload() {
    let Some(content) = content() else { return };
    let (sc, trace) = rl_scenario::load_fixture(&fixtures(), "walk").unwrap();
    let p = packs(&content, &sc);
    let world = rl_scenario::load_world(&content, &sc, &trace).unwrap();
    let skin = content.skin_or_default("olx-skin-default");
    let mut pr = Presentation::new(&setup(&content, &p), &world);
    let core_images = pr.assets().images().len();
    assert_eq!(pr.add_worm(&skin, default_colour(0), true, true), 0);
    assert!(pr.take_atlas_dirty(), "a new presentation uploads its atlas");
    assert!(!pr.take_atlas_dirty(), "once");
    assert_eq!(pr.add_worm(&skin, default_colour(1), false, false), 1);
    assert!(pr.assets().images().len() > core_images);
    assert!(pr.take_atlas_dirty(), "the second worm's skin and gibs are uploaded");
    assert_ne!(pr.fx_assets().gibs(1).0, pr.fx_assets().gibs(0).0, "worm 1 has its own gibs");
    pr.invalidate_gpu();
    assert!(pr.take_atlas_dirty(), "a new renderer gets the atlas again");
}

/// An edit of the colour terrain the world did not make is drift, and
/// [`Presentation::terrain_drift`] sees it.
#[test]
fn terrain_drift_is_detected() {
    let Some(content) = content() else { return };
    let (sc, trace) = rl_scenario::load_fixture(&fixtures(), "dig").unwrap();
    let p = packs(&content, &sc);
    let mut world = rl_scenario::load_world(&content, &sc, &trace).unwrap();
    let start = world.clone();
    let mut pr = Presentation::new(&setup(&content, &p), &world);
    let mut events = rl_sim::TickEvents::new();
    world.tick(&[rl_sim::Keys(0)], &mut events);
    pr.on_tick(&world, &events);
    assert!(!pr.terrain_drift(&world), "the spawn hole was replayed");
    // Dirt where the worm stands (free space): an edit the world did not make.
    let at = world.worms()[0].pos;
    pr.color_mut().apply(ColorEdit::Dirt { hole: 0, x: at.x as i32, y: at.y as i32 });
    assert!(pr.terrain_drift(&world), "dirt the world did not place");
    // A restart takes the level's map again, before its spawn holes.
    pr.restart(&start, &p.level, &p.theme);
    let mut world = start;
    world.tick(&[rl_sim::Keys(0)], &mut events);
    pr.on_tick(&world, &events);
    assert!(!pr.terrain_drift(&world), "a restart starts from the level again");
}
