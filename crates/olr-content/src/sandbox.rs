//! Sandbox worlds: a mod on a level with worms, ready to tick.

use anyhow::Context as _;
use olr_math::AbsTime;
use olr_sim::{Loadout, MAX_WORMS, MatchSetup, OracleMode, SimSettings, Slot, World, WormSetup};
use olr_terrain::Terrain;

use crate::{Content, LoadedLevel, LoadedMod, LoadedTheme};

/// Where a sandbox world's frame clock starts: [`MatchSetup::T0`]. Not 0:
/// OLX treats time 0 as "never" (a worm's last bleeding, a bonus's spawn time).
pub const SANDBOX_START: AbsTime = MatchSetup::T0;

/// One worm of a [`SandboxSetup`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnSpec {
    /// Where it spawns; `None` draws a position with the world's `FindSpot`
    /// ([`olr_sim::WormSetup::spawn`]), as OLX's server spawns worms.
    pub pos: Option<(i32, i32)>,
    /// Weapon ids for its five slots (cycled if fewer); empty for the mod's
    /// weapons from the first one. All start full and ready.
    pub weapons: Vec<i32>,
}

/// A sandbox match (spec §19 E11): what [`Content::new_world`] builds.
#[derive(Clone, Debug)]
pub struct SandboxSetup {
    /// A mod pack id, such as `olx-classic` ([`crate::olx_mod_id`]).
    pub mod_id: String,
    /// A level pack id, such as `olx-level-castlestrike` ([`crate::olx_level_id`]).
    pub level_id: String,
    /// The match seed (`simSetSeed`).
    pub seed: u32,
    pub settings: SimSettings,
    /// At most [`MAX_WORMS`].
    pub worms: Vec<SpawnSpec>,
    /// OLX's `WeaponRestrictionsFile`, such as [`crate::DEFAULT_PRESET`]: the
    /// weapons a natural bonus may hold are those it does not ban.
    pub restrictions: String,
}

impl Content {
    /// The world `setup` describes, loading its mod, level and theme
    /// ([`new_world_from`] builds it from assets already loaded).
    pub fn new_world(&self, setup: &SandboxSetup) -> anyhow::Result<World> {
        let m = self.load_mod(&setup.mod_id)?;
        let level = self.load_level(&setup.level_id)?;
        let theme = self.load_theme(&level.theme).with_context(|| format!("level {}", setup.level_id))?;
        new_world_from(&m, &level, &theme, setup)
    }
}

/// The world `setup` describes, from its mod `m` (with placeholders, as
/// [`Content::load_mod`]), `level` and its `theme` already loaded (a restart
/// decodes nothing again): [`match_setup`], played with OLX's crediting and
/// no end ([`OracleMode`]).
pub fn new_world_from(
    m: &LoadedMod,
    level: &LoadedLevel,
    theme: &LoadedTheme,
    setup: &SandboxSetup,
) -> anyhow::Result<World> {
    let setup = match_setup(m, level, theme, setup)?;
    World::new(setup, Box::new(OracleMode)).map_err(|e| anyhow::anyhow!("{e}"))
}

/// The [`MatchSetup`] `setup` describes, for a world of any game mode: the
/// level with the theme's holes and green-dirt mask and its live copies
/// (`LoadedLevel::live_sim`, issue #86), and the worms in
/// order with their weapons. Worms without a position draw one from
/// `FindSpot` when they spawn, so the same setup always gives the same
/// world. Natural bonuses hold the weapons `setup.restrictions` does not ban
/// ([`LoadedMod::weapon_rules`]). Settings outside OLX's range, too many
/// worms or an unknown weapon id are errors. `setup`'s pack ids are not read.
/// The settings take the mod's worm and rope physics, as OLX's do
/// (`SimSettings::with_mod_physics`).
pub fn match_setup(
    m: &LoadedMod,
    level: &LoadedLevel,
    theme: &LoadedTheme,
    setup: &SandboxSetup,
) -> anyhow::Result<MatchSetup> {
    let settings = setup.settings.clone().with_mod_physics(&m.worm, &m.rope);
    settings.validate().map_err(|e| anyhow::anyhow!("settings: {}", e.0))?;
    anyhow::ensure!(setup.worms.len() <= MAX_WORMS, "{} worms, at most {MAX_WORMS}", setup.worms.len());
    let n = m.sim.weapons().len() as i32;
    let mut worms = Vec::with_capacity(setup.worms.len());
    for (i, w) in setup.worms.iter().enumerate() {
        if let Some(bad) = w.weapons.iter().find(|&&id| !(0..n).contains(&id)) {
            anyhow::bail!("worm {i}: no weapon {bad} in {} ({n} weapons)", setup.mod_id);
        }
        let ids: Vec<i32> = if w.weapons.is_empty() { (0..n.min(5)).collect() } else { w.weapons.clone() };
        let slots =
            (0..5).filter_map(|s| ids.get(s % ids.len().max(1))).map(|&weapon| Slot::ready(weapon)).collect();
        worms.push(WormSetup { team: 0, loadout: Loadout::new(slots, &settings), spawn: w.pos });
    }
    let terrain = Terrain::from_grid(&level.material);
    Ok(MatchSetup {
        holes: theme.stamps(),
        green_mask: theme.green_mask(),
        start: SANDBOX_START,
        seed: setup.seed,
        worms,
        bonus_weapons: Some(m.weapon_rules(&setup.restrictions).bonus_weapons()),
        live: level.live_sim(),
        director: level.live_director(),
        ..MatchSetup::new(settings, m.sim.clone(), terrain)
    })
}
