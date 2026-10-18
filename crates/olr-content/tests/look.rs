//! Loading level and mod looks (plan 15a Task 6).

use std::path::Path;

use olr_content::Content;
use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_format::Colour;
use olr_format::image_io;
use olr_format::levels::{GlowLayers, LevelObject, LevelSource, write_level_files};
use olr_format::look::{GlowGroupSource, LightSource, LookSource, ReachSource};
use olr_vfs::DirFs;

fn manifest(id: &str, kind: PackKind) -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: id.into(),
        version: "1".into(),
        kind,
        authors: vec![],
        origin: None,
        license: None,
    }
}

/// A 6 × 4 level at art scale 2 with a look (one group, one light) and glow.
fn lit_level(dir: &Path) {
    let (w, h, s) = (6, 4, 2);
    let mut glow = GlowLayers::empty((w * s, h * s), (w, h));
    glow.front[0] = 200;
    let group = GlowGroupSource {
        id: "lava".into(),
        colour: Default::default(),
        intensity: 150,
        reach: ReachSource::Pixels(32),
        animation: Default::default(),
        phase_ms: None,
    };
    let light = LightSource {
        colour: Colour(olr_data::image::Rgba::rgb(255, 0, 0)),
        intensity: 100,
        reach: ReachSource::Unlimited,
        animation: Default::default(),
        phase_ms: None,
    };
    let src = LevelSource {
        name: "Lit".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: vec![LevelObject::light((2, 1), 3, light)],
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
        look: Some(LookSource { glow_groups: vec![group], ..Default::default() }),
        glow: Some(glow),
    };
    let mut fs = DirFs::new(dir.join("lit"));
    write_level_files(&mut fs, &src).unwrap();
    olr_pack::write_manifest(&mut fs, &manifest("lit", PackKind::Level)).unwrap();
}

#[test]
fn a_level_look_loads() {
    let tmp = tempfile::tempdir().unwrap();
    lit_level(tmp.path());
    let level = Content::open(tmp.path()).unwrap().load_level("lit").unwrap();
    assert!(level.has_look());
    let look = level.look.as_ref().unwrap();
    assert_eq!((look.groups.len(), look.lights.len()), (1, 1));
    assert_eq!(look.lights[0].at, (2, 1));
    assert_eq!(level.glow.as_ref().unwrap().front[0], 200);
    assert!(level.warnings.is_empty(), "{:?}", level.warnings);
    let from_source = olr_content::LoadedLevel::from_source(
        &olr_format::levels::read_level_files(&DirFs::new(tmp.path().join("lit"))).unwrap(),
    )
    .unwrap();
    assert_eq!(from_source.look, level.look);
    assert_eq!(from_source.glow, level.glow);
}

#[test]
fn a_broken_glow_loads_without_glow() {
    let tmp = tempfile::tempdir().unwrap();
    lit_level(tmp.path());
    std::fs::write(tmp.path().join("lit/look/glow.png"), image_io::encode_grey(3, 3, &[0; 9])).unwrap();
    let level = Content::open(tmp.path()).unwrap().load_level("lit").unwrap();
    assert_eq!(level.glow, None);
    assert!(level.look.is_some(), "the look itself still loads");
    assert_eq!(level.warnings.len(), 1, "{:?}", level.warnings);
    assert!(level.warnings[0].contains("look/glow.png"), "{:?}", level.warnings);
    // pack check still says it is broken.
    let diags = olr_pack::validate(&DirFs::new(tmp.path().join("lit")));
    assert!(diags.iter().any(|d| d.to_string().contains("look/glow.png")), "{diags:?}");
}

#[test]
fn a_broken_look_loads_without_look() {
    let tmp = tempfile::tempdir().unwrap();
    lit_level(tmp.path());
    let path = tmp.path().join("lit/level.ron");
    let text = std::fs::read_to_string(&path).unwrap().replace("intensity: 150", "intensity: 1500");
    std::fs::write(&path, text).unwrap();
    let level = Content::open(tmp.path()).unwrap().load_level("lit").unwrap();
    assert_eq!((level.look.is_some(), level.glow.is_some()), (false, false));
    assert!(!level.warnings.is_empty());
}

#[test]
fn the_color_map_carries_the_glow() {
    let Some(root) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    lit_level(tmp.path());
    let content = Content::open_layers(&[Path::new(&root), tmp.path()]).unwrap();
    let level = content.load_level("lit").unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let map = level.color_map(&theme);
    assert_eq!(map.glow().unwrap()[0], 200);
}

const MOD: &str = "(name: \"T\", weapons: [\"w\"])";
const WEAPON: &str = r##"(
    name: "W",
    recharge: 0.5,
    drain: 30.0,
    between_shots_s: 0.25,
    fire: Projectile(
        shot: (
            speed: 100,
            projectile: (
                gfx: Pixels(["#948800"]),
                hit_terrain: [Explode(damage: 4), Spawn("s")],
                spawns: {"s": (amount: 3, projectile: (gfx: Pixels([])))},
                look: (glow: 80),
            ),
        ),
    ),
    look: (muzzle_light: (colour: "#ffffff", intensity: 50, reach: Pixels(40))),
)
"##;

#[test]
fn mod_look_is_indexed_like_simmod() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("lit-mod");
    std::fs::create_dir_all(dir.join("weapons")).unwrap();
    std::fs::write(dir.join("mod.ron"), MOD).unwrap();
    std::fs::write(dir.join("weapons/w.ron"), WEAPON).unwrap();
    olr_pack::write_manifest(&mut DirFs::new(&dir), &manifest("lit-mod", PackKind::Mod)).unwrap();
    let m = Content::open(tmp.path()).unwrap().load_mod("lit-mod").unwrap();
    assert_eq!(m.look.projectiles.len(), m.sim.projectiles().len());
    for (i, p) in m.sim.projectiles().iter().enumerate() {
        let want = (p.name == "w").then_some(80);
        assert_eq!(m.look.projectiles[i].as_ref().and_then(|l| l.glow), want, "{}", p.name);
    }
    assert_eq!(m.look.weapons.len(), 1);
    assert_eq!(m.look.weapons[0].as_ref().unwrap().muzzle_light.as_ref().unwrap().intensity, 50);
}

#[test]
fn bundled_content_has_no_look() {
    let Some(root) = content_dir() else { return };
    let content = Content::open(Path::new(&root)).unwrap();
    for id in [
        "olx-level-dirt-level",
        "olx-level-liero-factory",
        "olx-level-castlestrike",
        "olx-level-fossilfacility",
    ] {
        let l = content.load_level(id).unwrap();
        assert!(l.look.is_none() && l.glow.is_none() && !l.has_look(), "{id}");
    }
    for id in ["olx-classic", "olx-mw-1-0"] {
        assert!(content.load_mod(id).unwrap().look.is_empty(), "{id}");
    }
}

fn content_dir() -> Option<String> {
    let dir = std::env::var("OLR_CONTENT_DIR").ok()?;
    Path::new(&dir).is_dir().then_some(dir)
}

/// Shared projectiles, explicit ids and their children keep their looks
/// (review of batch A, Critical 1): looks are keyed by the authoring key,
/// not the occurrence path.
#[test]
fn shared_and_named_projectiles_keep_their_looks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("shared-mod");
    std::fs::create_dir_all(dir.join("weapons")).unwrap();
    std::fs::create_dir_all(dir.join("projectiles")).unwrap();
    std::fs::write(dir.join("mod.ron"), "(name: \"S\", weapons: [\"a\", \"b\", \"c\"])").unwrap();
    let shared = |w: &str| {
        format!(
            "(name: \"{w}\", recharge: 0.5, drain: 30.0, between_shots_s: 0.25, fire: Projectile(shot: (speed: 100, shared: \"bullet\")))"
        )
    };
    std::fs::write(dir.join("weapons/a.ron"), shared("A")).unwrap();
    std::fs::write(dir.join("weapons/b.ron"), shared("B")).unwrap();
    std::fs::write(
        dir.join("weapons/c.ron"),
        r##"(name: "C", recharge: 0.5, drain: 30.0, between_shots_s: 0.25, fire: Projectile(shot: (speed: 100,
            projectile: (id: "named", gfx: Pixels(["#ffffff"]), look: (glow: 30)))))"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("projectiles/bullet.ron"),
        r##"(gfx: Pixels(["#ffffff"]), hit_terrain: [Spawn("s")],
            spawns: {"s": (amount: 2, projectile: (gfx: Pixels([]), look: (glow: 20)))}, look: (glow: 10))"##,
    )
    .unwrap();
    olr_pack::write_manifest(&mut DirFs::new(&dir), &manifest("shared-mod", PackKind::Mod)).unwrap();
    let m = Content::open(tmp.path()).unwrap().load_mod("shared-mod").unwrap();
    let glow_of = |name: &str| {
        let i = m.sim.projectiles().iter().position(|p| p.name == name).unwrap_or_else(|| panic!("{name}"));
        m.look.projectiles[i].as_ref().and_then(|l| l.glow)
    };
    assert_eq!(glow_of("a"), Some(10));
    assert_eq!(glow_of("b"), Some(10));
    assert_eq!(glow_of("a.spawn"), Some(20));
    assert_eq!(glow_of("c"), Some(30));
}
