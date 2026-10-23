//! Issue #37: surface materials in the bake. Each element kind's
//! `surface` lands where its pixels land (rock and dirt in the front map,
//! background in the back map, a mixed layer per pixel), `back_surface`
//! with back art, painted maps override the defaults, and a plain level's
//! maps survive being opened as elements.

use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{DistantLayer, Material};
use olr_edit::compose::{
    Decoded, GlowMasks, PaintedSurfaces, Rect, bake, bake_elements_with, compose_from, split_from,
};
use olr_format::levels::{
    Element, ElementKind, ElementMaterial, LevelSource, SurfaceMaps, surface_back_file, surface_file,
};
use olr_format::look::LookSource;
use olr_format::materials::MaterialSource;

const GREY: Rgba = Rgba::rgb(120, 120, 120);

fn material(id: &str) -> MaterialSource {
    MaterialSource { id: id.into(), name: id.into(), notes: String::new(), heat: None, light_soak: None }
}

/// A `w` × `h` level at art scale `s` whose look has stone (1), wood (2),
/// metal (3) and glass (4).
fn level(w: u32, h: u32, s: u32) -> LevelSource {
    LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Background; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
        look: Some(LookSource {
            materials: ["stone", "wood", "metal", "glass"].map(material).into(),
            ..LookSource::default()
        }),
        glow: None,
        clear: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    }
}

fn solid(w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, GREY);
        }
    }
    img
}

fn element(id: &str, kind: ElementKind, surface: Option<&str>, back_surface: Option<&str>) -> Element {
    Element {
        id: id.into(),
        name: String::new(),
        visible: true,
        locked: false,
        kind,
        glow: None,
        clear: false,
        distant: None,
        surface: surface.map(str::to_string),
        back_surface: back_surface.map(str::to_string),
        surface_map: false,
        back_surface_map: false,
    }
}

fn boxed(m: ElementMaterial, at: (i32, i32), size: (u32, u32), mixed: bool, back: bool) -> ElementKind {
    ElementKind::Drawn { material: m, at, size: Some(size), mixed, back }
}

fn bake_all(l: &mut LevelSource, items: &[(Element, Decoded)], painted: &PaintedSurfaces) {
    let items: Vec<(&Element, &Decoded)> = items.iter().map(|(e, d)| (e, d)).collect();
    let all = Rect { x: 0, y: 0, w: l.size.0, h: l.size.1 };
    bake_elements_with(l, &items, &GlowMasks::new(), painted, &RgbaImage::new(0, 0), all);
}

/// A row of six world pixels: a background layer of wood under a rock
/// picture of stone (x 0), a rock layer of metal with back art of glass
/// (x 2-3) and a mixed layer of stone, dirt then background (x 4-5).
fn row() -> Vec<(Element, Decoded)> {
    let image = ElementKind::Image {
        material: ElementMaterial::Rock,
        source: "a.png".into(),
        at: (0, 0),
        size: (1, 1),
        pixel_size: 1,
        colours: None,
        dither: olr_format::levels::Dither::None,
        alpha_threshold: 128,
        mask: false,
    };
    vec![
        (
            element("sky", ElementKind::drawn(ElementMaterial::Background), Some("wood"), None),
            Decoded::Drawn { art: solid(6, 1), cells: None, back: None },
        ),
        (
            element("pic", image, Some("stone"), None),
            Decoded::Image { grid: Some(Arc::new(solid(1, 1))), mask: None },
        ),
        (
            element(
                "wall",
                boxed(ElementMaterial::Rock, (2, 0), (2, 1), false, true),
                Some("metal"),
                Some("glass"),
            ),
            Decoded::Drawn { art: solid(2, 1), cells: None, back: Some(solid(2, 1)) },
        ),
        (
            element("mix", boxed(ElementMaterial::Dirt, (4, 0), (2, 1), true, false), Some("stone"), None),
            Decoded::Drawn {
                art: solid(2, 1),
                cells: Some(vec![Material::Dirt, Material::Background]),
                back: None,
            },
        ),
    ]
}

#[test]
fn each_kind_of_element_lands_front_or_back() {
    let mut l = level(6, 1, 1);
    bake_all(&mut l, &row(), &PaintedSurfaces::new());
    let maps = l.surface.clone().expect("maps baked");
    assert_eq!(maps.world, (6, 1));
    // Rock and dirt in front; a background pixel has no front material.
    assert_eq!(maps.front, [1, 0, 3, 3, 1, 0]);
    // The background layer's wood behind, the wall's glass back art, and
    // the mixed layer's background pixel its own stone.
    assert_eq!(maps.back, [2, 2, 4, 4, 2, 1]);
    // Written and read back, the level checks.
    let mut fs = olr_vfs::MemFs::new();
    olr_format::levels::write_level_files(&mut fs, &l).unwrap();
    assert_eq!(olr_format::levels::read_level_files(&fs).unwrap().surface, Some(maps));
}

#[test]
fn a_painted_map_overrides_the_default_per_pixel() {
    let mut items = row();
    items[0].0.surface_map = true;
    items[2].0.back_surface_map = true;
    let mut painted = PaintedSurfaces::new();
    // 0 keeps the default; a value past the materials is the default too.
    painted.insert(surface_file("sky"), Arc::new(vec![0, 3, 0, 0, 0, 9]));
    painted.insert(surface_back_file("wall"), Arc::new(vec![1, 0]));
    let mut l = level(6, 1, 1);
    bake_all(&mut l, &items, &painted);
    let maps = l.surface.unwrap();
    assert_eq!(maps.front, [1, 0, 3, 3, 1, 0]);
    assert_eq!(maps.back, [2, 3, 1, 4, 2, 1]);
    // A map of the wrong size is left out, never indexed.
    painted.insert(surface_file("sky"), Arc::new(vec![3; 2]));
    let mut l = level(6, 1, 1);
    bake_all(&mut l, &items, &painted);
    assert_eq!(l.surface.unwrap().back, [2, 2, 1, 4, 2, 1]);
}

#[test]
fn a_raster_lands_by_its_cells_and_its_back_art() {
    let mut l = level(3, 1, 2);
    let raster = (
        element("base", ElementKind::Raster, Some("metal"), Some("wood")),
        Decoded::Raster {
            material: vec![Material::Rock, Material::Background, Material::Dirt],
            front: solid(6, 2),
            back: solid(6, 2),
        },
    );
    bake_all(&mut l, &[raster], &PaintedSurfaces::new());
    let maps = l.surface.unwrap();
    assert_eq!((maps.front, maps.back), (vec![3, 0, 3], vec![2, 2, 2]));
}

#[test]
fn no_material_named_bakes_no_maps() {
    let strip = |items: Vec<(Element, Decoded)>| {
        items.into_iter().map(|(e, d)| (e.without_surfaces(), d)).collect::<Vec<_>>()
    };
    let mut l = level(6, 1, 1);
    bake_all(&mut l, &strip(row()), &PaintedSurfaces::new());
    assert_eq!(l.surface, None);
    // Ids the look lacks name nothing; nor does a level without a look.
    let mut l = level(6, 1, 1);
    l.look.as_mut().unwrap().materials.clear();
    l.surface = Some(SurfaceMaps::empty((6, 1)));
    bake_all(&mut l, &row(), &PaintedSurfaces::new());
    assert_eq!(l.surface, None);
    let mut l = level(6, 1, 1);
    l.look = None;
    bake_all(&mut l, &row(), &PaintedSurfaces::new());
    assert_eq!(l.surface, None);
}

#[test]
fn a_distant_layer_takes_one_surface() {
    let mut l = level(4, 2, 1);
    let mut far =
        element("far", boxed(ElementMaterial::Background, (0, 0), (2, 2), false, false), None, None);
    far.distant = Some(60);
    let mut near = element(
        "near",
        boxed(ElementMaterial::Background, (1, 0), (2, 2), false, false),
        Some("glass"),
        None,
    );
    near.distant = Some(20);
    let d = || Decoded::Drawn { art: solid(2, 2), cells: None, back: None };
    bake_all(&mut l, &[(far, d()), (near, d())], &PaintedSurfaces::new());
    assert_eq!(l.distant.len(), 2);
    assert_eq!(l.distant_surfaces, [None, Some("glass".to_string())]);
    assert_eq!(l.surface, None, "a distant layer is in neither map");
    let mut fs = olr_vfs::MemFs::new();
    olr_format::levels::write_level_files(&mut fs, &l).unwrap();
    let text = String::from_utf8(
        olr_vfs::ContentSource::read(&fs, &olr_vfs::VPath::new("level.ron").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(text.contains("surface: \"glass\""), "{text}");
    assert_eq!(olr_format::levels::read_level_files(&fs).unwrap().distant_surfaces, l.distant_surfaces);
}

/// A plain level's maps (no composition) survive being opened as one
/// raster or as three layers: both bake back to the same maps.
#[test]
fn a_plain_levels_maps_bake_back() {
    let mut l = level(6, 1, 1);
    bake_all(&mut l, &row(), &PaintedSurfaces::new());
    l.distant = vec![DistantLayer { image: solid(2, 1), at: (0, 0), depth: 50 }];
    l.distant_surfaces = vec![Some("stone".into())];
    let want = (l.surface.clone(), l.distant_surfaces.clone());
    assert!(want.0.is_some());
    for src in [compose_from(&l), split_from(&l)] {
        let mut again = l.clone();
        again.surface = None;
        again.distant_surfaces.clear();
        bake(&mut again, &src, &RgbaImage::new(1, 1)).unwrap();
        assert_eq!((again.surface, again.distant_surfaces), want);
    }
}

/// A plain level's maps follow `level::resize` (the new area none), as
/// its glow and clear flags do.
#[test]
fn a_resize_re_lays_a_plain_levels_maps() {
    let mut l = level(3, 1, 1);
    l.surface = Some(SurfaceMaps { world: (3, 1), front: vec![1, 2, 3], back: vec![4, 0, 4] });
    let paint = olr_edit::level::Paint::Colour(GREY);
    olr_edit::level::resize(&mut l, (4, 1), (1, 0), Material::Rock, &paint).unwrap();
    let maps = l.surface.clone().unwrap();
    assert_eq!((maps.world, maps.front, maps.back), ((4, 1), vec![0, 1, 2, 3], vec![0, 4, 0, 4]));
    olr_format::levels::write_level_files(&mut olr_vfs::MemFs::new(), &l).unwrap();
    // Cut down to nothing that had a material: no maps.
    olr_edit::level::resize(&mut l, (1, 1), (0, 0), Material::Rock, &paint).unwrap();
    assert_eq!(l.surface, None);
}

/// A size past the level's bounds is refused before any layer or map is
/// laid out at it (no allocation of its size).
#[test]
fn a_huge_size_is_refused_before_anything_is_laid_out() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("huge");
    olr_edit::new::new_pack(
        &dir,
        olr_data::manifest::PackKind::Level,
        "huge",
        "Huge",
        olr_edit::new::Template::Level { size: (8, 4), theme: "olx-theme-dirt" },
    )
    .unwrap();
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    olr_edit::element::ensure_composed(&mut pack).unwrap();
    let id = olr_edit::element::add_layer(&mut pack, ElementMaterial::Rock, "Wall").unwrap();
    let huge = (4_000_000_000, 4_000_000_000);
    let paint = olr_edit::level::Paint::Colour(GREY);
    assert!(olr_edit::element::resize(&mut pack, huge, (0, 0), Material::Rock, &paint).is_err());
    let size = olr_edit::element::Property::Size(huge);
    assert!(olr_edit::element::set_all(&mut pack, &id, &[size]).is_err());
}
