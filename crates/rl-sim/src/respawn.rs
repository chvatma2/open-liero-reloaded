//! Respawning: the server's respawn loop (`GameServer::SimulateGame`,
//! `src/server/CServer_Game.cpp:200-225`), `GameServer::SpawnWorm` (`:38-104`),
//! `CMap::FindSpot` (`src/common/CMap.cpp:2381-2396`) and `CWorm::Spawn`
//! (`src/common/CWorm.cpp:591-651`).

use rl_math::{AbsTime, Mt19937, Vec2};
use rl_terrain::{PX_ROCK, Stamp, Terrain};

use crate::SimSettings;
use crate::damage::{WRM_OUT, WRM_UNLIM};
use crate::effects::carve_px;
use crate::events::{CosmeticKind, Event, TerrainOp};
use crate::input::Latches;
use crate::projectile::{c_i64, c_int, c_u64};
use crate::rng::Draws;
use crate::worm::{Dir, Worm, WormState};

/// OLX `SPAWN_HOLESIZE` (`include/Consts.h:36`): the size-4 (largest) hole.
pub(crate) const SPAWN_HOLE: usize = 4;

/// `rnd()` (`src/util/math_func.cpp:3-4`): boost's `uniform_01<mt19937>`,
/// `double(mt()) * (1.0 / 2^32)`, which is always below 1.
fn rnd(mt: &mut Mt19937) -> f64 {
    mt.next_u32() as f64 * (1.0 / 4294967296.0)
}

/// `CMap::IsGoodSpawnPoint` (`src/game/CMap.h:458`): no rock (outside the
/// map counts as rock) in the 6 × 6 square from `(x - 3, y - 3)` to
/// `(x + 2, y + 2)`, at the position truncated to `long`.
fn is_good_spawn_point(t: &Terrain, p: Vec2) -> bool {
    let (x, y) = (c_i64(p.x), c_i64(p.y));
    (y.saturating_sub(3)..y.saturating_add(3))
        .all(|yy| (x.saturating_sub(3)..x.saturating_add(3)).all(|xx| t.flags(xx, yy) & PX_ROCK == 0))
}

/// `CMap::FindSpot`: up to 1000 random positions, two `rnd()` draws each,
/// until one has no rock around it; after 1000 failures, the last one.
pub(crate) fn find_spot(t: &Terrain, mt: &mut Mt19937) -> Vec2 {
    let (w, h) = (t.width() as f64, t.height() as f64);
    let mut tries = 1000;
    loop {
        let x = (rnd(mt) * w) as f32;
        let y = (rnd(mt) * h) as f32;
        let pos = Vec2::new(x, y);
        tries -= 1;
        if tries == 0 || is_good_spawn_point(t, pos) {
            return pos;
        }
    }
}

/// `TimeDiff(float seconds)` (`include/olx-types.h:33`): `(Uint64)(s * 1000.0f)`.
pub(crate) fn seconds(s: f32) -> u64 {
    c_u64(s * 1000.0)
}

impl Worm {
    /// `CWorm::Spawn` at `pos`, except the carving: full health, at rest,
    /// aiming level to the right, rope cleared, input cleared
    /// (`clearInput`), and every weapon slot full and ready with its weapon
    /// kept. The position recorder is cleared to `pos` but keeps its old
    /// entries (`clear(vPos)`, spec §18 D7). Lives, kills, the dirt count,
    /// `fLastBlood` and the factors stay.
    pub(crate) fn respawn(&mut self, pos: Vec2, now: AbsTime) {
        self.pos = pos;
        self.vel = Vec2::ZERO;
        self.health = 100.0;
        self.spawned_once = true;
        self.can_respawn_now = false;
        self.respawn_requested = false;
        self.angle = 0.0;
        self.angle_speed = 0.0;
        self.face = Dir::Right;
        self.move_dir = Dir::Right;
        self.last_input_time = now;
        self.pos_recorder.clear_to(pos);
        self.rope.clear();
        self.frame = 0.0;
        self.on_ground = false;
        for s in &mut self.slots {
            s.charge = 1.0;
            s.reloading = false;
            s.last_fire = 0.0;
        }
        self.state = WormState::default();
        self.alive = true;
    }
}

/// What the server's respawn loop reads or changes besides the worms.
pub(crate) struct Spawner<'a> {
    pub settings: &'a SimSettings,
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub draws: &'a mut Draws,
    /// `GetPhysicsTime()`.
    pub now: AbsTime,
    /// `tLX->currentTime`, pinned to `now + K` (spec §18 D2).
    pub wall_clock: AbsTime,
}

/// The respawn loop of `GameServer::SimulateGame`, once per frame after the
/// physics: a dead worm with lives left is due once `RespawnTime` has passed
/// (strictly), and the server spawns it if `MaxRespawnTime` (from 0 on) has
/// passed too, or if it asked to. Otherwise the local client may respawn it
/// itself, so the server only allows it (`bCanRespawnNow`) and waits for the
/// worm's jump key. Game over is not ported (`docs/porting/STATUS.md`).
///
/// OLX also skips a dead worm whose `bWeaponsReady` is false
/// (`CServer_Game.cpp:204`): one still choosing its weapons, which
/// `GameServer::PrepareWorm` (`CServer.cpp:403-424`, random weapons by
/// default) or the weapon selection menu (`CWormHuman.cpp:714-748`) settles.
/// `rl-sim` has no weapon selection and no such flag: every worm counts as
/// ready (its weapons come from `World::set_loadout` at setup), so the gate
/// never holds a respawn back.
pub(crate) fn simulate_respawns(worms: &mut [Worm], latches: &mut [Latches], sp: &mut Spawner) {
    let s = sp.settings;
    for (w, keys) in worms.iter_mut().zip(latches.iter_mut()) {
        if w.alive || w.lives == WRM_OUT {
            continue;
        }
        let tod = w.time_of_death.0;
        let current = sp.wall_clock.0;
        if w.spawned_once && current <= tod.wrapping_add(seconds(s.respawn_time)) {
            continue;
        }
        // "max respawn-time reached" or "respawn requested".
        let forced = s.max_respawn_time >= 0.0 && current > tod.wrapping_add(seconds(s.max_respawn_time));
        if forced || w.respawn_requested {
            spawn_worm(w, keys, sp);
        } else {
            w.can_respawn_now = true;
        }
    }
}

/// `GameServer::SpawnWorm` in deathmatch: `FindSpot`, `CWorm::Spawn` with its
/// size-4 hole (not counted as dirt), lives back from `WRM_OUT`, and
/// `EmptyWeaponsOnRespawn` (`GameServer::SendEmptyWeaponsOnRespawn`,
/// `src/server/CServer_Send.cpp:1066-1072`).
fn spawn_worm(w: &mut Worm, keys: &mut Latches, sp: &mut Spawner) {
    let mut pos = find_spot(sp.terrain, &mut sp.draws.mt);
    if pos.x == -1.0 && pos.y == -1.0 {
        pos = find_spot(sp.terrain, &mut sp.draws.mt);
    }
    w.respawn(pos, sp.now);
    keys.reset();
    let ev = &mut sp.draws.events;
    ev.push(Event::Spawned { worm: w.id });
    carve_px(sp.terrain, sp.holes, ev, TerrainOp::Carve, SPAWN_HOLE, c_int(pos.x), c_int(pos.y));
    // The local client's `ParseSpawnWorm` (`src/client/CClient_Parse.cpp:1047-1053`)
    // shows `ENT_SPAWN`; a dedicated server has no viewport, so it takes no slot.
    ev.cosmetic(CosmeticKind::Spawn, pos, Vec2::ZERO);
    if w.lives == WRM_OUT {
        w.lives = if sp.settings.lives < 0 { WRM_UNLIM } else { 0 };
    }
    if sp.settings.empty_weapons_on_respawn {
        for s in &mut w.slots {
            s.charge = 0.0;
            s.reloading = true;
        }
    }
}
