//! `olr-cli mod …` (issue #16): the mod editor's operations for agents and
//! tests, `olr_edit::mods`'s; issue #19's `mod gfx` and `mod sfx`, its
//! images, glow masks and sounds (`olr_edit::mods::art`). Values are read and written by field path
//! (`mod.worm.gravity`, `weapons/bazooka.fire.shot.speed`); every edit
//! loads the pack, is validated (refused with the errors it would add,
//! each at its path), and saves through `olr_edit::pack::save`. New mods
//! are `olr-cli pack new mod <id> [--from <pack>]`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Subcommand;
use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_edit::mods::{self, ModError, WeaponFrom, art};
use olr_edit::pack::{self, Pack};

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: ModCommand,
}

#[derive(Subcommand)]
enum ModCommand {
    /// Print a value as RON by path: `mod.worm.gravity`,
    /// `weapons/bazooka.fire.shot.speed`, `restrictions["Standard 100lt"]`,
    /// a whole file (`weapons/bazooka`). A spawn's `projectile` leads into
    /// the shared projectile it names.
    Get {
        /// A mod pack's directory.
        pack: PathBuf,
        path: String,
    },
    /// Set a value by path and save: RON read as the field's type, a bare
    /// string, or an enum's variant by name (made with its defaults). A
    /// weapon's `name` and `mod.name` rename (presets follow).
    Set {
        /// A mod pack's directory.
        pack: PathBuf,
        path: String,
        value: String,
    },
    /// List the fields under a path (the files without one): each leaf with
    /// its value, an enum with its variant and choices.
    Fields {
        /// A mod pack's directory.
        pack: PathBuf,
        path: Option<String>,
    },
    /// Add, remove, move, copy, rename and list weapons.
    Weapon {
        #[command(subcommand)]
        command: WeaponCommand,
    },
    /// Shared projectiles (`projectiles/<id>.ron`): who uses each, and a
    /// spawn's projectile made inline or shared.
    Projectile {
        #[command(subcommand)]
        command: ProjectileCommand,
    },
    /// The mod's images (`gfx/`) and their glow masks (issue #19).
    Gfx {
        #[command(subcommand)]
        command: GfxCommand,
    },
    /// The mod's sounds (`sfx/`, issue #19).
    Sfx {
        #[command(subcommand)]
        command: SfxCommand,
    },
    /// Every error and warning of mod packs, each with its field path;
    /// exits with status 1 if any has errors.
    Check {
        /// Mod pack directories.
        #[arg(required = true)]
        packs: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum WeaponCommand {
    /// Print `<index>: <id> "<name>"` per weapon; the index is its id in the game.
    List { pack: PathBuf },
    /// Add a weapon: a blank one, or a copy of another mod's (its shared
    /// projectiles and files come along).
    Add {
        pack: PathBuf,
        /// The new weapon's id (its file name).
        id: String,
        /// The name menus and presets show. Default: the copied weapon's,
        /// or "New weapon", numbered when taken.
        #[arg(long)]
        name: Option<String>,
        /// Copy this weapon: `<mod>:<weapon id>`, the mod an installed mod's
        /// pack id or a mod pack's directory.
        #[arg(long)]
        from: Option<String>,
        /// Its index. Default: last.
        #[arg(long)]
        at: Option<usize>,
        /// Imported content (packs written by `olr-cli import`), OLR's own
        /// packs over it: where `--from` finds installed mods.
        #[arg(long, default_value = "content/packs")]
        content: PathBuf,
    },
    /// Remove a weapon (and its presets' entries).
    Remove { pack: PathBuf, id: String },
    /// Move a weapon to an index.
    Move { pack: PathBuf, id: String, index: usize },
    /// Copy a weapon, right after it.
    Copy {
        pack: PathBuf,
        id: String,
        new_id: String,
        /// Default: its name, numbered.
        #[arg(long)]
        name: Option<String>,
    },
    /// Rename a weapon: its name (presets and shipped loadouts follow) and/or its id.
    Rename {
        pack: PathBuf,
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        new_id: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProjectileCommand {
    /// Print each shared projectile with the spawns naming it.
    UsedBy {
        pack: PathBuf,
        /// One shared projectile's id. Default: all.
        id: Option<String>,
    },
    /// Make a spawn's shared projectile its own inline copy (the shared one
    /// goes when nothing else names it).
    Inline {
        pack: PathBuf,
        /// The spawn's path, such as `weapons/bazooka.fire.shot.projectile.spawns.spawn`.
        spawn: String,
    },
    /// Make a spawn's inline projectile a shared one.
    Share {
        pack: PathBuf,
        /// The spawn's path, such as `weapons/bazooka.fire.shot`.
        spawn: String,
        /// Its id. Default: its key.
        #[arg(long)]
        id: Option<String>,
    },
}

#[derive(Subcommand)]
enum GfxCommand {
    /// Print each image: its size, frames, glow and the fields using it.
    List { pack: PathBuf },
    /// Add a blank image of `--frames` square frames `--side` pixels a
    /// side, or a copy of another of the mod's images (`--copy`).
    New {
        pack: PathBuf,
        /// `gfx/<name>.png`.
        path: String,
        #[arg(long, default_value_t = 8)]
        side: u32,
        #[arg(long, default_value_t = 1)]
        frames: u32,
        /// Copy this image (and its glow mask) instead.
        #[arg(long)]
        copy: Option<String>,
    },
    /// Add a PNG file as an image (`--as`, default `gfx/<file name>`), or
    /// with `--replace` put it in place of an existing one (a change of its
    /// frame count changes play, and says so).
    Import {
        pack: PathBuf,
        file: PathBuf,
        #[arg(long = "as")]
        to: Option<String>,
        #[arg(long)]
        replace: bool,
    },
    /// Paint an image's glow mask (`gfx/<name>.glow.png`): from a greyscale
    /// PNG of its size, pixels of a colour, or rectangles; `--clear` first
    /// removes it; `--export` writes it as a PNG. Painting adds to the mask.
    Glow {
        pack: PathBuf,
        /// The image, `gfx/<name>.png`.
        image: String,
        /// A greyscale PNG of the image's size: the whole mask.
        #[arg(long)]
        from: Option<PathBuf>,
        /// Glow the pixels of this colour (`#rrggbb`, repeatable).
        #[arg(long = "colour")]
        colours: Vec<String>,
        /// How far a colour may be from `--colour`, per channel.
        #[arg(long, default_value_t = 0)]
        tolerance: u8,
        /// Glow a rectangle `x,y,w,h` of image pixels (repeatable).
        #[arg(long = "rect")]
        rects: Vec<String>,
        /// The rectangles are within a frame, painted in every frame.
        #[arg(long)]
        every_frame: bool,
        /// The glow painted, 1 to 255 (drawn in quarters).
        #[arg(long, default_value_t = 255)]
        strength: u8,
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        export: Option<PathBuf>,
    },
    /// Rename an image (its glow mask and every field naming it follow).
    Rename { pack: PathBuf, from: String, to: String },
    /// Remove an unused image and its glow mask (refused while used).
    Remove { pack: PathBuf, path: String },
}

#[derive(Subcommand)]
enum SfxCommand {
    /// Print each sound with its size and the fields using it.
    List { pack: PathBuf },
    /// Add a WAV or Ogg file as a sound (`--as`, default `sfx/<file name>`).
    Import {
        pack: PathBuf,
        file: PathBuf,
        #[arg(long = "as")]
        to: Option<String>,
    },
    /// Rename a sound (every field naming it follows).
    Rename { pack: PathBuf, from: String, to: String },
    /// Remove an unused sound (refused while used).
    Remove { pack: PathBuf, path: String },
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        ModCommand::Get { pack, path } => {
            let p = load(pack)?;
            println!("{}", mods::get(source(&p)?, path).map_err(refused)?);
        }
        ModCommand::Set { pack, path, value } => {
            let shown = edit(pack, |p| {
                mods::set(p, path, value)?;
                mods::get(mods::source(p)?, path)
            })?;
            println!("set {path} = {shown}");
        }
        ModCommand::Fields { pack, path } => fields(&load(pack)?, path.as_deref())?,
        ModCommand::Weapon { command } => weapon(command)?,
        ModCommand::Projectile { command } => projectile(command)?,
        ModCommand::Gfx { command } => gfx(command)?,
        ModCommand::Sfx { command } => sfx(command)?,
        ModCommand::Check { packs } => return Ok(check(packs)),
    }
    Ok(true)
}

fn weapon(command: &WeaponCommand) -> anyhow::Result<()> {
    match command {
        WeaponCommand::List { pack } => {
            for (i, (id, w)) in source(&load(pack)?)?.weapons.iter().enumerate() {
                println!("{i}: {id} {:?}", w.name);
            }
        }
        WeaponCommand::Add { pack, id, name, from, at, content } => {
            let from = match from {
                Some(f) => {
                    let (m, w) = f.rsplit_once(':').context("--from is <mod>:<weapon id>")?;
                    Some((load(&mod_dir(m, content)?)?, w.to_string()))
                }
                None => None,
            };
            let what = match &from {
                Some((p, w)) => WeaponFrom::Copy { pack: p, id: w },
                None => WeaponFrom::Blank,
            };
            let (i, name) = edit(pack, |p| {
                mods::add_weapon(p, id, name.as_deref(), what, *at)?;
                let m = mods::source(p)?;
                let i = m.weapons.iter().position(|(w, _)| w == id).unwrap_or_default();
                Ok((i, m.weapons[i].1.name.clone()))
            })?;
            println!("added weapon {id} ({name:?}) at {i}");
        }
        WeaponCommand::Remove { pack, id } => {
            edit(pack, |p| mods::remove_weapon(p, id))?;
            println!("removed weapon {id}");
        }
        WeaponCommand::Move { pack, id, index } => {
            edit(pack, |p| mods::move_weapon(p, id, *index))?;
            println!("moved weapon {id} to {index}");
        }
        WeaponCommand::Copy { pack, id, new_id, name } => {
            let name = edit(pack, |p| {
                mods::copy_weapon(p, id, new_id, name.as_deref())?;
                mods::get(mods::source(p)?, &format!("weapons/{new_id}.name"))
            })?;
            println!("copied weapon {id} to {new_id} ({name})");
        }
        WeaponCommand::Rename { pack, id, name, new_id } => {
            if name.is_none() && new_id.is_none() {
                bail!("give --name, --new-id or both");
            }
            let now = new_id.as_deref().unwrap_or(id);
            let shown = edit(pack, |p| {
                mods::rename_weapon(p, id, name.as_deref(), new_id.as_deref())?;
                mods::get(mods::source(p)?, &format!("weapons/{now}.name"))
            })?;
            println!("renamed weapon {id} to {now} ({shown})");
        }
    }
    Ok(())
}

fn projectile(command: &ProjectileCommand) -> anyhow::Result<()> {
    match command {
        ProjectileCommand::UsedBy { pack, id } => {
            let p = load(pack)?;
            let used = mods::used_by(source(&p)?);
            if let Some(id) = id
                && !used.contains_key(id)
            {
                bail!("no shared projectile {id:?}");
            }
            for (shared, users) in used.iter().filter(|(s, _)| id.as_ref().is_none_or(|id| id == *s)) {
                match users.as_slice() {
                    [] => println!("{shared}: unused"),
                    u => println!("{shared}: {}", u.join(", ")),
                }
            }
        }
        ProjectileCommand::Inline { pack, spawn } => {
            edit(pack, |p| mods::inline(p, spawn))?;
            println!("made {spawn}'s projectile inline");
        }
        ProjectileCommand::Share { pack, spawn, id } => {
            let id = edit(pack, |p| mods::share(p, spawn, id.as_deref()))?;
            println!("shared {spawn}'s projectile as projectiles/{id}");
        }
    }
    Ok(())
}

/// The users of `asset`, for a list: `Bazooka's shot, …` or `unused`.
fn users_line(p: &Pack, asset: &str) -> anyhow::Result<String> {
    let u = art::users(source(p)?, asset);
    Ok(if u.is_empty() {
        "unused".into()
    } else {
        u.iter().map(|u| u.label.as_str()).collect::<Vec<_>>().join(", ")
    })
}

/// A file read whole, at most `MAX_ASSET_BYTES`.
fn read_file(file: &Path) -> anyhow::Result<Vec<u8>> {
    let len = std::fs::metadata(file).with_context(|| format!("{}", file.display()))?.len();
    if len > art::MAX_ASSET_BYTES as u64 {
        bail!("{}: {len} bytes, more than {}", file.display(), art::MAX_ASSET_BYTES);
    }
    std::fs::read(file).with_context(|| format!("{}", file.display()))
}

/// `dir` and `file`'s name: an import's default path.
fn named(dir: &str, file: &Path, to: &Option<String>) -> anyhow::Result<String> {
    match to {
        Some(t) => Ok(t.clone()),
        None => {
            let name = file.file_name().and_then(|n| n.to_str()).context("the file has no name")?;
            Ok(format!("{dir}{name}"))
        }
    }
}

fn gfx(command: &GfxCommand) -> anyhow::Result<()> {
    match command {
        GfxCommand::List { pack } => {
            let p = load(pack)?;
            for path in art::images(&p) {
                let bytes = &p.other[&olr_vfs::VPath::new(&path)?];
                let (w, h) = olr_format::image_io::png_size(bytes).unwrap_or((0, 0));
                let frames = art::image_frames(bytes)
                    .map_or_else(|e| format!("unreadable: {e}"), |n| format!("{n} frames"));
                let glow = match art::glow(&p, &path) {
                    Some(v) if v.iter().any(|&g| g != 0) => {
                        format!(
                            ", glow on {} pixels",
                            v.iter().filter(|&&g| olr_data::skin::glow_level(g) > 0).count()
                        )
                    }
                    Some(_) => String::new(),
                    None => ", a glow mask that does not fit".into(),
                };
                println!("{path}: {w} × {h}, {frames}{glow}; {}", users_line(&p, &path)?);
            }
        }
        GfxCommand::New { pack, path, side, frames, copy } => {
            edit(pack, |p| {
                let (bytes, glow) = match copy {
                    Some(from) => {
                        let b = p
                            .other
                            .get(&olr_vfs::VPath::new(from).map_err(|e| ModError::Refused(e.to_string()))?)
                            .cloned()
                            .ok_or_else(|| ModError::Refused(format!("no image {from}")))?;
                        (b, art::glow(p, from))
                    }
                    None => (art::blank_image(*side, *frames)?, None),
                };
                art::import_image(p, path, &bytes)?;
                art::set_glow(p, path, glow.as_deref())
            })?;
            println!("added {path}");
        }
        GfxCommand::Import { pack, file, to, replace } => {
            let bytes = read_file(file)?;
            let path = named("gfx/", file, to)?;
            if *replace {
                let change = edit(pack, |p| art::set_image(p, &path, &bytes))?;
                println!("replaced {path}");
                if change.changes_play() {
                    println!(
                        "note: {path} had {} frames and has {}: projectiles using it play differently (try it in Play)",
                        change.frames_before, change.frames_after
                    );
                }
            } else {
                edit(pack, |p| art::import_image(p, &path, &bytes))?;
                println!("added {path} ({} frames)", art::image_frames(&bytes).unwrap_or(0));
            }
        }
        GfxCommand::Glow {
            pack,
            image,
            from,
            colours,
            tolerance,
            rects,
            every_frame,
            strength,
            clear,
            export,
        } => {
            let paint = from.is_some() || !colours.is_empty() || !rects.is_empty() || *clear;
            if paint {
                let mask = from.as_deref().map(read_file).transpose()?;
                let colours = colours
                    .iter()
                    .map(|c| {
                        olr_format::Colour::parse(c).ok()
                            .map(|c| c.0)
                            .with_context(|| format!("{c:?} is no #rrggbb"))
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                let rects = rects.iter().map(|r| parse_rect(r)).collect::<anyhow::Result<Vec<_>>>()?;
                let lit = edit(pack, |p| {
                    if let Some(m) = &mask {
                        art::set_glow_png(p, image, m)?;
                    }
                    let img = p
                        .other
                        .get(&olr_vfs::VPath::new(image).map_err(|e| ModError::Refused(e.to_string()))?)
                        .and_then(|b| olr_format::image_io::decode_rgba(b).ok())
                        .ok_or_else(|| ModError::Refused(format!("no image {image}")))?;
                    let mut v = if *clear {
                        vec![0; img.as_raw().len() / 4]
                    } else {
                        art::glow(p, image).unwrap_or_default()
                    };
                    let (w, h) = (img.width(), img.height());
                    v.resize((w * h) as usize, 0);
                    let near = |a: u8, b: u8| a.abs_diff(b) <= *tolerance;
                    for y in 0..h {
                        for x in 0..w {
                            let c = img.get(x, y);
                            let by_colour =
                                colours.iter().any(|k| near(c.r, k.r) && near(c.g, k.g) && near(c.b, k.b));
                            let fx = if *every_frame && h > 0 { x % h } else { x };
                            let in_rect = rects.iter().any(|&(rx, ry, rw, rh)| {
                                fx >= rx && fx < rx.saturating_add(rw) && y >= ry && y < ry.saturating_add(rh)
                            });
                            if c.a != 0 && (by_colour || in_rect) {
                                let i = (y * w + x) as usize;
                                v[i] = v[i].max(*strength);
                            }
                        }
                    }
                    art::set_glow(p, image, Some(&v))?;
                    Ok(v.iter().filter(|&&g| olr_data::skin::glow_level(g) > 0).count())
                })?;
                println!("{image}: glow on {lit} pixels");
            }
            if let Some(out) = export {
                let p = load(pack)?;
                let img =
                    p.other.get(&olr_vfs::VPath::new(image)?).with_context(|| format!("no image {image}"))?;
                let size = olr_format::image_io::png_size(img).context("not a PNG")?;
                let v = art::glow(&p, image).context("its glow mask does not fit")?;
                std::fs::write(out, olr_format::mods::encode_glow_mask(size, &v))
                    .with_context(|| format!("{}", out.display()))?;
                println!("wrote {}", out.display());
            }
            if !paint && export.is_none() {
                bail!("give --from, --colour, --rect, --clear or --export");
            }
        }
        GfxCommand::Rename { pack, from, to } => {
            edit(pack, |p| art::rename(p, from, to))?;
            println!("renamed {from} to {to}");
        }
        GfxCommand::Remove { pack, path } => {
            if !olr_format::mods::is_image(path) {
                bail!("{path:?} is no image (gfx/<name>.png)");
            }
            edit(pack, |p| art::remove(p, path))?;
            println!("removed {path}");
        }
    }
    Ok(())
}

/// `x,y,w,h`.
fn parse_rect(r: &str) -> anyhow::Result<(u32, u32, u32, u32)> {
    let v: Vec<u32> = r
        .split(',')
        .map(|n| n.trim().parse())
        .collect::<Result<_, _>>()
        .with_context(|| format!("{r:?} is no x,y,w,h"))?;
    match v[..] {
        [x, y, w, h] => Ok((x, y, w, h)),
        _ => bail!("{r:?} is no x,y,w,h"),
    }
}

fn sfx(command: &SfxCommand) -> anyhow::Result<()> {
    match command {
        SfxCommand::List { pack } => {
            let p = load(pack)?;
            for path in art::sounds(&p) {
                let n = p.other[&olr_vfs::VPath::new(&path)?].len();
                println!("{path}: {n} bytes; {}", users_line(&p, &path)?);
            }
        }
        SfxCommand::Import { pack, file, to } => {
            let bytes = read_file(file)?;
            let path = named("sfx/", file, to)?;
            edit(pack, |p| art::import_sound(p, &path, &bytes))?;
            println!("added {path}");
        }
        SfxCommand::Rename { pack, from, to } => {
            edit(pack, |p| art::rename(p, from, to))?;
            println!("renamed {from} to {to}");
        }
        SfxCommand::Remove { pack, path } => {
            if !path.starts_with("sfx/") {
                bail!("{path:?} is no sound (sfx/<name>)");
            }
            edit(pack, |p| art::remove(p, path))?;
            println!("removed {path}");
        }
    }
    Ok(())
}

/// Prints each child of `path` (the files at the top): a leaf with its
/// value, an enum with its variant and choices.
fn fields(p: &Pack, path: Option<&str>) -> anyhow::Result<()> {
    let m = source(p)?;
    let children = match path {
        Some(path) => mods::children(m, path).map_err(refused)?,
        None => {
            let mut files = vec!["mod".to_string(), "restrictions".to_string()];
            files.extend(m.weapons.iter().map(|(id, _)| format!("weapons/{id}")));
            files.extend(m.shared.keys().map(|id| format!("projectiles/{id}")));
            files
        }
    };
    for c in children {
        let choices = mods::choices(m, &c).map_err(refused)?;
        if !choices.is_empty() {
            let v = mods::variant(m, &c).map_err(refused)?.unwrap_or_default();
            println!("{c}: {v} (variants: {})", choices.join(", "));
        } else if mods::children(m, &c).map_err(refused)?.is_empty() {
            println!("{c} = {}", mods::get(m, &c).map_err(refused)?);
        } else {
            println!("{c}");
        }
    }
    Ok(())
}

fn check(packs: &[PathBuf]) -> bool {
    let (mut errors, mut warnings) = (0, 0);
    for dir in packs {
        let issues = mods::check_dir(dir);
        for i in &issues {
            println!("{}: {i}", dir.display());
        }
        let e = issues.iter().filter(|i| i.is_error()).count();
        (errors, warnings) = (errors + e, warnings + issues.len() - e);
    }
    println!("checked {} mods: {errors} errors, {warnings} warnings", packs.len());
    errors == 0
}

/// A mod pack's directory: `given` itself (holding a `pack.ron`), else an
/// installed mod's by pack id.
fn mod_dir(given: &str, content: &Path) -> anyhow::Result<PathBuf> {
    let path = Path::new(given);
    if path.join(olr_pack::MANIFEST).is_file() {
        return Ok(path.to_path_buf());
    }
    let c = Content::open_bundled(content).context("opening the content")?;
    match (c.pack_kind(given), c.pack_dir(given)) {
        (Some(PackKind::Mod), Some(dir)) => Ok(dir.to_path_buf()),
        _ => bail!("{given}: neither a pack directory nor an installed mod in {}", content.display()),
    }
}

fn load(dir: &Path) -> anyhow::Result<Pack> {
    let p = pack::load(dir).with_context(|| format!("{}", dir.display()))?;
    if p.manifest.kind != PackKind::Mod {
        bail!("{}: a {:?} pack, not a mod", dir.display(), p.manifest.kind);
    }
    Ok(p)
}

fn source(p: &Pack) -> anyhow::Result<&olr_format::mods::ModSource> {
    mods::source(p).map_err(refused)
}

/// An operation's refusal, each error on its own line.
fn refused(e: ModError) -> anyhow::Error {
    match e {
        ModError::Invalid(issues) => anyhow::anyhow!(
            "refused, it would add:\n{}",
            issues.iter().map(|i| format!("  {i}")).collect::<Vec<_>>().join("\n")
        ),
        e => anyhow::anyhow!("{e}"),
    }
}

/// Loads the mod at `dir`, applies `op` and saves; the pack stays as it was
/// when `op` or the save refuses.
fn edit<T>(dir: &Path, op: impl FnOnce(&mut Pack) -> Result<T, ModError>) -> anyhow::Result<T> {
    let mut p = load(dir)?;
    let out = op(&mut p).map_err(refused)?;
    let warnings = pack::save(dir, &p).with_context(|| format!("saving {}", dir.display()))?;
    crate::pack_cmd::warn(&warnings);
    Ok(out)
}
