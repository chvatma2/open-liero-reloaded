//! Issue #88: script state in the world, with `tests/common/director.rs`'
//! director: hashed, restored by rollback, transferred and checked.

use olr_sim::{Keys, ScriptWireEdit, StateError, TickEvents, World, WorldState};

use crate::director::{FUNCS, TestDirector, scripted};

fn keys(t: u32) -> Vec<Keys> {
    (0..8)
        .map(|i| Keys(if (t / 30 + i).is_multiple_of(2) { Keys::RIGHT | Keys::FIRE } else { Keys::LEFT }))
        .collect()
}

fn fresh() -> World {
    let live = scripted(&crate::live_level::live_level());
    crate::world::world_directed(8, 400, 300, live, Box::new(TestDirector))
}

fn played(ticks: u32) -> World {
    let mut w = fresh();
    tick(&mut w, 0, ticks);
    w
}

fn tick(w: &mut World, from: u32, n: u32) -> Vec<u64> {
    let mut ev = TickEvents::new();
    (from..from + n)
        .map(|t| {
            w.tick(&keys(t), &mut ev);
            w.checksum()
        })
        .collect()
}

#[test]
fn script_state_is_in_the_checksum() {
    let w = played(60);
    // Each part of the scripts' state changes the checksum.
    for edit in [
        ScriptWireEdit::MemoryByte(3, b'x'),
        ScriptWireEdit::Errors(1, 15),
        ScriptWireEdit::Flags(2, 1),
        ScriptWireEdit::TimerFunc(0, 1),
        ScriptWireEdit::Entered(0, 1),
    ] {
        let mut s = w.state();
        s.live_mut_for_test().as_mut().unwrap().script_mut_for_test().as_mut().unwrap().edit_for_test(edit);
        let mut other = fresh();
        other.load_state(s).unwrap_or_else(|e| panic!("{edit:?}: {e:?}"));
        assert_ne!(other.checksum(), w.checksum(), "{edit:?}");
    }
    // The same level without a director hashes its copies alone, and
    // differently.
    let mut plain = crate::world::world_live(8, 400, 300, scripted(&crate::live_level::live_level()));
    let mut f = fresh();
    tick(&mut plain, 0, 1);
    tick(&mut f, 0, 1);
    assert_ne!(plain.checksum(), f.checksum());
}

#[test]
fn a_restore_replays_scripts_the_same() {
    let mut w = played(30);
    w.enable_rollback();
    let snap = w.save();
    let first = tick(&mut w, 30, 200);
    w.restore(&snap).unwrap();
    let again = tick(&mut w, 30, 200);
    assert_eq!(first, again);
    // Without rollback too.
    let mut w = played(30);
    let snap = w.save();
    let first = tick(&mut w, 30, 200);
    w.restore(&snap).unwrap();
    assert_eq!(tick(&mut w, 30, 200), first);
}

#[test]
fn script_state_round_trips_through_a_state_transfer() {
    let mut src = played(120);
    let s = src.state();
    let (timers, memory) = s
        .clone()
        .live_mut_for_test()
        .as_mut()
        .unwrap()
        .script_mut_for_test()
        .as_ref()
        .unwrap()
        .sizes_for_test();
    assert!(timers > 0 && memory > 0, "the state holds timers and memory");
    let bytes = s.encode();
    let mut dst = fresh();
    dst.load_state(WorldState::decode(&bytes).unwrap()).unwrap();
    assert_eq!(dst.checksum(), src.checksum());
    assert_eq!(tick(&mut dst, 120, 150), tick(&mut src, 120, 150));
}

/// `s` edited by `edit` is refused by a fresh world as `Live`, changing
/// nothing.
fn refused(s: &WorldState, edit: impl FnOnce(&mut WorldState)) {
    let mut s = s.clone();
    edit(&mut s);
    let mut w = fresh();
    let before = w.clone();
    assert_eq!(w.load_state(s), Err(StateError::Live));
    assert_eq!(w, before);
}

#[test]
fn a_bad_script_part_is_refused() {
    let src = played(80);
    let s = src.state();
    let script = |e: ScriptWireEdit| {
        move |s: &mut WorldState| {
            s.live_mut_for_test().as_mut().unwrap().script_mut_for_test().as_mut().unwrap().edit_for_test(e)
        }
    };
    refused(&s, script(ScriptWireEdit::TimerFunc(0, FUNCS)));
    refused(&s, script(ScriptWireEdit::Errors(0, olr_sim::MAX_SCRIPT_ERRORS + 1)));
    refused(&s, script(ScriptWireEdit::Flags(0, 4)));
    refused(&s, script(ScriptWireEdit::Entered(9, 0)));
    refused(&s, script(ScriptWireEdit::Entered(0, 9)));
    // Memory: a key out of order (`fired` < `n`: "n" first), a bad tag,
    // trailing junk, a huge tail.
    refused(&s, script(ScriptWireEdit::MemoryByte(1, b'z')));
    refused(&s, script(ScriptWireEdit::MemoryByte(6, 9)));
    refused(&s, script(ScriptWireEdit::MemoryGrow(1, 0)));
    refused(&s, script(ScriptWireEdit::MemoryGrow(10_000, 1)));
    // A script part for a world without one, and the reverse.
    refused(&s, |s| *s.live_mut_for_test().as_mut().unwrap().script_mut_for_test() = None);
    let plain = crate::world::world_live(8, 400, 300, scripted(&crate::live_level::live_level()));
    let mut w = plain.clone();
    assert_eq!(w.load_state(s.clone()), Err(StateError::Live));
    assert_eq!(w, plain);
}

#[test]
fn script_peak_is_not_state() {
    let src = played(50);
    assert!(src.script_peak().run > 0 && src.script_peak().tick > 0, "{:?}", src.script_peak());
    let mut dst = fresh();
    dst.load_state(src.state()).unwrap();
    assert_eq!(dst.script_peak(), Default::default());
    assert_eq!(dst, src);
    assert_eq!(dst.checksum(), src.checksum());
}

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]

    /// Hostile host: a scripts' part spoiled anywhere is taken (and
    /// ticks) or refused as `Live`, changing nothing; never a panic.
    #[test]
    fn hostile_script_parts_never_panic(
        kind in 0u8..6,
        at in 0usize..400,
        byte in any::<u8>(),
        word in any::<u16>(),
    ) {
        let src = played(45);
        let mut s = src.state();
        let edit = match kind {
            0 => ScriptWireEdit::MemoryByte(at, byte),
            1 => ScriptWireEdit::MemoryGrow(at % 80, byte),
            2 => ScriptWireEdit::TimerFunc(at % 8, word % 4),
            3 => ScriptWireEdit::Errors(at % 8, byte % 20),
            4 => ScriptWireEdit::Flags(at % 8, byte % 8),
            _ => ScriptWireEdit::Entered(word % 5, u16::from(byte % 3)),
        };
        s.live_mut_for_test().as_mut().unwrap().script_mut_for_test().as_mut().unwrap().edit_for_test(edit);
        let mut w = fresh();
        let before = w.clone();
        match w.load_state(s) {
            Ok(()) => {
                tick(&mut w, 45, 10);
            }
            Err(e) => {
                prop_assert_eq!(e, StateError::Live);
                prop_assert_eq!(w, before, "a refused state changes nothing");
            }
        }
    }
}
