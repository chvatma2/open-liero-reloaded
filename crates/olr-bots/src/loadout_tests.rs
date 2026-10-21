use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use olr_data::restrictions::{Preset, WeaponRules, WeaponState};
use olr_math::GlibcRand;
use olr_sim::{MAX_SLOTS, SimSettings};

use super::*;

const CLASSIC: [&str; 8] =
    ["Bazooka", "Super Shotgun", "Napalm", "Cannon", "Doomsday", "Chaingun", "Rifle", "Minigun"];

/// Rules for mod `name` with weapons `weapons` and one preset `P1` that
/// bans `banned` (every other mod weapon enabled) and lists `extra` as enabled.
fn rules(name: &str, weapons: &[&str], banned: &[&str], extra: &[&str]) -> WeaponRules {
    let mut p = Preset::new();
    for b in banned {
        p.insert(b.to_string(), WeaponState::Banned);
    }
    for e in extra {
        p.insert(e.to_string(), WeaponState::Enabled);
    }
    let presets: BTreeMap<String, Preset> = [("Preset 1".to_string(), p)].into_iter().collect();
    WeaponRules::from_parts(name, weapons.iter().copied(), &presets, "Preset 1")
}

fn id(rules: &WeaponRules, name: &str) -> i32 {
    rules.find_weapon_id(name)
}

#[test]
fn bot_presets_match_olx() {
    let preset = |r: &WeaponRules, lt: i32| {
        let mut rng = BotRng::from_seed(3);
        let w = olx_bot_weapons(r, lt, &mut rng);
        (w, rng.calls())
    };

    // 100lt: Classic at LoadingTime 100 with all five enabled, no draws.
    let classic = rules("Liero v1.0", &CLASSIC, &["Bazooka", "Minigun"], &[]);
    let lt100 = ["Super Shotgun", "Napalm", "Cannon", "Doomsday", "Chaingun"].map(|n| id(&classic, n));
    assert_eq!(preset(&classic, 100), (BotWeapons { weapons: lt100, game_type: GameType::Lt100 }, 0));
    // "Liero" anywhere in the name will do for 100lt ("Liero v1.11").
    let liero = rules("Liero v1.11", &CLASSIC, &[], &[]);
    assert_eq!(preset(&liero, 100).0.game_type, GameType::Lt100);
    // One of the five banned: random weapons, all enabled and distinct.
    let no_doomsday = rules("Liero v1.0", &CLASSIC, &["Doomsday"], &[]);
    let (w, calls) = preset(&no_doomsday, 100);
    assert_eq!(w.game_type, GameType::Other);
    assert_eq!(calls, MAX_SLOTS as u64);
    assert!(w.weapons.iter().all(|&i| i >= 0 && i != id(&no_doomsday, "Doomsday")));
    assert!(distinct(&w.weapons));
    // Another LoadingTime: random.
    assert_eq!(preset(&classic, 99).0.game_type, GameType::Other);

    // Rifles: Classic ("Classic" or "Liero v1.0" in the name) at 16..=25 %.
    let rifle = id(&classic, "Rifle");
    for (name, lt) in [("Liero v1.0", 16), ("Classic 2", 25)] {
        let r = rules(name, &CLASSIC, &[], &[]);
        assert_eq!(
            preset(&r, lt),
            (BotWeapons { weapons: [rifle; MAX_SLOTS], game_type: GameType::Rifles }, 0)
        );
    }
    assert_eq!(preset(&classic, 15).0.game_type, GameType::Other);
    assert_eq!(preset(&classic, 26).0.game_type, GameType::Other);
    // "Liero v1.11" is no rifle mod, and 20 % is no 100lt: random.
    assert_eq!(preset(&liero, 20).0.game_type, GameType::Other);
    // Rifle banned: random weapons without it.
    let no_rifle = rules("Liero v1.0", &CLASSIC, &["Rifle"], &[]);
    let (w, _) = preset(&no_rifle, 20);
    assert_eq!(w.game_type, GameType::Other);
    assert!(!w.weapons.contains(&rifle));

    // Mortars: MW 1.0 below 50 %.
    let mw_weapons = ["Bazooka", "Mortar Launcher", "Flak", "Twin Rockets", "Ripper", "Laser"];
    let mw = rules("Modern Warfare1.0", &mw_weapons, &[], &[]);
    let mortar = id(&mw, "Mortar Launcher");
    assert_eq!(
        preset(&mw, 49),
        (BotWeapons { weapons: [mortar; MAX_SLOTS], game_type: GameType::Mortars }, 0)
    );
    assert_eq!(preset(&mw, 50).0.game_type, GameType::Other);
    // MW's default preset bans the Mortar Launcher: random.
    let mw_std = rules("MW 1.0 beta", &mw_weapons, &["Mortar Launcher"], &[]);
    let (w, _) = preset(&mw_std, 40);
    assert_eq!(w.game_type, GameType::Other);
    assert!(!w.weapons.contains(&mortar));

    // A preset weapon the list enables but the mod lacks: FindWeaponId gives
    // -1, and the slot gets a random enabled weapon (one draw each).
    let no_chaingun = rules("Liero v1.0", &CLASSIC[..5], &[], &["Chaingun"]);
    let (w, calls) = preset(&no_chaingun, 100);
    assert_eq!(w.game_type, GameType::Lt100);
    assert_eq!(
        &w.weapons[..4],
        &["Super Shotgun", "Napalm", "Cannon", "Doomsday"].map(|n| id(&no_chaingun, n))
    );
    assert_eq!(calls, 1);
    let mut rng = GlibcRand::new(3);
    let enabled = no_chaingun.enabled_weapons();
    let pick = (rng.random_pos_num() * enabled.len() as f32) as usize;
    assert_eq!(w.weapons[4], enabled[pick.min(enabled.len() - 1)]);
    let no_rifle_in_mod = rules("Liero v1.0", &CLASSIC[..6], &[], &["Rifle"]);
    let (w, calls) = preset(&no_rifle_in_mod, 20);
    assert_eq!(w.game_type, GameType::Rifles);
    assert_eq!(calls, MAX_SLOTS as u64);
    assert!(w.weapons.iter().all(|&i| (0..6).contains(&i)));
}

fn distinct(w: &[i32]) -> bool {
    w.iter().enumerate().all(|(i, a)| !w[..i].contains(a))
}

#[test]
fn random_weapons_follow_restrictions() {
    // Distinct allowed weapons while there are enough, one draw per slot.
    let mut enabled = vec![false; 38];
    for i in [0, 3, 5, 6, 9, 10, 12, 20, 25, 30, 33, 37] {
        enabled[i] = true;
    }
    for seed in 0..200 {
        let mut rng = BotRng::from_seed(seed);
        let w = get_random_weapons(&enabled, &mut rng);
        assert_eq!(rng.calls(), MAX_SLOTS as u64);
        assert!(w.iter().all(|&i| i >= 0 && enabled[i as usize]), "{w:?}");
        assert!(distinct(&w), "{w:?}");
        // Slot 0 is the first allowed weapon from max(1, GetRandomInt(37)), wrapping.
        let start = GlibcRand::new(seed).random_int(37).max(1) as usize;
        let first = (0..38).map(|k| (start + k) % 38).find(|&n| enabled[n]).unwrap();
        assert_eq!(w[0], first as i32);
    }
    // Two allowed of three: the third slot on repeats the last allowed
    // duplicate the scan passed (weapon 0), whatever the draws.
    for seed in 0..20 {
        assert_eq!(get_random_weapons(&[true, false, true], &mut BotRng::from_seed(seed)), [2, 0, 0, 0, 0]);
    }
    // One weapon: every slot.
    assert_eq!(get_random_weapons(&[true], &mut BotRng::from_seed(1)), [0; MAX_SLOTS]);
    // Nothing allowed: empty slots, still one draw each.
    let mut rng = BotRng::from_seed(1);
    assert_eq!(get_random_weapons(&[false; 4], &mut rng), [-1; MAX_SLOTS]);
    assert_eq!(rng.calls(), MAX_SLOTS as u64);
    // No weapons at all: OLX returns before drawing.
    let mut rng = BotRng::from_seed(1);
    assert_eq!(get_random_weapons(&[], &mut rng), [-1; MAX_SLOTS]);
    assert_eq!(rng.calls(), 0);

    // getRandomEnabledWpn: -1 without a draw when nothing is enabled.
    let mut rng = BotRng::from_seed(5);
    assert_eq!(random_enabled_weapon(&[], &mut rng), -1);
    assert_eq!(rng.calls(), 0);
    let picks: Vec<i32> = (0..50).map(|_| random_enabled_weapon(&[4, 9], &mut rng)).collect();
    assert!(picks.contains(&4) && picks.contains(&9) && picks.iter().all(|p| [4, 9].contains(p)));

    // A Random-policy bot under restrictions; a chaos bot ignores them.
    let r = rules("Liero v1.0", &CLASSIC, &["Bazooka", "Napalm", "Cannon", "Doomsday", "Chaingun"], &[]);
    let s = SimSettings::lx56();
    let cfg = BotConfig { loadout: LoadoutPolicy::Random, ..BotConfig::default() };
    let l = choose_loadout(&cfg, &r, &s, &mut BotRng::from_seed(9));
    let ids: Vec<i32> = l.slots.iter().map(|s| s.weapon).collect();
    assert_eq!(ids.len(), MAX_SLOTS);
    // Three allowed weapons: the scan repeats one of them in the last slots.
    assert!(ids.iter().all(|&i| r.enabled_weapons().contains(&i)), "{ids:?}");
    let chaos = BotConfig { personality: Personality::Chaos, ..BotConfig::default() };
    let banned_hits = (0..50)
        .flat_map(|seed| choose_loadout(&chaos, &r, &s, &mut BotRng::from_seed(seed)).slots)
        .filter(|s| !r.enabled_weapons().contains(&s.weapon))
        .count();
    assert!(banned_hits > 0);
    // The default bot on Classic at 100 % with its five enabled: the 100lt set, full and ready.
    let l = choose_loadout(&BotConfig::default(), &classic_rules(), &s, &mut BotRng::from_seed(9));
    assert_eq!(l.slots.iter().map(|s| s.weapon).collect::<Vec<_>>(), [1, 2, 3, 4, 5]);
    assert!(l.slots.iter().all(|s| s.charge == 1.0 && !s.reloading));
}

fn classic_rules() -> WeaponRules {
    rules("Liero v1.0", &CLASSIC, &[], &[])
}

#[test]
fn unique_loadouts_keep_bots_from_repeating() {
    use olr_data::loadouts::{MatchLoadout, unique_ok};
    // Three of eight enabled at 20 % (Rifles game type for a Classic mod
    // name): OLX's pick repeats; with unique every slot until the three are
    // used differs, deterministically per seed.
    let names = ["Rifle", "Bazooka", "Napalm"].map(String::from).to_vec();
    let mut l = MatchLoadout { name: "Three".into(), weapons: names, loading_time: 20, unique: false };
    let plain = WeaponRules::from_loadout("Liero v1.0", CLASSIC, &l);
    l.unique = true;
    let unique = WeaponRules::from_loadout("Liero v1.0", CLASSIC, &l);
    assert!(unique.unique() && !plain.unique());
    let s = SimSettings { loading_time: 20, ..SimSettings::lx56() };
    let pick = |r: &WeaponRules, seed| -> Vec<i32> {
        let l = choose_loadout(&BotConfig::default(), r, &s, &mut BotRng::from_seed(seed));
        l.slots.iter().map(|s| s.weapon).collect()
    };
    let enabled = unique.enabled_weapons();
    for seed in 0..50 {
        let w = pick(&unique, seed);
        assert!(w.iter().all(|i| enabled.contains(i)), "{w:?}");
        assert!(unique_ok(&w, &enabled), "{w:?}");
        assert!(enabled.iter().all(|e| w.contains(e)), "{w:?}");
        assert_eq!(w, pick(&unique, seed));
    }
    assert!((0..50).any(|seed| !distinct(&pick(&plain, seed)[..3])));

    // Nothing repeats: no draw, the pick unchanged.
    let mut w = [0, 1, 2, 3, 4];
    let mut rng = BotRng::from_seed(1);
    unique_weapons(&mut w, &[0, 1, 2, 3, 4, 5], &mut rng);
    assert_eq!((w, rng.calls()), ([0, 1, 2, 3, 4], 0));
    // Repeats take unused weapons until none is left.
    let mut w = [1, 1, 1, 1, 1];
    unique_weapons(&mut w, &[1, 6], &mut rng);
    assert_eq!(w, [1, 6, 1, 1, 1]);
}
