//! Glow as a channel of the colour terrain (spec §Glow, plan 15a Task 4):
//! digging reveals the backdrop's glow, placed dirt has none, stains keep it.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_terrain::{Area, ColorEdit, ColorMap, GlowLayers, Rect, Stamp, Terrain, ThemeArt};
use proptest::prelude::*;

fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, f(x, y));
        }
    }
    img
}

/// `R` rock, `D` dirt, `.` background.
fn map(rows: &[&str]) -> Terrain {
    let cells = rows
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

/// A hole carving everything under it (all magenta), `n` × `n`.
fn hole(n: u32) -> Stamp {
    Stamp::from_image(&image(n, n, |_, _| Rgba::MAGENTA))
}

fn theme() -> ThemeArt {
    ThemeArt {
        holes: vec![hole(1), hole(3)],
        green_mask: None,
        front_tile: image(2, 2, |_, _| Rgba::rgb(9, 9, 9)),
    }
}

const S: u32 = 2;

/// Front glow 10 + x, back glow 100 + x (art pixels); front group 1, back
/// group 2. As a bake makes them, the front equals the back on background.
fn layers(t: &Terrain) -> GlowLayers {
    let (aw, ah) = (t.width() * S, t.height() * S);
    let (ww, wh) = (t.width(), t.height());
    let air = |x: u32, y: u32| t.material(i64::from(x), i64::from(y)) == Some(Material::Background);
    let back: Vec<u8> = (0..ah).flat_map(|_| (0..aw).map(|x| 100 + x as u8)).collect();
    let front = (0..ah)
        .flat_map(|y| (0..aw).map(move |x| (x, y)))
        .map(|(x, y)| if air(x / S, y / S) { 100 + x as u8 } else { 10 + x as u8 })
        .collect();
    let groups = (0..wh)
        .flat_map(|y| (0..ww).map(move |x| (x, y)))
        .map(|(x, y)| if air(x, y) { 2 } else { 1 })
        .collect();
    GlowLayers {
        art: (aw, ah),
        world: (ww, wh),
        front,
        back,
        groups,
        groups_back: vec![2; (ww * wh) as usize],
    }
}

fn glowing(t: &Terrain) -> ColorMap {
    let (w, h) = (t.width() * S, t.height() * S);
    let mut c = ColorMap::new(
        t,
        &image(w, h, |_, _| Rgba::rgb(1, 1, 1)),
        &image(w, h, |_, _| Rgba::rgb(2, 2, 2)),
        S,
        theme(),
    );
    assert!(c.set_glow(Some(layers(t))));
    c
}

/// The glow strength of world pixel (x, y)'s top-left art pixel, and its group.
fn at(c: &ColorMap, x: u32, y: u32) -> (u8, u8) {
    let g = c.glow().unwrap()[((y * S) * c.width() + x * S) as usize];
    let k = c.glow_groups().unwrap()[(y * c.material().width() + x) as usize];
    (g, k)
}

#[test]
fn a_new_map_shows_the_front_glow() {
    let t = map(&["D.R"]);
    let c = glowing(&t);
    assert_eq!(at(&c, 0, 0), (10, 1));
    assert_eq!(at(&c, 2, 0), (14, 1));
}

#[test]
fn carving_reveals_the_backdrop_glow() {
    let t = map(&["DDD", "DDD", "DRD"]);
    let mut c = glowing(&t);
    c.apply(ColorEdit::Carve { hole: 0, x: 1, y: 1 });
    assert_eq!(at(&c, 1, 1), (102, 2));
    assert_eq!(at(&c, 0, 0), (10, 1), "untouched dirt keeps its glow");
    assert!(c.take_dirty().count() > 0);
}

#[test]
fn placed_dirt_has_no_glow() {
    let t = map(&["...", "...", "..."]);
    let mut c = glowing(&t);
    c.apply(ColorEdit::Dirt { hole: 1, x: 1, y: 1 });
    for y in 0..3 {
        for x in 0..3 {
            assert_eq!(at(&c, x, y), (0, 0), "({x}, {y})");
        }
    }
}

#[test]
fn stains_keep_glow() {
    let t = map(&["DR"]);
    let mut c = glowing(&t);
    c.stain(0, 0, Rgba::rgb(200, 0, 0));
    c.stain_image(1, 0, &image(1, 1, |_, _| Rgba::rgb(0, 200, 0)), Rect { x: 0, y: 0, w: 1, h: 1 }, false);
    assert_eq!(at(&c, 0, 0), (10, 1));
    assert_eq!(at(&c, 1, 0), (12, 1));
}

#[test]
fn glow_follows_rebuild_and_replace() {
    let t = map(&["DDDD", "RRRR"]);
    let mut c = glowing(&t);
    // rebuild_area: background takes the back glow, rock the front, dirt none (the tile).
    let after = map(&["..DD", "RRR."]);
    c.rebuild_area(Area { x0: 0, y0: 0, x1: 4, y1: 2 }, &after);
    assert_eq!(at(&c, 0, 0), (100, 2));
    assert_eq!(at(&c, 2, 0), (0, 0));
    assert_eq!(at(&c, 0, 1), (10, 1));
    assert_eq!(at(&c, 3, 1), (106, 2));
    // replace_glow_rect: the editor's new glow over a rectangle (the composite takes its front).
    let mut new = layers(&t);
    new.front.iter_mut().for_each(|g| *g = 77);
    new.groups.iter_mut().for_each(|g| *g = 5);
    c.take_dirty().for_each(drop);
    c.replace_glow_rect(Rect { x: 0, y: 0, w: 2, h: 2 }, &new);
    assert_eq!(at(&c, 0, 0), (77, 5));
    assert_eq!(at(&c, 1, 0), (102, 2), "outside the rectangle unchanged (rebuilt background)");
    assert_eq!(c.take_dirty().collect::<Vec<_>>(), [Rect { x: 0, y: 0, w: 2, h: 2 }]);
    // A rectangle past the map, or layers of another size, never panic.
    c.replace_glow_rect(Rect { x: 7, y: 3, w: u32::MAX, h: u32::MAX }, &new);
    c.replace_glow_rect(Rect { x: 0, y: 0, w: 8, h: 4 }, &layers(&map(&["D"])));
    assert_eq!(at(&c, 0, 0), (77, 5));
}

#[test]
fn a_misfit_glow_is_refused() {
    let t = map(&["DD"]);
    let (w, h) = (4, 2);
    let mut c =
        ColorMap::new(&t, &image(w, h, |_, _| Rgba::BLACK), &image(w, h, |_, _| Rgba::BLACK), S, theme());
    let mut bad = layers(&t);
    bad.front.pop();
    assert!(!c.set_glow(Some(bad)));
    assert!(c.glow().is_none());
    let mut wrong = layers(&t);
    wrong.world = (3, 1);
    assert!(!c.set_glow(Some(wrong)));
    assert!(c.set_glow(Some(layers(&t))));
    assert!(c.set_glow(None));
    assert!(c.glow().is_none() && c.glow_groups().is_none());
}

#[test]
fn no_glow_costs_nothing() {
    let t = map(&["DDD", "D.D"]);
    let (w, h) = (6, 4);
    let mk = || {
        ColorMap::new(
            &t,
            &image(w, h, |x, _| Rgba::rgb(x as u8, 0, 0)),
            &image(w, h, |_, y| Rgba::rgb(0, y as u8, 0)),
            S,
            theme(),
        )
    };
    let (mut plain, mut lit) = (mk(), mk());
    lit.set_glow(Some(layers(&t)));
    for c in [&mut plain, &mut lit] {
        c.apply(ColorEdit::Carve { hole: 1, x: 1, y: 0 });
        c.apply(ColorEdit::Dirt { hole: 0, x: 1, y: 1 });
    }
    assert!(plain.glow().is_none());
    assert_eq!(plain.pixels(), lit.pixels());
    assert_eq!(plain.material(), lit.material());
    assert_eq!(plain.take_dirty().collect::<Vec<_>>(), lit.take_dirty().collect::<Vec<_>>());
}

#[derive(Clone, Debug)]
enum Op {
    Carve(u8, i32, i32),
    Dirt(u8, i32, i32),
}

proptest! {
    /// After any edits, background shows the back glow, rock the front glow,
    /// and dirt either its original front glow or none (placed dirt).
    #[test]
    fn random_edits_keep_glow_consistent(ops in proptest::collection::vec(
        prop_oneof![
            (0u8..2, -2i32..10, -2i32..8).prop_map(|(h, x, y)| Op::Carve(h, x, y)),
            (0u8..2, -2i32..10, -2i32..8).prop_map(|(h, x, y)| Op::Dirt(h, x, y)),
        ],
        0..40,
    )) {
        let t = map(&["DDDDDDDD", "DD..DDRD", "D....DDD", "DDRRDDDD", "DDDDDDDD", "........"]);
        let mut c = glowing(&t);
        for op in ops {
            c.apply(match op {
                Op::Carve(hole, x, y) => ColorEdit::Carve { hole, x, y },
                Op::Dirt(hole, x, y) => ColorEdit::Dirt { hole, x, y },
            });
        }
        let l = layers(&t);
        let aw = c.width();
        for y in 0..t.height() {
            for x in 0..t.width() {
                let (g, k) = at(&c, x, y);
                let i = ((y * S) * aw + x * S) as usize;
                match c.material().material(x as i64, y as i64).unwrap() {
                    Material::Background => prop_assert_eq!((g, k), (l.back[i], 2)),
                    Material::Rock => prop_assert_eq!((g, k), (l.front[i], 1)),
                    Material::Dirt => prop_assert!((g, k) == (l.front[i], 1) || (g, k) == (0, 0)),
                }
            }
        }
    }
}
