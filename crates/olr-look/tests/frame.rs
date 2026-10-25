//! `LookFrame` building, fade and culling (spec §Light sources, §Pipeline).

use olr_look::{
    Ambient, Animation, ExplosionHeatLook, ExplosionLight, FrameLight, GlowColour, GlowGroup, Grade,
    LevelLook, Light, LookFrame, MAX_EMITTERS, ModLook, ProjectileHeat, ProjectileLook, Reach, ViewMode,
    engine, fade, light_at, srgb_to_linear,
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
        occlusion: None,
        max_light: 150,
        light_steps: 4,
        softness: 2,
        light_soak: 6,
        solid_floor: Ambient { colour: [255, 255, 255], intensity: 40 },
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
        materials: Vec::new(),
    }
}

#[test]
fn a_filtered_group_casts_its_tint_by_the_art() {
    // The background glass design: the group's colour × intensity, the
    // pixel's art (normalised by the renderer) as an own colour.
    let mut look = level();
    look.groups[1].colour = GlowColour::Filtered([200, 212, 240]);
    look.groups[1].intensity = 220;
    let mut f = LookFrame::default();
    f.begin(Some(&look), 0.0);
    let g = f.groups[1];
    assert!(g.own_colour && g.filtered);
    for (c, srgb) in g.cast.iter().zip([200u8, 212, 240]) {
        assert!(near(*c, olr_look::srgb_to_linear(srgb) * 2.2), "{:?}", g.cast);
    }
    assert!(!f.groups[0].filtered);
}

#[test]
fn begin_fills_level_part() {
    let mut f = LookFrame::default();
    f.begin(Some(&level()), 0.0);
    assert!(near(f.ambient[0], 0.3) && near(f.ambient[2], 0.3));
    assert!(near(f.max_light, 1.5));
    assert_eq!((f.light_steps, f.softness, f.light_soak), (4, 2, 6));
    assert!(f.solid_floor.iter().all(|&c| near(c, 0.4)));
    assert_eq!(f.grade.exposure, 50);
    assert_eq!(f.mode, ViewMode::Lit);
    assert_eq!(f.groups.len(), 2);
    let lava = f.groups[0];
    assert!(!lava.own_colour && near(lava.cast[0], 0.5) && near(lava.cast[1], 0.0));
    assert!(near(lava.self_scale, 1.0) && near(lava.reach, 64.0));
    let sky = f.groups[1];
    assert!(sky.own_colour && near(sky.cast[1], 2.0) && sky.reach.is_infinite());
    // Begin again: cleared, not appended.
    f.push_emitter(at(1.0, 2.0, 3.0));
    f.begin(Some(&level()), 5.0);
    assert_eq!((f.groups.len(), f.emitters.len()), (2, 0));
}

#[test]
fn begin_without_a_level_look_is_full_bright() {
    let mut f = LookFrame::default();
    f.begin(None, 0.0);
    assert!(near(f.ambient[0], 1.0) && near(f.max_light, 1.0));
    assert!(f.groups.is_empty() && f.emitters.is_empty());
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
    FrameLight { pos: [x, y], radius: 2.0, colour: [1.0; 3], reach }
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

/// Issue #40: the engine's explosion heat grows with the damage, reaches
/// past the hole it carves, and stays bounded; nothing for no damage.
#[test]
fn explosion_heat_grows_with_the_damage_and_is_bounded() {
    let h = |d: f32| engine::explosion_heat(d).unwrap();
    // A minigun hit (3) stays under the lava ramp's first key at its rim
    // (6 px out of its 5.5 px hole), even at its centre barely over it.
    let minigun = h(3.0);
    assert_eq!(minigun.radius, 12.5);
    assert!((97.0..98.0).contains(&minigun.amount));
    assert!(minigun.amount * (1.0 - (6.0f32 / minigun.radius).powi(2)) < 100.0);
    // A Bazooka's (4) rim, 8 px out, glows.
    let bazooka = h(4.0);
    assert_eq!(bazooka.radius, 17.0);
    assert!(bazooka.amount * (1.0 - (8.0f32 / bazooka.radius).powi(2)) > 200.0);
    // A Big Nuke's fragments (15): hot and wide.
    assert_eq!((h(15.0).amount, h(15.0).radius), (1000.0, 22.5));
    assert!(h(2.0).amount < minigun.amount && minigun.amount < bazooka.amount);
    for d in [1e9, f32::INFINITY, f32::MAX] {
        assert_eq!((h(d).amount, h(d).radius), (1000.0, engine::MAX_EXPLOSION_HEAT_RADIUS), "{d}");
    }
    for d in [0.0, -5.0, f32::NAN, f32::NEG_INFINITY, 0.5] {
        assert!(engine::explosion_heat(d).is_none(), "{d}");
    }
}

/// Issue #41: a distant layer takes the slice of an explosion's heat ball
/// at its depth: smaller and cooler the further back, none past the ball.
/// Issue #65: on one depth line with the back wall, `z = 14 + 1.1 d`: a
/// Big Nuke's fragments reach depth 7, a Bazooka's depth 2, a mod's
/// widest depth 30.
#[test]
fn distant_layers_take_the_balls_slice() {
    use engine::ExplosionHeat;
    assert_eq!(engine::BACK_DEPTH_PX, 14.0);
    let ball = ExplosionHeat { radius: 20.0, amount: 800.0 };
    // z = 14 + 1.1: R' = sqrt(400 - 15.1²), the amount × that over 400.
    let z = 14.0f32 + 1.1;
    let near = engine::heat_slice(ball, 1).unwrap();
    assert!((near.radius - (400.0 - z * z).sqrt()).abs() < 1e-4, "{near:?}");
    assert!((near.amount - 800.0 * (400.0 - z * z) / 400.0).abs() < 1e-3, "{near:?}");
    let farther = engine::heat_slice(ball, 4).unwrap();
    assert!(farther.radius < near.radius && farther.amount < near.amount);
    assert!(engine::heat_slice(ball, 5).is_some(), "z = 19.5 < R");
    assert!(engine::heat_slice(ball, 6).is_none(), "z = 20.6 ≥ R");
    // Depth 0 (none ships) would be the back wall itself.
    let wall = engine::heat_slice(ball, 0).unwrap();
    assert!((wall.amount - 800.0 * (400.0 - 196.0) / 400.0).abs() < 1e-3, "{wall:?}");
    let h = |d: f32| engine::explosion_heat(d).unwrap();
    assert!(engine::heat_slice(h(15.0), 7).is_some(), "a Big Nuke reaches depth 7");
    assert!(engine::heat_slice(h(15.0), 8).is_none());
    assert!(engine::heat_slice(h(4.0), 2).is_some(), "a Bazooka the nearest layers");
    assert!(engine::heat_slice(h(4.0), 3).is_none());
    assert!(engine::heat_slice(h(3.0), 1).is_none(), "a minigun's hit none");
    let widest = ExplosionHeat { radius: engine::MAX_EXPLOSION_HEAT_RADIUS, amount: 1000.0 };
    assert!(engine::heat_slice(widest, 30).is_some(), "a mod's widest reaches depth 30");
    assert!(engine::heat_slice(widest, 31).is_none());
    for bad in [f32::NAN, 0.0, -1.0, f32::INFINITY] {
        assert!(engine::heat_slice(ExplosionHeat { radius: bad, amount: 1.0 }, 1).is_none());
        assert!(engine::heat_slice(ExplosionHeat { radius: 5.0, amount: bad }, 1).is_none());
    }
}

/// Issue #15: a mod's explosion light flashes as the engine's, capped;
/// a projectile's own wins over the mod's, and -1 (no projectile) has none.
#[test]
fn explosion_lights_are_capped_and_fall_back_to_the_mods() {
    let l = engine::explosion_flash([128, 176, 255], 400, 120);
    assert_eq!((l.colour, l.intensity, l.reach), ([128, 176, 255], 400, Reach::Pixels(120)));
    assert_eq!(l.animation, engine::explosion_light(5).animation, "the engine's flash keys");
    let big = engine::explosion_flash([255; 3], u16::MAX, u16::MAX);
    assert_eq!((big.intensity, big.reach), (1000, Reach::Pixels(engine::MAX_EXPLOSION_REACH)));
    let mut x = ExplosionLight { light: l, radius: 200, fitted: true };
    assert_eq!(x.radius(), f32::from(engine::MAX_EXPLOSION_RADIUS));
    x.radius = 0;
    assert_eq!(x.radius(), 1.0);
    x.light.reach = Reach::Unlimited;
    assert_eq!(x.reach(), f32::from(engine::MAX_EXPLOSION_REACH));

    let own = ExplosionLight { radius: 4, ..x.clone() };
    let mut m = ModLook {
        projectiles: vec![None, Some(ProjectileLook { explosion: Some(own.clone()), ..Default::default() })],
        ..Default::default()
    };
    assert!(m.explosion(0).is_none() && m.explosion(-1).is_none());
    assert_eq!(m.explosion(1), Some(&own));
    m.explosion = Some(x.clone());
    assert!(!ModLook { explosion: Some(x.clone()), ..Default::default() }.is_empty());
    assert_eq!((m.explosion(0), m.explosion(1), m.explosion(7)), (Some(&x), Some(&own), Some(&x)));
    assert!(m.explosion(-1).is_none(), "a bonus or beam keeps the engine's");
}

/// Issue #42: a mod's explosion heat scales the engine's, capped, over its
/// own radius or the engine's; a projectile's own wins over the mod's, and
/// -1 (a bonus blown up) keeps the engine's. A projectile's tick of heat is
/// capped too.
#[test]
fn mod_heat_scales_the_engines_capped_and_falls_back_to_the_mods() {
    let bazooka = engine::explosion_heat(4.0).unwrap();
    let half = ExplosionHeatLook { amount: 50, radius: None }.heat(4.0).unwrap();
    assert_eq!((half.amount, half.radius), (bazooka.amount / 2.0, bazooka.radius));
    let wide = ExplosionHeatLook { amount: 100, radius: Some(30) }.heat(4.0).unwrap();
    assert_eq!((wide.amount, wide.radius), (bazooka.amount, 30.0));
    let hostile = ExplosionHeatLook { amount: u16::MAX, radius: Some(u8::MAX) }.heat(4.0).unwrap();
    assert_eq!((hostile.amount, hostile.radius), (1000.0, engine::MAX_EXPLOSION_HEAT_RADIUS));
    assert_eq!(ExplosionHeatLook { amount: 100, radius: Some(0) }.heat(4.0).unwrap().radius, 1.0);
    for (amount, damage) in [(0, 4.0), (100, 0.0), (100, f32::NAN), (1, 2.0)] {
        assert!(ExplosionHeatLook { amount, radius: Some(9) }.heat(damage).is_none(), "{amount} {damage}");
    }
    let flame = ProjectileHeat { amount: 4, radius: 5 }.tick();
    assert_eq!((flame.amount, flame.radius), (4.0, 5.0));
    let hot = ProjectileHeat { amount: u16::MAX, radius: u8::MAX }.tick();
    assert_eq!(
        (hot.amount, hot.radius),
        (f32::from(engine::MAX_PROJECTILE_HEAT), f32::from(engine::MAX_PROJECTILE_HEAT_RADIUS))
    );
    assert_eq!(ProjectileHeat { amount: 0, radius: 0 }.tick().radius, 1.0);

    let own = ExplosionHeatLook { amount: 200, radius: None };
    let wide_mod = ExplosionHeatLook { amount: 30, radius: Some(12) };
    let heat = ProjectileHeat { amount: 3, radius: 4 };
    let p = ProjectileLook { explosion_heat: Some(own), heat: Some(heat), ..Default::default() };
    let mut m = ModLook { projectiles: vec![None, Some(p)], ..Default::default() };
    assert!(m.explosion_heat(0).is_none() && m.explosion_heat(-1).is_none());
    assert_eq!((m.explosion_heat(1), m.heat(1), m.heat(0), m.heat(9)), (Some(&own), Some(&heat), None, None));
    assert!(m.explosion(1).is_none(), "heat is not light");
    m.explosion_heat = Some(wide_mod);
    assert!(!ModLook { explosion_heat: Some(wide_mod), ..Default::default() }.is_empty());
    let got = (m.explosion_heat(0), m.explosion_heat(1), m.explosion_heat(7));
    assert_eq!(got, (Some(&wide_mod), Some(&own), Some(&wide_mod)));
    assert!(m.explosion_heat(-1).is_none(), "a bonus blown up keeps the engine's");
}

/// Issue #12: a skin's light is steady and capped, whatever it asks.
#[test]
fn skin_light_is_steady_and_capped() {
    const { assert!(engine::SKIN_LIGHT, "skin light is on by default") };
    let l = engine::skin_light([10, 200, 255], 150, 24);
    assert_eq!((l.colour, l.intensity, l.reach), ([10, 200, 255], 150, Reach::Pixels(24)));
    assert_eq!(l.animation, olr_look::Animation::Steady);
    let big = engine::skin_light([255; 3], u16::MAX, u16::MAX);
    assert_eq!(big.intensity, engine::MAX_SKIN_LIGHT_INTENSITY);
    assert_eq!(big.reach, Reach::Pixels(engine::MAX_SKIN_LIGHT_REACH));
    assert_eq!(engine::MAX_SKIN_LIGHT_REACH, 48);
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
    assert_eq!(f.occlusion, None, "a look without occlusion has none");
    f.begin(None, 0.0);
    assert_eq!((f.light_soak, f.solid_floor), (0, [0.0; 3]));
    // Issue #10: the occlusion is clamped, and none without a look or a strength.
    l.occlusion = Some(olr_look::Occlusion { strength: 250, reach: 200 });
    f.begin(Some(&l), 0.0);
    let max = olr_look::MAX_OCCLUSION_REACH;
    assert_eq!(f.occlusion, Some(olr_look::Occlusion { strength: 100, reach: max }));
    l.occlusion = Some(olr_look::Occlusion { strength: 40, reach: 0 });
    f.begin(Some(&l), 0.0);
    let min = olr_look::MIN_OCCLUSION_REACH;
    assert_eq!(f.occlusion, Some(olr_look::Occlusion { strength: 40, reach: min }));
    l.occlusion = Some(olr_look::Occlusion { strength: 0, reach: 12 });
    f.begin(Some(&l), 0.0);
    assert_eq!(f.occlusion, None, "strength 0 is none");
    l.occlusion = Some(olr_look::Occlusion { strength: 60, reach: 12 });
    f.begin(Some(&l), 0.0);
    f.begin(None, 0.0);
    assert_eq!(f.occlusion, None, "none without a look");
}

/// Issue #54: the worm halo takes the full strength right next to a worm,
/// fades as `(1 − x)²` to nothing `r` pixels past it, and is wider and
/// fainter the deeper the background, none from the fade depth on.
#[test]
fn the_worm_halo_fades_out_and_back() {
    let s = engine::WORM_HALO_STRENGTH;
    let r = engine::WORM_HALO_RADIUS;
    assert_eq!(engine::worm_halo(1.0, 0.0), s, "next to the silhouette");
    assert_eq!(engine::worm_halo(0.0, 0.0), s, "inside it");
    assert!(near(engine::worm_halo(2.0, 0.0), s * (1.0 - 1.0 / r).powi(2)));
    assert_eq!(engine::worm_halo(r + 1.0, 0.0), 0.0, "r pixels past it");
    let (r20, s20) = engine::worm_halo_shape(0.2).unwrap();
    let (r50, s50) = engine::worm_halo_shape(0.5).unwrap();
    assert!(r < r20 && r20 < r50 && r50 <= engine::WORM_HALO_RADIUS_MAX, "{r20} {r50}");
    assert!(s > s20 && s20 > s50 && s50 > 0.0, "{s20} {s50}");
    assert!(engine::worm_halo_shape(engine::WORM_HALO_FADE).is_none());
    // A byte's rounding under the fade depth is the fade depth.
    assert!(engine::worm_halo_shape(engine::WORM_HALO_FADE - 0.4 / 255.0).is_none());
    assert!(engine::worm_halo_shape(0.9).is_none(), "a night sky at depth 90");
    for bad in [f32::NAN, f32::INFINITY] {
        assert_eq!(engine::worm_halo(bad, 0.0), 0.0);
        assert_eq!(engine::worm_halo(1.0, bad), 0.0);
    }
    assert_eq!(engine::worm_halo(1.0, -1.0), s, "a depth below 0 is 0");
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
    // A reach no farther than twice the radius still lights (the
    // calibration point stays inside the reach).
    for (r, reach) in [(2.0, 4.0), (1.0, 2.0), (6.0, 8.0)] {
        let l = engine::disc_radiance([1.0; 3], r, reach)[0];
        assert!(l > 0.0 && l <= 64.0, "{r} {reach}: {l}");
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

/// Plan 15c: emitters are kept wherever they are (bounced light may reach
/// the view from past their reach), the first [`MAX_EMITTERS`] in order.
#[test]
fn emitters_past_the_cap_are_dropped() {
    let mut f = LookFrame::default();
    f.begin(None, 0.0);
    for i in 0..(MAX_EMITTERS + 300) {
        f.push_emitter(at(-5000.0 + i as f32, 50.0, 20.0 + i as f32));
    }
    assert_eq!(f.emitters.len(), MAX_EMITTERS);
    assert!(f.emitters.windows(2).all(|w| w[0].reach < w[1].reach), "order kept");
    assert_eq!(f.emitters[0].pos[0], -5000.0, "far off any view, kept");
    f.begin(None, 0.0);
    assert!(f.emitters.is_empty(), "begin clears them");
}

/// Plan 15c: a lit level's light bounces at the engine's strength; a frame
/// without a look has no bounce.
#[test]
fn bounce_is_the_engines_in_a_lit_level() {
    let mut f = LookFrame::default();
    f.begin(Some(&level()), 0.0);
    assert_eq!(f.bounce, engine::BOUNCE);
    f.begin(None, 0.0);
    assert_eq!(f.bounce, 0.0);
}

/// Plan 15c: a `Flicker` group leaves its flicker to the renderer (per
/// region): its cast and self-light are unscaled, its speed, depth and
/// clock are given.
#[test]
fn a_flicker_group_flickers_per_region() {
    let mut look = level();
    look.groups[0].animation = Animation::Flicker { speed: 8, depth: 25 };
    look.groups[0].phase_ms = Some(100);
    let mut f = LookFrame::default();
    for t in [0.0, 37.0, 1234.0] {
        f.begin(Some(&look), t);
        let g = f.groups[0];
        assert_eq!(g.flicker, Some((8.0, 25.0)));
        assert!(near(g.self_scale, 1.0) && near(g.cast[0], 0.5), "{g:?}");
        assert!(near(g.t_ms, t as f32 + 100.0));
        assert_eq!(f.groups[1].flicker, None);
    }
}
