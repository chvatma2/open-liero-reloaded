//! Whole bot matches on OLX's content (notes §4.3, §4.8): the same seed
//! gives the same keys and checksums, thinking allocates nothing, and every
//! think stays within its budget.

mod common;

use common::olx::*;
use rl_bots::{AimModel, BotConfig, Difficulty, Personality};
use rl_modes::{ChaosCycle, DeathmatchSettings, RespawnLoadout};
use rl_sim::{Keys, TickEvents};

/// Two Deathmatch bots (Easy, Xtreme) and two chaos bots on Dirt Level.
fn players() -> [Player; 4] {
    let dm = |difficulty| Player::Bot(BotConfig { difficulty, ..BotConfig::default() });
    let chaos = Player::Bot(BotConfig { personality: Personality::Chaos, ..BotConfig::default() });
    [dm(Difficulty::Easy), dm(Difficulty::Xtreme), chaos, chaos]
}

fn mixed(c: &rl_content::Content, seed: u32) -> Match {
    let weapons = c.load_mod(CLASSIC).unwrap().sim.weapons().len();
    let dm = DeathmatchSettings {
        respawn_loadout: RespawnLoadout::ChaosCycle(ChaosCycle::new(&[2, 3], weapons)),
        ..DeathmatchSettings::default()
    };
    deathmatch(c, CLASSIC, DIRT_LEVEL, seed, &players(), dm, |_| {})
}

/// Runs `ticks` ticks without allocating in the loop: every tick's keys and
/// checksum, the allocations made, and the units of every think.
fn play(m: &mut Match, ticks: usize) -> (Vec<[Keys; 4]>, Vec<u64>, u64, Vec<u32>) {
    let mut keys = Vec::with_capacity(ticks);
    let mut sums = Vec::with_capacity(ticks);
    let mut units = Vec::with_capacity(ticks * 4);
    let mut out = TickEvents::new();
    let info = allocation_counter::measure(|| {
        for _ in 0..ticks {
            let mut k = [Keys(0); 4];
            for (i, b) in m.bots.iter_mut().enumerate() {
                let b = b.as_mut().unwrap();
                k[i] = b.think(&m.w, &out);
                assert!(b.units_used() <= b.budget());
                units.push(b.units_used());
            }
            m.w.tick(&k, &mut out);
            keys.push(k);
            sums.push(m.w.checksum());
        }
    });
    (keys, sums, info.count_total, units)
}

#[test]
fn bot_matches_are_deterministic_and_allocation_free() {
    let Some(c) = content() else { return };
    let (keys, sums, allocs, units) = play(&mut mixed(&c, 11), 3000);
    assert_eq!(allocs, 0, "thinking and ticking allocated");
    let again = play(&mut mixed(&c, 11), 3000);
    assert_eq!((&again.0, &again.1), (&keys, &sums), "same seed, same keys and checksums");
    let other = play(&mut mixed(&c, 12), 3000);
    assert_ne!(other.0, keys, "another seed, another match");
    let most = units.iter().max().unwrap();
    let mean = units.iter().map(|&u| u as u64).sum::<u64>() / units.len() as u64;
    eprintln!("units per think: max {most}, mean {mean} (budget {})", rl_bots::DEFAULT_BUDGET);
}

/// OLX's own aim (spec §20 F2: kept for comparison) fights too: it aims
/// straight at the target and checks OLX's parabola.
#[test]
fn olx_direct_aim_still_fights() {
    let Some(c) = content() else { return };
    let cfg = BotConfig { difficulty: Difficulty::Xtreme, aim: AimModel::OlxDirect, ..BotConfig::default() };
    let (mut shots, mut hits) = (0, 0);
    for seed in [1, 2, 8] {
        let players = [Player::Bot(cfg), Player::Target];
        let mut m =
            deathmatch(&c, CLASSIC, DIRT_LEVEL, seed, &players, DeathmatchSettings::default(), |_| {});
        m.run_until(3000, |_| false);
        let (s, h) = m.shots_and_hits(0, 1);
        shots += s;
        hits += h;
    }
    eprintln!("OlxDirect: {hits} hits of {shots} shots");
    assert!(shots > 10 && hits > 10, "{hits} hits of {shots} shots");
}
