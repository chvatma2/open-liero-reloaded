//! Light soak by material (issue #57): an opaque solid soaks by its front
//! material's `light_soak`, else by the level's.

use olr_look::{Heat, LookFrame, SurfaceMaps, SurfaceMaterial};

use super::*;

/// The pocket's lamp, high on its right: bright, so its light soaks deep.
const LAMP: [f32; 2] = [56.5, 8.5];

/// A 100 × 40 level at art scale 1: rock but for an air pocket from
/// column 40 to 59 (the left block's face at 39, the right's at 60), with
/// holes for dirt.
fn pocket() -> ColorMap {
    let (w, h) = (100, 40);
    let cells =
        (0..w * h).map(|i| if (40..60).contains(&(i % w)) { Material::Background } else { Material::Rock });
    let t = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells.collect()).unwrap());
    let holes = [5, 7, 9, 11, 15].map(|s| Stamp::from_image(&solid(s, s, Rgba::MAGENTA))).to_vec();
    let theme = ThemeArt { holes, green_mask: None, front_tile: solid(4, 4, Rgba::rgb(120, 80, 40)) };
    let grey = solid(w, h, Rgba::rgb(128, 128, 128));
    ColorMap::new(&t, &grey, &grey, 1, theme)
}

/// [`pocket`]'s front map: value 1 left of column `split`, 2 from it.
fn maps(split: u32) -> SurfaceMaps {
    let mut m = SurfaceMaps::empty((100, 40));
    for (i, v) in m.front.iter_mut().enumerate() {
        *v = if (i as u32 % 100) < split { 1 } else { 2 };
    }
    m
}

fn material(id: &str, light_soak: Option<u8>, heats: bool) -> SurfaceMaterial {
    let heat = heats.then(|| Heat {
        sensitivity: 100,
        capacity: 100,
        conductivity: 100,
        cooling: 100,
        ramp: vec![],
        light: 100,
    });
    SurfaceMaterial { id: id.into(), heat, light_soak }
}

/// A dark look soaking `soak`, the pocket's lamp in it.
fn look(soak: u8) -> LookFrame {
    let mut look = dim(0.1);
    look.light_soak = soak;
    look.emitters.push(olr_look::FrameLight { colour: [4.0; 3], ..lamp(LAMP, 1000.0) });
    look
}

fn pocket_list() -> DrawList {
    let mut l = list(View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 });
    l.map = (100, 40);
    l
}

/// A lit renderer of `map` with `materials` on `maps`.
fn soaking(
    gpu: &Offscreen,
    map: &ColorMap,
    materials: &[SurfaceMaterial],
    maps: Option<&SurfaceMaps>,
) -> Renderer {
    let mut r = gpu.renderer(RenderSettings::default());
    r.set_lighting(true);
    r.set_bloom(false);
    r.set_surface(materials, maps);
    r.upload_terrain(map);
    r
}

/// One lit frame of [`pocket_list`] soaking `soak`: the light grid and
/// the picture.
fn frame(gpu: &Offscreen, r: &mut Renderer, soak: u8) -> (Vec<[f32; 3]>, RgbaImage) {
    r.reset_light_history();
    let img = gpu.render_lit_to_image(r, &pocket_list(), Some(&look(soak)), (200, 80));
    (gpu.read_light(r).unwrap().1, img)
}

/// The light at world pixel `(x, y)` of `r`'s last frame.
fn at(gpu: &Offscreen, r: &Renderer, x: i32, y: i32) -> [f32; 3] {
    light_at(gpu, r, x, y)
}

#[test]
fn a_materials_soak_overrides_the_levels() {
    let Some(gpu) = offscreen() else { return };
    let map = pocket();
    // Today's: the level's 6 everywhere; and no soak at all.
    let mut today = soaking(&gpu, &map, &[], None);
    frame(&gpu, &mut today, 6);
    let mut hard = soaking(&gpu, &map, &[], None);
    frame(&gpu, &mut hard, 0);
    let materials = [material("concrete", Some(2), false), material("wood", None, false)];
    let mut r = soaking(&gpu, &map, &materials, Some(&maps(50)));
    assert!(r.has_material_soak() && !r.has_heat());
    frame(&gpu, &mut r, 6);
    for y in 0..40 {
        // The left block (concrete): lit at its face (column 39, which the
        // sum lights) and 2 px past it, the solid floor from 3 px deep
        // (column 36 and deeper).
        for x in 0..40 {
            let (got, floor) = (at(&gpu, &r, x, y), at(&gpu, &hard, x, y));
            if x <= 36 {
                assert_eq!(got, floor, "concrete ({x}, {y}) 3+ px deep");
            } else if x < 39 {
                assert!(got[0] > floor[0] + 1e-3, "concrete ({x}, {y}) behind its face is lit: {got:?}");
            }
        }
        // The pocket and the right block (no soak of its own): today's.
        for x in 40..100 {
            assert_eq!(at(&gpu, &r, x, y), at(&gpu, &today, x, y), "({x}, {y})");
        }
    }
    // Today's 6 px reached deeper into the left block.
    assert!(at(&gpu, &today, 36, 20)[0] > at(&gpu, &hard, 36, 20)[0] + 1e-3);
    // A soak of 0 on the material soaks nothing, even beside the level's 6.
    let zero = [material("glass", Some(0), false), material("wood", None, false)];
    let mut r = soaking(&gpu, &map, &zero, Some(&maps(50)));
    frame(&gpu, &mut r, 6);
    for x in 30..40 {
        assert_eq!(at(&gpu, &r, x, 20), at(&gpu, &hard, x, 20), "glass ({x}, 20)");
    }
    // A level soaking 0 still runs the material's own soak (the across
    // pass too) and leaves the rest hard.
    let deep = [material("felt", Some(6), false), material("wood", None, false)];
    let mut r = soaking(&gpu, &map, &deep, Some(&maps(50)));
    frame(&gpu, &mut r, 0);
    for y in [5, 20, 35] {
        for x in 0..40 {
            assert_eq!(at(&gpu, &r, x, y), at(&gpu, &today, x, y), "felt ({x}, {y}) soaks 6");
        }
        for x in 60..100 {
            assert_eq!(at(&gpu, &r, x, y), at(&gpu, &hard, x, y), "wood ({x}, {y}) soaks the level's 0");
        }
    }
}

#[test]
fn no_material_soak_draws_todays_bytes() {
    let Some(gpu) = offscreen() else { return };
    let map = pocket();
    for soak in [0, 6] {
        let mut plain = soaking(&gpu, &map, &[], None);
        let want = frame(&gpu, &mut plain, soak);
        let cases = [
            vec![material("stone", None, true), material("wood", None, false)],
            vec![material("wood", None, false)],
            vec![material("stone", None, true)],
        ];
        for (i, m) in cases.iter().enumerate() {
            let mut r = soaking(&gpu, &map, m, Some(&maps(50)));
            assert!(!r.has_material_soak(), "case {i}");
            assert!(frame(&gpu, &mut r, soak) == want, "soak {soak}, case {i}: the same light and picture");
        }
        // A soak with maps naming nothing, or not the level's size, is none.
        let soaked = [material("concrete", Some(2), false)];
        let mut wrong = maps(50);
        wrong.world = (50, 80);
        for m in [None, Some(SurfaceMaps::empty((100, 40))), Some(wrong)] {
            let mut r = soaking(&gpu, &map, &soaked, m.as_ref());
            assert!(!r.has_material_soak());
            assert!(frame(&gpu, &mut r, soak) == want, "soak {soak}: maps {:?}", m.map(|m| m.world));
        }
    }
}

#[test]
fn a_soak_material_without_heat_still_works() {
    let Some(gpu) = offscreen() else { return };
    let map = pocket();
    let cold = [material("concrete", Some(2), false), material("wood", None, false)];
    let mut r = soaking(&gpu, &map, &cold, Some(&maps(50)));
    assert!(r.has_material_soak());
    assert!(!r.has_heat(), "no heat textures");
    let want = frame(&gpu, &mut r, 6);
    // The same soak reading the heat's own surface texture.
    let hot = [material("concrete", Some(2), true), material("wood", None, true)];
    let mut h = soaking(&gpu, &map, &hot, Some(&maps(50)));
    assert!(h.has_material_soak() && h.has_heat());
    assert!(frame(&gpu, &mut h, 6) == want, "with heat or without, the same soak");
    // Set after the terrain is up, it takes effect at once; and goes.
    let mut late = soaking(&gpu, &map, &[], None);
    late.set_surface(&cold, Some(&maps(50)));
    assert!(late.has_material_soak());
    assert!(frame(&gpu, &mut late, 6) == want);
    late.set_surface(&[], None);
    assert!(!late.has_material_soak());
    // The live preview's new values (issue #45): the heat kept, the soak
    // changed in place, then gone.
    assert!(h.set_surface_materials(&[material("concrete", Some(6), true), material("wood", None, true)]));
    let mut today = soaking(&gpu, &map, &[], None);
    let six = frame(&gpu, &mut today, 6);
    assert!(h.has_material_soak());
    assert_eq!(frame(&gpu, &mut h, 6).0, six.0, "concrete at the level's 6");
    assert!(h.set_surface_materials(&[material("concrete", None, true), material("wood", None, true)]));
    assert!(!h.has_material_soak());
    assert!(h.set_surface_materials(&hot));
    assert!(h.has_material_soak());
    assert_eq!(frame(&gpu, &mut h, 6).0, want.0);
}

#[test]
fn material_soak_follows_digging() {
    let Some(gpu) = offscreen() else { return };
    // The front map names concrete (soak 2) left of 55: the pocket's left
    // part is painted but open; wood (the level's 6) from 55.
    let materials = [material("concrete", Some(2), false), material("wood", None, false)];
    let mut map = pocket();
    let mut r = soaking(&gpu, &map, &materials, Some(&maps(55)));
    map.take_dirty().for_each(drop);
    frame(&gpu, &mut r, 6);
    // Dirt placed in the pocket's painted part: concrete where it stands.
    map.apply(ColorEdit::Dirt { hole: 4, x: 46, y: 26 });
    r.update_terrain(&mut map);
    let dug = frame(&gpu, &mut r, 6);
    let mut fresh = soaking(&gpu, &map, &materials, Some(&maps(55)));
    assert!(frame(&gpu, &mut fresh, 6) == dug, "a dirty rectangle equals a whole upload");
    // The same dirt with wood's front soaks the level's 6.
    let mut wood = soaking(&gpu, &map, &materials, Some(&maps(40)));
    frame(&gpu, &mut wood, 6);
    let mut hard = soaking(&gpu, &map, &[], None);
    frame(&gpu, &mut hard, 0);
    // A new dirt pixel 4 or 5 px from the nearest air (3 or 4 past the
    // face the sum lights), left of 55.
    let cells = map.material().cells();
    let open = |x: i32, y: i32| {
        !(0..100).contains(&x)
            || !(0..40).contains(&y)
            || cells[(y * 100 + x) as usize] == Material::Background as u8
    };
    let depth = |x: i32, y: i32| {
        (1..=6).find(|d| (-d..=*d).any(|i| (-d..=*d).any(|j| open(x + i, y + j)))).unwrap_or(7)
    };
    let pixel = (41..55)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .find(|&(x, y)| {
            cells[(y * 100 + x) as usize] == Material::Dirt as u8 && (4..=5).contains(&depth(x, y))
        })
        .expect("the dirt has a pixel 4 or 5 px deep");
    let (x, y) = pixel;
    assert_eq!(at(&gpu, &r, x, y), at(&gpu, &hard, x, y), "concrete dirt {pixel:?}: the solid floor");
    assert!(at(&gpu, &wood, x, y)[0] > at(&gpu, &hard, x, y)[0] + 1e-3, "wood dirt {pixel:?} soaks 6");
}
