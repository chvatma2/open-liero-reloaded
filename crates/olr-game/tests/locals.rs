//! Several local players in one match (issue #29, split screen headless):
//! four humans on slots 0, 2, 3 and 5 between two bots, each moved by its
//! own scripted keys, with its own look, skin, view, tally, rope throws
//! heard and weapon name; the replay keeps every local slot and plays back
//! to the same checksums with a view each. Classic on Dirt Level from
//! `OLR_CONTENT_DIR`, skipped without it.

use std::path::Path;

use olr_audio::Audio;
use olr_bots::BotConfig;
use olr_content::{MatchOptions, Player};
use olr_game::session::{Driver, Session};
use olr_game::setup::{self, Assets, DeathmatchOptions, MAX_LOCALS, PlayerLook};
use olr_present::default_colour;
use olr_scene::Interpolation;
use olr_sim::{Event, EventKind, Keys, SoundId};

use crate::online::{LEVEL, MOD, content_dir, viewport};

/// The local slots: humans between the bots, not in a row from 0.
const SLOTS: [u8; MAX_LOCALS] = [0, 2, 3, 5];

/// Local player `k`'s keys at tick `t`: walking one way then the other,
/// jumping, aiming, firing in bursts and throwing the rope, each on
/// periods of its own, so no two players press alike.
fn script(k: usize, t: u64) -> Keys {
    let k = k as u64;
    let phase = |period: u64| (t + 13 * k) % period;
    let walk = 100 + 30 * k;
    let mut keys = if (phase(walk) < walk / 2) != k.is_multiple_of(2) { Keys::RIGHT } else { Keys::LEFT };
    if phase(40 + 7 * k) < 4 {
        keys |= Keys::JUMP;
    }
    if (t / (25 + 5 * k)).is_multiple_of(3) {
        keys |= Keys::UP;
    }
    if (t / (60 + 10 * k)).is_multiple_of(2) && t % 8 < 4 {
        keys |= Keys::FIRE;
    }
    if phase(150 + 20 * k) == 0 {
        keys |= Keys::ROPE;
    }
    Keys(keys)
}

fn session(a: &Assets) -> Session {
    Session::new(a, viewport(), Interpolation::Linear, Audio::silent()).unwrap()
}

/// The match: humans on [`SLOTS`], bots on 1 and 4; players 1 and 2 with
/// profiles (Ann in the default skin, Bert in Ant), 3 and 4 without.
fn four_locals(dir: &Path) -> Assets {
    let bot = Player::Bot(BotConfig::default());
    let h = Player::Human;
    let o = MatchOptions::deathmatch(MOD, LEVEL, 21, vec![h, bot, h, h, bot, h]);
    let mut a =
        setup::load_match_for(&[dir], &o, &["default", "olx-skin-ant", "default", "olx-skin-ant"]).unwrap();
    a.locals[0].look = Some(PlayerLook { name: "Ann".into(), colour: [250, 10, 10] });
    a.locals[1].look = Some(PlayerLook { name: "Bert".into(), colour: [0, 240, 0] });
    a
}

fn is_throw(e: &Event) -> bool {
    matches!(e.kind, EventKind::Sound { id: SoundId::THROW, .. })
}

/// Four local players, each moved by its own scripted keys and no other's,
/// replay from their recording to the same checksums; the tally, the
/// deaths and spawns, the names, colours and skins are each player's; the
/// rope throws heard are the four players' and no bot's; the replay keeps
/// the local slots and plays back with a view on each.
#[test]
fn four_local_players_replay_from_their_recording() {
    let Some(dir) = content_dir() else { return };
    let a = four_locals(&dir);
    assert_eq!(a.local_slots(), SLOTS);
    let mut s = session(&a);
    assert_eq!(s.locals(), SLOTS);
    assert_eq!(s.local(), Some(0), "the HUD's player is the first");
    assert!(!s.spectating() && s.human());
    // A view each, on its own worm (split screen's cameras, issue #27).
    assert_eq!(s.present.views(), MAX_LOCALS);
    for (v, &slot) in SLOTS.iter().enumerate() {
        assert_eq!(s.present.followed_view(v), Some(i32::from(slot)));
    }
    // Each its own name, colour and skin; the event log names each.
    let names: Vec<String> = SLOTS.iter().map(|&i| s.names.of(i32::from(i))).collect();
    assert_eq!(names, ["Ann", "Bert", "Player 3", "Player 4"]);
    let rgb = |c: olr_data::image::Rgba| [c.r, c.g, c.b];
    assert_eq!(
        SLOTS.map(|i| s.colour(usize::from(i))),
        [[250, 10, 10], [0, 240, 0], rgb(default_colour(2)), rgb(default_colour(3))]
    );
    let bots = olr_ui::bot_colours_away(&SLOTS.map(|i| s.colour(usize::from(i))), 2);
    assert_eq!([s.colour(1), s.colour(4)], [bots[0], bots[1]], "the bots' away from every player's");
    assert_eq!((s.names.of(1), s.names.of(4)), ("Bot 1".into(), "Bot 2".into()));
    let skin = |i: usize| s.looks()[i].skin.skin.def.name.clone();
    assert_eq!(
        (skin(0), skin(2), skin(3), skin(5)),
        ("default".into(), "Ant".into(), "default".into(), "Ant".into())
    );

    let (mut kills, mut deaths, mut spawns) = ([0u64; MAX_LOCALS], [0u32; MAX_LOCALS], [0u32; MAX_LOCALS]);
    let (mut heard, mut bot_throws) = (0, 0);
    let local = |w: i32| SLOTS.iter().position(|&l| i32::from(l) == w);
    for t in 0..1500 {
        let keys: Vec<Keys> = (0..MAX_LOCALS).map(|k| script(k, t)).collect();
        let before: Vec<u32> = s.world.worms().iter().map(|w| w.rope.throws()).collect();
        assert!(s.tick_locals(&keys));
        let after: Vec<u32> = s.world.worms().iter().map(|w| w.rope.throws()).collect();
        let thrown = |i: usize| after[i] - before[i];
        heard += s.sounds().iter().filter(|e| is_throw(e)).count();
        bot_throws += thrown(1) + thrown(4);
        assert_eq!(
            s.sounds().iter().filter(|e| is_throw(e)).count() as u32,
            SLOTS.iter().map(|&i| thrown(usize::from(i))).sum::<u32>(),
            "tick {t}: the players' throws are heard, the bots' are not"
        );
        for e in s.events.iter() {
            match e.kind {
                EventKind::Kill { victim, killer } => {
                    if let Some(k) = local(victim) {
                        deaths[k] += 1;
                    }
                    if let Some(k) = local(killer).filter(|_| killer != victim) {
                        kills[k] += 1;
                    }
                }
                EventKind::Spawned { worm } => {
                    if let Some(k) = local(worm) {
                        spawns[k] += 1;
                    }
                }
                _ => {}
            }
        }
    }
    assert!(heard > 20, "every player throws its rope: {heard}");
    eprintln!("bot throws (not heard): {bot_throws}; kills {kills:?}, deaths {deaths:?}");
    // The tally, deaths and spawns by player.
    assert_eq!(s.tally.player_kills, kills);
    assert_eq!((s.player_deaths, s.player_spawns), (deaths, spawns));
    assert!(spawns.iter().all(|&n| n >= 1), "{spawns:?}");

    let end = s.world.checksum();
    let replay = s.take_replay().expect("a Deathmatch records");
    assert_eq!(replay.ticks(), 1500);
    assert_eq!(replay.setup.viewers, SLOTS, "the local slots, in local order");
    // Each worm pressed its own player's keys, never another's.
    for t in 0..replay.ticks() {
        for (k, &slot) in SLOTS.iter().enumerate() {
            assert_eq!(replay.row(t)[usize::from(slot)], script(k, u64::from(t)), "tick {t}, player {k}");
        }
    }
    let looks: Vec<_> = SLOTS.iter().map(|&i| replay.setup.looks[usize::from(i)].clone()).collect();
    assert_eq!(looks.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), names);
    assert_eq!(
        looks.iter().map(|l| l.skin.as_str()).collect::<Vec<_>>(),
        [setup::DEFAULT_SKIN, "olx-skin-ant", setup::DEFAULT_SKIN, "olx-skin-ant"]
    );
    assert!(looks.iter().all(|l| l.skin_hash != olr_replay::NO_SKIN_HASH), "each player's own pack");

    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("split.olrp");
    std::fs::write(&file, replay.encode()).unwrap();
    let r = setup::load_replay(&[dir.as_path()], &file, &[]).unwrap();
    assert!(r.local_slots().is_empty(), "nobody plays a replay");
    let mut r = session(&r);
    assert_eq!(r.present.views(), MAX_LOCALS, "a view on each local slot");
    for (v, &slot) in SLOTS.iter().enumerate() {
        assert_eq!(r.present.followed_view(v), Some(i32::from(slot)));
    }
    assert_eq!(SLOTS.map(|i| r.names.of(i32::from(i))).to_vec(), names);
    let mut heard_back = 0;
    while r.tick(Keys(Keys::FIRE)) {
        heard_back += r.sounds().iter().filter(|e| is_throw(e)).count();
    }
    assert_eq!(heard_back, heard, "the viewers' throws, as the game heard them");
    let Driver::Replay(d) = &r.driver else { panic!("a replay") };
    assert_eq!(d.mismatch, None);
    assert_eq!(d.cursor.checked(), replay.checksums.len(), "every checksum checked");
    assert_eq!(r.world.checksum(), end, "the same world");
}

/// Each player's weapon name shows over its own worm only, after its own
/// change (OLX's per-worm `bForceWeapon_Name`), or while its own change
/// key is held; a missing player's keys are none.
#[test]
fn weapon_names_are_each_players() {
    let Some(dir) = content_dir() else { return };
    let o = DeathmatchOptions { locals: 2, bots: 0, seed: 3, ..DeathmatchOptions::default() };
    let a = setup::load_deathmatch(&[dir.as_path()], "Classic", "Dirt Level", "default", &o).unwrap();
    assert_eq!(a.local_slots(), [0, 1]);
    assert_eq!(a.worms(), 2, "two players, no bot");
    let mut s = session(&a);
    assert_eq!((s.names.of(0), s.names.of(1)), ("Player 1".into(), "Player 2".into()));
    for _ in 0..30 {
        s.tick_locals(&[Keys(0), Keys(0)]);
    }
    assert!(s.with_world(|w| w.worms().iter().all(|w| w.alive)));
    let name = |s: &Session, i: usize| {
        let w = &s.world.worms()[i];
        s.world.sim_mod().weapon(w.current_slot().weapon).unwrap().name.clone()
    };
    assert_eq!((s.weapon_name_of(0, false), s.weapon_name_of(1, false)), (None, None));
    assert_eq!(s.weapon_name_of(1, true), Some((1, name(&s, 1))), "held: its own worm's");
    // Player 2 changes weapons; player 1 holds its change key and no arrow.
    s.tick_locals(&[Keys(Keys::CHANGE), Keys(Keys::CHANGE)]);
    s.tick_locals(&[Keys(Keys::CHANGE), Keys(Keys::CHANGE | Keys::RIGHT)]);
    s.tick_locals(&[Keys(0), Keys(0)]);
    assert_eq!(s.weapon_name_of(1, false), Some((1, name(&s, 1))), "player 2's change shows");
    assert_eq!(s.weapon_name_of(0, false), None, "player 1 changed nothing");
    assert_eq!(s.weapon_name(false), None, "the first player's");
    assert_eq!(s.weapon_name_of(2, true), None, "no third player");
    // One player's keys alone: the other presses none.
    s.tick(Keys(Keys::RIGHT));
    let replay = s.take_replay().unwrap();
    let last = replay.ticks() - 1;
    assert_eq!(replay.row(last), [Keys(Keys::RIGHT), Keys(0)]);
}
