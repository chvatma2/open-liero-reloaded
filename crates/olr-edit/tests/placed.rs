//! Issue #73: a level of placed copies baked (`olr_edit::placed`): a
//! ground alone bakes as its composition, stretched and rescaled copies
//! resample nearest, positions scale with the copy, opacity mixes, distant
//! pieces lie below, the gathering shares and renames, Live copies bake
//! static and refuse dirt cells.

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
        live: None,
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
    Pack::new(manifest, Source::Level(level))
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

// Issue #74: packs of placed copies, the conversion and the refusal.

/// Every file of the pack at `dir` by path, its bytes.
fn on_disk(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let fs = olr_vfs::DirFs::new(dir);
    olr_vfs::walk(&fs, &olr_vfs::VPath::root())
        .unwrap()
        .into_iter()
        .map(|p| (p.as_str().to_string(), olr_vfs::ContentSource::read(&fs, &p).unwrap()))
        .collect()
}

fn outside_editor(
    files: &std::collections::BTreeMap<String, Vec<u8>>,
) -> std::collections::BTreeMap<String, Vec<u8>> {
    files.iter().filter(|(p, _)| !p.starts_with("editor/")).map(|(p, b)| (p.clone(), b.clone())).collect()
}

/// A level of OLR 0.2 written into `dir` from the kept composition types:
/// a raster base, a distant glowing sky, a drawn wall with a painted
/// surface map, a glass pane and a glowing image, with `level.ron`'s
/// glow groups and materials; baked by `compose::bake` as 0.2 did.
fn write_02_level(dir: &std::path::Path) {
    let look = LookSource {
        glow_groups: vec![group("sky", GlowColourSource::Own), group("lamps", GlowColourSource::Own)],
        materials: vec![material("stone", "Stone"), material("wood", "Wood")],
        ..Default::default()
    };
    let mut level = blank((12, 8), 2, Some(look));
    for (i, m) in level.material.iter_mut().enumerate() {
        *m = if i >= 12 * 6 { Material::Rock } else { Material::Background };
    }
    for y in 0..16 {
        for x in 0..24 {
            let c = Rgba::rgb(x as u8 * 9, y as u8 * 13, 40);
            level.front.set(x, y, c);
            level.back.set(x, y, if y >= 12 { Rgba::rgb(10, x as u8 * 7, y as u8 * 5) } else { c });
        }
    }
    let mut p = pack(level);
    ensure_composed(&mut p).unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Sky").unwrap();
    draw(&mut p, "sky", &Shape::Rect { x: 0, y: 0, w: 12, h: 3 }, &Paint::Colour(BLUE), None).unwrap();
    set(&mut p, "sky", Property::Distant(Some(80))).unwrap();
    set(
        &mut p,
        "sky",
        Property::Glow(Some(ElementGlow { strength: 40, group: "sky".into(), mask: false, beam: None })),
    )
    .unwrap();
    add_layer(&mut p, ElementMaterial::Rock, "Wall").unwrap();
    draw(&mut p, "wall", &Shape::Rect { x: 2, y: 1, w: 3, h: 4 }, &Paint::Colour(WALL), None).unwrap();
    set(&mut p, "wall", Property::Surface(Some("stone".into()))).unwrap();
    let mut map = vec![0u8; 12 * 8];
    map[12 * 2 + 3] = 2;
    set_surface_map(&mut p, "wall", false, Some(&map)).unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Pane").unwrap();
    draw(&mut p, "pane", &Shape::Rect { x: 6, y: 1, w: 4, h: 2 }, &Paint::Colour(PANE), None).unwrap();
    set(&mut p, "pane", Property::Glass(Some(ElementGlass { opacity: 35, tint: 40, frost: 5 }))).unwrap();
    let image = png(5, 3, |x, y| Rgba::rgb(x as u8 * 50, y as u8 * 80, 120));
    let id = add_image(&mut p, "sign.png", image, (5, 4), (4, 2), ElementMaterial::Rock, "Sign").unwrap();
    set(
        &mut p,
        &id,
        Property::Glow(Some(ElementGlow { strength: 60, group: "lamps".into(), mask: false, beam: None })),
    )
    .unwrap();
    let src = p.composition.clone().unwrap();
    let Source::Level(l) = &mut p.source else { panic!() };
    compose::bake(l, &src, &tile()).unwrap();
    assert!(l.distant.len() == 1 && l.glass.is_some() && l.surface.is_some() && l.glow.is_some());
    // The 0.2 pack: its files, its composition beside them.
    let mut fs = olr_vfs::DirFs::new(dir);
    olr_pack::write_manifest(&mut fs, &p.manifest).unwrap();
    p.source.write(&mut fs).unwrap();
    olr_format::levels::write_composition_files(&mut fs, &src).unwrap();
}

#[test]
fn converting_a_02_level_keeps_its_bake() {
    use olr_edit::placed::{Conversion, bake_pack, from_composition};
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("old");
    std::fs::create_dir(&dir).unwrap();
    write_02_level(&dir);
    let before = on_disk(&dir);
    let composition =
        olr_format::levels::read_composition_files(&olr_vfs::DirFs::new(&dir)).unwrap().unwrap();
    let mut p = olr_edit::pack::load(&dir).unwrap();
    assert!(p.legacy_composition && p.composition.is_none() && p.placed.is_none());
    assert_eq!(from_composition(&mut p).unwrap(), Conversion::Converted);
    assert!(!p.legacy_composition);
    bake_pack(&mut p, &tile()).unwrap();
    olr_edit::pack::save(&dir, &p).unwrap();
    let after = on_disk(&dir);
    assert!(outside_editor(&before) == outside_editor(&after), "the baked files changed");
    assert!(!after.contains_key("editor/composition.ron"));
    for (path, bytes) in &composition.files {
        assert_eq!(after.get(path), Some(bytes), "{path} kept its path and bytes");
    }
    // The ground: the composition's elements, the look's groups and
    // materials, placed alone.
    let p = olr_edit::pack::load(&dir).unwrap();
    let placed = p.placed.as_ref().unwrap();
    let ground = &placed.assets[GROUND_KEY];
    assert_eq!(ground.file.name, "Ground");
    assert_eq!(ground.file.elements, composition.composition.elements);
    let look = level_of(&p).look.clone().unwrap();
    assert_eq!((&ground.file.glow_groups, &ground.file.materials), (&look.glow_groups, &look.materials));
    assert_eq!(placed.file.copies.len(), 1);
    assert!(placed.file.copies[0].locked);
    assert_eq!(p.composition.as_ref().map(|c| &c.composition), Some(&composition.composition));
    // A second conversion has nothing to do; a plain level neither.
    let mut again = p.clone();
    assert_eq!(from_composition(&mut again).unwrap(), Conversion::Already);
    let mut plain = pack(blank((4, 4), 1, None));
    assert_eq!(from_composition(&mut plain).unwrap(), Conversion::Plain);
    assert_eq!(olr_pack::validate(&olr_vfs::DirFs::new(&dir)), vec![]);
}

#[test]
fn a_02_pack_is_refused_by_ensure_composed() {
    let tmp = tempfile::tempdir().unwrap();
    write_02_level(tmp.path());
    let before = on_disk(tmp.path());
    let mut p = olr_edit::pack::load(tmp.path()).unwrap();
    let loaded = p.clone();
    let refused = || ElementError::NotTheGround(olr_edit::pack::UNCONVERTED);
    assert_eq!(ensure_composed(&mut p), Err(refused()));
    assert!(refused().to_string().contains("run olr-cli level convert"), "{}", refused());
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "X").map(|_| ()), Err(refused()));
    assert!(matches!(
        olr_edit::placed::bake_pack(&mut p, &tile()),
        Err(PlacedError::Level(m)) if m == olr_edit::pack::UNCONVERTED
    ));
    assert!(olr_edit::asset::DecodedLevel::of_pack(&p).is_err());
    assert!(p == loaded, "the pack changed");
    assert!(on_disk(tmp.path()) == before, "a file changed");
}

/// A level of its ground alone, saved into `dir` and loaded again.
fn saved_ground(dir: &std::path::Path, look: Option<LookSource>) -> Pack {
    let mut p = pack(blank((8, 4), 1, look));
    ensure_composed(&mut p).unwrap();
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    olr_edit::pack::save(dir, &p).unwrap();
    olr_edit::pack::load(dir).unwrap()
}

#[test]
fn save_writes_unedited_copies_verbatim() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = saved_ground(tmp.path(), None);
    let mut stripes = placement(vec![("a", &stripes())], vec![PlacedCopy::new("lamp", "a", (2, 1))]);
    let placed = p.placed.as_mut().unwrap();
    placed.file.assets.extend(stripes.file.assets);
    placed.file.copies.append(&mut stripes.file.copies);
    placed.assets.append(&mut stripes.assets);
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    olr_edit::pack::save(tmp.path(), &p).unwrap();
    // `asset.ron` with its own spacing, saved through an edit elsewhere.
    let file = tmp.path().join("editor/a/asset.ron");
    let odd = std::fs::read_to_string(&file).unwrap().replace("    ", "  ").replace("),", "),\n");
    std::fs::write(&file, &odd).unwrap();
    let mut p = olr_edit::pack::load(tmp.path()).unwrap();
    let Source::Level(l) = &mut p.source else { panic!() };
    l.name = "Renamed".into();
    olr_edit::pack::save(tmp.path(), &p).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), odd, "the unedited copy is as it was");
    // An edited copy is written canonically.
    let mut p = olr_edit::pack::load(tmp.path()).unwrap();
    p.placed.as_mut().unwrap().assets.get_mut("a").unwrap().file.name = "Stripes 2".into();
    olr_edit::pack::save(tmp.path(), &p).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text != odd && text.contains("Stripes 2") && text.contains("    "), "{text}");
}

#[test]
fn a_ground_edit_lands_in_the_ground_asset() {
    let tmp = tempfile::tempdir().unwrap();
    let look = LookSource { glow_groups: vec![group("lamps", GlowColourSource::Own)], ..Default::default() };
    let mut p = saved_ground(tmp.path(), Some(look));
    add_layer(&mut p, ElementMaterial::Rock, "Crate").unwrap();
    draw(&mut p, "crate", &Shape::Rect { x: 1, y: 1, w: 2, h: 2 }, &Paint::Colour(WALL), None).unwrap();
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    olr_edit::pack::save(tmp.path(), &p).unwrap();
    let ground = std::fs::read_to_string(tmp.path().join("editor/asset.ron")).unwrap();
    assert!(ground.contains("id: \"crate\""), "{ground}");
    assert!(tmp.path().join("editor/layers/crate.png").is_file());
    assert!(!tmp.path().join("editor/composition.ron").exists());
    // A group edited in the look goes to the ground, and the bake gathers
    // it back into level.ron.
    let mut p = olr_edit::pack::load(tmp.path()).unwrap();
    let Source::Level(l) = &mut p.source else { panic!() };
    let mut g = l.look.as_ref().unwrap().glow_groups[0].clone();
    g.intensity = 777;
    olr_edit::look::set_group(l, "lamps", g).unwrap();
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    olr_edit::pack::save(tmp.path(), &p).unwrap();
    let p = olr_edit::pack::load(tmp.path()).unwrap();
    assert_eq!(p.placed.as_ref().unwrap().assets[GROUND_KEY].file.glow_groups[0].intensity, 777);
    assert_eq!(level_of(&p).look.as_ref().unwrap().glow_groups[0].intensity, 777);
    assert_eq!(olr_edit::placed::stale(&p, &tile()).unwrap(), Vec::<String>::new());
}

#[test]
fn stale_lists_level_ron_when_its_groups_drift() {
    let tmp = tempfile::tempdir().unwrap();
    let look = LookSource { glow_groups: vec![group("lamps", GlowColourSource::Own)], ..Default::default() };
    let p = saved_ground(tmp.path(), Some(look));
    assert_eq!(olr_edit::placed::stale(&p, &tile()).unwrap(), Vec::<String>::new());
    // The ground's group changed by hand: level.ron's no longer gathers.
    let file = tmp.path().join("editor/asset.ron");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.replace("intensity: 300", "intensity: 301")).unwrap();
    let p = olr_edit::pack::load(tmp.path()).unwrap();
    assert_eq!(olr_edit::placed::stale(&p, &tile()).unwrap(), ["level.ron"]);
    // So does a material added to the ground alone.
    let mut p = saved_ground(tmp.path(), None);
    p.placed.as_mut().unwrap().assets.get_mut(GROUND_KEY).unwrap().file.materials =
        vec![material("wood", "Wood")];
    assert_eq!(olr_edit::placed::stale(&p, &tile()).unwrap(), ["level.ron"]);
}

#[test]
fn the_editor_scope_is_the_ground_alone() {
    use olr_edit::pack::{NOT_THE_GROUND_ALONE, ground_composition};
    let tmp = tempfile::tempdir().unwrap();
    let mut p = saved_ground(tmp.path(), None);
    let ground = p.placed.as_ref().unwrap().assets[GROUND_KEY].clone();
    assert_eq!(p.composition, Some(ground_composition(&ground)));
    assert_eq!(p.ground_refusal(), None);
    // A lamp asset bringing its own glow group, placed beside the ground.
    let mut lamp = stripes();
    lamp.pieces[0].element.glow =
        Some(ElementGlow { strength: 50, group: "lamp".into(), mask: false, beam: None });
    lamp.groups = vec![group("lamp", GlowColourSource::Own)];
    let mut more = placement(vec![("a", &lamp)], vec![PlacedCopy::new("lamp-1", "a", (2, 1))]);
    let placed = p.placed.as_mut().unwrap();
    placed.file.assets.append(&mut more.file.assets);
    placed.file.copies.append(&mut more.file.copies);
    placed.assets.append(&mut more.assets);
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    olr_edit::pack::save(tmp.path(), &p).unwrap();
    let mut p = olr_edit::pack::load(tmp.path()).unwrap();
    assert_eq!(p.composition, None);
    assert_eq!(p.ground_refusal(), Some(NOT_THE_GROUND_ALONE));
    assert_eq!(ensure_composed(&mut p), Err(ElementError::NotTheGround(NOT_THE_GROUND_ALONE)));
    assert_eq!(level_of(&p).look.as_ref().unwrap().glow_groups[0].id, "lamp");
    // The lamp's group is its asset's, named by the copy; a ground group is
    // the ground's, edited through `edit_ground`, then gathered back.
    assert_eq!(olr_edit::placed::brought_by(&p, "lamp", false).as_deref(), Some("lamp-1"));
    let message = olr_edit::placed::brought_by_message("glow group", "lamp", "lamp-1");
    assert!(message.contains("it comes with copy lamp-1's asset: edit that asset"), "{message}");
    olr_edit::placed::edit_ground(&mut p, |p| {
        let Source::Level(l) = &mut p.source else { panic!() };
        olr_edit::look::add_group(l, group("ground-glow", GlowColourSource::Own)).map(|_| ())
    })
    .unwrap();
    assert_eq!(olr_edit::placed::brought_by(&p, "ground-glow", false), None);
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    let ids: Vec<&str> =
        level_of(&p).look.as_ref().unwrap().glow_groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["ground-glow", "lamp"]);
    let ground = &p.placed.as_ref().unwrap().assets[GROUND_KEY];
    assert_eq!(ground.file.glow_groups.len(), 1);
}

/// Hostile packs (the anti-goals): a 0.2 composition beside a placement,
/// a placement without its ground, a ground whose files are missing: each
/// loads or is an error, never a panic, and none is taken for plain.
#[test]
fn hostile_placed_levels_never_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let p = saved_ground(dir, None);
    // A leftover composition beside the placement: kept as it is, unread.
    std::fs::write(dir.join("editor/composition.ron"), "(elements: [])\n").unwrap();
    let both = olr_edit::pack::load(dir).unwrap();
    assert!(!both.legacy_composition && both.composition == p.composition);
    assert!(both.other.keys().any(|k| k.as_str() == "editor/composition.ron"));
    std::fs::remove_file(dir.join("editor/composition.ron")).unwrap();
    // A ground whose layer is gone.
    let layer = dir.join("editor/layers/base.front.png");
    let bytes = std::fs::read(&layer).unwrap();
    std::fs::remove_file(&layer).unwrap();
    assert!(olr_edit::pack::load(dir).is_err());
    std::fs::write(&layer, bytes).unwrap();
    // A placement without its ground.
    std::fs::remove_file(dir.join("editor/asset.ron")).unwrap();
    assert!(olr_edit::pack::load(dir).is_err());
    // An unreadable composition is refused by the conversion, the pack
    // unchanged.
    let tmp = tempfile::tempdir().unwrap();
    write_02_level(tmp.path());
    std::fs::remove_file(tmp.path().join("editor/layers/wall.png")).unwrap();
    let mut p = olr_edit::pack::load(tmp.path()).unwrap();
    let loaded = p.clone();
    assert!(olr_edit::placed::from_composition(&mut p).is_err());
    assert!(p == loaded);
}

// Issue #75: the operations on a level's placed copies.

use olr_edit::placed::{
    CopyProperty as C, Size, add_copy, duplicate_copy, keep_aspect, lower_copy, move_copy, raise_copy,
    remove_copy, resize_copy, set_copies, set_copy, set_copy_script, set_level_script,
};

/// A composed 32 × 16 level (its plain bake the ground's raster base,
/// placed alone and locked).
fn composed() -> Pack {
    let mut p = pack(blank((32, 16), 1, None));
    ensure_composed(&mut p).unwrap();
    p
}

fn placed_of(p: &Pack) -> &PlacedSource {
    p.placed.as_ref().unwrap()
}

fn yours(id: &str) -> AssetOrigin {
    AssetOrigin::Yours(id.into())
}

fn add(p: &mut Pack, a: &AssetSource, at: (i32, i32)) -> olr_edit::placed::Added {
    add_copy(p, a, yours("lamp"), at, None, None).unwrap()
}

/// A rock box of `size`, red, named `name`.
fn boxed(name: &str, size: (u32, u32)) -> Asset {
    let p = piece("body", ElementMaterial::Rock, (0, 0), size, 1, |_, _| RED);
    Asset::new(name, 1, size, vec![p], vec![], vec![])
}

/// `edit` refused on `p`: the pack unchanged; the error's text.
fn refused<T: std::fmt::Debug>(
    p: &mut Pack,
    edit: impl FnOnce(&mut Pack) -> Result<T, PlacedError>,
) -> String {
    let before = p.clone();
    let e = edit(p).expect_err("refused");
    assert!(*p == before, "a refusal changed the pack: {e}");
    e.to_string()
}

#[test]
fn adding_the_same_asset_twice_reuses_it() {
    let mut p = composed();
    let lamp = encode(&lamp("Wood")).unwrap();
    let one = add(&mut p, &lamp, (2, 2));
    assert_eq!((one.id.as_str(), one.key.as_str(), one.new_asset), ("lamp", "a", true));
    assert_eq!(one.groups_added, ["lamps"]);
    assert_eq!(one.materials_added, ["wood"]);
    assert!(one.added_look);
    let two = add(&mut p, &lamp, (5, 2));
    assert_eq!((two.id.as_str(), two.key.as_str(), two.new_asset), ("lamp-2", "a", false));
    assert_eq!(placed_of(&p).file.assets.len(), 2, "the ground and the lamp");
    // Another thumbnail is the same asset.
    let mut thumb = lamp.clone();
    thumb.thumb = Some(png(2, 2, |_, _| BLUE));
    assert_eq!(add(&mut p, &thumb, (8, 2)).key, "a");
    // Another layer file is not.
    let mut changed = lamp.clone();
    changed.files.insert("layers/body.png".into(), png(1, 1, |_, _| GREEN));
    let three = add(&mut p, &changed, (11, 2));
    assert_eq!((three.key.as_str(), three.new_asset), ("b", true));
    // Nor is the same files from elsewhere.
    let builtin = add_copy(&mut p, &lamp, AssetOrigin::Builtin("lamp".into()), (14, 2), None, None).unwrap();
    assert_eq!(builtin.key, "c");
    // A lamp placed again after the bake brings no group.
    olr_edit::placed::bake_pack(&mut p, &tile()).unwrap();
    let again = add(&mut p, &lamp, (17, 2));
    assert!(again.groups_added.is_empty() && again.materials_added.is_empty() && !again.added_look);
}

#[test]
fn keys_are_generated_short_and_chosen_keys_are_checked() {
    let mut p = composed();
    let a = |n: &str| encode(&boxed(n, (2, 2))).unwrap();
    assert_eq!(add(&mut p, &a("Crate"), (0, 0)).key, "a");
    let chosen = add_copy(&mut p, &a("Barrel"), yours("barrel"), (0, 0), None, Some("b")).unwrap();
    assert_eq!(chosen.key, "b");
    assert_eq!(add_copy(&mut p, &a("Box"), yours("box"), (0, 0), None, Some("bx")).unwrap().key, "bx");
    // The first free: b is taken.
    assert_eq!(add(&mut p, &a("Drum"), (0, 0)).key, "c");
    for key in ["a", "layers", "ground", "Bad!", "-x", "seventeen-bytes-x"] {
        let e = refused(&mut p, |p| add_copy(p, &a("Other"), yours("other"), (0, 0), None, Some(key)));
        assert!(e.contains(key), "{key}: {e}");
    }
    // A reused asset takes no key.
    let e = refused(&mut p, |p| add_copy(p, &a("Crate"), yours("lamp"), (0, 0), None, Some("z")));
    assert!(e.contains("in the level already"), "{e}");
    // Ids number from the asset's name.
    assert_eq!(add(&mut p, &a("Crate"), (0, 0)).id, "crate-2");
    assert_eq!(add(&mut p, &a("Crate 2"), (0, 0)).id, "crate-2-2");
    assert_eq!(add(&mut p, &a("3D box"), (0, 0)).id, "copy-3d-box");
    let id = add(&mut p, &a("A very long name of a crate that goes on and on"), (0, 0)).id;
    assert!(id.len() <= 32 && id.starts_with("a-very-long"), "{id}");
    let e = refused(&mut p, |p| add_copy(p, &a("Crate"), yours("lamp"), (0, 0), Some("crate"), None));
    assert!(e.contains("taken"), "{e}");
    let e = refused(&mut p, |p| add_copy(p, &a("Crate"), yours("lamp"), (0, 0), Some("Crate"), None));
    assert!(e.contains("not an id"), "{e}");
    assert_eq!(add_copy(&mut p, &a("Crate"), yours("lamp"), (0, 0), Some("mine"), None).unwrap().id, "mine");
}

#[test]
fn keep_aspect_rounds_and_never_reaches_zero() {
    assert_eq!(keep_aspect((14, 16), Size::Width(7)), Some((7, 8)));
    assert_eq!(keep_aspect((14, 16), Size::Height(8)), Some((7, 8)));
    // 1.5 rounds up, 0.01 becomes 1.
    assert_eq!(keep_aspect((2, 1), Size::Width(3)), Some((3, 2)));
    assert_eq!(keep_aspect((100, 1), Size::Width(1)), Some((1, 1)));
    assert_eq!(keep_aspect((1, 100), Size::Height(1)), Some((1, 1)));
    assert_eq!(keep_aspect((3, 7), Size::Width(4096)), Some((4096, 9557)));
    assert_eq!(keep_aspect((3, 7), Size::Natural), None);
    assert_eq!(keep_aspect((3, 7), Size::Free(5, 9)), Some((5, 9)));
}

#[test]
fn free_aspect_off_snaps_the_height() {
    let mut p = composed();
    let id = add(&mut p, &encode(&boxed("Crate", (4, 2))).unwrap(), (1, 1)).id;
    let copy = |p: &Pack| placed_of(p).file.copies.iter().find(|c| c.id == id).unwrap().clone();
    assert_eq!(resize_copy(&mut p, &id, Size::Free(8, 8)).unwrap(), (8, 8));
    assert!(copy(&p).free_aspect);
    set_copy(&mut p, &id, C::FreeAspect(false)).unwrap();
    assert_eq!((copy(&p).size, copy(&p).free_aspect), (Some((8, 4)), false));
    assert_eq!(resize_copy(&mut p, &id, Size::Height(3)).unwrap(), (6, 3));
    assert_eq!(resize_copy(&mut p, &id, Size::Natural).unwrap(), (4, 2));
    assert_eq!(copy(&p).size, None);
    // Beyond the bound: refused, unchanged.
    let e = refused(&mut p, |p| resize_copy(p, &id, Size::Free(5000, 1)));
    assert!(e.contains(&id), "{e}");
}

#[test]
fn live_is_refused_with_the_reason() {
    let mut p = composed();
    // Two states and a Dirt layer.
    let mut dirt = boxed("Mud", (2, 1));
    dirt.pieces[0].element.kind = ElementKind::Drawn {
        material: ElementMaterial::Dirt,
        at: (0, 0),
        size: Some((2, 1)),
        mixed: false,
        back: false,
    };
    dirt.states.push(State::new("Wet"));
    let id = add(&mut p, &encode(&dirt).unwrap(), (0, 0)).id;
    let e = refused(&mut p, |p| set_copy(p, &id, C::Live(true)));
    assert!(e.contains(&id) && e.contains("it holds dirt"), "{e}");
    // A mixed layer with a dirt cell.
    let mut mixed = boxed("Crate", (2, 1));
    mixed.pieces[0].element.kind = ElementKind::Drawn {
        material: ElementMaterial::Rock,
        at: (0, 0),
        size: Some((2, 1)),
        mixed: true,
        back: false,
    };
    mixed.pieces[0].decoded = Decoded::Drawn {
        art: png_image(2, 1, RED),
        cells: Some(vec![Material::Rock, Material::Dirt]),
        back: None,
    };
    mixed.states.push(State::new("Open"));
    let id = add(&mut p, &encode(&mixed).unwrap(), (0, 0)).id;
    let e = refused(&mut p, |p| set_copy(p, &id, C::Live(true)));
    assert!(e.contains("it holds dirt"), "{e}");
    // The one-state ground (a raster too): nothing to drive, said first.
    let e = refused(&mut p, |p| set_copies(p, GROUND_KEY, &[C::Locked(false), C::Live(true)]));
    assert!(e.contains(&olr_edit::asset::LiveRefusal::NothingToDrive.to_string()), "{e}");
    // A lamp drives its colour, holds rock: Live.
    let id = add(&mut p, &encode(&lamp("Wood")).unwrap(), (0, 0)).id;
    set_copy(&mut p, &id, C::Live(true)).unwrap();
    assert!(placed_of(&p).file.copies.iter().any(|c| c.id == id && c.live));
}

fn png_image(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut i = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            i.set(x, y, c);
        }
    }
    i
}

#[test]
fn renaming_a_copy_moves_its_script() {
    let mut p = composed();
    let id = add(&mut p, &encode(&lamp("Wood")).unwrap(), (0, 0)).id;
    set_copy_script(&mut p, &id, Some("print(\"lamp\");\n".into())).unwrap();
    set_copy(&mut p, &id, C::Id("porch".into())).unwrap();
    let src = placed_of(&p);
    assert_eq!(src.scripts.keys().collect::<Vec<_>>(), ["porch"]);
    assert!(src.file.copies.iter().any(|c| c.id == "porch" && c.script));
    let files = olr_edit::pack::files(&p).unwrap();
    assert!(files.keys().any(|k| k.as_str() == "editor/scripts/porch.rhai"));
    assert!(!files.keys().any(|k| k.as_str() == "editor/scripts/lamp.rhai"));
    // Taken or not an id: refused.
    let e = refused(&mut p, |p| set_copy(p, "porch", C::Id(GROUND_KEY.into())));
    assert!(e.contains("taken"), "{e}");
    let e = refused(&mut p, |p| set_copy(p, "porch", C::Id("9lives".into())));
    assert!(e.contains("not an id"), "{e}");
    // Scripts: bounded, cleared; the level's too.
    let big = "x".repeat(olr_format::assets::MAX_SCRIPT_BYTES as usize + 1);
    refused(&mut p, |p| set_copy_script(p, "porch", Some(big.clone())));
    refused(&mut p, |p| set_level_script(p, Some(big.clone())));
    set_level_script(&mut p, Some("// level\n".into())).unwrap();
    assert!(placed_of(&p).file.script);
    set_copy_script(&mut p, "porch", None).unwrap();
    assert!(placed_of(&p).scripts.is_empty());
}

#[test]
fn removing_the_last_copy_removes_its_asset() {
    let mut p = composed();
    let lamp = encode(&lamp("Wood")).unwrap();
    let one = add(&mut p, &lamp, (0, 0)).id;
    let two = add(&mut p, &lamp, (3, 0)).id;
    set_copy_script(&mut p, &two, Some("// two\n".into())).unwrap();
    assert!(p.composition.is_none(), "more than the ground");
    let r = remove_copy(&mut p, &two).unwrap();
    assert_eq!(
        (r.copy.id.as_str(), r.index, r.script.as_deref(), r.asset.is_none()),
        (two.as_str(), 2, Some("// two\n"), true)
    );
    let r = remove_copy(&mut p, &one).unwrap();
    assert_eq!(r.asset.as_ref().map(|(a, _)| a.key.as_str()), Some("a"));
    let src = placed_of(&p);
    assert!(!src.assets.contains_key("a") && src.file.assets.len() == 1);
    // The ground alone again: edited as a composition, the look the
    // ground's.
    assert!(p.composition.is_some());
    // The ground, once unlocked, goes with its only copy.
    let e = refused(&mut p, |p| remove_copy(p, GROUND_KEY));
    assert!(e.contains("locked"), "{e}");
    set_copy(&mut p, GROUND_KEY, C::Locked(false)).unwrap();
    let r = remove_copy(&mut p, GROUND_KEY).unwrap();
    assert_eq!(r.asset.map(|(a, _)| a.key), Some(GROUND_KEY.to_string()));
    let src = placed_of(&p);
    assert!(src.file.copies.is_empty() && src.file.assets.is_empty() && src.assets.is_empty());
    check_placed(src, (32, 16), 1).unwrap();
}

#[test]
fn a_locked_copy_refuses_moves_resizes_sets_and_removal() {
    let mut p = composed();
    let id = add(&mut p, &encode(&lamp("Wood")).unwrap(), (0, 0)).id;
    set_copy(&mut p, &id, C::Locked(true)).unwrap();
    for e in [
        refused(&mut p, |p| move_copy(p, &id, (4, 4))),
        refused(&mut p, |p| resize_copy(p, &id, Size::Width(3))),
        refused(&mut p, |p| set_copy(p, &id, C::Live(true))),
        refused(&mut p, |p| set_copy(p, &id, C::Id("other".into()))),
        refused(&mut p, |p| set_copy(p, &id, C::Distant(Some(50)))),
        refused(&mut p, |p| set_copy_script(p, &id, Some(String::new()))),
        refused(&mut p, |p| remove_copy(p, &id)),
    ] {
        assert!(e.contains("locked") && e.contains(&id), "{e}");
    }
    // Its order is not its content.
    lower_copy(&mut p, &id).unwrap();
    raise_copy(&mut p, &id).unwrap();
    // Unlocked first, locked last, in one set.
    set_copies(&mut p, &id, &[C::Locked(true), C::Id("porch".into()), C::Locked(false)]).unwrap();
    set_copies(&mut p, "porch", &[C::Locked(true), C::Live(true), C::Locked(false)]).unwrap();
    let c = placed_of(&p).file.copies.iter().find(|c| c.id == "porch").unwrap();
    assert!(c.live && c.locked);
}

#[test]
fn duplicate_copy_lands_above_with_a_new_id() {
    let mut p = composed();
    let lamp = add(&mut p, &encode(&lamp("Wood")).unwrap(), (2, 3)).id;
    add(&mut p, &encode(&stripes()).unwrap(), (0, 0));
    let red = ValueSource::Colour(Colour::parse("#ff2020").unwrap());
    set_copies(
        &mut p,
        &lamp,
        &[C::Value("colour".into(), Some(red.clone())), C::Live(true), C::Locked(true)],
    )
    .unwrap();
    set_copies(&mut p, &lamp, &[C::Locked(false)]).unwrap();
    set_copy_script(&mut p, &lamp, Some("// mine\n".into())).unwrap();
    resize_copy(&mut p, &lamp, Size::Free(3, 2)).unwrap();
    let new = duplicate_copy(&mut p, &lamp, (4, -1)).unwrap();
    assert_eq!(new, "lamp-2");
    let src = placed_of(&p);
    let ids: Vec<&str> = src.file.copies.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, [GROUND_KEY, "lamp", "lamp-2", "stripes"]);
    let (a, b) = (&src.file.copies[1], &src.file.copies[2]);
    assert_eq!(b.at, (6, 2));
    assert_eq!(PlacedCopy { id: a.id.clone(), at: a.at, ..b.clone() }, *a);
    assert_eq!(src.scripts.get("lamp-2").map(String::as_str), Some("// mine\n"));
    assert_eq!(duplicate_copy(&mut p, "lamp-2", (0, 0)).unwrap(), "lamp-3");
    // The ground's raster stays at the origin: refused.
    set_copy(&mut p, GROUND_KEY, C::Locked(false)).unwrap();
    refused(&mut p, |p| duplicate_copy(p, GROUND_KEY, (1, 0)));
}

/// One operation of the property test, on a copy picked by index.
#[derive(Clone, Debug)]
enum Op {
    Add(usize, (i32, i32), Option<&'static str>),
    Move(usize, (i32, i32)),
    Resize(usize, Size),
    Set(usize, C),
    Script(usize, bool),
    Duplicate(usize, (i32, i32)),
    Raise(usize),
    Lower(usize),
    Remove(usize),
    LevelScript(bool),
}

fn op() -> impl proptest::strategy::Strategy<Value = Op> {
    use proptest::prelude::*;
    let at = (-20000i32..20000, -20000i32..20000).prop_map(|(x, y)| (x / 16, y / 16));
    let far = prop_oneof![at.clone(), Just((20000, 0))];
    let side = prop_oneof![0u32..12, Just(5000u32)];
    let size = prop_oneof![
        Just(Size::Natural),
        side.clone().prop_map(Size::Width),
        side.clone().prop_map(Size::Height),
        (side.clone(), side).prop_map(|(w, h)| Size::Free(w, h)),
    ];
    let names = prop_oneof![Just("porch"), Just("ground"), Just("Bad"), Just("lamp")];
    let property = prop_oneof![
        names.prop_map(|n: &str| C::Id(n.into())),
        any::<bool>().prop_map(C::Live),
        prop_oneof![Just(None), Just(Some("Wet".to_string())), Just(Some("Nope".to_string()))]
            .prop_map(C::State),
        prop_oneof![
            Just(C::Value("colour".into(), Some(ValueSource::Colour(Colour::parse("#ff0000").unwrap())))),
            Just(C::Value("colour".into(), Some(ValueSource::Int(3)))),
            Just(C::Value("nothing".into(), Some(ValueSource::Bool(true)))),
            Just(C::Value("colour".into(), None)),
        ],
        any::<bool>().prop_map(C::DefaultScript),
        prop_oneof![Just(None), Just(Some(0u8)), Just(Some(40u8)), Just(Some(120u8))].prop_map(C::Distant),
        any::<bool>().prop_map(C::FreeAspect),
        any::<bool>().prop_map(C::Locked),
    ];
    let key = prop_oneof![Just(None), Just(Some("k")), Just(Some("layers"))];
    prop_oneof![
        (0usize..4, far.clone(), key).prop_map(|(a, at, k)| Op::Add(a, at, k)),
        (0usize..8, far.clone()).prop_map(|(i, at)| Op::Move(i, at)),
        (0usize..8, size).prop_map(|(i, s)| Op::Resize(i, s)),
        (0usize..8, property).prop_map(|(i, p)| Op::Set(i, p)),
        (0usize..8, any::<bool>()).prop_map(|(i, s)| Op::Script(i, s)),
        (0usize..8, at).prop_map(|(i, o)| Op::Duplicate(i, o)),
        (0usize..8).prop_map(Op::Raise),
        (0usize..8).prop_map(Op::Lower),
        (0usize..8).prop_map(Op::Remove),
        any::<bool>().prop_map(Op::LevelScript),
    ]
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]
    /// Every operation leaves a placement that checks, or changes nothing.
    #[test]
    fn every_operation_leaves_a_checked_placement(ops in proptest::collection::vec(op(), 1..16)) {
        let mut wet = boxed("Mud", (3, 2));
        wet.states.push(State::new("Wet"));
        let assets = [
            encode(&lamp("Wood")).unwrap(),
            encode(&stripes()).unwrap(),
            encode(&wet).unwrap(),
            encode(&lamp("Dark wood")).unwrap(),
        ];
        let mut p = composed();
        for op in ops {
            let ids: Vec<String> = p.placed.as_ref().map(|s| s.file.copies.iter().map(|c| c.id.clone()).collect()).unwrap_or_default();
            let id = |i: usize| ids.get(i % ids.len().max(1)).cloned().unwrap_or_else(|| "none".into());
            let before = p.clone();
            let ok = match op.clone() {
                Op::Add(a, at, key) => add_copy(&mut p, &assets[a], yours("x"), at, None, key).map(drop),
                Op::Move(i, at) => move_copy(&mut p, &id(i), at),
                Op::Resize(i, s) => resize_copy(&mut p, &id(i), s).map(drop),
                Op::Set(i, c) => set_copy(&mut p, &id(i), c),
                Op::Script(i, on) => set_copy_script(&mut p, &id(i), on.then(|| "// s\n".to_string())),
                Op::Duplicate(i, o) => duplicate_copy(&mut p, &id(i), o).map(drop),
                Op::Raise(i) => raise_copy(&mut p, &id(i)),
                Op::Lower(i) => lower_copy(&mut p, &id(i)),
                Op::Remove(i) => remove_copy(&mut p, &id(i)).map(drop),
                Op::LevelScript(on) => set_level_script(&mut p, on.then(|| "// l\n".to_string())),
            };
            match ok {
                Ok(()) => {
                    let src = olr_edit::pack::placement(&p).unwrap();
                    proptest::prop_assert!(check_placed(&src, (32, 16), 1).is_ok(), "{op:?}");
                    proptest::prop_assert!(olr_edit::pack::files(&p).is_ok(), "{op:?}");
                }
                Err(e) => proptest::prop_assert!(p == before, "{op:?} refused ({e}) but changed the pack"),
            }
        }
    }
}
