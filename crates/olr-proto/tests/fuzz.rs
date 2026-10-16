use olr_proto::*;
use proptest::prelude::*;

fn sample_host() -> Datagram {
    Datagram::Host(HostInputs {
        stamp: Stamp { sent_ms: 1, echo_ms: 2, held_ms: 3 },
        tick: 500,
        your_ack: 490,
        margin: -2,
        first: 490,
        frames: (0..10).map(|i| Frame { known: 0xff, changed: 1 << (i % 8), keys: vec![i as u8] }).collect(),
        gone: vec![(1, 492), (5, 499)],
    })
}

/// Withdrawal marks for a message whose frames begin at `first`: mostly
/// near its frames, with slots past any match, repeats and lists past
/// `MAX_SLOTS` (decision N83).
fn marks(first: u32) -> impl Strategy<Value = Vec<(u8, u32)>> {
    let tick = prop_oneof![
        (0u32..16).prop_map(move |k| first.wrapping_add(k)),
        Just(first.wrapping_sub(1)),
        any::<u32>(),
    ];
    let mark = (prop_oneof![0u8..40, any::<u8>()], tick);
    prop_oneof![
        Just(vec![]),
        prop::collection::vec(mark.clone(), 0..6),
        prop::collection::vec(mark, MAX_SLOTS..=MAX_SLOTS + 2),
    ]
}

fn host_datagram() -> impl Strategy<Value = Datagram> {
    let edge = prop_oneof![Just(0u32), Just(1), Just(u32::MAX), Just(u32::MAX - 5), any::<u32>()];
    let frame = (any::<u32>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..4))
        .prop_map(|(known, changed, keys)| Frame { known, changed, keys });
    let quiet = Frame { known: 0, changed: 0, keys: vec![] };
    let frames = prop_oneof![
        prop::collection::vec(frame, 0..8),
        (0..=MAX_FRAMES + 1).prop_map(move |n| vec![quiet.clone(); n])
    ];
    (edge, frames, any::<bool>(), any::<u32>()).prop_flat_map(|(first, frames, host, tick)| {
        marks(first).prop_map(move |gone| {
            let frames = frames.clone();
            if host {
                Datagram::Host(HostInputs {
                    stamp: Stamp::default(),
                    tick,
                    your_ack: 0,
                    margin: 0,
                    first,
                    frames,
                    gone,
                })
            } else {
                Datagram::Resume(HostResume { tick, at_ms: 0, first, frames, gone })
            }
        })
    })
}

#[test]
fn datagrams_and_controls_round_trip() {
    let d = sample_host();
    assert_eq!(decode_datagram(&encode_datagram(&d), 8), Ok(d));
    let c = Control::StateChunk { index: 3, bytes: vec![7; 100] };
    assert_eq!(decode_control(&encode_control(&c)), Ok(c));
}

#[test]
fn limits_are_enforced() {
    assert_eq!(decode_datagram(&vec![0u8; MAX_DATAGRAM + 1], 32), Err(ProtoError::TooLong));
    let big = Control::StateChunk { index: 0, bytes: vec![0; STATE_CHUNK + 1] };
    assert_eq!(decode_control(&encode_control(&big)), Err(ProtoError::Invalid));
    let many = Datagram::Client(ClientInputs {
        stamp: Stamp::default(),
        tick: 0,
        ack: 0,
        first: 0,
        keys: vec![0; MAX_KEYS + 1],
    });
    assert_eq!(decode_datagram(&encode_datagram(&many), 32), Err(ProtoError::Invalid));
}

#[test]
fn a_late_range_is_never_empty() {
    let late = |from, to| decode_control(&encode_control(&Control::Late { from, to }));
    assert_eq!(late(5, 9), Ok(Control::Late { from: 5, to: 9 }));
    assert_eq!(late(5, 5), Err(ProtoError::Invalid));
    assert_eq!(late(u32::MAX, 0), Err(ProtoError::Invalid));
}

#[test]
fn the_fingerprint_is_set() {
    assert_ne!(FINGERPRINT, 0);
}

fn migration_control() -> impl Strategy<Value = Control> {
    let edge = prop_oneof![Just(0u32), Just(1), Just(u32::MAX), Just(u32::MAX - 1), any::<u32>()];
    let frame = (any::<u32>(), any::<u32>(), prop::collection::vec(any::<u8>(), 0..4))
        .prop_map(|(known, changed, keys)| Frame { known, changed, keys });
    let frames = prop_oneof![
        prop::collection::vec(frame, 0..8),
        Just(vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES - 1]),
        Just(vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES]),
        Just(vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES + 1]),
    ];
    let own = prop_oneof![
        prop::collection::vec(any::<u8>(), 0..8),
        Just(vec![1u8; MAX_REPORT_OWN]),
        Just(vec![1u8; MAX_REPORT_OWN + 1]),
    ];
    prop_oneof![
        prop::collection::vec(edge.clone(), 0..=MAX_SLOTS + 1).prop_map(|known| Control::Collect { known }),
        (edge.clone(), edge.clone(), edge.clone(), frames.clone()).prop_flat_map(
            |(tick, at_ms, first, frames)| {
                marks(first).prop_map(move |gone| Control::Resume {
                    tick,
                    at_ms,
                    first,
                    frames: frames.clone(),
                    gone,
                })
            }
        ),
        (edge.clone(), edge.clone(), edge.clone(), frames, edge, own).prop_flat_map(
            |(ack, origin, first, frames, own_first, own)| {
                marks(first).prop_map(move |gone| {
                    let (frames, own) = (frames.clone(), own.clone());
                    Control::Report(InputsReport { ack, origin, first, frames, gone, own_first, own })
                })
            }
        ),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn decode_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..2000)) {
        let _ = decode_datagram(&bytes, 32);
        let _ = decode_control(&bytes);
    }

    #[test]
    fn mutated_datagrams_never_panic(edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..6)) {
        let mut b = encode_datagram(&sample_host());
        for (i, v) in &edits { let i = i.index(b.len()); b[i] = *v; }
        if let Ok(Datagram::Host(h)) = decode_datagram(&b, 8) {
            let _ = decode_frames(h.first, &h.frames, 8, &|_| Some(0), &mut |_, _, _| {});
        }
    }

    #[test]
    fn host_datagrams_with_marks_never_panic(
        d in host_datagram(),
        slots in prop_oneof![Just(1usize), Just(4), Just(8), Just(32)],
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..4),
    ) {
        let mut b = encode_datagram(&d);
        for (i, v) in &edits { let i = i.index(b.len()); b[i] = *v; }
        if let Ok(m) = decode_datagram(&b, slots) {
            let (first, len, gone) = match &m {
                Datagram::Host(h) => (h.first, h.frames.len(), &h.gone),
                Datagram::Resume(r) => (r.first, r.frames.len(), &r.gone),
                Datagram::Client(_) => (0, 0, &Vec::new()),
            };
            prop_assert!(valid_marks(first, len, gone, slots), "accepted marks are valid");
            prop_assert_eq!(decode_datagram(&encode_datagram(&m), slots), Ok(m));
        }
    }

    #[test]
    fn mutated_migration_controls_never_panic(
        c in migration_control(),
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..4),
    ) {
        let mut b = encode_control(&c);
        for (i, v) in &edits { let i = i.index(b.len()); b[i] = *v; }
        if let Ok(m) = decode_control(&b) {
            if let Control::Report(r) = &m {
                prop_assert!(r.origin <= r.ack, "a report's origin is bounded by its ack");
                prop_assert!(valid_marks(r.first, r.frames.len(), &r.gone, MAX_SLOTS));
            }
            if let Control::Resume { first, frames, gone, .. } = &m {
                prop_assert!(valid_marks(*first, frames.len(), gone, MAX_SLOTS));
            }
            prop_assert_eq!(decode_control(&encode_control(&m)), Ok(m));
        }
    }
}
