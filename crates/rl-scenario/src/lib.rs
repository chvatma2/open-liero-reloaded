//! Plays oracle scenarios (`fixtures/oracle/**/*.ron`) in the Rust simulation and
//! records a [`Trace`] in the oracle's format, so `rl_oracle::compare` can
//! check it against OLX's recording.
//!
//! A scenario's weapons are set by name, as OLX's `simSetWeapons` does;
//! without them, and for lives and kills, the slots are copied from OLX's
//! frame -1 (OLX picks them before the scenario's seed is set).

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use rl_content::{Content, olx_level_id, olx_mod_id};
use rl_math::AbsTime;
use rl_oracle::{
    BonusRec, Fields, Frame, Num, ProjRec, RopeRec, Scenario, ServerBonusRec, ShotRec, SlotRec, Trace,
    WormRec,
};
use rl_sim::{
    BonusItem, CLOCK_OFFSET_MS, Keys, Loadout, MAX_WORMS, MatchSetup, OracleMode, SimMod, SimSettings, Slot,
    TickEvents, World, WormSetup,
};
use rl_terrain::{ColorMap, Terrain};

/// The trace fields the simulation reproduces; the acceptance test compares
/// these. Since plan 4 task 9 (bonuses) it is every group, [`Fields::ALL`].
pub const CHECKED: Fields = Fields::WORM_MOTION
    .union(Fields::SLOTS)
    .union(Fields::ROPE)
    .union(Fields::MATERIAL)
    .union(Fields::HEALTH)
    .union(Fields::SCORE)
    .union(Fields::RESPAWN)
    .union(Fields::CLOCKS)
    .union(Fields::RAND)
    .union(Fields::SHOTS)
    .union(Fields::PROJECTILES)
    .union(Fields::PROJ_HIDDEN)
    .union(Fields::BONUSES);

/// Reads `<dir>/<name>.ron` and `<dir>/<name>.jsonl.gz`.
pub fn load_fixture(dir: &Path, name: &str) -> anyhow::Result<(Scenario, Trace)> {
    let ron = dir.join(format!("{name}.ron"));
    let sc = Scenario::from_ron(&std::fs::read_to_string(&ron).with_context(|| ron.display().to_string())?)?;
    let gz = dir.join(format!("{name}.jsonl.gz"));
    let trace = Trace::from_gz(&std::fs::read(&gz).with_context(|| gz.display().to_string())?)?;
    Ok((sc, trace))
}

/// Scenario names in `dir` and its subdirectories, sorted: the `.ron` paths
/// relative to `dir` without the extension, with `/` separators (such as
/// `walk` or `weapons/classic/bazooka`).
pub fn fixture_names(dir: &Path) -> anyhow::Result<Vec<String>> {
    fn walk(dir: &Path, prefix: &str, names: &mut Vec<String>) -> anyhow::Result<()> {
        for e in std::fs::read_dir(dir).with_context(|| dir.display().to_string())? {
            let p = e?.path();
            if p.is_dir() {
                let sub = p.file_name().expect("directory name").to_string_lossy();
                walk(&p, &format!("{prefix}{sub}/"), names)?;
            } else if p.extension().is_some_and(|x| x == "ron") {
                names.push(format!("{prefix}{}", p.file_stem().expect("file name").to_string_lossy()));
            }
        }
        Ok(())
    }
    let mut names = Vec::new();
    walk(dir, "", &mut names)?;
    names.sort();
    Ok(names)
}

/// Checks that the simulation numbers weapons and projectiles as OLX did
/// (the trace header's names, indexed by OLX's ids). v1 headers have none.
fn check_ids(m: &SimMod, header: &rl_oracle::Header) -> anyhow::Result<()> {
    let weapons: Vec<&str> = m.weapons().iter().map(|w| w.name.as_str()).collect();
    if !header.weapons.is_empty() && weapons != header.weapons {
        anyhow::bail!("weapons differ from OLX's: {weapons:?} vs {:?}", header.weapons);
    }
    let projs: Vec<&str> = m.projectiles().iter().map(|p| p.name.as_str()).collect();
    if !header.projectiles.is_empty() && projs != header.projectiles {
        anyhow::bail!("projectiles differ from OLX's: {projs:?} vs {:?}", header.projectiles);
    }
    Ok(())
}

/// The client options `rl-sim` assumes (`Game.Blood` 100, `Game.Particles`
/// on, OLX's defaults); a v1 header without them passes.
fn check_options(header: &rl_oracle::Header) -> anyhow::Result<()> {
    for (k, want) in [("Game.Blood", "100"), ("Game.Particles", "true")] {
        if let Some(v) = header.options.get(k) {
            anyhow::ensure!(v == want, "option {k}={v} is not supported (rl-sim assumes {want})");
        }
    }
    Ok(())
}

/// `T0`, frame -1's absolute `simulationTime` (0 for traces without it).
fn t0(expected: &Trace) -> u64 {
    expected.header.options.get("T0").and_then(|t| t.parse().ok()).unwrap_or(0)
}

/// The world a scenario starts from: the imported level, theme and mod from
/// `content` (packs written by `rl-cli import`; the mod loaded strictly, as
/// OLX's client, [`Content::sim_mod`]), OLX's settings from the trace header,
/// and the scenario's worms spawned with their weapons and health.
pub fn load_world(content: &Content, sc: &Scenario, expected: &Trace) -> anyhow::Result<World> {
    let level = content.load_level(&olx_level_id(&sc.level))?;
    let theme = content.load_theme(&level.theme).with_context(|| format!("level {}", sc.level))?;
    let sim_mod = content.sim_mod(&olx_mod_id(&sc.mod_dir))?;
    check_ids(&sim_mod, &expected.header)?;
    check_options(&expected.header)?;
    let settings =
        SimSettings::from_olx_features(&expected.header.features).map_err(|e| anyhow::anyhow!(e.0))?;
    // The scenario's seed is the header's: the generators start clean at the second `simSetSeed`.
    let seed = expected.header.seed.unwrap_or(sc.seed);
    let start = expected.frames.first().context("trace has no frames")?;
    anyhow::ensure!(sc.worms.len() <= MAX_WORMS, "the scenario has more worms than rl-sim allows");
    let mut worms = Vec::with_capacity(sc.worms.len());
    for (i, spawn) in sc.worms.iter().enumerate() {
        let rec = start.w.get(i).context("trace has fewer worms than the scenario")?;
        let mut loadout = Loadout {
            slots: rec
                .slots
                .iter()
                .map(|s| Slot { weapon: s.id, charge: s.ch.0, reloading: s.rl, last_fire: s.lf.0 })
                .collect(),
            current_weapon: rec.wpn,
            lives: rec.lives,
            kills: rec.kills,
            deaths: rec.deaths,
        };
        if let Some(ws) = sc.weapons.iter().find(|w| w.worm == i) {
            // OLX's simSetWeapons: FindWeaponId by name, full and ready.
            let id = |name: &String| sim_mod.weapons().iter().position(|w| &w.name == name).map(|i| i as i32);
            loadout.slots = ws
                .names
                .iter()
                .map(|n| Ok(Slot::ready(id(n).with_context(|| format!("no weapon {n:?}"))?)))
                .collect::<anyhow::Result<_>>()?;
            loadout.current_weapon = ws.current as i32;
        }
        worms.push(WormSetup { team: 0, loadout, spawn: Some((spawn.x, spawn.y)) });
    }
    let setup = MatchSetup {
        holes: theme.stamps(),
        green_mask: theme.green_mask(),
        start: AbsTime(t0(expected)),
        seed,
        worms,
        ..MatchSetup::new(settings, Arc::new(sim_mod), Terrain::from_grid(&level.material))
    };
    // OLX's oracle runs with game over off: `OracleMode` credits kills and never ends.
    let mut world = World::new(setup, Box::new(OracleMode)).map_err(|e| anyhow::anyhow!("{e}"))?;
    for &(i, hp) in &sc.health {
        anyhow::ensure!(i < world.worms().len(), "health for missing worm {i}");
        world.set_health(i, hp);
    }
    Ok(world)
}

/// Milliseconds since frame -1, or `None` for OLX's unset time (0).
fn since_t0(t: AbsTime, t0: u64) -> Option<i64> {
    (t.0 != 0).then(|| t.0 as i64 - t0 as i64)
}

/// The world's state as a trace frame. `t0` is frame -1's absolute time.
pub fn record(world: &World, f: i64, t0: u64) -> Frame {
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
            suicides: Some(w.suicides),
            tod: if w.alive { None } else { since_t0(w.time_of_death, t0) },
            respawn: Some(
                w.respawn_requested as u32 | (w.can_respawn_now as u32) << 1 | (w.spawned_once as u32) << 2,
            ),
            blood: since_t0(w.last_blood, t0),
        })
        .collect();
    let s = world
        .last_shots()
        .iter()
        .map(|s| ShotRec {
            w: s.worm,
            wpn: s.weapon,
            x: Num(s.pos.x),
            y: Num(s.pos.y),
            vx: Num(s.worm_vel.x),
            vy: Num(s.worm_vel.y),
            ang: s.angle,
            rnd: s.random,
            spd: s.speed,
            rel: s.release,
            t: s.time.0 as i64,
            spawn: since_t0(s.spawn, t0).unwrap_or(0),
        })
        .collect();
    let p = world
        .projectiles()
        .map(|(i, p)| ProjRec {
            i: i as i32,
            t: p.proj as i32,
            x: Num(p.pos.x),
            y: Num(p.pos.y),
            vx: Num(p.vel.x),
            vy: Num(p.vel.y),
            life: Num(p.life),
            own: Some(p.owner),
            rnd: Some(p.random),
            ls: since_t0(p.last_sim_time, t0),
            sp: since_t0(p.spawn_time, t0),
            ig: since_t0(p.ignore_worm_coll_before, t0),
            ws: Some(Num(p.wallshoot_time)),
            tr: since_t0(p.last_trail_proj, t0),
            ox: Some(Num(p.old_pos.x)),
            oy: Some(Num(p.old_pos.y)),
            fr: Some(Num(p.frame)),
            col: Some(p.colour),
        })
        .collect();
    let b = (0..)
        .zip(world.bonuses())
        .filter(|(_, b)| b.used)
        .map(|(i, b)| BonusRec {
            i,
            t: b.kind as i32,
            x: Num(b.pos.x),
            y: Num(b.pos.y),
            vx: Some(Num(b.vel.x)),
            vy: Some(Num(b.vel.y)),
            life: Some(Num(b.life)),
            wpn: Some(b.weapon),
        })
        .collect();
    let sb = (0..)
        .zip(world.server_bonuses())
        .filter(|(_, b)| b.used)
        .map(|(i, b)| ServerBonusRec {
            i,
            t: b.kind as i32,
            x: Num(b.pos.x),
            y: Num(b.pos.y),
            wpn: b.weapon,
            spawn: since_t0(b.spawn_time, t0).unwrap_or(0),
        })
        .collect();
    // OLX records a frame before `simulationTime` advances past it.
    let frame_ms = if f >= 0 { world.frame_dt().0 as i64 } else { 0 };
    let st = world.time().0 as i64 - t0 as i64 - frame_ms;
    Frame {
        f,
        sf: Some(world.server_frame() as i64),
        st: Some(st),
        ct: Some(st + CLOCK_OFFSET_MS as i64),
        s,
        p,
        w,
        b,
        sb,
        rc: Some(world.rand_calls()),
        mt: Some(world.mt_calls()),
        ents: Some(world.entity_count() as i64),
        mh: format!("{:016x}", world.terrain().material_hash()),
    }
}

/// The scenario's bonuses as `(frame, x, y, item)`. A weapon is found by
/// name, ignoring case, as OLX's `FindWeaponId` does.
fn bonus_items(world: &World, sc: &Scenario) -> anyhow::Result<Vec<(u32, i32, i32, BonusItem)>> {
    sc.bonuses
        .iter()
        .map(|b| {
            let item = match (b.kind, &b.weapon) {
                (rl_oracle::BonusKind::Health, _) => BonusItem::Health,
                (rl_oracle::BonusKind::Weapon, None) => anyhow::bail!("a weapon bonus without a weapon"),
                (rl_oracle::BonusKind::Weapon, Some(name)) => {
                    let weapons = world.sim_mod().weapons();
                    let id = weapons.iter().position(|w| w.name.eq_ignore_ascii_case(name));
                    BonusItem::Weapon(id.with_context(|| format!("no bonus weapon {name:?}"))? as i32)
                }
            };
            Ok((b.frame, b.x, b.y, item))
        })
        .collect()
}

/// Plays `sc` from frame -1 to its last frame and returns the Rust trace,
/// with the same header as `expected`.
pub fn replay(content: &Content, sc: &Scenario, expected: &Trace) -> anyhow::Result<Trace> {
    let t0 = t0(expected);
    let mut frames = Vec::new();
    play(content, sc, expected, |world, f, _| frames.push(record(world, f, t0)))?;
    Ok(Trace { header: expected.header.clone(), frames })
}

/// Plays `sc` and calls `each(world, frame, events)` for frame -1 (no events
/// yet) and after every frame's tick.
pub fn play(
    content: &Content,
    sc: &Scenario,
    expected: &Trace,
    mut each: impl FnMut(&World, i64, &TickEvents),
) -> anyhow::Result<World> {
    let mut world = load_world(content, sc, expected)?;
    let bonuses = bonus_items(&world, sc)?;
    let mut events = TickEvents::new();
    each(&world, -1, &events);
    for (f, keys) in sc.keys_per_frame().into_iter().enumerate() {
        let keys: Vec<Keys> = keys.iter().map(|k| Keys(k.0)).collect();
        for &(frame, x, y, item) in &bonuses {
            if frame as usize == f {
                world.place_bonus(x, y, item).map_err(|e| anyhow::anyhow!("frame {f}: bonus: {e}"))?;
            }
        }
        world.tick(&keys, &mut events);
        each(&world, f as i64, &events);
    }
    Ok(world)
}

/// The level's colour terrain after `sc` (its terrain edits replayed with
/// `rl_content::apply_edits`): what the CPU render goldens compare.
pub fn color_after(content: &Content, sc: &Scenario, expected: &Trace) -> anyhow::Result<ColorMap> {
    let level = content.load_level(&olx_level_id(&sc.level))?;
    let theme = content.load_theme(&level.theme).with_context(|| format!("level {}", sc.level))?;
    let mut map = level.color_map(&theme);
    play(content, sc, expected, |world, _, events| {
        rl_content::apply_edits(&mut map, events, world.terrain())
    })?;
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_client_options_are_errors() {
        let mut h = rl_oracle::Header::default();
        check_options(&h).unwrap();
        h.options.insert("Game.Blood".into(), "100".into());
        h.options.insert("Game.Particles".into(), "true".into());
        check_options(&h).unwrap();
        h.options.insert("Game.Blood".into(), "50".into());
        assert!(check_options(&h).unwrap_err().to_string().contains("Game.Blood=50"));
    }

    #[test]
    fn fixture_names_include_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "walk.ron",
            "walk.jsonl.gz",
            "v1.0/x.ron",
            "weapons/classic/zimm.ron",
            "weapons/classic/bazooka.ron",
            "a/notes.txt",
        ] {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "").unwrap();
        }
        assert_eq!(
            fixture_names(dir.path()).unwrap(),
            ["v1.0/x", "walk", "weapons/classic/bazooka", "weapons/classic/zimm"]
        );
    }
}
