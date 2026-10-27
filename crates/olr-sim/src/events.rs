//! What a tick reports to the renderer, audio, the HUD and tests (spec §5.7):
//! [`TickEvents`], a buffer of [`Event`]s that [`crate::World::tick`] refills.
//!
//! Events are emitted where OLX plays a sound (`StartSound`,
//! `PlaySoundSample`), spawns an entity (`SpawnEntity`, `SetWormBeamEntity`)
//! or changes the map, and where the server credits damage, kills, pickups
//! and spawns. They carry no randomness of their own (spec §18 D4, D5): a
//! cosmetic's velocity is the one OLX computes from the draws it already
//! makes, and the renderer's own effects use their own generator. A
//! cosmetic event is emitted even when the dedicated server's entity pool is
//! full, since a real client frees its entities and would show it.

use alloc::vec::Vec;

use olr_data::image::Rgba;
use olr_math::Vec2;

use crate::bonus::BonusItem;
use crate::mode::{MatchResult, Message};

/// An index into [`crate::SimMod::sounds`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SoundId(pub u16);

impl SoundId {
    /// `sfxGame.smpNinja` (`data/sounds/throw.wav`): the rope is thrown.
    pub const THROW: SoundId = SoundId(0);
    /// `sfxGame.smpPickup`: a bonus is picked up.
    pub const PICKUP: SoundId = SoundId(1);
    /// `sfxGame.smpBump`: a worm hits the map fast.
    pub const BUMP: SoundId = SoundId(2);
    /// `sfxGame.smpDeath[0..3]`, chosen by `GetRandomInt(2)`.
    pub const DEATH: [SoundId; 3] = [SoundId(3), SoundId(4), SoundId(5)];
}

/// The prefix of OLX's game sounds in [`crate::SimMod::sounds`]: the rest is
/// a path in OLX's data directory. Every other entry is a mod sound, a path
/// in the mod's pack (a mod's own path with this prefix is ignored, so the
/// roots cannot be confused).
pub const GAME_SOUND_PREFIX: &str = "olx:";

/// OLX's game sounds (`src/game/Sounds.cpp:25-30`), the first entries of
/// every [`crate::SimMod::sounds`] table, each with [`GAME_SOUND_PREFIX`].
pub const GAME_SOUNDS: [&str; 6] = [
    "olx:data/sounds/throw.wav",
    "olx:data/sounds/pickup.wav",
    "olx:data/sounds/bump.wav",
    "olx:data/sounds/death1.wav",
    "olx:data/sounds/death2.wav",
    "olx:data/sounds/death3.wav",
];

/// What a [`EventKind::TerrainEdit`] did to the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainOp {
    /// `CMap::CarveHole`: an explosion, a `Carve` action, digging, or a spawn hole.
    Carve,
    /// `CMap::PlaceDirt`.
    Dirt,
    /// `CMap::PlaceGreenDirt`. Its shades are cosmetic: OLX draws them from
    /// `rand()`, so the renderer picks its own (spec §18 D5).
    GreenDirt,
    /// `CMap::CarveHole` at the point a beam stopped (`ProcessShot_Beam`,
    /// `DrawBeam`).
    Beam,
}

/// The stamp a [`EventKind::TerrainEdit`] applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampId {
    /// The theme's hole `0..=4` (`hole1` to `hole5`), as the simulation's.
    Hole(u8),
    /// The green-dirt mask (`data/gfx/greenball.png`).
    GreenBall,
}

/// A cosmetic entity (`SpawnEntity`'s `ENT_*` type, plus the beam that
/// `SetWormBeamEntity` draws). `pos` and `vel` are OLX's arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CosmeticKind {
    /// `ENT_BLOOD` with colour `(red, 0, 0)`.
    Blood { red: u8 },
    /// `ENT_BLOODDROPPER`, colour `(128, 0, 0)`.
    BloodDropper,
    /// `ENT_GIB` with worm `worm`'s gib image (its colorized `giblets.png`).
    Gib { worm: i32 },
    /// `ENT_PARTICLE`: thrown dirt, coloured like the map's colour image at
    /// `probe`. A renderer samples the colour image after applying the
    /// preceding edits in this buffer (the event precedes its own carve).
    /// `probe` is in material (world) pixels: scale it by the level's
    /// `art_scale` to read the colour image (OLX's `getColorAt` reads the
    /// top-left art pixel). Each source probes its own pixel: digging and an explosion
    /// the first dirt pixel they found near `pos` (`CarveHole(CVec)`,
    /// `CMap.cpp:2181-2192`; `CClient::Explosion`, `CClient_Game.cpp:233-251`),
    /// a rope hook its wrapped position (`PhysicsLX56.cpp:603-606`).
    Dirt { probe: (i32, i32) },
    /// `ENT_EXPLOSION` of this size. `proj` is the `SimMod` index of the
    /// projectile that exploded (`effects::explosion` from its effects or
    /// a `Bounce` explosion), [`BEAM_EXPLOSION`] for a beam's (issue #69:
    /// the beam's tip light replaces its light), -1 for any other: a bonus
    /// blown up, a projectile's explosive trail. Presentation only (issue
    /// #15: the projectile look's explosion light); OLX's entity has no
    /// such field.
    Explosion { size: i32, proj: i32 },
    /// `ENT_SPARKLE`: an air jump.
    Sparkle,
    /// `ENT_SPAWN`: a worm or a bonus appears.
    Spawn,
    /// `ENT_SMOKE`, a smoke trail.
    Smoke,
    /// `ENT_CHEMSMOKE`, a chemical smoke trail.
    ChemSmoke,
    /// `ENT_DOOMSDAY`, a doomsday trail.
    Doomsday,
    /// `ENT_JETPACKSPRAY`.
    JetpackSpray,
    /// `ENT_LASERSIGHT` of worm `worm` (an id): `vel` is the unit aim
    /// direction and `len` the length OLX passes (`CClient_Game.cpp:807`):
    /// the pixels the 3 px trace went before the first solid pixel or worm
    /// (a multiple of 3, at most 9999).
    LaserSight { worm: i32, len: i16 },
    /// The beam `DrawBeam` draws for this worm in this sub-step: from `pos`
    /// (the worm) to `pos + vel` (where the trace stopped), in the beam
    /// weapon's `Colour` (`SetWormBeamEntity`, `CClient_Game.cpp:699-710`).
    /// Issue #69 (presentation only, OLX's entity has neither): `weapon`,
    /// the beam weapon's index, and `stopped`, whether a part of it
    /// stopped on rock, dirt or a worm (false: it ran its length or left
    /// the map).
    Beam { worm: i32, colour: Rgba, weapon: i32, stopped: bool },
    /// Issue #69 (presentation only): where a part of this worm's beam
    /// stopped on rock or dirt in this sub-step (`pos`; `vel` is zero),
    /// for the beam's heat. `parts` is how many such events the sub-step's
    /// beam gave, the first [`MAX_BEAM_HITS`] of its parts that stopped
    /// on terrain (centre outward), so a wide beam splits its heat.
    BeamHit { worm: i32, weapon: i32, parts: u8 },
}

/// [`CosmeticKind::Explosion`]'s `proj` for a beam's explosions (issue
/// #69): its carve on rock or dirt, its hit on a worm.
pub const BEAM_EXPLOSION: i32 = -2;

/// The most [`CosmeticKind::BeamHit`]s one beam gives a sub-step (issue
/// #69): a beam of a hostile width never crowds the cosmetics out.
pub const MAX_BEAM_HITS: usize = 8;

/// One thing that happened during a tick, and when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event {
    /// How many of the tick's physics sub-steps had run OLX's entity pass
    /// (`SimulateEntities`, after the worm loop of each sub-step,
    /// `CClient_Game.cpp:206-208`) when this happened: from 0 (setup, shot
    /// delivery and the worm loop of the first sub-step) up to
    /// [`crate::World::substeps_last_tick`] (projectiles and bonuses of the
    /// last sub-step, and the server after the physics). A renderer that
    /// spawns each entity right before the tick's entity pass number
    /// `substep` (from 0; `substeps_last_tick()` means after the last one)
    /// steps it exactly as often as OLX's client does.
    pub substep: u8,
    /// Part of a worm's death effects: its sound, 7 gibs and 50 rounds of
    /// blood (`death_effects`). They count against the gameplay quota, so
    /// no tick's other cosmetics crowd them out, and `olr-fx` lets their
    /// entities use the pool's death reserve: every death shows (spec §18
    /// D9 as amended 2026-10-13).
    pub death: bool,
    pub kind: EventKind,
}

/// What happened (spec §5.7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// A sound starts at `pos`.
    Sound { id: SoundId, pos: Vec2 },
    /// The map changed: `stamp` applied by `op` at `(x, y)`, the integer
    /// position OLX passes to the map function. Replaying the same calls on
    /// the colour images keeps them in step with the material grid.
    TerrainEdit { op: TerrainOp, stamp: StampId, x: i32, y: i32 },
    /// `CClient::Explosion` at `pos` with `damage` (its hole is a separate
    /// [`EventKind::TerrainEdit`]). `shake` is OLX's viewport shake
    /// argument: the projectile's largest `Shake` among its explode actions,
    /// 0 for a `BounceExplode`, 5 for a bonus blown up by an explosion or a
    /// beam (`CClient_Game.cpp:268`, `:1000`). `proj` is the exploded
    /// projectile's `SimMod` index, -1 for none, as
    /// [`CosmeticKind::Explosion`]'s: presentation only (issue #42: the
    /// projectile look's explosion heat), in no checksum.
    Explosion { pos: Vec2, damage: f32, shake: i32, proj: i32 },
    /// A cosmetic entity.
    SpawnCosmetic { kind: CosmeticKind, pos: Vec2, vel: Vec2 },
    /// Worm `worm` fired `weapon` (a weapon id): the client processed its
    /// projectile or beam shot (`CClient::ProcessShot`), where OLX plays the
    /// weapon's sound and draws the muzzle flash.
    WeaponFired { worm: i32, weapon: i32 },
    /// Worm `worm` lost `amount` health (negative heals) to worm `by` (-1
    /// for nobody), after the damage and shield factors. `dealt` is the
    /// amount OLX credits to the attacker: `MIN(health before, amount)`
    /// (`CClient::InjureWorm`, `CClient_Game.cpp:380`), what damage popups sum.
    Damage { worm: i32, amount: f32, by: i32, dealt: f32 },
    /// Worm `victim` died, credited to `killer` (-1 for nobody, the victim
    /// itself for a suicide).
    Kill { victim: i32, killer: i32 },
    /// Worm `worm` picked up a bonus.
    BonusPickup { worm: i32, kind: BonusItem },
    /// Worm `worm` spawned or respawned.
    Spawned { worm: i32 },
    /// A game mode's message for the players ([`crate::ModeCtx::message`]).
    Message(Message),
    /// The match ended (`GameServer::GameOver`), once.
    GameOver(MatchResult),
    /// Live copy `copy` (an index into [`crate::LiveLevel::copies`])
    /// entered its state `state` (issue #86): at setup, its starting state
    /// (with the first tick's events), or a setup call's. No OLX counterpart.
    LiveState { copy: u16, state: u16 },
    /// A script's run failed (issue #88): an error, an overflow, a limit;
    /// it applied none of its commands. `instance` indexes the level's
    /// script instances in run order (`olr_sim::live_instances`). No OLX
    /// counterpart.
    ScriptError { instance: u16 },
    /// Live copy `copy`'s value `prop` (a [`crate::LiveCopy::props`]
    /// index) reached its motion's target (issue #89), in the motions'
    /// step before the frame's first physics sub-step. No OLX counterpart.
    LiveArrived { copy: u16, prop: u16 },
}

impl EventKind {
    /// The quota an event counts against by its shape; a death's effects
    /// are classified where they are emitted ([`TickEvents::death`]).
    fn class(&self) -> Class {
        match self {
            EventKind::SpawnCosmetic { .. } | EventKind::Sound { .. } => Class::Cosmetic,
            EventKind::TerrainEdit { .. } => Class::Edit,
            _ => Class::Gameplay,
        }
    }
}

/// Which quota of [`TickEvents`] an event counts against.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Cosmetic,
    Edit,
    Gameplay,
}

/// The map area a dropped [`EventKind::TerrainEdit`] could have changed: the
/// union of the stamps' boxes, `x0..x1` × `y0..y1` (exclusive ends), not
/// clipped to the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LostArea {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl LostArea {
    fn union(a: Option<LostArea>, b: LostArea) -> LostArea {
        match a {
            None => b,
            Some(a) => {
                LostArea { x0: a.x0.min(b.x0), y0: a.y0.min(b.y0), x1: a.x1.max(b.x1), y1: a.y1.max(b.y1) }
            }
        }
    }
}

/// The events of a tick, in the order they happened, in a buffer the caller
/// owns and passes to [`crate::World::tick`], which refills it. Events from
/// setup calls since the last tick ([`crate::World::spawn_worm`]) come first.
/// [`TickEvents::new`] sizes it once (32-byte events, about 1.5 MB), so ticking does not
/// allocate (spec §5.3); it is not part of the world, so a cloned world (a
/// snapshot) copies no events.
///
/// Three quotas share the buffer, so no class of events can crowd out
/// another: sounds and cosmetics ([`TickEvents::COSMETIC_LIMIT`]), terrain
/// edits ([`TickEvents::EDIT_LIMIT`]) and the other gameplay events
/// ([`TickEvents::GAMEPLAY_LIMIT`]), a death's sound, gibs and blood
/// included ([`Event::death`]). What does not fit is counted in
/// [`TickEvents::cosmetic_dropped`] or [`TickEvents::gameplay_dropped`].
///
/// A renderer replays every [`EventKind::TerrainEdit`] on its colour images
/// (spec §5.4), so a lost edit would desync them for good. No fixed size
/// bounds the edits of every tick: a projectile edits the map at most 7
/// times a step (a `Carve` from its timer and from its hit, an explosion's
/// hole, three `PlaceDirt` and one `PlaceGreenDirt`, the last three ending
/// it; an explosion also blows up the bonuses near it, at most 32 a tick),
/// but how many steps it catches up in one tick grows with the
/// `GameSpeed` settings and, through children spawned into lower pool slots,
/// with the mod; and a beam carves once per width part. `EDIT_LIMIT` covers
/// 7 edits for every slot of a full default pool (3000 × 7 = 21000) with
/// room for worms, beams and bonuses; the most a smoke match makes in a
/// tick is 59. A tick beyond it does not drop edits silently: they count in
/// [`TickEvents::gameplay_dropped`], and [`TickEvents::lost_area`] gives the
/// area they covered, which a renderer must rebuild from the material grid.
#[derive(Debug, PartialEq)]
pub struct TickEvents {
    list: Vec<Event>,
    /// Events in `list` per [`Class`].
    counts: [usize; 3],
    cosmetic_dropped: u32,
    gameplay_dropped: u32,
    lost: Option<LostArea>,
    /// The [`Event::substep`] of the events pushed now.
    substep: u8,
}

impl Default for TickEvents {
    fn default() -> Self {
        Self::new()
    }
}

impl TickEvents {
    pub const COSMETIC_LIMIT: usize = 7168;
    pub const EDIT_LIMIT: usize = 32768;
    pub const GAMEPLAY_LIMIT: usize = 8192;
    pub const CAPACITY: usize = Self::COSMETIC_LIMIT + Self::EDIT_LIMIT + Self::GAMEPLAY_LIMIT;

    /// An empty buffer with room for [`TickEvents::CAPACITY`] events.
    pub fn new() -> Self {
        Self::with_capacity(Self::CAPACITY)
    }

    /// The world's buffer for setup events between ticks: they are few, and
    /// a cloned world copies them.
    pub(crate) fn pending() -> Self {
        Self::with_capacity(0)
    }

    fn with_capacity(n: usize) -> Self {
        Self {
            list: Vec::with_capacity(n),
            counts: [0; 3],
            cosmetic_dropped: 0,
            gameplay_dropped: 0,
            lost: None,
            substep: 0,
        }
    }

    pub fn as_slice(&self) -> &[Event] {
        &self.list
    }

    pub fn iter(&self) -> core::slice::Iter<'_, Event> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Sounds and cosmetics that did not fit this tick (harmless: the
    /// renderer shows fewer effects).
    pub fn cosmetic_dropped(&self) -> u32 {
        self.cosmetic_dropped
    }

    /// Gameplay events, terrain edits included, that did not fit this tick.
    /// Nonzero only beyond the limits above; never with Classic or MW 1.0
    /// in the smoke matches.
    pub fn gameplay_dropped(&self) -> u32 {
        self.gameplay_dropped
    }

    /// The area of the terrain edits dropped this tick, if any: the renderer
    /// must rebuild it from the material grid ([`crate::World::terrain`]).
    pub fn lost_area(&self) -> Option<LostArea> {
        self.lost
    }

    fn limit(class: Class) -> usize {
        match class {
            Class::Cosmetic => Self::COSMETIC_LIMIT,
            Class::Edit => Self::EDIT_LIMIT,
            Class::Gameplay => Self::GAMEPLAY_LIMIT,
        }
    }

    /// Adds `e` at the current sub-step within its class's quota. Returns
    /// whether it fit.
    pub(crate) fn push(&mut self, e: EventKind) -> bool {
        self.push_as(e, e.class(), false)
    }

    /// Adds `e`, a part of a worm's death effects ([`Event::death`]), within
    /// the gameplay quota, whatever its shape. Returns whether it fit.
    pub(crate) fn death(&mut self, e: EventKind) -> bool {
        self.push_as(e, Class::Gameplay, true)
    }

    fn push_as(&mut self, e: EventKind, class: Class, death: bool) -> bool {
        let n = &mut self.counts[class as usize];
        if *n < Self::limit(class) {
            *n += 1;
            self.list.push(Event { substep: self.substep, death, kind: e });
            true
        } else if class == Class::Cosmetic {
            self.cosmetic_dropped = self.cosmetic_dropped.saturating_add(1);
            false
        } else {
            self.gameplay_dropped = self.gameplay_dropped.saturating_add(1);
            false
        }
    }

    pub(crate) fn sound(&mut self, id: SoundId, pos: Vec2) {
        self.push(EventKind::Sound { id, pos });
    }

    pub(crate) fn cosmetic(&mut self, kind: CosmeticKind, pos: Vec2, vel: Vec2) {
        self.push(EventKind::SpawnCosmetic { kind, pos, vel });
    }

    /// A terrain edit with a `w` × `h` stamp centred as OLX centres it
    /// (`x - w / 2`). A dropped edit's box joins [`lost_area`](Self::lost_area).
    pub(crate) fn edit(&mut self, op: TerrainOp, stamp: StampId, x: i32, y: i32, (w, h): (u32, u32)) {
        if !self.push(EventKind::TerrainEdit { op, stamp, x, y }) {
            let (w, h) = (w.min(i32::MAX as u32) as i32, h.min(i32::MAX as u32) as i32);
            let (x0, y0) = (x.saturating_sub(w / 2), y.saturating_sub(h / 2));
            self.add_lost(LostArea { x0, y0, x1: x0.saturating_add(w), y1: y0.saturating_add(h) });
        }
    }

    fn add_lost(&mut self, area: LostArea) {
        self.lost = Some(LostArea::union(self.lost, area));
    }

    /// Stamps the events pushed from now on with `substep`.
    pub(crate) fn set_substep(&mut self, substep: u8) {
        self.substep = substep;
    }

    fn clear(&mut self) {
        self.list.clear();
        self.counts = [0; 3];
        self.cosmetic_dropped = 0;
        self.gameplay_dropped = 0;
        self.lost = None;
        self.substep = 0;
    }

    /// At the start of a tick: empties this buffer, then moves the setup
    /// events waiting in `pending` (and their drop counts) into it.
    pub(crate) fn start_tick(&mut self, pending: &mut TickEvents) {
        self.clear();
        for e in &pending.list {
            if e.death {
                self.death(e.kind);
            } else {
                self.push(e.kind);
            }
        }
        self.cosmetic_dropped = self.cosmetic_dropped.saturating_add(pending.cosmetic_dropped);
        self.gameplay_dropped = self.gameplay_dropped.saturating_add(pending.gameplay_dropped);
        if let Some(area) = pending.lost {
            self.add_lost(area);
        }
        pending.clear();
    }
}

impl Clone for TickEvents {
    /// Keeps the capacity, so a clone of a full-sized buffer ticks without
    /// allocating either, and a clone of the world's small setup buffer stays small.
    fn clone(&self) -> Self {
        let mut list = Vec::with_capacity(self.list.capacity());
        list.extend_from_slice(&self.list);
        let Self { list: _, counts, cosmetic_dropped, gameplay_dropped, lost, substep } = *self;
        Self { list, counts, cosmetic_dropped, gameplay_dropped, lost, substep }
    }
}

impl<'a> IntoIterator for &'a TickEvents {
    type Item = &'a Event;
    type IntoIter = core::slice::Iter<'a, Event>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "events_data_tests.rs"]
mod data_tests;

#[cfg(test)]
#[path = "events_source_tests.rs"]
mod source_tests;
