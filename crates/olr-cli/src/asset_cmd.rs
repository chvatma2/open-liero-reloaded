//! `olr-cli asset save|place|check` (issue #9): the level editor's asset
//! library for agents. `save` cuts an asset out of a level pack (its
//! elements, or a world rectangle of every visible layer) and writes its
//! folder; `place` puts one into a level pack (one save, baked, as `level
//! element …`); `check` reads asset folders whole. The cut and the placing
//! are `olr_edit::asset`'s, shared with the editor. An asset carries the
//! glow groups and surface materials (issue #38) its elements name;
//! placing joins those the level has (by id, its own settings kept) and
//! adds the others. Issue #72: `state`, `expose`, `unexpose`, `sound`,
//! `script` and `health` edit an asset folder in place (read whole, one
//! `olr_edit::asset` operation, encoded with its thumbnail and written
//! through `replace_dir`; a refusal prints `<dir>: <why>`, exits 1 and
//! leaves the folder), and `info` lists what it holds.

use std::io::Read;
use std::path::{Path, PathBuf};

use clap::Subcommand;
use olr_data::level::Material;
use olr_edit::asset::{self, Asset, DecodedLevel, OwnPicture};
use olr_edit::compose::{Decoded, Rect};
use olr_format::assets::{
    ExposedSource, HealthSource, MAX_ASSET_SOUND_BYTES, MAX_SCRIPT_BYTES, PropertySource,
};
use olr_format::image_io::{self, LEVEL_ART};
use olr_format::levels::{ElementKind, ElementMaterial, element_box};

use crate::element_cmd::{ContentArg, bake_pack, load_level, save_and_print};
use crate::level_cmd::{parse_ints, parse_point};

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: AssetCommand,
}

#[derive(Subcommand)]
enum AssetCommand {
    /// Saves elements of a level pack, or a world rectangle cut from its
    /// visible layers, as an asset folder.
    Save {
        /// The level pack.
        pack: PathBuf,
        /// The elements, by id (comma-separated).
        #[arg(long, value_delimiter = ',', conflicts_with = "rect", required_unless_present = "rect")]
        elements: Vec<String>,
        /// A rectangle X,Y,W,H, world pixels, cut from every visible layer.
        #[arg(long, value_parser = parse_ints::<4>, allow_hyphen_values = true)]
        rect: Option<[i32; 4]>,
        /// With --rect: only these layers (ids, comma-separated), visible ones.
        #[arg(long, value_delimiter = ',', requires = "rect")]
        layers: Vec<String>,
        /// The asset's name.
        #[arg(long)]
        name: String,
        /// The asset folder to write (made anew).
        #[arg(long)]
        out: PathBuf,
    },
    /// Places an asset in a level pack with its top-left at a world pixel.
    Place {
        /// The level pack.
        pack: PathBuf,
        /// The asset folder.
        asset: PathBuf,
        /// The asset's top-left, world pixels X,Y.
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
        #[command(flatten)]
        content: ContentArg,
    },
    /// Reads asset folders whole; exits with status 1 if any does not.
    Check {
        /// The asset folders.
        #[arg(required = true)]
        assets: Vec<PathBuf>,
    },
    /// Edits an asset's states, in place.
    State {
        #[command(subcommand)]
        command: StateCommand,
    },
    /// Exposes a property under a name scripts and copies use, in place.
    Expose {
        /// The asset folder.
        dir: PathBuf,
        /// The name (`[a-z_][a-z0-9_]*`).
        name: String,
        /// The property: x, y, visible, facing, health or <layer>.<name>.
        #[arg(value_parser = asset::parse_property)]
        property: PropertySource,
        /// Its default (none: the layer's own setting): a whole number, true
        /// or false, #rrggbb, steady or an animation in RON.
        #[arg(long, value_parser = asset::parse_value, allow_hyphen_values = true)]
        default: Option<olr_format::assets::ValueSource>,
        /// Its least value (a number; none: the property's).
        #[arg(long, allow_hyphen_values = true)]
        min: Option<i32>,
        /// Its greatest value (a number; none: the property's).
        #[arg(long, allow_hyphen_values = true)]
        max: Option<i32>,
    },
    /// Stops exposing a name, in place.
    Unexpose {
        /// The asset folder.
        dir: PathBuf,
        /// The exposed name.
        name: String,
    },
    /// Adds or removes an asset's sounds, in place.
    Sound {
        #[command(subcommand)]
        command: SoundCommand,
    },
    /// Sets or clears an asset's default script (Rhai, not run), in place.
    Script {
        /// The asset folder.
        dir: PathBuf,
        /// The script file.
        #[arg(long, conflicts_with = "clear", required_unless_present = "clear")]
        set: Option<PathBuf>,
        /// Removes the script.
        #[arg(long)]
        clear: bool,
    },
    /// Sets or clears an asset's health, in place.
    Health {
        /// The asset folder.
        dir: PathBuf,
        /// Its points, 1 to 10000.
        #[arg(long, requires = "at_zero", conflicts_with = "clear", required_unless_present = "clear")]
        points: Option<u32>,
        /// The state entered at 0.
        #[arg(long, requires = "points")]
        at_zero: Option<String>,
        /// Removes the health.
        #[arg(long)]
        clear: bool,
    },
    /// Reads an asset folder whole and lists what it holds, one item a line.
    Info {
        /// The asset folder.
        dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum StateCommand {
    /// Adds a state at the end: a copy of the start's values owning nothing,
    /// or of another state's values, pictures and sounds.
    Add {
        /// The asset folder.
        dir: PathBuf,
        /// The new state's name.
        name: String,
        /// The state to copy.
        #[arg(long)]
        from: Option<String>,
    },
    /// Removes a state (never the start, nor health's state at 0).
    Remove {
        /// The asset folder.
        dir: PathBuf,
        /// The state.
        name: String,
    },
    /// Renames a state, makes it the start or sets its sounds.
    Set {
        /// The asset folder.
        dir: PathBuf,
        /// The state.
        name: String,
        /// Its new name.
        #[arg(long = "name")]
        new_name: Option<String>,
        /// Makes it the starting state (every state's look kept).
        #[arg(long)]
        start: bool,
        /// The sound looped while it lasts (one of the asset's), or none.
        #[arg(long)]
        loop_sound: Option<String>,
        /// The sound played when it is entered (one of the asset's), or none.
        #[arg(long)]
        enter_sound: Option<String>,
    },
    /// Sets or clears (`none`) one of a state's values.
    Value {
        /// The asset folder.
        dir: PathBuf,
        /// The state.
        state: String,
        /// The property: x, y, visible, facing, health or <layer>.<name>.
        #[arg(value_parser = asset::parse_property)]
        property: PropertySource,
        /// A whole number, true or false, #rrggbb, steady, an animation in
        /// RON, or none.
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// Paints a state's own picture of a drawn layer, or follows the start.
    Picture {
        /// The asset folder.
        dir: PathBuf,
        /// The state.
        state: String,
        /// The drawn layer.
        layer: String,
        /// The picture of the layer's box at art resolution (RGBA PNG, alpha
        /// 0 uncovered).
        #[arg(long, conflicts_with = "follow", required_unless_present = "follow")]
        art: Option<PathBuf>,
        /// A mixed layer's cells (material.png's palette, the box at world
        /// resolution); otherwise the start's.
        #[arg(long, requires = "art")]
        material: Option<PathBuf>,
        /// The back art (as the layer's own file); otherwise the start's.
        #[arg(long, requires = "art")]
        back: Option<PathBuf>,
        /// The glow mask (grey, as the layer's own file); otherwise the
        /// start's.
        #[arg(long, requires = "art")]
        glow: Option<PathBuf>,
        /// Drops the state's own picture: it shows the start's.
        #[arg(long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum SoundCommand {
    /// Adds a WAV or Ogg sound.
    Add {
        /// The asset folder.
        dir: PathBuf,
        /// The sound file.
        file: PathBuf,
        /// Its name in the asset (default: the file's name, lower-cased).
        #[arg(long)]
        name: Option<String>,
    },
    /// Removes a sound no state plays.
    Remove {
        /// The asset folder.
        dir: PathBuf,
        /// The sound's name.
        name: String,
    },
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        AssetCommand::Save { pack, elements, rect, layers, name, out } => {
            save(pack, elements, *rect, layers, name, out)
        }
        AssetCommand::Place { pack, asset, at, content } => place(pack, asset, *at, &content.content),
        AssetCommand::Check { assets } => Ok(check(assets)),
        AssetCommand::State { command } => state(command),
        AssetCommand::Expose { dir, name, property, default, min, max } => edit(dir, |a| {
            let x = ExposedSource {
                name: name.clone(),
                property: property.clone(),
                default: default.clone(),
                min: *min,
                max: *max,
            };
            asset::expose(a, x).map_err(|e| e.to_string())?;
            Ok(vec![format!("exposed {name}: {}", asset::property_label(property))])
        }),
        AssetCommand::Unexpose { dir, name } => edit(dir, |a| {
            asset::unexpose(a, name).map_err(|e| e.to_string())?;
            Ok(vec![format!("unexposed {name}")])
        }),
        AssetCommand::Sound { command: SoundCommand::Add { dir, file, name } } => edit(dir, |a| {
            let name = match name {
                Some(n) => n.clone(),
                None => file.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default(),
            };
            let bytes = read_bounded(file, MAX_ASSET_SOUND_BYTES)?;
            let length = seconds(&bytes);
            asset::add_sound(a, &name, bytes).map_err(|e| e.to_string())?;
            Ok(vec![format!("added sound {name}: {length}")])
        }),
        AssetCommand::Sound { command: SoundCommand::Remove { dir, name } } => edit(dir, |a| {
            asset::remove_sound(a, name).map_err(|e| e.to_string())?;
            Ok(vec![format!("removed sound {name}")])
        }),
        AssetCommand::Script { dir, set, .. } => edit(dir, |a| {
            let script = match set {
                Some(f) => Some(
                    String::from_utf8(read_bounded(f, MAX_SCRIPT_BYTES)?)
                        .map_err(|_| format!("{}: not UTF-8", f.display()))?,
                ),
                None => None,
            };
            let said = match &script {
                Some(s) => format!("set the script: {}", lines(s)),
                None => "cleared the script".to_string(),
            };
            asset::set_script(a, script).map_err(|e| e.to_string())?;
            Ok(vec![said])
        }),
        AssetCommand::Health { dir, points, at_zero, .. } => edit(dir, |a| {
            let health = match (points, at_zero) {
                (Some(points), Some(at_zero)) => {
                    Some(HealthSource { points: *points, at_zero: at_zero.clone() })
                }
                _ => None,
            };
            let said = match &health {
                Some(h) => format!("health {}, at 0 {}", h.points, h.at_zero),
                None => "cleared the health".to_string(),
            };
            asset::set_health(a, health).map_err(|e| e.to_string())?;
            Ok(vec![said])
        }),
        AssetCommand::Info { dir } => match asset::load_dir(dir) {
            Ok(a) => {
                for line in info(&a) {
                    println!("{line}");
                }
                Ok(true)
            }
            Err(e) => refuse(dir, &e),
        },
    }
}

fn state(command: &StateCommand) -> anyhow::Result<bool> {
    match command {
        StateCommand::Add { dir, name, from } => edit(dir, |a| {
            asset::add_state(a, name, from.as_deref()).map_err(|e| e.to_string())?;
            Ok(vec![format!("added state {name}")])
        }),
        StateCommand::Remove { dir, name } => edit(dir, |a| {
            asset::remove_state(a, name).map_err(|e| e.to_string())?;
            Ok(vec![format!("removed state {name}")])
        }),
        StateCommand::Set { dir, name, new_name, start, loop_sound, enter_sound } => edit(dir, |a| {
            let sound = |s: &Option<String>| s.as_ref().map(|s| (s != "none").then(|| s.clone()));
            let mut said = Vec::new();
            if loop_sound.is_some() || enter_sound.is_some() {
                asset::set_state_sounds(a, name, sound(loop_sound), sound(enter_sound))
                    .map_err(|e| e.to_string())?;
                for (what, s) in [("loops", loop_sound), ("enters", enter_sound)] {
                    if let Some(s) = s {
                        said.push(format!("state {name} {what} {s}"));
                    }
                }
            }
            if *start {
                asset::set_start(a, name).map_err(|e| e.to_string())?;
                said.push(format!("state {name} is the start"));
            }
            if let Some(new) = new_name {
                asset::rename_state(a, name, new).map_err(|e| e.to_string())?;
                said.push(format!("renamed state {name} to {new}"));
            }
            if said.is_empty() {
                return Err(format!(
                    "state {name:?}: nothing to set (--name, --start, --loop-sound, --enter-sound)"
                ));
            }
            Ok(said)
        }),
        StateCommand::Value { dir, state, property, value } => edit(dir, |a| {
            let label = asset::property_label(property);
            let v = match value.as_str() {
                "none" => None,
                v => Some(asset::parse_value(v)?),
            };
            let said = match &v {
                Some(v) => format!("state {state}: {label} = {}", asset::value_label(v)),
                None => format!("state {state}: {label} cleared"),
            };
            asset::set_state_value(a, state, property.clone(), v).map_err(|e| e.to_string())?;
            Ok(vec![said])
        }),
        StateCommand::Picture { dir, state, layer, art, material, back, glow, follow } => edit(dir, |a| {
            if *follow {
                asset::follow_start(a, state, layer).map_err(|e| e.to_string())?;
                return Ok(vec![format!("state {state} follows the start for {layer}")]);
            }
            let Some(art) = art else { return Err("--art or --follow".into()) };
            let picture = read_picture(a, layer, art, material.as_deref(), back.as_deref(), glow.as_deref())?;
            asset::own_picture(a, state, layer, picture).map_err(|e| e.to_string())?;
            Ok(vec![format!("state {state} paints {layer}")])
        }),
    }
}

/// Prints `<dir>: <why>` and fails.
fn refuse(dir: &Path, why: &dyn std::fmt::Display) -> anyhow::Result<bool> {
    eprintln!("{}: {why}", dir.display());
    Ok(false)
}

/// Asset folder `dir` read whole, edited by `f` (which returns what to
/// print), encoded with its thumbnail and written through `replace_dir`;
/// a refusal at any step leaves the folder as it was.
fn edit(dir: &Path, f: impl FnOnce(&mut Asset) -> Result<Vec<String>, String>) -> anyhow::Result<bool> {
    let mut a = match asset::load_dir(dir) {
        Ok(a) => a,
        Err(e) => return refuse(dir, &e),
    };
    let said = match f(&mut a) {
        Ok(s) => s,
        Err(e) => return refuse(dir, &e),
    };
    let src = match asset::encode(&a) {
        Ok(s) => s,
        Err(e) => return refuse(dir, &e),
    };
    match asset::save_dir(dir, &src) {
        Ok(w) => crate::pack_cmd::warn(&w),
        Err(e) => return refuse(dir, &e),
    }
    for line in said {
        println!("{line}");
    }
    Ok(true)
}

/// File `path`'s bytes, at most `max` (more is refused before it is read
/// whole).
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let f = std::fs::File::open(path).map_err(|e| fail(&e))?;
    let mut out = Vec::new();
    f.take(max + 1).read_to_end(&mut out).map_err(|e| fail(&e))?;
    if out.len() as u64 > max {
        return Err(fail(&format!("more than {max} bytes")));
    }
    Ok(out)
}

/// A PNG at `path` decoded by `decode`.
fn png<T>(path: &Path, decode: impl FnOnce(&[u8]) -> Result<T, String>) -> Result<T, String> {
    let bytes = read_bounded(path, olr_format::ron_io::MAX_FILE_BYTES)?;
    decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// State picture of `layer` from the files: `art`, else the start's cells,
/// back art and glow mask (a misfit file names the size it should be).
fn read_picture(
    a: &Asset,
    layer: &str,
    art: &Path,
    material: Option<&Path>,
    back: Option<&Path>,
    glow: Option<&Path>,
) -> Result<OwnPicture, String> {
    let p = a.pieces.iter().find(|p| p.element.id == layer).ok_or_else(|| format!("no layer {layer:?}"))?;
    let s = a.art_scale;
    let (_, (w, h)) = element_box(&p.element.kind, a.size).unwrap_or(((0, 0), a.size));
    let (start_cells, start_back) = match &p.decoded {
        Decoded::Drawn { cells, back, .. } => (cells.clone(), back.clone()),
        _ => (None, None),
    };
    let misfit = |f: &Path, got: (u32, u32), want: (u32, u32)| {
        format!("{}: {} × {}: {layer:?}'s box takes {} × {}", f.display(), got.0, got.1, want.0, want.1)
    };
    let art = png(art, |b| image_io::decode_rgba_within(b, LEVEL_ART))?;
    let cells = match material {
        Some(f) => {
            let (mw, mh, idx) = png(f, |b| image_io::decode_indexed_within(b, LEVEL_ART))?;
            if (mw, mh) != (w, h) {
                return Err(misfit(f, (mw, mh), (w, h)));
            }
            let cells: Option<Vec<Material>> = idx.iter().map(|&i| Material::from_index(i)).collect();
            Some(cells.ok_or_else(|| format!("{}: a palette index that is no material", f.display()))?)
        }
        None => start_cells,
    };
    let back = match back {
        Some(f) => {
            let b = png(f, |b| image_io::decode_rgba_within(b, LEVEL_ART))?;
            if (b.width(), b.height()) != (w * s, h * s) {
                return Err(misfit(f, (b.width(), b.height()), (w * s, h * s)));
            }
            Some(b)
        }
        None => start_back,
    };
    let glow_mask = match glow {
        Some(f) => {
            let (gw, gh, v) = png(f, |b| image_io::decode_grey_within(b, LEVEL_ART))?;
            if (gw, gh) != (w * s, h * s) {
                return Err(misfit(f, (gw, gh), (w * s, h * s)));
            }
            Some(std::sync::Arc::new(v))
        }
        None => None,
    };
    Ok(OwnPicture { decoded: Decoded::Drawn { art, cells, back }, glow_mask })
}

/// "1.4 s", or "length unknown".
fn seconds(bytes: &[u8]) -> String {
    match olr_pack::sounds::sound_seconds(bytes) {
        Some(s) if s.is_finite() => format!("{s:.1} s"),
        _ => "length unknown".into(),
    }
}

/// "1 line", "18 lines".
fn lines(s: &str) -> String {
    match s.lines().count() {
        1 => "1 line".into(),
        n => format!("{n} lines"),
    }
}

/// `asset info`'s lines: the asset, its layers, states, exposed entries,
/// sounds, health and script, one item a line.
fn info(a: &Asset) -> Vec<String> {
    let mut out = vec![format!(
        "{:?}, {} × {}, art scale {}, {}",
        a.name,
        a.size.0,
        a.size.1,
        a.art_scale,
        count_elements(a.pieces.len())
    )];
    for p in &a.pieces {
        let e = &p.element;
        let material = |m: &ElementMaterial| match m {
            ElementMaterial::Rock => "rock",
            ElementMaterial::Dirt => "dirt",
            ElementMaterial::Background => "background",
        };
        let mut parts = vec![match &e.kind {
            ElementKind::Image { material: m, .. } => format!("{} image", material(m)),
            ElementKind::Drawn { mixed: true, .. } => "mixed".into(),
            ElementKind::Drawn { material: m, .. } => material(m).into(),
            ElementKind::Raster => "raster".into(),
        }];
        if e.has_back_art() {
            parts.push("back art".into());
        }
        if let Some(g) = &e.glow {
            let others: Vec<&str> = a
                .pieces
                .iter()
                .filter(|o| {
                    o.element.id != e.id && o.element.glow.as_ref().is_some_and(|x| x.group == g.group)
                })
                .map(|o| o.element.id.as_str())
                .collect();
            let also = match others.len() {
                0 => String::new(),
                _ => format!(" (also: {})", others.join(", ")),
            };
            parts.push(format!("glows in {}{also}", g.group));
        }
        if e.glass.is_some() {
            parts.push("glass".into());
        }
        out.push(format!("layer {}: {}", e.id, parts.join(", ")));
    }
    for (i, st) in a.states.iter().enumerate() {
        let star = if i == a.start { " ★" } else { "" };
        let mut parts = Vec::new();
        if !st.own.is_empty() {
            parts.push(format!("owns {}", st.own.keys().cloned().collect::<Vec<_>>().join(", ")));
        }
        for v in &st.values {
            parts.push(format!("{} = {}", asset::property_label(&v.property), asset::value_label(&v.value)));
        }
        if let Some(s) = &st.loop_sound {
            parts.push(format!("loops {s}"));
        }
        if let Some(s) = &st.enter_sound {
            parts.push(format!("enters {s}"));
        }
        let rest = match parts.is_empty() {
            true => String::new(),
            false => format!(": {}", parts.join("; ")),
        };
        out.push(format!("state {}{star}{rest}", st.name));
    }
    for x in &a.exposed {
        let default = match asset::default_value(a, &x.property) {
            Some(v) => asset::value_label(&v),
            None => "none".into(),
        };
        let range = match (x.min, x.max) {
            (Some(lo), Some(hi)) => format!(", {lo} to {hi}"),
            _ => String::new(),
        };
        out.push(format!(
            "exposed {}: {}, default {default}{range}",
            x.name,
            asset::property_label(&x.property)
        ));
    }
    for (name, bytes) in &a.sounds {
        out.push(format!("sound {name}: {}", seconds(bytes)));
    }
    if let Some(h) = &a.health {
        out.push(format!("health {}, at 0 {}", h.points, h.at_zero));
    }
    if let Some(s) = &a.script {
        out.push(format!("script: {}", lines(s)));
    }
    out
}

fn save(
    dir: &Path,
    elements: &[String],
    rect: Option<[i32; 4]>,
    layers: &[String],
    name: &str,
    out: &Path,
) -> anyhow::Result<bool> {
    let fail = |e: &dyn std::fmt::Display| anyhow::anyhow!("{}: {e}", dir.display());
    let pack = load_level(dir)?;
    let level = DecodedLevel::of_pack(&pack).map_err(|e| fail(&e))?;
    let made = match rect {
        Some([x, y, w, h]) => {
            let (w, h) = (u32::try_from(w).unwrap_or(0), u32::try_from(h).unwrap_or(0));
            // Off the level's top-left, the rectangle starts at it.
            let (x0, y0) = (x.max(0), y.max(0));
            let cut = |v: i32, v0: i32, side: u32| side.saturating_sub(v0.abs_diff(v));
            let r = Rect { x: x0 as u32, y: y0 as u32, w: cut(x, x0, w), h: cut(y, y0, h) };
            let only = (!layers.is_empty()).then_some(layers);
            asset::cut_rect(&level.level(), r, only, name)
        }
        None => asset::from_elements(&level.level(), elements, name),
    }
    .map_err(|e| fail(&e))?;
    let src = asset::encode(&made).map_err(|e| fail(&e))?;
    crate::pack_cmd::warn(
        &asset::save_dir(out, &src).map_err(|e| anyhow::anyhow!("{}: {e}", out.display()))?,
    );
    let list = |label: &str, ids: Vec<&str>| match ids.len() {
        0 => String::new(),
        _ => format!(", {label} {}", ids.join(", ")),
    };
    println!(
        "saved asset {:?} ({} × {}, {}{}{}) to {}",
        made.name,
        made.size.0,
        made.size.1,
        count_elements(made.pieces.len()),
        list("glow groups", made.groups.iter().map(|g| g.id.as_str()).collect()),
        list("materials", made.materials.iter().map(|m| m.id.as_str()).collect()),
        out.display()
    );
    Ok(true)
}

fn place(dir: &Path, asset_dir: &Path, at: (i32, i32), content: &Path) -> anyhow::Result<bool> {
    let a = asset::load_dir(asset_dir).map_err(|e| anyhow::anyhow!("{}: {e}", asset_dir.display()))?;
    let mut pack = load_level(dir)?;
    let placed =
        asset::place_in_pack(&mut pack, &a, at).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    bake_pack(dir, &mut pack, content)?;
    let mut messages: Vec<String> = placed.ids.iter().map(|id| format!("added element {id}")).collect();
    if let Some(f) = &placed.folder {
        messages.push(format!("added folder {f}"));
    }
    for g in &placed.groups.added {
        messages.push(format!("added glow group {g}"));
    }
    for g in &placed.groups.joined {
        messages.push(format!("joined glow group {g}"));
    }
    for m in &placed.materials.added {
        messages.push(format!("added material {m}"));
    }
    for m in &placed.materials.joined {
        messages.push(format!("joined material {m}"));
    }
    if placed.groups.added_look {
        messages.push(olr_edit::asset::ADDED_LIGHTING.to_string());
    }
    if placed.materials.added_look && !placed.groups.added_look {
        messages.push("added a look to hold the asset's materials (the level plays lit)".to_string());
    }
    messages.extend(placed.note);
    save_and_print(dir, &pack, &messages)
}

fn check(dirs: &[PathBuf]) -> bool {
    let mut ok = true;
    for dir in dirs {
        match asset::load_dir(dir) {
            Ok(a) => {
                let materials = match a.materials.len() {
                    0 => String::new(),
                    _ => format!(
                        ", materials {}",
                        a.materials.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                };
                // Issue #71: a stateless asset's line is as before.
                let states = match a.states.len() {
                    1 => String::new(),
                    n => format!(", {n} states"),
                };
                println!(
                    "{}: {:?}, {} × {}, {}{states}{materials}",
                    dir.display(),
                    a.name,
                    a.size.0,
                    a.size.1,
                    count_elements(a.pieces.len())
                )
            }
            Err(e) => {
                ok = false;
                println!("{}: error: {e}", dir.display());
            }
        }
    }
    ok
}

/// "1 element", "3 elements".
fn count_elements(n: usize) -> String {
    match n {
        1 => "1 element".into(),
        n => format!("{n} elements"),
    }
}

#[cfg(test)]
mod tests {
    use olr_format::Colour;
    use olr_format::assets::{PropertySource as P, ValueSource as V};
    use olr_format::look::AnimationSource;

    use super::asset::{parse_property, parse_value, property_label, value_label};

    #[test]
    fn cli_parses_property_and_value_syntax() {
        let l = || "door.left".to_string();
        let all = [
            ("x", P::X),
            ("y", P::Y),
            ("visible", P::Visible),
            ("facing", P::Facing),
            ("health", P::Health),
            ("door.left.x", P::OffsetX(l())),
            ("door.left.y", P::OffsetY(l())),
            ("door.left.visible", P::LayerVisible(l())),
            ("door.left.opacity", P::Opacity(l())),
            ("door.left.glow", P::GlowStrength(l())),
            ("door.left.glow-colour", P::GlowColour(l())),
            ("door.left.light", P::LightIntensity(l())),
            ("door.left.reach", P::LightReach(l())),
            ("door.left.animation", P::GlowAnimation(l())),
            ("door.left.beam-direction", P::BeamDirection(l())),
            ("door.left.beam-spread", P::BeamSpread(l())),
            ("door.left.glass-opacity", P::GlassOpacity(l())),
            ("door.left.glass-tint", P::GlassTint(l())),
        ];
        for (text, p) in &all {
            assert_eq!(parse_property(text).as_ref(), Ok(p), "{text}");
            assert_eq!(property_label(p), *text);
        }
        // A plain id; the last dot splits.
        assert_eq!(parse_property("lamp.glow"), Ok(P::GlowStrength("lamp".into())));
        assert_eq!(parse_property("a.x.x"), Ok(P::OffsetX("a.x".into())));
        for bad in ["", "colour", "lamp.colour", ".glow", "lamp.", "lamp.Glow"] {
            let e = parse_property(bad).unwrap_err();
            assert!(e.contains(&format!("{bad:?}")) && e.contains("<layer>"), "{bad}: {e}");
        }
        let flicker = AnimationSource::Flicker { speed: 40, depth: 60 };
        let values = [
            ("0", V::Int(0)),
            ("-12", V::Int(-12)),
            ("true", V::Bool(true)),
            ("false", V::Bool(false)),
            ("#40ff80", V::Colour(Colour::parse("#40ff80").unwrap())),
            ("steady", V::Animation(AnimationSource::Steady)),
            ("Flicker(speed: 40, depth: 60)", V::Animation(flicker)),
        ];
        for (text, v) in &values {
            assert_eq!(parse_value(text).as_ref(), Ok(v), "{text}");
            assert_eq!(value_label(v), *text);
        }
        // Every animation's label reads back.
        for text in [
            "Pulse(period_ms: 1000, low: 20, high: 100)",
            "Keys(keys: [(0, 100), (500, 0, \"#ff0000\")], repeat: true)",
        ] {
            let v = parse_value(text).unwrap();
            assert_eq!(parse_value(&value_label(&v)), Ok(v), "{text}");
        }
        for bad in ["", "yes", "1.5", "#12", "Flicker(speed: 40)", "99999999999"] {
            assert!(parse_value(bad).is_err(), "{bad}");
        }
    }
}
