//! `olr-cli render-match`: the last frame of a headless bot match as a PNG,
//! drawn the way the game draws it (its look included), so agents can see
//! a match on any packs, such as the look test level and mod (plan 15a
//! Task 16), which no OLX trace can replay.

use std::path::PathBuf;

use anyhow::{Context, ensure};
use olr_cli::assets::LevelAssets;
use olr_cli::runner::{Match, Options};
use olr_cli::spec::MatchSpec;
use olr_content::Content;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{Offscreen, RenderSettings, UpscaleFilter};
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
    /// Pack directories laid over `--content` (the first wins), such as
    /// `fixtures/look/mods`.
    #[arg(long)]
    packs: Vec<PathBuf>,
    /// Imported content (packs written by `olr-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    ensure!(args.scale.is_finite() && args.scale > 0.0 && args.scale <= 16.0, "--scale must be in (0, 16]");
    let mut roots: Vec<&std::path::Path> = args.packs.iter().map(PathBuf::as_path).collect();
    roots.push(&args.content);
    let content = Content::open_layers(&roots).context("opening the content")?;
    let assets = LevelAssets::load(&content, &args.spec.mod_id, &args.spec.level_id)?;
    for w in assets.m.warnings.iter().chain(&assets.level.warnings).chain(&assets.theme.warnings) {
        eprintln!("warning: {w}");
    }
    let mut game = Match::with_assets(&assets, &args.spec, &Options::default())?;
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
    let skin = content.skin_or_default("olx-skin-default");
    for i in 0..game.world().worms().len() {
        present.add_worm(&skin, default_colour(i), i == 0, false);
    }
    // `Match::ticks` counts settled ticks; this plays without settling.
    for _ in 0..args.spec.ticks {
        game.advance();
        present.on_tick(game.world(), game.events());
    }
    let mut list = DrawList::new();
    present.build(game.world(), 1.0, &mut list);
    let gpu = Offscreen::new()?;
    let mut renderer = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    present.upload(&mut renderer);
    let px = |world: i32| ((world as f32 * args.scale).round() as u32).max(1);
    let size = (px(viewport.width), px(viewport.height));
    let img = gpu.render_lit_to_image(&mut renderer, &list, present.look(), size);
    std::fs::write(&args.out, olr_pack::image_io::encode_rgba(&img))
        .with_context(|| format!("writing {}", args.out.display()))?;
    let lights = present.look().map_or(0, |l| l.lights.len());
    println!(
        "wrote {} ({}x{}, tick {}, {lights} lights)",
        args.out.display(),
        size.0,
        size.1,
        args.spec.ticks
    );
    Ok(true)
}
