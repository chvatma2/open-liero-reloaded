//! OLR's own packs (`content/reloaded/`, issue #7: the Reloaded levels;
//! issue #21: Liero v1.0 Reloaded; issue #13: the Firefly skin, tested in
//! `skin.rs`), shipped beside the imported ones: each
//! checks clean against the committed content, none shares an id with it,
//! the game's content lists them, a bot match plays on each, Liero Factory
//! Reloaded is OLX's Liero Factory but for its look and its distant sky's
//! stars (issue #63), and
//! Liero v1.0 Reloaded plays exactly as Classic (`olx-classic`) but for its
//! name and look. On the GPU tier each plays and draws lit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_vfs::DirFs;

const FACTORY: &str = "olr-level-liero-factory-reloaded";
const CASTLE: &str = "olr-level-castlestrike-reloaded";
const RELOADED_MOD: &str = "olr-mod-liero-v1-reloaded";
const CLASSIC: &str = "olx-classic";
const FIREFLY: &str = "olr-skin-firefly";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reloaded_dir() -> PathBuf {
    repo().join("content/reloaded")
}

/// The committed imported content they ship beside (not `OLR_CONTENT_DIR`:
/// what a release holds, offline too).
fn packs_dir() -> PathBuf {
    repo().join("content/packs")
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(args).output().unwrap()
}

/// The pack ids of the packs directly under `root`.
fn ids(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(root).unwrap() {
        let dir = e.unwrap().path();
        if dir.join(olr_pack::MANIFEST).is_file() {
            out.insert(olr_pack::read_manifest(&DirFs::new(&dir)).unwrap().id);
        }
    }
    out
}

#[test]
fn reloaded_packs_check_clean() {
    let (dir, packs) = (reloaded_dir(), packs_dir());
    let o = cli(&["pack", "check", dir.to_str().unwrap(), "--content", packs.to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success() && out.contains("checked 4 packs: 0 errors, 0 warnings"),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// A higher layer hides a lower one's pack of the same id by design (spec
/// §9.4); the two shipped roots must not rely on it.
#[test]
fn reloaded_and_imported_packs_share_no_id() {
    let own = ids(&reloaded_dir());
    assert_eq!(own, BTreeSet::from([FACTORY, CASTLE, RELOADED_MOD, FIREFLY].map(String::from)));
    let imported = ids(&packs_dir());
    let shared: Vec<&String> = own.intersection(&imported).collect();
    assert!(shared.is_empty(), "content/reloaded and content/packs share {shared:?}");
}

/// The bundled content (`Content::open_bundled`, what the game, the server
/// and the tools open by default) lists both levels by their names.
#[test]
fn the_bundled_content_lists_the_reloaded_levels() {
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let levels = content.ids(PackKind::Level);
    for (id, name) in [(FACTORY, "Liero Factory Reloaded"), (CASTLE, "Castle Strike Reloaded")] {
        assert!(levels.contains(&id.to_string()), "{id}");
        assert_eq!(content.pack_name(id), Some(name));
        assert!(content.load_level(id).unwrap().has_look(), "{id} is lit");
    }
    assert!(content.ids(PackKind::Mod).contains(&RELOADED_MOD.to_string()));
    assert_eq!(content.pack_name(RELOADED_MOD), Some("Liero v1.0 Reloaded"));
    assert!(!content.load_mod(RELOADED_MOD).unwrap().look.is_empty(), "the mod has a look");
}

/// The factory's gameplay and art are OLX's: its composition (checked
/// fresh by `reloaded_packs_check_clean`) bakes the imported level's
/// material byte for byte, and its front and back pixel for pixel but
/// where the sky is see-through (issue #63): there the distant sky
/// (`distant/0.png`, level-sized at its reference placement) shows the
/// imported sky, but for its stars. Only the look and the stars are added.
#[test]
fn the_factory_is_the_imported_level_but_for_its_look() {
    use olr_format::image_io::decode_rgba;
    let (ours, olx) = (reloaded_dir().join(FACTORY), packs_dir().join("olx-level-liero-factory"));
    let read = |p: PathBuf| decode_rgba(&std::fs::read(&p).unwrap()).unwrap();
    assert!(
        std::fs::read(ours.join("material.png")).unwrap() == std::fs::read(olx.join("material.png")).unwrap()
    );
    let sky = read(ours.join("distant/0.png"));
    for file in ["front.png", "back.png"] {
        let (a, b) = (read(ours.join(file)), read(olx.join(file)));
        assert_eq!((a.width(), a.height()), (b.width(), b.height()), "{file}");
        assert_eq!((sky.width(), sky.height()), (a.width(), a.height()), "the sky is level-sized");
        let (mut through, mut stars) = (0, 0);
        for y in 0..a.height() {
            for x in 0..a.width() {
                let (p, old) = (a.get(x, y), b.get(x, y));
                if p.a != 0 {
                    assert_eq!(p, old, "{file} ({x}, {y})");
                    continue;
                }
                through += 1;
                let s = sky.get(x, y);
                assert!(p.same_rgb(s), "{file} ({x}, {y}): the distant sky's colour");
                // A star is pale; the sky is blue.
                if !s.same_rgb(old) {
                    assert!(s.r >= 0x90, "{file} ({x}, {y}): only a star differs from OLX's sky");
                    stars += 1;
                }
            }
        }
        assert!(
            through > 100_000 && stars > 0 && stars * 100 < through,
            "{file}: {through} see-through, {stars} stars"
        );
    }
    let (a, b) = (
        olr_pack::read_manifest(&DirFs::new(&ours)).unwrap(),
        olr_pack::read_manifest(&DirFs::new(&olx)).unwrap(),
    );
    assert_eq!(a.origin, b.origin, "OLX's Liero Factory.lxl at d1ad953f");
    let src = olr_format::levels::read_level_files(&DirFs::new(&ours)).unwrap();
    let imported = olr_format::levels::read_level_files(&DirFs::new(&olx)).unwrap();
    assert_eq!((src.size, &src.theme, &src.objects), (imported.size, &imported.theme, &imported.objects));
    assert!(src.look.is_some() && src.glow.is_some());
}

/// Liero v1.0 Reloaded is Classic but for its name and look (issue #21):
/// the compiled mod (`ModDef`) but for the name, every image and sound
/// byte for byte, the origin (OLX's Classic at d1ad953f); only the look
/// differs.
#[test]
fn liero_reloaded_is_classic_but_for_its_name_and_look() {
    let read = |dir: PathBuf| olr_pack::read_mod(&DirFs::new(&dir)).unwrap();
    let (ours, olx) = (read(reloaded_dir().join(RELOADED_MOD)), read(packs_dir().join(CLASSIC)));
    assert_eq!(ours.manifest.name, "Liero v1.0 Reloaded");
    assert_eq!(ours.def.name, "Liero v1.0 Reloaded");
    assert_eq!(ours.manifest.origin, olx.manifest.origin, "OLX's Classic at d1ad953f");
    let mut def = ours.def.clone();
    def.name = olx.def.name.clone();
    assert!(def == olx.def, "the compiled mod differs from Classic's");
    // Issue #19: Classic's images and sounds byte for byte, and glow masks
    // beside some of them (presentation only).
    let (masks, assets): (BTreeMap<_, _>, BTreeMap<_, _>) =
        ours.assets.clone().into_iter().partition(|(p, _)| olr_format::mods::is_glow_mask(p.as_str()));
    assert!(assets == olx.assets, "an image or sound differs from Classic's");
    let masked: Vec<_> = masks.keys().map(|p| p.as_str()).collect();
    assert_eq!(
        masked,
        [
            "gfx/blaster.glow.png",
            "gfx/flame.glow.png",
            "gfx/mininuke.glow.png",
            "gfx/napalm.glow.png",
            "gfx/nuke.glow.png"
        ]
    );
    assert_ne!(ours.look, olx.look);
    // What a match loads: the simulation's mod, the presets, the physics.
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let (a, b) = (content.load_mod(RELOADED_MOD).unwrap(), content.load_mod(CLASSIC).unwrap());
    assert!(a.sim == b.sim, "the simulation's mod differs");
    assert!(a.images == b.images && a.sounds == b.sounds);
    assert!(b.glow.is_empty() && a.glow.len() == masks.len(), "every mask loads");
    assert_eq!((&a.restrictions, a.worm, a.rope), (&b.restrictions, b.worm, b.rope));
    assert!(b.look.is_empty() && !a.look.is_empty());
    // Issue #15: its explosion lights, the mod's small default and the
    // Big Nuke's cave-wide flash; Classic keeps the engine's.
    let nuke = a.sim.projectiles().iter().position(|p| p.key == "big-nuke").unwrap() as i32;
    let (default, flash) = (a.look.explosion.as_ref().unwrap(), a.look.explosion(nuke).unwrap());
    assert_eq!((default.radius, default.light.intensity), (2, 60));
    assert_eq!(
        (flash.radius, flash.light.intensity, flash.light.reach),
        (16, 600, olr_look::Reach::Pixels(480))
    );
    assert!((0..b.sim.projectiles().len() as i32).all(|p| b.look.explosion(p).is_none()));
}

/// The same bot matches on Classic and on Liero v1.0 Reloaded give the
/// same checksums (issue #21): OLX's bots and chaos bots (which fire
/// everything), on an OLX level and a Reloaded one.
#[test]
fn liero_reloaded_plays_as_classic() {
    use olr_cli::runner::{Options, play};
    use olr_cli::spec::{MatchSpec, Personality};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    for (level, personality, seed) in
        [("olx-level-dirt-level", Personality::Deathmatch, 4), (CASTLE, Personality::Chaos, 7)]
    {
        let run = |m: &str| {
            let spec = MatchSpec { personality, ..MatchSpec::new(m, level, 6, seed, 1500) };
            let run = play(&content, &spec, &Options::default()).unwrap();
            assert!(run.failure().is_none(), "{m} on {level}: {}", run.summary());
            (run.ticks, run.replay.checksums)
        };
        let (classic, ours) = (run(CLASSIC), run(RELOADED_MOD));
        assert!(classic.0 == 1500 && classic.1.len() > 10, "{level}: {} ticks", classic.0);
        assert_eq!(ours, classic, "{level}: Liero v1.0 Reloaded played differently from Classic");
    }
}

/// OLX's bots choose their weapons by the mod's name (`olx_bot_weapons`:
/// "Classic" or "Liero v1.0" in it): Liero v1.0 Reloaded's are Classic's,
/// under every shipped preset at 20 % (rifles) and 100 % (the 100lt set).
#[test]
fn bots_arm_for_liero_reloaded_as_for_classic() {
    use olr_bots::{BotRng, GameType, olx_bot_weapons};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let (ours, classic) = (content.load_mod(RELOADED_MOD).unwrap(), content.load_mod(CLASSIC).unwrap());
    assert!(!ours.restrictions.is_empty());
    for preset in ours.restrictions.keys() {
        for lt in [20, 100] {
            let arm = |m: &olr_content::LoadedMod| {
                let mut rng = BotRng::from_seed(3);
                let w = olx_bot_weapons(&m.weapon_rules(preset), lt, &mut rng);
                (w, rng.calls())
            };
            assert_eq!(arm(&ours), arm(&classic), "{preset} at {lt} %");
        }
    }
    for (preset, lt, want) in [("Rifle only", 20, GameType::Rifles), ("Standard 100lt", 100, GameType::Lt100)]
    {
        let rules = ours.weapon_rules(preset);
        assert_eq!(olx_bot_weapons(&rules, lt, &mut BotRng::from_seed(1)).game_type, want, "{preset}");
    }
}

/// A headless bot match plays on each level, and on Liero v1.0 Reloaded
/// (invariants every tick).
#[test]
fn the_reloaded_packs_play() {
    let packs = packs_dir();
    for (m, level) in [(CLASSIC, FACTORY), (CLASSIC, CASTLE), (RELOADED_MOD, FACTORY)] {
        let o = cli(&[
            "match",
            "--mod",
            m,
            "--level",
            level,
            "--bots",
            "3",
            "--seed",
            "1",
            "--ticks",
            "300",
            "--content",
            packs.to_str().unwrap(),
        ]);
        assert!(
            o.status.success(),
            "{m} on {level}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

/// Both levels draw lit in a bot match (the GPU tier, as the playtest
/// packs), with Classic and with Liero v1.0 Reloaded.
#[test]
fn the_reloaded_levels_draw_lit() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let packs = packs_dir();
    for (m, level) in [(CLASSIC, FACTORY), (CLASSIC, CASTLE), (RELOADED_MOD, CASTLE)] {
        let out = tmp.path().join(format!("{m}-{level}.png"));
        let o = cli(&[
            "render-match",
            "--mod",
            m,
            "--level",
            level,
            "--bots",
            "3",
            "--seed",
            "1",
            "--ticks",
            "200",
            "--content",
            packs.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(o.status.success(), "{m} on {level}: {}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stdout).contains("glowing discs"), "{m} on {level}: a lit match");
    }
}
