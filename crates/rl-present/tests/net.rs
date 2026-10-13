//! The presentation under rollback (spec §3.4): a real match (MW 1.0 on
//! Dirt Level, four worms) under `rl_netcode::WorldSim`, driven by a
//! scripted mini-session that predicts three remote worms, learns their
//! final keys a few ticks late and rolls back by hand (restore, re-tick).
//! Sounds and effects come once per event, the colour image follows the
//! terrain after every advance, the confirmed records are the final
//! timeline's. Skips with a notice without `RL_CONTENT_DIR`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rl_content::{Content, LoadedLevel, LoadedMod, LoadedTheme, MatchOptions, Player};
use rl_fx::Kind;
use rl_netcode::world_sim::{ConfirmedTick, TickRecorder, TickedTick};
use rl_netcode::{Simulation, TickInfo, WorldSim};
use rl_present::net::{EventKey, NetPresenter, event_keys};
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_scene::{SceneOptions, Viewport};
use rl_sim::{CosmeticKind, Event, EventKind, Keys, SoundId, StampId, TerrainOp};

fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

const MOD: &str = "olx-mw-1-0";
const LEVEL: &str = "olx-level-dirt-level";
const WORMS: usize = 4;

struct Packs {
    m: LoadedMod,
    level: LoadedLevel,
    theme: LoadedTheme,
}

fn packs(c: &Content) -> Packs {
    let level = c.load_level(LEVEL).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    Packs { m: c.load_mod(MOD).unwrap(), level, theme }
}

fn world(c: &Content) -> rl_sim::World {
    let players = (0..WORMS).map(|_| Player::Bot(Default::default())).collect();
    c.build_match(&MatchOptions::deathmatch(MOD, LEVEL, 7, players)).unwrap().world
}

fn presentation(c: &Content, p: &Packs, world: &rl_sim::World) -> Presentation {
    let setup = PresentSetup {
        core: c.core(),
        m: &p.m,
        level: &p.level,
        theme: &p.theme,
        viewport: Viewport::GAME,
        options: SceneOptions::default(),
        seed: 1,
    };
    let mut present = Presentation::new(&setup, world);
    let skin = c.skin_or_default("olx-skin-default");
    for i in 0..WORMS {
        present.add_worm(&skin, default_colour(i), i == 0, i == 0);
    }
    present
}

/// Every worm's final keys at tick `t`: firing, and walking or aiming in
/// turns, each worm on its own period, so a prediction (the last final
/// keys) is wrong now and then.
fn final_keys(t: u32) -> Vec<Keys> {
    (0..WORMS as u32)
        .map(|i| {
            let phase = t / (5 + 2 * i) + i;
            Keys(match phase % 4 {
                0 => Keys::FIRE | Keys::RIGHT,
                1 => Keys::FIRE | Keys::UP,
                2 => Keys::LEFT,
                _ => Keys::FIRE | Keys::DOWN,
            })
        })
        .collect()
}

/// The keys a peer playing worm 0 uses at tick `t` when the ticks below
/// `confirmed` are final: its own, and the remote worms' last final keys.
fn predicted_keys(t: u32, confirmed: u32) -> Vec<Keys> {
    if t < confirmed {
        return final_keys(t);
    }
    let last = confirmed.checked_sub(1).map(final_keys);
    let mine = final_keys(t);
    (0..WORMS).map(|i| if i == 0 { mine[0] } else { last.as_ref().map_or(Keys(0), |l| l[i]) }).collect()
}

/// A scripted peer: a world under rollback, its tick records and its
/// presentation, fed the way the online game feeds them.
struct Peer {
    sim: WorldSim,
    rec: TickRecorder,
    present: Presentation,
    net: NetPresenter,
    /// The snapshot before each tick from the confirmed tick on.
    snaps: BTreeMap<u32, rl_sim::Snapshot>,
    /// The keys each tick was last simulated with.
    used: Vec<Vec<Keys>>,
    /// The next tick to simulate, and the ticks below `confirmed` are final.
    current: u32,
    confirmed: u32,
}

impl Peer {
    /// Simulates tick `t` with the keys known now, then reads it as the
    /// online driver's `on_tick` does: the scene's history and marks.
    fn tick(&mut self, t: u32, resim: bool) {
        self.snaps.insert(t, self.sim.save());
        let keys = predicted_keys(t, self.confirmed);
        self.sim.tick(&keys);
        if self.used.len() <= t as usize {
            self.used.push(keys);
        } else {
            self.used[t as usize] = keys;
        }
        let (world, events) = (self.sim.world(), self.sim.events());
        if resim {
            self.present.scene.after_tick_resimulated(world, events);
        } else {
            self.present.scene.after_tick(world, events);
        }
        self.rec.simulated(&self.sim, TickInfo { tick: t, resim, all_final: false });
    }

    /// One advance: the keys below `confirmed` became final; a tick
    /// simulated with other keys rolls back to it, then the new ticks up to
    /// `target` run. Returns the restored areas, the ticked records and the
    /// confirmed ones.
    fn advance(
        &mut self,
        target: u32,
        confirmed: u32,
    ) -> (Vec<rl_terrain::Area>, Vec<TickedTick>, Vec<ConfirmedTick>) {
        let wrong =
            (self.confirmed..confirmed.min(self.current)).find(|&t| self.used[t as usize] != final_keys(t));
        self.confirmed = confirmed;
        if let Some(w) = wrong {
            self.sim.restore(&self.snaps[&w]);
            for t in w..self.current {
                self.tick(t, true);
            }
        }
        for t in self.current..target {
            self.tick(t, false);
        }
        self.current = target;
        self.snaps = self.snaps.split_off(&self.confirmed);
        if let Some(s) = self.snaps.get(&self.confirmed) {
            self.sim.forget_before(s);
        }
        let restored = self.sim.take_restored_areas();
        (restored, self.rec.take_ticked(), self.rec.take_confirmed(self.confirmed))
    }
}

const TICKS: u32 = 400;
/// Ticks per advance, and how late the remote keys become final.
const FRAME: u32 = 3;
const LAG: u32 = 10;

/// The ticks the final inputs give, simulated once each (no prediction).
fn final_timeline(c: &Content) -> (Vec<TickedTick>, Vec<ConfirmedTick>) {
    let mut sim = WorldSim::new(world(c));
    let mut rec = TickRecorder::default();
    for t in 0..TICKS {
        sim.tick(&final_keys(t));
        rec.simulated(&sim, TickInfo { tick: t, resim: false, all_final: true });
    }
    (rec.take_ticked(), rec.take_confirmed(TICKS))
}

fn keyed(ticks: &[TickedTick]) -> impl Iterator<Item = (EventKey, Event)> + '_ {
    ticks.iter().flat_map(|t| {
        event_keys(t.tick, &t.events)
            .into_iter()
            .zip(t.events.iter().copied())
            .filter_map(|(k, e)| Some((k?, e)))
    })
}

/// An entity a spawn event makes (beams take no slot); blood also drips
/// from blood droppers, so it is left out of the count.
fn counted_spawn(e: &Event) -> bool {
    matches!(e.kind, EventKind::SpawnCosmetic { kind, .. }
        if !matches!(kind, CosmeticKind::Beam { .. } | CosmeticKind::Blood { .. }))
}

#[test]
fn rollbacks_play_each_sound_once_and_keep_the_terrain() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let w = world(&c);
    let present = presentation(&c, &p, &w);
    let mut peer = Peer {
        sim: WorldSim::new(w),
        rec: TickRecorder::default(),
        present,
        net: NetPresenter::new(),
        snaps: BTreeMap::new(),
        used: Vec::new(),
        current: 0,
        confirmed: 0,
    };
    let (final_ticked, final_confirmed) = final_timeline(&c);

    // What the test expects, from every simulation the peer made.
    let mut played: BTreeSet<EventKey> = BTreeSet::new();
    let mut spawn_keys: BTreeSet<EventKey> = BTreeSet::new();
    let mut spawned_keys: BTreeSet<EventKey> = BTreeSet::new();
    let (mut naive_sounds, mut naive_spawns, mut resims) = (0usize, 0usize, 0usize);
    let mut confirmed: Vec<ConfirmedTick> = Vec::new();
    let mut sounds = Vec::new();
    while peer.confirmed < TICKS {
        let target = (peer.current + FRAME).min(TICKS);
        let conf = if target == TICKS { TICKS } else { target.saturating_sub(LAG) };
        let (restored, ticked, done) = peer.advance(target, conf);
        sounds.clear();
        let spawned = peer.net.spawned();
        peer.net.apply(&mut peer.present, peer.sim.world(), &restored, &ticked, &mut sounds);
        // (b) the colour image's material is the world's after every advance.
        assert!(!peer.present.terrain_drift(peer.sim.world()), "tick {}: drift", peer.current);
        assert_eq!(peer.net.drifts(), 0, "tick {}: the presenter saw no drift", peer.current);
        // (a) the sounds whose key is new, in order, never one twice.
        let (mut want, mut new_spawns) = (Vec::new(), 0);
        for (k, e) in keyed(&ticked) {
            match e.kind {
                EventKind::Sound { .. } => {
                    naive_sounds += 1;
                    if played.insert(k) {
                        want.push(e);
                    }
                }
                EventKind::SpawnCosmetic { .. } => {
                    new_spawns += u64::from(spawned_keys.insert(k));
                    if counted_spawn(&e) {
                        naive_spawns += 1;
                        spawn_keys.insert(k);
                    }
                }
                _ => {}
            }
        }
        assert_eq!(sounds, want, "tick {}: the sounds to play now", peer.current);
        // (c) each spawn key reaches the entities once, in the advance it
        // first appears; and no more entities than distinct spawns, erased
        // ones included.
        assert_eq!(peer.net.spawned() - spawned, new_spawns, "tick {}: spawns", peer.current);
        let live = peer.present.fx.entities().filter(|(_, e)| e.kind != Kind::Blood).count();
        assert!(
            live <= spawn_keys.len(),
            "tick {}: {live} entities, {} spawns",
            peer.current,
            spawn_keys.len()
        );
        resims += ticked.iter().filter(|t| t.resim).count();
        peer.net.confirmed(peer.confirmed);
        confirmed.extend(done);
    }
    assert!(resims > 0, "the script rolls back");

    // (a) every final sound played; the erased ones too, each once.
    let final_sounds: BTreeSet<EventKey> = keyed(&final_ticked)
        .filter(|(_, e)| matches!(e.kind, EventKind::Sound { .. }))
        .map(|(k, _)| k)
        .collect();
    assert!(final_sounds.is_subset(&played), "every sound of the final timeline played");
    assert!(played.len() > final_sounds.len(), "a correction erased sounds that had played");
    assert!(
        naive_sounds > played.len(),
        "re-simulations repeated sounds: {naive_sounds} vs {}",
        played.len()
    );
    assert!(naive_spawns > spawn_keys.len(), "re-simulations repeated spawns");

    // (d) confirmed records once per tick, in order, the final timeline's.
    assert_eq!(confirmed.len(), TICKS as usize);
    for (i, (got, want)) in confirmed.iter().zip(&final_confirmed).enumerate() {
        assert_eq!(got.tick, i as u32, "once per tick, in order");
        assert_eq!(got, want, "tick {i}");
    }
    assert!(final_confirmed.iter().any(|t| !t.events.is_empty()), "something happened");
}

/// A loaded state (a repair, a late spectator): the presentation starts
/// over from it, the whole-map restored area repaints the colour image, and
/// the ticks shown before the load and simulated again after it play no
/// sound twice, whether the driver resets before or after applying them.
#[test]
fn a_loaded_state_restarts_the_presentation() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    // The state after tick 59 of the same inputs (a repair that matches).
    let mut a = WorldSim::new(world(&c));
    for t in 0..60 {
        a.tick(&final_keys(t));
    }
    let state = a.encode_state();
    for reset_first in [true, false] {
        let w = world(&c);
        let mut present = presentation(&c, &p, &w);
        let mut b = WorldSim::new(w);
        let mut net = NetPresenter::new();
        let mut rec = TickRecorder::default();
        let mut sounds = Vec::new();
        // Ticks 0..80 shown, as one advance.
        for t in 0..80 {
            b.tick(&final_keys(t));
            present.scene.after_tick(b.world(), b.events());
            rec.simulated(&b, TickInfo { tick: t, resim: false, all_final: true });
        }
        let ticked = rec.take_ticked();
        let later = ticked[60..].iter().flat_map(|t| &t.events);
        assert!(
            later.filter(|e| matches!(e.kind, EventKind::Sound { .. })).count() > 0,
            "sounds after the state"
        );
        net.apply(&mut present, b.world(), &[], &ticked, &mut sounds);
        // The state loads; ticks 60..80 run again.
        b.decode_state(&state).unwrap();
        rec.state_loaded(59);
        let loaded = |present: &mut Presentation, net: &mut NetPresenter, b: &WorldSim| {
            present.state_loaded(b.world());
            net.reset(59);
        };
        if reset_first {
            loaded(&mut present, &mut net, &b);
        }
        for t in 60..80 {
            b.tick(&final_keys(t));
            present.scene.after_tick_resimulated(b.world(), b.events());
            rec.simulated(&b, TickInfo { tick: t, resim: true, all_final: true });
        }
        sounds.clear();
        let restored = b.take_restored_areas();
        net.apply(&mut present, b.world(), &restored, &rec.take_ticked(), &mut sounds);
        if !reset_first {
            loaded(&mut present, &mut net, &b);
        }
        assert!(!present.terrain_drift(b.world()));
        assert_eq!(sounds, [], "reset first: {reset_first}: no sound twice");
        // The next tick is new.
        b.tick(&final_keys(80));
        rec.simulated(&b, TickInfo { tick: 80, resim: false, all_final: true });
        net.apply(&mut present, b.world(), &[], &rec.take_ticked(), &mut sounds);
        assert!(!present.terrain_drift(b.world()));
    }
}

/// Keys at the confirmed tick survive [`NetPresenter::confirmed`], those
/// below it go: a rollback starting at the confirmed tick plays nothing
/// again.
#[test]
fn keys_from_the_confirmed_tick_on_survive() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut sim = WorldSim::new(world(&c));
    let mut present = presentation(&c, &p, sim.world());
    let mut rec = TickRecorder::default();
    sim.tick(&final_keys(0));
    rec.simulated(&sim, TickInfo { tick: 0, resim: false, all_final: true });
    let mut net = NetPresenter::new();
    let mut sounds = Vec::new();
    net.apply(&mut present, sim.world(), &[], &rec.take_ticked(), &mut sounds);
    // Hand-made ticks with a sound each and no terrain edit.
    let beep = ev(0, EventKind::Sound { id: SoundId(3), pos: rl_math::Vec2::new(10.0, 10.0) });
    let tick = |tick, resim| TickedTick { tick, resim, substeps: 0, lost: None, events: vec![beep] };
    sounds.clear();
    net.apply(&mut present, sim.world(), &[], &[tick(1, false), tick(2, false)], &mut sounds);
    assert_eq!(sounds.len(), 2);
    net.confirmed(2);
    sounds.clear();
    net.apply(&mut present, sim.world(), &[], &[tick(2, true), tick(3, false)], &mut sounds);
    assert_eq!(sounds, [beep], "tick 2 is kept, tick 3 is new");
    sounds.clear();
    net.apply(&mut present, sim.world(), &[], &[tick(1, true)], &mut sounds);
    assert_eq!(sounds, [beep], "tick 1's keys went with the confirmation");
}

fn ev(substep: u8, kind: EventKind) -> Event {
    Event { substep, death: false, kind }
}

/// Keys leave out what a corrected re-simulation moves (positions,
/// velocities, amounts) and count equal events in a tick.
#[test]
fn event_keys_ignore_floats_and_count_equal_events() {
    use rl_math::Vec2;
    let sound = |id, x| EventKind::Sound { id: SoundId(id), pos: Vec2::new(x, 1.0) };
    let damage = |amount| EventKind::Damage { worm: 1, amount, by: 2, dealt: amount };
    let edit = EventKind::TerrainEdit { op: TerrainOp::Carve, stamp: StampId::Hole(1), x: 3, y: 4 };
    let a = event_keys(9, &[ev(0, sound(7, 1.0)), ev(0, edit), ev(0, sound(7, 5.0)), ev(1, sound(7, 1.0))]);
    let b = event_keys(9, &[ev(0, sound(7, 2.0)), ev(0, sound(7, 6.0)), ev(1, sound(7, 3.0))]);
    assert_eq!(a[1], None, "terrain edits are never deduped");
    let a: Vec<EventKey> = a.into_iter().flatten().collect();
    let b: Vec<EventKey> = b.into_iter().flatten().collect();
    assert_eq!(a, b, "moved sounds keep their keys");
    assert_eq!((a[0].tick, a[0].substep, a[0].ordinal), (9, 0, 0));
    assert_eq!((a[1].substep, a[1].ordinal), (0, 1), "the second equal sound");
    assert_eq!((a[2].substep, a[2].ordinal), (1, 0), "another sub-step counts anew");
    assert_eq!(a[0].class, a[2].class);
    let other = event_keys(9, &[ev(0, sound(8, 1.0)), ev(0, damage(3.0))]);
    assert_ne!(other[0].unwrap().class, a[0].class, "another sound");
    assert_eq!(other[1], event_keys(9, &[ev(0, damage(40.0))])[0], "damage amounts are left out");
    assert_ne!(event_keys(10, &[ev(0, sound(7, 1.0))])[0].unwrap(), a[0], "another tick");
    let gib = |worm| EventKind::SpawnCosmetic {
        kind: CosmeticKind::Gib { worm },
        pos: Vec2::ZERO,
        vel: Vec2::ZERO,
    };
    assert_ne!(
        event_keys(9, &[ev(0, gib(1))])[0],
        event_keys(9, &[ev(0, gib(2))])[0],
        "a cosmetic's worm counts"
    );
}

/// A drifted colour terrain after an advance (a presentation bug) is
/// counted and logged, not a panic: debug builds keep playing.
#[test]
fn a_drift_is_counted_not_a_panic() {
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut w = world(&c);
    let mut present = presentation(&c, &p, &w);
    let mut net = NetPresenter::new();
    let mut events = rl_sim::TickEvents::new();
    w.tick(&[Keys(0); WORMS], &mut events);
    let tick = |tick, events: &rl_sim::TickEvents| TickedTick {
        tick,
        resim: false,
        substeps: 1,
        lost: None,
        events: events.iter().copied().collect(),
    };
    net.apply(&mut present, &w, &[], &[tick(0, &events)], &mut Vec::new());
    assert_eq!(net.drifts(), 0);
    // Dirt where a worm stands, which the world never placed.
    let at = w.worms()[0].pos;
    present.color_mut().apply(rl_terrain::ColorEdit::Dirt { hole: 0, x: at.x as i32, y: at.y as i32 });
    w.tick(&[Keys(0); WORMS], &mut events);
    net.apply(&mut present, &w, &[], &[tick(1, &events)], &mut Vec::new());
    assert!(present.terrain_drift(&w));
    if cfg!(debug_assertions) {
        assert_eq!(net.drifts(), 1, "counted (checked in debug builds)");
    }
}

/// A death as the simulation emits it in sub-step 1: the kill, then the
/// death's own events (`Event::death`): the death sound `id` (one of
/// three, drawn), worm `worm`'s 7 gibs and a round of its blood.
fn death(id: u16, worm: i32) -> Vec<Event> {
    use rl_math::Vec2;
    let cosmetic = |kind| EventKind::SpawnCosmetic { kind, pos: Vec2::ZERO, vel: Vec2::ZERO };
    let part = |kind| Event { substep: 1, death: true, kind };
    let mut v = vec![
        ev(1, EventKind::Kill { victim: worm, killer: 0 }),
        part(EventKind::Sound { id: SoundId(id), pos: Vec2::ZERO }),
    ];
    v.extend((0..7).map(|_| part(cosmetic(CosmeticKind::Gib { worm }))));
    v.extend(
        [CosmeticKind::BloodDropper, CosmeticKind::Blood { red: 200 }, CosmeticKind::Blood { red: 128 }]
            .map(|k| part(cosmetic(k))),
    );
    v
}

/// The cosmetics in [`death`]: 7 gibs and 3 blood.
const DEATH_SPAWNS: u64 = 10;

/// A death's sound is keyed by the worm whose gibs follow it, not by which
/// of the three death sounds it drew: a correction that re-rolls the sound
/// plays no second one, and one that kills another worm instead plays that
/// death's sound and spawns its gibs (neither is deduped away).
#[test]
fn every_death_plays_its_sound_and_gibs_once_under_corrections() {
    assert!(SoundId::DEATH.contains(&SoundId(3)) && SoundId::DEATH.contains(&SoundId(4)));
    let key = |events: &[Event]| event_keys(5, events)[1].unwrap();
    assert_eq!(key(&death(3, 1)), key(&death(4, 1)), "a re-rolled death sound");
    assert_ne!(key(&death(3, 1)), key(&death(3, 2)), "another worm's death");

    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut sim = WorldSim::new(world(&c));
    let mut present = presentation(&c, &p, sim.world());
    let mut rec = TickRecorder::default();
    sim.tick(&final_keys(0));
    rec.simulated(&sim, TickInfo { tick: 0, resim: false, all_final: true });
    let mut net = NetPresenter::new();
    let mut sounds = Vec::new();
    net.apply(&mut present, sim.world(), &[], &rec.take_ticked(), &mut sounds);
    let tick = |resim, events: Vec<Event>| TickedTick { tick: 1, resim, substeps: 2, lost: None, events };
    let mut step = |resim, events: Vec<Event>| {
        let (spawned, mut sounds) = (net.spawned(), Vec::new());
        net.apply(&mut present, sim.world(), &[], &[tick(resim, events)], &mut sounds);
        (sounds.len(), net.spawned() - spawned)
    };
    assert_eq!(step(false, death(3, 1)), (1, DEATH_SPAWNS), "worm 1 dies: its sound, its gibs");
    assert_eq!(step(true, death(4, 1)), (0, 0), "the same death, its sound re-rolled");
    assert_eq!(step(true, death(3, 2)), (1, DEATH_SPAWNS), "worm 2 dies instead");
}

/// A death is one per worm and life, whatever tick a correction moves it
/// to: it shows once whether it moves to an earlier tick (shown before) or
/// a later one (never shown), and the worm's next life dies anew.
#[test]
fn a_death_moved_to_another_tick_by_a_correction_shows_once() {
    use rl_math::Vec2;
    let Some(c) = content() else { return };
    let p = packs(&c);
    let mut sim = WorldSim::new(world(&c));
    let mut present = presentation(&c, &p, sim.world());
    let mut rec = TickRecorder::default();
    sim.tick(&final_keys(0));
    rec.simulated(&sim, TickInfo { tick: 0, resim: false, all_final: true });
    let first = rec.take_ticked();
    let spawned = |w| first[0].events.iter().any(|e| e.kind == EventKind::Spawned { worm: w });
    assert!((1..=3).all(spawned), "tick 0 spawns the worms");
    let mut net = NetPresenter::new();
    net.apply(&mut present, sim.world(), &[], &first, &mut Vec::new());
    let mut step = |tick, events: Vec<Event>| {
        let (spawned, mut sounds) = (net.spawned(), Vec::new());
        let t = TickedTick { tick, resim: false, substeps: 2, lost: None, events };
        net.apply(&mut present, sim.world(), &[], &[t], &mut sounds);
        (sounds.len(), net.spawned() - spawned)
    };
    assert_eq!(step(2, death(3, 1)), (1, DEATH_SPAWNS), "worm 1 dies at tick 2");
    assert_eq!(step(1, death(4, 1)), (0, 0), "a correction: it died at tick 1, shown before");
    assert_eq!(step(2, vec![]), (0, 0), "and not at tick 2");
    assert_eq!(step(3, death(5, 2)), (1, DEATH_SPAWNS), "worm 2 dies at tick 3");
    assert_eq!(step(3, vec![]), (0, 0), "a correction: not at tick 3");
    assert_eq!(step(4, death(3, 2)), (0, 0), "but at tick 4, never shown before");
    let spawn = ev(0, EventKind::Spawned { worm: 1 });
    let mut later = vec![spawn, ev(0, EventKind::Sound { id: SoundId::THROW, pos: Vec2::ZERO })];
    later.extend(death(3, 1));
    assert_eq!(step(5, later), (2, DEATH_SPAWNS), "worm 1's next life dies anew");
}
