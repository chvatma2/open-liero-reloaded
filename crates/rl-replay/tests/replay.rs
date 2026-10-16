//! Recording and replaying a synthetic Deathmatch: the replay rebuilds the
//! same world, reproduces every checksum, reports the first mismatch, and
//! its encoding is compact, versioned and safe to decode.

use crate::common;

use common::*;
use rl_replay::{
    CHECKSUM_EVERY, DecodeError, FORMAT_VERSION, Mismatch, ModeSpec, RecordError, Recorder, Replay,
    ReplayError, SetupCall,
};
use rl_sim::{BonusItem, Keys, TickEvents, World};

/// Records `ticks` ticks of a 4-worm Deathmatch (with setup calls made
/// through the recorder, and rows of varying width); gives the replay, the
/// world as it was before the first tick and the world at the end.
fn record(ticks: u32, seed: u32) -> (Replay, World, World) {
    let (setup, mode) = deathmatch(4, seed);
    let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    rec.apply(&mut world, SetupCall::AlignPhysicsClock).unwrap();
    rec.apply(&mut world, SetupCall::SetHealth { worm: 1, health: 30.0 }).unwrap();
    rec.apply(&mut world, SetupCall::PlaceBonus { x: 160, y: 100, item: BonusItem::Weapon(1) }).unwrap();
    rec.apply(&mut world, SetupCall::FindSpot).unwrap();
    rec.start(&world).unwrap();
    let start = world.clone();
    let mut keys = KeyGen::new(u64::from(seed), 4, 12);
    let mut out = TickEvents::new();
    for t in 0..ticks {
        let row = keys.row();
        // Short rows are worms pressing nothing: the replay must agree.
        let row = &row[..[4, 4, 3, 0][t as usize % 4]];
        world.tick(row, &mut out);
        rec.push(row, &world).unwrap();
    }
    (rec.finish(&world).unwrap(), start, world)
}

#[test]
fn record_then_replay_gives_identical_checksums() {
    let (replay, start, end) = record(1050, 7);
    assert_eq!(replay.ticks(), 1050);
    let ticks: Vec<u32> = replay.checksums.iter().map(|c| c.0).collect();
    let mut expected: Vec<u32> = (0..=10).map(|i| i * CHECKSUM_EVERY).collect();
    expected.push(1050);
    assert_eq!(ticks, expected, "before the first tick, every 100 ticks, and the end");

    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded, replay, "encoding round-trips");
    let world = decoded.world(assets()).unwrap();
    assert_eq!(world, start, "the replay rebuilds the world, setup calls included");
    let run = decoded.play(world).unwrap();
    assert_eq!((run.ticks, run.checked), (1050, 12));
    assert_eq!(run.world.checksum(), end.checksum());
    assert_eq!(run.world, end);
}

/// A withdrawal is recorded at its tick and replayed there.
#[test]
fn a_withdrawal_replays_at_its_tick() {
    let (setup, mode) = deathmatch(4, 9);
    let new_world = || World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut world = new_world();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    assert_eq!(rec.withdraw(&mut world, 1), Err(RecordError::NotStarted));
    rec.start(&world).unwrap();
    let mut keys = KeyGen::new(9, 4, 12);
    let mut out = TickEvents::new();
    for t in 0..250u32 {
        if t == 120 {
            rec.withdraw(&mut world, 1).unwrap();
        }
        let row = keys.row();
        world.tick(&row, &mut out);
        rec.push(&row, &world).unwrap();
    }
    assert_eq!(rec.withdraw(&mut world, 1), Err(RecordError::NoWorm(1)), "gone already");
    assert_eq!(rec.withdraw(&mut world, 40), Err(RecordError::NoWorm(40)));
    let failed = rec.clone().finish_failed(&world, &[Keys(0); 4]).unwrap();
    assert_eq!(failed.withdrawals, [(120, 1)], "a failed match keeps them");
    let replay = rec.finish(&world).unwrap();
    assert_eq!(replay.withdrawals, [(120, 1)]);
    let back = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(back, replay);
    let run = back.play(new_world()).unwrap();
    assert_eq!(run.world.gone(), 0b10);
    // Without the withdrawal the checksums no longer match.
    let mut cut = back.clone();
    cut.withdrawals.clear();
    assert!(cut.play(new_world()).is_err());
}

/// A withdrawal after the last tick stays valid and replays exactly, with
/// or without a periodic checksum at the last tick.
#[test]
fn a_withdrawal_after_the_last_tick_replays() {
    let (setup, mode) = deathmatch(4, 4);
    let new_world = || World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    for ticks in [150u32, 200] {
        let mut world = new_world();
        let mut rec = Recorder::new(packs(), &setup, &mode);
        rec.start(&world).unwrap();
        let mut keys = KeyGen::new(4, 4, 12);
        let mut out = TickEvents::new();
        for _ in 0..ticks {
            let row = keys.row();
            world.tick(&row, &mut out);
            rec.push(&row, &world).unwrap();
        }
        rec.withdraw(&mut world, 2).unwrap();
        let replay = rec.finish(&world).unwrap();
        assert_eq!(replay.withdrawals, [(ticks, 2)]);
        assert_eq!(replay.checksums.last(), Some(&(ticks, world.checksum())));
        let back = Replay::decode(&replay.encode()).unwrap();
        assert_eq!(back, replay);
        let run = back.play(new_world()).unwrap();
        assert_eq!(run.world.gone(), 0b100, "{ticks}");
        assert_eq!(run.world.checksum(), world.checksum());
    }
    // No ticks at all.
    let mut world = new_world();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    rec.start(&world).unwrap();
    rec.withdraw(&mut world, 0).unwrap();
    let back = Replay::decode(&rec.finish(&world).unwrap().encode()).unwrap();
    assert_eq!(back.play(new_world()).unwrap().world.gone(), 1);
}

/// Final review A-M1: a failing tick after a trailing withdrawal. The
/// withdrawal is no longer after the last tick (the failing one follows),
/// so it is played before that tick, and the checksum at it is the world's
/// before it: the replay plays into the failure without a false `Mismatch`.
#[test]
fn a_failing_tick_after_a_trailing_withdrawal_replays() {
    let (setup, mode) = deathmatch(4, 4);
    let new_world = || World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let failing = [Keys(Keys::FIRE), Keys(Keys::LEFT)];
    for ticks in [0u32, 150, 200] {
        let mut world = new_world();
        let mut rec = Recorder::new(packs(), &setup, &mode);
        rec.start(&world).unwrap();
        let mut keys = KeyGen::new(4, 4, 12);
        let mut out = TickEvents::new();
        for _ in 0..ticks {
            let row = keys.row();
            world.tick(&row, &mut out);
            rec.push(&row, &world).unwrap();
        }
        rec.withdraw(&mut world, 2).unwrap();
        let replay = rec.finish_failed(&world, &failing).unwrap();
        assert_eq!(replay.withdrawals, [(ticks, 2)]);
        let back = Replay::decode(&replay.encode()).unwrap();
        let run = back.play(new_world()).unwrap_or_else(|m| panic!("{ticks}: {m:?}"));
        world.tick(&failing, &mut out);
        assert_eq!(run.world, world, "{ticks}");
    }
}

/// Stepping a `Player` across a withdrawal withdraws at its tick, not before.
#[test]
fn stepping_across_a_withdrawal() {
    let (setup, mode) = deathmatch(4, 9);
    let new_world = || World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut world = new_world();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    rec.start(&world).unwrap();
    let mut keys = KeyGen::new(9, 4, 12);
    let mut out = TickEvents::new();
    for t in 0..150u32 {
        if t == 120 {
            rec.withdraw(&mut world, 1).unwrap();
        }
        let row = keys.row();
        world.tick(&row, &mut out);
        rec.push(&row, &world).unwrap();
    }
    let replay = rec.finish(&world).unwrap();
    let mut w = new_world();
    let mut player = replay.player();
    while player.tick() < 120 {
        assert!(player.step(&mut w, &mut out).unwrap());
    }
    assert_eq!(w.gone(), 0, "not before its tick");
    assert!(player.step(&mut w, &mut out).unwrap());
    assert_eq!((player.tick(), w.gone()), (121, 0b10), "withdrawn right before tick 120 is played");
    while player.step(&mut w, &mut out).unwrap() {}
    assert_eq!(w.checksum(), world.checksum());
}

#[test]
fn withdrawals_are_checked_on_decode() {
    let base = record(5, 2).0;
    for bad in [
        vec![(base.ticks() + 1, 0)],
        vec![(2, 0), (1, 1)],
        vec![(1, 0), (2, 0)],
        vec![(1, 40)],
        (0..33).map(|i| (0, i as u8)).collect(),
    ] {
        let mut r = base.clone();
        r.withdrawals = bad.clone();
        assert!(matches!(Replay::decode(&r.encode()), Err(DecodeError::Invalid(_))), "{bad:?}");
    }
}

#[test]
fn replay_reports_the_first_mismatching_tick() {
    let (replay, _, _) = record(600, 3);
    // A wrong checksum is found where it is.
    let mut bad = replay.clone();
    bad.checksums[3].1 ^= 1;
    let err = bad.play(bad.world(assets()).unwrap()).unwrap_err();
    assert_eq!(err.tick, 300);
    assert_eq!(err, Mismatch { tick: 300, expected: bad.checksums[3].1, actual: replay.checksums[3].1 });
    // A changed key shows at the next checksum.
    let mut bad = replay.clone();
    for t in 150..160 {
        bad.row_mut(t)[0] = Keys(Keys::LEFT | Keys::FIRE);
    }
    let err = bad.play(bad.world(assets()).unwrap()).unwrap_err();
    assert_eq!(err.tick, 200, "{err}");
    // Stepping stops at the same tick.
    let mut world = bad.world(assets()).unwrap();
    let mut out = TickEvents::new();
    let mut player = bad.player();
    while player.step(&mut world, &mut out).unwrap_or(false) {}
    assert_eq!(player.tick(), 200);
}

/// The slot of the player whose game made the replay is recorded and
/// survives the encoding; a slot that is no worm is dropped when
/// recording, and none (bots alone) stays none.
#[test]
fn the_viewer_is_recorded() {
    let (setup, mode) = deathmatch(4, 6);
    let make = |viewer: Option<u8>| {
        let world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
        let mut rec = Recorder::new(packs(), &setup, &mode);
        rec.set_viewer(viewer);
        rec.start(&world).unwrap();
        Replay::decode(&rec.finish(&world).unwrap().encode()).unwrap()
    };
    assert_eq!(make(None).setup.viewer, None);
    assert_eq!(make(Some(2)).setup.viewer, Some(2));
    assert_eq!(make(Some(4)).setup.viewer, None, "worm 4 of 4 does not exist");
}

/// Review Focus 3: a replay of another format version is refused, never
/// replayed into a silent divergence.
#[test]
fn replay_refuses_unknown_format_versions() {
    let (replay, _, _) = record(120, 5);
    let bytes = replay.encode();
    assert_eq!(&bytes[..4], b"RLRP");
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), FORMAT_VERSION);
    let mut other = bytes.clone();
    other[4..6].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
    let err = Replay::decode(&other).unwrap_err();
    assert_eq!(err, DecodeError::Version { found: FORMAT_VERSION + 1, supported: FORMAT_VERSION });
    assert!(err.to_string().contains("version"), "{err}");
    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert_eq!(Replay::decode(&magic).unwrap_err(), DecodeError::Magic);
    assert!(Replay::decode(&bytes[..bytes.len() - 1]).is_err(), "truncated");
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Replay::decode(&longer).is_err(), "trailing bytes");
}

/// Setup calls a world cannot take are errors when recording and replaying.
#[test]
fn bad_setup_calls_are_errors() {
    let (setup, mode) = deathmatch(2, 1);
    let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    assert!(rec.apply(&mut world, SetupCall::SetHealth { worm: 2, health: 1.0 }).is_err());
    assert!(rec.apply(&mut world, SetupCall::PlaceBonus { x: 1, y: 1, item: BonusItem::Weapon(9) }).is_err());
    rec.start(&world).unwrap();
    assert_eq!(
        rec.apply(&mut world, SetupCall::AlignPhysicsClock),
        Err(RecordError::Started),
        "after the start"
    );
    let mut out = TickEvents::new();
    world.tick(&[], &mut out);
    rec.push(&[], &world).unwrap();
    let mut replay = rec.finish(&world).unwrap();
    replay.setup.calls.push(SetupCall::SetHealth { worm: 5, health: 1.0 });
    assert!(matches!(replay.world(assets()), Err(ReplayError::NoWorm { index: 0, worm: 5 })));
}

/// The recorder sees every tick of the world it records: a tick played
/// before the first push or after the last is an error, never a replay
/// that silently diverges.
#[test]
fn recorder_refuses_ticks_it_did_not_see() {
    let (setup, mode) = deathmatch(2, 1);
    let new = || {
        (
            World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap(),
            Recorder::new(packs(), &setup, &mode),
        )
    };
    let mut out = TickEvents::new();

    let (mut world, mut rec) = new();
    world.tick(&[], &mut out);
    assert_eq!(rec.push(&[], &world), Err(RecordError::NotStarted));
    assert_eq!(rec.clone().finish(&world).unwrap_err(), RecordError::NotStarted);
    rec.start(&world).unwrap();
    assert_eq!(rec.start(&world), Err(RecordError::Started));

    // Ticks played between the start and the first push.
    let (mut world, mut rec) = new();
    rec.start(&world).unwrap();
    world.tick(&[], &mut out);
    world.tick(&[], &mut out);
    assert!(matches!(rec.push(&[], &world), Err(RecordError::OutOfStep { recorded: 0, .. })));

    // A tick played after the last push.
    let (mut world, mut rec) = new();
    rec.start(&world).unwrap();
    for _ in 0..3 {
        world.tick(&[], &mut out);
        rec.push(&[], &world).unwrap();
    }
    assert_eq!(rec.clone().finish(&world).unwrap().ticks(), 3);
    world.tick(&[], &mut out);
    assert!(matches!(rec.finish(&world), Err(RecordError::OutOfStep { recorded: 3, .. })));
}

/// A replay whose world differs before its first tick (a setup call
/// missing, one made outside the recorder, another seed) is reported at
/// tick 0, not as a divergence of the simulation.
#[test]
fn replay_reports_a_different_setup_at_tick_0() {
    let (replay, _, _) = record(150, 4);
    let mut missing = replay.clone();
    missing.setup.calls.retain(|c| !matches!(c, SetupCall::SetHealth { .. }));
    let err = missing.play(missing.world(assets()).unwrap()).unwrap_err();
    assert_eq!(err.tick, 0);
    assert!(err.to_string().contains("setup differs"), "{err}");
    let mut seed = replay.clone();
    seed.setup.seed ^= 1;
    assert_eq!(seed.play(seed.world(assets()).unwrap()).unwrap_err().tick, 0);

    // A setup call made on the world directly, not through the recorder.
    let (setup, mode) = deathmatch(2, 1);
    let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    world.set_health(1, 40.0);
    rec.start(&world).unwrap();
    let mut out = TickEvents::new();
    for _ in 0..50 {
        world.tick(&[], &mut out);
        rec.push(&[], &world).unwrap();
    }
    let replay = rec.finish(&world).unwrap();
    let err = replay.play(replay.world(assets()).unwrap()).unwrap_err();
    assert_eq!(err.tick, 0);
    let mut player = replay.player();
    let mut world = replay.world(assets()).unwrap();
    assert_eq!(player.step(&mut world, &mut out).unwrap_err().tick, 0, "the player checks tick 0 first");
}

/// One minute (6000 ticks) of 8 worms encodes in at most 50 000 bytes
/// whatever the keys (one byte per worm and tick, plus the setup and the
/// checksums), and in far less when keys are held like players hold them.
#[test]
fn replay_size_is_reasonable() {
    let size = |hold: u32| {
        let (setup, mode) = deathmatch(8, 2);
        let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
        let mut rec = Recorder::new(packs(), &setup, &mode);
        rec.start(&world).unwrap();
        let mut keys = KeyGen::new(9, 8, hold);
        let mut out = TickEvents::new();
        for _ in 0..6000 {
            let row = keys.row();
            world.tick(&row, &mut out);
            rec.push(&row, &world).unwrap();
        }
        let replay = rec.finish(&world).unwrap();
        assert_eq!(replay.checksums.len(), 61);
        replay.encode().len()
    };
    let random = size(1);
    let held = size(20);
    eprintln!("one minute of 8 worms: {random} bytes with new keys every tick, {held} held 1-20 ticks");
    assert!(random <= 50_000, "{random} bytes");
    assert!(held <= 16_000, "{held} bytes");
}

mod corrupt {
    use super::*;
    use proptest::prelude::*;

    type Setter = fn(&mut Replay, f32);

    fn dm(r: &mut Replay) -> &mut rl_modes::DeathmatchSettings {
        match &mut r.mode {
            ModeSpec::Deathmatch(d) => d,
            ModeSpec::Oracle => unreachable!("the sample is a Deathmatch"),
        }
    }

    /// Floats of the settings and of the mode.
    const SETTERS: [Setter; 10] = [
        |r, v| r.setup.settings.game_speed = v,
        |r, v| r.setup.settings.worm_gravity = v,
        |r, v| r.setup.settings.worm_jump_force = v,
        |r, v| r.setup.settings.rope_strength = v,
        |r, v| r.setup.settings.respawn_time = v,
        |r, v| r.setup.settings.bonus_frequency = v,
        |r, v| r.setup.settings.worm_damage_factor = v,
        |r, v| r.setup.settings.death_decreases_score = v,
        |r, v| dm(r).death_decreases_score = v,
        |r, v| dm(r).time_limit = v,
    ];

    fn sample() -> Vec<u8> {
        static BYTES: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
        BYTES.get_or_init(|| record(230, 11).0.encode()).clone()
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

        /// Flipped bytes, truncation or junk: decoding never panics, and
        /// what decodes builds a world or gives an error.
        #[test]
        fn decode_rejects_corrupt_input(
            edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6),
            cut in any::<prop::sample::Index>(),
            truncate in any::<bool>(),
        ) {
            let mut bytes = sample();
            for (at, b) in edits {
                let i = at.index(bytes.len());
                bytes[i] = b;
            }
            if truncate {
                bytes.truncate(cut.index(bytes.len()));
            }
            if let Ok(r) = Replay::decode(&bytes) {
                prop_assert!(r.ticks() <= rl_replay::MAX_TICKS);
                let _ = r.world(assets());
            }
        }

        /// Settings that are not numbers are refused by decoding, and by
        /// building a world from a replay made in code.
        #[test]
        fn non_finite_settings_are_refused(
            field in 0..SETTERS.len(),
            value in prop_oneof![Just(f32::NAN), Just(f32::INFINITY), Just(f32::NEG_INFINITY)],
        ) {
            let mut r = Replay::decode(&sample()).unwrap();
            SETTERS[field](&mut r, value);
            prop_assert!(matches!(Replay::decode(&r.encode()), Err(DecodeError::Invalid(_))));
            prop_assert!(r.world(assets()).is_err());
        }

        /// Any bytes after a good header.
        #[test]
        fn decode_never_panics_on_junk(body in prop::collection::vec(any::<u8>(), 0..600)) {
            let mut bytes = b"RLRP".to_vec();
            bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
            bytes.extend_from_slice(&body);
            let _ = Replay::decode(&bytes);
        }
    }
}

/// A match whose tick failed (a panic inside `World::tick` leaves the world
/// broken) is saved with the failing tick's keys and no checksum after
/// them: the replay checks every tick before and then plays the failing
/// one, so it reproduces the failure (spec §7.3).
#[test]
fn a_failing_tick_is_recorded_without_a_checksum() {
    let (setup, mode) = deathmatch(4, 9);
    let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    rec.start(&world).unwrap();
    let mut keys = KeyGen::new(9, 4, 12);
    let mut out = TickEvents::new();
    for _ in 0..250 {
        let row = keys.row();
        world.tick(&row, &mut out);
        rec.push(&row, &world).unwrap();
    }
    let failing = [Keys(Keys::FIRE), Keys(Keys::LEFT)];
    let replay = rec.clone().finish_failed(&world, &failing).unwrap();
    assert_eq!(replay.ticks(), 251);
    assert_eq!(replay.row(250), [Keys(Keys::FIRE), Keys(Keys::LEFT), Keys(0), Keys(0)]);
    assert_eq!(replay.checksums.last().unwrap(), &(250, world.checksum()), "none after the failing tick");
    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded, replay);
    let run = decoded.play(decoded.world(assets()).unwrap()).unwrap();
    assert_eq!((run.ticks, run.checked), (251, 4));
    let mut after = world.clone();
    after.tick(&failing, &mut out);
    assert_eq!(run.world, after);

    // Failing before any tick: rows as wide as the failing keys.
    let mut world = World::new(setup.clone(), mode.game_mode(&setup.settings).unwrap()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    rec.start(&world).unwrap();
    let replay = rec.finish_failed(&world, &failing).unwrap();
    assert_eq!((replay.ticks(), replay.width()), (1, 2));
    world.tick(&failing, &mut out);
    assert_eq!(replay.play(replay.world(assets()).unwrap()).unwrap().world, world);
}
