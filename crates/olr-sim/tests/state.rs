//! World-state transfer (networking spec §3.5, §3.6): a world built from the
//! same setup takes another world's state and then evolves identically; a
//! corrupted or hostile state is refused, never a panic.

use crate::world;

use std::sync::OnceLock;

use olr_sim::{Keys, Latches, RestoreError, SimSettings, StateError, TickEvents, World, WorldState};
use olr_terrain::JournalError;
use proptest::prelude::*;

fn keys(t: u32) -> Vec<Keys> {
    (0..8u32)
        .map(|i| Keys(if (t + i) % 5 < 3 { Keys::FIRE | Keys::LEFT } else { Keys::UP | Keys::JUMP }))
        .collect()
}

/// Pellets carve, so the state carries terrain edits.
fn played(ticks: u32) -> World {
    let mut w = world::world_carving(8, 400, 300);
    let mut ev = TickEvents::new();
    for t in 0..ticks {
        w.tick(&keys(t), &mut ev);
    }
    w
}

#[test]
fn a_loaded_state_evolves_identically() {
    let src = played(150);
    assert_ne!(src.terrain(), world::world_carving(8, 400, 300).terrain(), "the terrain was edited");
    let bytes = src.state().encode();
    let mut dst = world::world_carving(8, 400, 300);
    dst.load_state(WorldState::decode(&bytes).unwrap()).unwrap();
    assert_eq!(dst.checksum(), src.checksum());
    assert_eq!(dst, src);
    assert_eq!(
        dst.take_restored_areas(),
        [olr_terrain::Area { x0: 0, y0: 0, x1: 400, y1: 300 }],
        "the whole map was replaced"
    );
    let (mut a, mut b) = (src, dst);
    let mut ev = TickEvents::new();
    for t in 150..350 {
        a.tick(&keys(t), &mut ev);
        b.tick(&keys(t), &mut ev);
    }
    assert_eq!(a.checksum(), b.checksum());
}

/// Desync repair: a world that went its own way takes the other's state.
#[test]
fn a_diverged_world_takes_the_state() {
    let src = played(200);
    let mut dst = world::world_carving(8, 400, 300);
    let mut ev = TickEvents::new();
    for t in 0..260 {
        dst.tick(&keys(t + 3), &mut ev);
    }
    dst.load_state(src.state()).unwrap();
    assert_eq!(dst, src);
    assert_eq!(dst.checksum(), src.checksum());
}

/// Every state a match passes through is accepted (validation is never
/// stricter than the simulation).
#[test]
fn every_played_state_is_accepted() {
    for carving in [false, true] {
        let new = || if carving { world::world_carving(8, 400, 300) } else { world::world(8, 400, 300) };
        let mut src = new();
        let mut ev = TickEvents::new();
        for t in 0..600 {
            src.tick(&keys(t), &mut ev);
            if t % 15 == 0 {
                let mut dst = new();
                dst.load_state(src.state()).unwrap_or_else(|e| panic!("tick {t}: {e:?}"));
                assert_eq!(dst, src, "tick {t}");
            }
        }
    }
}

/// The same at the extremes of `SimSettings::validate`'s ranges, where
/// clocks lag furthest: one physics step a second, 200 a second, slow and
/// fast games, and ropes thrown.
#[test]
fn every_state_is_accepted_at_extreme_settings() {
    let lx56 = SimSettings::lx56();
    let variants = [
        (1, 1.0, false),
        (200, 1.0, false),
        (84, 0.1, false),
        (84, 10.0, false),
        (10, 0.1, false),
        (1, 0.1, false),
        (1, 10.0, false),
        (200, 10.0, false),
        (1, 0.1, true),
        (200, 10.0, true),
    ];
    for (fps, speed, only_projs) in variants {
        let settings = SimSettings {
            lx56_physics_fps: fps,
            game_speed: speed,
            game_speed_only_for_projs: only_projs,
            rope_speed: 1000.0,
            ninjarope_precision: 10.0,
            ..lx56.clone()
        };
        for carving in [false, true] {
            let new = || world::world_with(8, 400, 300, carving, settings.clone());
            let mut src = new();
            let mut ev = TickEvents::new();
            for t in 0..400 {
                let mut k = keys(t);
                if t % 40 < 3 {
                    k.iter_mut().for_each(|k| k.0 |= Keys::ROPE);
                }
                src.tick(&k, &mut ev);
                if t % 7 == 0 {
                    let mut dst = new();
                    let what = format!("fps {fps}, speed {speed}, only projs {only_projs}, tick {t}");
                    dst.load_state(src.state()).unwrap_or_else(|e| panic!("{what}: {e:?}"));
                    assert_eq!(dst, src, "{what}");
                }
            }
        }
    }
}

/// Spec §5.3 holds after a load: every buffer kept its capacity.
#[test]
fn a_loaded_world_ticks_without_allocating() {
    let src = played(150);
    let mut dst = world::world_carving(8, 400, 300);
    dst.load_state(src.state()).unwrap();
    let mut ev = TickEvents::new();
    let keys: Vec<Vec<Keys>> = (150..300).map(keys).collect();
    let info = allocation_counter::measure(|| {
        for k in &keys {
            dst.tick(k, &mut ev);
        }
    });
    assert_eq!(info.count_total, 0, "a loaded world's tick allocated");
}

/// Snapshots taken before a load cannot be restored after it: the load is
/// not in the terrain journal.
#[test]
fn a_load_forgets_older_snapshots() {
    let src = played(150);
    let mut dst = world::world_carving(8, 400, 300);
    dst.enable_rollback();
    let before = dst.save();
    dst.load_state(src.state()).unwrap();
    assert!(dst.rollback_enabled());
    assert_eq!(dst.sync_checksum(), src.sync_checksum(), "the cell hash follows the load");
    let loaded = dst.clone();
    assert_eq!(dst.restore(&before), Err(RestoreError::Journal(JournalError::Forgotten)));
    assert_eq!(dst, loaded, "a refused restore changes nothing");
    // Snapshots after the load work as before.
    let after = dst.save();
    let mut ev = TickEvents::new();
    for t in 150..200 {
        dst.tick(&keys(t), &mut ev);
    }
    dst.restore(&after).unwrap();
    assert_eq!(dst, loaded);
}

#[test]
fn a_state_for_another_level_is_refused() {
    let src = played(10);
    let mut dst = world::world_carving(8, 300, 300);
    let before = dst.clone();
    assert_eq!(dst.load_state(src.state()), Err(StateError::Terrain));
    assert_eq!(dst, before, "a refused state changes nothing");
    assert!(dst.take_restored_areas().is_empty());
}

#[test]
fn a_state_with_more_worms_is_refused() {
    let src = played(10);
    let mut dst = world::world_carving(4, 400, 300);
    assert_eq!(dst.load_state(src.state()), Err(StateError::Worms));
}

#[test]
fn garbage_is_refused() {
    assert_eq!(WorldState::decode(&[0x78, 0x9c, 0xff, 0xff]), Err(StateError::Decode));
    assert_eq!(WorldState::from_raw(&[]), Err(StateError::Decode));
}

#[test]
fn a_bomb_is_refused() {
    // 65 MiB of zeros deflate to a few kilobytes.
    let bomb = miniz_oxide::deflate::compress_to_vec(&vec![0u8; WorldState::MAX_INFLATED + (1 << 20)], 6);
    assert_eq!(WorldState::decode(&bomb), Err(StateError::TooLarge));
}

/// A played world's raw state, with where its parts lie (`WorldState`'s
/// field order): past the cells, the worms, latches and clocks come first
/// (`head` bytes, encoded here the same way), the generators, bonuses and
/// the mode last (`TAIL` bytes reach into the generators, whose
/// `Mt19937` alone takes 624 varints), the shots and the projectile pool
/// between.
struct Raw {
    bytes: Vec<u8>,
    carving: bool,
    cells_end: usize,
    head: usize,
}

const TAIL: usize = 4500;

fn raws() -> &'static [Raw] {
    static RAWS: OnceLock<Vec<Raw>> = OnceLock::new();
    RAWS.get_or_init(|| {
        // Carving pellets vanish on impact; bouncing ones fill the pool.
        [(120, true), (250, false)]
            .into_iter()
            .map(|(ticks, carving)| {
                let mut w =
                    if carving { world::world_carving(8, 400, 300) } else { world::world(8, 400, 300) };
                let mut ev = TickEvents::new();
                for t in 0..ticks {
                    w.tick(&keys(t), &mut ev);
                }
                let t = w.terrain();
                let cells_end = postcard::to_allocvec(&(t.width(), t.height(), t.cells())).unwrap().len();
                let n = w.worms().len();
                let head = (
                    n as u64,
                    w.worms().to_vec(),
                    vec![Latches::default(); n],
                    w.time(),
                    w.physics_time(),
                    w.server_frame(),
                    w.substeps_last_tick(),
                );
                let head = postcard::to_allocvec(&head).unwrap();
                let bytes = w.state().to_raw();
                let worms = postcard::to_allocvec(&w.worms().to_vec()).unwrap();
                let at = cells_end + 1; // past the setup's worm count, one varint byte
                assert_eq!(bytes[at..at + worms.len()], worms, "the layout WorldState's fields give");
                Raw { bytes, carving, cells_end, head: head.len() }
            })
            .collect()
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Hostile host: random edits of the raw (uncompressed) encoding.
    #[test]
    fn corrupt_states_never_panic(
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        cut in any::<prop::sample::Index>(),
    ) {
        let mut raw = played(120).state().to_raw();
        for (i, b) in &edits {
            let i = i.index(raw.len());
            raw[i] = *b;
        }
        raw.truncate(raw.len() - cut.index(raw.len() / 8 + 1));
        if let Ok(s) = WorldState::from_raw(&raw) {
            let mut w = world::world_carving(8, 400, 300);
            let before = w.clone();
            if w.load_state(s).is_ok() {
                // Accepted states must tick without panicking.
                let mut ev = TickEvents::new();
                for t in 0..50 {
                    w.tick(&keys(t), &mut ev);
                }
            } else {
                prop_assert_eq!(w, before, "a refused state changes nothing");
            }
        }
    }

    /// Hostile host, aimed past the cells (which the edits above mostly
    /// hit): bytes, words and bit flips in the worms, clocks, shots, pool,
    /// generators, bonuses and mode.
    #[test]
    fn hostile_states_never_panic(
        which in any::<prop::sample::Index>(),
        edits in prop::collection::vec((0u8..3, any::<prop::sample::Index>(), any::<u8>(), 0u8..4), 1..6),
    ) {
        let r = which.get(raws());
        let mut raw = r.bytes.clone();
        let rest = raw.len() - r.cells_end;
        for &(region, i, b, how) in &edits {
            let i = match region {
                0 => r.cells_end + i.index(r.head),
                1 => raw.len() - TAIL + i.index(TAIL),
                _ => r.cells_end + i.index(rest),
            };
            let word = i..(i + 4).min(raw.len());
            match how {
                0 => raw[i] = b,
                1 => raw[word].fill(b),
                2 => raw[word].fill(0xff),
                _ => raw[i] ^= 1 << (b % 8),
            }
        }
        if let Ok(s) = WorldState::from_raw(&raw) {
            let mut w = if r.carving { world::world_carving(8, 400, 300) } else { world::world(8, 400, 300) };
            let before = w.clone();
            if w.load_state(s).is_ok() {
                let mut ev = TickEvents::new();
                for t in 0..30 {
                    w.tick(&keys(t), &mut ev);
                }
            } else {
                prop_assert_eq!(w, before, "a refused state changes nothing");
            }
        }
    }
}

// Issue #86: a state's live part.

fn live_world() -> World {
    world::world_live(8, 400, 300, crate::live_level::live_level())
}

/// A live world played a while, its press moved by a sub-pixel.
fn live_played() -> World {
    let mut w = live_world();
    w.set_live_state(0, 1).unwrap();
    w.set_live_value(1, 0, 3 * olr_sim::LIVE_SUBPIXELS + 5).unwrap();
    let mut ev = TickEvents::new();
    for t in 0..40 {
        w.tick(&keys(t), &mut ev);
    }
    w
}

/// `dst.load_state(s)` is refused as `Live` and changes nothing.
fn refused(mut dst: World, s: WorldState) {
    let before = dst.clone();
    assert_eq!(dst.load_state(s), Err(StateError::Live));
    assert_eq!(dst, before, "a refused state changes nothing");
}

#[test]
fn a_live_part_must_fit_the_receivers_level() {
    let src = live_played();
    // A live part for a world without one, and none for a world with one.
    refused(world::world_carving(8, 400, 300), src.state());
    let mut plain = world::world_carving(8, 400, 300);
    let mut ev = TickEvents::new();
    plain.tick(&keys(0), &mut ev);
    refused(live_world(), plain.state());
    // Another copy count: a level of the first two copies.
    let mut two = (*crate::live_level::live_level()).clone();
    two.copies.truncate(2);
    refused(world::world_live(8, 400, 300, std::sync::Arc::new(two)), src.state());
    // A state out of range, a value outside its limits.
    for (state, value) in [
        (Some((0, 2)), None),
        (Some((2, u16::MAX)), None),
        (None, Some((0, 0x100_0000))),
        (None, Some((2, 21 * olr_sim::LIVE_SUBPIXELS))),
        (None, Some((5, 41))),
        (None, Some((5, -1))),
    ] {
        let mut s = src.state();
        s.live_mut_for_test().as_mut().unwrap().edit_for_test(state, value);
        refused(live_world(), s);
    }
    // Values at their limits are taken.
    let mut s = src.state();
    s.live_mut_for_test().as_mut().unwrap().edit_for_test(Some((2, 1)), Some((5, 40)));
    live_world().load_state(s).unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Hostile host: a live part with any state and value, or dropped.
    #[test]
    fn hostile_live_parts_never_panic(
        state in (0usize..4, any::<u16>()),
        value in (0usize..7, any::<i64>()),
        small in -30i64 * 256..30 * 256,
        drop in 0u8..8,
    ) {
        let src = live_played();
        let mut s = src.state();
        let value = (value.0, if value.1 % 2 == 0 { small } else { value.1 });
        let state = (state.0, state.1 % 4);
        if drop == 0 {
            *s.live_mut_for_test() = None;
        } else {
            s.live_mut_for_test().as_mut().unwrap().edit_for_test(Some(state), Some(value));
        }
        let mut w = live_world();
        let before = w.clone();
        match w.load_state(s) {
            Ok(()) => {
                let mut ev = TickEvents::new();
                for t in 0..10 {
                    w.tick(&keys(t), &mut ev);
                }
                let live = w.live().unwrap();
                for c in live.copies() {
                    let _ = (c.rect(), c.rock_parts().count(), c.state_name());
                }
            }
            Err(e) => {
                prop_assert_eq!(e, StateError::Live);
                prop_assert_eq!(w, before, "a refused state changes nothing");
            }
        }
    }
}
