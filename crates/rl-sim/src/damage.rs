//! Worm damage and death: OLX's `CClient::InjureWorm`
//! (`src/client/CClient_Game.cpp:335-460`), `GameServer::killWorm`
//! (`src/server/CServer_Game.cpp:108-162`), `CGameMode::Kill`
//! (`src/server/CGameMode.cpp:69-252`) and `CWorm::Kill`
//! (`src/common/CWorm.cpp:1399-1413`).

use rl_math::{AbsTime, Vec2};

use crate::SimSettings;
use crate::events::{CosmeticKind, Event, SoundId};
use crate::projectile::c_int;
use crate::rng::Draws;
use crate::world::MAX_WORMS;
use crate::worm::Worm;

/// Everything damage and death read or change.
pub(crate) struct Harm<'a> {
    pub settings: &'a SimSettings,
    pub worms: &'a mut [Worm],
    pub draws: &'a mut Draws,
    /// `GetPhysicsTime()`: the frame clock.
    pub now: AbsTime,
}

/// OLX `WRM_OUT` (`src/game/CWorm.h:85`): out of lives.
pub(crate) const WRM_OUT: i32 = -1;
/// OLX `WRM_UNLIM`: unlimited lives.
pub(crate) const WRM_UNLIM: i32 = -2;

/// `tLXOptions->iBloodAmount`, in percent: OLX's default, which dedicated
/// servers do not change (`Options.cpp:213`; the trace header's `Game.Blood`).
const BLOOD_AMOUNT: i32 = 100;

fn index(worms: &[Worm], id: i32) -> Option<usize> {
    if !(0..MAX_WORMS as i32).contains(&id) {
        return None;
    }
    worms.iter().position(|w| w.id == id)
}

/// `CWorm::getScore` (`src/common/CWorm.cpp:1415-1428`) in deathmatch, where
/// the teamkill terms are 0.
fn score(w: &Worm, s: &SimSettings) -> i32 {
    let mut score = w.kills;
    if s.death_decreases_score > 0.0 {
        score = score.wrapping_sub(c_int(w.deaths as f32 * s.death_decreases_score + 0.01));
    }
    if s.suicide_decreases_score {
        score = score.wrapping_sub(w.suicides);
    }
    score
}

/// The gate of `CWorm::addDeath` and `addSuicide` (`CWorm.cpp:1430-1442`).
fn counts(w: &Worm, s: &SimSettings) -> bool {
    s.allow_negative_score || score(w, s) > 0
}

/// `CWorm_addDamage` (`CWorm.cpp:1600-1620`): the damage score of `w`, the
/// owner of damage to `victim`, outside team games.
fn add_damage(w: &mut Worm, damage: f32, victim: i32, s: &SimSettings) {
    if damage < 0.0 {
        return;
    }
    if w.id == victim {
        if s.suicide_decreases_score {
            w.damage -= damage;
        }
    } else {
        w.damage += damage;
    }
}

/// The three blood entities of one round of `InjureWorm`'s blood at `pos`,
/// each with a `GetRandomVec()` velocity (2 `rand()`) scaled by `sp`; the
/// dropper's by `(sp, dropper_y)`.
fn blood_round(d: &mut Draws, pos: Vec2, sp: f32, dropper_y: f32, reds: [u8; 2]) {
    let v = d.rand.random_vec();
    d.entity(CosmeticKind::BloodDropper, pos, Vec2::new(v.x * sp, v.y * dropper_y));
    for red in reds {
        let v = d.rand.random_vec() * sp;
        d.entity(CosmeticKind::Blood { red }, pos, v);
    }
}

/// `CClient::InjureWorm(w, damage, owner)` on the worm with id `id`, for a
/// local worm on the server (deathmatch, so team protection never applies):
/// - the owner's damage factor, then the victim's shield factor;
/// - `SelfInjure` off stops a worm's positive damage to itself;
/// - the owner's damage score gets `MIN(health, damage)`;
/// - `CGameObject::injure` (`src/common/CGameObject.cpp:35-44`) with the
///   full damage: below 0 health the worm dies (`killWorm`) and the death's
///   draws follow (sound, 7 gibs, 50 rounds of blood); exactly 0 lives on
///   until [`die_at_zero_health`];
/// - positive damage then bleeds `ceil(clamp(damage, 10, 100))` rounds.
///
/// The damage report to the server (`QueueReportDamage`) and its late second
/// count of the damage score are not ported (`docs/porting/STATUS.md`).
pub(crate) fn injure_worm(h: &mut Harm, id: i32, damage: f32, owner: i32) {
    let Some(vi) = index(h.worms, id) else { return };
    if !h.worms[vi].alive {
        return;
    }
    let s = h.settings;
    let oi = index(h.worms, owner);
    let me = oi == Some(vi);
    let mut damage = damage;
    if let Some(o) = oi {
        damage *= h.worms[o].damage_factor;
    }
    let health = h.worms[vi].health;
    let shield = h.worms[vi].shield_factor;
    if shield == 0.0 {
        if damage > 0.0 {
            damage = health + 1.0;
        } else if damage < 0.0 {
            damage = min(health - 100.0, 0.0);
        }
    } else {
        damage /= shield;
    }
    if damage > 0.0 && oi.is_some() && !s.self_injure && me {
        return;
    }
    let real = min(health, damage);
    if let Some(o) = oi {
        add_damage(&mut h.worms[o], real, id, s);
    }
    let w = &mut h.worms[vi];
    w.health -= damage;
    let pos = w.pos;
    h.draws.events.push(Event::Damage { worm: id, amount: damage, by: owner });
    if w.health < 0.0 {
        w.health = 0.0;
        kill_worm(h, id, owner);
        let d = &mut *h.draws;
        let s = d.rand.random_int(2);
        d.events.sound(SoundId::DEATH[s.clamp(0, 2) as usize], pos);
        for _ in 0..7 {
            // A gib draws its spin inside SpawnEntity, if the pool has room.
            let v = d.rand.random_vec() * 80.0;
            if d.entity(CosmeticKind::Gib, pos, v) {
                d.rand.random_num();
                d.rand.random_num();
            }
        }
        let amount = (50.0f32 * (BLOOD_AMOUNT as f32 / 100.0)) as i32;
        for _ in 0..amount {
            let sp = d.rand.random_num() * 100.0 + 50.0;
            blood_round(d, pos, sp, sp, [200, 128]);
        }
    }
    if damage > 0.0 {
        let amount = clamp(damage, 10.0, 100.0) * (BLOOD_AMOUNT as f32 / 100.0);
        let mut i: u16 = 0;
        while (i as f32) < amount {
            let sp = h.draws.rand.random_num() * (50.0 + damage * 2.0);
            blood_round(h.draws, pos, sp, sp * 4.0, [128, 200]);
            i = i.wrapping_add(1);
        }
    }
}

/// OLX `MIN` (`include/MathLib.h:66`): `b` if either is NaN.
pub(crate) fn min(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// OLX `CLAMP` (`include/MathLib.h:69-70`), NaN passes through.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// `GameServer::killWorm(victim, killer)`: unless the victim was already
/// out of lives, `CGameMode::Kill` credits the killer (a suicide, gated by
/// the score, or a kill; nobody for killer -1) and `CWorm::Kill(true)` kills
/// the victim. Position, velocity, weapons, input state and the rope stay as
/// they are (`damage-death-bonuses.md` §2.5). Game over is not ported, so
/// the `gameOver` early return is not either.
pub(crate) fn kill_worm(h: &mut Harm, victim: i32, killer: i32) {
    let s = h.settings;
    let Some(vi) = index(h.worms, victim) else { return };
    let ki = index(h.worms, killer);
    if h.worms[vi].lives < 0 && s.lives >= 0 {
        // "Cheat prevention": the worm is not killed.
        h.worms[vi].lives = WRM_OUT;
        return;
    }
    if let Some(k) = ki {
        let w = &mut h.worms[k];
        if k == vi {
            if counts(w, s) {
                w.suicides = w.suicides.wrapping_add(1);
            }
        } else {
            w.kills = w.kills.wrapping_add(1);
        }
    }
    h.draws.events.push(Event::Kill { victim, killer });
    // `CWorm::Kill(true)`.
    let w = &mut h.worms[vi];
    w.alive = false;
    w.time_of_death = h.now;
    w.can_respawn_now = false;
    w.respawn_requested = false;
    if counts(w, s) {
        w.deaths = w.deaths.wrapping_add(1);
    }
    if w.lives != WRM_UNLIM {
        w.lives = w.lives.wrapping_sub(1);
    }
}

/// `CWorm::think` for classic mods (`src/gusanos/base_worm.cpp:365-381`),
/// run by `gusLogicFrame` at the start of every frame: an alive worm with
/// health `<= 0` (only exactly 0 is possible) dies through `base_die`
/// (`:623-653`) with `fTimeofDeath = tLX->currentTime`, then `killWorm` with
/// killer -1, then the rope is cleared. Its Gusanos death particles, which
/// draw from the `FindSpot` stream, are not ported (spec §18 D9).
pub(crate) fn die_at_zero_health(h: &mut Harm, wall_clock: AbsTime) {
    for i in 0..h.worms.len() {
        let w = &mut h.worms[i];
        if w.alive && w.health <= 0.0 {
            w.alive = false;
            w.time_of_death = wall_clock;
            let id = w.id;
            kill_worm(h, id, -1);
            h.worms[i].rope.clear();
        }
    }
}

#[cfg(test)]
#[path = "damage_tests.rs"]
mod tests;
