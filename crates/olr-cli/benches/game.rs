//! Match benchmarks on the imported OLX content (spec §7, §21 G8): eight
//! of OLX's bots thinking and the world ticking, and one frame's draw list
//! built by the presentation (scene and cosmetic entities), both on a
//! Classic Deathmatch on Liero Factory 20 s in. They need `OLR_CONTENT_DIR`
//! (`cargo xtask bench` sets it): without it they print a notice and
//! measure nothing.

use std::path::PathBuf;

use olr_cli::quick_bench::{self, Warm};
use olr_content::Content;
use olr_scene::DrawList;
use olr_sim::Keys;

fn content() -> Option<PathBuf> {
    std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir())
}

fn main() {
    match content() {
        Some(_) => divan::main(),
        None => println!("skip olr-cli benches: OLR_CONTENT_DIR not set"),
    }
}

/// The match after `quick_bench::WARM_TICKS` (`olr_cli::quick_bench::warm`,
/// shared with `olr-cli quick-bench`).
fn warm() -> Warm {
    let content = Content::open(&content().expect("checked in main")).expect("the content");
    quick_bench::warm(&content).expect("the warm match")
}

/// Eight bots think, then the world ticks (one tick of `olr-cli match`).
#[divan::bench(sample_count = 100, sample_size = 20)]
fn bots_8_tick(b: divan::Bencher) {
    let w = warm();
    let mut keys = vec![Keys(0); w.bots.len()];
    b.with_inputs(|| (w.world.clone(), w.bots.clone(), w.events.clone())).bench_local_refs(
        |(world, bots, events)| {
            for (i, bot) in bots.iter_mut().enumerate() {
                keys[i] = bot.think(world, events);
            }
            world.tick(&keys, events);
        },
    );
}

/// One frame's draw list: the scene and the cosmetic entities.
#[divan::bench(sample_count = 100, sample_size = 100)]
fn drawlist_build(b: divan::Bencher) {
    let mut w = warm();
    let mut list = DrawList::new();
    b.bench_local(|| w.present.build(&w.world, 0.5, &mut list));
}
