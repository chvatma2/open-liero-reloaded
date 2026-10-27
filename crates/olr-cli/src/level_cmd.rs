//! `olr-cli level …`: the level operations on a pack (plan 13d ruling R3),
//! and a preview to PNG. Each operation loads the pack, applies the
//! operation, refreshes the thumbnail and saves through
//! `olr_edit::pack::save` (the pack on disk is the old one or the new one).
//! Every argument is parsed strictly: a bad value is a clap error naming
//! its flag. Coordinates may be negative (`--offset -4,0`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Subcommand, ValueEnum};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::PackManifest;
use olr_edit::level::{self, Layer, Paint, Shape, Solid, StampMask};
use olr_edit::pack::{Pack, Source, load};
use olr_format::image_io::{IMAGES, LEVEL_ART, check_size, decode_rgba_within, encode_rgba};
use olr_format::levels::{LevelObject, LevelSource, MATERIAL_PALETTE};
use olr_vfs::VPath;

use crate::element_cmd::{self, ContentArg, ElementCommand};
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
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
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
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true, default_value = "0,0")]
        offset: (i32, i32),
        /// The new area's material.
        #[arg(long, default_value = "background")]
        material: MaterialArg,
        /// The new area's colour, front and back, RRGGBB.
        #[arg(long, value_parser = parse_colour, default_value = "000000")]
        colour: Rgba,
        #[command(flatten)]
        content: ContentArg,
    },
    /// A composed level's elements (plan 14a): list, add, set, reorder, draw.
    Element {
        #[command(subcommand)]
        command: ElementCommand,
    },
    /// A level's placed copies of assets (issue #75): add, list, move,
    /// resize, set, script, raise, lower, remove.
    Copy {
        #[command(subcommand)]
        command: crate::copy_cmd::CopyCommand,
    },
    /// Set or clear the level script (`editor/level.rhai`, Rhai, not run).
    Script {
        pack: PathBuf,
        #[command(flatten)]
        script: crate::copy_cmd::ScriptArgs,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Rebake a composed level's material, front, back and thumbnail.
    Bake {
        pack: PathBuf,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Convert levels of OLR 0.2 (issue #74): each `editor/composition.ron`
    /// becomes the level's ground asset, `editor/asset.ron`, placed alone by
    /// `editor/placed.ron`, every element file in its place. The level is
    /// baked and must bake to the same files; else nothing is written.
    /// Prints `converted <pack>: 1 asset, 1 copy`, `<pack>: already
    /// converted` or `<pack>: a plain level: nothing to convert`.
    Convert {
        #[arg(required = true)]
        packs: Vec<PathBuf>,
        #[command(flatten)]
        content: ContentArg,
    },
    /// The level's objects: list, add, move, remove.
    Object {
        #[command(subcommand)]
        command: ObjectCommand,
    },
    /// Set the level's look (look system): ambient, light settings, grade.
    Look(crate::look_cmd::LookArgs),
    /// The level's glow groups: add, set, remove.
    Group {
        #[command(subcommand)]
        command: crate::look_cmd::GroupCommand,
    },
    /// The level's haze map (directional glow): paint it, clear it.
    Haze {
        #[command(subcommand)]
        command: crate::look_cmd::HazeCommand,
    },
    /// The level's surface materials (issue #38): list them, copy the
    /// library's in. Elements name them: `level element set --surface`,
    /// `level element surface`.
    Material {
        #[command(subcommand)]
        command: crate::material_cmd::LevelMaterialCommand,
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
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
        #[arg(long, default_value = "")]
        notes: String,
    },
    /// Move the object at INDEX to world X,Y.
    Move {
        pack: PathBuf,
        index: usize,
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
    },
    /// Remove the object at INDEX.
    Remove { pack: PathBuf, index: usize },
}

/// One shape, in world pixels.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ShapeArgs {
    /// A rectangle X,Y,W,H.
    #[arg(long, value_parser = parse_ints::<4>, allow_hyphen_values = true)]
    rect: Option<[i32; 4]>,
    /// A circle X,Y,R.
    #[arg(long, value_parser = parse_ints::<3>, allow_hyphen_values = true)]
    circle: Option<[i32; 3]>,
    /// A PNG laid on the level from its top-left, one pixel per world
    /// pixel: its pixels neither black nor transparent (issue #63).
    #[arg(long, value_parser = parse_mask)]
    mask: Option<Arc<StampMask>>,
}

impl ShapeArgs {
    pub(crate) fn shape(&self) -> Shape {
        match (self.rect, self.circle, &self.mask) {
            (Some([x, y, w, h]), ..) => Shape::Rect { x, y, w, h },
            (None, Some([cx, cy, r]), _) => Shape::Circle { cx, cy, r },
            // A stamp lies centred: half its size from the origin puts its
            // top-left there.
            (None, None, Some(mask)) => {
                let half = |v: u32| i32::try_from(v / 2).unwrap_or(i32::MAX);
                Shape::Stamp { x: half(mask.size.0), y: half(mask.size.1), scale: 1, mask: mask.clone() }
            }
            // clap's group requires one.
            (None, None, None) => Shape::Rect { x: 0, y: 0, w: 0, h: 0 },
        }
    }
}

/// One paint.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct PaintArgs {
    /// A colour, RRGGBB (a leading # is allowed).
    #[arg(long, value_parser = parse_colour)]
    colour: Option<Rgba>,
    /// A PNG repeated from the art origin (`--texture` alike).
    #[arg(long, visible_alias = "texture", value_parser = parse_image)]
    tile: Option<RgbaImage>,
}

impl PaintArgs {
    pub(crate) fn paint(&self) -> Paint {
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
pub(crate) enum MaterialArg {
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
pub(crate) fn parse_ints<const N: usize>(s: &str) -> Result<[i32; N], String> {
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

pub(crate) fn parse_point(s: &str) -> Result<(i32, i32), String> {
    parse_ints::<2>(s).map(|[x, y]| (x, y))
}

/// `RRGGBB` or `#RRGGBB`, opaque.
pub(crate) fn parse_colour(s: &str) -> Result<Rgba, String> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{s:?} is not a colour RRGGBB"));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string());
    Ok(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// A regular file's bytes, at most `MAX_FILE_BYTES`.
pub(crate) fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let s = path.display();
    // A device or FIFO reports no length and may never end.
    if !std::fs::metadata(path).map_err(|e| format!("{s}: {e}"))?.is_file() {
        return Err(format!("{s}: not a file"));
    }
    let limit = olr_pack::MAX_FILE_BYTES;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{s}: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{s}: above {limit} bytes"));
    }
    Ok(bytes)
}

/// A PNG file, read and decoded within the image bounds.
fn parse_image(s: &str) -> Result<RgbaImage, String> {
    let bytes = read_bounded(Path::new(s))?;
    let img = decode_rgba_within(&bytes, IMAGES).map_err(|e| format!("{s}: {e}"))?;
    if img.width() == 0 || img.height() == 0 {
        return Err(format!("{s}: an empty image"));
    }
    Ok(img)
}

/// A mask PNG (`--mask`), read and decoded as `parse_image` does: covered
/// where neither black nor transparent, as a theme hole.
fn parse_mask(s: &str) -> Result<Arc<StampMask>, String> {
    Ok(Arc::new(StampMask::from_hole(&parse_image(s)?)))
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        LevelCommand::Carve { pack, shape } => edit_raster(pack, |l| {
            level::carve(l, &shape.shape());
            Ok(Vec::new())
        }),
        LevelCommand::Fill { pack, shape, material, paint } => edit_raster(pack, |l| {
            let solid = match material {
                SolidArg::Rock => Solid::Rock,
                SolidArg::Dirt => Solid::Dirt,
            };
            level::fill(l, &shape.shape(), solid, &paint.paint());
            Ok(Vec::new())
        }),
        LevelCommand::Paint { pack, shape, layer, paint } => edit_raster(pack, |l| {
            level::paint(l, &shape.shape(), layer.layer(), &paint.paint());
            Ok(Vec::new())
        }),
        LevelCommand::Stamp { pack, at, image, layer } => edit_raster(pack, |l| {
            level::stamp(l, *at, image, layer.layer());
            Ok(Vec::new())
        }),
        LevelCommand::Resize { pack, size, offset, material, colour, content } => {
            let material = match material {
                MaterialArg::Rock => Material::Rock,
                MaterialArg::Dirt => Material::Dirt,
                MaterialArg::Background => Material::Background,
            };
            let paint = Paint::Colour(*colour);
            let mut p = element_cmd::load_level(pack)?;
            // The composition first: it reads the old size from the level.
            olr_edit::element::resize(&mut p, *size, *offset, material, &paint)
                .map_err(|e| anyhow::anyhow!("{}: {e}", pack.display()))?;
            let Source::Level(l) = &mut p.source else {
                anyhow::bail!("{}: not a level pack", pack.display())
            };
            let dropped = level::resize(l, *size, *offset, material, &paint)?;
            level::refresh_thumb(l);
            element_cmd::bake_pack(pack, &mut p, &content.content)?;
            let messages: Vec<String> = dropped.iter().map(|o| format!("dropped: {}", describe(o))).collect();
            element_cmd::save_and_print(pack, &p, &messages)
        }
        LevelCommand::Element { command } => element_cmd::run(command),
        LevelCommand::Copy { command } => crate::copy_cmd::run(command),
        LevelCommand::Script { pack, script, content } => {
            crate::copy_cmd::level_script(pack, script, content)
        }
        LevelCommand::Look(a) => crate::look_cmd::look_cmd(a),
        LevelCommand::Group { command } => crate::look_cmd::group_cmd(command),
        LevelCommand::Haze { command } => crate::look_cmd::haze_cmd(command),
        LevelCommand::Material { command } => crate::material_cmd::level_cmd(command),
        LevelCommand::Bake { pack, content } => element_cmd::bake_cmd(pack, content),
        LevelCommand::Convert { packs, content } => {
            for pack in packs {
                element_cmd::convert_cmd(pack, content)?;
            }
            Ok(true)
        }
        LevelCommand::Object { command } => match command {
            ObjectCommand::List { pack } => {
                // `level.ron` alone: no image is decoded.
                let header = olr_format::levels::read_level_header(&olr_vfs::DirFs::new(pack))
                    .map_err(|e| anyhow::anyhow!("{}: {e}", pack.display()))?;
                for (i, o) in header.objects.iter().enumerate() {
                    println!("{i}: {}", describe(o));
                }
                Ok(true)
            }
            ObjectCommand::Add { pack, at, notes } => edit(pack, |l| {
                let i = level::add_spawn(l, *at, notes)?;
                warn_outside(l, i);
                Ok(vec![format!("added object {i}")])
            }),
            ObjectCommand::Move { pack, index, at } => edit(pack, |l| {
                level::move_object(l, *index, *at)?;
                warn_outside(l, *index);
                Ok(Vec::new())
            }),
            ObjectCommand::Remove { pack, index } => edit(pack, |l| {
                level::remove_object(l, *index)?;
                Ok(Vec::new())
            }),
        },
        LevelCommand::Preview { pack, out, scale, material } => {
            refuse_inside(pack, out)?;
            let (_, _, l) = read(pack)?;
            let img = preview(&l, *scale, *material)?;
            std::fs::write(out, encode_rgba(&img))
                .map_err(|e| anyhow::anyhow!("writing {}: {e}", out.display()))?;
            Ok(true)
        }
    }
}

/// `Spawn at (x, y)[ — notes]`, on one line: control characters in the
/// notes are escaped (`\n`).
fn describe(o: &LevelObject) -> String {
    match o {
        LevelObject::Spawn { at, notes } if notes.is_empty() => format!("Spawn at ({}, {})", at.0, at.1),
        LevelObject::Spawn { at, notes } => {
            let notes: String = notes
                .chars()
                .map(|c| if c.is_control() { c.escape_default().to_string() } else { c.to_string() })
                .collect();
            format!("Spawn at ({}, {}) — {notes}", at.0, at.1)
        }
    }
}

/// A warning (stderr) when object `i` lies outside the level, as
/// `olr-cli pack check` words it.
fn warn_outside(l: &LevelSource, i: usize) {
    let Some(LevelObject::Spawn { at, .. }) = l.objects.get(i) else { return };
    let (w, h) = l.size;
    let inside = |v: i32, side: u32| u32::try_from(v).is_ok_and(|v| v < side);
    if !(inside(at.0, w) && inside(at.1, h)) {
        eprintln!(
            "warning: level.ron: objects[{i}]: spawn at ({}, {}) is outside the level ({w}×{h})",
            at.0, at.1
        );
    }
}

/// Refuses an `--out` inside the pack: the preview never writes into it.
fn refuse_inside(pack: &Path, out: &Path) -> anyhow::Result<()> {
    let pack = pack.canonicalize().map_err(|e| anyhow::anyhow!("{}: {e}", pack.display()))?;
    let parent = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    if parent.canonicalize().is_ok_and(|p| p.starts_with(&pack)) {
        anyhow::bail!("{}: inside the pack; write the preview elsewhere", out.display());
    }
    Ok(())
}

/// The pack at `dir`, taken apart: its manifest, its other files and its
/// level source.
fn read(dir: &Path) -> anyhow::Result<(PackManifest, BTreeMap<VPath, Vec<u8>>, LevelSource)> {
    let Pack { manifest, source, other, .. } =
        load(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    match source {
        Source::Level(l) => Ok((manifest, other, l)),
        _ => anyhow::bail!("{}: not a level pack", dir.display()),
    }
}

/// Loads the level pack at `dir`, applies `op`, refreshes the thumbnail and
/// saves, then prints `op`'s messages; an error from `op` or the save
/// leaves the pack as it was and prints none. A composed level keeps its
/// composition (`op` changes no art the bake makes).
fn edit(
    dir: &Path,
    op: impl FnOnce(&mut LevelSource) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<bool> {
    let mut pack = element_cmd::load_level(dir)?;
    let Source::Level(l) = &mut pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let messages = op(l)?;
    level::refresh_thumb(l);
    element_cmd::save_and_print(dir, &pack, &messages)
}

/// [`edit`] for an operation on the level's raster: refused on a composed
/// level, whose art is its elements' bake (plan 14a ruling R1).
fn edit_raster(
    dir: &Path,
    op: impl FnOnce(&mut LevelSource) -> anyhow::Result<Vec<String>>,
) -> anyhow::Result<bool> {
    // An unconverted level of OLR 0.2 is `load_level`'s refusal.
    if dir.join(olr_format::levels::PLACED_FILE).exists() {
        anyhow::bail!(
            "{}: a composed level: edit its elements with olr-cli level element draw|erase, or in the editor",
            dir.display()
        );
    }
    edit(dir, op)
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
