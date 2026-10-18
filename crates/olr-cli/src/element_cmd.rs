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

use crate::level_cmd::{MaterialArg, PaintArgs, ShapeArgs, parse_ints, parse_point, read_bounded};
use crate::pack_cmd::parse_size;

/// Where the theme packs are, for the bake's back tile.
#[derive(clap::Args)]
pub(crate) struct ContentArg {
    /// The content directory holding the level's theme pack.
    #[arg(long, default_value = "content/packs")]
    pub(crate) content: PathBuf,
}

/// `mask --colour-at`'s tolerance when none is given, percent (the
/// editor's Remove colour starts at it too).
const DEFAULT_TOLERANCE: u8 = 10;

/// What `mask` masks: a world shape, or a region of similar colour. Its
/// own group (not `ShapeArgs` flattened in): clap's groups do not nest, and
/// one of the three is required.
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
pub(crate) struct MaskTarget {
    /// A rectangle X,Y,W,H, world pixels.
    #[arg(long, value_parser = parse_ints::<4>, allow_hyphen_values = true)]
    rect: Option<[i32; 4]>,
    /// A circle X,Y,R, world pixels.
    #[arg(long, value_parser = parse_ints::<3>, allow_hyphen_values = true)]
    circle: Option<[i32; 3]>,
    /// The connected region of similar colour around world pixel X,Y.
    #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
    colour_at: Option<(i32, i32)>,
}

impl MaskTarget {
    fn op(&self, tolerance: u8) -> element::MaskOp {
        use olr_edit::level::Shape;
        match (self.rect, self.circle, self.colour_at) {
            (Some([x, y, w, h]), ..) => element::MaskOp::Shape(Shape::Rect { x, y, w, h }),
            (None, Some([cx, cy, r]), _) => element::MaskOp::Shape(Shape::Circle { cx, cy, r }),
            (None, None, Some(at)) => element::MaskOp::ColourAt { at, tolerance },
            // clap's group requires one.
            (None, None, None) => element::MaskOp::Shape(Shape::Rect { x: 0, y: 0, w: 0, h: 0 }),
        }
    }
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
        /// Glow strength, 0 to 100 percent (0 removes the glow; look system).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        glow: Option<u8>,
        /// The glow group (an id in level.ron's look) the glow lights with.
        #[arg(long)]
        glow_group: Option<String>,
        /// Its rock and dirt let light through (look system), still solid.
        #[arg(long)]
        clear: Option<bool>,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Paint a glowing element's glow mask over a shape (0 erases).
    Glow {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        shape: ShapeArgs,
        /// 0 to 100 percent of the element's glow.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        strength: u8,
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
        /// What the painted pixels are (default the layer's material); another
        /// material makes the layer hold a material per pixel.
        #[arg(long)]
        material: Option<MaterialArg>,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Flood-fill the connected region of a world pixel in a drawn layer:
    /// pixels alike in covered state and colour.
    Fill {
        pack: PathBuf,
        id: String,
        /// The world pixel to fill from, X,Y.
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
        /// How alike a colour must be, 0 to 100 percent (default 0: exact).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100), default_value_t = 0)]
        tolerance: u8,
        #[command(flatten)]
        paint: PaintArgs,
        /// What the filled pixels are (default the layer's material).
        #[arg(long)]
        material: Option<MaterialArg>,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Copy an element directly above it, at the same place; prints the new id.
    Duplicate {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Merge an element into the drawn layer directly below it: per pixel
    /// rock, then dirt, then background wins.
    MergeDown {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Split an element into Background, Dirt and Rock layers in its place;
    /// prints the new ids, bottom first.
    Split {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Erase part of an image element's original through its mask.
    Mask {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        target: MaskTarget,
        /// How alike a colour must be for --colour-at, 0 to 100 percent (default 10).
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        tolerance: Option<u8>,
        /// Bring masked pixels back (shapes only).
        #[arg(long)]
        restore: bool,
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
    // `composition.ron` alone: the list still shows a stack whose files
    // are broken.
    let composition = olr_format::levels::read_composition(&olr_vfs::DirFs::new(dir))
        .map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    for (i, e) in composition.iter().flat_map(|c| c.elements.iter()).enumerate() {
        let (kind, material) = match &e.kind {
            ElementKind::Image { material, .. } => ("Image", material_name(*material)),
            ElementKind::Drawn { material, .. } => ("Drawn", material_name(*material)),
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
                let settings: Vec<Property> = [
                    (*pixel_size != 1).then_some(Property::PixelSize(*pixel_size)),
                    colours.map(|n| Property::Colours(Some(n))),
                    Some(Property::Dither(dither.dither())).filter(|d| *d != Property::Dither(Dither::None)),
                    (*threshold != 128).then_some(Property::Threshold(*threshold)),
                ]
                .into_iter()
                .flatten()
                .collect();
                // The settings apply with the image, checked together.
                let id = element::add_image_with(
                    p,
                    &file_name,
                    bytes,
                    *at,
                    *size,
                    material.element(),
                    name,
                    &settings,
                )?;
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
            glow,
            glow_group,
            clear,
            content,
        } => edit(pack, content, |p| {
            let glow = glow_property(p, id, *glow, glow_group.as_deref())?;
            // Unlocking first and locking last, so one command can unlock,
            // change and lock again; all in one step, checked together.
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
                glow,
                clear.map(Property::Clear),
                locked.filter(|l| *l).map(Property::Locked),
            ];
            let properties: Vec<Property> = properties.into_iter().flatten().collect();
            element::set_all(p, id, &properties)?;
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
        ElementCommand::Draw { pack, id, shape, paint, material, content } => edit(pack, content, |p| {
            element::draw(p, id, &shape.shape(), &paint.paint(), material.map(|m| m.element()))?;
            Ok(Vec::new())
        }),
        ElementCommand::Fill { pack, id, at, tolerance, paint, material, content } => {
            edit(pack, content, |p| {
                element::fill(p, id, *at, &paint.paint(), *tolerance, material.map(|m| m.element()))?;
                Ok(Vec::new())
            })
        }
        ElementCommand::Duplicate { pack, id, content } => {
            edit(pack, content, |p| Ok(vec![format!("added element {}", element::duplicate(p, id)?)]))
        }
        ElementCommand::MergeDown { pack, id, content } => edit(pack, content, |p| {
            element::merge_down(p, id)?;
            Ok(Vec::new())
        }),
        ElementCommand::Split { pack, id, content } => edit(pack, content, |p| {
            Ok(element::split(p, id)?.into_iter().map(|id| format!("added element {id}")).collect())
        }),
        ElementCommand::Mask { tolerance: Some(_), target: MaskTarget { colour_at: None, .. }, .. } => {
            anyhow::bail!("--tolerance takes --colour-at")
        }
        ElementCommand::Mask { pack, id, target, tolerance, restore, content } => edit(pack, content, |p| {
            element::mask(p, id, &target.op(tolerance.unwrap_or(DEFAULT_TOLERANCE)), *restore)?;
            Ok(Vec::new())
        }),
        ElementCommand::Erase { pack, id, shape, content } => edit(pack, content, |p| {
            element::erase(p, id, &shape.shape())?;
            Ok(Vec::new())
        }),
        ElementCommand::Glow { pack, id, shape, strength, content } => edit(pack, content, |p| {
            element::paint_glow(p, id, &shape.shape(), *strength)?;
            Ok(Vec::new())
        }),
    }
}

/// `set`'s `--glow` and `--glow-group` as one property: a strength of 0
/// removes the glow; either flag alone keeps the other's current value
/// (and the painted mask).
fn glow_property(
    p: &olr_edit::pack::Pack,
    id: &str,
    strength: Option<u8>,
    group: Option<&str>,
) -> Result<Option<Property>, element::ElementError> {
    if strength == Some(0) {
        return Ok(Some(Property::Glow(None)));
    }
    let now = p
        .composition
        .as_ref()
        .and_then(|c| c.composition.elements.iter().find(|e| e.id == id))
        .and_then(|e| e.glow.clone());
    let (strength, group, mask) = match (strength, group, now) {
        (Some(s), Some(g), now) => (s, g.to_string(), now.is_some_and(|n| n.mask)),
        (Some(s), None, Some(n)) => (s, n.group, n.mask),
        (None, Some(g), Some(n)) => (n.strength, g.to_string(), n.mask),
        (None, None, _) => return Ok(None),
        (_, _, None) => {
            return Err(element::ElementError::BadValue(
                "a new glow needs both --glow and --glow-group".into(),
            ));
        }
    };
    Ok(Some(Property::Glow(Some(olr_format::levels::ElementGlow { strength, group, mask }))))
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
