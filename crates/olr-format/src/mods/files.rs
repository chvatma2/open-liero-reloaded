//! A pack's mod files: `mod.ron`, `weapons/<id>.ron`,
//! `projectiles/<id>.ron` and `restrictions.ron`. Other paths
//! (`pack.ron`, assets) belong to `olr-pack` and are ignored here.

use std::collections::BTreeSet;

use olr_vfs::{ContentSink, ContentSource, VPath, walk};
use serde::{Deserialize, Serialize};

use super::compile::{MAX_SHARED, MOD_FILE, shared_file, valid_id, weapon_file};
use super::{ModSource, Rope, Worm};
use crate::FormatError;
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};

pub const RESTRICTIONS_FILE: &str = "restrictions.ron";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModFile {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    #[serde(default, skip_serializing_if = "Worm::is_default")]
    worm: Worm,
    #[serde(default, skip_serializing_if = "Rope::is_default")]
    rope: Rope,
    weapons: Vec<String>,
}

pub fn write_mod_files(sink: &mut dyn ContentSink, m: &ModSource) -> Result<(), FormatError> {
    // Validate every id before the first write, so a failure leaves the sink alone.
    let mut ids = BTreeSet::new();
    for (id, _) in &m.weapons {
        if !valid_id(id) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("invalid weapon id {id:?}")));
        }
        if !ids.insert(id.as_str()) {
            return Err(FormatError::invalid(
                MOD_FILE,
                "weapons",
                format!("weapon id {id:?} is listed twice"),
            ));
        }
    }
    for id in m.shared.keys() {
        if !valid_id(id) {
            return Err(FormatError::invalid(&shared_file(id), "", format!("invalid projectile id {id:?}")));
        }
    }
    let file = ModFile {
        name: m.name.clone(),
        notes: m.notes.clone(),
        worm: m.worm.clone(),
        rope: m.rope.clone(),
        weapons: m.weapons.iter().map(|(id, _)| id.clone()).collect(),
    };
    write_text(sink, MOD_FILE, &to_canonical(MOD_FILE, &file)?)?;
    for (id, w) in &m.weapons {
        let f = weapon_file(id);
        write_text(sink, &f, &to_canonical(&f, w)?)?;
    }
    for (id, p) in &m.shared {
        let f = shared_file(id);
        write_text(sink, &f, &to_canonical(&f, p)?)?;
    }
    if !m.restrictions.is_empty() {
        write_text(sink, RESTRICTIONS_FILE, &to_canonical(RESTRICTIONS_FILE, &m.restrictions)?)?;
    }
    Ok(())
}

pub fn read_mod_files(src: &dyn ContentSource) -> Result<ModSource, FormatError> {
    let file: ModFile = from_canonical(MOD_FILE, &read_text(src, MOD_FILE)?)?;
    let mut listed = BTreeSet::new();
    let mut weapons = Vec::with_capacity(file.weapons.len());
    for id in &file.weapons {
        if !valid_id(id) {
            return Err(FormatError::invalid(MOD_FILE, "weapons", format!("invalid weapon id {id:?}")));
        }
        if !listed.insert(id.clone()) {
            return Err(FormatError::invalid(
                MOD_FILE,
                "weapons",
                format!("weapon id {id:?} is listed twice"),
            ));
        }
        let f = weapon_file(id);
        weapons.push((id.clone(), from_canonical(&f, &read_text(src, &f)?)?));
    }
    let io = |file: &str, message: String| FormatError::Io { file: file.into(), message };
    let mut shared = std::collections::BTreeMap::new();
    let mut restrictions = std::collections::BTreeMap::new();
    for path in walk(src, &VPath::root()).map_err(|e| io("", e.to_string()))? {
        let s = path.as_str();
        if let Some(rest) = s.strip_prefix("weapons/") {
            let id = rest.strip_suffix(".ron").unwrap_or(rest);
            if !listed.contains(id) {
                return Err(io(s, "not listed in mod.ron's weapons".into()));
            }
        } else if let Some(rest) = s.strip_prefix("projectiles/") {
            let id = rest.strip_suffix(".ron").filter(|id| !id.contains('/')).ok_or_else(|| {
                io(s, "shared projectiles are `projectiles/<id>.ron`, directly in the folder".into())
            })?;
            if !valid_id(id) {
                return Err(io(s, format!("invalid projectile id {id:?}")));
            }
            if shared.len() >= MAX_SHARED {
                return Err(io(s, format!("more than {MAX_SHARED} shared projectiles")));
            }
            shared.insert(id.to_string(), from_canonical(s, &read_text(src, s)?)?);
        } else if s == RESTRICTIONS_FILE {
            restrictions = from_canonical(s, &read_text(src, s)?)?;
        }
    }
    Ok(ModSource {
        name: file.name,
        notes: file.notes,
        worm: file.worm,
        rope: file.rope,
        weapons,
        shared,
        restrictions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{Fire, Gfx, Projectile, ShotSpawn, Weapon, WeaponClass};
    use olr_vfs::MemFs;

    fn sample() -> ModSource {
        let shot = ShotSpawn {
            speed: 100,
            projectile: Some(Box::new(Projectile::new(Gfx::Pixels(vec![])))),
            ..ShotSpawn::default()
        };
        let w = Weapon {
            name: "Gun".into(),
            notes: String::new(),
            recharge: 0.5,
            drain: 30.0,
            between_shots_s: 0.25,
            laser_sight: false,
            fire: Fire::Projectile { class: WeaponClass::Automatic, recoil: 0, sound: None, shot },
        };
        ModSource {
            name: "Test".into(),
            notes: String::new(),
            worm: Worm { gravity: 100.0, ..Worm::default() },
            rope: Rope::default(),
            weapons: vec![("gun".into(), w)],
            shared: [("shell".to_string(), Projectile::new(Gfx::Pixels(vec![])))].into_iter().collect(),
            restrictions: Default::default(),
        }
    }

    fn files(fs: &MemFs) -> Vec<String> {
        fs.files().map(|(p, _)| p.to_string()).collect()
    }

    #[test]
    fn writes_the_layout_and_reads_it_back() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        assert_eq!(files(&fs), ["mod.ron", "projectiles/shell.ron", "weapons/gun.ron"]);
        assert_eq!(read_mod_files(&fs).unwrap(), sample());
        let text =
            String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "mod.ron").unwrap().1.to_vec()).unwrap();
        assert!(text.contains("gravity: 100.0") && !text.contains("rope"), "{text}");
    }

    #[test]
    fn writing_what_was_read_gives_the_same_bytes() {
        let mut a = MemFs::new();
        write_mod_files(&mut a, &sample()).unwrap();
        let mut b = MemFs::new();
        write_mod_files(&mut b, &read_mod_files(&a).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_listed_weapon_without_its_file_names_it() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let fs = MemFs::new()
            .with("mod.ron", fs.files().find(|(p, _)| p.as_str() == "mod.ron").unwrap().1.to_vec());
        let e = read_mod_files(&fs).unwrap_err();
        assert!(e.to_string().contains("weapons/gun.ron"), "{e}");
    }

    #[test]
    fn an_unlisted_weapon_file_is_an_error() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let fs = fs.with("weapons/stray.ron", "()");
        let e = read_mod_files(&fs).unwrap_err();
        assert!(e.to_string().contains("weapons/stray.ron") && e.to_string().contains("not listed"), "{e}");
    }

    #[test]
    fn a_duplicate_id_and_nested_projectile_folders_are_errors() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let dup = fs.clone().with("mod.ron", "(name: \"T\", weapons: [\"gun\", \"gun\"])");
        assert!(read_mod_files(&dup).unwrap_err().to_string().contains("twice"));
        let nested = fs.clone().with("projectiles/a/b.ron", "(gfx: Pixels([]))");
        assert!(read_mod_files(&nested).unwrap_err().to_string().contains("projectiles/a/b.ron"));
        let stray = fs.with("projectiles/notes.txt", "hi");
        assert!(read_mod_files(&stray).is_err());
    }

    #[test]
    fn bad_ids_fail_before_anything_is_written() {
        let mut dup = sample();
        dup.weapons.push(dup.weapons[0].clone());
        let mut fs = MemFs::new();
        assert!(write_mod_files(&mut fs, &dup).unwrap_err().to_string().contains("twice"));
        assert!(files(&fs).is_empty());
        let mut bad = sample();
        bad.weapons[0].0 = "../evil".into();
        assert!(write_mod_files(&mut fs, &bad).is_err());
        assert!(files(&fs).is_empty());
        let mut bad = sample();
        bad.shared.insert("Bad Id".into(), Projectile::new(Gfx::Pixels(vec![])));
        assert!(write_mod_files(&mut fs, &bad).is_err());
        assert!(files(&fs).is_empty());
    }

    #[test]
    fn assets_and_the_manifest_are_ignored() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        let fs = fs.with("pack.ron", "()").with("gfx/a.png", "png").with("sfx/b.ogg", "ogg");
        assert_eq!(read_mod_files(&fs).unwrap(), sample());
    }

    #[test]
    fn too_many_shared_projectiles_stop_the_reader() {
        let mut fs = MemFs::new();
        write_mod_files(&mut fs, &sample()).unwrap();
        for i in 0..=MAX_SHARED {
            let name = format!("projectiles/p{i}.ron");
            fs = fs.with(name.as_str(), "(gfx: Pixels([]))");
        }
        let e = read_mod_files(&fs).unwrap_err();
        assert!(e.to_string().contains("shared projectiles"), "{e}");
    }
}
