//! Imports every OLX skin and the Dirt theme when `RL_OLX_GAMEDIR` is set.

use rl_data::manifest::Origin;
use rl_vfs::{ContentSource, DirFs, MemFs, VPath};

fn origin(path: &str) -> Origin {
    Origin { source: "OpenLieroX".into(), path: path.into(), revision: "test".into() }
}

#[test]
fn all_skins_and_dirt_theme() {
    let Ok(dir) = std::env::var("RL_OLX_GAMEDIR") else {
        eprintln!("RL_OLX_GAMEDIR not set; skipping real skin/theme import test");
        return;
    };
    let fs = DirFs::new(dir);
    let skins = VPath::new("skins").unwrap();
    let mut imported = 0;
    for name in fs.list(&skins).unwrap() {
        if !name.to_ascii_lowercase().ends_with(".png") {
            continue;
        }
        let s = rl_import_gfx::import_skin(&fs, &skins.join(&name).unwrap(), origin("skins")).unwrap();
        let mut out = MemFs::new();
        rl_pack::write_skin(&mut out, &s.pack).unwrap();
        assert_eq!(rl_pack::read_skin(&out).unwrap(), s.pack, "{name}");
        imported += 1;
    }
    assert_eq!(imported, 409);

    let t =
        rl_import_gfx::import_theme(&fs, &VPath::new("data/themes/Dirt").unwrap(), origin("Dirt")).unwrap();
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    assert_eq!((t.pack.theme.stones.len(), t.pack.theme.misc.len()), (8, 8));
    let mut out = MemFs::new();
    rl_pack::write_theme(&mut out, &t.pack).unwrap();
    assert_eq!(rl_pack::validate(&out), vec![]);
}
