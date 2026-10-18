//! `olr-cli level look|group|light …` (look system, plan 15a Task 7): a
//! level's look settings, glow groups and light objects, through
//! `olr_edit::look`. Each loads the pack, applies, rebakes a composed level
//! when its glow changed, and saves; a refused edit writes nothing.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use olr_data::image::Rgba;
use olr_edit::look;
use olr_edit::pack::Source;
use olr_format::Colour;
use olr_format::look::{
    AmbientSource, AnimationSource, GlowColourSource, GlowGroupSource, LightSource, ReachSource,
};

use crate::element_cmd::{self, ContentArg};
use crate::level_cmd::parse_point;

/// `level look`'s flags: each sets one field of the look (made when the
/// level has none).
#[derive(clap::Args)]
pub(crate) struct LookArgs {
    pack: PathBuf,
    /// The ambient light, RRGGBB:PERCENT (0-100).
    #[arg(long, value_parser = parse_ambient)]
    ambient: Option<(Colour, u16)>,
    /// The brightest light, percent, 100-200.
    #[arg(long)]
    max_light: Option<u16>,
    /// Bands of light: 0 (off) or 2-32.
    #[arg(long)]
    steps: Option<u8>,
    /// Light blur, world pixels, 0-4.
    #[arg(long)]
    softness: Option<u8>,
    /// Hundredths of a stop, -200 to 200.
    #[arg(long, allow_hyphen_values = true)]
    exposure: Option<i16>,
    /// Percent, 0-200.
    #[arg(long)]
    contrast: Option<u16>,
    /// Percent, 0-200.
    #[arg(long)]
    saturation: Option<u16>,
    /// The grade's tint, RRGGBB.
    #[arg(long, value_parser = parse_colour)]
    tint: Option<Colour>,
    /// Remove the look: its settings and every glow group (their elements
    /// stop glowing).
    #[arg(long, conflicts_with_all = ["ambient", "max_light", "steps", "softness", "exposure", "contrast", "saturation", "tint"])]
    clear: bool,
    #[command(flatten)]
    content: ContentArg,
}

/// A glow group's flags (`set` keeps what is not given).
#[derive(clap::Args)]
pub(crate) struct GroupFields {
    /// `own` (each pixel's art colour) or RRGGBB.
    #[arg(long, value_parser = parse_glow_colour)]
    colour: Option<GlowColourSource>,
    /// Percent, 0-1000.
    #[arg(long)]
    intensity: Option<u16>,
    /// World pixels, or `unlimited`.
    #[arg(long, value_parser = parse_reach)]
    reach: Option<ReachSource>,
    /// An animation in RON, such as `Flicker(speed: 8, depth: 25)`.
    #[arg(long, value_parser = parse_animation)]
    animation: Option<AnimationSource>,
    /// Milliseconds.
    #[arg(long)]
    phase: Option<u32>,
}

#[derive(Subcommand)]
pub(crate) enum GroupCommand {
    /// Add a glow group (`--reach` needed).
    Add {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        fields: GroupFields,
    },
    /// Change a glow group's fields.
    Set {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        fields: GroupFields,
        /// A new id (the elements naming it follow).
        #[arg(long)]
        rename: Option<String>,
    },
    /// Remove a glow group: its pixels stop glowing.
    Remove {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
}

/// A light's flags (`set` keeps what is not given).
#[derive(clap::Args)]
pub(crate) struct LightFields {
    /// World pixels X,Y.
    #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
    at: Option<(i32, i32)>,
    /// RRGGBB.
    #[arg(long, value_parser = parse_colour)]
    colour: Option<Colour>,
    /// Percent, 0-1000.
    #[arg(long)]
    intensity: Option<u16>,
    /// World pixels, or `unlimited`.
    #[arg(long, value_parser = parse_reach)]
    reach: Option<ReachSource>,
    /// World pixels, 1-16 (default 2).
    #[arg(long)]
    radius: Option<u8>,
    /// An animation in RON, such as `Pulse(period_ms: 2000, low: 60, high: 100)`.
    #[arg(long, value_parser = parse_animation)]
    animation: Option<AnimationSource>,
    /// Milliseconds.
    #[arg(long)]
    phase: Option<u32>,
}

#[derive(Subcommand)]
pub(crate) enum LightCommand {
    /// Add a light object (`--at`, `--colour`, `--intensity` and `--reach` needed); prints its index.
    Add {
        pack: PathBuf,
        #[command(flatten)]
        fields: LightFields,
    },
    /// Change light object INDEX.
    Set {
        pack: PathBuf,
        index: usize,
        #[command(flatten)]
        fields: LightFields,
    },
    /// Remove light object INDEX.
    Remove { pack: PathBuf, index: usize },
}

fn parse_colour(s: &str) -> Result<Colour, String> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{s:?} is not a colour RRGGBB"));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string());
    Ok(Colour(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)))
}

fn parse_ambient(s: &str) -> Result<(Colour, u16), String> {
    let (c, p) = s.split_once(':').ok_or_else(|| format!("{s:?} is not RRGGBB:PERCENT"))?;
    let p: u16 = p.parse().map_err(|_| format!("{p:?} is not a percent"))?;
    Ok((parse_colour(c)?, p))
}

fn parse_glow_colour(s: &str) -> Result<GlowColourSource, String> {
    if s == "own" { Ok(GlowColourSource::Own) } else { parse_colour(s).map(GlowColourSource::Tint) }
}

fn parse_reach(s: &str) -> Result<ReachSource, String> {
    if s == "unlimited" {
        return Ok(ReachSource::Unlimited);
    }
    s.parse().map(ReachSource::Pixels).map_err(|_| format!("{s:?} is not world pixels or `unlimited`"))
}

fn parse_animation(s: &str) -> Result<AnimationSource, String> {
    olr_format::ron_io::from_canonical("--animation", s).map_err(|e| e.to_string())
}

fn bail(dir: &Path, e: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("{}: {e}", dir.display())
}

/// `level look`.
pub(crate) fn look_cmd(a: &LookArgs) -> anyhow::Result<bool> {
    let dir = &a.pack;
    let mut pack = element_cmd::load_level(dir)?;
    if a.clear {
        let ids: Vec<String> = match &pack.source {
            Source::Level(l) => {
                l.look.iter().flat_map(|k| k.glow_groups.iter().map(|g| g.id.clone())).collect()
            }
            _ => Vec::new(),
        };
        for id in ids {
            look::remove_group(&mut pack, &id).map_err(|e| bail(dir, e))?;
        }
        let Source::Level(l) = &mut pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
        look::set_look(l, None).map_err(|e| bail(dir, e))?;
        element_cmd::bake_pack(dir, &mut pack, &a.content.content)?;
        return element_cmd::save_and_print(dir, &pack, &[]);
    }
    let Source::Level(l) = &mut pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let mut k = l.look.clone().unwrap_or_default();
    if let Some((colour, intensity)) = a.ambient {
        k.ambient = AmbientSource { colour: Some(colour), intensity };
    }
    k.max_light = a.max_light.unwrap_or(k.max_light);
    k.light_steps = a.steps.unwrap_or(k.light_steps);
    k.softness = a.softness.unwrap_or(k.softness);
    k.grade.exposure = a.exposure.unwrap_or(k.grade.exposure);
    k.grade.contrast = a.contrast.unwrap_or(k.grade.contrast);
    k.grade.saturation = a.saturation.unwrap_or(k.grade.saturation);
    k.grade.tint = a.tint.or(k.grade.tint);
    look::set_look(l, Some(k)).map_err(|e| bail(dir, e))?;
    element_cmd::save_and_print(dir, &pack, &[])
}

fn group_of(id: &str, f: &GroupFields, now: Option<GlowGroupSource>) -> anyhow::Result<GlowGroupSource> {
    let reach = f
        .reach
        .or(now.as_ref().map(|g| g.reach))
        .ok_or_else(|| anyhow::anyhow!("a new group needs --reach"))?;
    let now = now.unwrap_or(GlowGroupSource {
        id: id.into(),
        colour: GlowColourSource::Own,
        intensity: 100,
        reach,
        animation: AnimationSource::Steady,
        phase_ms: None,
    });
    Ok(GlowGroupSource {
        id: id.into(),
        colour: f.colour.unwrap_or(now.colour),
        intensity: f.intensity.unwrap_or(now.intensity),
        reach,
        animation: f.animation.clone().unwrap_or(now.animation),
        phase_ms: f.phase.or(now.phase_ms),
    })
}

/// `level group …`.
pub(crate) fn group_cmd(c: &GroupCommand) -> anyhow::Result<bool> {
    match c {
        GroupCommand::Add { pack: dir, id, fields } => {
            let mut pack = element_cmd::load_level(dir)?;
            let Source::Level(l) = &mut pack.source else {
                anyhow::bail!("{}: not a level pack", dir.display())
            };
            let i = look::add_group(l, group_of(id, fields, None)?).map_err(|e| bail(dir, e))?;
            element_cmd::save_and_print(dir, &pack, &[format!("added group {i}")])
        }
        GroupCommand::Set { pack: dir, id, fields, rename } => {
            let mut pack = element_cmd::load_level(dir)?;
            let Source::Level(l) = &mut pack.source else {
                anyhow::bail!("{}: not a level pack", dir.display())
            };
            let now = l.look.iter().flat_map(|k| k.glow_groups.iter()).find(|g| g.id == *id).cloned();
            let Some(now) = now else { anyhow::bail!("{}: no glow group {id:?}", dir.display()) };
            look::set_group(l, id, group_of(id, fields, Some(now))?).map_err(|e| bail(dir, e))?;
            if let Some(to) = rename {
                look::rename_group(&mut pack, id, to).map_err(|e| bail(dir, e))?;
            }
            element_cmd::save_and_print(dir, &pack, &[])
        }
        GroupCommand::Remove { pack: dir, id, content } => {
            let mut pack = element_cmd::load_level(dir)?;
            let gone = look::remove_group(&mut pack, id).map_err(|e| bail(dir, e))?;
            element_cmd::bake_pack(dir, &mut pack, &content.content)?;
            let messages: Vec<String> = gone.iter().map(|e| format!("unglowed element {e}")).collect();
            element_cmd::save_and_print(dir, &pack, &messages)
        }
    }
}

fn light_of(
    f: &LightFields,
    now: Option<((i32, i32), u8, LightSource)>,
) -> anyhow::Result<((i32, i32), u8, LightSource)> {
    let need = |what: &str| anyhow::anyhow!("a new light needs --{what}");
    let at = f.at.or(now.as_ref().map(|n| n.0)).ok_or_else(|| need("at"))?;
    let radius = f.radius.or(now.as_ref().map(|n| n.1)).unwrap_or(2);
    let old = now.map(|n| n.2);
    let light = LightSource {
        colour: f.colour.or(old.as_ref().map(|l| l.colour)).ok_or_else(|| need("colour"))?,
        intensity: f.intensity.or(old.as_ref().map(|l| l.intensity)).ok_or_else(|| need("intensity"))?,
        reach: f.reach.or(old.as_ref().map(|l| l.reach)).ok_or_else(|| need("reach"))?,
        animation: f.animation.clone().or(old.as_ref().map(|l| l.animation.clone())).unwrap_or_default(),
        phase_ms: f.phase.or(old.as_ref().and_then(|l| l.phase_ms)),
    };
    Ok((at, radius, light))
}

/// `level light …`.
pub(crate) fn light_cmd(c: &LightCommand) -> anyhow::Result<bool> {
    let (dir, op): (&Path, _) = match c {
        LightCommand::Add { pack, .. }
        | LightCommand::Set { pack, .. }
        | LightCommand::Remove { pack, .. } => (pack, c),
    };
    let mut pack = element_cmd::load_level(dir)?;
    let Source::Level(l) = &mut pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let messages = match op {
        LightCommand::Add { fields, .. } => {
            let (at, radius, light) = light_of(fields, None)?;
            let i = look::add_light(l, at, light, radius).map_err(|e| bail(dir, e))?;
            vec![format!("added object {i}")]
        }
        LightCommand::Set { index, fields, .. } => {
            let now = match l.objects.get(*index) {
                None => {
                    anyhow::bail!("{}: no object {index} (the level has {})", dir.display(), l.objects.len())
                }
                Some(o) => match o.as_light() {
                    Some((radius, light)) => (o.at(), radius, light),
                    None => anyhow::bail!("{}: object {index} is not a light", dir.display()),
                },
            };
            let (at, radius, light) = light_of(fields, Some(now))?;
            look::set_light(l, *index, at, light, radius).map_err(|e| bail(dir, e))?;
            Vec::new()
        }
        LightCommand::Remove { index, .. } => {
            if !l.objects.get(*index).is_some_and(|o| o.as_light().is_some()) {
                anyhow::bail!("{}: object {index} is not a light", dir.display());
            }
            olr_edit::level::remove_object(l, *index).map_err(|e| bail(dir, e))?;
            Vec::new()
        }
    };
    element_cmd::save_and_print(dir, &pack, &messages)
}
