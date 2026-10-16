//! Imports the real OLX levels in scope when `OLR_OLX_GAMEDIR` is set.

use olr_data::level::Material;
use olr_data::manifest::Origin;
use olr_vfs::{DirFs, MemFs, VPath};

#[test]
fn levels_in_scope() {
    let Ok(dir) = std::env::var("OLR_OLX_GAMEDIR") else {
        eprintln!("OLR_OLX_GAMEDIR not set; skipping real-level import test");
        return;
    };
    let fs = DirFs::new(dir);
    // (file, name, width, height, dirt cells) from the pinned OLX commit.
    let cases = [
        ("levels/Liero Factory.lxl", "Liero Factory", 1200, 480, 24490),
        ("levels/CastleStrike.lxl", "Castle Strike", 1350, 550, 5536),
        ("levels/FossilFacility.lxl", "Fossil Facility", 640, 480, 19261),
        ("levels/Dirt Level.lxl", "Dirt Level", 504, 350, 159596),
    ];
    for (file, name, w, h, dirt) in cases {
        let origin = Origin { source: "OpenLieroX".into(), path: file.into(), revision: "test".into() };
        let l = olr_import_lxl::import_lxl(&fs, &VPath::new(file).unwrap(), origin).unwrap();
        assert!(l.warnings.is_empty(), "{file}: {:?}", l.warnings);
        let lv = &l.pack.level;
        assert_eq!((lv.def.name.as_str(), lv.def.width, lv.def.height), (name, w, h));
        assert_eq!(lv.material.count(Material::Dirt), dirt, "{file}");
        let mut out = MemFs::new();
        olr_pack::write_level(&mut out, &l.pack).unwrap();
        assert_eq!(olr_pack::validate(&out), vec![], "{file}");
        assert_eq!(olr_pack::read_level(&out).unwrap(), l.pack, "{file}");
    }
}
