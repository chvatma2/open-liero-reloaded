//! Turns a parsed [`RawMod`] into a native [`ModPack`]:
//! - weapons get stable ids (slugs of their names);
//! - inline projectiles get names, and identical ones are shared;
//! - images and sounds are found the way OLX finds them and copied into the pack.

use std::collections::BTreeMap;

use rl_data::manifest::{Origin, PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_data::mod_def::{ModDef, ProjectileDef, ProjectileGfx, WeaponDef, WeaponKind};
use rl_data::slug::slug;
use rl_pack::ModPack;
use rl_vfs::{ContentSource, VPath, find_ci, find_ci_any};

use crate::LgsError;
use crate::parse::{ProjNode, RawMod, parse_lgs};

/// The result of importing one mod.
#[derive(Debug)]
pub struct ImportedMod {
    pub pack: ModPack,
    /// Non-fatal findings, such as sounds OLX would also fail to load.
    pub warnings: Vec<String>,
}

/// Imports the mod in directory `mod_dir` of an OLX game directory (`share/gamedir`).
/// Images are looked up in `<mod_dir>/gfx/` then `data/gfx/`, sounds in
/// `<mod_dir>/sfx/` then `data/sounds/`, ignoring case and retrying sounds with
/// `.ogg` (`CGameScript::LoadGSImage`, `LoadGSSample`, `src/sound/Sounds.cpp:113`).
pub fn import_mod(olx: &dyn ContentSource, mod_dir: &VPath, origin: Origin) -> Result<ImportedMod, LgsError> {
    let script = find_ci(olx, &mod_dir.join("script.lgs")?)
        .ok_or_else(|| LgsError::MissingFile(format!("{mod_dir}/script.lgs")))?;
    let raw = parse_lgs(&olx.read(&script)?)?;
    let mut cx =
        Converter { olx, mod_dir, assets: BTreeMap::new(), sources: BTreeMap::new(), warnings: Vec::new() };
    let def = cx.convert(raw)?;
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: format!("olx-{}", slug(mod_dir.file_name())),
        name: def.name.clone(),
        version: "1".to_string(),
        kind: PackKind::Mod,
        authors: Vec::new(),
        origin: Some(origin),
        license: None,
    };
    Ok(ImportedMod { pack: ModPack { manifest, def, assets: cx.assets }, warnings: cx.warnings })
}

struct Converter<'a> {
    olx: &'a dyn ContentSource,
    mod_dir: &'a VPath,
    assets: BTreeMap<VPath, Vec<u8>>,
    /// Pack path -> OLX path it was copied from, to detect conflicts.
    sources: BTreeMap<VPath, VPath>,
    warnings: Vec<String>,
}

#[derive(Clone, Copy)]
enum AssetKind {
    Image,
    Sound,
}

impl Converter<'_> {
    fn convert(&mut self, raw: RawMod) -> Result<ModDef, LgsError> {
        if raw.trailing_bytes > 0 {
            self.warnings.push(format!("{} bytes after the worm block were ignored", raw.trailing_bytes));
        }
        let mut weapons = Vec::with_capacity(raw.weapons.len());
        let mut projectiles = BTreeMap::new();
        let mut interned: Vec<(ProjectileDef, String)> = Vec::new();
        let mut used_ids = BTreeMap::new();
        for rw in raw.weapons {
            let mut w: WeaponDef = rw.def;
            w.id = unique(&slug(&w.name), |id| used_ids.contains_key(id));
            used_ids.insert(w.id.clone(), ());
            if let WeaponKind::Projectile { sound, spawn, .. } = &mut w.kind {
                *sound = self.sound(sound.take(), &format!("weapon {:?}", w.name))?;
                let node = rw.projectile.expect("projectile weapons always carry a projectile");
                spawn.projectile = self.intern(&node, &w.id, &mut projectiles, &mut interned)?;
            }
            weapons.push(w);
        }
        Ok(ModDef { name: raw.name, worm: raw.worm, rope: raw.rope, weapons, projectiles })
    }

    /// Names `node` (and its children, depth first), reusing the name of an
    /// identical definition seen before.
    fn intern(
        &mut self,
        node: &ProjNode,
        base: &str,
        out: &mut BTreeMap<String, ProjectileDef>,
        interned: &mut Vec<(ProjectileDef, String)>,
    ) -> Result<String, LgsError> {
        let mut def = node.def.clone();
        if let (Some(child), Some(spawn)) = (&node.event_child, def.event_spawn.as_mut()) {
            spawn.projectile = self.intern(child, &format!("{base}.spawn"), out, interned)?;
        }
        if let (Some(child), Some(spawn)) = (&node.trail_child, def.trail.spawn.as_mut()) {
            spawn.projectile = self.intern(child, &format!("{base}.trail"), out, interned)?;
        }
        let ctx = format!("projectile {base}");
        if let ProjectileGfx::Image { file, .. } = &mut def.gfx {
            *file = self.image(file, &ctx)?;
        }
        def.hit.sound = self.sound(def.hit.sound.take(), &ctx)?;
        if let Some(a) = def.timer.action.as_mut() {
            a.sound = self.sound(a.sound.take(), &ctx)?;
        }
        def.player_hit.sound = self.sound(def.player_hit.sound.take(), &ctx)?;

        if let Some((_, name)) = interned.iter().find(|(d, _)| *d == def) {
            return Ok(name.clone());
        }
        let name = unique(base, |n| out.contains_key(n));
        out.insert(name.clone(), def.clone());
        interned.push((def, name.clone()));
        Ok(name)
    }

    fn image(&mut self, name: &str, ctx: &str) -> Result<String, LgsError> {
        match self.asset(AssetKind::Image, name)? {
            Some(p) => Ok(p),
            None => Err(LgsError::MissingImage(format!("{name} ({ctx})"))),
        }
    }

    /// Like OLX, a sound that cannot be found is dropped (`UseSound = false`).
    fn sound(&mut self, name: Option<String>, ctx: &str) -> Result<Option<String>, LgsError> {
        let Some(name) = name else { return Ok(None) };
        let found = self.asset(AssetKind::Sound, &name)?;
        if found.is_none() {
            self.warnings.push(format!("sound {name:?} not found ({ctx}); dropped as OLX does"));
        }
        Ok(found)
    }

    /// Finds an OLX asset, copies it into the pack, and returns its pack path.
    fn asset(&mut self, kind: AssetKind, name: &str) -> Result<Option<String>, LgsError> {
        let Ok(rel) = VPath::from_olx(name) else {
            self.warnings.push(format!("unsafe asset name {name:?} ignored"));
            return Ok(None);
        };
        let (mod_sub, data_dir, pack_dir) = match kind {
            AssetKind::Image => ("gfx", "data/gfx", "gfx"),
            AssetKind::Sound => ("sfx", "data/sounds", "sfx"),
        };
        let mut names = vec![rel.clone()];
        if let AssetKind::Sound = kind {
            let s = rel.as_str();
            let stem = s.rsplit_once('.').map_or(s, |(stem, _)| stem);
            if !s.to_ascii_lowercase().ends_with(".ogg") {
                names.push(VPath::new(&format!("{stem}.ogg"))?);
            }
        }
        let mut candidates = Vec::new();
        for base in [self.mod_dir.join(mod_sub)?, VPath::new(data_dir)?] {
            for n in &names {
                candidates.push(base.join(n.as_str())?);
            }
        }
        let Some(found) = find_ci_any(self.olx, &candidates) else { return Ok(None) };
        let file_name = found.file_name().to_ascii_lowercase();
        let pack_path = VPath::new(pack_dir)?.join(&file_name)?;
        match self.sources.get(&pack_path) {
            Some(prev) if *prev != found => return Err(LgsError::AssetConflict(pack_path.to_string())),
            Some(_) => {}
            None => {
                self.assets.insert(pack_path.clone(), self.olx.read(&found)?);
                self.sources.insert(pack_path.clone(), found);
            }
        }
        Ok(Some(pack_path.to_string()))
    }
}

/// `base`, or `base-2`, `base-3`, … whichever is not yet taken.
fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}-{i}")).find(|c| !taken(c)).expect("unbounded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::tests::Lgs;
    use rl_vfs::MemFs;

    fn origin() -> Origin {
        Origin { source: "test".into(), path: "Tiny".into(), revision: "0".into() }
    }

    fn gamedir(script: Vec<u8>) -> MemFs {
        MemFs::new()
            .with("Tiny/SCRIPT.LGS", script)
            .with("Tiny/Gfx/Shared.png", "mod-image")
            .with("data/sounds/boom.ogg", "ogg-from-data")
            .with("data/gfx/shared.png", "data-image")
    }

    fn two_weapons() -> Vec<u8> {
        // Both weapons fire an identical projectile that spawns an identical child.
        let child = |b: Lgs| b.pixel_projectile(3, None);
        Lgs::header("Tiny", 2)
            .projectile_weapon("Rocket", Some("Boom.wav"))
            .pixel_projectile(12, Some(child))
            .projectile_weapon("Rocket", Some("missing.wav"))
            .pixel_projectile(12, Some(child))
            .footer()
            .0
    }

    #[test]
    fn ids_names_and_dedup() {
        let fs = gamedir(two_weapons());
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let d = &m.pack.def;
        assert_eq!(m.pack.manifest.id, "olx-tiny");
        assert_eq!(d.weapons.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), vec!["rocket", "rocket-2"]);
        assert_eq!(d.projectiles.keys().collect::<Vec<_>>(), vec!["rocket", "rocket.spawn"]);
        assert_eq!(d.weapons[1].projectile(), Some("rocket"));
        assert_eq!(d.projectiles["rocket"].event_spawn.as_ref().unwrap().projectile, "rocket.spawn");
    }

    #[test]
    fn sounds_resolve_like_olx_and_missing_ones_are_dropped() {
        let fs = gamedir(two_weapons());
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        assert_eq!(m.pack.def.weapons[0].sound(), Some("sfx/boom.ogg"));
        assert_eq!(m.pack.assets[&VPath::new("sfx/boom.ogg").unwrap()], b"ogg-from-data");
        assert_eq!(m.pack.def.weapons[1].sound(), None);
        assert!(m.warnings.iter().any(|w| w.contains("missing.wav")), "{:?}", m.warnings);
    }

    #[test]
    fn images_prefer_the_mod_directory() {
        let script = Lgs::header("Tiny", 1).projectile_weapon("Gun", None);
        // Image projectile: type 1, no timer, no trail, no gravity, damp 1.
        let script = script.i(1).f(0.0).f(0.0).i(0).i(0).f(1.0).s("shared.PNG");
        let script = script.i(0).i(0).i(0).i(0).i(0).i(0); // rotating, inc, speed, angle, spec angle, animating
        let script = script.i(8).i(8).i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0).footer().0; // hit nothing, plyhit nothing, exp, touch
        let fs = gamedir(script);
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let ProjectileGfx::Image { file, .. } = &m.pack.def.projectiles["gun"].gfx else { panic!("image") };
        assert_eq!(file, "gfx/shared.png");
        assert_eq!(m.pack.assets[&VPath::new("gfx/shared.png").unwrap()], b"mod-image");
    }

    #[test]
    fn missing_image_is_an_error() {
        let script = Lgs::header("Tiny", 1).projectile_weapon("Gun", None);
        let script = script.i(1).f(0.0).f(0.0).i(0).i(0).f(1.0).s("nope.png").i(0).i(0).i(0).i(0).i(0).i(0);
        let script = script.i(8).i(8).i(0).i(0).i(0).i(0).i(0).i(0).i(0).i(0).footer().0;
        let err = import_mod(&gamedir(script), &VPath::new("Tiny").unwrap(), origin()).unwrap_err();
        assert!(matches!(err, LgsError::MissingImage(ref n) if n.contains("nope.png")), "{err}");
    }

    #[test]
    fn unsafe_asset_names_never_escape() {
        let script = Lgs::header("Tiny", 1).projectile_weapon("Gun", Some("..\\..\\secret.ogg"));
        let script = script.pixel_projectile(1, None).footer().0;
        let fs = gamedir(script).with("secret.ogg", "do not copy");
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        assert_eq!(m.pack.def.weapons[0].sound(), None);
        assert!(m.pack.assets.values().all(|v| v != b"do not copy"));
    }

    #[test]
    fn imported_pack_validates_and_roundtrips() {
        let fs = gamedir(two_weapons());
        let m = import_mod(&fs, &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let mut out = MemFs::new();
        rl_pack::write_mod(&mut out, &m.pack).unwrap();
        assert_eq!(rl_pack::validate(&out), vec![]);
        assert_eq!(rl_pack::read_mod(&out).unwrap(), m.pack);
    }

    #[test]
    fn non_ascii_names_get_unique_ids() {
        let script = Lgs::header("Tiny", 2);
        let script = script.projectile_weapon("\u{e9}\u{e9}", None).pixel_projectile(1, None);
        let script = script.projectile_weapon("!!", None).pixel_projectile(2, None).footer().0;
        let m = import_mod(&gamedir(script), &VPath::new("Tiny").unwrap(), origin()).unwrap();
        let ids: Vec<&str> = m.pack.def.weapons.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, vec!["unnamed", "unnamed-2"]);
    }

    #[test]
    fn missing_script_is_reported() {
        let err =
            import_mod(&MemFs::new().with("x/a.txt", ""), &VPath::new("x").unwrap(), origin()).unwrap_err();
        assert!(matches!(err, LgsError::MissingFile(_)));
    }
}
