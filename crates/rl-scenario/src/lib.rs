//! Plays oracle scenarios (`fixtures/oracle/*.ron`) in the Rust simulation and
//! records a [`Trace`] in the oracle's format, so `rl_oracle::compare` can
//! check it against OLX's recording.
//!
//! Weapon slots, lives and kills are copied from OLX's frame -1: weapon
//! selection is simulated only from M4 on.

use std::path::Path;

use anyhow::Context;
use rl_math::AbsTime;
use rl_oracle::{Frame, Num, RopeRec, Scenario, SlotRec, Trace, WormRec};
use rl_sim::{Keys, Loadout, SimSettings, Slot, World};
use rl_terrain::{Stamp, Terrain};
use rl_vfs::DirFs;

/// Reads `<dir>/<name>.ron` and `<dir>/<name>.jsonl.gz`.
pub fn load_fixture(dir: &Path, name: &str) -> anyhow::Result<(Scenario, Trace)> {
    let ron = dir.join(format!("{name}.ron"));
    let sc = Scenario::from_ron(&std::fs::read_to_string(&ron).with_context(|| ron.display().to_string())?)?;
    let gz = dir.join(format!("{name}.jsonl.gz"));
    let trace = Trace::from_gz(&std::fs::read(&gz).with_context(|| gz.display().to_string())?)?;
    Ok((sc, trace))
}

/// Scenario names in `dir`, sorted.
pub fn fixture_names(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "ron") {
            names.push(p.file_stem().expect("file name").to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// The world a scenario starts from: the imported level and theme from
/// `content` (packs written by `rl-cli import`), OLX's settings from the trace
/// header, and the scenario's worms spawned.
pub fn load_world(content: &Path, sc: &Scenario, expected: &Trace) -> anyhow::Result<World> {
    let stem = sc.level.rsplit_once('.').map_or(sc.level.as_str(), |(s, _)| s);
    let level_id = format!("olx-level-{}", rl_data::slug::slug(stem));
    let level = rl_pack::read_level(&DirFs::new(content.join(&level_id)))
        .with_context(|| format!("level pack {level_id}"))?
        .level;
    let theme = rl_pack::read_theme(&DirFs::new(content.join(&level.def.theme)))
        .with_context(|| format!("theme pack {}", level.def.theme))?
        .theme;
    let settings =
        SimSettings::from_olx_features(&expected.header.features).map_err(|e| anyhow::anyhow!(e.0))?;
    let holes = theme.holes.iter().map(Stamp::from_image).collect();
    let mut world = World::new(settings, Terrain::from_grid(&level.material), holes, AbsTime(0));
    let start = expected.frames.first().context("trace has no frames")?;
    for (i, spawn) in sc.worms.iter().enumerate() {
        world.spawn_worm(spawn.x, spawn.y).context("the scenario has more worms than rl-sim allows")?;
        let rec = start.w.get(i).context("trace has fewer worms than the scenario")?;
        let slots = rec
            .slots
            .iter()
            .map(|s| Slot { weapon: s.id, charge: s.ch.0, reloading: s.rl, last_fire: s.lf.0 })
            .collect();
        let loadout = Loadout {
            slots,
            current_weapon: rec.wpn,
            lives: rec.lives,
            kills: rec.kills,
            deaths: rec.deaths,
        };
        world.set_loadout(i, &loadout);
    }
    Ok(world)
}

/// The world's state as a trace frame.
pub fn record(world: &World, f: i64) -> Frame {
    let w = world
        .worms()
        .iter()
        .map(|w| WormRec {
            id: w.id,
            alive: w.alive,
            x: Num(w.pos.x),
            y: Num(w.pos.y),
            vx: Num(w.vel.x),
            vy: Num(w.vel.y),
            hp: Num(w.health),
            ang: Num(w.angle),
            angv: Num(w.angle_speed),
            face: w.face as i32,
            move_dir: w.move_dir as i32,
            ground: w.on_ground,
            keys: w.state.as_int() as u32,
            wpn: w.current_weapon,
            slots: w
                .slots
                .iter()
                .map(|s| SlotRec { id: s.weapon, ch: Num(s.charge), rl: s.reloading, lf: Num(s.last_fire) })
                .collect(),
            rope: RopeRec {
                rel: w.rope.released,
                sh: w.rope.shooting,
                at: w.rope.attached,
                pw: w.rope.player_attached,
                x: Num(w.rope.pos.x),
                y: Num(w.rope.pos.y),
                vx: Num(w.rope.vel.x),
                vy: Num(w.rope.vel.y),
            },
            lives: w.lives,
            kills: w.kills,
            deaths: w.deaths,
            dirt: w.dirt_count,
        })
        .collect();
    Frame { f, w, p: vec![], b: vec![], mh: format!("{:016x}", world.terrain().material_hash()) }
}

/// Plays `sc` from frame -1 to its last frame and returns the Rust trace,
/// with the same header as `expected`.
pub fn replay(content: &Path, sc: &Scenario, expected: &Trace) -> anyhow::Result<Trace> {
    let mut world = load_world(content, sc, expected)?;
    let mut frames = vec![record(&world, -1)];
    for (f, keys) in sc.keys_per_frame().into_iter().enumerate() {
        let keys: Vec<Keys> = keys.iter().map(|k| Keys(k.0)).collect();
        world.tick(&keys);
        frames.push(record(&world, f as i64));
    }
    Ok(Trace { header: expected.header.clone(), frames })
}
