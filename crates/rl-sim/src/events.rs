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
    fn class(&self) -> Class {
        match self {
            Event::SpawnCosmetic { .. } | Event::Sound { .. } => Class::Cosmetic,
            Event::TerrainEdit { .. } => Class::Edit,
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

/// The map area a dropped [`Event::TerrainEdit`] could have changed: the
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

/// The events of the last tick, in the order they happened, in a buffer
/// sized once so that ticking does not allocate (spec §5.3). Events from
/// setup calls since the last tick ([`crate::World::spawn_worm`]) come first.
///
/// Three quotas share the buffer, so no class of events can crowd out
/// another: sounds and cosmetics ([`TickEvents::COSMETIC_LIMIT`]), terrain
/// edits ([`TickEvents::EDIT_LIMIT`]) and the other gameplay events
/// ([`TickEvents::GAMEPLAY_LIMIT`]). What does not fit is counted in
/// [`TickEvents::cosmetic_dropped`] or [`TickEvents::gameplay_dropped`].
///
/// A renderer replays every [`Event::TerrainEdit`] on its colour images
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
    /// The buffer holds setup events for the next tick.
    carried: bool,
}

impl TickEvents {
    pub const COSMETIC_LIMIT: usize = 7168;
    pub const EDIT_LIMIT: usize = 32768;
    pub const GAMEPLAY_LIMIT: usize = 8192;
    pub const CAPACITY: usize = Self::COSMETIC_LIMIT + Self::EDIT_LIMIT + Self::GAMEPLAY_LIMIT;

    pub(crate) fn new() -> Self {
        Self {
            list: Vec::with_capacity(Self::CAPACITY),
            counts: [0; 3],
            cosmetic_dropped: 0,
            gameplay_dropped: 0,
            lost: None,
            carried: false,
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

    /// Adds `e` within its class's quota. Returns whether it fit.
    pub(crate) fn push(&mut self, e: Event) -> bool {
        let class = e.class();
        let n = &mut self.counts[class as usize];
        if *n < Self::limit(class) {
            *n += 1;
            self.list.push(e);
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
        self.push(Event::Sound { id, pos });
    }

    pub(crate) fn cosmetic(&mut self, kind: CosmeticKind, pos: Vec2, vel: Vec2) {
        self.push(Event::SpawnCosmetic { kind, pos, vel });
    }

    /// A terrain edit with a `w` × `h` stamp centred as OLX centres it
    /// (`x - w / 2`). A dropped edit's box joins [`lost_area`](Self::lost_area).
    pub(crate) fn edit(&mut self, op: TerrainOp, stamp: StampId, x: i32, y: i32, (w, h): (u32, u32)) {
        if !self.push(Event::TerrainEdit { op, stamp, x, y }) {
            let (w, h) = (w.min(i32::MAX as u32) as i32, h.min(i32::MAX as u32) as i32);
            let (x0, y0) = (x.saturating_sub(w / 2), y.saturating_sub(h / 2));
            let area = LostArea { x0, y0, x1: x0.saturating_add(w), y1: y0.saturating_add(h) };
            self.lost = Some(LostArea::union(self.lost, area));
        }
    }

    fn clear(&mut self) {
        self.list.clear();
        self.counts = [0; 3];
        self.cosmetic_dropped = 0;
        self.gameplay_dropped = 0;
        self.lost = None;
    }

    /// At the start of a tick: drops the last tick's events, unless setup
    /// events wait in the buffer.
    pub(crate) fn begin_tick(&mut self) {
        if !self.carried {
            self.clear();
        }
        self.carried = false;
    }

    /// Before a setup call that emits: the last tick's events go, once.
    pub(crate) fn begin_setup(&mut self) {
        if !self.carried {
            self.clear();
            self.carried = true;
        }
    }
}

impl Clone for TickEvents {
    /// Keeps the full capacity, so a cloned world's ticks do not allocate either.
    fn clone(&self) -> Self {
        let mut list = Vec::with_capacity(Self::CAPACITY.max(self.list.len()));
        list.extend_from_slice(&self.list);
        let Self { list: _, counts, cosmetic_dropped, gameplay_dropped, lost, carried } = *self;
        Self { list, counts, cosmetic_dropped, gameplay_dropped, lost, carried }
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
