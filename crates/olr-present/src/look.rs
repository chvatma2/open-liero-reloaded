//! The look's dynamic lights (plan 15a Task 15; spec §Mod packs, §The
//! engine): after the level's own, in R9's order, the projectiles' lights
//! (slot order), their end flashes (start order), the muzzle lights (worm
//! order) and the explosions' lights (effect slot order).

use olr_fx::{Fx, Kind};
use olr_look::{Light, LookFrame, ModLook, engine, light_at};
use olr_scene::{DrawnProjectile, Scene};

/// How long a flashed projectile is remembered, ms: a rollback that ends
/// it again within that time never flashes it twice (review focus 5).
const FLASH_MEMORY_MS: f64 = 2000.0;

/// An end flash showing.
#[derive(Clone, Copy, Debug)]
struct Flash {
    /// The projectile's slot and serial.
    key: (usize, u32),
    /// Its `SimMod` index.
    proj: usize,
    pos: [f32; 2],
    start_ms: f64,
}

/// The dynamic lights' state between frames. Allocates nothing once warm.
#[derive(Debug, Default)]
pub(crate) struct Dynamic {
    /// The projectiles with an end flash the last frame drew, in slot order.
    last: Vec<DrawnProjectile>,
    active: Vec<Flash>,
    /// Every (slot, serial) flashed within [`FLASH_MEMORY_MS`], with when.
    flashed: Vec<((usize, u32), f64)>,
    /// The flashes the last frame started.
    started: Vec<(usize, u32)>,
    /// The engine's lights, made once: the muzzle's, and explosions' by size.
    muzzle: Option<Light>,
    explosions: Vec<(i32, Light)>,
}

impl Dynamic {
    /// Forgets every flash (a new or loaded world).
    pub(crate) fn clear(&mut self) {
        self.last.clear();
        self.active.clear();
        self.flashed.clear();
        self.started.clear();
    }

    /// The (slot, serial) of the end flashes the last frame started.
    pub(crate) fn started(&self) -> &[(usize, u32)] {
        &self.started
    }

    /// Adds the frame's dynamic lights to `look` at the match clock `t_ms`;
    /// `tick_ms` is the latest tick's time, where the scene's marks began.
    pub(crate) fn push(
        &mut self,
        look: &mut LookFrame,
        m: &ModLook,
        scene: &Scene,
        fx: &Fx,
        t_ms: f64,
        tick_ms: f64,
    ) {
        let drawn = scene.drawn_projectiles();
        let projectile = |proj: usize| m.projectiles.get(proj).and_then(Option::as_ref);
        for d in drawn {
            if let Some(l) = projectile(d.proj).and_then(|p| p.light.as_ref()) {
                let clock = t_ms - d.spawn_time.0 as f64;
                if let Some(f) = light_at(l, clock, u64::from(d.serial), [d.pos.x, d.pos.y], 1.0) {
                    look.push(f);
                }
            }
        }
        self.flashes(look, m, drawn, t_ms);
        for mz in scene.drawn_muzzles() {
            let own = m.weapons.get(mz.weapon).and_then(Option::as_ref).and_then(|w| w.muzzle_light.as_ref());
            let l = own.unwrap_or_else(|| self.muzzle.get_or_insert_with(engine::muzzle_light));
            if let Some(f) = light_at(l, t_ms - tick_ms, mz.worm as u64, [mz.pos.x, mz.pos.y], 1.0) {
                look.push(f);
            }
        }
        for (slot, e) in fx.entities().filter(|(_, e)| e.kind == Kind::Explosion) {
            // An explosion's frame starts at 15 - size and grows 40 a second.
            let age_ms = f64::from((e.frame - 15.0 + e.type2 as f32) * 25.0).max(0.0);
            let i = match self.explosions.iter().position(|(s, _)| *s == e.type2) {
                Some(i) => i,
                None => {
                    self.explosions.push((e.type2, engine::explosion_light(e.type2)));
                    self.explosions.len() - 1
                }
            };
            if let Some(f) = light_at(&self.explosions[i].1, age_ms, slot as u64, [e.pos.x, e.pos.y], 1.0) {
                look.push(f);
            }
        }
    }

    /// End flashes (plan 15a R8): a projectile with an end flash drawn last
    /// frame whose slot is gone or holds another serial ended where it was
    /// last drawn; each (slot, serial) flashes at most once.
    fn flashes(&mut self, look: &mut LookFrame, m: &ModLook, drawn: &[DrawnProjectile], t_ms: f64) {
        let flash =
            |proj: usize| m.projectiles.get(proj).and_then(Option::as_ref).and_then(|p| p.end_flash.as_ref());
        self.started.clear();
        // A clock that went back (a restart, a seek) forgets what came after.
        self.active.retain(|f| f.start_ms <= t_ms);
        self.flashed.retain(|(_, at)| *at <= t_ms && t_ms - at < FLASH_MEMORY_MS);
        // Both lists are in slot order: one walk finds the projectiles gone.
        let mut now = drawn.iter().peekable();
        for d in &self.last {
            while now.next_if(|n| n.slot < d.slot).is_some() {}
            let key = (d.slot, d.serial);
            if now.peek().is_some_and(|n| (n.slot, n.serial) == key)
                || self.flashed.iter().any(|(k, _)| *k == key)
            {
                continue;
            }
            self.active.push(Flash { key, proj: d.proj, pos: [d.pos.x, d.pos.y], start_ms: t_ms });
            self.flashed.push((key, t_ms));
            self.started.push(key);
        }
        self.last.clear();
        self.last.extend(drawn.iter().filter(|d| flash(d.proj).is_some()));
        self.active.retain(|f| {
            let Some(l) = flash(f.proj) else { return false };
            // A looping flash is let go with the memory.
            let age = t_ms - f.start_ms;
            if age >= FLASH_MEMORY_MS || l.animation.ended(age + f64::from(l.phase_ms.unwrap_or(0))) {
                return false;
            }
            if let Some(x) = light_at(l, t_ms - f.start_ms, u64::from(f.key.1), f.pos, 1.0) {
                look.push(x);
            }
            true
        });
    }
}
