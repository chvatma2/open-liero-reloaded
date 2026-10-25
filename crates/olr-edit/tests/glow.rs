//! Glow in the composition and the bake (look system §Composition, plan 15a
//! Task 5): the channel rules, region bakes, painted masks, groups.

use std::collections::BTreeMap;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::compose::{Baker, Rect, bake};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{ElementGlow, ElementMaterial, LevelSource, glow_file};
use olr_format::look::{GlowGroupSource, LookSource, ReachSource};

const RED: Rgba = Rgba::rgb(255, 0, 0);

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
    let group = |id: &str| GlowGroupSource {
        id: id.into(),
        colour: Default::default(),
        intensity: 100,
        reach: ReachSource::Pixels(32),
        animation: Default::default(),
        phase_ms: None,
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
        look: Some(LookSource { glow_groups: vec![group("sky"), group("lava")], ..Default::default() }),
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

fn glow(strength: u8, group: &str) -> Property {
    Property::Glow(Some(ElementGlow { strength, group: group.into(), mask: false, beam: None }))
}

/// 6 × 1: a glowing sky (background) everywhere, a plain dirt ground at
/// x 2-3, a half-glowing lava (dirt) at x 4-5.
fn stack(s: u32) -> Pack {
    let mut p = pack(6, 1, s);
    let paint = |p: &mut Pack, id: &str, x: i32, w: i32| {
        draw(p, id, &Shape::Rect { x, y: 0, w, h: 1 }, &Paint::Colour(RED), None).unwrap();
    };
    add_layer(&mut p, ElementMaterial::Background, "Sky").unwrap();
    paint(&mut p, "sky", 0, 6);
    add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap();
    paint(&mut p, "ground", 2, 2);
    add_layer(&mut p, ElementMaterial::Dirt, "Lava").unwrap();
    paint(&mut p, "lava", 4, 2);
    set(&mut p, "sky", glow(100, "sky")).unwrap();
    set(&mut p, "lava", glow(50, "lava")).unwrap();
    p
}

fn baked(p: &Pack) -> LevelSource {
    let mut l = level(p).clone();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    l
}

#[test]
fn glow_follows_the_channel_rules() {
    for s in [1, 2] {
        let l = baked(&stack(s));
        assert_eq!(
            l.material[..],
            [
                Material::Background,
                Material::Background,
                Material::Dirt,
                Material::Dirt,
                Material::Dirt,
                Material::Dirt
            ]
        );
        let g = l.glow.as_ref().expect("glow");
        let row: Vec<u8> = (0..6).map(|x| g.front[(x * s) as usize]).collect();
        assert_eq!(row, [255, 255, 0, 0, 128, 128], "front, scale {s}");
        assert!(g.back.iter().all(|v| *v == 255), "the sky is the backdrop everywhere");
        // A group map holds 1 + the group's index; 0 is no group.
        assert_eq!(g.groups, [1, 1, 0, 0, 2, 2]);
        assert!(g.groups_back.iter().all(|v| *v == 1));
    }
}

#[test]
fn glow_region_bake_equals_full_bake() {
    let p = stack(2);
    let full = baked(&p);
    let src = p.composition.as_ref().unwrap();
    for r in
        [Rect { x: 0, y: 0, w: 3, h: 1 }, Rect { x: 3, y: 0, w: 3, h: 1 }, Rect { x: 5, y: 0, w: 9, h: 9 }]
    {
        let mut part = full.clone();
        // Spoil the region, then rebake it.
        let g = part.glow.as_mut().unwrap();
        g.front.iter_mut().for_each(|v| *v = 7);
        g.groups.iter_mut().for_each(|v| *v = 9);
        let rest = baked(&p);
        let baker = Baker::new(src, (6, 1), 2, &RgbaImage::new(1, 1)).unwrap();
        baker.bake_region(&mut part, Rect { x: 0, y: 0, w: 6, h: 1 });
        baker.bake_region(&mut part, r);
        assert_eq!(part.glow, rest.glow, "{r:?}");
    }
}

#[test]
fn no_glow_writes_no_look_files() {
    let mut p = stack(1);
    set(&mut p, "sky", Property::Glow(None)).unwrap();
    set(&mut p, "lava", Property::Glow(None)).unwrap();
    let l = baked(&p);
    assert_eq!(l.glow, None);
    let mut fs = olr_vfs::MemFs::new();
    olr_format::levels::write_level_files(&mut fs, &l).unwrap();
    assert!(fs.files().all(|(f, _)| !f.as_str().starts_with("look/")));
}

#[test]
fn a_painted_mask_scales_the_strength() {
    let mut p = stack(1);
    paint_glow(&mut p, "lava", &Shape::Rect { x: 5, y: 0, w: 1, h: 1 }, 0).unwrap();
    let e = &p.composition.as_ref().unwrap().composition.elements;
    assert!(e.iter().find(|e| e.id == "lava").unwrap().glow.as_ref().unwrap().mask);
    assert!(p.composition.as_ref().unwrap().files.contains_key(&glow_file("lava")));
    let l = baked(&p);
    let g = l.glow.as_ref().unwrap();
    assert_eq!((g.front[4], g.front[5]), (128, 0));
    // An element without glow has no mask to paint.
    assert!(paint_glow(&mut p, "ground", &Shape::Rect { x: 2, y: 0, w: 1, h: 1 }, 50).is_err());
}

#[test]
fn an_unknown_group_is_an_error() {
    let mut p = stack(1);
    let e = set(&mut p, "ground", glow(10, "nowhere")).unwrap_err().to_string();
    assert!(e.contains("nowhere"), "{e}");
    let e = set(&mut p, "ground", glow(101, "sky")).unwrap_err().to_string();
    assert!(e.contains("strength"), "{e}");
    // A raster element cannot glow.
    let mut q = pack(4, 1, 1);
    add_layer(&mut q, ElementMaterial::Dirt, "X").unwrap();
    assert!(set(&mut q, "base", glow(10, "sky")).is_err());
}

#[test]
fn a_duplicate_keeps_its_glow_and_mask() {
    let mut p = stack(1);
    paint_glow(&mut p, "lava", &Shape::Rect { x: 5, y: 0, w: 1, h: 1 }, 0).unwrap();
    let id = duplicate(&mut p, "lava").unwrap();
    let src = p.composition.as_ref().unwrap();
    assert!(src.files.contains_key(&glow_file(&id)));
    assert_eq!(
        src.composition.elements.iter().find(|e| e.id == id).unwrap().glow.as_ref().unwrap().strength,
        50
    );
}

#[test]
fn pack_check_refuses_a_glow_naming_no_group() {
    let mut p = stack(1);
    let Source::Level(l) = &mut p.source else { panic!() };
    // The look loses "lava" (renamed by hand): the composition still names it.
    l.look.as_mut().unwrap().glow_groups[1].id = "magma".into();
    let files = olr_edit::pack::files(&p).unwrap();
    let e = olr_edit::pack::check(&files).unwrap_err().to_string();
    assert!(e.contains("lava") && e.contains("composition.ron"), "{e}");
}

/// A world pixel whose centre art pixel does not glow takes the group of a
/// glowing art pixel in it, never a group by default (review of batch A,
/// Important 3).
#[test]
fn a_cell_glowing_off_centre_takes_its_glow_group() {
    let mut p = stack(2);
    // Mask lava's art pixels so only the left column of world pixel 4 glows.
    paint_glow(&mut p, "lava", &Shape::Rect { x: 4, y: 0, w: 2, h: 1 }, 0).unwrap();
    let src = p.composition.as_mut().unwrap();
    let file = glow_file("lava");
    let (w, h, mut mask) =
        olr_format::image_io::decode_grey_within(&src.files[&file], olr_format::image_io::LEVEL_ART).unwrap();
    assert_eq!((w, h), (12, 2));
    mask[8] = 255; // art (8, 0): world pixel 4's top-left, off centre at scale 2
    src.files.insert(file, olr_format::image_io::encode_grey(w, h, &mask));
    let l = baked(&p);
    let g = l.glow.as_ref().unwrap();
    assert_eq!(g.groups[4], 2, "lava's group");
    assert_eq!(g.groups[5], 0, "nothing glows there");
}

#[test]
fn clear_follows_the_element_giving_the_material() {
    let mut p = stack(2);
    // A glass (rock) layer over x 3-4, above the ground and the lava.
    add_layer(&mut p, ElementMaterial::Rock, "Glass").unwrap();
    draw(&mut p, "glass", &Shape::Rect { x: 3, y: 0, w: 2, h: 1 }, &Paint::Colour(RED), None).unwrap();
    assert_eq!(baked(&p).clear, None, "nothing clear yet");
    set(&mut p, "glass", Property::Clear(true)).unwrap();
    assert_eq!(baked(&p).clear.as_deref(), Some(&[0, 0, 0, 255, 255, 0][..]));
    set(&mut p, "lava", Property::Clear(true)).unwrap();
    let full = baked(&p);
    assert_eq!(full.clear.as_deref(), Some(&[0, 0, 0, 255, 255, 255][..]));
    // A region bake equals the full one.
    let mut part = full.clone();
    part.clear.as_mut().unwrap().iter_mut().for_each(|v| *v = 0);
    let baker = Baker::new(p.composition.as_ref().unwrap(), (6, 1), 2, &RgbaImage::new(1, 1)).unwrap();
    baker.bake_region(&mut part, Rect { x: 0, y: 0, w: 4, h: 1 });
    baker.bake_region(&mut part, Rect { x: 4, y: 0, w: 2, h: 1 });
    assert_eq!(part.clear, full.clear);
    // A background element's clear never counts: air already lets light through.
    set(&mut p, "glass", Property::Clear(false)).unwrap();
    set(&mut p, "lava", Property::Clear(false)).unwrap();
    set(&mut p, "sky", Property::Clear(true)).unwrap();
    assert_eq!(baked(&p).clear, None);
}

#[test]
fn a_raster_element_cannot_be_clear() {
    let mut p = pack(4, 1, 1);
    ensure_composed(&mut p).unwrap();
    let e = set(&mut p, "base", Property::Clear(true)).unwrap_err().to_string();
    assert!(e.contains("clear"), "{e}");
}

#[test]
fn resize_re_lays_clear_flags() {
    let mut l = level(&pack(3, 1, 1)).clone();
    l.clear = Some(vec![0, 255, 0]);
    olr_edit::level::resize(&mut l, (4, 1), (1, 0), Material::Rock, &Paint::Colour(RED)).unwrap();
    assert_eq!(l.clear.as_deref(), Some(&[0, 0, 255, 0][..]));
    olr_edit::level::resize(&mut l, (1, 1), (-2, 0), Material::Rock, &Paint::Colour(RED)).unwrap();
    assert_eq!(l.clear.as_deref(), Some(&[255][..]));
}

/// Plan 15a's final review: resizing a masked image resamples its glow mask
/// to the new box, as the editor does (it was refused).
#[test]
fn resizing_a_masked_image_resamples_its_mask() {
    let mut p = pack(8, 4, 1);
    let mut img = RgbaImage::new(4, 4);
    for y in 0..4 {
        for x in 0..4 {
            img.set(x, y, RED);
        }
    }
    let bytes = olr_format::image_io::encode_rgba(&img);
    let id = add_image(&mut p, "a.png", bytes, (0, 0), (4, 4), ElementMaterial::Rock, "A").unwrap();
    set(&mut p, &id, glow(100, "lava")).unwrap();
    paint_glow(&mut p, &id, &Shape::Rect { x: 0, y: 0, w: 2, h: 4 }, 0).unwrap();
    set(&mut p, &id, Property::Size((8, 4))).unwrap();
    let c = p.composition.as_ref().unwrap();
    let (w, h, m) =
        olr_format::image_io::decode_grey_within(&c.files[&glow_file(&id)], olr_format::image_io::LEVEL_ART)
            .unwrap();
    assert_eq!((w, h), (8, 4));
    assert_eq!((m[0], m[3], m[4], m[7]), (0, 0, 255, 255), "the erased left half stays the left half");
}
