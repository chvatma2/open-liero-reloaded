//! `build_match` on OLX's content (`OLR_CONTENT_DIR`, skipped without it).

use std::path::Path;

use olr_bots::{BotConfig, Difficulty, Personality};
use olr_content::{Content, DEFAULT_PRESET, MatchLoadout, MatchOptions, Player, WeaponChoice};
use olr_modes::{DeathmatchSettings, RespawnLoadout};
use olr_replay::ModeSpec;
use olr_sim::{MatchSetup, SimSettings};

fn content() -> Option<Content> {
    let dir = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir());
    let Some(dir) = dir else {
        eprintln!("OLR_CONTENT_DIR not set; skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).unwrap())
}

/// A human, a Deathmatch bot and a chaos bot: the bots play worms 1 and 2
/// in worm order, only the chaos bot's loadout rotates, the human holds
/// weapons the restrictions enable, the recording has started, and the
/// same options give the same match.
#[test]
fn match_builder_seats_humans_and_bots() {
    let Some(c) = content() else { return };
    let chaos = BotConfig { personality: Personality::Chaos, ..BotConfig::default() };
    let o = MatchOptions {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-liero-factory".into(),
        seed: 11,
        settings: SimSettings::lx56(),
        players: vec![
            Player::Human,
            Player::Bot(BotConfig { difficulty: Difficulty::Hard, ..BotConfig::default() }),
            Player::Bot(chaos),
        ],
        deathmatch: DeathmatchSettings { kill_limit: 5, ..DeathmatchSettings::default() },
        weapons: WeaponChoice::Preset(DEFAULT_PRESET.into()),
        pool_capacity: MatchSetup::DEFAULT_POOL_CAPACITY,
    };
    let m = c.build_match(&o).unwrap();
    assert_eq!(m.world.worms().len(), 3);
    assert_eq!(m.bot_worms().collect::<Vec<_>>(), [1, 2]);
    let ModeSpec::Deathmatch(dm) = &m.mode else { panic!("a Deathmatch") };
    assert_eq!(dm.kill_limit, 5);
    let RespawnLoadout::ChaosCycle(cycle) = dm.respawn_loadout else { panic!("the chaos bot rotates") };
    assert_eq!(cycle.worms().collect::<Vec<_>>(), [2]);
    let enabled = c.weapon_rules("olx-classic", DEFAULT_PRESET).unwrap().enabled_weapons();
    let human: Vec<i32> = m.setup.worms[0].loadout.slots.iter().map(|s| s.weapon).collect();
    assert_eq!(human.len(), 5);
    assert!(human.iter().all(|w| enabled.contains(w)), "{human:?} of {enabled:?}");
    let replay = m.recorder.clone().finish(&m.world).unwrap();
    assert_eq!((replay.ticks(), replay.checksums.len()), (0, 1), "tick 0's checksum");
    // Without better looks the replay names the bots, in OLX's bot colours.
    let looks: Vec<_> = replay.setup.looks.iter().map(|l| (l.name.as_str(), l.colour)).collect();
    assert_eq!(looks, [("Player 1", [160, 160, 160]), ("Bot 2", [255, 0, 0]), ("Bot 3", [0, 255, 0])]);
    assert!(replay.setup.looks.iter().all(|l| l.skin == olr_content::DEFAULT_SKIN));
    let again = c.build_match(&o).unwrap();
    assert_eq!(again.world, m.world);
    assert_eq!(again.setup.worms[0].loadout, m.setup.worms[0].loadout);
    // A Deathmatch needs two worms (spec §20 F5).
    let one = MatchOptions { players: vec![Player::Human], ..o };
    assert!(c.build_match(&one).is_err());
}

/// The weapon selection's choice (spec §21 G4): the human starts with
/// exactly those weapons, in that order, and it is in the recording's
/// setup; a weapon the restrictions do not enable is refused.
#[test]
fn chosen_weapons_are_the_humans_and_recorded() {
    let Some(c) = content() else { return };
    let rules = c.weapon_rules("olx-classic", DEFAULT_PRESET).unwrap();
    let enabled = rules.enabled_weapons();
    let chosen = [enabled[3], enabled[0], enabled[0], enabled[7], enabled[1]];
    let o = MatchOptions {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-liero-factory".into(),
        seed: 5,
        settings: SimSettings::lx56(),
        players: vec![Player::HumanWith(chosen), Player::Bot(BotConfig::default())],
        deathmatch: DeathmatchSettings::default(),
        weapons: WeaponChoice::Preset(DEFAULT_PRESET.into()),
        pool_capacity: MatchSetup::DEFAULT_POOL_CAPACITY,
    };
    let m = c.build_match(&o).unwrap();
    let human: Vec<i32> = m.setup.worms[0].loadout.slots.iter().map(|s| s.weapon).collect();
    assert_eq!(human, chosen);
    assert_eq!(m.bot_worms().collect::<Vec<_>>(), [1]);
    let replay = m.recorder.clone().finish(&m.world).unwrap();
    let recorded: Vec<i32> = replay.setup.worms[0].loadout.slots.iter().map(|s| s.weapon).collect();
    assert_eq!(recorded, chosen, "the replay holds the choice");
    let banned = (0..rules.weapon_count() as i32).find(|w| !enabled.contains(w)).expect("a banned weapon");
    let mut bad = chosen;
    bad[2] = banned;
    let refused =
        MatchOptions { players: vec![Player::HumanWith(bad), Player::Bot(BotConfig::default())], ..o };
    let e = c.build_match(&refused).err().expect("refused").to_string();
    assert!(e.contains("worm 0"), "{e}");
}

/// The recording names the humans' worms as the replay's local slots
/// (issue #29): every worm that is no bot, in worm order, wherever they
/// sit; none for bots alone.
#[test]
fn the_recording_names_the_humans_worms() {
    let Some(c) = content() else { return };
    let bot = Player::Bot(BotConfig::default());
    let o = MatchOptions {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-liero-factory".into(),
        seed: 5,
        settings: SimSettings::lx56(),
        players: vec![bot, bot, Player::Human],
        deathmatch: DeathmatchSettings::default(),
        weapons: WeaponChoice::Preset(DEFAULT_PRESET.into()),
        pool_capacity: MatchSetup::DEFAULT_POOL_CAPACITY,
    };
    let viewers = |o: &MatchOptions| {
        let m = c.build_match(o).unwrap();
        m.recorder.clone().finish(&m.world).unwrap().setup.viewers
    };
    assert_eq!(viewers(&o), [2]);
    let split = MatchOptions { players: vec![Player::Human, bot, Player::Human, Player::Human], ..o.clone() };
    assert_eq!(viewers(&split), [0, 2, 3]);
    let alone = MatchOptions { players: vec![bot, bot], ..o };
    assert!(viewers(&alone).is_empty());
}

/// A loadout (issue #5): its loading time replaces the settings', natural
/// bonuses hold only its weapons, the bots pick within it, and with its
/// unique rule a human's repeat is refused while other weapons are free.
#[test]
fn a_loadout_sets_the_weapons_and_the_loading_time() {
    let Some(c) = content() else { return };
    let names = ["Rifle", "Bazooka", "Napalm"].map(String::from).to_vec();
    let loadout = MatchLoadout { name: "Three".into(), weapons: names, loading_time: 20, unique: true };
    let rules = c.rules("olx-classic", &WeaponChoice::Loadout(loadout.clone())).unwrap();
    let enabled = rules.enabled_weapons();
    assert_eq!(enabled.len(), 3);
    let o = MatchOptions {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-liero-factory".into(),
        seed: 5,
        settings: SimSettings { bonuses: true, ..SimSettings::lx56() },
        players: vec![Player::Human, Player::Bot(BotConfig::default())],
        deathmatch: DeathmatchSettings::default(),
        weapons: WeaponChoice::Loadout(loadout),
        pool_capacity: MatchSetup::DEFAULT_POOL_CAPACITY,
    };
    let m = c.build_match(&o).unwrap();
    assert_eq!(m.setup.settings.loading_time, 20);
    assert_eq!(m.setup.bonus_weapons.as_deref(), Some(&enabled[..]));
    for w in &m.setup.worms {
        let ids: Vec<i32> = w.loadout.slots.iter().map(|s| s.weapon).collect();
        assert!(ids.iter().all(|i| enabled.contains(i)), "{ids:?}");
        assert!(olr_data::loadouts::unique_ok(&ids, &enabled), "{ids:?}");
    }
    let human = |weapons| MatchOptions {
        players: vec![Player::HumanWith(weapons), Player::Bot(BotConfig::default())],
        ..o.clone()
    };
    let [a, b, d] = [enabled[0], enabled[1], enabled[2]];
    assert!(c.build_match(&human([a, b, d, a, b])).is_ok());
    let e = c.build_match(&human([a, a, b, b, b])).err().expect("refused").to_string();
    assert!(e.contains("twice"), "{e}");
    // Default: every weapon of the mod.
    let all = c.rules("olx-classic", &WeaponChoice::default_loadout()).unwrap();
    assert_eq!(all.enabled_weapons().len(), all.weapon_count());
}
