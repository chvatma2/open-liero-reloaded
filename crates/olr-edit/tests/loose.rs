//! Loose dirt in the composition and the bake (the loose dirt design): a
//! dirt layer's loose material bakes into `loose.png`'s kinds, a mixed
//! layer's on its dirt only, a higher element covering it; the assets'
//! loose materials gather into the level's.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::{Asset, Piece, encode};
use olr_edit::compose::{Baker, Decoded, Rect, bake};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{
    AssetOrigin, Element, ElementKind, ElementMaterial, LevelAsset, LevelSource, LooseSource, PlacedCopy,
    PlacedSource,
};

const SAND: Rgba = Rgba::rgb(200, 170, 90);
const ROCK: Rgba = Rgba::rgb(100, 100, 104);

fn level(w: u32, h: u32, loose: Vec<LooseSource>) -> LevelSource {
    LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: 1,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Background; (w * h) as usize],
        front: RgbaImage::new(w, h),
        back: RgbaImage::new(w, h),
        thumb: None,
        look: None,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
        live: None,
        loose,
        loose_map: None,
    }
}

fn pack(w: u32, loose: Vec<LooseSource>) -> Pack {
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
    Pack::new(manifest, Source::Level(level(w, 1, loose)))
}

fn sand() -> LooseSource {
    LooseSource { repose: 30, ..LooseSource::new("sand", "Sand") }
}

fn of(p: &Pack) -> &LevelSource {
    let Source::Level(l) = &p.source else { panic!("a level") };
    l
}

fn baked(p: &Pack) -> LevelSource {
    let mut l = of(p).clone();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    l
}

fn rect(x: i32, w: i32) -> Shape {
    Shape::Rect { x, y: 0, w, h: 1 }
}

/// A dirt layer "Sand" of loose sand drawn over x `x..x + w` of a 6 × 1
/// level listing sand.
fn sand_layer(x: i32, w: i32) -> Pack {
    let mut p = pack(6, vec![sand()]);
    add_layer(&mut p, ElementMaterial::Dirt, "Sand").unwrap();
    draw(&mut p, "sand", &rect(x, w), &Paint::Colour(SAND), None).unwrap();
    p
}

#[test]
fn a_loose_dirt_element_bakes_its_kinds() {
    let mut p = sand_layer(1, 3);
    assert_eq!(baked(&p).loose_map, None, "no loose element, no map");
    set(&mut p, "sand", Property::Loose(Some("sand".into()))).unwrap();
    let full = baked(&p);
    assert_eq!(full.loose_map.as_deref(), Some(&[0, 1, 1, 1, 0, 0][..]));
    assert_eq!(full.material[1], Material::Dirt);
    // A region bake equals the full one.
    let mut part = full.clone();
    part.loose_map.as_mut().unwrap().iter_mut().for_each(|v| *v = 0);
    let baker = Baker::new(p.composition.as_ref().unwrap(), (6, 1), 1, &RgbaImage::new(1, 1)).unwrap();
    baker.bake_region(&mut part, Rect { x: 0, y: 0, w: 2, h: 1 });
    baker.bake_region(&mut part, Rect { x: 2, y: 0, w: 4, h: 1 });
    assert_eq!(part.loose_map, full.loose_map);
    // Naming no loose material of the level is refused, the pack unchanged.
    let e = set(&mut p, "sand", Property::Loose(Some("gravel".into()))).unwrap_err().to_string();
    assert!(e.contains("gravel"), "{e}");
    assert_eq!(baked(&p).loose_map, full.loose_map);
    // Back to plain dirt: no map.
    set(&mut p, "sand", Property::Loose(None)).unwrap();
    assert_eq!(baked(&p).loose_map, None);
}

#[test]
fn a_mixed_layer_bakes_loose_only_on_its_dirt() {
    let mut p = sand_layer(0, 6);
    draw(&mut p, "sand", &rect(2, 2), &Paint::Colour(ROCK), Some(ElementMaterial::Rock)).unwrap();
    set(&mut p, "sand", Property::Loose(Some("sand".into()))).unwrap();
    let l = baked(&p);
    assert_eq!(l.material[2], Material::Rock);
    assert_eq!(l.loose_map.as_deref(), Some(&[1, 1, 0, 0, 1, 1][..]));
}

#[test]
fn a_higher_rock_element_covers_loose() {
    let mut p = sand_layer(0, 6);
    set(&mut p, "sand", Property::Loose(Some("sand".into()))).unwrap();
    add_layer(&mut p, ElementMaterial::Rock, "Shelf").unwrap();
    draw(&mut p, "shelf", &rect(2, 2), &Paint::Colour(ROCK), None).unwrap();
    // So does a higher plain dirt layer: it gives the pixel's material.
    add_layer(&mut p, ElementMaterial::Dirt, "Dirt").unwrap();
    draw(&mut p, "dirt", &rect(5, 1), &Paint::Colour(ROCK), None).unwrap();
    assert_eq!(baked(&p).loose_map.as_deref(), Some(&[1, 1, 0, 0, 1, 0][..]));
}

#[test]
fn a_rock_or_background_layer_refuses_loose() {
    let mut p = pack(6, vec![sand()]);
    add_layer(&mut p, ElementMaterial::Rock, "Shelf").unwrap();
    let e = set(&mut p, "shelf", Property::Loose(Some("sand".into()))).unwrap_err().to_string();
    assert!(e.contains("loose"), "{e}");
}

/// A 1 × 1 asset of one loose dirt pixel of `material`.
fn heap(material: LooseSource) -> Asset {
    let mut art = RgbaImage::new(1, 1);
    art.set(0, 0, SAND);
    let piece = Piece {
        element: Element {
            id: "heap".into(),
            name: String::new(),
            visible: true,
            locked: false,
            folder: None,
            kind: ElementKind::Drawn {
                material: ElementMaterial::Dirt,
                at: (0, 0),
                size: Some((1, 1)),
                mixed: false,
                back: false,
            },
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
            clip: false,
            loose: Some(material.id.clone()),
        },
        decoded: Decoded::Drawn { art, cells: None, back: None },
        original: None,
        glow_mask: None,
        surface_map: None,
        back_surface_map: None,
    };
    let mut a = Asset::new("Heap", 1, (1, 1), vec![piece], vec![], vec![]);
    a.loose = vec![material];
    a
}

#[test]
fn assets_loose_materials_are_gathered() {
    let (one, other) = (heap(sand()), heap(LooseSource { repose: 60, ..sand() }));
    let mut src = PlacedSource::default();
    for (key, a) in [("a", &one), ("b", &other)] {
        src.file
            .assets
            .push(LevelAsset { key: key.into(), origin: AssetOrigin::Yours(format!("asset-{key}")) });
        src.assets.insert(key.into(), encode(a).unwrap());
    }
    src.file.copies = vec![
        PlacedCopy::new("heap-1", "a", (0, 0)),
        PlacedCopy::new("heap-2", "a", (1, 0)),
        PlacedCopy::new("heap-3", "b", (2, 0)),
    ];
    let mut l = level(4, 1, Vec::new());
    olr_edit::placed::bake(&mut l, &src, &RgbaImage::new(1, 1)).unwrap();
    let ids: Vec<&str> = l.loose.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["sand", "sand-2"], "identical ones shared, the other renamed");
    assert_eq!((l.loose[0].repose, l.loose[1].repose), (30, 60));
    assert_eq!(l.loose_map.as_deref(), Some(&[1, 1, 2, 0][..]));
    // The asset's list round-trips through its folder.
    assert_eq!(olr_edit::asset::decode(&encode(&other).unwrap()).unwrap().loose, other.loose);
}

#[test]
fn a_cut_asset_carries_the_loose_materials_it_names() {
    let gravel = LooseSource::new("gravel", "Gravel");
    let mut p = pack(6, vec![gravel, sand()]);
    add_layer(&mut p, ElementMaterial::Dirt, "Sand").unwrap();
    draw(&mut p, "sand", &rect(0, 6), &Paint::Colour(SAND), None).unwrap();
    set(&mut p, "sand", Property::Loose(Some("sand".into()))).unwrap();
    let level = olr_edit::asset::DecodedLevel::of_pack(&p).unwrap();
    let cut =
        olr_edit::asset::cut_rect(&level.level(), Rect { x: 1, y: 0, w: 3, h: 1 }, None, "Heap").unwrap();
    assert_eq!(cut.loose, [sand()], "only the one named");
    let named: Vec<_> = cut.pieces.iter().filter_map(|p| p.element.loose.as_deref()).collect();
    assert_eq!(named, ["sand"]);
    olr_format::assets::check_asset(&encode(&cut).unwrap()).unwrap();
}
