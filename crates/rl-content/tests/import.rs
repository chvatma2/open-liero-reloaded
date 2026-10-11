//! The OLX import pipeline (`rl_content::import`, plan 7 ruling Q3) on the
//! committed tiny OLX game directory `fixtures/olx-mini` (made by
//! `fixtures/olx-mini.py`), and the layered content directories it feeds
//! (spec §9.4: user packs over the bundled ones).

use std::path::{Path, PathBuf};

use rl_content::Content;
use rl_content::import::{self, ImportOptions, ImportPlan, OLX_CORE_FILES, Progress, REPORT_FILE};
use rl_data::image::RgbaImage;
use rl_data::level::{Level, LevelDef, Material, MaterialGrid};
use rl_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_pack::LevelPack;
use rl_vfs::DirFs;

fn olx_mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/olx-mini")
}

fn local() -> ImportOptions {
    ImportOptions { revision: "local".into(), prune: false }
}

/// A level pack `id` named `name` in `dir/<sub>`.
fn level(dir: &Path, sub: &str, id: &str, name: &str) {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: name.into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    };
    let def = LevelDef {
        name: name.into(),
        width: 3,
        height: 2,
        art_scale: 1,
        theme: "olx-theme-dirt".into(),
        spawns: vec![],
    };
    let level = Level {
        def,
        material: MaterialGrid::new(3, 2, Material::Dirt),
        front: RgbaImage::new(3, 2),
        back: RgbaImage::new(3, 2),
    };
    let pack = LevelPack { manifest, level, thumb: None };
    rl_pack::write_level(&mut DirFs::new(dir.join(sub)), &pack).unwrap();
}

/// The core pack's files are the manifest's (`cargo xtask
/// fetch-olx-content` imports those; the game's import looks for them).
#[test]
fn olx_core_files_are_the_manifests() {
    #[derive(serde::Deserialize)]
    struct Manifest {
        core: Vec<String>,
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/olx-manifest.ron");
    let text = std::fs::read_to_string(path).unwrap();
    let m: Manifest = ron::from_str(&text).unwrap();
    assert_eq!(m.core, OLX_CORE_FILES.map(String::from).to_vec());
}

/// Discovery finds what OLX's menus list: mods with a `script.lgs`, the
/// `.lxl` levels, the themes and the skins; a partial core is left out
/// with a note, a Liero `.lev` level too.
#[test]
fn discover_finds_what_olx_lists() {
    let d = import::discover(&olx_mini()).unwrap();
    assert_eq!(d.plan.mods, ["Broken", "Tiny Mod"]);
    assert_eq!(d.plan.levels, ["levels/Bad.lxl", "levels/Tiny Level.lxl"]);
    assert_eq!(d.plan.themes, ["data/themes/Dirt"]);
    assert!(d.plan.skins);
    assert!(d.plan.core.is_empty(), "one core file of 18 is no core pack");
    assert!(d.notes.iter().any(|n| n.contains("1 of OLX's 18 core files")), "{:?}", d.notes);
    assert!(d.notes.iter().any(|n| n.contains("not LieroX .lxl")), "{:?}", d.notes);

    // An OpenLieroX checkout's root finds its share/gamedir.
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("share/gamedir/levels");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::copy(olx_mini().join("levels/Tiny Level.lxl"), nested.join("Tiny Level.lxl")).unwrap();
    let d = import::discover(tmp.path()).unwrap();
    assert_eq!(d.gamedir, tmp.path().join("share/gamedir"));
    assert_eq!(d.plan.levels, ["levels/Tiny Level.lxl"]);

    // What is not an OLX directory is an error saying so, never a panic.
    let empty = tempfile::tempdir().unwrap();
    std::fs::write(empty.path().join("notes.txt"), "hi").unwrap();
    let e = format!("{:#}", import::discover(empty.path()).unwrap_err());
    assert!(e.contains("not an OpenLieroX game directory"), "{e}");
    let e = format!("{:#}", import::discover(&empty.path().join("missing")).unwrap_err());
    assert!(e.contains("missing"), "{e}");
    let e = format!("{:#}", import::discover(&empty.path().join("notes.txt")).unwrap_err());
    assert!(e.contains("not a directory"), "{e}");
}

/// Every item is reported (a broken mod or level fails alone, with its
/// error; warnings kept), the packs load, and a cancel stops between
/// items leaving only whole packs, none hidden.
#[test]
fn import_reports_every_item_and_writes_whole_packs() {
    let d = import::discover(&olx_mini()).unwrap();
    let out = tempfile::tempdir().unwrap();
    let mut seen = Vec::new();
    let r = import::import(&d.gamedir, &d.plan, out.path(), &local(), &mut |p| {
        if let Progress::Item { index, total, source } = p {
            seen.push((index, total, source.to_string()));
        }
        true
    })
    .unwrap();
    assert_eq!(r.total, 6);
    assert_eq!(seen.len(), 6);
    assert_eq!(seen[0], (0, 6, "Broken".to_string()));
    assert!(!r.cancelled);
    assert_eq!((r.imported(), r.failed()), (4, 2));
    let broken = r.entries.iter().find(|e| e.source == "Broken").unwrap();
    assert!(broken.error.as_deref().is_some_and(|e| e.contains("header")), "{broken:?}");
    let tiny = r.entries.iter().find(|e| e.source == "Tiny Mod").unwrap();
    assert_eq!(tiny.pack.as_deref(), Some("olx-tiny-mod"));
    assert!(tiny.warnings.iter().any(|w| w.contains("boom.wav")), "{tiny:?}");
    assert!(out.path().join(REPORT_FILE).is_file());

    let c = Content::open(out.path()).unwrap();
    let catalog = c.catalog();
    assert_eq!(catalog.mod_info("olx-tiny-mod").unwrap().name, "Tiny Mod");
    let lvl = catalog.level("olx-level-tiny-level").unwrap();
    assert_eq!((lvl.width, lvl.height), (64, 48));
    assert!(lvl.thumb.is_some(), "a thumbnail is made at import");
    assert_eq!(c.pack_origin("olx-tiny-mod").unwrap().revision, "local");
    c.load_mod("olx-tiny-mod").unwrap();
    c.load_level("olx-level-tiny-level").unwrap();

    // Cancelled after the second item: two tried, the rest not; nothing
    // hidden left behind; a re-import replaces packs in place.
    let again = tempfile::tempdir().unwrap();
    let r = import::import(&d.gamedir, &d.plan, again.path(), &local(), &mut |p| {
        !matches!(p, Progress::Done { index: 1, .. })
    })
    .unwrap();
    assert!(r.cancelled);
    assert_eq!(r.entries.len(), 2);
    let names: Vec<String> = std::fs::read_dir(again.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.starts_with('.')), "{names:?}");
    let r = import::import(&d.gamedir, &d.plan, again.path(), &local(), &mut |_| true).unwrap();
    assert_eq!(r.imported(), 4);
    assert!(Content::open(again.path()).unwrap().warnings().iter().all(|w| !w.contains("also")));

    // A pack an import was killed in the middle of writing is hidden
    // (`.import-<id>`), never listed, and the next import removes it.
    let hidden = again.path().join(".import-olx-level-x");
    level(again.path(), ".import-olx-level-x", "olx-level-x", "Half");
    assert!(Content::open(again.path()).unwrap().pack_dir("olx-level-x").is_none());
    let one = ImportPlan { levels: vec!["levels/Tiny Level.lxl".into()], ..ImportPlan::default() };
    import::import(&d.gamedir, &one, again.path(), &local(), &mut |_| true).unwrap();
    assert!(!hidden.exists());
    // Without `prune`, the packs of earlier imports stay.
    assert!(again.path().join("olx-tiny-mod").is_dir());
}

/// A write that fails (no permission; a full disk alike) is an error,
/// not a panic, and leaves no hidden pack.
#[cfg(unix)]
#[test]
fn a_failed_write_is_an_error() {
    use std::os::unix::fs::PermissionsExt as _;
    let d = import::discover(&olx_mini()).unwrap();
    let out = tempfile::tempdir().unwrap();
    std::fs::set_permissions(out.path(), std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(out.path().join("probe"), b"").is_ok() {
        eprintln!("running as root: permissions do not stop writes; skipping");
        return;
    }
    let e = import::import(&d.gamedir, &d.plan, out.path(), &local(), &mut |_| true).unwrap_err();
    assert!(format!("{e:#}").contains("ermission"), "{e:#}");
    std::fs::set_permissions(out.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

/// Imported packs (the user data's `packs/`) override the bundled ones
/// (spec §9.4): the same id is the imported pack's, whatever the bundled
/// one; new ids add to the bundled ones; an unreadable imported pack
/// hides nothing; a pack being written (hidden) is never listed.
#[test]
fn imported_packs_override_bundled() {
    let bundled = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    level(bundled.path(), "a", "lvl-a", "Bundled A");
    level(bundled.path(), "b", "lvl-b", "Bundled B");
    level(bundled.path(), "d", "lvl-d", "Bundled D");
    level(user.path(), "a", "lvl-a", "Imported A");
    level(user.path(), "c", "lvl-c", "Imported C");
    level(user.path(), ".import-lvl-d", "lvl-d", "Half-written D");
    // Of a format this build cannot read.
    level(user.path(), "b", "lvl-b", "Future B");
    let manifest = user.path().join("b/pack.ron");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace(&format!("format_version: {PACK_FORMAT_VERSION}"), "format_version: 999"),
    )
    .unwrap();

    let c = Content::open_layers(&[user.path(), bundled.path()]).unwrap();
    let catalog = c.catalog();
    let name = |id: &str| catalog.level(id).map(|l| l.name.clone());
    assert_eq!(name("lvl-a").as_deref(), Some("Imported A"));
    assert_eq!(c.pack_dir("lvl-a"), Some(user.path().join("a").as_path()));
    assert_eq!(name("lvl-b").as_deref(), Some("Bundled B"), "an unreadable import hides nothing");
    assert!(c.warnings().iter().any(|w| w.contains("999") || w.contains("format")), "{:?}", c.warnings());
    assert_eq!(name("lvl-c").as_deref(), Some("Imported C"));
    assert_eq!(name("lvl-d").as_deref(), Some("Bundled D"), "a hidden pack is never listed");
    assert!(
        !c.warnings().iter().any(|w| w.contains("is also")),
        "overriding is no duplicate: {:?}",
        c.warnings()
    );

    // No imports yet (no `packs/`): the bundled content alone; neither
    // directory: an error.
    let none = user.path().join("missing");
    let c = Content::open_layers(&[none.as_path(), bundled.path()]).unwrap();
    assert_eq!(c.catalog().level("lvl-a").unwrap().name, "Bundled A");
    assert!(c.warnings().iter().all(|w| !w.contains("missing")), "{:?}", c.warnings());
    assert!(Content::open_layers(&[none.as_path(), &none.join("x")]).is_err());
}

/// A decompression bomb (a small PNG declaring 12000 × 12000) is refused
/// from its header, never decoded: on import (a skin) and when a pack
/// holding one is loaded (a skin sheet; a mod's image at match start).
#[test]
fn a_decompression_bomb_is_refused_on_import_and_load() {
    let bomb = rl_pack::image_io::declared_png(12000, 12000, false);
    let olx = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(olx.path().join("skins")).unwrap();
    std::fs::write(olx.path().join("skins/bomb.png"), &bomb).unwrap();
    std::fs::copy(olx_mini().join("skins/default.png"), olx.path().join("skins/default.png")).unwrap();
    let out = tempfile::tempdir().unwrap();
    let plan = ImportPlan { skins: true, ..ImportPlan::default() };
    let r = import::import(olx.path(), &plan, out.path(), &local(), &mut |_| true).unwrap();
    let e = r.entries.iter().find(|e| e.source == "skins/bomb.png").unwrap();
    assert!(e.error.as_deref().is_some_and(|e| e.contains("above")), "{e:?}");
    assert_eq!(r.imported(), 1);

    // Packs that hold one: a skin sheet, and a mod's image.
    let mini = import::discover(&olx_mini()).unwrap();
    let mods = ImportPlan { mods: vec!["Tiny Mod".into()], ..ImportPlan::default() };
    import::import(&mini.gamedir, &mods, out.path(), &local(), &mut |_| true).unwrap();
    std::fs::write(out.path().join("olx-skin-default/skin.png"), &bomb).unwrap();
    std::fs::create_dir_all(out.path().join("olx-tiny-mod/gfx")).unwrap();
    std::fs::write(out.path().join("olx-tiny-mod/gfx/bomb.png"), &bomb).unwrap();
    let c = Content::open(out.path()).unwrap();
    let e = format!("{:#}", c.load_skin("olx-skin-default").unwrap_err());
    assert!(e.contains("above"), "{e}");
    let m = c.load_mod("olx-tiny-mod").unwrap();
    assert!(m.warnings.iter().any(|w| w.contains("bomb.png") && w.contains("above")), "{:?}", m.warnings);
}

/// Copies directory `from` to `to` (a pack).
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// An import killed between moving a pack aside and moving its new one
/// into place leaves only `.replaced-<id>`: the content still lists that
/// pack, and the next import puts it back (whatever it imports); a
/// `.replaced-<id>` beside its `<id>` is the old one and goes.
#[test]
fn an_interrupted_replace_is_restored() {
    let d = import::discover(&olx_mini()).unwrap();
    let level = ImportPlan { levels: vec!["levels/Tiny Level.lxl".into()], ..ImportPlan::default() };
    let theme = ImportPlan { themes: vec!["data/themes/Dirt".into()], ..ImportPlan::default() };
    let out = tempfile::tempdir().unwrap();
    import::import(&d.gamedir, &level, out.path(), &local(), &mut |_| true).unwrap();
    let id = "olx-level-tiny-level";
    std::fs::rename(out.path().join(id), out.path().join(format!(".replaced-{id}"))).unwrap();
    let c = Content::open(out.path()).unwrap();
    assert!(c.pack_dir(id).is_some(), "the moved-aside pack is still listed");
    c.load_level(id).unwrap();
    import::import(&d.gamedir, &theme, out.path(), &local(), &mut |_| true).unwrap();
    assert!(out.path().join(id).join("pack.ron").is_file(), "restored");
    assert!(!out.path().join(format!(".replaced-{id}")).exists());

    // Killed after the new pack was in place: the old one goes.
    copy_dir(&out.path().join(id), &out.path().join(format!(".replaced-{id}")));
    let c = Content::open(out.path()).unwrap();
    assert_eq!(c.pack_dir(id), Some(out.path().join(id).as_path()));
    assert!(c.warnings().iter().all(|w| !w.contains("also")), "{:?}", c.warnings());
    import::import(&d.gamedir, &theme, out.path(), &local(), &mut |_| true).unwrap();
    assert!(!out.path().join(format!(".replaced-{id}")).exists());
    assert!(out.path().join(id).join("pack.ron").is_file());
}

/// The replaced pack that cannot be removed (no permission) is a warning
/// of that item: its new pack is in place and the import goes on.
#[cfg(unix)]
#[test]
fn a_replaced_pack_left_behind_is_a_warning() {
    use std::os::unix::fs::PermissionsExt as _;
    let d = import::discover(&olx_mini()).unwrap();
    let plan = ImportPlan {
        levels: vec!["levels/Tiny Level.lxl".into()],
        themes: vec!["data/themes/Dirt".into()],
        ..ImportPlan::default()
    };
    let out = tempfile::tempdir().unwrap();
    import::import(&d.gamedir, &plan, out.path(), &local(), &mut |_| true).unwrap();
    let locked = out.path().join("olx-level-tiny-level/locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("f"), b"x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::remove_file(locked.join("f")).is_ok() {
        eprintln!("running as root: permissions do not stop removal; skipping");
        return;
    }
    let r = import::import(&d.gamedir, &plan, out.path(), &local(), &mut |_| true).unwrap();
    assert_eq!(r.imported(), 2);
    let e = &r.entries[0];
    assert!(e.warnings.iter().any(|w| w.contains(".replaced-olx-level-tiny-level")), "{e:?}");
    assert!(out.path().join("olx-level-tiny-level/pack.ron").is_file());
    // The next import cannot remove it either: a note, not a failure.
    let r = import::import(&d.gamedir, &plan, out.path(), &local(), &mut |_| true).unwrap();
    assert_eq!(r.imported(), 2);
    assert!(r.notes.iter().any(|n| n.contains(".replaced-")), "{:?}", r.notes);
    let old = out.path().join(".replaced-olx-level-tiny-level/locked");
    std::fs::set_permissions(old, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// Finding the content stops when asked (a path such as `/` takes long).
#[test]
fn discovery_can_be_cancelled() {
    let e = import::discover_until(&olx_mini(), &|| true).unwrap_err();
    assert!(e.is::<import::Cancelled>(), "{e:#}");
    assert!(import::discover_until(&olx_mini(), &|| false).is_ok());
}

/// Mod, level, theme and skin directories that are symbolic links (a
/// common way to add content) are followed, one level: what they link
/// to is imported.
#[cfg(unix)]
#[test]
fn symlinked_directories_are_followed() {
    use std::os::unix::fs::symlink;
    let elsewhere = tempfile::tempdir().unwrap();
    let mini = olx_mini().canonicalize().unwrap();
    let olx = tempfile::tempdir().unwrap();
    symlink(mini.join("Tiny Mod"), olx.path().join("Linked Mod")).unwrap();
    symlink(mini.join("levels"), olx.path().join("levels")).unwrap();
    std::fs::create_dir_all(olx.path().join("data/themes")).unwrap();
    symlink(mini.join("data/themes/Dirt"), olx.path().join("data/themes/Dirt")).unwrap();
    copy_dir(&mini.join("skins"), &elsewhere.path().join("skins"));
    symlink(elsewhere.path().join("skins"), olx.path().join("skins")).unwrap();
    let d = import::discover(olx.path()).unwrap();
    assert_eq!(d.plan.mods, ["Linked Mod"]);
    assert_eq!(d.plan.levels, ["levels/Bad.lxl", "levels/Tiny Level.lxl"]);
    assert_eq!(d.plan.themes, ["data/themes/Dirt"]);
    assert!(d.plan.skins);
    let out = tempfile::tempdir().unwrap();
    let r = import::import(&d.gamedir, &d.plan, out.path(), &local(), &mut |_| true).unwrap();
    assert_eq!((r.imported(), r.failed()), (4, 1), "{:?}", r.entries);
    assert!(out.path().join("olx-linked-mod/pack.ron").is_file());
}

/// `docs/porting/STATUS.md`'s claim: a replay recorded on a bundled pack
/// plays on after the same OLX item is imported again (another origin
/// revision, `local`), and is refused with the content-hash error when a
/// different item is imported under the same id.
#[test]
fn a_replay_survives_reimporting_the_same_content() {
    use rl_bots::BotConfig;
    use rl_content::{DEFAULT_PRESET, MatchOptions, Player};
    use rl_modes::DeathmatchSettings;
    use rl_sim::SimSettings;
    let d = import::discover(&olx_mini()).unwrap();
    let plan = ImportPlan {
        mods: vec!["Tiny Mod".into()],
        levels: vec!["levels/Tiny Level.lxl".into()],
        themes: vec!["data/themes/Dirt".into()],
        ..ImportPlan::default()
    };
    let bundled = tempfile::tempdir().unwrap();
    let o = ImportOptions { revision: "bundled".into(), prune: true };
    import::import(&d.gamedir, &plan, bundled.path(), &o, &mut |_| true).unwrap();
    let options = MatchOptions {
        mod_id: "olx-tiny-mod".into(),
        level_id: "olx-level-tiny-level".into(),
        seed: 3,
        settings: SimSettings::lx56(),
        players: vec![Player::Human, Player::Bot(BotConfig::default())],
        deathmatch: DeathmatchSettings::default(),
        restrictions: DEFAULT_PRESET.into(),
        pool_capacity: rl_sim::MatchSetup::DEFAULT_POOL_CAPACITY,
    };
    let m = Content::open(bundled.path()).unwrap().build_match(&options).unwrap();
    let replay = m.recorder.clone().finish(&m.world).unwrap();

    let user = tempfile::tempdir().unwrap();
    import::import(&d.gamedir, &plan, user.path(), &local(), &mut |_| true).unwrap();
    let c = Content::open_layers(&[user.path(), bundled.path()]).unwrap();
    assert_eq!(c.pack_origin("olx-tiny-mod").unwrap().revision, "local");
    c.check_replay_packs(&replay.setup).unwrap();

    // Another mod under the same directory name: the same id, other content.
    let other = tempfile::tempdir().unwrap();
    std::fs::create_dir(other.path().join("Tiny Mod")).unwrap();
    let mut script = std::fs::read(olx_mini().join("Tiny Mod/script.lgs")).unwrap();
    script[24 + 7] = b'e'; // "Tiny Mod" -> "Tiny Moe"
    std::fs::write(other.path().join("Tiny Mod/script.lgs"), script).unwrap();
    let mods = ImportPlan { mods: vec!["Tiny Mod".into()], ..ImportPlan::default() };
    import::import(other.path(), &mods, user.path(), &local(), &mut |_| true).unwrap();
    let c = Content::open_layers(&[user.path(), bundled.path()]).unwrap();
    let e = format!("{:#}", c.check_replay_packs(&replay.setup).unwrap_err());
    assert!(e.contains("content hash") && e.contains("olx-tiny-mod"), "{e}");
}

/// Two packs moved aside for one id (`.replaced-<id>` stuck, so a later
/// import used `.replaced-<id>~2`; or the reverse, `~2` stuck and the
/// first slot used again later): the newest is the one listed and put
/// back, whatever the order of their names, and the older goes.
#[test]
fn the_newest_replaced_pack_is_restored() {
    let d = import::discover(&olx_mini()).unwrap();
    let level = ImportPlan { levels: vec!["levels/Tiny Level.lxl".into()], ..ImportPlan::default() };
    let theme = ImportPlan { themes: vec!["data/themes/Dirt".into()], ..ImportPlan::default() };
    let id = "olx-level-tiny-level";
    let hour = std::time::Duration::from_secs(3600);
    for newest_first_slot in [false, true] {
        let out = tempfile::tempdir().unwrap();
        import::import(&d.gamedir, &level, out.path(), &local(), &mut |_| true).unwrap();
        let home = out.path().join(id);
        let (first, second) = (format!(".replaced-{id}"), format!(".replaced-{id}~2"));
        let (newer, older) = if newest_first_slot { (&first, &second) } else { (&second, &first) };
        for (aside, which, age) in [(older, "older", hour), (newer, "newer", hour / 60)] {
            let to = out.path().join(aside);
            copy_dir(&home, &to);
            std::fs::write(to.join("which"), which).unwrap();
            let manifest = std::fs::File::options().write(true).open(to.join("pack.ron")).unwrap();
            manifest.set_modified(std::time::SystemTime::now() - age).unwrap();
        }
        std::fs::remove_dir_all(&home).unwrap();
        let c = Content::open(out.path()).unwrap();
        assert_eq!(c.pack_dir(id), Some(out.path().join(newer).as_path()), "the newest is listed");
        import::import(&d.gamedir, &theme, out.path(), &local(), &mut |_| true).unwrap();
        assert_eq!(std::fs::read_to_string(home.join("which")).unwrap(), "newer", "{newer} put back");
        assert!(!out.path().join(&first).exists() && !out.path().join(&second).exists());
    }
}

/// Skins from a second directory (`ImportPlan::extra_skins`: OLX 0.58's
/// release has skins its repository lacks) come in the same run, each pack
/// with its own origin, and a pruning run keeps them.
#[test]
fn extra_skins_come_from_their_own_source() {
    let extra = tempfile::tempdir().unwrap();
    std::fs::copy(olx_mini().join("skins/default.png"), extra.path().join("Hotrod.png")).unwrap();
    std::fs::write(extra.path().join("notes.txt"), "not a skin").unwrap();
    let plan = ImportPlan {
        skins: true,
        extra_skins: vec![import::SkinSource {
            dir: extra.path().to_path_buf(),
            source: "OpenLieroX 0.58 rc5".into(),
            revision: "0.58_rc5".into(),
        }],
        ..ImportPlan::default()
    };
    let out = tempfile::tempdir().unwrap();
    let pruning = ImportOptions { revision: "abc".into(), prune: true };
    for _ in 0..2 {
        let r = import::import(&olx_mini(), &plan, out.path(), &pruning, &mut |_| true).unwrap();
        assert_eq!(r.failed(), 0, "{:?}", r.entries);
        assert_eq!(r.imported(), 2, "{:?}", r.entries);
    }
    let c = Content::open(out.path()).unwrap();
    let origin = c.pack_origin("olx-skin-hotrod").expect("the extra skin");
    assert_eq!(
        (origin.source.as_str(), origin.path.as_str(), origin.revision.as_str()),
        ("OpenLieroX 0.58 rc5", "skins/Hotrod.png", "0.58_rc5")
    );
    let main = c.pack_origin("olx-skin-default").expect("the game directory's skin");
    assert_eq!((main.source.as_str(), main.revision.as_str()), ("OpenLieroX", "abc"));

    // A missing directory is the import's error, as a missing skins/ is.
    let gone = ImportPlan {
        extra_skins: vec![import::SkinSource {
            dir: extra.path().join("nope"),
            ..plan.extra_skins[0].clone()
        }],
        ..ImportPlan::default()
    };
    assert!(import::import(&olx_mini(), &gone, out.path(), &local(), &mut |_| true).is_err());
}
