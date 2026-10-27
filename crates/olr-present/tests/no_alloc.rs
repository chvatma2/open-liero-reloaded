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
        occlusion: None,
        max_light: 100,
        light_steps: 0,
        softness: 0,
        light_soak: 0,
        solid_floor: olr_look::Ambient { colour: [255, 255, 255], intensity: 0 },
        grade: Default::default(),
        groups: Vec::new(),
        materials: Vec::new(),
        haze: 0,
    });
    let theme = content.load_theme(&level.theme).unwrap();
    let mut m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    // Every projectile lights, glows and flashes, every weapon's muzzle
    // lights; every other projectile explodes in its own light, the rest in
    // the mod's (issue #15).
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
    let explosion = |fitted| olr_look::ExplosionLight { light: light(400), radius: 10, fitted };
    let projectile = |i: usize| olr_look::ProjectileLook {
        glow: Some(80),
        light: Some(light(5000)),
        end_flash: Some(light(250)),
        explosion: i.is_multiple_of(2).then(|| explosion(false)),
        ..Default::default()
    };
    let weapon = olr_look::WeaponLook { muzzle_light: Some(light(60)), beam: None };
    m.look = olr_look::ModLook {
        projectiles: (0..m.sim.projectiles().len()).map(|i| Some(projectile(i))).collect(),
        weapons: vec![Some(weapon); m.sim.weapons().len()],
        explosion: Some(explosion(true)),
        ..Default::default()
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

/// Issue #87: on a level with live copies (`fixtures/live/levels/live-test`
/// over the content), drawing the copies after a tick and building a lit
/// frame with their glow groups allocate nothing once warm, while their
/// states change.
#[test]
fn live_copies_do_not_allocate_when_warm() {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else { return };
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/live/levels");
    let content = Content::open_layers(&[Path::new(&dir), &fixtures]).unwrap();
    let (mod_id, level_id) = ("olx-classic", "olr-level-live-test");
    let players = vec![olr_content::Player::Human, olr_content::Player::Human];
    let o = olr_content::MatchOptions::deathmatch(mod_id, level_id, 5, players);
    let mut world = content.build_match(&o).unwrap().world;
    let (m, level) = (content.load_mod(mod_id).unwrap(), content.load_level(level_id).unwrap());
    let theme = content.load_theme(&level.theme).unwrap();
    let setup = PresentSetup {
        core: content.core(),
        m: &m,
        level: &level,
        theme: &theme,
        viewport: Viewport::GAME,
        options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: true },
        seed: 1,
    };
    let mut p = Presentation::new(&setup, &world);
    assert!(p.has_look());
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..2 {
        p.add_worm(&skin, default_colour(i), i == 0, i == 0);
    }
    let live = world.live().unwrap().level().clone();
    let lamp = live.copy("lamp").unwrap();
    let lamp_states = live.copy_by_id("lamp").unwrap().states.len() as u16;
    let console = live.copy("console-a").unwrap();
    let console_states = live.copy_by_id("console-a").unwrap().states.len() as u16;
    let (mut list, mut events) = (DrawList::new(), olr_sim::TickEvents::new());
    let (mut measured, mut dirty) = (0, 0);
    for t in 0..200u16 {
        if t % 7 == 0 {
            world.set_live_state(lamp, t / 7 % lamp_states).unwrap();
            world.set_live_state(console, t / 7 % console_states).unwrap();
        }
        world.tick(&[olr_sim::Keys(0); 2], &mut events);
        let mut frame = || {
            p.on_tick(&world, &events);
            p.build(&world, 0.5, &mut list);
            dirty += p.color_mut().take_dirty().count();
        };
        if t < 50 {
            frame();
            continue;
        }
        let a = allocation_counter::measure(frame);
        assert_eq!(a.count_total, 0, "tick {t}: drawing the live copies allocated");
        measured += 1;
        assert!(p.look().unwrap().groups.len() >= 3, "the copies' groups");
    }
    assert!(measured > 0 && dirty > 0, "{measured} frames, {dirty} rectangles drawn anew");
}
