//! The screen state machine (spec §8.3, §21 G1): pure logic, no window.

use olr_content::Player;
use olr_input::{Action, Bindings, KeyCode};
use olr_ui::{
    AppCommand, AudioSettings, BotLevel, Profile, Profiles, Screen, Settings, UiAction, UiState, WindowPreset,
};

fn state() -> UiState {
    UiState::new(Settings::default(), Profiles::default(), 41)
}

fn saves_settings(c: &[AppCommand]) -> bool {
    c.iter().any(|c| matches!(c, AppCommand::SaveSettings(_)))
}

#[test]
fn state_machine_navigates_and_emits_commands() {
    let mut ui = state();
    assert_eq!(ui.screen(), Screen::Main);
    // Main's buttons open the menu screens; Back returns to Main.
    for s in Screen::MENUS {
        let c = ui.update(UiAction::Open(s));
        // The Report screen asks the app for the system's details.
        let expected = if s == Screen::Report { vec![AppCommand::PrepareReport] } else { Vec::new() };
        assert_eq!(c, expected, "{s:?}");
        assert_eq!(ui.screen(), s);
        assert!(ui.update(UiAction::Back).is_empty());
        assert_eq!(ui.screen(), Screen::Main);
    }
    // In-match screens are not opened from the menu, and Back on Main stays.
    for s in [Screen::InGame, Screen::Results, Screen::WeaponSelect, Screen::Main] {
        ui.update(UiAction::Open(s));
        assert_eq!(ui.screen(), Screen::Main);
    }
    ui.update(UiAction::Back);
    assert_eq!(ui.screen(), Screen::Main);

    // Local play: Start emits one StartMatch with the saved preferences; a
    // second press while it starts does nothing (no double matches).
    ui.update(UiAction::Open(Screen::LocalPlay));
    let mut prefs = ui.settings().gameplay.match_prefs.clone();
    prefs.bots = 2;
    prefs.difficulty = BotLevel::Hard;
    let c = ui.update(UiAction::SetMatch(prefs.clone()));
    assert!(saves_settings(&c), "{c:?}");
    let c = ui.update(UiAction::StartMatch);
    let [AppCommand::StartMatch(o)] = &c[..] else { panic!("{c:?}") };
    assert_eq!(o.seed, 41);
    assert_eq!((o.mod_id.as_str(), o.level_id.as_str()), (prefs.mod_id.as_str(), prefs.level_id.as_str()));
    assert_eq!(o.players.len(), 3);
    assert_eq!(o.players[0], Player::Human);
    assert!(matches!(o.players[1], Player::Bot(b) if b.difficulty == olr_bots::Difficulty::Hard));
    assert_eq!(o.deathmatch, prefs.rules.deathmatch());
    assert!(ui.is_starting());
    assert!(ui.update(UiAction::StartMatch).is_empty());
    assert!(ui.update(UiAction::Back).is_empty(), "the start is in flight");
    assert!(ui.update(UiAction::PadPause).is_empty(), "no pause out of a match");
    // The app reports the match ready: the weapon selection, then in game.
    ui.update(UiAction::MatchStarted);
    assert_eq!(ui.screen(), Screen::WeaponSelect);
    assert!(!ui.is_starting());
    // A stale report changes nothing.
    ui.update(UiAction::MatchStarted);
    assert_eq!(ui.screen(), Screen::WeaponSelect);
    let c = ui.update(UiAction::ChooseWeapons { weapons: [3; 5], names: vec![] });
    assert_eq!(c, [AppCommand::ArmPlayer([3; 5])], "no names: the profile is kept");
    assert_eq!(ui.screen(), Screen::InGame);
    assert!(ui.update(UiAction::ChooseWeapons { weapons: [1; 5], names: vec![] }).is_empty(), "once");

    // A pad's Start pauses and resumes as Esc (issue #28).
    assert_eq!(ui.update(UiAction::PadPause), [AppCommand::SetPaused(true)]);
    assert!(ui.is_paused());
    assert_eq!(ui.update(UiAction::PadPause), [AppCommand::SetPaused(false)]);
    assert!(!ui.is_paused());

    // Esc pauses and resumes; the pause menu leaves the match.
    assert_eq!(ui.update(UiAction::Back), [AppCommand::SetPaused(true)]);
    assert!(ui.is_paused());
    assert_eq!(ui.update(UiAction::Back), [AppCommand::SetPaused(false)]);
    ui.update(UiAction::Back);
    assert_eq!(ui.update(UiAction::LeaveMatch), [AppCommand::EndMatch]);
    assert_eq!((ui.screen(), ui.is_paused()), (Screen::Main, false));

    // A match that cannot start returns to local play with the message.
    ui.update(UiAction::Open(Screen::LocalPlay));
    let c = ui.update(UiAction::StartMatch);
    let [AppCommand::StartMatch(o)] = &c[..] else { panic!("{c:?}") };
    assert_eq!(o.seed, 42, "every match gets the next seed");
    ui.update(UiAction::MatchFailed("no level pack olx-level-gone".into()));
    assert_eq!(ui.screen(), Screen::LocalPlay);
    assert_eq!(ui.message(), Some("no level pack olx-level-gone"));
    assert!(!ui.is_starting());

    // The end of a match shows the results; a rematch starts the same
    // match on the next seed; Back leaves for the main menu.
    let armed = UiAction::ChooseWeapons { weapons: [0; 5], names: vec![] };
    ui.update(UiAction::StartMatch);
    assert_eq!(ui.message(), None);
    ui.update(UiAction::MatchStarted);
    assert!(ui.update(UiAction::MatchOver).is_empty(), "no end during the weapon selection");
    ui.update(armed.clone());
    ui.update(UiAction::MatchOver);
    assert_eq!(ui.screen(), Screen::Results);
    let c = ui.update(UiAction::StartMatch);
    assert!(matches!(&c[..], [AppCommand::EndMatch, AppCommand::StartMatch(o)] if o.seed == 44), "{c:?}");
    ui.update(UiAction::MatchStarted);
    ui.update(armed);
    // The match ends while paused: the app is told to resume.
    ui.update(UiAction::Back);
    assert_eq!(ui.update(UiAction::MatchOver), [AppCommand::SetPaused(false)]);
    assert!(!ui.is_paused());
    assert_eq!(ui.update(UiAction::Back), [AppCommand::EndMatch]);
    assert_eq!(ui.screen(), Screen::Main);

    // Replays play from the replays screen; one that fails returns there.
    ui.update(UiAction::Open(Screen::Replays));
    let file = std::path::PathBuf::from("a.olrp");
    assert_eq!(ui.update(UiAction::PlayReplay(file.clone())), [AppCommand::PlayReplay(file.clone())]);
    assert!(ui.update(UiAction::PlayReplay(file.clone())).is_empty());
    ui.update(UiAction::MatchFailed("no".into()));
    assert_eq!((ui.screen(), ui.message()), (Screen::Replays, Some("no")));
    ui.update(UiAction::PlayReplay(file));
    ui.update(UiAction::MatchStarted);
    assert_eq!(ui.game(), Some(olr_ui::Game::Replay));
    // F5 plays it again in place, not while paused; the results' Start
    // is no rematch for a replay.
    assert_eq!(ui.update(UiAction::Restart), [AppCommand::Restart]);
    ui.update(UiAction::Back);
    assert!(ui.update(UiAction::Restart).is_empty());
    ui.update(UiAction::Back);
    ui.update(UiAction::MatchOver);
    assert!(ui.update(UiAction::StartMatch).is_empty());
    assert_eq!(ui.update(UiAction::Restart), [AppCommand::Restart]);
    assert_eq!(ui.screen(), Screen::InGame);
    ui.update(UiAction::Back);
    ui.update(UiAction::LeaveMatch);
    assert_eq!(ui.game(), None);
    assert!(ui.update(UiAction::Restart).is_empty());

    // Options: a change is applied and saved; the same values again do nothing.
    ui.update(UiAction::Open(Screen::Options));
    let mut video = ui.settings().video.clone();
    video.window = WindowPreset::W1600x900;
    let c = ui.update(UiAction::SetVideo(video.clone()));
    assert_eq!(c[0], AppCommand::ApplyVideo(video.clone()));
    assert!(saves_settings(&c));
    assert!(ui.update(UiAction::SetVideo(video.clone())).is_empty());
    // A UI scale out of range is clamped before it is applied.
    video.ui_scale = 9.0;
    let c = ui.update(UiAction::SetVideo(video));
    assert!(matches!(&c[0], AppCommand::ApplyVideo(v) if v.ui_scale == 3.0), "{c:?}");
    let audio = AudioSettings { master: 0.25, ..ui.settings().audio.clone() };
    let c = ui.update(UiAction::SetAudio(audio.clone()));
    assert_eq!(c[0], AppCommand::ApplyAudio(audio));
    let mut keys = Bindings::olx();
    keys.keys.push((Action::Jump, KeyCode::Space));
    let c = ui.update(UiAction::SetBindings(keys.clone()));
    assert_eq!(c[0], AppCommand::ApplyBindings(keys.clone()));

    // Profiles: selecting one with its own bindings applies them.
    ui.update(UiAction::Back);
    ui.update(UiAction::Open(Screen::Profiles));
    let mut profiles = ui.profiles().clone();
    profiles.list.push(Profile { bindings: Some(Bindings::olx()), ..Profile::default() });
    profiles.selected = 1;
    let c = ui.update(UiAction::SetProfiles(profiles.clone()));
    assert!(c.contains(&AppCommand::SaveProfiles(profiles)), "{c:?}");
    assert!(c.contains(&AppCommand::ApplyBindings(Bindings::olx())), "{c:?}");
    assert_eq!(ui.bindings(), Bindings::olx());

    assert_eq!(ui.update(UiAction::Quit), [AppCommand::Quit]);
}
