//! OLR's own packs (`content/reloaded/`, issue #7: the Reloaded levels),
//! shipped beside the imported ones: each checks clean against the
//! committed content, none shares an id with it, the game's content lists
//! them, a bot match plays on each, and Liero Factory Reloaded is OLX's
//! Liero Factory byte for byte but for its look. On the GPU tier each plays
//! and draws lit.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_vfs::DirFs;

const FACTORY: &str = "olr-level-liero-factory-reloaded";
const CASTLE: &str = "olr-level-castlestrike-reloaded";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reloaded_dir() -> PathBuf {
    repo().join("content/reloaded")
}

/// The committed imported content they ship beside (not `OLR_CONTENT_DIR`:
/// what a release holds, offline too).
fn packs_dir() -> PathBuf {
    repo().join("content/packs")
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(args).output().unwrap()
}

/// The pack ids of the packs directly under `root`.
fn ids(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(root).unwrap() {
        let dir = e.unwrap().path();
        if dir.join(olr_pack::MANIFEST).is_file() {
            out.insert(olr_pack::read_manifest(&DirFs::new(&dir)).unwrap().id);
        }
    }
    out
}

#[test]
fn reloaded_packs_check_clean() {
    let (dir, packs) = (reloaded_dir(), packs_dir());
    let o = cli(&["pack", "check", dir.to_str().unwrap(), "--content", packs.to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success() && out.contains("checked 2 packs: 0 errors, 0 warnings"),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// A higher layer hides a lower one's pack of the same id by design (spec
/// §9.4); the two shipped roots must not rely on it.
#[test]
fn reloaded_and_imported_packs_share_no_id() {
    let own = ids(&reloaded_dir());
    assert_eq!(own, BTreeSet::from([FACTORY.to_string(), CASTLE.to_string()]));
    let imported = ids(&packs_dir());
    let shared: Vec<&String> = own.intersection(&imported).collect();
    assert!(shared.is_empty(), "content/reloaded and content/packs share {shared:?}");
}

/// The bundled content (`Content::open_bundled`, what the game, the server
/// and the tools open by default) lists both levels by their names.
#[test]
fn the_bundled_content_lists_the_reloaded_levels() {
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let levels = content.ids(PackKind::Level);
    for (id, name) in [(FACTORY, "Liero Factory Reloaded"), (CASTLE, "Castle Strike Reloaded")] {
        assert!(levels.contains(&id.to_string()), "{id}");
        assert_eq!(content.pack_name(id), Some(name));
        assert!(content.load_level(id).unwrap().has_look(), "{id} is lit");
    }
}

/// The factory's gameplay and art are OLX's: its composition (checked
/// fresh by `reloaded_packs_check_clean`) bakes the imported level's
/// material, front and back byte for byte; only the look is added.
#[test]
fn the_factory_is_the_imported_level_but_for_its_look() {
    let (ours, olx) = (reloaded_dir().join(FACTORY), packs_dir().join("olx-level-liero-factory"));
    for file in ["material.png", "front.png", "back.png"] {
        assert!(std::fs::read(ours.join(file)).unwrap() == std::fs::read(olx.join(file)).unwrap(), "{file}");
    }
    let (a, b) = (
        olr_pack::read_manifest(&DirFs::new(&ours)).unwrap(),
        olr_pack::read_manifest(&DirFs::new(&olx)).unwrap(),
    );
    assert_eq!(a.origin, b.origin, "OLX's Liero Factory.lxl at d1ad953f");
    let src = olr_format::levels::read_level_files(&DirFs::new(&ours)).unwrap();
    let imported = olr_format::levels::read_level_files(&DirFs::new(&olx)).unwrap();
    assert_eq!((src.size, &src.theme, &src.objects), (imported.size, &imported.theme, &imported.objects));
    assert!(src.look.is_some() && src.glow.is_some());
}

/// A headless bot match plays on each (invariants every tick).
#[test]
fn the_reloaded_levels_play() {
    let packs = packs_dir();
    for level in [FACTORY, CASTLE] {
        let o = cli(&[
            "match",
            "--mod",
            "olx-classic",
            "--level",
            level,
            "--bots",
            "3",
            "--seed",
            "1",
            "--ticks",
            "300",
            "--content",
            packs.to_str().unwrap(),
        ]);
        assert!(
            o.status.success(),
            "{level}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

/// Both draw lit in a bot match (the GPU tier, as the playtest packs).
#[test]
fn the_reloaded_levels_draw_lit() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let packs = packs_dir();
    for level in [FACTORY, CASTLE] {
        let out = tmp.path().join(format!("{level}.png"));
        let o = cli(&[
            "render-match",
            "--mod",
            "olx-classic",
            "--level",
            level,
            "--bots",
            "3",
            "--seed",
            "1",
            "--ticks",
            "200",
            "--content",
            packs.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(o.status.success(), "{level}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stdout).contains("glowing discs"), "{level}: a lit match");
    }
}
