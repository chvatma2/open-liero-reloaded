//! Issue #38: the element operations giving a level's elements surface
//! materials (`olr-cli level element set --surface`, `surface`): a
//! surface by id, a painted map set whole, and a map painted by rules of
//! collision kind and colour, reproducibly; every bad input refused with
//! the pack unchanged.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::compose::bake;
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{ElementMaterial, LevelSource, surface_file};
use olr_format::look::LookSource;
use olr_format::materials::MaterialSource;

const GREY: Rgba = Rgba::rgb(128, 128, 128);
const BROWN: Rgba = Rgba::rgb(120, 70, 20);
const RUST: Rgba = Rgba::rgb(150, 60, 30);

fn material(id: &str) -> MaterialSource {
    MaterialSource { id: id.into(), name: id.into(), notes: String::new(), heat: None, light_soak: None }
}

/// A 10 × 6 level at art scale `s` whose look has stone (1), wood (2) and
/// metal (3), with a mixed drawn layer `layer`: grey rock on the left half,
/// brown dirt on the right half's top rows, rust dirt below them.
fn level(s: u32) -> Pack {
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
    let (w, h) = (10, 6);
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
        look: Some(LookSource {
            materials: vec![material("stone"), material("wood"), material("metal")],
            ..Default::default()
        }),
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    };
    let mut p = Pack::new(manifest, Source::Level(level));
    add_layer(&mut p, ElementMaterial::Dirt, "Layer").unwrap();
    remove(&mut p, "base").unwrap();
    let rect = |x, y, w, h| Shape::Rect { x, y, w, h };
    draw(&mut p, "layer", &rect(0, 0, 5, 6), &Paint::Colour(GREY), Some(ElementMaterial::Rock)).unwrap();
    draw(&mut p, "layer", &rect(5, 0, 5, 3), &Paint::Colour(BROWN), None).unwrap();
    draw(&mut p, "layer", &rect(5, 3, 5, 3), &Paint::Colour(RUST), None).unwrap();
    p
}

fn map(p: &Pack, id: &str) -> Option<Vec<u8>> {
    let src = p.composition.as_ref()?;
    let e = src.composition.elements.iter().find(|e| e.id == id)?;
    let bytes = src.files.get(&surface_file(id)).filter(|_| e.surface_map)?;
    Some(olr_format::image_io::decode_grey_within(bytes, olr_format::image_io::IMAGES).unwrap().2)
}

fn rule(
    kind: Option<ElementMaterial>,
    colour: Option<Rgba>,
    tolerance: u8,
    material: Option<&str>,
) -> SurfaceRule {
    SurfaceRule { kind, colour, tolerance, material: material.map(str::to_string) }
}

#[test]
fn surfaces_name_the_looks_materials() {
    let mut p = level(1);
    set(&mut p, "layer", Property::Surface(Some("stone".into()))).unwrap();
    let before = p.clone();
    let e = set(&mut p, "layer", Property::Surface(Some("lava".into()))).unwrap_err().to_string();
    assert!(e.contains("surface") && e.contains("lava"), "{e}");
    // No back art: no back surface.
    let e = set(&mut p, "layer", Property::BackSurface(Some("wood".into()))).unwrap_err().to_string();
    assert!(e.contains("back art"), "{e}");
    assert_eq!(p, before, "refusals change nothing");
    set(&mut p, "layer", Property::Surface(None)).unwrap();
    let src = p.composition.as_ref().unwrap();
    assert_eq!(src.composition.elements[0].surface, None);
}

#[test]
fn a_painted_map_is_set_whole_and_checked() {
    let mut p = level(2);
    let mut values = vec![0u8; 60];
    values[0] = 3;
    set_surface_map(&mut p, "layer", false, Some(&values)).unwrap();
    assert_eq!(map(&p, "layer"), Some(values.clone()));
    let mut l = match &p.source {
        Source::Level(l) => l.clone(),
        _ => panic!(),
    };
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    assert_eq!(l.surface.as_ref().unwrap().front[0], 3);
    let before = p.clone();
    for bad in [vec![0u8; 59], vec![4u8; 60]] {
        assert!(set_surface_map(&mut p, "layer", false, Some(&bad)).is_err());
    }
    assert!(set_surface_map(&mut p, "layer", true, Some(&values)).is_err(), "no back art");
    assert!(set_surface_map(&mut p, "nothing", false, Some(&values)).is_err());
    set(&mut p, "layer", Property::Locked(true)).unwrap();
    assert!(set_surface_map(&mut p, "layer", false, None).is_err(), "locked");
    set(&mut p, "layer", Property::Locked(false)).unwrap();
    assert_eq!(p, before);
    // All 0, or none: no map.
    set_surface_map(&mut p, "layer", false, Some(&[0; 60])).unwrap();
    assert_eq!(map(&p, "layer"), None);
    assert!(!p.composition.as_ref().unwrap().files.contains_key(&surface_file("layer")));
}

#[test]
fn rules_paint_by_kind_and_colour_reproducibly() {
    for s in [1, 3] {
        let mut p = level(s);
        let rules = [
            rule(Some(ElementMaterial::Rock), None, 0, Some("stone")),
            rule(Some(ElementMaterial::Dirt), Some(Rgba::rgb(150, 62, 30)), 2, Some("metal")),
            rule(None, Some(BROWN), 0, Some("wood")),
        ];
        let n = surface_by_colour(&mut p, "layer", false, &rules).unwrap();
        assert_eq!(n, 60, "every pixel matched, scale {s}");
        let m = map(&p, "layer").unwrap();
        assert_eq!((m[0], m[5], m[3 * 10 + 5], m[5 * 10 + 9]), (1, 2, 3, 3), "scale {s}");
        // Again: the same map; the pack's files byte for byte.
        let once = p.clone();
        surface_by_colour(&mut p, "layer", false, &rules).unwrap();
        assert_eq!(p, once);
        // A rule matching nothing keeps the map; `none` paints the default.
        assert_eq!(
            surface_by_colour(
                &mut p,
                "layer",
                false,
                &[rule(None, Some(Rgba::rgb(0, 0, 255)), 10, Some("wood"))]
            )
            .unwrap(),
            0
        );
        assert_eq!(p, once);
        surface_by_colour(&mut p, "layer", false, &[rule(Some(ElementMaterial::Rock), None, 0, None)])
            .unwrap();
        let m = map(&p, "layer").unwrap();
        assert_eq!((m[0], m[5]), (0, 2));
    }
}

#[test]
fn bad_rules_are_refused() {
    let mut p = level(1);
    let before = p.clone();
    let e = surface_by_colour(&mut p, "layer", false, &[rule(None, None, 0, Some("lava"))])
        .unwrap_err()
        .to_string();
    assert!(e.contains("lava"), "{e}");
    assert!(surface_by_colour(&mut p, "layer", true, &[rule(None, None, 0, Some("wood"))]).is_err());
    assert!(surface_by_colour(&mut p, "nothing", false, &[]).is_err());
    assert_eq!(p, before);
    // No rule: nothing matched, no map.
    assert_eq!(surface_by_colour(&mut p, "layer", false, &[]).unwrap(), 0);
    assert_eq!(map(&p, "layer"), None);
}

fn materials(p: &Pack) -> Vec<String> {
    match &p.source {
        Source::Level(l) => l.look.iter().flat_map(|l| l.materials.iter().map(|m| m.id.clone())).collect(),
        _ => panic!("a level"),
    }
}

/// Issue #43: a material is removed only when nothing uses it (the users
/// named: an element's surface, or a painted value); the painted maps
/// after it are renumbered, so each pixel keeps its material.
#[test]
fn a_material_in_use_is_not_removed() {
    use olr_edit::look::{material_users, remove_material};
    let mut p = level(1);
    set(&mut p, "layer", Property::Surface(Some("stone".into()))).unwrap();
    let mut values = vec![0u8; 60];
    values[7] = 3;
    set_surface_map(&mut p, "layer", false, Some(&values)).unwrap();
    assert_eq!(material_users(&p, "stone").unwrap(), ["Layer"]);
    assert_eq!(material_users(&p, "metal").unwrap(), ["Layer"], "a painted value uses it");
    assert!(material_users(&p, "wood").unwrap().is_empty());
    assert!(material_users(&p, "lava").is_err());
    let before = p.clone();
    for used in ["stone", "metal"] {
        let e = remove_material(&mut p, used).unwrap_err().to_string();
        assert!(e.contains(used) && e.contains("Layer"), "{e}");
    }
    assert_eq!(p, before, "refusals change nothing");
    // Wood goes; metal moves down to 2, and the map with it.
    remove_material(&mut p, "wood").unwrap();
    assert_eq!(materials(&p), ["stone", "metal"]);
    let m = map(&p, "layer").unwrap();
    assert_eq!((m[7], m.iter().filter(|v| **v != 0).count()), (2, 1));
    assert_eq!(material_users(&p, "metal").unwrap(), ["Layer"]);
}

#[test]
fn renumbering_drops_the_removed_value() {
    let mut v = vec![0, 1, 2, 3, 4];
    olr_edit::look::renumber_removed(&mut v, 2);
    assert_eq!(v, [0, 1, 0, 2, 3]);
}
