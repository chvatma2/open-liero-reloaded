//! Assets (issue #9): saved and placed pieces round-trip their pixels,
//! materials, glow masks and groups, at one art scale and across two; a
//! rectangle's cut follows its rules; placing (issue #75: a copy,
//! `placed::add_copy`, baked) shares or adds glow groups; a bad folder is
//! refused; the user's library.

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
    Pack::new(manifest, Source::Level(level))
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

/// `asset` placed in `target` as a copy of the user's (issue #75) with its
/// top-left at `at`, and baked: what the copy brought.
fn place(target: &mut Pack, asset: &Asset, at: (i32, i32)) -> olr_edit::placed::Added {
    ensure_composed(target).unwrap();
    let origin = olr_format::levels::AssetOrigin::Yours("a".into());
    let added = olr_edit::placed::add_copy(target, &encode(asset).unwrap(), origin, at, None, None).unwrap();
    olr_edit::placed::bake_pack(target, &RgbaImage::new(1, 1)).unwrap();
    added
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
        let placed = place(&mut target, &back, (3, 4));
        assert_eq!(placed.id, "bits");
        assert!(placed.groups_added.is_empty() && !placed.added_look, "no glow, no look");
        assert_eq!(level(&target).look, None);
        let landed = level(&target).clone();
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
    let placed = place(&mut bare, &asset, (0, 0));
    assert!(placed.added_look);
    assert_eq!(placed.groups_added, ids(&["lamps"]));
    assert_eq!(
        level(&bare).look,
        Some(LookSource { glow_groups: vec![group("lamps", 777)], ..LookSource::default() })
    );
    let copy = &bare.placed.as_ref().unwrap().assets[&placed.key];
    assert_eq!(
        copy.files["glow/wall.png"],
        olr_format::image_io::encode_grey(24, 16, &mask),
        "the mask is kept"
    );
    let glow = level(&bare).glow.clone().expect("it glows");
    assert_eq!(glow.groups[(2 * 12 + 2) as usize], 1);

    // A level with the group of other settings: the copy's own under a
    // new id (issue #73's gathering), the level's kept.
    let mut has =
        pack(12, 8, 2, Some(LookSource { glow_groups: vec![group("lamps", 5)], ..Default::default() }));
    let placed = place(&mut has, &asset, (0, 0));
    assert_eq!((placed.groups_added.clone(), placed.added_look), (ids(&["lamps-2"]), false));
    let renamed = GlowGroupSource { id: "lamps-2".into(), ..group("lamps", 777) };
    assert_eq!(level(&has).look.as_ref().unwrap().glow_groups, vec![group("lamps", 5), renamed]);
    // Of the same settings: shared.
    let mut same =
        pack(12, 8, 2, Some(LookSource { glow_groups: vec![group("lamps", 777)], ..Default::default() }));
    assert!(place(&mut same, &asset, (0, 0)).groups_added.is_empty());
    assert_eq!(level(&same).look.as_ref().unwrap().glow_groups, vec![group("lamps", 777)]);

    // A level with another group: the asset's added after it, with the
    // asset's settings (not a new group's defaults).
    let mut other =
        pack(12, 8, 2, Some(LookSource { glow_groups: vec![group("sky", 300)], ..Default::default() }));
    place(&mut other, &asset, (0, 0));
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
    let placed = place(&mut target, &asset, (0, 0));
    assert_eq!(placed.id, "two");
    let l = level(&target).clone();
    assert_eq!(l.art_scale, 3);
    // Each world pixel's art block is the one colour it had.
    assert_eq!(cell(&l, 5, 7), (Material::Dirt, vec![RED; 9], cell(&l, 5, 7).2));
    assert_eq!(cell(&l, 5, 1).1, vec![BLUE; 9]);
    // The mask, resampled: three times as fine, glowing where it did.
    let glow = l.glow.expect("it glows");
    assert_eq!(glow.art, (36, 24));
    assert!(glow.back[(7 * 36 + 7) as usize] > 0);
    assert_eq!(glow.back[(7 * 36 + 13) as usize], 0);
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
    place(&mut target, &through_folder(&a), (1, 1));
    let landed = level(&target).clone();
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

        // A level with wood of other settings: stone, the asset's wood
        // (as wood-2: issue #73's gathering) and metal added after it.
        let mut target = pack(
            20,
            12,
            s,
            Some(LookSource { materials: vec![material("wood", "Oak")], ..Default::default() }),
        );
        let placed = place(&mut target, &back, (3, 4));
        assert_eq!(placed.materials_added, ids(&["stone", "wood-2", "metal"]));
        assert!(!placed.added_look);
        let look = level(&target).look.clone().unwrap();
        assert_eq!(
            look.materials,
            vec![
                material("wood", "Oak"),
                material("stone", "Stone"),
                material("wood-2", "Wood"),
                material("metal", "Metal")
            ]
        );
        let l = level(&target).clone();
        // The pillar's top (asset 8, 0) is the asset's wood (3), its foot
        // metal (4), the ground stone (2).
        assert_eq!(front_surface(&l, 3 + 8, 4), 3, "scale {s}");
        assert_eq!(front_surface(&l, 3 + 9, 4 + 1), 3, "scale {s}");
        assert_eq!(front_surface(&l, 3 + 8, 4 + 2), 4, "scale {s}");
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
    let placed = place(&mut bare, &asset, (0, 0));
    assert!(placed.added_look && placed.groups_added.is_empty());
    assert_eq!(placed.materials_added, ids(&["wood", "metal"]));
    // Baked, the pack checks as a save would.
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
    let placed = place(&mut target, &asset, (0, 0));
    assert!(placed.materials_added.is_empty() && !placed.added_look);
    assert_eq!(level(&target).look, None);
}

// Issue #71: states, exposed properties, sounds, health and a script.

use olr_format::Colour;
use olr_format::assets::{ExposedSource, PropertySource as P, StateValue, ValueKind, ValueSource as V};
use olr_format::look::GlowColourSource;

/// A WAV of `data` bytes at `byte_rate` bytes a second.
fn wav(byte_rate: u32, data: u32) -> Vec<u8> {
    let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 1, 0]);
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&byte_rate.to_le_bytes());
    b.extend_from_slice(&[1, 0, 8, 0]);
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data.min(1000) as usize, 0);
    b
}

fn filled(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut i = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            i.set(x, y, c);
        }
    }
    i
}

/// A drawn layer `id` of `material` at `at`, `size` world pixels, filled
/// with `c` at art scale 2, glowing in `glow` (strength 80, a mask of 200
/// everywhere) when given.
fn layer(
    id: &str,
    material: ElementMaterial,
    at: (i32, i32),
    size: (u32, u32),
    c: Rgba,
    glow: Option<&str>,
) -> Piece {
    let element = olr_format::levels::Element {
        id: id.into(),
        name: String::new(),
        visible: true,
        locked: false,
        folder: None,
        kind: ElementKind::Drawn { material, at, size: Some(size), mixed: false, back: false },
        glow: glow.map(|g| ElementGlow { strength: 80, group: g.into(), mask: true, beam: None }),
        clear: false,
        glass: None,
        distant: None,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    };
    let cells = (size.0 * 2 * size.1 * 2) as usize;
    Piece {
        element,
        decoded: Decoded::Drawn { art: filled(size.0 * 2, size.1 * 2, c), cells: None, back: None },
        original: None,
        glow_mask: glow.map(|_| std::sync::Arc::new(vec![200; cells])),
        surface_map: None,
        back_surface_map: None,
    }
}

fn colour(hex: &str) -> Colour {
    Colour::parse(hex).unwrap()
}

/// A 4 × 3 lamp at art scale 2: `shade` (rock, green, the top row) over
/// `lamp` (background, blue, glowing in `light`, a white `Tint`), one
/// state.
fn lamp() -> Asset {
    let mut light = group("light", 300);
    light.colour = GlowColourSource::Tint(colour("#ffffff"));
    Asset::new(
        "Lamp",
        2,
        (4, 3),
        vec![
            layer("lamp", ElementMaterial::Background, (0, 0), (4, 3), BLUE, Some("light")),
            layer("shade", ElementMaterial::Rock, (0, 0), (4, 1), GREEN, None),
        ],
        vec![light],
        Vec::new(),
    )
}

fn state(
    name: &str,
    values: Vec<(olr_format::assets::PropertySource, olr_format::assets::ValueSource)>,
) -> State {
    State {
        values: values.into_iter().map(|(property, value)| StateValue { property, value }).collect(),
        ..State::new(name)
    }
}

fn own(piece: &Piece, c: Rgba, mask: u8) -> OwnPicture {
    let Decoded::Drawn { art, .. } = &piece.decoded else { panic!() };
    OwnPicture {
        decoded: Decoded::Drawn { art: filled(art.width(), art.height(), c), cells: None, back: None },
        glow_mask: piece.glow_mask.as_ref().map(|m| std::sync::Arc::new(vec![mask; m.len()])),
    }
}

fn expose(
    name: &str,
    property: olr_format::assets::PropertySource,
    default: Option<olr_format::assets::ValueSource>,
) -> ExposedSource {
    ExposedSource { name: name.into(), property, default, min: None, max: None }
}

/// The lamp with states Normal, Alarm (its own red lamp at mask 50, a red
/// glow, both sounds) and Broken (its own lamp and shade, dark), an
/// exposed glow strength, a script and health.
fn alarm_lamp() -> Asset {
    let mut a = lamp();
    let mut alarm = state("Alarm", vec![(P::GlowColour("lamp".into()), V::Colour(colour("#ff2020")))]);
    alarm.own.insert("lamp".into(), own(&a.pieces[0], RED, 50));
    alarm.loop_sound = Some("alarm.wav".into());
    alarm.enter_sound = Some("tick.wav".into());
    let mut broken = state("Broken", vec![(P::GlowStrength("lamp".into()), V::Int(0))]);
    broken.own.insert("lamp".into(), own(&a.pieces[0], Rgba::rgb(10, 10, 10), 0));
    broken.own.insert("shade".into(), own(&a.pieces[1], Rgba::rgb(20, 20, 20), 0));
    a.states = vec![State::new("Normal"), alarm, broken];
    a.exposed = vec![expose("glow", P::GlowStrength("lamp".into()), Some(V::Int(60)))];
    a.sounds = [("alarm.wav", wav(8000, 100)), ("tick.wav", wav(8000, 10))]
        .into_iter()
        .map(|(k, v)| (k.to_string(), std::sync::Arc::new(v)))
        .collect();
    a.script = Some("fn on_hit() {}\n".into());
    a.health = Some(olr_format::assets::HealthSource { points: 50, at_zero: "Broken".into() });
    a
}

fn piece<'a>(p: &'a Posed, id: &str) -> Option<&'a PosedPiece> {
    p.pieces.iter().find(|x| x.piece.element.id == id)
}

fn strength(p: &Posed) -> Option<u8> {
    piece(p, "lamp").and_then(|x| x.piece.element.glow.as_ref()).map(|g| g.strength)
}

#[test]
fn decode_and_encode_round_trip_states() {
    let a = alarm_lamp();
    let src = encode(&a).unwrap();
    let sheet = olr_format::image_io::png_size(&src.files["layers/lamp.png"]).unwrap();
    assert_eq!(sheet, (8, 6 * 3), "the lamp stacks three pictures");
    assert_eq!(olr_format::image_io::png_size(&src.files["layers/shade.png"]), Some((8, 2 * 2)));
    assert_eq!(src.file.states[2].own, ["lamp", "shade"]);
    let back = through_folder(&a);
    for (x, y) in a.states.iter().zip(&back.states) {
        assert_eq!(x.own, y.own, "{}", x.name);
    }
    assert_eq!(back, a);
}

#[test]
fn posed_takes_the_state_then_the_copy_then_the_default() {
    let mut a = alarm_lamp();
    a.states.push(state("Dim", vec![(P::GlowStrength("lamp".into()), V::Int(10))]));
    let none = BTreeMap::new();
    let copy: BTreeMap<String, olr_format::assets::ValueSource> = [("glow".to_string(), V::Int(70))].into();
    let dim = a.state("Dim").unwrap();
    // 1: the state's own value, whatever the copy says.
    assert_eq!(strength(&posed(&a, dim, &copy).unwrap()), Some(10));
    // 2: the copy's value.
    assert_eq!(strength(&posed(&a, 0, &copy).unwrap()), Some(70));
    // 3: the exposed default.
    assert_eq!(strength(&posed(&a, 0, &none).unwrap()), Some(60));
    // 4: the layer's own setting.
    a.exposed.clear();
    assert_eq!(strength(&posed(&a, 0, &none).unwrap()), Some(80));
    // A copy's unknown name or wrong value is an error.
    let a = alarm_lamp();
    assert!(posed(&a, 0, &[("nope".to_string(), V::Int(1))].into()).is_err());
    assert!(posed(&a, 0, &[("glow".to_string(), V::Bool(true))].into()).is_err());
    assert!(posed(&a, 0, &[("glow".to_string(), V::Int(101))].into()).is_err());
    assert!(posed(&a, 9, &none).is_err());
}

#[test]
fn a_state_is_a_complete_look() {
    // Off sets the glow to 0 and On nothing: On glows at the layer's own.
    let mut a = lamp();
    a.states = vec![State::new("On"), state("Off", vec![(P::GlowStrength("lamp".into()), V::Int(0))])];
    let none = BTreeMap::new();
    assert_eq!(strength(&posed(&a, 1, &none).unwrap()), None);
    assert_eq!(strength(&posed(&a, 0, &none).unwrap()), Some(80));
    // Alarm sets a colour and Normal does not: Normal has the copy's
    // colour, else the default.
    let mut a = lamp();
    a.states = vec![
        State::new("Normal"),
        state("Alarm", vec![(P::GlowColour("lamp".into()), V::Colour(colour("#ff2020")))]),
    ];
    a.exposed = vec![expose("tint", P::GlowColour("lamp".into()), Some(V::Colour(colour("#40ff80"))))];
    let tint = |p: Posed| p.groups[0].colour;
    let blue: BTreeMap<String, olr_format::assets::ValueSource> =
        [("tint".to_string(), V::Colour(colour("#0000ff")))].into();
    assert_eq!(tint(posed(&a, 0, &none).unwrap()), GlowColourSource::Tint(colour("#40ff80")));
    assert_eq!(tint(posed(&a, 0, &blue).unwrap()), GlowColourSource::Tint(colour("#0000ff")));
    assert_eq!(tint(posed(&a, 1, &blue).unwrap()), GlowColourSource::Tint(colour("#ff2020")));
}

#[test]
fn posed_mirrors_offsets_hides_and_tints() {
    let none = BTreeMap::new();
    let with = |values: Vec<(olr_format::assets::PropertySource, olr_format::assets::ValueSource)>| {
        let mut a = lamp();
        a.states = vec![state("S", values)];
        a
    };
    // Facing mirrors the art within the asset's box and turns a beam.
    let mut a = with(vec![(P::Facing, V::Bool(true))]);
    let mut half = layer("half", ElementMaterial::Rock, (0, 1), (2, 1), RED, Some("light"));
    if let Decoded::Drawn { art, .. } = &mut half.decoded {
        art.set(0, 0, BLUE);
    }
    half.element.glow.as_mut().unwrap().beam =
        Some(olr_format::levels::GlowBeam { direction: 0, spread: 20 });
    a.pieces.push(half);
    let p = posed(&a, 0, &none).unwrap();
    let h = &piece(&p, "half").unwrap().piece;
    let ElementKind::Drawn { at, .. } = h.element.kind else { panic!() };
    assert_eq!(at, (2, 1));
    let Decoded::Drawn { art, .. } = &h.decoded else { panic!() };
    assert_eq!((art.get(3, 0), art.get(0, 0)), (BLUE, RED));
    assert_eq!(h.element.glow.as_ref().unwrap().beam.unwrap().direction, 180);
    // OffsetX moves one piece.
    let p = posed(&with(vec![(P::OffsetX("shade".into()), V::Int(2))]), 0, &none).unwrap();
    let at = |id: &str| match piece(&p, id).unwrap().piece.element.kind {
        ElementKind::Drawn { at, .. } => at,
        _ => panic!(),
    };
    assert_eq!((at("shade"), at("lamp")), ((2, 0), (0, 0)));
    // LayerVisible false drops it.
    let p = posed(&with(vec![(P::LayerVisible("shade".into()), V::Bool(false))]), 0, &none).unwrap();
    assert!(piece(&p, "shade").is_none() && piece(&p, "lamp").is_some());
    assert!(posed(&with(vec![(P::Visible, V::Bool(false))]), 0, &none).unwrap().pieces.is_empty());
    // GlowStrength 0 unglows; 50 keeps the mask (each pixel mask × 50).
    let p = posed(&with(vec![(P::GlowStrength("lamp".into()), V::Int(0))]), 0, &none).unwrap();
    let l = &piece(&p, "lamp").unwrap().piece;
    assert!(l.element.glow.is_none() && l.glow_mask.is_none());
    let p = posed(&with(vec![(P::GlowStrength("lamp".into()), V::Int(50))]), 0, &none).unwrap();
    let l = &piece(&p, "lamp").unwrap().piece;
    assert_eq!(l.element.glow.as_ref().unwrap().strength, 50);
    assert_eq!(l.glow_mask, lamp().pieces[0].glow_mask);
    // GlowColour sets a Tint, and keeps a Filtered group Filtered.
    let red = V::Colour(colour("#ff2020"));
    let p = posed(&with(vec![(P::GlowColour("lamp".into()), red.clone())]), 0, &none).unwrap();
    assert_eq!(p.groups[0].colour, GlowColourSource::Tint(colour("#ff2020")));
    let mut a = with(vec![(P::GlowColour("lamp".into()), red)]);
    a.groups[0].colour = GlowColourSource::Filtered(colour("#ffffff"));
    assert_eq!(posed(&a, 0, &none).unwrap().groups[0].colour, GlowColourSource::Filtered(colour("#ff2020")));
    // Opacity 50 is carried, the art untouched.
    let p = posed(&with(vec![(P::Opacity("lamp".into()), V::Int(50))]), 0, &none).unwrap();
    let l = piece(&p, "lamp").unwrap();
    assert_eq!(l.opacity, 50);
    assert_eq!(l.piece.decoded, lamp().pieces[0].decoded);
    assert_eq!(piece(&p, "shade").unwrap().opacity, 100);
}

#[test]
fn the_starting_state_poses_as_the_pieces() {
    for a in [lamp(), alarm_lamp()] {
        let mut a = a;
        a.exposed.clear();
        let p = posed(&a, a.start, &BTreeMap::new()).unwrap();
        let pieces: Vec<Piece> = p.pieces.iter().map(|x| x.piece.clone()).collect();
        assert_eq!(pieces, a.pieces);
        assert!(p.pieces.iter().all(|x| x.opacity == 100));
        assert_eq!((p.groups, p.materials), (a.groups.clone(), a.materials.clone()));
    }
}

#[test]
fn live_refusal_says_why() {
    let a = lamp();
    assert_eq!(live_refusal(&a), Some(LiveRefusal::NothingToDrive));
    assert_eq!(
        LiveRefusal::NothingToDrive.to_string(),
        "it has a single state, no exposed values, no sounds and no script"
    );
    // A live-able lamp: two states.
    let mut a = lamp();
    a.states.push(State::new("Off"));
    assert_eq!(live_refusal(&a), None);
    // Nothing to drive comes first.
    let mut d = lamp();
    d.pieces.push(layer("mud", ElementMaterial::Dirt, (0, 2), (4, 1), RED, None));
    assert_eq!(live_refusal(&d), Some(LiveRefusal::NothingToDrive));
    d.script = Some(String::new());
    assert_eq!(live_refusal(&d), Some(LiveRefusal::Dirt));
    assert_eq!(LiveRefusal::Dirt.to_string(), "it holds dirt");
    // A mixed layer with a dirt cell.
    let mut m = a.clone();
    let mut mixed = layer("mix", ElementMaterial::Rock, (0, 2), (4, 1), RED, None);
    if let ElementKind::Drawn { mixed: on, .. } = &mut mixed.element.kind {
        *on = true;
    }
    if let Decoded::Drawn { cells, .. } = &mut mixed.decoded {
        *cells = Some(vec![Material::Rock; 4]);
    }
    m.pieces.push(mixed.clone());
    assert_eq!(live_refusal(&m), None);
    if let Decoded::Drawn { cells: Some(c), .. } = &mut m.pieces[2].decoded {
        c[3] = Material::Dirt;
    }
    assert_eq!(live_refusal(&m), Some(LiveRefusal::Dirt));
    // A raster.
    let mut r = a.clone();
    r.pieces.insert(
        0,
        Piece {
            element: olr_format::levels::Element {
                kind: ElementKind::Raster,
                glow: None,
                ..layer("base", ElementMaterial::Rock, (0, 0), (4, 3), RED, None).element
            },
            decoded: Decoded::Raster {
                material: vec![Material::Background; 12],
                front: RgbaImage::new(8, 6),
                back: RgbaImage::new(8, 6),
            },
            original: None,
            glow_mask: None,
            surface_map: None,
            back_surface_map: None,
        },
    );
    assert_eq!(live_refusal(&r), Some(LiveRefusal::RasterOrDistant));
    assert_eq!(LiveRefusal::RasterOrDistant.to_string(), "it holds a raster or distant layer");
}

#[test]
fn offers_follow_the_design() {
    let mut a = lamp();
    a.pieces.push(layer("mud", ElementMaterial::Dirt, (0, 2), (4, 1), RED, None));
    a.pieces.push(layer("button", ElementMaterial::Rock, (1, 1), (1, 1), RED, Some("light")));
    let offers = offers(&a);
    let of = |id: &str| {
        offers
            .iter()
            .filter(|o| o.property.layer() == Some(id))
            .map(|o| o.property.clone())
            .collect::<Vec<_>>()
    };
    // The asset's own: no health without health.
    let own: Vec<_> =
        offers.iter().filter(|o| o.property.layer().is_none()).map(|o| o.property.clone()).collect();
    assert_eq!(own, [P::X, P::Y, P::Visible, P::Facing]);
    // A rock layer: its place and visibility, no opacity.
    assert_eq!(
        of("shade"),
        [P::OffsetX("shade".into()), P::OffsetY("shade".into()), P::LayerVisible("shade".into())]
    );
    // A dirt layer: none.
    assert!(of("mud").is_empty());
    // A glowing background layer: opacity and its glow too.
    assert_eq!(
        of("lamp"),
        [
            P::OffsetX("lamp".into()),
            P::OffsetY("lamp".into()),
            P::LayerVisible("lamp".into()),
            P::Opacity("lamp".into()),
            P::GlowStrength("lamp".into()),
            P::GlowColour("lamp".into()),
            P::LightIntensity("lamp".into()),
            P::LightReach("lamp".into()),
            P::GlowAnimation("lamp".into()),
        ]
    );
    let offer =
        |p: olr_format::assets::PropertySource| offers.iter().find(|o| o.property == p).unwrap().clone();
    let o = offer(P::GlowColour("lamp".into()));
    assert_eq!((o.kind, o.range, o.sharing), (ValueKind::Colour, None, vec!["button".to_string()]));
    assert_eq!(offer(P::GlowColour("button".into())).sharing, ["lamp"]);
    assert_eq!(offer(P::LightIntensity("button".into())).sharing, ["lamp"]);
    assert!(offer(P::GlowStrength("button".into())).sharing.is_empty());
    assert_eq!(offer(P::LightReach("lamp".into())).range, Some((1, 4096)));
    assert_eq!(offer(P::X).range, Some((-4096, 4096)));
    a.health = Some(olr_format::assets::HealthSource { points: 30, at_zero: "Normal".into() });
    assert!(olr_edit::asset::offers(&a).iter().any(|o| o.property == P::Health && o.range == Some((0, 30))));
}

#[test]
fn asset_sound_limit_is_the_mods() {
    assert_eq!(olr_format::assets::MAX_ASSET_SOUND_BYTES, olr_pack::sounds::MAX_SOUND_BYTES as u64);
    let mut src = encode(&alarm_lamp()).unwrap();
    decode(&src).unwrap();
    // A WAV a thousand seconds long by its header.
    let long = wav(1, 1000);
    assert!(olr_pack::sounds::check_sound(&long).is_err());
    src.sounds.insert("tick.wav".into(), long);
    let e = decode(&src).unwrap_err().to_string();
    assert!(e.contains("sounds/tick.wav"), "{e}");
    src.sounds.insert("tick.wav".into(), b"not a sound".to_vec());
    assert!(decode(&src).unwrap_err().to_string().contains("WAV or Ogg"));
}

#[test]
fn a_copy_of_a_stated_asset_bakes_its_start() {
    // Off (dark, unlit) first; the start, On, owns nothing.
    let mut a = lamp();
    let mut off = state("Off", vec![(P::GlowStrength("lamp".into()), V::Int(0))]);
    off.own.insert("lamp".into(), own(&a.pieces[0], RED, 0));
    a.states = vec![off, State::new("On")];
    a.start = 1;
    let a = through_folder(&a);
    assert_eq!(a.start, 1);
    let mut p = pack(12, 8, 2, None);
    let placed = place(&mut p, &a, (3, 2));
    // The copy's asset keeps its states (issue #75: nothing stays behind).
    assert_eq!(p.placed.as_ref().unwrap().assets[&placed.key].file.states.len(), 2);
    let l = level(&p).clone();
    // The lamp's start, blue and glowing, not Off's red.
    let (_, front, _) = cell(&l, 4, 4);
    assert!(front.iter().all(|c| *c == BLUE), "{front:?}");
    let glow = l.glow.expect("the start glows");
    assert!(glow.back[(4 * 2 * 24 + 4 * 2) as usize] > 0);
}

// Issue #72: the operations that edit states, pictures, values, exposed
// properties, sounds, script and health.

/// `f` on `a` is refused with a message holding every one of `words`, and
/// leaves `a` as it was.
fn refused(a: &Asset, words: &[&str], f: impl FnOnce(&mut Asset) -> Result<(), AssetError>) {
    let mut b = a.clone();
    let e = f(&mut b).expect_err("refused").to_string();
    for w in words {
        assert!(e.contains(w), "{w:?} not in {e:?}");
    }
    assert_eq!(&b, a, "a refusal leaves the asset unchanged");
}

#[test]
fn a_new_state_copies_the_start() {
    let mut a = alarm_lamp();
    a.states[0].values = vec![StateValue { property: P::Facing, value: V::Bool(true) }];
    let i = add_state(&mut a, "Dim", None).unwrap();
    assert_eq!(i, 3);
    let dim = &a.states[3];
    assert_eq!(dim.name, "Dim");
    assert!(dim.own.is_empty());
    assert_eq!(dim.values, a.states[0].values);
    assert_eq!((dim.loop_sound.clone(), dim.enter_sound.clone()), (None, None));
    // It looks like the start.
    let none = BTreeMap::new();
    assert_eq!(posed(&a, 3, &none).unwrap(), posed(&a, 0, &none).unwrap());
    // From Alarm: its pictures, values and sounds.
    let i = add_state(&mut a, "Alarm 2", Some("Alarm")).unwrap();
    let (x, y) = (&a.states[1], &a.states[i]);
    assert_eq!(
        (&x.own, &x.values, &x.loop_sound, &x.enter_sound),
        (&y.own, &y.values, &y.loop_sound, &y.enter_sound)
    );
    let a = through_folder(&a);
    assert_eq!(a.states.len(), 5);
    // A taken name (case aside), a bad one, an unknown source.
    refused(&a, &["\"alarm\"", "Alarm"], |a| add_state(a, "alarm", None).map(drop));
    refused(&a, &["\"Al/arm\""], |a| add_state(a, "Al/arm", None).map(drop));
    refused(&a, &["no state \"Gone\""], |a| add_state(a, "New", Some("Gone")).map(drop));
}

/// A small asset of `layers` drawn layers (each glowing, masked), `states`
/// states, state `k` owning layer `l` when `owns[k][l]`, picture colours
/// and masks from `seed`.
fn random_asset(layers: usize, owns: &[Vec<bool>], seed: u8, start: usize) -> Asset {
    let mut a = Asset::new("R", 2, (4, 3), Vec::new(), vec![group("light", 300)], Vec::new());
    for l in 0..layers {
        let c = Rgba::rgb(seed.wrapping_add(l as u8 * 40), 10, 10);
        a.pieces.push(layer(&format!("l{l}"), ElementMaterial::Background, (0, 0), (4, 3), c, Some("light")));
    }
    a.states = (0..owns.len()).map(|k| State::new(&format!("S{k}"))).collect();
    for (k, row) in owns.iter().enumerate() {
        for (l, on) in row.iter().enumerate().take(layers) {
            if *on && k != start {
                let c = Rgba::rgb(10, seed.wrapping_add(k as u8 * 30 + l as u8), 10);
                let mask = seed.wrapping_mul(k as u8 + 1);
                let mut pic = own(&a.pieces[l], c, mask);
                // Some pictures keep the start's mask.
                if (k + l) % 3 == 0 {
                    pic.glow_mask = None;
                }
                a.states[k].own.insert(format!("l{l}"), pic);
            }
        }
    }
    a.states[start].values = vec![StateValue { property: P::GlowStrength("l0".into()), value: V::Int(40) }];
    a.start = start;
    a
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]
    #[test]
    fn set_start_keeps_every_states_look(
        layers in 1usize..4,
        owns in proptest::collection::vec(proptest::collection::vec(proptest::bool::ANY, 3), 2..5),
        seed in proptest::prelude::any::<u8>(),
        start in 0usize..4,
        to in 0usize..4,
    ) {
        let n = owns.len();
        let (start, to) = (start % n, to % n);
        let mut a = random_asset(layers, &owns, seed, start);
        let none = BTreeMap::new();
        let looks: Vec<Posed> = (0..n).map(|s| posed(&a, s, &none).unwrap()).collect();
        let name = a.states[to].name.clone();
        set_start(&mut a, &name).unwrap();
        proptest::prop_assert_eq!(a.start, to);
        proptest::prop_assert!(a.states[to].own.is_empty());
        for (s, look) in looks.iter().enumerate() {
            proptest::prop_assert_eq!(&posed(&a, s, &none).unwrap(), look, "state {}", s);
        }
        // And through the folder.
        let b = decode(&encode(&a).unwrap()).unwrap();
        for (s, look) in looks.iter().enumerate() {
            proptest::prop_assert_eq!(&posed(&b, s, &none).unwrap(), look, "state {} read back", s);
        }
    }
}

#[test]
fn removing_states_is_refused_when_needed() {
    let a = alarm_lamp();
    refused(&a, &["\"Normal\"", "make another state the start first"], |a| remove_state(a, "Normal"));
    refused(&a, &["\"Broken\"", "health"], |a| remove_state(a, "Broken"));
    refused(&a, &["no state \"Gone\""], |a| remove_state(a, "Gone"));
    // Another goes, the start's index following.
    let mut b = a.clone();
    set_start(&mut b, "Broken").unwrap();
    b.health = None;
    remove_state(&mut b, "Alarm").unwrap();
    assert_eq!(b.states.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Normal", "Broken"]);
    assert_eq!(b.states[b.start].name, "Broken");
    remove_state(&mut b, "Normal").unwrap();
    assert_eq!((b.states.len(), b.start), (1, 0));
    // The last state is the start: never removed.
    refused(&b, &["starting state"], |a| remove_state(a, "Broken"));
}

#[test]
fn renaming_a_state_follows_start_and_health() {
    let mut a = alarm_lamp();
    set_start(&mut a, "Broken").unwrap();
    rename_state(&mut a, "Broken", "Wrecked").unwrap();
    assert_eq!(a.states[a.start].name, "Wrecked");
    assert_eq!(a.health.as_ref().unwrap().at_zero, "Wrecked");
    let b = through_folder(&a);
    assert_eq!((b.states[b.start].name.as_str(), b.health.unwrap().at_zero.as_str()), ("Wrecked", "Wrecked"));
    // Its own case is free; another's name is not.
    rename_state(&mut a, "Wrecked", "WRECKED").unwrap();
    refused(&a, &["\"normal\"", "Normal"], |a| rename_state(a, "Alarm", "normal"));
    refused(&a, &["no state \"Gone\""], |a| rename_state(a, "Gone", "X"));
}

#[test]
fn own_picture_checks_fit_and_kind() {
    let mut a = alarm_lamp();
    let fit = own(&a.pieces[1], RED, 0);
    // A misfit picture names the sizes.
    let small = OwnPicture {
        decoded: Decoded::Drawn { art: RgbaImage::new(4, 2), cells: None, back: None },
        glow_mask: None,
    };
    refused(&a, &["\"Alarm\"", "\"shade\"", "4 × 2", "8 × 2"], |a| own_picture(a, "Alarm", "shade", small));
    // A glow mask on a layer without one.
    let masked = OwnPicture { glow_mask: Some(std::sync::Arc::new(vec![9; 16])), ..fit.clone() };
    refused(&a, &["\"shade\"", "no glow mask"], |a| own_picture(a, "Alarm", "shade", masked));
    let mut wrong = own(&a.pieces[0], RED, 0);
    wrong.glow_mask = Some(std::sync::Arc::new(vec![9; 5]));
    refused(&a, &["\"lamp\"", "glow mask", "8 × 6"], |a| own_picture(a, "Alarm", "lamp", wrong));
    // The start paints the layer itself.
    refused(&a, &["\"Normal\"", "starting state"], |a| own_picture(a, "Normal", "shade", fit.clone()));
    refused(&a, &["no layer \"roof\""], |a| own_picture(a, "Alarm", "roof", fit.clone()));
    // An image is the same in every state.
    let mut img = a.clone();
    let decoded = Decoded::Image { grid: Some(std::sync::Arc::new(RgbaImage::new(8, 6))), mask: None };
    img.pieces.push(Piece {
        element: olr_format::levels::Element {
            kind: ElementKind::Image {
                material: ElementMaterial::Background,
                source: "logo.png".into(),
                at: (0, 0),
                size: (4, 3),
                pixel_size: 1,
                colours: None,
                dither: olr_format::levels::Dither::None,
                alpha_threshold: 128,
                mask: false,
            },
            glow: None,
            ..layer("logo", ElementMaterial::Background, (0, 0), (4, 3), RED, None).element
        },
        decoded,
        original: Some((
            "logo.png".into(),
            std::sync::Arc::new(olr_format::image_io::encode_rgba(&RgbaImage::new(4, 3))),
        )),
        glow_mask: None,
        surface_map: None,
        back_surface_map: None,
    });
    let pic = own(&a.pieces[0], RED, 0);
    refused(&img, &["\"logo\"", "image"], |a| own_picture(a, "Alarm", "logo", pic));
    // A fitting one is kept, and follow_start drops it.
    own_picture(&mut a, "Alarm", "shade", fit.clone()).unwrap();
    assert_eq!(a.states[1].own.get("shade"), Some(&fit));
    let b = through_folder(&a);
    assert_eq!(b.states[1].own.get("shade"), Some(&fit));
    follow_start(&mut a, "Alarm", "shade").unwrap();
    assert!(!a.states[1].own.contains_key("shade"));
    refused(&a, &["\"Alarm\"", "\"shade\""], |a| follow_start(a, "Alarm", "shade"));
}

#[test]
fn state_values_are_set_and_cleared() {
    let mut a = alarm_lamp();
    set_state_value(&mut a, "Alarm", P::GlowStrength("lamp".into()), Some(V::Int(30))).unwrap();
    set_state_value(&mut a, "Alarm", P::GlowColour("lamp".into()), Some(V::Colour(colour("#00ff00"))))
        .unwrap();
    // Replaced in place, the new one after.
    let alarm = &a.states[1].values;
    assert_eq!(
        alarm[0],
        StateValue { property: P::GlowColour("lamp".into()), value: V::Colour(colour("#00ff00")) }
    );
    assert_eq!(alarm[1].property, P::GlowStrength("lamp".into()));
    refused(&a, &["\"Alarm\"", "lamp.glow", "true is not a whole number"], |a| {
        set_state_value(a, "Alarm", P::GlowStrength("lamp".into()), Some(V::Bool(true)))
    });
    refused(&a, &["lamp.glow", "0 to 100"], |a| {
        set_state_value(a, "Alarm", P::GlowStrength("lamp".into()), Some(V::Int(101)))
    });
    refused(&a, &["ghost.x", "no layer"], |a| {
        set_state_value(a, "Alarm", P::OffsetX("ghost".into()), Some(V::Int(1)))
    });
    set_state_value(&mut a, "Alarm", P::GlowStrength("lamp".into()), None).unwrap();
    assert_eq!(a.states[1].values.len(), 1);
    refused(&a, &["\"Alarm\"", "sets no lamp.glow"], |a| {
        set_state_value(a, "Alarm", P::GlowStrength("lamp".into()), None)
    });
}

#[test]
fn forget_layer_drops_every_reference() {
    let mut a = alarm_lamp();
    a.exposed.push(expose("shade_x", P::OffsetX("shade".into()), Some(V::Int(0))));
    a.exposed[1].min = Some(0);
    a.exposed[1].max = Some(4);
    a.states[2].values.push(StateValue { property: P::LayerVisible("shade".into()), value: V::Bool(false) });
    let before = a.clone();
    let f = forget_layer(&mut a, "lamp");
    assert_eq!(f.own.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["Alarm", "Broken"]);
    assert_eq!(f.values.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["Alarm", "Broken"]);
    assert_eq!(f.exposed.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["glow"]);
    assert!(
        a.states
            .iter()
            .all(|s| !s.own.contains_key("lamp")
                && s.values.iter().all(|v| v.property.layer() != Some("lamp")))
    );
    // The shade's references stay.
    assert!(a.states[2].own.contains_key("shade"));
    assert_eq!(a.exposed[0].name, "shade_x");
    assert_eq!(a.states[2].values.len(), 1);
    // Nothing else changed.
    assert_eq!((&a.pieces, &a.sounds, &a.health), (&before.pieces, &before.sounds, &before.health));
    assert!(forget_layer(&mut a, "lamp").is_empty());
}

#[test]
fn expose_offers_only_what_offers_lists() {
    let mut a = lamp();
    a.pieces.push(layer("mud", ElementMaterial::Dirt, (0, 2), (4, 1), RED, None));
    refused(&a, &["mud.visible", "dirt"], |a| {
        olr_edit::asset::expose(a, expose_entry("hidden", P::LayerVisible("mud".into())))
    });
    // The light's reach on an unlimited group.
    let mut u = a.clone();
    u.groups[0].reach = ReachSource::Unlimited;
    refused(&u, &["lamp.reach", "not Pixels"], |a| {
        olr_edit::asset::expose(a, expose_entry("reach", P::LightReach("lamp".into())))
    });
    // The ranges filled in.
    olr_edit::asset::expose(&mut a, expose_entry("glow", P::GlowStrength("lamp".into()))).unwrap();
    olr_edit::asset::expose(&mut a, expose_entry("reach", P::LightReach("lamp".into()))).unwrap();
    assert_eq!((a.exposed[0].min, a.exposed[0].max), (Some(0), Some(100)));
    assert_eq!((a.exposed[1].min, a.exposed[1].max), (Some(1), Some(4096)));
    assert_eq!(a.exposed[0].default, None, "none: the layer's own");
    // Offsets need their limits.
    refused(&a, &["shade.x", "min and a max"], |a| {
        olr_edit::asset::expose(a, expose_entry("shade_x", P::OffsetX("shade".into())))
    });
    let mut x = expose_entry("shade_x", P::OffsetX("shade".into()));
    (x.min, x.max) = (Some(-2), Some(2));
    olr_edit::asset::expose(&mut a, x).unwrap();
    // One per glow group: the button shares the lamp's group.
    a.pieces.push(layer("button", ElementMaterial::Rock, (1, 1), (1, 1), RED, Some("light")));
    refused(&a, &["button.reach", "\"reach\"", "lamp.reach"], |a| {
        olr_edit::asset::expose(a, expose_entry("button_reach", P::LightReach("button".into())))
    });
    refused(&a, &["lamp.glow", "exposed already as \"glow\""], |a| {
        olr_edit::asset::expose(a, expose_entry("glow_2", P::GlowStrength("lamp".into())))
    });
    refused(&a, &["\"glow\" is exposed already"], |a| {
        olr_edit::asset::expose(a, expose_entry("glow", P::Visible))
    });
    // An Own group's colour needs a default.
    let mut o = a.clone();
    o.groups[0].colour = GlowColourSource::Own;
    refused(&o, &["lamp.glow-colour", "default colour"], |a| {
        olr_edit::asset::expose(a, expose_entry("tint", P::GlowColour("lamp".into())))
    });
    let with_default = ExposedSource {
        default: Some(V::Colour(colour("#ff8000"))),
        ..expose_entry("tint", P::GlowColour("lamp".into()))
    };
    olr_edit::asset::expose(&mut o, with_default).unwrap();
    // An exposed health starts at the points.
    a.health = Some(olr_format::assets::HealthSource { points: 30, at_zero: "Normal".into() });
    olr_edit::asset::expose(&mut a, expose_entry("hp", P::Health)).unwrap();
    let hp = a.exposed.last().unwrap();
    assert_eq!((hp.default.clone(), hp.min, hp.max), (Some(V::Int(30)), Some(0), Some(30)));
    // Renamed and unexposed.
    rename_exposed(&mut a, "hp", "health_points").unwrap();
    refused(&a, &["\"glow\" is exposed already"], |a| rename_exposed(a, "reach", "glow"));
    unexpose(&mut a, "reach").unwrap();
    assert!(a.exposed.iter().all(|x| x.name != "reach"));
    refused(&a, &["\"reach\""], |a| unexpose(a, "reach"));
    through_folder(&a);
}

fn expose_entry(name: &str, property: olr_format::assets::PropertySource) -> ExposedSource {
    expose(name, property, None)
}

#[test]
fn sounds_are_checked_and_kept_while_named() {
    let mut a = lamp();
    refused(&a, &["\"bad.wav\"", "WAV or Ogg"], |a| add_sound(a, "bad.wav", b"not a sound".to_vec()));
    refused(&a, &["\"long.wav\"", "s long"], |a| add_sound(a, "long.wav", wav(1, 1000)));
    refused(&a, &["\"Tick.wav\""], |a| add_sound(a, "Tick.wav", wav(8000, 10)));
    add_sound(&mut a, "tick.wav", wav(8000, 10)).unwrap();
    refused(&a, &["\"tick.wav\"", "already"], |a| add_sound(a, "tick.wav", wav(8000, 10)));
    a.states.push(State::new("Off"));
    set_state_sounds(&mut a, "Off", None, Some(Some("tick.wav".into()))).unwrap();
    refused(&a, &["\"Off\"", "\"bell.wav\""], |a| {
        set_state_sounds(a, "Off", Some(Some("bell.wav".into())), None)
    });
    refused(&a, &["\"tick.wav\"", "\"Off\"", "enter"], |a| remove_sound(a, "tick.wav"));
    let b = through_folder(&a);
    assert_eq!(b.states[1].enter_sound.as_deref(), Some("tick.wav"));
    set_state_sounds(&mut a, "Off", None, Some(None)).unwrap();
    remove_sound(&mut a, "tick.wav").unwrap();
    assert!(a.sounds.is_empty());
    refused(&a, &["no sound \"tick.wav\""], |a| remove_sound(a, "tick.wav"));
    // The script: bounded, not parsed.
    set_script(&mut a, Some("this is not rhai {".into())).unwrap();
    refused(&a, &["script", "bytes"], |a| set_script(a, Some(" ".repeat(64 << 10 | 1))));
    set_script(&mut a, None).unwrap();
}

#[test]
fn health_needs_its_state_and_unexposing_first() {
    use olr_format::assets::HealthSource;
    let mut a = lamp();
    refused(&a, &["no state \"Gone\""], |a| {
        set_health(a, Some(HealthSource { points: 10, at_zero: "Gone".into() }))
    });
    refused(&a, &["1 to"], |a| set_health(a, Some(HealthSource { points: 0, at_zero: "Normal".into() })));
    set_health(&mut a, Some(HealthSource { points: 40, at_zero: "Normal".into() })).unwrap();
    olr_edit::asset::expose(&mut a, expose_entry("hp", P::Health)).unwrap();
    refused(&a, &["\"hp\"", "unexpose health first"], |a| set_health(a, None));
    // New points: the exposed entry follows.
    set_health(&mut a, Some(HealthSource { points: 20, at_zero: "Normal".into() })).unwrap();
    assert_eq!((a.exposed[0].default.clone(), a.exposed[0].max), (Some(V::Int(20)), Some(20)));
    unexpose(&mut a, "hp").unwrap();
    set_health(&mut a, None).unwrap();
    assert_eq!(a.health, None);
}

/// The sum of a picture's channels.
fn brightness(png: &[u8]) -> u64 {
    let img = olr_format::image_io::decode_rgba_within(png, olr_format::image_io::IMAGES).unwrap();
    img.as_raw().iter().map(|v| u64::from(*v)).sum()
}

#[test]
fn the_thumbnail_shows_the_starting_state() {
    let mut a = lamp();
    let mut off = state("Off", vec![(P::GlowStrength("lamp".into()), V::Int(0))]);
    off.own.insert("lamp".into(), own(&a.pieces[0], Rgba::rgb(5, 5, 5), 0));
    a.states.push(off);
    let lit = brightness(encode(&a).unwrap().thumb.as_ref().unwrap());
    set_start(&mut a, "Off").unwrap();
    let dark = brightness(encode(&a).unwrap().thumb.as_ref().unwrap());
    assert!(dark < lit, "{dark} < {lit}");
    // Back again: the same thumbnail.
    set_start(&mut a, "Normal").unwrap();
    assert_eq!(brightness(encode(&a).unwrap().thumb.as_ref().unwrap()), lit);
}
