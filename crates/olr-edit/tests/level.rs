//! The level operations (plan 13d Task 5, ruling R3).

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::level::{
    Layer, Paint, Rect, Shape, Solid, StampMask, add_spawn, carve, draw_art, erase_art, fill, move_object,
    paint, refresh_thumb, remove_object, rescale_art, resize, shape_bounds, stamp,
};
use olr_format::levels::{LevelObject, LevelSource, MAX_OBJECTS, THUMB_SIZE, compile_level};
use proptest::prelude::*;
use std::sync::Arc;

/// A `w` × `h` level of dirt at `art_scale`, every art pixel's front and
/// back colour distinct (front blue 1, back blue 2).
fn level(w: u32, h: u32, art_scale: u32) -> LevelSource {
    let art = |blue: u8| {
        let mut img = RgbaImage::new(w * art_scale, h * art_scale);
        for y in 0..h * art_scale {
            for x in 0..w * art_scale {
                img.set(x, y, Rgba::rgb(x as u8, y as u8, blue));
            }
        }
        img
    };
    LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Dirt; (w * h) as usize],
        front: art(1),
        back: art(2),
        thumb: None,
        look: None,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    }
}

fn material(l: &LevelSource, x: u32, y: u32) -> Material {
    l.material[(y * l.size.0 + x) as usize]
}

/// Whether world pixel (x, y) lies in the rect.
fn in_rect(x: u32, y: u32, (rx, ry, rw, rh): (u32, u32, u32, u32)) -> bool {
    (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y)
}

/// Every art pixel of `l` with its world pixel.
fn art_pixels(l: &LevelSource) -> impl Iterator<Item = (u32, u32, u32, u32)> + '_ {
    let s = l.art_scale;
    (0..l.size.1 * s).flat_map(move |ay| (0..l.size.0 * s).map(move |ax| (ax, ay, ax / s, ay / s)))
}

fn background_invariant(l: &LevelSource) -> bool {
    art_pixels(l).all(|(ax, ay, x, y)| {
        material(l, x, y) != Material::Background || l.front.get(ax, ay) == l.back.get(ax, ay)
    })
}

const RED: Rgba = Rgba::rgb(255, 0, 0);

#[test]
fn carve_makes_background_and_reveals_back() {
    let before = level(8, 8, 2);
    let mut l = before.clone();
    carve(&mut l, &Shape::Rect { x: 2, y: 2, w: 3, h: 3 });
    let rect = (2, 2, 3, 3);
    for y in 0..8 {
        for x in 0..8 {
            let want = if in_rect(x, y, rect) { Material::Background } else { Material::Dirt };
            assert_eq!(material(&l, x, y), want, "({x}, {y})");
        }
    }
    let mut revealed = 0;
    for (ax, ay, x, y) in art_pixels(&l) {
        assert_eq!(l.back.get(ax, ay), before.back.get(ax, ay));
        if in_rect(x, y, rect) {
            assert_eq!(l.front.get(ax, ay), l.back.get(ax, ay));
            revealed += 1;
        } else {
            assert_eq!(l.front.get(ax, ay), before.front.get(ax, ay));
        }
    }
    assert_eq!(revealed, 36);
}

#[test]
fn fill_lays_rock_and_paints_front() {
    let mut l = level(8, 8, 2);
    carve(&mut l, &Shape::Rect { x: 0, y: 0, w: 8, h: 8 });
    let before = l.clone();
    fill(&mut l, &Shape::Rect { x: 1, y: 1, w: 2, h: 2 }, Solid::Rock, &Paint::Colour(RED));
    let rect = (1, 1, 2, 2);
    for (ax, ay, x, y) in art_pixels(&l) {
        assert_eq!(l.back.get(ax, ay), before.back.get(ax, ay));
        if in_rect(x, y, rect) {
            assert_eq!(material(&l, x, y), Material::Rock);
            assert_eq!(l.front.get(ax, ay), RED);
        } else {
            assert_eq!(material(&l, x, y), Material::Background);
            assert_eq!(l.front.get(ax, ay), before.front.get(ax, ay));
        }
    }
}

#[test]
fn paint_back_shows_through_on_background_only() {
    let mut l = level(8, 8, 2);
    carve(&mut l, &Shape::Rect { x: 2, y: 2, w: 2, h: 2 });
    let before = l.clone();
    paint(&mut l, &Shape::Rect { x: 1, y: 1, w: 5, h: 5 }, Layer::Back, &Paint::Colour(RED));
    let (carved, painted) = ((2, 2, 2, 2), (1, 1, 5, 5));
    for (ax, ay, x, y) in art_pixels(&l) {
        let back = if in_rect(x, y, painted) { RED } else { before.back.get(ax, ay) };
        assert_eq!(l.back.get(ax, ay), back, "back ({ax}, {ay})");
        let front = if in_rect(x, y, carved) { RED } else { before.front.get(ax, ay) };
        assert_eq!(l.front.get(ax, ay), front, "front ({ax}, {ay})");
    }
    assert_eq!(l.material, before.material);
}

#[test]
fn paint_front_skips_background() {
    let mut l = level(8, 8, 3);
    carve(&mut l, &Shape::Rect { x: 2, y: 2, w: 2, h: 2 });
    let before = l.clone();
    paint(&mut l, &Shape::Rect { x: 1, y: 1, w: 5, h: 5 }, Layer::Front, &Paint::Colour(RED));
    let (carved, painted) = ((2, 2, 2, 2), (1, 1, 5, 5));
    for (ax, ay, x, y) in art_pixels(&l) {
        assert_eq!(l.back.get(ax, ay), before.back.get(ax, ay));
        let front =
            if in_rect(x, y, painted) && !in_rect(x, y, carved) { RED } else { before.front.get(ax, ay) };
        assert_eq!(l.front.get(ax, ay), front, "front ({ax}, {ay})");
    }
}

#[test]
fn a_tile_repeats_from_the_art_origin() {
    let mut l = level(4, 4, 2);
    let mut tile = RgbaImage::new(3, 2);
    for y in 0..2 {
        for x in 0..3 {
            tile.set(x, y, Rgba::rgb(10 * x as u8, 10 * y as u8, 99));
        }
    }
    paint(&mut l, &Shape::Rect { x: 0, y: 0, w: 4, h: 4 }, Layer::Front, &Paint::Tile(tile.clone()));
    for (ax, ay, _, _) in art_pixels(&l) {
        assert_eq!(l.front.get(ax, ay), tile.get(ax % 3, ay % 2));
    }
    let before = l.clone();
    paint(&mut l, &Shape::Rect { x: 0, y: 0, w: 4, h: 4 }, Layer::Front, &Paint::Tile(RgbaImage::new(0, 0)));
    assert_eq!(l, before, "an empty tile paints nothing");
}

/// Review B-3: a tile's transparent pixels leave the art as it was.
#[test]
fn a_tile_composites_over_the_art() {
    let mut l = level(4, 4, 1);
    let before = l.clone();
    let mut tile = RgbaImage::new(2, 1);
    tile.set(0, 0, RED);
    // (1, 0) stays transparent.
    fill(&mut l, &Shape::Rect { x: 0, y: 0, w: 4, h: 4 }, Solid::Rock, &Paint::Tile(tile.clone()));
    paint(&mut l, &Shape::Rect { x: 0, y: 0, w: 4, h: 4 }, Layer::Back, &Paint::Tile(tile));
    for (ax, ay, _, _) in art_pixels(&l) {
        let (front, back) =
            if ax % 2 == 0 { (RED, RED) } else { (before.front.get(ax, ay), before.back.get(ax, ay)) };
        assert_eq!((l.front.get(ax, ay), l.back.get(ax, ay)), (front, back), "({ax}, {ay})");
    }
}

#[test]
fn stamp_alpha_composites() {
    let mut l = level(4, 4, 2);
    let before = l.clone();
    let mut image = RgbaImage::new(2, 2);
    image.set(0, 0, RED);
    image.set(1, 1, Rgba::rgb(0, 255, 0));
    // (1, 0) and (0, 1) stay transparent.
    stamp(&mut l, (1, 1), &image, Layer::Front);
    for (ax, ay, _, _) in art_pixels(&l) {
        let want = match (ax, ay) {
            (2, 2) => RED,
            (3, 3) => Rgba::rgb(0, 255, 0),
            _ => before.front.get(ax, ay),
        };
        assert_eq!(l.front.get(ax, ay), want, "({ax}, {ay})");
    }
    assert_eq!(l.back, before.back);
    // Half alpha over the art blends.
    let mut half = RgbaImage::new(1, 1);
    half.set(0, 0, Rgba { r: 255, g: 255, b: 255, a: 128 });
    let under = l.front.get(0, 0);
    stamp(&mut l, (0, 0), &half, Layer::Front);
    let blend = |c: u8| ((255 * 128 + u32::from(c) * 127 + 127) / 255) as u8;
    assert_eq!(l.front.get(0, 0), Rgba { r: blend(under.r), g: blend(under.g), b: blend(under.b), a: 255 });
    // Off the art: clipped, never a panic.
    stamp(&mut l, (i32::MAX, i32::MIN), &image, Layer::Back);
    stamp(&mut l, (-1, -1), &image, Layer::Back);
}

#[test]
fn resize_moves_content_and_objects() {
    let mut l = level(8, 8, 1);
    l.material[0] = Material::Rock;
    add_spawn(&mut l, (7, 7), "").unwrap();
    let corner = l.front.get(0, 0);
    let dropped = resize(&mut l, (10, 10), (1, 1), Material::Background, &Paint::Colour(RED)).unwrap();
    assert!(dropped.is_empty());
    assert_eq!(l.size, (10, 10));
    assert_eq!((l.front.width(), l.front.height()), (10, 10));
    assert_eq!(material(&l, 1, 1), Material::Rock);
    assert_eq!(l.front.get(1, 1), corner);
    for i in 0..10 {
        for (x, y) in [(i, 0), (0, i), (i, 9), (9, i)] {
            assert_eq!(material(&l, x, y), Material::Background);
            assert_eq!((l.front.get(x, y), l.back.get(x, y)), (RED, RED));
        }
    }
    assert_eq!(l.objects, [LevelObject::Spawn { at: (8, 8), notes: String::new() }]);

    let mut l = level(8, 8, 2);
    add_spawn(&mut l, (1, 1), "lost").unwrap();
    add_spawn(&mut l, (6, 1), "").unwrap();
    let dropped = resize(&mut l, (4, 8), (-4, 0), Material::Dirt, &Paint::Colour(RED)).unwrap();
    assert_eq!(dropped, [LevelObject::Spawn { at: (1, 1), notes: "lost".into() }], "as it was");
    assert_eq!(l.objects, [LevelObject::Spawn { at: (2, 1), notes: String::new() }]);
    assert_eq!((l.front.width(), l.back.height()), (8, 16));
    assert!(compile_level(&l).is_ok());

    let before = l.clone();
    assert!(resize(&mut l, (5000, 10), (0, 0), Material::Dirt, &Paint::Colour(RED)).is_err());
    assert!(resize(&mut l, (0, 10), (0, 0), Material::Dirt, &Paint::Colour(RED)).is_err());
    assert_eq!(l, before, "a refused resize changes nothing");
}

#[test]
fn shapes_outside_the_level_are_clipped_never_panic() {
    let shapes = [
        Shape::Rect { x: -100, y: -100, w: 5, h: 5 },
        Shape::Rect { x: i32::MAX - 1, y: 0, w: i32::MAX, h: 1 },
        Shape::Circle { cx: 0, cy: 0, r: i32::MAX },
        Shape::Rect { x: 0, y: 0, w: -3, h: 4 },
        Shape::Circle { cx: i32::MIN, cy: i32::MAX, r: i32::MAX },
        Shape::Circle { cx: 3, cy: 3, r: -1 },
    ];
    for (i, shape) in shapes.iter().enumerate() {
        let before = level(6, 5, 2);
        let mut l = before.clone();
        carve(&mut l, shape);
        if i == 2 {
            assert!(l.material.iter().all(|m| *m == Material::Background), "the circle covers the level");
        } else {
            assert_eq!(l, before, "shape {i} changes nothing");
        }
        fill(&mut l, shape, Solid::Dirt, &Paint::Colour(RED));
        paint(&mut l, shape, Layer::Back, &Paint::Colour(RED));
    }
}

#[test]
fn a_circle_covers_pixels_within_its_radius() {
    let mut l = level(9, 9, 1);
    carve(&mut l, &Shape::Circle { cx: 4, cy: 4, r: 2 });
    let carved: Vec<(u32, u32)> = (0..9)
        .flat_map(|y| (0..9).map(move |x| (x, y)))
        .filter(|&(x, y)| material(&l, x, y) == Material::Background)
        .collect();
    let want: Vec<(u32, u32)> = (0..9)
        .flat_map(|y| (0..9).map(move |x| (x, y)))
        .filter(|&(x, y)| (x as i32 - 4).pow(2) + (y as i32 - 4).pow(2) <= 4)
        .collect();
    assert_eq!(carved, want);
    assert_eq!(carved.len(), 13);
}

#[test]
fn object_ops_bound_and_index() {
    let mut l = level(8, 8, 1);
    assert_eq!(add_spawn(&mut l, (1, 2), "a").unwrap(), 0);
    move_object(&mut l, 0, (3, 4)).unwrap();
    assert_eq!(l.objects, [LevelObject::Spawn { at: (3, 4), notes: "a".into() }]);
    assert!(remove_object(&mut l, 5).is_err());
    assert!(move_object(&mut l, 1, (0, 0)).is_err());
    assert_eq!(remove_object(&mut l, 0).unwrap(), LevelObject::Spawn { at: (3, 4), notes: "a".into() });
    l.objects = vec![LevelObject::Spawn { at: (0, 0), notes: String::new() }; MAX_OBJECTS];
    assert!(add_spawn(&mut l, (1, 1), "").is_err());
    assert_eq!(l.objects.len(), MAX_OBJECTS);
}

#[test]
fn refresh_thumb_draws_the_front() {
    let mut l = level(16, 8, 2);
    refresh_thumb(&mut l);
    let thumb = olr_format::image_io::decode_rgba(l.thumb.as_ref().unwrap()).unwrap();
    assert_eq!((thumb.width(), thumb.height()), THUMB_SIZE);
    assert!(thumb == olr_format::levels::thumbnail(&l.front));
}

/// The user's note (2026-10-19): a level with a look gets its lit
/// thumbnail (`olr_format::levels::level_thumbnail`), darker when dark.
#[test]
fn refresh_thumb_draws_a_lit_level_lit() {
    let mut l = level(16, 8, 2);
    l.look = Some(olr_format::look::LookSource {
        ambient: olr_format::look::AmbientSource { colour: None, intensity: 10 },
        ..Default::default()
    });
    refresh_thumb(&mut l);
    let thumb = olr_format::image_io::decode_rgba(l.thumb.as_ref().unwrap()).unwrap();
    assert!(thumb == olr_format::levels::level_thumbnail(&l));
    assert!(thumb != olr_format::levels::thumbnail(&l.front));
}

/// One random operation.
/// The world pixels `shape` covers on a `w` × `h` level, drawn at art scale 1.
fn coverage(shape: &Shape, w: u32, h: u32) -> Vec<(u32, u32)> {
    let mut art = RgbaImage::new(w, h);
    draw_art(&mut art, 1, (w, h), shape, &Paint::Colour(RED));
    (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| art.get(x, y).a != 0).collect()
}

#[test]
fn ellipse_and_line_cover_by_centre() {
    let all = |xs: std::ops::Range<u32>, ys: std::ops::Range<u32>| -> Vec<(u32, u32)> {
        ys.flat_map(|y| xs.clone().map(move |x| (x, y))).collect()
    };
    // A 4 × 4 ellipse: its box without the four corners.
    let want: Vec<_> =
        all(0..4, 0..4).into_iter().filter(|&(x, y)| !matches!((x, y), (0 | 3, 0 | 3))).collect();
    assert_eq!(coverage(&Shape::Ellipse { x: 0, y: 0, w: 4, h: 4 }, 8, 8), want);
    // A 4 × 2 one: every pixel centre lies inside, corners too.
    assert_eq!(coverage(&Shape::Ellipse { x: 0, y: 0, w: 4, h: 2 }, 8, 8), all(0..4, 0..2));
    assert_eq!(coverage(&Shape::Line { from: (0, 0), to: (4, 0), width: 1 }, 8, 8), all(0..5, 0..1));
    assert_eq!(coverage(&Shape::Line { from: (0, 0), to: (0, 0), width: 3 }, 8, 8), all(0..2, 0..2));
    assert_eq!(coverage(&Shape::Line { from: (0, 0), to: (4, 4), width: 0 }, 8, 8), vec![]);
    assert_eq!(coverage(&Shape::Ellipse { x: 1, y: 1, w: 0, h: 3 }, 8, 8), vec![]);
}

#[test]
fn draw_and_erase_art_return_their_rectangles() {
    let mut art = RgbaImage::new(16, 12);
    let shape = Shape::Rect { x: 1, y: 1, w: 2, h: 2 };
    let rect = Some(Rect { x: 1, y: 1, w: 2, h: 2 });
    assert_eq!(draw_art(&mut art, 2, (8, 6), &shape, &Paint::Colour(Rgba { a: 9, ..RED })), rect);
    for ay in 0..12 {
        for ax in 0..16 {
            let inside = (2..6).contains(&ax) && (2..6).contains(&ay);
            assert_eq!(art.get(ax, ay), if inside { RED } else { Rgba::TRANSPARENT }, "({ax}, {ay})");
        }
    }
    assert_eq!(erase_art(&mut art, 2, (8, 6), &shape), rect);
    assert_eq!(art, RgbaImage::new(16, 12));
    assert_eq!(shape_bounds(&Shape::Rect { x: 9, y: 0, w: 2, h: 2 }, (8, 6)), None);
}

#[test]
fn rescale_art_is_nearest() {
    let colours = [RED, Rgba::rgb(0, 255, 0), Rgba::rgb(0, 0, 255), Rgba::rgb(9, 9, 9)];
    let mut img = RgbaImage::new(2, 2);
    for (i, c) in colours.iter().enumerate() {
        img.set(i as u32 % 2, i as u32 / 2, *c);
    }
    let big = rescale_art(&img, 1, 2);
    assert_eq!((big.width(), big.height()), (4, 4));
    for ay in 0..4 {
        for ax in 0..4 {
            assert_eq!(big.get(ax, ay), colours[(ay / 2 * 2 + ax / 2) as usize]);
        }
    }
    assert_eq!(rescale_art(&big, 2, 1), img);
}

#[test]
fn far_lines_are_clipped_and_exact() {
    let mut art = RgbaImage::new(16, 16);
    let far = Shape::Line { from: (i32::MIN, i32::MIN), to: (i32::MAX, i32::MAX), width: 1 };
    let rect = draw_art(&mut art, 1, (16, 16), &far, &Paint::Colour(RED)).expect("the diagonal crosses");
    assert_eq!(rect, Rect { x: 0, y: 0, w: 16, h: 16 });
    // The diagonal through the level, one pixel wide: each pixel centre on it.
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(art.get(x, y).a != 0, x == y, "({x}, {y})");
        }
    }
    let mut art = RgbaImage::new(16, 16);
    for shape in [
        Shape::Line { from: (i32::MIN, 0), to: (i32::MIN, 5), width: u32::MAX },
        Shape::Ellipse { x: i32::MIN, y: i32::MIN, w: i32::MAX, h: i32::MAX },
        Shape::Ellipse { x: i32::MAX, y: 0, w: i32::MAX, h: 3 },
    ] {
        draw_art(&mut art, 1, (16, 16), &shape, &Paint::Colour(RED));
        erase_art(&mut art, 1, (16, 16), &shape);
    }
}

fn any_shape() -> impl Strategy<Value = Shape> {
    let c = -10_000i32..10_000;
    prop_oneof![
        (c.clone(), c.clone(), -100i32..10_000, -100i32..10_000).prop_map(|(x, y, w, h)| Shape::Rect {
            x,
            y,
            w,
            h
        }),
        (c.clone(), c.clone(), -100i32..10_000).prop_map(|(cx, cy, r)| Shape::Circle { cx, cy, r }),
        (c.clone(), c.clone(), -100i32..10_000, -100i32..10_000).prop_map(|(x, y, w, h)| Shape::Ellipse {
            x,
            y,
            w,
            h
        }),
        (c.clone(), c.clone(), c.clone(), c, 0u32..300).prop_map(|(a, b, d, e, width)| Shape::Line {
            from: (a, b),
            to: (d, e),
            width
        }),
    ]
}

fn op() -> impl Strategy<Value = (u8, i32, i32, i32, i32, u8)> {
    (0u8..5, -10i32..30, -10i32..30, -5i32..30, -5i32..30, 0u8..3)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_background_invariant_holds_at_every_art_scale(
        scale in 1u32..=4,
        w in 1u32..=24,
        h in 1u32..=24,
        ops in prop::collection::vec(op(), 1..=8),
    ) {
        let mut l = level(w, h, scale);
        for (kind, a, b, c, d, extra) in ops {
            let shape = if extra == 0 { Shape::Circle { cx: a, cy: b, r: c } } else { Shape::Rect { x: a, y: b, w: c, h: d } };
            let layer = if extra == 1 { Layer::Front } else { Layer::Back };
            let colour = Paint::Colour(Rgba::rgb(a as u8, b as u8, 77));
            match kind {
                0 => carve(&mut l, &shape),
                1 => fill(&mut l, &shape, if extra == 1 { Solid::Rock } else { Solid::Dirt }, &colour),
                2 => paint(&mut l, &shape, layer, &colour),
                3 => {
                    let mut img = RgbaImage::new(3, 2);
                    img.set(0, 0, Rgba { r: 9, g: 9, b: 9, a: 100 });
                    img.set(2, 1, Rgba::rgb(1, 2, 3));
                    stamp(&mut l, (a, b), &img, layer);
                }
                _ => {
                    let size = (c.clamp(1, 24) as u32, d.clamp(1, 24) as u32);
                    let m = [Material::Rock, Material::Background, Material::Dirt][extra as usize];
                    resize(&mut l, size, (a / 3, b / 3), m, &colour).unwrap();
                }
            }
            prop_assert!(background_invariant(&l));
        }
        prop_assert!(compile_level(&l).is_ok());
    }

    #[test]
    fn art_painting_is_clipped_and_total(
        shape in any_shape(),
        w in 1u32..=16,
        h in 1u32..=16,
        scale in 1u32..=3,
        erase in any::<bool>(),
    ) {
        let mut art = RgbaImage::new(w * scale, h * scale);
        if erase {
            for y in 0..h * scale {
                for x in 0..w * scale {
                    art.set(x, y, RED);
                }
            }
        }
        let before = art.clone();
        let rect = if erase {
            erase_art(&mut art, scale, (w, h), &shape)
        } else {
            draw_art(&mut art, scale, (w, h), &shape, &Paint::Colour(Rgba::rgb(1, 2, 3)))
        };
        prop_assert_eq!(rect, shape_bounds(&shape, (w, h)));
        if let Some(r) = rect {
            prop_assert!(r.w > 0 && r.h > 0 && r.x + r.w <= w && r.y + r.h <= h, "{:?}", r);
        }
        for ay in 0..h * scale {
            for ax in 0..w * scale {
                if art.get(ax, ay) != before.get(ax, ay) {
                    let r = rect.expect("a change inside a rectangle");
                    prop_assert!(in_rect(ax / scale, ay / scale, (r.x, r.y, r.w, r.h)));
                }
            }
        }
    }
}

#[test]
fn a_stamp_covers_its_mask_scaled() {
    let mask = StampMask { size: (3, 2), cover: vec![true, false, true, false, true, false] };
    let shape = Shape::Stamp { x: 10, y: 10, scale: 2, mask: Arc::new(mask) };
    let mut want = Vec::new();
    for (cx, cy) in [(0u32, 0u32), (2, 0), (1, 1)] {
        for dy in 0..2 {
            for dx in 0..2 {
                want.push((7 + cx * 2 + dx, 8 + cy * 2 + dy));
            }
        }
    }
    want.sort_by_key(|&(x, y)| (y, x));
    assert_eq!(coverage(&shape, 20, 20), want);
    assert_eq!(shape_bounds(&shape, (20, 20)), Some(Rect { x: 7, y: 8, w: 6, h: 4 }));
}

#[test]
fn a_stamp_far_or_huge_never_panics() {
    let mask = Arc::new(StampMask { size: (64, 64), cover: vec![true; 64 * 64] });
    for (x, y) in [(i32::MAX, 0), (i32::MIN, i32::MIN), (0, i32::MAX), (5, 5)] {
        let shape = Shape::Stamp { x, y, scale: 16, mask: mask.clone() };
        let _ = coverage(&shape, 16, 16);
        let _ = shape_bounds(&shape, (16, 16));
    }
    // A mask whose cover does not match its size covers nothing.
    let bad = Shape::Stamp {
        x: 5,
        y: 5,
        scale: 1,
        mask: Arc::new(StampMask { size: (4, 4), cover: vec![true; 3] }),
    };
    assert_eq!(coverage(&bad, 16, 16), vec![]);
}

#[test]
fn from_hole_takes_carve_and_paint() {
    let mut img = RgbaImage::new(4, 1);
    img.set(0, 0, Rgba::BLACK);
    img.set(1, 0, Rgba::MAGENTA);
    img.set(2, 0, Rgba::rgb(90, 60, 30));
    img.set(3, 0, Rgba::TRANSPARENT);
    let m = StampMask::from_hole(&img);
    assert_eq!(m.size, (4, 1));
    assert_eq!(m.cover, vec![false, true, true, false]);
}
