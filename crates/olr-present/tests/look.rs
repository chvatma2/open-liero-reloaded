//! The level part of the look frame (plan 15a Task 13): none for bundled
//! content, the level's groups and lights for a lit level, no allocation
//! once warm. Plays oracle scenarios from the imported content
//! (`OLR_CONTENT_DIR`); skips without it.

use std::path::{Path, PathBuf};

use olr_content::{Content, olx_level_id, olx_mod_id};
use olr_look::{
    Ambient, Animation, GlowColour, GlowGroup, Grade, Key, LevelLook, Light, ModLook, PlacedLight,
    ProjectileLook, Reach,
};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{DrawList, Interpolation, Item, Layer, SceneOptions, Shade, Viewport};
use olr_sim::World;

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
        light_soak: 0,
        solid_floor: Ambient { colour: [255, 255, 255], intensity: 0 },
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
/// building a frame after every tick.
fn play(
    content: &Content,
    name: &str,
    level_look: Option<LevelLook>,
    mut each: impl FnMut(&mut Presentation, i64),
) {
    play_with(content, name, level_look, None, |p, _, _, f| each(p, f));
}

/// [`play`] with the mod's look replaced by `mod_look` too; `each` also
/// gets the world and the frame's draw list.
fn play_with(
    content: &Content,
    name: &str,
    level_look: Option<LevelLook>,
    mod_look: Option<ModLook>,
    each: impl FnMut(&mut Presentation, &World, &DrawList, i64),
) {
    play_every(content, name, level_look, mod_look, 1, each);
}

/// [`play_with`] drawing a frame after every `stride`th tick only (a
/// slower display).
fn play_every(
    content: &Content,
    name: &str,
    level_look: Option<LevelLook>,
    mod_look: Option<ModLook>,
    stride: i64,
    mut each: impl FnMut(&mut Presentation, &World, &DrawList, i64),
) {
    let (sc, trace) = olr_scenario::load_fixture(&fixtures(), name).unwrap();
    let mut level = content.load_level(&olx_level_id(&sc.level)).unwrap();
    level.look = level_look;
    let theme = content.load_theme(&level.theme).unwrap();
    let mut m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    if let Some(look) = mod_look {
        m.look = look;
    }
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
        if f.rem_euclid(stride) == 0 {
            p.build(world, 0.5, &mut list);
            each(p, world, &list, f);
        }
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

const BAZOOKA: &str = "weapons/classic/bazooka";

fn steady(colour: [u8; 3], intensity: u16, reach: u16) -> Light {
    Light { colour, intensity, reach: Reach::Pixels(reach), animation: Animation::Steady, phase_ms: None }
}

/// Every projectile of the scenario's mod glows at 80 with `light` and
/// `end_flash`.
fn mod_look(content: &Content, light: Option<Light>, end_flash: Option<Light>) -> ModLook {
    let (sc, _) = olr_scenario::load_fixture(&fixtures(), BAZOOKA).unwrap();
    let n = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap().sim.projectiles().len();
    let look = ProjectileLook { glow: Some(80), light, end_flash };
    ModLook { projectiles: vec![Some(look); n], weapons: Vec::new() }
}

/// A green flash: no engine light is green, so it tells end flashes apart.
fn green_flash() -> Light {
    let key = |at_ms, level| Key { at_ms, level, colour: None };
    Light {
        colour: [0, 255, 0],
        intensity: 200,
        reach: Reach::Pixels(96),
        animation: Animation::Keys { keys: vec![key(0, 100), key(250, 0)], looped: false },
        phase_ms: None,
    }
}

fn sprites(list: &DrawList, layer: Layer) -> impl Iterator<Item = &olr_scene::Sprite> {
    list.layer(layer).iter().filter_map(|i| match i {
        Item::Sprite(s) => Some(s),
        Item::Line(_) => None,
    })
}

#[test]
fn a_glowing_projectile_lights_and_glows() {
    let Some(content) = content() else { return };
    let look = mod_look(&content, Some(steady([255, 140, 40], 80, 48)), None);
    let mut lit_frames = 0;
    play_with(&content, BAZOOKA, None, Some(look), |p, _, list, _| {
        let l = p.look().expect("a mod with a look lights the match");
        let v = list.view;
        for d in p.scene.drawn_projectiles() {
            if v.contains(d.pos.x, d.pos.y) {
                assert!(l.lights.iter().any(|f| f.pos == [d.pos.x, d.pos.y]), "a light on {d:?}");
                lit_frames += 1;
            }
        }
        for s in sprites(list, Layer::Projectiles) {
            assert_eq!(s.shade, Shade { self_light: 80, marker: false });
        }
    });
    assert!(lit_frames > 10, "{lit_frames}");
}

#[test]
fn an_end_flash_follows_the_projectile() {
    let Some(content) = content() else { return };
    let look = mod_look(&content, None, Some(green_flash()));
    let mut last: Vec<olr_scene::DrawnProjectile> = Vec::new();
    let (mut ended, mut flashes) = (0, 0);
    let mut longest = 0i64;
    let mut seen: Vec<([f32; 2], i64)> = Vec::new();
    play_with(&content, BAZOOKA, None, Some(look), |p, _, _, f| {
        let l = p.look().unwrap();
        let green: Vec<[f32; 2]> =
            l.lights.iter().filter(|x| x.colour[0] == 0.0 && x.colour[1] > 0.0).map(|x| x.pos).collect();
        let now = p.scene.drawn_projectiles();
        for d in &last {
            if !now.iter().any(|n| (n.slot, n.serial) == (d.slot, d.serial)) {
                ended += 1;
                // Shown where it was last drawn (when its reach touches the view).
                if green.contains(&[d.pos.x, d.pos.y]) {
                    flashes += 1;
                }
            }
        }
        for g in &green {
            match seen.iter().find(|(pos, _)| pos == g) {
                Some((_, first)) => longest = longest.max(f - first),
                None => seen.push((*g, f)),
            }
        }
        last = now.to_vec();
    });
    assert!(ended > 0 && flashes > 0, "{ended} ended, {flashes} flashed");
    // 250 ms of keys at 10 ms a frame: gone after.
    assert!((20..=26).contains(&longest), "a flash lasted {longest} frames");
}

#[test]
fn engine_lights_only_in_lit_matches() {
    let Some(content) = content() else { return };
    play_with(&content, BAZOOKA, None, None, |p, _, list, _| {
        assert!(p.look().is_none());
        for (_, i) in list.items() {
            if let Item::Sprite(s) = i {
                assert_eq!(s.shade, Shade::LIT);
            }
        }
    });
    let (mut explosion, mut muzzle) = (false, false);
    play_with(&content, BAZOOKA, Some(look()), None, |p, _, _, _| {
        for l in &p.look().unwrap().lights {
            let ratio = l.colour[2] / l.colour[0].max(1e-6);
            // The engine's explosion (255, 200, 140) and muzzle (255, 220, 160), in linear.
            explosion |= (ratio - 0.262).abs() < 0.01;
            muzzle |= (ratio - 0.352).abs() < 0.01;
        }
    });
    assert!(explosion && muzzle, "explosion {explosion}, muzzle {muzzle}");
}

#[test]
fn the_light_cap_is_deterministic() {
    let Some(content) = content() else { return };
    let mut many = look();
    many.lights = (0..2000)
        .map(|i| PlacedLight {
            at: (i % 50, i / 50),
            radius: 1,
            light: Light {
                colour: [255, 255, 255],
                intensity: 10,
                reach: Reach::Unlimited,
                animation: Animation::Steady,
                phase_ms: None,
            },
        })
        .collect();
    let mut runs = Vec::new();
    for _ in 0..2 {
        let mut frames = Vec::new();
        play_with(&content, BAZOOKA, Some(many.clone()), None, |p, _, _, _| {
            let l = p.look().unwrap();
            assert_eq!(l.lights.len(), olr_look::MAX_LIGHTS);
            // The level's lights come first (plan 15a R9): its first 256 are kept.
            for (i, f) in l.lights.iter().enumerate() {
                assert_eq!(f.pos, [(i % 50) as f32 + 0.5, (i / 50) as f32 + 0.5]);
            }
            frames.push(l.lights.clone());
        });
        runs.push(frames);
    }
    assert_eq!(runs[0], runs[1]);
}

#[test]
fn floors_apply_only_in_lit_matches() {
    let Some(content) = content() else { return };
    for lit in [false, true] {
        let mut checked = 0;
        play_with(&content, "dig", lit.then(look), None, |p, _, list, _| {
            let skin = p.assets().worm(0).skin;
            let crosshair = p.assets().crosshair;
            for s in sprites(list, Layer::Worms) {
                if s.atlas == skin {
                    let want = if lit {
                        Shade { self_light: olr_look::engine::FLOOR, marker: false }
                    } else {
                        Shade::LIT
                    };
                    assert_eq!(s.shade, want);
                    checked += 1;
                }
                if s.atlas == crosshair {
                    assert_eq!(s.shade, if lit { Shade::MARKER } else { Shade::LIT });
                }
            }
        });
        assert!(checked > 0);
    }
}

/// Spec §Fair play: a frame depends on the world and the clock, never on
/// the frame rate. A projectile ending between two drawn frames flashes
/// as on a faster display.
#[test]
fn end_flashes_do_not_depend_on_the_frame_rate() {
    let Some(content) = content() else { return };
    let mut runs = Vec::new();
    for stride in [1, 4] {
        let mut keys = Vec::new();
        let look = mod_look(&content, None, Some(green_flash()));
        play_every(&content, BAZOOKA, None, Some(look), stride, |p, _, _, _| {
            keys.extend_from_slice(p.flashes_started());
        });
        keys.sort();
        runs.push(keys);
    }
    assert!(!runs[0].is_empty());
    assert_eq!(runs[0], runs[1]);
}

/// A weapon's muzzle light runs its keys (60 ms here), not one tick.
#[test]
fn a_muzzle_light_runs_its_curve() {
    let Some(content) = content() else { return };
    let (sc, _) = olr_scenario::load_fixture(&fixtures(), BAZOOKA).unwrap();
    let weapons = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap().sim.weapons().len();
    let key = |at_ms, level| Key { at_ms, level, colour: None };
    let muzzle = Light {
        colour: [0, 255, 0],
        intensity: 100,
        reach: Reach::Pixels(64),
        animation: Animation::Keys { keys: vec![key(0, 100), key(60, 0)], looped: false },
        phase_ms: None,
    };
    let weapon = olr_look::WeaponLook { muzzle_light: Some(muzzle) };
    let look = ModLook { projectiles: Vec::new(), weapons: vec![Some(weapon); weapons] };
    let (mut run, mut longest) = (Vec::new(), Vec::new());
    play_with(&content, BAZOOKA, None, Some(look), |p, _, _, _| {
        let green = p.look().unwrap().lights.iter().find(|l| l.colour[0] == 0.0 && l.colour[1] > 0.0);
        match green {
            Some(l) => run.push(l.colour[1]),
            None => {
                if run.len() > longest.len() {
                    longest = std::mem::take(&mut run);
                }
                run.clear();
            }
        }
    });
    // Frames 10 ms apart at alpha 0.5: about 6 frames of a 60 ms fade.
    assert!(longest.len() >= 5, "{longest:?}");
    assert!(longest.windows(2).all(|w| w[1] <= w[0]), "it fades: {longest:?}");
}
