//! Placed and mixed drawn layers (plan 14d Tasks 4-5): painting, erasing
//! (digging to the back art), merging and splitting.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::{Decoded, Rect};
use olr_edit::drawn::{draw, erase, priority};
use olr_edit::level::{Paint, Shape};

const BLUE: Rgba = Rgba::rgb(20, 40, 200);
const BROWN: Rgba = Rgba::rgb(140, 90, 40);
const GREY: Rgba = Rgba::rgb(128, 128, 128);

fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

fn layer(art: RgbaImage, cells: Option<Vec<Material>>, back: Option<RgbaImage>) -> Decoded {
    Decoded::Drawn { art, cells, back }
}

fn art(d: &Decoded) -> &RgbaImage {
    let Decoded::Drawn { art, .. } = d else { panic!("{d:?}") };
    art
}

#[test]
fn priority_is_rock_then_dirt_then_background() {
    assert!(priority(Material::Rock) > priority(Material::Dirt));
    assert!(priority(Material::Dirt) > priority(Material::Background));
}

#[test]
fn drawing_a_moved_layer_paints_where_it_shows() {
    let mut d = layer(RgbaImage::new(2, 2), None, None);
    let paint = Paint::Colour(BROWN);
    let r = draw(&mut d, (3, 3), 1, (8, 8), &Shape::Rect { x: 3, y: 3, w: 1, h: 1 }, &paint, Material::Dirt);
    assert_eq!(r, Some(Rect { x: 3, y: 3, w: 1, h: 1 }));
    assert_eq!(art(&d).get(0, 0), BROWN);
    assert_eq!(art(&d).get(1, 0).a, 0);
    let miss =
        draw(&mut d, (3, 3), 1, (8, 8), &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &paint, Material::Dirt);
    assert_eq!(miss, None);
}

/// Review Focus 5: the paint's phase is the level's art origin, wherever
/// the layer lies.
#[test]
fn tiles_stay_aligned_on_a_moved_layer() {
    let mut tile = RgbaImage::new(3, 2);
    for (i, (x, y)) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)].into_iter().enumerate() {
        tile.set(x, y, Rgba::rgb(10 * i as u8 + 10, 0, 0));
    }
    let paint = Paint::Tile(tile);
    let all = Shape::Rect { x: 0, y: 0, w: 20, h: 20 };
    let s = 2;
    let mut whole = layer(RgbaImage::new(20 * s, 20 * s), None, None);
    draw(&mut whole, (0, 0), s, (20, 20), &all, &paint, Material::Dirt);
    let mut moved = layer(RgbaImage::new(8 * s, 6 * s), None, None);
    draw(&mut moved, (3, 5), s, (20, 20), &all, &paint, Material::Dirt);
    for ly in 0..6 * s {
        for lx in 0..8 * s {
            assert_eq!(art(&moved).get(lx, ly), art(&whole).get(lx + 3 * s, ly + 5 * s), "({lx}, {ly})");
        }
    }
}

#[test]
fn painting_sets_cells_where_it_paints() {
    let mut d = layer(solid(4, 1, BROWN), Some(vec![Material::Dirt; 4]), None);
    let shape = Shape::Rect { x: 1, y: 0, w: 2, h: 1 };
    draw(&mut d, (0, 0), 1, (4, 1), &shape, &Paint::Colour(GREY), Material::Rock);
    let Decoded::Drawn { cells: Some(cells), .. } = &d else { panic!() };
    assert_eq!(cells, &[Material::Dirt, Material::Rock, Material::Rock, Material::Dirt]);
}

/// Plan 14d R5: erasing a mixed layer's dirt with back art under it digs
/// to that art; erasing again clears it.
#[test]
fn erasing_digs_dirt_to_its_back_then_clears() {
    let mut d = layer(solid(2, 1, BROWN), Some(vec![Material::Dirt; 2]), Some(solid(2, 1, BLUE)));
    let one = Shape::Rect { x: 0, y: 0, w: 1, h: 1 };
    assert!(erase(&mut d, (0, 0), 1, (2, 1), &one).is_some());
    let Decoded::Drawn { art, cells: Some(cells), back: Some(back) } = &d else { panic!() };
    assert_eq!(cells[0], Material::Background);
    assert_eq!(art.get(0, 0), BLUE);
    assert_eq!(back.get(0, 0).a, 0);
    assert_eq!((cells[1], art.get(1, 0)), (Material::Dirt, BROWN), "outside the shape: unchanged");
    erase(&mut d, (0, 0), 1, (2, 1), &one);
    let Decoded::Drawn { art, .. } = &d else { panic!() };
    assert_eq!(art.get(0, 0).a, 0);
}

#[test]
fn erasing_without_back_clears() {
    let mut d = layer(solid(2, 2, BROWN), None, None);
    let r = erase(&mut d, (0, 0), 2, (1, 1), &Shape::Rect { x: 0, y: 0, w: 1, h: 1 });
    assert_eq!(r, Some(Rect { x: 0, y: 0, w: 1, h: 1 }));
    assert!((0..2).all(|y| (0..2).all(|x| art(&d).get(x, y).a == 0)));
}

#[test]
fn huge_or_far_shapes_never_panic() {
    let shapes = [
        Shape::Rect { x: i32::MIN, y: i32::MIN, w: i32::MAX, h: i32::MAX },
        Shape::Circle { cx: i32::MAX, cy: i32::MIN, r: i32::MAX },
        Shape::Line { from: (i32::MIN, i32::MIN), to: (i32::MAX, i32::MAX), width: u32::MAX },
    ];
    for at in [(i32::MIN, i32::MIN), (i32::MAX, i32::MAX), (-1, -1), (0, 0)] {
        for shape in &shapes {
            let mut d = layer(solid(4, 4, BROWN), Some(vec![Material::Dirt; 16]), Some(solid(4, 4, BLUE)));
            if let Some(r) = draw(&mut d, at, 1, (6, 6), shape, &Paint::Colour(GREY), Material::Rock) {
                assert!(r.x + r.w <= 6 && r.y + r.h <= 6, "{r:?}");
            }
            if let Some(r) = erase(&mut d, at, 1, (6, 6), shape) {
                assert!(r.x + r.w <= 6 && r.y + r.h <= 6, "{r:?}");
            }
        }
    }
    // Data that does not match its box (a caller's mistake) changes nothing.
    let mut bad = layer(solid(3, 3, BROWN), Some(vec![Material::Dirt; 2]), None);
    assert_eq!(draw(&mut bad, (0, 0), 1, (6, 6), &shapes[0], &Paint::Colour(GREY), Material::Rock), None);
}
