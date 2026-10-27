//! Issue #91: bots on a level with live parts see them and keep out of
//! where they crush. On the live fixture (`fixtures/live/levels`, its
//! press, lift and door), a Deathmatch of 4 bots scores kills, and
//! crushing makes at most a tenth of the deaths.

use crate::common;

use common::*;
use olr_cli::runner::{Match, Options};
use olr_cli::spec::{MatchSpec, Personality};
use olr_sim::EventKind;

#[test]
fn bots_rarely_die_crushed() {
    let Some(c) = live_content() else { return };
    let spec = MatchSpec {
        personality: Personality::Deathmatch,
        kill_limit: 0,
        time_limit: 0.0,
        ..MatchSpec::new(CLASSIC, LIVE_LEVELS[0], 4, 101, 6000)
    };
    let mut m = Match::new(&c, &spec, &Options::default()).unwrap();
    // Per worm id, whether the last hit it took was a crushing chunk (its
    // `Damage` right after its `Crushed` marker).
    let mut crushed_last = [false; 32];
    let (mut deaths, mut crushed, mut kills) = (0, 0, 0);
    for _ in 0..spec.ticks {
        m.advance();
        if let Err(f) = m.settle() {
            panic!("{}", capture(&m.finish(), &f));
        }
        let mut marked = None;
        for e in m.events().iter() {
            match e.kind {
                EventKind::Crushed { worm, .. } => marked = Some(worm),
                EventKind::Damage { worm, .. } => {
                    if let Some(slot) = usize::try_from(worm).ok().and_then(|i| crushed_last.get_mut(i)) {
                        *slot = marked == Some(worm);
                    }
                    marked = None;
                }
                EventKind::Kill { victim, killer } => {
                    deaths += 1;
                    if killer >= 0 && killer != victim {
                        kills += 1;
                    }
                    if usize::try_from(victim).ok().and_then(|i| crushed_last.get(i)).is_some_and(|c| *c) {
                        crushed += 1;
                    }
                }
                _ => {}
            }
        }
    }
    let run = m.finish();
    eprintln!("{}; {deaths} deaths, {crushed} crushed, {kills} kills", run.summary());
    assert!(run.failure().is_none(), "{}", run.summary());
    assert!(kills > 0, "the bots scored no kill");
    assert!(crushed * 10 <= deaths, "{crushed} of {deaths} deaths by crushing");
}
