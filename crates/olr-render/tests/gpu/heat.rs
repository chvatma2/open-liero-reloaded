//! Heat (issue #39): the heat texture, its step, its look and its light.

use olr_look::{Heat, LookFrame, RampKey, SurfaceMaps, SurfaceMaterial, ViewMode};
use olr_render::{HEAT_SCALE, HEAT_STEP_MS, HeatDeposit, MAX_HEAT_DEPOSITS, MAX_HEAT_STEPS, Renderer};

use super::*;

fn heat(sensitivity: u8, conductivity: u16, light: u8) -> Heat {
    Heat { sensitivity, capacity: 100, conductivity, cooling: 100, ramp: vec![], light }
}

fn material(id: &str, heat: Option<Heat>) -> SurfaceMaterial {
    SurfaceMaterial { id: id.into(), heat }
}

/// 1 stone, 2 an insulator (it heats but conducts nothing), 3 numb
/// (sensitivity 0), 4 cold (no heat).
fn materials() -> Vec<SurfaceMaterial> {
    vec![
        material("stone", Some(heat(100, 100, 100))),
        material("insulator", Some(heat(100, 0, 100))),
        material("numb", Some(heat(0, 100, 100))),
        material("cold", None),
    ]
}

/// [`map`]'s surface maps: the front by column, stone left of 40, the
/// insulator to 60, numb to 80, cold to 100, none past; the back stone
/// left of 40, none elsewhere.
fn maps() -> SurfaceMaps {
    let mut m = SurfaceMaps::empty((120, 90));
    for y in 0..90 {
        for x in 0..120 {
            let i = y * 120 + x;
            m.front[i] = match x {
                0..40 => 1,
                40..60 => 2,
                60..80 => 3,
                80..100 => 4,
                _ => 0,
            };
            m.back[i] = u8::from(x < 40);
        }
    }
    m
}

/// A dark look: ambient 0.02, nothing glowing.
fn dark() -> LookFrame {
    let mut look = LookFrame::default();
    look.begin(None, 0.0);
    look.ambient = [0.02; 3];
    look
}

fn lit(gpu: &Offscreen) -> Renderer {
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.set_lighting(true);
    r.set_bloom(false);
    r
}

/// [`map`] lit, with [`materials`] on [`maps`].
fn heated(gpu: &Offscreen) -> Renderer {
    let mut r = lit(gpu);
    r.set_surface(&materials(), Some(&maps()));
    r.upload_terrain(&map());
    assert!(r.has_heat());
    r
}

/// Each world pixel's temperature, degrees.
fn temps(gpu: &Offscreen, r: &Renderer) -> Vec<f32> {
    gpu.read_bytes(r.heat_texture().unwrap())
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(u16::from_le_bytes(*b)) / HEAT_SCALE)
        .collect()
}

fn at(t: &[f32], x: usize, y: usize) -> f32 {
    t[y * 120 + x]
}

fn frame(gpu: &Offscreen, r: &mut Renderer, look: &LookFrame) -> RgbaImage {
    let (l, size) = one_to_one();
    gpu.render_lit_to_image(r, &l, Some(look), size)
}

fn blast(x: f32, y: f32, radius: f32) -> HeatDeposit {
    HeatDeposit { pos: [x, y], radius, amount: 1000.0 }
}

/// Steps `n` heat steps, a frame each.
fn steps(gpu: &Offscreen, r: &mut Renderer, n: u32) {
    for _ in 0..n {
        r.advance_heat(HEAT_STEP_MS);
        frame(gpu, r, &dark());
    }
}

/// A level without materials, or whose materials never heat, draws and
/// costs exactly as before: no heat texture, the same bytes; one with heat
/// draws the same bytes while nothing is hot, running no pass.
#[test]
fn no_materials_draws_todays_bytes() {
    let Some(gpu) = offscreen() else { return };
    let look = dark();
    let mut plain = lit(&gpu);
    plain.upload_terrain(&map());
    let want = frame(&gpu, &mut plain, &look);
    let cold = [material("cold", None), material("numb", Some(heat(0, 100, 100)))];
    let mut none = SurfaceMaps::empty((120, 90));
    let cases: [(&[SurfaceMaterial], Option<SurfaceMaps>); 4] =
        [(&[], None), (&materials(), None), (&materials(), Some(none.clone())), (&cold, Some(maps()))];
    for (i, (m, s)) in cases.into_iter().enumerate() {
        let mut r = lit(&gpu);
        r.set_surface(m, s.as_ref());
        r.upload_terrain(&map());
        assert!(!r.has_heat(), "case {i}: no heat texture");
        r.deposit_heat(blast(20.0, 70.0, 10.0));
        r.advance_heat(1000);
        assert_eq!(frame(&gpu, &mut r, &look), want, "case {i}");
    }
    // A map of the wrong size makes none either.
    none.world = (60, 90);
    let mut r = lit(&gpu);
    r.set_surface(&materials(), Some(&none));
    r.upload_terrain(&map());
    assert!(!r.has_heat());
    // Heat with nothing hot: the same bytes, no pass.
    let mut idle = heated(&gpu);
    for _ in 0..3 {
        idle.advance_heat(100);
        assert_eq!(frame(&gpu, &mut idle, &look), want);
    }
    assert_eq!(idle.heat_passes(), 0);
    // Set after the terrain is up, it takes effect at once.
    let mut late = lit(&gpu);
    late.upload_terrain(&map());
    late.set_surface(&materials(), Some(&maps()));
    assert!(late.has_heat());
    late.set_surface(&[], None);
    assert!(!late.has_heat());
}

/// A deposit heats only pixels whose material heats, by the deposit's
/// falloff, and heat spreads only into conducting materials.
#[test]
fn a_deposit_heats_only_material_pixels_and_spreads_only_into_conducting_ones() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated(&gpu);
    // Across stone (to 40), the insulator (to 60) and numb (to 80).
    r.deposit_heat(HeatDeposit { pos: [50.0, 40.0], radius: 25.0, amount: 800.0 });
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(at(&t, 30, 40) > 100.0, "stone heats: {}", at(&t, 30, 40));
    assert!(at(&t, 50, 40) > 700.0, "the centre: {}", at(&t, 50, 40));
    assert!(at(&t, 50, 40) <= 800.0);
    assert!(at(&t, 45, 40) < at(&t, 50, 40), "it falls off");
    assert_eq!(at(&t, 65, 40), 0.0, "numb never heats");
    assert_eq!(at(&t, 50, 10), 0.0, "past the radius");
    // Cold and no material hold none.
    r.deposit_heat(blast(100.0, 40.0, 15.0));
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!((86..115).all(|x| at(&t, x, 40) == 0.0), "cold and none hold no heat");

    // Spreading: in stone it goes past its circle; in the insulator not;
    // from stone it never enters the insulator.
    let mut r = heated(&gpu);
    r.deposit_heat(blast(15.0, 40.0, 3.0));
    r.deposit_heat(blast(50.0, 40.0, 3.0));
    r.deposit_heat(blast(37.0, 20.0, 2.5));
    frame(&gpu, &mut r, &dark());
    let before = temps(&gpu, &r);
    assert_eq!(at(&before, 22, 40), 0.0);
    steps(&gpu, &mut r, 60);
    let t = temps(&gpu, &r);
    assert!(at(&t, 22, 40) > 0.0, "stone conducts: {}", at(&t, 22, 40));
    assert!(at(&t, 15, 47) > 0.0);
    assert_eq!(at(&t, 57, 40), 0.0, "the insulator conducts nothing");
    assert!(at(&t, 50, 40) > 0.0, "and keeps its own heat");
    assert!(at(&t, 38, 20) > 0.0);
    assert!((40..46).all(|x| at(&t, x, 20) == 0.0), "stone's heat stops at the insulator");
    assert_eq!(r.heat_steps(), 60);
}

/// A material of sensitivity 0 never heats: alone it makes no heat
/// texture; beside others its pixels stay cold.
#[test]
fn sensitivity_zero_never_heats() {
    let Some(gpu) = offscreen() else { return };
    let numb = [material("numb", Some(heat(0, 100, 100)))];
    let mut m = SurfaceMaps::empty((120, 90));
    m.front.iter_mut().for_each(|v| *v = 1);
    let mut r = lit(&gpu);
    r.set_surface(&numb, Some(&m));
    r.upload_terrain(&map());
    assert!(!r.has_heat());
    let mut r = heated(&gpu);
    r.deposit_heat(blast(70.0, 40.0, 9.0));
    steps(&gpu, &mut r, 5);
    assert!(temps(&gpu, &r).iter().all(|&v| v == 0.0));
}

/// Heat cools to nothing (the rim beside the air first), the boxes go
/// and the passes stop.
#[test]
fn heat_cools_to_nothing() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated(&gpu);
    r.deposit_heat(blast(20.0, 76.0, 8.0));
    frame(&gpu, &mut r, &dark());
    let start = temps(&gpu, &r);
    assert!(at(&start, 20, 79) > 800.0, "the rim: {}", at(&start, 20, 79));
    steps(&gpu, &mut r, 100);
    let t = temps(&gpu, &r);
    assert!(at(&t, 20, 79) < at(&start, 20, 79) * 0.6, "it cools: {}", at(&t, 20, 79));
    // (20, 72) started as hot, as far from the centre inside.
    assert!(at(&t, 20, 79) < at(&t, 20, 72), "the rim beside the air faster than inside");
    let mut seconds = 0;
    while r.heat_active() {
        r.advance_heat(1000);
        frame(&gpu, &mut r, &dark());
        seconds += 1;
        assert!(seconds <= 120, "still hot after {seconds} s");
    }
    assert!(temps(&gpu, &r).iter().all(|&v| v == 0.0), "nothing left");
    let passes = r.heat_passes();
    steps(&gpu, &mut r, 10);
    assert_eq!(r.heat_passes(), passes, "no pass once cold");
}

/// The step runs on the match time: N ticks at once (an undrawn stretch,
/// a deposit in it) equal N single ticks, each drawn.
#[test]
fn stepping_n_ticks_at_once_equals_n_single_steps() {
    let Some(gpu) = offscreen() else { return };
    let mut single = heated(&gpu);
    let mut once = heated(&gpu);
    for r in [&mut single, &mut once] {
        r.deposit_heat(blast(20.0, 76.0, 8.0));
    }
    for tick in 0..30 {
        if tick == 12 {
            single.deposit_heat(blast(30.0, 60.0, 5.0));
            once.deposit_heat(blast(30.0, 60.0, 5.0));
        }
        single.advance_heat(10);
        once.advance_heat(10);
        frame(&gpu, &mut single, &dark());
    }
    frame(&gpu, &mut once, &dark());
    assert_eq!(single.heat_steps(), 15);
    assert_eq!(once.heat_steps(), 15);
    assert!(temps(&gpu, &single) == temps(&gpu, &once), "the same heat");
    assert_eq!(frame(&gpu, &mut single, &dark()), frame(&gpu, &mut once, &dark()));
    // Paused, nothing changes.
    let before = temps(&gpu, &once);
    for _ in 0..3 {
        once.advance_heat(0);
        frame(&gpu, &mut once, &dark());
    }
    assert!(temps(&gpu, &once) == before);
}

/// With nothing hot no heat pass runs; a deposit runs them.
#[test]
fn nothing_hot_runs_no_pass() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated(&gpu);
    steps(&gpu, &mut r, 5);
    assert_eq!((r.heat_passes(), r.heat_steps()), (0, 0));
    assert!(!r.heat_active());
    r.deposit_heat(blast(20.0, 60.0, 5.0));
    assert!(r.heat_active());
    steps(&gpu, &mut r, 2);
    assert_eq!(r.heat_steps(), 2);
    assert_eq!(r.heat_passes(), 3, "the deposit and two steps");
}

/// A hostile table (the extremes, values past them, a ramp out of order)
/// and hostile deposits (not numbers, huge, many) stay bounded: no
/// temperature past the hottest, a finite light, at most the frame's
/// passes.
#[test]
fn hostile_values_stay_bounded() {
    let Some(gpu) = offscreen() else { return };
    let key = |temperature, glow| RampKey { temperature, colour: [255, 255, 255], glow };
    let wild = |capacity, conductivity, cooling| Heat {
        sensitivity: 255,
        capacity,
        conductivity,
        cooling,
        ramp: vec![key(900, 255), key(5, 100), key(5, 50), key(60000, 200)],
        light: 255,
    };
    let hostile = [
        material("a", Some(wild(0, u16::MAX, 0))),
        material("b", Some(wild(10, 1000, 1))),
        material("c", Some(wild(u16::MAX, 0, u16::MAX))),
    ];
    let mut m = SurfaceMaps::empty((120, 90));
    for (i, v) in m.front.iter_mut().enumerate() {
        *v = (i % 120 / 40) as u8 + 1;
    }
    m.back.iter_mut().for_each(|v| *v = 255);
    let mut r = lit(&gpu);
    r.set_surface(&hostile, Some(&m));
    r.upload_terrain(&map());
    let mut look = dark();
    look.max_light = 1e4;
    for f in 0..12u32 {
        let passes = r.heat_passes();
        for i in 0..500u32 {
            let p = [(i * 7 % 140) as f32 - 10.0, (i * 13 % 100) as f32 - 5.0];
            r.deposit_heat(HeatDeposit { pos: p, radius: 1e9, amount: f32::INFINITY });
            r.deposit_heat(HeatDeposit { pos: [f32::NAN, 1.0], radius: 3.0, amount: 1.0 });
            r.deposit_heat(HeatDeposit { pos: p, radius: f32::NAN, amount: -5.0 });
        }
        r.advance_heat(if f == 5 { u32::MAX } else { 1000 });
        frame(&gpu, &mut r, &look);
        let ran = r.heat_passes() - passes;
        assert!(ran <= (MAX_HEAT_DEPOSITS as u32 + MAX_HEAT_STEPS + 16) as u64, "frame {f}: {ran} passes");
        assert!(temps(&gpu, &r).iter().all(|&v| v <= 1000.0));
        let (_, light) = gpu.read_light(&r).unwrap();
        assert!(light.iter().flatten().all(|v| v.is_finite()), "frame {f}: the light stays finite");
    }
}

/// Split screen: the heat steps once a frame for the whole level, as
/// with one view.
#[test]
fn split_screen_steps_once_per_frame() {
    let Some(gpu) = offscreen() else { return };
    let view = |x: f32, y: f32| list(View { x, y, w: 60.0, h: 40.0 });
    let lists = [view(0.0, 40.0), view(40.0, 0.0), view(20.0, 45.0), view(60.0, 50.0)];
    let areas = [[0, 0, 120, 80], [120, 0, 120, 80], [0, 80, 120, 80], [120, 80, 120, 80]];
    let views: Vec<_> = (0..4).map(|i| olr_render::ViewDraw { list: &lists[i], area: areas[i] }).collect();
    let (mut four, mut one) = (heated(&gpu), heated(&gpu));
    for f in 1..=6 {
        for r in [&mut four, &mut one] {
            if f == 1 {
                r.deposit_heat(blast(20.0, 76.0, 8.0));
            }
            r.advance_heat(2 * HEAT_STEP_MS);
        }
        gpu.render_views_to_image(&mut four, &views, Some(&dark()), (240, 160));
        gpu.render_views_to_image(&mut one, &views[..1], Some(&dark()), (240, 160));
        assert_eq!((four.heat_steps(), one.heat_steps()), (2 * f, 2 * f));
    }
    assert!(temps(&gpu, &four) == temps(&gpu, &one));
}

/// A hot pixel lights the dark air by its material's light; one whose
/// light is 0 shows hot but lights nothing. (In the insulator's columns,
/// whose air has no back wall to heat.)
#[test]
fn a_hot_pixel_adds_glow_light() {
    let Some(gpu) = offscreen() else { return };
    let light_below = |light: u8| {
        let mut r = lit(&gpu);
        let mut m = materials();
        m[1] = material("insulator", Some(heat(100, 0, light)));
        r.set_surface(&m, Some(&maps()));
        r.upload_terrain(&map());
        let cold = frame(&gpu, &mut r, &dark());
        let (grid, before) = gpu.read_light(&r).unwrap();
        r.deposit_heat(blast(50.0, 76.0, 8.0));
        let hot = frame(&gpu, &mut r, &dark());
        let (_, after) = gpu.read_light(&r).unwrap();
        let i = |x: i32, y: i32| ((y - grid.origin.1) * grid.size.0 as i32 + (x - grid.origin.0)) as usize;
        (before[i(50, 84)], after[i(50, 84)], cold != hot)
    };
    let (before, after, shows) = light_below(100);
    assert!(shows);
    assert!(after[0] > before[0] * 3.0 && after[0] > 0.1, "the air below lights: {before:?} → {after:?}");
    assert!(after[0] > after[2], "warm light: {after:?}");
    let (before, after, shows) = light_below(0);
    assert!(shows, "it still shows hot");
    assert_eq!(before, after, "but casts nothing");
}

/// A hot pixel shows its ramp's colour over its art, lit, in the dark,
/// and in the editor's full-bright view.
#[test]
fn a_hot_pixel_shows_its_colour_in_every_view() {
    let Some(gpu) = offscreen() else { return };
    for mode in [ViewMode::Lit, ViewMode::FullBright] {
        let mut look = dark();
        look.mode = mode;
        let mut r = heated(&gpu);
        let cold = frame(&gpu, &mut r, &look);
        r.deposit_heat(blast(30.0, 60.0, 8.0));
        let hot = frame(&gpu, &mut r, &look);
        // World pixel (30, 60) is texel (20, 60) of the 2× frame.
        let (x, y) = ((30 - 20) * 2, (60 - 30) * 2);
        let c = hot.get(x, y);
        assert!(c.r >= 240 && c.g >= 200 && c.b >= 160, "{mode:?}: yellow-white at the centre: {c:?}");
        assert_ne!(cold.get(x, y), c);
        let edge = hot.get((35 - 20) * 2, y);
        assert!(edge.r > edge.b, "{mode:?}: redder at the edge: {edge:?}");
        assert_eq!(hot.get(0, 0), cold.get(0, 0), "{mode:?}: far pixels unchanged");
    }
}

/// A pixel dug out keeps its heat when its back material heats, and loses
/// it where the back holds none.
#[test]
fn a_dug_pixel_is_ruled_by_its_back() {
    let Some(gpu) = offscreen() else { return };
    let mut map = map();
    let mut r = lit(&gpu);
    r.set_surface(&materials(), Some(&maps()));
    r.upload_terrain(&map);
    map.take_dirty().for_each(drop);
    r.deposit_heat(blast(20.0, 40.0, 6.0));
    r.deposit_heat(blast(50.0, 40.0, 10.0));
    frame(&gpu, &mut r, &dark());
    map.apply(ColorEdit::Carve { hole: 2, x: 20, y: 40 });
    map.apply(ColorEdit::Carve { hole: 2, x: 50, y: 40 });
    r.update_terrain(&mut map);
    steps(&gpu, &mut r, 1);
    let t = temps(&gpu, &r);
    assert!(at(&t, 20, 40) > 500.0, "its back is stone: {}", at(&t, 20, 40));
    assert_eq!(at(&t, 50, 40), 0.0, "no back material: no heat");
    assert!(at(&t, 50, 31) > 0.0, "the insulator round the hole keeps its heat");
}

/// A reset, a new level or a restart clears the heat.
#[test]
fn a_reset_or_a_new_level_clears_the_heat() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated(&gpu);
    r.deposit_heat(blast(20.0, 40.0, 6.0));
    frame(&gpu, &mut r, &dark());
    assert!(temps(&gpu, &r).iter().any(|&v| v > 0.0));
    r.reset_heat();
    frame(&gpu, &mut r, &dark());
    assert!(temps(&gpu, &r).iter().all(|&v| v == 0.0), "a reset");
    assert!(!r.heat_active());
    r.deposit_heat(blast(20.0, 40.0, 6.0));
    frame(&gpu, &mut r, &dark());
    r.upload_terrain(&map());
    assert!(temps(&gpu, &r).iter().all(|&v| v == 0.0), "a new level");
    assert!(!r.heat_active());
}
