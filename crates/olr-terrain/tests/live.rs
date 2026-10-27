//! Live copies drawn over the ground (issue #87): the colour map's shown
//! composite takes the parts, the ground (its edits, `material()`,
//! `rebuild_area`) never does.

use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_terrain::{
    Area, ColorEdit, ColorMap, GlowBeamMap, GlowLayers, LiveArt, LiveFrame, LivePicture, LivePlaced, Rect,
    Stamp, Terrain, ThemeArt, live_glow,
};

/// Art pixels per world pixel.
const S: u32 = 2;

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
fn terrain(rows: &[&str]) -> Terrain {
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

const FRONT: Rgba = Rgba::rgb(200, 100, 0);
const BACK: Rgba = Rgba::rgb(0, 0, 80);
const RED: Rgba = Rgba::rgb(255, 0, 0);
const GREEN: Rgba = Rgba::rgb(0, 255, 0);

/// A 32 × 20 level: rock along the top row, dirt in columns 20..24, the
/// rest background; front art `FRONT` on solids, `BACK` elsewhere.
fn level() -> Terrain {
    let mut rows = vec![String::from("RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRR")];
    for _ in 1..20 {
        rows.push(format!("{}DDDD{}", ".".repeat(20), ".".repeat(8)));
    }
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    terrain(&rows)
}

fn map_of(t: &Terrain) -> ColorMap {
    let (w, h) = (t.width() * S, t.height() * S);
    let air = |x: u32, y: u32| t.material(i64::from(x / S), i64::from(y / S)) == Some(Material::Background);
    let front = image(w, h, |x, y| if air(x, y) { BACK } else { FRONT });
    let back = image(w, h, |_, _| BACK);
    let theme = ThemeArt {
        holes: vec![Stamp::from_image(&image(3, 3, |_, _| Rgba::MAGENTA))],
        green_mask: None,
        front_tile: image(2, 2, |_, _| Rgba::rgb(9, 9, 9)),
    };
    ColorMap::new(t, &front, &back, S, theme)
}

/// A picture of `size` world pixels all `c` at alpha `a`, rock (solid
/// everywhere) or not, glowing `glow` (mask) in group `group` when given.
fn picture(size: (u32, u32), c: Rgba, a: u8, rock: bool, glow: Option<(u8, u8)>) -> LivePicture {
    let (aw, ah) = (size.0 * S, size.1 * S);
    let art = (0..aw * ah).flat_map(|_| [c.r, c.g, c.b, a]).collect();
    let world = (size.0 * size.1) as usize;
    LivePicture {
        size,
        art,
        rock: rock.then(|| vec![1; world]),
        glow: glow.map(|(m, _)| vec![m; (aw * ah) as usize]),
        groups: glow.map(|(_, g)| vec![g; world]),
    }
}

fn part(picture: u16, rock: bool, at: (i32, i32)) -> LivePlaced {
    LivePlaced { picture, rock, at, flip: false, alpha: 255, glow: 100 }
}

/// Copy 0: picture 0 a red rock block 4 × 4 glowing 200 in group 3,
/// picture 1 a green background panel 6 × 6; copy 1: a green rock 2 × 2.
fn art() -> Arc<LiveArt> {
    Arc::new(LiveArt {
        copies: vec![
            vec![picture((4, 4), RED, 255, true, Some((200, 3))), picture((6, 6), GREEN, 255, false, None)],
            vec![picture((2, 2), GREEN, 255, true, None)],
        ],
    })
}

/// A frame of `copies`' parts.
fn frame(copies: &[&[LivePlaced]]) -> LiveFrame {
    let mut f = LiveFrame::default();
    for c in copies {
        for p in *c {
            f.push(*p);
        }
        f.end_copy();
    }
    f
}

/// The shown colour of art pixel `(x, y)`.
fn shown(m: &ColorMap, x: u32, y: u32) -> Rgba {
    let i = ((y * m.width() + x) * 4) as usize;
    let p = &m.pixels()[i..i + 4];
    Rgba { r: p[0], g: p[1], b: p[2], a: p[3] }
}

/// World pixel `(x, y)`'s top-left art pixel, shown.
fn at(m: &ColorMap, x: u32, y: u32) -> Rgba {
    shown(m, x * S, y * S)
}

fn drain(m: &mut ColorMap) -> Vec<Rect> {
    m.take_dirty().collect()
}

/// A live map of [`level`] with a glow channel, drained clean.
fn live_map() -> ColorMap {
    let t = level();
    let mut m = map_of(&t);
    m.set_glow(Some(GlowLayers::empty((m.width(), m.height()), (t.width(), t.height()))));
    m.set_live_art(Some(art()));
    drain(&mut m);
    m
}

#[test]
fn a_live_rock_part_draws_over_the_ground() {
    let mut m = live_map();
    // Over air and over the dirt column alike, it hides what is under it.
    m.set_live(&frame(&[&[part(0, true, (5, 5)), part(0, true, (19, 10))], &[]]));
    assert_eq!(at(&m, 5, 5), RED);
    assert_eq!(at(&m, 8, 8), RED);
    assert_eq!(at(&m, 9, 9), BACK, "past the picture");
    assert_eq!(at(&m, 21, 12), RED, "over dirt");
    // Its glow and group over the ground's.
    let i = ((5 * S) * m.width() + 5 * S) as usize;
    assert_eq!(m.glow().unwrap()[i], live_glow(100, 200));
    assert_eq!(m.glow_groups().unwrap()[5 * 32 + 5], 3);
    // A rock part above it in a later copy covers it.
    m.set_live(&frame(&[&[part(0, true, (5, 5))], &[part(0, true, (6, 6))]]));
    assert_eq!(at(&m, 6, 6), GREEN);
    assert_eq!(at(&m, 5, 5), RED);
}

#[test]
fn a_background_part_shows_only_where_the_ground_is_air() {
    let mut m = live_map();
    // The panel spans air (18, 19), dirt (20..24) and the rock row's (0).
    m.set_live(&frame(&[&[part(1, false, (18, 0))], &[]]));
    assert_eq!(at(&m, 18, 3), GREEN, "on air");
    assert_eq!(at(&m, 21, 3), FRONT, "dirt hides it");
    assert_eq!(at(&m, 18, 0), FRONT, "rock hides it");
    // In the stack's order: a rock part above it covers it, one below it
    // is covered (and open for the light there, as the static bake's
    // background element above rock).
    m.set_live(&frame(&[&[part(1, false, (18, 0)), part(0, true, (16, 2))], &[]]));
    assert_eq!(at(&m, 18, 3), RED);
    assert_eq!(at(&m, 18, 1), GREEN, "beside the rock part");
    m.set_live(&frame(&[&[part(0, true, (16, 2)), part(1, false, (18, 0))], &[]]));
    assert_eq!(at(&m, 18, 3), GREEN);
    assert_eq!(at(&m, 16, 3), RED, "where the panel is not");
    assert_eq!(m.live_solid().unwrap()[3 * 32 + 18], 0);
    assert_eq!(m.live_solid().unwrap()[3 * 32 + 16], 1);
}

#[test]
fn digging_under_a_background_part_reveals_it() {
    let mut m = live_map();
    m.set_live(&frame(&[&[part(1, false, (19, 4))], &[]]));
    assert_eq!(at(&m, 21, 6), FRONT);
    m.apply(ColorEdit::Carve { hole: 0, x: 21, y: 6 });
    let dirty = drain(&mut m);
    assert_eq!(at(&m, 21, 6), GREEN, "the dug pixel shows the part at once");
    assert_eq!(m.ground_pixels(), ground_after_carve().ground_pixels(), "the ground as without it");
    assert!(dirty.iter().any(|r| r.x <= 21 * S && r.x + r.w > 21 * S && r.y <= 6 * S && r.y + r.h > 6 * S));
}

/// [`level`] with the same carve as [`digging_under_a_background_part_reveals_it`], without live copies.
fn ground_after_carve() -> ColorMap {
    let t = level();
    let mut m = map_of(&t);
    m.set_glow(Some(GlowLayers::empty((m.width(), m.height()), (t.width(), t.height()))));
    m.apply(ColorEdit::Carve { hole: 0, x: 21, y: 6 });
    m
}

#[test]
fn the_ground_never_sees_live_parts() {
    let t = level();
    let mut plain = map_of(&t);
    let mut live = map_of(&t);
    live.set_live_art(Some(art()));
    live.set_live(&frame(&[&[part(0, true, (19, 3)), part(1, false, (10, 5))], &[part(0, true, (0, 0))]]));
    let edits = [
        ColorEdit::Carve { hole: 0, x: 21, y: 4 },
        ColorEdit::Dirt { hole: 0, x: 11, y: 7 },
        ColorEdit::Carve { hole: 0, x: 1, y: 1 },
    ];
    for e in edits {
        plain.apply(e);
        live.apply(e);
    }
    let mut world = t.clone();
    world.carve_hole(&Stamp::from_image(&image(3, 3, |_, _| Rgba::MAGENTA)), 22, 9);
    let area = Area { x0: 15, y0: 0, x1: 30, y1: 20 };
    plain.rebuild_area(area, &world);
    live.rebuild_area(area, &world);
    drain(&mut live);
    assert_eq!(live.material(), plain.material());
    assert_eq!(live.ground_pixels(), plain.pixels());
    assert_ne!(live.pixels(), plain.pixels(), "the parts are shown");
    assert_eq!(at(&live, 20, 4), RED, "over the rebuilt area too");
}

#[test]
fn a_state_change_dirties_only_its_rectangle() {
    let mut m = live_map();
    m.set_live(&frame(&[&[part(0, true, (2, 4))], &[part(0, true, (24, 10))]]));
    drain(&mut m);
    // Copy 1's state changes: only its rectangle is drawn again.
    m.set_live(&frame(&[&[part(0, true, (2, 4))], &[part(0, true, (24, 10)), part(1, false, (26, 12))]]));
    let dirty = drain(&mut m);
    assert!(!dirty.is_empty());
    // Copy 0's dirty tile (world 0..16) is not among them.
    for r in &dirty {
        assert!(r.x >= ColorMap::DIRTY_TILE * S, "{r:?} reaches copy 0");
    }
    assert_eq!(at(&m, 2, 4), RED, "copy 0 as it was");
    assert_eq!(at(&m, 24, 10), GREEN);
}

#[test]
fn an_unchanged_live_layer_dirties_nothing() {
    let mut m = live_map();
    let f = frame(&[&[part(0, true, (2, 4)), part(1, false, (8, 8))], &[part(0, true, (24, 10))]]);
    m.set_live(&f);
    assert!(!drain(&mut m).is_empty());
    let before = m.pixels().to_vec();
    m.set_live(&f);
    assert!(drain(&mut m).is_empty(), "an unchanged frame uploads nothing");
    assert_eq!(m.pixels(), &before[..]);
}

#[test]
fn facing_mirrors_a_copy() {
    // A picture 4 × 1: red, red, green, green, rock.
    let mut pic = picture((4, 1), RED, 255, true, None);
    for x in 2 * S..4 * S {
        for y in 0..S {
            let i = ((y * 4 * S + x) * 4) as usize;
            pic.art[i..i + 3].copy_from_slice(&[GREEN.r, GREEN.g, GREEN.b]);
        }
    }
    let t = level();
    let mut m = map_of(&t);
    m.set_live_art(Some(Arc::new(LiveArt { copies: vec![vec![pic]] })));
    m.set_live(&frame(&[&[part(0, true, (4, 6))]]));
    let row = |m: &ColorMap| (0..4).map(|x| at(m, 4 + x, 6)).collect::<Vec<_>>();
    assert_eq!(row(&m), [RED, RED, GREEN, GREEN]);
    m.set_live(&frame(&[&[LivePlaced { flip: true, ..part(0, true, (4, 6)) }]]));
    assert_eq!(row(&m), [GREEN, GREEN, RED, RED]);
    // Within a world pixel too: its art pixels mirror.
    assert_eq!(shown(&m, 4 * S, 6 * S), GREEN);
}

#[test]
fn opacity_scales_a_background_parts_alpha() {
    let mut m = live_map();
    let half = LivePlaced { alpha: 128, ..part(1, false, (2, 2)) };
    m.set_live(&frame(&[&[half], &[]]));
    let c = at(&m, 3, 3);
    // Green at 128 over the back blue, straight alpha, opaque.
    assert_eq!(c.a, 255);
    let out = 128 * 255 + 255 * 127;
    let mix = |s: u32, d: u32| ((s * 128 * 255 + d * 255 * 127 + out / 2) / out) as u8;
    assert_eq!((c.r, c.g, c.b), (0, mix(255, 0), mix(0, 80)));
    m.set_live(&frame(&[&[LivePlaced { alpha: 0, ..half }], &[]]));
    assert_eq!(at(&m, 3, 3), BACK, "none at 0");
    m.set_live(&frame(&[&[LivePlaced { alpha: 255, ..half }], &[]]));
    assert_eq!(at(&m, 3, 3), GREEN);
}

#[test]
fn live_rock_is_solid_for_the_light_but_not_for_the_colour_map() {
    let mut m = live_map();
    let before = m.material().clone();
    m.set_live(&frame(&[&[part(0, true, (5, 5)), part(1, false, (12, 5))], &[]]));
    let solid = m.live_solid().unwrap();
    assert_eq!(solid[5 * 32 + 5], 1);
    assert_eq!(solid[8 * 32 + 8], 1);
    assert_eq!(solid[9 * 32 + 9], 0);
    assert_eq!(solid[6 * 32 + 13], 0, "a background part is not solid");
    assert_eq!(m.material(), &before, "the material never takes it");
    m.set_live(&frame(&[&[], &[]]));
    assert!(m.live_solid().unwrap().iter().all(|&v| v == 0), "gone with the part");
    assert_eq!(map_of(&level()).live_solid(), None, "none without live copies");
}

#[test]
fn the_static_beams_never_see_live_copies() {
    let t = level();
    let mut m = map_of(&t);
    let (art, world) = ((m.width(), m.height()), (t.width(), t.height()));
    m.set_glow(Some(GlowLayers::empty(art, world)));
    let n = (world.0 * world.1 * 2) as usize;
    let beams = GlowBeamMap { world, front: vec![0; n], back: vec![0; n] };
    assert!(m.set_glow_beams(Some(beams)));
    let (layers, arts, generation) =
        (m.glow_layers().cloned(), m.art_layers().0.to_vec(), m.beams_generation());
    m.set_live_art(Some(art_of_glow()));
    m.set_live(&frame(&[&[part(0, true, (5, 5))]]));
    drain(&mut m);
    assert_eq!(m.beams_generation(), generation, "no new derivation");
    assert_eq!(m.glow_layers().cloned(), layers, "the beams' glow is the ground's");
    assert_eq!(m.art_layers().0, &arts[..]);
    assert!(m.glow().unwrap().iter().any(|&g| g != 0), "while the shown glow has the part's");
}

fn art_of_glow() -> Arc<LiveArt> {
    Arc::new(LiveArt { copies: vec![vec![picture((4, 4), RED, 255, true, Some((255, 1)))]] })
}

#[test]
fn a_level_lit_only_by_live_copies_gets_a_glow_channel() {
    let t = level();
    let mut m = map_of(&t);
    assert!(m.glow().is_none());
    m.set_live_art(Some(art_of_glow()));
    assert!(m.glow().is_some() && m.glow_groups().is_some(), "an empty channel, so the look uploads glow");
    m.set_live(&frame(&[&[part(0, true, (5, 5))]]));
    assert_eq!(m.glow_groups().unwrap()[5 * 32 + 5], 1);
    // Pictures that never glow add none.
    let mut plain = map_of(&t);
    plain.set_live_art(Some(Arc::new(LiveArt { copies: vec![vec![picture((2, 2), RED, 255, true, None)]] })));
    assert!(plain.glow().is_none());
}

#[test]
fn hostile_frames_never_panic() {
    let mut m = live_map();
    let far = [
        part(9, true, (0, 0)),
        part(0, true, (i32::MIN, i32::MAX)),
        part(1, false, (-3, 18)),
        part(0, true, (30, -2)),
        LivePlaced { flip: true, ..part(1, false, (29, 17)) },
    ];
    m.set_live(&frame(&[&far, &far, &far]));
    drain(&mut m);
    // A picture whose buffers do not fit draws nothing.
    let bad = LivePicture { size: (3, 3), art: vec![1; 5], rock: Some(vec![1; 2]), glow: None, groups: None };
    m.set_live_art(Some(Arc::new(LiveArt { copies: vec![vec![bad]] })));
    m.set_live(&frame(&[&[part(0, true, (1, 1))]]));
    assert_eq!(at(&m, 1, 1), BACK);
}
