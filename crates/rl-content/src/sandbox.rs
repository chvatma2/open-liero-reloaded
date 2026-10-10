//! Sandbox worlds: a mod on a level with worms, ready to tick.

use anyhow::Context as _;
use rl_math::AbsTime;
use rl_sim::{Loadout, MAX_WORMS, SimSettings, Slot, World};
use rl_terrain::Terrain;

use crate::Content;

/// Where a sandbox world's frame clock starts. Not 0: OLX treats time 0 as
/// "never" (a worm's last bleeding, a bonus's spawn time).
pub const SANDBOX_START: AbsTime = AbsTime(100_000);

/// One worm of a [`SandboxSetup`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnSpec {
    /// Where it spawns; `None` draws a position with the world's `FindSpot`
    /// ([`World::find_spot`]), as OLX's server spawns worms.
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
}

impl Content {
    /// The world `setup` describes: the mod (with placeholders, as
    /// [`Content::load_mod`]), the level with its theme's holes and green-dirt
    /// mask, and the worms spawned in order with their weapons. Worms
    /// without a position draw one from `FindSpot` when they spawn, so the
    /// same setup always gives the same world. Settings outside OLX's range,
    /// too many worms or an unknown weapon id are errors.
    pub fn new_world(&self, setup: &SandboxSetup) -> anyhow::Result<World> {
        setup.settings.validate().map_err(|e| anyhow::anyhow!("settings: {}", e.0))?;
        anyhow::ensure!(setup.worms.len() <= MAX_WORMS, "{} worms, at most {MAX_WORMS}", setup.worms.len());
        let m = self.load_mod(&setup.mod_id)?;
        let level = self.load_level(&setup.level_id)?;
        let theme = self.load_theme(&level.theme).with_context(|| format!("level {}", setup.level_id))?;
        let n = m.sim.weapons().len() as i32;
        for (i, w) in setup.worms.iter().enumerate() {
            if let Some(bad) = w.weapons.iter().find(|&&id| !(0..n).contains(&id)) {
                anyhow::bail!("worm {i}: no weapon {bad} in {} ({n} weapons)", setup.mod_id);
            }
        }
        let terrain = Terrain::from_grid(&level.material);
        let mut world =
            World::new(setup.settings.clone(), m.sim, terrain, theme.stamps(), SANDBOX_START, setup.seed);
        if let Some(mask) = theme.green_mask() {
            world.set_green_mask(mask);
        }
        for (i, w) in setup.worms.iter().enumerate() {
            let (x, y) = w.pos.unwrap_or_else(|| world.find_spot());
            world.spawn_worm(x, y).context("too many worms")?;
            let ids: Vec<i32> =
                if w.weapons.is_empty() { (0..n.min(5)).collect() } else { w.weapons.clone() };
            let slots = (0..5)
                .filter_map(|s| ids.get(s % ids.len().max(1)))
                .map(|&weapon| Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 })
                .collect();
            let lives = world.worms()[i].lives;
            world.set_loadout(i, &Loadout { slots, current_weapon: 0, lives, kills: 0, deaths: 0 });
        }
        Ok(world)
    }
}
