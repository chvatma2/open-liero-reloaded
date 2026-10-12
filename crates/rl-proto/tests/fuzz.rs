use proptest::prelude::*;
use rl_proto::*;

fn sample_host() -> Datagram {
    Datagram::Host(HostInputs {
        stamp: Stamp { sent_ms: 1, echo_ms: 2, held_ms: 3 },
        tick: 500,
        your_ack: 490,
        margin: -2,
        first: 490,
        frames: (0..10).map(|i| Frame { known: 0xff, changed: 1 << (i % 8), keys: vec![i as u8] }).collect(),
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
fn the_fingerprint_is_set() {
    assert_ne!(FINGERPRINT, 0);
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
}
