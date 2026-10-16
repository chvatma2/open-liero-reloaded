//! OLX's weapon name over the local worm (`CWorm::Draw`,
//! `src/common/CWorm.cpp:1304-1315` at `d1ad953f`): while the player holds
//! the change-weapon key, and for 0.75 s after a change made with it, the
//! player's worm shows its current weapon's name; bots and a game nobody
//! plays show none. Played headless through `Session`; Classic on Dirt
//! Level from `OLR_CONTENT_DIR`, skipped with a notice without it.

use olr_audio::Audio;
use olr_bots::BotConfig;
use olr_content::{MatchOptions, Player};
use olr_game::session::{Session, WEAPON_NAME_TICKS};
use olr_game::setup;
use olr_scene::Interpolation;
use olr_sim::Keys;

use crate::online::{LEVEL, MOD, content_dir, viewport};

/// The name of worm `i`'s current weapon, as the mod has it.
fn current_name(s: &Session, i: usize) -> String {
    let w = &s.world.worms()[i];
    s.world.sim_mod().weapon(w.current_slot().weapon).unwrap().name.clone()
}

/// The sandbox: the player (worm 0) with the mod's first five weapons
/// and two idle targets, ticked until the player's worm is alive.
fn sandbox(dir: &std::path::Path) -> (setup::Assets, Session) {
    let o = setup::SandboxOptions {
        mod_id: String::new(),
        level_id: String::new(),
        seed: 5,
        targets: 2,
        bonuses: false,
    };
    let a = setup::load(&[dir], "Classic", "Dirt Level", "default", o).unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert_eq!(s.local(), Some(0));
    let mut t = 0;
    while !s.world.worms()[0].alive {
        assert!(t < 500, "the player spawns");
        s.tick(Keys(0));
        t += 1;
    }
    assert!(s.world.worms()[0].slots.len() > 1, "the player has weapons to change between");
    (a, s)
}

/// The name shows while the change key is held, over the player's worm
/// only, and is the current weapon's; after a change it stays for
/// `WEAPON_NAME_TICKS` (OLX's 0.75 s) once the key is released, then
/// hides. Holding the key without changing leaves no tail.
#[test]
fn the_weapon_name_shows_while_changing() {
    let Some(dir) = content_dir() else { return };
    let (a, mut s) = sandbox(&dir);
    assert_eq!(WEAPON_NAME_TICKS, 75, "OLX's fForceWeapon_Time = currentTime + 0.75f");

    // Not held: nothing. Held: the player's worm and its weapon.
    assert_eq!(s.weapon_name(false), None);
    let first = current_name(&s, 0);
    assert_eq!(s.weapon_name(true), Some((0, first.clone())));

    // Held without a change: hidden as soon as it is released.
    s.tick(Keys(Keys::CHANGE));
    s.tick(Keys(Keys::CHANGE));
    assert_eq!(s.weapon_name(true), Some((0, first.clone())));
    s.tick(Keys(0));
    assert_eq!(s.weapon_name(false), None, "no change, no tail");

    // Change + Right: the next weapon, named at once.
    let before = s.world.worms()[0].current_weapon;
    s.tick(Keys(Keys::CHANGE));
    s.tick(Keys(Keys::CHANGE | Keys::RIGHT));
    assert_ne!(s.world.worms()[0].current_weapon, before, "the weapon changed");
    let second = current_name(&s, 0);
    assert_eq!(s.weapon_name(true), Some((0, second.clone())));

    // Released: it stays for 0.75 s after the change, then hides.
    for t in 1..WEAPON_NAME_TICKS {
        s.tick(Keys(0));
        assert_eq!(s.weapon_name(false), Some((0, second.clone())), "{t} ticks after the change");
    }
    s.tick(Keys(0));
    s.tick(Keys(0));
    assert_eq!(s.weapon_name(false), None, "hidden 0.75 s after the change");

    // A change made just before a restart leaves nothing over.
    s.tick(Keys(Keys::CHANGE));
    s.tick(Keys(Keys::CHANGE | Keys::LEFT));
    assert!(s.weapon_name(false).is_some());
    s.restart(&a).unwrap();
    assert_eq!(s.weapon_name(false), None, "a restart clears it");
}

/// In a Deathmatch whose player is slot 2, after two bots, the name is
/// the player's worm's, never a bot's.
#[test]
fn the_weapon_name_is_the_players_worm() {
    let Some(dir) = content_dir() else { return };
    let bot = Player::Bot(BotConfig::default());
    let o = MatchOptions::deathmatch(MOD, LEVEL, 5, vec![bot, bot, Player::Human]);
    let a = setup::load_match(&[dir.as_path()], &o, "default").unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert_eq!(s.local(), Some(2));
    let mut t = 0;
    while !s.world.worms()[2].alive {
        assert!(t < 500, "the player spawns");
        s.tick(Keys(0));
        t += 1;
    }
    assert_eq!(s.weapon_name(true), Some((2, current_name(&s, 2))));
    assert_eq!(s.weapon_name(false), None);
}

/// A game nobody here plays (bots alone) has no player: no weapon name,
/// whatever is held (OLX draws it only for a local human worm).
#[test]
fn bots_show_no_weapon_name() {
    let Some(dir) = content_dir() else { return };
    let o = setup::DeathmatchOptions { bots: 2, spectate: true, ..setup::DeathmatchOptions::default() };
    let a = setup::load_deathmatch(&[dir.as_path()], "Classic", "Dirt Level", "default", &o).unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    for _ in 0..300 {
        s.tick(Keys(Keys::CHANGE | Keys::RIGHT));
        assert_eq!(s.weapon_name(true), None);
    }
}
