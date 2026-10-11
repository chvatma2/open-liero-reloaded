//! OLX's brain in whole matches (notes §1.2-1.9, §4.8): Deathmatch bots on
//! OLX's Dirt Level kill a target that stands still, harder bots hit more
//! often, and bots that are stuck or hold odd weapons stay within their
//! budget, keep moving and let the match end (Review Focus 1 and 4).

mod common;

use common::olx::*;
use common::*;
use rl_bots::{BotConfig, Difficulty, LoadoutPolicy, choose_loadout};
use rl_data::restrictions::{Preset, WeaponRules, WeaponState};
use rl_modes::DeathmatchSettings;
use rl_sim::{EndReason, EventKind, MatchResult, SimSettings};

fn bot(difficulty: Difficulty) -> Player {
    Player::Bot(BotConfig { difficulty, ..BotConfig::default() })
}

/// Classic on Dirt Level with OLX's defaults: the bots play the 100lt set.
fn duel(c: &rl_content::Content, seed: u32, difficulty: Difficulty) -> Match {
    deathmatch(
        c,
        CLASSIC,
        DIRT_LEVEL,
        seed,
        &[bot(difficulty), Player::Target],
        DeathmatchSettings::default(),
        |_| {},
    )
}

const SEEDS: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// The spawns (`FindSpot`) are up to 400 px apart in solid dirt, which
/// the bot digs through (OLX's bots dig as they walk, every 0.2 s): far
/// spawns, the target 250 px above, take over a minute.
#[test]
fn xtreme_bot_kills_static_target_quickly() {
    let Some(c) = content() else { return };
    let mut times = Vec::new();
    for seed in SEEDS {
        let mut m = duel(&c, seed, Difficulty::Xtreme);
        m.run_until(9000, |m| m.first_kill(0, 1).is_some());
        times.push(m.first_kill(0, 1).map_or(f32::INFINITY, |t| t as f32 / 100.0));
    }
    let mut sorted = times.clone();
    sorted.sort_by(f32::total_cmp);
    let median = (sorted[3] + sorted[4]) / 2.0;
    eprintln!("Xtreme kill times (s): {times:?}, median {median}");
    assert!(times.iter().all(|&t| t <= 90.0), "every target dies within 90 s: {times:?}");
    assert!(median <= 30.0, "half of them within 30 s: {times:?}");
}

#[test]
fn easy_bot_hits_less() {
    let Some(c) = content() else { return };
    let rate = |difficulty| {
        let (mut shots, mut hits) = (0, 0);
        for seed in SEEDS {
            let mut m = duel(&c, seed, difficulty);
            m.run_until(3000, |_| false);
            let (s, h) = m.shots_and_hits(0, 1);
            shots += s;
            hits += h;
        }
        eprintln!("{difficulty:?}: {hits} hits of {shots} shots");
        (shots, hits)
    };
    let (easy_shots, easy_hits) = rate(Difficulty::Easy);
    let (xtreme_shots, xtreme_hits) = rate(Difficulty::Xtreme);
    assert!(easy_shots > 50 && xtreme_shots > 50, "both fire: {easy_shots}, {xtreme_shots}");
    let (easy, xtreme) = (easy_hits as f32 / easy_shots as f32, xtreme_hits as f32 / xtreme_shots as f32);
    assert!(easy * 1.5 < xtreme, "Easy hits {easy:.3} per shot, Xtreme {xtreme:.3}");
}

/// 300 × 200 with a rock frame, three sealed rock pockets and the open
/// middle: a pit (worm 0), a pocket under an overhang (worm 1), and a
/// closed room (worm 2, with no weapons).
fn pockets() -> rl_terrain::Terrain {
    rocks(300, 200, |x, y| {
        let frame = !inside(x, y, (4, 4, 295, 195));
        let pit = inside(x, y, (10, 120, 60, 195)) && !inside(x, y, (20, 130, 50, 185));
        let overhang = inside(x, y, (100, 100, 180, 195)) && !inside(x, y, (110, 150, 170, 185));
        let room = inside(x, y, (220, 20, 290, 100)) && !inside(x, y, (230, 30, 280, 90));
        frame || pit || overhang || room
    })
}

#[test]
fn stuck_bots_stay_within_budget_and_match_ends() {
    let dm = DeathmatchSettings { time_limit: 0.2, ..DeathmatchSettings::default() };
    let worms = [((35, 180), vec![0, 1, 2, 3, 4]), ((140, 180), vec![1; 5]), ((255, 85), vec![-1; 5])];
    let w = dm_world(pockets(), sim_mod(), &worms, dm, 3);
    let cfg = BotConfig { budget: 2000, ..BotConfig::default() };
    let mut m = Match::new(w, &[Player::Bot(cfg); 3], 3);
    // `Match::tick` checks every think against its budget.
    let mut most = 0;
    let ended = m.run_until(1300, |m| {
        most = m.bots.iter().flatten().map(|b| b.units_used()).fold(most, u32::max);
        m.w.result().is_some()
    });
    assert!(ended, "the match ended");
    assert!(most > 1000, "the bots searched for a way out ({most} units at most)");
    assert_eq!(m.w.result(), Some(MatchResult { winner: 0, reason: EndReason::TimeLimit }));
    assert!(m.log.iter().all(|(_, e)| !matches!(e, EventKind::Kill { .. })), "nobody reached anybody");
    assert!(m.ticks >= 1200, "by the 12 s limit, not before ({})", m.ticks);
}

/// The path a worm walked: the sum of its moves, tick by tick.
fn walked(m: &mut Match, worm: usize, ticks: u32) -> f32 {
    let mut last = m.w.worms()[worm].pos;
    let mut sum = 0.0;
    for _ in 0..ticks {
        m.tick();
        let p = m.w.worms()[worm].pos;
        if m.w.worms()[worm].alive {
            sum += (p - last).length();
        }
        last = p;
    }
    sum
}

#[test]
fn bots_survive_odd_loadouts() {
    // Beams only, a jetpack only, and nothing at all, in a flat arena.
    let flat = || rocks(400, 200, |x, y| y >= 150 || !inside(x, y, (4, 4, 395, 195)));
    let dm = DeathmatchSettings::default;
    let cfg = Player::Bot(BotConfig::default());
    let cases = [
        (
            "beams only",
            dm_world(
                flat(),
                sim_mod(),
                &[((50, 140), vec![1, 2, 3, 4, 1]), ((350, 140), vec![1; 5])],
                dm(),
                4,
            ),
        ),
        (
            "jetpack only",
            dm_world(flat(), jetpack_mod(), &[((50, 140), vec![0; 5]), ((350, 140), vec![0; 5])], dm(), 4),
        ),
        (
            "no weapons",
            dm_world(flat(), sim_mod(), &[((50, 140), vec![-1; 5]), ((350, 140), vec![-1; 5])], dm(), 4),
        ),
    ];
    for (what, w) in cases {
        let mut m = Match::new(w, &[cfg, cfg], 4);
        let moved = walked(&mut m, 0, 600);
        assert!(moved > 60.0, "{what}: worm 0 walked {moved} px");
    }

    // OLX's content with every weapon banned: the loadout comes out empty.
    let Some(c) = content() else { return };
    let classic = c.load_mod(CLASSIC).unwrap();
    let names: Vec<&str> = classic.sim.weapons().iter().map(|w| w.name.as_str()).collect();
    let banned: Preset = names.iter().map(|n| (n.to_string(), WeaponState::Banned)).collect();
    let presets = [("all banned".to_string(), banned)].into_iter().collect();
    let rules = WeaponRules::from_parts(&classic.name, names.iter().copied(), &presets, "all banned");
    for policy in [LoadoutPolicy::OlxPreset, LoadoutPolicy::Random] {
        let cfg = BotConfig { loadout: policy, ..BotConfig::default() };
        let l = choose_loadout(&cfg, &rules, &SimSettings::lx56(), &mut rl_bots::BotRng::from_seed(1));
        assert!(l.slots.iter().all(|s| s.weapon == -1), "{policy:?}: {:?}", l.slots);
    }
    // Dirt Level with Classic, both worms holding nothing.
    let level = c.load_level(DIRT_LEVEL).unwrap();
    let terrain = rl_terrain::Terrain::from_grid(&level.material);
    let worms = [((60, 60), vec![-1; 5]), ((440, 60), vec![-1; 5])];
    let mut m = Match::new(dm_world(terrain, classic.sim.clone(), &worms, dm(), 5), &[cfg, cfg], 5);
    let moved = walked(&mut m, 0, 800);
    assert!(moved > 60.0, "Classic with every weapon banned: worm 0 walked {moved} px");
}
