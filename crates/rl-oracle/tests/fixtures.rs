//! The committed oracle fixtures (`fixtures/oracle/`) are complete, consistent
//! with their scenarios, and still show the behaviour each scenario describes.
//! A failure here usually means a trace must be re-recorded with
//! `cargo xtask oracle-record <name>` (needs Docker).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rl_oracle::{Frame, Scenario, Trace, WormRec};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/oracle")
}

/// Scenario names relative to `fixtures/oracle`, such as `walk` or
/// `weapons/classic/bazooka`, sorted.
fn names() -> Vec<String> {
    fn walk(root: &Path, sub: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(root.join(sub)).unwrap() {
            let p = e.unwrap().path();
            let rel = sub.join(p.file_name().unwrap());
            if p.is_dir() {
                walk(root, &rel, out);
            } else if p.extension().is_some_and(|x| x == "ron") {
                out.push(rel.with_extension("").to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(&dir(), Path::new(""), &mut out);
    out.sort();
    out
}

fn weapon_names() -> Vec<String> {
    names().into_iter().filter(|n| n.starts_with("weapons/")).collect()
}

fn load(name: &str) -> (Scenario, Trace) {
    let sc =
        Scenario::from_ron(&std::fs::read_to_string(dir().join(format!("{name}.ron"))).unwrap()).unwrap();
    let gz = std::fs::read(dir().join(format!("{name}.jsonl.gz")))
        .unwrap_or_else(|_| panic!("missing trace; run `cargo xtask oracle-record {name}`"));
    (sc, Trace::from_gz(&gz).unwrap())
}

/// The trace line of frame `f` (frame -1 is the first line).
fn frame(t: &Trace, f: i64) -> &Frame {
    &t.frames[(f + 1) as usize]
}

/// The first frame in which worm `worm` holds a key set containing `key`.
fn first_input_with(sc: &Scenario, worm: usize, key: &str) -> Option<u32> {
    sc.inputs.iter().filter(|c| c.worm == worm && c.keys.split('+').any(|k| k == key)).map(|c| c.frame).min()
}

const SPECIAL: [&str; 16] = [
    "beam-burst",
    "bleed",
    "bonus-health",
    "bonus-natural",
    "bonus-natural-default",
    "bonus-weapon",
    "castle-shoot-rope",
    "die-before-delivery",
    "dm-kill-limit",
    "explode-kill-respawn",
    "multi-shot",
    "reload-switch",
    "respawn-request",
    "rope-dirt",
    "suicide-score",
    "two-worms-fire",
];

#[test]
fn every_scenario_has_a_consistent_trace() {
    let names = names();
    for n in ["aim", "dig", "fall", "fire-minigun", "jump", "rope", "tap", "walk"].iter().chain(&SPECIAL) {
        assert!(names.iter().any(|m| m == n), "missing scenario {n}");
    }
    // One scenario per weapon of each mod (`cargo xtask oracle-gen-weapons`).
    let count = |m: &str| names.iter().filter(|n| n.starts_with(&format!("weapons/{m}/"))).count();
    assert_eq!((count("classic"), count("mw-1-0")), (38, 50), "weapon scenarios");
    assert_eq!(names.len(), 8 + SPECIAL.len() + 38 + 50, "{names:?}");
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
        assert!(h.weapons.iter().chain(&h.projectiles).all(|n| !n.is_empty()), "{name}: an empty name");
        assert_eq!(t.frames.len(), sc.frames as usize + 1, "{name}: frames -1..{}", sc.frames - 1);
        for (i, f) in t.frames.iter().enumerate() {
            assert_eq!(f.f, i as i64 - 1, "{name}");
            assert_eq!(f.w.len(), sc.worms.len(), "{name} frame {}", f.f);
            let st = f.st.unwrap_or_else(|| panic!("{name}: v2 trace"));
            assert_eq!((st, f.ct), (10 * f.f.max(0), Some(st + 10)), "{name}: pinned clock at frame {}", f.f);
        }
        for (i, spawn) in sc.worms.iter().enumerate() {
            let start = &t.frames[0].w[i];
            assert_eq!((start.x.0, start.y.0), (spawn.x as f32, spawn.y as f32), "{name}: spawn of worm {i}");
        }
        for w in &sc.weapons {
            let ids: Vec<i32> = t.frames[0].w[w.worm].slots.iter().map(|s| s.id).collect();
            let want: Vec<i32> =
                w.names.iter().map(|n| h.weapons.iter().position(|m| m == n).unwrap() as i32).collect();
            assert_eq!(ids, want, "{name}: simSetWeapons for worm {}", w.worm);
        }
    }
}

fn worm(t: &Trace) -> impl Iterator<Item = &WormRec> {
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
fn rope_hooking_dirt_draws_ten_cosmetic_rands() {
    let (_, t) = load("rope-dirt");
    let i = t.frames.iter().position(|f| f.w[0].rope.at).expect("the rope hooks");
    assert!(t.frames[i..].iter().filter(|f| f.w[0].rope.at).count() > 50, "stays hooked");
    // OLX's rope spawns 5 dirt particles (2 rand() each) when it first hooks dirt.
    assert_eq!((t.frames[i - 1].rc, t.frames[i].rc), (Some(0), Some(10)), "rand() calls around the hook");
    assert_eq!(t.frames[i].ents, Some(5));
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

/// Every weapon scenario (`fixtures/oracle/weapons/<mod>/<weapon>.ron`) shows
/// its weapon working: within 20 frames of the first fire key there are
/// projectiles, or a beam's effect (carved terrain or a hurt dummy); the
/// firer aims as the scenario says when each later phase fires; and the dummy
/// (worm 1) is hurt in at least 70 % of the scenarios.
#[test]
fn weapon_scenarios_fire_aim_and_hurt_the_dummy() {
    let names = weapon_names();
    assert_eq!(names.len(), 88, "weapon scenarios");
    let mut hurt = 0;
    let mut problems = Vec::new();
    for name in &names {
        let (sc, t) = load(name);
        let fire = first_input_with(&sc, 0, "fire").expect("a fire key") as i64;
        let start = frame(&t, fire);
        let acted = (fire..=fire + 20)
            .map(|f| frame(&t, f))
            .any(|f| !f.p.is_empty() || f.mh != start.mh || f.w[1].hp.0 < start.w[1].hp.0 || !f.w[1].alive);
        if !acted {
            problems.push(format!("{name}: nothing within 20 frames of the first fire (frame {fire})"));
        }
        for (keys, angle) in [("up+fire", -90.0), ("down+fire", 60.0)] {
            let Some(c) = sc.inputs.iter().find(|c| c.worm == 0 && c.keys == keys) else {
                problems.push(format!("{name}: no {keys} phase"));
                continue;
            };
            let w = &frame(&t, c.frame as i64).w[0];
            if w.alive && w.ang.0 != angle {
                problems.push(format!("{name}: angle {} when {keys} starts, not {angle}", w.ang.0));
            }
        }
        if t.frames.iter().any(|f| f.w[1].hp.0 < 100.0 || !f.w[1].alive) {
            hurt += 1;
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert!(hurt * 10 >= names.len() * 7, "the dummy was hurt in only {hurt} of {} scenarios", names.len());
}

/// Spec §18 D3: Classic's Flamer flames have no timer, but their image
/// animation plays once (6 frames at rate 10), so a flame fired straight up
/// into the open sky disappears after 0.6 s instead of rising off the map.
#[test]
fn flames_vanish_when_their_animation_ends() {
    let (sc, t) = load("weapons/classic/flamer");
    let flamer = t.header.projectiles.iter().position(|n| n == "flamer").unwrap() as i32;
    let up = sc.inputs.iter().find(|c| c.keys == "up+fire").unwrap().frame as i64;
    let down = first_input_with(&sc, 0, "down").unwrap() as i64;
    // Projectile slots are reused, so a flame is (slot, spawn time).
    let mut life: BTreeMap<(i32, Option<i64>), (i64, i64)> = BTreeMap::new();
    for f in &t.frames {
        for p in f.p.iter().filter(|p| p.t == flamer) {
            life.entry((p.i, p.sp)).and_modify(|l| l.1 = f.f).or_insert((f.f, f.f));
        }
    }
    let rising: Vec<i64> =
        life.values().filter(|(first, _)| (up..down).contains(first)).map(|(a, b)| b - a).collect();
    assert!(rising.len() > 20, "{} flames fired up", rising.len());
    assert!(rising.iter().all(|l| (55..=62).contains(l)), "flame lifetimes in frames: {rising:?}");
}

/// Two frames of Flamer shots (one per sub-step) form one list. When they make
/// one run, the S2C_MULTISHOOT packet gives every later shot the first shot's
/// position and random seed. Runs also break, for instance while the worm's
/// speed changes, and then the shots arrive as single-shot packets.
#[test]
fn multi_shot_packets_reuse_the_first_shots_seed_and_position() {
    let (_, t) = load("multi-shot");
    let lists: Vec<&Frame> = t.frames.iter().filter(|f| f.s.len() > 1).collect();
    assert!(lists.iter().all(|f| f.s.iter().all(|s| s.w == 0)), "one worm fires");
    assert!(lists.iter().all(|f| f.s.windows(2).all(|p| p[0].t < p[1].t)), "later shots are later");
    let runs: Vec<&&Frame> = lists
        .iter()
        .filter(|f| f.s.iter().all(|s| s.rnd == f.s[0].rnd && s.x == f.s[0].x && s.y == f.s[0].y))
        .collect();
    assert!(runs.len() > 10, "{} multi-shot packets", runs.len());
    assert!(runs.iter().any(|f| f.w[0].vx.0 != 0.0), "some are fired while walking");
    assert!(runs.len() < lists.len(), "some two-shot lists are split into single shots");
}

#[test]
fn two_worms_firing_send_separate_runs() {
    let (_, t) = load("two-worms-fire");
    assert!(t.frames.iter().any(|f| f.s.iter().any(|s| s.w == 0) && f.s.iter().any(|s| s.w == 1)));
    assert!(t.frames.last().unwrap().w.iter().all(|w| w.hp.0 < 100.0), "both worms are hit");
}

#[test]
fn shots_of_a_worm_that_died_before_delivery_are_dropped() {
    let (_, t) = load("die-before-delivery");
    let died = t.frames.iter().position(|f| !f.w[0].alive).expect("worm 0 dies");
    let late = t.frames[died..].iter().position(|f| f.s.iter().any(|s| s.w == 0));
    assert!(late.is_some(), "a shot of worm 0 is delivered after it died");
    let spawned_late = t.frames[died..]
        .iter()
        .flat_map(|f| &f.p)
        .any(|p| p.own == Some(0) && p.sp.unwrap() >= t.frames[died].st.unwrap());
    assert!(!spawned_late, "a dead worm's shot spawned a projectile");
}

#[test]
fn a_beam_fires_in_bursts() {
    let (_, t) = load("beam-burst");
    // ProcessShot_Beam sets the shared slot's LastFire to 0.3 when the shot arrives.
    let lf: Vec<f32> = worm(&t).map(|w| w.slots[0].lf.0).collect();
    let bursts = lf.windows(2).filter(|p| p[0] <= 0.0 && p[1] > 0.25).count();
    assert!(bursts >= 5, "{bursts} bursts");
}

#[test]
fn explosion_kills_and_the_server_respawns_after_respawn_time() {
    let (_, t) = load("explode-kill-respawn");
    let died = t.frames.iter().position(|f| !f.w[1].alive).expect("worm 1 dies");
    let (before, at) = (&t.frames[died - 1], &t.frames[died]);
    assert_eq!((at.w[1].deaths, at.w[0].kills), (before.w[1].deaths + 1, before.w[0].kills + 1));
    assert!(at.p.len() > 1, "the bazooka exploded into fragments");
    let back = died + t.frames[died..].iter().position(|f| f.w[1].alive).expect("worm 1 respawns");
    // RespawnTime 2.5 s, strict `>`, and currentTime = simulationTime + 10 ms (D2).
    assert_eq!(back - died, 250, "frames between death and respawn");
    let w = &t.frames[back].w[1];
    assert_eq!(w.hp.0, 100.0);
    assert!((w.x.0, w.y.0) != (at.w[1].x.0, at.w[1].y.0), "FindSpot chose a new place");
    assert_eq!(t.frames[back].mt, Some(at.mt.unwrap() + 2), "one FindSpot try");
}

#[test]
fn a_worm_below_15_hp_bleeds_every_two_seconds() {
    let (_, t) = load("bleed");
    let mut bleeds = Vec::new();
    for p in t.frames.windows(2) {
        if p[1].w[0].blood != p[0].w[0].blood {
            bleeds.push((p[1].w[0].blood.unwrap(), p[1].rc.unwrap() - p[0].rc.unwrap()));
        }
    }
    assert!(bleeds.len() >= 3, "{bleeds:?}");
    assert!(bleeds.iter().all(|(_, rc)| *rc == 80), "80 rand() per bleed: {bleeds:?}");
    assert!(bleeds.windows(2).all(|b| b[1].0 - b[0].0 > 2000), "{bleeds:?}");
}

#[test]
fn a_health_bonus_is_picked_up() {
    let (_, t) = load("bonus-health");
    let i = t.frames.iter().position(|f| !f.b.is_empty()).expect("the bonus appears");
    let took = i + t.frames[i..].iter().position(|f| f.b.is_empty()).expect("the bonus is taken");
    let (before, after) = (&t.frames[took - 1].w[0], &t.frames[took].w[0]);
    assert!(after.hp.0 > before.hp.0, "health {} -> {}", before.hp.0, after.hp.0);
    // The server keeps its copy until BonusLife ends (ParseGrabBonus never frees it).
    assert_eq!(t.frames.last().unwrap().sb.len(), 1);
}

#[test]
fn a_weapon_bonus_replaces_the_current_weapon() {
    let (sc, t) = load("bonus-weapon");
    let want = sc.bonuses[0].weapon.as_deref().unwrap();
    let id = t.header.weapons.iter().position(|w| w == want).unwrap() as i32;
    let i = t.frames.iter().position(|f| !f.b.is_empty()).expect("the bonus appears");
    let took = i + t.frames[i..].iter().position(|f| f.b.is_empty()).expect("the bonus is taken");
    let (before, after) = (&t.frames[took - 1].w[0], &t.frames[took].w[0]);
    assert_ne!(before.slots[before.wpn as usize].id, id);
    assert_eq!(after.slots[after.wpn as usize].id, id);
    assert!(t.frames[took..].iter().any(|f| f.s.iter().any(|s| s.wpn == id)), "the new weapon fires");
}

#[test]
fn only_the_current_slot_reloads() {
    let (_, t) = load("reload-switch");
    let reloading = t.frames.iter().position(|f| f.w[0].slots[0].rl).expect("slot 0 empties and reloads");
    let away: Vec<&Frame> = t.frames[reloading..].iter().filter(|f| f.w[0].wpn != 0).collect();
    assert!(away.len() > 20, "switched away while reloading");
    assert!(away.iter().all(|f| f.w[0].slots[0].rl && f.w[0].slots[0].ch == away[0].w[0].slots[0].ch));
    assert!(away.iter().any(|f| !f.s.is_empty()), "the other slot fires");
    let full = t.frames[reloading..].iter().position(|f| !f.w[0].slots[0].rl).expect("reloads again");
    assert_eq!(t.frames[reloading + full].w[0].slots[0].ch.0, 1.0);
}

/// The frame in which worm `w` dies for the first time at or after `from`.
fn death(t: &Trace, w: usize, from: usize) -> usize {
    from + t.frames[from..].iter().position(|f| !f.w[w].alive).unwrap_or_else(|| panic!("worm {w} dies"))
}

#[test]
fn suicides_and_deaths_count_only_above_score_zero() {
    let (_, t) = load("suicide-score");
    assert_eq!(t.header.features["AllowNegativeScore"], "false");
    let score = |f: &Frame, w: usize| (f.w[w].kills, f.w[w].deaths, f.w[w].suicides.unwrap());
    // Worm 1 is killed by worm 0: a kill for worm 0, and worm 1's death at score 0 does not count.
    let d1 = death(&t, 1, 0);
    assert_eq!((score(&t.frames[d1], 0), score(&t.frames[d1], 1)), ((1, 0, 0), (0, 0, 0)));
    // Worm 2 kills itself at score 0: neither the suicide nor the death counts, and nobody gets a kill.
    let d2 = death(&t, 2, 0);
    assert!(d2 > d1);
    assert_eq!(score(&t.frames[d2], 2), (0, 0, 0));
    assert_eq!(t.frames[d2].w.iter().map(|w| w.kills).sum::<i32>(), 1, "a suicide, not a kill");
    // Worm 0 kills itself at score 1: the suicide counts, and so does the death.
    let d0 = death(&t, 0, 0);
    assert!(d0 > d2);
    assert_eq!((score(&t.frames[d0 - 1], 0), score(&t.frames[d0], 0)), ((1, 0, 0), (1, 1, 1)));
    let last = t.frames.last().unwrap();
    assert!(last.w.iter().all(|w| w.alive), "the server respawns every worm");
}

#[test]
fn a_dead_worm_respawns_when_its_jump_asks_for_it() {
    let (sc, t) = load("respawn-request");
    assert_eq!(
        (t.header.features["RespawnTime"].as_str(), t.header.features["MaxRespawnTime"].as_str()),
        ("1", "6")
    );
    let died = death(&t, 1, 0);
    let jumps: Vec<u32> =
        sc.inputs.iter().filter(|c| c.worm == 1 && c.keys == "jump").map(|c| c.frame).collect();
    // `respawn`: bit 0 bRespawnRequested, bit 1 bCanRespawnNow, bit 2 spawned once.
    let allowed = died + t.frames[died..].iter().position(|f| f.w[1].respawn == Some(6)).expect("allowed");
    // RespawnTime 1 s, strict `>`, with currentTime = simulationTime + 10 ms (D2).
    assert_eq!(allowed - died, 100, "frames between death and bCanRespawnNow");
    assert!((jumps[0] as usize + 1) < allowed, "the first jump comes too early");
    let back = died + t.frames[died..].iter().position(|f| f.w[1].alive).expect("worm 1 respawns");
    assert_eq!(back, jumps[1] as usize + 1, "the second jump respawns it in its own frame");
    assert!(back - died < 600, "before MaxRespawnTime would force it");
    assert_eq!(t.frames[back].w[1].respawn, Some(4));
}

#[test]
fn natural_bonuses_spawn_by_frequency_and_expire_after_bonus_life() {
    let (_, t) = load("bonus-natural");
    let mut spawns: Vec<(usize, i32, i32, i64)> = Vec::new();
    for (i, f) in t.frames.iter().enumerate() {
        for b in &f.sb {
            if !spawns.iter().any(|s| (s.1, s.3) == (b.i, b.spawn)) {
                spawns.push((i, b.i, b.t, b.spawn));
            }
        }
    }
    assert!(spawns.len() >= 6, "{spawns:?}");
    // `currentTime - fLastBonusTime > BonusFrequency` (1 s): one spawn every 101 frames.
    assert!(spawns.windows(2).all(|s| s[1].3 - s[0].3 == 1010), "{spawns:?}");
    assert!(spawns.iter().any(|s| s.2 == 0) && spawns.iter().any(|s| s.2 == 1), "both kinds: {spawns:?}");
    // Each spawn draws its kind and its weapon: two rand() calls, and FindSpot at least two rnd().
    for &(i, ..) in &spawns {
        let (a, b) = (&t.frames[i - 1], &t.frames[i]);
        assert_eq!(b.rc.unwrap() - a.rc.unwrap(), 2, "frame {}", b.f);
        assert!(b.mt.unwrap() - a.mt.unwrap() >= 2, "frame {}", b.f);
    }
    // A slot older than BonusLife (3 s) is freed and used again.
    let reused = spawns.iter().filter(|s| spawns.iter().any(|o| o.1 == s.1 && o.3 < s.3)).count();
    assert!(reused >= 2, "{spawns:?}");
}

#[test]
fn natural_bonuses_hold_only_weapons_the_default_restrictions_allow() {
    let (_, t) = load("bonus-natural");
    assert_eq!(t.header.features["WeaponRestrictionsFile"], "Bonus only");
    let (_, t) = load("bonus-natural-default");
    assert_eq!(t.header.features["WeaponRestrictionsFile"], "Standard 100lt");
    // Classic's `Standard 100lt.wps` enables these 12 and bans the other 26.
    let allowed = [
        "Blaster",
        "Cannon",
        "Chaingun",
        "Dart",
        "Doomsday",
        "Handgun",
        "Mini Rockets",
        "Napalm",
        "Shotgun",
        "Super Shotgun",
        "Uzi",
        "Winchester",
    ];
    let mut weapons = std::collections::BTreeSet::new();
    for f in &t.frames {
        for b in &f.sb {
            let name = t.header.weapons[b.wpn as usize].as_str();
            assert!(allowed.contains(&name), "frame {}: {name}", f.f);
            if b.t == 0 {
                weapons.insert((b.spawn, name));
            }
        }
    }
    assert!(weapons.len() >= 5, "{weapons:?}");
}

#[test]
fn deathmatch_ends_in_the_frame_of_the_deciding_kill() {
    let (sc, t) = load("dm-kill-limit");
    assert_eq!(sc.mode, rl_oracle::ScenarioMode::Deathmatch);
    assert_eq!(t.header.features["KillLimit"], "2");
    // One explosion kills worms 1 and 2: worm 0 reaches the kill limit.
    let d = death(&t, 1, 0);
    assert_eq!(death(&t, 2, 0), d, "both in one frame");
    assert_eq!(t.frames[d].w[0].kills, 2);
    // `CheckGameOver` ends the match in that frame: `go` names its server
    // frame and worm 0, and stays.
    assert!(t.frames[..d].iter().all(|f| f.go.is_none()), "no game over before");
    let go = t.frames[d].go.expect("game over in the kill's frame");
    assert_eq!((go.f, go.w), (t.frames[d].sf.unwrap(), 0));
    assert!(t.frames[d..].iter().all(|f| f.go == Some(go)));
    // The client clears the explosion's projectiles at the next delivery
    // (`ParseGameOver`), and worm 0, holding "up", aims no more.
    assert!(t.frames[d].p.len() > 5 && t.frames[d + 1].p.is_empty());
    assert!(t.frames[d - 1].w[0].ang != t.frames[d - 2].w[0].ang, "aiming before");
    assert!(t.frames[d..].iter().all(|f| f.w[0].ang == t.frames[d].w[0].ang), "no input after");
    // Nobody respawns, and the physics stops 300 frames (3.0000002 s in
    // `float`) after the end: worm 0's fall freezes.
    assert!(t.frames[d..].iter().all(|f| !f.w[1].alive && !f.w[2].alive));
    let y = |i: usize| t.frames[i].w[0].y;
    assert!(y(d + 299) != y(d + 298), "still falling");
    assert!(t.frames[d + 300..].iter().all(|f| f.w[0].y == y(d + 299)), "frozen");
}
