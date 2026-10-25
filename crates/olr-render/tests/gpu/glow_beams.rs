//! Glow beams (the directional-glow design): a directional patch's light
//! is limited to its cone, and it draws a crisp beam, as wide as the patch
//! where it starts, soft-shadowed by rock and dirt; haze shows it in the
//! air and over the sky.

use olr_data::level::DistantLayer;
use olr_terrain::GlowBeamMap;

use super::*;

/// The level's size, world pixels (art scale 1).
const W: u32 = 128;
const H: u32 = 64;

/// The patch: rock at x 20, rows 30 to 34, glowing white in group 1,
/// facing right (direction 0) with this spread.
const PATCH_X: u32 = 20;
const PATCH_ROWS: std::ops::Range<u32> = 30..35;
const SPREAD: u8 = 30;

/// The dark ambient.
const AMBIENT: f32 = 0.02;

fn in_patch(x: u32, y: u32) -> bool {
    x == PATCH_X && PATCH_ROWS.contains(&y)
}

/// A `W` × `H` level at art scale 1: open (grey art) but where `solid`
/// names a material, the patch rock (white art) glowing at full strength
/// in group 1; see-through open air (back alpha 0) when `see_through`.
/// Its 9 × 9 hole carves.
fn level(fill: impl Fn(u32, u32) -> Option<Material>, see_through: bool) -> ColorMap {
    let at = |x: u32, y: u32| if in_patch(x, y) { Some(Material::Rock) } else { fill(x, y) };
    let cells = (0..W * H).map(|i| at(i % W, i / W).unwrap_or(Material::Background)).collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(W, H, cells).unwrap());
    let mut front = RgbaImage::new(W, H);
    let mut back = RgbaImage::new(W, H);
    for y in 0..H {
        for x in 0..W {
            let c = if in_patch(x, y) { Rgba::rgb(255, 255, 255) } else { Rgba::rgb(128, 128, 128) };
            front.set(x, y, c);
            let open = at(x, y).is_none();
            back.set(x, y, if open && see_through { Rgba { a: 0, ..c } } else { c });
        }
    }
    let holes = vec![Stamp::from_image(&solid(9, 9, Rgba::MAGENTA))];
    let theme = ThemeArt { holes, green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    let mut map = if see_through {
        ColorMap::new_see_through(&t, &front, &back, 1, theme)
    } else {
        ColorMap::new(&t, &front, &back, 1, theme)
    };
    let mut g = olr_terrain::GlowLayers::empty((W, H), (W, H));
    for y in PATCH_ROWS {
        let i = (y * W + PATCH_X) as usize;
        (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
    }
    assert!(map.set_glow(Some(g)));
    map
}

/// The patch's beam map: directional (facing right at `spread`) when
/// `directed`, else none of its texels set.
fn beams(directed: bool, spread: u8) -> GlowBeamMap {
    let mut b = GlowBeamMap::none((W, H));
    if directed {
        for y in PATCH_ROWS {
            let i = ((y * W + PATCH_X) * 2) as usize;
            (b.front[i], b.front[i + 1]) = (1, spread);
        }
    }
    b
}

/// [`level`] with the patch's beams at [`SPREAD`] (`directed`).
fn beam_level(fill: impl Fn(u32, u32) -> Option<Material>, directed: bool) -> ColorMap {
    let mut map = level(fill, false);
    assert!(map.set_glow_beams(Some(beams(directed, SPREAD))));
    map
}

/// The light only (a white albedo) at a dark ambient, no bounce, max light
/// 2 at half the exposure (so up to 2 shows), haze `haze` percent; group 1
/// at intensity 1000 (cast 10), its own colour, reach 60.
fn beam_look(haze: u8) -> olr_look::LookFrame {
    let mut look = olr_look::LookFrame::default();
    look.begin(None, 0.0);
    look.ambient = [AMBIENT; 3];
    look.max_light = 2.0;
    look.grade.exposure = -100;
    look.mode = olr_look::ViewMode::LightOnly;
    look.haze = haze;
    look.groups.push(olr_look::FrameGroup {
        cast: [10.0; 3],
        own_colour: true,
        filtered: false,
        self_scale: 1.0,
        reach: 60.0,
        ..Default::default()
    });
    look
}

fn renderer(gpu: &Offscreen, map: &ColorMap) -> Renderer {
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.set_lighting(true);
    r.set_bloom(false);
    r.upload_terrain(map);
    r
}

/// A frame drawn at view origin `at`, `w` × `h` world pixels, two output
/// pixels a world pixel.
struct Frame {
    img: RgbaImage,
    at: (u32, u32),
}

impl Frame {
    /// World pixel `(x, y)`'s four texels' mean light, linear, at the
    /// look's exposure (light / 2).
    fn light(&self, x: u32, y: u32) -> f32 {
        let (i, j) = (2 * (x - self.at.0), 2 * (y - self.at.1));
        let px = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| self.img.get(i + dx, j + dy));
        px.iter().map(|p| srgb(p.r)).sum::<f32>() / 4.0
    }

    /// World pixel `(x, y)`'s four texels.
    fn texels(&self, x: u32, y: u32) -> [Rgba; 4] {
        let (i, j) = (2 * (x - self.at.0), 2 * (y - self.at.1));
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| self.img.get(i + dx, j + dy))
    }
}

fn srgb(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn frame_at(
    gpu: &Offscreen,
    r: &mut Renderer,
    look: &olr_look::LookFrame,
    at: (u32, u32),
    size: (u32, u32),
) -> Frame {
    let mut l = list(View { x: at.0 as f32, y: at.1 as f32, w: size.0 as f32, h: size.1 as f32 });
    l.map = (W, H);
    Frame { img: gpu.render_lit_to_image(r, &l, Some(look), (2 * size.0, 2 * size.1)), at }
}

/// The whole level.
fn frame(gpu: &Offscreen, r: &mut Renderer, look: &olr_look::LookFrame) -> Frame {
    frame_at(gpu, r, look, (0, 0), (W, H))
}

/// A world pixel far past the reach, in every frame's view.
const FAR: (u32, u32) = (90, 10);

/// The ambient as a frame shows it: far past the reach.
fn ambient(f: &Frame) -> f32 {
    f.light(FAR.0, FAR.1)
}

#[test]
fn a_beam_lights_ahead_and_not_behind() {
    let Some(gpu) = offscreen() else { return };
    let look = beam_look(0);
    let map = beam_level(|_, _| None, true);
    let mut r = renderer(&gpu, &map);
    let f = frame(&gpu, &mut r, &look);
    let amb = ambient(&f);
    assert!(amb > 0.0);
    let (ahead, behind) = (f.light(41, 32), f.light(15, 32));
    assert!(ahead > 3.0 * amb, "ahead: {ahead} against {amb}");
    assert!(behind < 1.1 * amb, "behind: {behind} against {amb}");
    // Undirected, the patch lights behind it: the cone darkens it.
    let mut r = renderer(&gpu, &beam_level(|_, _| None, false));
    let f = frame(&gpu, &mut r, &look);
    assert!(f.light(15, 32) > 1.5 * amb, "undirected behind: {} against {amb}", f.light(15, 32));
}

#[test]
fn a_beam_starts_as_wide_as_its_source() {
    let Some(gpu) = offscreen() else { return };
    let mut r = renderer(&gpu, &beam_level(|_, _| None, true));
    let f = frame(&gpu, &mut r, &beam_look(0));
    let x = PATCH_X + 2;
    let across: Vec<f32> = PATCH_ROWS.map(|y| f.light(x, y)).collect();
    let (lo, hi) = across.iter().fold((f32::MAX, 0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    assert!(hi <= 1.1 * lo, "the five rows ahead: {across:?}");
    for y in [PATCH_ROWS.start - 2, PATCH_ROWS.end + 1] {
        assert!(f.light(x, y) < 0.5 * lo, "two rows outside, {y}: {} against {lo}", f.light(x, y));
    }
}

#[test]
fn a_wall_stops_a_beam() {
    let Some(gpu) = offscreen() else { return };
    let look = beam_look(0);
    let mut r = renderer(&gpu, &beam_level(|x, _| (x == 40).then_some(Material::Rock), true));
    let f = frame(&gpu, &mut r, &look);
    assert!(f.light(38, 32) > 3.0 * ambient(&f), "in front of the wall: lit");
    assert_eq!(f.texels(45, 32), f.texels(FAR.0, FAR.1), "behind it: the ambient");
}

#[test]
fn shadow_edges_are_smooth() {
    let Some(gpu) = offscreen() else { return };
    // A rock step from x 25 cuts the beam's lower half.
    let step = |x: u32, y: u32| ((25..29).contains(&x) && y >= 33).then_some(Material::Rock);
    let mut r = renderer(&gpu, &beam_level(step, true));
    let f = frame(&gpu, &mut r, &beam_look(0));
    let column: Vec<f32> = (20..50).map(|y| f.light(55, y)).collect();
    let (lo, hi) = column.iter().fold((f32::MAX, 0f32), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    assert!(hi > 2.0 * lo, "the step casts a shadow: {column:?}");
    for (y, pair) in (20..).zip(column.windows(2)) {
        let jump = (pair[1] - pair[0]).abs();
        assert!(jump <= 0.25 * (hi - lo), "row {y}: {jump} of {} ({column:?})", hi - lo);
    }
}

#[test]
fn digging_opens_a_shadow() {
    let Some(gpu) = offscreen() else { return };
    let look = beam_look(0);
    let mut map = beam_level(|x, _| (x == 40).then_some(Material::Dirt), true);
    let mut r = renderer(&gpu, &map);
    let f = frame(&gpu, &mut r, &look);
    assert_eq!(f.texels(45, 32), f.texels(FAR.0, FAR.1), "behind the dirt: the ambient");
    for y in (0..H as i32).step_by(4) {
        map.apply(ColorEdit::Carve { hole: 0, x: 40, y });
    }
    assert!((0..H).all(|y| map.material().material(40, i64::from(y)) == Some(Material::Background)));
    r.update_terrain(&mut map);
    let f = frame(&gpu, &mut r, &look);
    assert!(f.light(45, 32) > 3.0 * ambient(&f), "dug: lit ({})", f.light(45, 32));
}

#[test]
fn still_glow_draws_steady_bytes() {
    let Some(gpu) = offscreen() else { return };
    let mut r = renderer(&gpu, &beam_level(|x, y| (x == 60 && y > 36).then_some(Material::Rock), true));
    let look = beam_look(40);
    let a = frame(&gpu, &mut r, &look);
    let b = frame(&gpu, &mut r, &look);
    assert!(a.light(41, 32) > 3.0 * ambient(&a));
    assert_eq!(a.img, b.img);
}

#[test]
fn beams_are_camera_independent() {
    let Some(gpu) = offscreen() else { return };
    let mut r = renderer(&gpu, &beam_level(|x, y| (x == 60 && y > 36).then_some(Material::Rock), true));
    let look = beam_look(40);
    let a = frame_at(&gpu, &mut r, &look, (0, 0), (96, 48));
    let b = frame_at(&gpu, &mut r, &look, (16, 8), (96, 48));
    assert!(a.light(41, 32) > 3.0 * ambient(&b));
    for y in 8..48 {
        for x in 16..96 {
            assert_eq!(a.texels(x, y), b.texels(x, y), "({x}, {y})");
        }
    }
}

#[test]
fn haze_shows_a_beam_over_the_sky() {
    let Some(gpu) = offscreen() else { return };
    let sky =
        [DistantLayer { image: solid(W, H, Rgba::rgb(40, 60, 120)), at: (0, 0), depth: 50, glow: None }];
    // Haze `haze` percent; the beams with `directed`, wide (so the glow
    // light along the beam is the undirected patch's); a haze map of 0 over
    // x 36 to 55 with `cleared`.
    let draw = |directed: bool, haze: u8, cleared: bool| {
        let mut map = level(|_, _| None, true);
        if directed {
            assert!(map.set_glow_beams(Some(beams(true, 60))));
        }
        if cleared {
            let values = (0..W * H).map(|i| if (36..56).contains(&(i % W)) { 0 } else { 255 }).collect();
            assert!(map.set_haze(Some(values)));
        }
        let mut r = renderer(&gpu, &map);
        r.set_distant(&sky);
        frame(&gpu, &mut r, &beam_look(haze))
    };
    let window = |f: &Frame| -> Vec<[Rgba; 4]> {
        (31..34).flat_map(|y| (40..50).map(move |x| (x, y))).map(|(x, y)| f.texels(x, y)).collect()
    };
    let plain = draw(false, 60, false);
    assert_eq!(window(&draw(true, 0, false)), window(&plain), "haze 0: the sky as with no beam");
    let hazy = draw(true, 60, false);
    for y in 31..34 {
        for x in 40..50 {
            assert!(hazy.light(x, y) > plain.light(x, y), "({x}, {y}): haze shows the beam");
        }
    }
    assert_eq!(window(&draw(true, 60, true)), window(&plain), "a haze map of 0 shows none");
}

#[test]
fn no_glow_beams_no_haze_changes_no_byte() {
    let Some(gpu) = offscreen() else { return };
    let mut look = beam_look(60);
    look.mode = olr_look::ViewMode::Lit;
    look.grade.exposure = 0;
    let draw = |empty: bool| {
        let mut map = level(|x, y| (x == 60 && y > 36).then_some(Material::Rock), false);
        if empty {
            assert!(map.set_glow_beams(Some(GlowBeamMap::none((W, H)))));
            assert!(map.set_haze(Some(vec![0; (W * H) as usize])));
        } else {
            assert!(map.set_glow_beams(None) && map.set_haze(None));
        }
        let mut r = renderer(&gpu, &map);
        let f = frame(&gpu, &mut r, &look);
        (f.img, gpu.read_light(&r).unwrap())
    };
    assert_eq!(draw(true), draw(false));
}
