//! `olr-cli level loose …` (the loose dirt design,
//! `docs/superpowers/specs/2026-11-05-loose-dirt-design.md`): a level's
//! loose materials, which its dirt layers name (`level element set
//! --loose`), and the hidden `probe`, the factory scripts' measure of
//! which dirt regions stay put.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Subcommand;
use olr_data::level::{LooseMaterial, Material, MaterialGrid};
use olr_edit::pack::{Pack, Source};
use olr_format::levels::{LevelSource, LooseSource};
use olr_format::loose::{GRAVITY, MAX_SLIDE_SPEED, REPOSE, TOP_SPEED};
use olr_sim::{LooseLevel, MatchSetup, OracleMode, SimSettings, TickEvents, World};

use crate::element_cmd::{self, ContentArg};

/// A loose material's rates, each left as it is when not given.
#[derive(clap::Args)]
pub(crate) struct Rates {
    /// The angle of repose, degrees (5-85, default 45).
    #[arg(long, value_parser = clap::value_parser!(u8).range(REPOSE.0 as i64..=REPOSE.1 as i64))]
    repose: Option<u8>,
    /// Gravity, pixels a second squared (1-2000, default 200).
    #[arg(long, value_parser = clap::value_parser!(u16).range(GRAVITY.0 as i64..=GRAVITY.1 as i64))]
    gravity: Option<u16>,
    /// A falling grain's top speed, pixels a second (1-500, default 50).
    #[arg(long, value_parser = clap::value_parser!(u16).range(TOP_SPEED.0 as i64..=TOP_SPEED.1 as i64))]
    top_speed: Option<u16>,
    /// How fast it slides down a slope steeper than its repose, pixels a
    /// second (0-100, default 25).
    #[arg(long, value_parser = clap::value_parser!(u8).range(0..=MAX_SLIDE_SPEED as i64))]
    slide_speed: Option<u8>,
    /// Free text for authors.
    #[arg(long)]
    notes: Option<String>,
}

impl Rates {
    fn apply(&self, m: &mut LooseSource) {
        if let Some(v) = self.repose {
            m.repose = v;
        }
        if let Some(v) = self.gravity {
            m.gravity = v;
        }
        if let Some(v) = self.top_speed {
            m.top_speed = v;
        }
        if let Some(v) = self.slide_speed {
            m.slide_speed = v;
        }
        if let Some(n) = &self.notes {
            m.notes = n.clone();
        }
    }
}

/// `level loose …`.
#[derive(Subcommand)]
pub(crate) enum LooseCommand {
    /// Print the level's loose materials: `<value>: <id> "<name>" repose
    /// R° gravity G top-speed T slide-speed S`, the value naming it in
    /// `loose.png`.
    List { pack: PathBuf },
    /// Add a loose material (at most 16); dirt layers name it with `level
    /// element set --loose`.
    Add {
        pack: PathBuf,
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[command(flatten)]
        rates: Rates,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Change a loose material's name or rates.
    Set {
        pack: PathBuf,
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        rates: Rates,
        #[command(flatten)]
        content: ContentArg,
    },
    /// Remove a loose material, refused while an element names it (or, in
    /// a level without layers, while `loose.png` holds it).
    Remove {
        pack: PathBuf,
        id: String,
        #[command(flatten)]
        content: ContentArg,
    },
    /// With every dirt pixel of the box loose at the level's first loose
    /// material (else the default rates), nothing dug and no worms, runs
    /// the step and prints each 4-connected dirt region of the box, top
    /// left first: `x,y,w,h pixels moved%`, the share of its grains that
    /// moved in the ticks. The pack is not changed.
    #[command(hide = true)]
    Probe {
        pack: PathBuf,
        /// The box in world pixels, X,Y,W,H.
        #[arg(long = "box", value_parser = parse_box)]
        area: (u32, u32, u32, u32),
        #[arg(long, default_value_t = 300)]
        ticks: u32,
        /// The mod the world is made with (the step reads none of it).
        #[arg(long, default_value = "olx-classic")]
        mod_id: String,
        #[command(flatten)]
        content: ContentArg,
    },
}

fn parse_box(s: &str) -> Result<(u32, u32, u32, u32), String> {
    let v: Vec<&str> = s.split(',').collect();
    let [x, y, w, h] = v[..] else { return Err(format!("{s:?}: X,Y,W,H")) };
    let n = |t: &str| t.trim().parse::<u32>().map_err(|_| format!("{s:?}: X,Y,W,H, whole pixels"));
    Ok((n(x)?, n(y)?, n(w)?, n(h)?))
}

fn level_of<'a>(dir: &Path, p: &'a mut Pack) -> anyhow::Result<&'a mut LevelSource> {
    match &mut p.source {
        Source::Level(l) => Ok(l),
        _ => anyhow::bail!("{}: not a level pack", dir.display()),
    }
}

pub(crate) fn run(c: &LooseCommand) -> anyhow::Result<bool> {
    match c {
        LooseCommand::List { pack } => {
            let p = element_cmd::read_level(pack)?;
            let Source::Level(l) = &p.source else { anyhow::bail!("{}: not a level pack", pack.display()) };
            for (i, m) in l.loose.iter().enumerate() {
                println!(
                    "{}: {} {:?} repose {}° gravity {} top-speed {} slide-speed {}",
                    i + 1,
                    m.id,
                    m.name,
                    m.repose,
                    m.gravity,
                    m.top_speed,
                    m.slide_speed
                );
            }
            Ok(true)
        }
        LooseCommand::Add { pack, id, name, rates, content } => edit(pack, content, |l| {
            if l.loose.iter().any(|m| m.id == *id) {
                anyhow::bail!("the level has a loose material {id:?}");
            }
            let mut m = LooseSource::new(id, name);
            rates.apply(&mut m);
            l.loose.push(m);
            Ok(format!("added loose material {id} ({})", l.loose.len()))
        }),
        LooseCommand::Set { pack, id, name, rates, content } => edit(pack, content, |l| {
            let Some(m) = l.loose.iter_mut().find(|m| m.id == *id) else {
                anyhow::bail!("no loose material {id:?} in the level");
            };
            if let Some(n) = name {
                m.name = n.clone();
            }
            rates.apply(m);
            Ok(format!("set loose material {id}"))
        }),
        LooseCommand::Remove { pack, id, content } => remove(pack, id, content),
        LooseCommand::Probe { pack, area, ticks, mod_id, content } => {
            probe(pack, *area, *ticks, mod_id, content)
        }
    }
}

/// Edits the ground's loose materials (`olr_edit::placed::edit_ground`,
/// whose level is the ground's), rebakes and saves; `op`'s message is
/// printed. The save checks the list (ids, names, ranges, the count).
fn edit(
    dir: &Path,
    content: &ContentArg,
    op: impl FnOnce(&mut LevelSource) -> anyhow::Result<String>,
) -> anyhow::Result<bool> {
    edit_loaded(dir, &mut element_cmd::load_level(dir)?, content, op)
}

/// `level loose remove`: refused while an element of the ground names it,
/// or in a plain level while `loose.png` holds its value (those above it
/// move down one).
fn remove(dir: &Path, id: &str, content: &ContentArg) -> anyhow::Result<bool> {
    let mut pack = element_cmd::load_level(dir)?;
    let users: Vec<String> = olr_edit::placed::edit_ground(&mut pack, |p| -> anyhow::Result<Vec<String>> {
        Ok(p.composition
            .iter()
            .flat_map(|c| c.composition.elements.iter())
            .filter(|e| e.loose.as_deref() == Some(id))
            .map(|e| e.id.clone())
            .collect())
    })?;
    if !users.is_empty() {
        anyhow::bail!("{}: loose material {id:?} is used by {}", dir.display(), users.join(", "));
    }
    let plain = pack.composition.is_none() && pack.placed.is_none();
    edit_loaded(dir, &mut pack, content, |l| {
        let Some(i) = l.loose.iter().position(|m| m.id == id) else {
            anyhow::bail!("no loose material {id:?} in the level");
        };
        let value = u8::try_from(i + 1).unwrap_or(u8::MAX);
        if plain && let Some(map) = l.loose_map.as_mut() {
            if map.contains(&value) {
                anyhow::bail!("loose material {id:?} is in loose.png");
            }
            map.iter_mut().filter(|v| **v > value).for_each(|v| *v -= 1);
        }
        l.loose.remove(i);
        Ok(format!("removed loose material {id}"))
    })
}

/// [`edit`] on a pack already loaded.
fn edit_loaded(
    dir: &Path,
    pack: &mut Pack,
    content: &ContentArg,
    op: impl FnOnce(&mut LevelSource) -> anyhow::Result<String>,
) -> anyhow::Result<bool> {
    let message = olr_edit::placed::edit_ground(pack, |p| -> anyhow::Result<String> {
        op(level_of(dir, p)?).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))
    })?;
    element_cmd::bake_pack(dir, pack, &content.content)?;
    crate::look_cmd::save(dir, pack, &[message])
}

/// `level loose probe`.
fn probe(
    dir: &Path,
    (bx, by, bw, bh): (u32, u32, u32, u32),
    ticks: u32,
    mod_id: &str,
    content: &ContentArg,
) -> anyhow::Result<bool> {
    let pack = element_cmd::read_level(dir)?;
    let Source::Level(l) = &pack.source else { anyhow::bail!("{}: not a level pack", dir.display()) };
    let (w, h) = l.size;
    let grid = MaterialGrid::from_cells(w, h, l.material.clone())
        .ok_or_else(|| anyhow::anyhow!("{}: material.png is not the level's size", dir.display()))?;
    let (x1, y1) = (bx.saturating_add(bw).min(w), by.saturating_add(bh).min(h));
    let in_box = |x: u32, y: u32| x >= bx && x < x1 && y >= by && y < y1;
    let cells = w as usize * h as usize;
    let mut kinds = vec![0u8; cells];
    for y in by.min(h)..y1 {
        for x in bx.min(w)..x1 {
            let i = (y * w + x) as usize;
            if l.material[i] == Material::Dirt {
                kinds[i] = 1;
            }
        }
    }
    // Each grain's region, by the cell it starts in (u32::MAX none).
    let mut region = vec![u32::MAX; cells];
    let mut regions: Vec<(u32, u32, u32, u32, u32)> = Vec::new(); // x0, y0, x1, y1, pixels
    for y in by.min(h)..y1 {
        for x in bx.min(w)..x1 {
            let i = (y * w + x) as usize;
            if kinds[i] == 0 || region[i] != u32::MAX {
                continue;
            }
            let r = regions.len() as u32;
            let mut b = (x, y, x, y, 0);
            let mut stack = vec![(x, y)];
            region[i] = r;
            while let Some((cx, cy)) = stack.pop() {
                b = (b.0.min(cx), b.1.min(cy), b.2.max(cx), b.3.max(cy), b.4 + 1);
                let near = [(cx.wrapping_sub(1), cy), (cx + 1, cy), (cx, cy.wrapping_sub(1)), (cx, cy + 1)];
                for (nx, ny) in near {
                    if !in_box(nx, ny) {
                        continue;
                    }
                    let j = (ny * w + nx) as usize;
                    if kinds[j] == 1 && region[j] == u32::MAX {
                        region[j] = r;
                        stack.push((nx, ny));
                    }
                }
            }
            regions.push(b);
        }
    }
    let material = l.loose.first().map_or(LooseMaterial::DEFAULT, LooseSource::material);
    let terrain = olr_terrain::Terrain::from_grid(&grid)
        .with_loose(kinds)
        .ok_or_else(|| anyhow::anyhow!("{}: the loose layer does not fit", dir.display()))?;
    let content = olr_content::Content::open_bundled(&content.content)?;
    let sim_mod = Arc::new(content.sim_mod(mod_id)?);
    let setup = MatchSetup {
        loose: LooseLevel::new(vec![material]).map(Arc::new),
        ..MatchSetup::new(SimSettings::lx56(), sim_mod, terrain)
    };
    let mut world =
        World::new(setup, Box::new(OracleMode)).map_err(|e| anyhow::anyhow!("{}: {e:?}", dir.display()))?;
    // Which grains moved: `grain[cell]` is the starting cell of the grain
    // there now, `moved` by starting cell.
    let mut grain: Vec<u32> = (0..cells as u32).collect();
    let mut moved = vec![false; cells];
    let mut events = TickEvents::new();
    for _ in 0..ticks {
        world.tick(&[], &mut events);
        for &(from, to) in world.loose_moves() {
            let g = grain[from as usize];
            moved[g as usize] = true;
            grain[to as usize] = g;
        }
    }
    let mut counts = vec![0u32; regions.len()];
    for (i, r) in region.iter().enumerate() {
        if *r != u32::MAX && moved[i] {
            counts[*r as usize] += 1;
        }
    }
    for ((x0, y0, rx1, ry1, n), m) in regions.iter().zip(&counts) {
        let share = f64::from(*m) * 100.0 / f64::from((*n).max(1));
        println!("{x0},{y0},{},{} {n} {share:.1}%", rx1 - x0 + 1, ry1 - y0 + 1);
    }
    Ok(true)
}
