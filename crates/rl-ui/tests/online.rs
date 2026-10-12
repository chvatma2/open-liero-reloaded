//! The Multiplayer, Host and Lobby screens through their widgets
//! (`egui_kittest`, plan 10 task 11): what each click or key emits, what a
//! client may not change, and the state machine's online flow.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use rl_content::{Catalog, LevelInfo, ModInfo};
use rl_ui::screens::{self, MenuData};
use rl_ui::{
    AppCommand, HostPrefs, LanEntry, LobbyCommand, LobbyView, MatchPrefs, MemberView, PHASE_LOBBY, Profiles,
    SEAT_SPECTATOR, Screen, Settings, UiAction, UiState, Visibility,
};

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
        ping_ms: Some(23),
        admin: id == 0,
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
    h.get_by_label("Searching…");
    h.get_by_role_and_label(Role::TextInput, "Address").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::TextInput, "Address").type_text("192.168.1.5:23400");
    settle(&mut h);
    h.get_by_role_and_label(Role::PasswordInput, "Password").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::PasswordInput, "Password").type_text("hunter2");
    settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Join").click();
    settle(&mut h);
    let cmds = &h.state().commands;
    assert!(
        matches!(&cmds[..], [AppCommand::JoinGame { addr, password: Some(p) }, AppCommand::SaveSettings(s)]
            if addr == "192.168.1.5:23400" && p == "hunter2" && s.online.last_address == addr.as_str()),
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
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Address").value().as_deref(), Some("10.0.0.7"));
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
            password: false,
            addr,
        },
        LanEntry {
            name: "Private".into(),
            map: "Dirt".into(),
            players: 1,
            max_players: 8,
            password: true,
            addr: locked,
        },
    ];
    h.state_mut().ui.update(UiAction::Open(Screen::Multiplayer));
    settle(&mut h);
    h.get_by_label_contains("Private").click();
    settle(&mut h);
    assert!(h.state().commands.is_empty(), "{:?}", h.state().commands);
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Address").value().as_deref(),
        Some("192.168.1.10:23400")
    );
    h.get_by_label_contains("Open game").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::JoinGame { addr: addr.to_string(), password: None }]);
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
    h.state_mut().data.chat = vec![rl_ui::hud::LogLine::event("Guest joined")];
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
