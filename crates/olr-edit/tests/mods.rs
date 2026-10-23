//! Mod editing operations (issue #16) on copies of the bundled mods:
//! get and set by path, weapons, shared projectiles, located errors.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use olr_data::loadouts::MatchLoadout;
use olr_data::manifest::PackKind;
use olr_data::restrictions::WeaponState;
use olr_edit::mods::{self, ModError, WeaponFrom};
use olr_edit::new::{Template, new_pack};
use olr_edit::pack::{self, Pack};
use olr_format::mods::{Fire, ModSource, compile_mod};
use olr_pack::Severity;

use crate::common::{content, copy_pack, tree};

const CLASSIC: &str = "olx-classic";
const MW: &str = "olx-mw-1-0";

fn src(p: &Pack) -> &ModSource {
    mods::source(p).unwrap()
}

fn files(p: &Pack) -> BTreeMap<String, Vec<u8>> {
    pack::files(p).unwrap().into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn load(root: &Path, id: &str) -> Pack {
    pack::load(&root.join(id)).unwrap()
}

fn errors(e: ModError) -> Vec<mods::Issue> {
    match e {
        ModError::Invalid(list) => list,
        e => panic!("not refused for its errors: {e}"),
    }
}

/// The issue's acceptance test: a copy of Classic with the worm's gravity
/// and the bazooka's shot speed set differs from Classic in exactly those
/// fields, beside its id and name.
#[test]
fn set_changes_exactly_the_fields_set() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my-classic");
    new_pack(&dir, PackKind::Mod, "my-classic", "My Classic", Template::From(&root.join(CLASSIC))).unwrap();
    let mut p = pack::load(&dir).unwrap();
    mods::set(&mut p, "mod.worm.gravity", "120.0").unwrap();
    mods::set(&mut p, "weapons/bazooka.fire.shot.speed", "150").unwrap();
    assert_eq!(mods::get(src(&p), "mod.worm.gravity").unwrap(), "120.0");
    assert_eq!(mods::get(src(&p), "weapons/bazooka.fire.shot.speed").unwrap(), "150");
    pack::save(&dir, &p).unwrap();

    let saved = pack::load(&dir).unwrap();
    let mut want = src(&load(&root, CLASSIC)).clone();
    want.name = "My Classic".into();
    want.worm.gravity = 120.0;
    let Fire::Projectile { shot, .. } = &mut want.weapons[0].1.fire else { panic!("a projectile weapon") };
    shot.speed = 150;
    assert_eq!(src(&saved), &want);
    let (a, b) = (tree(&root.join(CLASSIC)), tree(&dir));
    let changed: BTreeSet<&String> = a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)).collect();
    let changed: Vec<&String> = changed.into_iter().collect();
    assert_eq!(changed, ["mod.ron", "pack.ron", "weapons/bazooka.ron"]);
    assert!(olr_pack::validate(&olr_vfs::DirFs::new(&dir)).iter().all(|d| d.severity == Severity::Warning));
}

/// Every leaf of every bundled mod's files, got and set again, leaves the
/// files byte for byte.
#[test]
fn every_leaf_round_trips_through_set() {
    let Some(root) = content() else { return };
    for id in [CLASSIC, MW] {
        let mut p = load(&root, id);
        let before = files(&p);
        let leaves = mods::leaves(src(&p));
        assert!(leaves.len() > 1000, "{id}: {} leaves", leaves.len());
        for leaf in &leaves {
            let v = mods::get(src(&p), leaf).unwrap_or_else(|e| panic!("{id}: get {leaf}: {e}"));
            mods::set(&mut p, leaf, &v).unwrap_or_else(|e| panic!("{id}: set {leaf} = {v}: {e}"));
        }
        assert!(files(&p) == before, "{id}: a leaf set to its own value changed the files");
    }
}

#[test]
fn paths_reach_fields_through_shared_projectiles_and_defaults() {
    let Some(root) = content() else { return };
    let p = load(&root, CLASSIC);
    let m = src(&p);
    // `bazooka`'s spawn names the shared `bazooka.s`: the path goes there.
    let through = "weapons/bazooka.fire.shot.projectile.spawns.spawn.projectile.gravity";
    assert_eq!(mods::get(m, through).unwrap(), mods::get(m, "projectiles/bazooka.s.gravity").unwrap());
    assert_eq!(mods::get(m, "weapons/bazooka.fire.shot.projectile.spawns.spawn.amount").unwrap(), "12");
    assert_eq!(mods::get(m, r#"weapons/bazooka.fire.shot.projectile.spawns["spawn"].amount"#).unwrap(), "12");
    // A default the file leaves out, a list item, a map entry with spaces.
    assert_eq!(mods::get(m, "weapons/bazooka.fire.shot.amount").unwrap(), "1");
    assert_eq!(mods::get(m, "weapons/bazooka.fire.shot.projectile.hit_worm[0].damage").unwrap(), "27");
    assert_eq!(mods::get(m, r#"restrictions["Bonus only"]["Big Nuke"]"#).unwrap(), "Bonus");
    assert_eq!(mods::get(m, "mod.rope.max_length").unwrap(), "300");
    assert!(mods::get(m, "weapons/bazooka").unwrap().starts_with("(\n    name: \"Bazooka\""));
    assert!(mods::get(m, "mod.weapons").unwrap().contains("\"zimm\""));
    // Shared ids hold dots: the longest id wins.
    assert_eq!(mods::get(m, "projectiles/big-nuke.s2.gravity").unwrap(), "None");
    assert_eq!(mods::choices(m, "weapons/bazooka.fire").unwrap(), ["Projectile", "Beam", "Special"]);
    assert_eq!(mods::variant(m, "weapons/bazooka.fire").unwrap().as_deref(), Some("Projectile"));
    assert_eq!(
        mods::children(m, "mod.rope").unwrap(),
        ["mod.rope.max_length", "mod.rope.rest_length", "mod.rope.strength"]
    );
    for bad in [
        "weapons/nope.name",
        "mod.worm.gravty",
        "mod.worm.gravity.x",
        "weapons/bazooka.fire.shot.projectile.hit_worm[9]",
        "x",
        "",
    ] {
        assert!(matches!(mods::get(m, bad), Err(ModError::Path(_))), "{bad}");
    }
}

#[test]
fn set_takes_variants_new_keys_and_bare_strings_and_refuses_bad_values() {
    let Some(root) = content() else { return };
    let mut p = load(&root, CLASSIC);
    mods::set(&mut p, "weapons/bazooka.fire", "Beam").unwrap();
    assert!(mods::get(src(&p), "weapons/bazooka.fire").unwrap().starts_with("Beam("));
    mods::set(&mut p, "weapons/bazooka.fire", "Projectile").unwrap();
    assert!(mods::choices(src(&p), "weapons/bazooka.fire.shot.projectile.trail").unwrap().contains(&"Smoke"));
    mods::set(&mut p, "weapons/bazooka.fire.shot.projectile.trail", "Smoke").unwrap();
    mods::set(&mut p, "weapons/bazooka.fire.sound", "sfx/bazooka.ogg").unwrap();
    assert_eq!(mods::get(src(&p), "weapons/bazooka.fire.sound").unwrap(), "\"sfx/bazooka.ogg\"");
    mods::set(&mut p, r#"restrictions["Only rifles"]"#, r#"{"Rifle": Enabled}"#).unwrap();
    assert_eq!(src(&p).restrictions["Only rifles"]["Rifle"], WeaponState::Enabled);
    // `Unknown` is kept, never offered.
    assert!(!mods::choices(src(&p), "weapons/bazooka.fire.class").unwrap().contains(&"Unknown"));
    assert!(matches!(
        mods::set(&mut p, "weapons/bazooka.fire.class", "Unknown"),
        Err(ModError::Value { .. })
    ));
    let before = files(&p);
    assert!(matches!(mods::set(&mut p, "mod.worm.gravity", "\"heavy\""), Err(ModError::Value { .. })));
    assert!(matches!(mods::set(&mut p, "mod.worm.gravity", "NaN"), Err(ModError::Refused(_))));
    assert!(matches!(mods::set(&mut p, "mod.weapons", "[]"), Err(ModError::Refused(_))));
    let e = errors(mods::set(&mut p, "weapons/bazooka.fire.sound", "\"sfx/nope.ogg\"").unwrap_err());
    assert_eq!(e[0].path.as_deref(), Some("weapons/bazooka.fire.sound"));
    assert!(files(&p) == before, "a refused set changed the pack");
}

/// What LX56 cannot express is refused with its path.
#[test]
fn a_cycle_and_two_effects_in_an_event_are_refused_with_a_path() {
    let Some(root) = content() else { return };
    let mut p = load(&root, CLASSIC);
    let before = files(&p);
    let two = r#"[Explode(damage: 1), Injure(damage: 2)]"#;
    let e = errors(mods::set(&mut p, "weapons/bazooka.fire.shot.projectile.hit_worm", two).unwrap_err());
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].path.as_deref(), Some("weapons/bazooka.fire.shot.projectile.hit_worm[1]"));
    assert!(e[0].message.contains("more than one effect per event"), "{}", e[0]);
    let e = errors(
        mods::set(&mut p, "projectiles/bazooka.s.spawns", r#"{"s": (shared: "bazooka.s")}"#).unwrap_err(),
    );
    assert!(
        e.iter().any(|i| i.path.as_deref() == Some("projectiles/bazooka.s") && i.message.contains("cycle")),
        "{e:?}"
    );
    assert!(files(&p) == before);
}

/// Issue #17: the editor's lenient set keeps a change with an error and
/// returns it; `check` lists it, the save refuses the mod, and a strict
/// edit still goes through on a mod with that old error.
#[test]
fn a_lenient_set_keeps_the_error_and_the_save_refuses_it() {
    let Some(root) = content() else { return };
    let mut p = load(&root, CLASSIC);
    let cycle = r#"{"s": (shared: "bazooka.s")}"#;
    let new = mods::set_lenient(&mut p, "projectiles/bazooka.s.spawns", cycle).unwrap();
    assert!(new.iter().any(|i| i.is_error() && i.message.contains("cycle")), "{new:?}");
    assert!(mods::get(src(&p), "projectiles/bazooka.s.spawns").unwrap().contains("bazooka.s"));
    assert!(mods::check(&p).iter().any(|i| i.is_error() && i.message.contains("cycle")));
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(pack::save(&tmp.path().join("x"), &p), Err(pack::SaveError::Invalid(_))));
    mods::set(&mut p, "mod.worm.gravity", "90.0").unwrap();
    // A value of the wrong type still changes nothing.
    assert!(mods::set_lenient(&mut p, "mod.worm.gravity", "\"heavy\"").is_err());
    assert_eq!(mods::get(src(&p), "mod.worm.gravity").unwrap(), "90.0");
}

/// Issue #18: an unsaved mod as the game reads it, from memory: the value
/// set is in its definition; one with an error is refused as a save is.
#[test]
fn an_unsaved_mod_reads_as_a_saved_one() {
    use olr_data::mod_def::WeaponKind;
    let Some(root) = content() else { return };
    let mut p = load(&root, CLASSIC);
    let saved = pack::mod_pack(&p).unwrap();
    assert_eq!(saved, olr_pack::read_mod(&olr_vfs::DirFs::new(root.join(CLASSIC))).unwrap());
    mods::set(&mut p, "weapons/bazooka.fire.shot.speed", "150").unwrap();
    let m = pack::mod_pack(&p).unwrap();
    let bazooka = m.def.weapons.iter().find(|w| w.id == "bazooka").unwrap();
    let WeaponKind::Projectile { spawn, .. } = &bazooka.kind else { panic!("the bazooka shoots") };
    assert_eq!(spawn.speed, 150);
    let cycle = r#"{"s": (shared: "bazooka.s")}"#;
    mods::set_lenient(&mut p, "projectiles/bazooka.s.spawns", cycle).unwrap();
    assert!(matches!(pack::mod_pack(&p), Err(pack::SaveError::Invalid(_))));
}

#[test]
fn weapons_add_remove_move_copy_and_rename() {
    let Some(root) = content() else { return };
    let mut p = load(&root, CLASSIC);
    let n = src(&p).weapons.len();
    let ids = |p: &Pack| src(p).weapons.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();

    mods::add_weapon(&mut p, "plasma", Some("Plasma"), WeaponFrom::Blank, Some(1)).unwrap();
    assert_eq!(ids(&p)[1], "plasma");
    mods::move_weapon(&mut p, "plasma", n).unwrap();
    assert_eq!(ids(&p)[n], "plasma");
    mods::copy_weapon(&mut p, "bazooka", "bazooka-2", None).unwrap();
    assert_eq!((ids(&p)[1].as_str(), src(&p).weapons[1].1.name.as_str()), ("bazooka-2", "Bazooka 2"));
    mods::remove_weapon(&mut p, "plasma").unwrap();
    assert_eq!(src(&p).weapons.len(), n + 1);

    // A rename carries through every preset and so the shipped loadouts.
    let had: Vec<String> = src(&p)
        .restrictions
        .iter()
        .filter(|(_, p)| p.contains_key("Bazooka"))
        .map(|(k, _)| k.clone())
        .collect();
    assert!(!had.is_empty());
    mods::set(&mut p, "weapons/bazooka.name", "\"Rocket\"").unwrap();
    mods::rename_weapon(&mut p, "bazooka", None, Some("rocket")).unwrap();
    assert_eq!(ids(&p)[0], "rocket");
    let m = src(&p);
    for preset in &had {
        assert!(
            !m.restrictions[preset].contains_key("Bazooka") && m.restrictions[preset].contains_key("Rocket"),
            "{preset}"
        );
    }
    let def = compile_mod(m).unwrap();
    let names: Vec<&str> = def.weapons.iter().map(|w| w.name.as_str()).collect();
    let firebird =
        MatchLoadout::shipped("Firebird", &names, &def.restrictions, def.loadouts["Firebird"]).unwrap();
    assert!(firebird.weapons.contains(&"Rocket".to_string()), "{:?}", firebird.weapons);
    // A whole weapon set with a new name renames too.
    let whole = mods::get(src(&p), "weapons/rocket").unwrap().replace("\"Rocket\"", "\"Missile\"");
    mods::set(&mut p, "weapons/rocket", &whole).unwrap();
    assert!(had.iter().all(|k| src(&p).restrictions[k].contains_key("Missile")));
    // Removing drops its presets' entries.
    mods::remove_weapon(&mut p, "zimm").unwrap();
    assert!(src(&p).restrictions.values().all(|p| !p.contains_key("Zimm")));
    // The mod's rename is the pack's too.
    mods::set(&mut p, "mod.name", "Classic Remix").unwrap();
    assert_eq!((src(&p).name.as_str(), p.manifest.name.as_str()), ("Classic Remix", "Classic Remix"));

    // Refusals change nothing.
    let before = files(&p);
    assert!(mods::add_weapon(&mut p, "rocket", None, WeaponFrom::Blank, None).is_err());
    assert!(mods::add_weapon(&mut p, "Bad Id", None, WeaponFrom::Blank, None).is_err());
    assert!(mods::rename_weapon(&mut p, "rocket", Some("rifle"), None).is_err()); // "Rifle", any case
    assert!(mods::copy_weapon(&mut p, "nope", "x", None).is_err());
    assert!(mods::move_weapon(&mut p, "rocket", 999).is_err());
    assert!(files(&p) == before);
    assert!(mods::check(&p).iter().all(|i| !i.is_error()));
    pack::check(&pack::files(&p).unwrap()).unwrap();
}

#[test]
fn the_last_weapon_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("m");
    new_pack(&dir, PackKind::Mod, "m", "M", Template::Mod).unwrap();
    let mut p = pack::load(&dir).unwrap();
    assert!(matches!(mods::remove_weapon(&mut p, "gun"), Err(ModError::Refused(_))));
}

/// A weapon of another mod comes with its shared projectiles and files:
/// MW's fireball names `fireball.s2`, whose `gfx/flame.png`
/// differs from Classic's (copied as `gfx/flame-2.png`), and
/// `gfx/none.png`, the same in both (kept).
#[test]
fn a_weapon_of_another_mod_brings_its_projectiles_and_files() {
    let Some(root) = content() else { return };
    let mw = load(&root, MW);
    let mut p = load(&root, CLASSIC);
    mods::add_weapon(&mut p, "fireball", None, WeaponFrom::Copy { pack: &mw, id: "fireball" }, None).unwrap();
    let m = src(&p);
    assert_eq!(
        m.weapons.last().unwrap().1.name,
        src(&mw).weapons.iter().find(|(id, _)| id == "fireball").unwrap().1.name
    );
    assert!(m.shared.contains_key("fireball.s2"));
    assert_eq!(mods::get(m, "projectiles/fireball.s2.gfx.file").unwrap(), "\"gfx/flame-2.png\"");
    let flame2 = olr_vfs::VPath::new("gfx/flame-2.png").unwrap();
    assert_eq!(p.other[&flame2], mw.other[&olr_vfs::VPath::new("gfx/flame.png").unwrap()]);
    assert!(!p.other.contains_key(&olr_vfs::VPath::new("gfx/none-2.png").unwrap()));
    assert!(mods::check(&p).iter().all(|i| !i.is_error()), "{:?}", mods::check(&p));
    // Twice: the shared projectile is the same, so it is kept.
    mods::add_weapon(
        &mut p,
        "fireball-2",
        Some("Fireball again"),
        WeaponFrom::Copy { pack: &mw, id: "fireball" },
        None,
    )
    .unwrap();
    assert!(!src(&p).shared.contains_key("fireball.s2-2"));
}

#[test]
fn used_by_matches_the_files() {
    let Some(root) = content() else { return };
    for id in [CLASSIC, MW] {
        let p = load(&root, id);
        let used = mods::used_by(src(&p));
        assert_eq!(used.keys().collect::<Vec<_>>(), src(&p).shared.keys().collect::<Vec<_>>());
        // From the text: each file naming `shared: "<id>"`, as often as it does.
        let mut want: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (path, bytes) in tree(&root.join(id)) {
            let Some(stem) = path.strip_suffix(".ron") else { continue };
            let text = String::from_utf8(bytes).unwrap();
            for line in text.lines() {
                if let Some(rest) = line.trim().strip_prefix("shared: \"") {
                    want.entry(rest.trim_end_matches(['"', ','].as_slice()).to_string())
                        .or_default()
                        .push(stem.to_string());
                }
            }
        }
        for (shared, users) in &used {
            let mut files: Vec<String> = users.iter().map(|u| file_of(src(&p), u)).collect();
            files.sort();
            let mut w = want.get(shared).cloned().unwrap_or_default();
            w.sort();
            assert_eq!(files, w, "{id}: {shared}");
        }
    }
}

/// The file part of a path (`weapons/bazooka` of `weapons/bazooka.fire.shot`).
fn file_of(m: &ModSource, path: &str) -> String {
    mods::path::parse(m, path).unwrap().file.to_string()
}

#[test]
fn inline_and_share_round_trip() {
    let Some(root) = content() else { return };
    let mut p = load(&root, CLASSIC);
    let original = files(&p);
    // An inline shot shared and made inline again.
    let id = mods::share(&mut p, "weapons/bazooka.fire.shot", None).unwrap();
    assert_eq!(id, "bazooka");
    assert!(files(&p).contains_key("projectiles/bazooka.ron"));
    assert_eq!(mods::used_by(src(&p))["bazooka"], ["weapons/bazooka.fire.shot"]);
    mods::inline(&mut p, "weapons/bazooka.fire.shot.projectile").unwrap();
    assert!(files(&p) == original);
    // A shared projectile of several users (lifted mods share only
    // those): inline for one leaves it to the others under a free key.
    let used = mods::used_by(src(&p));
    let (shared, users) =
        used.iter().find(|(_, u)| u.len() > 1).map(|(s, u)| (s.clone(), u.clone())).unwrap();
    mods::inline(&mut p, &users[0]).unwrap();
    assert!(src(&p).shared.contains_key(&shared));
    assert_eq!(mods::used_by(src(&p))[&shared].len(), users.len() - 1);
    assert!(mods::check(&p).iter().all(|i| !i.is_error()));
    // Inline for the last user removes it; shared again from the first
    // user, and the others named it again, it is the original.
    for u in &users[1..] {
        mods::inline(&mut p, u).unwrap();
    }
    assert!(!src(&p).shared.contains_key(&shared));
    assert_eq!(mods::share(&mut p, &users[0], Some(&shared)).unwrap(), shared);
    let classic = load(&root, CLASSIC);
    for u in &users[1..] {
        mods::set(&mut p, u, &mods::get(src(&classic), u).unwrap()).unwrap();
    }
    assert!(files(&p) == original);
    assert!(matches!(mods::share(&mut p, "mod.worm", None), Err(ModError::Refused(_))));
    assert!(mods::inline(&mut p, "weapons/rifle.fire.shot").is_err()); // inline already
}

/// `check` lists every error at once with its path, the unused files'
/// warnings too; a pack that does not parse names each bad file.
#[test]
fn check_lists_every_error_with_its_path() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, CLASSIC);
    let edit = |file: &str, from: &str, to: &str| {
        let path = dir.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{file}: {from}");
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    };
    edit("weapons/bazooka.ron", "speed: 140,", "speed: 140,\n            amount: 5000,");
    edit("weapons/rifle.ron", "Injure(", "Remove,\n                    Injure(");
    edit(
        "weapons/zimm.ron",
        "name: \"Zimm\",",
        "name: \"Zimm\",\n    fire: Special(kind: Jetpack, thrust: 1),\n    notes: \"x\",",
    );
    edit("mod.ron", "\"Rifle only\": (", "\"No such preset\": (");
    std::fs::write(dir.join("gfx/unused.png"), b"not even a png").unwrap();
    // zimm.ron does not parse (`fire` twice): it is named with its place.
    let issues = mods::check_dir(&dir);
    assert!(
        issues
            .iter()
            .any(|i| i.is_error() && i.file.as_deref() == Some("weapons/zimm.ron") && i.line.is_some()),
        "{issues:#?}"
    );
    edit("weapons/zimm.ron", "    fire: Special(kind: Jetpack, thrust: 1),\n", "");
    let issues = mods::check_dir(&dir);
    let at = |path: &str| issues.iter().any(|i| i.is_error() && i.path.as_deref() == Some(path));
    assert!(at("weapons/bazooka.fire.shot.amount"), "{issues:#?}");
    assert!(at("weapons/rifle.fire.shot.projectile.hit_worm[1]"), "{issues:#?}");
    assert!(at(r#"mod.loadouts["No such preset"]"#), "{issues:#?}");
    assert!(
        issues.iter().any(|i| !i.is_error() && i.file.as_deref() == Some("gfx/unused.png")),
        "{issues:#?}"
    );
    assert!(issues.iter().filter(|i| i.is_error()).count() >= 3);
    // Errors first, each once.
    let firsts: Vec<bool> = issues.iter().map(|i| i.is_error()).collect();
    assert!(firsts.windows(2).all(|w| w[0] || !w[1]));
    let unique: BTreeSet<String> = issues.iter().map(|i| i.to_string()).collect();
    assert_eq!(unique.len(), issues.len(), "{issues:#?}");
}

#[test]
fn files_that_do_not_parse_are_listed_together_never_a_panic() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, CLASSIC);
    std::fs::write(dir.join("weapons/bazooka.ron"), "(name: \"Bazooka\",").unwrap();
    std::fs::write(dir.join("projectiles/shotgun.ron"), "(gfx: Pixel([]))").unwrap();
    let issues = mods::check_dir(&dir);
    for file in ["weapons/bazooka.ron", "projectiles/shotgun.ron"] {
        assert!(
            issues.iter().any(|i| i.is_error() && i.file.as_deref() == Some(file)),
            "{file}: {issues:#?}"
        );
    }
    assert!(pack::load(&dir).is_err());
    std::fs::write(dir.join("mod.ron"), [0xff, 0xfe, 0x00]).unwrap();
    assert!(!mods::check_dir(&dir).is_empty());
}
