//! Corpus test (spec §7): every imported pack in `RL_CONTENT_DIR` validates
//! without errors and round-trips through write and read. A snapshot of the
//! mods and levels catches unintended importer changes. `cargo xtask check`
//! sets `RL_CONTENT_DIR` after importing the pinned OLX content.

use std::fmt::Write;

use rl_data::level::Material;
use rl_data::manifest::PackKind;
use rl_pack::Severity;
use rl_vfs::{DirFs, MemFs};

/// The imported content (`RL_CONTENT_DIR`, which `.cargo/config.toml` defaults
/// to `target/content`), or `None` with a notice if it has not been imported.
fn content_dir() -> Option<String> {
    let dir = std::env::var("RL_CONTENT_DIR").ok()?;
    if std::path::Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

#[test]
fn imported_content() {
    let Some(dir) = content_dir() else { return };
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
                for (name, preset) in &d.restrictions {
                    let rules = rl_data::restrictions::WeaponRules::new(d, name);
                    let (enabled, bonus) = (rules.enabled_weapons().len(), rules.bonus_weapons().len());
                    let n = preset.len();
                    writeln!(
                        summary,
                        "  preset {name:?}: {n} entries, {enabled} enabled, {bonus} in bonuses"
                    )
                    .unwrap();
                }
            }
            PackKind::Level => {
                let p = rl_pack::read_level(&src).unwrap();
                rl_pack::write_level(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_level(&copy).unwrap(), p);
                let l = &p.level;
                writeln!(
                    summary,
                    "level {}: {:?} {}x{} art x{}, dirt {}, rock {}, theme {}, thumb {}",
                    p.manifest.id,
                    l.def.name,
                    l.def.width,
                    l.def.height,
                    l.def.art_scale,
                    l.material.count(Material::Dirt),
                    l.material.count(Material::Rock),
                    l.def.theme,
                    rl_pack::read_level_thumb(&src).unwrap().map_or("none".to_string(), |t| format!(
                        "{}x{}",
                        t.width(),
                        t.height()
                    ))
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
                let green = match &p.theme.green_ball {
                    Some(g) => format!("green ball {}x{}", g.width(), g.height()),
                    None => "no green ball".into(),
                };
                writeln!(
                    summary,
                    "theme {}: {} holes, {} stones, {} misc, {green}",
                    p.manifest.id, t.holes, t.stones, t.misc
                )
                .unwrap();
            }
            PackKind::Core => {
                let p = rl_pack::read_core(&src).unwrap();
                rl_pack::write_core(&mut copy, &p).unwrap();
                assert_eq!(rl_pack::read_core(&copy).unwrap(), p);
                let d = &p.def;
                let rope = ["RopeColour1", "RopeColour2"].map(|k| d.colours.get(k).map(|c| (c.r, c.g, c.b)));
                writeln!(
                    summary,
                    "core {}: sprites {:?}, sounds {:?}, {} colours, rope {rope:?}",
                    p.manifest.id,
                    d.sprites,
                    d.sounds,
                    d.colours.len()
                )
                .unwrap();
            }
        }
    }
    writeln!(summary, "skins: {skins}").unwrap();
    insta::assert_snapshot!(summary);
}
