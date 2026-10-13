//! The presentation under rollback (spec §3.4, decision N20): online play
//! draws the newest predicted tick, and a correction re-simulates ticks the
//! presentation already showed. Sounds and effects play at once, keyed by
//! tick and source ([`EventKey`]), so a re-simulated event never plays
//! twice; an event a correction erased is not undone (its sound runs out,
//! its entities live on). The colour terrain follows the corrected
//! terrain: the areas the rollbacks put back are rebuilt, then the ticks'
//! edits are applied in order.
//!
//! Per advance of an `rl_netcode::Session<WorldSim>` (rl-online's
//! `OnlinePeer::poll`):
//! 1. inside it, for every simulated tick (the `on_tick` callback, where
//!    the world is as of that tick): `Scene::after_tick` for a new tick,
//!    `Scene::after_tick_resimulated` for a re-simulated one, and
//!    `TickRecorder::simulated`;
//! 2. after it: [`NetPresenter::apply`] with the session's restored areas
//!    and the recorder's ticked records; play the sounds it returns;
//! 3. [`NetPresenter::confirmed`] with `Session::confirmed`;
//! 4. on `Event::StateLoaded { tick }`: `Presentation::state_loaded` and
//!    [`NetPresenter::reset`]`(tick)`, before or after step 2 of the same
//!    poll: the result is the same (the whole-map restored area comes with
//!    an [`NetPresenter::apply`], and the keys of the ticks after the state
//!    are kept, so the ticks simulated again after it play no sound twice).
//!
//! A sound is keyed by its id, not its source (events carry none): when a
//! correction swaps which worm fires the same weapon in the same tick and
//! sub-step, the sound stays where it first played (spec §3.4: not undone).
//! A death's sound is the exception: it is keyed by the worm whose gibs
//! follow it, not by which of the three death sounds it drew, so a
//! correction that re-rolls it plays no second death sound, and one that
//! kills another worm instead plays that death's.

use std::collections::{BTreeMap, BTreeSet};

use rl_fx::FxTick;
use rl_netcode::world_sim::TickedTick;
use rl_sim::{BonusItem, CosmeticKind, EndReason, Event, EventKind, SoundId, World};
use rl_terrain::Area;

use crate::Presentation;

/// One event's identity across simulations of its tick: the tick, the
/// sub-step, a hash of its kind and integer identity fields (`class`, no
/// float: a corrected re-simulation moves positions and amounts), and how
/// many equal `(substep, class)` came before it in the tick (`ordinal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventKey {
    pub tick: u32,
    pub substep: u8,
    pub class: u64,
    pub ordinal: u16,
}

/// The keys of one tick's events, in order: `None` for terrain edits,
/// which are never deduped (the colour terrain replays every one).
///
/// The class is the kind and: a sound's id (a death sound's worm, from the
/// gibs right after it, `rl_sim::damage`'s order); a shot's worm and weapon; a
/// damage's worm and attacker; a kill's victim and killer; a spawn's worm;
/// a cosmetic's kind with its worm (gib, laser sight, beam) or size
/// (explosion); an explosion's shake; a pickup's worm and bonus; a
/// message's text and numbers; a match end's winner and reason.
pub fn event_keys(tick: u32, events: &[Event]) -> Vec<Option<EventKey>> {
    let mut seen: BTreeMap<(u8, u64), u16> = BTreeMap::new();
    events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let class = class(&e.kind, events.get(i + 1).map(|n| &n.kind))?;
            let n = seen.entry((e.substep, class)).or_insert(0);
            let ordinal = *n;
            *n = n.saturating_add(1);
            Some(EventKey { tick, substep: e.substep, class, ordinal })
        })
        .collect()
}

/// The hash of `kind`'s identity (`next` is the event after it), `None`
/// for a terrain edit.
fn class(kind: &EventKind, next: Option<&EventKind>) -> Option<u64> {
    let mut h = Fnv::new();
    match *kind {
        EventKind::TerrainEdit { .. } => return None,
        EventKind::Sound { id, .. } => match next {
            Some(&EventKind::SpawnCosmetic { kind: CosmeticKind::Gib { worm }, .. })
                if SoundId::DEATH.contains(&id) =>
            {
                h.words(&[10, worm.into()])
            }
            _ => h.words(&[0, id.0.into()]),
        },
        EventKind::Explosion { shake, .. } => h.words(&[1, shake.into()]),
        EventKind::SpawnCosmetic { kind, .. } => {
            let (tag, n) = match kind {
                CosmeticKind::Blood { .. } => (0, 0),
                CosmeticKind::BloodDropper => (1, 0),
                CosmeticKind::Gib { worm } => (2, worm),
                CosmeticKind::Dirt { .. } => (3, 0),
                CosmeticKind::Explosion { size } => (4, size),
                CosmeticKind::Sparkle => (5, 0),
                CosmeticKind::Spawn => (6, 0),
                CosmeticKind::Smoke => (7, 0),
                CosmeticKind::ChemSmoke => (8, 0),
                CosmeticKind::Doomsday => (9, 0),
                CosmeticKind::JetpackSpray => (10, 0),
                CosmeticKind::LaserSight { worm, .. } => (11, worm),
                CosmeticKind::Beam { worm, .. } => (12, worm),
            };
            h.words(&[2, tag, n.into()]);
        }
        EventKind::WeaponFired { worm, weapon } => h.words(&[3, worm.into(), weapon.into()]),
        EventKind::Damage { worm, by, .. } => h.words(&[4, worm.into(), by.into()]),
        EventKind::Kill { victim, killer } => h.words(&[5, victim.into(), killer.into()]),
        EventKind::BonusPickup { worm, kind } => match kind {
            BonusItem::Health => h.words(&[6, worm.into(), 0]),
            BonusItem::Weapon(w) => h.words(&[6, worm.into(), 1, w.into()]),
        },
        EventKind::Spawned { worm } => h.words(&[7, worm.into()]),
        EventKind::Message(m) => {
            h.words(&[8, m.text.0.into(), m.args[0].into(), m.args[1].into(), m.args[2].into()])
        }
        EventKind::GameOver(r) => {
            let reason = match r.reason {
                EndReason::KillLimit => 0,
                EndReason::TimeLimit => 1,
                EndReason::LastStanding => 2,
                EndReason::Other(n) => 3 + i64::from(n),
            };
            h.words(&[9, r.winner.into(), reason]);
        }
    }
    Some(h.0)
}

/// FNV-1a 64 over little-endian words: a stable hash (`std::hash::Hash`'s
/// output may change between releases).
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn words(&mut self, words: &[i64]) {
        for b in words.iter().flat_map(|w| w.to_le_bytes()) {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// What online play's presentation has shown: the highest tick whose
/// entities have run, and the keys played or spawned from the confirmed
/// tick on (a re-simulation never reaches below it).
#[derive(Debug, Default)]
pub struct NetPresenter {
    /// Ticks from it on were never shown.
    next: u32,
    /// The sounds, cosmetics and explosions shown, by key.
    shown: BTreeSet<EventKey>,
    /// A re-simulated tick's events for the entities (reused).
    kept: Vec<Event>,
    /// Its explosions seen for the first time (reused).
    shakes: Vec<Event>,
    /// Cosmetic events handed to the entities so far.
    spawned: u64,
    /// Advances that left the colour terrain drifted ([`NetPresenter::drifts`]).
    drifts: u64,
}

impl NetPresenter {
    pub fn new() -> Self {
        NetPresenter::default()
    }

    /// Applies one advance to `present` after it ran, with `world` as the
    /// advance left it: first rebuilds every `restored` area from the
    /// terrain (`Session::take_restored_areas`), then replays `ticks`
    /// (`TickRecorder::take_ticked`) in order, and appends the sounds to
    /// play now to `sounds` (`rl_audio::Audio::play`).
    ///
    /// A tick never shown runs as in local play (`Fx::step_tick` with all
    /// its events: spawns, terrain edits, entity passes) and plays its
    /// sounds (all of them, but after a loaded state those of a tick shown
    /// before it, whose keys [`NetPresenter::reset`] kept). A tick shown before (re-simulated) runs no entity pass (the
    /// entities do not age twice): its terrain edits and only its
    /// cosmetics with a new key spawn, only its sounds with a new key
    /// play, only its explosions with a new key shake the camera. The
    /// scene's history and marks are not touched here: they are read
    /// inside the advance, where the world is as of each tick.
    ///
    /// In debug builds it checks the colour terrain's material is the
    /// world's afterwards ([`Presentation::terrain_drift`]): a drift is
    /// counted ([`NetPresenter::drifts`]) and logged once, not a panic.
    pub fn apply(
        &mut self,
        present: &mut Presentation,
        world: &World,
        restored: &[Area],
        ticks: &[TickedTick],
        sounds: &mut Vec<Event>,
    ) {
        for &area in restored {
            present.color_mut().rebuild_area(area, world.terrain());
        }
        let dt = world.settings().lx56_physics_dt();
        for t in ticks {
            let keys = event_keys(t.tick, &t.events);
            let fx =
                |events, substeps| FxTick { events, substeps, dt, lost: t.lost, material: world.terrain() };
            if t.tick >= self.next {
                present.step_fx(&fx(&t.events, t.substeps));
                for (e, k) in t.events.iter().zip(&keys) {
                    let new = k.is_some_and(|k| self.shown.insert(k));
                    match e.kind {
                        EventKind::Sound { .. } if new => sounds.push(*e),
                        EventKind::SpawnCosmetic { .. } => self.spawned += 1,
                        _ => {}
                    }
                }
                self.next = t.tick.saturating_add(1);
                continue;
            }
            self.kept.clear();
            self.shakes.clear();
            for (e, k) in t.events.iter().zip(&keys) {
                match k {
                    None => self.kept.push(*e),
                    Some(k) if self.shown.insert(*k) => match e.kind {
                        EventKind::SpawnCosmetic { .. } => {
                            self.kept.push(*e);
                            self.spawned += 1;
                        }
                        EventKind::Sound { .. } => sounds.push(*e),
                        EventKind::Explosion { .. } => self.shakes.push(*e),
                        _ => {}
                    },
                    Some(_) => {}
                }
            }
            present.step_fx(&fx(&self.kept, 0));
            present.scene.camera.shake_for(&self.shakes, world.time());
        }
        if cfg!(debug_assertions) && !(self.next == 0 && restored.is_empty()) && present.terrain_drift(world)
        {
            self.drifts += 1;
            if self.drifts == 1 {
                eprintln!(
                    "error: tick {}: the colour terrain shows terrain the world no longer has \
                     (counted in NetPresenter::drifts; logged once)",
                    self.next
                );
            }
        }
    }

    /// Ticks below `tick` are confirmed (`Session::confirmed`): no
    /// rollback reaches them again, so their keys go.
    pub fn confirmed(&mut self, tick: u32) {
        let first = EventKey { tick, substep: 0, class: 0, ordinal: 0 };
        self.shown = self.shown.split_off(&first);
    }

    /// The cosmetic events handed to the entities so far: every one of a
    /// new tick, and those with a new key of a re-simulated one.
    pub fn spawned(&self) -> u64 {
        self.spawned
    }

    /// Advances after which the colour terrain had drifted from the
    /// world's (checked in debug builds only: the whole map is compared).
    pub fn drifts(&self) -> u64 {
        self.drifts
    }

    /// The state after `loaded_tick` loaded (`Event::StateLoaded`): the
    /// ticks after it are new again (with `Presentation::state_loaded`,
    /// which clears the entities), but the keys of those already shown are
    /// kept, so their sounds do not play twice when they run again.
    pub fn reset(&mut self, loaded_tick: u32) {
        self.next = 0;
        self.shown.retain(|k| k.tick > loaded_tick);
    }
}
