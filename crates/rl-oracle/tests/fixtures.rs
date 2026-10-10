//! The committed oracle fixtures (`fixtures/oracle/`) are complete, consistent
//! with their scenarios, and still show the behaviour each scenario describes.
//! A failure here usually means a trace must be re-recorded with
//! `cargo xtask oracle-record <name>` (needs Docker).

use std::path::PathBuf;

use rl_oracle::{Scenario, Trace};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle")
}

fn load(name: &str) -> (Scenario, Trace) {
    let sc =
        Scenario::from_ron(&std::fs::read_to_string(dir().join(format!("{name}.ron"))).unwrap()).unwrap();
    let gz = std::fs::read(dir().join(format!("{name}.jsonl.gz")))
        .unwrap_or_else(|_| panic!("missing trace; run `cargo xtask oracle-record {name}`"));
    (sc, Trace::from_gz(&gz).unwrap())
}

#[test]
fn every_scenario_has_a_consistent_trace() {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["aim", "dig", "fall", "fire-minigun", "jump", "rope", "tap", "walk"]);
    for name in names {
        let (sc, t) = load(&name);
        let h = &t.header;
        assert_eq!(h.features["ForceLX56Aim"], "true", "{name}");
        assert_eq!(h.features["LX56PhysicsFPS"], "84", "{name}");
        assert_eq!((h.seed, h.options["K"].as_str()), (Some(sc.seed), "10"), "{name}");
        assert!(
            h.weapons.len() > 10 && h.projectiles.len() >= h.weapons.len() / 2,
            "{name}: names in the header"
        );
        assert_eq!(t.frames.len(), sc.frames as usize + 1, "{name}: frames -1..{}", sc.frames - 1);
        for (i, f) in t.frames.iter().enumerate() {
            assert_eq!(f.f, i as i64 - 1, "{name}");
            assert_eq!(f.w.len(), sc.worms.len(), "{name} frame {}", f.f);
            let st = f.st.unwrap_or_else(|| panic!("{name}: v2 trace"));
            assert_eq!((st, f.ct), (10 * f.f.max(0), Some(st + 10)), "{name}: pinned clock at frame {}", f.f);
        }
        let start = &t.frames[0].w[0];
        assert_eq!((start.x.0, start.y.0), (sc.worms[0].x as f32, sc.worms[0].y as f32), "{name}: spawn");
    }
}

fn worm(t: &Trace) -> impl Iterator<Item = &rl_oracle::WormRec> {
    t.frames.iter().map(|f| &f.w[0])
}

#[test]
fn fall_lands() {
    let (_, t) = load("fall");
    assert!(!t.frames[0].w[0].ground && t.frames.last().unwrap().w[0].ground);
}

#[test]
fn walk_moves_both_ways() {
    let (_, t) = load("walk");
    let xs: Vec<f32> = worm(&t).map(|w| w.x.0).collect();
    let max = xs.iter().cloned().fold(f32::MIN, f32::max);
    assert!(max - xs[0] > 30.0, "walked right {}", max - xs[0]);
    assert!(max - xs.last().unwrap() > 30.0, "walked back left");
    assert!(worm(&t).any(|w| w.face == 0) && worm(&t).any(|w| w.face == 1), "faced both ways");
}

#[test]
fn jump_leaves_the_ground() {
    let (sc, t) = load("jump");
    let lowest_y = worm(&t).map(|w| w.y.0).fold(f32::MAX, f32::min);
    assert!(lowest_y < sc.worms[0].y as f32 - 5.0, "peak y {lowest_y}");
}

#[test]
fn aim_reaches_both_limits() {
    let (_, t) = load("aim");
    assert!(worm(&t).any(|w| w.ang.0 == -90.0) && worm(&t).any(|w| w.ang.0 == 60.0));
}

#[test]
fn rope_hooks_and_releases() {
    let (_, t) = load("rope");
    let attached = worm(&t).filter(|w| w.rope.at).count();
    assert!(attached > 100, "attached for {attached} frames");
    assert!(!t.frames.last().unwrap().w[0].rope.at, "released at the end");
}

#[test]
fn dig_changes_the_material_map() {
    let (_, t) = load("dig");
    let hashes: std::collections::BTreeSet<&str> = t.frames.iter().map(|f| f.mh.as_str()).collect();
    assert!(hashes.len() > 3, "material changed {} times", hashes.len() - 1);
}

#[test]
fn minigun_shots_arrive_two_frames_later_and_fly() {
    let (sc, t) = load("fire-minigun");
    let h = &t.header;
    let minigun = h.weapons.iter().position(|w| w == "Minigun").unwrap() as i32;
    assert_eq!(t.frames[0].w[0].slots[sc.weapons[0].current].id, minigun, "simSetWeapons");
    let first_shot = t.frames.iter().find(|f| !f.s.is_empty()).unwrap();
    let first_proj = t.frames.iter().find(|f| !f.p.is_empty()).unwrap();
    assert!((3..=4).contains(&first_proj.f), "first projectile at frame {}", first_proj.f);
    assert_eq!(first_shot.f, first_proj.f, "a shot spawns its projectile before the frame it is listed in");
    assert!(t.frames.iter().all(|f| f.s.iter().all(|s| s.wpn == minigun && s.w == 0)));
    let p = &first_proj.p[0];
    assert_eq!(h.projectiles[p.t as usize], "minigun");
    assert_eq!(p.own, Some(0));
    let moved =
        t.frames.iter().filter_map(|f| f.p.iter().find(|q| q.i == p.i)).map(|q| q.x.0).fold(p.x.0, f32::max);
    assert!(moved > p.x.0 + 20.0, "the first bullet flies right: {} -> {moved}", p.x.0);
    assert!(t.frames.last().unwrap().w[0].slots[0].ch.0 < 1.0, "firing drained the charge");
    assert!(t.frames.last().unwrap().rc.unwrap() > 0, "shots draw rand()");
}

#[test]
fn tap_between_sub_steps_still_turns_the_aim() {
    let (_, t) = load("tap");
    assert!(worm(&t).any(|w| w.ang.0 != 0.0), "a press during frames without a sub-step was lost");
}
