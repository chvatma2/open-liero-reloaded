//! `olr-cli pack …`: checking, formatting and making packs (plan 13d).

use std::path::{Path, PathBuf};

use clap::{Subcommand, ValueEnum};
use olr_data::manifest::PackKind;
use olr_edit::fmt::Formatted;
use olr_edit::new::{Template, new_pack};
use olr_pack::Severity;
use olr_vfs::DirFs;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: PackCommand,
}

#[derive(Subcommand)]
enum PackCommand {
    /// Parse, compile and validate packs; exits with status 1 if any pack has errors.
    Check {
        /// Pack directories, or directories containing pack directories.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// The content directory with the theme packs a composed level's
        /// bake needs (its stale-bake check).
        #[arg(long, default_value = "content/packs")]
        content: PathBuf,
    },
    /// Rewrite packs' files in canonical form; foreign files are kept as they are.
    Fmt {
        /// Only list the files that are not canonical; exits with status 1 if any are.
        #[arg(long)]
        check: bool,
        /// Pack directories, or directories containing pack directories.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Make a new pack: a mod or a level from a template, or any kind as a copy of a pack.
    New {
        kind: Kind,
        /// The new pack's id: a lowercase slug (a-z, 0-9 and -).
        id: String,
        /// The directory to write; it must not exist or be empty. Default: ./<id>.
        #[arg(long)]
        out: Option<PathBuf>,
        /// The name menus show. Default: the id.
        #[arg(long)]
        name: Option<String>,
        /// A level's size in world pixels, WxH. Default: 640x480.
        #[arg(long, value_parser = parse_size, conflicts_with = "from")]
        size: Option<(u32, u32)>,
        /// A level's theme pack id. Default: olx-theme-dirt.
        #[arg(long, conflicts_with = "from")]
        theme: Option<String>,
        /// Copy this pack (of the same kind) under the new id and name.
        #[arg(long)]
        from: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    Mod,
    Level,
    Skin,
    Theme,
    Core,
}

impl Kind {
    fn pack_kind(self) -> PackKind {
        match self {
            Kind::Mod => PackKind::Mod,
            Kind::Level => PackKind::Level,
            Kind::Skin => PackKind::Skin,
            Kind::Theme => PackKind::Theme,
            Kind::Core => PackKind::Core,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Mod => "mod",
            Kind::Level => "level",
            Kind::Skin => "skin",
            Kind::Theme => "theme",
            Kind::Core => "core",
        }
    }
}

/// `WxH`, two whole numbers.
pub(crate) fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or_else(|| format!("{s:?} is not WxH"))?;
    let num = |v: &str| v.parse::<u32>().map_err(|_| format!("{s:?} is not WxH (whole numbers)"));
    Ok((num(w)?, num(h)?))
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        PackCommand::Check { paths, content } => check(paths, content),
        PackCommand::Fmt { check, paths } => fmt(paths, *check),
        PackCommand::New { kind, id, out, name, size, theme, from } => {
            let out = out.clone().unwrap_or_else(|| PathBuf::from(id));
            new(*kind, id, &out, name.as_deref().unwrap_or(id), *size, theme.as_deref(), from.as_deref())
        }
    }
}

/// Pack directories under `path`: `path` itself if it has `pack.ron`, else its
/// immediate subdirectories that do, but hidden ones (an interrupted save's
/// or import's `.import-<id>` and `.replaced-<id>`).
pub(crate) fn packs(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.join(olr_pack::MANIFEST).is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    let read = |e| anyhow::anyhow!("{}: {e}", path.display());
    for entry in std::fs::read_dir(path).map_err(read)? {
        let entry = entry.map_err(read)?;
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        let p = entry.path();
        if !hidden && p.join(olr_pack::MANIFEST).is_file() {
            out.push(p);
        }
    }
    out.sort();
    if out.is_empty() {
        anyhow::bail!("{}: no pack.ron here or in any subdirectory", path.display());
    }
    Ok(out)
}

fn check(paths: &[PathBuf], content: &Path) -> anyhow::Result<bool> {
    let (mut checked, mut errors, mut warnings) = (0, 0, 0);
    for path in paths {
        for pack in packs(path)? {
            checked += 1;
            let diagnostics = olr_pack::validate(&DirFs::new(&pack));
            let valid = diagnostics.iter().all(|d| d.severity != Severity::Error);
            let mut lines: Vec<(Severity, String)> =
                diagnostics.iter().map(|d| (d.severity, d.to_string())).collect();
            if valid {
                lines.extend(stale_bake(&pack, content));
            }
            for (severity, line) in lines {
                match severity {
                    Severity::Error => errors += 1,
                    Severity::Warning => warnings += 1,
                }
                println!("{}: {line}", pack.display());
            }
        }
    }
    println!("checked {checked} packs: {errors} errors, {warnings} warnings");
    Ok(errors == 0)
}

/// A valid composed level's baked files that differ from its
/// composition's bake (plan 14a: the composition is the source of truth),
/// as errors; a theme not found in `content` is a warning.
fn stale_bake(dir: &Path, content: &Path) -> Vec<(Severity, String)> {
    if !dir.join(olr_format::levels::COMPOSITION_FILE).is_file() {
        return Vec::new();
    }
    let pack = match olr_edit::pack::load(dir) {
        Ok(p) => p,
        Err(e) => return vec![(Severity::Error, format!("error: {e}"))],
    };
    let olr_edit::pack::Source::Level(level) = &pack.source else { return Vec::new() };
    let tile = match olr_edit::compose::theme_back_tile(content, &level.theme) {
        Ok(t) => t,
        Err(e) => return vec![(Severity::Warning, format!("warning: cannot check the bake: {e}"))],
    };
    match olr_edit::compose::stale(&pack, &tile) {
        Ok(files) => files
            .iter()
            .map(|f| {
                (
                    Severity::Error,
                    format!("error: {f}: differs from the composition's bake (run olr-cli level bake)"),
                )
            })
            .collect(),
        Err(e) => vec![(Severity::Error, format!("error: {e}"))],
    }
}

/// `pack fmt`: a pack that does not load, or would not check, is an error
/// (on stderr) and stays untouched.
fn fmt(paths: &[PathBuf], check_only: bool) -> anyhow::Result<bool> {
    let mut ok = true;
    for path in paths {
        for pack in packs(path)? {
            let result = if check_only {
                olr_edit::fmt::unformatted(&pack).map(|files| Formatted { files, warnings: Vec::new() })
            } else {
                olr_edit::fmt::format(&pack)
            };
            match result {
                Ok(Formatted { files, .. }) if check_only => {
                    for file in &files {
                        println!("{}/{file}: not canonical", pack.display());
                    }
                    ok &= files.is_empty();
                }
                Ok(Formatted { files, warnings }) => {
                    if !files.is_empty() {
                        println!("formatted {}: {} files", pack.display(), files.len());
                    }
                    warn(&warnings);
                }
                Err(e) => {
                    eprintln!("{}: error: {e:#}", pack.display());
                    ok = false;
                }
            }
        }
    }
    Ok(ok)
}

/// A save's warnings, on stderr.
pub(crate) fn warn(warnings: &[String]) {
    for w in warnings {
        eprintln!("warning: {w}");
    }
}

fn new(
    kind: Kind,
    id: &str,
    out: &Path,
    name: &str,
    size: Option<(u32, u32)>,
    theme: Option<&str>,
    from: Option<&Path>,
) -> anyhow::Result<bool> {
    if !matches!(kind, Kind::Level) && (size.is_some() || theme.is_some()) {
        anyhow::bail!("--size and --theme apply to level packs only");
    }
    let template = match (kind, from) {
        (_, Some(from)) => Template::From(from),
        (Kind::Mod, None) => Template::Mod,
        (Kind::Level, None) => {
            Template::Level { size: size.unwrap_or((640, 480)), theme: theme.unwrap_or("olx-theme-dirt") }
        }
        (Kind::Skin | Kind::Theme | Kind::Core, None) => {
            anyhow::bail!("{} packs start from an existing pack: pass --from <pack dir>", kind.name())
        }
    };
    warn(&new_pack(out, kind.pack_kind(), id, name, template)?);
    println!("made {} pack {id} in {}", kind.name(), out.display());
    Ok(true)
}
