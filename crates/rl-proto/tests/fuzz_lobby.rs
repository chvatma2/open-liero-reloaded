use proptest::prelude::*;
use rl_proto::*;

fn look() -> Look {
    Look { name: "Martin".into(), colour: [1, 2, 3], skin: "olx-skin-afro".into() }
}

fn setup() -> MatchSetupWire {
    MatchSetupWire {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-dirt-level".into(),
        packs: vec![("olx-classic".into(), "ab".repeat(32))],
        rules: RulesWire {
            lives: 10,
            kill_limit: -1,
            time_limit_min: 5.5,
            allow_negative_score: true,
            death_decreases_score: 0.5,
            suicide_decreases_score: false,
            sprees: [1, 2, 3, 4, 5],
            dying_sprees: [5, 4, 3, 2, 1],
        },
        bonuses: true,
        loading_time: 100,
        restrictions: String::new(),
        bots: vec![BotWire { difficulty: 2, personality: 1, loadout: 0, look: look() }],
    }
}

fn samples() -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let signal = encode_signal(&Signal::Offer { protocol: 1, fingerprint: 5, sdp: "v=0\r\n".into() });
    let host = encode_to_host(&ToHost::Hello { key: [7; 32], look: look(), password: Some([1; 32]) });
    let client = encode_to_client(&ToClient::Start(MatchStartWire {
        seed: 1,
        setup: setup(),
        slots: vec![SlotWire { member: Some(0), bot: None, look: look(), loadout: None }],
        input_delay: vec![(0, 2)],
        start_checksum: 3,
    }));
    let lan = encode_lan(&LanAnnounce {
        fingerprint: 1,
        name: "Office".into(),
        port: 23400,
        players: 1,
        max_players: 8,
        spectators: 0,
        mod_id: "olx-classic".into(),
        level_id: "dirt".into(),
        password: false,
        playing: false,
    });
    (signal, host, client, lan)
}

fn decode_all(b: &[u8]) {
    let _ = decode_signal(b);
    let _ = decode_to_host(b);
    let _ = decode_to_client(b);
    let _ = decode_lan(b);
    let _ = tag(b);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..3000)) {
        decode_all(&bytes);
    }

    #[test]
    fn mutated_encodings_never_panic(
        which in 0usize..4,
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        cut in any::<prop::sample::Index>(),
    ) {
        let (s, h, c, l) = samples();
        let mut b = vec![s, h, c, l].swap_remove(which);
        for (i, v) in &edits { let i = i.index(b.len()); b[i] = *v; }
        let n = cut.index(b.len() + 1);
        decode_all(&b);
        decode_all(&b[..n]);
    }
}
