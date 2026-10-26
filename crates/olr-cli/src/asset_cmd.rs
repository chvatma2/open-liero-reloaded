//! `olr-cli asset save|place|check` (issue #9): the level editor's asset
//! library for agents. `save` cuts an asset out of a level pack (its
//! elements, or a world rectangle of every visible layer) and writes its
//! folder; `place` puts one into a level pack (one save, baked, as `level
//! element …`); `check` reads asset folders whole. The cut and the placing
//! are `olr_edit::asset`'s, shared with the editor. An asset carries the
//! glow groups and surface materials (issue #38) its elements name;
//! placing joins those the level has (by id, its own settings kept) and
//! adds the others.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use olr_edit::asset::{self, DecodedLevel};
use olr_edit::compose::Rect;

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
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        AssetCommand::Save { pack, elements, rect, layers, name, out } => {
            save(pack, elements, *rect, layers, name, out)
        }
        AssetCommand::Place { pack, asset, at, content } => place(pack, asset, *at, &content.content),
        AssetCommand::Check { assets } => Ok(check(assets)),
    }
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
