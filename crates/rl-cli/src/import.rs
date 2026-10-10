use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rl_data::manifest::Origin;
use rl_vfs::{ContentSource, DirFs, VPath};
use serde::Serialize;

#[derive(clap::Args)]
pub struct Args {
    /// OLX game directory (`share/gamedir` of an OpenLieroX checkout).
    #[arg(long)]
    olx: PathBuf,
    /// Output directory; each pack is written to `<out>/<pack id>/`.
    #[arg(long)]
    out: PathBuf,
    /// Source revision recorded in each pack's origin, such as a git commit.
    #[arg(long, default_value = "unknown")]
    revision: String,
    /// Mod directories relative to the game directory, such as "MW 1.0".
    #[arg(long = "mod")]
    mods: Vec<String>,
    /// Level files relative to the game directory, such as "levels/CastleStrike.lxl".
    #[arg(long = "level")]
    levels: Vec<String>,
    /// Theme directories relative to the game directory, such as "data/themes/Dirt".
    #[arg(long = "theme")]
    themes: Vec<String>,
    /// Import every PNG in `skins/`.
    #[arg(long)]
    skins: bool,
}

/// One line of `import-report.ron`.
#[derive(Serialize)]
struct Entry {
    source: String,
    pack: Option<String>,
    error: Option<String>,
    warnings: Vec<String>,
}

enum Imported {
    Mod(rl_import_lgs::ImportedMod),
    Level(rl_import_lxl::ImportedLevel),
    Skin(rl_import_gfx::ImportedSkin),
    Theme(rl_import_gfx::ImportedTheme),
}

impl Imported {
    fn id(&self) -> &str {
        match self {
            Imported::Mod(m) => &m.pack.manifest.id,
            Imported::Level(l) => &l.pack.manifest.id,
            Imported::Skin(s) => &s.pack.manifest.id,
            Imported::Theme(t) => &t.pack.manifest.id,
        }
    }

    fn set_id(&mut self, id: String) {
        match self {
            Imported::Mod(m) => m.pack.manifest.id = id,
            Imported::Level(l) => l.pack.manifest.id = id,
            Imported::Skin(s) => s.pack.manifest.id = id,
            Imported::Theme(t) => t.pack.manifest.id = id,
        }
    }

    fn warnings(&self) -> Vec<String> {
        match self {
            Imported::Mod(m) => m.warnings.clone(),
            Imported::Level(l) => l.warnings.clone(),
            Imported::Skin(s) => s.warnings.clone(),
            Imported::Theme(t) => t.warnings.clone(),
        }
    }

    fn write(&self, dir: &Path) -> anyhow::Result<()> {
        if dir.exists() {
            std::fs::remove_dir_all(dir).with_context(|| format!("removing old {}", dir.display()))?;
        }
        let mut sink = DirFs::new(dir);
        match self {
            Imported::Mod(m) => rl_pack::write_mod(&mut sink, &m.pack)?,
            Imported::Level(l) => rl_pack::write_level(&mut sink, &l.pack)?,
            Imported::Skin(s) => rl_pack::write_skin(&mut sink, &s.pack)?,
            Imported::Theme(t) => rl_pack::write_theme(&mut sink, &t.pack)?,
        }
        Ok(())
    }
}

enum Job {
    Mod(String),
    Level(String),
    Theme(String),
    Skin(VPath),
}

impl Job {
    fn source(&self) -> String {
        match self {
            Job::Mod(s) | Job::Level(s) | Job::Theme(s) => s.clone(),
            Job::Skin(p) => p.to_string(),
        }
    }

    fn run(&self, olx: &DirFs, revision: &str) -> anyhow::Result<Imported> {
        let source = self.source();
        let origin = Origin {
            source: "OpenLieroX".to_string(),
            path: format!("share/gamedir/{source}"),
            revision: revision.to_string(),
        };
        Ok(match self {
            Job::Mod(m) => Imported::Mod(rl_import_lgs::import_mod(olx, &VPath::new(m)?, origin)?),
            Job::Level(l) => Imported::Level(rl_import_lxl::import_lxl(olx, &VPath::new(l)?, origin)?),
            Job::Theme(t) => Imported::Theme(rl_import_gfx::import_theme(olx, &VPath::new(t)?, origin)?),
            Job::Skin(p) => Imported::Skin(rl_import_gfx::import_skin(olx, p, origin)?),
        })
    }
}

/// Returns `Ok(false)` if any item failed; the report lists every item either way.
pub fn run(args: &Args) -> anyhow::Result<bool> {
    let olx = DirFs::new(&args.olx);
    let mut jobs: Vec<Job> = Vec::new();
    jobs.extend(args.mods.iter().cloned().map(Job::Mod));
    jobs.extend(args.levels.iter().cloned().map(Job::Level));
    jobs.extend(args.themes.iter().cloned().map(Job::Theme));
    if args.skins {
        let skins = VPath::new("skins")?;
        for name in olx.list(&skins).context("listing skins/")? {
            if name.to_ascii_lowercase().ends_with(".png") {
                jobs.push(Job::Skin(skins.join(&name)?));
            }
        }
    }

    std::fs::create_dir_all(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    let mut report = Vec::new();
    let mut ids = BTreeSet::new();
    for job in &jobs {
        let source = job.source();
        match job.run(&olx, &args.revision) {
            Ok(mut item) => {
                let base = item.id().to_string();
                let id = (1..)
                    .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
                    .find(|c| !ids.contains(c))
                    .expect("unbounded");
                item.set_id(id.clone());
                ids.insert(id.clone());
                item.write(&args.out.join(&id))?;
                report.push(Entry { source, pack: Some(id), error: None, warnings: item.warnings() });
            }
            Err(e) => {
                eprintln!("error: {source}: {e:#}");
                report.push(Entry { source, pack: None, error: Some(format!("{e:#}")), warnings: vec![] });
            }
        }
    }
    let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())? + "\n";
    std::fs::write(args.out.join("import-report.ron"), text)?;
    let failed = report.iter().filter(|e| e.error.is_some()).count();
    let warned: usize = report.iter().map(|e| e.warnings.len()).sum();
    println!(
        "imported {} packs, {failed} failed, {warned} warnings (see import-report.ron)",
        report.len() - failed
    );
    Ok(failed == 0)
}
