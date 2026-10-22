//! The worm halo (issue #54): a soft dark outline on the background round a
//! living worm, wider and fainter the farther back the background lies.

use olr_look::engine::{self, WORM_HALO_RADIUS, WORM_HALO_STRENGTH};

use super::*;

/// A renderer for the halo's frames: lit, no bloom (the halo's bytes
/// alone), integer nearest, `map` and its distant `layers` uploaded.
fn halo_renderer(gpu: &Offscreen, map: &ColorMap, layers: &[olr_data::level::DistantLayer]) -> Renderer {
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.upload_images(&[white_image()]);
    r.set_lighting(true);
    r.set_bloom(false);
    r.upload_terrain(map);
    r.set_distant(layers);
    r
}

/// The whole level's list with `bodies` as living worms (occluders and
/// white `Shade::WORM` sprites); `more` adds items.
fn halo_list(map: &ColorMap, bodies: &[Body], more: impl FnOnce(&mut DrawList)) -> DrawList {
    let (w, h) = (map.material().width(), map.material().height());
    let mut l = list(View { x: 0.0, y: 0.0, w: w as f32, h: h as f32 });
    l.map = (w, h);
    push_bodies(&mut l, bodies);
    push_worm_sprites(&mut l, bodies);
    more(&mut l);
    l
}

/// The look of the halo's frames: full white ambient, so a background
/// texel's light is 1 and the halo's share shows whole.
fn halo_look() -> olr_look::LookFrame {
    let mut look = dim(1.0);
    look.worm_floor = [0.2; 3];
    look
}

/// A frame of [`halo_list`] at two output pixels a world pixel: world
/// pixel (x, y) is output pixels (2x..2x + 2, 2y..2y + 2).
fn halo_frame(
    gpu: &Offscreen,
    r: &mut Renderer,
    map: &ColorMap,
    bodies: &[Body],
    more: impl FnOnce(&mut DrawList),
) -> RgbaImage {
    let (w, h) = (map.material().width(), map.material().height());
    gpu.render_lit_to_image(r, &halo_list(map, bodies, more), Some(&halo_look()), (2 * w, 2 * h))
}

/// World pixel (x, y)'s colour: the same in each of its four texels (the
/// halo is never finer than the art).
fn world_px(img: &RgbaImage, x: u32, y: u32) -> Rgba {
    let c = img.get(2 * x, 2 * y);
    for (dx, dy) in [(1, 0), (0, 1), (1, 1)] {
        assert_eq!(img.get(2 * x + dx, 2 * y + dy), c, "({x}, {y}): its texels differ");
    }
    c
}

/// The distance from world pixel (x, y)'s centre to the nearest pixel of
/// `bodies`, centre to centre (0 inside one).
fn worm_distance(bodies: &[Body], x: u32, y: u32) -> f32 {
    let mut best = f32::INFINITY;
    for &([bx, by, bw, bh], _) in bodies {
        for qy in by as i32..(by + bh) as i32 {
            for qx in bx as i32..(bx + bw) as i32 {
                let (dx, dy) = ((x as i32 - qx) as f32, (y as i32 - qy) as f32);
                best = best.min((dx * dx + dy * dy).sqrt());
            }
        }
    }
    best
}

/// The red byte of a background texel of red `albedo` under light 1, `d`
/// from the nearest worm pixel, `depth` back (`engine::worm_halo`).
fn halo_want(albedo: u8, d: f32, depth: f32) -> u8 {
    let light = 1.0 - engine::worm_halo(d, depth);
    olr_render::shade_texel([albedo; 4], [light; 3], 0.0, false, &Default::default(), 1.0)[0]
}

/// A distant layer's depth as the composite reads it: `1 − shade alpha`,
/// the alpha a byte.
fn stored_depth(depth: u8) -> f32 {
    1.0 - ((1.0 - f32::from(depth) / 100.0) * 255.0).round() / 255.0
}

/// Checks every world pixel of `img` within `reach` of `bodies` against
/// [`halo_want`] at `depth` (albedo red `albedo`); a worm's own pixels
/// against `off` (the halo never falls on the worm).
fn check_halo(img: &RgbaImage, off: &RgbaImage, bodies: &[Body], albedo: u8, depth: f32, reach: u32) {
    let (w, h) = (img.width() / 2, img.height() / 2);
    for y in 0..h {
        for x in 0..w {
            let d = worm_distance(bodies, x, y);
            if d > reach as f32 {
                continue;
            }
            let got = world_px(img, x, y);
            if d == 0.0 {
                assert_eq!(got, world_px(off, x, y), "({x}, {y}): a worm pixel changed");
                continue;
            }
            let want = halo_want(albedo, d, depth);
            assert!(got.r.abs_diff(want) <= 1, "({x}, {y}) at {d}, depth {depth}: {} want {want}", got.r);
        }
    }
}

/// How many pixels right of a worm's right edge `x1` (its last column + 1)
/// along row `y` `img` shows darker than `off`, and the first one's red.
fn halo_reach(img: &RgbaImage, off: &RgbaImage, x1: u32, y: u32) -> (u32, u8) {
    let reach = (x1..img.width() / 2).take_while(|&x| world_px(img, x, y).r < world_px(off, x, y).r).count();
    (reach as u32, world_px(img, x1, y).r)
}

/// A white room of `w` × `h` world pixels, all open air but `rock`.
fn white_room(w: u32, h: u32, rock: impl Fn(u32, u32) -> bool) -> ColorMap {
    painted_room(w, h, rock, |_, _| Rgba::rgb(255, 255, 255))
}

/// A see-through level of `w` × `h` world pixels at art scale 1 whose
/// back tile is white (its opaque backing white too).
fn white_see_through(w: u32, h: u32) -> ColorMap {
    let t = Terrain::from_grid(&MaterialGrid::new(w, h, Material::Background));
    let back = solid(w, h, Rgba { a: 0, ..Rgba::rgb(255, 255, 255) });
    let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    ColorMap::new_see_through(&t, &back, &back, 1, theme)
}

const WORM: [Body; 1] = [([20.0, 12.0, 4.0, 6.0], 1)];

/// On the level's back (depth 0): right next to the silhouette the light
/// is `1 − WORM_HALO_STRENGTH`, `WORM_HALO_RADIUS` pixels past it it is
/// whole, the profile between is `engine::worm_halo`'s, and the worm's own
/// pixels are unchanged.
#[test]
fn the_halo_darkens_the_back_round_a_worm() {
    let Some(gpu) = offscreen() else { return };
    let map = white_room(50, 30, |_, _| false);
    let mut r = halo_renderer(&gpu, &map, &[]);
    let img = halo_frame(&gpu, &mut r, &map, &WORM, |_| {});
    r.set_worm_halo(false);
    let off = halo_frame(&gpu, &mut r, &map, &WORM, |_| {});
    assert_eq!(world_px(&off, 24, 15).r, 255, "no halo: the back under full light");
    let next = olr_render::shade_texel(
        [255; 4],
        [1.0 - WORM_HALO_STRENGTH; 3],
        0.0,
        false,
        &Default::default(),
        1.0,
    );
    for (x, y) in [(24, 15), (19, 15), (22, 18), (22, 11)] {
        assert!(
            world_px(&img, x, y).r.abs_diff(next[0]) <= 1,
            "({x}, {y}) next to it: {:?}",
            world_px(&img, x, y)
        );
    }
    let past = 23 + WORM_HALO_RADIUS as u32 + 1;
    assert_eq!(world_px(&img, past, 15), world_px(&off, past, 15), "{past}: r pixels past the silhouette");
    assert!(world_px(&img, past - 1, 15).r < 255, "the halo reaches r pixels");
    check_halo(&img, &off, &WORM, 255, 0.0, 8);
}

/// The same worm over distant layers at depths 20 and 50: the halo is
/// wider and fainter at each step; at the fade depth or deeper (60, 90:
/// a night sky) no byte changes.
#[test]
fn a_deeper_layer_takes_a_wider_fainter_halo() {
    let Some(gpu) = offscreen() else { return };
    let map = white_see_through(60, 30);
    let white = Rgba::rgb(255, 255, 255);
    let frames = |layers: &[olr_data::level::DistantLayer]| {
        let mut r = halo_renderer(&gpu, &map, layers);
        let img = halo_frame(&gpu, &mut r, &map, &WORM, |_| {});
        r.set_worm_halo(false);
        (img, halo_frame(&gpu, &mut r, &map, &WORM, |_| {}))
    };
    // Depth 0: the opaque backing, no layer.
    let (img, off) = frames(&[]);
    check_halo(&img, &off, &WORM, 255, 0.0, 8);
    let mut last = halo_reach(&img, &off, 24, 15);
    assert_eq!(last.0, WORM_HALO_RADIUS as u32, "the back's halo");
    for depth in [20, 50] {
        let (img, off) = frames(&[layer(60, 30, white, (0, 0), depth)]);
        assert_eq!(world_px(&off, 30, 15), white, "depth {depth}: the layer covers the level");
        check_halo(&img, &off, &WORM, 255, stored_depth(depth), 8);
        let now = halo_reach(&img, &off, 24, 15);
        assert!(now.0 > last.0, "depth {depth}: {now:?} wider than {last:?}");
        assert!(now.1 > last.1, "depth {depth}: {now:?} fainter than {last:?}");
        last = now;
    }
    for depth in [60, 90] {
        let (img, off) = frames(&[layer(60, 30, white, (0, 0), depth)]);
        assert!(img == off, "depth {depth}: no halo");
    }
}

/// Rock beside the worm, a glowing back pixel and an opaque projectile
/// over the back within the halo's reach keep their bytes; the back
/// round them takes the halo.
#[test]
fn the_halo_spares_rock_glow_and_sprites() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(50, 30, |x, y| x == 24 && (10..20).contains(&y), |x, y| (x, y) == (22, 18));
    let projectile = |l: &mut DrawList| {
        l.sprite(
            Layer::Projectiles,
            Sprite {
                atlas: AtlasId(0),
                src: Rect { x: 0, y: 0, w: 1, h: 1 },
                pos: [19.0, 15.0],
                size: [1.0, 1.0],
                flip: false,
                tint: Rgba::rgb(255, 255, 255),
                shade: olr_scene::Shade::LIT,
            },
        );
    };
    // At ambient 0.3 the glowing pixel (self-light 0.5) shows brighter.
    let mut look = halo_look();
    look.ambient = [0.3; 3];
    let mut r = halo_renderer(&gpu, &map, &[]);
    let frame = |r: &mut Renderer| {
        gpu.render_lit_to_image(r, &halo_list(&map, &WORM, projectile), Some(&look), (100, 60))
    };
    let img = frame(&mut r);
    r.set_worm_halo(false);
    let off = frame(&mut r);
    for (what, x, y) in [("rock", 24, 15), ("rock", 24, 12), ("glow", 22, 18), ("projectile", 19, 15)] {
        assert_eq!(world_px(&img, x, y), world_px(&off, x, y), "{what} at ({x}, {y})");
    }
    assert!(world_px(&off, 22, 18) != world_px(&off, 21, 18), "the glowing pixel glows");
    for (x, y) in [(19, 14), (21, 18), (22, 11)] {
        assert!(world_px(&img, x, y).r < world_px(&off, x, y).r, "the back at ({x}, {y}) takes the halo");
    }
}

/// Two worms side by side, a two-pixel gap between them: no texel is
/// darker than one halo's strongest (the halos never add), and the gap's
/// pixels take the nearer worm's.
#[test]
fn overlapping_halos_take_the_strongest() {
    let Some(gpu) = offscreen() else { return };
    let map = white_room(50, 30, |_, _| false);
    let bodies = [([20.0, 12.0, 4.0, 6.0], 1), ([26.0, 12.0, 4.0, 6.0], 2)];
    let mut r = halo_renderer(&gpu, &map, &[]);
    let img = halo_frame(&gpu, &mut r, &map, &bodies, |_| {});
    r.set_worm_halo(false);
    let off = halo_frame(&gpu, &mut r, &map, &bodies, |_| {});
    let strongest = halo_want(255, 1.0, 0.0);
    for y in 0..30 {
        for x in 0..50 {
            if worm_distance(&bodies, x, y) > 0.0 {
                let v = world_px(&img, x, y).r;
                assert!(
                    v.saturating_add(1) >= strongest,
                    "({x}, {y}): {v} darker than one halo's {strongest}"
                );
            }
        }
    }
    for x in [24, 25] {
        assert!(world_px(&img, x, 15).r.abs_diff(strongest) <= 1, "the gap at {x}");
    }
    check_halo(&img, &off, &bodies, 255, 0.0, 8);
}

/// A still worm's halo keeps its bytes frame after frame, and follows the
/// worm the frame it moves.
#[test]
fn the_halo_is_steady() {
    let Some(gpu) = offscreen() else { return };
    let map = white_room(50, 30, |_, _| false);
    let mut r = halo_renderer(&gpu, &map, &[]);
    let first = halo_frame(&gpu, &mut r, &map, &WORM, |_| {});
    for frame in 1..4 {
        assert!(halo_frame(&gpu, &mut r, &map, &WORM, |_| {}) == first, "frame {frame} changed");
    }
    let moved = [([24.0, 13.0, 4.0, 6.0], 1)];
    let after = halo_frame(&gpu, &mut r, &map, &moved, |_| {});
    let mut fresh = halo_renderer(&gpu, &map, &[]);
    assert!(after == halo_frame(&gpu, &mut fresh, &map, &moved, |_| {}), "the halo is where the worm is");
    assert!(after != first, "the worm moved");
    assert!(world_px(&after, 28, 15).r < 255 && world_px(&after, 22, 15).r < 255);
    assert_eq!(world_px(&after, 20, 15).r, 255, "the old place is clear");
}

/// A worm just outside the first of two views (`render_views`) halos that
/// view's edge exactly as a view wide enough for both shows it; the
/// second view, holding the worm, shows its halo too.
#[test]
fn an_off_view_worm_halos_the_view_edge() {
    let Some(gpu) = offscreen() else { return };
    let map = white_room(120, 40, |_, _| false);
    let bodies = [([61.0, 12.0, 4.0, 6.0], 1)];
    let look = halo_look();
    let mut wide = halo_renderer(&gpu, &map, &[]);
    let whole = gpu.render_lit_to_image(&mut wide, &halo_list(&map, &bodies, |_| {}), Some(&look), (240, 80));
    let mut split = halo_renderer(&gpu, &map, &[]);
    let lists: Vec<DrawList> = [0.0, 60.0]
        .map(|x| {
            let mut l = halo_list(&map, &bodies, |_| {});
            l.view = View { x, y: 0.0, w: 60.0, h: 40.0 };
            l
        })
        .into();
    let areas = [[0, 0, 120, 80], [120, 0, 120, 80]];
    let draws: Vec<_> =
        lists.iter().zip(areas).map(|(list, area)| olr_render::ViewDraw { list, area }).collect();
    let views = gpu.render_views_to_image(&mut split, &draws, Some(&look), (240, 80));
    assert!(views == whole, "two views show the wide view's halo");
    assert!(world_px(&views, 59, 15).r < 255, "the first view's edge takes the halo");
    assert!(world_px(&views, 58, 15).r == 255, "past its reach");
}

/// The background tag changes no byte by itself: with the halo off, a lit
/// frame with it equals one without it (the shade #53 wrote) over a back,
/// distant layers near and far, an opaque wall tile, a glowing pixel, a lamp's light and
/// its bloom, worms, an opaque sprite, a see-through one and a marker. (Its
/// match: `lit-hashes.ron` drawn with the halo off is #53's.)
#[test]
fn the_tag_changes_no_other_byte() {
    let Some(gpu) = offscreen() else { return };
    let lamp_px = |x: u32, y: u32| (40..43).contains(&x) && (4..7).contains(&y);
    let mut map = see_through(80, 40, 1, |x, y| y >= 34 || (x == 50 && y > 10));
    let mut g = olr_terrain::GlowLayers::empty((80, 40), (80, 40));
    for y in 0..40 {
        for x in 0..80 {
            if lamp_px(x, y) {
                let i = (y * 80 + x) as usize;
                (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
            }
        }
    }
    assert!(map.set_glow(Some(g)));
    let layers = [layer(30, 30, SKY, (0, 0), 20), layer(30, 30, SKY, (45, 0), 90)];
    let bodies = [([20.0, 12.0, 4.0, 6.0], 1), ([46.0, 20.0, 4.0, 6.0], 2), ([62.0, 14.0, 4.0, 6.0], 3)];
    let more = |l: &mut DrawList| {
        for (x, tint, shade) in [
            (19.0, Rgba::rgb(255, 255, 255), olr_scene::Shade::LIT),
            (25.0, Rgba { a: 128, ..Rgba::rgb(255, 255, 255) }, olr_scene::Shade::LIT),
            (60.0, Rgba::rgb(0, 255, 0), olr_scene::Shade::MARKER),
            (66.0, Rgba::rgb(255, 0, 0), olr_scene::Shade::FLOORED),
        ] {
            let src = Rect { x: 0, y: 0, w: 2, h: 2 };
            let sprite =
                Sprite { atlas: AtlasId(0), src, pos: [x, 15.0], size: [2.0, 2.0], flip: false, tint, shade };
            l.sprite(Layer::Projectiles, sprite);
        }
    };
    let mut look = glow_look(3.0, 60.0);
    look.worm_floor = [0.2; 3];
    let frame = |tag: bool, halo: bool| {
        let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
        r.upload_images(&[white_image()]);
        r.set_lighting(true);
        r.upload_terrain(&map);
        r.set_distant(&layers);
        r.set_background_tag(tag);
        r.set_worm_halo(halo);
        gpu.render_lit_to_image(&mut r, &halo_list(&map, &bodies, more), Some(&look), (160, 80))
    };
    let (untagged, tagged) = (frame(false, false), frame(true, false));
    assert!(tagged == untagged, "the tag changed a byte");
    assert!(frame(true, true) != tagged, "a worthwhile scene: the halo shows");
    assert!(frame(false, true) == untagged, "untagged, no texel is background");
}
