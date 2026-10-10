//! Worm damage: OLX's `CClient::InjureWorm`
//! (`src/client/CClient_Game.cpp:335-460`).
//!
//! So far only the health change (Task 6): the blood and gib draws, the
//! damage report, death and the kill are Task 7's.

use crate::proj_sim::ProjCtx;

/// `CClient::InjureWorm(w, damage, owner)` on the worm with id `id`, without
/// its draws or death yet. With LX56's defaults (`WormDamageFactor` and
/// `WormShieldFactor` 1, `SelfInjure` and `TeamInjure` on, deathmatch), the
/// damage reaches `CGameObject::injure` (`src/common/CGameObject.cpp:35-44`)
/// unchanged: health falls by `damage` and stops at 0.
pub(crate) fn injure_worm(ctx: &mut ProjCtx, id: i32, damage: f32, _owner: i32) {
    let Some(w) = ctx.worms.iter_mut().find(|w| w.id == id) else { return };
    if !w.alive {
        return;
    }
    w.health -= damage;
    if w.health < 0.0 {
        // Task 7: the worm dies here (`killWorm`, gibs and blood).
        w.health = 0.0;
    }
}
