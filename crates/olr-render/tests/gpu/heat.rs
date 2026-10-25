//! Heat (issue #39): the heat texture, its step, its look and its light.

use olr_look::engine::BACK_DEPTH_PX;
use olr_look::{Heat, LookFrame, RampKey, SurfaceMaps, SurfaceMaterial, ViewMode};
use olr_render::{
    DIRT_HEAT_GAIN, DIRT_SKIN, DIRT_SKIN_WEIGHTS, HEAT_SCALE, HEAT_SKIN_GAIN, HEAT_SKIN_WEIGHTS,
    HEAT_STEP_MS, HeatDeposit, HeatView, MAX_HEAT_DEPOSITS, MAX_HEAT_STEPS, PassGroup, Renderer,
};

use super::*;

fn heat(sensitivity: u8, conductivity: u16, light: u8) -> Heat {
    Heat { sensitivity, capacity: 100, conductivity, cooling: 100, ramp: vec![], light }
}

fn material(id: &str, heat: Option<Heat>) -> SurfaceMaterial {
    SurfaceMaterial { id: id.into(), heat, light_soak: None }
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

/// [`map`] lit in rock (issue #65: the tests of rock's rim), with
/// [`materials`] on [`maps`].
fn heated_rock(gpu: &Offscreen) -> Renderer {
    heated_level(gpu, &map_of(Material::Rock))
}

/// Issue #56: a 120 × 90 level like [`map`], `fill` (issue #65: rock for
/// the tests of rock's rim, dirt for dirt's) but for open air where
/// `open(x, y)` (world pixels) and in the last ten rows.
fn level(fill: Material, open: impl Fn(u32, u32) -> bool) -> ColorMap {
    let cells = (0..120 * 90)
        .map(|i| if i / 120 >= 80 || open(i % 120, i / 120) { Material::Background } else { fill })
        .collect();
    level_of_cells(cells)
}

/// A 120 × 90 level like [`map`] of `cells` (row by row).
fn level_of_cells(cells: Vec<Material>) -> ColorMap {
    let t = Terrain::from_grid(&MaterialGrid::from_cells(120, 90, cells).unwrap());
    let mut front = RgbaImage::new(240, 180);
    for y in 0..180 {
        for x in 0..240 {
            front.set(x, y, palette(x / 2, y / 2));
        }
    }
    let theme =
        ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(4, 4, Rgba::rgb(120, 80, 40)) };
    ColorMap::new(&t, &front, &solid(240, 180, Rgba::rgb(0, 0, 40)), 2, theme)
}

/// Issue #56: a rock [`level`] open above row 40, its face.
fn surface() -> ColorMap {
    level(Material::Rock, |_, y| y < 40)
}

/// `map` lit, with [`materials`] on [`maps`].
fn heated_level(gpu: &Offscreen, map: &ColorMap) -> Renderer {
    let mut r = lit(gpu);
    r.set_surface(&materials(), Some(&maps()));
    r.upload_terrain(map);
    assert!(r.has_heat());
    r
}

/// Issue #56: what deposit `d` gives stone at world pixel `(x, y)` by its
/// falloff alone, degrees.
fn falloff(d: HeatDeposit, x: usize, y: usize) -> f32 {
    let (dx, dy) = (x as f32 + 0.5 - d.pos[0], y as f32 + 0.5 - d.pos[1]);
    let q = (dx * dx + dy * dy) / (d.radius * d.radius);
    if q < 1.0 { d.amount * (1.0 - q) } else { 0.0 }
}

/// Issue #65: what deposit `d` gives a back pixel at world pixel `(x, y)`
/// (stone, in sight), degrees: the ball's slice [`BACK_DEPTH_PX`] behind
/// its plane.
fn slice(d: HeatDeposit, x: usize, y: usize) -> f32 {
    let (dx, dy) = (x as f32 + 0.5 - d.pos[0], y as f32 + 0.5 - d.pos[1]);
    let z = BACK_DEPTH_PX;
    d.amount * (1.0 - (dx * dx + dy * dy + z * z) / (d.radius * d.radius)).max(0.0)
}

/// Issue #56: whether `got` is `want` to the texel units' rounding.
fn near(got: f32, want: f32) -> bool {
    (got - want).abs() <= 2.0 / HEAT_SCALE
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
/// falloff, and heat spreads only into conducting materials. (Issue #56:
/// the deposits lie in the air above [`surface`]'s face at row 40, which
/// they heat, one over each material: a walk meets the face's pixels
/// nearer than the target first at a shallow angle; they lay inside the
/// dirt and heated it deep.)
#[test]
fn a_deposit_heats_only_material_pixels_and_spreads_only_into_conducting_ones() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_level(&gpu, &surface());
    // Over stone (to 40), the insulator (to 60) and numb (to 80).
    r.deposit_heat(HeatDeposit { pos: [50.0, 39.0], radius: 25.0, amount: 800.0 });
    r.deposit_heat(HeatDeposit { pos: [30.0, 39.0], radius: 6.0, amount: 800.0 });
    r.deposit_heat(HeatDeposit { pos: [65.0, 39.0], radius: 6.0, amount: 800.0 });
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(at(&t, 30, 40) > 100.0, "stone heats: {}", at(&t, 30, 40));
    assert!(at(&t, 50, 40) > 700.0, "the centre: {}", at(&t, 50, 40));
    assert!(at(&t, 50, 40) <= 800.0);
    assert!(at(&t, 45, 40) < at(&t, 50, 40), "it falls off");
    assert_eq!(at(&t, 65, 40), 0.0, "numb never heats");
    assert_eq!(at(&t, 50, 10), 0.0, "past the radius");
    // Cold and no material hold none.
    r.deposit_heat(blast(100.0, 39.0, 15.0));
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!((86..115).all(|x| at(&t, x, 40) == 0.0), "cold and none hold no heat");

    // Spreading: in stone it goes past its circle; in the insulator not;
    // from stone it never enters the insulator.
    let mut r = heated_level(&gpu, &surface());
    r.deposit_heat(blast(15.0, 39.0, 3.0));
    r.deposit_heat(blast(50.0, 39.0, 3.0));
    r.deposit_heat(blast(37.0, 39.0, 2.5));
    frame(&gpu, &mut r, &dark());
    let before = temps(&gpu, &r);
    assert_eq!(at(&before, 22, 40), 0.0);
    steps(&gpu, &mut r, 60);
    let t = temps(&gpu, &r);
    assert!(at(&t, 22, 40) > 0.0, "stone conducts: {}", at(&t, 22, 40));
    assert!(at(&t, 15, 46) > 0.0);
    assert_eq!(at(&t, 57, 40), 0.0, "the insulator conducts nothing");
    assert!(at(&t, 50, 40) > 0.0, "and keeps its own heat");
    assert!(at(&t, 38, 40) > 0.0);
    assert!((40..46).all(|x| (40..44).all(|y| at(&t, x, y) == 0.0)), "stone's heat stops at the insulator");
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
/// and the passes stop. (Issue #56: the blast lies in the air below the
/// rock's face, where it lay inside the dirt and heated it deep.)
#[test]
fn heat_cools_to_nothing() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_rock(&gpu);
    r.deposit_heat(blast(20.0, 82.0, 8.0));
    frame(&gpu, &mut r, &dark());
    let start = temps(&gpu, &r);
    assert!(at(&start, 20, 79) > 800.0, "the rim: {}", at(&start, 20, 79));
    steps(&gpu, &mut r, 100);
    let t = temps(&gpu, &r);
    assert!(at(&t, 20, 79) < at(&start, 20, 79) * 0.6, "it cools: {}", at(&t, 20, 79));
    // (20, 78), the skin's second pixel, is not beside the air.
    let kept = |x, y| at(&t, x, y) / at(&start, x, y);
    assert!(kept(20, 79) < kept(20, 78), "the rim beside the air faster than inside");
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

/// New values of the level's materials (issue #45, the material editor's
/// live preview) keep the heat: the temperatures stay, the next frame
/// draws and steps by the new values; without heat to keep, nothing
/// changes and `set_surface` is the call.
#[test]
fn new_material_values_keep_the_heat() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated(&gpu);
    r.deposit_heat(blast(20.0, 60.0, 8.0));
    let hot = frame(&gpu, &mut r, &dark());
    let before = temps(&gpu, &r);
    let steps_before = r.heat_steps();
    // Stone shows blue now, and conducts nothing.
    let mut edited = materials();
    edited[0].heat = Some(Heat {
        conductivity: 0,
        ramp: vec![RampKey { temperature: 1, colour: [0, 0, 255], glow: 100 }],
        ..heat(100, 100, 100)
    });
    assert!(r.set_surface_materials(&edited));
    assert_eq!(temps(&gpu, &r), before, "the heat is kept");
    let blue = frame(&gpu, &mut r, &dark());
    // World pixel (20, 60) is texel (0, 60) of the 2× frame from (20, 30).
    let (x, y) = (1, 60);
    assert_ne!(blue.get(x, y), hot.get(x, y), "the next frame draws the new ramp");
    let c = blue.get(x, y);
    assert!(c.b > c.r && c.b > c.g, "blue: {c:?}");
    assert_eq!(r.heat_steps(), steps_before, "the clock is kept");
    // It steps by the new values: stone no longer spreads.
    steps(&gpu, &mut r, 30);
    let t = temps(&gpu, &r);
    assert_eq!(at(&t, 30, 60), 0.0, "past the deposit nothing warms: {}", at(&t, 30, 60));
    assert!(at(&t, 20, 60) > 0.0);
    // No heat to keep: nothing changes.
    let cold = [material("cold", None)];
    assert!(!r.set_surface_materials(&cold));
    assert!(r.has_heat());
    let mut plain = lit(&gpu);
    plain.upload_terrain(&map());
    assert!(!plain.set_surface_materials(&materials()));
    assert!(!plain.has_heat());
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
/// back stone left of 40) takes a share of a deposit, a front pixel all
/// of it. Issue #65: the share is the ball's slice 14 pixels back.
#[test]
fn the_back_takes_a_share_of_a_deposit() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_rock(&gpu);
    r.deposit_heat(HeatDeposit { pos: [20.5, 40.5], radius: 20.0, amount: 800.0 });
    r.deposit_heat(HeatDeposit { pos: [20.5, 85.5], radius: 20.0, amount: 800.0 });
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert_eq!(at(&t, 20, 40), 800.0);
    assert!(near(at(&t, 20, 85), 800.0 * (1.0 - 196.0 / 400.0)), "the back: {}", at(&t, 20, 85));
}

/// Issue #65: the back wall lies [`BACK_DEPTH_PX`] behind the blast's
/// plane and takes the ball's slice there: a circle of radius `sqrt(R² −
/// 14²)` whose pixels take `amount × (1 − (d² + 14²) / R²)`; a blast no
/// wider than 14 leaves it cold.
#[test]
fn the_back_takes_the_balls_slice_14_px_back() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_rock(&gpu);
    // [`map`]'s open rows 80 to 90; its back is stone left of 40.
    let d = HeatDeposit { pos: [20.5, 85.5], radius: 17.0, amount: 300.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    let edge = (17.0f32 * 17.0 - 14.0 * 14.0).sqrt();
    let mut seen = 0;
    for y in 80..90 {
        for x in 0..40 {
            let (dx, dy) = (x as f32 + 0.5 - 20.5, y as f32 + 0.5 - 85.5);
            assert!(
                near(at(&t, x, y), slice(d, x, y)),
                "({x}, {y}): {} for {}",
                at(&t, x, y),
                slice(d, x, y)
            );
            if (dx * dx + dy * dy).sqrt() >= edge {
                assert_eq!(at(&t, x, y), 0.0, "({x}, {y}) past the circle");
            }
            seen += usize::from(at(&t, x, y) > 0.0);
        }
    }
    assert!(near(at(&t, 20, 85), 300.0 * (1.0 - 196.0 / 289.0)), "its centre: {}", at(&t, 20, 85));
    assert!((150..=19 * 10).contains(&seen), "{seen} warm");
    // A minigun's hit (12.5) leaves the back cold (the rock's face above
    // it takes its rim).
    let mut r = heated_rock(&gpu);
    r.deposit_heat(HeatDeposit { radius: 12.5, ..d });
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!((80..90).all(|y| (0..120).all(|x| at(&t, x, y) == 0.0)), "no back heat");
    assert!(at(&t, 20, 79) > 0.0, "the rim");
}

/// Issue #65: a blast beside a thick dirt block (the centre in the air,
/// the face 5 pixels away) heats the k-th dirt pixel along a straight
/// walk by `amount × DIRT_HEAT_GAIN × DIRT_SKIN_WEIGHTS[k − 1]`, no
/// falloff, 6 pixels deep; past them nothing. The same scene in rock
/// gives rock's rim.
#[test]
fn dirt_takes_the_blast_whole_six_pixels_deep() {
    let Some(gpu) = offscreen() else { return };
    // Stone's columns: solid above row 50, the face row 49, the centre
    // 5 pixels below it (row 54's centre).
    let d = HeatDeposit { pos: [20.5, 54.5], radius: 17.0, amount: 300.0 };
    let mut r = heated_level(&gpu, &level(Material::Dirt, |_, y| y >= 50));
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    for k in 1..=DIRT_SKIN {
        let y = 50 - k;
        let want = 300.0 * DIRT_HEAT_GAIN * DIRT_SKIN_WEIGHTS[k - 1];
        assert!(near(at(&t, 20, y), want), "dirt's {k}th pixel: {} for {want}", at(&t, 20, y));
    }
    assert_eq!(at(&t, 20, 49), at(&t, 20, 48), "the face and the next whole, no falloff");
    for y in 0..50 - DIRT_SKIN {
        assert_eq!(at(&t, 20, y), 0.0, "({y}) past the skin");
    }
    assert!((0..50 - DIRT_SKIN).all(|y| (0..120).all(|x| at(&t, x, y) == 0.0)), "nothing deeper");
    // In rock: rock's rim, by the falloff, 2 pixels deep.
    let mut r = heated_level(&gpu, &level(Material::Rock, |_, y| y >= 50));
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    for k in 1..=2 {
        let want = falloff(d, 20, 50 - k) * HEAT_SKIN_GAIN * HEAT_SKIN_WEIGHTS[k - 1];
        assert!(near(at(&t, 20, 50 - k), want), "rock's {k}th pixel: {} for {want}", at(&t, 20, 50 - k));
    }
    assert!((0..48).all(|y| (0..120).all(|x| at(&t, x, y) == 0.0)), "rock: nothing deeper");
}

/// Issue #65: a rock crust 3 pixels thick before thick dirt shields the
/// dirt: rock's limit (2) holds once the walk has met rock.
#[test]
fn rock_shields_the_dirt_behind_it() {
    let Some(gpu) = offscreen() else { return };
    // Stone's columns: rock in rows 47 to 49, dirt above, open below 50.
    let cells = (0..120 * 90)
        .map(|i| match i / 120 {
            50.. => Material::Background,
            47..50 => Material::Rock,
            _ => Material::Dirt,
        })
        .collect();
    let mut r = heated_level(&gpu, &level_of_cells(cells));
    let d = HeatDeposit { pos: [20.5, 54.5], radius: 17.0, amount: 300.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(at(&t, 20, 49) > 0.0 && at(&t, 20, 48) > 0.0, "the crust's skin");
    assert!((0..48).all(|y| (0..120).all(|x| at(&t, x, y) == 0.0)), "the dirt behind takes none");
    // Dirt behind a rock face 1 pixel thin takes dirt's second weight, whole.
    let cells = (0..120 * 90)
        .map(|i| match i / 120 {
            50.. => Material::Background,
            49 => Material::Rock,
            _ => Material::Dirt,
        })
        .collect();
    let mut r = heated_level(&gpu, &level_of_cells(cells));
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    let want = 300.0 * DIRT_HEAT_GAIN * DIRT_SKIN_WEIGHTS[1];
    assert!(near(at(&t, 20, 48), want), "dirt behind the face: {} for {want}", at(&t, 20, 48));
    assert!((0..48).all(|y| (0..120).all(|x| at(&t, x, y) == 0.0)), "nothing past rock's limit");
}

/// Issue #56: a blast in the air beside a thick rock block heats its face
/// whole, the next pixel in half of it, and nothing deeper.
#[test]
fn a_deposit_heats_only_the_rock_face() {
    let Some(gpu) = offscreen() else { return };
    // Stone's columns: rock above row 50, the face row 49.
    let mut r = heated_level(&gpu, &level(Material::Rock, |_, y| y >= 50));
    let d = HeatDeposit { pos: [20.5, 55.5], radius: 20.0, amount: 1000.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    let skin = |k: usize, x, y| falloff(d, x, y) * HEAT_SKIN_GAIN * HEAT_SKIN_WEIGHTS[k];
    for x in 14..=26 {
        assert!(near(at(&t, x, 49), skin(0, x, 49)), "the face at {x}: {}", at(&t, x, 49));
    }
    // Farther out a walk to row 48 crosses two pixels of the face first.
    for x in 18..=22 {
        assert!(near(at(&t, x, 48), skin(1, x, 48)), "the next pixel at {x}: {}", at(&t, x, 48));
    }
    assert!(at(&t, 20, 49) > 900.0);
    assert!((0..48).all(|y| (0..120).all(|x| at(&t, x, y) == 0.0)), "nothing deeper");
}

/// Issue #56: conduction carries the face's heat in: after some steps
/// the pixels 3 and 4 deep are warm, cooler than the face.
#[test]
fn heat_soaks_inward_from_the_face() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_level(&gpu, &level(Material::Rock, |_, y| y >= 50));
    r.deposit_heat(HeatDeposit { pos: [20.5, 55.5], radius: 20.0, amount: 1000.0 });
    frame(&gpu, &mut r, &dark());
    let before = temps(&gpu, &r);
    steps(&gpu, &mut r, 40);
    let t = temps(&gpu, &r);
    for y in [47, 46] {
        assert!(at(&t, 20, y) > at(&before, 20, y), "{y} warms: {}", at(&t, 20, y));
        assert!(at(&t, 20, y) > 1.0, "{y} is warm: {}", at(&t, 20, y));
        assert!(at(&t, 20, y) < at(&t, 20, 49), "{y} is cooler than the face");
    }
}

/// Issue #56: a rock wall 3 pixels thick between a blast and a tunnel in
/// its radius shields the wall's far face and the tunnel's back.
#[test]
fn a_wall_shields_what_is_behind_it() {
    let Some(gpu) = offscreen() else { return };
    // A cave in stone's columns, rows 30 to 60, cut by a wall at 20 to 22.
    let cave = level(Material::Rock, |x, y| (30..60).contains(&y) && x < 40 && !(20..23).contains(&x));
    let mut r = heated_level(&gpu, &cave);
    let d = HeatDeposit { pos: [14.5, 45.5], radius: 20.0, amount: 1000.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(near(at(&t, 17, 45), slice(d, 17, 45)), "the back in sight");
    assert!(near(at(&t, 20, 45), falloff(d, 20, 45) * HEAT_SKIN_GAIN), "the wall's face");
    assert!(at(&t, 21, 45) > 0.0, "the skin's second pixel");
    assert!((0..90).all(|y| (22..120).all(|x| at(&t, x, y) == 0.0)), "the far face and the tunnel take none");
}

/// Issue #56: a diagonal wall one pixel thin (its pixels touching at
/// corners) shields what lies behind it: a walk through a corner both of
/// whose sides are rock meets rock.
#[test]
fn a_diagonal_wall_shields_too() {
    let Some(gpu) = offscreen() else { return };
    // Open in stone's columns, rows 10 to 80, but for the wall x + y = 60.
    let cave = level(Material::Rock, |x, y| (10..80).contains(&y) && x < 40 && x + y != 60);
    let mut r = heated_level(&gpu, &cave);
    let d = HeatDeposit { pos: [18.5, 30.5], radius: 24.0, amount: 1000.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(near(at(&t, 30, 30), falloff(d, 30, 30) * HEAT_SKIN_GAIN), "the wall's face");
    assert!(near(at(&t, 25, 30), slice(d, 25, 30)), "the back in sight");
    let behind = (10..80).flat_map(|y| (0..40).map(move |x| (x, y))).filter(|&(x, y)| x + y > 60);
    for (x, y) in behind {
        assert_eq!(at(&t, x, y), 0.0, "({x}, {y}) behind the wall");
    }
}

/// Issue #56: a blast centred inside rock heats its own pixel whole and
/// the skin's next pixels round it in half, nothing past them.
#[test]
fn a_deposit_inside_rock_heats_round_its_centre() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_rock(&gpu);
    let d = HeatDeposit { pos: [20.5, 40.5], radius: 10.0, amount: 1000.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    for y in 0..90 {
        for x in 0..120 {
            let k = (x as i32 - 20).abs().max((y as i32 - 40).abs());
            let want = match k {
                0 => falloff(d, x, y) * HEAT_SKIN_GAIN * HEAT_SKIN_WEIGHTS[0],
                1 => falloff(d, x, y) * HEAT_SKIN_GAIN * HEAT_SKIN_WEIGHTS[1],
                _ => 0.0,
            };
            assert!(near(at(&t, x, y), want), "({x}, {y}): {} for {want}", at(&t, x, y));
        }
    }
}

/// Issue #56: every back pixel in sight of a blast, no rock or dirt on
/// the way, takes its share of it (issue #65: the ball's slice 14
/// pixels back).
#[test]
fn the_back_in_sight_takes_its_share() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_rock(&gpu);
    // [`map`]'s open rows 80 to 90; its back is stone left of 40.
    let d = HeatDeposit { pos: [20.5, 85.5], radius: 20.0, amount: 1000.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    let mut seen = 0;
    for y in 80..90 {
        for x in 0..40 {
            assert!(near(at(&t, x, y), slice(d, x, y)), "({x}, {y}): {}", at(&t, x, y));
            seen += usize::from(at(&t, x, y) > 0.0);
        }
    }
    assert!(seen > 150, "{seen} warm");
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
/// whose air has no back wall to heat; issue #56: the blast lies in the
/// air below the face it heats, where it lay inside the dirt.)
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
        r.deposit_heat(blast(50.0, 82.0, 8.0));
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
/// and in the editor's full-bright view. (Issue #56: the blast lies in
/// the air below the face it heats, where it lay inside the dirt.)
#[test]
fn a_hot_pixel_shows_its_colour_in_every_view() {
    let Some(gpu) = offscreen() else { return };
    for mode in [ViewMode::Lit, ViewMode::FullBright] {
        let mut look = dark();
        look.mode = mode;
        let mut r = heated(&gpu);
        let cold = frame(&gpu, &mut r, &look);
        r.deposit_heat(blast(30.0, 81.0, 8.0));
        let hot = frame(&gpu, &mut r, &look);
        // World pixel (30, 79), the face, is texel (20, 98) of the 2× frame.
        let (x, y) = ((30 - 20) * 2, (79 - 30) * 2);
        let c = hot.get(x, y);
        assert!(c.r >= 240 && c.g >= 200 && c.b >= 160, "{mode:?}: yellow-white at the centre: {c:?}");
        assert_ne!(cold.get(x, y), c);
        // (30, 74), the dirt skin's last pixel, takes half (issue #68: lit,
        // the skin's hottest pixels show their art lit by the heat's own
        // light plus the ramp's, white, no longer the ramp's colour alone).
        let edge = hot.get(x, y - 10);
        assert!(edge.r > edge.b, "{mode:?}: redder at half the heat: {edge:?}");
        assert_eq!(hot.get(0, 0), cold.get(0, 0), "{mode:?}: far pixels unchanged");
    }
}

/// Issue #68: in the dark, a Bazooka-sized blast (its radius, at the
/// hottest amount) leaves its rim yellow-white at its centre and redder at
/// its edge, as before heat added light over the cold look.
#[test]
fn a_hot_rim_still_glows_in_the_dark() {
    let Some(gpu) = offscreen() else { return };
    let radius = olr_look::engine::explosion_heat(4.0).unwrap().radius;
    let mut r = heated(&gpu);
    let cold = frame(&gpu, &mut r, &dark());
    r.deposit_heat(HeatDeposit { pos: [40.0, 81.0], radius, amount: 1000.0 });
    let hot = frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    let texel = |x: usize| ((x as u32 - 20) * 2, (79 - 30) * 2);
    // World pixel (40, 79), the face above the centre.
    assert!(at(&t, 40, 79) > 900.0, "{}", at(&t, 40, 79));
    let (x, y) = texel(40);
    let c = hot.get(x, y);
    assert!(c.r >= 240 && c.g >= 200 && c.b >= 160, "yellow-white at the centre: {c:?}");
    // Along the face, the first pixel at half the heat or less.
    let edge = (41..60).find(|&x| at(&t, x, 79) <= 500.0).unwrap();
    assert!(at(&t, edge, 79) > 300.0, "({edge}, 79) glows: {}", at(&t, edge, 79));
    let (x, y) = texel(edge);
    let (c, k) = (hot.get(x, y), cold.get(x, y));
    assert!(c.r > c.g && c.r > c.b + 100, "({edge}, 79) redder at the edge: {c:?}");
    assert!(c.r > k.r + 100, "and glowing: {c:?} cold {k:?}");
}

/// Issue #68: a lava material that casts no light (so no pixel's heat
/// lights another), on the back.
fn ember() -> SurfaceMaterial {
    material("ember", Some(heat(100, 100, 0)))
}

/// Issue #68: a light that lifts the light above 1 and blooms, the grade
/// neutral.
fn bright() -> LookFrame {
    let mut look = LookFrame::default();
    look.begin(None, 0.0);
    look.ambient = [1.6; 3];
    look.max_light = 2.0;
    look
}

/// Issue #68: steps `r`'s heat (ten steps a frame) until `hottest` (the
/// hottest pixel's temperature) is under the lava ramp's first key (100,
/// less a ramp bin), checking each frame drawn with `look` against
/// `cold`: no channel of any texel below its cold value (less 1 for
/// rounding), texel `(x, y)`'s distance from its cold value never growing
/// (1 of tolerance), and once nothing glows, exactly the cold frame.
fn fades_back(
    gpu: &Offscreen,
    r: &mut Renderer,
    look: &LookFrame,
    cold: &RgbaImage,
    (x, y): (u32, u32),
    hottest: impl Fn(&Offscreen, &Renderer) -> f32,
) {
    let distance = |c: Rgba| {
        let k = cold.get(x, y);
        [c.r.abs_diff(k.r), c.g.abs_diff(k.g), c.b.abs_diff(k.b)]
    };
    let mut img = frame(gpu, r, look);
    let start = distance(img.get(x, y));
    assert!(start.iter().any(|&d| d > 40), "it starts hot: {:?} cold {:?}", img.get(x, y), cold.get(x, y));
    let mut last = start;
    for n in 0.. {
        for ty in 0..img.height() {
            for tx in 0..img.width() {
                let (c, k) = (img.get(tx, ty), cold.get(tx, ty));
                assert!(
                    c.r.saturating_add(1) >= k.r
                        && c.g.saturating_add(1) >= k.g
                        && c.b.saturating_add(1) >= k.b,
                    "frame {n}: texel ({tx}, {ty}) {c:?} darker than cold {k:?}"
                );
            }
        }
        let now = distance(img.get(x, y));
        for ch in 0..3 {
            assert!(now[ch] <= last[ch] + 1, "frame {n}: channel {ch} went back: {last:?} then {now:?}");
        }
        last = now;
        if hottest(gpu, r) < 97.0 {
            assert!(img == *cold, "frame {n}: nothing glows, the cold bytes");
            assert!(n > 20, "it took {n} frames: a fade, not a snap");
            return;
        }
        assert!(n < 1000, "still hot after {n} frames");
        r.advance_heat(10 * HEAT_STEP_MS);
        img = frame(gpu, r, look);
    }
}

/// Issue #68: a 120 × 90 open level whose back is `back` (art scale 2).
fn open_back(back: Rgba) -> ColorMap {
    let t = Terrain::from_grid(&MaterialGrid::new(120, 90, Material::Background));
    let theme =
        ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(4, 4, Rgba::rgb(120, 80, 40)) };
    ColorMap::new(&t, &solid(240, 180, back), &solid(240, 180, back), 2, theme)
}

/// Issue #68: a hot pixel is drawn as it looks cold plus its heat as
/// emitted light. A hot back pixel under a light above 1, with bloom,
/// never draws darker than cold, moves steadily back to its cold look as
/// it cools, and once its glow is 0 draws exactly its cold bytes (it
/// used to turn dark first: capped at 1, no bloom, pulled toward the
/// ramp's dark red, then snap back).
#[test]
fn a_cooling_pixel_fades_back_to_its_cold_look() {
    let Some(gpu) = offscreen() else { return };
    let map = open_back(Rgba::rgb(110, 90, 70));
    let mut r = lit(&gpu);
    r.set_bloom(true);
    let mut m = SurfaceMaps::empty((120, 90));
    m.back.iter_mut().for_each(|v| *v = 1);
    r.set_surface(&[ember()], Some(&m));
    r.upload_terrain(&map);
    let cold = frame(&gpu, &mut r, &bright());
    r.set_bloom(false);
    assert_ne!(frame(&gpu, &mut r, &bright()), cold, "the light blooms");
    r.set_bloom(true);
    // The back 14 behind the ball's plane: 1000 (1 - 196 / 1600), 877.
    r.deposit_heat(blast(60.5, 45.5, 40.0));
    // World pixel (60, 45), the centre.
    fades_back(&gpu, &mut r, &bright(), &cold, (80, 30), |gpu, r| {
        temps(gpu, r).into_iter().fold(0.0, f32::max)
    });
}

/// Issue #68: [`a_cooling_pixel_fades_back_to_its_cold_look`] on a hot
/// distant layer (issue #41).
#[test]
fn a_cooling_distant_layer_fades_back_too() {
    let Some(gpu) = offscreen() else { return };
    let mut r = lit(&gpu);
    r.set_bloom(true);
    r.set_surface(&[ember()], None);
    r.set_distant_surface(&[1]);
    r.upload_terrain(&see_through(120, 90, 2, |_, _| false));
    r.set_distant(&[layer(240, 180, Rgba::rgb(110, 90, 70), (0, 0), 5)]);
    assert!(r.distant_heat_texture(0).is_some() && r.heat_texture().is_none());
    let (l, _) = one_to_one();
    let cold = frame(&gpu, &mut r, &bright());
    // Depth 5 lies 19.5 back: 1000 (1 - 380.25 / 1600), 762.
    r.deposit_heat_seen(blast(60.5, 50.5, 40.0), &[HeatView::of(&l)]);
    let layer_hottest =
        |gpu: &Offscreen, r: &Renderer| hottest(&layer_temps(gpu, r.distant_heat_texture(0).unwrap()).1).1;
    fades_back(&gpu, &mut r, &bright(), &cold, (2 * (60 - 20), 2 * (50 - 30)), layer_hottest);
}

/// A hot back wall lets the light through: a lamp left of a hot patch of
/// back wall that casts light lights the wall beyond the patch, and the
/// patch, never less than cold, as the patch glows and as it cools
/// ([`fades_back`]). The glow light used to take a glowing cell for a
/// stopper: the patch cast a shadow, darkest as it cooled.
#[test]
fn a_hot_back_wall_lets_the_light_through() {
    let Some(gpu) = offscreen() else { return };
    let map = open_back(Rgba::rgb(110, 90, 70));
    let mut r = lit(&gpu);
    let mut m = SurfaceMaps::empty((120, 90));
    m.back.iter_mut().for_each(|v| *v = 1);
    r.set_surface(&[material("stone", Some(heat(100, 100, 100)))], Some(&m));
    r.upload_terrain(&map);
    let mut look = dark();
    look.emitters.push(olr_look::FrameLight { colour: [4.0; 3], ..lamp([25.5, 45.5], 1000.0) });
    let cold = frame(&gpu, &mut r, &look);
    let beyond = light_at(&gpu, &r, 90, 45);
    assert!(beyond[0] > 0.1, "the lamp lights the wall beyond: {beyond:?}");
    // The back 14 behind the ball's plane: 1000 (1 - 196 / 400), 510 at
    // the centre, a circle of radius sqrt(204) from x 46 to 75.
    r.deposit_heat(blast(60.5, 45.5, 20.0));
    frame(&gpu, &mut r, &look);
    let hot = light_at(&gpu, &r, 90, 45);
    assert!(
        (0..3).all(|c| hot[c] >= beyond[c] * 0.99),
        "no shadow beyond the patch: {hot:?}, cold {beyond:?}"
    );
    fades_back(&gpu, &mut r, &look, &cold, (80, 30), |gpu, r| temps(gpu, r).into_iter().fold(0.0, f32::max));
}

/// A hot patch of back wall in open air, far from any rock, lights the
/// air round it: no ray jumps over the haze (a ray far from every
/// stopper jumps; only the haze's edge stops a jump, so it walks into
/// the patch).
#[test]
fn hot_open_air_lights_the_air_round_it() {
    let Some(gpu) = offscreen() else { return };
    let map = open_back(Rgba::rgb(110, 90, 70));
    let mut r = lit(&gpu);
    let mut m = SurfaceMaps::empty((120, 90));
    m.back.iter_mut().for_each(|v| *v = 1);
    r.set_surface(&[material("stone", Some(heat(100, 100, 100)))], Some(&m));
    r.upload_terrain(&map);
    frame(&gpu, &mut r, &dark());
    let cold = [light_at(&gpu, &r, 20, 45), light_at(&gpu, &r, 60, 15)];
    // A circle of radius sqrt(204) from x 46 to 75, y 31 to 60.
    r.deposit_heat(blast(60.5, 45.5, 20.0));
    frame(&gpu, &mut r, &dark());
    let hot = [light_at(&gpu, &r, 20, 45), light_at(&gpu, &r, 60, 15)];
    for (h, c) in hot.iter().zip(&cold) {
        assert!(h[0] > c[0] + 0.025, "the haze lights the air 16 to 26 pixels off: {hot:?}, cold {cold:?}");
    }
}

/// Issue #67: a dug pixel loses its heat, whatever its back: the dirt's
/// glow goes with the dirt. The dirt left round the hole keeps its own.
#[test]
fn a_dug_pixel_loses_its_heat() {
    let Some(gpu) = offscreen() else { return };
    let mut map = map();
    let mut r = lit(&gpu);
    r.set_surface(&materials(), Some(&maps()));
    r.upload_terrain(&map);
    map.take_dirty().for_each(drop);
    // Inside [`map`]'s dirt, over its stone back: the dirt round it hot.
    r.deposit_heat(blast(20.5, 40.5, 8.0));
    frame(&gpu, &mut r, &dark());
    let before = temps(&gpu, &r);
    assert!(at(&before, 20, 40) > 500.0, "the dirt is hot: {}", at(&before, 20, 40));
    // Hole 2 carves the 9 × 9 square round (20, 40).
    map.apply(ColorEdit::Carve { hole: 2, x: 20, y: 40 });
    r.update_terrain(&mut map);
    steps(&gpu, &mut r, 1);
    let t = temps(&gpu, &r);
    for y in 36..=44 {
        for x in 16..=24 {
            assert_eq!(at(&t, x, y), 0.0, "({x}, {y}) dug: cold");
        }
    }
    for (x, y) in [(20, 45), (25, 40), (20, 35), (15, 40)] {
        assert!(at(&t, x, y) > 100.0, "({x}, {y}) the dirt round the hole keeps its heat: {}", at(&t, x, y));
    }
    // And stays in the dirt: the hole's back stays cold.
    steps(&gpu, &mut r, 20);
    let t = temps(&gpu, &r);
    assert!((36..=44).all(|y| (16..=24).all(|x| at(&t, x, y) == 0.0)), "the hole's back stays cold");
    assert!(at(&t, 20, 45) > 0.0);
}

/// Issue #67: [`map`]'s columns left of 40 (a stone front and back)
/// open but for a dirt block (14..30 × 10..40) and a rock column right
/// of it (30..34), touching it.
fn block() -> ColorMap {
    let cells = (0..120 * 90)
        .map(|i| match (i % 120, i / 120) {
            (14..30, 10..40) => Material::Dirt,
            (30..34, 10..40) => Material::Rock,
            _ => Material::Background,
        })
        .collect();
    level_of_cells(cells)
}

/// Issue #67: heat stays in its layer. A deposit inside a dirt block
/// whose walk reaches only the dirt heats it; however long it steps, the
/// background beside the block's faces stays at 0, while the rock
/// touching the dirt warms (both solid). A hot spot on the background
/// alone still spreads to its background neighbours.
#[test]
fn heat_stays_in_its_layer() {
    let Some(gpu) = offscreen() else { return };
    let mut r = heated_level(&gpu, &block());
    // 6.5 from the left face, 9.5 from the rock: the walk's dirt skin
    // (6 pixels) reaches neither the face nor the rock.
    r.deposit_heat(blast(20.5, 25.5, 8.0));
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(at(&t, 20, 25) > 500.0, "the dirt is hot: {}", at(&t, 20, 25));
    assert!((10..40).all(|y| at(&t, 30, y) == 0.0), "the rock takes none of the deposit");
    let back = |t: &[f32]| {
        let beside = (10..40).flat_map(|y| [(13, y), (34, y)]);
        let over = (13..35).flat_map(|x| [(x, 9), (x, 40)]);
        beside.chain(over).map(|(x, y)| at(t, x, y)).fold(0.0, f32::max)
    };
    assert_eq!(back(&t), 0.0);
    for n in [1, 50, 200] {
        r.advance_heat(n * HEAT_STEP_MS);
        frame(&gpu, &mut r, &dark());
        let t = temps(&gpu, &r);
        assert_eq!(back(&t), 0.0, "after {n} more steps the background beside the block is cold");
        assert!(
            (0..120).all(|x| (0..90).all(|y| {
                let solid = (14..34).contains(&x) && (10..40).contains(&y);
                solid || at(&t, x, y) == 0.0
            })),
            "no background pixel warms"
        );
    }
    let t = temps(&gpu, &r);
    assert!(at(&t, 14, 25) > 0.0, "the dirt's face warms within the dirt: {}", at(&t, 14, 25));
    assert!(at(&t, 30, 25) > 0.0, "the rock touching the dirt warms: {}", at(&t, 30, 25));

    // A hot spot on the background alone, far from the block: its slice
    // (radius 9.6) spreads past it along the back wall.
    let mut r = heated_level(&gpu, &block());
    let d = HeatDeposit { pos: [20.5, 70.5], radius: 17.0, amount: 1000.0 };
    r.deposit_heat(d);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    assert!(near(at(&t, 20, 70), slice(d, 20, 70)));
    assert_eq!(at(&t, 20, 80), 0.0, "past the slice");
    steps(&gpu, &mut r, 5);
    let t = temps(&gpu, &r);
    assert!(at(&t, 20, 80) > 0.0, "the back's heat spreads along the back: {}", at(&t, 20, 80));
    assert!(at(&t, 20, 60) > 0.0);
}

/// Issue #67: a crater carved and its blast deposited in the same frame
/// (play's order: `update_terrain`, then the deposits): its back takes
/// the ball's slice whole, as [`the_back_takes_the_balls_slice_14_px_back`]
/// computes it; the dirt's heat from before goes with the dirt. With
/// nothing hot before (no pass to order) and with the dirt hot.
#[test]
fn a_fresh_craters_back_takes_its_slice() {
    let Some(gpu) = offscreen() else { return };
    for hot_before in [false, true] {
        let mut map = map();
        let mut r = lit(&gpu);
        r.set_surface(&materials(), Some(&maps()));
        r.upload_terrain(&map);
        map.take_dirty().for_each(drop);
        if hot_before {
            r.deposit_heat(blast(20.5, 40.5, 8.0));
            frame(&gpu, &mut r, &dark());
            assert!(at(&temps(&gpu, &r), 20, 40) > 500.0);
        }
        // Hole 4 carves the 15 × 15 square round (20, 40).
        map.apply(ColorEdit::Carve { hole: 4, x: 20, y: 40 });
        r.update_terrain(&mut map);
        let d = HeatDeposit { pos: [20.5, 40.5], radius: 17.0, amount: 300.0 };
        r.deposit_heat(d);
        frame(&gpu, &mut r, &dark());
        let t = temps(&gpu, &r);
        let mut warm = 0;
        for y in 33..=47 {
            for x in 13..=27 {
                assert!(
                    near(at(&t, x, y), slice(d, x, y)),
                    "hot before {hot_before}: ({x}, {y}): {} for {}",
                    at(&t, x, y),
                    slice(d, x, y)
                );
                warm += usize::from(at(&t, x, y) > 0.0);
            }
        }
        assert!(warm > 150, "{warm} warm");
        assert!(near(at(&t, 20, 40), 300.0 * (1.0 - 196.0 / 289.0)));
        // The dirt round the crater takes its face's heat.
        assert!(at(&t, 20, 48) > 300.0, "the crater's wall: {}", at(&t, 20, 48));
    }
}

/// Issue #67: dirt added over a hot back starts cold; the back beside it
/// keeps its heat, and the new dirt takes none from it.
#[test]
fn new_dirt_starts_cold() {
    let Some(gpu) = offscreen() else { return };
    let mut map = map();
    let mut r = lit(&gpu);
    r.set_surface(&materials(), Some(&maps()));
    r.upload_terrain(&map);
    map.take_dirty().for_each(drop);
    // [`map`]'s open rows 80 to 90 over its stone back.
    r.deposit_heat(HeatDeposit { pos: [20.5, 85.5], radius: 30.0, amount: 600.0 });
    frame(&gpu, &mut r, &dark());
    let before = temps(&gpu, &r);
    assert!(at(&before, 20, 85) > 100.0, "the back is hot: {}", at(&before, 20, 85));
    // Hole 0 (5 × 5) of dirt round (20, 85): 18..=22 × 83..=87.
    map.apply(ColorEdit::Dirt { hole: 0, x: 20, y: 85 });
    r.update_terrain(&mut map);
    frame(&gpu, &mut r, &dark());
    let t = temps(&gpu, &r);
    let new = |x: usize, y: usize| (18..=22).contains(&x) && (83..=87).contains(&y);
    for y in 80..90 {
        for x in 0..40 {
            if new(x, y) {
                assert_eq!(at(&t, x, y), 0.0, "({x}, {y}) new dirt: cold");
            } else {
                assert_eq!(at(&t, x, y), at(&before, x, y), "({x}, {y}) the back unchanged");
            }
        }
    }
    steps(&gpu, &mut r, 10);
    let t = temps(&gpu, &r);
    assert!(
        (83..=87).all(|y| (18..=22).all(|x| at(&t, x, y) == 0.0)),
        "the new dirt takes none from the back"
    );
    assert!(at(&t, 17, 85) > 0.0, "the back beside it stays hot");
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
/// a farther one smaller still, and the layers cool to nothing. Issue
/// #65: on one depth line, the level's back wall 14 back, a layer of
/// depth `d` at `14 + 1.1 d`.
#[test]
fn a_near_layer_takes_a_smaller_circle_than_the_plane() {
    let Some(gpu) = offscreen() else { return };
    let stone = [material("stone", Some(heat(100, 100, 100)))];
    let mut r = layered(&gpu, &stone, 1, &[1, 4], &[1, 1]);
    let (l, _) = one_to_one();
    r.deposit_heat_seen(HeatDeposit { pos: [60.5, 45.5], radius: 20.0, amount: 1000.0 }, &[HeatView::of(&l)]);
    frame(&gpu, &mut r, &dark());
    let plane = temps(&gpu, &r);
    let (w, near) = layer_temps(&gpu, r.distant_heat_texture(0).unwrap());
    let (_, far) = layer_temps(&gpu, r.distant_heat_texture(1).unwrap());
    assert_eq!(w, 120, "the layer's world size");
    // The ball's share at each one's centre: z = 14, 15.1 and 18.4.
    let ((_, p), (ni, n), (_, f)) = (hottest(&plane), hottest(&near), hottest(&far));
    assert!((p - 1000.0 * (1.0 - 196.0 / 400.0)).abs() < 2.0, "the back wall: {p}");
    assert!((n - 1000.0 * (1.0 - 228.01 / 400.0)).abs() < 2.0, "the near layer: {n}");
    assert!((f - 1000.0 * (1.0 - 338.56 / 400.0)).abs() < 2.0, "the far layer: {f}");
    assert!(p > n && n > f, "fainter the further back");
    // Placed behind the deposit: the camera (60, 60) moved the layer 0.15 down.
    assert_eq!((ni % w, ni / w), (60, 45), "the near spot's centre");
    let across = |t: &[f32], y| warm_in_row(120, t, y);
    let (cp, cn, cf) = (across(&plane, 45), across(&near, 45), across(&far, 45));
    assert!((28..=30).contains(&cp), "the back wall's circle, radius sqrt(204): {cp}");
    assert!((26..=28).contains(&cn), "radius sqrt(171.99) at depth 1: {cn}");
    assert!((14..=16).contains(&cf), "radius sqrt(61.44) at depth 4: {cf}");
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
    // Issue #65: z = 14 + 1.1 d; 63.5 is within MAX_HEAT_RADIUS (64),
    // 64.6 not; then a cold material, none and a value past the list.
    let r = layered(&gpu, &m, 0, &[45, 46, 10, 10, 10], &[1, 1, 2, 0, 9]);
    assert!(r.distant_heat_texture(0).is_some(), "depth 45 is within reach");
    assert!((1..5).all(|i| r.distant_heat_texture(i).is_none()));
    assert!(r.has_heat() && r.heat_texture().is_none(), "the layer's heat alone");
    let mut far = layered(&gpu, &m, 0, &[46, 10], &[1, 2]);
    assert!(!far.has_heat());
    let mut plain = layered(&gpu, &[], 0, &[46, 10], &[]);
    let (l, size) = one_to_one();
    let want = gpu.render_lit_to_image(&mut plain, &l, Some(&dark()), size);
    far.deposit_heat_seen(blast(60.0, 45.0, 64.0), &[HeatView::of(&l)]);
    far.advance_heat(100);
    assert_eq!(gpu.render_lit_to_image(&mut far, &l, Some(&dark()), size), want, "far layers unchanged");
    assert_eq!(far.heat_passes(), 0);
    // A heated layer with nothing hot: the same bytes.
    let mut idle = layered(&gpu, &m, 0, &[46, 10], &[1, 1]);
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
    // Issue #65: depth 10 lies 25 back; a blast of 30 reaches it.
    r.deposit_heat_seen(HeatDeposit { amount: 300.0, ..blast(70.5, 50.5, 30.0) }, &[HeatView::of(&a)]);
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
        // Issue #65: depth 10 lies 25 back; a blast of 30 reaches it.
        r.deposit_heat_seen(HeatDeposit { amount: 200.0, ..blast(70.5, 50.5, 30.0) }, views);
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
    none.deposit_heat_seen(blast(70.5, 50.5, 30.0), &[]);
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
    // Issue #65: depth 5 lies 19.5 back.
    r.deposit_heat_seen(HeatDeposit { amount: 300.0, ..blast(60.5, 50.5, 30.0) }, &[HeatView::of(&l)]);
    let hot = frame(&gpu, &mut r, &dark());
    let (_, after) = gpu.read_light(&r).unwrap();
    let c = hot.get(2 * (60 - 20), 2 * (50 - 30));
    assert!(c.r > 200 && c.r > c.b + 60, "the layer glows: {c:?}");
    assert_ne!(cold, hot);
    assert!(before == after, "but lights nothing");
}

/// The render profile: each heat operation is timed in its own group (the
/// copies back too where the device has timestamps inside encoders), each
/// pass keeps its own span, and the workload counts the hot area.
#[test]
fn heat_ops_are_timed_in_their_groups_and_counted_in_the_workload() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Ok(gpu) =
        olr_render::Offscreen::open(&OffscreenOptions { timestamps: true, ..OffscreenOptions::default() })
    else {
        return;
    };
    let mut r = heated(&gpu);
    if !r.enable_gpu_timing() {
        eprintln!("no timestamps; skipping");
        return;
    }
    frame(&gpu, &mut r, &dark());
    let _ = r.gpu_times();
    r.deposit_heat(blast(20.0, 45.0, 10.0));
    r.advance_heat(HEAT_STEP_MS * 3);
    frame(&gpu, &mut r, &dark());
    let t = r.gpu_times().expect("timed");
    assert!(t.passes[PassGroup::HeatDeposit as usize] > 0.0, "{t:?}");
    assert!(t.passes[PassGroup::HeatStep as usize] > 0.0, "{t:?}");
    assert_eq!(t.passes[PassGroup::HeatCopy as usize] > 0.0, t.copies_timed, "{t:?}");
    assert_eq!(t.untimed, 0);
    let steps = r.last_passes().iter().filter(|(g, _)| *g == PassGroup::HeatStep).count();
    assert_eq!(steps, 3, "{:?}", r.last_passes());
    let w = r.workload();
    assert!(w.heat_area > 0 && w.heat_boxes >= 1 && w.heat_steps >= 3, "{w:?}");
    assert!(!w.world_targets.is_empty() && w.glow_grid.is_some() && w.cascades > 0, "{w:?}");
}

/// Review Focus 3: a catch-up frame of `MAX_HEAT_STEPS` steps outruns the
/// timestamp slots: its extra passes run untimed and are counted, the
/// frame is still timed, nothing panics.
#[test]
fn a_catch_up_frame_counts_the_passes_past_the_slots() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Ok(gpu) =
        olr_render::Offscreen::open(&OffscreenOptions { timestamps: true, ..OffscreenOptions::default() })
    else {
        return;
    };
    let mut r = heated(&gpu);
    if !r.enable_gpu_timing() {
        eprintln!("no timestamps; skipping");
        return;
    }
    frame(&gpu, &mut r, &dark());
    let _ = r.gpu_times();
    r.deposit_heat(blast(20.0, 45.0, 10.0));
    r.advance_heat(HEAT_STEP_MS * MAX_HEAT_STEPS);
    frame(&gpu, &mut r, &dark());
    let t = r.gpu_times().expect("timed");
    assert!(t.untimed > 0 && t.frame_ms > 0.0, "{t:?}");
    assert!(r.heat_steps() >= u64::from(MAX_HEAT_STEPS), "{}", r.heat_steps());
    assert!(t.passes[PassGroup::HeatStep as usize] > 0.0, "{t:?}");
}
