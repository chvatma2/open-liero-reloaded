//! Look operations (plan 15a Task 7): the look's settings, glow groups and
//! light objects, each validated as the format checks them.

use std::collections::BTreeMap;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::element::{Property, add_layer, set};
use olr_edit::look::*;
use olr_edit::pack::{Pack, Source};
use olr_format::Colour;
use olr_format::levels::{ElementGlow, ElementMaterial, GlowLayers, LevelObject, LevelSource};
use olr_format::look::{AnimationSource, GlowGroupSource, LightSource, LookSource, ReachSource};

fn group(id: &str) -> GlowGroupSource {
    GlowGroupSource {
        id: id.into(),
        colour: Default::default(),
        intensity: 100,
        reach: ReachSource::Pixels(32),
        animation: Default::default(),
        phase_ms: None,
    }
}

fn light(intensity: u16) -> LightSource {
    LightSource {
        colour: Colour(Rgba::rgb(255, 128, 0)),
        intensity,
        reach: ReachSource::Pixels(64),
        animation: AnimationSource::Flicker { speed: 8, depth: 25 },
        phase_ms: None,
    }
}

fn level() -> LevelSource {
    let (w, h, s) = (4, 2, 1);
    LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: vec![LevelObject::spawn((1, 1))],
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
        look: None,
        glow: None,
    }
}

fn pack(l: LevelSource) -> Pack {
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
    Pack { manifest, source: Source::Level(l), composition: None, other: BTreeMap::new() }
}

fn level_of(p: &Pack) -> &LevelSource {
    let Source::Level(l) = &p.source else { panic!() };
    l
}

#[test]
fn look_edits_validate() {
    let mut l = level();
    set_look(&mut l, Some(LookSource { max_light: 150, ..Default::default() })).unwrap();
    assert_eq!(l.look.as_ref().unwrap().max_light, 150);
    let before = l.clone();
    let e = set_look(&mut l, Some(LookSource { max_light: 300, ..Default::default() })).unwrap_err();
    assert!(e.to_string().contains("max_light"), "{e}");
    assert_eq!(l, before, "a refused edit changes nothing");
    set_look(&mut l, None).unwrap();
    assert_eq!(l.look, None);
}

#[test]
fn group_ids_are_unique() {
    let mut l = level();
    assert_eq!(add_group(&mut l, group("lava")).unwrap(), 0);
    assert_eq!(add_group(&mut l, group("sky")).unwrap(), 1);
    let e = add_group(&mut l, group("lava")).unwrap_err().to_string();
    assert!(e.contains("lava"), "{e}");
    let mut g = group("lava");
    g.intensity = 300;
    set_group(&mut l, "lava", g).unwrap();
    assert_eq!(l.look.as_ref().unwrap().glow_groups[0].intensity, 300);
    assert!(set_group(&mut l, "nowhere", group("nowhere")).is_err());
    assert!(add_group(&mut l, GlowGroupSource { intensity: 2000, ..group("hot") }).is_err());
}

#[test]
fn removing_a_group_renumbers_and_unglows() {
    let mut l = level();
    add_group(&mut l, group("a")).unwrap();
    add_group(&mut l, group("b")).unwrap();
    let mut g = GlowLayers::empty((4, 2), (4, 2));
    // World pixels 0 and 1 glow in group a (0), 2 and 3 in b (1).
    g.front = vec![10, 11, 12, 13, 0, 0, 0, 0];
    g.groups = vec![0, 0, 1, 1, 0, 0, 0, 0];
    g.back = vec![20, 0, 0, 23, 0, 0, 0, 0];
    g.groups_back = vec![0, 0, 0, 1, 0, 0, 0, 0];
    l.glow = Some(g);
    let mut p = pack(l);
    add_layer(&mut p, ElementMaterial::Dirt, "Lava").unwrap();
    set(&mut p, "lava", Property::Glow(Some(ElementGlow { strength: 50, group: "a".into(), mask: false })))
        .unwrap();
    let unglowed = remove_group(&mut p, "a").unwrap();
    assert_eq!(unglowed, ["lava"]);
    let l = level_of(&p);
    assert_eq!(l.look.as_ref().unwrap().glow_groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["b"]);
    let g = l.glow.as_ref().unwrap();
    assert_eq!(g.front, [0, 0, 12, 13, 0, 0, 0, 0]);
    assert_eq!(g.groups, [0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(g.back, [0, 0, 0, 23, 0, 0, 0, 0]);
    assert_eq!(g.groups_back, [0, 0, 0, 0, 0, 0, 0, 0]);
    let e = &p.composition.as_ref().unwrap().composition.elements;
    assert!(e.iter().all(|e| e.glow.is_none()));
    assert!(remove_group(&mut p, "a").is_err());
}

#[test]
fn renaming_a_group_follows_its_elements() {
    let mut l = level();
    add_group(&mut l, group("a")).unwrap();
    let mut p = pack(l);
    add_layer(&mut p, ElementMaterial::Dirt, "Lava").unwrap();
    set(&mut p, "lava", Property::Glow(Some(ElementGlow { strength: 50, group: "a".into(), mask: false })))
        .unwrap();
    rename_group(&mut p, "a", "magma").unwrap();
    assert_eq!(level_of(&p).look.as_ref().unwrap().glow_groups[0].id, "magma");
    let e = &p.composition.as_ref().unwrap().composition.elements;
    assert_eq!(e.iter().find(|e| e.id == "lava").unwrap().glow.as_ref().unwrap().group, "magma");
    assert!(rename_group(&mut p, "magma", "Bad Id").is_err());
}

#[test]
fn lights_round_trip() {
    let mut l = level();
    let i = add_light(&mut l, (2, 1), light(120), 3).unwrap();
    assert_eq!(i, 1);
    assert_eq!(l.objects[1].as_light().unwrap(), (3, light(120)));
    set_light(&mut l, 1, (3, 0), light(50), 2).unwrap();
    assert_eq!((l.objects[1].at(), l.objects[1].as_light().unwrap()), ((3, 0), (2, light(50))));
    assert!(set_light(&mut l, 0, (0, 0), light(50), 2).is_err(), "object 0 is a spawn");
    assert!(set_light(&mut l, 7, (0, 0), light(50), 2).is_err());
    assert!(add_light(&mut l, (0, 0), light(5000), 2).is_err());
    assert!(add_light(&mut l, (0, 0), light(10), 17).is_err());
    assert_eq!(l.objects.len(), 2);
}
