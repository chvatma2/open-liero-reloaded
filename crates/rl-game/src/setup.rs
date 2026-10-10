//! The sandbox (spec §19 E11): which packs to load, the world to build, and
//! the assets loaded once for every restart.

use std::path::Path;

use anyhow::Context as _;
use rl_content::{Content, LoadedLevel, LoadedMod, LoadedSkin, LoadedTheme, SandboxSetup, SpawnSpec};
use rl_data::manifest::PackKind;
use rl_sim::{MAX_WORMS, SimSettings};

/// What the command line chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxOptions {
    pub mod_id: String,
    pub level_id: String,
    pub seed: u32,
    /// Idle target worms besides the player.
    pub targets: usize,
    /// OLX's `Bonuses` option: bonuses spawn and fall.
    pub bonuses: bool,
}

/// The world [`SandboxOptions`] describe: the player (worm 0) and the
/// targets, each at a `FindSpot` position with the mod's first five
/// weapons, under OLX's LieroX 0.56 settings (unlimited lives). The targets
/// are capped so the worms fit OLX's `MAX_WORMS`.
pub fn sandbox_setup(o: &SandboxOptions) -> SandboxSetup {
    let worms = 1 + o.targets.min(MAX_WORMS - 1);
    SandboxSetup {
        mod_id: o.mod_id.clone(),
        level_id: o.level_id.clone(),
        seed: o.seed,
        settings: SimSettings { bonuses: o.bonuses, ..SimSettings::lx56() },
        worms: vec![SpawnSpec::default(); worms],
        restrictions: rl_content::DEFAULT_PRESET.into(),
    }
}

/// The pack id `given` means among `ids`: itself, or an OLX import's id
/// (`prefix` and the slug of `given`, so `--level "Liero Factory"` and
/// `--mod MW 1.0` work).
pub fn resolve_id(given: &str, prefix: &str, ids: &[String]) -> anyhow::Result<String> {
    let stem = given.rsplit_once('.').map_or(given, |(s, _)| s);
    let slug = |s: &str| format!("{prefix}{}", rl_data::slug::slug(s));
    let candidates = [given.to_string(), slug(given), slug(stem)];
    candidates
        .into_iter()
        .find(|c| ids.contains(c))
        .with_context(|| format!("no pack {given:?}; there are: {}", ids.join(", ")))
}

/// Everything a sandbox draws and plays, loaded once: a restart (F5)
/// builds a new world from it without decoding any image or sound again.
pub struct Assets {
    pub content: Content,
    pub m: LoadedMod,
    pub level: LoadedLevel,
    pub theme: LoadedTheme,
    pub skin: LoadedSkin,
    pub setup: SandboxSetup,
    /// Every warning of the loading, to log once.
    pub warnings: Vec<String>,
}

/// The packs `--mod`, `--level` and `--skin` name in content directory `root`.
pub fn load(
    root: &Path,
    mod_name: &str,
    level: &str,
    skin: &str,
    o: SandboxOptions,
) -> anyhow::Result<Assets> {
    let content = Content::open(root)?;
    let mod_id = resolve_id(mod_name, "olx-", &content.ids(PackKind::Mod)).context("--mod")?;
    let level_id = resolve_id(level, "olx-level-", &content.ids(PackKind::Level)).context("--level")?;
    let skin_id =
        resolve_id(skin, "olx-skin-", &content.ids(PackKind::Skin)).unwrap_or_else(|_| skin.to_string());
    let m = content.load_mod(&mod_id)?;
    let lvl = content.load_level(&level_id)?;
    let theme = content.load_theme(&lvl.theme).with_context(|| format!("level {level_id}"))?;
    let skin = content.skin_or_default(&skin_id);
    let mut warnings = content.warnings().to_vec();
    warnings.extend(m.warnings.iter().chain(&theme.warnings).chain(&skin.warnings).cloned());
    let setup = sandbox_setup(&SandboxOptions { mod_id, level_id, ..o });
    Ok(Assets { content, m, level: lvl, theme, skin, setup, warnings })
}

impl Assets {
    /// A new world of the sandbox, from the assets loaded.
    pub fn new_world(&self) -> anyhow::Result<rl_sim::World> {
        rl_content::new_world_from(&self.m, &self.level, &self.theme, &self.setup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(targets: usize) -> SandboxOptions {
        SandboxOptions {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-liero-factory".into(),
            seed: 1,
            targets,
            bonuses: false,
        }
    }

    #[test]
    fn sandbox_setup_spawns_targets() {
        let s = sandbox_setup(&options(3));
        assert_eq!(s.worms.len(), 4, "the player and 3 targets");
        assert!(
            s.worms.iter().all(|w| w.pos.is_none() && w.weapons.is_empty()),
            "FindSpot, first five weapons"
        );
        assert_eq!(s.settings.lives, SimSettings::lx56().lives);
        assert_eq!(sandbox_setup(&options(0)).worms.len(), 1);
        assert_eq!(sandbox_setup(&options(1000)).worms.len(), MAX_WORMS);
        assert!(sandbox_setup(&SandboxOptions { bonuses: true, ..options(0) }).settings.bonuses);

        // On every imported level and mod, when there is content: every
        // worm alive on its own free spot.
        let Some(dir) = std::env::var_os("RL_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("RL_CONTENT_DIR not set; skipping the imported levels");
            return;
        };
        for m in ["Classic", "MW 1.0"] {
            for level in ["Liero Factory", "CastleStrike", "FossilFacility", "Dirt Level"] {
                let a = load(Path::new(&dir), m, level, "default", options(3)).unwrap();
                let world = a.new_world().unwrap();
                assert_eq!(world.worms().len(), 4, "{m} on {level}");
                assert!(world.worms().iter().all(|w| w.alive && w.slots.len() == 5), "{m} on {level}");
                let mut spots: Vec<_> =
                    world.worms().iter().map(|w| (w.pos.x as i32, w.pos.y as i32)).collect();
                spots.sort_unstable();
                spots.dedup();
                assert_eq!(spots.len(), 4, "{m} on {level}: {spots:?}");
                assert_eq!(a.new_world().unwrap(), world, "a restart is the same world");
            }
        }
    }

    #[test]
    fn pack_ids_resolve_from_olx_names() {
        let ids: Vec<String> = ["olx-classic", "olx-mw-1-0"].map(String::from).to_vec();
        assert_eq!(resolve_id("olx-classic", "olx-", &ids).unwrap(), "olx-classic");
        assert_eq!(resolve_id("Classic", "olx-", &ids).unwrap(), "olx-classic");
        assert_eq!(resolve_id("MW 1.0", "olx-", &ids).unwrap(), "olx-mw-1-0");
        let levels = vec!["olx-level-liero-factory".to_string()];
        assert_eq!(
            resolve_id("Liero Factory.lxl", "olx-level-", &levels).unwrap(),
            "olx-level-liero-factory"
        );
        assert!(resolve_id("nope", "olx-", &ids).unwrap_err().to_string().contains("olx-classic"));
    }
}
