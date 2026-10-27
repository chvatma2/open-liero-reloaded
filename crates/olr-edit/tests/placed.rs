//! Issue #73: a level of placed copies baked (`olr_edit::placed`): a
//! ground alone bakes as its composition, stretched and rescaled copies
//! resample nearest, positions scale with the copy, opacity mixes, distant
//! pieces lie below, the gathering shares and renames, Live copies bake
//! static and refuse dirt cells.

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::{ADDED_LIGHTING, Asset, Piece, State, encode};
use olr_edit::compose::{self, Decoded};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape, rescale_art};
use olr_edit::pack::{Pack, Source};
use olr_edit::placed::{PlacedError, Renamed, bake};
use olr_format::Colour;
use olr_format::assets::{
    AssetFile, AssetSource, ExposedSource, PropertySource, ValueSource, check_own_asset,
};
use olr_format::levels::{
    AssetOrigin, CompositionSource, Element, ElementGlass, ElementGlow, ElementKind, ElementMaterial,
    GROUND_KEY, LevelAsset, LevelSource, PlacedCopy, PlacedSource, check_placed,
};
use olr_format::look::{GlowColourSource, GlowGroupSource, LookSource, ReachSource};
use olr_format::materials::MaterialSource;

const RED: Rgba = Rgba::rgb(200, 20, 20);
const GREEN: Rgba = Rgba::rgb(20, 200, 20);
const BLUE: Rgba = Rgba::rgb(20, 20, 200);
const WALL: Rgba = Rgba::rgb(60, 60, 60);
const PANE: Rgba = Rgba::rgb(200, 100, 50);

fn blank(size: (u32, u32), s: u32, look: Option<LookSource>) -> LevelSource {
    LevelSource {
        name: "T".into(),
        notes: String::new(),
        size,
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Background; (size.0 * size.1) as usize],
        front: RgbaImage::new(size.0 * s, size.1 * s),
        back: RgbaImage::new(size.0 * s, size.1 * s),
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
    }
}

fn pack(level: LevelSource) -> Pack {
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
    Pack { manifest, source: Source::Level(level), composition: None, other: BTreeMap::new() }
}

fn group(id: &str, colour: GlowColourSource) -> GlowGroupSource {
    GlowGroupSource {
        id: id.into(),
        colour,
        intensity: 300,
        reach: ReachSource::Pixels(40),
        animation: Default::default(),
        phase_ms: None,
    }
}

fn material(id: &str, name: &str) -> MaterialSource {
    MaterialSource { id: id.into(), name: name.into(), notes: String::new(), heat: None, light_soak: None }
}

fn tile() -> RgbaImage {
    let mut t = RgbaImage::new(2, 2);
    t.set(0, 0, Rgba::rgb(9, 8, 7));
    t.set(1, 1, Rgba::rgb(70, 80, 90));
    t
}

fn level_of(p: &Pack) -> &LevelSource {
    let Source::Level(l) = &p.source else { panic!("a level") };
    l
}

/// The composition `src` of `level` as the level's ground: an `Own`
/// asset of its elements, folders and files, the level's size and art
/// scale, the look's glow groups and materials, placed alone at `(0, 0)`.
fn ground_of(src: &CompositionSource, level: &LevelSource) -> PlacedSource {
    let look = level.look.clone().unwrap_or_default();
    let mut file = AssetFile::new("Ground", level.size, src.composition.elements.clone());
    file.art_scale = level.art_scale;
    file.folders = src.composition.folders.clone();
    file.glow_groups = look.glow_groups.clone();
    file.materials = look.materials.clone();
    let files =
        src.files.iter().map(|(k, v)| (k.strip_prefix("editor/").unwrap().to_string(), v.clone())).collect();
    let asset = AssetSource::new(file, files);
    check_own_asset(&asset).unwrap();
    let mut out = PlacedSource::default();
    out.file.assets = vec![LevelAsset { key: GROUND_KEY.into(), origin: AssetOrigin::Own }];
    out.file.copies = vec![PlacedCopy { locked: true, ..PlacedCopy::new(GROUND_KEY, GROUND_KEY, (0, 0)) }];
    out.assets.insert(GROUND_KEY.into(), asset);
    out
}

/// Bakes `p`'s composition both ways, holds them equal and returns it.
fn bakes_alike(p: &Pack) -> LevelSource {
    let level = level_of(p);
    let src = p.composition.as_ref().unwrap();
    let mut a = level.clone();
    compose::bake(&mut a, src, &tile()).unwrap();
    let mut b = level.clone();
    let baked = bake(&mut b, &ground_of(src, level), &tile()).unwrap();
    assert_eq!(baked.groups_renamed, vec![]);
    assert!(!baked.added_look);
    assert!(a == b, "the placed bake differs from the composition's");
    a
}

fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> Rgba) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, f(x, y));
        }
    }
    olr_format::image_io::encode_rgba(&img)
}

/// A dirt level with a rock floor at art scale 2 (its raster base), a
/// rock wall made of stone with a painted map, a mixed layer and an
/// image; and a see-through one: a glowing distant sky, a wall, a glass
/// pane, a masked glowing lamp.
#[test]
fn a_ground_alone_bakes_as_its_composition() {
    let s = 2;
    let look = LookSource {
        glow_groups: vec![group("sky", GlowColourSource::Own), group("lamps", GlowColourSource::Own)],
        materials: vec![material("stone", "Stone"), material("wood", "Wood")],
        ..Default::default()
    };
    let mut level = blank((12, 8), s, Some(look.clone()));
    for (i, m) in level.material.iter_mut().enumerate() {
        *m = if i >= 12 * 6 { Material::Rock } else { Material::Dirt };
    }
    for y in 0..16 {
        for x in 0..24 {
            level.front.set(x, y, Rgba::rgb(x as u8 * 9, y as u8 * 13, 40));
            level.back.set(x, y, Rgba::rgb(10, x as u8 * 7, y as u8 * 5));
        }
    }
    let mut p = pack(level);
    ensure_composed(&mut p).unwrap();
    add_layer(&mut p, ElementMaterial::Rock, "Wall").unwrap();
    draw(&mut p, "wall", &Shape::Rect { x: 2, y: 1, w: 3, h: 4 }, &Paint::Colour(WALL), None).unwrap();
    set(&mut p, "wall", Property::Surface(Some("stone".into()))).unwrap();
    let mut map = vec![0u8; 12 * 8];
    map[12 * 2 + 3] = 2;
    set_surface_map(&mut p, "wall", false, Some(&map)).unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Mixed").unwrap();
    draw(&mut p, "mixed", &Shape::Rect { x: 6, y: 2, w: 3, h: 2 }, &Paint::Colour(GREEN), None).unwrap();
    draw(
        &mut p,
        "mixed",
        &Shape::Rect { x: 7, y: 2, w: 1, h: 1 },
        &Paint::Colour(RED),
        Some(ElementMaterial::Rock),
    )
    .unwrap();
    let image = png(5, 3, |x, y| Rgba::rgb(x as u8 * 50, y as u8 * 80, 120));
    let id = add_image(&mut p, "sign.png", image, (5, 4), (4, 2), ElementMaterial::Rock, "Sign").unwrap();
    set(
        &mut p,
        &id,
        Property::Glow(Some(ElementGlow { strength: 60, group: "lamps".into(), mask: false, beam: None })),
    )
    .unwrap();
    let l = bakes_alike(&p);
    assert!(l.surface.is_some() && l.glow.is_some());

    // The see-through level.
    let mut p = pack(blank((8, 4), s, Some(look)));
    add_layer(&mut p, ElementMaterial::Background, "Sky").unwrap();
    draw(&mut p, "sky", &Shape::Rect { x: 0, y: 0, w: 8, h: 4 }, &Paint::Colour(BLUE), None).unwrap();
    set(&mut p, "sky", Property::Distant(Some(80))).unwrap();
    set(
        &mut p,
        "sky",
        Property::Glow(Some(ElementGlow { strength: 40, group: "sky".into(), mask: false, beam: None })),
    )
    .unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Wall").unwrap();
    draw(&mut p, "wall", &Shape::Rect { x: 0, y: 1, w: 3, h: 2 }, &Paint::Colour(WALL), None).unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Pane").unwrap();
    draw(&mut p, "pane", &Shape::Rect { x: 2, y: 1, w: 4, h: 2 }, &Paint::Colour(PANE), None).unwrap();
    set(&mut p, "pane", Property::Glass(Some(ElementGlass { opacity: 35, tint: 40, frost: 5 }))).unwrap();
    add_layer(&mut p, ElementMaterial::Rock, "Lamp").unwrap();
    draw(&mut p, "lamp", &Shape::Rect { x: 6, y: 3, w: 2, h: 1 }, &Paint::Colour(RED), None).unwrap();
    set(
        &mut p,
        "lamp",
        Property::Glow(Some(ElementGlow { strength: 90, group: "lamps".into(), mask: false, beam: None })),
    )
    .unwrap();
    paint_glow(&mut p, "lamp", &Shape::Rect { x: 6, y: 3, w: 1, h: 1 }, 30).unwrap();
    let l = bakes_alike(&p);
    assert!(l.distant.len() == 1 && l.distant[0].glow.is_some() && l.glass.is_some());
    assert!(l.glow.as_ref().unwrap().front.iter().any(|v| *v > 0 && *v < 200), "the masked lamp");
}

/// A drawn layer piece of `size` at `at` (art scale `s`), its art
/// `art(x, y)` at art resolution, of `material`.
fn piece(
    id: &str,
    material: ElementMaterial,
    at: (i32, i32),
    size: (u32, u32),
    s: u32,
    art: impl Fn(u32, u32) -> Rgba,
) -> Piece {
    let mut img = RgbaImage::new(size.0 * s, size.1 * s);
    for y in 0..size.1 * s {
        for x in 0..size.0 * s {
            img.set(x, y, art(x, y));
        }
    }
    Piece {
        element: Element {
            id: id.into(),
            name: String::new(),
            visible: true,
            locked: false,
            folder: None,
            kind: ElementKind::Drawn { material, at, size: Some(size), mixed: false, back: false },
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        },
        decoded: Decoded::Drawn { art: img, cells: None, back: None },
        original: None,
        glow_mask: None,
        surface_map: None,
        back_surface_map: None,
    }
}

/// A placement of `assets` (key, asset; library copies of the user's)
/// and `copies`.
fn placement(assets: Vec<(&str, &Asset)>, copies: Vec<PlacedCopy>) -> PlacedSource {
    let mut out = PlacedSource::default();
    for (key, a) in assets {
        out.file
            .assets
            .push(LevelAsset { key: key.into(), origin: AssetOrigin::Yours(format!("asset-{key}")) });
        out.assets.insert(key.into(), encode(a).unwrap());
    }
    out.file.copies = copies;
    out
}

fn baked(level: &LevelSource, src: &PlacedSource) -> LevelSource {
    let mut l = level.clone();
    bake(&mut l, src, &tile()).unwrap();
    l
}

/// Columns red, green, blue.
fn stripes() -> Asset {
    let colours = [RED, GREEN, BLUE];
    let p = piece("body", ElementMaterial::Rock, (0, 0), (3, 1), 1, |x, _| colours[x as usize]);
    Asset::new("Stripes", 1, (3, 1), vec![p], vec![], vec![])
}

#[test]
fn a_stretched_copy_maps_columns_by_floor() {
    let level = blank((12, 3), 1, None);
    let copy = |size| PlacedCopy { size: Some(size), ..PlacedCopy::new("s", "a", (1, 0)) };
    let l = baked(&level, &placement(vec![("a", &stripes())], vec![copy((7, 2))]));
    // World column u of the box shows asset column floor(u × 3 / 7).
    let want = [RED, RED, RED, GREEN, GREEN, BLUE, BLUE];
    for y in 0..2 {
        for (u, c) in want.iter().enumerate() {
            assert_eq!(l.front.get(1 + u as u32, y), *c, "({u}, {y})");
            assert_eq!(l.material[(y * 12 + 1 + u as u32) as usize], Material::Rock);
        }
    }
    assert_eq!(l.material[8], Material::Background);
    assert_eq!(l.material[2 * 12 + 1], Material::Background);
    // At 2 × 1, floor(u × 3 / 2) shows columns 0 and 1: the last one goes.
    let l = baked(&level, &placement(vec![("a", &stripes())], vec![copy((2, 1))]));
    assert_eq!((l.front.get(1, 0), l.front.get(2, 0)), (RED, GREEN));
    assert_eq!(l.material[3], Material::Background);
    // Unstretched, the copy is the asset.
    let l = baked(&level, &placement(vec![("a", &stripes())], vec![PlacedCopy::new("s", "a", (4, 1))]));
    assert_eq!([4, 5, 6].map(|x| l.front.get(x, 1)), [RED, GREEN, BLUE]);
}

#[test]
fn an_art_scale_change_resamples_nearest() {
    // An asset at art scale 2 in a level at 1: art pixel floor(u × 2).
    let colours = [RED, GREEN, BLUE, WALL];
    let p = piece("body", ElementMaterial::Rock, (0, 0), (2, 1), 2, |x, y| colours[((x + y) % 4) as usize]);
    let fine = Asset::new("Fine", 2, (2, 1), vec![p.clone()], vec![], vec![]);
    let l = baked(
        &blank((4, 2), 1, None),
        &placement(vec![("a", &fine)], vec![PlacedCopy::new("f", "a", (1, 0))]),
    );
    assert_eq!((l.front.get(1, 0), l.front.get(2, 0)), (RED, BLUE));
    // An asset at art scale 1 in a level at 3: each pixel a 3 × 3 block,
    // as `rescale_art` lays it.
    let l = baked(
        &blank((4, 2), 3, None),
        &placement(vec![("a", &stripes())], vec![PlacedCopy::new("s", "a", (1, 1))]),
    );
    let Decoded::Drawn { art, .. } = &stripes().pieces[0].decoded else { panic!() };
    let want = rescale_art(art, 1, 3);
    for y in 0..3 {
        for x in 0..9 {
            assert_eq!(l.front.get(3 + x, 3 + y), want.get(x, y), "({x}, {y})");
        }
    }
    // Stretched and rescaled at once: art at 2 placed at 4 × 1 in a level
    // at 1 (world column u shows art column floor(u × 4 / 4)).
    let l = baked(
        &blank((6, 2), 1, None),
        &placement(
            vec![("a", &fine)],
            vec![PlacedCopy { size: Some((4, 1)), ..PlacedCopy::new("f", "a", (0, 0)) }],
        ),
    );
    assert_eq!([0, 1, 2, 3].map(|x| l.front.get(x, 0)), [RED, GREEN, BLUE, WALL]);
    // An image's processed grid is resampled, not processed again: a
    // level's own image at art scale 2 placed in a level at 1 shows every
    // other art pixel of its bake there.
    let mut p = pack(blank((4, 2), 2, None));
    let image = png(8, 4, |x, y| Rgba::rgb(x as u8 * 30, y as u8 * 60, 90));
    add_image(&mut p, "sign.png", image, (0, 0), (4, 2), ElementMaterial::Rock, "Sign").unwrap();
    // Without its raster base (a raster stays at (0, 0)).
    remove(&mut p, "base").unwrap();
    let fine = bakes_alike(&p);
    let mut src = ground_of(p.composition.as_ref().unwrap(), level_of(&p));
    src.file.copies[0].at = (1, 1);
    let l = baked(&blank((6, 4), 1, None), &src);
    for y in 0..2 {
        for x in 0..4 {
            assert_eq!(l.front.get(1 + x, 1 + y), fine.front.get(2 * x, 2 * y), "({x}, {y})");
        }
    }
}

/// A 4 × 1 asset of one rock dot at its left, its offset and facing
/// exposed.
fn dot() -> Asset {
    let p = piece("dot", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED);
    let mut a = Asset::new("Dot", 1, (4, 1), vec![p], vec![], vec![]);
    a.exposed = vec![
        ExposedSource {
            name: "dot_x".into(),
            property: PropertySource::OffsetX("dot".into()),
            default: None,
            min: Some(-4),
            max: Some(4),
        },
        ExposedSource {
            name: "turned".into(),
            property: PropertySource::Facing,
            default: None,
            min: None,
            max: None,
        },
    ];
    a
}

#[test]
fn positions_and_facing_scale_with_the_copy() {
    let level = blank((12, 1), 1, None);
    let rock = |l: &LevelSource| -> Vec<u32> {
        (0..12).filter(|x| l.material[*x as usize] == Material::Rock).collect()
    };
    let copy = |at: (i32, i32), values: &[(&str, ValueSource)]| PlacedCopy {
        size: Some((6, 1)),
        values: values.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        ..PlacedCopy::new("d", "a", at)
    };
    let a = dot();
    // Unmoved: [0, 1) × 1.5 is [0, 1).
    assert_eq!(rock(&baked(&level, &placement(vec![("a", &a)], vec![copy((2, 0), &[])]))), [2]);
    // An offset of 1 is 1.5: 2 (half away from zero); -1 is -2.
    let l = baked(&level, &placement(vec![("a", &a)], vec![copy((2, 0), &[("dot_x", ValueSource::Int(1))])]));
    assert_eq!(rock(&l), [4]);
    let l =
        baked(&level, &placement(vec![("a", &a)], vec![copy((4, 0), &[("dot_x", ValueSource::Int(-1))])]));
    assert_eq!(rock(&l), [2]);
    // Facing mirrors the dot to the asset's right, [3, 4) × 1.5 = [4, 6);
    // its offset of 1 then moves it left by 2.
    let turned = ("turned", ValueSource::Bool(true));
    assert_eq!(
        rock(&baked(&level, &placement(vec![("a", &a)], vec![copy((2, 0), std::slice::from_ref(&turned))]))),
        [6, 7]
    );
    let l = baked(
        &level,
        &placement(vec![("a", &a)], vec![copy((2, 0), &[turned, ("dot_x", ValueSource::Int(1))])]),
    );
    assert_eq!(rock(&l), [4, 5]);
}

/// A wall asset (background, 3 × 1) and a pane asset (background, 1 × 1)
/// whose opacity is exposed as `alpha`.
fn wall_and_pane() -> (Asset, Asset) {
    let wall = Asset::new(
        "Wall",
        1,
        (3, 1),
        vec![piece("body", ElementMaterial::Background, (0, 0), (3, 1), 1, |_, _| WALL)],
        vec![],
        vec![],
    );
    let mut pane = Asset::new(
        "Pane",
        1,
        (1, 1),
        vec![piece("body", ElementMaterial::Background, (0, 0), (1, 1), 1, |_, _| PANE)],
        vec![],
        vec![],
    );
    pane.exposed = vec![ExposedSource {
        name: "alpha".into(),
        property: PropertySource::Opacity("body".into()),
        default: None,
        min: None,
        max: None,
    }];
    (wall, pane)
}

#[test]
fn opacity_mixes_a_background_piece_over_the_back_below() {
    let (wall, pane) = wall_and_pane();
    let level = blank((4, 1), 1, None);
    let with = |alpha: Option<i32>| {
        let mut c = PlacedCopy::new("pane", "b", (1, 0));
        if let Some(a) = alpha {
            c.values.insert("alpha".into(), ValueSource::Int(a));
        }
        baked(
            &level,
            &placement(vec![("a", &wall), ("b", &pane)], vec![PlacedCopy::new("wall", "a", (0, 0)), c]),
        )
    };
    // 40 % is alpha 102: straight alpha over the wall, rounded.
    let mix = |s: u8, d: u8| {
        ((u32::from(s) * 102 * 255 + u32::from(d) * 255 * 153 + 255 * 255 / 2) / (255 * 255)) as u8
    };
    let l = with(Some(40));
    assert_eq!(l.back.get(1, 0), Rgba::rgb(mix(PANE.r, WALL.r), mix(PANE.g, WALL.g), mix(PANE.b, WALL.b)));
    assert_eq!(l.front.get(1, 0), l.back.get(1, 0));
    // 0 bakes nothing; 100 bakes as an opaque piece does.
    let wall_only = baked(&level, &placement(vec![("a", &wall)], vec![PlacedCopy::new("wall", "a", (0, 0))]));
    assert_eq!(with(Some(0)), wall_only);
    assert_eq!(with(Some(100)), with(None));
    assert_eq!(with(None).back.get(1, 0), PANE);
    // Never over a see-through pixel: with a distant sky, the pane beside
    // the wall leaves the back see-through.
    let sky = {
        let mut p = piece("sky", ElementMaterial::Background, (0, 0), (1, 1), 1, |_, _| BLUE);
        p.element.glow = None;
        Asset::new("Sky", 1, (1, 1), vec![p], vec![], vec![])
    };
    let mut c = PlacedCopy::new("pane", "b", (3, 0));
    c.values.insert("alpha".into(), ValueSource::Int(40));
    let far = PlacedCopy { distant: Some(50), ..PlacedCopy::new("sky", "c", (0, 0)) };
    let src = placement(
        vec![("a", &wall), ("b", &pane), ("c", &sky)],
        vec![PlacedCopy::new("wall", "a", (0, 0)), c, far],
    );
    let l = baked(&level, &src);
    assert_eq!(l.back.get(3, 0).a, 0, "see-through stays see-through");
    assert_eq!(l.back.get(0, 0), WALL);
}

#[test]
fn distant_pieces_bake_below_everything() {
    let (wall, _) = wall_and_pane();
    let sky = Asset::new(
        "Sky",
        1,
        (2, 1),
        vec![piece("sky", ElementMaterial::Background, (0, 0), (2, 1), 1, |x, _| [BLUE, GREEN][x as usize])],
        vec![],
        vec![],
    );
    let rock = Asset::new(
        "Rock",
        1,
        (1, 1),
        vec![piece("body", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED)],
        vec![],
        vec![],
    );
    // The distant copy comes last in the placement, yet lies below.
    let src = placement(
        vec![("a", &wall), ("b", &rock), ("c", &sky)],
        vec![
            PlacedCopy::new("wall", "a", (0, 0)),
            PlacedCopy::new("rock", "b", (4, 0)),
            PlacedCopy { distant: Some(30), ..PlacedCopy::new("sky", "c", (-2, 0)) },
        ],
    );
    let l = baked(&blank((6, 1), 1, None), &src);
    assert_eq!(l.distant.len(), 1);
    let d = &l.distant[0];
    assert_eq!((d.depth, d.at), (30, (-2, 0)));
    assert_eq!((d.image.get(0, 0), d.image.get(1, 0)), (BLUE, GREEN));
    assert_eq!(l.material[4], Material::Rock);
    assert_eq!(l.back.get(0, 0), WALL);
    assert_eq!(l.back.get(3, 0).a, 0, "uncovered back pixels show the distant layer");
    // Stretched, a distant copy's layer is too.
    let mut s2 = src.clone();
    s2.file.copies[2].size = Some((4, 2));
    let l = baked(&blank((6, 2), 1, None), &s2);
    assert_eq!((l.distant[0].image.width(), l.distant[0].image.height()), (4, 2));
}

/// A 1 × 1 lamp glowing in `lamps` (tinted green), its colour exposed;
/// made of `wood` (`name`) with a painted value naming it.
fn lamp(wood: &str) -> Asset {
    let mut p = piece("body", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED);
    p.element.glow = Some(ElementGlow { strength: 80, group: "lamps".into(), mask: false, beam: None });
    p.element.surface = Some("wood".into());
    p.element.surface_map = true;
    p.surface_map = Some(Arc::new(vec![1]));
    let green = Colour::parse("#20ff20").unwrap();
    let mut a = Asset::new(
        "Lamp",
        1,
        (1, 1),
        vec![p],
        vec![group("lamps", GlowColourSource::Tint(green))],
        vec![material("wood", wood)],
    );
    a.exposed = vec![ExposedSource {
        name: "colour".into(),
        property: PropertySource::GlowColour("body".into()),
        default: None,
        min: None,
        max: None,
    }];
    a
}

#[test]
fn gathering_shares_equal_groups_and_renames_others() {
    let (one, other) = (lamp("Wood"), lamp("Dark wood"));
    let red = ValueSource::Colour(Colour::parse("#ff2020").unwrap());
    let src = placement(
        vec![("a", &one), ("b", &other)],
        vec![
            PlacedCopy::new("lamp-1", "a", (0, 0)),
            PlacedCopy::new("lamp-2", "a", (1, 0)),
            PlacedCopy {
                values: [("colour".to_string(), red)].into(),
                ..PlacedCopy::new("lamp-3", "a", (2, 0))
            },
            PlacedCopy::new("lamp-4", "b", (3, 0)),
        ],
    );
    let level = blank((4, 1), 1, None);
    let mut l = level.clone();
    let b = bake(&mut l, &src, &tile()).unwrap();
    let look = l.look.as_ref().unwrap();
    let ids = |v: Vec<&String>| v.into_iter().cloned().collect::<Vec<_>>();
    assert_eq!(ids(look.glow_groups.iter().map(|g| &g.id).collect()), ["lamps", "lamps-2"]);
    assert_eq!(look.glow_groups[1].colour, GlowColourSource::Tint(Colour::parse("#ff2020").unwrap()));
    assert_eq!(ids(look.materials.iter().map(|m| &m.id).collect()), ["wood", "wood-2"]);
    assert_eq!(look.materials[1].name, "Dark wood");
    let renamed =
        |copy: &str, from: &str, to: &str| Renamed { copy: copy.into(), from: from.into(), to: to.into() };
    assert_eq!(b.groups_renamed, [renamed("lamp-3", "lamps", "lamps-2")]);
    assert_eq!(b.materials_renamed, [renamed("lamp-4", "wood", "wood-2")]);
    assert_eq!((b.groups_added.len(), b.materials_added.len()), (2, 2));
    // The pieces follow: groups 1, 1, 2, 1 and materials 1, 1, 1, 2.
    let glow = l.glow.as_ref().unwrap();
    assert_eq!(glow.groups, [1, 1, 2, 1]);
    assert_eq!(l.surface.as_ref().unwrap().front, [1, 1, 1, 2]);
    // A look added for a level without one, with the message.
    assert!(b.added_look);
    assert_eq!(b.note(), Some(ADDED_LIGHTING));
    assert_eq!(look.ambient, LookSource::default().ambient);
    // A level's own look keeps its settings; its groups are the gathered.
    let own =
        LookSource { haze: 30, glow_groups: vec![group("old", GlowColourSource::Own)], ..Default::default() };
    let mut l = blank((4, 1), 1, Some(own));
    let b = bake(&mut l, &src, &tile()).unwrap();
    assert!(!b.added_look && b.note().is_none());
    let look = l.look.unwrap();
    assert_eq!(look.haze, 30);
    assert_eq!(look.glow_groups.len(), 2);
}

/// A 2 × 1 asset of a mixed layer, one cell dirt, with two states: it
/// drives something, and `check_placed` cannot see the dirt cell.
#[test]
fn a_live_copy_with_dirt_cells_is_refused_by_the_bake() {
    let mut p = piece("body", ElementMaterial::Rock, (0, 0), (2, 1), 1, |_, _| RED);
    p.element.kind = ElementKind::Drawn {
        material: ElementMaterial::Rock,
        at: (0, 0),
        size: Some((2, 1)),
        mixed: true,
        back: false,
    };
    p.decoded = Decoded::Drawn {
        art: {
            let mut i = RgbaImage::new(2, 1);
            i.set(0, 0, RED);
            i.set(1, 0, GREEN);
            i
        },
        cells: Some(vec![Material::Rock, Material::Dirt]),
        back: None,
    };
    let mut a = Asset::new("Crate", 1, (2, 1), vec![p], vec![], vec![]);
    a.states.push(State::new("Open"));
    let live = PlacedCopy { live: true, ..PlacedCopy::new("crate", "a", (0, 0)) };
    let src = placement(vec![("a", &a)], vec![live]);
    let level = blank((4, 1), 1, None);
    check_placed(&src, level.size, level.art_scale).unwrap();
    let mut l = level.clone();
    match bake(&mut l, &src, &tile()) {
        Err(e @ PlacedError::Copy { .. }) => {
            let e = e.to_string();
            assert!(e.contains("crate") && e.contains("dirt"), "{e}");
        }
        other => panic!("{other:?}"),
    }
    // Static, it bakes, its dirt cell dirt.
    let mut src = src;
    src.file.copies[0].live = false;
    let l = baked(&level, &src);
    assert_eq!(&l.material[..2], [Material::Rock, Material::Dirt]);
}

#[test]
fn live_copies_bake_as_static_for_now() {
    let a = lamp("Wood");
    let copy = PlacedCopy::new("lamp", "a", (1, 0));
    let level = blank((3, 1), 1, None);
    let still = baked(&level, &placement(vec![("a", &a)], vec![copy.clone()]));
    let live = baked(&level, &placement(vec![("a", &a)], vec![PlacedCopy { live: true, ..copy }]));
    assert_eq!(still, live);
    assert_eq!(live.material[1], Material::Rock);
    // Its groups count: a level lit only by a Live copy gets a look.
    assert!(live.look.is_some());
}
