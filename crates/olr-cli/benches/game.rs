//! Match benchmarks on the imported OLX content (spec §7, §21 G8): eight
//! of OLX's bots thinking and the world ticking, and one frame's draw list
//! built by the presentation (scene and cosmetic entities), both on a
//! Classic Deathmatch on Liero Factory 20 s in. They need `OLR_CONTENT_DIR`
//! (`cargo xtask bench` sets it): without it they print a notice and
//! measure nothing.

use std::path::PathBuf;

use olr_bots::Bot;
use olr_cli::spec::MatchSpec;
use olr_content::Content;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use olr_sim::{Keys, TickEvents, World};

/// Ticks played before measuring (20 s: the bots spread out, fight, dig).
const WARM_TICKS: u32 = 2000;

fn content() -> Option<PathBuf> {
    std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir())
}

fn main() {
    match content() {
        Some(_) => divan::main(),
        None => println!("skip olr-cli benches: OLR_CONTENT_DIR not set"),
    }
}

/// The match after [`WARM_TICKS`]: the world, its bots, the last tick's
/// events, and its presentation (which saw every tick).
struct Warm {
    world: World,
    bots: Vec<Bot>,
    events: TickEvents,
    present: Presentation,
}

fn warm() -> Warm {
    let content = Content::open(&content().expect("checked in main")).expect("the content");
    let mut spec = MatchSpec::new("olx-classic", "olx-level-liero-factory", 8, 5, WARM_TICKS);
    spec.time_limit = 0.0;
    spec.kill_limit = 0;
    let built = content.build_match(&spec.match_options()).expect("the match");
    let (m, level) = (content.load_mod(&spec.mod_id).unwrap(), content.load_level(&spec.level_id).unwrap());
    let theme = content.load_theme(&level.theme).unwrap();
    let skin = content.skin_or_default("olx-skin-default");
    let setup = PresentSetup {
        core: content.core(),
        m: &m,
        level: &level,
        theme: &theme,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
        options: SceneOptions { interpolation: Interpolation::Linear, show_health_bars: false },
        seed: 5,
    };
    let (mut world, mut bots) = (built.world, built.bots);
    let mut present = Presentation::new(&setup, &world);
    for i in 0..world.worms().len() {
        present.add_worm(&skin, default_colour(i), i == 0, false);
    }
    let mut events = TickEvents::new();
    let mut keys = vec![Keys(0); bots.len()];
    for _ in 0..WARM_TICKS {
        for (i, b) in bots.iter_mut().enumerate() {
            keys[i] = b.think(&world, &events);
        }
        world.tick(&keys, &mut events);
        present.on_tick(&world, &events);
    }
    Warm { world, bots, events, present }
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
