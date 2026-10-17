//! `olr-cli level …`: the level operations on a pack (plan 13d ruling R3),
//! and a preview to PNG. Each operation loads the pack, applies the
//! operation, refreshes the thumbnail and saves through
//! `olr_edit::pack::save` (the pack on disk is the old one or the new one).
//! Every argument is parsed strictly: a bad value is a clap error naming
//! its flag.

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::level::{self, Layer, Paint, Shape, Solid};
use olr_edit::pack::{Pack, Source, load, save};
use olr_format::image_io::{IMAGES, LEVEL_ART, check_size, decode_rgba_within, encode_rgba};
use olr_format::levels::{LevelObject, LevelSource, MATERIAL_PALETTE};

use crate::pack_cmd::parse_size;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: LevelCommand,
}

#[derive(Subcommand)]
enum LevelCommand {
    /// Carve: the material becomes background, and the front shows the back.
    Carve {
        pack: PathBuf,
        #[command(flatten)]
        shape: ShapeArgs,
    },
    /// Lay rock or dirt and paint the front over it.
    Fill {
        pack: PathBuf,
        #[command(flatten)]
        shape: ShapeArgs,
        #[arg(long)]
        material: SolidArg,
        #[command(flatten)]
        paint: PaintArgs,
    },
    /// Paint an art layer; the back shows through where the material is background.
    Paint {
        pack: PathBuf,
        #[command(flatten)]
        shape: ShapeArgs,
        #[arg(long)]
        layer: LayerArg,
        #[command(flatten)]
        paint: PaintArgs,
    },
    /// Alpha-composite an image onto an art layer, its top-left at world X,Y.
    Stamp {
        pack: PathBuf,
        /// World pixels X,Y.
        #[arg(long, value_parser = parse_point)]
        at: (i32, i32),
        /// A PNG, in art pixels.
        #[arg(long, value_parser = parse_image)]
        image: RgbaImage,
        #[arg(long)]
        layer: LayerArg,
    },
    /// Resize the level; objects that end up outside are removed and printed.
    Resize {
        pack: PathBuf,
        /// The new size in world pixels, WxH.
        #[arg(long, value_parser = parse_size)]
        size: (u32, u32),
        /// Where the old content's top-left lands, DX,DY.
        #[arg(long, value_parser = parse_point, default_value = "0,0")]
        offset: (i32, i32),
        /// The new area's material.
        #[arg(long, default_value = "background")]
        material: MaterialArg,
        /// The new area's colour, front and back, RRGGBB.
        #[arg(long, value_parser = parse_colour, default_value = "000000")]
        colour: Rgba,
    },
    /// The level's objects: list, add, move, remove.
    Object {
        #[command(subcommand)]
        command: ObjectCommand,
    },
    /// Draw the level (front, or with --material its material) to a PNG with
    /// each spawn marked; the pack is not changed.
    Preview {
        pack: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Output pixels per art pixel, 1 to 8.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=8))]
        scale: u32,
        /// Draw the material instead of the front art.
        #[arg(long)]
        material: bool,
    },
}

#[derive(Subcommand)]
enum ObjectCommand {
    /// Print each object as `<index>: Spawn at (x, y)[ — notes]`.
    List { pack: PathBuf },
    /// Add a spawn point at world X,Y.
    Add {
        pack: PathBuf,
        #[arg(long, value_parser = parse_point)]
        at: (i32, i32),
        #[arg(long, default_value = "")]
        notes: String,
    },
    /// Move the object at INDEX to world X,Y.
    Move {
        pack: PathBuf,
        index: usize,
        #[arg(long, value_parser = parse_point)]
        at: (i32, i32),
    },
    /// Remove the object at INDEX.
    Remove { pack: PathBuf, index: usize },
}

/// One shape, in world pixels.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct ShapeArgs {
    /// A rectangle X,Y,W,H.
    #[arg(long, value_parser = parse_ints::<4>)]
    rect: Option<[i32; 4]>,
    /// A circle X,Y,R.
    #[arg(long, value_parser = parse_ints::<3>)]
    circle: Option<[i32; 3]>,
}

impl ShapeArgs {
    fn shape(&self) -> Shape {
        match (self.rect, self.circle) {
            (Some([x, y, w, h]), _) => Shape::Rect { x, y, w, h },
            (None, Some([cx, cy, r])) => Shape::Circle { cx, cy, r },
            // clap's group requires one.
            (None, None) => Shape::Rect { x: 0, y: 0, w: 0, h: 0 },
        }
    }
}

/// One paint.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct PaintArgs {
    /// A colour, RRGGBB (a leading # is allowed).
    #[arg(long, value_parser = parse_colour)]
    colour: Option<Rgba>,
    /// A PNG repeated from the art origin.
    #[arg(long, value_parser = parse_image)]
    tile: Option<RgbaImage>,
}

impl PaintArgs {
    fn paint(&self) -> Paint {
        match (&self.tile, self.colour) {
            (Some(t), _) => Paint::Tile(t.clone()),
            (None, Some(c)) => Paint::Colour(c),
            (None, None) => Paint::Tile(RgbaImage::new(0, 0)),
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum SolidArg {
    Rock,
    Dirt,
}

#[derive(Clone, Copy, ValueEnum)]
enum MaterialArg {
    Rock,
    Dirt,
    Background,
}

#[derive(Clone, Copy, ValueEnum)]
enum LayerArg {
    Front,
    Back,
}

impl LayerArg {
    fn layer(self) -> Layer {
        match self {
            LayerArg::Front => Layer::Front,
            LayerArg::Back => Layer::Back,
        }
    }
}

/// `N` comma-separated whole numbers.
fn parse_ints<const N: usize>(s: &str) -> Result<[i32; N], String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != N {
        return Err(format!("{s:?}: expected {N} comma-separated whole numbers"));
    }
    let mut out = [0; N];
    for (o, p) in out.iter_mut().zip(parts) {
        *o = p.trim().parse().map_err(|_| format!("{s:?}: {p:?} is not a whole number"))?;
    }
    Ok(out)
}

fn parse_point(s: &str) -> Result<(i32, i32), String> {
    parse_ints::<2>(s).map(|[x, y]| (x, y))
}

/// `RRGGBB` or `#RRGGBB`, opaque.
fn parse_colour(s: &str) -> Result<Rgba, String> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{s:?} is not a colour RRGGBB"));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string());
    Ok(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// A PNG file, read and decoded within the image bounds.
fn parse_image(s: &str) -> Result<RgbaImage, String> {
    let path = Path::new(s);
    let len = std::fs::metadata(path).map_err(|e| format!("{s}: {e}"))?.len();
    if len > olr_pack::MAX_FILE_BYTES {
        return Err(format!("{s}: {len} bytes is above {}", olr_pack::MAX_FILE_BYTES));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{s}: {e}"))?;
    let img = decode_rgba_within(&bytes, IMAGES).map_err(|e| format!("{s}: {e}"))?;
    if img.width() == 0 || img.height() == 0 {
        return Err(format!("{s}: an empty image"));
    }
    Ok(img)
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        LevelCommand::Carve { pack, shape } => edit(pack, |l| {
            level::carve(l, &shape.shape());
            Ok(())
        }),
        LevelCommand::Fill { pack, shape, material, paint } => edit(pack, |l| {
            let solid = match material {
                SolidArg::Rock => Solid::Rock,
                SolidArg::Dirt => Solid::Dirt,
            };
            level::fill(l, &shape.shape(), solid, &paint.paint());
            Ok(())
        }),
        LevelCommand::Paint { pack, shape, layer, paint } => edit(pack, |l| {
            level::paint(l, &shape.shape(), layer.layer(), &paint.paint());
            Ok(())
        }),
        LevelCommand::Stamp { pack, at, image, layer } => edit(pack, |l| {
            level::stamp(l, *at, image, layer.layer());
            Ok(())
        }),
        LevelCommand::Resize { pack, size, offset, material, colour } => edit(pack, |l| {
            let material = match material {
                MaterialArg::Rock => Material::Rock,
                MaterialArg::Dirt => Material::Dirt,
                MaterialArg::Background => Material::Background,
            };
            for o in level::resize(l, *size, *offset, material, &Paint::Colour(*colour))? {
                println!("dropped: {}", describe(&o));
            }
            Ok(())
        }),
        LevelCommand::Object { command } => match command {
            ObjectCommand::List { pack } => {
                let (_, l) = read(pack)?;
                for (i, o) in l.objects.iter().enumerate() {
                    println!("{i}: {}", describe(o));
                }
                Ok(true)
            }
            ObjectCommand::Add { pack, at, notes } => edit(pack, |l| {
                println!("added object {}", level::add_spawn(l, *at, notes)?);
                Ok(())
            }),
            ObjectCommand::Move { pack, index, at } => {
                edit(pack, |l| Ok(level::move_object(l, *index, *at)?))
            }
            ObjectCommand::Remove { pack, index } => edit(pack, |l| {
                level::remove_object(l, *index)?;
                Ok(())
            }),
        },
        LevelCommand::Preview { pack, out, scale, material } => {
            let (_, l) = read(pack)?;
            let img = preview(&l, *scale, *material)?;
            std::fs::write(out, encode_rgba(&img))
                .map_err(|e| anyhow::anyhow!("writing {}: {e}", out.display()))?;
            Ok(true)
        }
    }
}

/// `Spawn at (x, y)[ — notes]`.
fn describe(o: &LevelObject) -> String {
    match o {
        LevelObject::Spawn { at, notes } if notes.is_empty() => format!("Spawn at ({}, {})", at.0, at.1),
        LevelObject::Spawn { at, notes } => format!("Spawn at ({}, {}) — {notes}", at.0, at.1),
    }
}

/// The pack at `dir` and its level source.
fn read(dir: &Path) -> anyhow::Result<(Pack, LevelSource)> {
    let pack = load(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    match &pack.source {
        Source::Level(l) => {
            let l = l.clone();
            Ok((pack, l))
        }
        _ => anyhow::bail!("{}: not a level pack", dir.display()),
    }
}

/// Loads the level pack at `dir`, applies `op`, refreshes the thumbnail and
/// saves; an error from `op` or the save leaves the pack as it was.
fn edit(dir: &Path, op: impl FnOnce(&mut LevelSource) -> anyhow::Result<()>) -> anyhow::Result<bool> {
    let (mut pack, mut l) = read(dir)?;
    op(&mut l)?;
    level::refresh_thumb(&mut l);
    pack.source = Source::Level(l);
    for w in save(dir, &pack).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))? {
        eprintln!("warning: {w}");
    }
    Ok(true)
}

/// The front (or the material through `MATERIAL_PALETTE`) at `scale`
/// output pixels per art pixel, nearest neighbour, each spawn marked by a
/// 3 × 3 white cross outlined in black.
fn preview(l: &LevelSource, scale: u32, material: bool) -> anyhow::Result<RgbaImage> {
    let s = l.art_scale;
    let (aw, ah) = (l.front.width(), l.front.height());
    let (w, h) = (aw.saturating_mul(scale), ah.saturating_mul(scale));
    check_size(w, h, LEVEL_ART)
        .map_err(|e| anyhow::anyhow!("a {w}x{h} preview: {e}; use a smaller --scale"))?;
    let palette = |m: Material| {
        let i = m as usize * 3;
        Rgba::rgb(MATERIAL_PALETTE[i], MATERIAL_PALETTE[i + 1], MATERIAL_PALETTE[i + 2])
    };
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (ax, ay) = (x / scale, y / scale);
            let c = if material {
                palette(l.material[(ay / s) as usize * l.size.0 as usize + (ax / s) as usize])
            } else {
                l.front.get(ax, ay)
            };
            img.set(x, y, c);
        }
    }
    let cross = [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)];
    for o in &l.objects {
        let LevelObject::Spawn { at, .. } = o;
        // The centre of the spawn's art block, in output pixels.
        let centre = |v: i32| {
            (i64::from(v) * i64::from(s) + i64::from(s / 2)) * i64::from(scale) + i64::from(scale / 2)
        };
        let (cx, cy) = (centre(at.0), centre(at.1));
        let mut put = |x: i64, y: i64, c: Rgba| {
            if (0..i64::from(w)).contains(&x) && (0..i64::from(h)).contains(&y) {
                img.set(x as u32, y as u32, c);
            }
        };
        for (dx, dy) in cross {
            for (ox, oy) in cross {
                put(cx + dx + ox, cy + dy + oy, Rgba::rgb(0, 0, 0));
            }
        }
        for (dx, dy) in cross {
            put(cx + dx, cy + dy, Rgba::rgb(255, 255, 255));
        }
    }
    Ok(img)
}
