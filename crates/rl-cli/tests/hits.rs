//! Hits land where the worm is (spec §18 D7, amended 2026-10-13): in seeded
//! bot matches without OLX's quirks, every projectile hit and explosion
//! injury on a worm comes from within the hit geometry of that worm's
//! current position (`World::hit_audit`, checked where the hit tests run);
//! with `SimSettings::olx_quirks` on (the oracle's OLX), some do not.

use crate::common;

use common::*;
use rl_cli::assets::LevelAssets;
use rl_cli::runner::{Match, Options};
use rl_cli::spec::{MatchSpec, Personality};
use rl_content::{BuiltMatch, build_match};
use rl_sim::{EventKind, HitAudit, Keys, TickEvents, World};

/// `spec`'s match with `olx_quirks` set, played as `Match::advance` plays
/// it (the bots think in worm order on the last tick's events, then the
/// world ticks); `each` sees the world and events after every tick.
fn play(
    content: &rl_content::Content,
    spec: &MatchSpec,
    olx_quirks: bool,
    mut each: impl FnMut(u32, &World, &TickEvents),
) -> World {
    let assets = LevelAssets::load(content, &spec.mod_id, &spec.level_id).unwrap();
    let mut o = spec.match_options();
    o.settings.olx_quirks = olx_quirks;
    let BuiltMatch { mut world, mut bots, .. } = build_match(assets.match_assets(), &o).unwrap();
    let mut out = TickEvents::new();
    let mut keys = vec![Keys(0); bots.len()];
    for tick in 0..spec.ticks {
        for (i, b) in bots.iter_mut().enumerate() {
            keys[i] = b.think(&world, &out);
        }
        world.tick(&keys, &mut out);
        each(tick, &world, &out);
    }
    world
}

fn audit(content: &rl_content::Content, spec: &MatchSpec, olx_quirks: bool) -> HitAudit {
    play(content, spec, olx_quirks, |_, _, _| {}).hit_audit()
}

/// Every hit of `specs` lands on the struck worm's current position, and
/// with OLX's quirks some of the same matches' hits do not.
fn hits_are_current(specs: &[MatchSpec]) {
    let Some(c) = content() else { return };
    for spec in specs {
        let a = audit(&c, spec, false);
        eprintln!("{}: {a:?}", spec.stem());
        assert!(a.projectile_hits > 0 && a.explosion_hits > 0, "{}: no hits to check: {a:?}", spec.stem());
        assert_eq!(
            (a.projectile_stale, a.explosion_stale),
            (0, 0),
            "{}: hits from outside the struck worm's current position: {a:?}",
            spec.stem()
        );
    }
    let olx =
        specs.iter().map(|s| audit(&c, s, true)).fold(0, |n, a| n + a.projectile_stale + a.explosion_stale);
    assert!(olx > 0, "with OLX's quirks some hits test a stale position");
}

#[test]
fn classic_hits_test_the_worms_current_position() {
    let specs: Vec<_> = [1, 2, 3]
        .map(|seed| MatchSpec::new(CLASSIC, "olx-level-dirt-level", 4, seed, 6000))
        .into_iter()
        .collect();
    hits_are_current(&specs);
}

#[test]
fn mw_hits_test_the_worms_current_position() {
    let specs: Vec<_> = [1, 2, 3]
        .map(|seed| MatchSpec {
            personality: Personality::Chaos,
            kill_limit: 0,
            ..MatchSpec::new(MW, "olx-level-castlestrike", 4, seed, 6000)
        })
        .into_iter()
        .collect();
    hits_are_current(&specs);
}

/// The investigation's match (Classic on Dirt Level, 2 bots, seed 2): with
/// OLX's quirks a worm dies by a stale hit (in 2026-10, worm 1 by its own
/// pellets at tick 4629, about 30 px from every one of them); in play that
/// phantom kill does not happen, and no hit is stale.
///
/// The control finds the phantom kill itself, whatever tick a later
/// blessed change to the bots or the simulation moves it to: the first
/// lethal hit (a `Kill` right after a `Damage` to its victim) on the worm
/// a stale hit struck in that tick (`HitAudit::last_stale_worm`). If a
/// change leaves this match with no such kill, the control says so, not
/// this fix: pick another seed or length where `play(.., true, ..)` shows
/// one (print each tick's `world.hit_audit()` and `Kill` events) and update
/// `spec` below.
#[test]
fn the_phantom_self_kill_of_the_investigation_is_gone() {
    let Some(c) = content() else { return };
    let spec = MatchSpec {
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, "olx-level-dirt-level", 2, 2, 4630)
    };
    let stale = |a: HitAudit| a.projectile_stale + a.explosion_stale;

    let mut before = 0;
    let mut phantom = None;
    play(&c, &spec, true, |tick, world, out| {
        let a = world.hit_audit();
        if phantom.is_none() && stale(a) > before {
            let ev = out.as_slice();
            phantom = ev.windows(2).find_map(|p| match (p[0].kind, p[1].kind) {
                (EventKind::Damage { worm, .. }, EventKind::Kill { victim, killer })
                    if worm == victim && a.last_stale_worm == Some(victim) =>
                {
                    Some((tick, victim, killer))
                }
                _ => None,
            });
        }
        before = stale(a);
    });
    let Some((tick, victim, killer)) = phantom else {
        panic!(
            "the control (OLX's quirks on) has no lethal stale hit in {} ticks of this match any more: \
             the match changed, not the fix; choose another one (see this test's doc comment)",
            spec.ticks
        );
    };
    eprintln!("with OLX's quirks: worm {victim} killed by {killer} at tick {tick}, by a stale hit on it");

    let mut m = Match::new(&c, &spec, &Options::default()).unwrap();
    let mut again = false;
    for t in 0..spec.ticks {
        m.advance();
        again |= t == tick && m.events().iter().any(|e| e.kind == EventKind::Kill { victim, killer });
    }
    let a = m.world().hit_audit();
    assert_eq!((a.projectile_stale, a.explosion_stale), (0, 0), "stale hits in play: {a:?}");
    assert!(!again, "in play, worm {victim} is killed by {killer} at tick {tick} too");
}
