//! The menus' lists of mods, levels and skins (`Content::catalog`).

use std::path::Path;

use rl_content::{Content, DEFAULT_PRESET, MAX_THUMB_SIDE};
use rl_data::image::RgbaImage;
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_pack::LevelPack;
use rl_vfs::{ContentSink, DirFs, VPath};

fn manifest(id: &str, name: &str, kind: PackKind) -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: name.into(),
        version: "1".into(),
        kind,
        authors: vec![],
        origin: None,
        license: None,
    }
}

fn level(dir: &Path, id: &str, name: &str) -> DirFs {
    let mut fs = DirFs::new(dir.join(id));
    let pack = LevelPack {
        manifest: manifest(id, name, PackKind::Level),
        level: Level {
            def: LevelDef {
                name: name.into(),
                width: 3,
                height: 2,
                art_scale: 1,
                theme: "olx-theme-dirt".into(),
                spawns: vec![],
            },
            material: MaterialGrid::new(3, 2, Material::Dirt),
            front: RgbaImage::new(3, 2),
            back: RgbaImage::new(3, 2),
        },
    };
    rl_pack::write_level(&mut fs, &pack).unwrap();
    fs
}

fn write(fs: &mut DirFs, file: &str, bytes: &[u8]) {
    fs.write(&VPath::new(file).unwrap(), bytes).unwrap();
}

/// Levels list with their size, a thumbnail when the pack has a good one
/// (a placeholder otherwise: none, too large, not a PNG); mods with their
/// presets; an unreadable pack is left out with a warning, never an error.
#[test]
fn catalog_lists_mods_and_levels_without_loading_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    level(root, "lvl-plain", "Plain");
    let mut thumbed = level(root, "lvl-thumb", "Thumbed");
    write(&mut thumbed, rl_pack::THUMB_FILE, &rl_pack::image_io::encode_rgba(&RgbaImage::new(4, 2)));
    let mut huge = level(root, "lvl-huge", "Huge thumb");
    let big = RgbaImage::new(MAX_THUMB_SIDE + 1, 1);
    write(&mut huge, rl_pack::THUMB_FILE, &rl_pack::image_io::encode_rgba(&big));
    let mut junk = level(root, "lvl-junk", "Junk thumb");
    write(&mut junk, rl_pack::THUMB_FILE, b"not a png");
    let mut broken = level(root, "lvl-broken", "Broken");
    write(&mut broken, "level.ron", b"(");
    let mut m = DirFs::new(root.join("mod-a"));
    rl_pack::write_manifest(&mut m, &manifest("mod-a", "Mod A", PackKind::Mod)).unwrap();
    write(&mut m, rl_pack::RESTRICTIONS_FILE, b"{\"Standard 100lt\": {}, \"All\": {}}");
    // A skin is listed by its manifest alone (nothing is decoded).
    let mut s = DirFs::new(root.join("olx-skin-ant"));
    rl_pack::write_manifest(&mut s, &manifest("olx-skin-ant", "ant", PackKind::Skin)).unwrap();

    let c = Content::open(root).unwrap().catalog();
    let ids: Vec<&str> = c.levels.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["lvl-huge", "lvl-junk", "lvl-plain", "lvl-thumb"]);
    let plain = c.level("lvl-plain").unwrap();
    assert_eq!((plain.name.as_str(), plain.width, plain.height), ("Plain", 3, 2));
    assert!(plain.thumb.is_none());
    let thumb = c.level("lvl-thumb").unwrap().thumb.as_ref().unwrap();
    assert_eq!((thumb.width(), thumb.height()), (4, 2));
    assert!(c.level("lvl-huge").unwrap().thumb.is_none());
    assert!(c.level("lvl-junk").unwrap().thumb.is_none());
    for bad in ["lvl-broken", "lvl-huge", "lvl-junk"] {
        assert!(c.warnings.iter().any(|w| w.contains(bad)), "{bad}: {:?}", c.warnings);
    }
    let a = c.mod_info("mod-a").unwrap();
    assert_eq!(a.name, "Mod A");
    assert_eq!(a.presets, ["All", "Standard 100lt"]);
    assert_eq!(c.skins, [rl_content::SkinInfo { id: "olx-skin-ant".into(), name: "ant".into() }]);
    assert_eq!(c.skin("ant").map(|s| s.id.as_str()), Some("olx-skin-ant"), "an OLX name");
    assert_eq!(c.skin("olx-skin-ant").map(|s| s.name.as_str()), Some("ant"));
    assert!(c.skin("batman").is_none());

    // OLX's imported content, when there is some.
    let Some(dir) = std::env::var_os("RL_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set; skipping the imported content");
        return;
    };
    let c = Content::open(Path::new(&dir)).unwrap().catalog();
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    let classic = c.mod_info("olx-classic").unwrap();
    assert!(classic.presets.iter().any(|p| p == DEFAULT_PRESET), "{:?}", classic.presets);
    let factory = c.level("olx-level-liero-factory").unwrap();
    assert_eq!((factory.name.as_str(), factory.width, factory.height), ("Liero Factory", 1200, 480));
    assert_eq!(c.skin("default").map(|s| s.id.as_str()), Some("olx-skin-default"));
    assert!(c.skins.len() > 100, "{} skins", c.skins.len());
}
