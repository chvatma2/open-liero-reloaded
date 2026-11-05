//! Liero Factory Reloaded's plant works (2026-11-04): the Dirt-ball line's
//! shift, the alarm, the blackout, the strips breaking and mended, the
//! panes, and the lift, the mine cart and the forklift at work, as
//! `tools/levels/factory/level.rhai` and the copies' scripts run them.

use olr_cli::runner::{Match, Options};
use olr_cli::spec::{MatchSpec, Personality};
use olr_content::Content;
use olr_math::Vec2;
use olr_sim::{EventKind, Keys, LiveGroup, TickEvents, World};

use super::reloaded::{CLASSIC, FACTORY, factory_world, live_copy, packs_dir};

/// `level.rhai`'s constants (ticks).
const QUIET: u32 = 1500;
const HORN: u32 = 150;
const RUN: u32 = 3000;
const ALARM: u32 = 1000;
const REARM: u32 = 2000;
const MAINTAIN: u32 = 2000;
/// The generator's own repair (`generator.rhai`).
const REPAIR: u32 = 3000;
/// The line's countdown, a lit LED every `STEP` ticks (`line.rhai`).
const STEP: u32 = 40;

const STRIPS: [&str; 9] =
    ["strip-1", "strip-2", "strip-3", "strip-4", "strip-5", "strip-6", "strip-7", "strip-8", "strip-9"];
/// Each strip's state at rest: the faulty tubes `On`, the others `Steady`.
const REST: [&str; 9] = ["Steady", "On", "Steady", "On", "Steady", "Steady", "On", "Steady", "Steady"];
const LAMP_GROUPS: [&str; 4] = ["lamps", "cool", "sodium", "mine"];

/// A match of two idle bots on the factory, seed `seed`, `ticks` long.
fn idle_match(seed: u32, ticks: u32) -> Match {
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let spec = MatchSpec {
        personality: Personality::Idle,
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, FACTORY, 2, seed, ticks)
    };
    Match::new(&content, &spec, &Options::default()).unwrap()
}

/// The factory's world a few ticks in (its scripts started), to tick by
/// hand.
fn world() -> World {
    let mut m = idle_match(1, 100);
    for _ in 0..5 {
        assert!(m.step());
    }
    m.world().clone()
}

/// Ticks `w` `n` times without keys, each tick's events to `f`.
fn run(w: &mut World, n: u32, mut f: impl FnMut(&World, &TickEvents)) {
    let mut ev = TickEvents::new();
    for _ in 0..n {
        w.tick(&[Keys(0), Keys(0)], &mut ev);
        f(w, &ev);
    }
}

/// Ticks `w` until `done` holds, at most `limit` ticks; how many it took.
fn until(w: &mut World, limit: u32, done: impl Fn(&World) -> bool) -> u32 {
    let mut ev = TickEvents::new();
    for n in 1..=limit {
        w.tick(&[Keys(0), Keys(0)], &mut ev);
        if done(w) {
            return n;
        }
    }
    panic!("not within {limit} ticks");
}

/// Copy `id`'s state.
fn state<'a>(w: &'a World, id: &str) -> &'a str {
    live_copy(w, id).state_name()
}

/// Level glow group `id` as the scripts set it.
fn group(w: &World, id: &str) -> LiveGroup {
    let live = w.live().unwrap();
    live.groups()[usize::from(live.level().group(id).unwrap_or_else(|| panic!("no group {id}")))]
}

/// A point to set off an explosion on copy `id`: the centre of one of its
/// rock pixels outside every other live copy's box (the ground is not
/// rock there), else its box's centre.
fn target(w: &World, id: &str) -> Vec2 {
    let live = w.live().unwrap();
    let (x0, y0, cw, ch) = live_copy(w, id).rect();
    let others: Vec<_> = live.copies().filter(|c| c.id() != id).map(|c| c.rect()).collect();
    let inside = |(bx, by, bw, bh): (i32, i32, u32, u32), x: i32, y: i32| {
        (bx..bx + bw as i32).contains(&x) && (by..by + bh as i32).contains(&y)
    };
    let solid = w.solid();
    for y in y0..y0 + ch as i32 {
        for x in x0..x0 + cw as i32 {
            let (px, py) = (i64::from(x), i64::from(y));
            let rock = solid.flags(px, py) & olr_terrain::PX_ROCK != 0;
            let ground = w.terrain().flags(px, py) & olr_terrain::PX_ROCK != 0;
            if rock && !ground && !others.iter().any(|&r| inside(r, x, y)) {
                return Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            }
        }
    }
    Vec2::new(x0 as f32 + cw as f32 / 2.0, y0 as f32 + ch as f32 / 2.0)
}

/// Explosions of `damage` on copy `id`, a tick apart, until it is in
/// `wrecked` (at most 20).
fn wreck(w: &mut World, id: &str, damage: f32, wrecked: &str) {
    let at = target(w, id);
    for _ in 0..20 {
        w.explode_for_test(at, damage);
        run(w, 1, |_, _| ());
        if state(w, id) == wrecked {
            return;
        }
    }
    panic!("{id} not {wrecked} after 20 explosions at {at:?}");
}

/// The line's shift: Stopped QUIET ticks, the horn, HORN ticks later it
/// runs (Lit 1 to Lit 7, a STEP apart, the die down onto the belt and up
/// again, balls leaving it and vanishing into the crate) for RUN ticks,
/// then Stopped again. A ball shown never jumps (the user's playtest,
/// 2026-11-05: it rolls through the drive, not past it): at most a pixel
/// a tick on each axis.
#[test]
fn the_line_runs_its_shift() {
    let mut m = idle_match(1, QUIET + HORN + RUN + 200);
    let line = live_copy(m.world(), "line");
    let horn = line.setup().sound("horn.wav").expect("the line's horn");
    let index = line.index();
    let (mut horns, mut entries) = (Vec::new(), Vec::new());
    let (mut die_max, mut shown, mut hidden_again) = (0, false, false);
    let mut balls: [Option<(i64, i64)>; 6] = [None; 6];
    let mut last = state(m.world(), "line").to_owned();
    assert_eq!(last, "Stopped");
    while m.step() {
        let t = m.ticks();
        for e in m.events() {
            if let EventKind::LiveSound { copy, sound } = e.kind
                && copy == index
                && sound == horn
            {
                horns.push(t);
            }
        }
        let w = m.world();
        let now = state(w, "line");
        if now != last {
            entries.push((t, now.to_owned()));
            last = now.to_owned();
        }
        let c = live_copy(w, "line");
        die_max = die_max.max(c.value("die_y").unwrap() / olr_sim::LIVE_SUBPIXELS);
        let on = (1..=6).any(|i| c.value(&format!("b{i}_on")).unwrap() != 0);
        for (i, last) in balls.iter_mut().enumerate() {
            let v = |n: &str| c.value(&format!("b{}_{n}", i + 1)).unwrap();
            let now = (v("on") != 0).then(|| (v("x"), v("y")));
            if let (Some((x0, y0)), Some((x1, y1))) = (*last, now) {
                let step = (x1 - x0).abs().max((y1 - y0).abs());
                assert!(
                    step <= olr_sim::LIVE_SUBPIXELS,
                    "ball {} jumped at {t}: {:?} to {:?}",
                    i + 1,
                    (x0, y0),
                    (x1, y1)
                );
            }
            *last = now;
        }
        shown |= on;
        hidden_again |= shown && c.value("b1_on").unwrap() == 0 && c.value("b1_x").unwrap() != 0;
    }
    assert_eq!(horns.len(), 1, "one horn: {horns:?}");
    assert!(horns[0].abs_diff(QUIET) <= 2, "the horn at {}", horns[0]);
    let (start, ref first) = entries[0];
    assert_eq!(first, "Lit 1");
    assert!(start.abs_diff(horns[0] + HORN) <= 2, "running at {start}, the horn at {}", horns[0]);
    for (k, (t, s)) in entries.iter().take(7).enumerate() {
        assert_eq!(s, &format!("Lit {}", k + 1), "{entries:?}");
        assert!(t.abs_diff(start + STEP * k as u32) <= 2, "Lit {} at {t}", k + 1);
    }
    let &(stop, ref stopped) = entries.last().unwrap();
    assert_eq!(stopped, "Stopped");
    assert!(stop.abs_diff(start + RUN) <= 2, "stopped at {stop}, running from {start}");
    assert_eq!(die_max, 18, "the die comes down onto the belt");
    assert!(shown && hidden_again, "a ball leaves the stamp and vanishes into the crate");
}

/// The press kills (the user's playtest, 2026-11-05): a worm standing on
/// the belt under the die (`factory_works.py`'s `DIE`, x 933-965, the belt
/// at y 351) dies the tick the die first wedges it, dealt by nobody, the
/// die going on down onto the belt.
#[test]
fn the_press_kills_a_worm_it_wedges() {
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let mut w = factory_world(&content, (949, 345));
    run(&mut w, 100, |_, _| ());
    let wm = &w.worms()[0];
    assert!(wm.alive && wm.on_ground && (940.0..958.0).contains(&wm.pos.x), "under the die: {:?}", wm.pos);
    let line = live_copy(&w, "line");
    let (index, lit) = (line.index(), line.setup().state("Lit 1").unwrap());
    w.set_live_state(index, lit).unwrap();
    let (mut crushed, mut killed, mut die) = (None, None, 0);
    let mut t = 0;
    run(&mut w, 8 * STEP, |w, ev| {
        t += 1;
        die = die.max(live_copy(w, "line").value("die_y").unwrap() / olr_sim::LIVE_SUBPIXELS);
        for e in ev.iter() {
            match e.kind {
                EventKind::Crushed { worm: 0, copy } if crushed.is_none() => {
                    assert_eq!(copy, index, "the line's die");
                    crushed = Some(t);
                }
                EventKind::Kill { victim: 0, killer } if killed.is_none() => killed = Some((t, killer)),
                _ => (),
            }
        }
    });
    let crushed = crushed.expect("the die wedged the worm");
    assert_eq!(killed, Some((crushed, -1)), "killed by nobody the tick it was crushed");
    assert_eq!(die, 18, "the die goes on down onto the belt");
}

/// The generator wrecked cuts the power: every strip Dark, the level's
/// lamp groups out, the line, the crane, the lift
/// and the mine cart stopped; repaired REPAIR ticks later, all come back.
#[test]
fn the_generator_cuts_the_power_until_it_is_repaired() {
    let mut w = world();
    // Blasts under the alarm's 5.
    wreck(&mut w, "generator", 4.0, "Wrecked");
    assert_eq!(state(&w, "siren"), "Normal", "small blasts sound no alarm");
    run(&mut w, 3, |_, _| ());
    for s in STRIPS {
        assert_eq!(state(&w, s), "Dark", "{s}");
    }
    for g in LAMP_GROUPS {
        assert_eq!(group(&w, g).intensity, 0, "{g}");
    }
    for (id, off) in [("line", "Stopped"), ("crane", "Off"), ("lift", "Off"), ("cart", "Off")] {
        assert_eq!(state(&w, id), off, "{id}");
    }
    let back = until(&mut w, REPAIR + 10, |w| state(w, "generator") == "Running");
    assert!(back + 3 >= REPAIR, "repaired after {back} ticks");
    assert_eq!(live_copy(&w, "generator").value("hp"), Some(40), "its health back");
    run(&mut w, 3, |_, _| ());
    for (s, rest) in STRIPS.iter().zip(REST) {
        assert_eq!(state(&w, s), rest, "{s}");
    }
    for g in LAMP_GROUPS {
        assert_eq!(group(&w, g), LiveGroup::OWN, "{g}");
    }
    for (id, on) in [("crane", "Idle"), ("lift", "Idle"), ("cart", "Stopped")] {
        assert_eq!(state(&w, id), on, "{id}");
    }
}

/// A big blast (5 damage or more: Classic's heaviest) sounds the alarm
/// for ALARM ticks: the sirens turning their beams, the lamp groups and
/// every strip red; then no blast sounds it again for REARM ticks.
#[test]
fn a_big_blast_sounds_the_alarm() {
    let mut w = world();
    let own: Vec<i64> = STRIPS.iter().map(|s| live_copy(&w, s).value("colour").unwrap()).collect();
    // In the sky over the garden: nothing live within reach.
    w.explode_for_test(Vec2::new(505.5, 60.5), 50.0);
    run(&mut w, 3, |_, _| ());
    assert_eq!(state(&w, "siren"), "Alarm");
    for g in LAMP_GROUPS {
        assert_eq!(group(&w, g), LiveGroup { intensity: 300, colour: 0xff3a20 }, "{g}");
    }
    for s in STRIPS {
        assert_eq!(live_copy(&w, s).value("colour"), Some(0xff3a20), "{s}");
    }
    let mut spreads = std::collections::BTreeSet::new();
    run(&mut w, 120, |w, _| {
        spreads.insert(live_copy(w, "siren").value("spread_a").unwrap());
    });
    assert!(spreads.len() > 5, "the beams turn: {spreads:?}");
    let calm = until(&mut w, ALARM, |w| state(w, "siren") == "Normal");
    assert!(calm + 123 + 5 >= ALARM, "calm after {}", calm + 123);
    run(&mut w, 2, |_, _| ());
    for g in LAMP_GROUPS {
        assert_eq!(group(&w, g), LiveGroup::OWN, "{g}");
    }
    for (s, c) in STRIPS.iter().zip(own) {
        assert_eq!(live_copy(&w, s).value("colour"), Some(c), "{s}");
    }
    // Rearming: a blast now sounds nothing; one REARM ticks on does.
    w.explode_for_test(Vec2::new(505.5, 60.5), 50.0);
    run(&mut w, 3, |_, _| ());
    assert_eq!(state(&w, "siren"), "Normal", "not yet rearmed");
    run(&mut w, REARM, |_, _| ());
    w.explode_for_test(Vec2::new(505.5, 60.5), 50.0);
    run(&mut w, 3, |_, _| ());
    assert_eq!(state(&w, "siren"), "Alarm", "rearmed");
}

/// A strip shot out is Broken, and maintenance mends it (its health
/// whole) within MAINTAIN ticks.
#[test]
fn a_broken_strip_is_mended() {
    let mut w = world();
    wreck(&mut w, "strip-6", 15.0, "Broken");
    assert_eq!(live_copy(&w, "strip-6").value("hp"), Some(0));
    until(&mut w, MAINTAIN + 10, |w| state(w, "strip-6") == "Steady");
    assert_eq!(live_copy(&w, "strip-6").value("hp"), Some(20));
}

/// An office pane shatters.
#[test]
fn a_pane_shatters() {
    let mut w = world();
    assert_eq!(state(&w, "pane-1"), "Intact");
    wreck(&mut w, "pane-1", 10.0, "Broken");
    assert_eq!(state(&w, "pane-3"), "Intact", "its neighbours stand");
}

/// The lift, the mine cart and the forklift go about their work: each
/// through its states, the lift's cage the shaft's whole height, the cart
/// most of its rails and the forklift its whole travel.
#[test]
fn the_lift_cart_and_forklift_work() {
    let mut m = idle_match(1, 4000);
    let watch = [("lift", "cage_y"), ("cart", "cart_x"), ("forklift", "fork_x")];
    let mut seen: Vec<std::collections::BTreeSet<String>> = vec![Default::default(); 3];
    let mut span = [(i64::MAX, i64::MIN); 3];
    while m.step() {
        for (k, (id, value)) in watch.iter().enumerate() {
            let c = live_copy(m.world(), id);
            seen[k].insert(c.state_name().to_owned());
            let v = c.value(value).unwrap() / olr_sim::LIVE_SUBPIXELS;
            span[k] = (span[k].0.min(v), span[k].1.max(v));
        }
    }
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(seen[0], names(&["Idle", "Moving"]), "the lift");
    assert_eq!(seen[1], names(&["Left", "Right", "Stopped"]), "the cart");
    assert_eq!(seen[2], names(&["Driving", "Idle", "Reversing"]), "the forklift");
    assert_eq!(span[0], (0, 87), "the lift's cage");
    assert!(span[1].0 == 0 && span[1].1 > 120, "the cart: {:?}", span[1]);
    assert_eq!(span[2], (0, 14), "the forklift");
}

/// A blast within 40 px of a lit strip flickers it (the level script sets
/// it Off, its own script bursts), the faulty tube and the steady strip
/// alike, and both come back to rest; a strip farther off stays lit.
#[test]
fn a_blast_near_a_strip_flickers_it() {
    let mut w = world();
    // Under strip-4 (centre 998, 178), a faulty tube: 30 px from it and
    // 83 px from strip-5 (1081, 178).
    w.explode_for_test(Vec2::new(998.5, 208.5), 2.0);
    let (mut off, mut far_off) = (false, false);
    run(&mut w, 60, |w, _| {
        off |= state(w, "strip-4") == "Off";
        far_off |= state(w, "strip-5") == "Off";
    });
    assert!(off, "strip-4 flickers");
    assert!(!far_off, "strip-5 stays lit");
    // A steady strip: strip-6 (328, 287).
    w.explode_for_test(Vec2::new(328.5, 310.5), 2.0);
    let mut off = false;
    run(&mut w, 3, |w, _| off |= state(w, "strip-6") == "Off");
    assert!(off, "strip-6 flickers");
    until(&mut w, 200, |w| state(w, "strip-6") == "Steady");
    assert_eq!(live_copy(&w, "strip-6").value("hp"), Some(20), "unhurt");
}

/// The spoil's boxes (`factory_works.py`): the press heap's and the mine's.
const HEAP: (u32, u32, u32, u32) = (820, 320, 90, 50);
const MINE: (u32, u32, u32, u32) = (590, 370, 380, 110);

fn within((bx, by, bw, bh): (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    x >= bx && x < bx + bw && y >= by && y < by + bh
}

/// The press heap and the mine's floor seam are loose spoil, and no dirt
/// outside their boxes is.
#[test]
fn the_factory_has_spoil() {
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let level = content.load_level(FACTORY).unwrap();
    let loose = level.loose.as_ref().expect("the factory has loose dirt");
    let w = level.material.width();
    let (mut heap, mut mine) = (0, 0);
    for (i, &k) in loose.kinds.iter().enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        if k == 0 {
            continue;
        }
        assert_eq!(k, 1, "({x}, {y}): spoil, the only loose material");
        if within(HEAP, x, y) {
            heap += 1;
        } else if within(MINE, x, y) {
            mine += 1;
        } else {
            panic!("({x}, {y}) is loose, outside the heap and the mine");
        }
    }
    assert!(heap > 1000 && mine > 1000, "heap {heap}, mine {mine} loose pixels");
}

/// The spoil rests where the level lays it: in a match of no worms, fewer
/// than 2 % of its grains move in 300 ticks.
#[test]
fn the_spoil_stays_put_in_an_empty_match() {
    let content = Content::open_bundled(&packs_dir()).unwrap();
    let level = content.load_level(FACTORY).unwrap();
    let kinds = &level.loose.as_ref().expect("the factory has loose dirt").kinds;
    let setup = olr_content::SandboxSetup {
        mod_id: CLASSIC.into(),
        level_id: FACTORY.into(),
        seed: 1,
        settings: olr_sim::SimSettings::lx56(),
        worms: Vec::new(),
        restrictions: olr_content::DEFAULT_PRESET.into(),
    };
    let mut w = content.new_world(&setup).unwrap();
    // `grain[cell]`: the starting cell of the grain there now.
    let mut grain: Vec<u32> = (0..kinds.len() as u32).collect();
    let mut moved = vec![false; kinds.len()];
    run(&mut w, 300, |w, _| {
        for &(from, to) in w.loose_moves() {
            let g = grain[from as usize];
            grain[to as usize] = g;
            moved[g as usize] = true;
        }
    });
    let loose = kinds.iter().filter(|&&k| k != 0).count();
    let gone = moved.iter().filter(|&&m| m).count();
    assert!(gone * 50 < loose, "{gone} of {loose} grains moved");
}
