//! Plays oracle scenarios (`fixtures/oracle/**/*.ron`) in the Rust simulation and
//! records a [`Trace`] in the oracle's format, so `rl_oracle::compare` can
//! check it against OLX's recording.
//!
//! A scenario's weapons are set by name, as OLX's `simSetWeapons` does;
//! without them, and for lives and kills, the slots are copied from OLX's
//! frame -1 (OLX picks them before the scenario's seed is set).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use rl_math::AbsTime;
use rl_oracle::{Fields, Frame, Num, ProjRec, RopeRec, Scenario, ShotRec, SlotRec, Trace, WormRec};
use rl_sim::{CLOCK_OFFSET_MS, Keys, Loadout, SimMod, SimSettings, Slot, World};
use rl_terrain::{Stamp, Terrain};
use rl_vfs::DirFs;

/// The trace fields the simulation reproduces so far; the acceptance test
/// compares only these. Grows with M4 until it is [`Fields::ALL`].
pub const CHECKED: Fields = Fields::WORM_MOTION
    .union(Fields::SLOTS)
    .union(Fields::ROPE)
    .union(Fields::MATERIAL)
    .union(Fields::HEALTH)
    .union(Fields::SCORE)
    .union(Fields::RAND)
    .union(Fields::SHOTS)
    .union(Fields::PROJECTILES)
    .union(Fields::PROJ_HIDDEN);

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

/// The mod pack for OLX mod directory `mod_dir` (such as `MW 1.0`), compiled
/// for the simulation. Image frame counts are `width / height` of the
/// pack's PNG images (spec §18 D3); an image projectile whose image is
/// missing (or has height 0) is an error.
pub fn load_mod(content: &Path, mod_dir: &str) -> anyhow::Result<SimMod> {
    let id = format!("olx-{}", rl_data::slug::slug(mod_dir));
    let pack = rl_pack::read_mod(&DirFs::new(content.join(&id))).with_context(|| format!("mod pack {id}"))?;
    let mut frames = BTreeMap::new();
    for (path, bytes) in &pack.assets {
        if path.as_str().ends_with(".png") {
            let img =
                rl_pack::image_io::decode_rgba(bytes).map_err(|e| anyhow::anyhow!("{id}/{path}: {e}"))?;
            if img.height() > 0 {
                frames.insert(path.as_str().to_string(), img.width() / img.height());
            }
        }
    }
    let m = SimMod::compile(&pack.def, &frames).map_err(|e| anyhow::anyhow!("{id}: {e}"))?;
    check_frames(&m).with_context(|| format!("mod pack {id}"))?;
    Ok(m)
}

/// Every image projectile must know its image's frame count: OLX's client
/// does, and without it `ANI_ONCE` would never end a projectile.
fn check_frames(m: &SimMod) -> anyhow::Result<()> {
    for p in m.projectiles() {
        if let rl_data::mod_def::ProjectileGfx::Image { file, .. } = &p.def.gfx {
            anyhow::ensure!(p.frames.is_some(), "projectile {} has no image {file:?}", p.name);
        }
    }
    Ok(())
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

/// `T0`, frame -1's absolute `simulationTime` (0 for traces without it).
fn t0(expected: &Trace) -> u64 {
    expected.header.options.get("T0").and_then(|t| t.parse().ok()).unwrap_or(0)
}

/// The world a scenario starts from: the imported level, theme and mod from
/// `content` (packs written by `rl-cli import`), OLX's settings from the
/// trace header, and the scenario's worms spawned with their weapons and health.
pub fn load_world(content: &Path, sc: &Scenario, expected: &Trace) -> anyhow::Result<World> {
    let stem = sc.level.rsplit_once('.').map_or(sc.level.as_str(), |(s, _)| s);
    let level_id = format!("olx-level-{}", rl_data::slug::slug(stem));
    let level = rl_pack::read_level(&DirFs::new(content.join(&level_id)))
        .with_context(|| format!("level pack {level_id}"))?
        .level;
    let theme = rl_pack::read_theme(&DirFs::new(content.join(&level.def.theme)))
        .with_context(|| format!("theme pack {}", level.def.theme))?
        .theme;
    let sim_mod = load_mod(content, &sc.mod_dir)?;
    check_ids(&sim_mod, &expected.header)?;
    let settings =
        SimSettings::from_olx_features(&expected.header.features).map_err(|e| anyhow::anyhow!(e.0))?;
    let holes = theme.holes.iter().map(Stamp::from_image).collect();
    // The scenario's seed is the header's: the generators start clean at the second `simSetSeed`.
    let seed = expected.header.seed.unwrap_or(sc.seed);
    let terrain = Terrain::from_grid(&level.material);
    let mut world = World::new(settings, Arc::new(sim_mod), terrain, holes, AbsTime(t0(expected)), seed);
    let start = expected.frames.first().context("trace has no frames")?;
    for (i, spawn) in sc.worms.iter().enumerate() {
        world.spawn_worm(spawn.x, spawn.y).context("the scenario has more worms than rl-sim allows")?;
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
            let id = |name: &String| {
                world.sim_mod().weapons().iter().position(|w| &w.name == name).map(|i| i as i32)
            };
            loadout.slots = ws
                .names
                .iter()
                .map(|n| {
                    let weapon = id(n).with_context(|| format!("no weapon {n:?}"))?;
                    Ok(Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 })
                })
                .collect::<anyhow::Result<_>>()?;
            loadout.current_weapon = ws.current as i32;
        }
        world.set_loadout(i, &loadout);
    }
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
            ..WormRec::default()
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
    // OLX records a frame before `simulationTime` advances past it.
    let frame_ms = if f >= 0 { 10 } else { 0 };
    let st = world.time().0 as i64 - t0 as i64 - frame_ms;
    Frame {
        f,
        sf: Some(world.server_frame() as i64),
        st: Some(st),
        ct: Some(st + CLOCK_OFFSET_MS as i64),
        s,
        p,
        w,
        rc: Some(world.rand_calls()),
        mt: Some(world.mt_calls()),
        ents: Some(world.entity_count() as i64),
        mh: format!("{:016x}", world.terrain().material_hash()),
        ..Frame::default()
    }
}

/// Plays `sc` from frame -1 to its last frame and returns the Rust trace,
/// with the same header as `expected`.
pub fn replay(content: &Path, sc: &Scenario, expected: &Trace) -> anyhow::Result<Trace> {
    let mut world = load_world(content, sc, expected)?;
    let t0 = t0(expected);
    let mut frames = vec![record(&world, -1, t0)];
    for (f, keys) in sc.keys_per_frame().into_iter().enumerate() {
        let keys: Vec<Keys> = keys.iter().map(|k| Keys(k.0)).collect();
        world.tick(&keys);
        frames.push(record(&world, f as i64, t0));
    }
    Ok(Trace { header: expected.header.clone(), frames })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_data::mod_def::*;

    #[test]
    fn an_image_projectile_without_its_image_is_an_error() {
        let gfx = ProjectileGfx::Image {
            file: "gfx/flame.png".into(),
            rotating: false,
            rot_increment: 0,
            rot_speed: 0,
            use_angle: false,
            use_spec_angle: false,
            angle_images: 0,
            animation: Some(Animation { rate: 10.0, kind: AnimKind::Once }),
        };
        let flame = ProjectileDef {
            gfx,
            gravity: None,
            dampening: 1.0,
            width: 4,
            height: 4,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef::olx_default(ActionKind::Explode),
            player_hit: ActionDef::olx_default(ActionKind::Injure),
            event_spawn: None,
        };
        let def = ModDef {
            name: "test".into(),
            worm: WormParams {
                angle_speed: 150.0,
                ground_speed: 8.0,
                air_speed: 4.0,
                gravity: 100.0,
                jump_force: -65.0,
                air_friction: 0.0,
                ground_friction: 0.4,
            },
            rope: RopeParams { max_length: 300, rest_length: 27, strength: 4.5 },
            weapons: vec![WeaponDef {
                id: "flamer".into(),
                name: "Flamer".into(),
                recharge: 1.0,
                drain: 1.0,
                rof: 0.01,
                laser_sight: false,
                kind: WeaponKind::Projectile {
                    class: WeaponClass::Automatic,
                    recoil: 0,
                    sound: None,
                    spawn: SpawnDef { amount: 1, ..SpawnDef::olx_default("flame".into()) },
                },
            }],
            projectiles: [("flame".to_string(), flame)].into_iter().collect(),
        };
        let missing = SimMod::compile(&def, &BTreeMap::new()).unwrap();
        let err = check_frames(&missing).unwrap_err().to_string();
        assert!(err.contains("flamer") && err.contains("gfx/flame.png"), "{err}");
        let found = SimMod::compile(&def, &[("gfx/flame.png".to_string(), 7)].into_iter().collect()).unwrap();
        check_frames(&found).unwrap();
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
