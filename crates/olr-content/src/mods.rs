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

use crate::placeholder::{PLACEHOLDER_SIZE, placeholder_image};
use crate::{Content, WeaponChoice};

/// A mod ready to play and draw.
#[derive(Clone, Debug)]
pub struct LoadedMod {
    pub sim: Arc<SimMod>,
    /// Every PNG in the pack, decoded, by pack path (such as
    /// `gfx/missile.png`, the projectiles' `file`); a projectile image that
    /// is missing or unreadable is a one-frame placeholder.
    pub images: BTreeMap<String, RgbaImage>,
    /// The images' glow masks (issue #19, `gfx/<name>.glow.png`), by the
    /// image's pack path: one value per pixel, row by row, of the image's
    /// size. A mask that does not fit or decode is left out with a warning.
    pub glow: BTreeMap<String, Vec<u8>>,
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
    /// Image `file`'s glowing pixels (issue #19), one image per glow
    /// strength that has any: `(strength, image)`, the strength 1 to
    /// [`crate::GLOW_LEVELS`] (quarters of full glow, as a skin's), each
    /// image the sprite's pixels of that strength (transparent elsewhere).
    /// Drawn over the sprite with that self-light, they show bright
    /// whatever the darkness. Empty without a mask.
    pub fn glow_overlays(&self, file: &str) -> Vec<(usize, RgbaImage)> {
        let (Some(img), Some(mask)) = (self.images.get(file), self.glow.get(file)) else { return Vec::new() };
        glow_overlays(img, mask)
    }

    /// The restrictions a match of this mod plays with when its
    /// `WeaponRestrictionsFile` is `file` (such as
    /// [`olr_data::restrictions::DEFAULT_PRESET`]): OLX's `CWpnRest` after
    /// `loadList` and `updateList`.
    pub fn weapon_rules(&self, file: &str) -> WeaponRules {
        let weapons = self.sim.weapons().iter().map(|w| w.name.as_str());
        WeaponRules::from_parts(&self.name, weapons, &self.restrictions, file)
    }

    /// The rules a match of this mod plays `w` with: a preset's
    /// ([`LoadedMod::weapon_rules`]) or a loadout's
    /// (`WeaponRules::from_loadout`: its weapons, its unique rule).
    pub fn rules(&self, w: &WeaponChoice) -> WeaponRules {
        match w {
            WeaponChoice::Preset(file) => self.weapon_rules(file),
            WeaponChoice::Loadout(l) => {
                WeaponRules::from_loadout(&self.name, self.sim.weapons().iter().map(|w| w.name.as_str()), l)
            }
        }
    }
}

/// `img`'s pixels by glow strength of `mask` (its values row by row; one
/// of another size gives none): [`LoadedMod::glow_overlays`].
pub fn glow_overlays(img: &RgbaImage, mask: &[u8]) -> Vec<(usize, RgbaImage)> {
    let (w, h) = (img.width(), img.height());
    if mask.len() != w as usize * h as usize {
        return Vec::new();
    }
    let mut out: Vec<(usize, RgbaImage)> = Vec::new();
    for level in 1..=crate::GLOW_LEVELS {
        let mut layer: Option<RgbaImage> = None;
        for y in 0..h {
            for x in 0..w {
                let p = img.get(x, y);
                if p.a != 0 && crate::glow_level(mask[(y * w + x) as usize]) == level {
                    layer.get_or_insert_with(|| RgbaImage::new(w, h)).set(x, y, p);
                }
            }
        }
        out.extend(layer.map(|l| (level, l)));
    }
    out
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
/// Glow masks are not images (issue #19): [`LoadedMod::glow`] holds them.
fn decode_pngs(pack: &olr_pack::ModPack) -> Vec<(String, Result<RgbaImage, String>)> {
    pack.assets
        .iter()
        .filter(|(path, _)| path.as_str().ends_with(".png") && !olr_format::mods::is_glow_mask(path.as_str()))
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

    /// The rules mod pack `id` plays `w` with ([`LoadedMod::rules`]), for
    /// callers of [`Content::sim_mod`].
    pub fn rules(&self, id: &str, w: &WeaponChoice) -> anyhow::Result<WeaponRules> {
        let pack = self.mod_pack(id)?;
        Ok(match w {
            WeaponChoice::Preset(file) => WeaponRules::new(&pack.def, file),
            WeaponChoice::Loadout(l) => {
                WeaponRules::from_loadout(&pack.def.name, pack.def.weapons.iter().map(|w| w.name.as_str()), l)
            }
        })
    }

    /// Mod pack `id` ready to play: like [`Content::sim_mod`], except that a
    /// missing or odd projectile image becomes a one-frame placeholder and a
    /// missing sound silence, each with a warning. Errors are only for what
    /// cannot run: an unreadable pack or a mod `SimMod::compile` refuses.
    pub fn load_mod(&self, id: &str) -> anyhow::Result<LoadedMod> {
        self.load_mod_pack(id, self.mod_pack(id)?)
    }

    /// Mod `pack`, read elsewhere (issue #18: the mod editor's unsaved mod,
    /// `olr_edit::pack::mod_pack`), ready to play as [`Content::load_mod`]
    /// makes an installed one: its game sounds from this content's core
    /// pack, its warnings naming `id`.
    pub fn load_mod_pack(&self, id: &str, pack: olr_pack::ModPack) -> anyhow::Result<LoadedMod> {
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
        let mut glow = BTreeMap::new();
        for (path, bytes) in &pack.assets {
            let Some(image) = olr_format::mods::image_of_mask(path.as_str()) else { continue };
            let Some(img) = images.get(&image) else {
                warnings.push(format!("{id}/{path}: a glow mask without its image"));
                continue;
            };
            match olr_format::mods::decode_glow_mask(bytes, (img.width(), img.height())) {
                Ok(values) => {
                    glow.insert(image, values);
                }
                Err(e) => warnings.push(format!("{id}/{path}: {e}: the image draws without its glow")),
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
                .map(|p| pack.look.projectiles.get(&p.key).cloned())
                .collect(),
            weapons: pack.look.weapons.clone(),
            explosion: pack.look.explosion.clone(),
            explosion_heat: pack.look.explosion_heat,
        };
        let (name, restrictions) = (pack.def.name, pack.def.restrictions);
        let (worm, rope) = (pack.def.worm, pack.def.rope);
        Ok(LoadedMod {
            sim: Arc::new(sim),
            images,
            glow,
            sounds,
            warnings,
            name,
            restrictions,
            worm,
            rope,
            look,
        })
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
            loadouts: Default::default(),
        };
        let missing = SimMod::compile(&def, &BTreeMap::new()).unwrap();
        let err = check_frames(&missing).unwrap_err().to_string();
        assert!(err.contains("flamer") && err.contains("gfx/flame.png"), "{err}");
        let found = SimMod::compile(&def, &[("gfx/flame.png".to_string(), 7)].into_iter().collect()).unwrap();
        check_frames(&found).unwrap();
    }
}
