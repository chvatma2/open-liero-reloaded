//! Heat (issue #39): the heat texture, its step, its look and its light.

use olr_look::{Heat, LookFrame, RampKey, SurfaceMaps, SurfaceMaterial, ViewMode};
use olr_render::{
    BACK_HEAT, HEAT_SCALE, HEAT_STEP_MS, HeatDeposit, HeatView, MAX_HEAT_DEPOSITS, MAX_HEAT_STEPS, Renderer,
};

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

/// Issue #40: `run_heat` runs what the next frame would, so a host that
/// deposits past [`MAX_HEAT_DEPOSITS`] or falls more than
/// [`MAX_HEAT_STEPS`] behind between frames, running it whenever the
/// deposits fill up or the steps pile up, draws the heat of a frame every
/// tick.
#[test]
fn running_heat_between_frames_equals_a_frame_a_tick() {
    let Some(gpu) = offscreen() else { return };
    let mut single = heated(&gpu);
    let mut batched = heated(&gpu);
    let ticks = 2 * MAX_HEAT_STEPS * HEAT_STEP_MS / 10 + 40;
    for tick in 0..ticks {
        for k in 0..3 {
            // A blast every tick of the first 60, three a tick: 180 in all.
            if tick < 60 {
                let d = blast(4.0 + (tick % 30) as f32, 40.0 + (k * 10) as f32, 3.0);
                if batched.heat_waiting() == MAX_HEAT_DEPOSITS {
                    batched.run_heat();
                }
                single.deposit_heat(d);
                batched.deposit_heat(d);
            }
        }
        single.advance_heat(10);
        batched.advance_heat(10);
        frame(&gpu, &mut single, &dark());
        while batched.heat_behind() > u64::from(MAX_HEAT_STEPS) {
            batched.run_heat();
        }
    }
    assert!(single.heat_steps() > u64::from(MAX_HEAT_STEPS), "past a frame's steps");
    frame(&gpu, &mut batched, &dark());
    assert_eq!((single.heat_steps(), single.heat_behind()), (batched.heat_steps(), 0));
    assert!(temps(&gpu, &single) == temps(&gpu, &batched), "the same heat");
    assert!(temps(&gpu, &single).iter().any(|&t| t > 1.0), "still warm");
    // Without heat it does nothing.
    let mut plain = lit(&gpu);
    plain.upload_terrain(&map());
    plain.deposit_heat(blast(20.0, 70.0, 10.0));
    plain.run_heat();
    assert_eq!((plain.heat_waiting(), plain.heat_behind(), plain.heat_passes()), (0, 0, 0));
}

/// Issue #40: a back pixel (no rock or dirt: [`map`] below row 80, its
/// back stone left of 40) takes [`BACK_HEAT`] of a deposit, a front pixel
/// all of it.
#[test]
fn the_back_takes_a_share_of_a_deposit() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated(&gpu);
    r.deposit_heat(HeatDeposit { pos: [20.5, 40.5], radius: 4.0, amount: 800.0 });
    r.deposit_heat(HeatDeposit { pos: [20.5, 85.5], radius: 4.0, amount: 800.0 });
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert_eq!((at(&t, 20, 40), at(&t, 20, 85)), (800.0, 800.0 * BACK_HEAT));
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

/// Issue #41: a texture's temperatures (degrees) and its width.
fn layer_temps(gpu: &Offscreen, t: &wgpu::Texture) -> (usize, Vec<f32>) {
    let bytes = gpu.read_bytes(t);
    let t = bytes.as_chunks::<2>().0.iter().map(|b| f32::from(u16::from_le_bytes(*b)) / HEAT_SCALE);
    (t.len() / 90, t.collect())
}

/// Issue #41: [`see_through`]'s open level, lit, with `materials`, the
/// level's back all `back` (none for 0), and white distant layers of the
/// level's size at `depths`, each of `values`' material.
fn layered(
    gpu: &Offscreen,
    materials: &[SurfaceMaterial],
    back: u8,
    depths: &[u8],
    values: &[u8],
) -> Renderer {
    let mut r = lit(gpu);
    let mut m = SurfaceMaps::empty((120, 90));
    m.back.iter_mut().for_each(|v| *v = back);
    r.set_surface(materials, Some(&m));
    r.set_distant_surface(values);
    r.upload_terrain(&see_through(120, 90, 2, |_, _| false));
    let white = Rgba::rgb(255, 255, 255);
    let layers: Vec<_> = depths.iter().map(|&d| layer(240, 180, white, (0, 0), d)).collect();
    r.set_distant(&layers);
    r
}

/// The texels of a row of `t` (`w` wide) above 0.
fn warm_in_row(w: usize, t: &[f32], y: usize) -> usize {
    t[y * w..(y + 1) * w].iter().filter(|&&v| v > 0.0).count()
}

fn hottest(t: &[f32]) -> (usize, f32) {
    t.iter().copied().enumerate().fold((0, 0.0), |b, (i, v)| if v > b.1 { (i, v) } else { b })
}

/// Issue #41: a deposit is a ball on the level's plane; a near distant
/// layer takes its slice, a smaller and cooler circle than the plane's,
/// a farther one smaller still, and the layers cool to nothing.
#[test]
fn a_near_layer_takes_a_smaller_circle_than_the_plane() {
    let Some(gpu) = offscreen() else { return };
    let stone = [material("stone", Some(heat(100, 100, 100)))];
    let mut r = layered(&gpu, &stone, 1, &[10, 15], &[1, 1]);
    let (l, _) = one_to_one();
    r.deposit_heat_seen(HeatDeposit { pos: [60.5, 45.5], radius: 20.0, amount: 1000.0 }, &[HeatView::of(&l)]);
    frame(&gpu, &mut r, &dark());
    let plane = temps(&gpu, &r);
    let (w, near) = layer_temps(&gpu, r.distant_heat_texture(0).unwrap());
    let (_, far) = layer_temps(&gpu, r.distant_heat_texture(1).unwrap());
    assert_eq!(w, 120, "the layer's world size");
    // The back's share of the plane's 1000 at its centre; z = 11 and 16.5.
    let ((_, p), (ni, n), (_, f)) = (hottest(&plane), hottest(&near), hottest(&far));
    assert!((p - 1000.0 * BACK_HEAT).abs() < 2.0, "the plane: {p}");
    assert!((n - 1000.0 * BACK_HEAT * (1.0 - 121.0 / 400.0)).abs() < 2.0, "the near layer: {n}");
    assert!((f - 1000.0 * BACK_HEAT * (1.0 - 272.25 / 400.0)).abs() < 2.0, "the far layer: {f}");
    // Placed behind the deposit: the camera (60, 60) moved the layer 1.5 down.
    assert_eq!((ni % w, ni / w), (60, 43), "the near spot's centre");
    let across = |t: &[f32], y| warm_in_row(120, t, y);
    let (cp, cn, cf) = (across(&plane, 45), across(&near, 43), across(&far, 43));
    assert!((38..=40).contains(&cp), "the plane's circle: {cp}");
    assert!((32..=34).contains(&cn), "radius sqrt(279) at depth 10: {cn}");
    assert!((21..=23).contains(&cf), "radius sqrt(127.75) at depth 15: {cf}");
    // The layers cool to nothing, by their material.
    steps(&gpu, &mut r, 50);
    let (_, cooler) = layer_temps(&gpu, r.distant_heat_texture(0).unwrap());
    assert!(hottest(&cooler).1 < n * 0.8, "it cools: {}", hottest(&cooler).1);
    let mut seconds = 0;
    while r.heat_active() {
        r.advance_heat(1000);
        frame(&gpu, &mut r, &dark());
        seconds += 1;
        assert!(seconds <= 120, "still hot after {seconds} s");
    }
    for i in 0..2 {
        let (_, t) = layer_temps(&gpu, r.distant_heat_texture(i).unwrap());
        assert!(t.iter().all(|&v| v == 0.0), "layer {i} cold");
    }
}

/// Issue #41: only a layer the largest heat radius reaches at its depth
/// and whose material heats gets a heat texture; a level whose layers
/// get none draws and costs as without materials, and one whose layers
/// have heat draws the same bytes while nothing is hot.
#[test]
fn a_layer_beyond_the_largest_radius_has_no_texture() {
    let Some(gpu) = offscreen() else { return };
    let m = [material("stone", Some(heat(100, 100, 100))), material("cold", None)];
    // z = 63.8 is within MAX_HEAT_RADIUS (64), 64.9 not; then a cold
    // material, none and a value past the list.
    let r = layered(&gpu, &m, 0, &[58, 59, 10, 10, 10], &[1, 1, 2, 0, 9]);
    assert!(r.distant_heat_texture(0).is_some(), "depth 58 is within reach");
    assert!((1..5).all(|i| r.distant_heat_texture(i).is_none()));
    assert!(r.has_heat() && r.heat_texture().is_none(), "the layer's heat alone");
    let mut far = layered(&gpu, &m, 0, &[59, 10], &[1, 2]);
    assert!(!far.has_heat());
    let mut plain = layered(&gpu, &[], 0, &[59, 10], &[]);
    let (l, size) = one_to_one();
    let want = gpu.render_lit_to_image(&mut plain, &l, Some(&dark()), size);
    far.deposit_heat_seen(blast(60.0, 45.0, 64.0), &[HeatView::of(&l)]);
    far.advance_heat(100);
    assert_eq!(gpu.render_lit_to_image(&mut far, &l, Some(&dark()), size), want, "far layers unchanged");
    assert_eq!(far.heat_passes(), 0);
    // A heated layer with nothing hot: the same bytes.
    let mut idle = layered(&gpu, &m, 0, &[59, 10], &[1, 1]);
    assert!(idle.distant_heat_texture(1).is_some());
    idle.advance_heat(100);
    assert_eq!(gpu.render_lit_to_image(&mut idle, &l, Some(&dark()), size), want, "idle");
    assert!(!idle.heat_active());
    // Unlit, no layer has heat.
    let mut unlit = layered(&gpu, &m, 0, &[10], &[1]);
    unlit.set_lighting(false);
    assert!(unlit.distant_heat_texture(0).is_none() && !unlit.has_heat());
}

/// A material that takes heat easily (capacity 25: four times stone's
/// gain), so a slice shows.
fn hot_stone() -> SurfaceMaterial {
    material("hot", Some(Heat { capacity: 25, ..heat(100, 100, 100) }))
}

/// The output pixels where `a` and `b` differ.
fn changed(a: &RgbaImage, b: &RgbaImage) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for y in 0..a.height() {
        for x in 0..a.width() {
            if a.get(x, y) != b.get(x, y) {
                out.push((x as i32, y as i32));
            }
        }
    }
    out
}

/// Issue #41: the heat stays on the layer pixel it landed on: as the
/// camera pans, the hot spot moves with the layer (by `1 - depth` of the
/// camera's movement), not with the level.
#[test]
fn the_spot_moves_with_the_layer_as_the_camera_pans() {
    let Some(gpu) = offscreen() else { return };
    let mut r = layered(&gpu, &[hot_stone()], 0, &[10], &[1]);
    let (a, size) = one_to_one();
    let b = list(View { x: 30.0, y: 30.0, w: 80.0, h: 60.0 });
    let cold = (frame(&gpu, &mut r, &dark()), gpu.render_lit_to_image(&mut r, &b, Some(&dark()), size));
    r.deposit_heat_seen(blast(70.5, 50.5, 14.0), &[HeatView::of(&a)]);
    let hot_a = gpu.render_lit_to_image(&mut r, &a, Some(&dark()), size);
    let hot_b = gpu.render_lit_to_image(&mut r, &b, Some(&dark()), size);
    let (sa, sb) = (changed(&cold.0, &hot_a), changed(&cold.1, &hot_b));
    assert!(sa.len() > 100, "the layer shows hot: {} pixels", sa.len());
    // World pixel (70, 50) is output (100, 40) in view a.
    assert!(sa.contains(&(100, 40)) && sa.contains(&(101, 41)));
    // The camera 10 right moves the layer 1 world pixel with it: on
    // screen 2 × (10 - 1) = 18 texels left, the spot with it.
    let moved: Vec<_> = sa.iter().map(|&(x, y)| (x - 18, y)).filter(|&(x, _)| x >= 0).collect();
    assert_eq!(sb, moved);
}

/// Issue #41: with two views (split screen) the view nearest the deposit
/// places its slices: its camera's layer position, whatever the views'
/// order; a lone far view places them by its own.
#[test]
fn with_two_views_the_nearest_views_camera_places_it() {
    let Some(gpu) = offscreen() else { return };
    let rect = |x: f32| View { x, y: 30.0, w: 30.0, h: 30.0 };
    // Cameras 40 left and 20 right of the level's centre (60, 45): the
    // layer (depth 10) 4 left and 2 right.
    let far = HeatView { view: rect(0.0), camera: (20.0, 45.0) };
    let near = HeatView { view: rect(60.0), camera: (80.0, 45.0) };
    let spot = |views: &[HeatView]| {
        let mut r = layered(&gpu, &[hot_stone()], 0, &[10], &[1]);
        r.deposit_heat_seen(blast(70.5, 50.5, 14.0), views);
        frame(&gpu, &mut r, &dark());
        let (w, t) = layer_temps(&gpu, r.distant_heat_texture(0).unwrap());
        let (i, _) = hottest(&t);
        (i % w, i / w)
    };
    assert_eq!(spot(&[far, near]), (68, 50), "70.5 - 2");
    assert_eq!(spot(&[near, far]), (68, 50));
    assert_eq!(spot(&[far]), (74, 50), "70.5 + 4");
    // Inside both, the first.
    let both = HeatView { view: View { x: 0.0, y: 0.0, w: 120.0, h: 90.0 }, ..far };
    assert_eq!(spot(&[both, near]), (74, 50));
    let mut none = layered(&gpu, &[hot_stone()], 0, &[10], &[1]);
    none.deposit_heat_seen(blast(70.5, 50.5, 14.0), &[]);
    assert!(!none.heat_active(), "no view, no slice");
}

/// Issue #41: a hot layer shows its ramp colour and glow but casts no
/// light: the light grid is the same as with it cold.
#[test]
fn a_hot_layer_adds_no_glow_light() {
    let Some(gpu) = offscreen() else { return };
    let mut r = lit(&gpu);
    r.set_surface(&[hot_stone()], None);
    r.set_distant_surface(&[1]);
    r.upload_terrain(&see_through(120, 90, 2, |_, _| false));
    r.set_distant(&[layer(240, 180, Rgba::rgb(90, 90, 90), (0, 0), 5)]);
    assert!(r.distant_heat_texture(0).is_some() && r.heat_texture().is_none());
    let (l, _) = one_to_one();
    let cold = frame(&gpu, &mut r, &dark());
    let (_, before) = gpu.read_light(&r).unwrap();
    r.deposit_heat_seen(blast(60.5, 50.5, 20.0), &[HeatView::of(&l)]);
    let hot = frame(&gpu, &mut r, &dark());
    let (_, after) = gpu.read_light(&r).unwrap();
    let c = hot.get(2 * (60 - 20), 2 * (50 - 30));
    assert!(c.r > 200 && c.r > c.b + 60, "the layer glows: {c:?}");
    assert_ne!(cold, hot);
    assert!(before == after, "but lights nothing");
}
