//! The Multiplayer, Host and Lobby screens through their widgets
//! (`egui_kittest`, plan 10 task 11): what each click or key emits, what a
//! client may not change, and the state machine's online flow.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, LevelInfo, ModInfo};
use olr_ui::screens::{self, MenuData};
use olr_ui::screens::{HOST_LOST_REPLAY, HOST_LOST_TITLE, STEPPED_DOWN_TITLE};
use olr_ui::{
    AppCommand, DownloadView, HostPrefs, InternetEntry, InternetStatus, InternetView, LanEntry, LobbyCommand,
    LobbyView, MIGRATED_LISTING, MatchPrefs, MemberView, PHASE_LOBBY, PingView, Profiles, Region,
    SEAT_SPECTATOR, Screen, Settings, UiAction, UiState, Visibility, key_hex,
};

/// The Direct tab's field.
const ADDRESS: &str = "Address or invite code";

pub fn catalog() -> Catalog {
    Catalog {
        mods: vec![ModInfo {
            id: "olx-classic".into(),
            name: "Liero v1.0".into(),
            presets: vec!["Standard 100lt".into()],
        }],
        levels: (0..3)
            .map(|i| LevelInfo {
                id: format!("lvl-{i}"),
                name: format!("Level {i}"),
                width: 640,
                height: 480,
                thumb: None,
            })
            .collect(),
        ..Catalog::default()
    }
}

fn member(id: u8, name: &str, seat: &str, ready: bool) -> MemberView {
    MemberView {
        id,
        name: name.into(),
        fingerprint: format!("{id:02x}abcdef0123456789"),
        colour: [200, 80, 40],
        seat: seat.into(),
        ready,
        missing: Vec::new(),
        download: None,
        ping_ms: Some(23),
        admin: id == 0,
        path: None,
    }
}

/// A lobby of the host (member 0) and one guest (member 1); the view's
/// owner is `host` or the guest.
pub fn lobby_view(host: bool) -> LobbyView {
    LobbyView {
        name: "Martin's game".into(),
        me: if host { 0 } else { 1 },
        host,
        phase: PHASE_LOBBY.into(),
        members: vec![member(0, "Martin", "Player 1", true), member(1, "Guest", "Player 2", false)],
        setup: MatchPrefs::default(),
        can_start: Err("Guest is not ready".into()),
        downloads: Vec::new(),
        prompt: None,
        invite: None,
        listing: None,
        connection: None,
        reach: Vec::new(),
    }
}

pub struct App {
    pub ui: UiState,
    pub data: MenuData,
    pub commands: Vec<AppCommand>,
}

/// Runs the frames a screen needs to settle: a page whose footer sits at
/// the bottom reports its first frame's place until it has run twice more
/// (a pointer click lands where the tree says).
fn settle(h: &mut Harness<'static, App>) {
    h.run();
    h.step();
    h.step();
}

fn harness() -> Harness<'static, App> {
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let app = App { ui, data: MenuData::new(catalog(), None), commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

/// The lobby screen as `host` or a guest sees `view`.
pub fn in_lobby(app: &mut App, view: LobbyView) {
    for a in [
        UiAction::Open(Screen::Multiplayer),
        UiAction::Open(Screen::Host),
        UiAction::Host(HostPrefs::default()),
        UiAction::LobbyJoined,
    ] {
        app.ui.update(a);
    }
    app.commands.clear();
    app.data.lobby = Some(view);
}

fn lobby(host: bool) -> Harness<'static, App> {
    let mut h = harness();
    in_lobby(h.state_mut(), lobby_view(host));
    settle(&mut h);
    h
}

fn lobby_commands<'a>(h: &'a Harness<'static, App>) -> Vec<&'a LobbyCommand> {
    h.state()
        .commands
        .iter()
        .filter_map(|c| match c {
            AppCommand::Lobby(l) => Some(l),
            _ => None,
        })
        .collect()
}

#[test]
fn main_opens_multiplayer_and_back_returns() {
    let mut h = harness();
    h.get_by_label("Multiplayer").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
    h.get_by_label("Back").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Main);
}

#[test]
fn typing_an_address_and_joining_emits_join_game() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    settle(&mut h);
    h.get_by_label("Direct").click();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, ADDRESS).focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, ADDRESS).type_text("192.168.1.5:23400");
    settle(&mut h);
    h.get_by_role_and_label(Role::PasswordInput, "Password").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::PasswordInput, "Password").type_text("hunter2");
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Join").click();
    settle(&mut h);
    let cmds = &h.state().commands;
    assert!(
        matches!(&cmds[..], [AppCommand::JoinGame { addr: Some(addr), password: Some(p), expected_host: None }, AppCommand::SaveSettings(s)]
            if addr == "192.168.1.5:23400" && p.as_str() == "hunter2" && s.online.last_address == addr.as_str()),
        "{cmds:?}"
    );
    // Waiting for the app: Join does nothing more, and the address is kept.
    h.get_by_label("Connecting…");
    assert!(h.state().ui.is_connecting());
}

#[test]
fn the_last_address_is_remembered_and_failures_show() {
    let mut settings = Settings::default();
    settings.online.last_address = "10.0.0.7".into();
    let ui = UiState::new(settings, Profiles::default(), 1);
    let app = App { ui, data: MenuData::new(catalog(), None), commands: Vec::new() };
    let mut h = Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                app.ui.update(a);
            }
        },
        app,
    );
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    settle(&mut h);
    h.get_by_label("Direct").click();
    settle(&mut h);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, ADDRESS).value().as_deref(), Some("10.0.0.7"));
    h.state_mut().ui.update(UiAction::Join { addr: "10.0.0.7".into(), password: None });
    h.state_mut().ui.update(UiAction::OnlineFailed("Wrong password".into()));
    settle(&mut h);
    h.get_by_label("Wrong password");
    assert!(!h.state().ui.is_connecting());
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
}

#[test]
fn a_lan_entry_click_joins_and_a_locked_one_asks_for_the_password() {
    let mut h = harness();
    let addr: std::net::SocketAddr = "192.168.1.9:23400".parse().unwrap();
    let locked: std::net::SocketAddr = "192.168.1.10:23400".parse().unwrap();
    h.state_mut().data.lan_games = vec![
        LanEntry {
            name: "Open game".into(),
            map: "Dirt".into(),
            players: 2,
            max_players: 8,
            bots: 0,
            password: false,
            addr,
        },
        LanEntry {
            name: "Private".into(),
            map: "Dirt".into(),
            players: 1,
            max_players: 8,
            bots: 0,
            password: true,
            addr: locked,
        },
    ];
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    settle(&mut h);
    h.get_by_label("LAN").click();
    settle(&mut h);
    h.get_by_label_contains("Private").click();
    settle(&mut h);
    assert!(h.state().commands.is_empty(), "{:?}", h.state().commands);
    // The Direct tab, its address filled in, asks for the password.
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, ADDRESS).value().as_deref(),
        Some("192.168.1.10:23400")
    );
    h.get_by_label("LAN").click();
    settle(&mut h);
    h.get_by_label_contains("Open game").click();
    settle(&mut h);
    assert_eq!(
        h.state().commands,
        [AppCommand::JoinGame { addr: Some(addr.to_string()), password: None, expected_host: None }]
    );
}

#[test]
fn host_start_hosting_emits_host_game() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    settle(&mut h);
    h.get_by_label("Host a game").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Host);
    h.get_by_role_and_label(Role::TextInput, "Name").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Name").type_text("Office");
    settle(&mut h);
    h.get_by_label("Start hosting").click();
    settle(&mut h);
    let cmds = &h.state().commands;
    let [AppCommand::HostGame(p), AppCommand::SaveSettings(s)] = &cmds[..] else { panic!("{cmds:?}") };
    assert!(p.name.ends_with("Office"), "{:?}", p.name);
    assert_eq!((p.visibility, p.max_players, p.port), (Visibility::Lan, 8, 23400));
    assert_eq!(s.online.host, *p);
    // Back from Host returns to Multiplayer once the attempt is given up.
    h.get_by_label("Back").click();
    settle(&mut h);
    assert_eq!(h.state().commands.last(), Some(&AppCommand::LeaveOnline));
    assert_eq!(h.state().ui.screen(), Screen::Host);
    h.get_by_label("Back").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
}

#[test]
fn the_host_changes_the_setup_and_starts() {
    let mut h = lobby(true);
    // Time limit: on by default, the box switches it off.
    h.get_by_label("Time limit").click();
    settle(&mut h);
    let setups: Vec<&MatchPrefs> = lobby_commands(&h)
        .into_iter()
        .filter_map(|c| match c {
            LobbyCommand::Setup(m) => Some(m),
            _ => None,
        })
        .collect();
    assert!(setups.iter().any(|m| m.rules.time_limit < 0.0), "{:?}", h.state().commands);
    // Start is disabled while somebody is not ready; the reason is its tooltip.
    h.state_mut().commands.clear();
    let start = h.get_by_role_and_label(Role::Button, "Start");
    assert!(start.accesskit_node().is_disabled());
    start.click();
    settle(&mut h);
    assert!(lobby_commands(&h).is_empty());
    h.get_by_role_and_label(Role::Button, "Start").hover();
    settle(&mut h);
    h.get_by_label("Guest is not ready");
    // Force start is on, with its own tooltip, and emits.
    h.get_by_label("Force start").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::ForceStart]);
    // Once startable, Start emits.
    h.state_mut().commands.clear();
    h.state_mut().data.lobby.as_mut().unwrap().can_start = Ok(());
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Start").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Start]);
}

#[test]
fn force_start_needs_two_worms() {
    let mut h = harness();
    let mut v = lobby_view(true);
    v.members.truncate(1);
    v.setup.bots = 0;
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    assert!(h.get_by_role_and_label(Role::Button, "Force start").accesskit_node().is_disabled());
    h.state_mut().data.lobby.as_mut().unwrap().setup.bots = 1;
    settle(&mut h);
    assert!(!h.get_by_role_and_label(Role::Button, "Force start").accesskit_node().is_disabled());
}

#[test]
fn the_host_kicks_bans_and_moves_members() {
    let mut h = lobby(true);
    h.get_by_label("Kick").click();
    settle(&mut h);
    h.get_by_label("Ban").click();
    settle(&mut h);
    // Ban asks first; Kick did not.
    h.get_by_label("Ban Guest?");
    assert_eq!(lobby_commands(&h).len(), 1);
    h.get_by_label("Yes, ban").click();
    settle(&mut h);
    h.get_by_label("To spectators").click();
    settle(&mut h);
    assert_eq!(
        lobby_commands(&h),
        [
            &LobbyCommand::Kick { member: 1, ban: false },
            &LobbyCommand::Kick { member: 1, ban: true },
            &LobbyCommand::Move { member: 1, spectator: true },
        ]
    );
    // The members' rows: the fingerprint's 8 characters and the ping.
    h.get_by_label("01abcdef");
    assert_eq!(h.get_all_by_label("23 ms").count(), 2);
}

#[test]
fn a_spectator_is_moved_back_to_the_players() {
    let mut h = harness();
    let mut v = lobby_view(true);
    v.members[1].seat = SEAT_SPECTATOR.into();
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("To players").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Move { member: 1, spectator: false }]);
}

#[test]
fn a_client_sees_the_setup_read_only() {
    let mut h = lobby(false);
    h.get_by_label("Waiting for the host");
    assert!(h.query_by_label("Force start").is_none());
    assert!(h.query_by_label("Kick").is_none());
    assert!(h.get_by_label("Time limit").accesskit_node().is_disabled());
    h.get_by_label("Time limit").click();
    h.get_by_label("Add bot").click();
    settle(&mut h);
    assert!(lobby_commands(&h).is_empty(), "{:?}", h.state().commands);
    // The client's own toggles work.
    h.get_by_label("Ready").click();
    settle(&mut h);
    h.get_by_label("Spectate").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Ready(true), &LobbyCommand::Spectate(true)]);
}

#[test]
fn a_client_sees_the_hosts_true_time_limit() {
    let mut h = harness();
    let mut v = lobby_view(false);
    v.setup.rules.time_limit = 0.2;
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("0.2 min");
}

#[test]
fn weapons_random_and_the_weapon_screen() {
    let mut h = lobby(false);
    h.get_by_label("Random").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Loadout(None)]);
    // The weapon screen is the lobby's sub-page: Done answers the lobby.
    h.state_mut().ui.update(UiAction::Open(Screen::WeaponSelect));
    assert_eq!(h.state().ui.screen(), Screen::WeaponSelect);
    let c = h.state_mut().ui.update(UiAction::ChooseWeapons { weapons: [1, 2, 3, 4, 5], names: Vec::new() });
    assert_eq!(c, [AppCommand::Lobby(LobbyCommand::Loadout(Some([1, 2, 3, 4, 5])))]);
    assert_eq!(h.state().ui.screen(), Screen::Lobby);
    h.state_mut().ui.update(UiAction::Open(Screen::WeaponSelect));
    h.state_mut().ui.update(UiAction::Back);
    assert_eq!(h.state().ui.screen(), Screen::Lobby);
}

#[test]
fn chat_enter_sends_and_clears_the_field() {
    let mut h = lobby(false);
    h.state_mut().data.chat = vec![olr_ui::hud::LogLine::event("Guest joined")];
    settle(&mut h);
    h.get_by_label("Guest joined");
    h.get_by_role_and_label(Role::TextInput, "Chat").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Chat").type_text("hello there");
    settle(&mut h);
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Chat("hello there".into())]);
    let field = h.get_by_role_and_label(Role::TextInput, "Chat");
    assert!(field.value().as_deref().unwrap_or("").is_empty());
    // An empty line sends nothing.
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(lobby_commands(&h).len(), 1);
}

#[test]
fn back_from_the_lobby_leaves() {
    let mut h = lobby(true);
    h.get_by_label("Back").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::LeaveOnline]);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
    assert!(!h.state().ui.is_online());
}

#[test]
fn back_asks_first_while_a_match_runs() {
    let mut h = harness();
    let mut v = lobby_view(false);
    v.phase = "Playing".into();
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("Back").click();
    settle(&mut h);
    h.get_by_label("Leave the match?");
    assert!(h.state().commands.is_empty());
    assert_eq!(h.state().ui.screen(), Screen::Lobby);
    h.get_by_label("Stay").click();
    settle(&mut h);
    assert!(h.query_by_label("Leave the match?").is_none());
    h.get_by_label("Back").click();
    settle(&mut h);
    h.get_by_label("Leave").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::LeaveOnline]);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
}

#[test]
fn online_failures_return_with_the_reason() {
    let mut h = lobby(false);
    h.state_mut().ui.update(UiAction::OnlineFailed("Kicked by the host".into()));
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
    h.get_by_label("Kicked by the host");
    assert!(!h.state().ui.is_online());
}

#[test]
fn lobby_actions_do_nothing_outside_the_lobby() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    for a in [
        UiAction::LobbyStart,
        UiAction::LobbyReady(true),
        UiAction::LobbyChat("x".into()),
        UiAction::LeaveLobby,
    ] {
        assert!(ui.update(a).is_empty());
    }
    assert!(ui.update(UiAction::Host(HostPrefs::default())).is_empty(), "Host needs its screen");
    assert!(ui.update(UiAction::Join { addr: "a".into(), password: None }).is_empty());
    // A hand-edited setting is fixed on hosting.
    ui.update(UiAction::Open(Screen::Multiplayer));
    ui.update(UiAction::Open(Screen::Host));
    let wild = HostPrefs { max_players: 99, port: 0, ..HostPrefs::default() };
    let c = ui.update(UiAction::Host(wild));
    let [AppCommand::HostGame(p), ..] = &c[..] else { panic!("{c:?}") };
    assert_eq!((p.max_players, p.port), (32, 23400));
}

#[test]
fn ban_can_be_cancelled() {
    let mut h = lobby(true);
    h.get_by_label("Ban").click();
    settle(&mut h);
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert!(h.query_by_label("Ban Guest?").is_none());
    assert!(lobby_commands(&h).is_empty());
}

#[test]
fn escape_in_the_chat_field_does_not_leave_the_lobby() {
    let mut h = lobby(false);
    h.get_by_role_and_label(Role::TextInput, "Chat").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Chat").type_text("half a thought");
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Lobby);
    assert!(h.state().commands.is_empty(), "{:?}", h.state().commands);
    // With nothing focused, Esc is Back again.
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
}

#[test]
fn the_chat_draft_survives_the_weapon_screen_and_a_moot_dialog_closes() {
    let mut h = harness();
    let mut v = lobby_view(false);
    v.phase = "Playing".into();
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Chat").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Chat").type_text("draft");
    settle(&mut h);
    h.get_by_label("Back").click();
    settle(&mut h);
    h.get_by_label("Leave the match?");
    // The match ends: the question goes.
    h.state_mut().data.lobby.as_mut().unwrap().phase = PHASE_LOBBY.into();
    settle(&mut h);
    assert!(h.query_by_label("Leave the match?").is_none());
    h.state_mut().ui.update(UiAction::Open(Screen::WeaponSelect));
    settle(&mut h);
    h.state_mut().ui.update(UiAction::Back);
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Lobby);
    let field = h.get_by_role_and_label(Role::TextInput, "Chat");
    assert_eq!(field.value().as_deref(), Some("draft"));
}

#[test]
fn passwords_are_neither_printed_nor_saved() {
    let p = HostPrefs { password: "s3cret".into(), ..HostPrefs::default() };
    assert!(!format!("{p:?}").contains("s3cret"));
    let cmd =
        AppCommand::JoinGame { addr: Some("a".into()), password: Some("s3cret".into()), expected_host: None };
    assert!(!format!("{cmd:?}").contains("s3cret"));
    let ron = ron::to_string(&Settings {
        online: olr_ui::OnlineSettings { host: p, ..Default::default() },
        ..Settings::default()
    })
    .unwrap();
    assert!(!ron.contains("s3cret"), "{ron}");
    // Hosting with a password alone changing saves nothing.
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    ui.update(UiAction::Open(Screen::Multiplayer));
    ui.update(UiAction::Open(Screen::Host));
    let c = ui.update(UiAction::Host(HostPrefs {
        name: "x".into(),
        password: "pw".into(),
        ..HostPrefs::default()
    }));
    assert!(c.iter().any(|c| matches!(c, AppCommand::SaveSettings(s) if s.online.host.password.is_empty())));
}

#[test]
fn long_fields_are_cut() {
    let mut s = olr_ui::OnlineSettings { last_address: "a".repeat(400), ..Default::default() };
    s.host.password = "p".repeat(100).as_str().into();
    assert_eq!(s.sanitize().len(), 2);
    assert_eq!((s.last_address.len(), s.host.password.as_str().len()), (253, 64));
}

/// An online match (plan 10 task 12): the lobby's start goes in game (no
/// weapon selection: it was the lobby's), the results return to the
/// lobby, the next start goes in game again, and the menu's Leave leaves
/// the online game. Nothing restarts in place.
#[test]
fn an_online_match_runs_from_the_lobby_to_the_results_and_back() {
    let mut h = harness();
    in_lobby(h.state_mut(), lobby_view(false));
    let ui = &mut h.state_mut().ui;
    assert!(ui.update(UiAction::MatchStarted).is_empty());
    assert_eq!((ui.screen(), ui.game()), (Screen::InGame, Some(olr_ui::Game::Online)));
    assert!(ui.update(UiAction::Restart).is_empty(), "no rematch in place online");
    assert_eq!(
        ui.update(UiAction::LobbyChat(" gg ".into())),
        [AppCommand::Lobby(LobbyCommand::Chat("gg".into()))]
    );
    assert!(ui.update(UiAction::MatchOver).is_empty());
    assert_eq!(ui.screen(), Screen::Results);
    assert!(ui.update(UiAction::StartMatch).is_empty() && ui.update(UiAction::SaveReplay).is_empty());
    assert_eq!(ui.update(UiAction::LobbyChat("again".into())).len(), 1, "chat on the results too");
    assert_eq!(ui.update(UiAction::Back), [AppCommand::EndMatch]);
    assert_eq!((ui.screen(), ui.game()), (Screen::Lobby, None));
    assert!(ui.is_online());
    // The next match, from the lobby's weapon screen too.
    ui.update(UiAction::Open(Screen::WeaponSelect));
    ui.update(UiAction::MatchStarted);
    assert_eq!(ui.screen(), Screen::InGame);
    assert_eq!(ui.update(UiAction::Back), [AppCommand::SetPaused(true)], "the match's menu");
    assert_eq!(ui.update(UiAction::LeaveMatch), [AppCommand::EndMatch, AppCommand::LeaveOnline]);
    assert_eq!(ui.screen(), Screen::Multiplayer);
    assert!(!ui.is_online() && ui.game().is_none());
    assert!(ui.update(UiAction::MatchStarted).is_empty());
    assert_eq!(ui.screen(), Screen::Multiplayer, "no online match without a lobby");
}

/// The connection lost (or a kick) during the match ends it and returns
/// to Multiplayer with the reason.
#[test]
fn a_lost_connection_ends_the_online_match() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut app = App { ui: ui.clone(), data: MenuData::new(catalog(), None), commands: Vec::new() };
    in_lobby(&mut app, lobby_view(false));
    ui = app.ui;
    ui.update(UiAction::MatchStarted);
    ui.update(UiAction::Back);
    assert_eq!(ui.update(UiAction::OnlineFailed("the host left".into())), [AppCommand::EndMatch]);
    assert_eq!((ui.screen(), ui.message()), (Screen::Multiplayer, Some("the host left")));
    assert!(!ui.is_online() && !ui.is_paused() && ui.game().is_none());
}

/// Online, the chat key (the app's: `settings.online.chat_key`) opens a
/// field over the match (`MenuData::ingame_chat`): Enter sends the line and
/// closes it, Esc closes it without opening the menu. The menu closes it,
/// and a local game has none.
#[test]
fn the_chat_field_in_game() {
    let mut h = harness();
    in_lobby(h.state_mut(), lobby_view(false));
    h.state_mut().ui.update(UiAction::MatchStarted);
    settle(&mut h);
    assert!(h.query_by_role_and_label(Role::TextInput, "Say:").is_none());
    h.state_mut().data.ingame_chat = Some(String::new());
    settle(&mut h);
    let field = h.get_by_role_and_label(Role::TextInput, "Say:");
    assert_eq!(field.value().as_deref(), Some(""));
    h.get_by_role_and_label(Role::TextInput, "Say:").type_text("good game");
    settle(&mut h);
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Chat("good game".into())]);
    assert!(h.query_by_role_and_label(Role::TextInput, "Say:").is_none(), "Enter closes it");
    assert!(h.state().data.ingame_chat.is_none());
    h.state_mut().data.ingame_chat = Some(String::new());
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Say:").type_text("never mind");
    settle(&mut h);
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.query_by_role_and_label(Role::TextInput, "Say:").is_none(), "Esc closes it");
    assert!(!h.state().ui.is_paused(), "Esc in the field is not the menu");
    assert_eq!(lobby_commands(&h).len(), 1, "nothing more sent");
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(h.state().ui.is_paused(), "then Esc opens the menu");
    h.get_by_label("Leave the game");
    h.state_mut().data.ingame_chat = Some(String::new());
    settle(&mut h);
    assert!(h.state().data.ingame_chat.is_none(), "not over the menu");

    // A local game: no chat.
    let mut h = harness();
    let ui = &mut h.state_mut().ui;
    ui.update(UiAction::StartSandbox);
    ui.update(UiAction::MatchStarted);
    h.state_mut().data.ingame_chat = Some(String::new());
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::InGame);
    assert!(h.query_by_role_and_label(Role::TextInput, "Say:").is_none());
}

/// The chat key is T unless the settings say otherwise, never a key the
/// game keeps.
#[test]
fn the_chat_key_is_a_setting() {
    use olr_input::KeyCode;
    let mut s = olr_ui::OnlineSettings::default();
    assert_eq!(s.chat_key, KeyCode::KeyT);
    s.chat_key = KeyCode::KeyY;
    assert!(s.sanitize().is_empty());
    s.chat_key = KeyCode::Escape;
    assert_eq!(s.sanitize().len(), 1);
    assert_eq!(s.chat_key, olr_ui::CHAT_KEY);
    let ron =
        ron::to_string(&olr_ui::OnlineSettings { chat_key: KeyCode::KeyY, ..Default::default() }).unwrap();
    let back: olr_ui::OnlineSettings = ron::from_str(&ron).unwrap();
    assert_eq!(back.chat_key, KeyCode::KeyY);
}

/// An online match's results lead back to the lobby, nothing else.
#[test]
fn online_results_lead_back_to_the_lobby() {
    let mut h = harness();
    in_lobby(h.state_mut(), lobby_view(false));
    let app = h.state_mut();
    app.ui.update(UiAction::MatchStarted);
    app.ui.update(UiAction::MatchOver);
    app.data.results = Some(olr_ui::hud::Results {
        winner: None,
        reason: "time limit".into(),
        rows: Vec::new(),
        time_left_ms: None,
    });
    settle(&mut h);
    assert!(h.query_by_label("Rematch").is_none() && h.query_by_label("Save replay").is_none());
    h.get_by_label("Back to lobby").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Lobby);
    assert_eq!(h.state().commands, [AppCommand::EndMatch]);
}

fn download(name: &str, kind: &str, have: u64, size: u64, state: &str) -> DownloadView {
    DownloadView { name: name.into(), kind: kind.into(), have, size, state: state.into() }
}

/// Plan 11: the lobby lists the downloads under the members, one row each
/// (name, kind, a progress bar, the state); a member downloading shows its
/// percent instead of what it lacks; content over the prompt asks first:
/// Download lets it come, Leave leaves the lobby.
#[test]
fn lobby_shows_downloads_and_the_prompt() {
    let mut h = harness();
    let mut v = lobby_view(false);
    v.downloads = vec![
        download("olx-level-dirt-level", "level", 32768, 131_072, "downloading"),
        download("olx-theme-dirt", "theme", 0, 2048, "queued"),
        download("test-skin", "skin", 0, 900, "failed: the host stopped sending"),
    ];
    v.members[1].missing = vec!["olx-level-dirt-level".into()];
    v.members[1].download = Some(45);
    v.members[0].missing = vec!["olx-skin-x".into()];
    v.prompt = Some(73 << 20 | 400 << 10);
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("Downloads");
    for text in ["olx-level-dirt-level", "level", "olx-theme-dirt", "theme", "queued", "downloading", "skin"]
    {
        h.get_by_label(text);
    }
    h.get_by_label("failed: the host stopped sending");
    assert_eq!(h.get_all_by_role(Role::ProgressIndicator).count(), 3, "a progress bar a row");
    h.get_by_label("downloading 45%");
    h.get_by_label("missing: olx-skin-x");
    assert!(h.query_by_label("missing: olx-level-dirt-level").is_none(), "the percent instead");
    // The prompt.
    h.get_by_label("This game needs 73.4 MB of content from the host.");
    h.get_by_role_and_label(Role::Button, "Download").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::ApproveDownloads]);
    h.state_mut().commands.clear();
    h.get_by_role_and_label(Role::Button, "Leave").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::LeaveOnline]);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
    // Outside the lobby the prompt's answers do nothing.
    assert!(h.state_mut().ui.update(UiAction::LobbyDownload(true)).is_empty());
    assert!(h.state_mut().ui.update(UiAction::LobbyDownload(false)).is_empty());
    assert_eq!(olr_ui::megabytes(312 << 20), "312 MB");
}

/// Review Minor 7: the prompt's sizes are in the threshold's units
/// (MiB, shown as "MB"): the 50 MB prompt's floor reads "50.0 MB".
#[test]
fn megabytes_match_the_prompts_threshold() {
    assert_eq!(olr_ui::megabytes(50 * 1024 * 1024), "50.0 MB");
    assert_eq!(olr_ui::megabytes(73 << 20 | 400 << 10), "73.4 MB");
    assert_eq!(olr_ui::megabytes(0), "0.0 MB");
}

/// A player lacking required content cannot say ready: the box is
/// disabled ("get the missing content first") until it has it.
#[test]
fn ready_is_disabled_while_content_is_missing() {
    let mut h = harness();
    let mut v = lobby_view(false);
    v.members[1].missing = vec!["olx-level-dirt-level".into()];
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    assert!(h.get_by_label("Ready").accesskit_node().is_disabled());
    h.get_by_label("Ready").click();
    settle(&mut h);
    assert!(lobby_commands(&h).is_empty(), "{:?}", h.state().commands);
    h.get_by_label("Ready").hover();
    settle(&mut h);
    h.get_by_label("get the missing content first");
    // The host's own missing list is not this player's.
    h.state_mut().data.lobby.as_mut().unwrap().members[1].missing.clear();
    h.state_mut().data.lobby.as_mut().unwrap().members[0].missing = vec!["olx-skin-x".into()];
    settle(&mut h);
    assert!(!h.get_by_label("Ready").accesskit_node().is_disabled());
    h.get_by_label("Ready").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Ready(true)]);
}

// ---- Finding games (plan 12a) ----

fn entry(key: u8, name: &str) -> InternetEntry {
    InternetEntry {
        key: [key; 32],
        name: name.into(),
        map: "Dirt Level".into(),
        mod_name: "Liero v1.0".into(),
        players: 2,
        max_players: 8,
        bots: 0,
        ping: PingView::Unknown,
        region: Region::Europe,
        password: false,
        addr: Some(format!("81.2.69.{key}:23400").parse().unwrap()),
        incompatible: None,
        playing: false,
    }
}

/// The three tabs: Internet first (searching, then its games), LAN, and
/// Direct, whose one field takes an invite code as well as an address
/// (any case; the app tells them apart and the code is remembered).
#[test]
fn tabs_and_direct_invite() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    settle(&mut h);
    // Internet: searching until a relay sent its games.
    h.get_by_label("Searching…");
    assert!(h.query_by_label(ADDRESS).is_none(), "the Direct field is in its own tab");
    h.state_mut().data.internet =
        InternetView { status: InternetStatus::Listed, games: vec![entry(1, "Office DM")] };
    settle(&mut h);
    h.get_by_label("Office DM");
    h.get_by_label("Europe");
    // LAN: today's list.
    h.get_by_label("LAN").click();
    settle(&mut h);
    h.get_by_label("Searching…");
    assert!(h.query_by_label("Office DM").is_none());
    // Direct: an invite code in upper case.
    h.get_by_label("Direct").click();
    settle(&mut h);
    let code = "RLX1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNSQQQQQQQQQQQQ";
    h.get_by_role_and_label(Role::TextInput, ADDRESS).focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, ADDRESS).type_text(code);
    settle(&mut h);
    h.get_by_label_contains("An invite code");
    h.get_by_role_and_label(Role::Button, "Join").click();
    settle(&mut h);
    let cmds = &h.state().commands;
    assert!(
        matches!(&cmds[..], [AppCommand::JoinGame { addr: Some(addr), password: None, expected_host: None }, AppCommand::SaveSettings(s)]
            if addr == code && s.online.last_address == code),
        "{cmds:?}"
    );
    assert!(olr_ui::is_invite(" rlx1abc") && olr_ui::is_invite("Rlx1") && !olr_ui::is_invite("rlx"));
    assert!(!olr_ui::is_invite("192.168.1.5"));
    // A typo in the prefix still reads as a code (the game then says it is
    // damaged), never as an address to look up: a code is at least 60
    // letters and digits, no host name a single label that long.
    let typo = code.replacen("RLX1", "RKX1", 1);
    assert!(olr_ui::is_invite(&format!("{typo}{}", "Q".repeat(20))));
    assert!(olr_ui::is_invite(&format!("rlz1{}", "q".repeat(60))));
    for address in ["localhost", "my-server.example.org:23400", "office", &"a".repeat(59), "[::1]:23400"] {
        assert!(!olr_ui::is_invite(address), "{address}");
    }
}

/// The Internet list: a game of another version greyed with its version,
/// one without an address "cannot be joined yet" (Join disabled, its
/// reason on hover), a locked one asks for its password; Join expects the
/// listed key; "Block this host" hides it for good (saved as hex).
#[test]
fn internet_games_join_by_key_and_block() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    let mut locked = entry(1, "Locked");
    locked.password = true;
    locked.ping = PingView::Ms(42);
    let mut old = entry(2, "Old game");
    old.incompatible = Some("needs version 2-abcdef".into());
    let games = vec![locked.clone(), old];
    h.state_mut().data.internet = InternetView { status: InternetStatus::Listed, games };
    settle(&mut h);
    h.get_by_label("42 ms");
    h.get_by_label("needs version 2-abcdef");
    // Another version: selected, Join stays disabled.
    h.get_by_label("Old game").click();
    settle(&mut h);
    assert_eq!(h.state().data.selected_game, Some([2; 32]), "the app pings the selection");
    let join = h.get_by_role_and_label(Role::Button, "Join");
    assert!(join.accesskit_node().is_disabled());
    join.hover();
    settle(&mut h);
    assert_eq!(h.get_all_by_label("needs version 2-abcdef").count(), 2, "in its row and on Join's hover");
    // The locked one: its password, then Join expecting its key.
    h.get_by_label("Locked").click();
    settle(&mut h);
    h.get_by_role_and_label(Role::PasswordInput, "Game password").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::PasswordInput, "Game password").type_text("hunter2");
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Join").click();
    settle(&mut h);
    let cmds = h.state().commands.clone();
    let [AppCommand::JoinGame { addr: Some(addr), password: Some(p), expected_host: Some(k) }] = &cmds[..]
    else {
        panic!("{cmds:?}")
    };
    assert_eq!((addr.as_str(), p.as_str(), *k), ("81.2.69.1:23400", "hunter2", [1; 32]));
    assert_eq!(h.state().ui.settings().online.last_address, "", "a listed game is not remembered");
    // Final review B-M7: the password typed for one game is not offered to
    // another: a new selection clears it.
    h.state_mut().ui.update(UiAction::Back);
    h.state_mut().commands.clear();
    let mut other = entry(4, "Other locked");
    other.password = true;
    h.state_mut().data.internet.games.push(other);
    settle(&mut h);
    h.get_by_label("Other locked").click();
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Join").click();
    settle(&mut h);
    let cmds = h.state().commands.clone();
    let [AppCommand::JoinGame { password, expected_host: Some(k), .. }] = &cmds[..] else {
        panic!("{cmds:?}")
    };
    assert_eq!((password, *k), (&None, [4; 32]), "no password carried over");
    // Give up, then block the first one.
    h.state_mut().ui.update(UiAction::Back);
    h.state_mut().commands.clear();
    h.get_by_label("Locked").click();
    settle(&mut h);
    h.get_by_label("Block this host").click();
    settle(&mut h);
    let cmds = &h.state().commands;
    assert!(
        matches!(&cmds[..], [AppCommand::BlockHost(k), AppCommand::SaveSettings(s)]
            if *k == [1; 32] && s.online.finding.blocked == [key_hex(&[1; 32])]),
        "{cmds:?}"
    );
    // Blocking twice saves nothing more.
    let again = h.state_mut().ui.update(UiAction::BlockHost([1; 32]));
    assert_eq!(again, [AppCommand::BlockHost([1; 32])]);
}

/// The user's report of 2026-10-15: bots take no seat a joiner could
/// have, so a listed game shows its players over its seats and its bots
/// beside them ("1/8 +3 bots", plain "1/8" without bots), and only full
/// seats make it full, on the Internet tab and on the LAN.
#[test]
fn listed_games_show_bots_apart_and_only_seats_fill() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    let game = |key: u8, name: &str, players: u8, max_players: u8, bots: u8| InternetEntry {
        players,
        max_players,
        bots,
        ..entry(key, name)
    };
    let mut playing = game(2, "One bot", 1, 8, 1);
    playing.playing = true;
    let games = vec![
        game(1, "With bots", 1, 8, 3),
        playing,
        game(3, "No bots", 1, 8, 0),
        game(4, "Seats full", 2, 2, 0),
        game(5, "Bots past the seats", 1, 2, 20),
    ];
    h.state_mut().data.internet = InternetView { status: InternetStatus::Listed, games };
    settle(&mut h);
    h.get_by_label("1/8 +3 bots");
    h.get_by_label("1/8 +1 bot (playing)");
    h.get_by_label("1/8");
    h.get_by_label("1/2 +20 bots");
    // Bots never fill a game: Join is enabled.
    h.get_by_label("Bots past the seats").click();
    settle(&mut h);
    assert!(!h.get_by_role_and_label(Role::Button, "Join").accesskit_node().is_disabled());
    // Its seats taken, it is full.
    h.get_by_label("Seats full").click();
    settle(&mut h);
    let join = h.get_by_role_and_label(Role::Button, "Join");
    assert!(join.accesskit_node().is_disabled());
    join.hover();
    settle(&mut h);
    h.get_by_label("This game is full.");
    // The LAN list alike.
    let lan = |name: &str, players: u8, max_players: u8, bots: u8| LanEntry {
        name: name.into(),
        map: "Dirt".into(),
        players,
        max_players,
        bots,
        password: false,
        addr: "192.168.1.9:23400".parse().unwrap(),
    };
    h.state_mut().data.lan_games =
        vec![lan("Desk", 1, 8, 3), lan("Plain", 1, 8, 0), lan("Packed", 2, 2, 0), lan("Botty", 1, 2, 20)];
    h.get_by_label("LAN").click();
    settle(&mut h);
    let enabled =
        |h: &Harness<'static, App>, text: &str| !h.get_by_label(text).accesskit_node().is_disabled();
    assert!(enabled(&h, "Desk   Dirt   1/8 +3 bots"));
    assert!(enabled(&h, "Plain   Dirt   1/8"));
    assert!(!enabled(&h, "Packed   Dirt   2/2"), "its seats are taken");
    assert!(enabled(&h, "Botty   Dirt   1/2 +20 bots"), "bots never fill a game");
}

/// No relay connected: the Internet tab says so.
#[test]
fn the_internet_tab_says_no_relay_reachable() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    h.state_mut().data.internet = InternetView { status: InternetStatus::NoRelay, games: Vec::new() };
    settle(&mut h);
    h.get_by_label("No relay reachable");
    assert!(h.query_by_label("Searching…").is_none());
    h.state_mut().data.internet.status = InternetStatus::Listed;
    settle(&mut h);
    h.get_by_label("No games found");
}

/// The host's lobby shows its invite code, Copy puts it on the clipboard,
/// and a public game's listing line.
#[test]
fn the_lobby_shows_the_invite_and_copies_it() {
    let mut h = harness();
    let mut v = lobby_view(true);
    v.invite = Some("rlx1qqqsyqcyq5rqwzqfpg9scrgwpugpzysn".into());
    v.listing = Some("Listed on the Internet".into());
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("rlx1qqqsyqcyq5rqwzqfpg9scrgwpugpzysn");
    h.get_by_label("Listed on the Internet");
    h.get_by_label("Copy").click();
    h.step();
    let copied = h.output().platform_output.commands.iter().any(
        |c| matches!(c, egui::OutputCommand::CopyText(t) if t == "rlx1qqqsyqcyq5rqwzqfpg9scrgwpugpzysn"),
    );
    assert!(copied, "{:?}", h.output().platform_output.commands);
    // An Invite game whose code has no address says so beside its Copy
    // (final review B-I3).
    let mut h = harness();
    let mut v = lobby_view(true);
    v.invite = Some("rlx1qqqsyqcyq5rqwzqfpg9scrgwpugpzysn".into());
    v.listing =
        Some("This code has no address: forward port 23400 or set the public address in Options".into());
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("Copy");
    h.get_by_label_contains("This code has no address");
    // A client's lobby has none.
    let mut h = lobby(false);
    settle(&mut h);
    assert!(h.query_by_label("Copy").is_none());
}

/// The Host screen's three choices of who can join.
#[test]
fn the_host_chooses_who_can_join() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    h.state_mut().ui.update(UiAction::Open(Screen::Host));
    settle(&mut h);
    h.get_by_label("Who can join").click();
    settle(&mut h);
    h.get_by_label("Invite only");
    h.get_by_label("Anyone (Internet)").click();
    settle(&mut h);
    h.get_by_label("Start hosting").click();
    settle(&mut h);
    let cmds = &h.state().commands;
    let [AppCommand::HostGame(p), ..] = &cmds[..] else { panic!("{cmds:?}") };
    assert_eq!(p.visibility, Visibility::Public);
    // Plan 10's word reads as Invite.
    let old: HostPrefs = ron::from_str("(visibility: DirectOnly)").unwrap();
    assert_eq!(old.visibility, Visibility::Invite);
}

/// Plan 12b (N46): a listed game without an address is joinable through
/// the relays: Join is enabled and its action carries no address, only the
/// key; the waiting text is the join's step as the app reports it.
#[test]
fn an_addressless_internet_game_is_joinable() {
    let mut h = harness();
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    let mut behind = entry(3, "Behind a router");
    behind.addr = None;
    h.state_mut().data.internet = InternetView { status: InternetStatus::Listed, games: vec![behind] };
    settle(&mut h);
    // Its ping cannot be measured: the row says how it is reached.
    h.get_by_label("via relays");
    h.get_by_label("Behind a router").click();
    settle(&mut h);
    let join = h.get_by_role_and_label(Role::Button, "Join");
    assert!(!join.accesskit_node().is_disabled(), "joinable through the relays");
    join.click();
    settle(&mut h);
    let cmds = h.state().commands.clone();
    assert_eq!(
        cmds,
        [AppCommand::JoinGame { addr: None, password: None, expected_host: Some([3; 32]) }],
        "no address: the app joins the key through Nostr"
    );
    let action = UiAction::JoinListed { addr: None, key: [3; 32], password: None };
    assert!(h.state_mut().ui.update(action).is_empty(), "once, while it connects");
    h.get_by_label("Connecting…");
    h.state_mut().data.joining = Some("Asking the game through the relays…".into());
    settle(&mut h);
    h.get_by_label("Asking the game through the relays…");
    assert!(h.query_by_label("Connecting…").is_none(), "the step replaces the generic text");
}

/// Plan 12b (N47, N49): a host's lobby shows its reachability lines under
/// the listing line, a client's lobby its connection, and every member's
/// path tag beside its ping.
#[test]
fn the_lobby_shows_reach_lines_and_path_tags() {
    let mut h = harness();
    let mut v = lobby_view(true);
    v.invite = Some("rlx1qqqsyqcyq5rqwzqfpg9scrgwpugpzysn".into());
    v.listing = Some("Listed on the Internet".into());
    v.reach = vec![
        "Your router did not open port 23400 (no router answered): players connect by hole punching or a TURN relay"
            .into(),
        "Taking joins through 2 of 3 relays".into(),
    ];
    v.members[0].path = None;
    v.members[1].path = Some("relayed".into());
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("Listed on the Internet");
    h.get_by_label_contains("Your router did not open port 23400");
    h.get_by_label("Taking joins through 2 of 3 relays");
    h.get_by_label("relayed");
    // The listing line comes first, then the reach lines in order.
    let top = |h: &Harness<'static, App>, label: &str| {
        h.get_by_label_contains(label).accesskit_node().bounding_box().unwrap().y0
    };
    assert!(top(&h, "Listed on the Internet") < top(&h, "Your router did not open"));
    assert!(top(&h, "Your router did not open") < top(&h, "Taking joins"));
    // A client: its connection, and the host's tag.
    let mut h = harness();
    let mut v = lobby_view(false);
    v.connection = Some("connected by hole punching through both routers".into());
    v.members[1].path = Some("punched".into());
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label("connected by hole punching through both routers");
    h.get_by_label("punched");
    assert!(h.query_by_label("Copy").is_none(), "a client has no invite");
}

/// The menus of an online match playing (`in_lobby`, then its start).
fn in_match(app: &mut App) {
    while app.ui.screen() != Screen::Main {
        app.ui.update(UiAction::Back);
    }
    in_lobby(app, lobby_view(false));
    app.ui.update(UiAction::MatchStarted);
    assert_eq!(app.ui.screen(), Screen::InGame);
}

/// Plan 12c Task 9: when no player could take the match over, the app
/// reports `HostLost(why)`: the match ends, the Host lost screen says so,
/// why, and where the replay is; its Replays opens the Replays screen, its
/// Back (or Esc) the Multiplayer screen. Outside an online game it is
/// ignored.
#[test]
fn the_host_lost_screen_says_why_and_goes_back() {
    let why = "The host left and no other player could take over the game.";
    let lost = |h: &mut Harness<'static, App>, replay: bool| {
        let app = h.state_mut();
        in_match(app);
        app.data.host_lost = Some(why.into());
        app.data.host_lost_replay = replay;
        let c = app.ui.update(UiAction::HostLost(why.into()));
        assert_eq!(c, [AppCommand::EndMatch], "the match ends");
        assert_eq!((app.ui.screen(), app.ui.is_online(), app.ui.game()), (Screen::HostLost, false, None));
        settle(h);
    };
    let mut h = harness();
    lost(&mut h, true);
    h.get_by_label(HOST_LOST_TITLE);
    h.get_by_label(why);
    h.get_by_label(HOST_LOST_REPLAY);
    h.get_by_role_and_label(Role::Button, "Replays").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Replays);
    // Back: the Multiplayer screen; no replay (a watcher that joined
    // mid-match keeps none): not said.
    lost(&mut h, false);
    assert!(h.query_by_label(HOST_LOST_REPLAY).is_none(), "no replay, none said");
    h.get_by_role_and_label(Role::Button, "Back").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
    // A host that stepped down: its own title.
    lost(&mut h, true);
    assert!(h.query_by_label(STEPPED_DOWN_TITLE).is_none());
    h.state_mut().data.host_lost_hosting = true;
    settle(&mut h);
    h.get_by_label(STEPPED_DOWN_TITLE);
    assert!(h.query_by_label(HOST_LOST_TITLE).is_none());
    // Esc too.
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Multiplayer);
    // No online game: nothing happens.
    let app = h.state_mut();
    assert!(app.ui.update(UiAction::HostLost(why.into())).is_empty());
    assert_eq!(app.ui.screen(), Screen::Multiplayer);
}

/// Plan 12c Task 9: a player who took the match over hosts the lobby
/// after it as any host does (Start, Force start, kick, the setup), and
/// its listing line says the game is closed to newcomers
/// (`MIGRATED_LISTING`).
#[test]
fn a_migrated_lobby_shows_host_controls() {
    let mut h = harness();
    let mut v = lobby_view(true);
    v.listing = Some(MIGRATED_LISTING.into());
    v.can_start = Ok(());
    in_lobby(h.state_mut(), v);
    settle(&mut h);
    h.get_by_label(MIGRATED_LISTING);
    h.get_by_label("Kick");
    assert!(!h.get_by_role_and_label(Role::Button, "Force start").accesskit_node().is_disabled());
    h.get_by_role_and_label(Role::Button, "Start").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Start]);
    assert_eq!(MIGRATED_LISTING, "Taken over from the host: closed to new players");
}

/// Review Important 1 (plan 12c, N72): a late rejoiner's match is over
/// before it could watch: the app reports `MatchLeft`, the match ends and
/// the Lobby shows (the online game goes on, Ready works); outside an
/// online match it does nothing.
#[test]
fn a_match_left_shows_the_lobby() {
    let mut h = harness();
    let app = h.state_mut();
    in_match(app);
    assert_eq!(app.ui.update(UiAction::MatchLeft), [AppCommand::EndMatch]);
    assert_eq!((app.ui.screen(), app.ui.is_online(), app.ui.game()), (Screen::Lobby, true, None));
    assert!(app.ui.update(UiAction::MatchLeft).is_empty(), "once");
    settle(&mut h);
    h.get_by_label("Ready").click();
    settle(&mut h);
    assert_eq!(lobby_commands(&h), [&LobbyCommand::Ready(true)]);
}
