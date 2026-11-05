//! A level's loose dirt (the loose dirt design): loaded with the level
//! (`fixtures/live/levels/loose-test`, made by
//! `fixtures/live/make-loose-test.py`), in the worlds built from it, and
//! the colour map following its grains as they fall.

use std::path::{Path, PathBuf};

use olr_content::{Content, SandboxSetup, SpawnSpec};
use olr_data::level::{LooseMaterial, Material};
use olr_sim::{Keys, SimSettings, TickEvents};

const ID: &str = "olr-level-loose-test";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/live/levels/loose-test")
}

/// A content directory of the synthetic packs and the fixture level on
/// the synthetic theme.
fn content() -> (tempfile::TempDir, Content) {
    let tmp = tempfile::tempdir().unwrap();
    super::content::synthetic(tmp.path());
    let dir = tmp.path().join("loose-test");
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(fixture()).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), dir.join(e.file_name())).unwrap();
    }
    let path = dir.join("level.ron");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("olx-theme-dirt", "test-theme")).unwrap();
    let content = Content::open(tmp.path()).unwrap();
    (tmp, content)
}

#[test]
fn a_loose_level_loads_its_kinds_and_materials() {
    let (_tmp, content) = content();
    let level = content.load_level(ID).unwrap();
    let sim = level.loose_sim().expect("loose materials");
    assert_eq!(
        sim.materials(),
        [LooseMaterial { repose: 30, ..LooseMaterial::DEFAULT }, LooseMaterial::DEFAULT]
    );
    let terrain = level.terrain();
    assert!(terrain.has_loose());
    // Sand on the left shelf, gravel on the right, the bridge plain dirt,
    // the walls rock.
    assert_eq!((terrain.loose(30, 40), terrain.loose(130, 40), terrain.loose(100, 61)), (1, 2, 0));
    assert_eq!(terrain.loose(0, 0), 0);
    assert_eq!(level.material.get(100, 61), Material::Dirt);
    // The sandbox's world and a replay's terrain hold them.
    // (Its worms' spawn holes are carved at once: the kinds are compared.)
    let world = content.new_world(&setup()).unwrap();
    assert_eq!(world.terrain().loose_cells(), terrain.loose_cells());
    let (m, theme) = (content.load_mod("broken-mod").unwrap(), content.load_theme(&level.theme).unwrap());
    let assets = olr_content::replay_assets_from(&m, &level, &theme);
    assert_eq!(assets.terrain, terrain);
    assert_eq!(assets.loose.as_deref(), Some(&*sim));
    // A level without loose dirt has none.
    let plain = content.load_level("test-level").unwrap();
    assert!(plain.loose.is_none() && plain.loose_sim().is_none() && !plain.terrain().has_loose());
}

fn setup() -> SandboxSetup {
    SandboxSetup {
        mod_id: "broken-mod".into(),
        level_id: ID.into(),
        seed: 5,
        settings: SimSettings::lx56(),
        worms: vec![SpawnSpec::default(), SpawnSpec::default()],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    }
}

/// Every world pixel's colour agrees with its material while the grains
/// fall: background shows the back art and dirt (a moved grain too) never
/// does; the colour map's material, kinds included, is the world's.
#[test]
fn colour_follows_the_grains() {
    let (_tmp, content) = content();
    let level = content.load_level(ID).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let mut world = content.new_world(&setup()).unwrap();
    let mut map = level.color_map(&theme);
    let (w, h) = (level.material.width(), level.material.height());
    let mut events = TickEvents::new();
    let mut moved = 0;
    for tick in 0u32..600 {
        // Fire and walk now and then: holes dug into the loose dirt.
        let keys = |i: u32| {
            Keys(if (tick / 37 + i).is_multiple_of(3) { Keys::FIRE | Keys::RIGHT } else { Keys::LEFT })
        };
        world.tick(&[keys(0), keys(1)], &mut events);
        moved += world.loose_moves().len();
        olr_content::apply_edits(&mut map, &events, &world);
        let _ = map.take_dirty().count();
        if tick % 50 == 49 {
            assert!(map.material() == world.terrain(), "tick {tick}: the colour map's material drifted");
            let art = |x: u32, y: u32| {
                let i = ((y * map.width() + x) * 4) as usize;
                [map.pixels()[i], map.pixels()[i + 1], map.pixels()[i + 2]]
            };
            for y in 0..h {
                for x in 0..w {
                    let b = level.back.get(x, y);
                    let shows_back = art(x, y) == [b.r, b.g, b.b];
                    match world.terrain().material(x as i64, y as i64).unwrap() {
                        Material::Background => {
                            assert!(shows_back, "tick {tick}: ({x}, {y}) open but not back")
                        }
                        Material::Dirt => assert!(!shows_back, "tick {tick}: ({x}, {y}) dirt shows the back"),
                        Material::Rock => {}
                    }
                }
            }
        }
    }
    assert!(moved > 1000, "the grains fell: {moved} moves");
}
