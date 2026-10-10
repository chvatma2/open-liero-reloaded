//! Mod packs: `mod.ron`, `weapons/<id>.ron`, `projectiles/<name>.ron`, and the
//! asset files (`gfx/`, `sfx/`) the definitions reference.

use std::collections::BTreeMap;

use rl_data::manifest::{PackKind, PackManifest};
use rl_data::mod_def::{ModDef, ProjectileDef, RopeParams, WeaponDef, WormParams};
use rl_vfs::{ContentSink, ContentSource, VPath, walk};
use serde::{Deserialize, Serialize};

use crate::ron_io::{read_ron, write_ron};
use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const MOD_FILE: &str = "mod.ron";
const WEAPONS_DIR: &str = "weapons";
const PROJECTILES_DIR: &str = "projectiles";

#[derive(Clone, Debug, PartialEq)]
pub struct ModPack {
    pub manifest: PackManifest,
    pub def: ModDef,
    /// Asset files by pack path, such as `gfx/missile.png`.
    pub assets: BTreeMap<VPath, Vec<u8>>,
}

/// On-disk form of `mod.ron`: everything but the weapon and projectile bodies.
#[derive(Serialize, Deserialize)]
struct ModFile {
    name: String,
    worm: WormParams,
    rope: RopeParams,
    /// Weapon ids in order; each is a file `weapons/<id>.ron`.
    weapons: Vec<String>,
}

fn check_name(file: &str, kind: &str, name: &str) -> Result<(), PackError> {
    let ok = !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.' || b == b'_')
        && !name.starts_with('.');
    if ok { Ok(()) } else { Err(PackError::invalid(file, format!("invalid {kind} name {name:?}"))) }
}

pub fn write_mod(sink: &mut dyn ContentSink, pack: &ModPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    write_manifest(sink, &pack.manifest)?;
    let def = &pack.def;
    let file = ModFile {
        name: def.name.clone(),
        worm: def.worm,
        rope: def.rope,
        weapons: def.weapons.iter().map(|w| w.id.clone()).collect(),
    };
    write_ron(sink, MOD_FILE, &file)?;
    for w in &def.weapons {
        check_name(MOD_FILE, "weapon", &w.id)?;
        write_ron(sink, &format!("{WEAPONS_DIR}/{}.ron", w.id), w)?;
    }
    for (name, p) in &def.projectiles {
        check_name(MOD_FILE, "projectile", name)?;
        write_ron(sink, &format!("{PROJECTILES_DIR}/{name}.ron"), p)?;
    }
    for (path, data) in &pack.assets {
        sink.write(path, data)?;
    }
    Ok(())
}

pub fn read_mod(src: &dyn ContentSource) -> Result<ModPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    let file: ModFile = read_ron(src, MOD_FILE)?;
    let mut weapons = Vec::with_capacity(file.weapons.len());
    for id in &file.weapons {
        check_name(MOD_FILE, "weapon", id)?;
        let mut w: WeaponDef = read_ron(src, &format!("{WEAPONS_DIR}/{id}.ron"))?;
        w.id = id.clone();
        weapons.push(w);
    }
    let mut projectiles = BTreeMap::new();
    let mut assets = BTreeMap::new();
    for path in walk(src, &VPath::root())? {
        let s = path.as_str();
        if s == MANIFEST || s == MOD_FILE || s.starts_with("weapons/") {
            continue;
        }
        if let Some(rest) = s.strip_prefix("projectiles/") {
            let name = rest
                .strip_suffix(".ron")
                .ok_or_else(|| PackError::invalid(s, "projectile files must end in .ron"))?;
            check_name(s, "projectile", name)?;
            let p: ProjectileDef = read_ron(src, s)?;
            projectiles.insert(name.to_string(), p);
        } else {
            assets.insert(path.clone(), src.read(&path)?);
        }
    }
    let def = ModDef { name: file.name, worm: file.worm, rope: file.rope, weapons, projectiles };
    Ok(ModPack { manifest, def, assets })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rl_data::image::Rgba;
    use rl_data::manifest::PACK_FORMAT_VERSION;
    use rl_data::mod_def::*;
    use rl_vfs::MemFs;

    pub(crate) fn manifest(kind: PackKind) -> PackManifest {
        PackManifest {
            format_version: PACK_FORMAT_VERSION,
            id: "test-pack".into(),
            name: "Test".into(),
            version: "1".into(),
            kind,
            authors: vec![],
            origin: None,
            license: None,
        }
    }

    pub(crate) fn sample_mod() -> ModPack {
        let bits = ProjectileDef {
            gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(200, 0, 0)] },
            gravity: None,
            dampening: 1.0,
            width: 2,
            height: 2,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef::olx_default(ActionKind::Bounce),
            player_hit: ActionDef { damage: 3, ..ActionDef::olx_default(ActionKind::Injure) },
            event_spawn: None,
        };
        let rocket = ProjectileDef {
            gfx: ProjectileGfx::Image {
                file: "gfx/missile.png".into(),
                rotating: false,
                rot_increment: 0,
                rot_speed: 0,
                use_angle: true,
                use_spec_angle: false,
                angle_images: 16,
                animation: None,
            },
            hit: ActionDef {
                damage: 10,
                spawns_projectiles: true,
                sound: Some("sfx/boom.ogg".into()),
                ..ActionDef::olx_default(ActionKind::Explode)
            },
            event_spawn: Some(SpawnDef { amount: 5, ..SpawnDef::olx_default("bits".into()) }),
            ..bits.clone()
        };
        let weapon = WeaponDef {
            id: "rocket".into(),
            name: "Rocket".into(),
            recharge: 0.5,
            drain: 30.0,
            rof: 0.25,
            laser_sight: false,
            kind: WeaponKind::Projectile {
                class: WeaponClass::Missile,
                recoil: 5,
                sound: Some("sfx/boom.ogg".into()),
                spawn: SpawnDef { amount: 1, speed: 100, ..SpawnDef::olx_default("rocket".into()) },
            },
        };
        ModPack {
            manifest: manifest(PackKind::Mod),
            def: ModDef {
                name: "Test Mod".into(),
                worm: WormParams {
                    angle_speed: 150.0,
                    ground_speed: 8.0,
                    air_speed: 4.0,
                    gravity: 100.0,
                    jump_force: -65.0,
                    air_friction: 0.0,
                    ground_friction: 0.4,
                },
                rope: RopeParams { max_length: 300, rest_length: 27, strength: 4.5 },
                weapons: vec![weapon],
                projectiles: [("bits".to_string(), bits), ("rocket".to_string(), rocket)]
                    .into_iter()
                    .collect(),
            },
            assets: [
                (VPath::new("gfx/missile.png").unwrap(), b"png".to_vec()),
                (VPath::new("sfx/boom.ogg").unwrap(), b"ogg".to_vec()),
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn roundtrip() {
        let pack = sample_mod();
        let mut fs = MemFs::new();
        write_mod(&mut fs, &pack).unwrap();
        assert_eq!(read_mod(&fs).unwrap(), pack);
    }

    #[test]
    fn writes_expected_layout() {
        let mut fs = MemFs::new();
        write_mod(&mut fs, &sample_mod()).unwrap();
        let paths: Vec<String> = fs.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            paths,
            vec![
                "gfx/missile.png",
                "mod.ron",
                "pack.ron",
                "projectiles/bits.ron",
                "projectiles/rocket.ron",
                "sfx/boom.ogg",
                "weapons/rocket.ron"
            ]
        );
    }

    #[test]
    fn writing_twice_is_byte_identical() {
        let (mut a, mut b) = (MemFs::new(), MemFs::new());
        write_mod(&mut a, &sample_mod()).unwrap();
        write_mod(&mut b, &sample_mod()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_unsafe_weapon_id() {
        let mut pack = sample_mod();
        pack.def.weapons[0].id = "../evil".into();
        assert!(write_mod(&mut MemFs::new(), &pack).is_err());
    }

    #[test]
    fn ron_errors_have_positions() {
        let mut fs = MemFs::new();
        write_mod(&mut fs, &sample_mod()).unwrap();
        let fs = fs.with("weapons/rocket.ron", "(\n  name: 5,\n)");
        match read_mod(&fs).unwrap_err() {
            PackError::Ron { file, line, .. } => {
                assert_eq!(file, "weapons/rocket.ron");
                assert_eq!(line, 2);
            }
            e => panic!("unexpected error {e}"),
        }
    }
}
