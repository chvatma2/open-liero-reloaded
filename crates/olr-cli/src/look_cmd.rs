//! `olr-cli level look|group …` (look system, plan 15a Task 7): a level's
//! look settings and glow groups (its only light, plan 15c), through
//! `olr_edit::look`. Each loads the pack, applies, rebakes a composed level
//! when its glow changed, and saves; a refused edit writes nothing.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use olr_data::image::Rgba;
use olr_edit::look;
use olr_edit::pack::Source;
use olr_format::Colour;
use olr_format::look::{
    AmbientSource, AnimationSource, GlowColourSource, GlowGroupSource, OcclusionSource, ReachSource,
    SolidFloorSource,
};

use crate::element_cmd::{self, ContentArg};

/// `level look`'s flags: each sets one field of the look (made when the
/// level has none).
#[derive(clap::Args)]
pub(crate) struct LookArgs {
    pack: PathBuf,
    /// The ambient light, RRGGBB:PERCENT (0-100).
    #[arg(long, value_parser = parse_ambient)]
    ambient: Option<(Colour, u16)>,
    /// Ambient occlusion, STRENGTH[,REACH]: percent of the ambient light
    /// taken from air against rock and dirt (1-100), fading out over REACH
    /// world pixels (2-32, default 12).
    #[arg(long, value_parser = parse_occlusion)]
    occlusion: Option<OcclusionSource>,
    /// Remove the ambient occlusion.
    #[arg(long, conflicts_with = "occlusion")]
    no_occlusion: bool,
    /// The brightest light, percent, 100-200.
    #[arg(long)]
    max_light: Option<u16>,
    /// Bands of light: 0 (off) or 2-32.
    #[arg(long)]
    steps: Option<u8>,
    /// Light blur, world pixels, 0-4.
    #[arg(long)]
    softness: Option<u8>,
    /// How deep light soaks into solids, world pixels, 0-12.
    #[arg(long)]
    soak: Option<u8>,
    /// The least light of an opaque solid, RRGGBB:PERCENT (0-100).
    #[arg(long, value_parser = parse_ambient)]
    solid_floor: Option<(Colour, u16)>,
    /// Hundredths of a stop, -200 to 200.
    #[arg(long, allow_hyphen_values = true)]
    exposure: Option<i16>,
    /// Percent, 0-200.
    #[arg(long)]
    contrast: Option<u16>,
    /// Percent, 0-200.
    #[arg(long)]
    saturation: Option<u16>,
    /// The haze of directional glow, percent 0-100 (`level haze paint`
    /// scales it per pixel).
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
    haze: Option<u8>,
    /// The grade's tint, RRGGBB.
    #[arg(long, value_parser = parse_colour)]
    tint: Option<Colour>,
    /// Remove the look: its settings and every glow group (their elements
    /// stop glowing).
    #[arg(long, conflicts_with_all = ["ambient", "occlusion", "no_occlusion", "max_light", "steps", "softness", "soak", "solid_floor", "exposure", "contrast", "saturation", "tint", "haze"])]
    clear: bool,
    #[command(flatten)]
    content: ContentArg,
}

/// A glow group's flags (`set` keeps what is not given).
#[derive(clap::Args)]
pub(crate) struct GroupFields {
    /// `own` (each pixel's art colour), RRGGBB, or `filtered:RRGGBB` (the
    /// colour by each pixel's art, its brightest channel full).
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

fn parse_occlusion(s: &str) -> Result<OcclusionSource, String> {
    let (strength, reach) = match s.split_once(',') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let strength = strength.trim().parse().map_err(|_| format!("{strength:?} is not a percent"))?;
    let reach = match reach {
        Some(r) => r.trim().parse().map_err(|_| format!("{r:?} is not world pixels"))?,
        None => olr_format::look::DEFAULT_OCCLUSION_REACH,
    };
    Ok(OcclusionSource { strength, reach })
}

fn parse_glow_colour(s: &str) -> Result<GlowColourSource, String> {
    match s.strip_prefix("filtered:") {
        Some(c) => parse_colour(c).map(GlowColourSource::Filtered),
        None if s == "own" => Ok(GlowColourSource::Own),
        None => parse_colour(s).map(GlowColourSource::Tint),
    }
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

/// Refreshes the level's thumbnail (lit by its look, the user's note
/// 2026-10-19) and saves, then prints `messages`.
pub(crate) fn save(dir: &Path, pack: &mut olr_edit::pack::Pack, messages: &[String]) -> anyhow::Result<bool> {
    if let Source::Level(l) = &mut pack.source {
        olr_edit::level::refresh_thumb(l);
    }
    element_cmd::save_and_print(dir, pack, messages)
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
        return save(dir, &mut pack, &[]);
    }
    let Source::Level(l) = &mut pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let mut k = l.look.clone().unwrap_or_default();
    if let Some((colour, intensity)) = a.ambient {
        k.ambient = AmbientSource { colour: Some(colour), intensity };
    }
    if a.occlusion.is_some() || a.no_occlusion {
        k.occlusion = a.occlusion;
    }
    k.max_light = a.max_light.unwrap_or(k.max_light);
    k.light_steps = a.steps.unwrap_or(k.light_steps);
    k.softness = a.softness.unwrap_or(k.softness);
    k.light_soak = a.soak.unwrap_or(k.light_soak);
    if let Some((colour, intensity)) = a.solid_floor {
        k.solid_floor = SolidFloorSource { colour: Some(colour), intensity };
    }
    k.grade.exposure = a.exposure.unwrap_or(k.grade.exposure);
    k.grade.contrast = a.contrast.unwrap_or(k.grade.contrast);
    k.grade.saturation = a.saturation.unwrap_or(k.grade.saturation);
    k.grade.tint = a.tint.or(k.grade.tint);
    k.haze = a.haze.unwrap_or(k.haze);
    look::set_look(l, Some(k)).map_err(|e| bail(dir, e))?;
    save(dir, &mut pack, &[])
}

/// `level haze paint|clear`: the per-pixel scale of the look's `haze`.
#[derive(Subcommand)]
pub(crate) enum HazeCommand {
    /// Paint the haze map over a shape: 0 none to 100 the look's haze. A new map starts at 100 everywhere
    Paint {
        pack: PathBuf,
        #[command(flatten)]
        shape: crate::level_cmd::ShapeArgs,
        /// Percent of the look's haze, 0-100.
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        strength: u8,
    },
    /// Remove the haze map.
    Clear { pack: PathBuf },
}

/// `level haze …`.
pub(crate) fn haze_cmd(c: &HazeCommand) -> anyhow::Result<bool> {
    let dir = match c {
        HazeCommand::Paint { pack, .. } | HazeCommand::Clear { pack } => pack,
    };
    let mut pack = element_cmd::load_level(dir)?;
    let Source::Level(l) = &mut pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let map = match c {
        HazeCommand::Clear { .. } => None,
        HazeCommand::Paint { shape, strength, .. } => {
            let (w, h) = l.size;
            let mut map = l.haze.clone().filter(|m| m.len() as u64 == u64::from(w) * u64::from(h));
            let value = (u32::from(*strength) * 255 + 50) / 100;
            let value = value as u8;
            // A new map starts full, the look's haze everywhere (ruling
            // T6a, 2026-10-25), so painting one area never takes the haze
            // from the rest.
            let map = map.get_or_insert_with(|| vec![255u8; w as usize * h as usize]);
            let shape = shape.shape();
            if let Some(r) = olr_edit::level::shape_bounds(&shape, (w, h)) {
                for y in r.y..r.y + r.h {
                    for x in r.x..r.x + r.w {
                        if olr_edit::level::covers(&shape, i64::from(x), i64::from(y)) {
                            map[y as usize * w as usize + x as usize] = value;
                        }
                    }
                }
            }
            Some(std::mem::take(map))
        }
    };
    look::set_haze_map(l, map).map_err(|e| bail(dir, e))?;
    save(dir, &mut pack, &[])
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
            save(dir, &mut pack, &[format!("added group {i}")])
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
            save(dir, &mut pack, &[])
        }
        GroupCommand::Remove { pack: dir, id, content } => {
            let mut pack = element_cmd::load_level(dir)?;
            let gone = look::remove_group(&mut pack, id).map_err(|e| bail(dir, e))?;
            element_cmd::bake_pack(dir, &mut pack, &content.content)?;
            let messages: Vec<String> = gone.iter().map(|e| format!("unglowed element {e}")).collect();
            save(dir, &mut pack, &messages)
        }
    }
}
