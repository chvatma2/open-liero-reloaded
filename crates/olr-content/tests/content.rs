//! `olr-content` on synthetic packs (placeholders, worlds) and on the imported
//! OLX content (`OLR_CONTENT_DIR`, skipped with a notice when it is missing).

use std::collections::BTreeMap;
use std::path::Path;

use olr_content::{Content, PLACEHOLDER, SandboxSetup, SpawnSpec, olx_level_id, olx_mod_id};
use olr_data::core_pack::CoreDef;
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Level, LevelDef, Material, MaterialGrid};
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_data::mod_def::*;
use olr_data::restrictions::WeaponState;
use olr_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use olr_data::theme::{Theme, ThemeDef};
use olr_sim::{GAME_SOUNDS, Keys, SimSettings, TickEvents};
use olr_terrain::PX_ROCK;
use olr_vfs::{DirFs, VPath};

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

/// A mod whose one weapon fires an image projectile whose image is missing,
/// with a missing firing sound.
fn broken_mod() -> olr_pack::ModPack {
    let gfx = ProjectileGfx::Image {
        file: "gfx/missing.png".into(),
        rotating: false,
        rot_increment: 0,
        rot_speed: 0,
        use_angle: false,
        use_spec_angle: false,
        angle_images: 0,
        animation: Some(Animation { rate: 10.0, kind: AnimKind::Once }),
    };
    let shell = ProjectileDef {
        gfx,
        gravity: None,
        dampening: 1.0,
        width: 4,
        height: 4,
        timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
        trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
        hit: ActionDef { damage: 5, ..ActionDef::olx_default(ActionKind::Explode) },
        player_hit: ActionDef::olx_default(ActionKind::Injure),
        event_spawn: None,
    };
    let def = ModDef {
        name: "Broken".into(),
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
            id: "gun".into(),
            name: "Gun".into(),
            recharge: 1.0,
            drain: 1.0,
            rof: 0.5,
            laser_sight: false,
            kind: WeaponKind::Projectile {
                class: WeaponClass::Automatic,
                recoil: 0,
                sound: Some("sfx/missing.ogg".into()),
                spawn: SpawnDef { amount: 1, speed: 100, ..SpawnDef::olx_default("shell".into()) },
            },
        }],
        projectiles: [("shell".to_string(), shell)].into_iter().collect(),
        // OLX's default bans the only weapon.
        restrictions: [("Standard 100lt".to_string(), [("gun".to_string(), WeaponState::Banned)].into())]
            .into(),
        loadouts: Default::default(),
    };
    let mut assets = BTreeMap::new();
    assets.insert(VPath::new("gfx/broken.png").unwrap(), b"not a png".to_vec());
    olr_pack::ModPack {
        manifest: manifest("broken-mod", PackKind::Mod),
        def,
        assets,
        look: Default::default(),
    }
}

/// A 100 × 100 level of rock with a 30 × 30 room of background at (40..70, 40..70).
fn room_level() -> olr_pack::LevelPack {
    let cells = (0..100 * 100)
        .map(|i| {
            let (x, y) = (i % 100, i / 100);
            if (40..70).contains(&x) && (40..70).contains(&y) { Material::Background } else { Material::Rock }
        })
        .collect();
    let def = LevelDef {
        name: "Room".into(),
        width: 100,
        height: 100,
        art_scale: 1,
        theme: "test-theme".into(),
        spawns: vec![],
    };
    let level = Level {
        def,
        material: MaterialGrid::from_cells(100, 100, cells).unwrap(),
        front: filled(100, 100, Rgba::rgb(128, 128, 128)),
        back: filled(100, 100, Rgba::BLACK),
    };
    olr_pack::LevelPack {
        manifest: manifest("test-level", PackKind::Level),
        level,
        thumb: None,
        look: None,
        glow: None,
        warnings: Vec::new(),
        clear: None,
    }
}

fn theme() -> olr_pack::ThemePack {
    let hole = |n| filled(n, n, Rgba::MAGENTA);
    let theme = Theme {
        def: ThemeDef { name: "Test".into(), holes: 5, stones: 0, misc: 0 },
        holes: vec![hole(5), hole(7), hole(9), hole(11), hole(15)],
        stones: vec![],
        misc: vec![],
        front_tile: filled(4, 4, Rgba::rgb(139, 90, 43)),
        back_tile: filled(4, 4, Rgba::rgb(60, 30, 10)),
        green_ball: None,
    };
    olr_pack::ThemePack { manifest: manifest("test-theme", PackKind::Theme), theme }
}

/// A core pack whose only sprite, the hook, has the wrong size, and no sounds.
fn thin_core() -> olr_pack::CorePack {
    olr_pack::CorePack {
        manifest: manifest("olx-core", PackKind::Core),
        def: CoreDef {
            sprites: vec!["hook".into()],
            sounds: vec![],
            colours: [("RopeColour1".to_string(), Rgba::rgb(160, 80, 0))].into_iter().collect(),
        },
        sprites: [("hook".to_string(), filled(3, 3, Rgba::BLACK))].into_iter().collect(),
        sounds: BTreeMap::new(),
    }
}

/// A skin pack whose `skin.png` is too small for OLX's frame layout (format
/// 5 cannot write one: the pack is made valid, then its image replaced).
fn narrow_skin(dir: &Path) {
    let skin = Skin {
        def: SkinDef { name: "narrow".into(), layout: FrameLayout::OLX },
        image: RgbaImage::new(672, 18),
        mask: vec![MaskKind::Transparent; 672 * 18],
    };
    let pack = olr_pack::SkinPack { manifest: manifest("narrow-skin", PackKind::Skin), skin };
    olr_pack::write_skin(&mut DirFs::new(dir), &pack).unwrap();
    let small = olr_pack::image_io::encode_rgba(&filled(100, 18, Rgba::rgb(10, 200, 10)));
    std::fs::write(dir.join("skin.png"), small).unwrap();
}

/// Writes the synthetic packs into `dir`.
fn synthetic(dir: &Path) {
    let sink = |id: &str| DirFs::new(dir.join(id));
    olr_pack::write_mod(&mut sink("broken-mod"), &broken_mod()).unwrap();
    olr_pack::write_level(&mut sink("test-level"), &room_level()).unwrap();
    olr_pack::write_theme(&mut sink("test-theme"), &theme()).unwrap();
    olr_pack::write_core(&mut sink("olx-core"), &thin_core()).unwrap();
    narrow_skin(&dir.join("narrow-skin"));
    // Not a pack, and a pack this build cannot read: listed as warnings, never fatal.
    std::fs::create_dir_all(dir.join("junk")).unwrap();
    std::fs::create_dir_all(dir.join("future")).unwrap();
    std::fs::write(dir.join("future/pack.ron"), "(format_version: 99)").unwrap();
}

fn setup(worms: Vec<SpawnSpec>, seed: u32) -> SandboxSetup {
    SandboxSetup {
        mod_id: "broken-mod".into(),
        level_id: "test-level".into(),
        seed,
        settings: SimSettings::lx56(),
        worms,
        restrictions: olr_content::DEFAULT_PRESET.into(),
    }
}

/// No rock in the 6 × 6 square `FindSpot` checks around `(x, y)`.
fn free(t: &olr_terrain::Terrain, x: f32, y: f32) -> bool {
    let (x, y) = (x as i64, y as i64);
    (y - 3..y + 3).all(|yy| (x - 3..x + 3).all(|xx| t.flags(xx, yy) & PX_ROCK == 0))
}

#[test]
fn missing_assets_use_placeholders() {
    let tmp = tempfile::tempdir().unwrap();
    synthetic(tmp.path());
    let content = Content::open(tmp.path()).unwrap();
    assert!(content.warnings().iter().any(|w| w.contains("future")), "{:?}", content.warnings());

    // The core pack: a missing sprite and a wrong-sized one get a placeholder
    // of the size the renderer expects; the colours that are there are kept.
    let core = content.core();
    assert_eq!((core.sprite("crosshair").width(), core.sprite("crosshair").height()), (12, 6));
    assert_eq!(core.sprite("crosshair").get(0, 0), PLACEHOLDER);
    assert_eq!((core.sprite("hook").width(), core.sprite("hook").height()), (6, 6));
    assert_eq!(core.sprite("hook").get(0, 0), PLACEHOLDER);
    assert!(content.warnings().iter().any(|w| w.contains("hook")), "{:?}", content.warnings());
    assert_eq!(core.colour("RopeColour1"), Some(Rgba::rgb(160, 80, 0)));
    assert_eq!(core.game_sound(GAME_SOUNDS[0]), None, "silence");

    // The mod: its missing image is a one-frame placeholder, its missing
    // sounds (its own and OLX's) are silent, and each is reported once.
    let m = content.load_mod("broken-mod").unwrap();
    let img = &m.images["gfx/missing.png"];
    assert_eq!((img.width(), img.height(), img.get(0, 0)), (8, 8, PLACEHOLDER));
    assert_eq!(m.sim.projectiles()[0].frames, Some(1));
    assert!(!m.sounds.contains_key("sfx/missing.ogg"));
    assert!(m.warnings.iter().any(|w| w.contains("gfx/missing.png")), "{:?}", m.warnings);
    assert!(m.warnings.iter().any(|w| w.contains("sfx/missing.ogg")), "{:?}", m.warnings);
    assert!(m.warnings.iter().any(|w| w.contains("gfx/broken.png")), "{:?}", m.warnings);
    // The oracle's strict loader still refuses it, as OLX's client would not run it.
    assert!(content.sim_mod("broken-mod").is_err());

    // A skin of the wrong size (format 5's reader refuses it), or none at
    // all, falls back to the default or a placeholder sheet.
    assert!(content.load_skin("narrow-skin").is_err());
    let skin = content.skin_or_default("narrow-skin");
    assert_eq!((skin.skin.image.width(), skin.skin.image.height()), (672, 18));
    assert!(!skin.warnings.is_empty());
    let fallback = content.skin_or_default("no-such-skin");
    assert_eq!((fallback.skin.image.width(), fallback.skin.image.height()), (672, 18));
    assert!(content.load_skin("no-such-skin").is_err());

    // And the game plays: the gun fires its placeholder shells for 3 s.
    let spec = SpawnSpec { pos: Some((50, 60)), weapons: vec![] };
    let mut world = content.new_world(&setup(vec![spec], 1)).unwrap();
    let mut events = TickEvents::new();
    let mut fired = 0;
    for _ in 0..300 {
        world.tick(&[Keys(Keys::FIRE)], &mut events);
        fired += events.iter().filter(|e| matches!(e.kind, olr_sim::EventKind::WeaponFired { .. })).count();
    }
    assert!(fired > 0);
}

/// One undecodable core sprite (or a missing sound file) costs only that
/// asset: a placeholder or silence and a warning, never the whole core pack.
#[test]
fn an_undecodable_core_sprite_is_a_placeholder_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("olx-core");
    let mut pack = thin_core();
    pack.def.sprites = vec!["crosshair".into(), "hook".into()];
    pack.sprites.insert("crosshair".into(), filled(12, 6, Rgba::rgb(0, 255, 0)));
    pack.sprites.insert("hook".into(), filled(6, 6, Rgba::rgb(255, 0, 0)));
    pack.def.sounds = vec!["throw.ogg".into()];
    pack.sounds.insert("throw.ogg".into(), b"OggS".to_vec());
    olr_pack::write_core(&mut DirFs::new(&dir), &pack).unwrap();
    std::fs::write(dir.join("gfx/hook.png"), b"not a png").unwrap();
    std::fs::remove_file(dir.join("sounds/throw.ogg")).unwrap();

    let content = Content::open(tmp.path()).unwrap();
    let core = content.core();
    assert!(core.has_sprite("crosshair"), "the good sprite is kept: {:?}", content.warnings());
    assert_eq!(core.sprite("crosshair").get(0, 0), Rgba::rgb(0, 255, 0));
    assert!(!core.has_sprite("hook"));
    assert_eq!(core.sprite("hook").get(0, 0), PLACEHOLDER);
    assert_eq!(core.colour("RopeColour1"), Some(Rgba::rgb(160, 80, 0)), "the colours are kept");
    let w = content.warnings();
    assert!(w.iter().any(|w| w.contains("gfx/hook.png")), "{w:?}");
    assert!(w.iter().any(|w| w.contains("sounds/throw.ogg")), "{w:?}");
    assert!(!w.iter().any(|w| w.contains("placeholders and silence instead")), "{w:?}");
}

#[test]
fn new_world_spawns_worms_on_free_spots() {
    let tmp = tempfile::tempdir().unwrap();
    synthetic(tmp.path());
    let content = Content::open(tmp.path()).unwrap();
    let specs = vec![
        SpawnSpec { pos: None, weapons: vec![] },
        SpawnSpec { pos: Some((50, 50)), weapons: vec![0] },
        SpawnSpec { pos: None, weapons: vec![] },
    ];
    let world = content.new_world(&setup(specs.clone(), 7)).unwrap();
    assert_eq!(world.worms().len(), 3);
    assert_eq!((world.worms()[1].pos.x, world.worms()[1].pos.y), (50.0, 50.0));
    for w in [&world.worms()[0], &world.worms()[2]] {
        assert!(free(world.terrain(), w.pos.x, w.pos.y), "worm {} at {:?}", w.id, w.pos);
        assert_eq!(w.slots.len(), 5, "the mod's weapons, from the first");
    }
    assert!(world.mt_calls() > 0, "FindSpot drew the positions");
    // The same setup gives the same world.
    assert_eq!(content.new_world(&setup(specs.clone(), 7)).unwrap(), world);
    // Bad setups are errors, not panics.
    let bad = SpawnSpec { pos: None, weapons: vec![5] };
    assert!(content.new_world(&setup(vec![bad], 7)).is_err());
    assert!(content.new_world(&setup(vec![SpawnSpec { pos: None, weapons: vec![] }; 33], 7)).is_err());
    let mut s = setup(vec![], 7);
    s.level_id = "nope".into();
    assert!(content.new_world(&s).is_err());

    // On every imported level, when there is content.
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    for level in ["Liero Factory.lxl", "CastleStrike.lxl", "FossilFacility.lxl", "Dirt Level.lxl"] {
        let s = SandboxSetup {
            mod_id: olx_mod_id("Classic"),
            level_id: olx_level_id(level),
            seed: 3,
            settings: SimSettings::lx56(),
            worms: vec![SpawnSpec { pos: None, weapons: vec![] }; 4],
            restrictions: olr_content::DEFAULT_PRESET.into(),
        };
        let world = content.new_world(&s).unwrap();
        for w in world.worms() {
            assert!(free(world.terrain(), w.pos.x, w.pos.y), "{level}: worm {} at {:?}", w.id, w.pos);
        }
    }
}

/// The imported content (`OLR_CONTENT_DIR`), or `None` with a notice.
fn content_dir() -> Option<String> {
    let Ok(dir) = std::env::var("OLR_CONTENT_DIR") else {
        eprintln!("OLR_CONTENT_DIR is not set (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    if Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

/// `content/olx-manifest.ron`, the list of imported OLX content.
#[derive(serde::Deserialize)]
struct Manifest {
    mods: Vec<String>,
    levels: Vec<String>,
    themes: Vec<String>,
    skins: bool,
    core: Vec<String>,
}

#[test]
fn loads_every_manifest_item() {
    let Some(dir) = content_dir() else { return };
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../content/olx-manifest.ron"));
    let manifest: Manifest = ron::from_str(&text.unwrap()).unwrap();
    let content = Content::open(Path::new(&dir)).unwrap();
    assert!(content.warnings().is_empty(), "{:?}", content.warnings());
    for m in &manifest.mods {
        let loaded = content.load_mod(&olx_mod_id(m)).unwrap();
        assert!(loaded.warnings.is_empty(), "{m}: {:?}", loaded.warnings);
        // Every sound the simulation can play has bytes, OLX's game sounds from the core pack.
        assert_eq!(loaded.sounds.len(), loaded.sim.sounds().len(), "{m}");
        assert_eq!(content.sim_mod(&olx_mod_id(m)).unwrap(), *loaded.sim, "{m}: strict and lenient agree");
    }
    for l in &manifest.levels {
        let level = content.load_level(&olx_level_id(l.trim_start_matches("levels/"))).unwrap();
        assert_eq!(level.front.width(), level.material.width() * level.art_scale, "{l}");
        let theme = content.load_theme(&level.theme).unwrap();
        assert!(theme.warnings.is_empty() && theme.green_ball.is_some(), "{l}: {:?}", theme.warnings);
    }
    for t in &manifest.themes {
        let id = format!("olx-theme-{}", olr_data::slug::slug(t.rsplit('/').next().unwrap()));
        assert_eq!(content.load_theme(&id).unwrap().holes.len(), 5, "{t}");
    }
    let skins = content.ids(PackKind::Skin);
    assert_eq!(skins.is_empty(), !manifest.skins);
    for id in &skins {
        let skin = content.load_skin(id).unwrap();
        assert!(skin.warnings.is_empty(), "{id}: {:?}", skin.warnings);
    }
    let core = content.core();
    for f in &manifest.core {
        let name = f.rsplit('/').next().unwrap();
        if let Some(sprite) = name.strip_suffix(".png") {
            assert!(core.has_sprite(sprite), "{f}");
        } else if name.ends_with(".ogg") {
            assert!(core.sound(name).is_some(), "{f}");
        }
    }
    for s in GAME_SOUNDS {
        assert!(core.game_sound(s).is_some(), "{s}");
    }
    for k in ["RopeColour1", "RopeColour2", "LaserSightColour1", "LaserSightColour2", "PlayerName"] {
        assert!(core.colour(k).is_some(), "{k}");
    }
}

/// A `size` × `size` level of background at `art_scale`.
fn scaled_level(id: &str, size: u32, art_scale: u32) -> olr_pack::LevelPack {
    let def = LevelDef {
        name: id.into(),
        width: size,
        height: size,
        art_scale,
        theme: "test-theme".into(),
        spawns: vec![],
    };
    let art = filled(size * art_scale, size * art_scale, Rgba::BLACK);
    let material = MaterialGrid::new(size, size, Material::Background);
    olr_pack::LevelPack {
        manifest: manifest(id, PackKind::Level),
        level: Level { def, material, front: art.clone(), back: art },
        thumb: None,
        look: None,
        glow: None,
        warnings: Vec::new(),
        clear: None,
    }
}

/// A hostile level pack cannot make the colour map allocate gigabytes:
/// an art scale above `MAX_ART_SCALE` is an error, never an allocation.
#[test]
fn hostile_art_scale_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    // The writer refuses scale 9, so the hostile pack is an 8 edited by hand.
    for id in ["scale-8", "scale-9"] {
        olr_pack::write_level(&mut DirFs::new(tmp.path().join(id)), &scaled_level(id, 4, 8)).unwrap();
    }
    let ron = tmp.path().join("scale-9/level.ron");
    let text = std::fs::read_to_string(&ron).unwrap();
    assert!(text.contains("art_scale: 8"), "{text}");
    std::fs::write(&ron, text.replace("art_scale: 8", "art_scale: 9")).unwrap();
    let content = Content::open(tmp.path()).unwrap();
    assert_eq!(content.load_level("scale-8").unwrap().art_scale, 8);
    let err = format!("{:#}", content.load_level("scale-9").unwrap_err());
    assert!(err.contains("art_scale") && err.contains("9"), "{err}");
}

/// Hostile packs: a sound too large or too long is never handed to the
/// decoder; it is silence with a warning, from the mod or the core pack.
#[test]
fn oversized_sounds_are_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let sink = |id: &str| DirFs::new(tmp.path().join(id));
    let huge = vec![0u8; olr_content::MAX_SOUND_BYTES + 1];
    let mut m = broken_mod();
    m.assets.insert(VPath::new("sfx/missing.ogg").unwrap(), huge.clone());
    olr_pack::write_mod(&mut sink("broken-mod"), &m).unwrap();
    let mut core = thin_core();
    core.def.sounds = vec!["throw.ogg".into()];
    core.sounds.insert("throw.ogg".into(), huge);
    olr_pack::write_core(&mut sink("olx-core"), &core).unwrap();

    let content = Content::open(tmp.path()).unwrap();
    assert_eq!(content.core().sound("throw.ogg"), None);
    assert!(
        content.warnings().iter().any(|w| w.contains("throw.ogg") && w.contains("bytes")),
        "{:?}",
        content.warnings()
    );
    let m = content.load_mod("broken-mod").unwrap();
    assert!(!m.sounds.contains_key("sfx/missing.ogg"));
    assert!(
        m.warnings.iter().any(|w| w.contains("sfx/missing.ogg") && w.contains("bytes")),
        "{:?}",
        m.warnings
    );
}

/// Every match plays its own mod's worm and rope physics, as OLX does
/// (`SimSettings::with_mod_physics`): MW 1.0's are not Classic's.
#[test]
fn worlds_play_their_mods_physics() {
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    for (mod_id, gravity, jump, rest, strength) in
        [("olx-classic", 100.0, -65.0, 27, 4.5), ("olx-mw-1-0", 50.0, -75.0, 30, 3.5)]
    {
        let s = SandboxSetup {
            mod_id: mod_id.into(),
            level_id: "olx-level-dirt-level".into(),
            seed: 1,
            settings: SimSettings::lx56(),
            worms: vec![SpawnSpec::default()],
            restrictions: olr_content::DEFAULT_PRESET.into(),
        };
        let world = content.new_world(&s).unwrap();
        let st = world.settings();
        assert_eq!(
            (st.worm_gravity, st.worm_jump_force, st.rope_rest_length, st.rope_strength),
            (gravity, jump, rest, strength),
            "{mod_id}"
        );
    }
}

/// A restart builds its world from the assets loaded once, the same world
/// `new_world` loads from the packs.
#[test]
fn new_world_from_loaded_assets_is_new_world() {
    let tmp = tempfile::tempdir().unwrap();
    synthetic(tmp.path());
    let content = Content::open(tmp.path()).unwrap();
    let specs =
        vec![SpawnSpec { pos: None, weapons: vec![] }, SpawnSpec { pos: Some((50, 50)), weapons: vec![0] }];
    let s = setup(specs, 9);
    let m = content.load_mod(&s.mod_id).unwrap();
    let level = content.load_level(&s.level_id).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let world = olr_content::new_world_from(&m, &level, &theme, &s).unwrap();
    assert_eq!(world, content.new_world(&s).unwrap());
    assert_eq!(world.worms().len(), 2);
    let bad = setup(vec![SpawnSpec { pos: None, weapons: vec![7] }], 9);
    assert!(olr_content::new_world_from(&m, &level, &theme, &bad).is_err());
}

#[test]
fn natural_bonuses_hold_what_the_restrictions_allow() {
    let tmp = tempfile::tempdir().unwrap();
    synthetic(tmp.path());
    let content = Content::open(tmp.path()).unwrap();
    let mut s = setup(vec![SpawnSpec::default()], 9);
    let m = content.load_mod(&s.mod_id).unwrap();
    assert_eq!(m.name, "Broken");
    let level = content.load_level(&s.level_id).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    // The default preset bans the mod's only weapon (named case-insensitively).
    let bonus = |s: &SandboxSetup| olr_content::match_setup(&m, &level, &theme, s).unwrap().bonus_weapons;
    assert_eq!(bonus(&s), Some(vec![]));
    assert_eq!(content.weapon_rules(&s.mod_id, &s.restrictions).unwrap(), m.weapon_rules(&s.restrictions));
    // A preset the mod lacks leaves OLX's list empty: every weapon allowed.
    s.restrictions = "Bonus only".into();
    assert_eq!(bonus(&s), Some(vec![0]));
}

/// Every bundled skin's colorized sheet cuts a left-facing frame that is the
/// exact mirror of the right-facing one (OLX's one-column offset dropped,
/// spec 2026-10-16 formats §4).
#[test]
fn every_bundled_skin_mirrors() {
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    let ids = content.ids(PackKind::Skin);
    assert!(!ids.is_empty());
    for id in ids.iter().filter(|i| i.starts_with("olx-skin-")) {
        let sheet = content.load_skin(id).unwrap().colorized(Rgba::rgb(200, 60, 20));
        for f in 0..olr_content::SHEET_FRAMES {
            let r = olr_content::skin_sprite(&sheet, f, olr_sim::Dir::Right);
            let l = olr_content::skin_sprite(&sheet, f, olr_sim::Dir::Left);
            for y in 0..r.height() {
                for x in 0..r.width() {
                    assert_eq!(l.get(x, y), r.get(r.width() - 1 - x, y), "{id} frame {f} ({x}, {y})");
                }
            }
        }
    }
}

/// Pack format 5 replaced 4: a format-4 pack in a user layer is skipped with
/// a "re-import" warning, hides nothing and never fails the open.
#[test]
fn a_format_4_pack_in_a_user_layer_is_skipped_with_a_re_import_warning() {
    let bundled = tempfile::tempdir().unwrap();
    synthetic(bundled.path());
    let user = tempfile::tempdir().unwrap();
    olr_pack::write_level(&mut DirFs::new(user.path().join("test-level")), &room_level()).unwrap();
    let old = PackManifest { format_version: 4, ..manifest("test-level", PackKind::Level) };
    olr_pack::write_manifest(&mut DirFs::new(user.path().join("test-level")), &old).unwrap();
    let content = Content::open_layers(&[user.path(), bundled.path()]).unwrap();
    let warning = content
        .warnings()
        .iter()
        .find(|w| w.contains("test-level") && w.contains("skipped"))
        .unwrap_or_else(|| panic!("no warning in {:?}", content.warnings()));
    assert!(warning.contains("re-import") && warning.contains("format_version 4"), "{warning}");
    // The bundled level of the same id still plays.
    let hash = olr_pack::content_hash(&DirFs::new(bundled.path().join("test-level"))).unwrap();
    assert!(content.has("test-level", &hash));
}

#[test]
fn from_source_equals_load_level() {
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    let id = "olx-level-dirt-level";
    let src = olr_format::levels::read_level_files(&DirFs::new(Path::new(&dir).join(id))).unwrap();
    let a = olr_content::LoadedLevel::from_source(&src).unwrap();
    let b = content.load_level(id).unwrap();
    assert_eq!(a.name, b.name);
    assert_eq!(a.material, b.material);
    assert_eq!(a.front, b.front);
    assert_eq!(a.back, b.back);
    assert_eq!(a.art_scale, b.art_scale);
    assert_eq!(a.theme, b.theme);
    assert_eq!(a.spawns, b.spawns);
    let mut bad = src.clone();
    bad.material.pop();
    assert!(olr_content::LoadedLevel::from_source(&bad).is_err(), "a source that does not compile");
}

#[test]
fn the_first_spawn_is_used_only_when_inside_the_level() {
    use olr_data::level::SpawnPoint;
    let mut cells = vec![Material::Background; 1200];
    cells[5 * 40 + 20] = Material::Rock;
    let level = |spawns: &[(i32, i32)]| olr_content::LoadedLevel {
        name: "l".into(),
        material: MaterialGrid::from_cells(40, 30, cells.clone()).unwrap(),
        front: RgbaImage::new(40, 30),
        back: RgbaImage::new(40, 30),
        art_scale: 1,
        theme: "t".into(),
        spawns: spawns.iter().map(|&(x, y)| SpawnPoint { x, y }).collect(),
        look: None,
        glow: None,
        warnings: Vec::new(),
        clear: None,
    };
    assert_eq!(level(&[(-1, 5), (10, 10)]).first_spawn(), None, "only the first one counts");
    assert_eq!(level(&[(10, 10)]).first_spawn(), Some((10, 10)));
    assert_eq!(level(&[]).first_spawn(), None);
    assert_eq!(level(&[(40, 0)]).first_spawn(), None);
    assert_eq!(level(&[(0, 30)]).first_spawn(), None);
    // OLX's `IsGoodSpawnPoint`: no rock (outside counts as rock) in the
    // 6 × 6 square from (x - 3, y - 3) to (x + 2, y + 2).
    assert_eq!(level(&[(37, 27)]).first_spawn(), Some((37, 27)));
    assert_eq!(level(&[(38, 27)]).first_spawn(), None, "touching the edge");
    assert_eq!(level(&[(3, 3)]).first_spawn(), Some((3, 3)));
    assert_eq!(level(&[(2, 3)]).first_spawn(), None);
    assert_eq!(level(&[(20, 5)]).first_spawn(), None, "inside rock: a free spot");
    assert_eq!(level(&[(23, 8)]).first_spawn(), None, "rock in the square");
    assert_eq!(level(&[(24, 8)]).first_spawn(), Some((24, 8)));
    assert_eq!(level(&[(i32::MIN, i32::MAX)]).first_spawn(), None);
}
