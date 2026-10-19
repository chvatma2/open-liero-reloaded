//! `LookFrame` building, fade and culling (spec §Light sources, §Pipeline).

use olr_look::{
    Ambient, Animation, FrameLight, GlowColour, GlowGroup, Grade, LevelLook, Light, LookFrame, MAX_EMITTERS,
    MAX_LIGHTS, PlacedLight, Reach, ViewMode, engine, fade, light_at, srgb_to_linear,
};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn fade_profile() {
    assert!(near(fade(0.0, Reach::Pixels(100)), 1.0));
    assert!(near(fade(50.0, Reach::Pixels(100)), 0.5625));
    assert!(near(fade(100.0, Reach::Pixels(100)), 0.0));
    assert!(near(fade(150.0, Reach::Pixels(100)), 0.0));
    assert!(near(fade(1e9, Reach::Unlimited), 1.0));
    assert!(near(fade(1.0, Reach::Pixels(0)), 0.0));
    assert!(near(fade(f32::NAN, Reach::Pixels(10)), 0.0));
}

fn white(intensity: u16, reach: Reach) -> Light {
    Light { colour: [255, 255, 255], intensity, reach, animation: Animation::Steady, phase_ms: None }
}

fn level() -> LevelLook {
    LevelLook {
        ambient: Ambient { colour: [255, 255, 255], intensity: 30 },
        max_light: 150,
        light_steps: 4,
        softness: 2,
        light_soak: 6,
        solid_floor: Ambient { colour: [255, 255, 255], intensity: 40 },
        shadows: olr_look::Shadows::Smooth,
        grade: Grade { exposure: 50, contrast: 110, saturation: 90, tint: [255, 240, 220] },
        groups: vec![
            GlowGroup {
                id: "lava".into(),
                colour: GlowColour::Tint([255, 0, 0]),
                intensity: 50,
                reach: Reach::Pixels(64),
                animation: Animation::Steady,
                phase_ms: None,
            },
            GlowGroup {
                id: "sky".into(),
                colour: GlowColour::Own,
                intensity: 200,
                reach: Reach::Unlimited,
                animation: Animation::Steady,
                phase_ms: None,
            },
        ],
        lights: vec![PlacedLight { at: (10, 20), radius: 2, light: white(100, Reach::Pixels(160)) }],
    }
}

#[test]
fn begin_fills_level_part() {
    let mut f = LookFrame::default();
    f.begin(Some(&level()), 0.0);
    assert!(near(f.ambient[0], 0.3) && near(f.ambient[2], 0.3));
    assert!(near(f.max_light, 1.5));
    assert_eq!((f.light_steps, f.softness, f.light_soak), (4, 2, 6));
    assert!(f.solid_floor.iter().all(|&c| near(c, 0.4)));
    assert_eq!(f.shadow_filter, 1, "smooth");
    assert_eq!(f.grade.exposure, 50);
    assert_eq!(f.mode, ViewMode::Lit);
    assert_eq!(f.groups.len(), 2);
    let lava = f.groups[0];
    assert!(!lava.own_colour && near(lava.cast[0], 0.5) && near(lava.cast[1], 0.0));
    assert!(near(lava.self_scale, 1.0) && near(lava.reach, 64.0));
    let sky = f.groups[1];
    assert!(sky.own_colour && near(sky.cast[1], 2.0) && sky.reach.is_infinite());
    assert_eq!(f.lights.len(), 1);
    let l = f.lights[0];
    assert_eq!(l.pos, [10.5, 20.5]);
    assert!(near(l.radius, 2.0) && near(l.colour[0], 1.0) && near(l.reach, 160.0));
    // Begin again: cleared, not appended.
    f.begin(Some(&level()), 5.0);
    assert_eq!((f.groups.len(), f.lights.len()), (2, 1));
}

#[test]
fn begin_without_a_level_look_is_full_bright() {
    let mut f = LookFrame::default();
    f.begin(None, 0.0);
    assert!(near(f.ambient[0], 1.0) && near(f.max_light, 1.0));
    assert!(f.groups.is_empty() && f.lights.is_empty());
    assert_eq!(f.grade, Grade::default());
}

#[test]
fn an_animated_group_scales_self_light_and_cast() {
    let mut look = level();
    look.groups[0].animation =
        Animation::Pulse { period_ms: 1000, low: 50, high: 50, shape: olr_look::PulseShape::Square };
    let mut f = LookFrame::default();
    f.begin(Some(&look), 0.0);
    assert!(near(f.groups[0].self_scale, 0.5));
    assert!(near(f.groups[0].cast[0], 0.25));
}

fn at(x: f32, y: f32, reach: f32) -> FrameLight {
    FrameLight { pos: [x, y], radius: 2.0, colour: [1.0; 3], reach, owner: 0 }
}

#[test]
fn cull_keeps_order_and_cap() {
    let mut f = LookFrame::default();
    f.begin(None, 0.0);
    for i in 0..300 {
        let off = i % 30 == 7; // 10 of them far off the view
        f.push(if off { at(-5000.0, 0.0, 10.0) } else { at(i as f32, 50.0, 20.0 + i as f32) });
    }
    f.cull([0.0, 0.0, 427.0, 192.0]);
    assert_eq!(f.lights.len(), MAX_LIGHTS);
    assert!(f.lights.iter().all(|l| l.pos[0] >= 0.0));
    assert!(f.lights.windows(2).all(|w| w[0].reach < w[1].reach), "order kept");
}

#[test]
fn a_light_far_off_view_whose_reach_touches_it_is_kept() {
    let mut f = LookFrame::default();
    f.begin(None, 0.0);
    f.push(at(-500.0, 100.0, 600.0));
    f.push(at(-500.0, 100.0, 400.0));
    f.push(at(10_000.0, 100.0, f32::INFINITY));
    f.cull([0.0, 0.0, 427.0, 192.0]);
    assert_eq!(f.lights.len(), 2);
    assert!(near(f.lights[0].reach, 600.0) && f.lights[1].reach.is_infinite());
}

#[test]
fn light_at_applies_animation_and_ends() {
    let flash = Light {
        colour: [255, 0, 0],
        intensity: 200,
        reach: Reach::Pixels(96),
        animation: Animation::Keys {
            keys: vec![
                olr_look::Key { at_ms: 0, level: 100, colour: None },
                olr_look::Key { at_ms: 250, level: 0, colour: None },
            ],
            looped: false,
        },
        phase_ms: Some(0),
    };
    let l = light_at(&flash, 125.0, 9, [3.0, 4.0], 2.0).expect("lit");
    assert!(near(l.colour[0], 1.0) && near(l.colour[1], 0.0) && near(l.reach, 96.0));
    assert!(light_at(&flash, 300.0, 9, [3.0, 4.0], 2.0).is_none());
    assert!(light_at(&white(0, Reach::Unlimited), 0.0, 1, [0.0; 2], 1.0).is_none());
}

#[test]
fn srgb_to_linear_endpoints() {
    assert!(near(srgb_to_linear(0), 0.0));
    assert!(near(srgb_to_linear(255), 1.0));
    assert!((srgb_to_linear(128) - 0.2158).abs() < 1e-3);
}

#[test]
fn engine_numbers_match_r13() {
    assert_eq!(engine::FLOOR, 50);
    let e = engine::explosion_light(5);
    assert_eq!((e.colour, e.intensity, e.reach), ([255, 200, 140], 100, Reach::Pixels(64)));
    assert_eq!(engine::explosion_light(100).intensity, 250);
    assert_eq!(engine::explosion_light(-3).reach, Reach::Pixels(24));
    let m = engine::muzzle_light();
    assert_eq!((m.colour, m.intensity, m.reach), ([255, 220, 160], 80, Reach::Pixels(40)));
    assert!(m.animation.ended(51.0) && !m.animation.ended(49.0));
}

#[test]
fn solids_settings_are_bounded_and_off_without_a_look() {
    let mut l = level();
    l.light_soak = 200;
    l.solid_floor.intensity = 500;
    let mut f = LookFrame::default();
    f.begin(Some(&l), 0.0);
    assert_eq!(f.light_soak, olr_look::MAX_SOAK);
    assert!(near(f.solid_floor[0], 1.0));
    f.begin(None, 0.0);
    assert_eq!((f.light_soak, f.solid_floor), (0, [0.0; 3]));
}

#[test]
fn worm_floor_hues() {
    let k = olr_look::engine::WORM_FLOOR as f32 / 100.0;
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-3);
    assert!(near(olr_look::worm_floor_of([255, 255, 255]), [k; 3]), "white");
    assert!(near(olr_look::worm_floor_of([0, 0, 0]), [k; 3]), "black: white");
    // #203048 at full brightness is #6ea5ff (by linear channels, the brightest at 1).
    let f = olr_look::worm_floor_of([0x20, 0x30, 0x48]);
    assert!((f[2] - k).abs() < 1e-3 && f[0] < f[1] && f[1] < f[2], "{f:?}");
    // Pure blue's 35 % is too dark: mixed towards white to half of 35 % white's luminance.
    let b = olr_look::worm_floor_of([0, 0, 255]);
    let lum = 0.2126 * b[0] + 0.7152 * b[1] + 0.0722 * b[2];
    assert!((lum - k * 0.5).abs() < 1e-3 && b[2] > b[0], "{b:?} {lum}");
    // A frame without a level look (a mod's look alone): 35 % white.
    let mut frame = LookFrame::default();
    frame.begin(None, 0.0);
    assert!(near(frame.worm_floor, [k; 3]));
    // A level's ambient hue, whatever its intensity.
    let mut l = level();
    l.ambient = olr_look::Ambient { colour: [0x20, 0x30, 0x48], intensity: 10 };
    frame.begin(Some(&l), 0.0);
    assert!(near(frame.worm_floor, f));
}

/// Plan 15c ("My calls"): a disc's radiance makes its light at
/// `d0 = max(reach / 3, 2 r)` in the open match today's point light there.
#[test]
fn disc_calibration_matches_todays_light() {
    let open = |reach: f32| if reach.is_finite() { Reach::Pixels(reach as u16) } else { Reach::Unlimited };
    for (reach, r) in [(40.0f32, 5.0f32), (300.0, 6.0)] {
        let l = engine::disc_radiance([1.0; 3], r, reach)[0];
        let d0 = (reach / 3.0).max(2.0 * r);
        let disc = l * (2.0 * r / (std::f32::consts::TAU * d0)).min(1.0) * fade(d0, open(reach));
        assert!((disc - fade(d0 - r, open(reach))).abs() < 1e-4, "reach {reach}, radius {r}: {disc}");
    }
    for (r, reach) in [(5.0, f32::INFINITY), (0.0, 40.0), (0.0, 0.0), (f32::NAN, f32::NAN), (6.0, 1e30)] {
        let l = engine::disc_radiance([1.0, 0.5, 0.0], r, reach);
        assert!(l.iter().all(|v| v.is_finite() && *v >= 0.0 && *v <= 64.0), "{r} {reach}: {l:?}");
    }
    assert_eq!(engine::disc_radius(40.0), 5.0);
    assert_eq!(engine::disc_radius(1.0), 1.0);
    assert_eq!(engine::disc_radius(4096.0), 6.0);
    assert_eq!(engine::disc_radius(f32::INFINITY), 1.0);
    assert_eq!(engine::disc_radius(f32::NAN), 1.0);
    assert_eq!(engine::explosion_radius(0), 2.0);
    assert_eq!(engine::explosion_radius(10), 7.0);
    assert_eq!(engine::explosion_radius(500), 12.0);
    assert_eq!(engine::explosion_radius(-5), 2.0);
}

#[test]
fn emitters_cull_like_lights() {
    let mut f = LookFrame::default();
    f.begin(None, 0.0);
    for i in 0..(MAX_EMITTERS + 300) {
        let off = i % 30 == 7;
        f.push_emitter(if off {
            at(-5000.0, 0.0, 10.0)
        } else {
            at(i as f32 % 400.0, 50.0, 20.0 + i as f32)
        });
    }
    f.push_emitter(at(-500.0, 100.0, 600.0));
    f.cull([0.0, 0.0, 427.0, 192.0]);
    assert_eq!(f.emitters.len(), MAX_EMITTERS);
    assert!(f.emitters.iter().all(|l| l.pos[0] >= 0.0));
    assert!(f.emitters.windows(2).all(|w| w[0].reach < w[1].reach), "order kept");
    f.begin(None, 0.0);
    assert!(f.emitters.is_empty(), "begin clears them");
}
