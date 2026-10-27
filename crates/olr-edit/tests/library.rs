//! Issue #76: a level's library links compared, updated, restored,
//! adopted and relinked (`olr_edit::library`), and levels exported and
//! imported with their assets.

use std::collections::BTreeSet;
use std::path::Path;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::asset::{
    Asset, MAX_USER_ASSETS, Piece, State, add_sound, encode, rename_state, save_dir, unexpose,
};
use olr_edit::compose::Decoded;
use olr_edit::element::ensure_composed;
use olr_edit::library::{
    self, Break, ImportedAsset, Libraries, LibraryError, Link, Places, Resolution, Status,
};
use olr_edit::pack::{self, Pack, Source};
use olr_edit::placed::{self, CopyProperty};
use olr_format::assets::{AssetSource, ExposedSource, PropertySource, ValueSource};
use olr_format::levels::{AssetOrigin, Element, ElementKind, ElementMaterial, LevelSource};

use crate::common::tree;

const RED: Rgba = Rgba::rgb(200, 20, 20);
const GREEN: Rgba = Rgba::rgb(20, 200, 20);
const BLUE: Rgba = Rgba::rgb(20, 20, 200);

fn tile() -> RgbaImage {
    let mut t = RgbaImage::new(2, 2);
    t.set(0, 0, Rgba::rgb(9, 8, 7));
    t.set(1, 1, Rgba::rgb(70, 80, 90));
    t
}

fn back_tile(_: &str) -> Result<RgbaImage, String> {
    Ok(tile())
}

fn level_pack(id: &str, name: &str) -> Pack {
    let size = (32, 16);
    let level = LevelSource {
        name: name.into(),
        notes: String::new(),
        size,
        art_scale: 1,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Background; (size.0 * size.1) as usize],
        front: RgbaImage::new(size.0, size.1),
        back: RgbaImage::new(size.0, size.1),
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
    };
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: name.into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    };
    let mut p = Pack::new(manifest, Source::Level(level));
    ensure_composed(&mut p).unwrap();
    p
}

/// A drawn layer `id` of `material` at `at`, filled with `colour`.
fn piece(id: &str, material: ElementMaterial, at: (i32, i32), size: (u32, u32), colour: Rgba) -> Piece {
    let mut img = RgbaImage::new(size.0, size.1);
    for y in 0..size.1 {
        for x in 0..size.0 {
            img.set(x, y, colour);
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

/// A crane: a rock body and claw, states Closed and Open, its claw's x
/// exposed as `claw_x`.
fn crane() -> Asset {
    let body = piece("body", ElementMaterial::Rock, (0, 0), (4, 2), RED);
    let claw = piece("claw", ElementMaterial::Rock, (0, 2), (1, 1), GREEN);
    let mut a = Asset::new("Crane", 1, (4, 3), vec![body, claw], vec![], vec![]);
    a.states = vec![State::new("Closed"), State::new("Open")];
    a.exposed = vec![ExposedSource {
        name: "claw_x".into(),
        property: PropertySource::OffsetX("claw".into()),
        default: None,
        min: Some(0),
        max: Some(3),
    }];
    a
}

/// A background strip, distant-able.
fn strip(colour: Rgba) -> Asset {
    let p = piece("strip", ElementMaterial::Background, (0, 0), (6, 1), colour);
    Asset::new("Strip light", 1, (6, 1), vec![p], vec![], vec![])
}

/// A console of `colour`.
fn console(colour: Rgba) -> Asset {
    let p = piece("screen", ElementMaterial::Rock, (0, 0), (2, 2), colour);
    Asset::new("Console", 1, (2, 2), vec![p], vec![], vec![])
}

/// `asset` with its first piece's pixel (0, 0) painted `c`.
fn painted(mut asset: Asset, c: Rgba) -> Asset {
    if let Decoded::Drawn { art, .. } = &mut asset.pieces[0].decoded {
        art.set(0, 0, c);
    }
    asset
}

fn add(p: &mut Pack, src: &AssetSource, origin: AssetOrigin, at: (i32, i32), id: &str) {
    placed::add_copy(p, src, origin, at, Some(id), None).unwrap();
}

fn yours(f: &str) -> AssetOrigin {
    AssetOrigin::Yours(f.into())
}

fn builtin(f: &str) -> AssetOrigin {
    AssetOrigin::Builtin(f.into())
}

fn links(p: &Pack, libs: &Libraries) -> Vec<Link> {
    library::compare(&pack::placement(p).unwrap(), libs)
}

fn status_of(links: &[Link], key: &str) -> Status {
    links.iter().find(|l| l.key == key).unwrap().status.clone()
}

/// A level placing a live crane (state Open, `claw_x` 2) of the user's,
/// a strip light of the built-in library and a console of the user's.
struct Fixture {
    pack: Pack,
    crane: AssetSource,
    strip: AssetSource,
    console: AssetSource,
}

fn fixture() -> Fixture {
    let (crane, strip, console) =
        (encode(&crane()).unwrap(), encode(&strip(BLUE)).unwrap(), encode(&console(RED)).unwrap());
    let mut p = level_pack("castle", "Castle");
    add(&mut p, &crane, yours("crane"), (2, 2), "crane");
    placed::set_copies(
        &mut p,
        "crane",
        &[
            CopyProperty::Live(true),
            CopyProperty::State(Some("Open".into())),
            CopyProperty::Value("claw_x".into(), Some(ValueSource::Int(2))),
        ],
    )
    .unwrap();
    add(&mut p, &strip, builtin("strip-light"), (10, 0), "strip");
    placed::set_copy(&mut p, "strip", CopyProperty::Distant(Some(50))).unwrap();
    add(&mut p, &console, yours("console"), (20, 4), "console");
    Fixture { pack: p, crane, strip, console }
}

fn key_of(p: &Pack, copy: &str) -> String {
    pack::placement(p).unwrap().file.copies.iter().find(|c| c.id == copy).unwrap().asset.clone()
}

fn libs(entries: Vec<(AssetOrigin, Option<AssetSource>)>) -> Libraries {
    entries.into_iter().collect()
}

#[test]
fn compare_reports_same_changed_and_missing() {
    let f = fixture();
    let changed = encode(&painted(crane(), BLUE)).unwrap();
    let l = libs(vec![
        (yours("crane"), Some(changed)),
        (builtin("strip-light"), Some(f.strip.clone())),
        (yours("console"), None),
    ]);
    let got = links(&f.pack, &l);
    assert_eq!(got.len(), 3, "the ground is its own: {got:?}");
    assert_eq!(
        got.iter().map(|l| l.origin.clone()).collect::<Vec<_>>(),
        [yours("crane"), builtin("strip-light"), yours("console")]
    );
    assert_eq!(status_of(&got, &key_of(&f.pack, "crane")), Status::Changed { breaks: vec![] });
    assert_eq!(status_of(&got, &key_of(&f.pack, "strip")), Status::Same);
    assert_eq!(status_of(&got, &key_of(&f.pack, "console")), Status::Missing);
    // Absent from the map: missing too.
    let got = links(&f.pack, &Libraries::new());
    assert!(got.iter().all(|l| l.status == Status::Missing), "{got:?}");
}

#[test]
fn compare_reports_breaks_by_copy() {
    let f = fixture();
    let crane_key = key_of(&f.pack, "crane");
    // The exposed property removed.
    let mut a = crane();
    unexpose(&mut a, "claw_x").unwrap();
    let l = libs(vec![(yours("crane"), Some(encode(&a).unwrap()))]);
    let Status::Changed { breaks } = status_of(&links(&f.pack, &l), &crane_key) else { panic!() };
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert_eq!(breaks[0].to_string(), "crane: value claw_x: no exposed property claw_x");
    // The starting state renamed.
    let mut a = crane();
    rename_state(&mut a, "Open", "Opened").unwrap();
    let l = libs(vec![(yours("crane"), Some(encode(&a).unwrap()))]);
    let Status::Changed { breaks } = status_of(&links(&f.pack, &l), &crane_key) else { panic!() };
    assert_eq!(breaks, [Break::State { copy: "crane".into(), state: "Open".into() }]);
    // Dirt painted into the live copy's asset.
    let mut a = crane();
    if let ElementKind::Drawn { material, .. } = &mut a.pieces[0].element.kind {
        *material = ElementMaterial::Dirt;
    }
    let l = libs(vec![(yours("crane"), Some(encode(&a).unwrap()))]);
    let Status::Changed { breaks } = status_of(&links(&f.pack, &l), &crane_key) else { panic!() };
    assert!(
        matches!(&breaks[..], [Break::Live { copy, message }] if copy == "crane" && message.contains("dirt")),
        "{breaks:?}"
    );
    // The distant strip turned to rock.
    let mut s = strip(BLUE);
    if let ElementKind::Drawn { material, .. } = &mut s.pieces[0].element.kind {
        *material = ElementMaterial::Rock;
    }
    let l = libs(vec![(builtin("strip-light"), Some(encode(&s).unwrap()))]);
    let Status::Changed { breaks } = status_of(&links(&f.pack, &l), &key_of(&f.pack, "strip")) else {
        panic!()
    };
    assert!(matches!(&breaks[..], [Break::Distant { copy, .. }] if copy == "strip"), "{breaks:?}");
    // A size change is no break.
    let mut big = crane();
    big.size = (5, 3);
    let l = libs(vec![(yours("crane"), Some(encode(&big).unwrap()))]);
    assert_eq!(status_of(&links(&f.pack, &l), &crane_key), Status::Changed { breaks: vec![] });
}

#[test]
fn builtin_origins_never_read_the_users_library() {
    let f = fixture();
    let t = tempfile::tempdir().unwrap();
    let (b, u) = (t.path().join("builtin"), t.path().join("user"));
    save_dir(&b.join("strip-light"), &f.strip).unwrap();
    save_dir(&u.join("strip-light"), &encode(&strip(GREEN)).unwrap()).unwrap();
    save_dir(&u.join("crane"), &f.crane).unwrap();
    let placement = pack::placement(&f.pack).unwrap();
    let got = library::compare_from(&placement, Some(&b), Some(&u));
    assert_eq!(status_of(&got, &key_of(&f.pack, "strip")), Status::Same);
    assert_eq!(status_of(&got, &key_of(&f.pack, "crane")), Status::Same);
    // Without the built-in library: missing, never the user's.
    let got = library::compare_from(&placement, None, Some(&u));
    assert_eq!(status_of(&got, &key_of(&f.pack, "strip")), Status::Missing);
    // And a `Yours` origin is never read from the built-in library.
    let got = library::compare_from(&placement, Some(&u), None);
    assert_eq!(status_of(&got, &key_of(&f.pack, "crane")), Status::Missing);
}

#[test]
fn update_applies_non_breaking_and_refuses_breaking() {
    let mut f = fixture();
    let (crane_key, strip_key) = (key_of(&f.pack, "crane"), key_of(&f.pack, "strip"));
    let edited = encode(&painted(crane(), BLUE)).unwrap();
    let l =
        libs(vec![(yours("crane"), Some(edited.clone())), (builtin("strip-light"), Some(f.strip.clone()))]);
    let u = library::update(&mut f.pack, &[crane_key.clone(), strip_key.clone()], false, &l).unwrap();
    assert_eq!((u.keys, u.same, u.resolutions), (vec![crane_key.clone()], vec![strip_key], vec![]));
    let p = pack::placement(&f.pack).unwrap();
    assert!(placed::same_asset(&p.assets[&crane_key], &edited));
    assert_eq!(status_of(&links(&f.pack, &l), &crane_key), Status::Same);
    placed::bake_pack(&mut f.pack, &tile()).unwrap();
    // A breaking one refused, the pack unchanged.
    let mut a = crane();
    rename_state(&mut a, "Open", "Opened").unwrap();
    let l = libs(vec![(yours("crane"), Some(encode(&a).unwrap()))]);
    let before = f.pack.clone();
    match library::update(&mut f.pack, std::slice::from_ref(&crane_key), false, &l) {
        Err(e @ LibraryError::Breaks { .. }) => {
            assert!(e.to_string().contains("crane: state \"Open\""), "{e}")
        }
        other => panic!("{other:?}"),
    }
    assert!(f.pack == before);
    // A missing one and an own one are refused.
    let console = key_of(&f.pack, "console");
    let e = library::update(&mut f.pack, &[console], false, &l).unwrap_err();
    assert!(e.to_string().contains("restore"), "{e}");
    let e = library::update(&mut f.pack, &["ground".into()], false, &l).unwrap_err();
    assert!(e.to_string().contains("own"), "{e}");
    assert!(f.pack == before);
}

#[test]
fn force_lists_every_resolution() {
    let mut f = fixture();
    let (crane_key, strip_key) = (key_of(&f.pack, "crane"), key_of(&f.pack, "strip"));
    let mut a = crane();
    unexpose(&mut a, "claw_x").unwrap();
    rename_state(&mut a, "Open", "Opened").unwrap();
    if let ElementKind::Drawn { material, .. } = &mut a.pieces[0].element.kind {
        *material = ElementMaterial::Dirt;
    }
    let mut s = strip(BLUE);
    if let ElementKind::Drawn { material, .. } = &mut s.pieces[0].element.kind {
        *material = ElementMaterial::Rock;
    }
    let l = libs(vec![
        (yours("crane"), Some(encode(&a).unwrap())),
        (builtin("strip-light"), Some(encode(&s).unwrap())),
    ]);
    let u = library::update(&mut f.pack, &[crane_key.clone(), strip_key.clone()], true, &l).unwrap();
    assert_eq!(u.keys, [crane_key, strip_key]);
    let said: Vec<String> = u.resolutions.iter().map(|r| r.to_string()).collect();
    assert_eq!(
        said,
        [
            "crane: dropped value claw_x",
            "crane: starts in the asset's start (no state \"Open\")",
            "crane: static now",
            "strip: not distant now",
        ]
    );
    assert!(matches!(u.resolutions[0], Resolution::DroppedValue { .. }));
    let p = pack::placement(&f.pack).unwrap();
    let c = &p.file.copies.iter().find(|c| c.id == "crane").unwrap();
    assert!(c.values.is_empty() && c.state.is_none() && !c.live);
    assert!(p.file.copies.iter().find(|c| c.id == "strip").unwrap().distant.is_none());
    placed::bake_pack(&mut f.pack, &tile()).unwrap();
    assert!(
        links(&f.pack, &l).iter().filter(|l| l.origin != yours("console")).all(|l| l.status == Status::Same)
    );
}

#[test]
fn restore_adds_from_the_levels_copy_and_relinks() {
    let mut f = fixture();
    let t = tempfile::tempdir().unwrap();
    let user = t.path().join("user");
    // The user has another asset named "Crane" in another folder.
    save_dir(&user.join("other"), &encode(&painted(crane(), BLUE)).unwrap()).unwrap();
    let crane_key = key_of(&f.pack, "crane");
    let folder = library::restore(&mut f.pack, &crane_key, &user).unwrap();
    assert_eq!(folder, "crane-2");
    let p = pack::placement(&f.pack).unwrap();
    assert_eq!(p.file.assets.iter().find(|a| a.key == crane_key).unwrap().origin, yours("crane-2"));
    assert_eq!(p.assets[&crane_key].file.name, "Crane 2");
    assert_eq!(status_of(&library::compare_from(&p, None, Some(&user)), &crane_key), Status::Same);
    // A built-in one a later version no longer ships comes back as the user's.
    let strip_key = key_of(&f.pack, "strip");
    let folder = library::restore(&mut f.pack, &strip_key, &user).unwrap();
    assert_eq!(folder, "strip-light");
    let p = pack::placement(&f.pack).unwrap();
    assert_eq!(p.file.assets.iter().find(|a| a.key == strip_key).unwrap().origin, yours("strip-light"));
    assert!(placed::same_asset(&p.assets[&strip_key], &f.strip));
    assert_eq!(status_of(&library::compare_from(&p, None, Some(&user)), &strip_key), Status::Same);
    placed::bake_pack(&mut f.pack, &tile()).unwrap();
    // The ground is its own: nothing to restore.
    assert!(library::restore(&mut f.pack, "ground", &user).is_err());
}

#[test]
fn a_rerendered_thumbnail_is_still_same() {
    let f = fixture();
    let key = key_of(&f.pack, "console");
    let mut lib = f.console.clone();
    let mut thumb = RgbaImage::new(3, 3);
    thumb.set(1, 1, GREEN);
    lib.thumb = Some(olr_format::image_io::encode_rgba(&thumb));
    assert_ne!(lib.thumb, f.console.thumb);
    let status = |lib: &AssetSource| {
        status_of(&links(&f.pack, &libs(vec![(yours("console"), Some(lib.clone()))])), &key)
    };
    assert_eq!(status(&lib), Status::Same);
    let mut scripted = lib.clone();
    scripted.script = Some("// blink\n".into());
    assert_eq!(status(&scripted), Status::Changed { breaks: vec![] });
    let mut a = console(RED);
    add_sound(&mut a, "beep.wav", wav()).unwrap();
    assert_eq!(status(&encode(&a).unwrap()), Status::Changed { breaks: vec![] });
}

/// A short silent WAV.
fn wav() -> Vec<u8> {
    let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 1, 0]);
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 8, 0]);
    b.extend_from_slice(b"data");
    b.extend_from_slice(&100u32.to_le_bytes());
    b.resize(b.len() + 100, 0);
    b
}

#[test]
fn relink_follows_the_users_copy_of_a_builtin() {
    let mut f = fixture();
    let t = tempfile::tempdir().unwrap();
    let user = t.path().join("user");
    let mine = encode(&painted(strip(BLUE), GREEN)).unwrap();
    save_dir(&user.join("strip-light-2"), &mine).unwrap();
    let key = key_of(&f.pack, "strip");
    let u =
        library::relink_from(&mut f.pack, &key, yours("strip-light-2"), None, Some(&user), false).unwrap();
    assert_eq!((u.keys, u.resolutions), (vec![key.clone()], vec![]));
    let p = pack::placement(&f.pack).unwrap();
    assert_eq!(p.file.assets.iter().find(|a| a.key == key).unwrap().origin, yours("strip-light-2"));
    assert!(placed::same_asset(&p.assets[&key], &mine));
    assert_eq!(status_of(&library::compare_from(&p, None, Some(&user)), &key), Status::Same);
    // A breaking one refused without force, then resolved with it.
    let mut rock = strip(BLUE);
    if let ElementKind::Drawn { material, .. } = &mut rock.pieces[0].element.kind {
        *material = ElementMaterial::Rock;
    }
    let rock = encode(&rock).unwrap();
    let before = f.pack.clone();
    assert!(matches!(
        library::relink(&mut f.pack, &key, yours("rock"), &rock, false),
        Err(LibraryError::Breaks { .. })
    ));
    assert!(f.pack == before);
    let u = library::relink(&mut f.pack, &key, yours("rock"), &rock, true).unwrap();
    assert_eq!(u.resolutions, [Resolution::Near { copy: "strip".into() }]);
    // Never to `Own`, never the ground.
    assert!(library::relink(&mut f.pack, &key, AssetOrigin::Own, &rock, true).is_err());
    assert!(library::relink(&mut f.pack, "ground", yours("rock"), &rock, true).is_err());
}

#[test]
fn cores_and_wrappers_agree() {
    let f = fixture();
    let t = tempfile::tempdir().unwrap();
    let (b, u) = (t.path().join("builtin"), t.path().join("user"));
    let edited = encode(&painted(crane(), BLUE)).unwrap();
    save_dir(&u.join("crane"), &edited).unwrap();
    save_dir(&b.join("strip-light"), &f.strip).unwrap();
    let p = pack::placement(&f.pack).unwrap();
    let read = library::read_libraries(&p, Some(&b), Some(&u));
    let built = libs(vec![
        (yours("crane"), Some(edited)),
        (builtin("strip-light"), Some(f.strip.clone())),
        (yours("console"), None),
    ]);
    assert_eq!(library::compare(&p, &read), library::compare(&p, &built));
    assert_eq!(library::compare_from(&p, Some(&b), Some(&u)), library::compare(&p, &built));
    // Issue #83: the editor's host reads by origins alone.
    let origins: Vec<AssetOrigin> = p.file.assets.iter().map(|a| a.origin.clone()).collect();
    assert_eq!(library::read_origins(&origins, Some(&b), Some(&u)), read);
}

#[test]
fn adopt_makes_it_own() {
    let mut f = fixture();
    let key = key_of(&f.pack, "console");
    library::adopt(&mut f.pack, &key).unwrap();
    let p = pack::placement(&f.pack).unwrap();
    assert_eq!(p.file.assets.iter().find(|a| a.key == key).unwrap().origin, AssetOrigin::Own);
    assert!(library::compare(&p, &Libraries::new()).iter().all(|l| l.key != key));
    placed::bake_pack(&mut f.pack, &tile()).unwrap();
    assert!(library::adopt(&mut f.pack, "nope").is_err());
}

/// `pack` baked and saved into `dir`.
fn saved(mut pack: Pack, dir: &Path) {
    placed::bake_pack(&mut pack, &tile()).unwrap();
    std::fs::create_dir_all(dir).unwrap();
    pack::save(dir, &pack).unwrap();
}

#[test]
fn export_refuses_a_stale_pack() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("castle");
    saved(fixture().pack, &dir);
    let bytes = library::export(&dir, &back_tile).unwrap();
    assert_eq!(&bytes[..6], olr_pack::archive::LEVEL_MAGIC);
    // A front pixel changed without a bake.
    let mut p = pack::load(&dir).unwrap();
    if let Source::Level(l) = &mut p.source {
        l.front.set(0, 0, GREEN);
    }
    pack::save(&dir, &p).unwrap();
    let e = library::export(&dir, &back_tile).unwrap_err().to_string();
    assert!(e.contains("front.png") && e.contains("olr-cli level bake"), "{e}");
}

/// A level placing `assets` (origin, source, copy id), baked, saved and
/// exported.
fn exported(t: &Path, id: &str, assets: &[(AssetOrigin, &AssetSource, &str)]) -> Vec<u8> {
    let mut p = level_pack(id, "Castle");
    for (i, (o, src, copy)) in assets.iter().enumerate() {
        add(&mut p, src, o.clone(), (i as i32 * 4, 0), copy);
    }
    let dir = t.join("author").join(id);
    saved(p, &dir);
    library::export(&dir, &back_tile).unwrap()
}

fn places<'a>(
    packs: &'a Path,
    user: &'a Path,
    builtin: Option<&'a Path>,
    bundled: &'a BTreeSet<String>,
) -> Places<'a> {
    Places { packs, user, builtin, bundled }
}

#[test]
fn import_reuses_identical_and_numbers_clashes() {
    let t = tempfile::tempdir().unwrap();
    let (packs, user, b) = (t.path().join("packs"), t.path().join("user"), t.path().join("builtin"));
    let (same, other, strip) =
        (encode(&console(RED)).unwrap(), encode(&console(GREEN)).unwrap(), encode(&strip(BLUE)).unwrap());
    save_dir(&user.join("my-console"), &same).unwrap();
    save_dir(&b.join("strip-light"), &strip).unwrap();
    let before = tree(&user.join("my-console"));
    let bytes = exported(
        t.path(),
        "castle",
        &[(yours("console"), &same, "a"), (yours("console-b"), &other, "b"), (yours("strip"), &strip, "c")],
    );
    let none = BTreeSet::new();
    let got = library::import(&bytes, &places(&packs, &user, Some(&b), &none), &back_tile).unwrap();
    assert_eq!((got.id.as_str(), got.name.as_str()), ("castle", "Castle"));
    let said: Vec<String> = got.assets.iter().map(|a| a.to_string()).collect();
    assert_eq!(
        said,
        [
            "own ground",
            "reused yours my-console",
            "added \"Console 2\" as yours console-2",
            "reused built-in strip-light"
        ]
    );
    assert!(matches!(&got.assets[2], ImportedAsset::Added { folder, .. } if folder == "console-2"));
    assert_eq!(tree(&user.join("my-console")), before, "the user's Console is untouched");
    // The level checks and links Same to everything.
    let p = pack::load(&packs.join("castle")).unwrap();
    assert!(placed::stale(&p, &tile()).unwrap().is_empty());
    let links = library::compare_from(&pack::placement(&p).unwrap(), Some(&b), Some(&user));
    assert!(links.iter().all(|l| l.status == Status::Same), "{links:?}");
    assert_eq!(library::compare_from(&pack::placement(&p).unwrap(), Some(&b), Some(&user)).len(), 3);
}

#[test]
fn import_never_overwrites_a_level() {
    let t = tempfile::tempdir().unwrap();
    let (packs, user) = (t.path().join("packs"), t.path().join("user"));
    let bytes = exported(t.path(), "castle", &[(yours("console"), &encode(&console(RED)).unwrap(), "a")]);
    let none = BTreeSet::new();
    let first = library::import(&bytes, &places(&packs, &user, None, &none), &back_tile).unwrap();
    let kept = tree(&packs.join("castle"));
    let second = library::import(&bytes, &places(&packs, &user, None, &none), &back_tile).unwrap();
    assert_eq!(
        (first.id.as_str(), second.id.as_str(), second.name.as_str()),
        ("castle", "castle-2", "Castle 2")
    );
    assert_eq!(tree(&packs.join("castle")), kept);
    assert_eq!(second.assets[1].to_string(), "reused yours console", "the first import's asset");
    let m = olr_pack::read_manifest(&olr_vfs::DirFs::new(packs.join("castle-2"))).unwrap();
    assert_eq!((m.id.as_str(), m.name.as_str()), ("castle-2", "Castle 2"));
    // A bundled id is never taken either.
    let bundled: BTreeSet<String> = ["castle-3".to_string()].into();
    let third = library::import(&bytes, &places(&packs, &user, None, &bundled), &back_tile).unwrap();
    assert_eq!(third.id, "castle-4");
}

#[test]
fn import_refuses_a_bad_pack_whole() {
    let t = tempfile::tempdir().unwrap();
    let (packs, user) = (t.path().join("packs"), t.path().join("user"));
    let none = BTreeSet::new();
    let pl = places(&packs, &user, None, &none);
    let bytes = exported(t.path(), "castle", &[(yours("console"), &encode(&console(RED)).unwrap(), "a")]);
    // Cut short, an `.olpk`, garbage.
    let cut = &bytes[..bytes.len() - 10];
    let olpk = olr_pack::archive::encode(&olr_vfs::DirFs::new(t.path().join("author/castle"))).unwrap();
    // A stale one: its front changed in the archive.
    let mut src = olr_vfs::MemFs::new();
    olr_pack::archive::decode_level_export(&mut &bytes[..], &mut src, &olr_pack::archive::Limits::PACK)
        .unwrap();
    let front = olr_vfs::VPath::new("front.png").unwrap();
    let mut img =
        olr_format::image_io::decode_rgba(&olr_vfs::ContentSource::read(&src, &front).unwrap()).unwrap();
    img.set(0, 0, GREEN);
    olr_vfs::ContentSink::write(&mut src, &front, &olr_format::image_io::encode_rgba(&img)).unwrap();
    let stale = olr_pack::archive::encode_level_export(&src).unwrap();
    // A broken placement.
    let mut broken = src.clone();
    olr_vfs::ContentSink::write(&mut broken, &olr_vfs::VPath::new("editor/placed.ron").unwrap(), b"(nope")
        .unwrap();
    let broken = olr_pack::archive::encode_level_export(&broken).unwrap();
    for (what, b) in [
        ("cut", cut),
        ("olpk", &olpk[..]),
        ("garbage", &b"OLRL1\nxx"[..]),
        ("stale", &stale[..]),
        ("broken", &broken[..]),
    ] {
        let r = library::import(b, &pl, &back_tile);
        assert!(r.is_err(), "{what}: {r:?}");
        assert!(!packs.exists() || tree(&packs).is_empty(), "{what}: a level was written");
        assert!(!user.exists() || tree(&user).is_empty(), "{what}: an asset was written");
    }
}

#[test]
fn import_respects_max_user_assets() {
    let t = tempfile::tempdir().unwrap();
    let (packs, user) = (t.path().join("packs"), t.path().join("user"));
    let filler = encode(&console(BLUE)).unwrap();
    for i in 0..MAX_USER_ASSETS - 1 {
        save_dir(&user.join(format!("a{i}")), &filler).unwrap();
    }
    let bytes = exported(
        t.path(),
        "castle",
        &[
            (yours("x"), &encode(&console(RED)).unwrap(), "a"),
            (yours("y"), &encode(&console(GREEN)).unwrap(), "b"),
        ],
    );
    let before = tree(&user);
    let none = BTreeSet::new();
    let e = library::import(&bytes, &places(&packs, &user, None, &none), &back_tile).unwrap_err();
    assert!(e.to_string().contains(&MAX_USER_ASSETS.to_string()), "{e}");
    assert_eq!(tree(&user), before);
    assert!(!packs.exists() || tree(&packs).is_empty());
    // One fits.
    let one = exported(t.path(), "castle-one", &[(yours("x"), &encode(&console(RED)).unwrap(), "a")]);
    library::import(&one, &places(&packs, &user, None, &none), &back_tile).unwrap();
}
