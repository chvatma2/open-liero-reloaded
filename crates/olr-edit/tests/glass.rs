//! The background glass design: a glass element's covered back pixels bake
//! at its opacity over what lies below, its tint and frost into
//! `look/glass.png`, and a distant layer's glow never passes through it.

use std::collections::BTreeMap;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::compose::bake;
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{ElementGlass, ElementGlow, ElementMaterial, LevelSource};
use olr_format::look::{GlowGroupSource, LookSource, ReachSource};

const SKY: Rgba = Rgba::rgb(30, 60, 220);
const WALL: Rgba = Rgba::rgb(60, 60, 60);
const PANE: Rgba = Rgba::rgb(200, 100, 50);
const S: u32 = 2;

/// 6 × 1 at art scale 2: a distant sky everywhere, a wall at x 0-1, a
/// glass pane at x 1-3 (over the wall at x 1).
fn window(glass: ElementGlass) -> Pack {
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
        id: "sky".into(),
        colour: Default::default(),
        intensity: 100,
        reach: ReachSource::Unlimited,
        animation: Default::default(),
        phase_ms: None,
    };
    let level = LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (6, 1),
        art_scale: S,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Background; 6],
        front: RgbaImage::new(6 * S, S),
        back: RgbaImage::new(6 * S, S),
        thumb: None,
        look: Some(LookSource { glow_groups: vec![group], ..Default::default() }),
        glow: None,
        clear: None,
        glass: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    };
    let mut p = Pack { manifest, source: Source::Level(level), composition: None, other: BTreeMap::new() };
    let paint = |p: &mut Pack, id: &str, x: i32, w: i32, c: Rgba| {
        draw(p, id, &Shape::Rect { x, y: 0, w, h: 1 }, &Paint::Colour(c), None).unwrap();
    };
    add_layer(&mut p, ElementMaterial::Background, "Sky").unwrap();
    paint(&mut p, "sky", 0, 6, SKY);
    set(&mut p, "sky", Property::Distant(Some(90))).unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Wall").unwrap();
    paint(&mut p, "wall", 0, 2, WALL);
    add_layer(&mut p, ElementMaterial::Background, "Pane").unwrap();
    paint(&mut p, "pane", 1, 3, PANE);
    set(&mut p, "pane", Property::Glass(Some(glass))).unwrap();
    p
}

fn clear(opacity: u8) -> ElementGlass {
    ElementGlass { opacity, tint: 0, frost: 0 }
}

fn baked(p: &Pack) -> LevelSource {
    let Source::Level(l) = &p.source else { panic!() };
    let mut l = l.clone();
    bake(&mut l, p.composition.as_ref().unwrap(), &RgbaImage::new(1, 1)).unwrap();
    l
}

/// `src` over `dst`, straight alpha, `dst` opaque, rounded.
fn over(src: Rgba, a: u8, dst: Rgba) -> Rgba {
    let mix = |s: u8, d: u8| {
        ((u32::from(s) * u32::from(a) + u32::from(d) * (255 - u32::from(a)) + 127) / 255) as u8
    };
    Rgba::rgb(mix(src.r, dst.r), mix(src.g, dst.g), mix(src.b, dst.b))
}

#[test]
fn glass_bakes_at_its_opacity() {
    let l = baked(&window(clear(35)));
    for ax in [2 * S, 3 * S + 1] {
        // Over nothing: the pane's own colour at 35 % (89), no tile colour.
        assert_eq!(l.back.get(ax, 0), Rgba { a: 89, ..PANE }, "art x {ax}");
        assert_eq!(l.front.get(ax, 0), l.back.get(ax, 0));
    }
    // Beside it, see-through as before.
    assert_eq!(l.back.get(4 * S, 0).a, 0);
}

#[test]
fn glass_over_an_opaque_element_is_opaque() {
    let l = baked(&window(clear(35)));
    assert_eq!(l.back.get(S, 0), over(PANE, 89, WALL));
    assert_eq!(l.back.get(0, 0), WALL);
}

#[test]
fn the_filter_and_frost_bake_per_art_pixel() {
    let texel = |l: &LevelSource, ax: u32| {
        let g = l.glass.as_ref().expect("a glass map");
        let i = (ax * 4) as usize;
        [g.texels[i], g.texels[i + 1], g.texels[i + 2], g.texels[i + 3]]
    };
    let l = baked(&window(ElementGlass { opacity: 35, tint: 100, frost: 15 }));
    assert_eq!(l.glass.as_ref().unwrap().art, (6 * S, S));
    assert_eq!(texel(&l, 2 * S), [200, 100, 50, 15]);
    assert_eq!(texel(&l, 4 * S), [255, 255, 255, 0], "no glass there");
    let l = baked(&window(ElementGlass { opacity: 35, tint: 50, frost: 0 }));
    assert_eq!(texel(&l, 2 * S), [228, 178, 153, 0]);
    // The topmost glass gives the texel: a clear pane over the tinted one.
    let mut p = window(ElementGlass { opacity: 35, tint: 100, frost: 15 });
    add_layer(&mut p, ElementMaterial::Background, "Over").unwrap();
    draw(&mut p, "over", &Shape::Rect { x: 3, y: 0, w: 1, h: 1 }, &Paint::Colour(WALL), None).unwrap();
    set(&mut p, "over", Property::Glass(Some(clear(50)))).unwrap();
    let l = baked(&p);
    assert_eq!(texel(&l, 3 * S), [255, 255, 255, 0]);
    assert_eq!(texel(&l, 2 * S), [200, 100, 50, 15]);
}

#[test]
fn clear_glass_writes_no_map() {
    assert_eq!(baked(&window(clear(35))).glass, None);
}

#[test]
fn glass_does_not_carry_distant_glow() {
    let mut p = window(clear(35));
    set(&mut p, "sky", Property::Glow(Some(ElementGlow { strength: 80, group: "sky".into(), mask: false })))
        .unwrap();
    let l = baked(&p);
    let g = l.glow.as_ref().expect("the sky glows");
    let strength = ((80u32 * 255 + 50) / 100) as u8;
    assert_eq!((g.back[(2 * S) as usize], g.groups_back[2]), (0, 0), "glass keeps its own glow");
    assert_eq!((g.back[(4 * S) as usize], g.groups_back[4]), (strength, 1), "see-through takes the sky's");
    assert_eq!(l.back.get(2 * S, 0), Rgba { a: 89, ..PANE });
}

#[test]
fn glass_in_a_level_without_distant_layers_is_opaque() {
    let mut p = window(clear(35));
    set(&mut p, "sky", Property::Visible(false)).unwrap();
    let l = baked(&p);
    assert_eq!(l.back.get(2 * S, 0).a, 255);
}

#[test]
fn the_glass_property_sets_and_refuses() {
    let mut p = window(clear(35));
    add_layer(&mut p, ElementMaterial::Rock, "Rock").unwrap();
    let e = set(&mut p, "rock", Property::Glass(Some(clear(35)))).unwrap_err().to_string();
    assert!(e.contains("glass") && e.contains("background"), "{e}");
    let e = set(&mut p, "base", Property::Glass(Some(clear(35)))).unwrap_err();
    assert!(matches!(e, ElementError::WrongKind { .. }), "{e}");
    set(&mut p, "pane", Property::Glass(None)).unwrap();
    assert_eq!(baked(&p).back.get(2 * S, 0), Rgba { a: 255, ..PANE });
}

#[test]
fn resize_re_lays_the_glass_map() {
    let mut l = baked(&window(ElementGlass { opacity: 35, tint: 100, frost: 15 }));
    olr_edit::level::resize(&mut l, (7, 1), (1, 0), Material::Rock, &Paint::Colour(WALL)).unwrap();
    let g = l.glass.as_ref().expect("kept");
    assert_eq!(g.art, (7 * S, S));
    let at = |ax: u32| &g.texels[(ax * 4) as usize..(ax * 4 + 4) as usize];
    assert_eq!(at(3 * S), [200, 100, 50, 15], "moved one world pixel right");
    assert_eq!(at(0), [255, 255, 255, 0], "the new area has none");
    let mut fs = olr_vfs::MemFs::new();
    olr_format::levels::write_level_files(&mut fs, &l).unwrap();
}

#[test]
fn an_asset_of_glass_carries_no_glass() {
    // An asset carries no glass (`check_asset`): saving a pane as one
    // keeps its pixels and drops the glass.
    let p = window(clear(35));
    let decoded = olr_edit::asset::DecodedLevel::of_pack(&p).unwrap();
    let asset = olr_edit::asset::from_elements(&decoded.level(), &["pane".to_string()], "Pane").unwrap();
    assert!(asset.pieces.iter().all(|piece| piece.element.glass.is_none()));
    olr_edit::asset::encode(&asset).unwrap();
}

/// A tint or frost changed in the composition leaves `look/glass.png`
/// stale until baked (tint and frost touch no other baked file).
#[test]
fn stale_lists_the_glass_map_after_a_tint_change() {
    let mut p = window(ElementGlass { opacity: 35, tint: 100, frost: 15 });
    let l = baked(&p);
    p.source = Source::Level(l);
    let tile = RgbaImage::new(1, 1);
    let stale = |p: &Pack| olr_edit::compose::stale(p, &tile).unwrap();
    assert!(!stale(&p).contains(&"look/glass.png".to_string()), "{:?}", stale(&p));
    set(&mut p, "pane", Property::Glass(Some(ElementGlass { opacity: 35, tint: 50, frost: 15 }))).unwrap();
    assert_eq!(stale(&p), ["look/glass.png"]);
}
