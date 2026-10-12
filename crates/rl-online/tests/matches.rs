//! A match from a start that came off the network (spec §5.4): whatever
//! rl-proto lets through, `matches::build` refuses or builds, never panics.

use crate::common;

use common::{LEVEL, MOD, content, look, setup, start};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use rl_modes::DeathmatchSettings;
use rl_online::matches::{self, rules_wire};
use rl_proto::{
    BotWire, MatchSetupWire, MatchStartWire, RulesWire, SlotWire, ToClient, decode_to_client,
    encode_to_client,
};

#[test]
fn a_good_start_builds() {
    let Some(c) = content() else { return };
    let w = matches::build(&c, &start(setup(&c, 0.1), 1)).unwrap();
    assert_eq!(w.sim.world().worms().len(), 2);
    assert_eq!(w.bots.keys().copied().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn a_slot_both_member_and_bot_is_refused() {
    let Some(c) = content() else { return };
    let mut s = start(setup(&c, 0.1), 1);
    s.slots[1].member = Some(4);
    let e = matches::build(&c, &s).err().expect("refused");
    assert!(format!("{e:#}").contains("neither a member's nor a bot's"), "{e:#}");
}

#[test]
fn a_single_slot_is_refused() {
    let Some(c) = content() else { return };
    let mut s = start(setup(&c, 0.1), 1);
    s.slots.truncate(1);
    assert!(matches::build(&c, &s).is_err());
}

#[test]
fn an_unknown_mod_is_refused() {
    let Some(c) = content() else { return };
    let mut s = start(setup(&c, 0.1), 1);
    s.setup.mod_id = "olx-no-such-mod".into();
    s.setup.packs.retain(|(id, _)| id != MOD);
    assert!(matches::build(&c, &s).is_err());
    // Named in the packs too.
    s.setup.packs.push(("olx-no-such-mod".into(), "0".repeat(64)));
    let e = matches::build(&c, &s).err().expect("refused");
    assert!(format!("{e:#}").contains("missing"), "{e:#}");
}

#[test]
fn a_pack_hash_mismatch_is_refused() {
    let Some(c) = content() else { return };
    let mut s = start(setup(&c, 0.1), 1);
    let level = s.setup.packs.iter_mut().find(|(id, _)| id == LEVEL).unwrap();
    level.1 = "f".repeat(64);
    let e = matches::build(&c, &s).err().expect("refused");
    assert!(format!("{e:#}").contains("differs"), "{e:#}");
}

fn finite() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(0.0f32),
        Just(-1.0),
        Just(0.05),
        Just(1e30),
        Just(-1e30),
        Just(f32::MIN_POSITIVE),
        -100.0f32..100.0
    ]
}

fn int() -> impl Strategy<Value = i32> {
    prop_oneof![Just(i32::MIN), Just(i32::MAX), Just(-2), Just(0), -5i32..200]
}

fn rules() -> impl Strategy<Value = RulesWire> {
    (
        int(),
        int(),
        finite(),
        any::<bool>(),
        finite(),
        any::<bool>(),
        [int(), int(), int(), int(), int()],
        [int(), int(), int(), int(), int()],
    )
        .prop_map(|(lives, kill_limit, t, neg, dds, sds, sprees, dying)| RulesWire {
            lives,
            kill_limit,
            time_limit_min: t,
            allow_negative_score: neg,
            death_decreases_score: dds,
            suicide_decreases_score: sds,
            sprees,
            dying_sprees: dying,
        })
}

fn small_u8() -> impl Strategy<Value = u8> {
    prop_oneof![4 => 0u8..2, 1 => any::<u8>()]
}

fn bot() -> impl Strategy<Value = BotWire> {
    (small_u8(), small_u8(), small_u8()).prop_map(|(difficulty, personality, loadout)| BotWire {
        difficulty,
        personality,
        loadout,
        look: look("Bot"),
    })
}

fn weapon() -> impl Strategy<Value = i32> {
    prop_oneof![-2i32..40, any::<i32>()]
}

fn slot(bots: usize) -> impl Strategy<Value = SlotWire> {
    let bot = if bots == 0 { Just(None).boxed() } else { proptest::option::of(0..bots as u8).boxed() };
    let weapons = proptest::option::of([weapon(), weapon(), weapon(), weapon(), weapon()]);
    (any::<u8>(), bot, any::<u8>(), weapons).prop_map(|(member, bot, odd, loadout)| SlotWire {
        // Mostly a member or a bot, now and then both or neither.
        member: (bot.is_none() != (odd < 16)).then_some(member),
        bot,
        look: look("Worm"),
        loadout,
    })
}

/// Starts within rl-proto's bounds: the real packs, some of them changed
/// or dropped, or other ids; any rules, bots and slots.
fn starts(good: Vec<(String, String)>) -> impl Strategy<Value = MatchStartWire> {
    let ids = |real: &'static str, other: &'static str| prop_oneof![4 => Just(real), 1 => Just(other)];
    let packs = (Just(good), 0usize..5).prop_map(|(mut p, how)| {
        match how {
            0 => p.clear(),
            1 => p[0].1 = "0".repeat(64),
            2 => p.push(("olx-unknown".into(), "a".repeat(64))),
            _ => {}
        }
        p
    });
    let setup = (
        ids(MOD, "olx-mw-1-0"),
        ids(LEVEL, "olx-level-castlestrike"),
        packs,
        prop_oneof![3 => Just(rules_wire(&DeathmatchSettings::default())), 1 => rules()],
        any::<bool>(),
        prop_oneof![3 => Just(100), 1 => int()],
        prop_oneof![
            Just(String::new()),
            Just(rl_content::DEFAULT_PRESET.to_string()),
            Just("Unknown".to_string())
        ],
        proptest::collection::vec(bot(), 0..4),
    )
        .prop_map(|(m, l, packs, rules, bonuses, loading_time, restrictions, bots)| MatchSetupWire {
            mod_id: m.into(),
            level_id: l.into(),
            packs,
            rules,
            bonuses,
            loading_time,
            restrictions,
            bots,
        });
    (setup, any::<u32>(), any::<u64>()).prop_flat_map(|(setup, seed, checksum)| {
        let n = setup.bots.len();
        prop_oneof![3 => 2usize..6, 1 => 0usize..2]
            .prop_flat_map(move |len| proptest::collection::vec(slot(n), len))
            .prop_map(move |slots| MatchStartWire {
                seed,
                setup: setup.clone(),
                slots,
                input_delay: vec![],
                start_checksum: checksum,
            })
    })
}

#[test]
fn hostile_starts_build_or_are_refused() {
    let Some(c) = content() else { return };
    let good = setup(&c, 0.1).packs;
    let mut runner = TestRunner::new(Config { cases: 256, failure_persistence: None, ..Config::default() });
    let built = std::cell::Cell::new(0);
    runner
        .run(&starts(good), |s| {
            // Only what the wire lets through reaches `build`.
            let wire = encode_to_client(&ToClient::Start(s));
            let Ok(ToClient::Start(s)) = decode_to_client(&wire) else { return Ok(()) };
            if matches::build(&c, &s).is_ok() {
                built.set(built.get() + 1);
            }
            Ok(())
        })
        .unwrap();
    // About 30 of 256 build a world.
    assert!(built.get() >= 10, "{} starts built", built.get());
}
