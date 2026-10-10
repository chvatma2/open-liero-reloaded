//! Corpus test (spec §7): every imported pack in `RL_CONTENT_DIR` validates
//! without errors and round-trips through write and read. A snapshot of the
//! mods and levels catches unintended importer changes. `cargo xtask check`
//! sets `RL_CONTENT_DIR` after importing the pinned OLX content.

use std::fmt::Write;

use rl_data::level::Material;
use rl_data::manifest::PackKind;
use rl_pack::Severity;
use rl_vfs::{DirFs, MemFs};

#[test]
fn imported_content() {
    let Ok(dir) = std::env::var("RL_CONTENT_DIR") else {
        eprintln!("RL_CONTENT_DIR not set; skipping corpus test");
        return;
    };
    let mut dirs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join(rl_pack::MANIFEST).is_file())
        .collect();
    dirs.sort();
    let mut summary = String::new();
    let mut skins = 0;
    for path in &dirs {
        let src = DirFs::new(path);
        let errors: Vec<_> =
            rl_pack::validate(&src).into_iter().filter(|d| d.severity == Severity::Error).collect();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        let mut copy = MemFs::new();
        match rl_pack::read_manifest(&src).unwrap().kind {
            PackKind::Mod => {
                let p = rl_pack::read_mod(&src).unwrap();
                rl_pack::write_mod(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_mod(&copy).unwrap(), p);
                let d = &p.def;
                writeln!(
                    summary,
                    "mod {}: {:?}, {} weapons, {} projectiles, {} assets, rope {:?}",
                    p.manifest.id,
                    d.name,
                    d.weapons.len(),
                    d.projectiles.len(),
                    p.assets.len(),
                    d.rope
                )
                .unwrap();
            }
            PackKind::Level => {
                let p = rl_pack::read_level(&src).unwrap();
                rl_pack::write_level(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_level(&copy).unwrap(), p);
                let l = &p.level;
                writeln!(
                    summary,
                    "level {}: {:?} {}x{} art x{}, dirt {}, rock {}, theme {}",
                    p.manifest.id,
                    l.def.name,
                    l.def.width,
                    l.def.height,
                    l.def.art_scale,
                    l.material.count(Material::Dirt),
                    l.material.count(Material::Rock),
                    l.def.theme
                )
                .unwrap();
            }
            PackKind::Skin => {
                let p = rl_pack::read_skin(&src).unwrap();
                rl_pack::write_skin(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_skin(&copy).unwrap(), p);
                skins += 1;
            }
            PackKind::Theme => {
                let p = rl_pack::read_theme(&src).unwrap();
                rl_pack::write_theme(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_theme(&copy).unwrap(), p);
                let t = &p.theme.def;
                writeln!(
                    summary,
                    "theme {}: {} holes, {} stones, {} misc",
                    p.manifest.id, t.holes, t.stones, t.misc
                )
                .unwrap();
            }
        }
    }
    writeln!(summary, "skins: {skins}").unwrap();
    insta::assert_snapshot!(summary);
}
