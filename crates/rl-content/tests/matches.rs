//! `build_match` on OLX's content (`RL_CONTENT_DIR`, skipped without it).

use std::path::Path;

use rl_bots::{BotConfig, Difficulty, Personality};
use rl_content::{Content, DEFAULT_PRESET, MatchOptions, Player};
use rl_modes::{DeathmatchSettings, RespawnLoadout};
use rl_replay::ModeSpec;
use rl_sim::SimSettings;

fn content() -> Option<Content> {
    let dir = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir());
    let Some(dir) = dir else {
        eprintln!("RL_CONTENT_DIR not set; skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).unwrap())
}

/// A human, a Deathmatch bot and a chaos bot: the bots play worms 1 and 2
/// in worm order, only the chaos bot's loadout rotates, the human holds
/// weapons the restrictions enable, the recording has started, and the
/// same options give the same match.
#[test]
fn match_builder_seats_humans_and_bots() {
    let Some(c) = content() else { return };
    let chaos = BotConfig { personality: Personality::Chaos, ..BotConfig::default() };
    let o = MatchOptions {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-liero-factory".into(),
        seed: 11,
        settings: SimSettings::lx56(),
        players: vec![
            Player::Human,
            Player::Bot(BotConfig { difficulty: Difficulty::Hard, ..BotConfig::default() }),
            Player::Bot(chaos),
        ],
        deathmatch: DeathmatchSettings { kill_limit: 5, ..DeathmatchSettings::default() },
        restrictions: DEFAULT_PRESET.into(),
    };
    let m = c.build_match(&o).unwrap();
    assert_eq!(m.world.worms().len(), 3);
    assert_eq!(m.bot_worms().collect::<Vec<_>>(), [1, 2]);
    let ModeSpec::Deathmatch(dm) = &m.mode else { panic!("a Deathmatch") };
    assert_eq!(dm.kill_limit, 5);
    let RespawnLoadout::ChaosCycle(cycle) = dm.respawn_loadout else { panic!("the chaos bot rotates") };
    assert_eq!(cycle.worms().collect::<Vec<_>>(), [2]);
    let enabled = c.weapon_rules("olx-classic", DEFAULT_PRESET).unwrap().enabled_weapons();
    let human: Vec<i32> = m.setup.worms[0].loadout.slots.iter().map(|s| s.weapon).collect();
    assert_eq!(human.len(), 5);
    assert!(human.iter().all(|w| enabled.contains(w)), "{human:?} of {enabled:?}");
    let replay = m.recorder.clone().finish(&m.world).unwrap();
    assert_eq!((replay.ticks(), replay.checksums.len()), (0, 1), "tick 0's checksum");
    let again = c.build_match(&o).unwrap();
    assert_eq!(again.world, m.world);
    assert_eq!(again.setup.worms[0].loadout, m.setup.worms[0].loadout);
    // A Deathmatch needs two worms (spec §20 F5).
    let one = MatchOptions { players: vec![Player::Human], ..o };
    assert!(c.build_match(&one).is_err());
}
