//! What a bot knows of the mod's weapons, and OLX's weapon choices (notes
//! §1.7, §2.2): `AI_GetBestWeapon`'s buckets and change delay, the 100lt,
//! rifle and mortar tactics, `AI_ReloadWeapons` and
//! `AI_FindClearingWeapon`.

use alloc::string::String;
use alloc::vec::Vec;

use rl_data::mod_def::{ActionKind, SpecialKind};
use rl_math::Vec2;
use rl_sim::{SimMod, WeaponType, Worm};

use crate::bot::{Act, Bot, Ctx};
use crate::budget::Budget;
use crate::loadout::{GameType, LT100};
use crate::nav::worm_line_clear;
use crate::{BotRng, LoadoutPolicy, Personality};

/// One weapon as the bot sees it (`weapon_t` and its `Proj`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Facts {
    pub kind: WeaponType,
    /// `Proj.Proj->Hit.Type`, for projectile weapons.
    pub hit: Option<ActionKind>,
    /// `Proj.Speed` and `Proj.SpeedVar`.
    pub speed: f32,
    pub speed_var: f32,
    /// `ParentVelFactor` when `AddParentVel`, else 0: how much of the
    /// shot's velocity (the worm's, with `ProjRelativeVel`) a projectile
    /// carries (`rl-sim` `spawn.rs`).
    pub carry: f32,
    /// `UseCustomGravity ? Gravity : 100` of the projectile fired.
    pub gravity: f32,
    /// `Proj.Proj->UseCustomGravity`, `Gravity`.
    pub custom_gravity: Option<i32>,
    /// The gravity `AI_Shoot` reads along the spawn chain (notes §1.6.3).
    pub chain_gravity: f32,
    /// `Bm.Length`.
    pub beam_length: i32,
    pub recoil: i32,
    pub jetpack: bool,
    /// The name has "dirt", "napalm", "grenade", "nuke" or "mine".
    pub suspicious: bool,
    /// `PlyHit.Damage`.
    pub player_damage: i32,
}

impl Facts {
    fn of(m: &SimMod, id: usize) -> Facts {
        let w = &m.weapons()[id];
        let name: String = w.name.to_lowercase();
        let mut f = Facts {
            kind: w.kind,
            hit: None,
            speed: 0.0,
            speed_var: 0.0,
            carry: 0.0,
            gravity: 100.0,
            custom_gravity: None,
            chain_gravity: 100.0,
            beam_length: w.beam.map_or(0, |b| b.length),
            recoil: w.recoil,
            jetpack: w.kind == WeaponType::Special && w.special == SpecialKind::Jetpack,
            suspicious: ["dirt", "napalm", "grenade", "nuke", "mine"].iter().any(|s| name.contains(s)),
            player_damage: 0,
        };
        if w.kind == WeaponType::Projectile
            && let Some(info) = &w.spawn
            && let Some(p) = m.projectiles().get(info.proj)
        {
            f.hit = Some(p.def.hit.kind);
            f.speed = info.speed as f32;
            f.speed_var = info.speed_var;
            f.carry = if info.add_parent_vel { info.parent_vel_factor } else { 0.0 };
            f.custom_gravity = p.def.gravity;
            f.gravity = p.def.gravity.map_or(100.0, |g| g as f32);
            f.chain_gravity = chain_gravity(m, info.proj);
            f.player_damage = p.def.player_hit.damage;
        }
        f
    }

    /// A projectile whose terrain hit makes dirt (`PJ_DIRT`, `PJ_GREENDIRT`).
    pub fn dirty(&self) -> bool {
        matches!(self.hit, Some(ActionKind::Dirt | ActionKind::GreenDirt))
    }

    pub fn projectile(&self) -> bool {
        self.kind == WeaponType::Projectile && self.hit.is_some()
    }
}

/// The gravity `AI_Shoot` uses (`src/common/CWormBot.cpp:2176-2200`): the
/// largest along the projectile and the children its `GeneralSpawnInfo`
/// spawns, until one spawns on a timer of at least 0.5 s or on a hit
/// (non-custom gravity counts as 100), before `ProjGravityFactor`.
fn chain_gravity(m: &SimMod, first: usize) -> f32 {
    let mut g = m.projectiles().get(first).and_then(|p| p.def.gravity).map_or(100.0, |g| g as f32);
    let mut at = Some(first);
    // A spawn chain can loop; OLX's would hang. 64 links is plenty.
    for _ in 0..64 {
        let Some(p) = at.and_then(|i| m.projectiles().get(i)) else { break };
        match p.def.gravity {
            Some(own) => g = g.max(own as f32),
            None => g = g.max(100.0),
        }
        let d = &p.def;
        let timer = d.timer.action.as_ref().is_some_and(|a| a.spawns_projectiles);
        if timer {
            if d.timer.time >= 0.5 {
                break;
            }
        } else if d.hit.spawns_projectiles || d.player_hit.spawns_projectiles {
            break;
        }
        at = p.event_spawn.as_ref().map(|s| s.proj);
    }
    g
}

/// Every weapon of the mod, by id.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Arsenal {
    facts: Vec<Facts>,
}

impl Arsenal {
    pub fn new(m: &SimMod) -> Self {
        Arsenal { facts: (0..m.weapons().len()).map(|i| Facts::of(m, i)).collect() }
    }

    /// Weapon `id`'s facts (`None` for an empty slot or a bad id).
    pub fn get(&self, id: i32) -> Option<&Facts> {
        usize::try_from(id).ok().and_then(|i| self.facts.get(i))
    }
}

/// The tactics a bot plays (`iAiGameType`, `initWeaponSelection`): the
/// presets of [`crate::olx_bot_weapons`] recognised in the worm's slots.
pub(crate) fn game_type(m: &SimMod, me: &Worm, personality: Personality, policy: LoadoutPolicy) -> GameType {
    if personality == Personality::Chaos || policy == LoadoutPolicy::Random || me.slots.len() < 5 {
        return GameType::Other;
    }
    let name = |i: usize| m.weapon(me.slots[i].weapon).map(|w| w.name.as_str());
    let all = |want: &str| (0..5).all(|i| name(i).is_some_and(|n| n.eq_ignore_ascii_case(want)));
    if all("Rifle") {
        GameType::Rifles
    } else if all("Mortar Launcher") {
        GameType::Mortars
    } else if (0..5).all(|i| name(i).is_some_and(|n| n.eq_ignore_ascii_case(LT100[i]))) {
        GameType::Lt100
    } else {
        GameType::Other
    }
}

/// `slots` tries of `GetRandomInt(slots - 1)` for a loaded slot.
fn random_tries(rng: &mut BotRng, me: &Worm) -> Option<usize> {
    let n = me.slots.len();
    (0..n).find_map(|_| {
        let i = rng.random_int(n as i32 - 1) as usize;
        loaded(me, i).then_some(i)
    })
}

/// Fire slot `slot` standing still (`bMove = false`, "avoid suicides").
fn stand(act: &mut Act, slot: usize) -> Option<Result<usize, ()>> {
    act.mv = false;
    Some(Ok(slot))
}

/// Whether slot `i` holds a weapon and is loaded (OLX's "loaded" is
/// `!Reloading`; a part charge counts).
fn loaded(me: &Worm, i: usize) -> bool {
    me.slots.get(i).is_some_and(|s| !s.reloading && s.weapon >= 0)
}

impl Bot {
    /// `AI_ReloadWeapons` (`CWormBot.cpp:1594-1603`): the lowest slot that
    /// reloads becomes the current one (only the current slot recharges).
    pub(crate) fn reload_weapons(&mut self, me: &Worm) {
        if let Some(i) = me.slots.iter().position(|s| s.reloading) {
            self.weapon = i;
        }
    }

    /// `AI_FindClearingWeapon` (`CWormBot.cpp:1776-1816`): a loaded
    /// projectile weapon that neither makes dirt nor bounces nor sounds
    /// explosive, else any beam.
    pub(crate) fn clearing_weapon(&self, c: &Ctx) -> Option<usize> {
        let can_ignore_self_hit = !c.s.self_injure;
        if !can_ignore_self_hit && self.game_type == GameType::Mortars {
            return None;
        }
        let facts = |i: usize| self.arsenal.get(c.me.slots[i].weapon);
        let projectile = (0..c.me.slots.len()).find(|&i| {
            let Some(f) = facts(i).filter(|f| f.projectile()) else { return false };
            if !can_ignore_self_hit && f.speed < 100.0 && f.custom_gravity.is_none_or(|g| g > 30) {
                return false;
            }
            !f.suspicious
                && !matches!(f.hit, Some(ActionKind::Dirt | ActionKind::GreenDirt | ActionKind::Bounce))
                && !c.me.slots[i].reloading
        });
        projectile
            .or_else(|| (0..c.me.slots.len()).find(|&i| facts(i).is_some_and(|f| f.kind == WeaponType::Beam)))
    }

    /// `AI_GetBestWeapon` (`CWormBot.cpp:2332-2633`) for a target `d` px
    /// away, `direct` when no rock is in the way, `trace` the fraction of
    /// the way `traceWeaponLine` got. Changes weapon at most every
    /// `weapon_change_delay`, else keeps the current one. The 100lt tactics
    /// may also stop the bot (`bMove`) or make it jump. The Cannon's rope
    /// test is fixed (notes §4.2: OLX tests `isReleased` where it means
    /// `isAttached`).
    pub(crate) fn best_weapon(
        &mut self,
        c: &Ctx,
        b: &mut Budget,
        d: f32,
        direct: bool,
        trace: f32,
        act: &mut Act,
    ) -> Option<usize> {
        let delay = self.params.weapon_change_delay.0;
        if self.mind.last_weapon_change.is_none_or(|t| c.now.0.saturating_sub(t.0) > delay) {
            self.mind.last_weapon_change = Some(c.now);
        } else {
            return Some(self.weapon);
        }
        let me = c.me;
        if matches!(self.game_type, GameType::Rifles | GameType::Mortars) {
            return (0..5).find(|&i| loaded(me, i));
        }
        let trg = self.target_pos(c);
        if self.game_type == GameType::Lt100
            && let Some(w) = self.lt100(c, b, trg, direct, act)
        {
            return w.ok();
        }
        let facts = |i: usize| self.arsenal.get(me.slots[i].weapon);
        let n = me.slots.len();
        let first =
            |pred: &dyn Fn(&Facts) -> bool| (0..n).find(|&i| loaded(me, i) && facts(i).is_some_and(pred));
        let beam = |f: &Facts| f.kind == WeaponType::Beam;
        let clean = |f: &Facts| f.projectile() && !f.dirty();
        // 1. The target at the bottom of the map: lob something explosive.
        if trg.y > c.t.height() as f32 - 50.0
            && d < 200.0
            && let Some(i) = first(&|f| f.projectile() && f.hit == Some(ActionKind::Explode))
        {
            return Some(i);
        }
        // 2-3. Close (< 100, then < 150, the same rule) and in sight: a
        // beam, else a projectile that makes no dirt.
        if d < 150.0
            && direct
            && let Some(i) = first(&beam).or_else(|| first(&clean))
        {
            return Some(i);
        }
        // 4. In sight: a projectile, a beam, else random tries.
        if direct {
            if let Some(i) = first(&clean).or_else(|| first(&beam)) {
                return Some(i);
            }
            if let Some(i) = random_tries(&mut self.rng, me) {
                return Some(i);
            }
        }
        // 5. Out of sight, far, healthy, half way traced, well above the
        // target: something that explodes or bounces.
        if d > 190.0
            && me.health > 25.0
            && trace > 0.5
            && trg.y - 20.0 > me.pos.y
            && let Some(i) =
                first(&|f| f.projectile() && matches!(f.hit, Some(ActionKind::Explode | ActionKind::Bounce)))
        {
            return Some(i);
        }
        // 6. The last resort: a beam, random tries, the first loaded.
        first(&beam).or_else(|| random_tries(&mut self.rng, me)).or_else(|| (0..n).find(|&i| loaded(me, i)))
    }

    /// The 100lt tactics (`CWormBot.cpp:2361-2484`): `Some(Ok(slot))` to
    /// fire, `Some(Err(()))` for none (-1), `None` to fall through to the
    /// generic rules. Slots: 0 Super Shotgun, 1 Napalm, 2 Cannon,
    /// 3 Doomsday, 4 Chaingun.
    fn lt100(
        &mut self,
        c: &Ctx,
        b: &mut Budget,
        trg: Vec2,
        direct: bool,
        act: &mut Act,
    ) -> Option<Result<usize, ()>> {
        let me = c.me;
        let ready = |i: usize| loaded(me, i);
        let attached = me.rope.attached;
        // Above the target and close: Napalm, if the target stands.
        if me.pos.y <= trg.y && (me.pos - trg).length2() < 10000.0 && ready(1) {
            let target_on_ground = self.target_worm(c).is_some_and(|w| w.on_ground);
            if worm_line_clear(c.t, b, trg, me.pos) == Some(true) && target_on_ground {
                return Some(Ok(1));
            }
        }
        let d = (me.pos - trg).length();
        if d < 50.0 {
            if !direct {
                act.jump = true;
                return Some(Err(()));
            }
            if ready(0) {
                return Some(Ok(0));
            }
            if ready(4) {
                return Some(Ok(4));
            }
            if ready(3) {
                return Some(Ok(3));
            }
            if ready(2) && !attached {
                return stand(act, 2);
            }
        }
        if d > 50.0 && d <= 300.0 && direct {
            if ready(4) {
                return Some(Ok(4));
            }
            if ready(2) && !attached {
                return stand(act, 2);
            }
            if ready(0) {
                return Some(Ok(0));
            }
            if ready(3) && !attached {
                return stand(act, 3);
            }
        }
        if d > 300.0 && direct {
            if ready(3) && !attached {
                return stand(act, 3);
            }
            if ready(0) {
                return Some(Ok(0));
            }
            if ready(4) {
                return Some(Ok(4));
            }
            if ready(2) && !attached {
                return stand(act, 2);
            }
        }
        None
    }
}
