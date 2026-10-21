//! `olr-cli mod …` (issue #16): the mod editor's operations for agents and
//! tests, `olr_edit::mods`'s. Values are read and written by field path
//! (`mod.worm.gravity`, `weapons/bazooka.fire.shot.speed`); every edit
//! loads the pack, is validated (refused with the errors it would add,
//! each at its path), and saves through `olr_edit::pack::save`. New mods
//! are `olr-cli pack new mod <id> [--from <pack>]`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::Subcommand;
use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_edit::mods::{self, ModError, WeaponFrom};
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
