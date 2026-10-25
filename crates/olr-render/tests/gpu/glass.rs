//! The background glass design: glass (a see-through level's back pixels
//! of alpha 1 to 254) shows the distant layers through it, takes their
//! light rather than the level's, shows no glow of its own while its glow
//! casts, filters by the level's glass map and frosts fixed to the world;
//! a `Filtered` group casts its colour by each pixel's art.

use olr_data::level::DistantLayer;
use olr_look::{FrameGroup, LookFrame, ViewMode};
use olr_render::HeatDeposit;
use olr_terrain::{GlassMap, GlowLayers};

use super::*;

const PANE: Rgba = Rgba { r: 200, g: 215, b: 235, a: 89 };
const WALL: Rgba = Rgba::rgb(90, 90, 90);
const GROUND: Rgba = Rgba::rgb(120, 80, 40);
const W: u32 = 100;
const H: u32 = 40;

/// Whether world pixel `(x, y)` is glass: a strip at x 40-59, y 10-19.
fn glassy(x: u32, y: u32) -> bool {
    (40..60).contains(&x) && (10..20).contains(&y)
}

/// 100 × 40 at art scale 1: rock from y 36, see-through air above but for
/// the glass strip and an opaque wall at x 30-34, y 10-19.
fn window() -> ColorMap {
    pane(PANE)
}

/// [`window`] with its strip of `pane`.
fn pane(pane: Rgba) -> ColorMap {
    let cells = (0..W * H).map(|i| if i / W >= 36 { Material::Rock } else { Material::Background }).collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(W, H, cells).unwrap());
    let mut art = RgbaImage::new(W, H);
    for y in 0..H {
        for x in 0..W {
            let c = if y >= 36 {
                GROUND
            } else if glassy(x, y) {
                pane
            } else if (30..35).contains(&x) && (10..20).contains(&y) {
                WALL
            } else {
                Rgba { a: 0, ..TILE }
            };
            art.set(x, y, c);
        }
    }
    let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    ColorMap::new_see_through(&t, &art, &art, 1, theme)
}

/// The whole level as one distant layer of `c` at depth 90.
fn sky(c: Rgba) -> DistantLayer {
    DistantLayer { image: solid(W, 36, c), at: (0, 0), depth: 90, glow: None }
}

fn renderer(gpu: &Offscreen, map: &ColorMap, layer: DistantLayer) -> Renderer {
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.set_lighting(true);
    r.set_bloom(false);
    r.upload_terrain(map);
    r.set_distant(&[layer]);
    r
}

fn frame(gpu: &Offscreen, r: &mut Renderer, look: &LookFrame, camera: (f32, f32)) -> RgbaImage {
    let mut l = list(View { x: 0.0, y: 0.0, w: W as f32, h: H as f32 });
    l.map = (W, H);
    l.parallax = Some(camera);
    gpu.render_lit_to_image(r, &l, Some(look), (2 * W, 2 * H))
}

/// A dark look (ambient 0.02); group 1 shows its art and casts `cast`.
fn dark(cast: f32) -> LookFrame {
    let mut look = dim(0.02);
    look.groups[0] =
        FrameGroup { cast: [cast; 3], own_colour: true, self_scale: 1.0, reach: 1e9, ..Default::default() };
    look
}

fn full_bright() -> LookFrame {
    let mut look = dark(0.0);
    look.mode = ViewMode::FullBright;
    look
}

/// World pixel `(x, y)` of a frame.
fn at(img: &RgbaImage, x: u32, y: u32) -> Rgba {
    img.get(2 * x, 2 * y)
}

/// `src` at alpha `a` over opaque `dst`, rounded as the blend does.
fn over(src: [f32; 3], a: f32, dst: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| src[i] * a + dst[i] * (1.0 - a))
}

fn near(c: Rgba, want: [f32; 3], tolerance: f32) -> bool {
    [c.r, c.g, c.b].iter().zip(want).all(|(v, w)| (f32::from(*v) - w).abs() <= tolerance)
}

fn rgb(c: Rgba) -> [f32; 3] {
    [c.r, c.g, c.b].map(f32::from)
}

#[test]
fn glass_shows_the_layer_through_it() {
    let Some(gpu) = offscreen() else { return };
    let mut r = renderer(&gpu, &window(), sky(Rgba::rgb(255, 255, 255)));
    let img = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    let want = over(rgb(PANE), 89.0 / 255.0, [255.0; 3]);
    assert!(near(at(&img, 50, 15), want, 1.5), "{:?} vs {want:?}", at(&img, 50, 15));
}

#[test]
fn glass_takes_the_layers_light_not_the_rooms() {
    let Some(gpu) = offscreen() else { return };
    // A dim lamp: nothing saturates. The same pane opaque is lit as the
    // level; as glass, by the night sky's tenth.
    let off = dark(0.0);
    let mut on = dark(0.0);
    on.push_emitter(olr_look::FrameLight { pos: [45.0, 25.0], radius: 3.0, colour: [0.4; 3], reach: 40.0 });
    let gain = |map: &ColorMap| {
        let mut r = renderer(&gpu, map, sky(Rgba::rgb(255, 255, 255)));
        let (a, b) = (frame(&gpu, &mut r, &off, (50.0, 20.0)), frame(&gpu, &mut r, &on, (50.0, 20.0)));
        f32::from(at(&b, 45, 18).g) - f32::from(at(&a, 45, 18).g)
    };
    let (opaque, glass) = (gain(&pane(Rgba { a: 255, ..PANE })), gain(&window()));
    assert!(opaque > 20.0, "the lamp lights the opaque pane: {opaque}");
    // A tenth of the light is about a third in sRGB bytes; lit as the
    // level, the paler glass would gain more than the pane.
    assert!(glass * 2.0 < opaque, "glass takes the layer's tenth: {glass} vs opaque {opaque}");
}

#[test]
fn glass_shows_no_glow_but_casts() {
    let Some(gpu) = offscreen() else { return };
    let mut map = window();
    let mut g = GlowLayers::empty((W, H), (W, H));
    for y in 10..20 {
        for x in 30..60 {
            let i = (y * W + x) as usize;
            (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
        }
    }
    assert!(map.set_glow(Some(g)));
    let mut r = renderer(&gpu, &map, sky(Rgba::rgb(20, 20, 40)));
    let img = frame(&gpu, &mut r, &dark(1.0), (50.0, 20.0));
    let below = light_at(&gpu, &r, 50, 25);
    let mut plain = renderer(&gpu, &window(), sky(Rgba::rgb(20, 20, 40)));
    let dim = frame(&gpu, &mut plain, &dark(1.0), (50.0, 20.0));
    let unlit = light_at(&gpu, &plain, 50, 25);
    // The wall glows: its art whole. The glass shows what it shows
    // unglowing, but for the tenth of the light the layer behind it takes.
    assert!(near(at(&img, 32, 15), rgb(WALL), 2.0), "the wall shows its art: {:?}", at(&img, 32, 15));
    // Glowing, it would show its art over the sky, (83, 88, 108); it takes
    // only the sky's tenth of the glow light round it.
    let (g, d) = (at(&img, 50, 15), at(&dim, 50, 15));
    assert!(g.b < 60, "glass shows no glow of its own: {g:?} vs {d:?}");
    // Its glow still casts: the air below it is lit.
    assert!(below[1] > unlit[1] + 0.05, "{below:?} vs {unlit:?}");
}

#[test]
fn hot_glass_still_glows() {
    let Some(gpu) = offscreen() else { return };
    let map = window();
    let stone = olr_look::SurfaceMaterial {
        id: "stone".into(),
        heat: Some(olr_look::Heat {
            sensitivity: 100,
            capacity: 100,
            conductivity: 100,
            cooling: 100,
            ramp: vec![],
            light: 100,
        }),
        light_soak: None,
    };
    let mut maps = olr_look::SurfaceMaps::empty((W, H));
    for y in 10..20 {
        for x in 40..60 {
            maps.back[(y * W + x) as usize] = 1;
        }
    }
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.set_lighting(true);
    r.set_bloom(false);
    r.set_surface(&[stone], Some(&maps));
    r.upload_terrain(&map);
    r.set_distant(&[sky(Rgba::rgb(20, 20, 40))]);
    let look = dark(0.0);
    let cold = frame(&gpu, &mut r, &look, (50.0, 20.0));
    assert!(r.has_heat());
    // The back lies 14 px behind the plane: a ball of 40 reaches it.
    r.deposit_heat(HeatDeposit { pos: [50.0, 15.0], radius: 40.0, amount: 1000.0 });
    let hot = frame(&gpu, &mut r, &look, (50.0, 20.0));
    let (c, h) = (at(&cold, 50, 15), at(&hot, 50, 15));
    assert!(u32::from(h.r) > u32::from(c.r) + 30, "hot glass glows: {h:?} vs {c:?}");
}

#[test]
fn tint_multiplies_what_shows_through() {
    let Some(gpu) = offscreen() else { return };
    let mut map = window();
    let mut glass = GlassMap::none((W, H));
    for y in 10..20 {
        for x in 40..60 {
            let i = ((y * W + x) * 4) as usize;
            glass.texels[i..i + 4].copy_from_slice(&[235, 165, 50, 0]);
        }
    }
    assert!(map.set_glass(Some(glass)));
    let a = 89.0 / 255.0;
    let mut r = renderer(&gpu, &map, sky(Rgba::rgb(255, 255, 255)));
    let img = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    let want = over(rgb(PANE), a, [235.0, 165.0, 50.0]);
    assert!(near(at(&img, 50, 15), want, 1.5), "{:?} vs {want:?}", at(&img, 50, 15));
    // Over a blue night sky amber glass goes dark: red and green fall.
    let blue = Rgba::rgb(30, 60, 140);
    let mut r = renderer(&gpu, &map, sky(blue));
    let img = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    let filtered = [30.0 * 235.0 / 255.0, 60.0 * 165.0 / 255.0, 140.0 * 50.0 / 255.0];
    let want = over(rgb(PANE), a, filtered);
    assert!(near(at(&img, 50, 15), want, 1.5), "{:?} vs {want:?}", at(&img, 50, 15));
    // Where the map filters nothing the glass is clear.
    let mut clear = window();
    assert!(clear.set_glass(Some(GlassMap::none((W, H)))));
    let mut r = renderer(&gpu, &clear, sky(blue));
    let img = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    assert!(near(at(&img, 50, 15), over(rgb(PANE), a, rgb(blue)), 1.5));
}

#[test]
fn frost_is_fixed_to_the_world() {
    let Some(gpu) = offscreen() else { return };
    let mut stripes = solid(W, 36, Rgba::rgb(255, 255, 255));
    for y in 0..36 {
        for x in (0..W).step_by(2) {
            stripes.set(x, y, Rgba::rgb(0, 0, 0));
        }
    }
    let layer = DistantLayer { image: stripes, at: (0, 0), depth: 90, glow: None };
    let frosted = |frost: u8| {
        let mut map = window();
        let mut glass = GlassMap::none((W, H));
        for y in 10..20 {
            for x in 40..60 {
                glass.texels[((y * W + x) * 4 + 3) as usize] = frost;
            }
        }
        assert!(map.set_glass(Some(glass)));
        map
    };
    let mut r = renderer(&gpu, &frosted(15), layer.clone());
    let a = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    assert_eq!(frame(&gpu, &mut r, &full_bright(), (50.0, 20.0)), a, "the same frame each time");
    let b = frame(&gpu, &mut r, &full_bright(), (54.0, 20.0));
    assert_eq!(frame(&gpu, &mut r, &full_bright(), (54.0, 20.0)), b);
    let mut clear = renderer(&gpu, &frosted(0), layer);
    let c = frame(&gpu, &mut clear, &full_bright(), (50.0, 20.0));
    let glass_pixels = |img: &RgbaImage| {
        (10..20).flat_map(|y| (40..60).map(move |x| (x, y))).map(|(x, y)| at(img, x, y)).collect::<Vec<_>>()
    };
    assert_ne!(glass_pixels(&a), glass_pixels(&c), "frost moves what shows through");
    // Away from the glass nothing changes.
    assert_eq!(at(&a, 80, 15), at(&c, 80, 15));
    // Frost 15 shows a layer texel at most 1.5 world px (art scale 1) from
    // the one behind: a layer whose red is twice its column, read back
    // from under the pane, at two cameras.
    let mut ramp = solid(W, 36, Rgba::rgb(0, 0, 0));
    for y in 0..36 {
        for x in 0..W {
            ramp.set(x, y, Rgba::rgb((2 * x) as u8, 0, 0));
        }
    }
    let layer = DistantLayer { image: ramp, at: (0, 0), depth: 90, glow: None };
    let alpha = 89.0 / 255.0;
    let column = |c: Rgba| (f32::from(c.r) - f32::from(PANE.r) * alpha) / (1.0 - alpha) / 2.0;
    for camera in [(50.0, 20.0), (54.0, 20.0)] {
        let mut r = renderer(&gpu, &frosted(15), layer.clone());
        let frost = frame(&gpu, &mut r, &full_bright(), camera);
        let mut r = renderer(&gpu, &frosted(0), layer.clone());
        let clear = frame(&gpu, &mut r, &full_bright(), camera);
        let mut moved = 0;
        for (x, y) in (10..20).flat_map(|y| (40..60).map(move |x| (x, y))) {
            let d = column(at(&frost, x, y)) - column(at(&clear, x, y));
            assert!(d.abs() <= 1.5 + 0.6, "({x}, {y}) at {camera:?}: {d}");
            moved += usize::from(d.abs() > 0.5);
        }
        assert!(moved > 50, "most pixels scatter: {moved}");
    }
}

#[test]
fn a_filtered_group_lights_by_each_pixels_colour() {
    let Some(gpu) = offscreen() else { return };
    let cells = vec![Material::Background; (W * H) as usize];
    let t = Terrain::from_grid(&MaterialGrid::from_cells(W, H, cells).unwrap());
    let mut art = solid(W, H, Rgba::rgb(0, 0, 0));
    art.set(30, 20, Rgba::rgb(255, 0, 0));
    art.set(69, 20, Rgba::rgb(128, 0, 0));
    let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    let mut map = ColorMap::new(&t, &art, &art, 1, theme);
    let mut g = GlowLayers::empty((W, H), (W, H));
    for x in [30, 69] {
        let i = (20 * W + x) as usize;
        (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
    }
    assert!(map.set_glow(Some(g)));
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.set_lighting(true);
    r.set_bloom(false);
    r.upload_terrain(&map);
    let mut look = dark(0.0);
    look.groups[0] = FrameGroup {
        cast: [4.0; 3],
        own_colour: true,
        filtered: true,
        self_scale: 1.0,
        reach: 1e9,
        ..Default::default()
    };
    let mut l = list(View { x: 0.0, y: 0.0, w: W as f32, h: H as f32 });
    l.map = (W, H);
    gpu.render_lit_to_image(&mut r, &l, Some(&look), (2 * W, 2 * H));
    // Mirrored about the level's middle: each lights the other alike.
    let (red, dark_red) = (light_at(&gpu, &r, 33, 20), light_at(&gpu, &r, 66, 20));
    assert!(red[0] > 0.05 && red[1] < red[0] * 0.05, "red light: {red:?}");
    assert!((red[0] - dark_red[0]).abs() < red[0] * 0.02, "normalised: {red:?} vs {dark_red:?}");
}

#[test]
fn no_glass_map_changes_no_byte() {
    let Some(gpu) = offscreen() else { return };
    let mut r = renderer(&gpu, &window(), sky(Rgba::rgb(40, 90, 230)));
    let a = frame(&gpu, &mut r, &dark(0.0), (50.0, 20.0));
    let mut map = window();
    assert!(map.set_glass(Some(GlassMap::none((W, H)))));
    let mut r = renderer(&gpu, &map, sky(Rgba::rgb(40, 90, 230)));
    assert_eq!(frame(&gpu, &mut r, &dark(0.0), (50.0, 20.0)), a);
}

/// Review: a glass pixel at a view's edge frosts as it does inside
/// another view (the frosted fetch is clamped to the layer, not to the
/// part of it the view draws): split screen and panning show one pattern.
#[test]
fn frost_is_the_same_at_a_views_edge() {
    let Some(gpu) = offscreen() else { return };
    let mut ramp = solid(W, 36, Rgba::rgb(0, 0, 0));
    for y in 0..36 {
        for x in 0..W {
            ramp.set(x, y, Rgba::rgb((2 * x) as u8, 0, 0));
        }
    }
    let layer = DistantLayer { image: ramp, at: (0, 0), depth: 90, glow: None };
    let frosted = |frost: u8| {
        let mut map = window();
        let mut glass = GlassMap::none((W, H));
        for y in 10..20 {
            for x in 40..60 {
                glass.texels[((y * W + x) * 4 + 3) as usize] = frost;
            }
        }
        assert!(map.set_glass(Some(glass)));
        map
    };
    // The same camera (the layer placed alike), two views: the whole
    // level, and its left half, whose edge cuts the glass at x 50.
    let draw = |frost: u8, w: f32| {
        let mut r = renderer(&gpu, &frosted(frost), layer.clone());
        let mut l = list(View { x: 0.0, y: 0.0, w, h: H as f32 });
        l.map = (W, H);
        l.parallax = Some((50.0, 20.0));
        gpu.render_lit_to_image(&mut r, &l, Some(&full_bright()), (2 * w as u32, 2 * H))
    };
    let (whole, half) = ((draw(40, W as f32), draw(0, W as f32)), (draw(40, 50.0), draw(0, 50.0)));
    let red = |img: &RgbaImage, x, y| i32::from(at(img, x, y).r);
    let mut edge = 0;
    for (x, y) in (10..20).flat_map(|y| (44..50).map(move |x| (x, y))) {
        let (dw, dh) = (red(&whole.0, x, y) - red(&whole.1, x, y), red(&half.0, x, y) - red(&half.1, x, y));
        assert!((dw - dh).abs() <= 1, "({x}, {y}): {dw} in the whole view, {dh} at the half's edge");
        edge += usize::from(dw > 4);
    }
    assert!(edge > 5, "frost reaches past the edge somewhere: {edge}");
}

/// Review: a see-through level without a glass map binds a 1 × 1 glass
/// texture (no art-size texture for nothing); a map set later (the
/// editor's canvas) is uploaded at art size by the next `update_terrain`,
/// dirty rectangles or none, and shows; taken away, the texture shrinks
/// and the picture is the plain one again.
#[test]
fn a_glass_map_set_later_is_uploaded_and_none_costs_nothing() {
    let Some(gpu) = offscreen() else { return };
    let blue = Rgba::rgb(30, 60, 140);
    let mut map = window();
    let mut r = renderer(&gpu, &map, sky(blue));
    assert_eq!(r.glass_texture_size(), Some((1, 1)));
    let plain = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    let mut glass = GlassMap::none((W, H));
    for y in 10..20 {
        for x in 40..60 {
            let i = ((y * W + x) * 4) as usize;
            glass.texels[i..i + 4].copy_from_slice(&[235, 165, 50, 0]);
        }
    }
    assert!(map.set_glass(Some(glass)));
    r.update_terrain(&mut map);
    assert_eq!(r.glass_texture_size(), Some((W, H)));
    let tinted = frame(&gpu, &mut r, &full_bright(), (50.0, 20.0));
    let a = 89.0 / 255.0;
    let want = over(rgb(PANE), a, [30.0 * 235.0 / 255.0, 60.0 * 165.0 / 255.0, 140.0 * 50.0 / 255.0]);
    assert!(near(at(&tinted, 50, 15), want, 1.5), "{:?} vs {want:?}", at(&tinted, 50, 15));
    assert!(map.set_glass(None));
    r.update_terrain(&mut map);
    assert_eq!(r.glass_texture_size(), Some((1, 1)));
    assert_eq!(frame(&gpu, &mut r, &full_bright(), (50.0, 20.0)), plain);
}
