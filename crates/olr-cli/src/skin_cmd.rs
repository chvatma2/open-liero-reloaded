//! `olr-cli skin new|double|check|preview` (issue #13): the skin editor's
//! operations for agents and tests. `new` makes a blank skin pack or copies
//! an installed skin, `double` turns a classic skin double resolution by
//! Scale2x, `check` loads and validates skin packs, `preview` draws every
//! frame to a PNG. The operations are `olr_edit::skin`'s, shared with the
//! editor.

use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Subcommand;
use olr_content::Content;
use olr_data::image::Rgba;
use olr_data::skin::Skin;
use olr_edit::skin::{self, MAX_PREVIEW_SCALE};

use crate::level_cmd::parse_colour;

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: SkinCommand,
}

/// Where installed skins are found.
#[derive(clap::Args)]
struct ContentArgs {
    /// Imported content (packs written by `olr-cli import`), OLR's own
    /// packs (the `reloaded` directory beside it) over it: the skins the
    /// profiles' picker lists.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
}

#[derive(Subcommand)]
enum SkinCommand {
    /// Make a skin pack: a blank one, or a copy of a skin (doubled with --double).
    New {
        /// The directory to write; it must not exist or be empty.
        out: PathBuf,
        /// The skin to copy: an installed skin's pack id (as the profiles'
        /// picker lists them) or OLX name, or a skin pack's directory.
        #[arg(long)]
        from: Option<String>,
        /// Double resolution (art scale 4, 40 × 36 frames): a blank double
        /// skin, or the copied classic skin doubled by Scale2x.
        #[arg(long)]
        double: bool,
        /// The name menus show.
        #[arg(long)]
        name: String,
        /// The pack id, a lowercase slug. Default: the name's, made unique
        /// among the installed packs.
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        content: ContentArgs,
    },
    /// Turn a classic skin double resolution by Scale2x of every frame, as a
    /// starting point to refine.
    Double {
        /// An installed skin's pack id or OLX name, or a skin pack's directory.
        skin: String,
        /// The directory to write; it must not exist or be empty.
        #[arg(long)]
        out: PathBuf,
        /// The name menus show. Default: the skin's, with " Double".
        #[arg(long)]
        name: Option<String>,
        /// The pack id. Default: the name's, made unique among the installed packs.
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        content: ContentArgs,
    },
    /// Load and validate skin packs; exits with status 1 if any has errors.
    Check {
        /// Skin pack directories.
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
    },
    /// Draw every frame of a skin, tinted, facing both ways, on a light and a
    /// dark background with its glow and light, to a PNG.
    Preview {
        /// A skin pack's directory, or an installed skin's pack id or OLX name.
        skin: String,
        /// The player's colour, RRGGBB.
        #[arg(long, value_parser = parse_colour)]
        colour: Rgba,
        /// The PNG to write.
        #[arg(long)]
        out: PathBuf,
        /// Output pixels per world pixel, 1 to 16 (a classic skin's art
        /// pixel is half of it, a double one's a quarter).
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_PREVIEW_SCALE)))]
        scale: u32,
        #[command(flatten)]
        content: ContentArgs,
    },
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        SkinCommand::New { out, from, double, name, id, content } => {
            let from = from.as_deref().map(|f| skin_dir(f, &content.content)).transpose()?;
            new(out, from.as_deref(), *double, name, id.as_deref(), &content.content)
        }
        SkinCommand::Double { skin, out, name, id, content } => {
            let from = skin_dir(skin, &content.content)?;
            let name = match name {
                Some(n) => n.clone(),
                None => format!("{} Double", olr_pack::read_manifest(&olr_vfs::DirFs::new(&from))?.name),
            };
            new(out, Some(&from), true, &name, id.as_deref(), &content.content)
        }
        SkinCommand::Check { dirs } => Ok(check(dirs)),
        SkinCommand::Preview { skin, colour, out, scale, content } => {
            let dir = skin_dir(skin, &content.content)?;
            let loaded = olr_pack::read_skin(&olr_vfs::DirFs::new(&dir))
                .with_context(|| format!("{}", dir.display()))?
                .skin;
            let img = skin::preview(&loaded, *colour, *scale);
            std::fs::write(out, olr_pack::image_io::encode_rgba(&img))
                .with_context(|| format!("writing {}", out.display()))?;
            println!("wrote {} ({}x{}, {})", out.display(), img.width(), img.height(), describe(&loaded));
            Ok(true)
        }
    }
}

/// The skin pack `given` names: a directory holding a `pack.ron`, else an
/// installed skin by pack id or OLX name (`Content::skin_id`) in the
/// bundled content of `content`.
fn skin_dir(given: &str, content: &Path) -> anyhow::Result<PathBuf> {
    let path = Path::new(given);
    if path.join(olr_pack::MANIFEST).is_file() {
        return Ok(path.to_path_buf());
    }
    let c = Content::open_bundled(content).context("opening the content")?;
    let id = c.skin_id(given).with_context(|| {
        format!("{given}: neither a pack directory nor an installed skin in {}", content.display())
    })?;
    Ok(c.pack_dir(&id).with_context(|| format!("{id}: no directory"))?.to_path_buf())
}

/// Makes the skin pack `out` (`olr_edit::skin::new_skin`); its id is `id`,
/// else the name's (`olr_edit::new::pack_id`) unique among the installed
/// packs of `content`.
fn new(
    out: &Path,
    from: Option<&Path>,
    double: bool,
    name: &str,
    id: Option<&str>,
    content: &Path,
) -> anyhow::Result<bool> {
    let id = match id {
        Some(id) => id.to_string(),
        None => {
            let c = Content::open_bundled(content).ok();
            olr_edit::new::pack_id(name, |id| c.as_ref().is_some_and(|c| c.pack_kind(id).is_some()))
        }
    };
    let warnings = skin::new_skin(out, &id, name, from, double)?;
    crate::pack_cmd::warn(&warnings);
    let what = match (from, double) {
        (Some(f), true) => format!("{} doubled", f.display()),
        (Some(f), false) => format!("a copy of {}", f.display()),
        (None, true) => "blank, double resolution".into(),
        (None, false) => "blank".into(),
    };
    println!("made skin pack {id} ({name:?}, {what}) in {}", out.display());
    Ok(true)
}

fn check(dirs: &[PathBuf]) -> bool {
    let mut ok = true;
    for dir in dirs {
        let checked = skin::check_dir(dir);
        for line in checked.errors.iter().chain(&checked.warnings) {
            println!("{}: {line}", dir.display());
        }
        ok &= checked.errors.is_empty();
        if let Some(s) = &checked.skin {
            println!("{}: {}", dir.display(), describe(s));
        }
    }
    ok
}

/// "\"Name\", double resolution, glowing, light 60 % over 24 px".
fn describe(s: &Skin) -> String {
    let mut out = format!(
        "{:?}, {}",
        s.def.name,
        match s.def.layout.art_scale() {
            4 => "double resolution".to_string(),
            2 => "classic resolution".to_string(),
            k => format!("art scale {k}"),
        }
    );
    if s.glow.iter().any(|&g| g > 0) {
        out.push_str(", glowing");
    }
    if let Some(l) = s.def.light {
        out.push_str(&format!(", light {} % over {} px", l.intensity, l.reach));
    }
    out
}
