//! The bake (plan 14a Task 5): a composition baked into the level's
//! material, front and back, whole or a region at a time.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::{
    Baker, Decoded, GRIDS_PER_SOURCE, ImageCache, Rect, bake, bake_elements, compose_from, decode_element,
    encode_decoded,
};
use olr_format::image_io;
use olr_format::levels::{
    CompositionSource, Dither, Element, ElementKind, ElementMaterial, LevelSource, image_file, layer_file,
};
use olr_vfs::MemFs;
use proptest::prelude::*;

use crate::common::content;

const BLUE: Rgba = Rgba::rgb(20, 40, 200);
const BROWN: Rgba = Rgba::rgb(140, 90, 40);

/// A `w` × `h` level at art scale `s`: all dirt, zero art.
fn level(w: u32, h: u32, s: u32) -> LevelSource {
    LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
    }
}

fn back_tile() -> RgbaImage {
    let mut t = RgbaImage::new(2, 2);
    for (i, c) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
        t.set(c.0, c.1, Rgba::rgb(10 * i as u8, 7, 7));
    }
    t
}

fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

fn element(id: &str, kind: ElementKind) -> Element {
    Element { id: id.into(), name: String::new(), visible: true, locked: false, kind }
}

/// Adds an image element `id` (source `<id>.png`) on top.
fn with_image(
    src: &mut CompositionSource,
    id: &str,
    img: RgbaImage,
    at: (i32, i32),
    size: (u32, u32),
    m: ElementMaterial,
) {
    let source = format!("{id}.png");
    src.files.insert(image_file(&source), image_io::encode_rgba(&img));
    src.composition.elements.push(element(
        id,
        ElementKind::Image {
            material: m,
            source,
            at,
            size,
            pixel_size: 1,
            colours: None,
            dither: Dither::None,
            alpha_threshold: 128,
            mask: false,
        },
    ));
}

/// Adds a drawn layer `id` holding `art` (art resolution) on top.
fn with_drawn(src: &mut CompositionSource, id: &str, art: &RgbaImage, m: ElementMaterial) {
    src.files.insert(layer_file(id), image_io::encode_rgba(art));
    src.composition.elements.push(element(id, ElementKind::drawn(m)));
}

/// Adds a drawn layer `id` at `at` (world) of `size` (`None`: the level's,
/// `level`) on top: `art` its art, `cells` its per-pixel material (mixed),
/// `back` its back art.
#[allow(clippy::too_many_arguments)]
fn with_placed(
    src: &mut CompositionSource,
    id: &str,
    at: (i32, i32),
    size: Option<(u32, u32)>,
    level: (u32, u32),
    art: &RgbaImage,
    cells: Option<&[Material]>,
    back: Option<&RgbaImage>,
    m: ElementMaterial,
) {
    let files = olr_format::levels::drawn_files(id, cells.is_some(), back.is_some());
    src.files.insert(files[0].clone(), image_io::encode_rgba(art));
    if let Some(c) = cells {
        src.files.insert(files[1].clone(), olr_format::levels::encode_material(size.unwrap_or(level), c));
    }
    if let Some(b) = back {
        src.files.insert(files.last().unwrap().clone(), image_io::encode_rgba(b));
    }
    src.composition.elements.push(element(
        id,
        ElementKind::Drawn { material: m, at, size, mixed: cells.is_some(), back: back.is_some() },
    ));
}

/// `front == back` on every art pixel of a background block.
fn background_invariant(l: &LevelSource) -> bool {
    let s = l.art_scale;
    (0..l.size.1 * s).all(|ay| {
        (0..l.size.0 * s).all(|ax| {
            l.material[((ay / s) * l.size.0 + ax / s) as usize] != Material::Background
                || l.front.get(ax, ay) == l.back.get(ax, ay)
        })
    })
}

/// Overwrites `r` (clipped) of `l` with junk.
fn scramble(l: &mut LevelSource, r: Rect) {
    let s = l.art_scale;
    for y in r.y..(r.y + r.h).min(l.size.1) {
        for x in r.x..(r.x + r.w).min(l.size.0) {
            l.material[(y * l.size.0 + x) as usize] = Material::Rock;
            for ay in y * s..(y + 1) * s {
                for ax in x * s..(x + 1) * s {
                    l.front.set(ax, ay, Rgba::rgb(1, 2, 3));
                    l.back.set(ax, ay, Rgba::rgb(3, 2, 1));
                }
            }
        }
    }
}

/// A stack from `spec`: (kind, material, x, y, w, h, visible); kind 0 an
/// image of a 3 × 3 gradient with a transparent corner, 1 a drawn layer
/// of a checkerboard, 2 the raster base of a random-ish level, 3 a mixed
/// and backed checkerboard layer at `(x, y)` (of `(w, h)`, or the level's
/// size when `w` is a multiple of 4), 4 a kind-0 image with its middle
/// pixel masked.
fn random_stack(spec: &[(u8, u8, i32, i32, u32, u32, bool)], size: (u32, u32), s: u32) -> CompositionSource {
    let mut src = CompositionSource::default();
    for (i, &(kind, m, x, y, w, h, visible)) in spec.iter().enumerate() {
        let m = [ElementMaterial::Rock, ElementMaterial::Dirt, ElementMaterial::Background][m as usize];
        let id = format!("e{i}");
        match kind {
            0 => {
                let mut img = solid(3, 3, Rgba::rgb(x as u8, y as u8, 9));
                img.set(0, 0, Rgba { r: 0, g: 0, b: 0, a: 0 });
                with_image(&mut src, &id, img, (x, y), (w, h), m);
            }
            3 => {
                let boxed = (!w.is_multiple_of(4)).then_some((w, h));
                let (bw, bh) = boxed.unwrap_or(size);
                let mut art = RgbaImage::new(bw * s, bh * s);
                let mut back = RgbaImage::new(bw * s, bh * s);
                for ay in 0..bh * s {
                    for ax in 0..bw * s {
                        if !(ax + 2 * ay + i as u32).is_multiple_of(3) {
                            art.set(ax, ay, Rgba::rgb(7, i as u8, 70));
                        }
                        if (ax + ay).is_multiple_of(2) {
                            back.set(ax, ay, Rgba::rgb(70, 7, i as u8));
                        }
                    }
                }
                let cells: Vec<Material> = (0..bw * bh)
                    .map(|k| [Material::Rock, Material::Background, Material::Dirt][(k as usize + i) % 3])
                    .collect();
                with_placed(&mut src, &id, (x, y), boxed, size, &art, Some(&cells), Some(&back), m);
            }
            4 => {
                let mut img = solid(3, 3, Rgba::rgb(x as u8, y as u8, 9));
                img.set(0, 0, Rgba { r: 0, g: 0, b: 0, a: 0 });
                with_image(&mut src, &id, img, (x, y), (w, h), m);
                let mut mask = olr_edit::mask::Mask::full((3, 3));
                mask.keep[4] = 0;
                with_mask(&mut src, &id, &mask);
            }
            1 => {
                let mut art = RgbaImage::new(size.0 * s, size.1 * s);
                for ay in 0..size.1 * s {
                    for ax in 0..size.0 * s {
                        if (ax + ay + i as u32).is_multiple_of(3) {
                            art.set(ax, ay, Rgba::rgb(9, i as u8, 99));
                        }
                    }
                }
                with_drawn(&mut src, &id, &art, m);
            }
            _ => {
                let mut l = level(size.0, size.1, s);
                for (k, cell) in l.material.iter_mut().enumerate() {
                    *cell = [Material::Rock, Material::Background, Material::Dirt][(k + i) % 3];
                }
                l.front = solid(size.0 * s, size.1 * s, BROWN);
                l.back = solid(size.0 * s, size.1 * s, BLUE);
                // Keep the raster's own invariant: front = back on background.
                for (k, cell) in l.material.clone().iter().enumerate() {
                    if *cell == Material::Background {
                        let (x, y) = (k as u32 % size.0, k as u32 / size.0);
                        for ay in y * s..(y + 1) * s {
                            for ax in x * s..(x + 1) * s {
                                l.front.set(ax, ay, BLUE);
                            }
                        }
                    }
                }
                let raster = compose_from(&l);
                let mut e = raster.composition.elements[0].clone();
                e.id = id.clone();
                for (path, bytes) in raster.files {
                    src.files.insert(path.replace("/base.", &format!("/{id}.")), bytes);
                }
                src.composition.elements.push(e);
            }
        }
        src.composition.elements.last_mut().unwrap().visible = visible;
    }
    src
}

/// Plan 14d R2/R4: a drawn layer covers only its box, placed at `at`.
#[test]
fn a_placed_layer_bakes_at_its_box() {
    let mut src = CompositionSource::default();
    let art = solid(4, 4, BROWN);
    with_placed(&mut src, "crate", (2, 1), Some((2, 2)), (6, 4), &art, None, None, ElementMaterial::Dirt);
    let mut l = level(6, 4, 2);
    l.material = vec![Material::Background; 24];
    bake(&mut l, &src, &back_tile()).unwrap();
    for y in 0..4u32 {
        for x in 0..6u32 {
            let inside = (2..4).contains(&x) && (1..3).contains(&y);
            let want = if inside { Material::Dirt } else { Material::Background };
            assert_eq!(l.material[(y * 6 + x) as usize], want, "({x}, {y})");
            if inside {
                assert_eq!(l.front.get(x * 2 + 1, y * 2), BROWN);
            }
        }
    }
}

/// Plan 14d R4: a mixed layer's material is its cells'; its back art is
/// the backdrop under its dirt; its background pixels are backdrop.
#[test]
fn a_mixed_layer_bakes_its_cells_and_back() {
    let mut src = CompositionSource::default();
    let art = solid(3, 1, BROWN);
    let cells = [Material::Rock, Material::Dirt, Material::Background];
    let mut back = RgbaImage::new(3, 1);
    back.set(1, 0, BLUE);
    with_placed(&mut src, "m", (0, 0), None, (3, 1), &art, Some(&cells), Some(&back), ElementMaterial::Dirt);
    let mut l = level(3, 1, 1);
    bake(&mut l, &src, &back_tile()).unwrap();
    assert_eq!(l.material, cells);
    assert_eq!(l.back.get(1, 0), BLUE, "carving the dirt shows its back art");
    assert_eq!(l.back.get(2, 0), BROWN, "a background pixel is backdrop");
    assert_eq!(l.front.get(2, 0), l.back.get(2, 0));
    assert_eq!(l.front.get(0, 0), BROWN);
}

/// Plan 14d R2: a level resize moves a boxed layer by its offset and
/// re-lays a level-sized one (its cells, the new area of its material).
#[test]
fn a_resize_moves_a_boxed_layer_and_relays_a_level_sized_one() {
    use olr_edit::compose::resize_decoded;
    use olr_edit::level::Paint;
    let kind = |material, at, size, mixed| ElementKind::Drawn { material, at, size, mixed, back: false };
    let mut e = element("b", kind(ElementMaterial::Dirt, (1, 1), Some((1, 1)), false));
    let mut d = Decoded::Drawn { art: solid(1, 1, BROWN), cells: None, back: None };
    let paint = Paint::Colour(BLUE);
    resize_decoded(&mut e, &mut d, (2, 2), 1, (5, 3), (3, 1), Material::Rock, &paint).unwrap();
    assert!(matches!(e.kind, ElementKind::Drawn { at: (4, 2), size: Some((1, 1)), .. }), "{:?}", e.kind);
    assert_eq!(d, Decoded::Drawn { art: solid(1, 1, BROWN), cells: None, back: None });
    let mut e = element("l", kind(ElementMaterial::Rock, (0, 0), None, true));
    let mut d = Decoded::Drawn { art: solid(2, 2, BROWN), cells: Some(vec![Material::Dirt; 4]), back: None };
    resize_decoded(&mut e, &mut d, (2, 2), 1, (5, 3), (3, 1), Material::Rock, &paint).unwrap();
    let Decoded::Drawn { art, cells: Some(cells), .. } = &d else { panic!("{d:?}") };
    assert_eq!((art.width(), art.height()), (5, 3));
    assert_eq!(cells.len(), 15);
    assert_eq!(cells[5 + 3], Material::Dirt, "the old cells moved by the offset");
    assert_eq!(cells[0], Material::Rock, "the new area takes the layer's material");
    assert_eq!(art.get(0, 0).a, 0, "the new area is uncovered");
    assert_eq!(art.get(3, 1), BROWN);
}

/// Batch A review I1: a level-sized layer moved off the origin keeps its
/// pixels through a resize: it is pinned to its box and moved, as boxed
/// layers are.
#[test]
fn a_moved_level_sized_layer_keeps_its_pixels_through_a_resize() {
    use olr_edit::compose::resize_decoded;
    use olr_edit::level::Paint;
    let mut art = RgbaImage::new(10, 1);
    for x in 0..10 {
        art.set(x, 0, Rgba::rgb(x as u8 * 10, 0, 0));
    }
    let kind = ElementKind::Drawn {
        material: ElementMaterial::Dirt,
        at: (-5, 0),
        size: None,
        mixed: false,
        back: false,
    };
    let mut e = element("m", kind);
    let mut d = Decoded::Drawn { art: art.clone(), cells: None, back: None };
    resize_decoded(&mut e, &mut d, (10, 1), 1, (5, 1), (0, 0), Material::Rock, &Paint::Colour(BLUE)).unwrap();
    assert!(matches!(e.kind, ElementKind::Drawn { at: (-5, 0), size: Some((10, 1)), .. }), "{:?}", e.kind);
    assert_eq!(d, Decoded::Drawn { art, cells: None, back: None });
}

/// Review Focus 3: a mixed layer's material file holding a palette index
/// that is no material is an error naming it.
#[test]
fn a_bad_material_index_is_an_error_naming_it() {
    let mut src = CompositionSource::default();
    with_placed(
        &mut src,
        "m",
        (0, 0),
        None,
        (2, 1),
        &solid(2, 1, BROWN),
        Some(&[Material::Dirt; 2]),
        None,
        ElementMaterial::Dirt,
    );
    let file = olr_format::levels::drawn_files("m", true, false)[1].clone();
    let palette: Vec<u8> = (0..8 * 3).map(|i| i as u8).collect();
    src.files.insert(file.clone(), image_io::encode_indexed(2, 1, &[7, 2], &palette));
    let e = bake(&mut level(2, 1, 1), &src, &back_tile()).unwrap_err().to_string();
    assert!(e.contains(&file) && e.contains("7"), "{e}");
}

#[test]
fn an_empty_composition_is_open_air_over_the_theme_tile() {
    let mut l = level(4, 3, 2);
    bake(&mut l, &CompositionSource::default(), &back_tile()).unwrap();
    assert!(l.material.iter().all(|m| *m == Material::Background));
    for y in 0..6 {
        for x in 0..8 {
            assert_eq!(l.back.get(x, y), back_tile().get(x % 2, y % 2));
            assert_eq!(l.front.get(x, y), l.back.get(x, y));
        }
    }
    assert!(l.thumb.is_some());
}

#[test]
fn topmost_wins_and_background_elements_are_the_backdrop() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "sky", solid(4, 3, BLUE), (0, 0), (4, 3), ElementMaterial::Background);
    with_image(&mut src, "dirt", solid(2, 3, BROWN), (0, 0), (2, 3), ElementMaterial::Dirt);
    let mut l = level(4, 3, 1);
    bake(&mut l, &src, &back_tile()).unwrap();
    for y in 0..3 {
        for x in 0..4 {
            let dirt = x < 2;
            assert_eq!(
                l.material[(y * 4 + x) as usize],
                if dirt { Material::Dirt } else { Material::Background }
            );
            assert_eq!(l.front.get(x, y), if dirt { BROWN } else { BLUE });
            assert_eq!(l.back.get(x, y), BLUE, "carving the dirt reveals the sky");
        }
    }
}

#[test]
fn hidden_elements_are_left_out() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "sky", solid(4, 3, BLUE), (0, 0), (4, 3), ElementMaterial::Background);
    with_image(&mut src, "dirt", solid(2, 3, BROWN), (0, 0), (2, 3), ElementMaterial::Dirt);
    src.composition.elements[1].visible = false;
    let mut l = level(4, 3, 1);
    bake(&mut l, &src, &back_tile()).unwrap();
    assert!(l.material.iter().all(|m| *m == Material::Background));
    assert!((0..3).all(|y| (0..4).all(|x| l.front.get(x, y) == BLUE)));
}

#[test]
fn every_bundled_level_round_trips_through_a_raster_base() {
    let Some(root) = content() else { return };
    for id in [
        "olx-level-castlestrike",
        "olx-level-dirt-level",
        "olx-level-fossilfacility",
        "olx-level-liero-factory",
    ] {
        let original = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
        let src = compose_from(&original);
        let mut baked = original.clone();
        baked.material.fill(Material::Rock);
        baked.front = RgbaImage::new(baked.front.width(), baked.front.height());
        baked.back = baked.front.clone();
        bake(&mut baked, &src, &back_tile()).unwrap();
        let files = |l: &LevelSource| {
            let mut fs = MemFs::new();
            olr_format::levels::write_level_files(&mut fs, l).unwrap();
            fs
        };
        assert!(files(&baked) == files(&original), "{id} changed");
    }
}

/// A plain level split into three drawn layers (the editor's way to open
/// one: background, dirt, rock) bakes to its files byte for byte, and its
/// layers hold what their names say.
#[test]
fn every_bundled_level_round_trips_through_three_layers() {
    let Some(root) = content() else { return };
    for id in [
        "olx-level-castlestrike",
        "olx-level-dirt-level",
        "olx-level-fossilfacility",
        "olx-level-liero-factory",
    ] {
        let original = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
        let src = olr_edit::compose::split_from(&original);
        let kinds: Vec<_> =
            src.composition.elements.iter().map(|e| (e.id.as_str(), e.kind.clone())).collect();
        assert_eq!(
            kinds,
            [
                ("background", ElementKind::drawn(ElementMaterial::Background)),
                ("dirt", ElementKind::drawn(ElementMaterial::Dirt)),
                ("rock", ElementKind::drawn(ElementMaterial::Rock)),
            ],
            "{id}"
        );
        let mut baked = original.clone();
        baked.material.fill(Material::Rock);
        baked.front = RgbaImage::new(baked.front.width(), baked.front.height());
        baked.back = baked.front.clone();
        bake(&mut baked, &src, &back_tile()).unwrap();
        let files = |l: &LevelSource| {
            let mut fs = MemFs::new();
            olr_format::levels::write_level_files(&mut fs, l).unwrap();
            fs
        };
        assert!(files(&baked) == files(&original), "{id} changed");
    }
}

#[test]
fn elements_outside_the_level_are_clipped() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "far", solid(3, 3, BLUE), (-100, -100), (3, 3), ElementMaterial::Rock);
    with_image(&mut src, "big", solid(9, 9, BROWN), (-2, -2), (4096, 4096), ElementMaterial::Dirt);
    with_image(&mut src, "edge", solid(3, 3, BLUE), (3, 2), (3, 3), ElementMaterial::Rock);
    let mut l = level(4, 3, 2);
    bake(&mut l, &src, &back_tile()).unwrap();
    // `far` reaches nothing; `big`, clipped, covers all but the edge's one
    // inside pixel with its colour.
    for y in 0..3u32 {
        for x in 0..4u32 {
            let edge = (x, y) == (3, 2);
            assert_eq!(
                l.material[(y * 4 + x) as usize],
                if edge { Material::Rock } else { Material::Dirt },
                "({x}, {y})"
            );
            for (ax, ay) in [(x * 2, y * 2), (x * 2 + 1, y * 2 + 1)] {
                assert_eq!(l.front.get(ax, ay), if edge { BLUE } else { BROWN }, "({ax}, {ay})");
            }
        }
    }
}

/// Deferred minors: a size whose art overflows is an error, never a panic.
#[test]
fn a_baker_refuses_an_overflowing_size() {
    let e = Baker::new(&CompositionSource::default(), (70_000, 70_000), 70_000, &back_tile());
    assert!(e.is_err());
}

/// Deferred minors: a theme pack linked into the content directory is found.
#[cfg(unix)]
#[test]
fn theme_back_tile_follows_links() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(root.join("olx-theme-dirt").canonicalize().unwrap(), tmp.path().join("dirt"))
        .unwrap();
    let tile = olr_edit::compose::theme_back_tile(tmp.path(), "olx-theme-dirt").unwrap();
    assert!(tile == olr_edit::compose::theme_back_tile(&root, "olx-theme-dirt").unwrap());
}

/// Gives image element `id` the mask `keep` (the original's size).
fn with_mask(src: &mut CompositionSource, id: &str, mask: &olr_edit::mask::Mask) {
    src.files.insert(olr_format::levels::mask_file(id), mask.encode());
    let e = src.composition.elements.iter_mut().find(|e| e.id == id).unwrap();
    if let ElementKind::Image { mask, .. } = &mut e.kind {
        *mask = true;
    }
}

/// Plan 14d: a masked pixel of the original is transparent before the
/// pipeline, so the bake leaves it uncovered.
#[test]
fn a_masked_image_bakes_without_its_erased_pixels() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "x", solid(3, 3, BLUE), (0, 0), (3, 3), ElementMaterial::Dirt);
    let mut mask = olr_edit::mask::Mask::full((3, 3));
    mask.keep[4] = 0;
    with_mask(&mut src, "x", &mask);
    let mut l = level(3, 3, 1);
    l.material = vec![Material::Background; 9];
    bake(&mut l, &src, &back_tile()).unwrap();
    let want: Vec<Material> =
        (0..9).map(|i| if i == 4 { Material::Background } else { Material::Dirt }).collect();
    assert_eq!(l.material, want);
}

/// Plan 14d R7: the grid cache keys an image's mask too.
#[test]
fn the_grid_cache_keys_masks() {
    let original = image_io::encode_rgba(&solid(4, 4, BLUE));
    let p = olr_edit::pixelate::Pixelation {
        pixel_size: 1,
        colours: None,
        dither: Dither::None,
        alpha_threshold: 128,
    };
    let mut cache = ImageCache::new();
    let mut a = olr_edit::mask::Mask::full((4, 4));
    let full = cache.grid("x.png", &original, (4, 4), &p, Some(&a)).unwrap();
    a.keep[0] = 0;
    let masked = cache.grid("x.png", &original, (4, 4), &p, Some(&a)).unwrap();
    assert_eq!(cache.processed(), 2);
    assert_eq!(full.get(0, 0).a, 255);
    assert_eq!(masked.get(0, 0).a, 0);
    cache.grid("x.png", &original, (4, 4), &p, Some(&a)).unwrap();
    assert_eq!(cache.processed(), 2, "the same mask is a hit");
}

#[test]
fn a_mask_of_another_size_is_an_error_naming_it() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "x", solid(4, 4, BLUE), (0, 0), (4, 4), ElementMaterial::Dirt);
    with_mask(&mut src, "x", &olr_edit::mask::Mask::full((3, 3)));
    let e = bake(&mut level(4, 4, 1), &src, &back_tile()).unwrap_err().to_string();
    assert!(e.contains("editor/masks/x.png"), "{e}");
}

#[test]
fn a_bad_original_is_an_error_naming_it() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "x", solid(1, 1, BLUE), (0, 0), (1, 1), ElementMaterial::Dirt);
    src.files.insert(olr_format::levels::image_file("x.png"), b"not an image".to_vec());
    let e = bake(&mut level(4, 3, 1), &src, &back_tile()).unwrap_err().to_string();
    assert!(e.contains("editor/images/x.png"), "{e}");
}

/// The spec's speed target (§3): a full bake of an OLX-sized level well
/// under a second in release, here with one full-level image at the
/// costliest settings (256 colours, diffusion). Run by hand:
/// `cargo nextest run -p olr-edit --release --run-ignored only bake_speed --no-capture`.
#[test]
#[ignore]
fn bake_speed() {
    let Some(root) = content() else { return };
    for id in ["olx-level-liero-factory", "olx-level-castlestrike"] {
        let original = olr_format::levels::read_level_files(&olr_vfs::DirFs::new(root.join(id))).unwrap();
        let mut src = compose_from(&original);
        let (w, h) = original.size;
        let mut photo = RgbaImage::new(640, 480);
        for y in 0..480 {
            for x in 0..640 {
                photo.set(
                    x,
                    y,
                    Rgba::rgb((x * 255 / 640) as u8, (y * 255 / 480) as u8, ((x ^ y) & 255) as u8),
                );
            }
        }
        with_image(&mut src, "photo", photo, (0, 0), (w, h), ElementMaterial::Dirt);
        if let ElementKind::Image { colours, dither, .. } = &mut src.composition.elements[1].kind {
            *colours = Some(256);
            *dither = Dither::Diffusion;
        }
        let mut level = original.clone();
        let start = std::time::Instant::now();
        bake(&mut level, &src, &back_tile()).unwrap();
        println!("{id}: bake of {w}x{h} at art scale {}: {:?}", original.art_scale, start.elapsed());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn region_bakes_equal_the_full_bake_and_keep_the_invariant(
        s in 1u32..=3, w in 1u32..=12, h in 1u32..=12,
        stack in prop::collection::vec((0u8..5, 0u8..3, -4i32..14, -4i32..14, 1u32..16, 1u32..16, any::<bool>()), 0..6),
        rect in (0u32..12, 0u32..12, 1u32..12, 1u32..12),
    ) {
        // Build images (kind 0), drawn layers (kind 1: a checkerboard layer) and a raster (kind 2: compose_from a random level).
        let src = random_stack(&stack, (w, h), s);
        let mut full = level(w, h, s);
        bake(&mut full, &src, &back_tile()).unwrap();
        prop_assert!(background_invariant(&full));
        let baker = Baker::new(&src, (w, h), s, &back_tile()).unwrap();
        let mut part = full.clone();
        let r = Rect { x: rect.0, y: rect.1, w: rect.2, h: rect.3 };
        // Scramble the region, then rebake it: it must come back as the full bake.
        scramble(&mut part, r);
        baker.bake_region(&mut part, r);
        prop_assert!(part.material == full.material && part.front == full.front && part.back == full.back);
    }
}

#[test]
fn stale_lists_the_files_a_hand_edit_changed() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("dirt");
    crate::common::copy_dir(&root.join("olx-level-dirt-level"), &dir);
    let tile = olr_edit::compose::theme_back_tile(&root, "olx-theme-dirt").unwrap();
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    let olr_edit::pack::Source::Level(level) = &mut pack.source else { panic!() };
    let src = compose_from(level);
    bake(level, &src, &tile).unwrap();
    pack.composition = Some(src);
    olr_edit::pack::save(&dir, &pack).unwrap();
    let pack = olr_edit::pack::load(&dir).unwrap();
    assert_eq!(olr_edit::compose::stale(&pack, &tile).unwrap(), Vec::<String>::new());
    let front = image_io::decode_rgba(&std::fs::read(dir.join("front.png")).unwrap()).unwrap();
    std::fs::write(dir.join("front.png"), image_io::encode_rgba(&solid(front.width(), front.height(), BLUE)))
        .unwrap();
    let stale = olr_edit::compose::stale(&olr_edit::pack::load(&dir).unwrap(), &tile).unwrap();
    assert!(stale.contains(&"front.png".to_string()), "{stale:?}");
    assert!(!stale.contains(&"material.png".to_string()), "{stale:?}");
}

#[test]
fn theme_back_tile_finds_the_theme_by_id() {
    let Some(root) = content() else { return };
    let tile = olr_edit::compose::theme_back_tile(&root, "olx-theme-dirt").unwrap();
    let theme =
        olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    assert!(tile == theme.back_tile);
    let e = olr_edit::compose::theme_back_tile(&root, "nope").unwrap_err().to_string();
    assert!(e.contains("nope"), "{e}");
}

/// `fixtures/edit/composed-level` (plan 14a Task 9) is baked: a change to
/// the pipeline or the bake that changes its bytes fails here.
#[test]
fn the_fixture_is_baked() {
    let Some(content) = content() else { return };
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level");
    let pack = olr_edit::pack::load(&fixture).unwrap();
    let tile = olr_edit::compose::theme_back_tile(&content, "olx-theme-dirt").unwrap();
    assert_eq!(olr_edit::compose::stale(&pack, &tile).unwrap(), Vec::<String>::new());
    // Plan 14d: the fixture holds a mask, a mixed layer and back art, so
    // the bake digest covers them.
    let elements = &pack.composition.as_ref().unwrap().composition.elements;
    assert!(elements.iter().any(|e| matches!(e.kind, ElementKind::Image { mask: true, .. })));
    assert!(
        elements
            .iter()
            .any(|e| matches!(e.kind, ElementKind::Drawn { mixed: true, back: true, size: Some(_), .. }))
    );
}

/// A hidden image is not processed, but its original still decodes: an
/// undecodable one is an error naming it.
#[test]
fn a_hidden_image_still_decodes() {
    const GRADIENT: &[u8] = include_bytes!("../../olr-format/tests/data/gradient.jpg");
    let mut src = CompositionSource::default();
    with_image(&mut src, "x", solid(1, 1, BLUE), (0, 0), (1, 1), ElementMaterial::Dirt);
    src.composition.elements[0].visible = false;
    bake(&mut level(4, 3, 1), &src, &back_tile()).unwrap();
    src.files.insert(image_file("x.png"), GRADIENT[..GRADIENT.len() / 2].to_vec());
    let e = bake(&mut level(4, 3, 1), &src, &back_tile()).unwrap_err().to_string();
    assert!(e.contains("editor/images/x.png") && e.contains("cut short"), "{e}");
}

/// A 16 × 12 composition at art scale 2: a drawn layer, then an image.
fn image_and_layer() -> CompositionSource {
    let mut src = CompositionSource::default();
    with_drawn(&mut src, "ground", &solid(32, 24, BROWN), ElementMaterial::Dirt);
    let mut img = solid(6, 4, BLUE);
    img.set(1, 1, BROWN);
    with_image(&mut src, "photo", img, (2, 3), (5, 4), ElementMaterial::Rock);
    src
}

fn set_image(src: &mut CompositionSource, mut f: impl FnMut(&mut (i32, i32), &mut u32)) {
    for e in &mut src.composition.elements {
        if let ElementKind::Image { at, pixel_size, .. } = &mut e.kind {
            f(at, pixel_size);
        }
    }
}

#[test]
fn a_move_never_reprocesses() {
    let mut cache = ImageCache::new();
    let mut src = image_and_layer();
    let new = |src: &CompositionSource, cache: &mut ImageCache| {
        Baker::new_cached(src, (16, 12), 2, &back_tile(), cache).unwrap();
    };
    new(&src, &mut cache);
    assert_eq!(cache.processed(), 1);
    set_image(&mut src, |at, _| *at = (7, -1));
    new(&src, &mut cache);
    assert_eq!(cache.processed(), 1, "a move");
    src.composition.elements.swap(0, 1);
    new(&src, &mut cache);
    assert_eq!(cache.processed(), 1, "a reorder");
    set_image(&mut src, |_, ps| *ps = 2);
    new(&src, &mut cache);
    assert_eq!(cache.processed(), 2, "a new pixel size");
    set_image(&mut src, |_, ps| *ps = 1);
    new(&src, &mut cache);
    assert_eq!(cache.processed(), 2, "the cached grid");
}

#[test]
fn the_cache_keeps_four_grids_per_source() {
    assert_eq!(GRIDS_PER_SOURCE, 4);
    let mut cache = ImageCache::new();
    let mut src = image_and_layer();
    for ps in [1, 2, 3, 4, 5, 1] {
        set_image(&mut src, |_, p| *p = ps);
        Baker::new_cached(&src, (16, 12), 2, &back_tile(), &mut cache).unwrap();
    }
    assert_eq!(cache.processed(), 6);
}

/// Each drawn and raster element of `fixtures/edit/composed-level`,
/// decoded and encoded again, gives back its files byte for byte.
#[test]
fn encode_decoded_round_trips() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level");
    let pack = olr_edit::pack::load(&fixture).unwrap();
    let olr_edit::pack::Source::Level(level) = &pack.source else { panic!("a level") };
    let src = pack.composition.as_ref().unwrap();
    let mut cache = ImageCache::new();
    let mut encoded = 0;
    for e in &src.composition.elements {
        let d = decode_element(src, e, level.size, level.art_scale, &mut cache).unwrap();
        let files = encode_decoded(e, &d, level.size);
        if matches!(e.kind, ElementKind::Image { .. }) {
            // An image's original is kept; only its mask is encoded.
            assert!(files.iter().all(|(p, _)| p.starts_with("editor/masks/")), "{files:?}");
        }
        for (path, bytes) in files {
            assert!(src.files[&path] == bytes, "{path}");
            encoded += 1;
        }
    }
    assert_eq!(encoded, 9, "the raster's three files, two drawn layers' four, two masks");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn bake_elements_equals_the_baker(
        s in 1u32..=3, w in 1u32..=12, h in 1u32..=12,
        stack in prop::collection::vec((0u8..5, 0u8..3, -4i32..14, -4i32..14, 1u32..16, 1u32..16, any::<bool>()), 0..6),
    ) {
        let src = random_stack(&stack, (w, h), s);
        let mut full = level(w, h, s);
        bake(&mut full, &src, &back_tile()).unwrap();
        let mut cache = ImageCache::new();
        let decoded: Vec<Decoded> = src
            .composition
            .elements
            .iter()
            .map(|e| decode_element(&src, e, (w, h), s, &mut cache).unwrap())
            .collect();
        let items: Vec<_> = src.composition.elements.iter().zip(&decoded).collect();
        let mut part = level(w, h, s);
        bake_elements(&mut part, &items, &back_tile(), Rect { x: 0, y: 0, w, h });
        prop_assert!(part.material == full.material && part.front == full.front && part.back == full.back);
    }
}

/// Batch A review: the cache keeps originals within `ORIGINAL_PIXELS`,
/// the least recently used dropped.
#[test]
fn the_cache_bounds_its_originals() {
    let mut cache = ImageCache::new();
    let png = image_io::encode_rgba(&RgbaImage::new(2048, 2048));
    let n = olr_edit::compose::ORIGINAL_PIXELS / (2048 * 2048) + 3;
    for i in 0..n {
        cache.original(&format!("{i}.png"), &png).unwrap();
        assert!(cache.original_pixels() <= olr_edit::compose::ORIGINAL_PIXELS);
    }
}

/// Batch A review: a source whose bytes change under the same name is
/// decoded again, never served from the cache.
#[test]
fn the_cache_notices_new_bytes_under_an_old_name() {
    let mut cache = ImageCache::new();
    let a = image_io::encode_rgba(&solid(2, 2, BLUE));
    let b = image_io::encode_rgba(&solid(3, 1, BROWN));
    assert_eq!(cache.original("a.png", &a).unwrap().width(), 2);
    assert_eq!(cache.original("a.png", &b).unwrap().width(), 3);
    let p = olr_edit::pixelate::Pixelation {
        pixel_size: 1,
        colours: None,
        dither: Dither::None,
        alpha_threshold: 128,
    };
    assert_eq!(cache.grid("a.png", &a, (2, 2), &p, None).unwrap().get(0, 0), BLUE);
    assert_eq!(cache.grid("a.png", &b, (2, 2), &p, None).unwrap().get(0, 0), BROWN);
}

/// Batch A review: data of the wrong size is refused or encodes to
/// nothing, never a panic.
#[test]
fn mismatched_decoded_data_never_panics() {
    use olr_edit::compose::resize_decoded;
    use olr_edit::level::Paint;
    let e = element("r", ElementKind::Raster);
    let d = Decoded::Raster {
        material: vec![Material::Dirt; 3],
        front: solid(2, 2, BLUE),
        back: solid(2, 2, BLUE),
    };
    assert!(encode_decoded(&e, &d, (5, 5)).is_empty());
    let (mut e2, mut d2) = (e.clone(), d.clone());
    assert!(
        resize_decoded(&mut e2, &mut d2, (2, 2), 1, (4, 4), (0, 0), Material::Rock, &Paint::Colour(BLUE))
            .is_err()
    );
    assert_eq!(d2, d);
    let drawn = element("d", ElementKind::drawn(ElementMaterial::Dirt));
    let mut art = Decoded::Drawn { art: solid(3, 3, BLUE), cells: None, back: None };
    let mut e3 = drawn.clone();
    assert!(
        resize_decoded(&mut e3, &mut art, (2, 2), 1, (4, 4), (0, 0), Material::Rock, &Paint::Colour(BLUE))
            .is_err()
    );
}

/// The cache keeps its grids within `GRID_PIXELS` altogether, the least
/// recently used dropped (besides `GRIDS_PER_SOURCE` per source).
#[test]
fn the_cache_bounds_its_grids() {
    let mut cache = ImageCache::new();
    let png = image_io::encode_rgba(&solid(2, 2, BLUE));
    let p = olr_edit::pixelate::Pixelation {
        pixel_size: 1,
        colours: None,
        dither: Dither::None,
        alpha_threshold: 128,
    };
    let n = olr_edit::compose::GRID_PIXELS / (2048 * 2048) + 3;
    for i in 0..n {
        cache.grid(&format!("{i}.png"), &png, (2048, 2048), &p, None).unwrap();
        assert!(cache.grid_pixels() <= olr_edit::compose::GRID_PIXELS);
    }
    // The newest is kept.
    let before = cache.processed();
    cache.grid(&format!("{}.png", n - 1), &png, (2048, 2048), &p, None).unwrap();
    assert_eq!(cache.processed(), before);
}
