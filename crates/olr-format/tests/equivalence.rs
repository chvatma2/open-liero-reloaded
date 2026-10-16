//! The bundled OLX mods, read by today's `olr-pack`, lifted, written as
//! authoring files, read back and compiled, give the import back exactly,
//! and the same `SimMod` (spec §6).

use std::collections::BTreeMap;
use std::path::PathBuf;

use olr_data::mod_def::ModDef;
use olr_format::mods::{compile_mod, lift_mod, read_mod_files, write_mod_files};
use olr_vfs::{DirFs, MemFs};

const MODS: [&str; 2] = ["olx-classic", "olx-mw-1-0"];

fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// Frame counts by pack path, as `olr-content` computes them.
fn frames(pack: &olr_pack::ModPack) -> BTreeMap<String, u32> {
    pack.assets
        .iter()
        .filter(|(p, _)| p.as_str().ends_with(".png"))
        .filter_map(|(p, bytes)| {
            let img = olr_pack::image_io::decode_rgba(bytes).ok()?;
            (img.height() > 0).then(|| (p.as_str().to_string(), img.width() / img.height()))
        })
        .collect()
}

/// A readable first difference, so a failure names the weapon or projectile.
fn assert_same(id: &str, got: &ModDef, want: &ModDef) {
    assert_eq!(got.name, want.name, "{id}: name");
    assert_eq!(got.worm, want.worm, "{id}: worm");
    assert_eq!(got.rope, want.rope, "{id}: rope");
    assert_eq!(got.restrictions, want.restrictions, "{id}: restrictions");
    assert_eq!(got.weapons.len(), want.weapons.len(), "{id}: weapon count");
    for (g, w) in got.weapons.iter().zip(&want.weapons) {
        assert_eq!(g, w, "{id}: weapon {}", w.id);
    }
    let gk: Vec<&String> = got.projectiles.keys().collect();
    let wk: Vec<&String> = want.projectiles.keys().collect();
    assert_eq!(gk, wk, "{id}: projectile keys");
    for (k, w) in &want.projectiles {
        assert_eq!(&got.projectiles[k], w, "{id}: projectile {k}");
    }
}

#[test]
fn bundled_mods_survive_lift_write_read_and_compile() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in MODS {
        let pack = olr_pack::read_mod(&DirFs::new(root.join(id))).unwrap();
        let src = lift_mod(&pack.def).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &src).unwrap();
        let back = read_mod_files(&fs).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(back, src, "{id}: read differs from what was written");
        let def = compile_mod(&back).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_same(id, &def, &pack.def);
        // `==` on floats cannot see a lost -0.0 or a changed NaN payload; the
        // RON text of both can.
        assert_eq!(
            ron::ser::to_string(&def).unwrap(),
            ron::ser::to_string(&pack.def).unwrap(),
            "{id}: ModDef text"
        );
        let f = frames(&pack);
        assert_eq!(
            olr_sim::SimMod::compile(&def, &f).unwrap(),
            olr_sim::SimMod::compile(&pack.def, &f).unwrap(),
            "{id}: SimMod"
        );
        let mut again = MemFs::new();
        write_mod_files(&mut again, &back).unwrap();
        assert_eq!(again, fs, "{id}: writing what was read changed bytes");
    }
}

#[test]
fn canonical_files_of_the_bundled_mods() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    let picks = [
        (
            "olx-classic",
            &["mod.ron", "weapons/bazooka.ron", "weapons/bouncy-larpa.ron", "projectiles/bazooka.spawn.ron"]
                [..],
        ),
        ("olx-mw-1-0", &["mod.ron", "weapons/rocket-launcher.ron", "weapons/floating-bomber.ron"][..]),
    ];
    for (id, files) in picks {
        let pack = olr_pack::read_mod(&DirFs::new(root.join(id))).unwrap();
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &lift_mod(&pack.def).unwrap()).unwrap();
        for f in files {
            let Some((_, bytes)) = fs.files().find(|(p, _)| p.as_str() == *f) else {
                panic!("{id}: no {f}; pick another weapon that exists in the pack");
            };
            insta::assert_snapshot!(
                format!("{id}__{}", f.replace('/', "__")),
                String::from_utf8_lossy(bytes)
            );
        }
    }
}
