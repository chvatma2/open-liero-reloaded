//! The rope's throw sound is the player's own: OLX plays it only where a
//! local human throws the rope, never for a bot or a worm across the
//! network (the user's playtest report: every worm's throw spammed the
//! game). Classic on Dirt Level from `OLR_CONTENT_DIR`, skipped without it.

use std::path::Path;

use olr_audio::Audio;
use olr_bots::Difficulty;
use olr_game::session::Session;
use olr_game::setup::{self, Assets, DeathmatchOptions};
use olr_scene::Interpolation;
use olr_sim::{Event, EventKind, Keys, SoundId};

use crate::online::{content_dir, viewport};

fn load(dir: &Path, o: &DeathmatchOptions) -> Assets {
    setup::load_deathmatch(&[dir], "Classic", "Dirt Level", "default", o).unwrap()
}

fn session(a: &Assets) -> Session {
    Session::new(a, viewport(), Interpolation::Linear, Audio::silent()).unwrap()
}

/// How many of `events` are rope throw sounds.
fn throws_in<'a>(events: impl IntoIterator<Item = &'a Event>) -> u32 {
    let n =
        events.into_iter().filter(|e| matches!(e.kind, EventKind::Sound { id: SoundId::THROW, .. })).count();
    n as u32
}

/// Every worm's rope throws so far.
fn throws(s: &Session) -> Vec<u32> {
    s.world.worms().iter().map(|w| w.rope.throws()).collect()
}

/// The player throws the rope once every `every` ticks (a one-tick press).
fn rope_every(t: u64, every: u64) -> Keys {
    Keys(if t % every == every - 1 { Keys::ROPE } else { 0 })
}

/// A Deathmatch of the player against seven bots: every throw of the
/// player's rope is heard, none of the bots' (they throw about once a
/// second each), and every other sound plays as the world made it.
#[test]
fn only_the_players_rope_throws_are_heard() {
    let Some(dir) = content_dir() else { return };
    let o =
        DeathmatchOptions { seed: 5, bots: 7, difficulty: Difficulty::Hard, ..DeathmatchOptions::default() };
    let mut s = session(&load(&dir, &o));
    assert_eq!(s.local(), Some(0));
    let (mut heard, mut made) = (0, 0);
    for t in 0..3000 {
        s.tick(rope_every(t, 150));
        heard += throws_in(s.sounds());
        made += throws_in(s.events.iter());
        let others = |e: &&Event| matches!(e.kind, EventKind::Sound { id, .. } if id != SoundId::THROW);
        assert!(
            s.sounds().iter().filter(others).eq(s.events.iter().filter(others)),
            "tick {t}: every other sound plays"
        );
    }
    let thrown = throws(&s);
    let bots: u32 = thrown[1..].iter().sum();
    assert!(thrown[0] >= 10, "the player threw the rope {} times", thrown[0]);
    assert!(bots > 100, "the bots threw the rope {bots} times");
    assert_eq!(made, thrown.iter().sum::<u32>(), "the world makes a sound for every throw");
    assert_eq!(heard, thrown[0], "only the player's throws are heard");
}

/// Bots alone, watched: nobody here throws, so no throw is heard.
#[test]
fn bots_alone_throw_silently() {
    let Some(dir) = content_dir() else { return };
    let o = DeathmatchOptions { seed: 5, bots: 8, spectate: true, ..DeathmatchOptions::default() };
    let mut s = session(&load(&dir, &o));
    assert_eq!(s.local(), None);
    let mut heard = 0;
    for _ in 0..1500 {
        s.tick(Keys(0));
        heard += throws_in(s.sounds());
    }
    let bots: u32 = throws(&s).iter().sum();
    assert!(bots > 50, "the bots threw the rope {bots} times");
    assert_eq!(heard, 0, "no throw is heard");
}

/// A replay sounds as its game did for the player who made it: the
/// viewer's throws are heard, the bots' are not.
#[test]
fn a_replay_hears_its_viewers_throws() {
    let Some(dir) = content_dir() else { return };
    let o = DeathmatchOptions { seed: 6, bots: 3, ..DeathmatchOptions::default() };
    let mut s = session(&load(&dir, &o));
    for t in 0..1200 {
        s.tick(rope_every(t, 100));
    }
    let mut replay = s.take_replay().expect("a Deathmatch records");
    assert_eq!(replay.setup.viewer, Some(0));
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("match.olrp");
    let mut heard = |viewer: Option<u8>| {
        replay.setup.viewer = viewer;
        std::fs::write(&file, replay.encode()).unwrap();
        let mut r = session(&setup::load_replay(&[dir.as_path()], &file, &[]).unwrap());
        assert_eq!(r.local(), None, "nobody plays a replay");
        let mut heard = 0;
        while r.tick(Keys(0)) {
            heard += throws_in(r.sounds());
        }
        (heard, throws(&r))
    };
    let (by_player, thrown) = heard(Some(0));
    assert!(thrown[0] >= 5 && thrown[1..].iter().sum::<u32>() > 20, "throws: {thrown:?}");
    assert_eq!(by_player, thrown[0], "the viewer's throws are heard");
    assert_eq!(heard(Some(2)).0, thrown[2], "another viewer's");
    assert_eq!(heard(None).0, 0, "none without a viewer");
}
