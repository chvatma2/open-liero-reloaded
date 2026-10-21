//! `olr-cli look-preview <pack> --at X,Y [--time MS] [--view lit|full|light]
//! [--size WxH] [--content DIR] --out f.png` (look system, plan 15a Task 13):
//! a level drawn with its look and no match, centred on a world position at
//! a clock time, through the real renderer offscreen. The fast loop for
//! agents and the look goldens.

use std::path::PathBuf;

use anyhow::Context as _;
use clap::ValueEnum;
use olr_content::{Content, LoadedLevel};
use olr_look::{LookFrame, ViewMode};
use olr_render::{Offscreen, RenderSettings, UpscaleFilter};
use olr_scene::{DrawList, View, Viewport};
use olr_vfs::DirFs;

use crate::level_cmd::parse_point;
use crate::pack_cmd::parse_size;

#[derive(Clone, Copy, ValueEnum)]
pub enum ViewArg {
    Lit,
    Full,
    Light,
}

#[derive(clap::Args)]
pub struct Args {
    /// A level pack directory.
    pack: PathBuf,
    /// The view's centre, world pixels X,Y.
    #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
    at: (i32, i32),
    /// The look's clock, milliseconds.
    #[arg(long, default_value_t = 0)]
    time: u64,
    #[arg(long, value_enum, default_value = "lit")]
    view: ViewArg,
    /// The picture's size in pixels (the game's 427 × 192 view, integer scaled).
    #[arg(long, value_parser = parse_size, default_value = "854x384")]
    size: (u32, u32),
    /// Where the level's theme is.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    let src = olr_format::levels::read_level_files(&DirFs::new(&args.pack))
        .map_err(|e| anyhow::anyhow!("{}: {e}", args.pack.display()))?;
    let level = LoadedLevel::from_source(&src).with_context(|| args.pack.display().to_string())?;
    let content = Content::open_bundled(&args.content).with_context(|| args.content.display().to_string())?;
    let theme = content.load_theme(&level.theme)?;
    let map = level.color_map(&theme);
    let vp = Viewport::GAME;
    let (w, h) = (vp.width as f32, vp.height as f32);
    let mut list = DrawList::new();
    list.view = View { x: args.at.0 as f32 - w / 2.0, y: args.at.1 as f32 - h / 2.0, w, h };
    list.map = (map.material().width(), map.material().height());
    let mode = match args.view {
        ViewArg::Lit => ViewMode::Lit,
        ViewArg::Full => ViewMode::FullBright,
        ViewArg::Light => ViewMode::LightOnly,
    };
    let mut look = LookFrame { mode, ..LookFrame::default() };
    look.begin(level.look.as_ref(), args.time as f64);
    let gpu = Offscreen::new()?;
    let mut renderer = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    renderer.set_lighting(true);
    renderer.upload_terrain(&map);
    renderer.set_distant(&level.distant);
    let img = gpu.render_lit_to_image(&mut renderer, &list, Some(&look), args.size);
    std::fs::write(&args.out, olr_pack::image_io::encode_rgba(&img))
        .with_context(|| format!("writing {}", args.out.display()))?;
    for w in renderer.warnings().drain(..) {
        eprintln!("warning: {w}");
    }
    Ok(true)
}
