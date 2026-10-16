//! The chaos personality (spec §5.9, §20 F10, notes §4.6): it fires every
//! weapon it gets for coverage. Its loadout changes on each spawn through
//! the mode (`olr_modes::RespawnLoadout::ChaosCycle`); the bot only presses
//! keys: it fires whatever is ready, at the nearest enemy in sight or at a
//! random angle, switches to a weapon it has not fired yet once the one it
//! holds has fired or reloads, and walks, jumps, ropes and digs at random.
//! It records the weapons it has fired (`WeaponFired` events; specials,
//! which report none, when their slot drains).

use alloc::vec::Vec;

use olr_math::AbsTime;
use olr_sim::{Dir, WeaponType, Worm};
use olr_terrain::PX_ROCK;

use crate::bot::{Act, Bot, Ctx};
use crate::brain::since;
use crate::budget::Budget;
use crate::probe::trace_line;

/// The random aim is drawn again this often.
const AIM_MS: u64 = 700;
/// Digging, as `AI_Carve`.
const CARVE_MS: u64 = 200;

/// The chaos bot's memory.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chaos {
    /// Which weapon ids this bot has fired.
    fired: Vec<bool>,
    walk: bool,
    next_walk: Option<AbsTime>,
    aim: (Dir, f32),
    next_aim: Option<AbsTime>,
    next_rope: Option<AbsTime>,
    release: Option<AbsTime>,
    last_carve: Option<AbsTime>,
}

impl Chaos {
    pub fn new(weapons: usize) -> Chaos {
        Chaos {
            fired: alloc::vec![false; weapons],
            walk: false,
            next_walk: None,
            aim: (Dir::Right, 0.0),
            next_aim: None,
            next_rope: None,
            release: None,
            last_carve: None,
        }
    }

    /// Weapon `weapon` was fired.
    pub fn record(&mut self, weapon: i32) {
        if let Some(f) = usize::try_from(weapon).ok().and_then(|i| self.fired.get_mut(i)) {
            *f = true;
        }
    }

    pub fn has_fired(&self, weapon: i32) -> bool {
        usize::try_from(weapon).ok().and_then(|i| self.fired.get(i)).is_some_and(|f| *f)
    }

    /// The weapon ids fired, in id order.
    pub fn fired(&self) -> impl Iterator<Item = i32> + '_ {
        self.fired.iter().enumerate().filter(|(_, f)| **f).map(|(i, _)| i as i32)
    }

    /// A new life: new timers, the record kept.
    pub fn respawned(&mut self) {
        let fired = core::mem::take(&mut self.fired);
        *self = Chaos { fired, ..Chaos::new(0) };
    }
}

impl Bot {
    /// One tick of the chaos personality.
    pub(crate) fn chaos_input(&mut self, c: &Ctx, b: &mut Budget) -> Act {
        let me = c.me;
        let now = c.now;
        let mut act = Act::default();
        if let Some(slot) = usize::try_from(me.current_weapon).ok().and_then(|i| me.slots.get(i))
            && self.arsenal.get(slot.weapon).is_some_and(|f| f.kind == WeaponType::Special)
            && (slot.last_fire > 0.0 || slot.charge < 1.0)
        {
            self.chaos.record(slot.weapon);
        }
        self.chaos_weapon(me);
        // Aim: the nearest enemy when no rock is in the way, else anywhere.
        let target = self.find_target(c).map(|w| w.pos);
        let seen =
            target.filter(|&p| trace_line(c.t, b, me.pos, p, 5).is_some_and(|(_, ty)| ty & PX_ROCK == 0));
        match seen {
            Some(p) => {
                self.set_aim(c, &mut act, p);
            }
            None => {
                if self.chaos.next_aim.is_none_or(|t| now >= t) {
                    let side = if self.rng.random_int(1) == 0 { Dir::Left } else { Dir::Right };
                    self.chaos.aim = (side, -90.0 + self.rng.random_pos_num() * 150.0);
                    self.chaos.next_aim = Some(AbsTime(now.0 + AIM_MS));
                }
                self.set_aim_at(c, &mut act, self.chaos.aim);
            }
        }
        act.shoot = true;
        // A random walk with jumps, rope throws and digging.
        if self.chaos.next_walk.is_none_or(|t| now >= t) {
            self.chaos.walk = self.rng.random_int(2) != 0;
            self.chaos.next_walk = Some(AbsTime(now.0 + 300 + self.rng.random_int(1200) as u64));
        }
        act.mv = self.chaos.walk;
        act.jump = self.rng.random_int(39) == 0;
        if self.chaos.next_rope.is_none_or(|t| now >= t) {
            act.rope_shoot = true;
            self.chaos.next_rope = Some(AbsTime(now.0 + 800 + self.rng.random_int(2000) as u64));
            self.chaos.release = Some(AbsTime(now.0 + 300 + self.rng.random_int(1200) as u64));
        }
        if me.rope.released && self.chaos.release.is_some_and(|t| now >= t) {
            act.rope_release = true;
            self.chaos.release = None;
        }
        if since(now, self.chaos.last_carve) > CARVE_MS {
            self.chaos.last_carve = Some(now);
            act.carve = true;
        }
        act
    }

    /// Keeps the weapon held while it is loaded and unfired (or nothing
    /// else unfired is loaded); else the next loaded unfired one, else the
    /// next loaded one, round the slots. With every slot reloading it
    /// stays (only the held slot recharges).
    fn chaos_weapon(&mut self, me: &Worm) {
        let n = me.slots.len();
        if n == 0 {
            return;
        }
        let ready = |i: usize| {
            me.slots[i].weapon >= 0
                && !me.slots[i].reloading
                && self.arsenal.get(me.slots[i].weapon).is_some()
        };
        let fresh = |i: usize| ready(i) && !self.chaos.has_fired(me.slots[i].weapon);
        let cur = self.weapon.min(n - 1);
        let others_fresh = (1..n).any(|k| fresh((cur + k) % n));
        if ready(cur) && (fresh(cur) || !others_fresh) {
            self.weapon = cur;
            return;
        }
        let next = (1..=n).map(|k| (cur + k) % n);
        if let Some(i) = next.clone().find(|&i| fresh(i)).or_else(|| next.clone().find(|&i| ready(i))) {
            self.weapon = i;
        }
    }
}
