//! Issue #62: a glowing distant layer's light is baked at its reference
//! placement (the camera at the level's centre: its box) into the level's
//! back glow and back groups, where the back is see-through.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{DistantGlow, Material};
use olr_edit::compose::{Baker, Rect, bake, compose_from, split_from, stale};
use olr_format::image_io;
use olr_format::levels::{
    CompositionSource, Element, ElementGlow, ElementKind, ElementMaterial, LevelSource, drawn_files,
    glow_file,
};
use olr_format::look::{GlowGroupSource, LookSource, ReachSource};

const SKY: Rgba = Rgba::rgb(30, 60, 220);
const HILL: Rgba = Rgba::rgb(40, 120, 40);
const WALL: Rgba = Rgba::rgb(140, 90, 40);

/// A `w` × `h` level at art scale `s` with the glow groups `lamp` and `sky`.
fn level(w: u32, h: u32, s: u32) -> LevelSource {
    let group = |id: &str| GlowGroupSource {
        id: id.into(),
        colour: Default::default(),
        intensity: 100,
        reach: ReachSource::Unlimited,
        animation: Default::default(),
        phase_ms: None,
    };
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
        look: Some(LookSource { glow_groups: vec![group("lamp"), group("sky")], ..Default::default() }),
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
        live: None,
    }
}

fn tile() -> RgbaImage {
    let mut t = RgbaImage::new(1, 1);
    t.set(0, 0, Rgba::rgb(9, 9, 9));
    t
}

fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

/// Adds a Background drawn layer `id` boxed at `at` (world) of `size`
/// holding `art`, `distant` at that depth, glowing `glow` (strength,
/// group) on top.
fn with_layer(
    src: &mut CompositionSource,
    id: &str,
    at: (i32, i32),
    size: (u32, u32),
    art: &RgbaImage,
    distant: Option<u8>,
    glow: Option<(u8, &str)>,
) {
    let files = drawn_files(id, false, false);
    src.files.insert(files[0].clone(), image_io::encode_rgba(art));
    src.composition.elements.push(Element {
        id: id.into(),
        name: id.into(),
        visible: true,
        locked: false,
        folder: None,
        kind: ElementKind::Drawn {
            material: ElementMaterial::Background,
            at,
            size: Some(size),
            mixed: false,
            back: false,
        },
        glow: glow.map(|(strength, group)| ElementGlow {
            strength,
            group: group.into(),
            mask: false,
            beam: None,
        }),
        clear: false,
        glass: None,
        distant,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    });
}

/// 8 × 4 at art scale 2: a glowing sky (distant, depth 90, boxed one pixel
/// off the level's left) behind a wall over the left quarter.
fn sky_level(strength: u8) -> (LevelSource, CompositionSource) {
    let (w, h, s) = (8, 4, 2);
    let mut src = CompositionSource::default();
    with_layer(&mut src, "sky", (-1, 0), (10, 4), &solid(20, 8, SKY), Some(90), Some((strength, "sky")));
    with_layer(&mut src, "wall", (0, 0), (2, 4), &solid(4, 8, WALL), None, None);
    let mut l = level(w, h, s);
    bake(&mut l, &src, &tile()).unwrap();
    (l, src)
}

#[test]
fn a_see_through_pixel_takes_the_covering_layers_glow_and_group() {
    let (l, src) = sky_level(80);
    let (w, s) = (8u32, 2u32);
    let g = l.glow.as_ref().expect("the sky glows");
    let strength = ((80u32 * 255 + 50) / 100) as u8;
    for ay in 0..8 {
        for ax in 0..16 {
            let i = (ay * 16 + ax) as usize;
            let back = l.back.get(ax, ay);
            if ax < 2 * s {
                // The wall: an opaque back pixel never takes a distant glow.
                assert_eq!((back, g.back[i], g.front[i]), (WALL, 0, 0), "({ax}, {ay})");
            } else {
                // See-through: the sky's colour at alpha 0, its glow.
                assert_eq!((back, g.back[i], g.front[i]), (Rgba { a: 0, ..SKY }, strength, strength));
                assert_eq!(l.front.get(ax, ay), back);
            }
        }
    }
    for y in 0..4 {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let want = if x < 2 { 0 } else { 2 };
            assert_eq!((g.groups_back[i], g.groups[i]), (want, want), "({x}, {y})");
        }
    }
    // The distant layer carries its glow (the group's value, the strength).
    assert_eq!(l.distant[0].glow, Some(DistantGlow { group: 2, strength: 80, mask: None }));
    // A region bake equals the whole one.
    let baker = Baker::new(&src, (8, 4), 2, &tile()).unwrap();
    let mut whole = l.clone();
    baker.bake_region(&mut whole, Rect { x: 0, y: 0, w: 8, h: 4 });
    assert_eq!(whole, l);
    let mut region = l.clone();
    baker.bake_region(&mut region, Rect { x: 3, y: 1, w: 2, h: 2 });
    assert_eq!(region, l);
}

#[test]
fn a_nearer_layer_that_does_not_glow_hides_a_farther_one_that_does() {
    let (w, h, s) = (8, 4, 2);
    let mut src = CompositionSource::default();
    with_layer(&mut src, "sky", (0, 0), (8, 4), &solid(16, 8, SKY), Some(90), Some((100, "sky")));
    // Hills over the bottom half, near, not glowing, partly see-through.
    let mut hills = solid(16, 4, HILL);
    hills.set(15, 0, Rgba::TRANSPARENT);
    with_layer(&mut src, "hills", (0, 2), (8, 2), &hills, Some(40), None);
    let mut l = level(w, h, s);
    bake(&mut l, &src, &tile()).unwrap();
    let g = l.glow.as_ref().unwrap();
    let at = |x: u32, y: u32| (l.back.get(x, y), g.back[(y * w * s + x) as usize]);
    assert_eq!(at(3, 1), (Rgba { a: 0, ..SKY }, 255), "the sky above the hills");
    // The hills hide the sky: no glow, the tile's colour (see-through).
    assert_eq!(at(3, 5), (Rgba { a: 0, ..tile().get(0, 0) }, 0));
    // Where the hills are uncovered the sky shows and glows again.
    assert_eq!(at(15, 4), (Rgba { a: 0, ..SKY }, 255));
    // Groups follow: the top rows the sky's, the hidden ones none (the
    // block of (7, 2) takes the sky at its uncovered art pixel).
    assert_eq!(g.groups_back[(w + 3) as usize], 2);
    assert_eq!(g.groups_back[(3 * w + 3) as usize], 0);
    assert_eq!(g.groups_back[(2 * w + 7) as usize], 2);
}

#[test]
fn a_painted_mask_scales_the_distant_glow() {
    let (w, h, s) = (4, 2, 1);
    let mut src = CompositionSource::default();
    with_layer(&mut src, "sky", (0, 0), (4, 2), &solid(4, 2, SKY), Some(50), Some((100, "sky")));
    src.composition.elements[0].glow.as_mut().unwrap().mask = true;
    let mask = [255, 128, 0, 0, 255, 255, 255, 255];
    src.files.insert(glow_file("sky"), image_io::encode_grey(4, 2, &mask));
    let mut l = level(w, h, s);
    bake(&mut l, &src, &tile()).unwrap();
    let g = l.glow.as_ref().unwrap();
    assert_eq!(&g.back[..4], &[255, 128, 0, 0]);
    // A pixel that does not glow keeps the tile's colour, groups none.
    assert_eq!(l.back.get(2, 0), Rgba { a: 0, ..tile().get(0, 0) });
    assert_eq!(&g.groups_back[..4], &[2, 2, 0, 0]);
    assert_eq!(l.distant[0].glow.as_ref().unwrap().mask.as_deref(), Some(&mask[..]));
}

/// A level without glowing distant layers bakes as before: no glow, the
/// tile's colour at alpha 0 where see-through.
#[test]
fn a_level_without_glowing_distant_layers_bakes_as_before() {
    let (mut glowing, mut src) = sky_level(80);
    src.composition.elements[0].glow = None;
    bake(&mut glowing, &src, &tile()).unwrap();
    assert_eq!(glowing.glow, None);
    assert_eq!(glowing.distant[0].glow, None);
    assert_eq!(glowing.back.get(10, 3), Rgba { a: 0, ..tile().get(0, 0) });
    // A glow group the look lacks lights nothing.
    src.composition.elements[0].glow =
        Some(ElementGlow { strength: 50, group: "moon".into(), mask: false, beam: None });
    let mut l = level(8, 4, 2);
    bake(&mut l, &src, &tile()).unwrap();
    assert_eq!((l.glow, &l.distant[0].glow), (None, &None));
}

/// The light test's bake (issue #62): a glowing sky element replaced by a
/// glowing distant layer of the same art and glow, the sky region
/// see-through, bakes the same back glow and back groups (and front ones):
/// the same light (`olr-render`'s `a_see_through_glow_lights_as_an_opaque_one`).
#[test]
fn a_distant_sky_bakes_the_glow_an_ordinary_sky_does() {
    let (w, h, s) = (8, 4, 2);
    let build = |distant: Option<u8>| {
        let mut src = CompositionSource::default();
        with_layer(&mut src, "sky", (0, 0), (8, 4), &solid(16, 8, SKY), distant, Some((70, "sky")));
        // A wall in the middle with a glowing lamp element on it.
        with_layer(&mut src, "wall", (3, 0), (2, 4), &solid(4, 8, WALL), None, Some((40, "lamp")));
        let mut l = level(w, h, s);
        bake(&mut l, &src, &tile()).unwrap();
        l
    };
    let (plain, distant) = (build(None), build(Some(60)));
    let (a, b) = (plain.glow.unwrap(), distant.glow.unwrap());
    assert_eq!((a.back, a.groups_back), (b.back, b.groups_back));
    assert_eq!((a.front, a.groups), (b.front, b.groups));
    // The colours are the same, the sky's alpha 0.
    for (p, q) in plain.back.as_raw().chunks(4).zip(distant.back.as_raw().chunks(4)) {
        assert_eq!(&p[..3], &q[..3]);
    }
}

/// A plain level's glowing distant layers become glowing distant
/// elements (their masks too), which bake back to the same level.
#[test]
fn glowing_distant_layers_split_and_bake_back() {
    let (w, h, s) = (4, 2, 1);
    let mut src = CompositionSource::default();
    with_layer(&mut src, "sky", (0, 0), (4, 2), &solid(4, 2, SKY), Some(50), Some((90, "sky")));
    src.composition.elements[0].glow.as_mut().unwrap().mask = true;
    src.files.insert(glow_file("sky"), image_io::encode_grey(4, 2, &[255, 9, 0, 0, 7, 7, 7, 7]));
    with_layer(&mut src, "far", (1, 0), (2, 1), &solid(2, 1, HILL), Some(80), Some((30, "lamp")));
    let mut l = level(w, h, s);
    bake(&mut l, &src, &tile()).unwrap();
    for split in [split_from(&l), compose_from(&l)] {
        let mut again = level(w, h, s);
        bake(&mut again, &split, &tile()).unwrap();
        assert!(again == l, "{:?}", split.composition.elements);
    }
    // `stale` follows a distant glow's strength.
    let mut pack = olr_edit::pack::Pack {
        manifest: olr_data::manifest::PackManifest {
            format_version: olr_data::manifest::PACK_FORMAT_VERSION,
            id: "t".into(),
            name: "T".into(),
            version: "1".into(),
            kind: olr_data::manifest::PackKind::Level,
            authors: Vec::new(),
            origin: None,
            license: None,
        },
        source: olr_edit::pack::Source::Level(l.clone()),
        composition: Some(src),
        placed: None,
        legacy_composition: false,
        other: Default::default(),
        verbatim: Default::default(),
    };
    assert_eq!(stale(&pack, &tile()).unwrap(), Vec::<String>::new());
    pack.composition.as_mut().unwrap().composition.elements[1].glow.as_mut().unwrap().strength = 31;
    let stale = stale(&pack, &tile()).unwrap();
    assert!(
        stale.contains(&"level.ron".to_string()) && stale.contains(&"look/glow-back.png".to_string()),
        "{stale:?}"
    );
}
