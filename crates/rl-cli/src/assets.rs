//! What a match is made of: a pack's mod, level and theme, or a generated
//! level (`genlevel`, plan 8) with Castle Strike's theme. `rl-cli`'s
//! matches, stress runs, replay checks and GPU scenes all load through
//! here, so a generated level's id is enough to make it again.

use anyhow::{Context as _, bail, ensure};
use rl_content::{Content, LoadedLevel, LoadedMod, LoadedTheme, MatchAssets, World, replay_assets_from};
use rl_data::manifest::PackKind;
use rl_replay::{PackRef, Packs, Replay};

use crate::genlevel::{GenLevel, MAX_ART_SIDE, MAX_SIDE, MIN_SIDE, PREFIX, THEME_LEVEL};

/// The hash a generated level's pack reference carries (there is no pack).
pub const GENERATED_HASH: &str = "generated";

/// A match's assets, loaded once.
pub struct LevelAssets {
    pub m: LoadedMod,
    pub level: LoadedLevel,
    pub theme: LoadedTheme,
    /// The packs a recording names (a generated level by its id).
    pub packs: Packs,
    pub generated: Option<GenLevel>,
}

impl LevelAssets {
    /// Mod pack `mod_id` and level `level_id`: a level pack (exactly what
    /// `Content::build_match` loads) or a generated level's id. An id that
    /// starts with `gen-` but names no generated level (malformed, or past
    /// the limits: plan 8 ruling PF7) is an error, made nothing of.
    pub fn load(content: &Content, mod_id: &str, level_id: &str) -> anyhow::Result<LevelAssets> {
        if level_id.starts_with(PREFIX) && GenLevel::parse(level_id).is_none() {
            bail!(
                "{level_id} is no generated level: gen-<width>x<height>-a<art scale>-s<seed>, sides \
                 {MIN_SIDE} to {MAX_SIDE}, art scale 1 to {}, side × art scale at most {MAX_ART_SIDE}",
                rl_content::MAX_ART_SCALE
            );
        }
        let m = content.load_mod(mod_id)?;
        if let Some(g) = GenLevel::parse(level_id) {
            let theme_id = content.load_level(THEME_LEVEL).context("the generated levels' theme")?.theme;
            let theme = content.load_theme(&theme_id).with_context(|| format!("level {level_id}"))?;
            let packs = Packs {
                mod_pack: PackRef { id: mod_id.into(), hash: content.pack_hash(mod_id, PackKind::Mod)? },
                level: PackRef { id: level_id.into(), hash: GENERATED_HASH.into() },
                theme: PackRef { id: theme_id.clone(), hash: content.pack_hash(&theme_id, PackKind::Theme)? },
            };
            return Ok(LevelAssets { m, level: g.level(&theme_id), theme, packs, generated: Some(g) });
        }
        let level = content.load_level(level_id)?;
        let theme = content.load_theme(&level.theme).with_context(|| format!("level {level_id}"))?;
        let packs = content.replay_packs(mod_id, level_id)?;
        Ok(LevelAssets { m, level, theme, packs, generated: None })
    }

    /// For `rl_content::build_match`.
    pub fn match_assets(&self) -> MatchAssets<'_> {
        MatchAssets { m: &self.m, level: &self.level, theme: &self.theme, packs: &self.packs }
    }

    /// The world `replay` (recorded on these assets) starts from. A pack
    /// level's packs are checked as `Content::world_for_replay` does; a
    /// generated level's must be these: the level's id, and the mod's and
    /// theme's ids and content hashes.
    pub fn replay_world(&self, content: &Content, replay: &Replay) -> anyhow::Result<World> {
        let recorded = &replay.setup.packs;
        match self.generated {
            None => content.check_replay_packs(&replay.setup)?,
            Some(_) => ensure!(
                *recorded == self.packs,
                "the replay was recorded on {} with {} and theme {}, not {} with {} and theme {} \
                 (or a different version of a pack)",
                recorded.level.id,
                recorded.mod_pack.id,
                recorded.theme.id,
                self.packs.level.id,
                self.packs.mod_pack.id,
                self.packs.theme.id
            ),
        }
        replay.world(replay_assets_from(&self.m, &self.level, &self.theme)).map_err(anyhow::Error::new)
    }
}
