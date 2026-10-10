//! Imports the real OLX mods in scope when `RL_OLX_GAMEDIR` points at an OLX
//! `share/gamedir`; `cargo xtask check` sets it after `fetch-olx-content`.

use rl_data::manifest::Origin;
use rl_vfs::{DirFs, MemFs, VPath};

fn gamedir() -> Option<DirFs> {
    match std::env::var("RL_OLX_GAMEDIR") {
        Ok(dir) => Some(DirFs::new(dir)),
        Err(_) => {
            eprintln!("RL_OLX_GAMEDIR not set; skipping real-mod import test");
            None
        }
    }
}

fn check(dir: &str, weapons: usize, name: &str) {
    let Some(fs) = gamedir() else { return };
    let origin = Origin { source: "OpenLieroX".into(), path: dir.into(), revision: "test".into() };
    let m = rl_import_lgs::import_mod(&fs, &VPath::new(dir).unwrap(), origin).unwrap();
    assert_eq!(m.pack.def.name, name);
    assert_eq!(m.pack.def.weapons.len(), weapons);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    let mut out = MemFs::new();
    rl_pack::write_mod(&mut out, &m.pack).unwrap();
    let errors: Vec<_> =
        rl_pack::validate(&out).into_iter().filter(|d| d.severity == rl_pack::Severity::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rl_pack::read_mod(&out).unwrap(), m.pack);
}

#[test]
fn liero_1_0() {
    check("Classic", 38, "Liero v1.0");
}

#[test]
fn modern_warfare_1_0() {
    check("MW 1.0", 50, "Modern Warfare1.0");
}
