//! Element operations on a composed level (plan 14a Task 7).

use std::collections::BTreeMap;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::image_io;
use olr_format::levels::{ElementKind, ElementMaterial, LevelSource, check_composition, layer_file};

const RED: Rgba = Rgba::rgb(255, 0, 0);

/// A plain `w` × `h` level pack at art scale `s` (all dirt), in memory.
fn pack(w: u32, h: u32, s: u32) -> Pack {
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
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
    };
    Pack { manifest, source: Source::Level(level), composition: None, other: BTreeMap::new() }
}

fn ids(p: &Pack) -> Vec<String> {
    p.composition.as_ref().unwrap().composition.elements.iter().map(|e| e.id.clone()).collect()
}

fn layer(p: &Pack, id: &str) -> RgbaImage {
    image_io::decode_rgba(&p.composition.as_ref().unwrap().files[&layer_file(id)]).unwrap()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    image_io::encode_rgba(&RgbaImage::new(w, h))
}

#[test]
fn a_plain_level_gains_a_raster_base_first() {
    let mut p = pack(8, 6, 1);
    assert!(p.composition.is_none());
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground");
    assert_eq!(ids(&p), ["base", "ground"]);
    let src = p.composition.as_ref().unwrap();
    check_composition(src, (8, 6), 1).unwrap();
}

#[test]
fn ids_are_unique_slugs() {
    let mut p = pack(8, 6, 1);
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground");
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground-2");
    let a = add_image(&mut p, "My Photo.PNG", png(4, 4), (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
    let b = add_image(&mut p, "My Photo.PNG", png(4, 4), (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("my-photo", "my-photo-2"));
    let sources: Vec<String> = p
        .composition
        .as_ref()
        .unwrap()
        .composition
        .elements
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Image { source, .. } => Some(source.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sources, ["My Photo.PNG", "My Photo-2.PNG"]);
}

/// A name no file system holds everywhere (a Windows device) is skipped
/// like a taken one (batch A review).
#[test]
fn ids_skip_device_names() {
    let mut p = pack(8, 6, 1);
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Aux").unwrap(), "aux-2");
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "").unwrap(), "layer");
}

#[test]
fn set_validates_and_respects_locks() {
    let mut p = pack(8, 6, 1);
    let id = add_image(&mut p, "a.png", png(4, 4), (0, 0), (4, 4), ElementMaterial::Dirt, "A").unwrap();
    assert!(
        matches!(set(&mut p, &id, Property::PixelSize(0)), Err(ElementError::BadValue(m)) if m.contains("pixel_size"))
    );
    assert!(matches!(set(&mut p, &id, Property::Colours(Some(1))), Err(ElementError::BadValue(_))));
    set(&mut p, &id, Property::Locked(true)).unwrap();
    assert_eq!(set(&mut p, &id, Property::Name("x".into())), Err(ElementError::Locked(id.clone())));
    set(&mut p, &id, Property::Locked(false)).unwrap();
    set(&mut p, &id, Property::Name("x".into())).unwrap();
    assert_eq!(set(&mut p, "nope", Property::Visible(false)), Err(ElementError::NoElement("nope".into())));
}

#[test]
fn raise_lower_remove_reorder_and_drop_files() {
    let mut p = pack(8, 6, 1);
    for name in ["a", "b", "c"] {
        add_layer(&mut p, ElementMaterial::Dirt, name).unwrap();
    }
    raise(&mut p, "a").unwrap();
    assert_eq!(ids(&p), ["base", "b", "a", "c"]);
    lower(&mut p, "c").unwrap();
    assert_eq!(ids(&p), ["base", "b", "c", "a"]);
    raise(&mut p, "a").unwrap(); // top: no-op
    lower(&mut p, "base").unwrap(); // bottom: no-op
    assert_eq!(ids(&p), ["base", "b", "c", "a"]);
    remove(&mut p, "b").unwrap();
    assert_eq!(ids(&p), ["base", "c", "a"]);
    assert!(!p.composition.as_ref().unwrap().files.contains_key(&layer_file("b")));
}

#[test]
fn draw_and_erase_paint_whole_art_blocks() {
    let mut p = pack(8, 6, 2);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    draw(&mut p, &id, &Shape::Rect { x: 1, y: 1, w: 2, h: 2 }, &Paint::Colour(RED)).unwrap();
    let art = layer(&p, &id);
    for ay in 0..12 {
        for ax in 0..16 {
            let inside = (2..6).contains(&ax) && (2..6).contains(&ay);
            assert_eq!(art.get(ax, ay).a, if inside { 255 } else { 0 }, "({ax}, {ay})");
            if inside {
                assert_eq!(art.get(ax, ay), RED);
            }
        }
    }
    erase(&mut p, &id, &Shape::Circle { cx: 2, cy: 2, r: 0 }).unwrap();
    let art = layer(&p, &id);
    for ay in 4..6 {
        for ax in 4..6 {
            assert_eq!(art.get(ax, ay).a, 0);
        }
    }
    assert_eq!(art.get(2, 2).a, 255);
    assert!(matches!(
        draw(&mut p, "base", &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED)),
        Err(ElementError::WrongKind { .. })
    ));
}

#[test]
fn add_image_refuses_a_hostile_file_and_writes_nothing() {
    let mut p = pack(8, 6, 1);
    add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    let before = p.composition.clone();
    for (name, bytes) in
        [("huge.png", image_io::declared_png(100_000, 100_000, false)), ("junk.png", b"junk".to_vec())]
    {
        let e = add_image(&mut p, name, bytes, (0, 0), (4, 4), ElementMaterial::Dirt, "").unwrap_err();
        assert!(matches!(&e, ElementError::BadValue(m) if m.contains(name)), "{e}");
        assert_eq!(p.composition, before);
    }
}

#[test]
fn resize_moves_boxes_and_relays_layers() {
    let mut p = pack(8, 6, 1);
    let img = add_image(&mut p, "a.png", png(2, 2), (3, 2), (2, 2), ElementMaterial::Rock, "").unwrap();
    let l = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    draw(&mut p, &l, &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED)).unwrap();
    resize(&mut p, (13, 9), (5, 3), Material::Rock, &Paint::Colour(RED)).unwrap();
    let src = p.composition.as_ref().unwrap();
    let at = src.composition.elements.iter().find(|e| e.id == img).map(|e| match &e.kind {
        ElementKind::Image { at, .. } => *at,
        _ => unreachable!(),
    });
    assert_eq!(at, Some((8, 5)));
    let art = layer(&p, &l);
    assert_eq!((art.width(), art.height()), (13, 9));
    assert_eq!(art.get(5, 3), RED);
    assert_eq!(art.get(0, 0).a, 0, "the new area is uncovered");
    let material = image_io::decode_indexed(&src.files["editor/layers/base.material.png"]).unwrap();
    assert_eq!((material.0, material.1), (13, 9));
    assert_eq!(material.2[0], Material::Rock as u8, "the raster's new area is the given material");
    check_composition(src, (13, 9), 1).unwrap();
}
