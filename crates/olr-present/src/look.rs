//! The look's dynamic lights (plan 15a Task 15; spec §Mod packs, §The
//! engine), glowing discs in the glow light since plan 15c (radii from
//! `engine::disc_radius` and `engine::explosion_radius`), the engine's
//! first so the disc cap never drops them (final review, Important 3): the
//! muzzle lights (start order), the explosions' lights (effect slot order),
//! the projectiles' lights (slot order) and their end flashes (start
//! order); then the skins' lights (issue #12, worm order), after the mods'
//! so the cap never starves those either.
//!
//! End flashes and muzzle lights start from the scene's per-tick records
//! (`Scene::ended_projectiles`, `Scene::shots`), so they never depend on
//! the frame rate (spec §Fair play), and run their keys from the tick they
//! started. A re-simulated tick reports them again: each starts once.

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

use olr_fx::{Fx, Kind};
use olr_look::{Light, LookFrame, MAX_EMITTERS, ModLook, engine, light_at};
use olr_scene::{Scene, SceneAssets};

/// An end flash's identity: the projectile's slot, serial, `SimMod` index
/// and spawn time (ms), which a corrected timeline cannot give another.
pub type FlashKey = (usize, u32, usize, u64);

/// How long a started flash or muzzle light is remembered, ms: well past
/// the rollback window (28 ticks), so neither a re-simulation nor a
/// projectile whose predicted end a correction erased, ending for real
/// soon after, starts it twice (the flash stays where it first showed).
const MEMORY_MS: f64 = 2000.0;

/// The most keys remembered: a hostile mod's thousands of projectiles a
/// tick cost bounded time.
const MEMORY: usize = 8192;

/// The most end flashes showing at once (start order); later ones are not
/// started. As many as a frame draws. Muzzle lights are one a worm.
const ACTIVE: usize = MAX_EMITTERS;

/// The longest a flash or muzzle light shows, ms (a looping one ends here).
const LIFE_MS: f64 = 2000.0;

/// The keys started within [`MEMORY_MS`]: a set for lookups (never
/// iterated, so nothing depends on its order), and in start order for
/// forgetting.
#[derive(Debug)]
struct Once<K> {
    set: HashSet<K>,
    order: VecDeque<(f64, K)>,
}

impl<K: Eq + Hash + Copy> Default for Once<K> {
    fn default() -> Self {
        Once { set: HashSet::new(), order: VecDeque::new() }
    }
}

impl<K: Eq + Hash + Copy> Once<K> {
    /// Whether `key`, of a tick at `at`, starts: false when it started
    /// within the memory. The frame's clock `now` forgets keys older than
    /// the memory, or newer than `now` (the clock went back). A
    /// re-simulated tick's key is older than the newest remembered.
    /// Allocates nothing once its buffers reached [`MEMORY`].
    fn start(&mut self, key: K, at: f64, now: f64) -> bool {
        while let Some(&(t, k)) = self.order.front() {
            if now - t < MEMORY_MS && t <= now && self.order.len() < MEMORY {
                break;
            }
            self.order.pop_front();
            self.set.remove(&k);
        }
        if !self.set.insert(key) {
            return false;
        }
        self.order.push_back((at, key));
        true
    }

    fn clear(&mut self) {
        self.set.clear();
        self.order.clear();
    }
}

/// An end flash or muzzle light showing.
#[derive(Clone, Copy, Debug)]
struct Showing {
    /// The projectile (end flash) or the weapon (muzzle light), `SimMod` index.
    def: usize,
    /// Where an end flash shows; a muzzle light follows its worm.
    pos: [f32; 2],
    worm: i32,
    identity: u64,
    start_ms: f64,
}

/// The dynamic lights' state between frames. Allocates nothing once warm.
#[derive(Debug, Default)]
pub(crate) struct Dynamic {
    flashes: Vec<Showing>,
    flashed: Once<FlashKey>,
    muzzles: Vec<Showing>,
    shot: Once<(i32, u64)>,
    /// The flashes the last frame started.
    started: Vec<FlashKey>,
    /// The engine's lights, made once: the muzzle's, and explosions' by size.
    muzzle: Option<Light>,
    explosions: Vec<(i32, Light)>,
    /// Overrides [`engine::SKIN_LIGHT`] (tests and tools).
    skin_light: Option<bool>,
}

impl Dynamic {
    /// Forgets every flash and muzzle light (a new or loaded world).
    pub(crate) fn clear(&mut self) {
        self.flashes.clear();
        self.flashed.clear();
        self.muzzles.clear();
        self.shot.clear();
        self.started.clear();
    }

    /// Overrides the engine's skin-light switch ([`engine::SKIN_LIGHT`]).
    pub(crate) fn set_skin_light(&mut self, on: bool) {
        self.skin_light = Some(on);
    }

    /// The keys of the end flashes the last frame started.
    pub(crate) fn started(&self) -> &[FlashKey] {
        &self.started
    }

    /// Adds the frame's dynamic lights to `look` at the match clock `t_ms`,
    /// starting the flashes and muzzle lights of the scene's ticks since the
    /// last frame (and clearing them). `skins`, the worms' looks, only in a
    /// level with a look (issue #12: a level without one has no darkness):
    /// each drawn worm's skin light, last.
    pub(crate) fn push(
        &mut self,
        look: &mut LookFrame,
        m: &ModLook,
        scene: &mut Scene,
        fx: &Fx,
        skins: Option<&SceneAssets>,
        t_ms: f64,
    ) {
        let projectile = |proj: usize| m.projectiles.get(proj).and_then(Option::as_ref);
        let flash = |proj: usize| projectile(proj).and_then(|p| p.end_flash.as_ref());
        let weapon =
            |w: usize| m.weapons.get(w).and_then(Option::as_ref).and_then(|w| w.muzzle_light.as_ref());
        // Start what the ticks since the last frame began.
        self.started.clear();
        for e in scene.ended_projectiles() {
            let key = (e.slot, e.serial, e.proj, e.spawn_time.0);
            let at = e.at.0 as f64;
            if flash(e.proj).is_some() && self.flashes.len() < ACTIVE && self.flashed.start(key, at, t_ms) {
                let pos = [e.pos.x, e.pos.y];
                let s = Showing { def: e.proj, pos, worm: -1, identity: u64::from(e.serial), start_ms: at };
                self.flashes.push(s);
                self.started.push(key);
            }
        }
        for s in scene.shots() {
            let at = s.at.0 as f64;
            if self.shot.start((s.worm, s.at.0), at, t_ms) {
                // One muzzle light a worm: a new shot restarts it (a held
                // rapid-fire weapon never stacks them, batch D review 3).
                self.muzzles.retain(|m| m.worm != s.worm);
                let identity = s.worm as u64;
                self.muzzles.push(Showing {
                    def: s.weapon,
                    pos: [0.0; 2],
                    worm: s.worm,
                    identity,
                    start_ms: at,
                });
            }
        }
        scene.clear_look_events();
        // A clock that went back (a restart, a seek) forgets what came after.
        let ended = |l: &Light, s: &Showing| {
            let age = t_ms - s.start_ms;
            !(0.0..LIFE_MS).contains(&age) || l.animation.ended(age + f64::from(l.phase_ms.unwrap_or(0)))
        };
        let engine_muzzle = self.muzzle.get_or_insert_with(engine::muzzle_light);
        let worms = scene.drawn_worms();
        self.muzzles.retain(|s| {
            let l = weapon(s.def).unwrap_or(engine_muzzle);
            if ended(l, s) {
                return false;
            }
            // It follows its worm as drawn; a worm not drawn (dead) shows none.
            let radius = engine::disc_radius(l.reach.pixels());
            // Plan 15c: a disc's cells never count as its worm's body.
            if let Some(w) = worms.iter().find(|w| w.id == s.worm)
                && let Some(f) = light_at(l, t_ms - s.start_ms, s.identity, [w.pos.x, w.pos.y], radius)
            {
                look.push_emitter(f);
            }
            true
        });
        for (slot, e) in fx.entities().filter(|(_, e)| e.kind == Kind::Explosion) {
            // An explosion's frame starts at 15 - size and grows 40 a second
            // to 15: it lives size × 25 ms, and its light's keys span that.
            let life_ms = f64::from(e.type2.max(1)) * 25.0;
            let age_ms = f64::from((e.frame - 15.0 + e.type2 as f32) * 25.0).clamp(0.0, life_ms);
            let i = match self.explosions.iter().position(|(s, _)| *s == e.type2) {
                Some(i) => i,
                None => {
                    self.explosions.push((e.type2, engine::explosion_light(e.type2)));
                    self.explosions.len() - 1
                }
            };
            let clock = age_ms / life_ms * f64::from(engine::EXPLOSION_MS);
            let (radius, pos) = (engine::explosion_radius(e.type2), [e.pos.x, e.pos.y]);
            if let Some(f) = light_at(&self.explosions[i].1, clock, slot as u64, pos, radius) {
                look.push_emitter(f);
            }
        }
        // Mods' lights after the engine's (final review, Important 3): past
        // the disc cap, explosions and muzzle flashes still light.
        for d in scene.drawn_projectiles() {
            if let Some(l) = projectile(d.proj).and_then(|p| p.light.as_ref()) {
                let clock = t_ms - d.spawn_time.0 as f64;
                let radius = engine::disc_radius(l.reach.pixels());
                if let Some(f) = light_at(l, clock, u64::from(d.serial), [d.pos.x, d.pos.y], radius) {
                    look.push_emitter(f);
                }
            }
        }
        self.flashes.retain(|s| {
            let Some(l) = flash(s.def).filter(|l| !ended(l, s)) else { return false };
            let radius = engine::disc_radius(l.reach.pixels());
            if let Some(f) = light_at(l, t_ms - s.start_ms, s.identity, s.pos, radius) {
                look.push_emitter(f);
            }
            true
        });
        // Issue #12: each drawn worm's skin light, its frame's colour and
        // strength, following the worm (a disc inside its own worm ignores
        // its body); behind the engine's one switch.
        let Some(assets) = skins.filter(|_| self.skin_light.unwrap_or(engine::SKIN_LIGHT)) else { return };
        for w in scene.drawn_worms() {
            if let Some((f, reach)) = assets.worm_light(w.index, w.frame) {
                let l = engine::skin_light(f.colour, f.intensity, reach);
                let radius = engine::disc_radius(l.reach.pixels());
                if let Some(f) = light_at(&l, t_ms, w.id as u64, [w.pos.x, w.pos.y], radius) {
                    look.push_emitter(f);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_starts_a_key_once_within_the_memory() {
        let mut o = Once::default();
        assert!(o.start((1, 2), 0.0, 0.0));
        assert!(!o.start((1, 2), 100.0, 100.0), "again within the memory");
        assert!(o.start((1, 3), 100.0, 100.0));
        assert!(!o.start((1, 2), 20.0, 120.0), "a re-simulated tick, earlier");
        assert!(o.start((1, 2), 2100.0, 2100.0), "forgotten past the memory");
    }

    #[test]
    fn once_is_bounded() {
        let mut o = Once::default();
        // A hostile mod: thousands of keys a tick for 3 s.
        for tick in 0..300u64 {
            for k in 0..4096u64 {
                o.start((tick, k), tick as f64 * 10.0, tick as f64 * 10.0);
            }
            assert!(o.set.len() <= MEMORY && o.order.len() <= MEMORY);
        }
    }
}
