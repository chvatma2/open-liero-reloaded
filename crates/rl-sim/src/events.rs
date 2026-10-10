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

use rl_math::Vec2;

use crate::bonus::BonusItem;

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

/// What a [`Event::TerrainEdit`] did to the map.
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

/// The stamp a [`Event::TerrainEdit`] applied.
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
    /// `ENT_GIB` with the worm's gib image.
    Gib,
    /// `ENT_PARTICLE`: thrown dirt, coloured like the map at `pos`.
    Dirt,
    /// `ENT_EXPLOSION` of this size.
    Explosion { size: i32 },
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
    /// `ENT_LASERSIGHT`: `vel` is the unit aim direction and `len` the
    /// length OLX passes (`CClient_Game.cpp:807`): the pixels the 3 px trace
    /// went before the first solid pixel or worm (a multiple of 3, at most 9999).
    LaserSight { len: i16 },
    /// The beam `DrawBeam` draws for this worm in this sub-step: from `pos`
    /// (the worm) to `pos + vel` (where the trace stopped).
    Beam { worm: i32 },
}

/// One thing that happened during a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A sound starts at `pos`.
    Sound { id: SoundId, pos: Vec2 },
    /// The map changed: `stamp` applied by `op` at `(x, y)`, the integer
    /// position OLX passes to the map function. Replaying the same calls on
    /// the colour images keeps them in step with the material grid.
    TerrainEdit { op: TerrainOp, stamp: StampId, x: i32, y: i32 },
    /// `CClient::Explosion` at `pos` with `damage` (its hole is a separate
    /// [`Event::TerrainEdit`]).
    Explosion { pos: Vec2, damage: f32 },
    /// A cosmetic entity.
    SpawnCosmetic { kind: CosmeticKind, pos: Vec2, vel: Vec2 },
    /// Worm `worm` fired `weapon` (a weapon id): the client processed its
    /// projectile or beam shot (`CClient::ProcessShot`), where OLX plays the
    /// weapon's sound and draws the muzzle flash.
    WeaponFired { worm: i32, weapon: i32 },
    /// Worm `worm` lost `amount` health (negative heals) to worm `by` (-1
    /// for nobody), after the damage and shield factors.
    Damage { worm: i32, amount: f32, by: i32 },
    /// Worm `victim` died, credited to `killer` (-1 for nobody, the victim
    /// itself for a suicide).
    Kill { victim: i32, killer: i32 },
    /// Worm `worm` picked up a bonus.
    BonusPickup { worm: i32, kind: BonusItem },
    /// Worm `worm` spawned or respawned.
    Spawned { worm: i32 },
}

impl Event {
    fn is_cosmetic(&self) -> bool {
        matches!(self, Event::SpawnCosmetic { .. } | Event::Sound { .. })
    }
}

/// The events of the last tick, in the order they happened, in a buffer
/// sized once so that ticking does not allocate (spec §5.3). Events from
/// setup calls since the last tick ([`crate::World::spawn_worm`]) come first.
///
/// The buffer holds [`TickEvents::CAPACITY`] events. Sounds and cosmetics
/// stop at [`TickEvents::COSMETIC_LIMIT`], which keeps the rest for gameplay
/// events; what does not fit is counted in [`TickEvents::dropped`].
#[derive(Debug, PartialEq)]
pub struct TickEvents {
    list: Vec<Event>,
    dropped: u32,
    /// The buffer holds setup events for the next tick.
    carried: bool,
}

impl TickEvents {
    pub const CAPACITY: usize = 8192;
    pub const COSMETIC_LIMIT: usize = Self::CAPACITY - 1024;

    pub(crate) fn new() -> Self {
        Self { list: Vec::with_capacity(Self::CAPACITY), dropped: 0, carried: false }
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

    /// Events that did not fit in the buffer this tick.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub(crate) fn push(&mut self, e: Event) {
        let limit = if e.is_cosmetic() { Self::COSMETIC_LIMIT } else { Self::CAPACITY };
        if self.list.len() < limit {
            self.list.push(e);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    pub(crate) fn sound(&mut self, id: SoundId, pos: Vec2) {
        self.push(Event::Sound { id, pos });
    }

    pub(crate) fn cosmetic(&mut self, kind: CosmeticKind, pos: Vec2, vel: Vec2) {
        self.push(Event::SpawnCosmetic { kind, pos, vel });
    }

    pub(crate) fn edit(&mut self, op: TerrainOp, stamp: StampId, x: i32, y: i32) {
        self.push(Event::TerrainEdit { op, stamp, x, y });
    }

    /// At the start of a tick: drops the last tick's events, unless setup
    /// events wait in the buffer.
    pub(crate) fn begin_tick(&mut self) {
        if !self.carried {
            self.list.clear();
            self.dropped = 0;
        }
        self.carried = false;
    }

    /// Before a setup call that emits: the last tick's events go, once.
    pub(crate) fn begin_setup(&mut self) {
        if !self.carried {
            self.list.clear();
            self.dropped = 0;
            self.carried = true;
        }
    }
}

impl Clone for TickEvents {
    /// Keeps the full capacity, so a cloned world's ticks do not allocate either.
    fn clone(&self) -> Self {
        let mut list = Vec::with_capacity(Self::CAPACITY.max(self.list.len()));
        list.extend_from_slice(&self.list);
        Self { list, dropped: self.dropped, carried: self.carried }
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
