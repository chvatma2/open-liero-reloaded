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

fn lobby() -> LobbyState {
    LobbyState {
        name: "Office".into(),
        phase: Phase::Playing { tick: 9 },
        setup: setup(),
        members: (0..3)
            .map(|i| Member {
                id: i,
                key: [i; 32],
                look: look(),
                seat: SeatWire::Player { slot: i },
                ready: true,
                missing: vec!["olx-skin-x".into()],
                ping_ms: Some(5),
                admin: i == 0,
            })
            .collect(),
    }
}

/// Valid encodings of every message family, tagged by decoder.
fn samples() -> Vec<Vec<u8>> {
    let mut v = vec![
        encode_signal(&Signal::Offer { protocol: 1, fingerprint: 5, sdp: "v=0\r\n".into() }),
        encode_signal(&Signal::Answer { sdp: "v=0".into() }),
        encode_lan(&LanAnnounce {
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
        }),
    ];
    for m in [
        ToHost::Hello { key: [7; 32], look: look(), password: Some([1; 32]) },
        ToHost::Prove { signature: [9; 64] },
        ToHost::Chat("hello there".into()),
        ToHost::Have { missing: vec!["olx-skin-a".into(), "olx-skin-b".into()] },
        ToHost::Admin(AdminCommand::Settings(setup())),
        ToHost::Admin(AdminCommand::Kick([1; 32])),
    ] {
        v.push(encode_to_host(&m));
    }
    for m in [
        ToClient::Welcome { you: 1, lobby: lobby() },
        ToClient::Lobby(lobby()),
        ToClient::Chat { from: 1, text: "hi".into() },
        ToClient::Notice(Notice::NotReady(vec![1, 2])),
        ToClient::Ended(MatchEndWire {
            reason: "limit".into(),
            rows: vec![(0, 1, 2, 3, 4)],
            last_tick: 4_000,
        }),
        ToClient::Start(MatchStartWire {
            seed: 1,
            setup: setup(),
            slots: vec![SlotWire { member: Some(0), bot: Some(0), look: look(), loadout: None }],
            input_delay: vec![(0, 2)],
            start_checksum: 3,
        }),
    ] {
        v.push(encode_to_client(&m));
    }
    v
}

/// Decodes with every decoder; an Ok decode must re-encode to an equal message.
fn decode_all(b: &[u8]) {
    let _ = tag(b);
    if let Ok(m) = decode_signal(b) {
        assert_eq!(decode_signal(&encode_signal(&m)), Ok(m));
    }
    if let Ok(m) = decode_to_host(b) {
        assert_eq!(decode_to_host(&encode_to_host(&m)), Ok(m));
    }
    if let Ok(m) = decode_to_client(b) {
        assert_eq!(decode_to_client(&encode_to_client(&m)), Ok(m));
    }
    if let Ok(m) = decode_lan(b) {
        assert_eq!(decode_lan(&encode_lan(&m)), Ok(m));
    }
}

#[test]
fn samples_decode() {
    for b in samples() {
        assert!(
            decode_signal(&b).is_ok()
                || decode_to_host(&b).is_ok()
                || decode_to_client(&b).is_ok()
                || decode_lan(&b).is_ok()
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..3000)) {
        decode_all(&bytes);
    }

    #[test]
    fn mutated_encodings_never_panic(
        which in any::<prop::sample::Index>(),
        edits in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
        cut in any::<prop::sample::Index>(),
    ) {
        let all = samples();
        let mut b = all[which.index(all.len())].clone();
        for (i, v) in &edits { let i = i.index(b.len()); b[i] = *v; }
        let n = cut.index(b.len() + 1);
        decode_all(&b);
        decode_all(&b[..n]);
    }
}
