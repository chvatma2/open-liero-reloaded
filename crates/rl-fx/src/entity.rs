//! One cosmetic entity (`entity_t`, `include/Entity.h`).

use rl_data::image::Rgba;
use rl_math::{TimeDiff, Vec2};
use rl_sim::CosmeticKind;
use rl_terrain::{PX_DIRT, PX_ROCK, Terrain};

/// An entity's type (`ENT_*`, `include/Entity.h:34-48`), without the beam,
/// which `rl-scene` draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `ENT_PARTICLE`: thrown dirt.
    Particle,
    /// `ENT_GIB` with worm `worm`'s gib image.
    Gib {
        worm: i32,
    },
    Spawn,
    Blood,
    BloodDropper,
    Explosion,
    Smoke,
    ChemSmoke,
    Sparkle,
    Doomsday,
    JetpackSpray,
    /// `ENT_LASERSIGHT`: takes a slot for two passes; `rl-scene` draws it.
    LaserSight,
}

/// `entity_t`'s fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entity {
    pub kind: Kind,
    /// `iType2`: an explosion's size, a sparkle's fade speed.
    pub type2: i32,
    pub pos: Vec2,
    pub vel: Vec2,
    /// `iColour`: particles, blood and droppers.
    pub colour: Rgba,
    /// `fFrame`, the animation frame.
    pub frame: f32,
    /// `fExtra`: a dropper's time since its last drop.
    pub extra: TimeDiff,
    /// `fAnglVel`: a gib's spin.
    pub angl_vel: f32,
    /// `iRotation`: a gib's frame.
    pub rotation: i32,
}

/// `realdt`: `tLX->fRealDeltaTime`, fixed at 10 ms inside the game loop
/// (`src/game/Game.cpp:905`), as `seconds()` gives it.
fn real_dt() -> f32 {
    TimeDiff(10).seconds()
}

/// `doomsday[]` (`Entity.cpp:81-86`).
pub(crate) const DOOMSDAY: [Rgba; 4] =
    [Rgba::rgb(244, 244, 112), Rgba::rgb(248, 192, 36), Rgba::rgb(248, 108, 20), Rgba::rgb(248, 108, 20)];

impl Entity {
    /// `SpawnEntity`'s fields for a new entity of `kind` (its gib spin and
    /// frame are drawn by the caller): `fFrame`, `fLife`, `fExtra` 0, except
    /// an explosion's frame `15 - size`. `None` for a beam, which `rl-scene`
    /// draws.
    ///
    /// OLX: src/client/Entity.cpp:161-205
    pub(crate) fn new(kind: CosmeticKind, pos: Vec2, vel: Vec2, colour: Rgba) -> Option<Entity> {
        let (kind, type2, colour) = match kind {
            CosmeticKind::Blood { red } => (Kind::Blood, 0, Rgba::rgb(red, 0, 0)),
            CosmeticKind::BloodDropper => (Kind::BloodDropper, 0, Rgba::rgb(128, 0, 0)),
            CosmeticKind::Gib { worm } => (Kind::Gib { worm }, 0, Rgba::BLACK),
            CosmeticKind::Dirt { .. } => (Kind::Particle, 0, colour),
            CosmeticKind::Explosion { size } => (Kind::Explosion, size, Rgba::BLACK),
            // The sim's sparkles are the air jump's (`PhysicsLX56.cpp:390-392`): fade 10.
            CosmeticKind::Sparkle => (Kind::Sparkle, 10, Rgba::BLACK),
            CosmeticKind::Spawn => (Kind::Spawn, 0, Rgba::BLACK),
            CosmeticKind::Smoke => (Kind::Smoke, 0, Rgba::BLACK),
            CosmeticKind::ChemSmoke => (Kind::ChemSmoke, 0, Rgba::BLACK),
            CosmeticKind::Doomsday => (Kind::Doomsday, 0, Rgba::BLACK),
            // `CClient_Game.cpp:571`: `blue`, which drawing ignores.
            CosmeticKind::JetpackSpray => (Kind::JetpackSpray, 0, Rgba::rgb(80, 150, 200)),
            CosmeticKind::LaserSight { len, .. } => (Kind::LaserSight, len as i32, Rgba::BLACK),
            CosmeticKind::Beam { .. } => return None,
        };
        let frame = if kind == Kind::Explosion { 15i32.wrapping_sub(type2) as f32 } else { 0.0 };
        Some(Entity { kind, type2, pos, vel, colour, frame, extra: TimeDiff(0), angl_vel: 0.0, rotation: 0 })
    }

    /// Whether gravity moves it and the map stops it.
    fn falls(&self) -> bool {
        matches!(
            self.kind,
            Kind::Gib { .. } | Kind::JetpackSpray | Kind::Particle | Kind::Blood | Kind::BloodDropper
        )
    }
}

/// What one pass did to an entity besides moving it.
pub(crate) enum Landed {
    Nothing,
    /// Remove it.
    Gone,
    /// Blood: stain this world pixel, then remove it.
    Stain(i32, i32),
    /// A gib: stamp worm `worm`'s low-res frame `rotation` with the top-left
    /// at `(x, y)`, then remove it.
    Stamp {
        x: i32,
        y: i32,
        worm: i32,
        rotation: i32,
    },
}

/// The collision part of `SimulateEntities` for one entity: falling
/// entities get gravity, move, and are removed outside the map or on rock
/// or dirt (blood stains, a slow gib stamps, a fast gib bounces).
///
/// Deviation: a falling entity whose position is not finite is removed
/// (OLX's `(uint)` cast of NaN is undefined), so it cannot hold a slot forever.
///
/// OLX: src/client/Entity.cpp:333-397
pub(crate) fn fall(e: &mut Entity, dt: TimeDiff, material: &Terrain) -> Landed {
    if !e.falls() {
        return Landed::Nothing;
    }
    if let Kind::Gib { .. } = e.kind {
        e.rotation = e.rotation.wrapping_add((e.angl_vel * real_dt()) as i32) % 5;
    }
    let dt = dt.seconds();
    e.vel.y += 100.0 * dt;
    e.pos += e.vel * dt;
    if !(e.pos.x.is_finite() && e.pos.y.is_finite()) {
        return Landed::Gone;
    }
    let (w, h) = (material.width(), material.height());
    if e.pos.x < 0.0 || e.pos.y < 0.0 || e.pos.x as u32 >= w || e.pos.y as u32 >= h {
        return Landed::Gone;
    }
    let (x, y) = (e.pos.x as u32 as i64, e.pos.y as u32 as i64);
    if material.flags(x, y) & (PX_ROCK | PX_DIRT) == 0 {
        return Landed::Nothing;
    }
    match e.kind {
        Kind::Blood => Landed::Stain(e.pos.x as i32, e.pos.y as i32),
        Kind::Gib { .. } if e.vel.length2() as i32 > 25600 => {
            // `EntityBounce` (`Entity.cpp:473-478`).
            e.vel.x *= -0.4;
            e.vel.y *= -0.4;
            e.angl_vel *= 0.8;
            Landed::Nothing
        }
        Kind::Gib { worm } => Landed::Stamp {
            x: (e.pos.x as i32).wrapping_sub(1),
            y: (e.pos.y as i32).wrapping_sub(1),
            worm,
            rotation: e.rotation,
        },
        _ => Landed::Gone,
    }
}

/// The animation part of `SimulateEntities` for one entity: frames advance
/// with OLX's fixed 10 ms frame time. Returns whether it ends here. A blood
/// dropper's drip is the caller's (it spawns).
///
/// OLX: src/client/Entity.cpp:401-464
pub(crate) fn animate(e: &mut Entity, dt: TimeDiff) -> bool {
    let rdt = real_dt();
    match e.kind {
        Kind::Explosion => {
            e.frame += rdt * 40.0;
            e.frame > 15.0
        }
        Kind::Smoke | Kind::ChemSmoke => {
            e.frame += rdt * 15.0;
            e.frame as i32 > 4
        }
        Kind::Doomsday => {
            e.frame += rdt * 15.0;
            e.frame as i32 > 3
        }
        Kind::Spawn => {
            e.frame += rdt * 15.0;
            e.frame as i32 > 5
        }
        Kind::Sparkle => {
            e.pos += e.vel * dt.seconds();
            let fade = if e.type2 != 0 { e.type2 } else { 5 };
            e.frame += rdt * fade as f32;
            e.frame as i32 > 2
        }
        Kind::JetpackSpray => {
            e.frame += rdt * 200.0;
            e.frame as i32 > 150
        }
        Kind::LaserSight => {
            let end = e.frame as i32 == 1;
            e.frame += 1.0;
            end
        }
        Kind::Particle | Kind::Gib { .. } | Kind::Blood | Kind::BloodDropper => false,
    }
}
