//! OLR's own packs (`content/reloaded/`, issue #7: the Reloaded levels;
//! issue #21: Liero v1.0 Reloaded; issue #13: the Firefly skin, tested in
//! `skin.rs`), shipped beside the imported ones: each
//! checks clean against the committed content, none shares an id with it,
//! the game's content lists them, a bot match plays on each, Liero Factory
//! After Dark (Liero Factory Reloaded until issue #70) is OLX's Liero
//! Factory but for its look and its distant sky's stars (issue #63), its
//! PCs and headlights cast glow beams, both levels bake as before the
//! renames of issue #70, and
//! Liero v1.0 Reloaded plays exactly as Classic (`olx-classic`) but for its
//! name and look. On the GPU tier each plays and draws lit.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use olr_content::Content;
use olr_data::manifest::PackKind;
use olr_vfs::DirFs;

const AFTER_DARK: &str = "olr-level-liero-factory-after-dark";
pub(crate) const FACTORY: &str = "olr-level-liero-factory-reloaded";
const CASTLE: &str = "olr-level-castlestrike-reloaded";
const RELOADED_MOD: &str = "olr-mod-liero-v1-reloaded";
pub(crate) const CLASSIC: &str = "olx-classic";
const FIREFLY: &str = "olr-skin-firefly";

pub(crate) fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub(crate) fn reloaded_dir() -> PathBuf {
    repo().join("content/reloaded")
}

/// The committed imported content they ship beside (not `OLR_CONTENT_DIR`:
/// what a release holds, offline too).
pub(crate) fn packs_dir() -> PathBuf {
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
        o.status.success() && out.contains("checked 5 packs: 0 errors, 0 warnings"),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// A higher layer hides a lower one's pack of the same id by design (spec
/// §9.4); the two shipped roots must not rely on it.
#[test]
fn reloaded_and_imported_packs_share_no_id() {
    let own = ids(&reloaded_dir());
    assert_eq!(own, BTreeSet::from([AFTER_DARK, FACTORY, CASTLE, RELOADED_MOD, FIREFLY].map(String::from)));
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
    for (id, name) in [
        (AFTER_DARK, "Liero Factory After Dark"),
        (FACTORY, "Liero Factory Reloaded"),
        (CASTLE, "Castle Strike Reloaded"),
    ] {
        assert!(levels.contains(&id.to_string()), "{id}");
        assert_eq!(content.pack_name(id), Some(name));
        assert!(content.load_level(id).unwrap().has_look(), "{id} is lit");
    }
    assert!(content.ids(PackKind::Mod).contains(&RELOADED_MOD.to_string()));
    assert_eq!(content.pack_name(RELOADED_MOD), Some("Liero v1.0 Reloaded"));
    assert!(!content.load_mod(RELOADED_MOD).unwrap().look.is_empty(), "the mod has a look");
}

/// Liero Factory After Dark's gameplay and art are OLX's: its
/// composition (checked fresh by `reloaded_packs_check_clean`) bakes the
/// imported level's material byte for byte, and its front and back pixel
/// for pixel but where the sky is see-through (issue #63): there the
/// distant sky (`distant/0.png`, level-sized at its reference placement)
/// shows the imported sky, but for its stars, and behind the roof garden (the old
/// sky the flowers hid, see-through since 2026-10-25) within 2 a channel
/// of it. The windows are glass (the background glass design): each pane
/// OLX's colour halfway to pale blue at 35 % (alpha 89), the hall
/// window's broken panes (its second, fifth and sixth, x 739-757, 799-817
/// and 819-838, y 151-172) open to the sky where broken, ringed by paler
/// glass at 82 % (alpha 209). Only the look,
/// the stars, the moon and the glass are added.
#[test]
fn the_factory_is_the_imported_level_but_for_its_look() {
    use olr_format::image_io::decode_rgba;
    let (ours, olx) = (reloaded_dir().join(AFTER_DARK), packs_dir().join("olx-level-liero-factory"));
    let read = |p: PathBuf| decode_rgba(&std::fs::read(&p).unwrap()).unwrap();
    assert!(
        std::fs::read(ours.join("material.png")).unwrap() == std::fs::read(olx.join("material.png")).unwrap()
    );
    let sky = read(ours.join("distant/0.png"));
    for file in ["front.png", "back.png"] {
        let (a, b) = (read(ours.join(file)), read(olx.join(file)));
        assert_eq!((a.width(), a.height()), (b.width(), b.height()), "{file}");
        assert_eq!((sky.width(), sky.height()), (a.width(), a.height()), "the sky is level-sized");
        let (mut through, mut stars, mut panes, mut rim, mut hole) = (0, 0, 0, 0, 0);
        let broken = |x: u32, y: u32| {
            [739..=757, 799..=817, 819..=838].iter().any(|r| r.contains(&x)) && (151..=172).contains(&y)
        };
        for y in 0..a.height() {
            for x in 0..a.width() {
                let (p, old) = (a.get(x, y), b.get(x, y));
                if p.a == 89 {
                    // Halfway, a half rounded either way.
                    let half =
                        |v: u8, o: u8, g: u8| (2 * i16::from(v) - i16::from(o) - i16::from(g)).abs() <= 1;
                    let pane = half(p.r, old.r, 200) && half(p.g, old.g, 215) && half(p.b, old.b, 235);
                    assert!(pane, "{file} ({x}, {y}): a pane {p:?} of {old:?}");
                    panes += 1;
                    continue;
                }
                if p.a == 209 {
                    assert!(broken(x, y) && (p.r, p.g, p.b) == (215, 225, 240), "{file} ({x}, {y}): the rim");
                    rim += 1;
                    continue;
                }
                if p.a != 0 {
                    assert_eq!(p, old, "{file} ({x}, {y})");
                    continue;
                }
                if broken(x, y) && old.a == 255 && !p.same_rgb(old) {
                    assert!(p.same_rgb(sky.get(x, y)), "{file} ({x}, {y}): the hole shows the sky");
                    hole += 1;
                    continue;
                }
                through += 1;
                let s = sky.get(x, y);
                assert!(p.same_rgb(s), "{file} ({x}, {y}): the distant sky's colour");
                // A star is pale; the sky is blue. The gradient the
                // distant sky continues behind the roof garden is
                // within 2 a channel of OLX's.
                let near =
                    [(s.r, old.r), (s.g, old.g), (s.b, old.b)].iter().all(|&(a, b)| a.abs_diff(b) <= 2);
                if !near {
                    assert!(s.r >= 0x90, "{file} ({x}, {y}): only a star differs from OLX's sky");
                    stars += 1;
                }
            }
        }
        assert!(
            through > 100_000 && stars > 0 && stars * 100 < through,
            "{file}: {through} see-through, {stars} stars"
        );
        assert_eq!((panes, rim, hole), (3739, 95, 622), "{file}: the glass");
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

/// Liero Factory After Dark's PCs and headlights cast glow beams (the
/// directional-glow design, 2026-10-25): the side PCs' screens at x 344 and 513 face right
/// and the one at 498 left, 35° each; the truck's headlights (42-44 ×
/// 410-414) left and a little down, 166° at 15°; the blue floor's PC stays
/// undirected, and no backdrop pixel is directional. The haze is 50: the
/// air and the sky in a beam glow with its own light.
#[test]
fn the_factorys_pcs_and_headlights_cast_beams() {
    let src = olr_format::levels::read_level_files(&DirFs::new(reloaded_dir().join(AFTER_DARK))).unwrap();
    assert_eq!(src.look.as_ref().unwrap().haze, 50);
    assert!(src.haze.is_none(), "no haze map: the haze is even");
    let map = src.glow_beams.expect("look/glow-beam.png");
    let (w, h) = map.world;
    let mut beams = BTreeMap::new();
    for y in 0..h {
        for x in 0..w {
            assert_eq!(map.get(true, x, y), None, "({x}, {y}): a backdrop beam");
            if let Some(b) = map.get(false, x, y) {
                beams.insert((x, y), b);
            }
        }
    }
    for (&(x, y), &b) in &beams {
        let want = match (x, y) {
            (42..=44, 410..=414) => (166, 15),
            (344 | 513, 259..=263) => (0, 35),
            (498, 259..=263) => (180, 35),
            _ => panic!("({x}, {y}): an unexpected beam {b:?}"),
        };
        assert_eq!(b, want, "({x}, {y})");
    }
    // 15 headlight pixels, 5 + 4 right-facing screen pixels, 5 left.
    assert_eq!(beams.len(), 29);
}

/// The SHA-256 of each baked file of a Reloaded level (`material.png`,
/// `front.png`, `back.png` and every `look/` file), against `want`.
fn assert_bakes_as(level: &str, want: &[(&str, &str)]) {
    use sha2::{Digest, Sha256};
    let dir = reloaded_dir().join(level);
    let mut look: Vec<String> = std::fs::read_dir(dir.join("look"))
        .unwrap()
        .map(|e| format!("look/{}", e.unwrap().file_name().to_str().unwrap()))
        .collect();
    look.sort();
    let mut files = vec!["material.png".to_string(), "front.png".into(), "back.png".into()];
    files.extend(look);
    let got: Vec<(String, String)> = files
        .into_iter()
        .map(|f| {
            let hash = Sha256::digest(std::fs::read(dir.join(&f)).unwrap());
            let hex = hash.iter().map(|b| format!("{b:02x}")).collect();
            (f, hex)
        })
        .collect();
    let want: Vec<(String, String)> = want.iter().map(|&(f, h)| (f.into(), h.into())).collect();
    assert_eq!(got, want, "{level}'s bake changed");
}

/// Issue #70 renamed Liero Factory Reloaded to Liero Factory After Dark
/// and shortened five element ids (`rock-scr`, `rock-disp`, `rock-bcn`,
/// `ctrl-scr`, `backdrop`): ids name files only, so the bake is the old
/// factory's byte for byte, the hashes taken before the renames. Issue
/// #74's conversion to the placement format is proven against them too.
#[test]
fn after_dark_bakes_as_the_old_factory() {
    assert_bakes_as(
        AFTER_DARK,
        &[
            ("material.png", "cc750fbac345973c858b238fd1352756d34854e8f54373aaf6ff63963ce33d2e"),
            ("front.png", "24fed3d690832539f89a784b271b8b7ec93c1dc404784f988892decb791bd99a"),
            ("back.png", "171a5f472d4482113c10433ffa6ce1f18202f5c87f307161fda13b271146e9a5"),
            ("look/glass.png", "3e639c76211d57a841c4af6ec7ac860c5b9775d6f206786ccc8fbe4515c4692f"),
            ("look/glow-back.png", "82174a094512f9ea5d62432dfd1e677d60f1683a3addad13c872114175e35f0c"),
            ("look/glow-beam-back.png", "439810bec0c41ff2c7ba94a727cde1898de9c3951b235be5bd5df0e99e876639"),
            ("look/glow-beam.png", "9bc44d7515b50750dda74ce4f30e19935101dc774e09184ad0c841bf9109d021"),
            ("look/glow.png", "8984a92588edd73e8315921341341da4592ce482181a9c8a2c7ae11a5657be81"),
            ("look/groups-back.png", "a8251f8ac65250246cb59f8fe1f0d9883ac894c104d5449f623d62a118974264"),
            ("look/groups.png", "6812b9f26d06cbeb0db6adc8e363285b79c28ecf22e0c9d691a9ed9d3f8e1ef2"),
            ("look/surface-back.png", "d62d18d863f007df97a207209e498a3bc59d22b82e0104689dc2a4c86b0fc328"),
            ("look/surface.png", "4300d424ad86e3510259413262611e33fb35c90b0b1d73ce321c898bcbafa4be"),
        ],
    );
}

/// Castle Strike Reloaded's bake as it was at issue #70, before issue
/// #74 converts it to the placement format (and is proven against this).
#[test]
fn castle_strike_bakes_as_before() {
    assert_bakes_as(
        CASTLE,
        &[
            ("material.png", "bd301fa29701decd0d66b385e8a96a33620eee72f59e7f85c842cf7b7aa9be02"),
            ("front.png", "90bbbbb3f65586124b346483221f3f48c1ec605bfe0a7b210391134a93aea609"),
            ("back.png", "0e609fd6eb6658d96fcb8bc4461b0488ae78accf7fc4d108cb67c0d8775459e3"),
            ("look/glow-back.png", "5cf13fc21b32cdf2e3b8f3a47afc729aab90c958a0d57b396d3e4e92e8642f51"),
            ("look/glow.png", "e99ae6477f420441760c4ada103990d141649abd481b4d3bf8a1ebc78879fb9a"),
            ("look/groups-back.png", "bc98848ede8fd115dcc26e9ed35368aedcca514ce8bee455c2bb49a8ec10cd8a"),
            ("look/groups.png", "06725d389e25170570a4576a2de875cb80f5bf5af30153b30685a744a7d778ea"),
            ("look/surface-back.png", "fa8637083513270731b4ea8b6ddbe354c2b26827d70a04417fa52a99eedac8ce"),
            ("look/surface.png", "552b9ecd0b324b08e9bbeeedaaf7b8f8620ad62ddf6ade23d573fd590dddf761"),
        ],
    );
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
/// (invariants every tick); Liero Factory Reloaded (issue #96) with both
/// mods.
#[test]
fn the_reloaded_packs_play() {
    let packs = packs_dir();
    for (m, level) in [
        (CLASSIC, AFTER_DARK),
        (CLASSIC, CASTLE),
        (RELOADED_MOD, AFTER_DARK),
        (CLASSIC, FACTORY),
        (RELOADED_MOD, FACTORY),
    ] {
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
    for (m, level) in [(CLASSIC, AFTER_DARK), (CLASSIC, CASTLE), (RELOADED_MOD, CASTLE)] {
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

/// The baked level of a Reloaded pack, as the game reads it.
fn baked(level: &str) -> olr_format::levels::LevelSource {
    olr_format::levels::read_level_files(&DirFs::new(reloaded_dir().join(level))).unwrap()
}

/// Issue #97: Liero Factory Reloaded's live copies' boxes (x, y, width,
/// height), in [`LIVE_COPIES`]' order: since the plant works (2026-11-04)
/// every strip, the screen, the desk, the siren, the terminal (issue #100;
/// the console before it), the crane, the Dirt-ball line, the sign, the
/// generator, the gauge, the lab flask, three window panes, the lift, the
/// mine cart and the forklift. The ground leaves live copies out (#85), so
/// its bake differs from After Dark's only inside them (and where
/// [`works_ground`] says).
const LIVE_BOXES: [(u32, u32, u32, u32); 25] = [
    (778, 80, 29, 2),
    (1004, 80, 29, 2),
    (1119, 80, 29, 2),
    (984, 177, 29, 2),
    (1067, 177, 29, 2),
    (314, 286, 29, 2),
    (427, 286, 29, 2),
    (524, 286, 29, 2),
    (187, 342, 29, 2),
    (906, 83, 14, 14),
    (903, 119, 30, 11),
    (951, 45, 62, 28),
    (723, 351, 15, 14),
    (762, 269, 114, 50),
    (855, 269, 204, 88),
    (934, 299, 31, 21),
    (1046, 146, 72, 25),
    (1059, 144, 48, 27),
    (370, 205, 22, 17),
    (719, 151, 19, 22),
    (759, 151, 19, 22),
    (779, 151, 19, 22),
    (1186, 352, 12, 104),
    (12, 456, 187, 17),
    (536, 374, 52, 30),
];

/// Issue #98: the ceiling strips that fail like faulty tubes, in copy
/// order, running the Strip light's default script; the plant works
/// makes the other six live too, each running `steady.rhai` of its own.
const TUBES: [&str; 3] = ["strip-2", "strip-4", "strip-7"];

/// Liero Factory Reloaded's live copies in copy order.
const LIVE_COPIES: [&str; 25] = [
    "strip-1",
    "strip-2",
    "strip-3",
    "strip-4",
    "strip-5",
    "strip-6",
    "strip-7",
    "strip-8",
    "strip-9",
    "screen",
    "desk",
    "siren",
    "terminal",
    "crane",
    "line",
    "sign",
    "generator",
    "gauge",
    "lab-flask",
    "pane-1",
    "pane-3",
    "pane-4",
    "lift",
    "cart",
    "forklift",
];

/// Issue #99: the copies whose displays change, with their assets' names,
/// starting states and default scripts (`tools/levels/factory/`).
const DISPLAYS: [(&str, &str, &str, &str); 4] = [
    ("screen", "Computer screen", "Truck", "screen.rhai"),
    ("sign", "Lit sign", "Normal", "sign.rhai"),
    ("desk", "Control desk", "A", "desk.rhai"),
    ("gauge", "Gauge", "A", "gauge.rhai"),
];

/// The plant works' (2026-11-04) live copies: id, asset name, starting
/// state and default script (`tools/levels/factory/`, none for an asset
/// without one).
const WORKS: [(&str, &str, &str, Option<&str>); 10] = [
    ("siren", "Siren", "Normal", Some("siren.rhai")),
    ("line", "Dirt-ball line", "Stopped", Some("line.rhai")),
    ("generator", "Generator", "Running", Some("generator.rhai")),
    ("lab-flask", "Lab flask", "Normal", None),
    ("pane-1", "Window pane", "Intact", None),
    ("pane-3", "Window pane", "Intact", None),
    ("pane-4", "Window pane", "Intact", None),
    ("lift", "Lift", "Idle", Some("lift.rhai")),
    ("cart", "Mine cart", "Stopped", Some("cart.rhai")),
    ("forklift", "Forklift", "Idle", Some("forklift.rhai")),
];

/// Whether world pixel `(x, y)` lies in one of [`LIVE_BOXES`].
fn in_live_box(x: u32, y: u32) -> bool {
    LIVE_BOXES.iter().any(|&(bx, by, w, h)| (bx..bx + w).contains(&x) && (by..by + h).contains(&y))
}

/// A box of world pixels: x, y, width, height.
type Box4 = (u32, u32, u32, u32);

/// Whether `(x, y)` lies in box `(bx, by, w, h)`.
fn in_box((bx, by, w, h): Box4, x: u32, y: u32) -> bool {
    (bx..bx + w).contains(&x) && (by..by + h).contains(&y)
}

/// The night shift's mine lamps' bulbs' top-left pixels
/// (`tools/levels/factory_assets.py`'s `MINE_LAMPS`); a lamp's art is the
/// 4 x 3 box from one pixel up and left of it.
const MINE_LAMPS: [(u32, u32); 9] = [
    (70, 457),
    (205, 443),
    (340, 434),
    (475, 436),
    (610, 433),
    (745, 420),
    (880, 421),
    (1015, 403),
    (1150, 441),
];

/// The night shift's new art (x, y, width, height): the mine lamps, the
/// two high-bays and the aviation light. The bake differs from After
/// Dark's art only there, and its material nowhere.
fn night_art(x: u32, y: u32) -> bool {
    MINE_LAMPS.iter().any(|&(lx, ly)| in_box((lx - 1, ly - 1, 4, 3), x, y))
        || [(895, 267, 24, 2), (1010, 267, 24, 2), (1082, 36, 2, 2)].into_iter().any(|b| in_box(b, x, y))
}

/// The plant works' (2026-11-04) ground outside the live copies
/// (`tools/levels/factory_works.py`): the truck's hazard lights, the
/// lift's landing and the mine cart's rails (rock, so the material differs
/// there too).
const WORKS_ART: [Box4; 4] = [(43, 405, 1, 2), (175, 404, 2, 2), (1186, 456, 12, 3), (10, 473, 181, 1)];
/// Where the plant works' ground is rock After Dark's is not: the landing,
/// the rails and (2026-11-05) the Dirt-ball line's end crate, After Dark's
/// dirt (`factory_works.py`'s `CRATE`).
const WORKS_ROCK: [Box4; 3] = [(1186, 456, 12, 3), (10, 473, 181, 1), (1053, 330, 60, 35)];
/// The production line design (2026-11-06): the dip in the mine's floor
/// under the crane's shaft filled with dirt level with the pile
/// (`factory_works.py`'s `MINE_FILL`), the mound on it (`MINE_MOUND`) and
/// the reservoir made a hopper (`RESERVOIR_BOX`), their art and material
/// new.
const LINE_GROUND: [Box4; 3] = [(773, 436, 46, 30), (745, 400, 101, 36), (826, 300, 84, 66)];
/// The truck's cab window, glowing dimly in `cab`.
const CAB_BOX: Box4 = (50, 378, 23, 16);

fn works_art(x: u32, y: u32) -> bool {
    WORKS_ART.iter().chain(&LINE_GROUND).any(|&b| in_box(b, x, y))
}

/// The red duct's elbow, whose red rock glows in the night shift's `flue`.
const FLUE_BOX: Box4 = (1080, 250, 120, 42);

/// The night shift's lamps of a new colour, keeping their strength and
/// their group's other settings: the lab floor's strips in the ground (the
/// strips are all live copies since the plant works).
const RECOLOURED: [(Box4, &str); 1] = [((318, 183, 248, 1), "#dce8ff")];

/// Issue #96: Liero Factory Reloaded, built from assets, bakes as Liero
/// Factory After Dark; since issue #97 outside its live copies' boxes
/// ([`LIVE_BOXES`]), which the ground leaves out. `material.png`,
/// `front.png` and `back.png` pixel for pixel there and the distant sky
/// byte for byte; per pixel there, the glow's strength, its group's
/// settings (colour, intensity, reach, animation: groups are internal to
/// assets, so the bake may number or name them otherwise) and the surface
/// materials (by their settings, not their numbers); the beams'
/// direction and spread and the glass everywhere; and the look but for
/// its groups and materials, which hold the same settings. The night
/// shift (2026-11-03) is named exactly: its new art ([`night_art`]: art,
/// glow and surface its own), the flue's red rock in [`FLUE_BOX`]
/// glowing in `flue`, the [`RECOLOURED`] lamps' groups of a new colour,
/// its groups among the look's, and the skyline's two distant layers;
/// `material.png` stays After Dark's everywhere outside the live boxes.
#[test]
fn the_new_factory_bakes_as_after_dark() {
    use olr_format::look::GlowGroupSource;
    use olr_format::materials::MaterialSource;
    let (ours, theirs) = (reloaded_dir().join(FACTORY), reloaded_dir().join(AFTER_DARK));
    let same = std::fs::read(ours.join("distant/0.png")).unwrap()
        == std::fs::read(theirs.join("distant/0.png")).unwrap();
    assert!(same, "distant/0.png differs from After Dark's");
    // The loose dirt design: the press heap and the mine's floor seam are
    // spoil (`factory_works.py`), loose but drawn and solid as before.
    let loose = |dir: &Path| {
        String::from_utf8(cli(&["level", "loose", "list", dir.to_str().unwrap()]).stdout).unwrap()
    };
    assert_eq!(loose(&ours), "1: spoil \"Mine spoil\" repose 35° gravity 300 top-speed 80 slide-speed 25\n");
    assert_eq!(loose(&theirs), "");
    for file in ["material.png", "front.png", "back.png"] {
        let decode =
            |dir: &Path| olr_pack::image_io::decode_rgba(&std::fs::read(dir.join(file)).unwrap()).unwrap();
        let (a, b) = (decode(&ours), decode(&theirs));
        assert_eq!((a.width(), a.height()), (1200, 480), "{file}: at art scale 1");
        assert_eq!((b.width(), b.height()), (1200, 480), "{file}");
        // Two transparent pixels are alike, whatever colour the bake left
        // in them (the night shift's new pixels move it).
        let alike = |x, y| {
            let (p, q) = (a.get(x, y), b.get(x, y));
            p == q || (p.a == 0 && q.a == 0)
        };
        let (mut inside, mut night) = (0, 0);
        for y in 0..a.height() {
            for x in 0..a.width() {
                if in_live_box(x, y) {
                    inside += usize::from(a.get(x, y) != b.get(x, y));
                } else if file != "material.png" && (night_art(x, y) || works_art(x, y)) {
                    night += usize::from(!alike(x, y));
                } else if file == "material.png"
                    && WORKS_ROCK.iter().chain(&LINE_GROUND).any(|&r| in_box(r, x, y))
                {
                    continue;
                } else {
                    assert!(alike(x, y), "{file} at ({x}, {y}) differs from After Dark's");
                }
            }
        }
        if file == "front.png" {
            assert!(inside > 0, "the live copies are left out of the ground's front");
            assert!(night > 0, "the night shift's lamps are in the ground's front");
        }
    }
    let (a, b) = (baked(FACTORY), baked(AFTER_DARK));
    assert_eq!((a.size, a.art_scale, &a.theme, &a.objects), (b.size, b.art_scale, &b.theme, &b.objects));
    let (la, lb) = (a.look.as_ref().unwrap(), b.look.as_ref().unwrap());
    // A group or a material by its settings, its id left out.
    let group = |l: &olr_format::look::LookSource, i: u8| {
        (i > 0).then(|| GlowGroupSource { id: String::new(), ..l.glow_groups[usize::from(i) - 1].clone() })
    };
    let material = |l: &olr_format::look::LookSource, i: u8| {
        (i > 0).then(|| MaterialSource { id: String::new(), ..l.materials[usize::from(i) - 1].clone() })
    };
    let bare = |l: &olr_format::look::LookSource| {
        let mut l = l.clone();
        l.glow_groups.clear();
        l.materials.clear();
        l
    };
    assert!(bare(la) == bare(lb), "the look differs");
    let settings = |l: &olr_format::look::LookSource| {
        let mut g: Vec<_> = (1..=l.glow_groups.len() as u8).map(|i| format!("{:?}", group(l, i))).collect();
        let mut m: Vec<_> = (1..=l.materials.len() as u8).map(|i| format!("{:?}", material(l, i))).collect();
        g.sort();
        m.sort();
        (g, m)
    };
    // The night shift's groups (`factory_assets.py`'s `GROUPS`, the
    // skyline's) and the recoloured strips' (gathered as groups of their
    // own) join After Dark's.
    let night = |id: &str, colour: &str, intensity: u16, reach: u32, animation: &str| {
        let ron = format!(
            r#"(id: "{id}", colour: {colour}, intensity: {intensity}, reach: Pixels({reach}){animation})"#
        );
        let g: GlowGroupSource = ron::from_str(&ron).unwrap();
        format!("{:?}", Some(GlowGroupSource { id: String::new(), ..g }))
    };
    let mut want = settings(lb);
    want.0.extend([
        night("mine", r##"Tint("#ffb060")"##, 1000, 110, ", animation: Flicker(speed: 3, depth: 10)"),
        night("cool", r##"Tint("#dce8ff")"##, 500, 90, ""),
        night("sodium", r##"Tint("#ffb050")"##, 520, 95, ""),
        night(
            "flue",
            r##"Tint("#ff5a20")"##,
            320,
            70,
            ", animation: Pulse(period_ms: 4200, low: 35, high: 100)",
        ),
        night(
            "aviation",
            r##"Tint("#ff2a14")"##,
            400,
            22,
            ", animation: Pulse(period_ms: 1500, low: 0, high: 100, shape: Square)",
        ),
        night("city", "Own", 120, 8, ", animation: Flicker(speed: 1, depth: 25)"),
        night(
            "pylon",
            r##"Tint("#ff3020")"##,
            200,
            10,
            ", animation: Pulse(period_ms: 2000, low: 0, high: 100, shape: Square)",
        ),
        // The plant works' (`factory_works.py`'s `HAZARD_GROUP`,
        // `CAB_GROUP`); the strips are all live copies now, so no static
        // strip brings a group of a new colour.
        night(
            "hazard",
            r##"Tint("#ffa020")"##,
            300,
            24,
            ", animation: Pulse(period_ms: 1000, low: 0, high: 100, shape: Square)",
        ),
        night("cab", r##"Tint("#ffd8a0")"##, 140, 22, ""),
    ]);
    want.0.sort();
    assert_eq!(settings(la), want, "the groups' and materials' settings");
    // The skyline: the far city at 75 and the near industry at 45, after
    // the sky, each glowing whole in its group through a painted mask.
    assert_eq!(a.distant.len(), 3, "the sky and the skyline's two layers");
    for (d, depth, id) in [(&a.distant[1], 75, "city"), (&a.distant[2], 45, "pylon")] {
        assert_eq!((d.at, d.depth), ((0, 0), depth), "{id}");
        let g = d.glow.as_ref().unwrap();
        assert!(g.strength == 100 && g.mask.is_some(), "{id}: its glow");
        assert_eq!(la.glow_groups[usize::from(g.group) - 1].id, id);
    }
    // The distant sky's glow.
    let (da, db) = (&a.distant[0], &b.distant[0]);
    assert_eq!((da.at, da.depth), (db.at, db.depth));
    let (ga, gb) = (da.glow.as_ref().unwrap(), db.glow.as_ref().unwrap());
    assert_eq!((ga.strength, &ga.mask), (gb.strength, &gb.mask));
    assert_eq!(group(la, ga.group), group(lb, gb.group), "the sky's group");
    assert_eq!(
        a.distant_surfaces.iter().map(|s| s.is_some()).collect::<Vec<_>>(),
        b.distant_surfaces.iter().map(|s| s.is_some()).collect::<Vec<_>>()
    );
    // The glow maps, per pixel.
    let (wa, wb) = (a.glow.as_ref().unwrap(), b.glow.as_ref().unwrap());
    assert_eq!((wa.art, wa.world), (wb.art, wb.world));
    assert_eq!(wa.art, wa.world, "art scale 1");
    let (w, h) = a.size;
    let (mut groups, mut flue, mut skyline) = (BTreeSet::new(), 0, 0);
    let see_through =
        olr_pack::image_io::decode_rgba(&std::fs::read(ours.join("back.png")).unwrap()).unwrap();
    for i in 0..(w * h) as usize {
        let (x, y) = (i as u32 % w, i as u32 / w);
        if in_live_box(x, y) {
            continue;
        }
        if night_art(x, y) || works_art(x, y) || in_box(CAB_BOX, x, y) {
            continue;
        }
        // Issue #62: where the back is see-through, the nearest skyline
        // layer opaque at its placement bakes its light (its mask, at 100)
        // in its group over the sky's.
        let seen = (see_through.get(x, y).a == 0)
            .then_some(())
            .and_then(|()| a.distant[1..].iter().rev().find(|d| d.image.get(x, y).a != 0));
        if let Some(d) = seen {
            let g = d.glow.as_ref().unwrap();
            let want = g.mask.as_ref().unwrap()[i];
            for (sa, ga) in [(wa.front[i], wa.groups[i]), (wa.back[i], wa.groups_back[i])] {
                assert_eq!(sa, want, "({x}, {y}): the skyline's glow");
                assert!(want == 0 || ga == g.group, "({x}, {y}): the skyline's group");
            }
            skyline += 1;
            continue;
        }
        let recolour = RECOLOURED.iter().find(|(b, _)| in_box(*b, x, y)).map(|&(_, c)| c);
        for (sa, sb, ga, gb, side) in [
            (wa.front[i], wb.front[i], wa.groups[i], wb.groups[i], "front"),
            (wa.back[i], wb.back[i], wa.groups_back[i], wb.groups_back[i], "back"),
        ] {
            if in_box(FLUE_BOX, x, y) && sa != sb {
                assert_eq!(sb, 0, "({x}, {y}): the flue glows where After Dark did not");
                assert_eq!(la.glow_groups[usize::from(ga) - 1].id, "flue", "({x}, {y}): the flue's group");
                flue += 1;
                continue;
            }
            assert_eq!(sa, sb, "({x}, {y}): the {side} glow's strength");
            let theirs = match (recolour, group(lb, gb)) {
                (Some(c), Some(g)) => {
                    Some(GlowGroupSource { colour: ron::from_str(&format!("Tint(\"{c}\")")).unwrap(), ..g })
                }
                (_, g) => g,
            };
            assert_eq!(group(la, ga), theirs, "({x}, {y}): the {side} glow group");
            groups.insert(ga);
        }
    }
    assert_eq!(flue, 605, "the duct's red rock glows in front");
    assert!(skyline > 10_000, "the skyline stands before the sky: {skyline}");
    // Issue #99: the screen's `control`, the displays' and the screens'
    // groups glow only inside live copies now.
    assert!(groups.len() >= 7, "every group of the ground glows somewhere: {groups:?}");
    let (ba, bb) = (a.glow_beams.as_ref().unwrap(), b.glow_beams.as_ref().unwrap());
    assert!(ba.world == bb.world && ba.front == bb.front && ba.back == bb.back, "the beams");
    // The glass outside the live copies (the plant works' panes are).
    let (sa, sb) = (a.glass.as_ref().unwrap(), b.glass.as_ref().unwrap());
    assert_eq!(sa.art, sb.art, "the glass");
    let (mut panes, mut glass) = (0, 0);
    for (i, (ta, tb)) in sa.texels.chunks(4).zip(sb.texels.chunks(4)).enumerate() {
        let (x, y) = (i as u32 % sa.art.0, i as u32 / sa.art.0);
        if in_live_box(x, y) {
            panes += usize::from(ta != tb);
        } else {
            assert_eq!(ta, tb, "({x}, {y}): the glass");
            glass += usize::from(ta != [255, 255, 255, 0]);
        }
    }
    assert!(panes > 0 && glass > 0, "the panes are the copies', the rest the ground's: {panes}, {glass}");
    let (ma, mb) = (a.surface.as_ref().unwrap(), b.surface.as_ref().unwrap());
    assert_eq!(ma.world, mb.world);
    for i in 0..(w * h) as usize {
        let (x, y) = (i as u32 % w, i as u32 / w);
        if in_live_box(x, y) || night_art(x, y) || works_art(x, y) {
            continue;
        }
        assert_eq!(material(la, ma.front[i]), material(lb, mb.front[i]), "({x}, {y}): the front material");
        assert_eq!(material(la, ma.back[i]), material(lb, mb.back[i]), "({x}, {y}): the back material");
    }
    assert_eq!((a.clear.is_some(), a.haze.is_some(), a.live.is_some()), (false, false, true));
    assert_eq!((b.clear.is_some(), b.haze.is_some()), (false, false));
}

/// The night shift's strip colours (the Strip light's exposed `colour`),
/// as `factory_live.py`'s `STRIP_COLOURS` sets them.
const RECOLOURED_STRIPS: [(&[&str], &str); 2] = [
    (&["strip-1", "strip-2", "strip-3"], "#dce8ff"),
    (&["strip-6", "strip-7", "strip-8", "strip-9"], "#ffb050"),
];

/// A copy as `the_new_factory_is_made_of_assets` reads it: its id, its
/// asset's key, its built-in folder (`None` the level's own), its place
/// and its size.
type CopyRow<'a> = (String, &'a str, Option<&'a str>, (i32, i32), (u32, u32));

/// Issue #96: Liero Factory Reloaded is made of assets: its copies by id,
/// each with its key, asset, origin, place and size, every one static
/// but the crane and the terminal (issues #97 and #100) and the three faulty tubes
/// (issue #98), in their starting states with their defaults and their
/// asset's default script;
/// the ground and the sky the level's own (the sky distant at 90), the
/// rest built-in and as the library has them (`level library` says
/// `same`: a built-in asset changed without the level fails here).
#[test]
fn the_new_factory_is_made_of_assets() {
    use olr_format::levels::{AssetOrigin, read_placed_files};
    let dir = reloaded_dir().join(FACTORY);
    let placed = read_placed_files(&DirFs::new(&dir)).unwrap().expect("editor/placed.ron");
    let strips = [
        (778, 80),
        (1004, 80),
        (1119, 80),
        (984, 177),
        (1067, 177),
        (314, 286),
        (427, 286),
        (524, 286),
        (187, 342),
    ];
    let mut want: Vec<CopyRow> = vec![
        ("ground".into(), "ground", None, (0, 0), (1200, 480)),
        ("sky".into(), "y", None, (0, 0), (1200, 480)),
        ("city".into(), "n", None, (0, 0), (1200, 480)),
        ("industry".into(), "p", None, (0, 0), (1200, 480)),
    ];
    for (i, at) in strips.into_iter().enumerate() {
        want.push((format!("strip-{}", i + 1), "s", Some("strip-light"), at, (29, 2)));
    }
    for (id, key, folder, at, size) in [
        ("ceil-lamp", "l", "ceiling-lamp", (319, 137), (13, 6)),
        ("screen", "c", "computer-screen", (906, 83), (14, 14)),
        ("desk", "d", "control-desk", (903, 119), (30, 11)),
        ("siren", "w", "siren", (951, 45), (62, 28)),
        ("terminal", "e", "terminal", (723, 351), (15, 14)),
        ("crane", "k", "crane", (762, 269), (114, 200)),
        ("line", "o", "ball-line", (855, 269), (204, 88)),
        ("sign", "t", "lit-sign", (934, 299), (31, 21)),
        ("generator", "r", "generator", (1046, 146), (72, 25)),
        ("gauge", "g", "gauge", (1059, 144), (48, 27)),
        ("lab-flask", "b", "lab-flask", (370, 205), (22, 17)),
        ("pane-1", "a", "window-pane", (719, 151), (19, 22)),
        ("pane-3", "a", "window-pane", (759, 151), (19, 22)),
        ("pane-4", "a", "window-pane", (779, 151), (19, 22)),
        ("lift", "f", "lift", (1186, 352), (12, 104)),
        ("cart", "m", "mine-cart", (12, 456), (187, 17)),
        ("forklift", "x", "forklift", (536, 374), (52, 30)),
    ] {
        want.push((id.into(), key, Some(folder), at, size));
    }
    let file = &placed.file;
    let got: Vec<_> = file
        .copies
        .iter()
        .map(|c| {
            let asset = file.assets.iter().find(|a| a.key == c.asset).unwrap();
            let origin = match &asset.origin {
                AssetOrigin::Own => None,
                AssetOrigin::Builtin(f) => Some(f.as_str()),
                o => panic!("{}: {o:?}", c.id),
            };
            let size = olr_format::levels::placed_size(c, &placed.assets[&c.asset].file);
            (c.id.clone(), c.asset.as_str(), origin, c.at, size)
        })
        .collect();
    assert_eq!(got, want);
    for c in &file.copies {
        assert_eq!(c.live, LIVE_COPIES.contains(&c.id.as_str()), "{}: live", c.id);
        // The plant works: the strips that are no faulty tubes start
        // Steady (`On` without the buzz).
        let steady = c.id.starts_with("strip-") && !TUBES.contains(&c.id.as_str());
        assert_eq!(c.state.as_deref(), steady.then_some("Steady"), "{}: its start", c.id);
        // The night shift's strip colours by area (`factory_live.py`'s
        // `STRIP_COLOURS`); every other value its default.
        let colour = RECOLOURED_STRIPS.iter().find(|(ids, _)| ids.contains(&c.id.as_str())).map(|&(_, v)| v);
        let values: Vec<String> = c.values.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
        let want: Vec<String> = colour
            .map(|v| format!("colour={:?}", olr_edit::asset::parse_value(v).unwrap()))
            .into_iter()
            .collect();
        assert_eq!(values, want, "{}: its values", c.id);
        // And they run `steady.rhai` of their own instead of their asset's.
        assert_eq!((c.script, c.default_script), (steady, !steady), "{}: its scripts", c.id);
        assert!(!c.free_aspect, "{}: its aspect kept", c.id);
        let distant = [("sky", 90), ("city", 75), ("industry", 45)].into_iter().find(|&(id, _)| id == c.id);
        assert_eq!(c.distant, distant.map(|(_, d)| d), "{}", c.id);
    }
    assert!(placed.level_script.is_some(), "the level script");
    // The plant works: each steady strip's own script.
    let steady = std::fs::read_to_string(repo().join("tools/levels/factory/steady.rhai")).unwrap();
    let own: Vec<&str> = placed.scripts.keys().map(String::as_str).collect();
    assert_eq!(
        own,
        ["strip-1", "strip-3", "strip-5", "strip-6", "strip-8", "strip-9"],
        "the copies' own scripts"
    );
    assert!(placed.scripts.values().all(|t| *t == steady), "each `steady.rhai`");
    let o = cli(&["level", "library", dir.to_str().unwrap(), "--content", packs_dir().to_str().unwrap()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    let lines: Vec<&str> = out.lines().collect();
    let builtin: BTreeSet<&str> = want.iter().filter_map(|w| w.2.map(|f| (w.1, f))).map(|(k, _)| k).collect();
    assert_eq!(lines.len(), builtin.len(), "{out}");
    for (key, folder) in want.iter().filter_map(|w| Some((w.1, w.2?))).collect::<BTreeSet<_>>() {
        assert!(lines.contains(&format!("{key}: same (built-in {folder})").as_str()), "{key}: {out}");
    }
}

/// The night shift (2026-11-03): every mine lamp sits on the tunnels'
/// back wall in open air, each of its pixels at least 3 px (in x and in
/// y) from any rock or dirt pixel, so none hangs from dirt or a ceiling.
#[test]
fn the_mine_lamps_keep_clear_of_rock_and_dirt() {
    let dir = reloaded_dir().join(FACTORY);
    let decode = |f: &str| olr_pack::image_io::decode_rgba(&std::fs::read(dir.join(f)).unwrap()).unwrap();
    let (lamps, material) = (decode("editor/layers/mine-lamps.png"), decode("material.png"));
    let open = |x: i64, y: i64| {
        let inside = (0..1200).contains(&x) && (0..480).contains(&y);
        inside && material.get(x as u32, y as u32).r == 0
    };
    let mut pixels = 0;
    for y in 0..lamps.height() {
        for x in 0..lamps.width() {
            if lamps.get(x, y).a == 0 {
                continue;
            }
            pixels += 1;
            assert!(MINE_LAMPS.iter().any(|&(lx, ly)| in_box((lx - 1, ly - 1, 4, 3), x, y)), "({x}, {y})");
            let (x, y) = (i64::from(x), i64::from(y));
            for (dx, dy) in (-3..=3).flat_map(|dx| (-3..=3).map(move |dy| (dx, dy))) {
                assert!(open(x + dx, y + dy), "the lamp pixel ({x}, {y}) is within 3 px of rock or dirt");
            }
        }
    }
    assert_eq!(pixels, MINE_LAMPS.len() * 12, "9 lamps of a 2 x 2 bulb and a plate of 8");
}

/// The factory's live copy `id` as the world has it now.
pub(crate) fn live_copy<'a>(w: &'a olr_sim::World, id: &str) -> olr_sim::LiveCopyView<'a> {
    w.live().expect("the factory's live copies").copy_by_id(id).unwrap_or_else(|| panic!("no live copy {id}"))
}

/// The crane's `claw_x` in whole world pixels (rounded down).
fn claw_x(w: &olr_sim::World) -> i64 {
    live_copy(w, "crane").value("claw_x").unwrap().div_euclid(olr_sim::LIVE_SUBPIXELS)
}

/// A world of the factory (Classic) whose worm 0, a human pressing what a
/// test gives it, first spawns at `at`, and a second worm wherever
/// `FindSpot` puts it (seed 3: the far left of the upper floor).
pub(crate) fn factory_world(content: &Content, at: (i32, i32)) -> olr_sim::World {
    use olr_cli::assets::LevelAssets;
    use olr_cli::spec::MatchSpec;
    let assets = LevelAssets::load(content, CLASSIC, FACTORY).unwrap();
    let spec = MatchSpec { locals: 1, ..MatchSpec::new(CLASSIC, FACTORY, 1, 3, 1000) };
    olr_content::build_match_spawning(assets.match_assets(), &spec.match_options(), Some(at)).unwrap().world
}

/// Issue #97: Liero Factory Reloaded's live copies are exactly the crane
/// and the terminal (issue #100; the console before it), (issue #98) the
/// tubes `strip-2`, `strip-4` and `strip-7`, the other strips static, and (issue #99) the screen, the
/// sign, the desk and the gauge ([`DISPLAYS`]), each a copy of its
/// built-in asset in its starting state with its asset's default script
/// (the Crane's `tools/levels/factory/crane.rhai`, the Strip light's
/// `tube.rhai`, the displays' their own; the Terminal has none), every
/// other copy static, the level script
/// (`tools/levels/factory/level.rhai`) names the crane and the terminal,
/// and no live asset holds a dirt layer (call 2). A match loads them so.
#[test]
fn the_live_factory_has_its_live_copies() {
    use olr_format::levels::{ElementKind, ElementMaterial, read_placed_files};
    let dir = reloaded_dir().join(FACTORY);
    let placed = read_placed_files(&DirFs::new(&dir)).unwrap().expect("editor/placed.ron");
    let live: Vec<&str> = placed.file.copies.iter().filter(|c| c.live).map(|c| c.id.as_str()).collect();
    assert_eq!(live, LIVE_COPIES);
    // The plant works: every strip is live, the faulty tubes running the
    // Strip light's script, the others `steady.rhai` of their own.
    for c in placed.file.copies.iter().filter(|c| c.id.starts_with("strip-")) {
        assert!(c.live, "{}", c.id);
        assert_eq!(c.default_script, TUBES.contains(&c.id.as_str()), "{}: its asset's script", c.id);
    }
    let crane_script = std::fs::read_to_string(repo().join("tools/levels/factory/crane.rhai")).unwrap();
    let tube_script = std::fs::read_to_string(repo().join("tools/levels/factory/tube.rhai")).unwrap();
    let mut want =
        vec![("crane", "Crane", "Idle", Some(&crane_script)), ("terminal", "Terminal", "Normal", None)];
    want.extend(TUBES.map(|t| (t, "Strip light", "On", Some(&tube_script))));
    let display_scripts: Vec<String> = DISPLAYS
        .iter()
        .map(|d| std::fs::read_to_string(repo().join("tools/levels/factory").join(d.3)).unwrap())
        .collect();
    for (d, script) in DISPLAYS.iter().zip(&display_scripts) {
        want.push((d.0, d.1, d.2, Some(script)));
    }
    // The plant works' copies, with their default scripts' files.
    let works: Vec<(&str, &str, &str, Option<String>)> = WORKS
        .iter()
        .map(|&(id, name, start, file)| {
            let script =
                file.map(|f| std::fs::read_to_string(repo().join("tools/levels/factory").join(f)).unwrap());
            (id, name, start, script)
        })
        .collect();
    for (id, name, start, script) in &works {
        want.push((*id, *name, *start, script.as_ref()));
    }
    for (id, name, start, script) in want {
        let c = placed.file.copies.iter().find(|c| c.id == id).unwrap();
        let a = &placed.assets[&c.asset];
        assert_eq!(a.file.name, name, "{id}");
        assert!(c.state.is_none(), "{id}: its asset's starting state");
        assert_eq!(a.file.states.first().map(|s| s.name.as_str()), Some(start), "{id}");
        assert!(a.file.start.is_none() || a.file.start.as_deref() == Some(start), "{id}");
        assert!(c.default_script && !c.script, "{id}: its asset's default script, none of its own");
        assert_eq!(a.script.as_ref(), script, "{id}: the default script");
        for e in &a.file.elements {
            let dirt = match e.kind {
                ElementKind::Drawn { material, mixed, .. } => material == ElementMaterial::Dirt || mixed,
                ElementKind::Image { material, .. } => material == ElementMaterial::Dirt,
                ElementKind::Raster => true,
            };
            assert!(!dirt, "{id}: layer {} may hold dirt", e.id);
        }
    }
    let level = placed.level_script.as_deref().expect("the level script");
    assert_eq!(level, std::fs::read_to_string(repo().join("tools/levels/factory/level.rhai")).unwrap());
    assert!(level.contains("this.copy(\"crane\")") && level.contains("this.copy(\"terminal\")"));
    // As a match loads them.
    use olr_cli::runner::{Match, Options};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let m =
        Match::new(&content, &olr_cli::spec::MatchSpec::new(CLASSIC, FACTORY, 2, 1, 10), &Options::default())
            .unwrap();
    let w = m.world();
    let ids: Vec<&str> = w.live().unwrap().copies().map(|c| c.id()).collect();
    assert_eq!(ids, LIVE_COPIES);
    for t in TUBES {
        assert_eq!(live_copy(w, t).state_name(), "On", "{t}");
    }
    assert_eq!(
        (live_copy(w, "crane").state_name(), live_copy(w, "terminal").state_name()),
        ("Idle", "Normal")
    );
    for d in DISPLAYS {
        assert_eq!(live_copy(w, d.0).state_name(), d.2, "{}", d.0);
    }
    for &(id, _, start, _) in &WORKS {
        assert_eq!(live_copy(w, id).state_name(), start, "{id}");
    }
    assert_eq!(claw_x(w), 0, "the claw starts where After Dark has it");
    let crane = live_copy(w, "crane").setup();
    let p = &crane.props[usize::from(crane.prop("claw_x").unwrap())];
    assert_eq!((p.min, p.max, p.default), (-10, 75, 0), "claw_x's limits and default (whole pixels)");
}

/// Issue #97: in a 2800-tick match of two idle bots spawned away from the
/// crane, its claw (the production line design: its round) travels over
/// the mine's hole (9) in 36 ticks (±2), then, its bite taken, to the
/// reservoir (75) in 264, and back to the hole in 264: 9, 75, 9. The
/// crane is `Moving` while `claw_x` changes and rests `Idle` over the
/// reservoir, and the terminal (issue #100; the console before it) is in
/// `Alarm` while the crane moves, `Normal` otherwise, at most 2 ticks
/// behind it (ruling R8: the level script reads the crane's state change
/// the tick after).
#[test]
fn the_crane_crosses_and_the_terminal_alarms() {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::{MatchSpec, Personality};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = MatchSpec { personality: Personality::Idle, ..MatchSpec::new(CLASSIC, FACTORY, 2, 1, 2800) };
    let mut m = Match::new(&content, &spec, &Options::default()).unwrap();
    // Per tick (after it): claw_x in 1/256 px, crane moving, terminal alarmed.
    let mut rows = vec![(live_copy(m.world(), "crane").value("claw_x").unwrap(), false, false)];
    let mut spawned = false;
    loop {
        let (before, go) = (m.ticks(), m.step());
        if m.ticks() == before {
            break;
        }
        let w = m.world();
        if !spawned && w.worms().iter().all(|wm| wm.alive) {
            spawned = true;
            for wm in w.worms() {
                let (x, y) = (wm.pos.x, wm.pos.y);
                let deck = (700.0..1000.0).contains(&x) && (230.0..380.0).contains(&y);
                let shaft = (760.0..830.0).contains(&x) && (380.0..480.0).contains(&y);
                assert!(!deck && !shaft, "a bot by the crane: {x}, {y}");
            }
        }
        rows.push((
            live_copy(w, "crane").value("claw_x").unwrap(),
            live_copy(w, "crane").state_name() == "Moving",
            live_copy(w, "terminal").state_name() == "Alarm",
        ));
        if !go {
            break;
        }
    }
    assert!(m.finish().failure().is_none());
    assert!(spawned);
    assert_eq!(rows.len(), 2801);
    // The moves: maximal runs of ticks whose claw_x differs from the tick before.
    let mut moves: Vec<(usize, usize)> = Vec::new();
    for t in 1..rows.len() {
        if rows[t].0 != rows[t - 1].0 {
            match moves.last_mut() {
                Some((_, end)) if *end == t - 1 => *end = t,
                _ => moves.push((t, t)),
            }
        }
    }
    let px = |t: usize| rows[t].0.div_euclid(olr_sim::LIVE_SUBPIXELS);
    let ends: Vec<i64> = moves.iter().map(|&(_, e)| px(e)).collect();
    assert!(ends.len() >= 3 && ends[..3] == [9, 75, 9], "the claw's ends: {ends:?}, moves {moves:?}");
    let near = |got: usize, want: usize| got.abs_diff(want) <= 2;
    for (i, &(s, e)) in moves.iter().enumerate().take(3) {
        let (len, want) = (e - s + 1, if i == 0 { 36 } else { 264 });
        assert!(near(len, want), "move {i} took {len} ticks, not {want}: {moves:?}");
    }
    // Moving while it travels, and resting Idle over the reservoir.
    let moving_at = |t: usize| moves.iter().any(|&(s, e)| (s..=e).contains(&t));
    let by_an_end = |t: usize| moves.iter().any(|&(s, e)| s.abs_diff(t) <= 2 || e.abs_diff(t) <= 2);
    for (t, row) in rows.iter().enumerate().skip(1) {
        if moving_at(t) && !by_an_end(t) {
            assert!(row.1, "tick {t}: the crane Moving");
        }
    }
    assert!((moves[1].1..moves[2].0).any(|t| !rows[t].1), "it rests Idle over the reservoir");
    // The terminal follows the crane, at most 2 ticks behind.
    for t in 2..rows.len() {
        let crane = [rows[t - 2].1, rows[t - 1].1, rows[t].1];
        if crane.iter().all(|m| *m) || crane.iter().all(|m| !*m) {
            assert_eq!(rows[t].2, rows[t].1, "tick {t}: the terminal's Alarm");
        }
    }
    assert!(rows.iter().any(|r| r.2) && rows.iter().any(|r| !r.2));
}

/// The rock pixels each state of a built-in asset shows, from its files:
/// every rock layer's alpha, its state pictures stacked under its own
/// (#71: a layer's PNG holds its own picture, then one per state owning
/// it, in state order), by state.
fn rock_by_state(folder: &str) -> Vec<(String, BTreeSet<(u32, u32)>)> {
    use olr_format::levels::{ElementKind, ElementMaterial};
    let src = olr_format::assets::read_asset_files(&DirFs::new(repo().join("content/assets").join(folder)))
        .unwrap();
    let file = &src.file;
    let mut out: Vec<(String, BTreeSet<(u32, u32)>)> =
        file.states.iter().map(|s| (s.name.clone(), BTreeSet::new())).collect();
    for e in &file.elements {
        let ElementKind::Drawn { material: ElementMaterial::Rock, at, .. } = e.kind else { continue };
        let img = olr_pack::image_io::decode_rgba(&src.files[&format!("layers/{}.png", e.id)]).unwrap();
        let owners: Vec<usize> =
            (0..file.states.len()).filter(|&i| file.states[i].own.contains(&e.id)).collect();
        let h = img.height() / (1 + owners.len() as u32);
        assert_eq!(h * (1 + owners.len() as u32), img.height(), "{folder}/{}: stacked pictures", e.id);
        for (i, state) in out.iter_mut().enumerate() {
            let block = owners.iter().position(|&o| o == i).map_or(0, |k| k as u32 + 1);
            for y in 0..h {
                for x in 0..img.width() {
                    if img.get(x, block * h + y).a != 0 {
                        state.1.insert((x + at.0 as u32, y + at.1 as u32));
                    }
                }
            }
        }
    }
    out
}

/// Issue #97: no state switch adds or removes a solid pixel: for the
/// Crane, the Console, (issue #98) the Strip light, (issue #99) the
/// Computer screen, the Lit sign, the Control desk and the Gauge and
/// (issue #100) the Terminal and (the plant works) the Dirt-ball line,
/// the Lift, the Mine cart, the Forklift, the Generator, the Siren and the
/// Window pane, every state's rock pixels (the stacked pictures included)
/// are the starting state's (none for the Strip light, the screen, the
/// desk, the siren and the pane: their layers are Background), and so are
/// the solid pixels of each state of the copies as a match loads them.
#[test]
fn live_states_keep_their_solid_pixels() {
    let backgrounds = ["strip-light", "computer-screen", "control-desk", "siren", "window-pane"];
    for folder in [
        "crane",
        "console",
        "strip-light",
        "computer-screen",
        "lit-sign",
        "control-desk",
        "gauge",
        "terminal",
        // The plant works (2026-11-04).
        "ball-line",
        "lift",
        "mine-cart",
        "forklift",
        "generator",
        "siren",
        "window-pane",
    ] {
        let states = rock_by_state(folder);
        assert!(states.len() >= 2, "{folder}: its states");
        for (name, rock) in &states[1..] {
            assert!(*rock == states[0].1, "{folder}: state {name}'s rock differs from {}'s", states[0].0);
        }
        assert_eq!(states[0].1.is_empty(), backgrounds.contains(&folder), "{folder}: its rock");
    }
    // As played: each state's rock parts at their places (no value moving them).
    use olr_cli::runner::{Match, Options};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let m =
        Match::new(&content, &olr_cli::spec::MatchSpec::new(CLASSIC, FACTORY, 2, 1, 10), &Options::default())
            .unwrap();
    for c in m.world().live().unwrap().copies() {
        let setup = c.setup();
        let solid = |state: &olr_sim::LiveState| {
            let mut px = BTreeSet::new();
            for p in state.parts.iter().filter(|p| p.kind == olr_sim::LivePartKind::Rock) {
                let pic = &setup.pictures[usize::from(p.picture)];
                for y in 0..setup.size.1 {
                    for x in 0..setup.size.0 {
                        if pic.solid(x, y) {
                            px.insert((x as i32 + p.at.0, y as i32 + p.at.1));
                        }
                    }
                }
            }
            px
        };
        let start = solid(&setup.states[usize::from(setup.start_state)]);
        let unsolid = c.id().starts_with("strip-")
            || c.id().starts_with("pane-")
            || ["screen", "desk", "siren", "lab-flask"].contains(&c.id());
        assert_eq!(start.is_empty(), unsolid, "{}: its solid pixels", c.id());
        for s in &setup.states {
            assert!(solid(s) == start, "{}: state {}'s solid pixels", c.id(), s.name);
        }
    }
}

/// Issue #97: the crane carries and pushes worms by #90's rules. A worm
/// standing on the hub's left shoulder (its crossbar's top at y 301)
/// rides the claw's round (the production line design: over the hole,
/// down the shaft, up and over to the reservoir) and keeps its place on
/// it, unhurt; a worm hanging on its rope from the rail at the cable's
/// height is passed by (the cable is background), unhurt.
#[test]
fn the_crane_carries_a_worm_and_its_cable_passes_a_hanging_one() {
    use olr_sim::{EventKind, Keys, TickEvents};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let to_dump =
        |w: &mut olr_sim::World, ev: &mut TickEvents, crushed: &mut usize, k: &dyn Fn(u32) -> u16| {
            for t in 0..3000u32 {
                w.tick(&[Keys(k(t)), Keys(0)], ev);
                *crushed +=
                    ev.iter().filter(|e| matches!(e.kind, EventKind::Crushed { worm: 0, .. })).count();
                if claw_x(w) == 75 {
                    return t;
                }
            }
            panic!("the claw never reached the reservoir");
        };
    // Carried: on the left shoulder (x 774-780 at the start).
    let mut w = factory_world(&content, (778, 296));
    let mut ev = TickEvents::new();
    let mut crushed = 0;
    for _ in 0..40 {
        w.tick(&[Keys(0), Keys(0)], &mut ev);
    }
    let wm = &w.worms()[0];
    assert!(wm.alive && wm.on_ground && (wm.pos.y - 296.0).abs() < 1.0, "on the shoulder: {:?}", wm.pos);
    let x0 = wm.pos.x - claw_x(&w) as f32;
    to_dump(&mut w, &mut ev, &mut crushed, &|_| 0);
    let wm = &w.worms()[0];
    assert!((wm.pos.x - claw_x(&w) as f32 - x0).abs() < 1.5, "carried with the claw: {} at 75", wm.pos.x);
    assert!(
        wm.alive && wm.on_ground && wm.health == 100.0 && crushed == 0,
        "unhurt: {:?} {}",
        wm.pos,
        wm.health
    );
    // Passed by: from the dirt under the claw's travel (x 845), the worm
    // aims up, ropes onto the rail above and hangs at the cable's height
    // (between the trolley and the hub); the cable, background since the
    // production line design, goes by it.
    let mut w = factory_world(&content, (845, 300));
    let k = |t: u32| match t {
        50..110 => Keys::UP,
        110..112 => Keys::ROPE,
        _ => 0,
    };
    for t in 0..200u32 {
        w.tick(&[Keys(k(t)), Keys(0)], &mut ev);
        crushed += ev.iter().filter(|e| matches!(e.kind, EventKind::Crushed { worm: 0, .. })).count();
    }
    let hung = w.worms()[0].pos;
    to_dump(&mut w, &mut ev, &mut crushed, &|_| 0);
    let wm = &w.worms()[0];
    assert!(wm.rope.attached && (wm.rope.pos.y - 270.0).abs() < 2.0, "roped to the rail: {:?}", wm.rope.pos);
    assert!(
        (hung.x - 845.0).abs() < 1.0 && (285.0..299.0).contains(&hung.y),
        "at the cable's height: {hung:?}"
    );
    assert!(wm.alive && wm.health == 100.0 && crushed == 0, "unhurt: {:?} {}", wm.pos, wm.health);
}

/// Issue #100's playtest: a laser sight stopped by a live part is no
/// worm, so the crosshair stays as it is. A worm with a rifle on the
/// crane's left shoulder aims down into it: every trace stops in the
/// crane, and the scene's marks never call it a target.
#[test]
fn a_sight_on_the_crane_finds_no_worm() {
    use olr_cli::assets::LevelAssets;
    use olr_cli::spec::MatchSpec;
    use olr_content::{MatchLoadout, WeaponChoice};
    use olr_sim::{CosmeticKind, EventKind, Keys, TickEvents};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let assets = LevelAssets::load(&content, CLASSIC, FACTORY).unwrap();
    let spec = MatchSpec { locals: 1, ..MatchSpec::new(CLASSIC, FACTORY, 1, 3, 1000) };
    let mut options = spec.match_options();
    let rifle = MatchLoadout {
        name: "Rifle".into(),
        weapons: vec!["Rifle".into()],
        loading_time: 100,
        unique: false,
    };
    options.weapons = WeaponChoice::Loadout(rifle);
    let mut w =
        olr_content::build_match_spawning(assets.match_assets(), &options, Some((778, 296))).unwrap().world;
    let (mut ev, mut marks) = (TickEvents::new(), olr_scene::Marks::new(1));
    let mut in_crane = 0;
    for t in 0..200 {
        let k = if t < 100 { Keys::DOWN } else { 0 };
        w.tick(&[Keys(k), Keys(0)], &mut ev);
        marks.observe(&w, &ev);
        let solid = w.solid();
        for e in ev.iter() {
            if let EventKind::SpawnCosmetic { kind: CosmeticKind::LaserSight { worm: 0, len }, pos, vel } =
                e.kind
            {
                let end = (0..len / 3).fold(pos, |p, _| p + vel * 3.0);
                let far = w.worms()[1..].iter().all(|o| (o.pos - end).length() > 30.0);
                if t >= 100
                    && far
                    && solid.material(end.x as i64, end.y as i64)
                        != Some(olr_data::level::Material::Background)
                {
                    in_crane += 1;
                    assert!(!marks.target(0), "tick {t}: a sight stopped at {end:?} in the crane is no worm");
                }
            }
        }
    }
    assert!(in_crane > 0, "the sight stopped in the crane");
}

/// The world's solid pixels over the live copies' boxes, and each copy's
/// state and values but the tubes' (issue #98: their scripts switch them
/// at any tick, explosion or none).
fn live_snapshot(w: &olr_sim::World) -> (Vec<bool>, Vec<(u16, Vec<i64>)>) {
    let solid = w.solid();
    let mut px = Vec::new();
    for &(bx, by, bw, bh) in &LIVE_BOXES {
        for y in by..by + bh {
            for x in bx..bx + bw {
                px.push(solid.flags(i64::from(x), i64::from(y)) & olr_terrain::PX_ROCK != 0);
            }
        }
    }
    let copies = w
        .live()
        .unwrap()
        .copies()
        .filter(|c| !TUBES.contains(&c.id()))
        .map(|c| (c.state(), c.values().to_vec()))
        .collect();
    (px, copies)
}

/// Issue #97, #90's rule: an explosion never digs a live copy. Bazooka
/// hits (the explosion of its shell striking, damage 4) on the claw, on
/// the dirt beside it and on the terminal (issue #100; the console
/// before it), between ticks of a match of two idle bots: the copies'
/// solid pixels, states and values are unchanged, and the ground changes
/// where the same explosions change After Dark's (outside the live
/// copies' boxes, whose rock is the copies' here). The production line
/// design: the hits come when the claw first rests over the reservoir
/// (its round's wait, `Idle` at 75), on its right jaw's wall where it is.
#[test]
fn explosions_leave_the_factorys_live_copies_alone() {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::{MatchSpec, Personality};
    use olr_math::Vec2;
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let start = |level: &str| {
        let spec = MatchSpec { personality: Personality::Idle, ..MatchSpec::new(CLASSIC, level, 2, 1, 4000) };
        Match::new(&content, &spec, &Options::default()).unwrap()
    };
    // A tick the claw rests over the reservoir the first time, and its
    // right jaw's wall then (x 795-796 at `jaw_r_x` 0, rows 306-312).
    let (ticks, jaw) = {
        let mut m = start(FACTORY);
        let mut t = 0;
        while !(claw_x(m.world()) == 75 && live_copy(m.world(), "crane").state_name() == "Idle") {
            assert!(m.step());
            t += 1;
            assert!(t < 3000, "the claw never rests over the reservoir");
        }
        // Near the wait's end (`crane.rhai`'s DUMP 100), the dropped
        // dirt settled.
        for _ in 0..90 {
            assert!(m.step());
            t += 1;
        }
        assert_eq!(live_copy(m.world(), "crane").state_name(), "Idle", "still resting");
        let jr = live_copy(m.world(), "crane").value("jaw_r_x").unwrap().div_euclid(olr_sim::LIVE_SUBPIXELS);
        (t, 795.5 + jr as f32)
    };
    // Its right jaw's wall; the dirt behind the belt's roller (below the
    // line's box, which covers the reservoir); the terminal's foot.
    let hits = [Vec2::new(jaw, 309.5), Vec2::new(911.5, 358.5), Vec2::new(730.5, 363.5)];
    let blast = |level: &str| {
        let mut m = start(level);
        for _ in 0..ticks {
            assert!(m.step());
        }
        let mut w = m.world().clone();
        let before = w.terrain().clone();
        // The plant works' machines move on their own: the copies are
        // compared with the same tick without the explosions.
        let mut calm = w.clone();
        for &at in &hits {
            w.explode_for_test(at, 4.0);
        }
        // What the explosions dug, before loose grains fall into it.
        let mut dug = BTreeSet::new();
        for y in 0..before.height() {
            for x in 0..before.width() {
                if !in_live_box(x, y)
                    && before.flags(x.into(), y.into()) != w.terrain().flags(x.into(), y.into())
                {
                    dug.insert((x, y));
                }
            }
        }
        let mut ev = olr_sim::TickEvents::new();
        w.tick(&[olr_sim::Keys(0), olr_sim::Keys(0)], &mut ev);
        calm.tick(&[olr_sim::Keys(0), olr_sim::Keys(0)], &mut ev);
        if w.live().is_some() {
            assert_eq!(claw_x(&w), 75, "at rest over the reservoir");
            assert!(live_snapshot(&w) == live_snapshot(&calm), "{level}: an explosion changed a live copy");
        }
        (dug, before)
    };
    let ((ours, before), (theirs, after_dark)) = (blast(FACTORY), blast(AFTER_DARK));
    // Compared where the ground was After Dark's before the hits: not on
    // the dirt the crane dropped into the reservoir, nor on what it took
    // from the mine.
    let alike =
        |&&(x, y): &&(u32, u32)| before.flags(x.into(), y.into()) == after_dark.flags(x.into(), y.into());
    let ours: BTreeSet<_> = ours.iter().filter(alike).collect();
    let theirs: BTreeSet<_> = theirs.iter().filter(alike).collect();
    assert!(!theirs.is_empty(), "the hits dig After Dark's dirt");
    assert!(ours == theirs, "dug {} pixels, After Dark {}", ours.len(), theirs.len());
}

/// Issue #97: a 6000-tick match of 4 bots on the factory runs its scripts
/// without a `ScriptError`, both instances still running at its end (the
/// crane still crossing and the terminal still following it in its last
/// 1000 ticks; issue #98: each tube still switching in the last 1500),
/// and no run or tick uses more than half its operations
/// (`World::script_peak`, #88). #99 keeps it passing with its own.
#[test]
fn the_factory_scripts_never_fail() {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::MatchSpec;
    use olr_sim::EventKind;
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = MatchSpec { kill_limit: 0, time_limit: 0.0, ..MatchSpec::new(CLASSIC, FACTORY, 4, 2, 6000) };
    let mut m = Match::new(&content, &spec, &Options::default()).unwrap();
    let (mut errors, mut late_moves, mut late_alarms) = (0, 0, 0);
    let mut last = claw_x(m.world());
    let tube_states = |w: &olr_sim::World| TUBES.map(|t| live_copy(w, t).state());
    let (mut tubes, mut late_switches) = (tube_states(m.world()), [0; 3]);
    // The plant works: a tube shot out (Broken) or in a blackout (Dark)
    // rightly stops switching.
    let mut held = [false; 3];
    // Issue #99: the screen, the desk and the gauge go on changing.
    let shows = |w: &olr_sim::World| ["screen", "desk", "gauge"].map(|d| live_copy(w, d).state());
    let (mut displays, mut late_changes) = (shows(m.world()), [0; 3]);
    while m.step() {
        errors += m.events().iter().filter(|e| matches!(e.kind, EventKind::ScriptError { .. })).count();
        if m.ticks() > 5000 {
            late_moves += usize::from(claw_x(m.world()) != last);
            late_alarms += usize::from(live_copy(m.world(), "terminal").state_name() == "Alarm");
        }
        last = claw_x(m.world());
        let now = tube_states(m.world());
        for i in 0..3 {
            late_switches[i] += usize::from(m.ticks() > 4500 && now[i] != tubes[i]);
            let s = live_copy(m.world(), TUBES[i]).state_name();
            held[i] |= m.ticks() > 4500 && (s == "Broken" || s == "Dark");
        }
        tubes = now;
        let now = shows(m.world());
        for i in 0..3 {
            late_changes[i] += usize::from(m.ticks() > 4500 && now[i] != displays[i]);
        }
        displays = now;
    }
    let peak = m.world().script_peak();
    let run = m.finish();
    assert!(run.failure().is_none(), "{}", run.summary());
    assert_eq!(run.ticks, 6000);
    assert_eq!(errors, 0, "a script failed");
    assert!(
        late_moves > 0 && late_alarms > 0,
        "the scripts still run: {late_moves} moves, {late_alarms} alarms"
    );
    assert!(
        late_switches.iter().zip(held).all(|(&n, held)| n > 0 || held),
        "the tubes still switch: {late_switches:?}, held {held:?}"
    );
    assert!(late_changes.iter().all(|&n| n > 0), "the screen, desk and gauge still change: {late_changes:?}");
    assert!(peak.run * 2 <= olr_script::MAX_RUN_OPERATIONS, "a run used {} operations", peak.run);
    assert!(peak.tick * 2 <= olr_script::MAX_TICK_OPERATIONS, "a tick used {} operations", peak.tick);
}

/// A tube's state as [`tube_run`] records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tube {
    On,
    Dim,
    Off,
}

/// Issue #98: a `ticks`-tick match of two idle bots on the live factory,
/// seed `seed`, recorded per tick (the start, then after each tick): each
/// of [`TUBES`]' state and `lamps.glow` value, the tick's `LiveState`
/// entries into `On` per tube, and its live sounds as the presentation
/// reads them (issue #92's `LiveSounds`: the starts from the tick's
/// events, then the world's loops).
struct TubeRun {
    states: Vec<[(Tube, i64); 3]>,
    entries_on: Vec<[usize; 3]>,
    sounds: Vec<olr_content::LiveSounds>,
    /// The tubes' live copy indices, and the level's sound indices of
    /// `buzz.wav` and `tick.wav`.
    copies: [u16; 3],
    buzz: u16,
    tick: u16,
}

fn tube_run(seed: u32, ticks: u32) -> TubeRun {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::{MatchSpec, Personality};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = MatchSpec {
        personality: Personality::Idle,
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, FACTORY, 2, seed, ticks)
    };
    let mut m = Match::new(&content, &spec, &Options::default()).unwrap();
    let copies = TUBES.map(|t| live_copy(m.world(), t).index());
    let setup = live_copy(m.world(), TUBES[0]).setup();
    let glow = setup
        .props
        .iter()
        .position(|p| matches!(p.target, olr_sim::LiveTarget::GlowStrength(_)))
        .expect("a value driving lamps.glow");
    let on = setup.state("On").unwrap();
    let (buzz, tick) = (setup.sound("buzz.wav").unwrap(), setup.sound("tick.wav").unwrap());
    let read = |w: &olr_sim::World| {
        TUBES.map(|t| {
            let c = live_copy(w, t);
            let state = match c.state_name() {
                "On" => Tube::On,
                "Dim" => Tube::Dim,
                "Off" => Tube::Off,
                s => panic!("{t}: state {s}"),
            };
            (state, c.values()[glow])
        })
    };
    let mut run = TubeRun {
        states: vec![read(m.world())],
        entries_on: vec![[0; 3]],
        sounds: vec![olr_content::LiveSounds::default()],
        copies,
        buzz,
        tick,
    };
    loop {
        let (before, go) = (m.ticks(), m.step());
        if m.ticks() == before {
            break;
        }
        let live = m.world().live().unwrap();
        let mut sounds = olr_content::LiveSounds::default();
        let mut entries = [0; 3];
        for e in m.events().iter() {
            sounds.push_start(live, &e.kind);
            if let olr_sim::EventKind::LiveState { copy, state } = e.kind
                && let Some(i) = copies.iter().position(|&c| c == copy)
                && state == on
            {
                entries[i] += 1;
            }
        }
        sounds.set_loops(Some(live));
        run.states.push(read(m.world()));
        run.entries_on.push(entries);
        run.sounds.push(sounds);
        if !go {
            break;
        }
    }
    assert!(m.finish().failure().is_none());
    assert_eq!(run.states.len(), ticks as usize + 1);
    run
}

/// The flickers of each burst of a tube's states: a flicker is an `Off`
/// run of at most 10 ticks (the script's `OFF_MAX`), a burst flickers
/// whose `On` gaps are at most 15 ticks (`BACK_MAX`).
fn bursts(states: &[Tube]) -> Vec<usize> {
    let mut runs: Vec<(Tube, usize)> = Vec::new();
    for &s in states {
        match runs.last_mut() {
            Some((t, n)) if *t == s => *n += 1,
            _ => runs.push((s, 1)),
        }
    }
    let mut out = Vec::new();
    let mut current = 0;
    for (i, &(s, n)) in runs.iter().enumerate() {
        let flicker = s == Tube::Off && n <= 10 && i > 0;
        let gap = s == Tube::On && n <= 15 && current > 0;
        if flicker {
            current += 1;
        } else if !gap {
            if current > 0 {
                out.push(current);
            }
            current = 0;
        }
    }
    if current > 0 {
        out.push(current);
    }
    out
}

/// Issue #98: the faulty tubes fail unevenly. In a 12 000-tick match each
/// live tube is `On` at least 70 % of the ticks and has a burst of 2
/// flickers or more; the three tubes' state sequences differ (the
/// scripts' one generator, drawn in run order); the same seed gives the
/// same sequences, another seed others; `lamps.glow` is 100 whenever a
/// tube is `On` (ruling R2: entering `On` restores it, whatever came
/// before), 35 `Dim` and 0 `Off`; and (the plant works) every other strip
/// stays `Steady`, fully lit, in an idle match.
#[test]
fn the_tubes_fail_unevenly() {
    let a = tube_run(1, 12_000);
    let seq = |r: &TubeRun, i: usize| r.states.iter().map(|s| s[i].0).collect::<Vec<_>>();
    for (i, tube) in TUBES.iter().enumerate() {
        let states = seq(&a, i);
        let on = states.iter().filter(|&&s| s == Tube::On).count();
        assert!(on * 10 >= states.len() * 7, "{tube}: On {on} of {} ticks", states.len());
        let b = bursts(&states);
        assert!(b.iter().any(|&n| n >= 2), "{tube}: bursts {b:?}");
        assert!(states.contains(&Tube::Off), "{tube}: never Off");
        for (t, s) in a.states.iter().enumerate() {
            let want = match s[i].0 {
                Tube::On => 100,
                Tube::Dim => 35,
                Tube::Off => 0,
            };
            assert_eq!(s[i].1, want, "{tube}: lamps.glow at tick {t}, {:?}", s[i].0);
        }
    }
    for (i, ti) in TUBES.iter().enumerate() {
        for (j, tj) in TUBES.iter().enumerate().skip(i + 1) {
            assert!(seq(&a, i) != seq(&a, j), "{ti} and {tj} flicker together");
        }
    }
    let again = tube_run(1, 12_000);
    let other = tube_run(2, 12_000);
    for (i, tube) in TUBES.iter().enumerate() {
        assert!(seq(&a, i) == seq(&again, i), "{tube}: seed 1 again flickers otherwise");
        assert!(seq(&a, i) != seq(&other, i), "{tube}: seed 2 flickers as seed 1");
    }
    // The plant works: the other strips are live copies that stay Steady
    // (no blast near them), fully lit.
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = olr_cli::spec::MatchSpec {
        personality: olr_cli::spec::Personality::Idle,
        ..olr_cli::spec::MatchSpec::new(CLASSIC, FACTORY, 2, 1, 2000)
    };
    let mut m = olr_cli::runner::Match::new(&content, &spec, &olr_cli::runner::Options::default()).unwrap();
    let steady = ["strip-1", "strip-3", "strip-5", "strip-6", "strip-8", "strip-9"];
    while m.step() {
        for id in steady {
            let c = live_copy(m.world(), id);
            assert_eq!(c.state_name(), "Steady", "{id} at tick {}", m.ticks());
            let glow =
                c.setup().props.iter().position(|p| matches!(p.target, olr_sim::LiveTarget::GlowStrength(_)));
            assert_eq!(c.values()[glow.unwrap()], 100, "{id}: fully lit");
        }
    }
}

/// Issue #98: the tubes buzz and tick in time, as the presentation reads
/// the live sounds (issue #92's `LiveSounds`, headless): in a 4000-tick
/// match a live tube's `buzz.wav` loops on every tick it is `On` or `Dim`
/// and on no other, `tick.wav` starts once on each tick it enters `On`
/// and on no other; no sound loops at a steady strip (the plant works:
/// `Steady` is `On` without the buzz), and none starts there after the
/// first tick.
#[test]
fn the_tubes_buzz_and_tick() {
    let r = tube_run(1, 4000);
    let mut ticks = [0; 3];
    for t in 1..r.states.len() {
        let s = &r.sounds[t];
        for (i, tube) in TUBES.iter().enumerate() {
            let c = r.copies[i];
            let loops = s.loops.iter().filter(|l| l.copy == c && l.sound == r.buzz).count();
            let lit = r.states[t][i].0 != Tube::Off;
            assert_eq!(loops, usize::from(lit), "{tube}: buzzing at tick {t}, {:?}", r.states[t][i].0);
            assert!(s.loops.iter().filter(|l| l.copy == c).all(|l| l.sound == r.buzz), "{tube}: its loops");
            let starts = s.starts.iter().filter(|x| x.copy == c && x.sound == r.tick).count();
            assert_eq!(starts, r.entries_on[t][i], "{tube}: ticks at tick {t}");
            let entered = r.states[t][i].0 == Tube::On && r.states[t - 1][i].0 != Tube::On;
            if entered {
                assert_eq!(starts, 1, "{tube}: entered On at tick {t} without a tick");
            }
            ticks[i] += starts;
        }
    }
    assert!(ticks.iter().all(|&n| n >= 2), "the tubes' ticks: {ticks:?}");
    let steady = [(778, 80), (1119, 80), (1067, 177), (314, 286), (524, 286), (187, 342)];
    for (t, s) in r.sounds.iter().enumerate() {
        let starts = s.starts.iter().map(|x| x.pos).filter(|_| t > 1);
        for p in starts.chain(s.loops.iter().map(|l| l.pos)) {
            let by_steady = steady
                .iter()
                .any(|&(x, y)| (p.x - (x as f32 + 14.5)).abs() < 1.0 && (p.y - (y as f32 + 1.0)).abs() < 1.0);
            assert!(!by_steady, "a sound at a steady strip at tick {t}: {p:?}");
        }
    }
}

/// Issue #97: bots play the live factory with Classic and with Liero v1.0
/// Reloaded (4 bots, 3000 ticks each), `olr_cli::invariants` holding
/// every tick (`Match::step` checks them) and no failure.
#[test]
fn bots_play_the_live_factory() {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::MatchSpec;
    let content = Content::open_bundled(&packs_dir()).unwrap();
    for m in [CLASSIC, RELOADED_MOD] {
        let spec = MatchSpec { kill_limit: 0, time_limit: 0.0, ..MatchSpec::new(m, FACTORY, 4, 1, 3000) };
        let mut game = Match::new(&content, &spec, &Options::default()).unwrap();
        while game.step() {}
        let run = game.finish();
        assert!(run.failure().is_none(), "{m}: {}", run.summary());
        assert_eq!(run.ticks, 3000, "{m}");
    }
}

/// Issue #97: the live factory online (`olr_cli::netsim`): Classic, 3
/// clients, a lossy network, 1500 ticks and a spectator joining late,
/// ends with an `Ok` verdict, so rollback, the state transfer and the
/// checksums agree with the scripts running.
#[test]
fn the_live_factory_stays_in_sync() {
    use olr_cli::netsim::{NetSpec, run};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = NetSpec {
        mod_id: CLASSIC.into(),
        level_id: FACTORY.into(),
        seed: 97,
        slots: 8,
        clients: 3,
        spectators: 1,
        join_tick: Some(400),
        profile: "bad".into(),
        drift_ppm: 2_000,
        ticks: 1500,
        outages: vec![],
        net_seed: 97,
        difficulty: None,
        faults: vec![],
        exits: vec![],
        host_freeze: None,
        profile_changes: vec![],
        migration: false,
        strafe: false,
        weapon_keys: false,
    };
    let r = run(&content, &spec).unwrap();
    if let Err(f) = &r.verdict {
        panic!("{} failed at tick {}: {}", spec.stem(), f.tick, f.what);
    }
    assert!(r.report.stats.values().any(|s| s.rollbacks > 0), "rollbacks re-ran the scripts");
}

/// Issue #99: each tick's state name of each of [`DISPLAYS`] (after the
/// tick), in a `ticks`-tick match of two idle bots on the live factory.
fn display_run(seed: u32, ticks: u32) -> Vec<[String; 4]> {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::{MatchSpec, Personality};
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = MatchSpec {
        personality: Personality::Idle,
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, FACTORY, 2, seed, ticks)
    };
    let mut m = Match::new(&content, &spec, &Options::default()).unwrap();
    let mut rows = Vec::new();
    loop {
        let (before, go) = (m.ticks(), m.step());
        if m.ticks() == before {
            break;
        }
        rows.push(DISPLAYS.map(|d| live_copy(m.world(), d.0).state_name().to_string()));
        if !go {
            break;
        }
    }
    assert!(m.finish().failure().is_none());
    assert_eq!(rows.len(), ticks as usize);
    rows
}

/// The runs of equal values: (value, length), in order.
fn runs<T: PartialEq + Clone>(values: impl Iterator<Item = T>) -> Vec<(T, usize)> {
    let mut out: Vec<(T, usize)> = Vec::new();
    for v in values {
        match out.last_mut() {
            Some((last, n)) if *last == v => *n += 1,
            _ => out.push((v, 1)),
        }
    }
    out
}

/// Issue #99: over 2000 ticks the security screen shows Truck, Crane,
/// Offices and Garden in that order, again and again, 250 ticks each (the
/// first from the match's start, a tick's slack).
#[test]
fn the_screen_shows_each_camera() {
    let rows = display_run(1, 2000);
    let shows = runs(rows.iter().map(|r| r[0].clone()));
    let cycle = ["Truck", "Crane", "Offices", "Garden"];
    assert!(shows.len() >= 8, "{shows:?}");
    for (i, (name, len)) in shows.iter().enumerate() {
        assert_eq!(name, cycle[i % 4], "picture {i}: {shows:?}");
        if i == 0 {
            assert!(len.abs_diff(250) <= 1, "the first picture {len} ticks: {shows:?}");
        } else if i + 1 < shows.len() {
            assert_eq!(*len, 250, "picture {i}: {shows:?}");
        }
    }
}

/// Issue #99: over 6000 ticks the Lit sign leaves Normal at least twice,
/// each time for one of its faults, and is back within 80 ticks; the
/// Control desk and the Gauge each change state at least 10 times, never
/// to the state they show; the same seed changes alike, another seed
/// otherwise.
#[test]
fn the_displays_change_now_and_then() {
    let rows = display_run(1, 6000);
    let sign = runs(rows.iter().map(|r| r[1].clone()));
    let faults: Vec<&(String, usize)> = sign.iter().filter(|(s, _)| s != "Normal").collect();
    assert!(faults.len() >= 2, "{sign:?}");
    for (i, (s, len)) in sign.iter().enumerate() {
        if s != "Normal" {
            assert!(["No dash", "No LD", "Dying 7"].contains(&s.as_str()), "{s}");
            assert!(*len <= 81, "{s} for {len} ticks");
            assert!(i + 1 == sign.len() || sign[i + 1].0 == "Normal", "{sign:?}");
        }
    }
    for (k, name) in [(2, "desk"), (3, "gauge")] {
        let changes = runs(rows.iter().map(|r| r[k].clone()));
        assert!(changes.len() > 10, "{name}: {changes:?}");
        assert!(changes.iter().all(|(s, _)| ["A", "B", "C"].contains(&s.as_str())), "{name}");
    }
    let again = display_run(1, 6000);
    assert!(rows == again, "the same seed, another sequence");
    let other = display_run(2, 6000);
    for k in 1..4 {
        let col = |r: &Vec<[String; 4]>| r.iter().map(|x| x[k].clone()).collect::<Vec<_>>();
        assert!(col(&rows) != col(&other), "{}: seed 2 changes alike", DISPLAYS[k].0);
    }
}

/// Issue #99: the crane's warning beacon is #95's pick. The Crane's
/// lenses beam right (`lens`, 0°) and left (`lens-b`, 180°); in a match
/// of idle bots they ride with the claw every tick; while the crane is
/// `Idle` the beacon is out (`light`, `halo_glow`, `halo_light` 0); while
/// `Moving` it turns 1.2° a tick (3 s a turn, counted over the moving
/// ticks only), and at phases 0°, 45°, …, 315° (the nearest tick) its
/// spread and reach are the design's formulas within 1° and 1 px, its
/// light within 4 (`400 w`) and the halo's `400 − light`.
#[test]
fn the_beacon_follows_the_pick() {
    use olr_cli::runner::{Match, Options};
    use olr_cli::spec::{MatchSpec, Personality};
    let src = olr_format::assets::read_asset_files(&DirFs::new(repo().join("content/assets/crane"))).unwrap();
    let beam = |id: &str| {
        let e = src.file.elements.iter().find(|e| e.id == id).unwrap();
        let b = e.glow.as_ref().unwrap().beam.as_ref().unwrap();
        (b.direction, b.spread)
    };
    assert_eq!((beam("lens"), beam("lens-b")), ((0, 10), (180, 10)));
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = MatchSpec { personality: Personality::Idle, ..MatchSpec::new(CLASSIC, FACTORY, 2, 1, 3000) };
    let mut m = Match::new(&content, &spec, &Options::default()).unwrap();
    // Per moving tick: spread, spread b, reach, light, halo glow, halo light.
    let mut moving = Vec::new();
    let px = |c: &olr_sim::LiveCopyView, n: &str| c.value(n).unwrap().div_euclid(olr_sim::LIVE_SUBPIXELS);
    while m.step() {
        let c = live_copy(m.world(), "crane");
        assert_eq!(
            (px(&c, "lens_x"), px(&c, "lens_b_x")),
            (px(&c, "claw_x"), px(&c, "claw_x")),
            "tick {}",
            m.ticks()
        );
        let v = |n: &str| c.value(n).unwrap();
        let row =
            [v("beam_spread"), v("beam_spread_b"), v("reach"), v("light"), v("halo_glow"), v("halo_light")];
        if c.state_name() == "Moving" {
            moving.push(row);
        } else {
            assert_eq!(row[3..], [0, 0, 0], "tick {}: the beacon out while Idle", m.ticks());
        }
    }
    assert!(m.finish().failure().is_none());
    assert!(moving.len() >= 600, "{} moving ticks", moving.len());
    let want = |k: usize| {
        let a = 10f64.to_radians();
        let l = (k as f64 * 1.2).to_radians().cos().abs();
        let den = a.cos().powi(2) * l * l - a.sin().powi(2) * (1.0 - l * l);
        let spread = if den <= 0.0 { 89.0 } else { (a.sin() / den.sqrt()).atan().to_degrees().min(89.0) };
        let t = ((l - a.sin()) / 0.25).clamp(0.0, 1.0);
        (spread, (360.0 * l).max(1.0), 400.0 * t * t * (3.0 - 2.0 * t))
    };
    // The first moving tick turns from 0 (or, as the script's reads fall, 1).
    let fits = |o: usize| {
        (0..8).all(|p| {
            let k = (p * 300 + 4) / 8;
            let Some(r) = moving.get(k + o) else { return false };
            let (spread, reach, light) = want(k);
            (r[0] as f64 - spread).abs() <= 1.0
                && r[0] == r[1]
                && (r[2] as f64 - reach).abs() <= 1.0
                && (r[3] as f64 - light).abs() <= 4.0
                && r[5] == 400 - r[3]
                && r[4] == (400 - r[3]) / 4
        })
    };
    assert!(fits(0) || fits(1), "the beacon's turn: {:?}", &moving[..80]);
}
