//! Mod packs (format 5, over `olr-format`): `mod.ron`, `weapons/<id>.ron`,
//! `projectiles/<id>.ron` (shared projectiles), the weapon restriction
//! presets (`restrictions.ron`, when the mod has any) and the asset files
//! (`gfx/`, `sfx/`) the definitions reference. `gfx/*.png` are alpha
//! images: the importer keys them once, nothing keys at load.

use std::collections::BTreeMap;

use olr_data::manifest::{PackKind, PackManifest};
use olr_data::mod_def::ModDef;
use olr_format::mods::{compile_mod, lift_mod, read_mod_files, write_mod_files};
use olr_vfs::{ContentSink, ContentSource, VPath, walk};

use crate::{MANIFEST, PackError, read_manifest, write_manifest};

pub const MOD_FILE: &str = "mod.ron";
/// The mod's weapon restriction presets ([`ModDef::restrictions`]), written
/// only when it has any; a pack without it has none.
pub const RESTRICTIONS_FILE: &str = olr_format::mods::RESTRICTIONS_FILE;

#[derive(Clone, Debug, PartialEq)]
pub struct ModPack {
    pub manifest: PackManifest,
    pub def: ModDef,
    /// The mod's look by authoring key (look system); not written by
    /// [`write_mod`] (the importers write none).
    pub look: olr_format::mods::ModLookSource,
    /// Asset files by pack path, such as `gfx/missile.png`.
    pub assets: BTreeMap<VPath, Vec<u8>>,
}

/// Writes the pack in format 5 (`olr-format`'s mod files); the assets are
/// written as they are.
pub fn write_mod(sink: &mut dyn ContentSink, pack: &ModPack) -> Result<(), PackError> {
    if pack.manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    let src = lift_mod(&pack.def).map_err(|e| PackError::invalid(MOD_FILE, e.to_string()))?;
    write_manifest(sink, &pack.manifest)?;
    write_mod_files(sink, &src)?;
    for (path, data) in &pack.assets {
        sink.write(path, data)?;
    }
    Ok(())
}

/// True for the files `olr-format` owns (everything else is an asset).
pub(crate) fn is_definition(path: &str) -> bool {
    path == MANIFEST
        || path == MOD_FILE
        || path == RESTRICTIONS_FILE
        || path.starts_with("weapons/")
        || path.starts_with("projectiles/")
}

pub fn read_mod(src: &dyn ContentSource) -> Result<ModPack, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    let source = read_mod_files(src)?;
    let def = compile_mod(&source)?;
    let look = olr_format::mods::compile_mod_look(&source)?;
    let mut assets = BTreeMap::new();
    for path in walk(src, &VPath::root())? {
        if !is_definition(path.as_str()) {
            assets.insert(path.clone(), crate::ron_io::read_file(src, &path)?);
        }
    }
    Ok(ModPack { manifest, def, look, assets })
}

/// The mod's definitions (`ModDef`: name, weapons, presets, shipped
/// loadouts) without its assets: no image or sound is read (a menu's list).
pub fn read_mod_def(src: &dyn ContentSource) -> Result<ModDef, PackError> {
    let manifest = read_manifest(src)?;
    if manifest.kind != PackKind::Mod {
        return Err(PackError::invalid(MANIFEST, "manifest kind is not Mod"));
    }
    Ok(compile_mod(&read_mod_files(src)?)?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use olr_data::image::Rgba;
    use olr_data::manifest::PACK_FORMAT_VERSION;
    use olr_data::mod_def::*;
    use olr_data::restrictions::WeaponState;
    use olr_vfs::MemFs;

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
                restrictions: [(
                    "Standard 100lt".to_string(),
                    [("Rocket".to_string(), WeaponState::Banned), ("Zimm".to_string(), WeaponState::Bonus)]
                        .into_iter()
                        .collect(),
                )]
                .into_iter()
                .collect(),
                loadouts: [(
                    "Standard 100lt".to_string(),
                    olr_data::loadouts::ShippedLoadout { loading_time: 40, unique: true },
                )]
                .into(),
            },
            assets: [
                (VPath::new("gfx/missile.png").unwrap(), b"png".to_vec()),
                (VPath::new("sfx/boom.ogg").unwrap(), b"ogg".to_vec()),
            ]
            .into_iter()
            .collect(),
            look: Default::default(),
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
                "restrictions.ron",
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
    fn a_mod_without_presets_has_no_restrictions_file() {
        let mut pack = sample_mod();
        pack.def.restrictions.clear();
        pack.def.loadouts.clear();
        let mut fs = MemFs::new();
        write_mod(&mut fs, &pack).unwrap();
        assert!(fs.files().all(|(p, _)| p.as_str() != RESTRICTIONS_FILE));
        assert_eq!(read_mod(&fs).unwrap(), pack);
        assert!(read_mod_def(&fs).unwrap().restrictions.is_empty());
    }

    #[test]
    fn definitions_read_without_assets() {
        let pack = sample_mod();
        let mut fs = MemFs::new();
        write_mod(&mut fs, &pack).unwrap();
        // The loadouts and presets come back; an asset is never read.
        let fs = fs.with("gfx/missile.png", b"not even a png".to_vec());
        assert_eq!(read_mod_def(&fs).unwrap(), pack.def);
        let fs = fs.with(RESTRICTIONS_FILE, b"[".to_vec());
        assert!(read_mod_def(&fs).is_err());
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
            PackError::Format(olr_format::FormatError::Parse { file, line, .. }) => {
                assert_eq!(file, "weapons/rocket.ron");
                assert_eq!(line, 2);
            }
            e => panic!("unexpected error {e}"),
        }
    }
}
