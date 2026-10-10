//! Smoke matches (spec §7.1, plan 4 task 10): four worms with random, seeded
//! keys and weapons on every imported level with both mods, bonuses on.
//! Every tick checks the invariants; at [`CLONE_AT`] the world is cloned and
//! both copies must give the same checksums and events to the end. Every
//! tick's terrain edits are replayed on a shadow terrain, as a renderer
//! replays them on its colour images, which must keep the world's material.
//! Needs the imported OLX content: `RL_CONTENT_DIR`, which `.cargo/config.toml`
//! sets to `target/content` unless the environment already does.

use std::path::Path;
use std::sync::Arc;

use rl_content::{Content, olx_mod_id};
use rl_sim::{
    EventKind, Keys, Loadout, MAX_BONUSES, MatchSetup, OracleMode, SimSettings, Slot, StampId, TerrainOp,
    TickEvents, World, WormSetup,
};
use rl_terrain::{GreenMask, PX_ROCK, Stamp, Terrain};

/// Ticks per match. 3000 (30 s) in the plan; at opt-level 1 eight matches of
/// 3000 ticks with a checksum per tick fit the gate's budget when nextest
/// runs them in parallel.
const TICKS: usize = 3000;
const CLONE_AT: usize = 1500;
const WORMS: usize = 4;

/// The imported content (`RL_CONTENT_DIR`), or `None` with a notice if it is
/// unset or has not been imported.
fn content_dir() -> Option<String> {
    let Ok(dir) = std::env::var("RL_CONTENT_DIR") else {
        eprintln!("RL_CONTENT_DIR is not set (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    if Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

/// xorshift64*: the test's own generator, so the keys do not touch the world's.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// Keys a worm holds for a random 5 to 50 ticks: mostly firing and moving,
/// sometimes jumping, roping, aiming or changing weapons.
struct Player {
    keys: u8,
    left: u64,
}

impl Player {
    fn keys(&mut self, rng: &mut Rng) -> Keys {
        if self.left == 0 {
            self.left = 5 + rng.below(46);
            let mut k = 0;
            for (bit, percent) in [
                (Keys::FIRE, 60),
                (Keys::LEFT, 35),
                (Keys::RIGHT, 35),
                (Keys::JUMP, 20),
                (Keys::ROPE, 10),
                (Keys::UP, 25),
                (Keys::DOWN, 20),
                (Keys::CHANGE, 5),
            ] {
                if rng.chance(percent) {
                    k |= bit;
                }
            }
            self.keys = k;
        }
        self.left -= 1;
        Keys(self.keys)
    }
}

fn in_rock(t: &Terrain, x: f32, y: f32) -> bool {
    t.flags(x as i64, y as i64) & PX_ROCK != 0
}

/// A position with no rock in the 9 × 9 square around it, or the map centre.
fn free_spot(t: &Terrain, rng: &mut Rng) -> (i32, i32) {
    for _ in 0..10_000 {
        let (x, y) = (rng.below(t.width() as u64) as i64, rng.below(t.height() as u64) as i64);
        if (y - 4..=y + 4).all(|yy| (x - 4..=x + 4).all(|xx| t.flags(xx, yy) & PX_ROCK == 0)) {
            return (x as i32, y as i32);
        }
    }
    (t.width() as i32 / 2, t.height() as i32 / 2)
}

fn check(w: &World, events: &TickEvents, tick: usize, what: &str) {
    for (i, worm) in w.worms().iter().enumerate() {
        assert!(worm.health <= 100.0, "{what} tick {tick}: worm {i} has health {}", worm.health);
        if worm.alive {
            assert!(
                !in_rock(w.terrain(), worm.pos.x, worm.pos.y),
                "{what} tick {tick}: worm {i} is inside rock at {:?}",
                worm.pos
            );
        }
    }
    assert!(w.projectiles().count() <= rl_sim::ProjectilePool::DEFAULT_CAPACITY, "{what} tick {tick}");
    assert!(w.entity_count() <= 1024, "{what} tick {tick}: entities {}", w.entity_count());
    assert!(w.bonuses().iter().filter(|b| b.used).count() <= MAX_BONUSES);
    assert_eq!(events.cosmetic_dropped(), 0, "{what} tick {tick}: cosmetics were dropped");
    // A dropped `TerrainEdit` would desync a renderer's colour image.
    assert_eq!(events.gameplay_dropped(), 0, "{what} tick {tick}: gameplay events were dropped");
    assert_eq!(events.lost_area(), None, "{what} tick {tick}");
}

/// Applies the tick's terrain edits to `shadow` in order, with the level's
/// stamps (green-dirt shades are cosmetic, so any `rand` will do).
fn replay(shadow: &mut Terrain, holes: &[Stamp], mask: Option<&GreenMask>, events: &TickEvents) {
    for e in events {
        let EventKind::TerrainEdit { op, stamp, x, y } = e.kind else { continue };
        match (op, stamp) {
            (TerrainOp::Carve | TerrainOp::Beam, StampId::Hole(i)) => {
                shadow.carve_hole(&holes[i as usize], x, y);
            }
            (TerrainOp::Dirt, StampId::Hole(i)) => {
                shadow.place_dirt(&holes[i as usize], x, y);
            }
            (TerrainOp::GreenDirt, StampId::GreenBall) => {
                shadow.place_green_dirt(mask.unwrap(), x, y, &mut || 0);
            }
            other => panic!("unexpected edit {other:?}"),
        }
    }
}

fn smoke(mod_dir: &str, level: &str, seed: u64) {
    let Some(content) = content_dir() else { return };
    let content = Path::new(&content);
    let what = format!("{mod_dir} on {level}");
    let content = Content::open(content).unwrap();
    let sim_mod = content.sim_mod(&olx_mod_id(mod_dir)).unwrap();
    let level = content.load_level(level).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let mut rng = Rng(seed);
    let settings = SimSettings { bonuses: true, bonus_frequency: 3.0, ..SimSettings::lx56() };
    let n_weapons = sim_mod.weapons().len() as u64;
    let (holes, mask) = (theme.stamps(), theme.green_mask());
    let terrain = Terrain::from_grid(&level.material);
    // Spawn holes carve no rock, so the free spots are the level's.
    let worms = (0..WORMS)
        .map(|_| {
            let spawn = Some(free_spot(&terrain, &mut rng));
            let slots = (0..5).map(|_| Slot::ready(rng.below(n_weapons) as i32)).collect();
            WormSetup { team: 0, loadout: Loadout::new(slots, &settings), spawn }
        })
        .collect();
    let setup = MatchSetup {
        holes: theme.stamps(),
        green_mask: theme.green_mask(),
        seed: seed as u32,
        worms,
        ..MatchSetup::new(settings, Arc::new(sim_mod), terrain)
    };
    let mut w = World::new(setup, Box::new(OracleMode)).unwrap();
    w.align_physics_clock();
    let mut shadow = w.terrain().clone();
    let mut players: Vec<Player> = (0..WORMS).map(|_| Player { keys: 0, left: 0 }).collect();
    let (mut fired, mut explosions, mut kills) = (0, 0, 0);
    let mut twin: Option<World> = None;
    let (mut events, mut twin_events) = (TickEvents::new(), TickEvents::new());
    for tick in 0..TICKS {
        if tick == CLONE_AT {
            twin = Some(w.clone());
        }
        let keys: Vec<Keys> = players.iter_mut().map(|p| p.keys(&mut rng)).collect();
        w.tick(&keys, &mut events);
        for e in &events {
            assert!(e.substep <= w.substeps_last_tick(), "{what} tick {tick}: {e:?}");
            match e.kind {
                EventKind::WeaponFired { .. } => fired += 1,
                EventKind::Explosion { .. } => explosions += 1,
                EventKind::Kill { .. } => kills += 1,
                _ => {}
            }
        }
        check(&w, &events, tick, &what);
        replay(&mut shadow, &holes, mask.as_ref(), &events);
        if tick % 100 == 99 || tick == TICKS - 1 {
            let same = shadow.material_hash() == w.terrain().material_hash();
            assert!(same, "{what} tick {tick}: the replayed edits differ from the material");
        }
        if let Some(t) = &mut twin {
            t.tick(&keys, &mut twin_events);
            assert!(twin_events == events, "{what} tick {tick}: the clone's events differ");
            assert_eq!(t.checksum(), w.checksum(), "{what} tick {tick}: the clone's checksum differs");
        }
    }
    eprintln!("{what}: {fired} shots, {explosions} explosions, {kills} kills");
    assert!(fired > 0 && explosions > 0, "{what}: nothing happened");
}

macro_rules! smoke_tests {
    ($($name:ident: $mod_dir:literal, $level:literal, $seed:literal;)*) => {
        $(
            #[test]
            fn $name() {
                smoke($mod_dir, $level, $seed);
            }
        )*
    };
}

smoke_tests! {
    classic_on_castlestrike: "Classic", "olx-level-castlestrike", 1;
    classic_on_dirt_level: "Classic", "olx-level-dirt-level", 2;
    classic_on_fossilfacility: "Classic", "olx-level-fossilfacility", 3;
    classic_on_liero_factory: "Classic", "olx-level-liero-factory", 4;
    mw_on_castlestrike: "MW 1.0", "olx-level-castlestrike", 5;
    mw_on_dirt_level: "MW 1.0", "olx-level-dirt-level", 6;
    mw_on_fossilfacility: "MW 1.0", "olx-level-fossilfacility", 7;
    mw_on_liero_factory: "MW 1.0", "olx-level-liero-factory", 8;
}
