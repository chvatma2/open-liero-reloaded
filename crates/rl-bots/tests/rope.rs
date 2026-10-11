//! The ninja rope (notes §1.5, §1.8): a bot climbs what it cannot jump.

mod common;

use common::olx::{Match, Player};
use common::*;
use rl_bots::BotConfig;
use rl_math::Vec2;
use rl_modes::DeathmatchSettings;

/// 400 × 240: a rock frame, the floor at 220, and a plateau from x 220 on
/// whose top is at 140, 80 px above the floor (a jump rises about 20).
fn cliff() -> rl_terrain::Terrain {
    rocks(400, 240, |x, y| !inside(x, y, (4, 4, 395, 219)) || x >= 220 && y >= 140)
}

#[test]
fn bots_use_the_rope_to_climb() {
    for seed in [1, 2, 3] {
        // Worm 1 stands on the plateau, holding nothing.
        let worms = [((60, 210), vec![1; 5]), ((380, 130), vec![-1; 5])];
        let w = dm_world(cliff(), sim_mod(), &worms, DeathmatchSettings::default(), seed);
        let mut m = Match::new(w, &[Player::Bot(BotConfig::default()), Player::Target], seed);
        // The scripted spot: up on the plateau.
        m.bots[0].as_mut().unwrap().set_goal(Some(Vec2::new(300.0, 130.0)));
        let (mut attached, mut climbed) = (false, None);
        for t in 0..2000 {
            m.tick();
            let me = &m.w.worms()[0];
            attached |= me.rope.attached;
            if me.pos.x > 230.0 && me.pos.y < 140.0 && me.on_ground {
                climbed = Some(t);
                break;
            }
        }
        let me = &m.w.worms()[0];
        assert!(me.rope.throws() > 0 && attached, "seed {seed}: the rope was thrown and hooked");
        assert!(climbed.is_some(), "seed {seed}: worm 0 got up the cliff (at {:?})", me.pos);
        eprintln!("seed {seed}: up in {:?} ticks, {} throws", climbed, me.rope.throws());
    }
}
