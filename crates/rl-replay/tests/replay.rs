//! Recording and replaying a synthetic Deathmatch: the replay rebuilds the
//! same world, reproduces every checksum, reports the first mismatch, and
//! its encoding is compact, versioned and safe to decode.

mod common;

use common::*;
use rl_replay::{CHECKSUM_EVERY, DecodeError, FORMAT_VERSION, Mismatch, Recorder, Replay, SetupCall};
use rl_sim::{BonusItem, Keys, TickEvents, World};

/// Records `ticks` ticks of a 4-worm Deathmatch (with setup calls made
/// through the recorder, and rows of varying width); gives the replay, the
/// world as it was before the first tick and the world at the end.
fn record(ticks: u32, seed: u32) -> (Replay, World, World) {
    let (setup, mode) = deathmatch(4, seed);
    let mut world = World::new(setup.clone(), mode.game_mode()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    rec.apply(&mut world, SetupCall::AlignPhysicsClock).unwrap();
    rec.apply(&mut world, SetupCall::SetHealth { worm: 1, health: 30.0 }).unwrap();
    rec.apply(&mut world, SetupCall::PlaceBonus { x: 160, y: 100, item: BonusItem::Weapon(1) }).unwrap();
    rec.apply(&mut world, SetupCall::FindSpot).unwrap();
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
    (rec.finish(&world), start, world)
}

#[test]
fn record_then_replay_gives_identical_checksums() {
    let (replay, start, end) = record(1050, 7);
    assert_eq!(replay.ticks(), 1050);
    let ticks: Vec<u32> = replay.checksums.iter().map(|c| c.0).collect();
    let mut expected: Vec<u32> = (1..=10).map(|i| i * CHECKSUM_EVERY).collect();
    expected.push(1050);
    assert_eq!(ticks, expected, "every 100 ticks, and the end");

    let decoded = Replay::decode(&replay.encode()).unwrap();
    assert_eq!(decoded, replay, "encoding round-trips");
    let world = decoded.world(assets()).unwrap();
    assert_eq!(world, start, "the replay rebuilds the world, setup calls included");
    let run = decoded.play(world).unwrap();
    assert_eq!((run.ticks, run.checked), (1050, 11));
    assert_eq!(run.world.checksum(), end.checksum());
    assert_eq!(run.world, end);
}

#[test]
fn replay_reports_the_first_mismatching_tick() {
    let (replay, _, _) = record(600, 3);
    // A wrong checksum is found where it is.
    let mut bad = replay.clone();
    bad.checksums[2].1 ^= 1;
    let err = bad.play(bad.world(assets()).unwrap()).unwrap_err();
    assert_eq!(err.tick, 300);
    assert_eq!(err, Mismatch { tick: 300, expected: bad.checksums[2].1, actual: replay.checksums[2].1 });
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
    let mut world = World::new(setup.clone(), mode.game_mode()).unwrap();
    let mut rec = Recorder::new(packs(), &setup, &mode);
    assert!(rec.apply(&mut world, SetupCall::SetHealth { worm: 2, health: 1.0 }).is_err());
    assert!(rec.apply(&mut world, SetupCall::PlaceBonus { x: 1, y: 1, item: BonusItem::Weapon(9) }).is_err());
    let mut out = TickEvents::new();
    world.tick(&[], &mut out);
    rec.push(&[], &world).unwrap();
    assert!(rec.apply(&mut world, SetupCall::AlignPhysicsClock).is_err(), "after the start");
    // A tick the recorder missed.
    world.tick(&[], &mut out);
    world.tick(&[], &mut out);
    assert!(rec.push(&[], &world).is_err(), "out of step");
    let mut replay = rec.finish(&world);
    replay.setup.calls.push(SetupCall::SetHealth { worm: 5, health: 1.0 });
    assert!(replay.world(assets()).is_err());
}

/// One minute (6000 ticks) of 8 worms encodes in at most 50 000 bytes
/// whatever the keys (one byte per worm and tick, plus the setup and the
/// checksums), and in far less when keys are held like players hold them.
#[test]
fn replay_size_is_reasonable() {
    let size = |hold: u32| {
        let (setup, mode) = deathmatch(8, 2);
        let mut world = World::new(setup.clone(), mode.game_mode()).unwrap();
        let mut rec = Recorder::new(packs(), &setup, &mode);
        let mut keys = KeyGen::new(9, 8, hold);
        let mut out = TickEvents::new();
        for _ in 0..6000 {
            let row = keys.row();
            world.tick(&row, &mut out);
            rec.push(&row, &world).unwrap();
        }
        let replay = rec.finish(&world);
        assert_eq!(replay.checksums.len(), 60);
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
