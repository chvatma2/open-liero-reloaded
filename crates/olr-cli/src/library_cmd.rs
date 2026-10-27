//! `olr-cli level library …`, `level export` and `level import` (issue
//! #76): a level's library links compared and followed, and a level
//! exported and imported with its assets (`.olrlevel`), through
//! `olr_edit::library`'s path wrappers. The libraries are `material`'s:
//! the built-in assets are `assets/` beside `--content`, the user's
//! `--user` (default `assets/` in the game's data directory). Each change
//! loads the level, applies, bakes (the theme's back tile from
//! `--content`) and saves, then prints a line a change; a refusal writes
//! nothing and exits 1.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use olr_data::image::RgbaImage;
use olr_edit::library::{self, LibraryError, Places, Status, origin_label};
use olr_edit::pack::Pack;
use olr_format::levels::AssetOrigin;
use olr_pack::archive::MAX_ARCHIVE;

use crate::asset_cmd::read_bounded;
use crate::element_cmd::{bake_pack, load_level, save_and_print};

/// The game's data directory's `sub` folder.
fn data_dir(sub: &str) -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "open-liero-reloaded").map(|d| d.data_dir().join(sub))
}

/// Where the libraries are.
#[derive(Args)]
pub(crate) struct Libraries {
    /// The content directory: the built-in assets are `assets/` beside it,
    /// the level's theme in it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// The user's asset library (default: `assets/` in the game's data
    /// directory, such as `~/.local/share/open-liero-reloaded/assets`).
    #[arg(long)]
    user: Option<PathBuf>,
}

impl Libraries {
    fn builtin(&self) -> Option<PathBuf> {
        olr_content::assets_dir(&self.content)
    }

    fn user(&self) -> Option<PathBuf> {
        self.user.clone().or_else(|| data_dir("assets"))
    }

    fn user_or_fail(&self) -> anyhow::Result<PathBuf> {
        self.user().ok_or_else(|| anyhow::anyhow!("no data directory for your assets: give --user"))
    }
}

/// A theme's back tile from content directory `content`.
fn back_tile(content: &Path) -> impl Fn(&str) -> Result<RgbaImage, String> + '_ {
    move |theme: &str| olr_edit::compose::theme_back_tile(content, theme).map_err(|e| e.to_string())
}

/// `level library`: with no subcommand, the comparison.
#[derive(Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub(crate) struct LibraryArgs {
    #[command(subcommand)]
    command: Option<LibraryCommand>,
    /// The level pack: print each linked asset, `<key>: same|changed|missing
    /// (<origin>)`, then each copy a change would break, indented.
    #[arg(required = true)]
    pack: Option<PathBuf>,
    #[command(flatten)]
    libraries: Libraries,
}

#[derive(Subcommand)]
enum LibraryCommand {
    /// Take the library's version of assets (`updated <key>`); a version
    /// breaking a copy is refused, naming the break, unless --force, which
    /// resolves each break and prints it.
    Update {
        pack: PathBuf,
        /// The assets' keys.
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        keys: Vec<String>,
        /// Every changed asset.
        #[arg(long)]
        all: bool,
        /// Resolve the breaks: a value dropped, the asset's start, static,
        /// near.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        libraries: Libraries,
    },
    /// Give an asset missing from the library back from the level's copy,
    /// as a new asset of yours the level follows.
    Restore {
        pack: PathBuf,
        key: String,
        #[command(flatten)]
        libraries: Libraries,
    },
    /// Make an asset the level's own: unlinked, it keeps its version.
    Adopt {
        pack: PathBuf,
        key: String,
        #[command(flatten)]
        libraries: Libraries,
    },
    /// Make an asset follow one of yours, taking its files.
    Relink {
        pack: PathBuf,
        key: String,
        /// The folder of your asset library.
        #[arg(long)]
        yours: String,
        /// Resolve the breaks, as `update --force`.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        libraries: Libraries,
    },
}

/// `level export`.
#[derive(Args)]
pub(crate) struct ExportArgs {
    pack: PathBuf,
    /// The archive to write (`.olrlevel`).
    #[arg(long)]
    out: PathBuf,
    /// The content directory holding the level's theme.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
}

/// `level import`.
#[derive(Args)]
pub(crate) struct ImportArgs {
    /// The archive (`.olrlevel`).
    file: PathBuf,
    /// Your packs (default: `packs/` in the game's data directory).
    #[arg(long)]
    packs: Option<PathBuf>,
    #[command(flatten)]
    libraries: Libraries,
}

/// Loads the level at `dir`, applies `op`, bakes and saves, then prints
/// `op`'s lines.
fn edit(
    dir: &Path,
    content: &Path,
    op: impl FnOnce(&mut Pack) -> Result<Vec<String>, LibraryError>,
) -> anyhow::Result<bool> {
    let mut pack = load_level(dir)?;
    let messages = op(&mut pack).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    bake_pack(dir, &mut pack, content)?;
    save_and_print(dir, &pack, &messages)
}

/// The placement of the level at `dir`, refusing a plain level.
fn placed(dir: &Path, pack: &Pack) -> anyhow::Result<olr_format::levels::PlacedSource> {
    olr_edit::pack::placement(pack)
        .ok_or_else(|| anyhow::anyhow!("{}: a plain level: it places no assets", dir.display()))
}

pub(crate) fn library(args: &LibraryArgs) -> anyhow::Result<bool> {
    let Some(command) = &args.command else {
        let dir = args.pack.as_deref().expect("clap requires it");
        return compare(dir, &args.libraries);
    };
    match command {
        LibraryCommand::Update { pack, keys, all, force, libraries } => {
            let (builtin, user) = (libraries.builtin(), libraries.user());
            let keys = if *all {
                let p = load_level(pack)?;
                let src = placed(pack, &p)?;
                library::compare_from(&src, builtin.as_deref(), user.as_deref())
                    .into_iter()
                    .filter(|l| matches!(l.status, Status::Changed { .. }))
                    .map(|l| l.key)
                    .collect()
            } else {
                keys.clone()
            };
            edit(pack, &libraries.content, |p| {
                let u = library::update_from(p, &keys, *force, builtin.as_deref(), user.as_deref())?;
                let mut out: Vec<String> = u.keys.iter().map(|k| format!("updated {k}")).collect();
                out.extend(u.same.iter().map(|k| format!("{k}: the library's already")));
                out.extend(u.resolutions.iter().map(|r| r.to_string()));
                if out.is_empty() {
                    out.push("nothing changed in the library".into());
                }
                Ok(out)
            })
        }
        LibraryCommand::Restore { pack, key, libraries } => {
            let user = libraries.user_or_fail()?;
            edit(pack, &libraries.content, |p| {
                let folder = library::restore(p, key, &user)?;
                Ok(vec![format!("restored {key} as yours {folder}")])
            })
        }
        LibraryCommand::Adopt { pack, key, libraries } => edit(pack, &libraries.content, |p| {
            library::adopt(p, key)?;
            Ok(vec![format!("{key} is the level's own now")])
        }),
        LibraryCommand::Relink { pack, key, yours, force, libraries } => {
            let user = libraries.user_or_fail()?;
            let origin = AssetOrigin::Yours(yours.clone());
            edit(pack, &libraries.content, |p| {
                let u = library::relink_from(p, key, origin, None, Some(&user), *force)?;
                let mut out = vec![format!("{key} now follows yours {yours}")];
                out.extend(u.resolutions.iter().map(|r| r.to_string()));
                Ok(out)
            })
        }
    }
}

/// `level library <pack>`'s lines; exits 0 whatever changed.
fn compare(dir: &Path, libraries: &Libraries) -> anyhow::Result<bool> {
    let pack = load_level(dir)?;
    let src = placed(dir, &pack)?;
    let (builtin, user) = (libraries.builtin(), libraries.user());
    for link in library::compare_from(&src, builtin.as_deref(), user.as_deref()) {
        let status = match &link.status {
            Status::Same => "same",
            Status::Changed { .. } => "changed",
            Status::Missing => "missing",
        };
        println!("{}: {status} ({})", link.key, origin_label(&link.origin));
        if let Status::Changed { breaks } = &link.status {
            for b in breaks {
                println!("  {b}");
            }
        }
    }
    Ok(true)
}

/// `level export <pack> --out <file>`: prints `exported <pack> to <file>:
/// <bytes> bytes, <n> assets`.
pub(crate) fn export(args: &ExportArgs) -> anyhow::Result<bool> {
    let (dir, out) = (&args.pack, &args.out);
    let fail = |e: &dyn std::fmt::Display| anyhow::anyhow!("{}: {e}", dir.display());
    if let (Ok(d), Some(parent)) = (dir.canonicalize(), out.parent())
        && let Ok(p) = (if parent.as_os_str().is_empty() { Path::new(".") } else { parent }).canonicalize()
        && p.starts_with(&d)
    {
        anyhow::bail!("{}: the archive would be inside the pack", out.display());
    }
    let bytes = library::export(dir, &back_tile(&args.content)).map_err(|e| fail(&e))?;
    let pack = olr_edit::pack::load(dir).map_err(|e| fail(&e))?;
    let assets = olr_edit::pack::placement(&pack).map_or(0, |p| p.file.assets.len());
    std::fs::write(out, &bytes).map_err(|e| anyhow::anyhow!("{}: {e}", out.display()))?;
    println!("exported {} to {}: {} bytes, {assets} assets", dir.display(), out.display(), bytes.len());
    Ok(true)
}

/// The ids of the bundled packs of content directory `content`: each
/// pack folder's name and its manifest's id.
fn bundled_ids(content: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for root in olr_content::bundled_roots(content) {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.filter_map(Result::ok) {
            if let Some(n) = e.file_name().to_str() {
                ids.insert(n.to_string());
            }
            if let Ok(m) = olr_pack::read_manifest(&olr_vfs::DirFs::new(e.path())) {
                ids.insert(m.id);
            }
        }
    }
    ids
}

/// `level import <file>`: prints `imported <id> ("<name>") into <dir>`,
/// then a line an asset.
pub(crate) fn import(args: &ImportArgs) -> anyhow::Result<bool> {
    let bytes = read_bounded(&args.file, MAX_ARCHIVE).map_err(anyhow::Error::msg)?;
    let packs = args
        .packs
        .clone()
        .or_else(|| data_dir("packs"))
        .ok_or_else(|| anyhow::anyhow!("no data directory for your packs: give --packs"))?;
    let user = args.libraries.user_or_fail()?;
    let builtin = args.libraries.builtin();
    let bundled = bundled_ids(&args.libraries.content);
    let places = Places { packs: &packs, user: &user, builtin: builtin.as_deref(), bundled: &bundled };
    let got = library::import(&bytes, &places, &back_tile(&args.libraries.content))
        .map_err(|e| anyhow::anyhow!("{}: {e}", args.file.display()))?;
    println!("imported {} ({:?}) into {}", got.id, got.name, got.dir.display());
    for a in &got.assets {
        println!("{a}");
    }
    Ok(true)
}
