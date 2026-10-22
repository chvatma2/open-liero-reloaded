//! Issue #27 (split screen): a presentation of two views draws, view by
//! view, what a presentation of one view with that camera draws: the
//! entities, the colour terrain and the look are one, shared by both.
//! Plays an oracle scenario from the imported content (`OLR_CONTENT_DIR`);
//! skips without it.

use std::path::Path;

use olr_content::{Content, olx_level_id, olx_mod_id};
use olr_look::{Ambient, Grade, LevelLook};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{DrawList, Interpolation, Layer, SceneOptions, Viewport};

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

/// A plain level look: the match is lit (occluders, the look frame).
fn look() -> LevelLook {
    LevelLook {
        ambient: Ambient { colour: [255, 255, 255], intensity: 40 },
        occlusion: None,
        max_light: 100,
        light_steps: 0,
        softness: 0,
        light_soak: 0,
        solid_floor: Ambient { colour: [255, 255, 255], intensity: 0 },
        grade: Grade::default(),
        groups: Vec::new(),
    }
}

#[test]
fn two_views_draw_what_single_views_draw_over_shared_entities() {
    let Some(content) = content() else { return };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("fixtures/oracle");
    let (sc, trace) = olr_scenario::load_fixture(&root, "explode-kill-respawn").unwrap();
    let skin = content.skin_or_default("olx-skin-default");
    for lit in [false, true] {
        let mut level = content.load_level(&olx_level_id(&sc.level)).unwrap();
        level.look = lit.then(look);
        let theme = content.load_theme(&level.theme).unwrap();
        let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
        let setup = PresentSetup {
            core: content.core(),
            m: &m,
            level: &level,
            theme: &theme,
            viewport: Viewport::GAME,
            options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: true },
            seed: 1,
        };
        let mut pr: Option<[Presentation; 3]> = None;
        let mut lists = [DrawList::new(), DrawList::new()];
        let mut solo = [DrawList::new(), DrawList::new()];
        let mut shared = 0;
        olr_scenario::play(&content, &sc, &trace, |world, f, events| {
            let [both, one, two] = pr.get_or_insert_with(|| {
                let mut p = core::array::from_fn(|_| {
                    let mut p = Presentation::new(&setup, world);
                    for i in 0..sc.worms.len() {
                        p.add_worm(&skin, default_colour(i), i == 0, i == 0);
                    }
                    p
                });
                let [both, one, two] = &mut p;
                both.set_views(&[0, 1]);
                assert_eq!((both.views(), both.followed_view(1)), (2, Some(1)));
                // Each single view takes its view's camera (its shake's seed).
                one.scene.cameras_mut()[0] = both.scene.cameras()[0].clone();
                two.scene.cameras_mut()[0] = both.scene.cameras()[1].clone();
                p
            });
            if f >= 0 {
                for p in [&mut *both, &mut *one, &mut *two] {
                    p.on_tick(world, events);
                }
            }
            both.build_views(world, 0.5, &mut lists);
            one.build(world, 0.5, &mut solo[0]);
            two.build(world, 0.5, &mut solo[1]);
            for v in 0..2 {
                let a = both.assets();
                assert_eq!(lists[v].describe(a), solo[v].describe(a), "lit {lit} frame {f}: view {v}");
                assert!(lists[v] == solo[v], "lit {lit} frame {f}: view {v}");
            }
            assert_eq!(both.look(), one.look(), "one look for every view");
            assert_eq!(both.look().is_some(), lit);
            assert!(both.color().pixels() == one.color().pixels(), "one colour terrain");
            let listeners: Vec<_> = [&*one, &*two].iter().map(|p| p.scene.camera().listener()).collect();
            assert_eq!(&both.listeners()[..], listeners, "a listener per view");
            if lists.iter().all(|l| !l.layer(Layer::Entities).is_empty()) {
                shared += 1;
            }
        })
        .unwrap();
        assert!(shared > 0, "the explosion's entities show in both views");
        let [both, ..] = pr.as_mut().unwrap();
        both.set_viewport(olr_scene::Viewport::from_aspect(4.0 / 3.0, 16.0 / 9.0));
        assert_eq!(both.views(), 2, "a new view shape keeps the views");
        assert_eq!(both.followed_view(1), Some(1));
        both.set_views(&[1]);
        assert_eq!((both.views(), both.followed()), (1, 1));
    }
}
