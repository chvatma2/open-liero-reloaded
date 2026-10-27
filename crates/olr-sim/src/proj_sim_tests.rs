use super::*;
use crate::actions::bounce;
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::spawn::{SpawnCtx, spawn_projectile};
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use olr_data::level::MaterialGrid;
use olr_data::mod_def::{ActionDef, Animation, ProjectileDef, SpawnDef, TimerDef, TrailDef};
use olr_math::{GlibcRand, Mt19937};

/// The frame clock the tests simulate up to.
pub(crate) const NOW: AbsTime = AbsTime(10_000);

/// A projectile that never collides with anything: no gravity, no actions.
pub(crate) fn inert() -> ProjectileDef {
    ProjectileDef {
        gravity: Some(0),
        hit: ActionDef::olx_default(ActionKind::Nothing),
        player_hit: ActionDef::olx_default(ActionKind::Nothing),
        ..pixel_projectile(None)
    }
}

/// Lives `secs` seconds, then disappears.
pub(crate) fn timed(secs: f32, p: ProjectileDef) -> ProjectileDef {
    let action = Some(ActionDef::olx_default(ActionKind::Disappear2));
    ProjectileDef { timer: TimerDef { time: secs, time_var: 0.0, action }, ..p }
}

/// Leaves a `child` trail projectile every step.
fn trailing(child: &str, amount: i32) -> ProjectileDef {
    let spawn =
        SpawnDef { amount, speed: 10, spread: 30.0, speed_var: 5.0, ..SpawnDef::olx_default(child.into()) };
    let trail = TrailDef { kind: TrailKind::Projectile, delay: 0.001, spawn: Some(spawn) };
    timed(100.0, ProjectileDef { trail, ..inert() })
}

/// One weapon per projectile, named like it, so each projectile's path
/// name is its own name and its children are `<name>.trail`.
pub(crate) fn sim_mod(projectiles: Vec<(&str, ProjectileDef)>, frames: &[(&str, u32)]) -> SimMod {
    let weapons = projectiles.iter().map(|(n, _)| projectile_weapon(n, n)).collect();
    let def = mod_def(weapons, projectiles);
    let frames: BTreeMap<_, _> = frames.iter().map(|(f, n)| (f.to_string(), *n)).collect();
    SimMod::compile(&def, &frames).unwrap()
}

pub(crate) struct Bench {
    pub sim_mod: SimMod,
    pub settings: SimSettings,
    pub terrain: Terrain,
    pub holes: Vec<Stamp>,
    pub green_mask: Option<GreenMask>,
    pub worms: Vec<Worm>,
    pub draws: Draws,
    pub pool: ProjectilePool,
    pub bonuses: [Bonus; MAX_BONUSES],
    pub referee: crate::referee::Referee,
}

impl Bench {
    pub fn new(sim_mod: SimMod, terrain: MaterialGrid, capacity: usize) -> Self {
        let draws = Draws::new(GlibcRand::new(1), Mt19937::new(1), sim_mod.projectiles().len());
        Self {
            sim_mod,
            settings: SimSettings::lx56(),
            terrain: Terrain::from_grid(&terrain),
            holes: Vec::new(),
            green_mask: None,
            worms: Vec::new(),
            draws,
            pool: ProjectilePool::new(capacity),
            bonuses: [Bonus::default(); MAX_BONUSES],
            referee: crate::referee::Referee::oracle(),
        }
    }

    pub fn open(sim_mod: SimMod, capacity: usize) -> Self {
        Self::new(sim_mod, MaterialGrid::new(200, 200, Material::Background), capacity)
    }

    /// Spawns projectile type `proj` at `time` (`CClient::SpawnProjectile`).
    pub fn spawn(&mut self, proj: &str, pos: Vec2, vel: Vec2, random: i32, time: AbsTime) {
        let proj = self.sim_mod.projectile_index(proj).unwrap();
        let mut ctx = SpawnCtx {
            sim_mod: &self.sim_mod,
            pool: &mut self.pool,
            rand: &mut self.draws.rand,
            settings: &self.settings,
            now: time,
            work: &mut self.draws.spawn_work.0,
            rot_bumped: &mut self.draws.rot_bumped,
        };
        spawn_projectile(&mut ctx, proj, pos, vel, 0, -1, random, time, time);
    }

    /// Runs `f` with the pool and a context at frame time `now`.
    pub fn with_ctx<R>(&mut self, now: AbsTime, f: impl FnOnce(&mut ProjectilePool, &mut ProjCtx) -> R) -> R {
        let mut ctx = ProjCtx {
            sim_mod: &self.sim_mod,
            settings: &self.settings,
            terrain: &mut self.terrain,
            live: None,
            worms: &mut self.worms,
            draws: &mut self.draws,
            holes: &self.holes,
            green_mask: self.green_mask.as_ref(),
            now,
            wall_clock: now + TimeDiff(10),
            physics_dt: TimeDiff(11),
            bonuses: &mut self.bonuses,
            referee: &mut self.referee,
        };
        f(&mut self.pool, &mut ctx)
    }

    /// One `simulateProjectiles` pass up to `now`.
    pub fn simulate(&mut self, now: AbsTime) {
        self.with_ctx(now, simulate_projectiles);
    }

    fn slots(&self) -> Vec<usize> {
        self.pool.iter().map(|(i, _)| i).collect()
    }
}

#[test]
fn pool_full_drops_new_projectiles_like_olx() {
    // 3000 trailing projectiles fill OLX's pool. Each wants two children per
    // step: the parent's two draws per child still happen (spread, speed
    // variation), but the child, and its colour rand(), do not.
    let m = sim_mod(vec![("parent", trailing("child", 2)), ("child", inert())], &[]);
    let mut b = Bench::open(m, ProjectilePool::DEFAULT_CAPACITY);
    for i in 0..3000 {
        b.spawn("parent", Vec2::new(100.0, 100.0), Vec2::ZERO, i % 255, NOW + TimeDiff(0));
    }
    b.spawn("parent", Vec2::new(100.0, 100.0), Vec2::ZERO, 0, NOW);
    assert_eq!(b.pool.iter().count(), 3000, "the 3001st projectile is dropped");
    let before: Vec<i32> = b.pool.iter().map(|(_, p)| p.random).collect();
    let rand_calls = b.draws.rand.calls();
    b.simulate(NOW + TimeDiff(11));
    assert_eq!(b.pool.iter().count(), 3000);
    assert_eq!(b.draws.rand.calls(), rand_calls, "no child drew its colour");
    for ((_, p), r) in b.pool.iter().zip(before) {
        assert_eq!(p.random, (r + 4) % 255, "two fixed-table draws per dropped child");
        assert_eq!(p.last_sim_time, NOW + TimeDiff(11));
    }
    // Many more steps never panic; a freed slot is reused by the next child.
    b.pool.set_unused(1234);
    b.simulate(NOW + TimeDiff(22));
    let p = b.pool.get(1234).unwrap();
    assert_eq!(
        (b.sim_mod.projectiles()[p.proj].name.as_str(), p.spawn_time),
        ("parent.trail", NOW + TimeDiff(22))
    );
    for k in 3..100 {
        b.simulate(NOW + TimeDiff(11 * k));
    }
    assert_eq!(b.pool.iter().count(), 3000);
}

#[test]
fn bounce_left_right_overrides_top_bottom() {
    let mut p = Projectile { vel: Vec2::new(10.0, 20.0), ..Projectile::default() };
    let cases = [
        (COL_TOP, (5.0, -10.0)),
        (COL_BOTTOM, (5.0, -10.0)),
        (COL_TOP | COL_LEFT, (-5.0, 10.0)),
        (COL_BOTTOM | COL_RIGHT, (-5.0, 10.0)),
        (0, (10.0, 20.0)),
    ];
    for (side, want) in cases {
        p.vel = Vec2::new(10.0, 20.0);
        p.collision_side = side;
        bounce(&mut p, 0.5);
        assert_eq!((p.vel.x, p.vel.y), want, "side {side}");
    }
}

#[test]
fn wallshoot_window_keeps_old_pos_at_spawn() {
    // Dirt from x = 52 on. A projectile at x = 50 moves 2.2 px per step. Its
    // first step lands in the dirt inside the wallshoot window (11 ms here),
    // which skips the terrain test and keeps vOldPos at the spawn position.
    // The second step collides, and the reset goes back to the spawn position.
    let mut cells = vec![Material::Background; 100 * 100];
    for (i, c) in cells.iter_mut().enumerate() {
        if i % 100 >= 52 {
            *c = Material::Dirt;
        }
    }
    let wall = ProjectileDef { hit: ActionDef::olx_default(ActionKind::Disappear), ..timed(10.0, inert()) };
    let m = sim_mod(vec![("wall", wall)], &[]);
    let mut b = Bench::new(m, MaterialGrid::from_cells(100, 100, cells).unwrap(), 10);
    let start = Vec2::new(50.0, 50.0);
    // iRandom 1: fWallshootTime = 0.011 + 0.415797 / 1000, an 11 ms window.
    b.spawn("wall", start, Vec2::new(200.0, 0.0), 1, NOW);
    assert_eq!(TimeDiff((b.pool.get(0).unwrap().wallshoot_time * 1000.0) as u64), TimeDiff(11));
    b.simulate(NOW + TimeDiff(11));
    let p = b.pool.get(0).unwrap();
    assert!(p.pos.x > 52.0, "moved into the dirt: {:?}", p.pos);
    assert_eq!(p.old_pos, start, "the window does not save the position");
    b.simulate(NOW + TimeDiff(22));
    let p = b.pool.get(0).unwrap();
    assert_eq!((p.pos, p.old_pos), (start, start), "the collision resets to the spawn position");
}

#[test]
fn children_in_higher_slots_run_in_the_same_pass() {
    // A parent catches up two steps and leaves a child in each. A child in a
    // slot above the parent's is simulated in the same pass, from its spawn
    // time; one in a slot below waits for the next pass and then catches up.
    let m = sim_mod(
        vec![
            ("parent", trailing("child", 1)),
            ("filler", timed(100.0, inert())),
            ("child", timed(100.0, inert())),
        ],
        &[],
    );
    let step = |b: &Bench, i: usize| {
        let p = b.pool.get(i).unwrap();
        (b.sim_mod.projectiles()[p.proj].name.clone(), p.spawn_time, p.last_sim_time, p.life)
    };
    let t = |ms: u64| AbsTime(NOW.0 + ms);
    let one_step = TimeDiff(11).seconds();

    // Slot 0 is taken: both children land above the parent (slot 1).
    let mut b = Bench::open(m.clone(), 10);
    b.spawn("filler", Vec2::new(10.0, 10.0), Vec2::ZERO, 0, t(0));
    b.spawn("parent", Vec2::new(100.0, 100.0), Vec2::ZERO, 0, t(0));
    b.simulate(t(22));
    assert_eq!(b.slots(), [0, 1, 2, 3]);
    assert_eq!(step(&b, 2), ("parent.trail".into(), t(11), t(22), one_step), "ran its step in this pass");
    assert_eq!(step(&b, 3), ("parent.trail".into(), t(22), t(22), 0.0), "spawned at the frame time");

    // Slot 0 is free: the first child lands below the parent and waits.
    let mut b = Bench::open(m, 10);
    b.spawn("filler", Vec2::new(10.0, 10.0), Vec2::ZERO, 0, t(0));
    b.spawn("parent", Vec2::new(100.0, 100.0), Vec2::ZERO, 0, t(0));
    b.pool.set_unused(0);
    b.simulate(t(22));
    assert_eq!(b.slots(), [0, 1, 2]);
    assert_eq!(step(&b, 0), ("parent.trail".into(), t(11), t(11), 0.0), "waits for the next pass");
    b.simulate(t(33));
    assert_eq!(step(&b, 0).2, t(33), "then catches up two steps");
    assert_eq!(step(&b, 0).3, one_step + one_step);
}

#[test]
fn ani_once_deletes_when_the_animation_ends() {
    // An action-less image projectile animating once through 3 frames at 100
    // frames per second: 1.1 frames per step, so it is deleted in its third
    // step. Without the image's frame count it is a "junk projectile",
    // deleted after its first step.
    let mut flame = inert();
    flame.gfx = ProjectileGfx::Image {
        file: "gfx/flame.png".into(),
        rotating: false,
        rot_increment: 0,
        rot_speed: 0,
        use_angle: false,
        use_spec_angle: false,
        angle_images: 0,
        animation: Some(Animation { rate: 100.0, kind: AnimKind::Once }),
    };
    let t = |ms: u64| AbsTime(NOW.0 + ms);
    let mut b = Bench::open(sim_mod(vec![("flame", flame.clone())], &[("gfx/flame.png", 3)]), 10);
    b.spawn("flame", Vec2::new(100.0, 100.0), Vec2::ZERO, 0, t(0));
    b.simulate(t(22));
    let frame = b.pool.get(0).unwrap().frame;
    assert_eq!(frame, 100.0 * TimeDiff(11).seconds() + 100.0 * TimeDiff(11).seconds());
    b.simulate(t(33));
    assert!(b.pool.get(0).is_none(), "frame 3.3 >= 3 frames");

    let mut b = Bench::open(sim_mod(vec![("flame", flame)], &[]), 10);
    b.spawn("flame", Vec2::new(100.0, 100.0), Vec2::ZERO, 0, t(0));
    b.simulate(t(11));
    assert!(b.pool.get(0).is_none(), "no image: deleted after one step");
}
