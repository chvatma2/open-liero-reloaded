//! Assets (issue #9): saved and placed pieces round-trip their pixels,
//! materials, glow masks and groups, at one art scale and across two; a
//! rectangle's cut follows its rules; placing joins or adds glow groups; a
//! bad folder is refused; the user's library.

use std::collections::BTreeMap;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::*;
use olr_edit::compose::{Decoded, Rect, bake};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{ElementGlow, ElementKind, ElementMaterial, LevelSource};
use olr_format::look::{GlowGroupSource, LookSource, ReachSource};

const RED: Rgba = Rgba::rgb(200, 30, 30);
const GREEN: Rgba = Rgba::rgb(30, 200, 30);
const BLUE: Rgba = Rgba::rgb(30, 30, 200);

fn group(id: &str, intensity: u16) -> GlowGroupSource {
    GlowGroupSource {
        id: id.into(),
        colour: Default::default(),
        intensity,
        reach: ReachSource::Pixels(40),
        animation: Default::default(),
        phase_ms: None,
    }
}

/// An empty level pack (background everywhere, no composition).
fn pack(w: u32, h: u32, s: u32, look: Option<LookSource>) -> Pack {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: "t".into(),
        name: "T".into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    };
    let level = LevelSource {
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
        look,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    };
    Pack { manifest, source: Source::Level(level), composition: None, other: BTreeMap::new() }
}

fn level(p: &Pack) -> &LevelSource {
    let Source::Level(l) = &p.source else { panic!() };
    l
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Shape {
    Shape::Rect { x, y, w, h }
}

/// 12 × 8 at art scale `s`, its look's `lamps` group at intensity 777: a
/// blue `wall` (background) everywhere glowing in `lamps` through a mask
/// painted at (2, 2, 2, 2), a red `ground` (dirt) on rows 6-7, a green
/// `pillar` (rock) at x 8-9, rows 2-7.
fn scene(s: u32) -> Pack {
    let look = LookSource { glow_groups: vec![group("sky", 100), group("lamps", 777)], ..Default::default() };
    let mut p = pack(12, 8, s, Some(look));
    add_layer(&mut p, ElementMaterial::Background, "Wall").unwrap();
    // The first layer composed the plain level as its raster base: gone.
    remove(&mut p, "base").unwrap();
    draw(&mut p, "wall", &rect(0, 0, 12, 8), &Paint::Colour(BLUE), None).unwrap();
    set(
        &mut p,
        "wall",
        Property::Glow(Some(ElementGlow { strength: 80, group: "lamps".into(), mask: false, beam: None })),
    )
    .unwrap();
    paint_glow(&mut p, "wall", &rect(2, 2, 2, 2), 100).unwrap();
    // Painting a first mask starts it whole: dark it but for the square.
    paint_glow(&mut p, "wall", &rect(0, 0, 12, 2), 0).unwrap();
    paint_glow(&mut p, "wall", &rect(0, 4, 12, 4), 0).unwrap();
    paint_glow(&mut p, "wall", &rect(0, 2, 2, 2), 0).unwrap();
    paint_glow(&mut p, "wall", &rect(4, 2, 8, 2), 0).unwrap();
    add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap();
    draw(&mut p, "ground", &rect(0, 6, 12, 2), &Paint::Colour(RED), None).unwrap();
    add_layer(&mut p, ElementMaterial::Rock, "Pillar").unwrap();
    draw(&mut p, "pillar", &rect(8, 2, 2, 6), &Paint::Colour(GREEN), None).unwrap();
    p
}

fn baked(p: &Pack) -> LevelSource {
    let mut l = level(p).clone();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    l
}

/// The asset written to a folder and read back.
fn through_folder(asset: &Asset) -> Asset {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("a");
    save_dir(&dir, &encode(asset).unwrap()).unwrap();
    assert!(dir.join("thumb.png").is_file());
    load_dir(&dir).unwrap()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// World pixel (x, y)'s material and its art block's colours (front, back).
fn cell(l: &LevelSource, x: u32, y: u32) -> (Material, Vec<Rgba>, Vec<Rgba>) {
    let s = l.art_scale;
    let block = |i: &RgbaImage| (0..s * s).map(|k| i.get(x * s + k % s, y * s + k / s)).collect::<Vec<_>>();
    (l.material[(y * l.size.0 + x) as usize], block(&l.front), block(&l.back))
}

#[test]
fn elements_round_trip_through_a_folder_and_land_where_placed() {
    for s in [1, 2] {
        let p = scene(s);
        let source = baked(&p);
        let decoded = DecodedLevel::of_pack(&p).unwrap();
        let asset = from_elements(&decoded.level(), &ids(&["ground", "pillar"]), "Bits").unwrap();
        // The covered box: the ground's rows and the pillar's top.
        assert_eq!((asset.size, asset.art_scale, asset.pieces.len()), ((12, 6), s, 2), "scale {s}");
        assert!(asset.groups.is_empty(), "neither glows");
        let back = through_folder(&asset);
        assert_eq!(back, asset, "scale {s}");
        let mut target = pack(20, 12, s, None);
        let placed = place_in_pack(&mut target, &back, (3, 4)).unwrap();
        assert_eq!(placed.ids, ids(&["bits-ground", "bits-pillar"]));
        assert!(!placed.groups.changes(), "no glow, no look");
        assert_eq!(level(&target).look, None);
        let landed = baked(&target);
        for y in 0..6 {
            for x in 0..12 {
                let (from, to) = (cell(&source, x, y + 2), cell(&landed, x + 3, y + 4));
                if from.0 != Material::Background {
                    assert_eq!(from.0, to.0, "material at {x},{y}, scale {s}");
                    assert_eq!(from.1, to.1, "front at {x},{y}, scale {s}");
                } else {
                    assert_eq!(to.0, Material::Background, "{x},{y}");
                }
            }
        }
    }
}

#[test]
fn glow_masks_and_groups_travel_with_an_asset() {
    let p = scene(2);
    let decoded = DecodedLevel::of_pack(&p).unwrap();
    let asset = through_folder(&from_elements(&decoded.level(), &ids(&["wall"]), "Wall").unwrap());
    // Only the group it names, with the level's settings.
    assert_eq!(asset.groups, vec![group("lamps", 777)]);
    let mask = asset.pieces[0].glow_mask.clone().expect("its mask");
    assert_eq!(mask.len(), 24 * 16);
    assert_eq!((mask[(4 * 24 + 4) as usize], mask[0]), (255, 0), "the square glows, the rest not");

    // No look: the default one and the asset's group, said.
    let mut bare = pack(12, 8, 2, None);
    let placed = place_in_pack(&mut bare, &asset, (0, 0)).unwrap();
    assert!(placed.groups.added_look);
    assert_eq!(placed.groups.added, ids(&["lamps"]));
    assert_eq!(
        level(&bare).look,
        Some(LookSource { glow_groups: vec![group("lamps", 777)], ..LookSource::default() })
    );
    let src = bare.composition.as_ref().unwrap();
    assert_eq!(
        src.files[&olr_format::levels::glow_file("wall")],
        olr_format::image_io::encode_grey(24, 16, &mask),
        "the mask is kept"
    );
    let glow = baked(&bare).glow.expect("it glows");
    assert_eq!(glow.groups[(2 * 12 + 2) as usize], 1);

    // A level with the group: joined, its own settings kept.
    let mut has =
        pack(12, 8, 2, Some(LookSource { glow_groups: vec![group("lamps", 5)], ..Default::default() }));
    let placed = place_in_pack(&mut has, &asset, (0, 0)).unwrap();
    assert_eq!((placed.groups.joined.clone(), placed.groups.added_look), (ids(&["lamps"]), false));
    assert_eq!(level(&has).look.as_ref().unwrap().glow_groups, vec![group("lamps", 5)]);

    // A level with another group: the asset's added after it, with the
    // asset's settings (not a new group's defaults).
    let mut other =
        pack(12, 8, 2, Some(LookSource { glow_groups: vec![group("sky", 300)], ..Default::default() }));
    place_in_pack(&mut other, &asset, (0, 0)).unwrap();
    assert_eq!(
        level(&other).look.as_ref().unwrap().glow_groups,
        vec![group("sky", 300), group("lamps", 777)]
    );
}

#[test]
fn an_asset_is_resampled_to_the_level_art_scale() {
    let p = scene(1);
    let decoded = DecodedLevel::of_pack(&p).unwrap();
    let asset = from_elements(&decoded.level(), &ids(&["wall", "ground"]), "Two").unwrap();
    let mut target = pack(12, 8, 3, None);
    let placed = place_in_pack(&mut target, &asset, (0, 0)).unwrap();
    assert_eq!(placed.ids, ids(&["two-wall", "two-ground"]));
    let l = baked(&target);
    assert_eq!(l.art_scale, 3);
    // Each world pixel's art block is the one colour it had.
    assert_eq!(cell(&l, 5, 7), (Material::Dirt, vec![RED; 9], cell(&l, 5, 7).2));
    assert_eq!(cell(&l, 5, 1).1, vec![BLUE; 9]);
    // The mask, resampled: three times as fine, glowing where it did.
    let src = target.composition.as_ref().unwrap();
    let mask = olr_format::image_io::decode_grey_within(
        &src.files[&olr_format::levels::glow_file("two-wall")],
        olr_format::image_io::IMAGES,
    )
    .unwrap();
    assert_eq!((mask.0, mask.1), (36, 24));
    assert_eq!(mask.2[(7 * 36 + 7) as usize], 255);
    assert_eq!(mask.2[(7 * 36 + 13) as usize], 0);
}

#[test]
fn a_rectangle_cuts_every_visible_layer_by_the_rules() {
    let mut p = scene(1);
    set(&mut p, "pillar", Property::Visible(false)).unwrap();
    let cut = |p: &Pack, r: Rect, layers: Option<&[String]>| {
        let d = DecodedLevel::of_pack(p).unwrap();
        cut_rect(&d.level(), r, layers, "Cut")
    };
    // The pillar is hidden; the wall's mask is dark there: no glow, no
    // group.
    let a = cut(&p, Rect { x: 6, y: 5, w: 5, h: 3 }, None).unwrap();
    assert_eq!(a.size, (5, 3));
    let names: Vec<&str> = a.pieces.iter().map(|x| x.element.id.as_str()).collect();
    assert_eq!(names, ["wall", "ground"]);
    assert_eq!(a.pieces[0].element.glow, None);
    assert!(a.groups.is_empty());
    // Only the layers asked for.
    let a = cut(&p, Rect { x: 6, y: 5, w: 5, h: 3 }, Some(&ids(&["ground", "pillar"]))).unwrap();
    assert_eq!(a.pieces.len(), 1);
    assert_eq!(a.pieces[0].element.id, "ground");
    // Cropped to what it covers, relative to the rectangle.
    assert_eq!(olr_format::levels::element_box(&a.pieces[0].element.kind, a.size), Some(((0, 1), (5, 2))));
    // A layer with nothing there is left out; the glowing square keeps
    // its glow and group.
    let a = cut(&p, Rect { x: 1, y: 1, w: 4, h: 3 }, None).unwrap();
    assert_eq!(a.pieces.len(), 1);
    assert_eq!(a.pieces[0].element.glow.as_ref().map(|g| g.group.as_str()), Some("lamps"));
    assert_eq!(a.groups, vec![group("lamps", 777)]);
    // A layer identical to one below it there is left out.
    let copy = duplicate(&mut p, "ground").unwrap();
    let a = cut(&p, Rect { x: 0, y: 6, w: 4, h: 2 }, None).unwrap();
    let names: Vec<&str> = a.pieces.iter().map(|x| x.element.id.as_str()).collect();
    assert_eq!(names, ["wall", "ground"], "{copy} dropped");
    // Nothing there at all.
    assert!(cut(&p, Rect { x: 0, y: 0, w: 4, h: 4 }, Some(&ids(&["ground"]))).is_err());
}

#[test]
fn a_picture_wholly_inside_is_kept_whole() {
    let mut p = scene(1);
    let mut img = RgbaImage::new(4, 4);
    for y in 0..4 {
        for x in 0..4 {
            img.set(x, y, if (x + y) % 2 == 0 { RED } else { GREEN });
        }
    }
    let png = olr_format::image_io::encode_rgba(&img);
    add_image(&mut p, "crate.png", png.clone(), (3, 1), (2, 2), ElementMaterial::Rock, "Crate").unwrap();
    let d = DecodedLevel::of_pack(&p).unwrap();
    let a = cut_rect(&d.level(), Rect { x: 2, y: 0, w: 4, h: 4 }, Some(&ids(&["crate"])), "Box").unwrap();
    let piece = &a.pieces[0];
    let ElementKind::Image { at, size, source, .. } = &piece.element.kind else { panic!("{piece:?}") };
    assert_eq!((*at, *size, source.as_str()), ((1, 1), (2, 2), "crate.png"));
    assert_eq!(piece.original.as_ref().map(|o| o.1.as_slice()), Some(png.as_slice()));
    let back = through_folder(&a);
    assert!(matches!(back.pieces[0].element.kind, ElementKind::Image { .. }));
    // Partly inside: its pixels, cropped.
    let a = cut_rect(&d.level(), Rect { x: 4, y: 0, w: 4, h: 4 }, Some(&ids(&["crate"])), "Half").unwrap();
    let piece = &a.pieces[0];
    assert!(matches!(piece.element.kind, ElementKind::Drawn { material: ElementMaterial::Rock, .. }));
    assert_eq!(olr_format::levels::element_box(&piece.element.kind, a.size), Some(((0, 1), (1, 2))));
}

#[test]
fn a_cut_raster_keeps_its_cells_and_back_art() {
    // A plain level kept as its raster base: dirt and rock over back art.
    let mut p = pack(6, 4, 2, None);
    let Source::Level(l) = &mut p.source else { panic!() };
    for (i, m) in l.material.iter_mut().enumerate() {
        *m = [Material::Dirt, Material::Rock, Material::Background][i % 3];
    }
    for y in 0..8 {
        for x in 0..12 {
            l.back.set(x, y, Rgba::rgb(x as u8 * 10, 0, 90));
            let m = l.material[(y / 2 * 6 + x / 2) as usize];
            l.front.set(
                x,
                y,
                if m == Material::Background { l.back.get(x, y) } else { Rgba::rgb(9, y as u8, 9) },
            );
        }
    }
    ensure_composed(&mut p).unwrap();
    let source = baked(&p);
    let d = DecodedLevel::of_pack(&p).unwrap();
    let a = cut_rect(&d.level(), Rect { x: 1, y: 1, w: 4, h: 2 }, None, "Bit").unwrap();
    let piece = &a.pieces[0];
    assert!(matches!(piece.element.kind, ElementKind::Drawn { mixed: true, back: true, .. }), "{piece:?}");
    assert!(matches!(&piece.decoded, Decoded::Drawn { cells: Some(_), back: Some(_), .. }));
    let mut target = pack(6, 4, 2, None);
    place_in_pack(&mut target, &through_folder(&a), (1, 1)).unwrap();
    let landed = baked(&target);
    for y in 1..3 {
        for x in 1..5 {
            assert_eq!(cell(&landed, x, y), cell(&source, x, y), "{x},{y}");
        }
    }
}

#[test]
fn bad_asset_folders_are_refused() {
    let p = scene(1);
    let d = DecodedLevel::of_pack(&p).unwrap();
    let asset = from_elements(&d.level(), &ids(&["wall", "ground"]), "Bits").unwrap();
    let good = encode(&asset).unwrap();
    assert!(decode(&good).is_ok());
    let refused = |what: &str, src: olr_format::assets::AssetSource| {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("a");
        // Written as files go (`save_dir` refuses nothing it can write).
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("asset.ron"),
            olr_format::ron_io::to_canonical("asset.ron", &src.file).unwrap(),
        )
        .unwrap();
        for (path, bytes) in &src.files {
            let to = dir.join(path);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(to, bytes).unwrap();
        }
        let e = load_dir(&dir).expect_err(what);
        e.to_string()
    };
    // A truncated PNG.
    let mut bad = good.clone();
    let layer = bad.files.keys().find(|k| k.starts_with("layers/")).unwrap().clone();
    let bytes = bad.files.get_mut(&layer).unwrap();
    bytes.truncate(bytes.len() / 2);
    refused("truncated", bad);
    // A mask of the wrong size.
    let mut bad = good.clone();
    let mask = bad.files.keys().find(|k| k.starts_with("glow/")).unwrap().clone();
    bad.files.insert(mask, olr_format::image_io::encode_grey(3, 3, &[255; 9]));
    refused("wrong-size mask", bad);
    // A size over the level limits.
    let mut bad = good.clone();
    bad.file.size = (olr_format::levels::MAX_SIDE + 1, 4);
    refused("too large", bad);
    // A glow naming a group the asset does not hold.
    let mut bad = good.clone();
    bad.file.glow_groups.clear();
    let why = refused("unknown group", bad);
    assert!(why.contains("lamps"), "{why}");
    // A missing file, and no folder at all.
    let mut bad = good.clone();
    bad.files.remove(&layer);
    refused("missing layer", bad);
    assert!(load_dir(std::path::Path::new("/nonexistent/asset")).is_err());
}

#[test]
fn the_library_adds_imports_lists_and_removes() {
    let p = scene(1);
    let d = DecodedLevel::of_pack(&p).unwrap();
    let asset = from_elements(&d.level(), &ids(&["ground"]), "Red ground!").unwrap();
    let t = tempfile::tempdir().unwrap();
    let lib = t.path().join("assets");
    assert_eq!(list_library(&lib, 10).0, vec![], "a missing library is empty");
    assert_eq!(add_to_library(&lib, &asset, None).unwrap(), "red-ground");
    assert_eq!(add_to_library(&lib, &asset, None).unwrap(), "red-ground-2");
    // Over a folder: replaced whole.
    let mut renamed = asset.clone();
    renamed.name = "Renamed".into();
    assert_eq!(add_to_library(&lib, &renamed, Some("red-ground-2")).unwrap(), "red-ground-2");
    assert!(add_to_library(&lib, &asset, Some("../x")).is_err());
    let (list, warnings) = list_library(&lib, 10);
    assert!(warnings.is_empty(), "{warnings:?}");
    let names: Vec<(&str, &str)> = list.iter().map(|(id, s)| (id.as_str(), s.name.as_str())).collect();
    assert_eq!(names, [("red-ground", "Red ground!"), ("red-ground-2", "Renamed")]);
    assert_eq!(list[0].1.tags, ["dirt"]);
    assert!(!list[0].1.glows && list[0].1.thumb.is_some());
    // An import is checked whole, and a name of yours gets a number.
    let id = import_into_library(&lib, &lib.join("red-ground")).unwrap();
    assert_eq!(id, "red-ground-2-2");
    assert_eq!(load_dir(&lib.join(&id)).unwrap().name, "Red ground! 2");
    std::fs::write(lib.join("red-ground").join("layers").join("ground.png"), b"not a png").unwrap();
    assert!(import_into_library(&lib, &lib.join("red-ground")).is_err());
    // A folder that does not read is a warning, not offered.
    let (list, warnings) = list_library(&lib, 10);
    assert_eq!(list.len(), 3, "the summary reads asset.ron and the thumbnail only");
    assert!(warnings.is_empty());
    std::fs::write(lib.join("red-ground").join("asset.ron"), b"(").unwrap();
    let (list, warnings) = list_library(&lib, 10);
    assert_eq!((list.len(), warnings.len()), (2, 1));
    // Removing: a valid folder of the library only.
    remove_from_library(&lib, "red-ground").unwrap();
    assert!(!lib.join("red-ground").exists());
    assert!(remove_from_library(&lib, "..").is_err());
    assert!(remove_from_library(&lib, "red-ground").is_err());
    assert_eq!(asset_slug("  Über lamp #2 "), "ber-lamp-2");
    assert_eq!(asset_slug("!!!"), "asset");
}

fn material(id: &str, name: &str) -> olr_format::materials::MaterialSource {
    olr_format::materials::MaterialSource {
        light_soak: None,
        id: id.into(),
        name: name.into(),
        notes: String::new(),
        heat: None,
    }
}

/// `scene(s)` whose look has glass, stone, wood and metal (issue #38): the
/// ground of stone, the pillar of metal but for its top two rows, painted
/// wood.
fn made_of(s: u32) -> Pack {
    let mut p = scene(s);
    let Source::Level(l) = &mut p.source else { panic!() };
    l.look.as_mut().unwrap().materials = vec![
        material("glass", "Glass"),
        material("stone", "Stone"),
        material("wood", "Wood"),
        material("metal", "Metal"),
    ];
    set(&mut p, "ground", Property::Surface(Some("stone".into()))).unwrap();
    set(&mut p, "pillar", Property::Surface(Some("metal".into()))).unwrap();
    // The pillar is a level-sized layer: its map is the level's 12 × 8.
    let mut map = vec![0u8; 12 * 8];
    for y in 2..4 {
        for x in 8..10 {
            map[y * 12 + x] = 3;
        }
    }
    set_surface_map(&mut p, "pillar", false, Some(&map)).unwrap();
    p
}

/// The baked front surface map's value at world pixel (x, y).
fn front_surface(l: &LevelSource, x: u32, y: u32) -> u8 {
    l.surface.as_ref().expect("surface maps").front[(y * l.size.0 + x) as usize]
}

#[test]
fn materials_and_painted_maps_travel_with_an_asset() {
    for s in [1, 2] {
        let p = made_of(s);
        let decoded = DecodedLevel::of_pack(&p).unwrap();
        let asset = from_elements(&decoded.level(), &ids(&["ground", "pillar"]), "Bits").unwrap();
        // The materials named, in the look's order: glass is not.
        let named: Vec<&str> = asset.materials.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(named, ["stone", "wood", "metal"], "scale {s}");
        let pillar = &asset.pieces[1];
        assert_eq!(pillar.element.surface.as_deref(), Some("metal"));
        // Cropped to the pillar's box (8, 2, 2 × 6) and re-numbered: wood
        // is 2 in the asset.
        let map = pillar.surface_map.as_ref().expect("its map");
        assert_eq!(map.len(), 12, "scale {s}");
        assert_eq!(&map[..], &[2, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0]);
        let back = through_folder(&asset);
        assert_eq!(back, asset, "scale {s}");

        // A level with wood (its own settings) and sky: wood joined,
        // stone and metal added after it.
        let mut target = pack(
            20,
            12,
            s,
            Some(LookSource { materials: vec![material("wood", "Oak")], ..Default::default() }),
        );
        let placed = place_in_pack(&mut target, &back, (3, 4)).unwrap();
        assert_eq!(placed.materials.joined, ids(&["wood"]));
        assert_eq!(placed.materials.added, ids(&["stone", "metal"]));
        assert!(!placed.materials.added_look);
        let look = level(&target).look.clone().unwrap();
        assert_eq!(
            look.materials,
            vec![material("wood", "Oak"), material("stone", "Stone"), material("metal", "Metal")]
        );
        let l = baked(&target);
        // The pillar's top (asset 8, 0) is wood (1), its foot metal (3),
        // the ground stone (2).
        assert_eq!(front_surface(&l, 3 + 8, 4), 1, "scale {s}");
        assert_eq!(front_surface(&l, 3 + 9, 4 + 1), 1, "scale {s}");
        assert_eq!(front_surface(&l, 3 + 8, 4 + 2), 3, "scale {s}");
        assert_eq!(front_surface(&l, 3, 4 + 5), 2, "scale {s}");
        assert_eq!(front_surface(&l, 0, 0), 0);
    }
}

#[test]
fn placing_materials_gives_a_level_a_look_and_checks() {
    let p = made_of(1);
    let decoded = DecodedLevel::of_pack(&p).unwrap();
    let asset =
        cut_rect(&decoded.level(), Rect { x: 6, y: 0, w: 6, h: 8 }, Some(&ids(&["pillar"])), "Top").unwrap();
    assert_eq!(asset.materials.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["wood", "metal"]);
    let mut bare = pack(12, 8, 1, None);
    let placed = place_in_pack(&mut bare, &asset, (0, 0)).unwrap();
    assert!(placed.materials.added_look && !placed.groups.added_look);
    assert_eq!(placed.materials.added, ids(&["wood", "metal"]));
    // Baked, the pack checks as a save would.
    let l = baked(&bare);
    bare.source = Source::Level(l);
    olr_edit::pack::check(&olr_edit::pack::files(&bare).unwrap()).unwrap();
}

/// Anti-goal of issue #38: an asset without materials names none, and its
/// placing changes no material.
#[test]
fn an_asset_without_materials_is_as_before() {
    let p = scene(1);
    let decoded = DecodedLevel::of_pack(&p).unwrap();
    let asset = from_elements(&decoded.level(), &ids(&["ground"]), "Ground").unwrap();
    assert!(asset.materials.is_empty() && asset.pieces[0].surface_map.is_none());
    let src = encode(&asset).unwrap();
    let text = olr_format::ron_io::to_canonical("asset.ron", &src.file).unwrap();
    assert!(!text.contains("materials") && !text.contains("surface"), "{text}");
    assert!(src.files.keys().all(|f| !f.starts_with("surface/")));
    let mut target = pack(12, 8, 1, None);
    let placed = place_in_pack(&mut target, &asset, (0, 0)).unwrap();
    assert!(!placed.materials.changes());
    assert_eq!(level(&target).look, None);
}
