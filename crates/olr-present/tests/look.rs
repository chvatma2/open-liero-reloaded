//! The level part of the look frame (plan 15a Task 13): none for bundled
//! content, the level's groups and lights for a lit level, no allocation
//! once warm. Plays oracle scenarios from the imported content
//! (`OLR_CONTENT_DIR`); skips without it.

use std::path::{Path, PathBuf};

use olr_content::{Content, LoadedMod, LoadedSkin, SandboxSetup, SpawnSpec, olx_level_id, olx_mod_id};
use olr_data::skin::{MaskKind, SkinLight};
use olr_look::{
    Ambient, Animation, GlowColour, GlowGroup, Grade, Key, LevelLook, Light, ModLook, ProjectileLook, Reach,
    engine,
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
        occlusion: None,
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
    each: impl FnMut(&mut Presentation, &World, &DrawList, i64),
) {
    let skin = content.skin_or_default("olx-skin-default");
    play_skinned(content, name, level_look, mod_look, &skin, stride, |_| {}, each);
}

/// [`play_every`] with every worm in `skin`, `before` run on the
/// presentation once its worms are added.
#[allow(clippy::too_many_arguments)]
fn play_skinned(
    content: &Content,
    name: &str,
    level_look: Option<LevelLook>,
    mod_look: Option<ModLook>,
    skin: &LoadedSkin,
    stride: i64,
    before: impl FnOnce(&mut Presentation),
    each: impl FnMut(&mut Presentation, &World, &DrawList, i64),
) {
    let edit = |m: &mut LoadedMod| {
        if let Some(look) = mod_look {
            m.look = look;
        }
    };
    play_modded(content, name, level_look, edit, skin, stride, before, each);
}

/// [`play_skinned`] with the mod changed by `edit` once loaded.
#[allow(clippy::too_many_arguments)]
fn play_modded(
    content: &Content,
    name: &str,
    level_look: Option<LevelLook>,
    edit: impl FnOnce(&mut LoadedMod),
    skin: &LoadedSkin,
    stride: i64,
    before: impl FnOnce(&mut Presentation),
    mut each: impl FnMut(&mut Presentation, &World, &DrawList, i64),
) {
    let (sc, trace) = olr_scenario::load_fixture(&fixtures(), name).unwrap();
    let mut level = content.load_level(&olx_level_id(&sc.level)).unwrap();
    level.look = level_look;
    let theme = content.load_theme(&level.theme).unwrap();
    let mut m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    edit(&mut m);
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
    let mut before = Some(before);
    let mut list = DrawList::new();
    olr_scenario::play(content, &sc, &trace, |world, f, events| {
        let p = present.get_or_insert_with(|| {
            let mut p = Presentation::new(&setup, world);
            for i in 0..sc.worms.len() {
                p.add_worm(skin, default_colour(i), i == 0, i == 0);
            }
            if let Some(b) = before.take() {
                b(&mut p);
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
    let look = ProjectileLook { glow: Some(80), light, end_flash, explosion: None };
    ModLook { projectiles: vec![Some(look); n], ..Default::default() }
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
                assert!(l.emitters.iter().any(|f| f.pos == [d.pos.x, d.pos.y]), "a light on {d:?}");
                lit_frames += 1;
            }
        }
        for s in sprites(list, Layer::Projectiles) {
            assert_eq!(s.shade, Shade { self_light: 80, ..Shade::LIT });
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
            l.emitters.iter().filter(|x| x.colour[0] == 0.0 && x.colour[1] > 0.0).map(|x| x.pos).collect();
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
        for l in &p.look().unwrap().emitters {
            let ratio = l.colour[2] / l.colour[0].max(1e-6);
            // The engine's explosion (255, 200, 140) and muzzle (255, 220, 160), in linear.
            explosion |= (ratio - 0.262).abs() < 0.01;
            muzzle |= (ratio - 0.352).abs() < 0.01;
        }
    });
    assert!(explosion && muzzle, "explosion {explosion}, muzzle {muzzle}");
}

/// Plan 15c: play's lights are glowing discs in the glow light, with the
/// engine's radii: an explosion's grows with its size, a muzzle flash's is
/// about an eighth of its reach.
#[test]
fn engine_lights_are_emitters() {
    let Some(content) = content() else { return };
    let (mut explosion, mut muzzle) = (0, 0);
    play_with(&content, BAZOOKA, Some(look()), None, |p, _, _, _| {
        let l = p.look().unwrap();
        for e in &l.emitters {
            let ratio = e.colour[2] / e.colour[0].max(1e-6);
            if (ratio - 0.262).abs() < 0.01 {
                assert!((0..=500).any(|s| engine::explosion_radius(s) == e.radius), "{e:?}");
                explosion += 1;
            }
            if (ratio - 0.352).abs() < 0.01 {
                assert_eq!(e.radius, engine::disc_radius(40.0), "{e:?}");
                muzzle += 1;
            }
        }
    });
    assert!(explosion > 0 && muzzle > 0, "explosion {explosion}, muzzle {muzzle}");
}

/// Play's discs are the same, in the same order, every run.
#[test]
fn emitters_are_deterministic() {
    let Some(content) = content() else { return };
    let mut runs = Vec::new();
    for _ in 0..2 {
        let mut frames = Vec::new();
        play_with(&content, BAZOOKA, Some(look()), None, |p, _, _, _| {
            frames.push(p.look().unwrap().emitters.clone());
        });
        runs.push(frames);
    }
    assert!(runs[0].iter().any(|f| !f.is_empty()));
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
                    // Plan 15b: worms take the worm floor, not a self-light.
                    assert_eq!(s.shade, if lit { Shade::WORM } else { Shade::LIT });
                    checked += 1;
                }
                if s.atlas == crosshair {
                    assert_eq!(s.shade, if lit { Shade::MARKER } else { Shade::LIT });
                }
            }
            // Projectiles keep the engine floor as self-light.
            let floor = Shade { self_light: olr_look::engine::FLOOR, ..Shade::LIT };
            for s in sprites(list, Layer::Projectiles) {
                assert_eq!(s.shade, if lit { floor } else { Shade::LIT });
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
    let look = ModLook { weapons: vec![Some(weapon); weapons], ..Default::default() };
    let (mut run, mut longest) = (Vec::new(), Vec::new());
    play_with(&content, BAZOOKA, None, Some(look), |p, _, _, _| {
        let green = p.look().unwrap().emitters.iter().find(|l| l.colour[0] == 0.0 && l.colour[1] > 0.0);
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

/// Batch D review Important 3: a weapon firing every few ticks keeps one
/// muzzle light, restarted by each shot, not one per shot stacked.
#[test]
fn muzzle_lights_do_not_stack() {
    let Some(content) = content() else { return };
    let (sc, _) = olr_scenario::load_fixture(&fixtures(), "fire-minigun").unwrap();
    let weapons = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap().sim.weapons().len();
    let key = |at_ms, level| Key { at_ms, level, colour: None };
    let muzzle = Light {
        colour: [0, 255, 0],
        intensity: 100,
        reach: Reach::Pixels(64),
        animation: Animation::Keys { keys: vec![key(0, 100), key(200, 0)], looped: false },
        phase_ms: None,
    };
    let weapon = olr_look::WeaponLook { muzzle_light: Some(muzzle) };
    let look = ModLook { weapons: vec![Some(weapon); weapons], ..Default::default() };
    let mut most = 0;
    play_with(&content, "fire-minigun", None, Some(look), |p, _, _, _| {
        let n = p.look().unwrap().emitters.iter().filter(|l| l.colour[0] == 0.0 && l.colour[1] > 0.0).count();
        most = most.max(n);
    });
    assert_eq!(most, 1, "one worm fires: one muzzle light");
}

/// Plan 15b: in a lit match each living worm's body is an occluder (its
/// index + 1), cut as its skin sprite; an unlit match lists none.
#[test]
fn living_worms_are_occluders() {
    let Some(content) = content() else { return };
    for lit in [false, true] {
        let mut checked = 0;
        play_with(&content, "dig", lit.then(look), None, |p, world, list, _| {
            if !lit {
                assert!(list.occluders.is_empty());
                return;
            }
            let living: Vec<u8> =
                world.worms().iter().enumerate().filter(|(_, w)| w.alive).map(|(i, _)| i as u8 + 1).collect();
            let ids: Vec<u8> = list.occluders.iter().map(|o| o.worm).collect();
            assert_eq!(ids, living);
            let skin = p.assets().worm(0).skin;
            if let (Some(o), Some(s)) = (
                list.occluders.iter().find(|o| o.worm == 1),
                sprites(list, Layer::Worms).find(|s| s.atlas == skin),
            ) {
                assert_eq!((o.atlas, o.src, o.pos, o.size, o.flip), (s.atlas, s.src, s.pos, s.size, s.flip));
                checked += 1;
            }
        });
        assert!(!lit || checked > 0);
    }
}

/// Final review, Important 3: the engine's explosion and muzzle lights come
/// before shot lights and end flashes, so a match past the disc cap keeps
/// them.
#[test]
fn engine_lights_come_first() {
    let Some(content) = content() else { return };
    let mods = mod_look(&content, Some(steady([0, 255, 0], 80, 48)), Some(green_flash()));
    let mut checked = 0;
    play_with(&content, BAZOOKA, Some(look()), Some(mods), |p, _, _, _| {
        let e = &p.look().unwrap().emitters;
        let engine = |f: &olr_look::FrameLight| f.colour[0] > 0.0;
        if let Some(last) = e.iter().rposition(engine)
            && let Some(first_mod) = e.iter().position(|f| !engine(f))
        {
            assert!(last < first_mod, "engine lights first: {e:?}");
            checked += 1;
        }
    });
    assert!(checked > 0);
}

/// Issue #12: `content`'s default skin with its tinted pixels glowing at
/// full and `light`.
fn glowing_skin(content: &Content, light: SkinLight) -> LoadedSkin {
    let mut s = content.skin_or_default("olx-skin-default");
    s.skin.glow = s.skin.mask.iter().map(|m| if *m == MaskKind::Colorize { 255 } else { 0 }).collect();
    s.skin.def.light = Some(light);
    s
}

const SKIN_LIGHT: SkinLight = SkinLight { intensity: 150, reach: 24 };

/// The emitters at a drawn worm's position with a skin light's disc.
fn skin_emitters(p: &Presentation, reach: u16) -> Vec<(usize, &olr_look::FrameLight)> {
    let radius = engine::disc_radius(f32::from(reach));
    let Some(l) = p.look() else { return Vec::new() };
    p.scene
        .drawn_worms()
        .iter()
        .flat_map(|w| {
            l.emitters
                .iter()
                .filter(move |e| {
                    e.pos == [w.pos.x, w.pos.y] && e.radius == radius && e.reach == f32::from(reach)
                })
                .map(move |e| (w.index, e))
        })
        .collect()
}

/// Issue #12: a skin with a light casts one disc per drawn worm, at it,
/// of its tinted glowing pixels' colour, after the engine's lights.
#[test]
fn a_skin_light_follows_its_worm_in_its_colour() {
    let Some(content) = content() else { return };
    let skin = glowing_skin(&content, SKIN_LIGHT);
    let mut checked = 0;
    play_skinned(
        &content,
        BAZOOKA,
        Some(look()),
        None,
        &skin,
        1,
        |_| {},
        |p, _, _, _| {
            let found = skin_emitters(p, SKIN_LIGHT.reach);
            assert_eq!(found.len(), p.scene.drawn_worms().len(), "one light a worm");
            for (i, e) in &found {
                // Red, then blue (default_colour): the tint is the light's.
                let main = if *i == 0 { 0 } else { 2 };
                assert!(
                    e.colour[main] > 0.0 && (0..3).all(|c| c == main || e.colour[c] <= e.colour[main]),
                    "{e:?}"
                );
                checked += 1;
            }
            let emitters = &p.look().unwrap().emitters;
            if let (Some((_, first)), Some(last_engine)) = (
                found.first(),
                emitters.iter().rposition(|e| e.colour[0] > 0.0 && e.radius != found[0].1.radius),
            ) {
                let at = emitters.iter().position(|e| std::ptr::eq(e, *first)).unwrap();
                assert!(at > last_engine, "the skins' lights come last");
            }
        },
    );
    assert!(checked > 20, "{checked}");
}

/// Issue #12: the engine switch off, a match lit only by its mod, or an
/// unlit one: no skin casts light; its glowing pixels still draw bright in
/// a lit match only.
#[test]
fn skin_lights_need_a_lit_level_and_the_switch() {
    let Some(content) = content() else { return };
    let skin = glowing_skin(&content, SKIN_LIGHT);
    let full = Shade { self_light: 100, ..Shade::WORM };
    let cases = [
        (Some(look()), None, false, true),
        (None, Some(mod_look(&content, Some(steady([0, 255, 0], 80, 48)), None)), true, true),
        (None, None, true, false),
    ];
    for (level, mods, switch, lit) in cases {
        let (mut frames, mut bright) = (0, 0);
        play_skinned(
            &content,
            BAZOOKA,
            level,
            mods,
            &skin,
            1,
            |p| p.set_skin_light(switch),
            |p, _, list, _| {
                assert_eq!(p.has_look(), lit);
                assert!(skin_emitters(p, SKIN_LIGHT.reach).is_empty());
                let glow = p.assets().worm(0).glow[3].expect("a full glow overlay");
                bright += sprites(list, Layer::Worms).filter(|s| s.atlas == glow && s.shade == full).count();
                frames += 1;
            },
        );
        assert_eq!(bright > 0, lit, "glowing pixels draw only lit: {bright} of {frames}");
    }
}

/// Issue #12: 32 worms whose skin asks far past the caps: each light is
/// clamped to them, the frame stays within `MAX_EMITTERS`, and nothing
/// panics.
#[test]
fn skin_light_caps_hold_with_many_worms() {
    let Some(content) = content() else { return };
    let (sc, _) = olr_scenario::load_fixture(&fixtures(), "dig").unwrap();
    let mut skin = glowing_skin(&content, SkinLight { intensity: u16::MAX, reach: u16::MAX });
    skin.skin.glow.fill(255);
    let sandbox = SandboxSetup {
        mod_id: olx_mod_id(&sc.mod_dir),
        level_id: olx_level_id(&sc.level),
        seed: 3,
        settings: olr_sim::SimSettings::lx56(),
        worms: vec![SpawnSpec::default(); olr_sim::MAX_WORMS],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    let mut world = content.new_world(&sandbox).unwrap();
    let mut level = content.load_level(&sandbox.level_id).unwrap();
    level.look = Some(look());
    let theme = content.load_theme(&level.theme).unwrap();
    let m = content.load_mod(&sandbox.mod_id).unwrap();
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
    for i in 0..olr_sim::MAX_WORMS {
        p.add_worm(&skin, default_colour(i % 4), i == 0, i == 0);
    }
    let reach = engine::MAX_SKIN_LIGHT_REACH;
    let (mut list, mut events) = (DrawList::new(), olr_sim::TickEvents::new());
    let fire = vec![olr_sim::Keys(olr_sim::Keys::FIRE); olr_sim::MAX_WORMS];
    for t in 0..60 {
        world.tick(&fire, &mut events);
        p.on_tick(&world, &events);
        p.build(&world, 0.5, &mut list);
        let lights = skin_emitters(&p, reach);
        assert_eq!(lights.len(), p.scene.drawn_worms().len(), "tick {t}");
        let cap = f32::from(engine::MAX_SKIN_LIGHT_INTENSITY) / 100.0;
        for (_, e) in &lights {
            assert!(e.colour.iter().all(|c| c.is_finite() && *c <= cap + 1e-3), "{e:?} over {cap}");
        }
        assert!(p.look().unwrap().emitters.len() <= olr_look::MAX_EMITTERS);
    }
    assert_eq!(p.scene.drawn_worms().len(), olr_sim::MAX_WORMS);
}

// ---- Issue #15: a projectile's explosion light ----

/// A hand-made mod over `content`'s Classic: weapon 0's pellet bounces with
/// an explosion of size 6 off the map, bounces off worms and explodes on
/// its timer (damage 7); weapon 1's rocket explodes on the map and a worm
/// (damage 9).
fn exploding_mod(content: &Content) -> (olr_content::LoadedMod, i32, i32) {
    use olr_data::mod_def::*;
    let projectile = |hit, player_hit, timer| ProjectileDef {
        gfx: ProjectileGfx::Pixel { colours: vec![olr_data::image::Rgba::rgb(208, 215, 208)] },
        gravity: Some(100),
        dampening: 1.0,
        width: 2,
        height: 2,
        timer,
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit,
        player_hit,
        event_spawn: None,
    };
    let explode = |damage| ActionDef { damage, ..ActionDef::olx_default(ActionKind::Explode) };
    let pellet = projectile(
        ActionDef { bounce_coeff: 0.7, bounce_explode: 6, ..ActionDef::olx_default(ActionKind::Bounce) },
        ActionDef { bounce_coeff: 0.7, ..ActionDef::olx_default(ActionKind::Bounce) },
        TimerDef { time: 0.6, time_var: 0.0, action: Some(explode(7)) },
    );
    let rocket = projectile(explode(9), explode(9), TimerDef { time: 0.0, time_var: 0.0, action: None });
    let weapon = |id: &str| WeaponDef {
        id: id.into(),
        name: id.into(),
        recharge: 0.14,
        drain: 1.43,
        rof: 0.1,
        laser_sight: false,
        kind: WeaponKind::Projectile {
            class: WeaponClass::Automatic,
            recoil: 0,
            sound: None,
            spawn: SpawnDef { amount: 1, speed: 200, use_angle: true, ..SpawnDef::olx_default(id.into()) },
        },
    };
    let (sc, _) = olr_scenario::load_fixture(&fixtures(), "dig").unwrap();
    let mut m = content.load_mod(&olx_mod_id(&sc.mod_dir)).unwrap();
    let def = ModDef {
        name: "Explosions".into(),
        worm: m.worm,
        rope: m.rope,
        weapons: vec![weapon("pellet"), weapon("rocket")],
        projectiles: [("pellet".to_string(), pellet), ("rocket".to_string(), rocket)].into_iter().collect(),
        restrictions: Default::default(),
        loadouts: Default::default(),
    };
    let sim = olr_sim::SimMod::compile(&def, &Default::default()).unwrap();
    let index = |key: &str| sim.projectiles().iter().position(|p| p.key == key).unwrap() as i32;
    let (pellet, rocket) = (index("pellet"), index("rocket"));
    m.sim = std::sync::Arc::new(sim);
    m.look = ModLook::default();
    (m, pellet, rocket)
}

/// One explosion light seen: the explosion's projectile and size, and the
/// disc at its position.
type Seen = (i32, i32, olr_look::FrameLight);

/// Plays [`exploding_mod`] in a dark level with `mod_look` for 2 s, worm 0
/// firing pellets and worm 1 rockets for the first 0.3 s: every explosion's
/// disc, and the world's checksum each tick.
fn explode(content: &Content, mod_look: ModLook) -> (Vec<Seen>, Vec<u64>) {
    let (mut m, ..) = exploding_mod(content);
    m.look = mod_look;
    let (sc, _) = olr_scenario::load_fixture(&fixtures(), "dig").unwrap();
    let sandbox = SandboxSetup {
        mod_id: olx_mod_id(&sc.mod_dir),
        level_id: olx_level_id(&sc.level),
        seed: 5,
        settings: olr_sim::SimSettings::lx56(),
        worms: vec![SpawnSpec { pos: None, weapons: vec![0] }, SpawnSpec { pos: None, weapons: vec![1] }],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    let mut level = content.load_level(&sandbox.level_id).unwrap();
    level.look = Some(look());
    let theme = content.load_theme(&level.theme).unwrap();
    let mut world = olr_content::new_world_from(&m, &level, &theme, &sandbox).unwrap();
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
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..2 {
        p.add_worm(&skin, default_colour(i), i == 0, i == 0);
    }
    let (mut list, mut events) = (DrawList::new(), olr_sim::TickEvents::new());
    let (mut seen, mut sums) = (Vec::new(), Vec::new());
    for t in 0..200 {
        let keys = olr_sim::Keys(if t < 30 { olr_sim::Keys::FIRE } else { 0 });
        world.tick(&[keys; 2], &mut events);
        sums.push(world.checksum());
        p.on_tick(&world, &events);
        p.build(&world, 0.0, &mut list);
        let l = p.look().unwrap();
        for (_, e) in p.fx.entities() {
            let olr_fx::Kind::Explosion { proj } = e.kind else { continue };
            for f in l.emitters.iter().filter(|f| f.pos == [e.pos.x, e.pos.y]) {
                seen.push((proj, e.type2, *f));
            }
        }
    }
    (seen, sums)
}

/// `f`'s colour is `colour` (sRGB) in linear light, at most `intensity`
/// percent of it.
fn colour_of(f: &olr_look::FrameLight, colour: [u8; 3]) -> bool {
    let want = colour.map(olr_look::srgb_to_linear);
    let k = f.colour[2] / want[2];
    (0..3).all(|i| (f.colour[i] - want[i] * k).abs() < 1e-4 * k.max(1.0))
}

/// Issue #15: a projectile's explosion light replaces the engine's for
/// each of its explosions (a bounce's, a timer's, a hit's): its colour,
/// intensity, disc and reach; the mod's applies to a projectile without
/// one, and without either the engine's does. The simulation never sees it.
#[test]
fn a_projectile_explosion_light_replaces_the_engines() {
    let Some(content) = content() else { return };
    let (m, pellet, rocket) = exploding_mod(&content);
    let blue = [128, 176, 255];
    let red = [255, 64, 32];
    let own =
        olr_look::ExplosionLight { light: engine::explosion_flash(blue, 400, 120), radius: 10, fitted: true };
    let wide = olr_look::ExplosionLight {
        light: Light { animation: Animation::Steady, ..steady(red, 150, 300) },
        radius: 4,
        fitted: false,
    };
    let mut projectiles = vec![None; m.sim.projectiles().len()];
    projectiles[pellet as usize] =
        Some(ProjectileLook { explosion: Some(own.clone()), ..Default::default() });
    let with_wide = ModLook { projectiles: projectiles.clone(), explosion: Some(wide), ..Default::default() };
    let (seen, sums) = explode(&content, with_wide);
    // The pellet's: blue at up to 400 %, a disc of 10, reach 120, from its
    // bounces (size 6) and its timer (size 7).
    let pellets: Vec<&Seen> = seen.iter().filter(|s| s.0 == pellet).collect();
    for (_, size, f) in &pellets {
        assert!(colour_of(f, blue), "{size}: {f:?}");
        assert_eq!((f.radius, f.reach), (10.0, 120.0), "{f:?}");
        assert!(f.colour[2] <= 4.0 + 1e-4, "{f:?}");
    }
    let brightest = pellets.iter().map(|s| s.2.colour[2]).fold(0.0, f32::max);
    assert!(brightest > 3.0, "the flash starts near 400 %: {brightest}");
    let sizes: std::collections::BTreeSet<i32> = pellets.iter().map(|s| s.1).collect();
    assert_eq!(sizes, [6, 7].into(), "a bounce's and a timer's");
    // The rocket has none of its own: the mod's, steady red at 150 %, for
    // its hits (size 9).
    let rockets: Vec<&Seen> = seen.iter().filter(|s| s.0 == rocket).collect();
    assert!(!rockets.is_empty() && rockets.iter().all(|s| s.1 == 9));
    for (_, _, f) in &rockets {
        assert!(colour_of(f, red) && (f.colour[0] - 1.5).abs() < 1e-4, "{f:?}");
        assert_eq!((f.radius, f.reach), (4.0, 300.0), "{f:?}");
    }
    // Without the mod's, the rocket's are the engine's.
    let (seen, plain) = explode(&content, ModLook { projectiles, ..Default::default() });
    let rockets: Vec<&Seen> = seen.iter().filter(|s| s.0 == rocket).collect();
    assert!(!rockets.is_empty());
    for (_, size, f) in &rockets {
        let e = engine::explosion_light(*size);
        assert!(colour_of(f, e.colour), "{f:?}");
        assert_eq!((f.radius, f.reach), (engine::explosion_radius(*size), e.reach.pixels()), "{f:?}");
    }
    assert!(seen.iter().filter(|s| s.0 == pellet).all(|s| colour_of(&s.2, blue)));
    // The simulation is the same whatever the look.
    assert_eq!(sums, plain);
    assert_eq!(sums, explode(&content, ModLook::default()).1);
}

/// Issue #19: the bazooka's missile with a glow mask (every opaque pixel
/// of its first frame's top half full, the rest none) draws its glowing
/// pixels over the sprite at full self-light in a lit match, the same
/// frame and place; unlit, nothing more is drawn.
#[test]
fn a_projectiles_glowing_pixels_draw_bright_in_a_lit_match() {
    let Some(content) = content() else { return };
    let skin = content.skin_or_default("olx-skin-default");
    let file = "gfx/missile.png";
    let edit = |m: &mut LoadedMod| {
        let img = &m.images[file];
        let (w, h) = (img.width(), img.height());
        let mask =
            (0..w * h).map(|i| if i / w < h / 2 && img.get(i % w, i / w).a != 0 { 255 } else { 0 }).collect();
        m.glow.insert(file.into(), mask);
    };
    for lit in [true, false] {
        let mut glowing = 0;
        let level = lit.then(look);
        play_modded(
            &content,
            BAZOOKA,
            level,
            edit,
            &skin,
            1,
            |_| {},
            |p, _, list, _| {
                let all: Vec<_> = sprites(list, Layer::Projectiles).collect();
                for (i, s) in all.iter().enumerate() {
                    let name = &p.assets().image(s.atlas).name;
                    if name.starts_with("mod:gfx/missile.png:glow") {
                        assert!(lit, "an unlit match draws no glow");
                        assert_eq!(name, "mod:gfx/missile.png:glow4");
                        assert_eq!(s.shade.self_light, 100);
                        let under = all[i - 1];
                        assert_eq!(p.assets().image(under.atlas).name, "mod:gfx/missile.png");
                        assert_eq!((under.src, under.pos, under.size), (s.src, s.pos, s.size));
                        glowing += 1;
                    }
                }
            },
        );
        assert_eq!(glowing > 10, lit, "{glowing}");
    }
}
