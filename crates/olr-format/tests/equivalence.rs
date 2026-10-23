//! The bundled OLX mods are format 5: read as authoring files and written
//! again they give the committed bytes, and they compile to the same
//! `ModDef` and `SimMod` the runtime reads (spec §6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use olr_data::mod_def::ModDef;
use olr_format::mods::{compile_mod, lift_mod, read_mod_files, write_mod_files};
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

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

/// Every file of the pack that `olr-format` owns (all but `pack.ron` and
/// the assets), as `(path, bytes)`.
fn definition_files(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            let owned = rel == "mod.ron"
                || rel == "restrictions.ron"
                || rel.starts_with("weapons/")
                || rel.starts_with("projectiles/");
            if owned {
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// The committed mods are format 5: read as files and written again they
/// give the same bytes, and they compile to the `ModDef` and `SimMod` the
/// runtime reads (spec §6).
#[test]
fn bundled_mods_are_canonical_format_5() {
    let Some(root) = content() else {
        eprintln!("skip: OLR_CONTENT_DIR is not a directory");
        return;
    };
    for id in MODS {
        let dir = DirFs::new(root.join(id));
        let src = read_mod_files(&dir).unwrap_or_else(|e| panic!("{id}: {e}"));
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &src).unwrap();
        // The same bytes: the committed files are what the writer writes.
        let committed = definition_files(&root.join(id));
        let written: Vec<String> =
            fs.files().map(|(p, _)| p.to_string()).filter(|p| p != "pack.ron").collect();
        assert_eq!(written, committed.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), "{id}: files");
        for (name, bytes) in &committed {
            let again = fs.read(&VPath::new(name).unwrap()).unwrap();
            assert!(again == *bytes, "{id}/{name}: written bytes differ from the committed file");
        }
        let pack = olr_pack::read_mod(&dir).unwrap();
        let def = compile_mod(&src).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_same(id, &def, &pack.def);
        assert_eq!(lift_mod(&def).unwrap(), src, "{id}: lift(compile(read)) is what was read");
        // `==` on floats cannot see a lost -0.0 or a changed NaN payload; the
        // RON text of both can.
        assert_eq!(ron::ser::to_string(&def).unwrap(), ron::ser::to_string(&pack.def).unwrap());
        let f = frames(&pack);
        assert_eq!(
            olr_sim::SimMod::compile(&def, &f).unwrap(),
            olr_sim::SimMod::compile(&pack.def, &f).unwrap(),
            "{id}: SimMod"
        );
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
            &["mod.ron", "weapons/bazooka.ron", "weapons/bouncy-larpa.ron", "projectiles/bazooka.s.ron"][..],
        ),
        ("olx-mw-1-0", &["mod.ron", "weapons/rocket-launcher.ron", "weapons/floating-bomber.ron"][..]),
    ];
    for (id, files) in picks {
        for f in files {
            let path = root.join(id).join(f);
            let Ok(bytes) = std::fs::read(&path) else {
                panic!("{id}: no {f}; pick another weapon that exists in the pack");
            };
            insta::assert_snapshot!(
                format!("{id}__{}", f.replace('/', "__")),
                String::from_utf8_lossy(&bytes)
            );
        }
    }
}
