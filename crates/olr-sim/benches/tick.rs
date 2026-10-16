//! `World::tick` benchmarks (spec §7, §21 G8), on a mod and level made
//! here so they need no imported content: a tick with OLX's projectile
//! pool full (3000 bouncing pellets), and a tick of 32 worms walking and
//! digging with nothing in the air. `cargo xtask bench` runs them and
//! compares the medians with `fixtures/bench/baselines.ron`.

use olr_sim::{Keys, TickEvents, World};

#[path = "../tests/common/world.rs"]
mod world;

use world::world;

fn main() {
    divan::main();
}

/// Eight worms firing until OLX's pool (3000) is full, then still firing.
fn full_pool() -> (World, Vec<Keys>) {
    let mut w = world(8, 640, 400);
    let keys = vec![Keys(Keys::FIRE | Keys::UP); 8];
    let mut ev = TickEvents::new();
    for _ in 0..3000 {
        w.tick(&keys, &mut ev);
        if w.projectiles().count() >= w.projectile_capacity() {
            break;
        }
    }
    assert_eq!(w.projectiles().count(), w.projectile_capacity(), "the pool fills");
    (w, keys)
}

/// One tick with the projectile pool full (3000 projectiles).
#[divan::bench(sample_count = 60)]
fn tick_full_pool(b: divan::Bencher) {
    let (w, keys) = full_pool();
    b.with_inputs(|| (w.clone(), TickEvents::new())).bench_local_refs(|(w, ev)| w.tick(&keys, ev));
}

/// One tick of 32 worms walking and digging (a 32-player match's worms
/// alone).
#[divan::bench(sample_count = 60, sample_size = 50)]
fn tick_32_worms(b: divan::Bencher) {
    let mut w = world(32, 1200, 500);
    let mut ev = TickEvents::new();
    let walk = |t: u32| vec![Keys(if (t / 50).is_multiple_of(2) { Keys::RIGHT } else { Keys::LEFT }); 32];
    for t in 0..200 {
        w.tick(&walk(t), &mut ev);
    }
    let keys = walk(1);
    b.with_inputs(|| (w.clone(), TickEvents::new())).bench_local_refs(|(w, ev)| w.tick(&keys, ev));
}

/// One snapshot and one restore of a Castle-Strike-sized world with rollback on.
#[divan::bench]
fn save_restore(b: divan::Bencher) {
    let mut w = world(16, 1350, 550);
    w.enable_rollback();
    let mut ev = TickEvents::new();
    b.bench_local(|| {
        let s = w.save();
        w.tick(&[Keys(Keys::FIRE); 16], &mut ev);
        w.restore(&s).unwrap();
    });
}
