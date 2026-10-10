//! Spec §5.3 with real content: firing every weapon of the imported Classic
//! mod for 500 ticks, with its explosions, trails, beams and events, does not
//! allocate. Needs the imported OLX content (`RL_CONTENT_DIR`).

use std::path::Path;
use std::sync::Arc;

use rl_sim::{
    EventKind, Fnv, GameMode, Keys, KillCredit, Loadout, MatchSetup, ModeCtx, SimSettings, Slot, TickEvents,
    WeaponType, World, WormSetup, olx_kill_credit,
};

const TICKS: usize = 500;
const WORMS: usize = 4;

/// Gives each worm the next weapon in all five slots every `per_round`
/// ticks: worm `i` gets weapon `(round * WORMS + i) % n` (a mode's command,
/// applied at the end of the round's last tick).
#[derive(Clone, Debug)]
struct Rounds {
    ticks: usize,
    per_round: usize,
    n: usize,
}

impl GameMode for Rounds {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(self.clone())
    }

    fn checksum(&self, h: &mut Fnv) {
        h.usize(self.ticks);
    }

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        olx_kill_credit(ctx.worms(), ctx.settings(), victim, killer)
    }

    fn on_tick(&mut self, ctx: &mut ModeCtx) {
        self.ticks += 1;
        if self.ticks.is_multiple_of(self.per_round) {
            let round = self.ticks / self.per_round;
            for i in 0..WORMS {
                let weapon = ((round * WORMS + i) % self.n) as i32;
                for slot in 0..5 {
                    ctx.give_weapon(i as i32, slot, weapon).unwrap();
                }
            }
        }
    }
}

#[test]
fn firing_every_classic_weapon_does_not_allocate() {
    let Ok(dir) = std::env::var("RL_CONTENT_DIR") else {
        return eprintln!("RL_CONTENT_DIR not set; skipping");
    };
    let content = Path::new(&dir);
    if !content.is_dir() {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        return;
    }
    let content = rl_content::Content::open(content).unwrap();
    let sim_mod = Arc::new(content.sim_mod(&rl_content::olx_mod_id("Classic")).unwrap());
    let level = content.load_level("olx-level-dirt-level").unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let n = sim_mod.weapons().len();
    // Dead worms respawn the next frame, so every round's worms can fire.
    let settings =
        SimSettings { respawn_time: 0.0, bonuses: true, bonus_frequency: 1.0, ..SimSettings::lx56() };
    let terrain = rl_terrain::Terrain::from_grid(&level.material);
    // Each round gives the worms the next four weapons, in all five slots.
    let rounds = n.div_ceil(WORMS);
    let worms = (0..WORMS)
        .map(|i| WormSetup {
            team: 0,
            loadout: Loadout::new(vec![Slot::ready((i % n) as i32); 5], &settings),
            spawn: Some((100 + 80 * i as i32, 100)),
        })
        .collect();
    let setup = MatchSetup {
        holes: theme.stamps(),
        green_mask: theme.green_mask(),
        seed: 7,
        worms,
        ..MatchSetup::new(settings, sim_mod.clone(), terrain)
    };
    let mode = Rounds { ticks: 0, per_round: TICKS / rounds, n };
    let mut w = World::new(setup, Box::new(mode)).unwrap();
    let mut fired = vec![false; n];
    let mut allocations = 0;
    let mut events = TickEvents::new();
    for _ in 0..rounds {
        let keys = [
            Keys::FIRE | Keys::UP,
            Keys::FIRE | Keys::LEFT,
            Keys::FIRE | Keys::DOWN,
            Keys::FIRE | Keys::RIGHT,
        ]
        .map(Keys);
        let info = allocation_counter::measure(|| {
            for _ in 0..TICKS / rounds {
                w.tick(&keys, &mut events);
                for e in &events {
                    if let EventKind::WeaponFired { weapon, .. } = e.kind {
                        fired[weapon as usize] = true;
                    }
                }
            }
        });
        allocations += info.count_total;
    }
    eprintln!("{n} weapons in {rounds} rounds, {} fired", fired.iter().filter(|f| **f).count());
    assert_eq!(allocations, 0, "tick allocated");
    let missed: Vec<&str> = (0..n)
        .filter(|&i| !fired[i] && sim_mod.weapons()[i].kind != WeaponType::Special)
        .map(|i| sim_mod.weapons()[i].name.as_str())
        .collect();
    assert!(missed.is_empty(), "never fired: {missed:?}");
}
