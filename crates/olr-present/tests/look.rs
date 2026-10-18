//! The level part of the look frame (plan 15a Task 13): none for bundled
//! content, the level's groups and lights for a lit level, no allocation
//! once warm. Plays oracle scenarios from the imported content
//! (`OLR_CONTENT_DIR`); skips without it.

use std::path::{Path, PathBuf};

use olr_content::{Content, olx_level_id, olx_mod_id};
use olr_look::{Ambient, Animation, GlowColour, GlowGroup, Grade, LevelLook, Light, PlacedLight, Reach};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};

fn content() -> Option<Content> {
    let dir = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir())?;
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("fixtures/oracle")
}

fn look() -> LevelLook {
    LevelLook {
        ambient: Ambient { colour: [255, 255, 255], intensity: 20 },
        max_light: 100,
        light_steps: 0,
        softness: 0,
        grade: Grade::default(),
        groups: vec![GlowGroup {
            id: "lava".into(),
            colour: GlowColour::Own,
            intensity: 100,
            reach: Reach::Pixels(64),
            animation: Animation::Pulse {
                period_ms: 1000,
                low: 0,
                high: 100,
                shape: olr_look::PulseShape::Square,
            },
            phase_ms: Some(0),
        }],
        lights: vec![PlacedLight {
            at: (100, 100),
            radius: 2,
            light: Light {
                colour: [255, 0, 0],
                intensity: 100,
                reach: Reach::Pixels(5000),
                animation: Animation::Steady,
                phase_ms: None,
            },
        }],
    }
}

/// Plays scenario `name` with the level's look replaced by `level_look`,
/// building a frame after every tick; returns the presentation.
fn play(
    content: &Content,
    name: &str,
    level_look: Option<LevelLook>,
    mut each: impl FnMut(&mut Presentation, i64),
) {
    let (sc, trace) = olr_scenario::load_fixture(&fixtures(), name).unwrap();
    let mut level = content.load_level(&olx_level_id(&sc.level)).unwrap();
    level.look = level_look;
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
    olr_scenario::play(content, &sc, &trace, |world, f, events| {
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
        each(p, f);
    })
    .unwrap();
}

#[test]
fn look_frame_is_none_for_bundled_content() {
    let Some(content) = content() else { return };
    play(&content, "dig", None, |p, _| {
        assert!(!p.has_look());
        assert!(p.look().is_none());
    });
}

#[test]
fn look_frame_level_part() {
    let Some(content) = content() else { return };
    let mut seen = Vec::new();
    play(&content, "dig", Some(look()), |p, f| {
        assert!(p.has_look());
        let l = p.look().expect("a lit level's frame has a look");
        assert!((l.ambient[0] - 0.2).abs() < 1e-6);
        assert_eq!(l.groups.len(), 1);
        assert_eq!(l.lights.len(), 1, "the lamp's reach touches the view");
        assert_eq!(l.lights[0].pos, [100.5, 100.5]);
        seen.push((f, l.groups[0].self_scale));
    });
    // The pulse follows the match clock: tick 10 ms each, alpha 0.5.
    assert!(seen.iter().any(|(_, s)| *s == 1.0) && seen.iter().any(|(_, s)| *s == 0.0), "{seen:?}");
}
