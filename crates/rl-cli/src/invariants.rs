//! What must hold after every tick of a headless match (spec §7.1), as the
//! plan-4 smoke matches check (`rl-scenario/tests/smoke.rs`), except that
//! health may pass 100: OLX's `CGameObject::injure` does not cap a heal
//! (MW 1.0 has healing weapons), so health is only a number, at least 0.
//! OLX: `src/common/CGameObject.cpp:35-44` (`injure`),
//! `src/client/CClient_Game.cpp:350-357` (`InjureWorm` caps a heal only
//! when `shieldFactor` is 0).

use rl_sim::{MAX_BONUSES, TickEvents, World};
use rl_terrain::{PX_ROCK, Terrain};

/// The dedicated server's entity pool (`tEntities`, 1024 entries).
const ENTITY_CAP: u32 = 1024;

/// Checks `world` after a tick whose events are `events`; gives what broke.
pub fn check(world: &World, events: &TickEvents) -> Result<(), String> {
    for (i, w) in world.worms().iter().enumerate() {
        if !(w.health >= 0.0 && w.health.is_finite()) {
            return Err(format!("worm {i} has health {}", w.health));
        }
        if !(w.pos.x.is_finite() && w.pos.y.is_finite() && w.vel.x.is_finite() && w.vel.y.is_finite()) {
            return Err(format!("worm {i} moves to {:?} at {:?}", w.pos, w.vel));
        }
        if w.alive && buried_in_rock(world.terrain(), w.pos.x, w.pos.y) {
            return Err(format!("worm {i} is inside rock at {:?}", w.pos));
        }
    }
    let projectiles = world.projectiles().count();
    if projectiles > world.projectile_capacity() {
        return Err(format!("{projectiles} projectiles in a pool of {}", world.projectile_capacity()));
    }
    if world.entity_count() > ENTITY_CAP {
        return Err(format!("{} entities", world.entity_count()));
    }
    let bonuses = world.bonuses().iter().filter(|b| b.used).count();
    if bonuses > MAX_BONUSES {
        return Err(format!("{bonuses} bonuses"));
    }
    if events.cosmetic_dropped() > 0 {
        return Err(format!("{} cosmetic events were dropped", events.cosmetic_dropped()));
    }
    // A dropped terrain edit would desync a renderer's colour image.
    if events.gameplay_dropped() > 0 || events.lost_area().is_some() {
        return Err(format!("{} gameplay events were dropped", events.gameplay_dropped()));
    }
    if let Some(e) = events.iter().find(|e| e.substep > world.substeps_last_tick()) {
        return Err(format!("{e:?} after the tick's {} sub-steps", world.substeps_last_tick()));
    }
    Ok(())
}

/// Whether a worm at `(x, y)` is buried in rock: its centre and every pixel
/// of OLX's collision cross (its row from -3 to +3, its column from -4 to
/// +5) are rock. A centre in rock alone is not: OLX's collision can push a
/// worm standing in dirt under a low rock ceiling up into it, where it stays
/// wedged (`moveAndCheckWormCollision` takes a hit at the worm's own row for
/// the floor and lifts it 5 px, rows it no longer checks:
/// `src/common/PhysicsLX56.cpp:216-238`); a worm that went through rock
/// would have no open pixel around it.
pub fn buried_in_rock(t: &Terrain, x: f32, y: f32) -> bool {
    let (cx, cy) = (x as i64, y as i64);
    let rock = |px: i64, py: i64| t.flags(px, py) & PX_ROCK != 0;
    (-3..=3).all(|dx| rock(cx + dx, cy)) && (-4..=5).all(|dy| rock(cx, cy + dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::level::{Material, MaterialGrid};

    /// 40 × 40 of background with rock rows 0-19 (a ceiling at y = 19).
    fn ceiling() -> Terrain {
        let cells =
            (0..40 * 40).map(|i| if i / 40 < 20 { Material::Rock } else { Material::Background }).collect();
        Terrain::from_grid(&MaterialGrid::from_cells(40, 40, cells).unwrap())
    }

    #[test]
    fn a_worm_in_the_open_is_not_buried() {
        assert!(!buried_in_rock(&ceiling(), 20.5, 30.5));
    }

    /// OLX's collision can push a worm's centre into rock: standing in dirt
    /// under a low ceiling, a jump takes the dirt at its own row for a floor
    /// and lifts it 5 px (`PhysicsLX56.cpp:216-238`, the `y = 0` case),
    /// where it stays wedged (seed 303 of the grid, Classic on
    /// FossilFacility). Its collision cross still reaches open space.
    #[test]
    fn a_worm_wedged_in_a_ceiling_like_olx_is_allowed() {
        assert!(!buried_in_rock(&ceiling(), 20.9, 19.5));
    }

    #[test]
    fn a_worm_inside_solid_rock_is_buried() {
        assert!(buried_in_rock(&ceiling(), 20.5, 8.5));
    }
}
