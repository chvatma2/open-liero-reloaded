//! `olr-cli render-match`: the last frame of a headless bot match (its
//! local players', `--locals`, too: two or more get a view each, split
//! screen as the game lays it out on a 16:9 window, issue #30) as a PNG,
//! drawn the way the game draws it (its look included), so agents can see
//! a match on any packs, such as the look test level and mod (plan 15a
//! Task 16), which no OLX trace can replay. `--live COPY=STATE` sets a
//! live copy's state before the first tick (issue #87).

use std::path::PathBuf;

use anyhow::{Context, ensure};
use olr_cli::assets::LevelAssets;
use olr_cli::runner::{Match, Options};
use olr_cli::spec::MatchSpec;
use olr_content::Content;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{Offscreen, RenderSettings, UpscaleFilter, ViewDraw};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};

/// Plays a headless bot match and renders its last tick offscreen.
#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    spec: MatchSpec,
    /// The PNG to write.
    #[arg(long)]
    out: PathBuf,
    /// Output pixels per world pixel.
    #[arg(long, default_value_t = 2.0)]
    scale: f32,
    /// The skin every worm wears: a skin pack id or OLX name (issue #12:
    /// a double-resolution skin draws at full resolution).
    #[arg(long, default_value = "olx-skin-default")]
    skin: String,
    /// Pack directories laid over `--content` (the first wins), such as
    /// `fixtures/look/mods`.
    #[arg(long)]
    packs: Vec<PathBuf>,
    /// Imported content (packs written by `olr-cli import`), OLR's own packs
    /// (the `reloaded` directory beside it) over it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// A live copy's state before the first tick, `COPY=STATE` (copy id,
    /// state name; issue #87), repeatable, in order.
    #[arg(long, value_name = "COPY=STATE", value_parser = live_state)]
    live: Vec<(String, String)>,
}

/// `--live`'s `COPY=STATE`.
fn live_state(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((copy, state)) if !copy.is_empty() && !state.is_empty() => Ok((copy.into(), state.into())),
        _ => Err(format!("{s}: expected COPY=STATE")),
    }
}

/// The seam between split screen's views, in world pixels: OLX's 4
/// screen pixels at its 2 screen pixels a world pixel
/// (`CClient.cpp:1653-1677`).
const SEAM: i32 = 2;

/// Where `n` views of `view` pixels go, `seam` pixels apart, and the
/// picture's size: one alone; two stacked; three or four in a 2 × 2 grid,
/// the empty quarter black (the game's arrangements on a 16:9 window,
/// `olr_ui::hud::SplitLayout`).
fn split_areas(n: usize, view: (u32, u32), seam: u32) -> (Vec<[u32; 4]>, (u32, u32)) {
    let (cols, rows) = match n {
        0 | 1 => (1, 1),
        2 => (1, 2),
        _ => (2, 2),
    };
    let areas = (0..n.max(1) as u32)
        .map(|i| [(i % cols) * (view.0 + seam), (i / cols) * (view.1 + seam), view.0, view.1])
        .collect();
    (areas, (cols * view.0 + (cols - 1) * seam, rows * view.1 + (rows - 1) * seam))
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    ensure!(args.scale.is_finite() && args.scale > 0.0 && args.scale <= 16.0, "--scale must be in (0, 16]");
    // The `--packs` directories, then the bundled content (OLR's own packs over `--content`).
    let bundled = olr_content::bundled_roots(&args.content);
    let roots: Vec<&std::path::Path> = args.packs.iter().chain(&bundled).map(PathBuf::as_path).collect();
    let content = Content::open_layers(&roots).context("opening the content")?;
    let assets = LevelAssets::load(&content, &args.spec.mod_id, &args.spec.level_id)?;
    for w in assets.m.warnings.iter().chain(&assets.level.warnings).chain(&assets.theme.warnings) {
        eprintln!("warning: {w}");
    }
    let mut game = Match::with_assets(&assets, &args.spec, &Options::default())?;
    for (copy, state) in &args.live {
        game.set_live_state(copy, state).context("--live")?;
    }
    let viewport = Viewport::GAME;
    let setup = PresentSetup {
        core: content.core(),
        m: &assets.m,
        level: &assets.level,
        theme: &assets.theme,
        viewport,
        options: SceneOptions { interpolation: Interpolation::Latest, show_health_bars: false },
        seed: args.spec.seed,
    };
    let mut present = Presentation::new(&setup, game.world());
    let id =
        content.skin_id(&args.skin).with_context(|| format!("--skin {}: no such skin pack", args.skin))?;
    let skin = content.load_skin(&id)?;
    // Each local player's worm has its crosshair (issue #29); without
    // any, worm 0's, as the game draws a game nobody plays.
    let locals = args.spec.locals;
    for i in 0..game.world().worms().len() {
        present.add_worm(&skin, default_colour(i), i < locals.max(1), i < locals);
    }
    // Split screen (issue #30): a view on each local player's worm.
    if locals >= 2 {
        let ids: Vec<i32> = game.world().worms().iter().take(locals).map(|w| w.id).collect();
        present.set_views(&ids);
    }
    // `Match::ticks` counts settled ticks; this plays without settling.
    for _ in 0..args.spec.ticks {
        game.advance();
        present.on_tick(game.world(), game.events());
    }
    let mut lists: Vec<DrawList> = (0..present.views()).map(|_| DrawList::new()).collect();
    present.build_views(game.world(), 1.0, &mut lists);
    let gpu = Offscreen::new()?;
    let mut renderer = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    present.upload(&mut renderer);
    let px = |world: i32| ((world as f32 * args.scale).round() as u32).max(1);
    let view = (px(viewport.width), px(viewport.height));
    let (areas, size) = split_areas(lists.len(), view, px(SEAM));
    let draws: Vec<ViewDraw> =
        lists.iter().zip(&areas).map(|(list, &area)| ViewDraw { list, area }).collect();
    let img = gpu.render_views_to_image(&mut renderer, &draws, present.look(), size);
    std::fs::write(&args.out, olr_pack::image_io::encode_rgba(&img))
        .with_context(|| format!("writing {}", args.out.display()))?;
    let discs = present.look().map_or(0, |l| l.emitters.len());
    let views = if draws.len() > 1 { format!(", {} views", draws.len()) } else { String::new() };
    println!(
        "wrote {} ({}x{}{views}, tick {}, {discs} glowing discs)",
        args.out.display(),
        size.0,
        size.1,
        args.spec.ticks
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    /// One view alone; two stacked; three and four in a 2 × 2 grid, a seam
    /// apart.
    #[test]
    fn views_are_laid_out_as_the_game_does_at_16_9() {
        let v = (854, 384);
        assert_eq!(super::split_areas(1, v, 4), (vec![[0, 0, 854, 384]], (854, 384)));
        assert_eq!(super::split_areas(2, v, 4), (vec![[0, 0, 854, 384], [0, 388, 854, 384]], (854, 772)));
        let (four, size) = super::split_areas(4, v, 4);
        assert_eq!(size, (1712, 772));
        assert_eq!(four, [[0, 0, 854, 384], [858, 0, 854, 384], [0, 388, 854, 384], [858, 388, 854, 384]]);
        assert_eq!(super::split_areas(3, v, 4).1, size, "the fourth quarter empty");
    }
}
