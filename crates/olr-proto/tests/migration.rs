//! The migration wire (spec §3.7, plan 12c): round trips, bounds, worst cases.

use olr_proto::*;

fn report(frames: usize, own: usize) -> InputsReport {
    let f =
        encode_frames(100, frames as u32, 32, &|t, s| Some(((t as usize * 7 + s) % 251) as u8), &|_| Some(0));
    InputsReport {
        ack: 90,
        origin: 40,
        first: 100,
        frames: f,
        gone: vec![],
        own_first: 120,
        own: vec![3; own],
    }
}

#[test]
fn migration_controls_round_trip() {
    for c in [
        Control::Collect { known: vec![0, 5, u32::MAX - 1] },
        Control::Report(report(64, 10)),
        Control::Resume {
            tick: 12_345,
            at_ms: 77_000,
            first: 12_300,
            frames: report(45, 0).frames,
            gone: vec![],
        },
        Control::Resume { tick: 7, at_ms: 0, first: 7, frames: vec![], gone: vec![] },
    ] {
        assert_eq!(decode_control(&encode_control(&c)), Ok(c));
    }
}

#[test]
fn migration_controls_out_of_bounds_are_refused() {
    let bad = [
        Control::Collect { known: vec![] },
        Control::Collect { known: vec![0; MAX_SLOTS + 1] },
        Control::Report(InputsReport { own: vec![0; MAX_REPORT_OWN + 1], ..report(1, 0) }),
        Control::Report(InputsReport { first: u32::MAX, ..report(2, 0) }),
        Control::Report(InputsReport { own_first: u32::MAX, ..report(1, 2) }),
        Control::Report(InputsReport { origin: 91, ..report(1, 0) }),
        Control::Report(InputsReport {
            frames: vec![Frame { known: 1, changed: 2, keys: vec![1] }],
            ..report(0, 0)
        }),
        Control::Report(InputsReport {
            frames: vec![Frame { known: 3, changed: 3, keys: vec![1] }],
            ..report(0, 0)
        }),
        Control::Resume {
            tick: 0,
            at_ms: u32::MAX,
            first: u32::MAX,
            frames: report(2, 0).frames,
            gone: vec![],
        },
        Control::Resume {
            tick: 0,
            at_ms: u32::MAX,
            first: 0,
            frames: vec![Frame { known: 1, changed: 2, keys: vec![1] }],
            gone: vec![],
        },
        Control::Resume {
            tick: 0,
            at_ms: u32::MAX,
            first: 0,
            frames: vec![Frame { known: 3, changed: 3, keys: vec![1] }],
            gone: vec![],
        },
        Control::Resume {
            tick: 0,
            at_ms: 1,
            first: 0,
            frames: vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES + 1],
            gone: vec![],
        },
    ];
    for c in bad {
        assert_eq!(decode_control(&encode_control(&c)), Err(ProtoError::Invalid), "{c:?}");
    }
    let mut long = report(1, 0);
    long.frames = vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES + 1];
    assert_eq!(decode_control(&encode_control(&Control::Report(long))), Err(ProtoError::Invalid));
    // The bounds themselves are accepted.
    let mut edge = report(1, MAX_REPORT_OWN);
    edge.frames = vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES];
    edge.origin = edge.ack;
    let c = Control::Report(edge);
    assert_eq!(decode_control(&encode_control(&c)), Ok(c));
    let c = Control::Collect { known: vec![7; MAX_SLOTS] };
    assert_eq!(decode_control(&encode_control(&c)), Ok(c));
    let frames = vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_REPORT_FRAMES];
    let c = Control::Resume {
        tick: 0,
        at_ms: u32::MAX,
        first: u32::MAX - MAX_REPORT_FRAMES as u32,
        frames,
        gone: vec![],
    };
    assert_eq!(decode_control(&encode_control(&c)), Ok(c));
}

/// A relay URL of exactly `MAX_RELAY_URL` bytes, unique per `n` and `i`.
fn long_relay(n: u8, i: u8) -> String {
    let head = format!("wss://{n}-{i}.example/");
    format!("{head}{}", "r".repeat(MAX_RELAY_URL - head.len()))
}

fn successor(n: u8) -> SuccessorWire {
    SuccessorWire {
        member: n,
        key: [n; 32],
        direct: Some(([10, 0, 0, n], 40_000 + n as u16)),
        relays: vec![long_relay(n, 0), long_relay(n, 1)],
        can_host: true,
    }
}

#[test]
fn a_worst_case_succession_fits_and_round_trips() {
    let list: Vec<SuccessorWire> = (0..MAX_MEMBERS as u8).map(successor).collect();
    for s in &list {
        assert!(s.relays.iter().all(|r| r.len() == MAX_RELAY_URL && valid_relay_url(r)));
    }
    let bytes = encode_to_client(&ToClient::Succession(list.clone()));
    assert!(bytes.len() <= MAX_LOBBY, "{} bytes", bytes.len());
    assert_eq!(decode_to_client(&bytes), Ok(ToClient::Succession(list)));
}

#[test]
fn successions_and_standbys_out_of_bounds_are_refused() {
    let mut twice = vec![successor(1), successor(1)];
    assert!(!valid_succession(&twice));
    twice[1].member = 2; // the same key under two ids
    assert!(!valid_succession(&twice));
    let low_port = SuccessorWire { direct: Some(([10, 0, 0, 1], 80)), ..successor(1) };
    let three = SuccessorWire { relays: vec!["wss://a.example".into(); 3], ..successor(1) };
    let not_ws = SuccessorWire { relays: vec!["https://a.example".into()], ..successor(1) };
    let spaced = SuccessorWire { relays: vec!["wss://a b.example".into()], ..successor(1) };
    let long = SuccessorWire { relays: vec![format!("wss://{}", "a".repeat(MAX_RELAY_URL))], ..successor(1) };
    let bare = SuccessorWire { relays: vec!["ws://".into()], ..successor(1) };
    let bare_s = SuccessorWire { relays: vec!["wss://".into()], ..successor(1) };
    let direct = |a: [u8; 4], p| SuccessorWire { direct: Some((a, p)), ..successor(1) };
    for s in [
        low_port,
        three,
        not_ws,
        spaced,
        long,
        bare,
        bare_s,
        direct([0, 0, 0, 0], 40_000),
        direct([255, 255, 255, 255], 40_000),
        direct([224, 0, 0, 1], 40_000),
        direct([10, 0, 0, 1], 0),
    ] {
        let bytes = encode_to_client(&ToClient::Succession(vec![s.clone()]));
        assert_eq!(decode_to_client(&bytes), Err(ProtoError::Invalid), "{s:?}");
    }
    // A member id is any `u8` (ids count up through a lobby's life, N64).
    assert!(valid_succession(&[successor(MAX_MEMBERS as u8), successor(255)]));
    let too_many: Vec<SuccessorWire> = (0..=MAX_MEMBERS as u8).map(successor).collect();
    assert_eq!(
        decode_to_client(&encode_to_client(&ToClient::Succession(too_many))),
        Err(ProtoError::Invalid)
    );
    for bad in [
        ToHost::Standby { port: 1023, relays: vec![] },
        ToHost::Standby { port: 40_000, relays: vec!["wss://a.example".into(); 3] },
        ToHost::Standby { port: 40_000, relays: vec!["file:///etc/passwd".into()] },
    ] {
        assert_eq!(decode_to_host(&encode_to_host(&bad)), Err(ProtoError::Invalid), "{bad:?}");
    }
    let ok = ToHost::Standby { port: 40_000, relays: vec!["wss://a.example".into()] };
    assert_eq!(decode_to_host(&encode_to_host(&ok)), Ok(ok));
    let ok = ToHost::Standby { port: MIN_STANDBY_PORT, relays: vec![] };
    assert_eq!(decode_to_host(&encode_to_host(&ok)), Ok(ok));
}

#[test]
fn the_new_refusals_round_trip() {
    for r in [Refusal::NotHost, Refusal::Closed] {
        let s = Signal::Refused { reason: r };
        assert_eq!(decode_signal(&encode_signal(&s)), Ok(s));
        let c = ToClient::Refused(r);
        assert_eq!(decode_to_client(&encode_to_client(&c)), Ok(c));
    }
}

#[test]
fn loopback_and_private_successors_are_accepted() {
    for a in [[127, 0, 0, 1], [192, 168, 1, 5]] {
        let s = SuccessorWire { direct: Some((a, 40_000)), ..successor(1) };
        assert!(valid_succession(&[s]));
    }
    assert!(valid_succession(&[SuccessorWire { member: MAX_MEMBERS as u8 - 1, ..successor(1) }]));
}

#[test]
fn the_resume_datagram_round_trips_within_its_bounds() {
    let frames = encode_frames(100, MAX_FRAMES as u32, 4, &|t, s| Some((t as usize + s) as u8), &|_| Some(0));
    let d = Datagram::Resume(HostResume { tick: 120, at_ms: 9_000, first: 100, frames, gone: vec![] });
    let b = encode_datagram(&d);
    assert!(b.len() <= MAX_DATAGRAM, "{} bytes", b.len());
    assert_eq!(decode_datagram(&b, 4), Ok(d));
    let long = vec![Frame { known: 0, changed: 0, keys: vec![] }; MAX_FRAMES + 1];
    let d = Datagram::Resume(HostResume { tick: 0, at_ms: 0, first: 0, frames: long, gone: vec![] });
    assert_eq!(decode_datagram(&encode_datagram(&d), 4), Err(ProtoError::Invalid));
    // A slot beyond the match's.
    let wide = vec![Frame { known: 1 << 4, changed: 1 << 4, keys: vec![1] }];
    let d = Datagram::Resume(HostResume { tick: 0, at_ms: 0, first: 0, frames: wide, gone: vec![] });
    assert_eq!(decode_datagram(&encode_datagram(&d), 4), Err(ProtoError::Invalid));
}

/// N83: the marks travel once per message, within its frames, each slot once.
#[test]
fn withdrawal_marks_are_bounded() {
    assert!(valid_marks(10, 5, &[(0, 10), (3, 14)], 4));
    for bad in [
        vec![(0, 9)],           // before the frames
        vec![(0, 15)],          // past them
        vec![(4, 12)],          // a slot past the match's
        vec![(1, 11), (1, 12)], // a slot twice
    ] {
        assert!(!valid_marks(10, 5, &bad, 4), "{bad:?}");
    }
    assert!(!valid_marks(0, 100, &(0..33).map(|s| (s as u8 % 32, s)).collect::<Vec<_>>(), 32));
    let h = HostInputs {
        stamp: Stamp::default(),
        tick: 20,
        your_ack: 0,
        margin: 0,
        first: 10,
        frames: encode_frames(10, 5, 4, &|_, _| Some(0), &|_| Some(0)),
        gone: vec![(2, 12)],
    };
    let d = Datagram::Host(h.clone());
    assert_eq!(decode_datagram(&encode_datagram(&d), 4), Ok(d));
    let bad = Datagram::Host(HostInputs { gone: vec![(2, 15)], ..h });
    assert_eq!(decode_datagram(&encode_datagram(&bad), 4), Err(ProtoError::Invalid));
}

/// N83: reports and both resumes carry marks within their frames; the
/// reliable ones are bounded by `MAX_SLOTS` (the receiver knows its match).
#[test]
fn migration_messages_carry_bounded_marks() {
    let frames = report(5, 0).frames;
    let ok = [
        Control::Report(InputsReport { gone: vec![(31, 104), (0, 100)], ..report(5, 0) }),
        Control::Resume { tick: 9, at_ms: 1, first: 100, frames: frames.clone(), gone: vec![(7, 102)] },
    ];
    for c in ok {
        assert_eq!(decode_control(&encode_control(&c)), Ok(c));
    }
    let bad = [
        Control::Report(InputsReport { gone: vec![(0, 105)], ..report(5, 0) }),
        Control::Report(InputsReport { gone: vec![(32, 100)], ..report(5, 0) }),
        Control::Report(InputsReport { gone: vec![(3, 100), (3, 101)], ..report(5, 0) }),
        Control::Resume { tick: 9, at_ms: 1, first: 100, frames: frames.clone(), gone: vec![(1, 99)] },
        Control::Resume { tick: 9, at_ms: 1, first: 100, frames: vec![], gone: vec![(1, 100)] },
    ];
    for c in bad {
        assert_eq!(decode_control(&encode_control(&c)), Err(ProtoError::Invalid), "{c:?}");
    }
    let resume = |gone| {
        let frames = encode_frames(100, 5, 4, &|_, _| Some(0), &|_| Some(0));
        Datagram::Resume(HostResume { tick: 120, at_ms: 9_000, first: 100, frames, gone })
    };
    assert_eq!(decode_datagram(&encode_datagram(&resume(vec![(3, 104)])), 4), Ok(resume(vec![(3, 104)])));
    assert_eq!(decode_datagram(&encode_datagram(&resume(vec![(4, 104)])), 4), Err(ProtoError::Invalid));
    assert_eq!(decode_datagram(&encode_datagram(&resume(vec![(3, 105)])), 4), Err(ProtoError::Invalid));
    // An empty list costs one byte (ruling PF12d-4).
    let marked = encode_datagram(&resume(vec![]));
    let Datagram::Resume(r) = resume(vec![]) else { unreachable!() };
    // The variant's tag, the fields before the marks, the empty list.
    let without = postcard::to_allocvec(&(r.tick, r.at_ms, r.first, &r.frames)).unwrap();
    assert_eq!(marked.len(), 1 + without.len() + 1);
}
