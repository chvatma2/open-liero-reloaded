//! Replays resolved through `olr-content` (spec §20 F8): pack ids and
//! content hashes name the content, a replay refuses content that differs,
//! and a bot match on OLX's content (`OLR_CONTENT_DIR`, skipped without it)
//! replays to the same checksums.

use std::path::Path;
use std::sync::Arc;

use olr_bots::{AreaCache, Bot, BotConfig, BotRng, Difficulty, Personality, choose_loadout};
use olr_content::{Content, DEFAULT_PRESET, SandboxSetup, SpawnSpec, match_setup};
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Level, LevelDef, Material, MaterialGrid};
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_data::mod_def::*;
use olr_data::theme::{Theme, ThemeDef};
use olr_modes::{ChaosCycle, Deathmatch, DeathmatchSettings, RespawnLoadout};
use olr_replay::{FORMAT_VERSION, Mismatch, ModeSpec, Recorder, Replay, ReplayError, SetupCall};
use olr_sim::{Keys, SimSettings, TickEvents, World};
use olr_vfs::DirFs;

fn manifest(id: &str, kind: PackKind) -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: id.into(),
        version: "1".into(),
        kind,
        authors: vec![],
        origin: None,
        license: None,
    }
}

fn filled(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

/// One beam weapon.
fn mod_pack() -> olr_pack::ModPack {
    let def = ModDef {
        name: "Beams".into(),
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
            id: "beam".into(),
            name: "Beam".into(),
            recharge: 1.0,
            drain: 0.0,
            rof: 0.05,
            laser_sight: false,
            kind: WeaponKind::Beam {
                recoil: 0,
                beam: BeamDef {
                    colour: Rgba::rgb(255, 0, 0),
                    damage: 1,
                    player_damage: 5,
                    length: 60,
                    init_width: 1,
                    width_increase: 0.0,
                    distribute_damage_over_width: false,
                },
            },
        }],
        projectiles: Default::default(),
        restrictions: Default::default(),
        loadouts: Default::default(),
    };
    olr_pack::ModPack {
        manifest: manifest("beam-mod", PackKind::Mod),
        def,
        assets: Default::default(),
        look: Default::default(),
    }
}

/// 160 × 120 of dirt over a rock floor, open above y 60; `rock` adds a
/// rock pixel at (0, 0).
fn level_pack(rock: bool) -> olr_pack::LevelPack {
    let (w, h) = (160u32, 120u32);
    let cells = (0..w * h)
        .map(|i| match (i % w, i / w) {
            (0, 0) if rock => Material::Rock,
            (_, y) if y >= 110 => Material::Rock,
            (_, y) if y >= 60 => Material::Dirt,
            _ => Material::Background,
        })
        .collect();
    let def = LevelDef {
        name: "Pit".into(),
        width: w,
        height: h,
        art_scale: 1,
        theme: "pit-theme".into(),
        spawns: vec![],
    };
    let level = Level {
        def,
        material: MaterialGrid::from_cells(w, h, cells).unwrap(),
        front: filled(w, h, Rgba::rgb(128, 128, 128)),
        back: filled(w, h, Rgba::BLACK),
    };
    olr_pack::LevelPack {
        manifest: manifest("pit-level", PackKind::Level),
        level,
        thumb: None,
        look: None,
        glow: None,
        warnings: Vec::new(),
        clear: None,
        distant: Vec::new(),
    }
}

fn theme_pack() -> olr_pack::ThemePack {
    let hole = |n| filled(n, n, Rgba::MAGENTA);
    let theme = Theme {
        def: ThemeDef { name: "Pit".into(), holes: 5, stones: 0, misc: 0 },
        holes: vec![hole(5), hole(7), hole(9), hole(11), hole(15)],
        stones: vec![],
        misc: vec![],
        front_tile: filled(4, 4, Rgba::rgb(139, 90, 43)),
        back_tile: filled(4, 4, Rgba::rgb(60, 30, 10)),
        green_ball: None,
    };
    olr_pack::ThemePack { manifest: manifest("pit-theme", PackKind::Theme), theme }
}

fn write_packs(dir: &Path, rock: bool) {
    let sink = |id: &str| DirFs::new(dir.join(id));
    olr_pack::write_mod(&mut sink("beam-mod"), &mod_pack()).unwrap();
    olr_pack::write_level(&mut sink("pit-level"), &level_pack(rock)).unwrap();
    olr_pack::write_theme(&mut sink("pit-theme"), &theme_pack()).unwrap();
}

/// A recorded 300-tick Deathmatch of two worms on the synthetic packs.
fn record(c: &Content) -> Replay {
    let sandbox = SandboxSetup {
        mod_id: "beam-mod".into(),
        level_id: "pit-level".into(),
        seed: 4,
        settings: SimSettings::lx56(),
        worms: vec![SpawnSpec::default(); 2],
        restrictions: DEFAULT_PRESET.into(),
    };
    let m = c.load_mod(&sandbox.mod_id).unwrap();
    let level = c.load_level(&sandbox.level_id).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    let mut setup = match_setup(&m, &level, &theme, &sandbox).unwrap();
    let dm = DeathmatchSettings::default();
    let mode = ModeSpec::Deathmatch(dm.clone());
    let game = Deathmatch::prepare(dm, &mut setup).unwrap();
    let mut world = World::new(setup.clone(), Box::new(game)).unwrap();
    let mut rec =
        Recorder::new(c.replay_packs(&sandbox.mod_id, &sandbox.level_id).unwrap(), &setup, &mode, vec![]);
    rec.apply(&mut world, SetupCall::AlignPhysicsClock).unwrap();
    rec.start(&world).unwrap();
    let mut out = TickEvents::new();
    for t in 0..300u32 {
        let k = [Keys(if t % 40 < 20 { Keys::LEFT | Keys::FIRE } else { Keys::RIGHT }), Keys(Keys::JUMP)];
        world.tick(&k, &mut out);
        rec.push(&k, &world).unwrap();
    }
    rec.finish(&world).unwrap()
}

/// Review Focus 3: a replay against content with another hash, missing
/// content or another format version is refused with a clear error.
#[test]
fn replay_refuses_mismatched_content() {
    let tmp = tempfile::tempdir().unwrap();
    write_packs(tmp.path(), false);
    let c = Content::open(tmp.path()).unwrap();
    let replay = record(&c);
    assert_eq!(replay.setup.packs.theme.id, "pit-theme");
    let run = c.play_replay(&replay).unwrap();
    assert_eq!((run.ticks, run.checked), (300, 4));

    // The errors stay typed, for `olr-cli replay` to read the tick.
    let mut diverged = replay.clone();
    diverged.checksums[2].1 ^= 1;
    let err = c.play_replay(&diverged).unwrap_err();
    assert_eq!(err.downcast_ref::<Mismatch>().map(|m| m.tick), Some(200), "{err}");
    let mut bad_call = replay.clone();
    bad_call.setup.calls.push(SetupCall::SetHealth { worm: 9, health: 1.0 });
    let err = c.world_for_replay(&bad_call).unwrap_err();
    assert!(matches!(err.downcast_ref::<ReplayError>(), Some(ReplayError::NoWorm { worm: 9, .. })), "{err}");

    // Plan 7 ruling Q4: a thumbnail is presentation only; adding one to
    // the level (as an import of format 4 does) keeps the replay valid.
    let mut thumbed = level_pack(false);
    thumbed.thumb = Some(filled(olr_pack::THUMB_SIZE.0, olr_pack::THUMB_SIZE.1, Rgba::rgb(1, 2, 3)));
    olr_pack::write_level(&mut DirFs::new(tmp.path().join("pit-level")), &thumbed).unwrap();
    let with_thumb = Content::open(tmp.path()).unwrap();
    assert_eq!(with_thumb.play_replay(&replay).unwrap().checked, 4);

    let mut other = replay.clone();
    other.setup.packs.mod_pack.hash = "0".repeat(64);
    let err = c.world_for_replay(&other).unwrap_err().to_string();
    assert!(err.contains("beam-mod") && err.contains("content hash"), "{err}");

    // The level changed by one pixel under the same id.
    write_packs(tmp.path(), true);
    let changed = Content::open(tmp.path()).unwrap();
    let err = changed.world_for_replay(&replay).unwrap_err().to_string();
    assert!(err.contains("pit-level") && err.contains("content hash"), "{err}");
    assert!(changed.play_replay(&replay).is_err());

    let mut missing = replay.clone();
    missing.setup.packs.level.id = "no-such-level".into();
    assert!(c.world_for_replay(&missing).is_err());

    let mut bytes = replay.encode();
    bytes[4..6].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    let err = Replay::decode(&bytes).unwrap_err().to_string();
    assert!(err.contains("version"), "{err}");
}

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

/// A minute of 8 bots (Deathmatch and chaos) on Classic: recorded, encoded,
/// decoded and replayed from the content to the same checksums.
#[test]
fn bot_match_replays_from_content() {
    let Some(c) = content() else { return };
    let (mod_id, level_id, seed, n) = ("olx-classic", "olx-level-dirt-level", 21u32, 8usize);
    let m = c.load_mod(mod_id).unwrap();
    let level = c.load_level(level_id).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    let settings = SimSettings::lx56();
    let sandbox = SandboxSetup {
        mod_id: mod_id.into(),
        level_id: level_id.into(),
        seed,
        settings: settings.clone(),
        worms: vec![SpawnSpec::default(); n],
        restrictions: DEFAULT_PRESET.into(),
    };
    let mut setup = match_setup(&m, &level, &theme, &sandbox).unwrap();
    let rules = m.weapon_rules(DEFAULT_PRESET);
    let cfg = |i: usize| BotConfig {
        personality: if i >= 6 { Personality::Chaos } else { Personality::Deathmatch },
        difficulty: [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard, Difficulty::Xtreme][i % 4],
        ..BotConfig::default()
    };
    for i in 0..n {
        setup.worms[i].loadout =
            choose_loadout(&cfg(i), &rules, &settings, &mut BotRng::new(seed, i, cfg(i).personality));
    }
    let dm = DeathmatchSettings {
        respawn_loadout: RespawnLoadout::ChaosCycle(ChaosCycle::new(&[6, 7], m.sim.weapons().len())),
        ..DeathmatchSettings::default()
    };
    let mode = ModeSpec::Deathmatch(dm.clone());
    let game = Deathmatch::prepare(dm, &mut setup).unwrap();
    let mut world = World::new(setup.clone(), Box::new(game)).unwrap();
    let mut rec = Recorder::new(c.replay_packs(mod_id, level_id).unwrap(), &setup, &mode, vec![]);
    rec.start(&world).unwrap();
    let cache = AreaCache::new(world.terrain());
    let mut bots: Vec<Bot> =
        (0..n).map(|i| Bot::with_cache(i, cfg(i), seed, &world, Arc::clone(&cache))).collect();
    let mut out = TickEvents::new();
    let mut keys = vec![Keys(0); n];
    for _ in 0..6000 {
        for (i, b) in bots.iter_mut().enumerate() {
            keys[i] = b.think(&world, &out);
        }
        world.tick(&keys, &mut out);
        rec.push(&keys, &world).unwrap();
    }
    let replay = rec.finish(&world).unwrap();
    let bytes = replay.encode();
    eprintln!("one minute of 8 bots on Classic: {} bytes", bytes.len());
    // About 12.6 KB (seed 21); the bound leaves room for changes to the bots.
    assert!(bytes.len() <= 20_000, "{} bytes", bytes.len());
    let run = c.play_replay(&Replay::decode(&bytes).unwrap()).unwrap();
    assert_eq!((run.ticks, run.checked), (6000, 61));
    assert_eq!(run.world, world);
}
