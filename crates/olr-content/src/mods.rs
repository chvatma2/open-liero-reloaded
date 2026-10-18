//! Mod packs as a [`SimMod`] with their images and sounds.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Context as _;
use olr_data::image::RgbaImage;
use olr_data::manifest::PackKind;
use olr_data::mod_def::{ProjectileGfx, RopeParams, WormParams};
use olr_data::restrictions::{Preset, WeaponRules};
use olr_sim::{GAME_SOUND_PREFIX, SimMod};
use olr_vfs::VPath;

use crate::Content;
use crate::placeholder::{PLACEHOLDER_SIZE, placeholder_image};

/// A mod ready to play and draw.
#[derive(Clone, Debug)]
pub struct LoadedMod {
    pub sim: Arc<SimMod>,
    /// Every PNG in the pack, decoded, by pack path (such as
    /// `gfx/missile.png`, the projectiles' `file`); a projectile image that
    /// is missing or unreadable is a one-frame placeholder.
    pub images: BTreeMap<String, RgbaImage>,
    /// Sound bytes by [`SimMod::sounds`] name: the mod's from its pack, OLX's
    /// game sounds (`olx:` names) from the core pack. A missing sound is
    /// absent: silence.
    pub sounds: BTreeMap<String, Vec<u8>>,
    /// Placeholders and silences, to log once.
    pub warnings: Vec<String>,
    /// The mod's name (`ModDef::name`, OLX's `ModInfo::name`, such as `Liero v1.0`).
    pub name: String,
    /// The mod's weapon restriction presets (`ModDef::restrictions`).
    pub restrictions: BTreeMap<String, Preset>,
    /// The mod's worm physics (`ModDef::worm`): every match of the mod plays
    /// with them ([`crate::match_setup`], `SimSettings::with_mod_physics`).
    pub worm: WormParams,
    /// The mod's ninja rope (`ModDef::rope`), applied the same way.
    pub rope: RopeParams,
    /// The mod's look (look system), indexed like `SimMod::projectiles`
    /// and `SimMod::weapons`.
    pub look: olr_look::ModLook,
}

impl LoadedMod {
    /// The restrictions a match of this mod plays with when its
    /// `WeaponRestrictionsFile` is `file` (such as
    /// [`olr_data::restrictions::DEFAULT_PRESET`]): OLX's `CWpnRest` after
    /// `loadList` and `updateList`.
    pub fn weapon_rules(&self, file: &str) -> WeaponRules {
        let weapons = self.sim.weapons().iter().map(|w| w.name.as_str());
        WeaponRules::from_parts(&self.name, weapons, &self.restrictions, file)
    }
}

/// Every image projectile must know its image's frame count: OLX's client
/// does, and without it `ANI_ONCE` would never end a projectile.
pub fn check_frames(m: &SimMod) -> anyhow::Result<()> {
    for p in m.projectiles() {
        if let ProjectileGfx::Image { file, .. } = &p.def.gfx {
            anyhow::ensure!(p.frames.is_some(), "projectile {} has no image {file:?}", p.name);
        }
    }
    Ok(())
}

/// Decodes the pack's PNGs: the images, and frame counts `width / height`
/// (spec §18 D3) of those with a height. Unreadable ones are `Err` entries.
fn decode_pngs(pack: &olr_pack::ModPack) -> Vec<(String, Result<RgbaImage, String>)> {
    pack.assets
        .iter()
        .filter(|(path, _)| path.as_str().ends_with(".png"))
        .map(|(path, bytes)| (path.as_str().to_string(), olr_pack::image_io::decode_rgba(bytes)))
        .collect()
}

fn frames_of(img: &RgbaImage) -> Option<u32> {
    (img.height() > 0).then(|| img.width() / img.height())
}

impl Content {
    fn mod_pack(&self, id: &str) -> anyhow::Result<olr_pack::ModPack> {
        olr_pack::read_mod(&self.pack(id, PackKind::Mod)?).with_context(|| format!("mod pack {id}"))
    }

    /// Mod pack `id` compiled for the simulation as OLX's client loads it
    /// (what the oracle replays): an unreadable PNG, or an image projectile
    /// whose image is missing or has height 0, is an error.
    pub fn sim_mod(&self, id: &str) -> anyhow::Result<SimMod> {
        let pack = self.mod_pack(id)?;
        let mut frames = BTreeMap::new();
        for (path, img) in decode_pngs(&pack) {
            let img = img.map_err(|e| anyhow::anyhow!("{id}/{path}: {e}"))?;
            if let Some(n) = frames_of(&img) {
                frames.insert(path, n);
            }
        }
        let m = SimMod::compile(&pack.def, &frames).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
        check_frames(&m).with_context(|| format!("mod pack {id}"))?;
        Ok(m)
    }

    /// The restrictions of mod pack `id` with `WeaponRestrictionsFile` `file`
    /// ([`LoadedMod::weapon_rules`]), for callers of [`Content::sim_mod`].
    pub fn weapon_rules(&self, id: &str, file: &str) -> anyhow::Result<WeaponRules> {
        let pack = self.mod_pack(id)?;
        Ok(WeaponRules::new(&pack.def, file))
    }

    /// Mod pack `id` ready to play: like [`Content::sim_mod`], except that a
    /// missing or odd projectile image becomes a one-frame placeholder and a
    /// missing sound silence, each with a warning. Errors are only for what
    /// cannot run: an unreadable pack or a mod `SimMod::compile` refuses.
    pub fn load_mod(&self, id: &str) -> anyhow::Result<LoadedMod> {
        let pack = self.mod_pack(id)?;
        let mut warnings = Vec::new();
        let (mut images, mut frames) = (BTreeMap::new(), BTreeMap::new());
        for (path, img) in decode_pngs(&pack) {
            match img {
                Ok(img) => {
                    if let Some(n) = frames_of(&img) {
                        frames.insert(path.clone(), n);
                    }
                    images.insert(path, img);
                }
                Err(e) => warnings.push(format!("{id}/{path}: {e}")),
            }
        }
        for p in pack.def.projectiles.values() {
            if let ProjectileGfx::Image { file, .. } = &p.gfx
                && !frames.contains_key(file)
            {
                warnings.push(format!("{id}/{file}: missing or unreadable, a placeholder instead"));
                images.insert(file.clone(), placeholder_image(PLACEHOLDER_SIZE, PLACEHOLDER_SIZE));
                frames.insert(file.clone(), 1);
            }
        }
        let sim = SimMod::compile(&pack.def, &frames).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
        let mut sounds = BTreeMap::new();
        for name in sim.sounds() {
            let bytes = if name.starts_with(GAME_SOUND_PREFIX) {
                // The core pack already warned about its missing sounds.
                self.core().game_sound(name).map(<[u8]>::to_vec)
            } else {
                let found = VPath::new(name).ok().and_then(|p| pack.assets.get(&p).cloned());
                if found.is_none() {
                    warnings.push(format!("{id}/{name}: missing, silence instead"));
                }
                found
            };
            match bytes.map(|b| (crate::check_sound(&b), b)) {
                Some((Ok(()), bytes)) => {
                    sounds.insert(name.clone(), bytes);
                }
                Some((Err(e), _)) => warnings.push(format!("{id}/{name}: {e}: silence instead")),
                None => {}
            }
        }
        let look = olr_look::ModLook {
            projectiles: sim
                .projectiles()
                .iter()
                .map(|p| pack.look.projectiles.get(&p.name).cloned())
                .collect(),
            weapons: pack.look.weapons.clone(),
        };
        let (name, restrictions) = (pack.def.name, pack.def.restrictions);
        let (worm, rope) = (pack.def.worm, pack.def.rope);
        Ok(LoadedMod { sim: Arc::new(sim), images, sounds, warnings, name, restrictions, worm, rope, look })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::mod_def::*;

    #[test]
    fn an_image_projectile_without_its_image_is_an_error() {
        let gfx = ProjectileGfx::Image {
            file: "gfx/flame.png".into(),
            rotating: false,
            rot_increment: 0,
            rot_speed: 0,
            use_angle: false,
            use_spec_angle: false,
            angle_images: 0,
            animation: Some(Animation { rate: 10.0, kind: AnimKind::Once }),
        };
        let flame = ProjectileDef {
            gfx,
            gravity: None,
            dampening: 1.0,
            width: 4,
            height: 4,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef::olx_default(ActionKind::Explode),
            player_hit: ActionDef::olx_default(ActionKind::Injure),
            event_spawn: None,
        };
        let def = ModDef {
            name: "test".into(),
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
            weapons: vec![WeaponDef {
                id: "flamer".into(),
                name: "Flamer".into(),
                recharge: 1.0,
                drain: 1.0,
                rof: 0.01,
                laser_sight: false,
                kind: WeaponKind::Projectile {
                    class: WeaponClass::Automatic,
                    recoil: 0,
                    sound: None,
                    spawn: SpawnDef { amount: 1, ..SpawnDef::olx_default("flame".into()) },
                },
            }],
            projectiles: [("flame".to_string(), flame)].into_iter().collect(),
            restrictions: Default::default(),
        };
        let missing = SimMod::compile(&def, &BTreeMap::new()).unwrap();
        let err = check_frames(&missing).unwrap_err().to_string();
        assert!(err.contains("flamer") && err.contains("gfx/flame.png"), "{err}");
        let found = SimMod::compile(&def, &[("gfx/flame.png".to_string(), 7)].into_iter().collect()).unwrap();
        check_frames(&found).unwrap();
    }
}
