//! Issue #85: Live copies baked to their played form (`olr_edit::live`,
//! through `placed::bake`): out of the ground, parts by target, runs,
//! shared pictures, stretched positions, complete states, unscaled glow,
//! the refusals and the file cap, the default look.

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::{Asset, LiveRefusal, OwnPicture, Piece, State, encode, live_refusal};
use olr_edit::compose::Decoded;
use olr_edit::pack::{Pack, Source};
use olr_edit::placed::{CopyProperty, bake, bake_pack, set_copy};
use olr_format::Colour;
use olr_format::assets::{ExposedSource, PropertySource, StateValue, ValueSource};
use olr_format::levels::{
    AssetOrigin, Element, ElementGlow, ElementKind, ElementMaterial, LevelAsset, LevelSource, LiveSource,
    PartKind, PlacedCopy, PlacedSource, Target, glow_value, read_level_files, write_level_files,
};
use olr_format::look::{GlowColourSource, GlowGroupSource, ReachSource};
use olr_vfs::MemFs;

const RED: Rgba = Rgba::rgb(200, 20, 20);
const GREEN: Rgba = Rgba::rgb(20, 200, 20);
const BLUE: Rgba = Rgba::rgb(20, 20, 200);

fn blank(size: (u32, u32), s: u32) -> LevelSource {
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
    }
}

fn tile() -> RgbaImage {
    let mut t = RgbaImage::new(1, 1);
    t.set(0, 0, Rgba::rgb(9, 8, 7));
    t
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

/// A drawn layer of `material` at `at` of `size` (art scale `s`), its
/// art `art(x, y)`.
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

fn exposed(name: &str, property: PropertySource, default: Option<ValueSource>) -> ExposedSource {
    ExposedSource { name: name.into(), property, default, min: None, max: None }
}

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

fn live(id: &str, key: &str, at: (i32, i32)) -> PlacedCopy {
    PlacedCopy { live: true, ..PlacedCopy::new(id, key, at) }
}

fn played(l: &LevelSource) -> &LiveSource {
    l.live.as_ref().expect("a played form")
}

/// A one-pixel red rock lamp glowing at 80 % in its own green group, its
/// colour exposed.
fn lamp() -> Asset {
    let mut p = piece("body", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED);
    p.element.glow = Some(ElementGlow { strength: 80, group: "lamps".into(), mask: false, beam: None });
    let green = Colour::parse("#20ff20").unwrap();
    let mut a =
        Asset::new("Lamp", 1, (1, 1), vec![p], vec![group("lamps", GlowColourSource::Tint(green))], vec![]);
    a.exposed = vec![exposed("colour", PropertySource::GlowColour("body".into()), None)];
    a
}

/// The level written and read back: its files check.
fn round_trip(l: &LevelSource) -> LevelSource {
    let mut fs = MemFs::new();
    write_level_files(&mut fs, l).unwrap();
    read_level_files(&fs).unwrap()
}

#[test]
fn a_live_copy_bakes_to_the_played_form_not_the_ground() {
    let level = blank((3, 1), 1);
    let src = placement(vec![("a", &lamp())], vec![live("lamp", "a", (1, 0))]);
    let l = baked(&level, &src);
    // Not in the ground.
    assert_eq!(l.material, vec![Material::Background; 3]);
    assert!(l.glow.is_none());
    // In the played form.
    let p = played(&l);
    assert_eq!(p.file.copies.len(), 1);
    let c = &p.file.copies[0];
    assert_eq!((c.id.as_str(), c.asset.as_str(), c.at, c.size), ("lamp", "Lamp", (1, 0), (1, 1)));
    assert_eq!(c.props.len(), 1);
    assert_eq!(c.props[0].name.as_deref(), Some("colour"));
    assert_eq!(c.props[0].drives, Target::GlowColour(0));
    assert_eq!(c.props[0].default, 0x20ff20);
    assert_eq!(c.groups.len(), 1);
    assert_eq!(c.states.len(), 1);
    let part = c.states[0].parts[0];
    assert_eq!((part.layer, part.kind, part.at, part.glow_strength), (Some(0), PartKind::Rock, (0, 0), 80));
    let pic = &p.pictures[0][usize::from(part.picture)];
    assert_eq!(pic.art.get(0, 0), RED);
    assert_eq!(pic.rock.as_deref(), Some(&[255][..]));
    assert_eq!(pic.glow.as_deref(), Some(&[255][..]), "unscaled: the strength is the part's");
    assert_eq!(pic.groups.as_deref(), Some(&[1][..]));
    // The thumbnail shows it as placed.
    let mut still = src.clone();
    still.file.copies[0].live = false;
    assert_eq!(l.thumb, baked(&level, &still).thumb);
    // Its files check and read back.
    assert_eq!(round_trip(&l).live, l.live);
}

#[test]
fn a_static_copy_still_bakes_into_the_ground() {
    let level = blank((3, 1), 1);
    let src = placement(vec![("a", &lamp())], vec![PlacedCopy::new("lamp", "a", (1, 0))]);
    let l = baked(&level, &src);
    assert_eq!(l.material[1], Material::Rock);
    assert_eq!(l.front.get(1, 0), RED);
    assert!(l.live.is_none());
    assert!(round_trip(&l).live.is_none());
}

/// Two rock layers, `a` below `b`, of one red and one green pixel.
fn two_layers() -> Asset {
    let a = piece("a", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED);
    let b = piece("b", ElementMaterial::Rock, (1, 0), (1, 1), 1, |_, _| GREEN);
    Asset::new("Pair", 1, (2, 1), vec![a, b], vec![], vec![])
}

#[test]
fn a_state_value_on_an_unexposed_layer_makes_it_a_part() {
    let mut a = two_layers();
    let mut off = State::new("Off");
    off.values = vec![StateValue {
        property: PropertySource::LayerVisible("b".into()),
        value: ValueSource::Bool(false),
    }];
    a.states.push(off);
    let l = baked(&blank((4, 2), 1), &placement(vec![("a", &a)], vec![live("pair", "a", (0, 0))]));
    let c = &played(&l).file.copies[0];
    // `a` untargeted, a run of its own; `b` targeted, its own part.
    let parts: Vec<_> = c.states[0].parts.iter().map(|p| (p.layer, p.kind, p.at)).collect();
    assert_eq!(parts, vec![(None, PartKind::Rock, (0, 0)), (Some(1), PartKind::Rock, (1, 0))]);
    assert_eq!(c.props.len(), 1);
    let v = &c.props[0];
    assert_eq!(
        (v.name.as_ref(), v.drives, v.min, v.max, v.default),
        (None, Target::LayerVisible(1), 0, 1, 1)
    );
    assert_eq!(c.states[0].values, vec![(0, 1)]);
    assert_eq!(c.states[1].values, vec![(0, 0)]);
    assert_eq!(round_trip(&l).live, l.live);
}

#[test]
fn a_targeted_mixed_layer_gives_two_parts_driven_together() {
    let mut p = piece("m", ElementMaterial::Background, (0, 0), (2, 1), 1, |x, _| [GREEN, RED][x as usize]);
    if let ElementKind::Drawn { mixed, .. } = &mut p.element.kind {
        *mixed = true;
    }
    if let Decoded::Drawn { cells, .. } = &mut p.decoded {
        *cells = Some(vec![Material::Background, Material::Rock]);
    }
    let mut a = Asset::new("Mixed", 1, (2, 1), vec![p], vec![], vec![]);
    a.exposed = vec![ExposedSource {
        name: "slide".into(),
        property: PropertySource::OffsetX("m".into()),
        default: None,
        min: Some(-1),
        max: Some(1),
    }];
    let l = baked(&blank((4, 1), 1), &placement(vec![("a", &a)], vec![live("m", "a", (1, 0))]));
    let p = played(&l);
    let c = &p.file.copies[0];
    let parts = &c.states[0].parts;
    assert_eq!(
        parts.iter().map(|p| (p.layer, p.kind)).collect::<Vec<_>>(),
        vec![(Some(0), PartKind::Background), (Some(0), PartKind::Rock)]
    );
    assert_eq!(c.props[0].drives, Target::OffsetX(0));
    let back = &p.pictures[0][usize::from(parts[0].picture)];
    let rock = &p.pictures[0][usize::from(parts[1].picture)];
    assert!(back.rock.is_none());
    assert_eq!(back.art.get(0, 0), GREEN);
    assert_eq!(rock.rock.as_deref(), Some(&[0, 255][..]));
    assert_eq!((rock.art.get(0, 0).a, rock.art.get(1, 0)), (0, RED));
    assert_eq!(round_trip(&l).live, l.live);
}

#[test]
fn untargeted_layers_flatten_in_runs_keeping_the_stack() {
    let r1 = piece("r1", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED);
    let r2 = piece("r2", ElementMaterial::Rock, (1, 0), (1, 1), 1, |_, _| GREEN);
    let b1 = piece("b1", ElementMaterial::Background, (0, 0), (2, 1), 1, |_, _| BLUE);
    let r3 = piece("r3", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| BLUE);
    let mut hidden = piece("hidden", ElementMaterial::Rock, (1, 0), (1, 1), 1, |_, _| RED);
    hidden.element.visible = false;
    let t = piece("t", ElementMaterial::Rock, (1, 0), (1, 1), 1, |_, _| GREEN);
    let r4 = piece("r4", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| GREEN);
    let mut a = Asset::new("Stack", 1, (2, 1), vec![r1, r2, b1, r3, hidden, t, r4], vec![], vec![]);
    a.exposed = vec![exposed("t_on", PropertySource::LayerVisible("t".into()), None)];
    let l = baked(&blank((2, 1), 1), &placement(vec![("a", &a)], vec![live("s", "a", (0, 0))]));
    let p = played(&l);
    let c = &p.file.copies[0];
    let parts: Vec<_> = c.states[0].parts.iter().map(|p| (p.layer, p.kind)).collect();
    assert_eq!(
        parts,
        vec![
            (None, PartKind::Rock),
            (None, PartKind::Background),
            (None, PartKind::Rock),
            (Some(5), PartKind::Rock),
            (None, PartKind::Rock),
        ]
    );
    // The first run is r1 and r2 together.
    let run = &p.pictures[0][usize::from(c.states[0].parts[0].picture)];
    assert_eq!((run.art.get(0, 0), run.art.get(1, 0)), (RED, GREEN));
    // The hidden layer is in no run.
    let r3 = &p.pictures[0][usize::from(c.states[0].parts[2].picture)];
    assert_eq!(r3.art.width(), 1);
}

#[test]
fn a_state_shares_its_unpainted_pictures() {
    let mut a = two_layers();
    a.exposed = vec![exposed("b_on", PropertySource::LayerVisible("b".into()), None)];
    let mut lit = State::new("Lit");
    let mut art = RgbaImage::new(1, 1);
    art.set(0, 0, BLUE);
    lit.own.insert(
        "b".into(),
        OwnPicture { decoded: Decoded::Drawn { art, cells: None, back: None }, glow_mask: None },
    );
    a.states.push(lit);
    a.states.push(State::new("Dark"));
    let l = baked(&blank((2, 1), 1), &placement(vec![("a", &a)], vec![live("p", "a", (0, 0))]));
    let p = played(&l);
    let c = &p.file.copies[0];
    let pics = |k: usize| c.states[k].parts.iter().map(|p| p.picture).collect::<Vec<_>>();
    assert_eq!(pics(0), pics(2), "Dark paints nothing: Normal's pictures");
    assert_eq!(pics(1)[0], pics(0)[0], "Lit shares the layer it does not paint");
    assert_ne!(pics(1)[1], pics(0)[1]);
    assert_eq!(c.pictures.len(), 3);
    assert_eq!(p.pictures[0][usize::from(pics(1)[1])].art.get(0, 0), BLUE);
}

#[test]
fn a_stretched_copy_scales_every_position_value() {
    // 4 wide placed 6 wide: 1.5 ×, rounded half away from zero.
    let p = piece("arm", ElementMaterial::Rock, (1, 0), (2, 2), 1, |_, _| RED);
    let mut a = Asset::new("Arm", 1, (4, 2), vec![p], vec![], vec![]);
    a.exposed = vec![
        ExposedSource {
            name: "x".into(),
            property: PropertySource::X,
            default: Some(ValueSource::Int(-1)),
            min: Some(-4),
            max: Some(4),
        },
        ExposedSource {
            name: "reach".into(),
            property: PropertySource::OffsetX("arm".into()),
            default: Some(ValueSource::Int(1)),
            min: Some(-2),
            max: Some(2),
        },
    ];
    let mut far = State::new("Far");
    far.values =
        vec![StateValue { property: PropertySource::OffsetX("arm".into()), value: ValueSource::Int(2) }];
    a.states.push(far);
    let mut copy = live("arm", "a", (0, 0));
    copy.size = Some((6, 2));
    copy.free_aspect = true;
    copy.values = BTreeMap::from([("x".to_string(), ValueSource::Int(3))]);
    let l = baked(&blank((8, 2), 1), &placement(vec![("a", &a)], vec![copy]));
    let c = &played(&l).file.copies[0];
    let x = &c.props[0];
    assert_eq!((x.drives, x.min, x.max, x.default), (Target::X, -6, 6, -2));
    let reach = &c.props[1];
    assert_eq!((reach.drives, reach.min, reach.max, reach.default), (Target::OffsetX(0), -3, 3, 2));
    // States: Normal the default, Far its value, both scaled.
    assert_eq!(c.states[0].values, vec![(1, 2)]);
    assert_eq!(c.states[1].values, vec![(1, 3)]);
    // The copy's own value, which no state sets.
    assert_eq!(c.start_values, vec![(0, 5)]);
    // The part's box: [1, 3) of 4 is [1, 4) of 6.
    let part = c.states[0].parts[0];
    assert_eq!(part.at, (1, 0));
    assert_eq!(c.pictures[usize::from(part.picture)].size, (3, 2));
    assert_eq!(round_trip(&l).live, l.live);
}

#[test]
fn each_state_is_a_complete_look() {
    let mut p = piece("glow", ElementMaterial::Rock, (0, 0), (1, 1), 1, |_, _| RED);
    p.element.glow = Some(ElementGlow { strength: 50, group: "g".into(), mask: false, beam: None });
    let mut a = Asset::new("Light", 1, (1, 1), vec![p], vec![group("g", GlowColourSource::Own)], vec![]);
    a.exposed = vec![exposed("power", PropertySource::GlowStrength("glow".into()), None)];
    let mut dim = State::new("Dim");
    dim.values = vec![StateValue {
        property: PropertySource::GlowStrength("glow".into()),
        value: ValueSource::Int(10),
    }];
    let mut off = State::new("Off");
    off.values = vec![StateValue {
        property: PropertySource::LayerVisible("glow".into()),
        value: ValueSource::Bool(false),
    }];
    a.states.extend([dim, off]);
    let mut copy = live("l", "a", (0, 0));
    copy.values = BTreeMap::from([("power".to_string(), ValueSource::Int(70))]);
    let l = baked(&blank((1, 1), 1), &placement(vec![("a", &a)], vec![copy]));
    let c = &played(&l).file.copies[0];
    let props: Vec<_> = c.props.iter().map(|p| (p.drives, p.default)).collect();
    assert_eq!(props, vec![(Target::GlowStrength(0), 50), (Target::LayerVisible(0), 1)]);
    // Every state sets both: its own value, else the copy's, else the
    // default.
    let values: Vec<_> = c.states.iter().map(|s| s.values.clone()).collect();
    assert_eq!(values, vec![vec![(0, 70), (1, 1)], vec![(0, 10), (1, 1)], vec![(0, 70), (1, 0)]]);
    assert!(c.start_values.is_empty());
}

#[test]
fn glow_strength_is_mask_times_value_as_posed() {
    let s = 2;
    let mut t = piece("t", ElementMaterial::Rock, (0, 0), (2, 1), s, |_, _| RED);
    t.element.glow = Some(ElementGlow { strength: 60, group: "g".into(), mask: true, beam: None });
    t.glow_mask = Some(Arc::new((0..8).map(|i| i * 30).collect()));
    let mut run = piece("run", ElementMaterial::Rock, (0, 1), (2, 1), s, |_, _| GREEN);
    run.element.glow = Some(ElementGlow { strength: 40, group: "g".into(), mask: true, beam: None });
    run.glow_mask = Some(Arc::new((0..8).map(|i| 255 - i * 20).collect()));
    let mut a = Asset::new("Glow", s, (2, 2), vec![t, run], vec![group("g", GlowColourSource::Own)], vec![]);
    a.exposed = vec![exposed("power", PropertySource::GlowStrength("t".into()), None)];
    let level = blank((2, 2), s);
    let l = baked(&level, &placement(vec![("a", &a)], vec![live("g", "a", (0, 0))]));
    let still = baked(&level, &placement(vec![("a", &a)], vec![PlacedCopy::new("g", "a", (0, 0))]));
    let front = &still.glow.as_ref().unwrap().front;
    let p = played(&l);
    let c = &p.file.copies[0];
    // The run (the bottom layer `run` at y 1) and `t`'s part (y 0).
    let parts = &c.states[0].parts;
    assert_eq!(
        parts.iter().map(|p| (p.layer, p.glow_strength)).collect::<Vec<_>>(),
        vec![(Some(0), 60), (None, 100)]
    );
    for part in parts {
        let pic = &p.pictures[0][usize::from(part.picture)];
        let mask = pic.glow.as_ref().unwrap();
        let aw = pic.art.width();
        for (i, m) in mask.iter().enumerate() {
            let (ax, ay) = (i as u32 % aw + part.at.0 as u32 * s, i as u32 / aw + part.at.1 as u32 * s);
            let shown = glow_value(part.glow_strength, *m);
            assert_eq!(shown, front[(ay * 4 + ax) as usize], "art pixel ({ax}, {ay})");
        }
    }
    assert!(parts.iter().all(|p| p.at.0 == 0));
}

#[test]
fn live_refusal_names_each_limit() {
    // A layer outside the box.
    let mut a = two_layers();
    a.pieces.push(piece("far", ElementMaterial::Rock, (5, 5), (1, 1), 1, |_, _| RED));
    a.states.push(State::new("Off"));
    let why = live_refusal(&a).unwrap();
    assert_eq!(why, LiveRefusal::Outside("far".into()));
    assert!(why.to_string().contains("\"far\""), "{why}");
    // 33 runs.
    let kinds = [ElementMaterial::Rock, ElementMaterial::Background];
    let layers = |n: usize| -> Vec<Piece> {
        (0..n).map(|i| piece(&format!("l{i}"), kinds[i % 2], (0, 0), (1, 1), 1, |_, _| RED)).collect()
    };
    let mut a = Asset::new("Many", 1, (1, 1), layers(33), vec![], vec![]);
    a.states.push(State::new("Off"));
    assert_eq!(live_refusal(&a), Some(LiveRefusal::TooManyParts(33)));
    assert!(live_refusal(&a).unwrap().to_string().contains("32"));
    // 5 runs, each painted by 31 more states: 160 pictures.
    let mut a = Asset::new("Painted", 1, (1, 1), layers(5), vec![], vec![]);
    for k in 1..32 {
        let mut st = State::new(&format!("S{k}"));
        for p in &a.pieces {
            let own = OwnPicture { decoded: p.decoded.clone(), glow_mask: None };
            st.own.insert(p.element.id.clone(), own);
        }
        a.states.push(st);
    }
    assert_eq!(live_refusal(&a), Some(LiveRefusal::TooManyPictures(160)));
    assert!(live_refusal(&a).unwrap().to_string().contains("128"));
    // 25 background layers with 4 values each, set by a state: 100.
    let mut a = Asset::new(
        "Values",
        1,
        (1, 1),
        (0..25)
            .map(|i| piece(&format!("b{i}"), ElementMaterial::Background, (0, 0), (1, 1), 1, |_, _| RED))
            .collect(),
        vec![],
        vec![],
    );
    let mut st = State::new("Set");
    for i in 0..25 {
        let id = format!("b{i}");
        st.values.extend([
            StateValue { property: PropertySource::OffsetX(id.clone()), value: ValueSource::Int(0) },
            StateValue { property: PropertySource::OffsetY(id.clone()), value: ValueSource::Int(0) },
            StateValue { property: PropertySource::LayerVisible(id.clone()), value: ValueSource::Bool(true) },
            StateValue { property: PropertySource::Opacity(id), value: ValueSource::Int(50) },
        ]);
    }
    a.states.push(st);
    assert_eq!(live_refusal(&a), Some(LiveRefusal::TooManyValues(100)));
    assert!(live_refusal(&a).unwrap().to_string().contains("96"));
}

fn manifest() -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: "t".into(),
        name: "T".into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    }
}

#[test]
fn a_level_past_the_file_cap_is_refused_at_save() {
    // Four glowing targeted layers painted by 31 more states: 128
    // pictures of 4 files each, 512 files a copy.
    let mut pieces = Vec::new();
    for i in 0..4 {
        let mut p = piece(&format!("l{i}"), ElementMaterial::Rock, (i, 0), (1, 1), 1, |_, _| RED);
        p.element.glow = Some(ElementGlow { strength: 50, group: "g".into(), mask: false, beam: None });
        pieces.push(p);
    }
    let mut a = Asset::new("Big", 1, (4, 1), pieces, vec![group("g", GlowColourSource::Own)], vec![]);
    a.exposed =
        (0..4).map(|i| exposed(&format!("x{i}"), PropertySource::OffsetX(format!("l{i}")), None)).collect();
    for x in &mut a.exposed {
        (x.min, x.max) = (Some(0), Some(0));
    }
    for k in 1..32 {
        let mut st = State::new(&format!("S{k}"));
        for p in &a.pieces {
            st.own.insert(p.element.id.clone(), OwnPicture { decoded: p.decoded.clone(), glow_mask: None });
        }
        a.states.push(st);
    }
    assert_eq!(live_refusal(&a), None);
    let mut copies: Vec<PlacedCopy> = (0..4).map(|i| live(&format!("c{i}"), "a", (0, i))).collect();
    copies[3].live = false;
    let mut pack = Pack::new(manifest(), Source::Level(blank((4, 4), 1)));
    pack.placed = Some(placement(vec![("a", &a)], copies));
    // Three copies bake: 1537 files.
    let n = bake_pack(&mut pack, &tile()).unwrap().map(|_| ());
    assert_eq!(n, Some(()));
    let Source::Level(l) = &pack.source else { unreachable!() };
    assert_eq!(olr_format::levels::live_paths(&played(l).file).len(), 1 + 3 * 512);
    // The fourth's Live switch is refused, naming the cap.
    let e = set_copy(&mut pack, "c3", CopyProperty::Live(true)).unwrap_err().to_string();
    assert!(e.contains("c3") && e.contains("cannot be Live") && e.contains("2048"), "{e}");
    // A placement Live past it (a hand-made file) is refused by the bake.
    pack.placed.as_mut().unwrap().file.copies[3].live = true;
    let e = bake_pack(&mut pack, &tile()).unwrap_err().to_string();
    assert!(e.contains("2048"), "{e}");
}

#[test]
fn a_level_lit_only_by_live_copies_gets_the_default_look() {
    let level = blank((3, 1), 1);
    let l = baked(&level, &placement(vec![("a", &lamp())], vec![live("lamp", "a", (1, 0))]));
    let look = l.look.as_ref().expect("a look for the live lamp");
    assert!(look.glow_groups.is_empty(), "the copy's groups are its own");
    assert_eq!(played(&l).file.copies[0].groups.len(), 1);
    // Without a glowing live copy, no look.
    let mut dark = lamp();
    dark.pieces[0].element.glow = None;
    dark.groups.clear();
    dark.exposed = vec![exposed("x", PropertySource::X, None)];
    dark.exposed[0].min = Some(0);
    dark.exposed[0].max = Some(0);
    let l = baked(&level, &placement(vec![("a", &dark)], vec![live("lamp", "a", (1, 0))]));
    assert!(l.look.is_none());
    assert!(l.live.is_some());
}
