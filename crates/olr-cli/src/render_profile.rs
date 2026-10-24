//! `olr-cli render-profile` (spec
//! `docs/superpowers/specs/2026-10-24-render-profile-design.md`): where a
//! heavy lit frame's time goes. A staged nuke fight (Liero v1.0 Reloaded
//! on Castle Strike Reloaded, a few bots carrying only the nukes) is
//! played headless and recorded, then drawn the way the game draws it,
//! and each frame's costs are reported by phase.

use anyhow::{Context as _, bail};
use olr_content::{Content, MatchLoadout, MatchOptions, WeaponChoice};
use olr_replay::Replay;
use olr_sim::{EventKind, TickEvents};

use crate::assets::LevelAssets;
use crate::runner::{Match, Options};
use crate::spec::{Difficulty, MatchSpec};

/// The fight's mod, level, players, seed and length (20 s): one local
/// player pressing `spec::local_keys`' scripted keys and three Medium
/// bots, as the maintainer's own matches. Medium bots alone fired 3 nukes
/// in 20 s, the first at 12.9 s; chaos bots change weapons from the whole
/// mod at each spawn; with the scripted player, 27 (16 Big, 11 Mini), the
/// first explosion at tick 200 (2026-10-24).
pub const FIGHT_MOD: &str = "olr-mod-liero-v1-reloaded";
pub const FIGHT_LEVEL: &str = "olr-level-castlestrike-reloaded";
pub const FIGHT_LOCALS: usize = 1;
pub const FIGHT_BOTS: usize = 3;
pub const FIGHT_SEED: u32 = 7;
pub const FIGHT_TICKS: u32 = 2000;
/// The only weapons the worms carry, and their loading time (percent:
/// quick reloads, so the fight keeps exploding).
pub const NUKES: [&str; 2] = ["Big Nuke", "Mini Nuke"];
pub const NUKE_LOADING_TIME: i32 = 20;

/// The fight's match: the scripted player and Medium bots, no time or
/// kill limit.
pub fn fight_spec() -> MatchSpec {
    let mut spec = MatchSpec::new(FIGHT_MOD, FIGHT_LEVEL, FIGHT_BOTS, FIGHT_SEED, FIGHT_TICKS);
    (spec.locals, spec.difficulty) = (FIGHT_LOCALS, Difficulty::Medium);
    (spec.time_limit, spec.kill_limit) = (0.0, 0);
    spec
}

/// `spec`'s options with a loadout of `weapons` alone.
fn options_with(spec: &MatchSpec, weapons: &[&str]) -> MatchOptions {
    let mut o = spec.match_options();
    o.weapons = WeaponChoice::Loadout(MatchLoadout {
        name: "Nukes".into(),
        weapons: weapons.iter().map(|w| w.to_string()).collect(),
        loading_time: NUKE_LOADING_TIME,
        unique: false,
    });
    o
}

/// The fight's options: [`NUKES`] alone, at [`NUKE_LOADING_TIME`].
pub fn fight_options(spec: &MatchSpec) -> MatchOptions {
    options_with(spec, &NUKES)
}

/// The staged fight, recorded.
#[derive(Clone, Debug)]
pub struct Fight {
    pub replay: Replay,
    /// Ticks with an explosion, and the first of them.
    pub explosions: u32,
    pub first_explosion: Option<u32>,
    /// The shots fired, by weapon name (in name order).
    pub shots: Vec<(String, u32)>,
}

/// Whether the tick had an explosion (what leaves heat in a level whose
/// materials heat: `olr-present`'s explosions).
pub fn has_explosion(events: &TickEvents) -> bool {
    events.iter().any(|e| matches!(e.kind, EventKind::Explosion { .. }))
}

/// Plays the fight headless and records it; an error when no nuke went
/// off, so a calm scene never passes for the nuke case.
pub fn stage_fight(content: &Content) -> anyhow::Result<Fight> {
    stage_with_weapons(content, &NUKES)
}

/// [`stage_fight`] with the worms carrying `weapons` (tests).
#[doc(hidden)]
pub fn stage_with_weapons(content: &Content, weapons: &[&str]) -> anyhow::Result<Fight> {
    stage_spec(content, &fight_spec(), weapons)
}

/// [`stage_fight`] of `spec` with the worms carrying `weapons` (tests).
#[doc(hidden)]
pub fn stage_spec(content: &Content, spec: &MatchSpec, weapons: &[&str]) -> anyhow::Result<Fight> {
    let spec = spec.clone();
    let assets = LevelAssets::load(content, FIGHT_MOD, FIGHT_LEVEL)?;
    let mut game =
        Match::with_match_options(&assets, &spec, options_with(&spec, weapons), &Options::default())
            .context("setting up the nuke fight")?;
    let (mut explosions, mut first_explosion) = (0, None);
    let mut shots = std::collections::BTreeMap::<String, u32>::new();
    while game.step() {
        if has_explosion(game.events()) {
            explosions += 1;
            first_explosion.get_or_insert(game.ticks() - 1);
        }
        for e in game.events() {
            if let EventKind::WeaponFired { weapon, .. } = e.kind {
                let w = usize::try_from(weapon).ok().and_then(|i| game.world().sim_mod().weapons().get(i));
                *shots
                    .entry(w.map_or_else(|| format!("weapon {weapon}"), |w| w.name.clone()))
                    .or_default() += 1;
            }
        }
    }
    let run = game.finish();
    if let Some(f) = run.failure() {
        bail!("the nuke fight failed: {f}");
    }
    if explosions == 0 {
        bail!("the staged fight set off no nukes");
    }
    Ok(Fight { replay: run.replay, explosions, first_explosion, shots: shots.into_iter().collect() })
}
