//! Directional glow in the bake: beam texels follow the element giving a
//! world pixel its glow group; the haze map is kept.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::compose::{bake, stale};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{ElementGlow, ElementMaterial, GlowBeam, GlowBeamMap, LevelSource};
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
    let group = GlowGroupSource {
        id: "sun".into(),
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
        look: Some(LookSource { glow_groups: vec![group], ..Default::default() }),
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

fn beam(direction: u16) -> Property {
    Property::Glow(Some(ElementGlow {
        strength: 100,
        group: "sun".into(),
        mask: false,
        beam: Some(GlowBeam { direction, spread: 15 }),
    }))
}

fn plain() -> Property {
    Property::Glow(Some(ElementGlow { strength: 100, group: "sun".into(), mask: false, beam: None }))
}

fn layer(p: &mut Pack, m: ElementMaterial, id: &str, x: i32, w: i32) {
    add_layer(p, m, id).unwrap();
    let id = id.to_lowercase();
    draw(p, &id, &Shape::Rect { x, y: 0, w, h: 1 }, &Paint::Colour(RED), None).unwrap();
}

fn baked(p: &Pack) -> LevelSource {
    let mut l = level(p).clone();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    l
}

fn texel(m: &GlowBeamMap, back: bool, x: usize) -> [u8; 2] {
    let l = if back { &m.back } else { &m.front };
    [l[x * 2], l[x * 2 + 1]]
}

#[test]
fn a_directional_rock_layer_bakes_its_beam() {
    let mut p = pack(6, 1, 2);
    layer(&mut p, ElementMaterial::Rock, "Cliff", 2, 2);
    set(&mut p, "cliff", beam(166)).unwrap();
    let l = baked(&p);
    let b = l.glow_beams.as_ref().expect("beams");
    for x in 0..6 {
        let want = if (2..4).contains(&x) { [84, 15] } else { [0, 0] };
        assert_eq!(texel(b, false, x), want, "front {x}");
        assert_eq!(texel(b, true, x), [0, 0], "back {x}");
    }
}

#[test]
fn a_dirt_pixel_takes_no_beam() {
    let mut p = pack(6, 1, 2);
    layer(&mut p, ElementMaterial::Rock, "Cliff", 0, 2);
    layer(&mut p, ElementMaterial::Dirt, "Soil", 2, 2);
    // Merged, the layer is mixed: rock cells and dirt cells.
    merge_down(&mut p, "soil").unwrap();
    set(&mut p, "cliff", beam(0)).unwrap();
    let l = baked(&p);
    assert!(l.glow.is_some());
    let b = l.glow_beams.as_ref().expect("beams");
    assert_eq!(texel(b, false, 0), [1, 15]);
    assert_eq!(l.glow.as_ref().unwrap().groups[2], 1, "the dirt cell glows");
    assert_eq!(texel(b, false, 2), [0, 0], "dirt");
    assert_eq!(texel(b, false, 3), [0, 0], "dirt");
}

#[test]
fn the_topmost_glowing_element_gives_the_beam() {
    let mut p = pack(4, 1, 2);
    layer(&mut p, ElementMaterial::Rock, "Low", 0, 4);
    layer(&mut p, ElementMaterial::Rock, "High", 1, 2);
    set(&mut p, "low", beam(0)).unwrap();
    set(&mut p, "high", beam(90)).unwrap();
    let l = baked(&p);
    let b = l.glow_beams.as_ref().unwrap();
    assert_eq!(
        (0..4).map(|x| texel(b, false, x)[0]).collect::<Vec<_>>(),
        [1, 46, 46, 1],
        "high's 90 / 2 + 1 where it covers"
    );
    // A glowing element without a beam on top gives no beam.
    set(&mut p, "high", plain()).unwrap();
    let l = baked(&p);
    let b = l.glow_beams.as_ref().unwrap();
    assert_eq!((0..4).map(|x| texel(b, false, x)[0]).collect::<Vec<_>>(), [1, 0, 0, 1]);
}

#[test]
fn the_backdrop_beam_follows_the_back_glow() {
    let mut p = pack(4, 1, 2);
    layer(&mut p, ElementMaterial::Background, "Sky", 0, 4);
    set(&mut p, "sky", beam(40)).unwrap();
    let l = baked(&p);
    let b = l.glow_beams.as_ref().unwrap();
    for x in 0..4 {
        assert_eq!(texel(b, true, x), [21, 15]);
        // Ruling T5a: the composite's map holds beams on rock only, so a
        // backdrop's beam is baked (and counted) once.
        assert_eq!(texel(b, false, x), [0, 0], "a background cell has no composite beam");
    }
}

#[test]
fn no_beam_writes_no_files() {
    let mut p = pack(4, 1, 2);
    layer(&mut p, ElementMaterial::Rock, "Cliff", 0, 4);
    set(&mut p, "cliff", plain()).unwrap();
    let l = baked(&p);
    assert!(l.glow.is_some());
    assert!(l.glow_beams.is_none());
    // Removing the beam removes a stale map.
    set(&mut p, "cliff", beam(10)).unwrap();
    let mut l = baked(&p);
    assert!(l.glow_beams.is_some());
    set(&mut p, "cliff", plain()).unwrap();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    assert!(l.glow_beams.is_none());
}

#[test]
fn stale_lists_the_beam_map_after_a_direction_change() {
    let mut p = pack(4, 1, 2);
    layer(&mut p, ElementMaterial::Rock, "Cliff", 0, 4);
    set(&mut p, "cliff", beam(10)).unwrap();
    let tile = RgbaImage::new(1, 1);
    let l = baked(&p);
    p.source = Source::Level(l);
    assert_eq!(stale(&p, &tile).unwrap(), Vec::<String>::new());
    set(&mut p, "cliff", beam(100)).unwrap();
    let s = stale(&p, &tile).unwrap();
    assert!(s.contains(&"look/glow-beam.png".to_string()), "{s:?}");
    assert!(!s.contains(&"look/glow.png".to_string()), "{s:?}");
}

#[test]
fn resize_relays_beams_and_haze() {
    let mut p = pack(4, 2, 1);
    let Source::Level(l) = &mut p.source else { panic!() };
    let mut b = GlowBeamMap::none((4, 2));
    b.front[(4 + 1) * 2..(4 + 1) * 2 + 2].copy_from_slice(&[7, 20]);
    b.back[0..2].copy_from_slice(&[3, 5]);
    l.glow_beams = Some(b);
    let mut haze = vec![0u8; 8];
    haze[5] = 200;
    l.haze = Some(haze);
    olr_edit::level::resize(l, (6, 4), (1, 1), Material::Background, &Paint::Colour(RED)).unwrap();
    let b = l.glow_beams.as_ref().unwrap();
    assert!(b.fits((6, 4)));
    assert_eq!(texel(b, false, 2 * 6 + 2), [7, 20]);
    assert_eq!(texel(b, true, 6 + 1), [3, 5]);
    assert_eq!(b.front.iter().filter(|v| **v != 0).count(), 2);
    let h = l.haze.as_ref().unwrap();
    assert_eq!(h.len(), 24);
    assert_eq!(h[2 * 6 + 2], 200);
    // The old 7 zeros relaid; the 16 new pixels full (ruling T6c).
    assert_eq!(h.iter().filter(|v| **v == 0).count(), 7);
    assert_eq!(h.iter().filter(|v| **v == 255).count(), 16);
}

/// Ruling T6c: a grown level's new area of a haze map takes the level's
/// whole haze (255), as a new map does (T6a); the old pixels are relaid.
#[test]
fn a_grown_levels_haze_map_is_full() {
    let mut p = pack(2, 1, 1);
    let Source::Level(l) = &mut p.source else { panic!() };
    l.haze = Some(vec![0, 40]);
    olr_edit::level::resize(l, (3, 2), (1, 0), Material::Background, &Paint::Colour(RED)).unwrap();
    assert_eq!(l.haze.as_deref(), Some(&[255, 0, 40, 255, 255, 255][..]));
}

#[test]
fn the_haze_map_survives_a_bake() {
    let mut p = pack(4, 1, 2);
    layer(&mut p, ElementMaterial::Rock, "Cliff", 0, 4);
    set(&mut p, "cliff", beam(10)).unwrap();
    let Source::Level(l) = &mut p.source else { panic!() };
    olr_edit::look::set_haze_map(l, Some(vec![0, 64, 128, 255])).unwrap();
    assert!(olr_edit::look::set_haze_map(l, Some(vec![1, 2])).is_err());
    assert_eq!(l.haze.as_deref(), Some(&[0, 64, 128, 255][..]));
    let l = baked(&p);
    assert_eq!(l.haze.as_deref(), Some(&[0, 64, 128, 255][..]));
}
