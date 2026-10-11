//! What must hold after every tick of a headless match (spec §7.1), as the
//! plan-4 smoke matches check (`rl-scenario/tests/smoke.rs`), except that
//! health may pass 100: OLX's `CGameObject::injure` does not cap a heal
//! (MW 1.0 has healing weapons), so health is only a number, at least 0.

use rl_sim::{MAX_BONUSES, TickEvents, World};
use rl_terrain::PX_ROCK;

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
        if w.alive && world.terrain().flags(w.pos.x as i64, w.pos.y as i64) & PX_ROCK != 0 {
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
