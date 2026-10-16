//! Snapshots (networking spec §3.3): restoring and re-simulating gives the
//! straight run's world exactly, terrain included, and the sync checksum
//! tracks the state like `checksum` does.

use crate::world;

use olr_sim::{Keys, RestoreError, TickEvents, World};
use olr_terrain::{Area, JournalError};

/// Fires constantly with a moving aim, so pellets fly and carve.
fn keys(t: u32, n: usize) -> Vec<Keys> {
    (0..n)
        .map(|i| {
            let mut k = Keys::FIRE;
            if (t / 7 + i as u32).is_multiple_of(2) {
                k |= Keys::UP
            } else {
                k |= Keys::DOWN
            }
            if (t / 31 + i as u32).is_multiple_of(3) {
                k |= Keys::LEFT
            }
            Keys(k)
        })
        .collect()
}

fn run(w: &mut World, worms: usize, from: u32, to: u32, ev: &mut TickEvents) {
    for t in from..to {
        w.tick(&keys(t, worms), ev);
    }
}

#[test]
fn restore_then_resimulate_matches_the_straight_run() {
    let mut ev = TickEvents::new();
    let mut straight = world::world_carving(8, 400, 300);
    run(&mut straight, 8, 0, 300, &mut ev);

    let mut w = world::world_carving(8, 400, 300);
    w.enable_rollback();
    run(&mut w, 8, 0, 100, &mut ev);
    let snap = w.save();
    // A different future, then back.
    for _ in 100..180 {
        w.tick(&[Keys(Keys::FIRE | Keys::JUMP); 8], &mut ev);
    }
    w.restore(&snap).unwrap();
    assert_eq!(w.server_frame(), snap.server_frame());
    run(&mut w, 8, 100, 300, &mut ev);
    assert_eq!(w.checksum(), straight.checksum());
    assert_eq!(w, straight);
}

#[test]
fn restore_undoes_terrain_edits() {
    let mut ev = TickEvents::new();
    let mut w = world::world_carving(8, 400, 300);
    w.enable_rollback();
    run(&mut w, 8, 0, 50, &mut ev);
    let before = w.terrain().clone();
    let snap = w.save();
    run(&mut w, 8, 50, 250, &mut ev);
    assert_ne!(w.terrain(), &before, "the fixture must carve");
    w.restore(&snap).unwrap();
    assert_eq!(w.terrain(), &before);
}

/// The presentation's colour map follows the ticks' terrain edits, so it
/// learns from `take_restored_areas` what a restore put back.
#[test]
fn restore_reports_every_cell_it_put_back() {
    let mut ev = TickEvents::new();
    let mut w = world::world_carving(8, 400, 300);
    w.enable_rollback();
    run(&mut w, 8, 0, 50, &mut ev);
    w.take_restored_areas();
    let before = w.terrain().clone();
    let snap = w.save();
    run(&mut w, 8, 50, 250, &mut ev);
    let carved = w.terrain().clone();
    assert!(w.take_restored_areas().is_empty(), "ticks restore nothing");
    w.restore(&snap).unwrap();
    let areas = w.take_restored_areas();
    let covered =
        |x: i32, y: i32| areas.iter().any(|a| (a.x0..a.x1).contains(&x) && (a.y0..a.y1).contains(&y));
    let mut changed = 0;
    for y in 0..300 {
        for x in 0..400 {
            if carved.material(x, y) != before.material(x, y) {
                changed += 1;
                assert!(covered(x as i32, y as i32), "({x}, {y}) was put back unreported");
            }
        }
    }
    assert!(changed > 0, "the fixture must carve");
    assert!(areas.len() <= olr_terrain::Terrain::MAX_RESTORED);
    assert!(w.take_restored_areas().is_empty(), "taken once");
    // Nothing edited since the snapshot: nothing to report.
    let snap = w.save();
    w.restore(&snap).unwrap();
    assert!(w.take_restored_areas().is_empty());
}

#[test]
fn snapshots_without_rollback_copy_the_terrain() {
    let mut ev = TickEvents::new();
    let mut w = world::world_carving(4, 200, 150);
    run(&mut w, 4, 0, 20, &mut ev);
    let snap = w.save();
    let at = w.clone();
    run(&mut w, 4, 20, 120, &mut ev);
    w.restore(&snap).unwrap();
    assert_eq!(w, at);
    // The copy replaced the whole map.
    assert_eq!(w.take_restored_areas(), [Area { x0: 0, y0: 0, x1: 200, y1: 150 }]);
}

#[test]
fn sync_checksum_follows_the_state() {
    let mut ev = TickEvents::new();
    let mut a = world::world_carving(4, 200, 150);
    a.enable_rollback();
    let mut b = world::world_carving(4, 200, 150);
    assert_eq!(a.sync_checksum(), b.sync_checksum(), "tracked and scratch hashes agree");
    run(&mut a, 4, 0, 60, &mut ev);
    assert_ne!(a.sync_checksum(), b.sync_checksum());
    run(&mut b, 4, 0, 60, &mut ev);
    assert_eq!(a.sync_checksum(), b.sync_checksum());
    b.set_health(1, 3.0);
    assert_ne!(a.sync_checksum(), b.sync_checksum());
}

#[test]
fn forgetting_keeps_newer_snapshots_usable() {
    let mut ev = TickEvents::new();
    let mut w = world::world_carving(8, 400, 300);
    w.enable_rollback();
    let old = w.save();
    run(&mut w, 8, 0, 40, &mut ev);
    let newer = w.save();
    let at = w.clone();
    run(&mut w, 8, 40, 90, &mut ev);
    w.forget_before(&newer);
    assert!(w.restore(&old).is_err());
    w.restore(&newer).unwrap();
    assert_eq!(w, at);
}

#[test]
fn restoring_an_older_snapshot_invalidates_newer_ones() {
    let mut ev = TickEvents::new();
    let mut w = world::world_carving(8, 400, 300);
    w.enable_rollback();
    run(&mut w, 8, 0, 30, &mut ev);
    let old = w.save();
    run(&mut w, 8, 30, 80, &mut ev);
    let newer = w.save();
    run(&mut w, 8, 80, 120, &mut ev);
    w.restore(&old).unwrap();
    let at = w.clone();
    assert_eq!(w.restore(&newer), Err(RestoreError::Journal(JournalError::Stale)));
    assert_eq!(w, at, "a refused restore changes nothing");
    // Even once the journal has regrown past the stale mark.
    run(&mut w, 8, 30, 200, &mut ev);
    assert_eq!(w.restore(&newer), Err(RestoreError::Journal(JournalError::Stale)));
}
