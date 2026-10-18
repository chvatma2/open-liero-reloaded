//! Building a lit frame's look allocates nothing once warm (plan 15a
//! Task 13): its own process, as it counts allocations.

use std::path::Path;

use olr_content::{Content, olx_level_id, olx_mod_id};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};

#[test]
fn look_frame_does_not_allocate_when_warm() {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("fixtures/oracle");
    let (sc, trace) = olr_scenario::load_fixture(&root, "two-worms-fire").unwrap();
    let mut level = content.load_level(&olx_level_id(&sc.level)).unwrap();
    level.look = Some(olr_look::LevelLook {
        ambient: olr_look::Ambient { colour: [255, 255, 255], intensity: 20 },
        max_light: 100,
        light_steps: 0,
        softness: 0,
        grade: Default::default(),
        groups: Vec::new(),
        lights: (0..20)
            .map(|i| olr_look::PlacedLight {
                at: (i * 40, 100),
                radius: 2,
                light: olr_look::Light {
                    colour: [255, 200, 100],
                    intensity: 100,
                    reach: olr_look::Reach::Pixels(200),
                    animation: olr_look::Animation::Flicker { speed: 8, depth: 25 },
                    phase_ms: None,
                },
            })
            .collect(),
    });
    let theme = content.load_theme(&level.theme).unwrap();
    let m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    let skin = content.skin_or_default("olx-skin-default");
    let setup = PresentSetup {
        core: content.core(),
        m: &m,
        level: &level,
        theme: &theme,
        viewport: Viewport::GAME,
        options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: true },
        seed: 1,
    };
    let mut present = None;
    let mut list = DrawList::new();
    let mut measured = 0;
    olr_scenario::play(&content, &sc, &trace, |world, f, events| {
        let p = present.get_or_insert_with(|| {
            let mut p = Presentation::new(&setup, world);
            for i in 0..sc.worms.len() {
                p.add_worm(&skin, default_colour(i), i == 0, i == 0);
            }
            p
        });
        if f >= 0 {
            p.on_tick(world, events);
        }
        p.build(world, 0.5, &mut list);
        if f > 2 {
            let a = allocation_counter::measure(|| p.build_look(world, 0.5, list.view));
            assert_eq!(a.count_total, 0, "frame {f}: building the look allocated");
            measured += 1;
        }
    })
    .unwrap();
    assert!(measured > 0);
}
