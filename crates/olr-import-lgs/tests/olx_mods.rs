//! Imports the real OLX mods in scope when `OLR_OLX_GAMEDIR` points at an OLX
//! `share/gamedir`; `cargo xtask check` sets it after `fetch-olx-content`.

use olr_data::manifest::Origin;
use olr_vfs::{DirFs, MemFs, VPath};

fn gamedir() -> Option<DirFs> {
    match std::env::var("OLR_OLX_GAMEDIR") {
        Ok(dir) => Some(DirFs::new(dir)),
        Err(_) => {
            eprintln!("OLR_OLX_GAMEDIR not set; skipping real-mod import test");
            None
        }
    }
}

fn check(dir: &str, weapons: usize, name: &str) {
    let Some(fs) = gamedir() else { return };
    let origin = Origin { source: "OpenLieroX".into(), path: dir.into(), revision: "test".into() };
    let m = olr_import_lgs::import_mod(&fs, &VPath::new(dir).unwrap(), origin).unwrap();
    assert_eq!(m.pack.def.name, name);
    assert_eq!(m.pack.def.weapons.len(), weapons);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    let mut out = MemFs::new();
    olr_pack::write_mod(&mut out, &m.pack).unwrap();
    let errors: Vec<_> =
        olr_pack::validate(&out).into_iter().filter(|d| d.severity == olr_pack::Severity::Error).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(olr_pack::read_mod(&out).unwrap(), m.pack);
}

#[test]
fn liero_1_0() {
    check("Classic", 38, "Liero v1.0");
}

#[test]
fn modern_warfare_1_0() {
    check("MW 1.0", 50, "Modern Warfare1.0");
}

/// The enabled weapons of `preset` by name, in the mod's weapon order.
fn enabled(def: &olr_data::mod_def::ModDef, preset: &str) -> Vec<String> {
    olr_data::restrictions::enabled_weapons(def, preset)
        .into_iter()
        .map(|id| def.weapons[id as usize].name.clone())
        .collect()
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    v.sort();
    v
}

/// What each bundled preset enables, from the tables of
/// `docs/porting/olx-notes/bots-and-deathmatch.md` §2.5 (OLX's `CWpnRest`
/// after `updateList` against the mod's weapons).
#[test]
fn wps_presets_enable_what_olx_enables() {
    let Some(fs) = gamedir() else { return };
    let origin = || Origin { source: "OpenLieroX".into(), path: "x".into(), revision: "test".into() };
    let classic =
        olr_import_lgs::import_mod(&fs, &VPath::new("Classic").unwrap(), origin()).unwrap().pack.def;
    let presets: Vec<&str> = classic.restrictions.keys().map(String::as_str).collect();
    assert_eq!(
        presets,
        [
            "Bonus only",
            "Firebird",
            "Restricted 100lt",
            "Retro Classic",
            "Retro Combo",
            "Retro Combo Unlimited",
            "Rifle only",
            "Standard 100lt",
            "Standard 20lt"
        ]
    );
    let table: [(&str, &[&str]); 8] = [
        (
            "Standard 100lt",
            &[
                "Blaster",
                "Cannon",
                "Chaingun",
                "Dart",
                "Doomsday",
                "Handgun",
                "Mini Rockets",
                "Napalm",
                "Shotgun",
                "Super Shotgun",
                "Uzi",
                "Winchester",
            ],
        ),
        ("Standard 20lt", &["Gauss Gun", "Rifle"]),
        ("Restricted 100lt", &["Cannon", "Chaingun", "Doomsday", "Handgun", "Napalm", "Super Shotgun"]),
        ("Rifle only", &["Rifle"]),
        ("Retro Combo", &["Gauss Gun", "Rifle"]),
        (
            "Retro Combo Unlimited",
            &["Bazooka", "Gauss Gun", "Mini Nuke", "Napalm", "Rifle", "Spikeballs", "Zimm"],
        ),
        (
            "Retro Classic",
            &[
                "Bazooka",
                "Blaster",
                "Cannon",
                "Chiquita Bomb",
                "Dart",
                "Doomsday",
                "Explosives",
                "Grasshopper",
                "Grenade",
                "Mini Rockets",
                "Napalm",
                "Shotgun",
                "Super Shotgun",
                "Uzi",
                "Winchester",
            ],
        ),
        (
            "Firebird",
            &[
                "Bazooka",
                "Cannon",
                "Chaingun",
                "Dart",
                "Doomsday",
                "Flamer",
                "Grasshopper",
                "Handgun",
                "Shotgun",
                "Super Shotgun",
                "Uzi",
            ],
        ),
    ];
    for (preset, want) in table {
        let mut got = enabled(&classic, preset);
        got.sort();
        assert_eq!(got, sorted(want), "Classic {preset}");
    }
    // `Bonus only`: nothing is selectable, every weapon may be a bonus.
    assert!(enabled(&classic, "Bonus only").is_empty());
    let rules = olr_data::restrictions::WeaponRules::new(&classic, "Bonus only");
    assert_eq!(rules.bonus_weapons(), (0..38).collect::<Vec<i32>>());
    // OLX's default; the setting's value may carry `.wps` and any case.
    assert_eq!(enabled(&classic, "standard 100LT.wps").len(), 12);
    // A file that is not there leaves the list empty: every weapon enabled.
    assert_eq!(enabled(&classic, "No such preset").len(), 38);

    let mw = olr_import_lgs::import_mod(&fs, &VPath::new("MW 1.0").unwrap(), origin()).unwrap().pack.def;
    let presets: Vec<&str> = mw.restrictions.keys().map(String::as_str).collect();
    assert_eq!(presets, ["Mortar Only", "Standard 100lt"]);
    let mut got = enabled(&mw, "Standard 100lt");
    got.sort();
    assert_eq!(
        got,
        sorted(&[
            "Cluster Grenade",
            "DoomRockets",
            "Electro Gun",
            "Flamer",
            "Poison Rifle",
            "Shockrifle",
            "Twin Rockets",
            "Twin Shotgun",
            "Toxic Waste",
            "Volcanic Gun",
            "Hellfire",
            "Napalm Grenade",
            "Plasma Launcher",
            "Rocket Launcher",
            "Ripper",
        ])
    );
    // Nothing in MW's presets is "bonus only" for MW's own weapons.
    assert_eq!(olr_data::restrictions::WeaponRules::new(&mw, "Standard 100lt").bonus_weapons().len(), 15);
    assert_eq!(enabled(&mw, "Mortar Only"), ["Mortar Launcher"]);
}
