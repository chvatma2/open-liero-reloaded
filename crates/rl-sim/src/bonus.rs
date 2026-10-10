//! Bonuses (`docs/porting/olx-notes/damage-death-bonuses.md` §4). OLX keeps
//! two arrays of [`MAX_BONUSES`] (`include/CBonus.h`):
//! - the server's (`cServer->cBonuses`), which spawns a bonus every
//!   `BonusFrequency` at a `FindSpot` position and frees a slot only when its
//!   `BonusLife` ends, even after a worm picked the bonus up;
//! - the client's (`cClient->cBonuses`), whose bonuses fall, are picked up
//!   and explode (the explosion chain is in `effects.rs`).
//!
//! They talk through reliable messages, handed over between frames (spec §18
//! D10): `S2C_SPAWNBONUS` and `S2C_DESTROYBONUS` to the client, then
//! `C2S_GRABBONUS` to the server, whose `S2C_DESTROYBONUS` reply goes out
//! after the next frame.

use alloc::vec::Vec;

use rl_math::{AbsTime, GlibcRand, Vec2};
use rl_terrain::{PX_DIRT, PX_ROCK, Stamp, Terrain};

use crate::SimSettings;
use crate::damage::{Harm, injure_worm, min};
use crate::effects::carve_at;
use crate::projectile::{c_int, c_u64};
use crate::respawn::{SPAWN_HOLE, find_spot, seconds};
use crate::rng::Draws;
use crate::worm::Worm;

/// OLX `MAX_BONUSES` (`include/CBonus.h:23`).
pub const MAX_BONUSES: usize = 32;
/// OLX `BONUS_LIFETIME` (`include/CBonus.h:25`): the client's bonus blinks
/// for its last 3 s (it is never removed for it).
const BONUS_LIFETIME: i32 = 60;

/// A bonus's kind (`BNS_WEAPON` 0, `BNS_HEALTH` 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum BonusKind {
    #[default]
    Weapon = 0,
    Health = 1,
}

/// What [`crate::World::place_bonus`] places: health, or a weapon by id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BonusItem {
    Health,
    Weapon(i32),
}

/// One slot of a bonus array (`CBonus`). An unused slot keeps its old values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bonus {
    pub used: bool,
    pub kind: BonusKind,
    /// `iWeapon`: the weapon id. A client's health bonus has 0, a server's
    /// placed health bonus -1, a server's spawned one a random weapon.
    pub weapon: i32,
    pub pos: Vec2,
    pub vel: Vec2,
    /// `fLife`: seconds simulated (client only).
    pub life: f32,
    /// `fSpawnTime`: `tLX->currentTime` at the spawn.
    pub spawn_time: AbsTime,
    /// `fFlashTime`: the blinking of an old bonus, only for rendering.
    pub flash_time: f32,
}

impl Bonus {
    /// `CBonus::Spawn` (`src/common/CBonus.cpp:31-44`).
    fn spawn(&mut self, pos: Vec2, kind: BonusKind, weapon: i32, now: AbsTime) {
        *self = Bonus {
            used: true,
            kind,
            weapon,
            pos,
            vel: Vec2::ZERO,
            life: 0.0,
            spawn_time: now,
            flash_time: 0.0,
        };
    }
}

/// A message from the server to the local client, as its bytes carry it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ToClient {
    /// `S2C_SPAWNBONUS` (`CServer_Game.cpp:303-313`): the weapon's low byte
    /// (sent for weapon bonuses only), the slot, and the position truncated to
    /// `int`, of which `writeInt(.., 2)` sends the low 16 bits.
    Spawn { kind: BonusKind, weapon: u8, id: u8, x: u16, y: u16 },
    /// `S2C_DESTROYBONUS`.
    Destroy(u8),
}

/// `C2S_GRABBONUS` (`CClientNetEngine::SendGrabBonus`, `src/client/CClient_Send.cpp:306-315`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grab {
    id: u8,
    worm: u8,
    /// `getCurrentWeapon()` at the pickup.
    current: u8,
}

/// Both bonus arrays and the messages between them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Bonuses {
    pub client: [Bonus; MAX_BONUSES],
    pub server: [Bonus; MAX_BONUSES],
    /// `GameServer::fLastBonusTime`.
    last_spawn: AbsTime,
    /// `goodWpns`: the weapon ids a spawned bonus may hold.
    weapons: Vec<i32>,
    to_client: Vec<ToClient>,
    to_server: Vec<Grab>,
    /// [`crate::World::place_bonus`] calls, for the next tick.
    placements: Vec<(i32, i32, BonusItem)>,
}

/// What the client needs to parse `S2C_SPAWNBONUS`.
pub(crate) struct ClientCtx<'a> {
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub draws: &'a mut Draws,
    pub num_weapons: i32,
    /// `tLX->currentTime`, pinned (spec §18 D2).
    pub wall_clock: AbsTime,
}

impl Bonuses {
    /// Every weapon allowed; the spawn timer starts at `wall_clock` (OLX:
    /// `GameServer::PrepareGame`, `CServer.cpp:383`; the oracle resets it when
    /// a recording starts).
    pub fn new(num_weapons: usize, wall_clock: AbsTime) -> Self {
        Self {
            client: [Bonus::default(); MAX_BONUSES],
            server: [Bonus::default(); MAX_BONUSES],
            last_spawn: wall_clock,
            weapons: (0..num_weapons as i32).collect(),
            // At most one expiry per slot, one spawn and one grab reply per slot a frame.
            to_client: Vec::with_capacity(2 * MAX_BONUSES + 1),
            // A client slot is picked up at most once a frame.
            to_server: Vec::with_capacity(MAX_BONUSES),
            placements: Vec::new(),
        }
    }

    pub fn set_weapons(&mut self, ids: impl Iterator<Item = i32>) {
        self.weapons.clear();
        self.weapons.extend(ids);
    }

    pub fn place(&mut self, x: i32, y: i32, item: BonusItem) {
        self.placements.push((x, y, item));
    }

    /// The oracle's `placeBonus` (`OracleHooks.cpp:286-301`) at the start of
    /// the frame: `GameServer::SpawnBonus` with the position and kind given
    /// (no `FindSpot`, no `rand()`; a health bonus's weapon is -1), whose
    /// packet the client parses at once.
    pub fn apply_placements(&mut self, ctx: &mut ClientCtx) {
        for k in 0..self.placements.len() {
            let (x, y, item) = self.placements[k];
            let (kind, weapon) = match item {
                BonusItem::Health => (BonusKind::Health, -1),
                // `CBonus::Spawn` reads the weapon's name: OLX would read out of bounds.
                BonusItem::Weapon(w) if (0..ctx.num_weapons).contains(&w) => (BonusKind::Weapon, w),
                BonusItem::Weapon(_) => continue,
            };
            let Some(spot) = self.server.iter().position(|b| !b.used) else { continue };
            let msg = self.spawn_server(spot, Vec2::new(x as f32, y as f32), kind, weapon, ctx.wall_clock);
            self.parse(msg, ctx);
        }
        self.placements.clear();
    }

    /// `GameServer::SpawnBonus` from `spot` on (`CServer_Game.cpp:296-313`).
    fn spawn_server(
        &mut self,
        spot: usize,
        pos: Vec2,
        kind: BonusKind,
        weapon: i32,
        now: AbsTime,
    ) -> ToClient {
        self.server[spot].spawn(pos, kind, weapon, now);
        ToClient::Spawn {
            kind,
            weapon: weapon as u8,
            id: spot as u8,
            x: c_int(pos.x) as u16,
            y: c_int(pos.y) as u16,
        }
    }

    /// The server's messages, parsed by the client before the frame
    /// (`CClientNetEngine::ParsePacket`).
    pub fn deliver_to_client(&mut self, ctx: &mut ClientCtx) {
        for k in 0..self.to_client.len() {
            let msg = self.to_client[k];
            self.parse(msg, ctx);
        }
        self.to_client.clear();
    }

    /// `ParseSpawnBonus` (`src/client/CClient_Parse.cpp:1464-1516`) and
    /// `ParseDestroyBonus` (`:1977-1990`). The client's bonus has the integer
    /// position, a spawn hole is carved (not counted as dirt) and `ENT_SPAWN`
    /// takes an entity slot.
    fn parse(&mut self, msg: ToClient, ctx: &mut ClientCtx) {
        match msg {
            ToClient::Destroy(id) => {
                if let Some(b) = self.client.get_mut(id as usize) {
                    b.used = false;
                }
            }
            ToClient::Spawn { kind, weapon, id, x, y } => {
                let wpn = if kind == BonusKind::Weapon { weapon as i32 } else { 0 };
                let (x, y) = (x as i32, y as i32);
                let (w, h) = (ctx.terrain.width() as i32, ctx.terrain.height() as i32);
                if id as usize >= MAX_BONUSES || x > w || y > h {
                    return;
                }
                if kind == BonusKind::Weapon && wpn >= ctx.num_weapons {
                    return;
                }
                let p = Vec2::new(x as f32, y as f32);
                self.client[id as usize].spawn(p, kind, wpn, ctx.wall_clock);
                carve_at(ctx.terrain, ctx.holes, SPAWN_HOLE as i32, p);
                ctx.draws.spawn_entity();
            }
        }
    }

    /// The client's messages, parsed by the server after the shots
    /// (`CServerNetEngine::ParseGrabBonus`, `src/server/CServer_Parse.cpp:834-903`).
    /// The server sets the grabbed weapon a second time, on the slot that was
    /// current at the pickup: full and not reloading. It keeps its own slot
    /// used and tells the client to destroy the bonus (a no-op by then).
    /// `WormShootEnd` for the replaced weapon only acts on a `FinalProj`,
    /// which LX56 mods do not have.
    pub fn deliver_to_server(&mut self, worms: &mut [Worm], num_weapons: i32) {
        for k in 0..self.to_server.len() {
            let g = self.to_server[k];
            let Some(w) = worms.iter_mut().find(|w| w.id == g.worm as i32) else { continue };
            let Some(b) = self.server.get(g.id as usize) else { continue };
            if !b.used {
                continue;
            }
            if b.kind == BonusKind::Weapon
                && let Some(slot) = w.slots.get_mut(g.current as usize)
            {
                if (0..num_weapons).contains(&b.weapon) {
                    slot.weapon = b.weapon;
                    slot.charge = 1.0;
                    slot.reloading = false;
                } else {
                    slot.weapon = -1;
                }
            }
            self.to_client.push(ToClient::Destroy(g.id));
        }
        self.to_server.clear();
    }

    /// The bonus check of `CClient::Simulation` (`src/client/CClient_Game.cpp:128-151`)
    /// for worm `i`, alive, after its physics: every used slot within 7 px on
    /// both axes (`CWorm::CheckBonusCollision`) that `GiveBonus` takes is
    /// freed and reported to the server (`CClient::DestroyBonus`, `:736-745`).
    pub fn pick_up(&mut self, h: &mut Harm, i: usize, num_weapons: i32) {
        for n in 0..MAX_BONUSES {
            let b = self.client[n];
            if !b.used {
                continue;
            }
            let d = h.worms[i].pos - b.pos;
            if !(libm::fabsf(d.x) < 7.0 && libm::fabsf(d.y) < 7.0) {
                continue;
            }
            if give_bonus(h, i, &b, num_weapons) {
                self.client[n].used = false;
                let w = &h.worms[i];
                let current = w.current_slot_index() as u8;
                self.to_server.push(Grab { id: n as u8, worm: w.id as u8, current });
            }
        }
    }

    /// `PhysicsEngine::simulateBonuses` (`src/common/PhysicsLX56.cpp:699-715`),
    /// after the projectiles in every sub-step, with `Bonuses` on.
    pub fn simulate_client(&mut self, terrain: &Terrain, dt: f32) {
        for b in self.client.iter_mut().filter(|b| b.used) {
            simulate_bonus(b, terrain, dt);
        }
    }

    /// The bonus part of `GameServer::SimulateGame` (`src/server/CServer_Game.cpp:227-250`),
    /// after the respawn loop: with `Bonuses` on, a slot older than `BonusLife`
    /// (strictly) is freed, then a bonus spawns once `BonusFrequency` has passed
    /// since the last spawn (strictly). Game over, which stops spawning, is not ported.
    pub fn simulate_server(
        &mut self,
        s: &SimSettings,
        terrain: &Terrain,
        draws: &mut Draws,
        wall_clock: AbsTime,
    ) {
        if s.bonuses {
            for i in 0..MAX_BONUSES {
                let b = &mut self.server[i];
                // `(currentTime - spawnTime) > TimeDiff(BonusLife)`: the subtraction wraps (no assert).
                if b.used && wall_clock.0.wrapping_sub(b.spawn_time.0) > seconds(s.bonus_life) {
                    b.used = false;
                    self.to_client.push(ToClient::Destroy(i as u8));
                }
            }
        }
        if wall_clock.0.wrapping_sub(self.last_spawn.0) > seconds(s.bonus_frequency) && s.bonuses {
            self.spawn_natural(s, terrain, draws, wall_clock);
            self.last_spawn = wall_clock;
        }
    }

    /// `GameServer::SpawnBonus()` (`CServer_Game.cpp:266-316`): `FindSpot` and
    /// the kind's `rand()` come first, so they are spent even when no slot is
    /// free; the weapon is drawn for a health bonus too, unless no weapon is
    /// allowed. The server carves no hole.
    fn spawn_natural(&mut self, s: &SimSettings, terrain: &Terrain, draws: &mut Draws, now: AbsTime) {
        let pos = find_spot(terrain, &mut draws.mt);
        let r = draws.rand.random_int(999);
        let kind = if r as f32 >= s.bonus_health_to_weapon_chance * 1000.0 {
            BonusKind::Health
        } else {
            BonusKind::Weapon
        };
        let Some(spot) = self.server.iter().position(|b| !b.used) else { return };
        let weapon = if self.weapons.is_empty() {
            -1
        } else {
            self.weapons[random_long(&mut draws.rand, self.weapons.len() as u64 - 1) as usize]
        };
        if kind == BonusKind::Weapon && weapon < 0 {
            return;
        }
        let msg = self.spawn_server(spot, pos, kind, weapon, now);
        self.to_client.push(msg);
    }
}

/// `GetRandomLong` (`src/common/MathLib.cpp:78-81`), which `randomChoiceFrom`
/// (`include/Utils.h:38-43`) uses: `MIN((unsigned long)(GetRandomPosNum() * (float(max) + 1)), max)`.
fn random_long(rand: &mut GlibcRand, max: u64) -> u64 {
    let f = rand.random_pos_num() * (max as f32 + 1.0);
    c_u64(f).min(max)
}

/// `CWorm::GiveBonus` (`src/common/CWorm.cpp:1465-1506`) for worm `i`. A
/// weapon bonus replaces the current slot's weapon, full and not reloading
/// (`LastFire` stays), and is taken even with an invalid weapon. A health
/// bonus is taken below 100 health: `GetRandomInt(40) + 10`, at most up to
/// 100, healed through `InjureWorm` with the worm as the owner.
fn give_bonus(h: &mut Harm, i: usize, b: &Bonus, num_weapons: i32) -> bool {
    let w = &mut h.worms[i];
    if w.slots.is_empty() {
        return false;
    }
    match b.kind {
        BonusKind::Weapon => {
            if (0..num_weapons).contains(&b.weapon) {
                let slot = w.current_slot_mut();
                slot.weapon = b.weapon;
                slot.charge = 1.0;
                slot.reloading = false;
            }
            true
        }
        BonusKind::Health => {
            if w.health >= 100.0 {
                return false;
            }
            let id = w.id;
            let health = (h.draws.rand.random_int(40) + 10) as f32;
            let health = min(health, 100.0 - h.worms[i].health);
            injure_worm(h, id, -health, id);
            true
        }
    }
}

/// `simulateBonus` (`PhysicsLX56.cpp:624-697`, `colideBonus` `:619-622`):
/// gravity 80, no sideways motion. The 5 × 5 box around the bonus is
/// scanned row by row from the top; at the first dirt or rock pixel, or the
/// bottom or right map edge, the bonus stops 2 px above that row. So a bonus
/// with solid ground in its top row moves up 2 px a sub-step.
fn simulate_bonus(b: &mut Bonus, t: &Terrain, dt: f32) {
    b.life += dt;
    if b.life > (BONUS_LIFETIME - 3) as f32 {
        b.flash_time += dt;
        if b.flash_time > 0.3 {
            b.flash_time = 0.0;
        }
    }
    b.vel.y += 80.0 * dt;
    b.pos += b.vel * dt;
    let (px, py) = (c_int(b.pos.x) as i64, c_int(b.pos.y) as i64);
    let (mw, mh) = (t.width() as i64, t.height() as i64);
    let collide = |b: &mut Bonus, y: i64| {
        b.vel = Vec2::ZERO;
        b.pos.y = y as f32 - 2.0;
    };
    for y in py - 2..=py + 2 {
        if y < 0 {
            continue;
        }
        if y >= mh {
            return collide(b, y);
        }
        for x in px - 2..=px + 2 {
            if x < 0 {
                continue;
            }
            if x >= mw || t.flags(x, y) & (PX_DIRT | PX_ROCK) != 0 {
                return collide(b, y);
            }
        }
    }
}

#[cfg(test)]
#[path = "bonus_tests.rs"]
mod tests;
