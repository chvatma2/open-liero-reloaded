//! Every death shows (spec §18 D9, amended 2026-10-13): in seeded bot
//! matches, each kill, by a lethal hit or at exactly 0 health, comes with
//! one death sound and the victim's 7 gibs right after it in the tick's
//! events, and the presentation (`Presentation::on_tick`, as local play
//! feeds it) spawns every entity of every death (its gibs and blood,
//! `Event::death`), counted as they spawn (`Fx::death_spawns`), never one
//! dropped. An exact-zero death is credited to the worm whose damage left
//! 0 health.

use crate::common;

use common::*;
use olr_cli::assets::LevelAssets;
use olr_cli::runner::{Match, Options};
use olr_cli::spec::{MatchSpec, Personality};
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_scene::{SceneOptions, Viewport};
use olr_sim::{CosmeticKind, Event, EventKind, SoundId};

#[derive(Debug, Default)]
struct Deaths {
    /// Kills by a lethal hit (a `Damage` to the victim right before).
    hits: usize,
    /// Kills at exactly 0 health, the next frame.
    zero: usize,
    /// Of those, credited to nobody.
    zero_uncredited: usize,
    /// The deaths' gibs and blood the entities took.
    spawned: u64,
}

fn is_death_sound(e: &Event) -> bool {
    matches!(e.kind, EventKind::Sound { id, .. } if SoundId::DEATH.contains(&id))
}

fn is_gib(e: &Event, worm: i32) -> bool {
    matches!(e.kind, EventKind::SpawnCosmetic { kind: CosmeticKind::Gib { worm: w }, .. } if w == worm)
}

/// Plays `spec` with a presentation and checks every death's effects.
fn deaths(c: &olr_content::Content, spec: &MatchSpec) -> Deaths {
    let assets = LevelAssets::load(c, &spec.mod_id, &spec.level_id).unwrap();
    let mut m = Match::with_assets(&assets, spec, &Options::default()).unwrap();
    let setup = PresentSetup {
        core: c.core(),
        m: &assets.m,
        level: &assets.level,
        theme: &assets.theme,
        viewport: Viewport::GAME,
        options: SceneOptions::default(),
        seed: spec.seed,
    };
    let mut present = Presentation::new(&setup, m.world());
    let skin = c.skin_or_default("olx-skin-default");
    for i in 0..m.world().worms().len() {
        present.add_worm(&skin, default_colour(i), i == 0, false);
    }
    let mut d = Deaths::default();
    for tick in 0..spec.ticks {
        m.advance();
        let (w, ev) = (m.world(), m.events().as_slice());
        let spawned = present.fx.death_spawns();
        present.on_tick(w, m.events());
        // Every death's gibs and blood reach the entities, whatever else
        // fills the pool (its death reserve), counted as they spawn.
        let parts =
            ev.iter().filter(|e| e.death && matches!(e.kind, EventKind::SpawnCosmetic { .. })).count();
        let took = present.fx.death_spawns() - spawned;
        assert_eq!(took, parts as u64, "{} tick {tick}: the deaths' entities spawned", spec.stem());
        assert_eq!(present.fx.death_drops(), 0, "{} tick {tick}: no death's entity dropped", spec.stem());
        d.spawned += took;
        let mut died = Vec::new();
        for (i, e) in ev.iter().enumerate() {
            if let EventKind::Kill { victim, killer } = e.kind {
                let at = format!("{} tick {tick}: worm {victim}'s death", spec.stem());
                assert!(ev.get(i + 1).is_some_and(is_death_sound), "{at}: its sound");
                assert!((i + 2..i + 9).all(|j| ev.get(j).is_some_and(|e| is_gib(e, victim))), "{at}: 7 gibs");
                let blood = ev[i + 9..].iter().take_while(|e| e.death && !is_death_sound(e)).count();
                assert_eq!(blood, 150, "{at}: 50 rounds of blood");
                assert!(ev[i + 1..i + 9 + blood].iter().all(|e| e.death), "{at}: marked as the death's");
                let by_hit =
                    i > 0 && matches!(ev[i - 1].kind, EventKind::Damage { worm, .. } if worm == victim);
                if by_hit {
                    d.hits += 1;
                } else {
                    d.zero += 1;
                    d.zero_uncredited += usize::from(killer == -1);
                }
                died.push(victim);
            }
        }
        let sounds = ev.iter().filter(|e| is_death_sound(e)).count();
        assert_eq!(sounds, died.len(), "{} tick {tick}: a death sound per kill", spec.stem());
    }
    d
}

#[test]
fn every_death_plays_its_sound_and_spawns_its_gibs() {
    let Some(c) = content() else { return };
    let specs = [
        MatchSpec {
            kill_limit: 0,
            time_limit: 0.0,
            ..MatchSpec::new(CLASSIC, "olx-level-dirt-level", 4, 1, 6000)
        },
        MatchSpec {
            personality: Personality::Chaos,
            kill_limit: 0,
            time_limit: 0.0,
            ..MatchSpec::new(MW, "olx-level-castlestrike", 4, 1, 6000)
        },
    ];
    let mut all = Deaths::default();
    for spec in &specs {
        let d = deaths(&c, spec);
        eprintln!("{}: {d:?}", spec.stem());
        all.hits += d.hits;
        all.zero += d.zero;
        all.zero_uncredited += d.zero_uncredited;
        all.spawned += d.spawned;
    }
    assert!(all.hits > 0 && all.zero > 0, "both death paths covered: {all:?}");
    assert_eq!(all.spawned, 157 * (all.hits + all.zero) as u64, "7 gibs and 150 blood a death: {all:?}");
    assert_eq!(all.zero_uncredited, 0, "exact-zero deaths are the last damager's kills: {all:?}");
}
