//! The playtest's floating pixels: in a seeded bot match played as local
//! play presents it, no effect leaves colour on open air. Every background
//! cell shows the level's back image: a gib stamps only rock and dirt in
//! play (OLX stamps the air around it too, kept with `olx_quirks`).

use crate::common::*;
use olr_cli::assets::LevelAssets;
use olr_cli::runner::{Match, Options};
use olr_cli::spec::{MatchSpec, Personality};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{SceneOptions, Viewport};

/// The art pixels of background cells that differ from `back`.
fn stray(present: &Presentation, back: &olr_data::image::RgbaImage) -> Vec<(u32, u32)> {
    let c = present.color();
    let (s, px) = (c.art_scale(), c.pixels());
    let t = c.material();
    let mut out = Vec::new();
    for y in 0..t.height() {
        for x in 0..t.width() {
            if t.material(x as i64, y as i64) != Some(olr_data::level::Material::Background) {
                continue;
            }
            // A cell's top-left art pixel: a stamp or stain covers whole cells.
            let (ax, ay) = (x * s, y * s);
            let i = ((ay * c.width() + ax) * 4) as usize;
            let b = back.get(ax, ay);
            if px[i..i + 3] != [b.r, b.g, b.b] {
                out.push((x, y));
            }
        }
    }
    out
}

#[test]
fn no_effect_leaves_colour_on_open_air() {
    let Some(c) = content() else { return };
    let spec = MatchSpec {
        personality: Personality::Chaos,
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, "olx-level-dirt-level", 4, 1, 3000)
    };
    let assets = LevelAssets::load(&c, &spec.mod_id, &spec.level_id).unwrap();
    let mut m = Match::with_assets(&assets, &spec, &Options::default()).unwrap();
    let setup = PresentSetup {
        core: c.core(),
        m: &assets.m,
        level: &assets.level,
        theme: &assets.theme,
        viewport: Viewport::GAME,
        options: SceneOptions::default(),
        seed: spec.seed,
    };
    let mut present = Presentation::new(&setup, m.world());
    let skin = c.skin_or_default("olx-skin-default");
    for i in 0..m.world().worms().len() {
        present.add_worm(&skin, default_colour(i), i == 0, false);
    }
    let mut deaths = 0;
    for _ in 0..spec.ticks {
        m.advance();
        deaths += m
            .events()
            .as_slice()
            .iter()
            .filter(|e| matches!(e.kind, olr_sim::EventKind::Kill { .. }))
            .count();
        present.on_tick(m.world(), m.events());
    }
    assert!(deaths >= 5, "{deaths} deaths: the match left gibs");
    let left = stray(&present, &assets.level.back);
    assert!(left.is_empty(), "{} background cells keep colour, such as {:?}", left.len(), left.first());
}
