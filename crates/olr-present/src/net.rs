//! The presentation under rollback (spec §3.4, decision N20): online play
//! draws the newest predicted tick, and a correction re-simulates ticks the
//! presentation already showed. Sounds and effects play at once, keyed by
//! tick and source ([`EventKey`]), so a re-simulated event never plays
//! twice; an event a correction erased is not undone (its sound runs out,
//! its entities live on). The colour terrain follows the corrected
//! terrain: the areas the rollbacks put back are rebuilt, then the ticks'
//! edits are applied in order.
//!
//! Per advance of an `olr_netcode::Session<WorldSim>` (olr-online's
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
//! A death is the exception: its sound, gibs and blood (`Event::death`)
//! show once per worm and life, whatever tick and sub-step a correction
//! moves the death to and whichever of the three death sounds it draws;
//! a correction that kills another worm instead shows that death.

use std::collections::{BTreeMap, BTreeSet};

use olr_fx::FxTick;
use olr_netcode::MAX_PREDICTION;
use olr_netcode::world_sim::TickedTick;
use olr_sim::{BonusItem, CosmeticKind, EndReason, Event, EventKind, SoundId, World};
use olr_terrain::Area;

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
/// gibs right after it, `olr_sim::damage`'s order); a shot's worm and weapon; a
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
/// entities have run, the keys played or spawned from the confirmed tick
/// on (a re-simulation never reaches below it), and the deaths shown.
#[derive(Debug, Default)]
pub struct NetPresenter {
    /// Ticks from it on were never shown.
    next: u32,
    /// The sounds, cosmetics and explosions shown, by key.
    shown: BTreeSet<EventKey>,
    /// The deaths shown, by worm and life ([`NetPresenter::life`]), with
    /// the tick each was last seen at: a death of that life within
    /// `MAX_PREDICTION` ticks of it is the same one moved by a correction;
    /// farther, a new one (the first was erased).
    deaths: BTreeMap<(i32, u32), u32>,
    /// The worms spawned in each tick from the confirmed tick on, as last
    /// simulated (a re-simulated tick replaces its entry).
    spawns: BTreeMap<u32, Vec<i32>>,
    /// Per worm, its spawns in the ticks below the confirmed tick.
    spawns_before: BTreeMap<i32, u32>,
    /// Per event of the tick being applied: for a death's event, whether
    /// that death shows now (reused).
    parts: Vec<Option<bool>>,
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
    /// play now to `sounds` (`olr_audio::Audio::play`).
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
            self.death_parts(t.tick, &t.events);
            let fx =
                |events, substeps| FxTick { events, substeps, dt, lost: t.lost, material: world.terrain() };
            self.kept.clear();
            if t.tick >= self.next {
                for ((e, k), part) in t.events.iter().zip(&keys).zip(&self.parts) {
                    // Every event, but a death shown before (moved here by a
                    // correction) shows nothing again.
                    let new = match part {
                        Some(false) => continue,
                        Some(true) => true,
                        None => k.is_some_and(|k| self.shown.insert(k)),
                    };
                    self.kept.push(*e);
                    match e.kind {
                        EventKind::Sound { .. } if new => sounds.push(*e),
                        EventKind::SpawnCosmetic { .. } => self.spawned += 1,
                        _ => {}
                    }
                }
                present.step_fx(&fx(&self.kept, t.substeps));
                self.next = t.tick.saturating_add(1);
                continue;
            }
            self.shakes.clear();
            for ((e, k), part) in t.events.iter().zip(&keys).zip(&self.parts) {
                // A terrain edit always; a death's event with its death; any
                // other with a new key.
                let new = match (part, k) {
                    (Some(new), _) => *new,
                    (None, None) => {
                        self.kept.push(*e);
                        continue;
                    }
                    (None, Some(k)) => self.shown.insert(*k),
                };
                if !new {
                    continue;
                }
                match e.kind {
                    EventKind::SpawnCosmetic { .. } => {
                        self.kept.push(*e);
                        self.spawned += 1;
                    }
                    EventKind::Sound { .. } => sounds.push(*e),
                    EventKind::Explosion { .. } => self.shakes.push(*e),
                    _ => {}
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
        let later = self.spawns.split_off(&tick);
        for worm in core::mem::replace(&mut self.spawns, later).into_values().flatten() {
            let n = self.spawns_before.entry(worm).or_insert(0);
            *n = n.saturating_add(1);
        }
        // A life followed by a confirmed spawn ended in a confirmed tick,
        // and no correction moves a death seen before the window.
        let before = &self.spawns_before;
        self.deaths.retain(|(worm, life), seen| {
            *life >= before.get(worm).copied().unwrap_or(0) && seen.saturating_add(MAX_PREDICTION) >= tick
        });
    }

    /// Worm `worm`'s life at `tick`: how many times it spawned up to that
    /// tick, as the ticks were last simulated (a correction that moves a
    /// spawn within the ticks before a death keeps the death's life). The
    /// spawns before the presenter's first tick (a state loaded mid-match)
    /// are not counted: every worm's life then starts at 0.
    fn life(&self, worm: i32, tick: u32) -> u32 {
        let later = self.spawns.range(..=tick).flat_map(|(_, w)| w).filter(|&&w| w == worm).count();
        let before = self.spawns_before.get(&worm).copied().unwrap_or(0);
        before.saturating_add(u32::try_from(later).unwrap_or(u32::MAX))
    }

    /// Fills `parts` for `tick`'s events: `None` for an event that is not
    /// a death's, and for a death's events (a run of `Event::death` from
    /// its sound, `olr_sim::damage`'s order) whether that death of the
    /// worm whose gibs it holds shows now: not when the same life's death
    /// was seen within `MAX_PREDICTION` ticks (a correction moved it). It
    /// records the tick it is seen at. A death
    /// without gibs (dropped beyond the event quota) is keyed like any
    /// event. The tick's spawns replace those it had before.
    fn death_parts(&mut self, tick: u32, events: &[Event]) {
        self.spawns.remove(&tick);
        self.parts.clear();
        let mut i = 0;
        while i < events.len() {
            let e = &events[i];
            if !e.death {
                if let EventKind::Spawned { worm } = e.kind {
                    self.spawns.entry(tick).or_default().push(worm);
                }
                self.parts.push(None);
                i += 1;
                continue;
            }
            let len = 1 + events[i + 1..]
                .iter()
                .take_while(|n| n.death && !matches!(n.kind, EventKind::Sound { .. }))
                .count();
            let worm = events[i..i + len].iter().find_map(|e| match e.kind {
                EventKind::SpawnCosmetic { kind: CosmeticKind::Gib { worm }, .. } => Some(worm),
                _ => None,
            });
            let part = worm.map(|w| {
                let life = self.life(w, tick);
                let new =
                    self.deaths.get(&(w, life)).is_none_or(|&seen| seen.abs_diff(tick) > MAX_PREDICTION);
                self.deaths.insert((w, life), tick);
                new
            });
            self.parts.extend(std::iter::repeat_n(part, len));
            i += len;
        }
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
    /// kept, so their sounds do not play twice when they run again. The
    /// deaths seen up to the state go with the old timeline, so the loaded
    /// one's death of the same life shows.
    pub fn reset(&mut self, loaded_tick: u32) {
        self.next = 0;
        self.shown.retain(|k| k.tick > loaded_tick);
        self.deaths.retain(|_, seen| *seen > loaded_tick);
    }
}
