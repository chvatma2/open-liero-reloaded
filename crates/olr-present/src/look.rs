//! The look's dynamic lights (plan 15a Task 15; spec §Mod packs, §The
//! engine), glowing discs in the glow light since plan 15c (radii from
//! `engine::disc_radius` and `engine::explosion_radius`), the engine's
//! first so the disc cap never drops them (final review, Important 3): the
//! muzzle lights (start order), the engine's explosion lights (effect slot
//! order); then the mods': their explosion lights (issue #15, a projectile
//! look's `explosion` or the mod's, effect slot order), the projectiles'
//! lights (slot order) and their end flashes (start order); then the skins'
//! lights (issue #12, worm order), after the mods' so the cap never starves
//! those either; last the beams' (issue #69: discs along each drawn beam
//! and one at its tip, worm order), so past the cap they drop, never
//! another light.
//!
//! End flashes and muzzle lights start from the scene's per-tick records
//! (`Scene::ended_projectiles`, `Scene::shots`), so they never depend on
//! the frame rate (spec §Fair play), and run their keys from the tick they
//! started. A re-simulated tick reports them again: each starts once.
//!
//! Issue #40: in a level whose surface materials heat, each explosion
//! (`Scene::explosions`) leaves the engine's heat
//! (`engine::explosion_heat`) once, the same way, queued with its tick's
//! time for the renderer (`Presentation::upload`). Issue #42: or its
//! projectile look's `explosion_heat` (else the mod's), and each tick a
//! projectile whose look has `heat` lives (`Scene::hot_projectiles`) leaves
//! that tick's where it is, once. Issue #69: and each sub-step a beam
//! stops on rock or dirt (`Scene::beam_hits`) leaves its weapon's beam heat
//! (else the engine's) there, once.

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

use olr_fx::{Entity, Fx, Kind};
use olr_look::{Light, LookFrame, MAX_EMITTERS, ModLook, engine, light_at};
use olr_render::HeatDeposit;
use olr_scene::{Scene, SceneAssets};

/// An end flash's identity: the projectile's slot, serial, `SimMod` index
/// and spawn time (ms), which a corrected timeline cannot give another.
pub type FlashKey = (usize, u32, usize, u64);

/// An explosion's identity for its heat (issue #40): its tick's time (ms)
/// and its place among the tick's explosions, which a re-simulated tick
/// gives again. A correction moving it leaves the heat where it first
/// landed.
pub type BlastKey = (u64, u16);

/// A beam hit's identity for its heat (issue #69): its tick's time (ms) and
/// its place among the tick's beam hits, which a re-simulated tick gives
/// again.
type BeamKey = (u64, u16);

/// A heating projectile's identity for a tick's heat (issue #42): the
/// tick's time (ms), its slot and serial, which a re-simulated tick gives
/// again.
type WarmKey = (u64, usize, u32);

/// The most heat deposits queued for the renderer between two uploads
/// (reserved at the first): a host that never uploads drops the rest.
const HEAT_QUEUE: usize = 2048;

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
    /// Issue #40: the explosions whose heat was queued, the heat waiting
    /// for the renderer (each with its tick's time, ms, in order) and the
    /// explosions the last frame queued.
    heated: Once<BlastKey>,
    /// Issue #42: the heating projectiles' ticks whose heat was queued.
    warmed: Once<WarmKey>,
    /// Issue #69: the beam hits whose heat was queued.
    beamed: Once<BeamKey>,
    heat: Vec<(u64, HeatDeposit)>,
    blasts: Vec<BlastKey>,
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
        self.heated.clear();
        self.warmed.clear();
        self.beamed.clear();
        self.heat.clear();
        self.blasts.clear();
    }

    /// Overrides the engine's skin-light switch ([`engine::SKIN_LIGHT`]).
    pub(crate) fn set_skin_light(&mut self, on: bool) {
        self.skin_light = Some(on);
    }

    /// The keys of the end flashes the last frame started.
    pub(crate) fn started(&self) -> &[FlashKey] {
        &self.started
    }

    /// The explosions whose heat the last frame queued.
    pub(crate) fn blasts(&self) -> &[BlastKey] {
        &self.blasts
    }

    /// The heat waiting for the renderer, in tick order (issue #40).
    pub(crate) fn heat(&self) -> &[(u64, HeatDeposit)] {
        &self.heat
    }

    /// Forgets the heat waiting (the renderer took it).
    pub(crate) fn clear_heat(&mut self) {
        self.heat.clear();
    }

    /// Adds the frame's dynamic lights to `look` at the match clock `t_ms`,
    /// starting the flashes and muzzle lights of the scene's ticks since the
    /// last frame (and clearing them). `skins`, the worms' looks, only in a
    /// level with a look (issue #12: a level without one has no darkness):
    /// each drawn worm's skin light, last. With `heats` (the level's
    /// materials heat, issue #40) each explosion's heat is queued once, and
    /// (issue #42) each heating projectile's tick and (issue #69) each beam
    /// hit, in tick order.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push(
        &mut self,
        look: &mut LookFrame,
        m: &ModLook,
        scene: &mut Scene,
        fx: &Fx,
        skins: Option<&SceneAssets>,
        t_ms: f64,
        heats: bool,
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
        // Issue #40: each explosion's heat, once, at its tick's time: from
        // the per-tick records, never the entities a frame may miss. Issue
        // #42: its projectile's (or the mod's) explosion heat, else the
        // engine's; merged in tick order with the heating projectiles' and
        // (issue #69) the beam hits' (a tick's explosions first, then its
        // heating projectiles, then its beams).
        self.blasts.clear();
        let (blasts, hot, beams) = if heats {
            (scene.explosions(), scene.hot_projectiles(), scene.beam_hits())
        } else {
            (&[][..], &[][..], &[][..])
        };
        let (mut i, mut j, mut k) = (0, 0, 0);
        while i < blasts.len() || j < hot.len() || k < beams.len() {
            if self.heat.len() == HEAT_QUEUE {
                break;
            }
            if self.heat.capacity() == 0 {
                self.heat.reserve_exact(HEAT_QUEUE);
            }
            let (x_at, h_at) = (blasts.get(i).map(|x| x.at), hot.get(j).map(|p| p.at));
            let b_at = beams.get(k).map(|b| b.at);
            // Whether stream `a` goes before `b`: it has one left, at a
            // tick no later than `b`'s next.
            let first = |a: Option<_>, b: Option<_>| a.is_some() && (b.is_none() || a <= b);
            let explosion = first(x_at, h_at) && first(x_at, b_at);
            let warm = !explosion && first(h_at, b_at);
            if explosion {
                let x = &blasts[i];
                i += 1;
                let key = (x.at.0, x.index);
                let h = match m.explosion_heat(x.proj) {
                    Some(own) => own.heat(x.damage),
                    None => engine::explosion_heat(x.damage),
                };
                if let Some(h) = h
                    && self.heated.start(key, x.at.0 as f64, t_ms)
                {
                    let deposit = HeatDeposit { pos: [x.pos.x, x.pos.y], radius: h.radius, amount: h.amount };
                    self.heat.push((x.at.0, deposit));
                    self.blasts.push(key);
                }
            } else if warm {
                let p = &hot[j];
                j += 1;
                if let Some(h) = m.heat(p.proj)
                    && self.warmed.start((p.at.0, p.slot, p.serial), p.at.0 as f64, t_ms)
                {
                    let h = h.tick();
                    let deposit = HeatDeposit { pos: [p.pos.x, p.pos.y], radius: h.radius, amount: h.amount };
                    self.heat.push((p.at.0, deposit));
                }
            } else {
                let b = &beams[k];
                k += 1;
                if let Some(h) = m.beam_heat(b.weapon).part(b.parts)
                    && self.beamed.start((b.at.0, b.index), b.at.0 as f64, t_ms)
                {
                    let deposit = HeatDeposit { pos: [b.pos.x, b.pos.y], radius: h.radius, amount: h.amount };
                    self.heat.push((b.at.0, deposit));
                }
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
            // Plan 15c: it shines from inside its worm, which blocks no light (#53).
            if let Some(w) = worms.iter().find(|w| w.id == s.worm)
                && let Some(f) = light_at(l, t_ms - s.start_ms, s.identity, [w.pos.x, w.pos.y], radius)
            {
                look.push_emitter(f);
            }
            true
        });
        // The engine's explosion lights: those whose projectile's look (or
        // the mod's) has none (issue #15). A beam's carve explosions cast
        // none: its tip light is its light there (issue #69).
        let engine_lit = |proj: i32| proj != olr_sim::BEAM_EXPLOSION && m.explosion(proj).is_none();
        for (slot, e, _) in explosions(fx).filter(|&(_, _, proj)| engine_lit(proj)) {
            let (age_ms, life_ms) = explosion_age(e);
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
        // the disc cap, explosions and muzzle flashes still light. First
        // the mods' explosion lights (issue #15), slot order: the engine's
        // flash fitted to the explosion's life, or the mod's animation at
        // the explosion's age; capped where read.
        for (slot, e, proj) in explosions(fx) {
            let Some(x) = m.explosion(proj) else { continue };
            let (age_ms, life_ms) = explosion_age(e);
            let clock = if x.fitted { age_ms / life_ms * f64::from(engine::EXPLOSION_MS) } else { age_ms };
            if let Some(mut f) = light_at(&x.light, clock, slot as u64, [e.pos.x, e.pos.y], x.radius()) {
                f.reach = f.reach.min(x.reach());
                look.push_emitter(f);
            }
        }
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
        if let Some(assets) = skins.filter(|_| self.skin_light.unwrap_or(engine::SKIN_LIGHT)) {
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
        // Issue #69, last: each drawn beam's discs along it, every
        // `BEAM_DISC_SPACING` from its start to its end (both included, at
        // most `MAX_BEAM_DISCS`, spaced wider to fit), and one at its tip
        // when it stopped on something; its weapon's own lights, else the
        // engine's in its colour.
        for b in scene.drawn_beams() {
            let own = m.beam(b.weapon);
            let rgb = [b.colour.r, b.colour.g, b.colour.b];
            let (from, to) = ([b.from.x, b.from.y], [b.to.x, b.to.y]);
            let id = b.worm as u64;
            let engine_light;
            let l = match own.and_then(|o| o.light.as_ref()) {
                Some(l) => l,
                None => {
                    engine_light = engine::beam_light(rgb);
                    &engine_light
                }
            };
            for p in beam_discs(from, to) {
                if let Some(f) = light_at(l, t_ms, id, p, engine::BEAM_DISC_RADIUS) {
                    look.push_emitter(f);
                }
            }
            if b.stopped {
                let engine_tip;
                let l = match own.and_then(|o| o.tip_light.as_ref()) {
                    Some(l) => l,
                    None => {
                        engine_tip = engine::beam_tip_light(rgb);
                        &engine_tip
                    }
                };
                if let Some(f) = light_at(l, t_ms, id, to, engine::BEAM_TIP_RADIUS) {
                    look.push_emitter(f);
                }
            }
        }
    }
}

/// Where a beam from `from` to `to` has its discs (issue #69): every
/// `BEAM_DISC_SPACING` world pixels, both ends included, at most
/// `MAX_BEAM_DISCS` (spaced wider to fit); one for a beam with no length.
fn beam_discs(from: [f32; 2], to: [f32; 2]) -> impl Iterator<Item = [f32; 2]> {
    let d = [to[0] - from[0], to[1] - from[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let n = if len.is_finite() {
        ((len / engine::BEAM_DISC_SPACING).ceil() as usize + 1).clamp(1, engine::MAX_BEAM_DISCS)
    } else {
        0
    };
    let last = (n.max(2) - 1) as f32;
    (0..n).map(move |i| {
        let t = i as f32 / last;
        [from[0] + d[0] * t, from[1] + d[1] * t]
    })
}

/// The explosion entities in slot order, each with its slot and
/// projectile (-1 for none).
fn explosions(fx: &Fx) -> impl Iterator<Item = (usize, &Entity, i32)> {
    fx.entities().filter_map(|(slot, e)| match e.kind {
        Kind::Explosion { proj } => Some((slot, e, proj)),
        _ => None,
    })
}

/// An explosion's age and life, ms: its frame starts at 15 - size and
/// grows 40 a second to 15, so it lives size × 25 ms.
fn explosion_age(e: &Entity) -> (f64, f64) {
    let life_ms = f64::from(e.type2.max(1)) * 25.0;
    let age_ms = f64::from((e.frame - 15.0 + e.type2 as f32) * 25.0).clamp(0.0, life_ms);
    (age_ms, life_ms)
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
    fn beam_discs_space_out_to_fit_the_bound() {
        let discs = |len: f32| beam_discs([10.0, 20.0], [10.0 + len, 20.0]).collect::<Vec<_>>();
        assert_eq!(discs(0.0), [[10.0, 20.0]], "a beam with no length: one");
        let short = discs(16.0);
        assert_eq!(short, [[10.0, 20.0], [18.0, 20.0], [26.0, 20.0]], "every 8 px, both ends");
        let long = discs(5000.0);
        assert_eq!(long.len(), engine::MAX_BEAM_DISCS);
        assert_eq!(long[0], [10.0, 20.0]);
        assert_eq!(*long.last().unwrap(), [5010.0, 20.0], "the end included");
        assert_eq!(beam_discs([0.0, 0.0], [f32::NAN, 0.0]).count(), 0, "never a NaN disc");
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
