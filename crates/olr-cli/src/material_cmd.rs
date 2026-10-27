//! `olr-cli material list|check` and `olr-cli level material
//! list|add` (issue #38): the surface material libraries (spec
//! `2026-10-22-materials-and-heat-design.md`) and a level's materials for
//! agents. The built-in library is `content/materials/` beside
//! `--content`; the user's is `materials/` in the game's data directory
//! (`--user` names another). A level holds copies of the materials it
//! uses, by id: `add` copies a library's in, and keeps the level's own
//! copy of one it has.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use olr_edit::material::{MaterialEntry, list_materials, read_file};
use olr_edit::pack::Source;
use olr_format::materials::MaterialSource;

use crate::element_cmd;

/// Where the libraries are.
#[derive(clap::Args)]
pub(crate) struct LibraryArgs {
    /// The content directory: the built-in materials are `materials/`
    /// beside it.
    #[arg(long, default_value = "content/packs")]
    content: PathBuf,
    /// The user's material library (default: `materials/` in the game's
    /// data directory, such as `~/.local/share/open-liero-reloaded/materials`).
    #[arg(long)]
    user: Option<PathBuf>,
}

impl LibraryArgs {
    /// Every material offered, built-in first, with the warnings.
    fn list(&self) -> (Vec<MaterialEntry>, Vec<String>) {
        let builtin = olr_content::materials_dir(&self.content);
        let user = self.user.clone().or_else(|| {
            directories::ProjectDirs::from("", "", "open-liero-reloaded")
                .map(|d| d.data_dir().join("materials"))
        });
        list_materials(builtin.as_deref(), user.as_deref())
    }
}

#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    command: MaterialCommand,
}

#[derive(Subcommand)]
enum MaterialCommand {
    /// Print the material library, built-in first, then the user's: `<id>
    /// "<name>" built-in|user` and its heat, or `no heat`.
    List {
        #[command(flatten)]
        library: LibraryArgs,
    },
    /// Read material files (`<id>.ron`) and check them; exits with status 1
    /// if any does not.
    Check {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
}

/// `level material …`.
#[derive(Subcommand)]
pub(crate) enum LevelMaterialCommand {
    /// Print the level's materials: `<value>: <id> "<name>"`, the value
    /// naming it in painted maps and `level element surface --map`.
    List { pack: PathBuf },
    /// Copy materials into the level's look, by id from the library (or
    /// --file); one the level has is kept as it is. The look is made when
    /// the level has none.
    Add {
        pack: PathBuf,
        /// Library material ids.
        ids: Vec<String>,
        /// Material files to copy in instead of library ones.
        #[arg(long)]
        file: Vec<PathBuf>,
        #[command(flatten)]
        library: LibraryArgs,
    },
}

pub fn run(args: &Args) -> anyhow::Result<bool> {
    match &args.command {
        MaterialCommand::List { library } => {
            let (list, warnings) = library.list();
            crate::pack_cmd::warn(&warnings);
            for e in &list {
                println!(
                    "{} {:?} {}{}",
                    e.material.id,
                    e.material.name,
                    if e.builtin { "built-in" } else { "user" },
                    heat(&e.material)
                );
            }
            Ok(true)
        }
        MaterialCommand::Check { files } => Ok(check(files)),
    }
}

/// ` heat: sensitivity S, capacity C, conductivity K, cooling L, light P`,
/// or ` no heat`.
fn heat(m: &MaterialSource) -> String {
    match &m.heat {
        None => " no heat".into(),
        Some(h) => format!(
            " heat: sensitivity {}, capacity {}, conductivity {}, cooling {}, light {}",
            h.sensitivity, h.capacity, h.conductivity, h.cooling, h.look.light
        ),
    }
}

fn check(files: &[PathBuf]) -> bool {
    let mut ok = true;
    for f in files {
        match read_file(f) {
            Ok(m) => println!("{}: {} {:?}{}", f.display(), m.id, m.name, heat(&m)),
            Err(e) => {
                ok = false;
                println!("{}: error: {e}", f.display());
            }
        }
    }
    ok
}

/// `level material …`.
pub(crate) fn level_cmd(c: &LevelMaterialCommand) -> anyhow::Result<bool> {
    match c {
        LevelMaterialCommand::List { pack } => {
            let p = element_cmd::read_level(pack)?;
            let Source::Level(l) = &p.source else { anyhow::bail!("{}: not a level pack", pack.display()) };
            for (i, m) in l.look.iter().flat_map(|k| k.materials.iter()).enumerate() {
                println!("{}: {} {:?}", i + 1, m.id, m.name);
            }
            Ok(true)
        }
        LevelMaterialCommand::Add { pack, ids, file, library } => add(pack, ids, file, library),
    }
}

fn add(dir: &Path, ids: &[String], files: &[PathBuf], library: &LibraryArgs) -> anyhow::Result<bool> {
    if ids.is_empty() && files.is_empty() {
        anyhow::bail!("name library materials by id, or material files with --file");
    }
    let mut wanted = Vec::new();
    if !ids.is_empty() {
        let (list, warnings) = library.list();
        crate::pack_cmd::warn(&warnings);
        for id in ids {
            // The built-in one first, as the library lists them.
            let Some(e) = list.iter().find(|e| e.material.id == *id) else {
                anyhow::bail!("no material {id:?} in the library (`olr-cli material list`)");
            };
            wanted.push(e.material.clone());
        }
    }
    for f in files {
        wanted.push(read_file(f).map_err(|e| anyhow::anyhow!("{}: {e}", f.display()))?);
    }
    let mut pack = element_cmd::load_level(dir)?;
    for m in &wanted {
        crate::look_cmd::refuse_brought(dir, &pack, "material", &m.id, true)?;
    }
    // The ground's materials (issue #74).
    let messages = olr_edit::placed::edit_ground(&mut pack, |p| -> anyhow::Result<Vec<String>> {
        let Source::Level(l) = &mut p.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
        let mut messages = Vec::new();
        for m in wanted {
            if l.look.iter().flat_map(|k| k.materials.iter()).any(|x| x.id == m.id) {
                messages.push(format!("kept material {} (the level has it)", m.id));
                continue;
            }
            let id = m.id.clone();
            let i =
                olr_edit::look::add_material(l, m).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
            messages.push(format!("added material {id} ({})", i + 1));
        }
        Ok(messages)
    })?;
    let content = element_cmd::ContentArg { content: library.content.clone() };
    crate::look_cmd::rebake_beside_copies(dir, &mut pack, &content)?;
    // A look made for them draws the thumbnail lit.
    crate::look_cmd::save(dir, &mut pack, &messages)
}
