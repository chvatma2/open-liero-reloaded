//! `olr-cli level element …` and `level bake` (plan 14a, spec §7): a
//! level's composition edited by agents. Each command loads the pack,
//! applies the `olr_edit::element` operation, bakes (`compose::bake`, the
//! theme's back tile from `--content`) and saves through
//! `olr_edit::pack::save`; its messages print once the save succeeded. A
//! refused operation writes nothing.

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use olr_edit::compose::{bake, theme_back_tile};
use olr_edit::element::{self, Property};
use olr_edit::pack::{Pack, Source, load, save};
use olr_format::levels::{Dither, ElementKind, ElementMaterial};

use crate::level_cmd::{MaterialArg, PaintArgs, ShapeArgs, parse_point, read_bounded};
use crate::pack_cmd::parse_size;

/// Where the theme packs are, for the bake's back tile.
#[derive(clap::Args)]
pub(crate) struct ContentArg {
    /// The content directory holding the level's theme pack.
    #[arg(long, default_value = "content/packs")]
    pub(crate) content: PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum DitherArg {
    None,
    Ordered,
    Diffusion,
}

impl DitherArg {
    fn dither(self) -> Dither {
        match self {
            DitherArg::None => Dither::None,
            DitherArg::Ordered => Dither::Ordered,
            DitherArg::Diffusion => Dither::Diffusion,
        }
    }
}

impl MaterialArg {
    fn element(self) -> ElementMaterial {
        match self {
            MaterialArg::Rock => ElementMaterial::Rock,
            MaterialArg::Dirt => ElementMaterial::Dirt,
            MaterialArg::Background => ElementMaterial::Background,
        }
    }
}

/// `--colours`' value on `set`: a number of colours, or none (no
/// reduction). A type of its own: clap reads `Option<Option<T>>` as a flag
/// with an optional value.
#[derive(Clone, Copy)]
pub(crate) struct Colours(Option<u16>);

fn parse_colours(s: &str) -> Result<Colours, String> {
    if s == "none" {
        return Ok(Colours(None));
    }
    s.parse().map(|n| Colours(Some(n))).map_err(|_| format!("{s:?}: a number of colours or none"))
}

#[derive(Subcommand)]
pub(crate) enum ElementCommand {
    /// Print each element, bottom first: `<index>: <id> <kind> <material>["<name>"][ hidden][ locked]`.
    List { pack: PathBuf },
    /// Add an image element on top, from a PNG or JPEG.
    AddImage {
        pack: PathBuf,
        file: PathBuf,
        /// The box's top-left, world pixels X,Y.
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
        /// The box's size, world pixels WxH.
        #[arg(long, value_parser = parse_size)]
        size: (u32, u32),
        /// Art pixels per element pixel.
        #[arg(long, default_value_t = 1)]
        pixel_size: u32,
        /// Reduce to this many colours (2 to 256).
        #[arg(long)]
        colours: Option<u16>,
        #[arg(long, default_value = "none")]
        dither: DitherArg,
        /// Alpha at or above which a pixel is covered.
        #[arg(long, default_value_t = 128)]
        threshold: u8,
        #[arg(long, default_value = "dirt")]
        material: MaterialArg,
        #[arg(long, default_value = "")]
        name: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Add an empty drawn layer on top.
    AddLayer {
        pack: PathBuf,
        #[arg(long, default_value = "dirt")]
        material: MaterialArg,
        #[arg(long, default_value = "")]
        name: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Set an element's properties.
    Set {
        pack: PathBuf,
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        visible: Option<bool>,
        #[arg(long)]
        locked: Option<bool>,
        #[arg(long)]
        material: Option<MaterialArg>,
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: Option<(i32, i32)>,
        #[arg(long, value_parser = parse_size)]
        size: Option<(u32, u32)>,
        #[arg(long)]
        pixel_size: Option<u32>,
        /// A number of colours, or none.
        #[arg(long, value_parser = parse_colours)]
        colours: Option<Colours>,
        #[arg(long)]
        dither: Option<DitherArg>,
        #[arg(long)]
        threshold: Option<u8>,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Remove an element and its files.
    Remove {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Move an element one step up.
    Raise {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Move an element one step down.
    Lower {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Paint a shape into a drawn layer.
    Draw {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        shape: ShapeArgs,
        #[command(flatten)]
        paint: PaintArgs,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Clear a shape in a drawn layer.
    Erase {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        shape: ShapeArgs,
        #[command(flatten)]
        content: ContentArg,
    },
}

/// The level pack at `dir`.
pub(crate) fn load_level(dir: &Path) -> anyhow::Result<Pack> {
    let pack = load(dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    if !matches!(pack.source, Source::Level(_)) {
        anyhow::bail!("{}: not a level pack", dir.display());
    }
    Ok(pack)
}

/// Bakes a composed `pack` with the back tile of its theme in `content`.
pub(crate) fn bake_pack(dir: &Path, pack: &mut Pack, content: &Path) -> anyhow::Result<()> {
    let fail = |e: &dyn std::fmt::Display| anyhow::anyhow!("{}: {e}", dir.display());
    let (Source::Level(level), Some(src)) = (&mut pack.source, &pack.composition) else { return Ok(()) };
    let tile = theme_back_tile(content, &level.theme).map_err(|e| fail(&e))?;
    bake(level, src, &tile).map_err(|e| fail(&e))
}

/// Saves `pack`, its save's warnings on stderr, then prints `messages`.
pub(crate) fn save_and_print(dir: &Path, pack: &Pack, messages: &[String]) -> anyhow::Result<bool> {
    crate::pack_cmd::warn(&save(dir, pack).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?);
    for m in messages {
        println!("{m}");
    }
    Ok(true)
}

/// Loads the level pack at `dir`, applies `op`, bakes and saves.
fn edit(
    dir: &Path,
    content: &ContentArg,
    op: impl FnOnce(&mut Pack) -> Result<Vec<String>, element::ElementError>,
) -> anyhow::Result<bool> {
    let mut pack = load_level(dir)?;
    let messages = op(&mut pack).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    bake_pack(dir, &mut pack, &content.content)?;
    save_and_print(dir, &pack, &messages)
}

fn material_name(m: ElementMaterial) -> &'static str {
    match m {
        ElementMaterial::Rock => "Rock",
        ElementMaterial::Dirt => "Dirt",
        ElementMaterial::Background => "Background",
    }
}

fn list(dir: &Path) -> anyhow::Result<bool> {
    let src = olr_format::levels::read_composition_files(&olr_vfs::DirFs::new(dir))
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    for (i, e) in src.iter().flat_map(|s| s.composition.elements.iter()).enumerate() {
        let (kind, material) = match &e.kind {
            ElementKind::Image { material, .. } => ("Image", material_name(*material)),
            ElementKind::Drawn { material } => ("Drawn", material_name(*material)),
            ElementKind::Raster => ("Raster", "-"),
        };
        let mut line = format!("{i}: {} {kind} {material}", e.id);
        if !e.name.is_empty() {
            line.push_str(&format!(" {:?}", e.name));
        }
        if !e.visible {
            line.push_str(" hidden");
        }
        if e.locked {
            line.push_str(" locked");
        }
        println!("{line}");
    }
    Ok(true)
}

pub(crate) fn run(command: &ElementCommand) -> anyhow::Result<bool> {
    match command {
        ElementCommand::List { pack } => list(pack),
        ElementCommand::AddImage {
            pack,
            file,
            at,
            size,
            pixel_size,
            colours,
            dither,
            threshold,
            material,
            name,
            content,
        } => {
            let bytes = read_bounded(file).map_err(|e| anyhow::anyhow!(e))?;
            let file_name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            edit(pack, content, |p| {
                let id = element::add_image(p, &file_name, bytes, *at, *size, material.element(), name)?;
                let settings = [
                    (*pixel_size != 1).then_some(Property::PixelSize(*pixel_size)),
                    colours.map(|n| Property::Colours(Some(n))),
                    Some(Property::Dither(dither.dither())).filter(|d| *d != Property::Dither(Dither::None)),
                    (*threshold != 128).then_some(Property::Threshold(*threshold)),
                ];
                for property in settings.into_iter().flatten() {
                    element::set(p, &id, property)?;
                }
                Ok(vec![format!("added element {id}")])
            })
        }
        ElementCommand::AddLayer { pack, material, name, content } => edit(pack, content, |p| {
            let id = element::add_layer(p, material.element(), name)?;
            Ok(vec![format!("added element {id}")])
        }),
        ElementCommand::Set {
            pack,
            id,
            name,
            visible,
            locked,
            material,
            at,
            size,
            pixel_size,
            colours,
            dither,
            threshold,
            content,
        } => edit(pack, content, |p| {
            // Unlocking first and locking last, so one command can do both
            // and change the element in between.
            let properties = [
                locked.filter(|l| !l).map(Property::Locked),
                name.clone().map(Property::Name),
                visible.map(Property::Visible),
                material.map(|m| Property::Material(m.element())),
                at.map(Property::At),
                size.map(Property::Size),
                pixel_size.map(Property::PixelSize),
                colours.map(|Colours(n)| Property::Colours(n)),
                dither.map(|d| Property::Dither(d.dither())),
                threshold.map(Property::Threshold),
                locked.filter(|l| *l).map(Property::Locked),
            ];
            for property in properties.into_iter().flatten() {
                element::set(p, id, property)?;
            }
            Ok(Vec::new())
        }),
        ElementCommand::Remove { pack, id, content } => edit(pack, content, |p| {
            element::remove(p, id)?;
            Ok(Vec::new())
        }),
        ElementCommand::Raise { pack, id, content } => edit(pack, content, |p| {
            element::raise(p, id)?;
            Ok(Vec::new())
        }),
        ElementCommand::Lower { pack, id, content } => edit(pack, content, |p| {
            element::lower(p, id)?;
            Ok(Vec::new())
        }),
        ElementCommand::Draw { pack, id, shape, paint, content } => edit(pack, content, |p| {
            element::draw(p, id, &shape.shape(), &paint.paint())?;
            Ok(Vec::new())
        }),
        ElementCommand::Erase { pack, id, shape, content } => edit(pack, content, |p| {
            element::erase(p, id, &shape.shape())?;
            Ok(Vec::new())
        }),
    }
}

/// `level bake <pack>`: rebakes a composed level.
pub(crate) fn bake_cmd(dir: &Path, content: &ContentArg) -> anyhow::Result<bool> {
    let mut pack = load_level(dir)?;
    if pack.composition.is_none() {
        anyhow::bail!("{}: no composition to bake", dir.display());
    }
    bake_pack(dir, &mut pack, &content.content)?;
    save_and_print(dir, &pack, &[format!("baked {}", dir.display())])
}
