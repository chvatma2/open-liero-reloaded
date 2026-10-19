//! Building a lit frame allocates nothing once warm (plan 15a Tasks 13
//! and 15): the scene, the entities and the look with every dynamic light.
//! Its own process, as it counts allocations.

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
        light_soak: 0,
        solid_floor: olr_look::Ambient { colour: [255, 255, 255], intensity: 0 },
        grade: Default::default(),
        groups: Vec::new(),
    });
    let theme = content.load_theme(&level.theme).unwrap();
    let mut m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    // Every projectile lights, glows and flashes, every weapon's muzzle lights.
    let light = |ms| olr_look::Light {
        colour: [255, 140, 40],
        intensity: 100,
        reach: olr_look::Reach::Pixels(48),
        animation: olr_look::Animation::Keys {
            keys: vec![
                olr_look::Key { at_ms: 0, level: 100, colour: None },
                olr_look::Key { at_ms: ms, level: 0, colour: None },
            ],
            looped: false,
        },
        phase_ms: None,
    };
    let projectile =
        olr_look::ProjectileLook { glow: Some(80), light: Some(light(5000)), end_flash: Some(light(250)) };
    let weapon = olr_look::WeaponLook { muzzle_light: Some(light(60)) };
    m.look = olr_look::ModLook {
        projectiles: vec![Some(projectile); m.sim.projectiles().len()],
        weapons: vec![Some(weapon); m.sim.weapons().len()],
    };
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
    let mut present: Option<Presentation> = None;
    let mut list = DrawList::new();
    let (mut measured, mut flashes) = (0, 0);
    // The first play warms every buffer; the second measures whole frames.
    for pass in 0..2 {
        olr_scenario::play(&content, &sc, &trace, |world, f, events| {
            let p = present.get_or_insert_with(|| {
                let mut p = Presentation::new(&setup, world);
                for i in 0..sc.worms.len() {
                    p.add_worm(&skin, default_colour(i), i == 0, i == 0);
                }
                p
            });
            if f < 0 && pass == 1 {
                p.restart(world, &level, &theme);
            }
            if f >= 0 {
                p.on_tick(world, events);
            }
            if pass == 0 || f <= 2 {
                p.build(world, 0.5, &mut list);
                return;
            }
            let a = allocation_counter::measure(|| p.build(world, 0.5, &mut list));
            assert_eq!(a.count_total, 0, "frame {f}: building the frame and its look allocated");
            measured += 1;
            flashes += p.flashes_started().len();
        })
        .unwrap();
    }
    assert!(measured > 0 && flashes > 0, "{measured} frames, {flashes} flashes");
}
