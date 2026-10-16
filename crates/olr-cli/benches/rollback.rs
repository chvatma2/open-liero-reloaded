//! The rollback benchmark (plan 9, networking spec §3.3): a Classic match
//! of 16 bots on Castle Strike, 600 ticks in, restoring the snapshot taken
//! 12 ticks back and re-simulating those 12 ticks with their recorded keys.
//! It needs `OLR_CONTENT_DIR` (`cargo xtask bench` sets it): without it a
//! notice is printed and nothing is measured.

use std::path::PathBuf;

use olr_cli::spec::MatchSpec;
use olr_content::Content;
use olr_sim::{Keys, Snapshot, TickEvents, World};

/// Ticks played (bots thinking) before the measured window.
const WARM_TICKS: u32 = 588;
/// The rolled-back ticks.
const DEPTH: usize = 12;
const WORMS: usize = 16;

fn content() -> Option<PathBuf> {
    std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir())
}

fn main() {
    match content() {
        Some(_) => divan::main(),
        None => println!("skip olr-cli rollback bench: OLR_CONTENT_DIR not set"),
    }
}

/// The world at the end of the window, the snapshot at its start and the
/// window's keys, one row per tick.
fn window() -> (World, Snapshot, Vec<Keys>) {
    let content = Content::open(&content().expect("checked in main")).expect("the content");
    let mut spec =
        MatchSpec::new("olx-classic", "olx-level-castlestrike", WORMS, 1, WARM_TICKS + DEPTH as u32);
    spec.time_limit = 0.0;
    spec.kill_limit = 0;
    let built = content.build_match(&spec.match_options()).expect("the match");
    let (mut world, mut bots) = (built.world, built.bots);
    world.enable_rollback();
    let (mut events, mut keys, mut rows) = (TickEvents::new(), vec![Keys(0); bots.len()], Vec::new());
    let mut play = |world: &mut World, record: bool| {
        for (i, b) in bots.iter_mut().enumerate() {
            keys[i] = b.think(world, &events);
        }
        world.tick(&keys, &mut events);
        if record {
            rows.extend_from_slice(&keys);
        }
    };
    for _ in 0..WARM_TICKS {
        play(&mut world, false);
    }
    let snap = world.save();
    for _ in 0..DEPTH {
        play(&mut world, true);
    }
    (world, snap, rows)
}

/// Restore, then 12 ticks with the recorded keys (the bots do not think).
#[divan::bench(sample_count = 100, sample_size = 10)]
fn rollback_12_castlestrike(b: divan::Bencher) {
    let (mut world, snap, rows) = window();
    let mut events = TickEvents::new();
    b.bench_local(|| {
        world.restore(&snap).expect("the snapshot restores");
        for row in rows.chunks(WORMS) {
            world.tick(row, &mut events);
        }
    });
}
