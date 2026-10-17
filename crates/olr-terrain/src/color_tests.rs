use super::*;
use alloc::vec;
use alloc::vec::Vec;
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};

const RIM: Rgba = Rgba::rgb(108, 56, 0);
const GREEN_RIM: Rgba = Rgba::rgb(108, 80, 0);

fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, f(x, y));
        }
    }
    img
}

fn rows_image(rows: &[&str], colour: impl Fn(char) -> Rgba) -> RgbaImage {
    image(rows[0].len() as u32, rows.len() as u32, |x, y| {
        colour(rows[y as usize].as_bytes()[x as usize] as char)
    })
}

/// `m` magenta, `e` the rim colour, `.` black.
fn stamp(rows: &[&str]) -> Stamp {
    Stamp::from_image(&rows_image(rows, |c| match c {
        'm' => Rgba::MAGENTA,
        'e' => RIM,
        _ => Rgba::BLACK,
    }))
}

/// `G` green, `r` the rim colour, `p` pink.
fn mask(rows: &[&str]) -> GreenMask {
    GreenMask::from_image(&rows_image(rows, |c| match c {
        'G' => Rgba::rgb(0, 255, 0),
        'r' => GREEN_RIM,
        _ => Rgba::MAGENTA,
    }))
}

/// `R` rock, `D` dirt, `.` background.
fn map(rows: &[&str]) -> Terrain {
    let cells: Vec<Material> = rows
        .iter()
        .flat_map(|r| r.chars())
        .map(|c| match c {
            'R' => Material::Rock,
            'D' => Material::Dirt,
            _ => Material::Background,
        })
        .collect();
    Terrain::from_grid(&MaterialGrid::from_cells(rows[0].len() as u32, rows.len() as u32, cells).unwrap())
}

/// Front art pixel `(x, y)` is `(x, y, 1)`, back art `(x, y, 2)`: every copy is traceable.
fn front_px(x: u32, y: u32) -> Rgba {
    Rgba::rgb(x as u8, y as u8, 1)
}
fn back_px(x: u32, y: u32) -> Rgba {
    Rgba::rgb(x as u8, y as u8, 2)
}
fn tile_px(x: u32, y: u32) -> Rgba {
    Rgba::rgb(200 + x as u8, 200 + y as u8, 3)
}

fn theme(holes: Vec<Stamp>, green: Option<GreenMask>) -> ThemeArt {
    ThemeArt { holes, green_mask: green, front_tile: image(3, 2, tile_px) }
}

fn color_map(t: &Terrain, scale: u32, theme: ThemeArt) -> ColorMap {
    let (w, h) = (t.width() * scale, t.height() * scale);
    ColorMap::new(t, &image(w, h, front_px), &image(w, h, back_px), scale, theme)
}

/// The art pixel at `(x, y)`.
fn px(c: &ColorMap, x: u32, y: u32) -> Rgba {
    let i = ((y * c.width() + x) * 4) as usize;
    let p = &c.pixels()[i..i + 4];
    Rgba { r: p[0], g: p[1], b: p[2], a: p[3] }
}

/// The colour of world pixel `(x, y)`, asserting its art block is uniform
/// or a copy of `art` (front or back) there.
enum Block {
    Colour(Rgba),
    Front,
    Back,
}

fn block(c: &ColorMap, x: u32, y: u32) -> Block {
    let s = c.art_scale();
    let all = |f: &dyn Fn(u32, u32) -> Rgba| {
        (0..s).all(|dy| (0..s).all(|dx| px(c, x * s + dx, y * s + dy) == f(x * s + dx, y * s + dy)))
    };
    if all(&front_px) {
        return Block::Front;
    }
    if all(&back_px) {
        return Block::Back;
    }
    let first = px(c, x * s, y * s);
    assert!(all(&|_, _| first), "world pixel ({x}, {y}) is not one colour");
    Block::Colour(first)
}

/// One letter per world pixel: `F` front, `B` back, `e` rim, `t` the front
/// tile's colour there, `g` a green shade, `r` the green rim, `?` other.
fn classes(c: &ColorMap) -> Vec<alloc::string::String> {
    let (w, h) = (c.width() / c.art_scale(), c.height() / c.art_scale());
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| match block(c, x, y) {
                    Block::Front => 'F',
                    Block::Back => 'B',
                    Block::Colour(k) if k == RIM => 'e',
                    Block::Colour(k) if k == GREEN_RIM => 'r',
                    Block::Colour(k) if k == tile_px(x % 3, y % 2) => 't',
                    Block::Colour(k) if GREEN_SHADES.contains(&k) => 'g',
                    Block::Colour(_) => '?',
                })
                .collect()
        })
        .collect()
}

fn rows(t: &Terrain) -> Vec<alloc::string::String> {
    (0..t.height() as i64)
        .map(|y| {
            (0..t.width() as i64)
                .map(|x| match t.material(x, y) {
                    Some(Material::Rock) => 'R',
                    Some(Material::Dirt) => 'D',
                    _ => '.',
                })
                .collect()
        })
        .collect()
}

#[test]
fn carve_copies_back_and_paints_rim() {
    let t = map(&["...DD", "DDDDD", "DDDRD", "DDDDD", "DDDDD"]);
    let hole = stamp(&["..m..", ".mmm.", "mmemm", ".mmm.", "..m.."]);
    let mut c = color_map(&t, 2, theme(vec![hole.clone()], None));
    // A stain on background (a gib's): OLX carves only dirt, so it stays.
    c.stain(2, 0, Rgba::rgb(200, 0, 0));
    c.apply(ColorEdit::Carve { hole: 0, x: 2, y: 2 });
    let mut cls = classes(&c);
    cls[0].replace_range(2..3, "s");
    assert_eq!(cls, ["FFsFF", "FBBBF", "BBeFB", "FBBBF", "FFBFF"]);
    assert!(matches!(block(&c, 2, 0), Block::Colour(k) if k == Rgba::rgb(200, 0, 0)));
    let mut expect = t.clone();
    expect.carve_hole(&hole, 2, 2);
    assert_eq!(c.material(), &expect);
    assert_eq!(rows(c.material()), ["...DD", "D...D", "..DR.", "D...D", "DD.DD"]);
    // The rim is painted only on dirt: carving again changes nothing.
    let before = c.pixels().to_vec();
    c.apply(ColorEdit::Carve { hole: 0, x: 2, y: 2 });
    assert_eq!(c.pixels(), &before[..]);
}

#[test]
fn carve_keeps_olx_edge_offset_quirk_and_missing_holes_do_nothing() {
    // Centred at x=0: the stamp is still read from its column 0 (as the material).
    let t = map(&["DDD"]);
    let hole = stamp(&["meD"]);
    let mut c = color_map(&t, 3, theme(vec![hole], None));
    c.apply(ColorEdit::Carve { hole: 0, x: 0, y: 0 });
    assert_eq!(classes(&c), ["BeF"]);
    let before = c.pixels().to_vec();
    c.apply(ColorEdit::Carve { hole: 3, x: 1, y: 0 });
    c.apply(ColorEdit::Dirt { hole: 3, x: 1, y: 0 });
    c.apply(ColorEdit::GreenDirt { x: 1, y: 0 });
    assert_eq!(c.pixels(), &before[..], "no such hole, no green mask");
}

#[test]
fn dirt_uses_front_tile_colours() {
    let s = stamp(&["me.", "emm", ".me"]);
    // Rock is untouched; dirt under the rim keeps the tile (OLX paints the rim
    // only over what was background); background under the rim gets the rim.
    let t = map(&["RDD.", "....", "DR..", "...."]);
    let mut c = color_map(&t, 2, theme(vec![s.clone()], None));
    c.apply(ColorEdit::Dirt { hole: 0, x: 1, y: 1 });
    assert_eq!(classes(&c), ["FtFF", "ettF", "FFeF", "FFFF"]);
    let mut expect = t.clone();
    expect.place_dirt(&s, 1, 1);
    assert_eq!(c.material(), &expect);
    // Top-left corner: offset by the clipped amount, so map (0,0) gets stamp (1,1).
    let t = map(&["...", "...", "..."]);
    let mut c = color_map(&t, 1, theme(vec![s.clone()], None));
    c.apply(ColorEdit::Dirt { hole: 0, x: 0, y: 0 });
    assert_eq!(classes(&c), ["ttF", "teF", "FFF"]);
    // The tile repeats from the map origin: (3, 2) of a 3x2 tile is (0, 0).
    let t = map(&["....", "....", "....", "...."]);
    let mut c = color_map(&t, 1, theme(vec![stamp(&["m"])], None));
    c.apply(ColorEdit::Dirt { hole: 0, x: 3, y: 2 });
    assert_eq!(c.color_at(3, 2), Some(tile_px(0, 0)));
}

#[test]
fn green_dirt_uses_olx_shades() {
    assert_eq!(
        GREEN_SHADES,
        [Rgba::rgb(148, 136, 0), Rgba::rgb(136, 124, 0), Rgba::rgb(124, 112, 0), Rgba::rgb(116, 100, 0)]
    );
    let m = mask(&["Grp", "rGG", "pGr"]);
    let t = map(&[".D.", "...", "R.."]);
    let mut c = color_map(&t, 2, theme(vec![], Some(m.clone())));
    c.apply(ColorEdit::GreenDirt { x: 1, y: 1 });
    assert_eq!(classes(&c), ["gFF", "rgg", "Fgr"]);
    let mut expect = t.clone();
    expect.place_green_dirt(&m, 1, 1, &mut || 0);
    assert_eq!(c.material(), &expect);
    // A big blob uses all four shades and nothing else, the same way every time.
    let big = GreenMask::from_image(&image(20, 20, |_, _| Rgba::rgb(0, 255, 0)));
    let row = ".".repeat(20);
    let t = map(&[row.as_str(); 20]);
    let blob = || {
        let mut c = color_map(&t, 1, theme(vec![], Some(big.clone())));
        c.apply(ColorEdit::GreenDirt { x: 10, y: 10 });
        c
    };
    let c = blob();
    let used: Vec<bool> =
        GREEN_SHADES.iter().map(|g| c.pixels().chunks(4).any(|p| p == [g.r, g.g, g.b, 255])).collect();
    assert_eq!(used, [true; 4]);
    assert!(classes(&c).iter().all(|r| r.chars().all(|k| k == 'g')));
    assert_eq!(blob().pixels(), c.pixels(), "deterministic");
}

#[test]
fn stains_paint_world_pixels_with_olx_clipping_and_colour_key() {
    let t = map(&["DDDD", "DDDD", "DDDD"]);
    let mut c = color_map(&t, 2, theme(vec![], None));
    let red = Rgba::rgb(128, 0, 0);
    c.stain(1, 2, red);
    c.stain(-1, 0, red);
    c.stain(4, 0, red);
    c.stain(i32::MIN, i32::MAX, red);
    assert_eq!(classes(&c).concat(), "FFFFFFFFF?FF");
    assert_eq!(c.color_at(1, 2), Some(red));
    assert_eq!(c.material(), &t, "stains never change material");
    // A gib: a 4x4 frame at (sx, 8) of the gib sheet, one world pixel per
    // source pixel, magenta skipped. Clipped at the left, it is still read
    // from its first column (OLX's DrawImageStretch2).
    let sheet =
        image(
            8,
            12,
            |x, y| if (x + y) % 3 == 0 { Rgba::MAGENTA } else { Rgba::rgb(10 * x as u8, 10 * y as u8, 9) },
        );
    let mut c = color_map(&t, 2, theme(vec![], None));
    c.stain_image(-1, 1, &sheet, Rect { x: 4, y: 8, w: 4, h: 4 });
    for (x, y) in [(0u32, 1u32), (1, 1), (2, 1), (0, 2), (1, 2), (2, 2)] {
        let (sx, sy) = (4 + x, 8 + y - 1);
        let expect = if (sx + sy) % 3 == 0 { None } else { Some(Rgba::rgb(10 * sx as u8, 10 * sy as u8, 9)) };
        match (block(&c, x, y), expect) {
            (Block::Front, None) => {}
            (Block::Colour(k), Some(e)) if k == e => {}
            _ => panic!("({x}, {y})"),
        }
    }
    assert!(matches!(block(&c, 3, 1), Block::Front));
    assert!(matches!(block(&c, 0, 0), Block::Front));
    // A source rect past the sheet is clipped; extreme positions are no-ops.
    c.stain_image(0, 0, &sheet, Rect { x: 6, y: 10, w: 4, h: 4 });
    c.stain_image(i32::MAX, i32::MIN, &sheet, Rect { x: 0, y: 0, w: u32::MAX, h: u32::MAX });
    c.stain_image(0, 0, &sheet, Rect { x: u32::MAX, y: u32::MAX, w: 4, h: 4 });
}

/// A deterministic edit sequence over a 24x16 map with a bit of everything.
fn edits() -> Vec<ColorEdit> {
    let mut v = Vec::new();
    let mut r = 0x1234_5678_u32;
    for i in 0..300 {
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        let (x, y) = ((r % 30) as i32 - 3, ((r >> 8) % 22) as i32 - 3);
        v.push(match i % 4 {
            0 | 3 => ColorEdit::Carve { hole: (r >> 16) as u8 % 2, x, y },
            1 => ColorEdit::Dirt { hole: (r >> 16) as u8 % 2, x, y },
            _ => ColorEdit::GreenDirt { x, y },
        });
    }
    v
}

fn busy_map() -> Terrain {
    let mut rows: Vec<alloc::string::String> = Vec::new();
    for y in 0..16 {
        rows.push((0..24).map(|x| ['D', 'D', '.', 'R', 'D'][(x * 7 + y * 3) % 5]).collect());
    }
    let refs: Vec<&str> = rows.iter().map(|s| s.as_str()).collect();
    map(&refs)
}

fn busy_theme() -> ThemeArt {
    theme(
        vec![stamp(&[".m.", "mem", ".m."]), stamp(&["eme", "mmm", "eme", "mm."])],
        Some(mask(&["pGp", "GrG", "pGp"])),
    )
}

#[test]
fn dirty_rects_cover_every_change() {
    for scale in [1, 2, 3] {
        let t = busy_map();
        let mut c = color_map(&t, scale, busy_theme());
        let sheet = image(8, 12, |x, y| Rgba::rgb(x as u8, y as u8, 7));
        for (i, e) in edits().into_iter().enumerate() {
            let before = c.pixels().to_vec();
            c.apply(e);
            if i % 5 == 0 {
                c.stain(i as i32 % 26 - 1, i as i32 % 17, Rgba::rgb(200, 0, 0));
            }
            if i % 7 == 0 {
                c.stain_image(i as i32 % 27 - 2, i as i32 % 19 - 2, &sheet, Rect { x: 0, y: 8, w: 4, h: 4 });
            }
            let dirty: Vec<Rect> = c.take_dirty().collect();
            for r in &dirty {
                assert!(r.w > 0 && r.h > 0 && r.x + r.w <= c.width() && r.y + r.h <= c.height(), "{r:?}");
            }
            for (k, (a, b)) in before.chunks(4).zip(c.pixels().chunks(4)).enumerate() {
                let (x, y) = (k as u32 % c.width(), k as u32 / c.width());
                let covered =
                    dirty.iter().any(|r| (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y));
                assert!(
                    a == b || covered,
                    "scale {scale}, edit {i} {e:?}: ({x}, {y}) changed outside {dirty:?}"
                );
            }
            assert_eq!(c.take_dirty().count(), 0, "taken");
        }
    }
}

#[test]
fn dirty_rects_stay_bounded_when_never_taken() {
    let mut c = color_map(&busy_map(), 2, busy_theme());
    for _ in 0..20 {
        for e in edits() {
            c.apply(e);
        }
    }
    let dirty: Vec<Rect> = c.take_dirty().collect();
    assert!(!dirty.is_empty() && dirty.len() <= ColorMap::MAX_DIRTY, "{}", dirty.len());
}

#[test]
fn rebuild_area_matches_material() {
    let t = busy_map();
    let mut c = color_map(&t, 2, busy_theme());
    // The simulation made edits the colour map never saw (dropped events).
    let mut sim = t.clone();
    let holes = busy_theme().holes;
    for (i, e) in edits().into_iter().enumerate().take(40) {
        if let ColorEdit::Carve { hole, x, y } = e {
            sim.carve_hole(&holes[hole as usize], x, y);
        } else if i % 2 == 0 {
            c.apply(e);
            if let ColorEdit::Dirt { hole, x, y } = e {
                sim.place_dirt(&holes[hole as usize], x, y);
            } else if let ColorEdit::GreenDirt { x, y } = e {
                sim.place_green_dirt(busy_theme().green_mask.as_ref().unwrap(), x, y, &mut || 0);
            }
        }
    }
    let _ = c.take_dirty().count();
    c.rebuild_area(Area { x0: -5, y0: -5, x1: 100, y1: 100 }, &sim);
    assert_eq!(c.material(), &sim);
    let dirty: Vec<Rect> = c.take_dirty().collect();
    assert_eq!(dirty, [Rect { x: 0, y: 0, w: 48, h: 32 }]);
    for y in 0..16 {
        for x in 0..24 {
            let ok = match (sim.material(x, y).unwrap(), block(&c, x as u32, y as u32)) {
                (Material::Background, Block::Back) | (Material::Rock, Block::Front) => true,
                (Material::Dirt, Block::Colour(k)) => k == tile_px(x as u32 % 3, y as u32 % 2),
                _ => false,
            };
            assert!(ok, "({x}, {y})");
        }
    }
    // Empty, inverted or far-away areas, or a material of another size: no panic.
    c.rebuild_area(Area { x0: 5, y0: 5, x1: 2, y1: 9 }, &sim);
    c.rebuild_area(Area { x0: i32::MIN, y0: i32::MIN, x1: i32::MAX, y1: i32::MAX }, &map(&["R"]));
    assert_eq!(c.material().material(0, 0), Some(Material::Rock));
    assert!(matches!(block(&c, 0, 0), Block::Front));
}

#[test]
fn odd_inputs_never_panic() {
    let t = busy_map();
    // Art of the wrong size, a zero art scale, an empty tile.
    let small = image(5, 3, front_px);
    let empty = RgbaImage::new(0, 0);
    let th = ThemeArt { front_tile: empty.clone(), ..busy_theme() };
    for (front, back, scale) in [(&small, &empty, 2), (&empty, &small, 0), (&small, &small, 1)] {
        let mut c = ColorMap::new(&t, front, back, scale, th.clone());
        assert_eq!((c.width(), c.height()), (24 * scale.max(1), 16 * scale.max(1)));
        for e in edits() {
            c.apply(e);
        }
        for p in [i32::MIN, -1, 0, 7, i32::MAX] {
            c.apply(ColorEdit::Carve { hole: 1, x: p, y: p });
            c.apply(ColorEdit::Dirt { hole: 1, x: p, y: -p.saturating_add(1) });
            c.apply(ColorEdit::GreenDirt { x: -p.saturating_add(1), y: p });
            assert_eq!(c.color_at(p as i64, 0).is_some(), (0..24).contains(&p));
        }
        assert!(c.pixels().chunks(4).all(|p| p[3] == 255), "the composite is opaque");
    }
}

fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    image(w, h, |_, _| c)
}

#[test]
fn replace_rect_copies_and_marks_dirty() {
    let t = map(&["RRRR", "RRRR", "RRRR"]);
    let mut c = color_map(&t, 2, theme(vec![], None));
    let before = c.pixels().to_vec();
    let (red, blue) = (Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255));
    let mat = map(&["....", "....", "...."]);
    let r = Rect { x: 2, y: 2, w: 4, h: 2 };
    c.replace_rect(r, &solid(8, 6, red), &solid(8, 6, blue), &mat);
    for y in 0..6 {
        for x in 0..8 {
            let inside = (2..6).contains(&x) && (2..4).contains(&y);
            let want = if inside {
                red
            } else {
                let i = ((y * 8 + x) * 4) as usize;
                Rgba { r: before[i], g: before[i + 1], b: before[i + 2], a: before[i + 3] }
            };
            assert_eq!(px(&c, x, y), want, "art pixel ({x}, {y})");
        }
    }
    let dirty: Vec<Rect> = c.take_dirty().collect();
    assert!(
        dirty.iter().any(|d| d.x <= r.x && d.y <= r.y && d.x + d.w >= r.x + r.w && d.y + d.h >= r.y + r.h),
        "{dirty:?} holds {r:?}"
    );
    assert_eq!(c.color_at(1, 1), Some(red));
    assert_eq!(c.material().material(1, 1), Some(Material::Background));
    assert_eq!(c.material().material(0, 0), Some(Material::Rock), "cells outside r keep their material");
    // A carve there now reveals the replaced back image.
    let mut c2 = c.clone();
    c2.rebuild_area(Area { x0: 1, y0: 1, x1: 2, y1: 2 }, c.material());
    assert_eq!(c2.color_at(1, 1), Some(blue));
}

#[test]
fn replace_rect_of_another_size_changes_nothing() {
    let t = map(&["RRRR", "RRRR", "RRRR"]);
    let mut c = color_map(&t, 2, theme(vec![], None));
    let before = c.pixels().to_vec();
    let red = Rgba::rgb(255, 0, 0);
    let r = Rect { x: 0, y: 0, w: 8, h: 6 };
    let mat = map(&["....", "....", "...."]);
    c.replace_rect(r, &solid(7, 6, red), &solid(8, 6, red), &mat);
    c.replace_rect(r, &solid(8, 6, red), &solid(8, 5, red), &mat);
    c.replace_rect(r, &solid(8, 6, red), &solid(8, 6, red), &map(&["...", "..."]));
    assert_eq!(c.pixels(), &before[..]);
    assert_eq!(c.take_dirty().count(), 0);
    assert_eq!(c.material().material(0, 0), Some(Material::Rock));
}

#[test]
fn replace_rect_clips() {
    let t = map(&["RRRR", "RRRR", "RRRR"]);
    let mut c = color_map(&t, 2, theme(vec![], None));
    let red = Rgba::rgb(255, 0, 0);
    let mat = map(&["....", "....", "...."]);
    c.replace_rect(Rect { x: 6, y: 4, w: 100, h: 100 }, &solid(8, 6, red), &solid(8, 6, red), &mat);
    for y in 0..6 {
        for x in 0..8 {
            assert_eq!(px(&c, x, y) == red, x >= 6 && y >= 4, "art pixel ({x}, {y})");
        }
    }
    assert_eq!(c.material().material(3, 2), Some(Material::Background));
    assert_eq!(c.material().material(2, 2), Some(Material::Rock));
    let dirty: Vec<Rect> = c.take_dirty().collect();
    assert!(dirty.iter().all(|d| d.x + d.w <= 8 && d.y + d.h <= 6), "{dirty:?} inside the map");
    for r in [
        Rect { x: u32::MAX, y: u32::MAX, w: u32::MAX, h: u32::MAX },
        Rect { x: 8, y: 0, w: 1, h: 1 },
        Rect { x: 0, y: 0, w: 0, h: 0 },
    ] {
        c.replace_rect(r, &solid(8, 6, red), &solid(8, 6, red), &mat);
    }
    assert_eq!(c.take_dirty().count(), 0);
}
