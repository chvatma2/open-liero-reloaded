//! Glowing distant layers (issue #62): a layer's light is the level's,
//! baked at its reference placement into the back glow; its picture is lit
//! by its own glow where it is drawn, and feeds no bloom.

use olr_data::level::{DistantGlow, DistantLayer};

use super::*;

const NIGHT: Rgba = Rgba::rgb(40, 90, 230);
const GROUND: Rgba = Rgba::rgb(120, 80, 40);

/// 100 × 40 at art scale 1: rock from y 36 down, see-through air above
/// (back alpha 0, colour `back`), the air's top `sky` rows glowing in
/// group 1 (the bake's reference glow) when `sky` is above 0; `opaque`
/// makes the back opaque (an ordinary glowing sky element's bake).
fn sky_map(back: Rgba, sky: u32, opaque: bool) -> ColorMap {
    let (w, h) = (100, 40);
    let cells = (0..w * h).map(|i| if i / w >= 36 { Material::Rock } else { Material::Background }).collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap());
    let mut art = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let c = if y >= 36 {
                GROUND
            } else if opaque {
                back
            } else {
                Rgba { a: 0, ..back }
            };
            art.set(x, y, c);
        }
    }
    let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    let mut map = if opaque {
        ColorMap::new(&t, &art, &art, 1, theme)
    } else {
        ColorMap::new_see_through(&t, &art, &art, 1, theme)
    };
    if sky > 0 {
        let mut g = olr_terrain::GlowLayers::empty((w, h), (w, h));
        for i in 0..(sky * w) as usize {
            (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
        }
        assert!(map.set_glow(Some(g)));
    }
    map
}

/// A `w` × `h` layer of `c` at `at`, `depth`, glowing in group 1 at
/// `strength` (0: not at all) through `mask`.
fn glowing(
    w: u32,
    h: u32,
    c: Rgba,
    at: (i32, i32),
    depth: u8,
    strength: u8,
    mask: Option<Vec<u8>>,
) -> DistantLayer {
    let glow = (strength > 0).then_some(DistantGlow { group: 1, strength, mask });
    DistantLayer { image: solid(w, h, c), at, depth, glow }
}

/// A look in the dark (ambient 0.02) whose group 1 shows its art whole
/// (self-light scale 1) and casts `cast` with unlimited reach, its own
/// colour.
fn dark_look(cast: f32) -> olr_look::LookFrame {
    let mut look = dim(0.02);
    look.groups[0] = olr_look::FrameGroup {
        cast: [cast; 3],
        own_colour: true,
        self_scale: 1.0,
        reach: 1e9,
        ..Default::default()
    };
    look
}

fn renderer(gpu: &Offscreen, map: &ColorMap, layers: &[DistantLayer]) -> Renderer {
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.set_lighting(true);
    r.upload_terrain(map);
    r.set_distant(layers);
    r
}

/// A lit frame of the whole level with the camera at `camera`.
fn frame(gpu: &Offscreen, r: &mut Renderer, look: &olr_look::LookFrame, camera: (f32, f32)) -> RgbaImage {
    let mut l = list(View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 });
    l.map = (100, 40);
    l.parallax = Some(camera);
    gpu.render_lit_to_image(r, &l, Some(look), (200, 80))
}

/// A glowing layer shows its art in a dark level (a masked-out texel and a
/// layer that does not glow stay dark, lit by the ambient as before).
#[test]
fn a_glowing_distant_layer_shows_its_art_in_the_dark() {
    let Some(gpu) = offscreen() else { return };
    let map = sky_map(TILE, 0, false);
    // Left: glowing, its second column masked out; right: not glowing.
    let mut mask = vec![255; 10 * 10];
    (0..10).for_each(|y| mask[y * 10 + 1] = 0);
    let layers = [
        glowing(10, 10, NIGHT, (20, 10), 60, 100, Some(mask)),
        glowing(10, 10, NIGHT, (60, 10), 60, 0, None),
    ];
    let mut r = renderer(&gpu, &map, &layers);
    let look = dark_look(0.0);
    let img = frame(&gpu, &mut r, &look, (50.0, 20.0));
    let at = |x: u32, y: u32| img.get(2 * x, 2 * y);
    assert_eq!(at(20, 15), NIGHT, "a glowing texel shows its art");
    let dark = |p: Rgba| p.b < 60;
    assert!(dark(at(21, 15)), "masked out: {:?}", at(21, 15));
    assert!(dark(at(65, 15)), "not glowing: {:?}", at(65, 15));
    assert_eq!(at(21, 15), at(65, 15), "a masked-out texel is lit as one that does not glow");
    // A layer glowing at half strength shows half its art's light.
    r.set_distant(&[glowing(10, 10, NIGHT, (20, 10), 60, 50, None)]);
    let half = frame(&gpu, &mut r, &look, (50.0, 20.0)).get(40, 30);
    assert!(half.b > 100 && half.b < NIGHT.b, "half: {half:?}");
    // The layer not glowing draws as one made without a glow (today's).
    let plain = [DistantLayer { glow: None, ..layers[1].clone() }];
    r.set_distant(&plain);
    let a = frame(&gpu, &mut r, &look, (50.0, 20.0));
    r.set_distant(&[DistantLayer {
        glow: Some(DistantGlow { group: 0, strength: 80, mask: None }),
        ..plain[0].clone()
    }]);
    assert_eq!(frame(&gpu, &mut r, &look, (50.0, 20.0)), a, "group 0 glows nothing");
}

/// The light comes from the baked reference glow, once a frame for the
/// world: the light grid is the same wherever the camera is, while the
/// layer's drawn pixels move with it.
#[test]
fn a_distant_layers_light_stays_put_as_the_camera_pans() {
    let Some(gpu) = offscreen() else { return };
    let map = sky_map(NIGHT, 12, false);
    let layers = [glowing(100, 12, NIGHT, (0, 0), 50, 100, None)];
    let mut r = renderer(&gpu, &map, &layers);
    let look = dark_look(1.0);
    let a = frame(&gpu, &mut r, &look, (50.0, 20.0));
    let (grid_a, light_a) = gpu.read_light(&r).unwrap();
    let b = frame(&gpu, &mut r, &look, (70.0, 30.0));
    let (grid_b, light_b) = gpu.read_light(&r).unwrap();
    assert_eq!(grid_a.origin, grid_b.origin);
    assert_eq!(light_a, light_b, "the light stays put");
    // The air under the sky takes its blue light.
    let air =
        light_a[((20 - grid_a.origin.1) as u32 * grid_a.size.0 + (50 - grid_a.origin.0) as u32) as usize];
    assert!(air[2] > 0.2 && air[2] > air[0], "{air:?}");
    // The layer moved: 10 world pixels right and 5 down at depth 50.
    let first = |img: &RgbaImage, x: u32| (0..80).find(|&y| img.get(2 * x, y) != NIGHT);
    assert_eq!(first(&a, 50), Some(24), "the sky's bottom edge in the first frame");
    assert_eq!(first(&b, 50), Some(34), "5 world pixels lower in the second");
    // A view panned with its camera: the light of every world pixel both
    // see is the same (`light_is_camera_independent` holds).
    let mut panned = |x: f32| {
        let mut l = list(View { x, y: 0.0, w: 60.0, h: 40.0 });
        l.map = (100, 40);
        gpu.render_lit_to_image(&mut r, &l, Some(&look), (120, 80));
        light_at_each(&gpu, &r)
    };
    let (left, right) = (panned(10.0), panned(30.0));
    for x in 30..70 {
        for y in [2, 12, 20, 35, 38] {
            assert_eq!(left(x, y), right(x, y), "({x}, {y})");
        }
    }
}

/// The last lit frame's light, by world pixel.
fn light_at_each(gpu: &Offscreen, r: &Renderer) -> impl Fn(i32, i32) -> [f32; 3] + use<> {
    let (grid, light) = gpu.read_light(r).unwrap();
    move |x, y| {
        let (i, j) = ((x - grid.origin.0) as u32, (y - grid.origin.1) as u32);
        light[(j * grid.size.0 + i) as usize]
    }
}

/// A glowing layer's drawn pixels feed no bloom, nor any light: the light
/// grid and the level's own pixels are the same with it glowing or not, and
/// no bloom lands on it.
#[test]
fn a_glowing_layers_drawn_pixels_feed_no_bloom() {
    let Some(gpu) = offscreen() else { return };
    let map = sky_map(TILE, 0, false);
    let look = {
        let mut l = dark_look(3.0);
        l.max_light = 2.0;
        l
    };
    let picture = |strength: u8, bloom: bool| {
        let mut r =
            renderer(&gpu, &map, &[glowing(30, 20, Rgba::rgb(255, 255, 255), (35, 8), 30, strength, None)]);
        r.set_bloom(bloom);
        let img = frame(&gpu, &mut r, &look, (50.0, 20.0));
        (img, gpu.read_light(&r).unwrap().1)
    };
    let (lit, lit_light) = picture(100, true);
    let (plain, plain_light) = picture(0, true);
    assert_eq!(lit_light, plain_light, "the drawn layer casts no light");
    // The ground beside and under it: the same bytes.
    for (x, y) in [(10, 37), (50, 38), (90, 37), (20, 20)] {
        assert_eq!(lit.get(2 * x, 2 * y), plain.get(2 * x, 2 * y), "({x}, {y})");
    }
    // On the glowing layer no bloom lands.
    let (off, _) = picture(100, false);
    assert_eq!(lit.get(2 * 50, 2 * 18), off.get(2 * 50, 2 * 18));
}

/// The light test (issue #62): a see-through sky whose back glow the bake
/// wrote from a glowing distant layer (its colour at alpha 0) lights the
/// level exactly as an opaque glowing sky of the same art and glow
/// (`olr-edit`'s `a_distant_sky_bakes_the_glow_an_ordinary_sky_does`
/// shows the bake writes the same maps).
#[test]
fn a_see_through_glow_lights_as_an_opaque_one() {
    let Some(gpu) = offscreen() else { return };
    let look = dark_look(1.0);
    let light = |map: ColorMap, layers: &[DistantLayer]| {
        let mut r = renderer(&gpu, &map, layers);
        frame(&gpu, &mut r, &look, (50.0, 20.0));
        gpu.read_light(&r).unwrap()
    };
    let opaque = light(sky_map(NIGHT, 12, true), &[]);
    let distant = light(sky_map(NIGHT, 12, false), &[glowing(100, 12, NIGHT, (0, 0), 70, 100, None)]);
    assert_eq!(opaque, distant);
}
