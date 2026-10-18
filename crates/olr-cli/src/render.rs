//! `olr-cli render`: one frame of an oracle scenario as a PNG, drawn the way
//! the game draws it (scene, cosmetic entities, colour terrain, wgpu
//! offscreen), so agents can see what a change does.

use std::path::PathBuf;

use anyhow::{Context, ensure};
use olr_content::{Content, olx_level_id, olx_mod_id};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_render::{Offscreen, RenderSettings, UpscaleFilter};
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};

/// The viewport's aspect: OLX's classic 4:3 (320 × 240 world pixels).
const ASPECT: f32 = 4.0 / 3.0;

#[derive(Clone, Copy, clap::ValueEnum)]
enum Filter {
    IntegerNearest,
    SharpBilinear,
}

/// Renders one frame of an oracle scenario offscreen.
#[derive(clap::Args)]
pub struct Args {
    /// Scenario name (`<fixtures>/<name>.ron`), such as `rope` or `weapons/classic/bazooka`.
    name: String,
    /// The frame to draw: the state after that frame's tick (-1 is the start).
    #[arg(long, allow_hyphen_values = true)]
    frame: i64,
    /// The PNG to write.
    #[arg(long)]
    out: PathBuf,
    /// Output pixels per world pixel (2 shows OLX's 2× art pixel for pixel).
    #[arg(long, default_value_t = 2.0)]
    scale: f32,
    /// The upscale filter.
    #[arg(long, value_enum, default_value = "sharp-bilinear")]
    filter: Filter,
    /// Draw between the previous tick (0) and the frame (1), interpolated;
    /// without it, the frame's state at whole pixels, as OLX draws.
    #[arg(long)]
    alpha: Option<f32>,
    /// Imported content (packs written by `olr-cli import`).
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// Directory with the scenarios and OLX's traces.
    #[arg(long, default_value = "fixtures/oracle")]
    fixtures: PathBuf,
}

/// What the frame needs: its draw list, and the presentation (images and
/// colour terrain) as it was at the frame.
struct Frame {
    list: DrawList,
    present: Presentation,
    viewport: Viewport,
}

/// Plays `args.name` up to `args.frame` with a presentation following
/// worm 0 (every worm in OLX's default skin and colours).
fn build(args: &Args) -> anyhow::Result<Frame> {
    let content = Content::open(&args.content)?;
    let (sc, trace) = olr_scenario::load_fixture(&args.fixtures, &args.name)?;
    let last = i64::from(sc.frames) - 1;
    ensure!((-1..=last).contains(&args.frame), "{} has frames -1 to {last}, not {}", args.name, args.frame);
    let level = content.load_level(&olx_level_id(&sc.level))?;
    let theme = content.load_theme(&level.theme).with_context(|| format!("level {}", sc.level))?;
    let m = content.load_mod(&olx_mod_id(&sc.mod_dir))?;
    let skin = content.skin_or_default("olx-skin-default");
    for w in content.warnings().iter().chain(&m.warnings).chain(&theme.warnings).chain(&skin.warnings) {
        eprintln!("warning: {w}");
    }
    let viewport = Viewport::from_aspect(ASPECT, ASPECT);
    let interpolation = if args.alpha.is_some() { Interpolation::Linear } else { Interpolation::Latest };
    let setup = PresentSetup {
        core: content.core(),
        m: &m,
        level: &level,
        theme: &theme,
        viewport,
        options: SceneOptions { interpolation, show_health_bars: false },
        seed: 1,
    };
    let mut present: Option<Presentation> = None;
    let mut list = DrawList::new();
    olr_scenario::play(&content, &sc, &trace, |world, f, events| {
        if f > args.frame {
            return;
        }
        let p = present.get_or_insert_with(|| {
            let mut p = Presentation::new(&setup, world);
            for i in 0..sc.worms.len() {
                p.add_worm(&skin, default_colour(i), i == 0, i == 0);
            }
            p
        });
        if f >= 0 {
            p.on_tick(world, events);
        }
        if f == args.frame {
            p.build(world, args.alpha.unwrap_or(1.0), &mut list);
        }
    })?;
    let present = present.context("the scenario has no start")?;
    Ok(Frame { list, present, viewport })
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    ensure!(args.scale.is_finite() && args.scale > 0.0 && args.scale <= 16.0, "--scale must be in (0, 16]");
    let mut frame = build(args)?;
    let gpu = Offscreen::new()?;
    let filter = match args.filter {
        Filter::IntegerNearest => UpscaleFilter::IntegerNearest,
        Filter::SharpBilinear => UpscaleFilter::SharpBilinear,
    };
    let mut renderer = gpu.renderer(RenderSettings { filter });
    frame.present.upload(&mut renderer);
    let px = |world: i32| ((world as f32 * args.scale).round() as u32).max(1);
    let size = (px(frame.viewport.width), px(frame.viewport.height));
    let img = gpu.render_lit_to_image(&mut renderer, &frame.list, frame.present.look(), size);
    std::fs::write(&args.out, olr_pack::image_io::encode_rgba(&img))
        .with_context(|| format!("writing {}", args.out.display()))?;
    for w in renderer.warnings().drain(..) {
        eprintln!("warning: {w}");
    }
    let info = gpu.adapter_info();
    println!(
        "wrote {} ({}x{}, {} frame {}; {} via {:?})",
        args.out.display(),
        size.0,
        size.1,
        args.name,
        args.frame,
        info.name,
        info.backend
    );
    Ok(true)
}
