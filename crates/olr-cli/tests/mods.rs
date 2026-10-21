//! `olr-cli mod get|set|fields|weapon …|projectile …|check` (issue #16)
//! through the binary, on copies of the committed Classic.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs the binary from the repository root (`--content`'s default,
/// `content/packs`, is the committed content).
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).current_dir(repo()).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn ok(args: &[&str]) -> String {
    let o = cli(args);
    assert!(o.status.success(), "{args:?}: {}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn fails(args: &[&str]) -> String {
    let o = cli(args);
    assert!(!o.status.success(), "{args:?} succeeded: {}", text(&o));
    text(&o)
}

/// A copy of Classic made with `pack new mod --from`, in a temporary directory.
fn classic_copy() -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("my-classic").to_str().unwrap().to_string();
    ok(&["pack", "new", "mod", "my-classic", "--out", &dir, "--from", "content/packs/olx-classic"]);
    (tmp, dir)
}

fn read(dir: &str) -> olr_format::mods::ModSource {
    olr_format::mods::read_mod_files(&olr_vfs::DirFs::new(Path::new(dir))).unwrap()
}

#[test]
fn mod_get_and_set_edit_by_path() {
    let (_tmp, d) = classic_copy();
    assert_eq!(ok(&["mod", "get", &d, "mod.worm.gravity"]), "100.0\n");
    assert_eq!(ok(&["mod", "set", &d, "mod.worm.gravity", "120.0"]), "set mod.worm.gravity = 120.0\n");
    ok(&["mod", "set", &d, "weapons/bazooka.fire.shot.speed", "150"]);
    let through = "weapons/bazooka.fire.shot.projectile.spawns.spawn.amount";
    assert_eq!(ok(&["mod", "get", &d, through]), "12\n");
    ok(&["mod", "set", &d, "weapons/bazooka.fire.shot.projectile.trail", "Doomsday"]);
    ok(&["mod", "set", &d, "weapons/bazooka.name", "Rocket"]);
    let m = read(&d);
    assert_eq!(m.worm.gravity, 120.0);
    assert_eq!(m.weapons[0].1.name, "Rocket");
    assert!(m.restrictions.values().all(|p| !p.contains_key("Bazooka")));
    let fields = ok(&["mod", "fields", &d, "weapons/bazooka.fire"]);
    assert!(
        fields.contains("weapons/bazooka.fire.class: Automatic (variants: Automatic, PowerGun"),
        "{fields}"
    );
    assert!(fields.contains("weapons/bazooka.fire.recoil = 15"), "{fields}");
    assert!(ok(&["mod", "fields", &d]).contains("projectiles/bazooka.spawn\n"));
    assert!(ok(&["mod", "--help"]).contains("pack new mod"), "the help names how to make a mod");

    // Refused: what LX56 cannot express, with its path; a bad value; a bad path.
    let before = read(&d);
    let two = "[Explode(damage: 1), Injure(damage: 2)]";
    let e = fails(&["mod", "set", &d, "weapons/bazooka.fire.shot.projectile.hit_worm", two]);
    assert!(e.contains("weapons/bazooka.fire.shot.projectile.hit_worm[1]: more than one effect"), "{e}");
    let e = fails(&["mod", "set", &d, "mod.worm.gravity", "[1]"]);
    assert!(e.contains("mod.worm.gravity"), "{e}");
    assert!(fails(&["mod", "get", &d, "mod.worm.gravty"]).contains("no such field"));
    assert_eq!(read(&d), before);
    ok(&["pack", "check", &d]);
}

#[test]
fn mod_weapon_and_projectile_commands() {
    let (_tmp, d) = classic_copy();
    let out = ok(&["mod", "weapon", "add", &d, "fireball", "--from", "olx-mw-1-0:fireball", "--at", "1"]);
    assert_eq!(out, "added weapon fireball (\"Fireball\") at 1\n");
    assert!(Path::new(&d).join("projectiles/fireball.spawn.spawn.ron").is_file());
    assert!(Path::new(&d).join("gfx/flame-2.png").is_file());
    ok(&["mod", "weapon", "add", &d, "plasma", "--name", "Plasma"]);
    ok(&["mod", "weapon", "move", &d, "plasma", "0"]);
    assert!(ok(&["mod", "weapon", "list", &d]).starts_with("0: plasma \"Plasma\"\n1: bazooka \"Bazooka\"\n"));
    assert_eq!(
        ok(&["mod", "weapon", "copy", &d, "bazooka", "bazooka-2"]),
        "copied weapon bazooka to bazooka-2 (\"Bazooka 2\")\n"
    );
    ok(&["mod", "weapon", "rename", &d, "bazooka", "--name", "Rocket", "--new-id", "rocket"]);
    ok(&["mod", "weapon", "remove", &d, "plasma"]);
    let m = read(&d);
    let ids: Vec<&str> = m.weapons.iter().take(3).map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["rocket", "bazooka-2", "fireball"]);
    let presets = std::fs::read_to_string(Path::new(&d).join("restrictions.ron")).unwrap();
    assert!(
        presets.contains("\"Rocket\"") && !presets.contains("\"Bazooka\"") && !presets.contains("\"Plasma\"")
    );
    assert!(fails(&["mod", "weapon", "add", &d, "rocket"]).contains("already"));
    assert!(fails(&["mod", "weapon", "add", &d, "x", "--from", "no-such-mod:x"]).contains("no-such-mod"));

    let used = ok(&["mod", "projectile", "used-by", &d, "shotgun"]);
    assert_eq!(used, "shotgun: weapons/shotgun.fire.shot, weapons/super-shotgun.fire.shot\n");
    let original = std::fs::read_to_string(Path::new(&d).join("weapons/rocket.ron")).unwrap();
    assert_eq!(
        ok(&["mod", "projectile", "share", &d, "weapons/rocket.fire.shot"]),
        "shared weapons/rocket.fire.shot's projectile as projectiles/rocket\n"
    );
    assert!(
        ok(&["mod", "projectile", "used-by", &d, "rocket"]).starts_with("rocket: weapons/rocket.fire.shot\n")
    );
    ok(&["mod", "projectile", "inline", &d, "weapons/rocket.fire.shot.projectile"]);
    assert_eq!(std::fs::read_to_string(Path::new(&d).join("weapons/rocket.ron")).unwrap(), original);
    assert!(!Path::new(&d).join("projectiles/rocket.ron").exists());
    assert!(ok(&["mod", "check", &d]).ends_with("checked 1 mods: 0 errors, 0 warnings\n"));
}

#[test]
fn mod_check_lists_every_error_with_its_path() {
    let (_tmp, d) = classic_copy();
    let edit = |file: &str, from: &str, to: &str| {
        let path = Path::new(&d).join(file);
        let t = std::fs::read_to_string(&path).unwrap();
        assert!(t.contains(from), "{file}: {from}");
        std::fs::write(&path, t.replacen(from, to, 1)).unwrap();
    };
    edit("weapons/bazooka.ron", "speed: 140,", "speed: 140,\n            amount: 5000,");
    edit("weapons/rifle.ron", "Injure(", "Remove,\n                    Injure(");
    edit("weapons/zimm.ron", "\"sfx/", "\"sfx/missing-");
    std::fs::write(Path::new(&d).join("gfx/unused.png"), b"x").unwrap();
    let out = fails(&["mod", "check", &d]);
    for line in [
        "error: weapons/bazooka.fire.shot.amount: amount 5000 is above 1000",
        "error: weapons/rifle.fire.shot.projectile.hit_worm[1]: more than one effect",
        "error: weapons/zimm.fire.sound: missing asset \"sfx/missing-",
        "warning: gfx/unused.png: asset is never used",
        "checked 1 mods: 3 errors, 1 warnings",
    ] {
        assert!(out.contains(line), "{line:?} missing:\n{out}");
    }
    // A file that does not parse is named with its line and column.
    std::fs::write(Path::new(&d).join("weapons/rifle.ron"), "(name: \"Rifle\",").unwrap();
    let out = fails(&["mod", "check", &d]);
    assert!(out.contains("error: weapons/rifle.ron:"), "{out}");
    assert!(fails(&["mod", "get", &d, "mod.name"]).contains("weapons/rifle.ron"));
}
