//! The bake (plan 14a Task 5): a composition baked into the level's
//! material, front and back, whole or a region at a time.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::{Baker, Rect, bake, compose_from};
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
        },
    ));
}

/// Adds a drawn layer `id` holding `art` (art resolution) on top.
fn with_drawn(src: &mut CompositionSource, id: &str, art: &RgbaImage, m: ElementMaterial) {
    src.files.insert(layer_file(id), image_io::encode_rgba(art));
    src.composition.elements.push(element(id, ElementKind::Drawn { material: m }));
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
/// of a checkerboard, 2 the raster base of a random-ish level.
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

#[test]
fn elements_outside_the_level_are_clipped() {
    let mut src = CompositionSource::default();
    with_image(&mut src, "far", solid(3, 3, BLUE), (-100, -100), (3, 3), ElementMaterial::Rock);
    with_image(&mut src, "big", solid(9, 9, BROWN), (-2, -2), (4096, 4096), ElementMaterial::Dirt);
    with_image(&mut src, "edge", solid(3, 3, BLUE), (3, 2), (3, 3), ElementMaterial::Rock);
    let mut l = level(4, 3, 2);
    bake(&mut l, &src, &back_tile()).unwrap();
    assert_eq!(l.material[(2 * 4 + 3) as usize], Material::Rock, "the edge's one inside pixel");
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
/// under a second in release. Run by hand:
/// `cargo nextest run -p olr-edit --release --run-ignored only bake_speed`.
#[test]
#[ignore]
fn bake_speed() {
    let Some(root) = content() else { return };
    let original =
        olr_format::levels::read_level_files(&olr_vfs::DirFs::new(root.join("olx-level-liero-factory")))
            .unwrap();
    let mut src = compose_from(&original);
    let (w, h) = original.size;
    with_image(&mut src, "photo", solid(640, 480, BLUE), (0, 0), (w, h), ElementMaterial::Dirt);
    if let ElementKind::Image { colours, dither, .. } = &mut src.composition.elements[1].kind {
        *colours = Some(16);
        *dither = Dither::Diffusion;
    }
    let mut level = original.clone();
    let start = std::time::Instant::now();
    bake(&mut level, &src, &back_tile()).unwrap();
    println!("bake of {w}x{h} at art scale {}: {:?}", original.art_scale, start.elapsed());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn region_bakes_equal_the_full_bake_and_keep_the_invariant(
        s in 1u32..=3, w in 1u32..=12, h in 1u32..=12,
        stack in prop::collection::vec((0u8..3, 0u8..3, -4i32..14, -4i32..14, 1u32..16, 1u32..16, any::<bool>()), 0..6),
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
}
