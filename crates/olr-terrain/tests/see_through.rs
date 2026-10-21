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
