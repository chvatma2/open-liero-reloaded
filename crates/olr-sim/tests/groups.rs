//! 2026-11-04: scripts set the level's own glow groups: world state,
//! hashed only once one is set, transferred and checked.

use std::sync::Arc;

use olr_sim::{
    Director, Keys, LiveCommand, LiveCtx, LiveGroup, LiveLevel, StateError, TickEvents, World, WorldState,
};

use crate::director::scripted;

/// At frame 20 the level script dims group 1 and tints it; nothing else.
#[derive(Clone, Debug)]
struct Dimmer;

impl Director for Dimmer {
    fn box_clone(&self) -> Box<dyn Director> {
        Box::new(Dimmer)
    }

    fn timer_functions(&self, _script: u16) -> u16 {
        0
    }

    fn run(&self, ctx: &mut LiveCtx<'_>) {
        if ctx.frame() == 20 {
            assert!(ctx.apply(LiveCommand::Group { group: 1, intensity: Some(40), colour: Some(0xff2010) }));
            assert!(!ctx.apply(LiveCommand::Group { group: 2, intensity: Some(40), colour: None }));
        }
    }
}

/// Nothing at all.
#[derive(Clone, Debug)]
struct Idle;

impl Director for Idle {
    fn box_clone(&self) -> Box<dyn Director> {
        Box::new(Idle)
    }

    fn timer_functions(&self, _script: u16) -> u16 {
        0
    }

    fn run(&self, _ctx: &mut LiveCtx<'_>) {}
}

fn level(groups: &[&str]) -> Arc<LiveLevel> {
    let l = scripted(&crate::live_level::live_level());
    Arc::new((*l).clone().with_groups(groups.iter().map(|g| g.to_string()).collect()))
}

fn world(groups: &[&str], d: Box<dyn Director>) -> World {
    crate::world::world_directed(4, 400, 300, level(groups), d)
}

fn tick(w: &mut World, n: u32) -> Vec<u64> {
    let mut ev = TickEvents::new();
    (0..n)
        .map(|_| {
            w.tick(&[Keys(0); 4], &mut ev);
            w.checksum()
        })
        .collect()
}

#[test]
fn groups_no_script_set_hash_as_before() {
    let mut with = world(&["sky", "lamps"], Box::new(Idle));
    let mut without = world(&[], Box::new(Idle));
    assert_eq!(tick(&mut with, 40), tick(&mut without, 40));
    assert_eq!(with.live().unwrap().groups(), &[LiveGroup::OWN; 2]);
}

#[test]
fn a_set_group_is_state() {
    let mut set = world(&["sky", "lamps"], Box::new(Dimmer));
    let mut idle = world(&["sky", "lamps"], Box::new(Idle));
    let (a, b) = (tick(&mut set, 30), tick(&mut idle, 30));
    assert_eq!(a[..19], b[..19], "the same until the script sets it");
    assert_ne!(a[29], b[29]);
    assert_eq!(
        set.live().unwrap().groups(),
        &[LiveGroup::OWN, LiveGroup { intensity: 40, colour: 0xff2010 }]
    );
    // Transferred whole.
    let bytes = set.state().encode();
    let mut dst = world(&["sky", "lamps"], Box::new(Dimmer));
    dst.load_state(WorldState::decode(&bytes).unwrap()).unwrap();
    assert_eq!(dst.checksum(), set.checksum());
    assert_eq!(dst.live().unwrap().groups(), set.live().unwrap().groups());
    // Restored by rollback.
    let mut w = world(&["sky", "lamps"], Box::new(Dimmer));
    tick(&mut w, 10);
    let snap = w.save();
    let first = tick(&mut w, 20);
    w.restore(&snap).unwrap();
    assert_eq!(tick(&mut w, 20), first);
}

#[test]
fn bad_group_settings_are_refused() {
    let mut src = world(&["sky", "lamps"], Box::new(Dimmer));
    tick(&mut src, 25);
    let s = src.state();
    let refused = |edit: &dyn Fn(&mut Vec<LiveGroup>), plain: bool| {
        let mut s = s.clone();
        edit(s.live_mut_for_test().as_mut().unwrap().groups_mut_for_test());
        if plain {
            *s.live_mut_for_test().as_mut().unwrap().script_mut_for_test() = None;
        }
        let mut w = if plain {
            crate::world::world_live(4, 400, 300, level(&["sky", "lamps"]))
        } else {
            world(&["sky", "lamps"], Box::new(Dimmer))
        };
        let before = w.clone();
        assert_eq!(w.load_state(s), Err(StateError::Live));
        assert_eq!(w, before);
    };
    refused(&|g| g.push(LiveGroup::OWN), false);
    refused(&|g| g[0].intensity = 1001, false);
    refused(&|g| g[0].intensity = -2, false);
    refused(&|g| g[1].colour = 0x100_0000, false);
    // A setting in a world without scripts.
    refused(&|_| {}, true);
}
