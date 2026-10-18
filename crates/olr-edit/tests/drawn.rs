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

// Task 5: merge and split.

use std::sync::Arc;

use olr_edit::compose::{ImageCache, bake_elements, decode_element, split_from};
use olr_edit::drawn::{LayerError, Placed, merge, merge_down, placed, split, split_element};
use olr_format::levels::{Dither, Element, ElementKind, ElementMaterial};

const DARK: Rgba = Rgba::rgb(90, 60, 30);

fn element(id: &str, kind: ElementKind) -> Element {
    Element { id: id.into(), name: id.into(), visible: true, locked: false, kind }
}

/// A 1-high strip at `at` of `pixels` (`None` uncovered) of `m`, art scale 1.
fn strip(at: (i32, i32), pixels: &[Option<Rgba>], m: Material) -> Placed {
    let mut art = RgbaImage::new(pixels.len() as u32, 1);
    for (x, p) in pixels.iter().enumerate() {
        if let Some(c) = p {
            art.set(x as u32, 0, *c);
        }
    }
    Placed { at, size: (pixels.len() as u32, 1), art, cells: vec![m; pixels.len()], back: None }
}

fn row(p: &Placed) -> Vec<Rgba> {
    (0..p.art.width()).map(|x| p.art.get(x, 0)).collect()
}

/// Review Focus 1's base: a loaded map's Dirt merged into its Background
/// bakes as the three layers did.
#[test]
fn merging_dirt_into_background_bakes_the_same() {
    let Some(root) = crate::common::content() else { return };
    for id in ["olx-level-dirt-level", "olx-level-castlestrike", "olx-level-liero-factory"] {
        let original = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
        let tile = olr_edit::compose::theme_back_tile(&root, &original.theme).unwrap();
        let (size, s) = (original.size, original.art_scale);
        let src = split_from(&original);
        let mut cache = ImageCache::new();
        let items: Vec<(Element, Decoded)> = src
            .composition
            .elements
            .iter()
            .map(|e| (e.clone(), decode_element(&src, e, size, s, &mut cache).unwrap()))
            .collect();
        let bake = |stack: &[(&Element, &Decoded)]| {
            let mut l = original.clone();
            bake_elements(&mut l, stack, &tile, Rect { x: 0, y: 0, w: size.0, h: size.1 });
            (l.material, l.front, l.back)
        };
        let three: Vec<(&Element, &Decoded)> = items.iter().map(|(e, d)| (e, d)).collect();
        let (e, d) = merge_down((&items[0].0, &items[0].1), (&items[1].0, &items[1].1), size, s).unwrap();
        assert_eq!(e.id, "background");
        assert!(
            matches!(e.kind, ElementKind::Drawn { mixed: true, back: true, size: None, .. }),
            "{id}: {:?}",
            e.kind
        );
        let two = [(&e, &d), (&items[2].0, &items[2].1)];
        assert!(bake(&two) == bake(&three), "{id}: the merge bakes alike");
        // And it splits back into the two layers it was made of.
        let p = placed(&e, &d, size, s).unwrap();
        let [bg, dirt, rock] = split(&p, s);
        let art = |i: usize| match &items[i].1 {
            Decoded::Drawn { art, .. } => art.clone(),
            _ => panic!(),
        };
        assert!(bg.unwrap().art == art(0), "{id}: the background comes back");
        assert!(dirt.unwrap().art == art(1), "{id}: the dirt comes back");
        assert!(rock.is_none(), "{id}: no rock in the merge");
    }
}

#[test]
fn a_merge_follows_rock_then_dirt_then_background() {
    let lower = strip((0, 0), &[Some(BROWN), Some(BROWN), None], Material::Dirt);
    let upper = strip((0, 0), &[Some(GREY), None, Some(GREY)], Material::Rock);
    let m = merge(&lower, &upper, 1).unwrap();
    assert_eq!(m.cells, [Material::Rock, Material::Dirt, Material::Rock]);
    assert_eq!(row(&m), [GREY, BROWN, GREY]);
    // Background over dirt: dirt wins, and the background is its back art.
    let sky = strip((0, 0), &[Some(BLUE), Some(BLUE), Some(BLUE)], Material::Background);
    let m = merge(&lower, &sky, 1).unwrap();
    assert_eq!(m.cells, [Material::Dirt, Material::Dirt, Material::Background]);
    assert_eq!(row(&m), [BROWN, BROWN, BLUE]);
    let back = m.back.unwrap();
    assert_eq!([back.get(0, 0), back.get(1, 0)], [BLUE, BLUE]);
    assert_eq!(back.get(2, 0).a, 0);
}

#[test]
fn a_tie_goes_to_the_upper_layer() {
    let lower = strip((0, 0), &[Some(BROWN)], Material::Dirt);
    let upper = strip((0, 0), &[Some(DARK)], Material::Dirt);
    assert_eq!(row(&merge(&lower, &upper, 1).unwrap()), [DARK]);
}

/// Review Focus 4: a merge covers both boxes; pixels off the level stay.
#[test]
fn a_merge_keeps_pixels_off_the_level() {
    let lower = strip((0, 0), &[Some(BROWN); 3], Material::Dirt);
    let upper = strip((-2, 0), &[Some(GREY); 4], Material::Rock);
    let m = merge(&lower, &upper, 1).unwrap();
    assert_eq!((m.at, m.size), ((-2, 0), (5, 1)));
    assert_eq!(row(&m), [GREY, GREY, GREY, GREY, BROWN]);
    assert_eq!(m.cells[4], Material::Dirt);
}

#[test]
fn split_of_an_image_is_its_processed_pixels() {
    let mut grid = RgbaImage::new(2, 2);
    grid.set(0, 0, BROWN);
    grid.set(1, 1, Rgba { a: 100, ..BROWN });
    let e = element(
        "photo",
        ElementKind::Image {
            material: ElementMaterial::Dirt,
            source: "photo.png".into(),
            at: (3, 1),
            size: (2, 2),
            pixel_size: 1,
            colours: None,
            dither: Dither::None,
            alpha_threshold: 128,
            mask: false,
        },
    );
    let d = Decoded::Image { grid: Some(Arc::new(grid)), mask: None };
    let out = split_element(&e, &d, (8, 8), 1, |name, _| name.to_ascii_lowercase()).unwrap();
    assert_eq!(out.len(), 1, "one material, one layer");
    let (layer, data) = &out[0];
    assert_eq!(layer.id, "dirt");
    assert!(matches!(layer.kind, ElementKind::Drawn { at: (3, 1), size: Some((2, 2)), mixed: false, .. }));
    let Decoded::Drawn { art, .. } = data else { panic!() };
    assert_eq!(art.get(0, 0), BROWN);
    assert_eq!(art.get(1, 1).a, 0, "below the threshold's 255: uncovered");
}

#[test]
fn merge_down_refuses_hidden_and_non_drawn() {
    let layer = |id: &str| element(id, ElementKind::drawn(ElementMaterial::Dirt));
    let d = Decoded::Drawn { art: RgbaImage::new(2, 2), cells: None, back: None };
    let mut hidden = layer("top");
    hidden.visible = false;
    let below = layer("below");
    assert_eq!(
        merge_down((&below, &d), (&hidden, &d), (2, 2), 1).unwrap_err(),
        LayerError::Hidden("top".into())
    );
    let image = element(
        "img",
        ElementKind::Image {
            material: ElementMaterial::Dirt,
            source: "a.png".into(),
            at: (0, 0),
            size: (2, 2),
            pixel_size: 1,
            colours: None,
            dither: Dither::None,
            alpha_threshold: 128,
            mask: false,
        },
    );
    let img = Decoded::Image { grid: Some(Arc::new(RgbaImage::new(2, 2))), mask: None };
    assert_eq!(
        merge_down((&image, &img), (&layer("top"), &d), (2, 2), 1).unwrap_err(),
        LayerError::NotDrawn("img".into())
    );
}

/// Review Focus 4: a union of boxes beyond `MAX_SIDE` is refused, never
/// allocated.
#[test]
fn a_merge_too_large_is_refused() {
    let lower = strip((0, 0), &[Some(BROWN)], Material::Dirt);
    let far = strip((5000, 0), &[Some(GREY)], Material::Rock);
    assert_eq!(merge(&lower, &far, 1), None);
    let extreme = strip((i32::MIN, i32::MAX), &[Some(GREY)], Material::Rock);
    assert_eq!(merge(&lower, &extreme, 1), None);
    let kind = |at| ElementKind::Drawn {
        material: ElementMaterial::Dirt,
        at,
        size: Some((1, 1)),
        mixed: false,
        back: false,
    };
    let d = Decoded::Drawn { art: solid(1, 1, BROWN), cells: None, back: None };
    let (a, b) = (element("a", kind((0, 0))), element("b", kind((5000, 0))));
    assert_eq!(merge_down((&a, &d), (&b, &d), (8, 8), 1).unwrap_err(), LayerError::TooLarge("a".into()));
}
