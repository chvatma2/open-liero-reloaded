//! The idle personality (plan 15c): a bot that presses nothing, so a
//! player can explore a level with worms around.

use crate::common;

use common::olx::*;
use olr_bots::{BotConfig, Personality};
use olr_modes::DeathmatchSettings;
use olr_sim::{EventKind, Keys};

const CASTLE_STRIKE: &str = "olx-level-castlestrike";

#[test]
fn an_idle_bot_presses_nothing() {
    let Some(c) = content() else { return };
    let idle = Player::Bot(BotConfig { personality: Personality::Idle, ..BotConfig::default() });
    let dm = Player::Bot(BotConfig::default());
    let mut m =
        deathmatch(&c, CLASSIC, CASTLE_STRIKE, 5, &[idle, dm, dm, dm], DeathmatchSettings::default(), |_| {});
    let respawned = |m: &Match| {
        let died = m.log.iter().position(|(_, k)| matches!(k, EventKind::Kill { victim: 0, .. }));
        died.is_some_and(|d| m.log[d..].iter().any(|(_, k)| matches!(k, EventKind::Spawned { worm: 0 })))
    };
    let mut t = 0;
    while t < 2000 || (!respawned(&m) && t < 20_000) {
        assert_eq!(m.tick()[0], Keys(0), "tick {t}");
        t += 1;
    }
    assert!(respawned(&m), "the idle bot died and came back in {t} ticks");
}
