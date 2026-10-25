//! See-through pixels (issue #11): a level with distant layers keeps the
//! back art's alpha 0, digging reveals it, stains never land on it, and a
//! level without them stays opaque.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_terrain::{Area, ColorEdit, ColorMap, Rect, Stamp, Terrain, ThemeArt};

fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, f(x, y));
        }
    }
    img
}

/// `D` dirt, `.` background, one row.
fn map(row: &str) -> Terrain {
    let cells = row.chars().map(|c| if c == 'D' { Material::Dirt } else { Material::Background }).collect();
    Terrain::from_grid(&MaterialGrid::from_cells(row.len() as u32, 1, cells).unwrap())
}

fn theme() -> ThemeArt {
    ThemeArt {
        holes: vec![Stamp::from_image(&image(1, 1, |_, _| Rgba::MAGENTA))],
        green_mask: None,
        front_tile: image(2, 2, |_, _| Rgba::rgb(9, 9, 9)),
    }
}

const S: u32 = 2;
const TILE: Rgba = Rgba { r: 7, g: 7, b: 7, a: 0 };
const DIRT: Rgba = Rgba::rgb(120, 80, 40);
const WALL: Rgba = Rgba::rgb(60, 60, 60);

/// `..DD`: the back is see-through but for the first world pixel (a wall);
/// the front is the back on background, dirt over the dirt.
fn level() -> (Terrain, RgbaImage, RgbaImage) {
    let t = map("..DD");
    let back = image(4 * S, S, |x, _| if x < S { WALL } else { TILE });
    let front = image(4 * S, S, |x, y| if x < 2 * S { back.get(x, y) } else { DIRT });
    (t, front, back)
}

fn alpha(m: &ColorMap, x: u32) -> u8 {
    m.pixels()[(x * 4 + 3) as usize]
}

#[test]
fn see_through_pixels_stay_and_carving_reveals_them() {
    let (t, front, back) = level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    assert!(m.see_through());
    assert_eq!(alpha(&m, 0), 255);
    assert_eq!(alpha(&m, S), 0);
    assert_eq!(alpha(&m, 2 * S), 255);
    // The colour the presentation reads stays opaque.
    assert_eq!(m.color_at(1, 0), Some(Rgba::rgb(7, 7, 7)));
    m.apply(ColorEdit::Carve { hole: 0, x: 2, y: 0 });
    assert_eq!(m.material().material(2, 0), Some(Material::Background));
    assert_eq!(alpha(&m, 2 * S), 0);
    assert_eq!(alpha(&m, 3 * S), 255);
    // Placed dirt is opaque again; rebuilding as background reveals it.
    m.apply(ColorEdit::Dirt { hole: 0, x: 2, y: 0 });
    assert_eq!(alpha(&m, 2 * S), 255);
    let mut dug = t.clone();
    dug.carve_hole(&Stamp::from_image(&image(1, 1, |_, _| Rgba::MAGENTA)), 2, 0);
    m.rebuild_area(Area { x0: 2, y0: 0, x1: 3, y1: 1 }, &dug);
    assert_eq!(alpha(&m, 2 * S), 0);
}

#[test]
fn stains_skip_see_through_pixels() {
    let (t, front, back) = level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    let red = Rgba::rgb(200, 0, 0);
    for x in 0..4 {
        m.stain(x, 0, red);
    }
    let gib = image(4, 1, |_, _| red);
    m.stain_image(0, 0, &gib, Rect { x: 0, y: 0, w: 4, h: 1 }, true);
    let px = |x: u32| &m.pixels()[(x * 4) as usize..(x * 4 + 4) as usize];
    assert_eq!(px(0), [200, 0, 0, 255]);
    assert_eq!(px(S), [7, 7, 7, 0]);
    assert_eq!(px(3 * S), [200, 0, 0, 255]);
}

#[test]
fn without_distant_layers_the_map_is_opaque() {
    let (t, front, back) = level();
    let m = ColorMap::new(&t, &front, &back, S, theme());
    assert!(!m.see_through());
    assert!(m.pixels().chunks(4).all(|p| p[3] == 255));
}

#[test]
fn replace_rect_keeps_see_through_pixels() {
    let (t, front, back) = level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    let mut plain = ColorMap::new(&t, &front, &back, S, theme());
    let cells = [Material::Background, Material::Background, Material::Dirt, Material::Dirt];
    let r = Rect { x: 0, y: 0, w: 4 * S, h: S };
    m.replace_rect(r, &front, &back, &cells);
    plain.replace_rect(r, &front, &back, &cells);
    assert_eq!(alpha(&m, S), 0);
    assert_eq!(alpha(&plain, S), 255);
}

// The background glass design: a back pixel of alpha 1 to 254 is glass.
const GLASS: Rgba = Rgba { r: 200, g: 215, b: 235, a: 90 };

/// `..DD` as [`level`], but the second world pixel's back and the fourth's
/// (under dirt) are glass.
fn glass_level() -> (Terrain, RgbaImage, RgbaImage) {
    let t = map("..DD");
    let back = image(4 * S, S, |x, _| match x / S {
        0 => WALL,
        1 | 3 => GLASS,
        _ => TILE,
    });
    let front = image(4 * S, S, |x, y| if x < 2 * S { back.get(x, y) } else { DIRT });
    (t, front, back)
}

#[test]
fn glass_keeps_its_alpha() {
    let (t, front, back) = glass_level();
    let m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    assert_eq!(&m.pixels()[(S * 4) as usize..(S * 4 + 4) as usize], [200, 215, 235, 90]);
    // Dirt over glass is opaque.
    assert_eq!(alpha(&m, 3 * S), 255);
}

#[test]
fn digging_in_front_of_glass_shows_the_glass() {
    let (t, front, back) = glass_level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    m.apply(ColorEdit::Carve { hole: 0, x: 3, y: 0 });
    assert_eq!(&m.pixels()[(3 * S * 4) as usize..(3 * S * 4 + 4) as usize], [200, 215, 235, 90]);
}

#[test]
fn stains_skip_glass() {
    let (t, front, back) = glass_level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    let red = Rgba::rgb(200, 0, 0);
    m.stain(1, 0, red);
    let gib = image(4, 1, |_, _| red);
    m.stain_image(0, 0, &gib, Rect { x: 0, y: 0, w: 4, h: 1 }, true);
    assert_eq!(&m.pixels()[(S * 4) as usize..(S * 4 + 4) as usize], [200, 215, 235, 90]);
}

#[test]
fn replace_rect_keeps_glass() {
    let (t, front, back) = glass_level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    let cells = [Material::Background, Material::Background, Material::Dirt, Material::Dirt];
    m.replace_rect(Rect { x: 0, y: 0, w: 4 * S, h: S }, &front, &back, &cells);
    assert_eq!(alpha(&m, S), 90);
}

#[test]
fn a_plain_map_keeps_glass_opaque() {
    let (t, front, back) = glass_level();
    let m = ColorMap::new(&t, &front, &back, S, theme());
    assert_eq!(alpha(&m, S), 255);
}

#[test]
fn a_misfit_glass_map_is_refused() {
    let (t, front, back) = glass_level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    assert!(!m.set_glass(Some(olr_terrain::GlassMap::none((4 * S, S + 1)))));
    assert!(m.glass().is_none());
    assert!(m.set_glass(Some(olr_terrain::GlassMap::none((4 * S, S)))));
    assert!(m.glass().is_some());
}

#[test]
fn each_glass_map_set_is_a_new_generation() {
    let (t, front, back) = glass_level();
    let mut m = ColorMap::new_see_through(&t, &front, &back, S, theme());
    let g0 = m.glass_generation();
    assert!(!m.set_glass(Some(olr_terrain::GlassMap::none((4 * S, S + 1)))));
    assert_eq!(m.glass_generation(), g0, "a refused map changes nothing");
    assert!(m.set_glass(Some(olr_terrain::GlassMap::none((4 * S, S)))));
    let g1 = m.glass_generation();
    assert_ne!(g1, g0);
    assert!(m.set_glass(None));
    assert_ne!(m.glass_generation(), g1);
}
