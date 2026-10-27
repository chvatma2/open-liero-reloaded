//! `olr-cli level copy …` and `level script` (issue #75): a level's placed
//! copies of assets edited by agents, through `olr_edit::placed`'s copy
//! operations (shared with the level tab). Each command loads the level
//! (an unconverted level of OLR 0.2 refused: "run olr-cli level
//! convert"), gives a plain level its ground first
//! (`element::ensure_composed`, as `level element` does), applies, bakes
//! (`placed::bake_pack`, the theme's back tile from `--content`) and saves,
//! then prints one line a change. A refusal writes nothing and exits 1.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use olr_edit::asset::{self, parse_value, value_label};
use olr_edit::pack::{Pack, Source};
use olr_edit::placed::{self, CopyProperty, Size};
use olr_format::assets::{MAX_SCRIPT_BYTES, ValueSource, valid_asset_id};
use olr_format::levels::{AssetOrigin, placed_size};

use crate::asset_cmd::{lines, read_bounded};
use crate::element_cmd::{ContentArg, bake_pack, load_level, save_and_print};
use crate::level_cmd::parse_point;
use crate::pack_cmd::parse_size;

/// A copy's size: one of `--width`, `--height` and `--size`.
#[derive(Args)]
#[group(multiple = false)]
pub(crate) struct SizeArgs {
    /// This width, world pixels, the height following the asset's aspect.
    #[arg(long)]
    width: Option<u32>,
    /// This height, world pixels, the width following the asset's aspect.
    #[arg(long)]
    height: Option<u32>,
    /// This size WxH, world pixels, the aspect free.
    #[arg(long, value_parser = parse_size)]
    size: Option<(u32, u32)>,
}

impl SizeArgs {
    fn size(&self) -> Option<Size> {
        match (self.width, self.height, self.size) {
            (Some(w), ..) => Some(Size::Width(w)),
            (None, Some(h), _) => Some(Size::Height(h)),
            (None, None, Some((w, h))) => Some(Size::Free(w, h)),
            (None, None, None) => None,
        }
    }
}

/// `resize`'s size: one of `--width`, `--height`, `--size` and `--natural`.
#[derive(Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ResizeArgs {
    /// This width, world pixels, the height following the asset's aspect.
    #[arg(long)]
    width: Option<u32>,
    /// This height, world pixels, the width following the asset's aspect.
    #[arg(long)]
    height: Option<u32>,
    /// This size WxH, world pixels, the aspect free.
    #[arg(long, value_parser = parse_size)]
    size: Option<(u32, u32)>,
    /// The asset's own size.
    #[arg(long)]
    natural: bool,
}

/// A script to set, or `--clear`.
#[derive(Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ScriptArgs {
    /// The Rhai file to set (UTF-8, at most 64 KiB; not run).
    #[arg(long)]
    set: Option<PathBuf>,
    /// Clear the script.
    #[arg(long)]
    clear: bool,
}

impl ScriptArgs {
    fn script(&self) -> anyhow::Result<Option<String>> {
        let Some(f) = &self.set else { return Ok(None) };
        let bytes = read_bounded(f, MAX_SCRIPT_BYTES).map_err(anyhow::Error::msg)?;
        Ok(Some(String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("{}: not UTF-8", f.display()))?))
    }
}

/// `--default-script`'s value.
#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum OnOff {
    On,
    Off,
}

#[derive(Subcommand)]
pub(crate) enum CopyCommand {
    /// Place a copy of an asset folder with its top-left at a world pixel,
    /// on top; prints `added copy <id> of "<name>" (asset <key>[, new])`
    /// and the look's changes. An asset already in the level (the same
    /// origin and files) is reused.
    Add {
        pack: PathBuf,
        /// The asset folder: in the built-in library (`assets/` beside
        /// `--content`), the user's (`--user`) or elsewhere (the level's
        /// own).
        asset: PathBuf,
        /// The copy's top-left, world pixels X,Y.
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
        /// The copy's id (default: from the asset's name).
        #[arg(long)]
        id: Option<String>,
        /// The key of an asset new to the level (default: the first free
        /// of a, b, … z, aa, …); refused when the asset is reused.
        #[arg(long)]
        key: Option<String>,
        #[command(flatten)]
        size: SizeArgs,
        /// The user's asset library (default: `assets/` in the game's data
        /// directory, such as `~/.local/share/open-liero-reloaded/assets`).
        #[arg(long)]
        user: Option<PathBuf>,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Print each copy, bottom first: `<id>: "<name>" <key> (<origin>) at
    /// X,Y W × H[ free][ live][ state S][ <name>=<value>…][ script][
    /// no-default-script][ distant D][ locked]`; then the level script.
    List { pack: PathBuf },
    /// Move a copy's top-left to a world pixel.
    Move {
        pack: PathBuf,
        id: String,
        /// The new top-left, world pixels X,Y.
        #[arg(long, value_parser = parse_point, allow_hyphen_values = true)]
        at: (i32, i32),
        #[command(flatten)]
        content: ContentArg,
    },
    /// Resize a copy (the asset stretched to it).
    Resize {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        size: ResizeArgs,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Set a copy's id, Live, state, values, default script, depth and
    /// lock, checked together and saved once.
    Set {
        pack: PathBuf,
        id: String,
        /// Its new id (its script file follows; scripts naming it do not).
        #[arg(long = "id", value_name = "NEW")]
        new_id: Option<String>,
        /// Live: an object in the match (for now baked as static).
        #[arg(long, conflicts_with = "static_")]
        live: bool,
        /// Static: baked into the level.
        #[arg(long = "static", id = "static_")]
        static_: bool,
        /// Its starting state.
        #[arg(long, conflicts_with = "start_state")]
        state: Option<String>,
        /// The asset's starting state.
        #[arg(long)]
        start_state: bool,
        /// Its value of an exposed property, NAME=VALUE (a whole number,
        /// true/false, #rrggbb, steady or RON); repeatable.
        #[arg(long, value_name = "NAME=VALUE")]
        value: Vec<String>,
        /// Back to an exposed property's default; repeatable.
        #[arg(long, value_name = "NAME")]
        unset: Vec<String>,
        /// Whether the asset's default script runs on it.
        #[arg(long)]
        default_script: Option<OnOff>,
        /// Placed distant at this depth, 1 (near) to 99 (far).
        #[arg(long, conflicts_with = "near")]
        distant: Option<u8>,
        /// Not distant.
        #[arg(long)]
        near: bool,
        /// Locked: `level copy` and the level tab leave it.
        #[arg(long, conflicts_with = "unlocked")]
        locked: bool,
        #[arg(long)]
        unlocked: bool,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Set or clear a copy's own script (`editor/scripts/<id>.rhai`).
    Script {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        script: ScriptArgs,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Raise a copy one place in the stack.
    Raise {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Lower a copy one place in the stack.
    Lower {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Remove a copy, and its asset when no copy uses it any more.
    Remove {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
}

/// Loads the level at `dir`, gives a plain one its ground, applies `op`,
/// bakes and saves, then prints `op`'s lines.
fn edit(
    dir: &Path,
    content: &ContentArg,
    op: impl FnOnce(&mut Pack) -> Result<Vec<String>, placed::PlacedError>,
) -> anyhow::Result<bool> {
    let fail = |e: &dyn std::fmt::Display| anyhow::anyhow!("{}: {e}", dir.display());
    let mut pack = load_level(dir)?;
    if pack.placed.is_none() {
        olr_edit::element::ensure_composed(&mut pack).map_err(|e| fail(&e))?;
    }
    let messages = op(&mut pack).map_err(|e| fail(&e))?;
    bake_pack(dir, &mut pack, &content.content)?;
    save_and_print(dir, &pack, &messages)
}

/// Where asset folder `dir` comes from: a folder of the built-in library
/// (`assets/` beside `content`) is `Builtin`, of the user's `Yours`, any
/// other the level's `Own`.
fn origin(dir: &Path, content: &Path, user: Option<&Path>) -> AssetOrigin {
    let canon = |p: &Path| p.canonicalize().ok();
    let Some(dir) = canon(dir) else { return AssetOrigin::Own };
    let (Some(parent), Some(folder)) = (dir.parent(), dir.file_name().and_then(|n| n.to_str())) else {
        return AssetOrigin::Own;
    };
    if !valid_asset_id(folder) {
        return AssetOrigin::Own;
    }
    let builtin = olr_content::assets_dir(content).and_then(|d| canon(&d));
    let user = user.map(Path::to_path_buf).or_else(|| {
        directories::ProjectDirs::from("", "", "open-liero-reloaded").map(|d| d.data_dir().join("assets"))
    });
    if builtin.as_deref() == Some(parent) {
        AssetOrigin::Builtin(folder.into())
    } else if user.and_then(|u| canon(&u)).as_deref() == Some(parent) {
        AssetOrigin::Yours(folder.into())
    } else {
        AssetOrigin::Own
    }
}

fn origin_label(o: &AssetOrigin) -> String {
    match o {
        AssetOrigin::Own => "own".into(),
        AssetOrigin::Builtin(f) => format!("built-in {f}"),
        AssetOrigin::Yours(f) => format!("yours {f}"),
    }
}

/// `NAME=VALUE` as a copy's value.
fn parse_named_value(s: &str) -> anyhow::Result<(String, ValueSource)> {
    let (name, value) =
        s.split_once('=').ok_or_else(|| anyhow::anyhow!("--value {s:?}: NAME=VALUE (such as glow=40)"))?;
    let v = parse_value(value).map_err(|e| anyhow::anyhow!("--value {s:?}: {e}"))?;
    Ok((name.to_string(), v))
}

pub(crate) fn run(command: &CopyCommand) -> anyhow::Result<bool> {
    match command {
        CopyCommand::Add { pack, asset, at, id, key, size, user, content } => {
            let src = asset::read_dir(asset).map_err(|e| anyhow::anyhow!("{}: {e}", asset.display()))?;
            let origin = origin(asset, &content.content, user.as_deref());
            let name = src.file.name.clone();
            edit(pack, content, |p| {
                let added = placed::add_copy(p, &src, origin, *at, id.as_deref(), key.as_deref())?;
                if let Some(s) = size.size() {
                    placed::resize_copy(p, &added.id, s)?;
                }
                let new = if added.new_asset { ", new" } else { "" };
                let mut out = vec![format!("added copy {} of {name:?} (asset {}{new})", added.id, added.key)];
                out.extend(added.groups_added.iter().map(|g| format!("added glow group {g}")));
                out.extend(added.materials_added.iter().map(|m| format!("added material {m}")));
                if added.added_look {
                    out.push(if added.groups_added.is_empty() {
                        "added a look to hold the asset's materials (the level plays lit)".to_string()
                    } else {
                        asset::ADDED_LIGHTING.to_string()
                    });
                }
                Ok(out)
            })
        }
        CopyCommand::List { pack } => list(pack),
        CopyCommand::Move { pack, id, at, content } => edit(pack, content, |p| {
            placed::move_copy(p, id, *at)?;
            Ok(vec![format!("moved copy {id} to {},{}", at.0, at.1)])
        }),
        CopyCommand::Resize { pack, id, size, content } => edit(pack, content, |p| {
            let s = match (size.width, size.height, size.size) {
                (Some(w), ..) => Size::Width(w),
                (None, Some(h), _) => Size::Height(h),
                (None, None, Some((w, h))) => Size::Free(w, h),
                (None, None, None) => Size::Natural,
            };
            let (w, h) = placed::resize_copy(p, id, s)?;
            Ok(vec![format!("resized copy {id} to {w} × {h}")])
        }),
        CopyCommand::Set {
            pack,
            id,
            new_id,
            live,
            static_,
            state,
            start_state,
            value,
            unset,
            default_script,
            distant,
            near,
            locked,
            unlocked,
            content,
        } => {
            let mut props = Vec::new();
            let mut said = Vec::new();
            if let Some(n) = new_id {
                props.push(CopyProperty::Id(n.clone()));
                said.push(format!("renamed copy {id} to {n}"));
            }
            let id_now = new_id.as_ref().unwrap_or(id).clone();
            let mut say = |what: String| said.push(format!("copy {id_now}: {what}"));
            if *live || *static_ {
                props.push(CopyProperty::Live(*live));
                say(if *live { "live".into() } else { "static".into() });
            }
            if let Some(s) = state {
                props.push(CopyProperty::State(Some(s.clone())));
                say(format!("state {s}"));
            } else if *start_state {
                props.push(CopyProperty::State(None));
                say("the asset's starting state".into());
            }
            for v in value {
                let (name, v) = parse_named_value(v)?;
                say(format!("{name} = {}", value_label(&v)));
                props.push(CopyProperty::Value(name, Some(v)));
            }
            for name in unset {
                props.push(CopyProperty::Value(name.clone(), None));
                say(format!("{name} unset"));
            }
            if let Some(on) = default_script {
                let on = matches!(on, OnOff::On);
                props.push(CopyProperty::DefaultScript(on));
                say(format!("default script {}", if on { "on" } else { "off" }));
            }
            if let Some(d) = distant {
                props.push(CopyProperty::Distant(Some(*d)));
                say(format!("distant {d}"));
            } else if *near {
                props.push(CopyProperty::Distant(None));
                say("near".into());
            }
            if *locked || *unlocked {
                props.push(CopyProperty::Locked(*locked));
                say(if *locked { "locked".into() } else { "unlocked".into() });
            }
            if props.is_empty() {
                anyhow::bail!("{}: copy {id}: nothing to set", pack.display());
            }
            edit(pack, content, |p| {
                placed::set_copies(p, id, &props)?;
                Ok(said)
            })
        }
        CopyCommand::Script { pack, id, script, content } => {
            let script = script.script()?;
            let said = match &script {
                Some(s) => format!("copy {id}: set the script: {}", lines(s)),
                None => format!("copy {id}: cleared the script"),
            };
            edit(pack, content, |p| {
                placed::set_copy_script(p, id, script)?;
                Ok(vec![said])
            })
        }
        CopyCommand::Raise { pack, id, content } => edit(pack, content, |p| {
            placed::raise_copy(p, id)?;
            Ok(vec![format!("raised copy {id}")])
        }),
        CopyCommand::Lower { pack, id, content } => edit(pack, content, |p| {
            placed::lower_copy(p, id)?;
            Ok(vec![format!("lowered copy {id}")])
        }),
        CopyCommand::Remove { pack, id, content } => edit(pack, content, |p| {
            let r = placed::remove_copy(p, id)?;
            let mut out = vec![format!("removed copy {id}")];
            out.extend(r.asset.map(|(a, _)| format!("removed asset {}", a.key)));
            Ok(out)
        }),
    }
}

/// `level script <pack> (--set <file> | --clear)`.
pub(crate) fn level_script(pack: &Path, script: &ScriptArgs, content: &ContentArg) -> anyhow::Result<bool> {
    let script = script.script()?;
    let said = match &script {
        Some(s) => format!("set the level script: {}", lines(s)),
        None => "cleared the level script".to_string(),
    };
    edit(pack, content, |p| {
        placed::set_level_script(p, script)?;
        Ok(vec![said])
    })
}

/// `list`'s lines: each copy, bottom first, then the level script.
fn list(dir: &Path) -> anyhow::Result<bool> {
    let pack = load_level(dir)?;
    let Source::Level(_) = &pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let Some(src) = olr_edit::pack::placement(&pack) else { return Ok(true) };
    for c in &src.file.copies {
        let Some(a) = src.assets.get(&c.asset) else { continue };
        let origin = src.file.assets.iter().find(|x| x.key == c.asset).map(|x| origin_label(&x.origin));
        let (w, h) = placed_size(c, &a.file);
        let mut line = format!(
            "{}: {:?} {} ({}) at {},{} {w} × {h}",
            c.id,
            a.file.name,
            c.asset,
            origin.unwrap_or_default(),
            c.at.0,
            c.at.1
        );
        if c.free_aspect {
            line.push_str(" free");
        }
        if c.live {
            line.push_str(" live");
        }
        if let Some(s) = &c.state {
            line.push_str(&format!(" state {s}"));
        }
        for (name, v) in &c.values {
            line.push_str(&format!(" {name}={}", value_label(v)));
        }
        if c.script {
            line.push_str(" script");
        }
        if !c.default_script {
            line.push_str(" no-default-script");
        }
        if let Some(d) = c.distant {
            line.push_str(&format!(" distant {d}"));
        }
        if c.locked {
            line.push_str(" locked");
        }
        println!("{line}");
    }
    if let Some(s) = &src.level_script {
        println!("level script: {}", lines(s));
    }
    Ok(true)
}
