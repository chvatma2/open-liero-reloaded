//! Finding games from the menus (plan 12a, spec §7.1, N39-N43): the
//! Internet tab against rl-discovery's in-process test relay, a dedicated
//! server in-process announcing to it, invites, and relays that cannot be
//! reached. Nothing leaves the machine: every relay is the test relay or a
//! closed local port, and no STUN server is asked. Classic on Dirt Level
//! from `RL_CONTENT_DIR`; skipped with a notice without it.

use std::net::{SocketAddr, TcpListener};
use std::time::{Duration, Instant};

use rl_content::Content;
use rl_discovery::{Invite, TestRelay};
use rl_game::shell::Shell;
use rl_online::peer::WRONG_HOST;
use rl_server::Server;
use rl_server::config::Config;
use rl_sim::Keys;
use rl_ui::{FindingSettings, HostPrefs, InternetEntry, InternetStatus, Screen, Store, UiAction, Visibility};

use crate::online::{LEVEL, MOD, content_dir, frame, members, menus};

/// `sh`'s relays are `relays` (and no STUN server).
fn use_relays(sh: &mut Shell, relays: &[String]) {
    let f = FindingSettings {
        relays: Some(relays.to_vec()),
        stun: Some(Vec::new()),
        ..sh.ui.settings().online.finding.clone()
    };
    sh.act(UiAction::SetFinding(f));
}

/// Hosts a game of `visibility` from the Host screen.
fn host_as(sh: &mut Shell, visibility: Visibility) {
    sh.act(UiAction::Open(Screen::Multiplayer));
    sh.act(UiAction::Open(Screen::Host));
    sh.act(UiAction::Host(HostPrefs { visibility, ..HostPrefs::default() }));
    assert_eq!(sh.ui.screen(), Screen::Lobby, "hosting opens the lobby: {:?}", sh.ui.message());
}

/// Frames of `shells` and polls of `server` on the real clock until
/// `done`; the slowest frame.
fn run_until(
    what: &str,
    limit: Duration,
    shells: &mut [&mut Shell],
    mut server: Option<&mut Server>,
    mut done: impl FnMut(&mut [&mut Shell]) -> bool,
) -> Duration {
    let start = Instant::now();
    let mut slowest = Duration::ZERO;
    loop {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        let now = Instant::now();
        if let Some(s) = server.as_deref_mut() {
            s.poll(now, &mut std::io::sink());
        }
        let t = Instant::now();
        frame(shells, now, &mut |_| Keys(0));
        slowest = slowest.max(t.elapsed());
        if done(shells) {
            return slowest;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn listed(sh: &Shell, key: [u8; 32]) -> Option<InternetEntry> {
    sh.data.internet.games.iter().find(|g| g.key == key).cloned()
}

/// A server on Dirt Level announcing on `relay` from 127.0.0.1.
fn server_config(relay: &str) -> Config {
    let text = format!(
        r#"(
            name: "Office server",
            visibility: Public,
            relays: ["{relay}"],
            stun: [],
            public_address: Some("127.0.0.1"),
            region: Europe,
            port: 0,
            max_players: 8,
            rotation: [(mod: "{MOD}", level: "{LEVEL}", time_limit: 1.0, kill_limit: -1, lives: -2, bonuses: false)],
        )"#
    );
    Config::parse(&text).unwrap()
}

/// Plan 12a Task 10: a dedicated server announces itself to a relay; the
/// game's Internet tab, pointed at it, lists it (its name, map, region,
/// address); selecting it measures its ping; a Join expecting another key
/// at its address is refused (N37); Join expecting the listed key reaches
/// its lobby; and its invite code joins it from the Direct tab.
#[test]
fn a_listed_server_is_joined_from_the_internet_tab() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let data = tempfile::tempdir().unwrap();
    let mut server = Server::start(
        server_config(&relay.url()),
        Content::open(&dir).unwrap(),
        data.path(),
        &mut std::io::sink(),
    )
    .unwrap();
    let (key, port) = (server.key(), server.local_port());
    let mut sh = menus(&dir, Store::in_memory(), "Anna", 1.0);
    use_relays(&mut sh, &[relay.url()]);
    sh.act(UiAction::Open(Screen::Multiplayer));
    run_until("the server is listed", Duration::from_secs(15), &mut [&mut sh], Some(&mut server), |s| {
        listed(s[0], key).is_some()
    });
    assert_eq!(sh.data.internet.status, InternetStatus::Listed);
    let game = listed(&sh, key).unwrap();
    assert_eq!((game.name.as_str(), game.map.as_str()), ("Office server", "Dirt Level"));
    assert_eq!(game.region, rl_ui::Region::Europe);
    assert_eq!(game.addr, Some(SocketAddr::from(([127, 0, 0, 1], port))));
    assert_eq!(game.incompatible, None);
    // Selected: its ping is measured on a thread.
    sh.data.selected_game = Some(key);
    run_until("its ping", Duration::from_secs(5), &mut [&mut sh], Some(&mut server), |s| {
        listed(s[0], key).is_some_and(|g| matches!(g.ping, rl_ui::PingView::Ms(_)))
    });
    // Another key at its address: refused.
    let addr = game.addr.unwrap();
    sh.act(UiAction::JoinListed { addr, key: [9; 32], password: None });
    run_until("the impostor refused", Duration::from_secs(10), &mut [&mut sh], Some(&mut server), |s| {
        !s[0].ui.is_connecting()
    });
    assert_eq!(sh.ui.message(), Some(WRONG_HOST));
    // The listed key: the lobby.
    sh.act(UiAction::JoinListed { addr, key, password: None });
    run_until("the lobby", Duration::from_secs(10), &mut [&mut sh], Some(&mut server), |s| {
        s[0].ui.screen() == Screen::Lobby && members(s[0]) == 1
    });
    assert_eq!(sh.data.lobby.as_ref().unwrap().name, "Office server");
    assert_eq!(sh.data.lobby.as_ref().unwrap().invite, None, "a client shows no invite");
    sh.act(UiAction::LeaveLobby);
    assert_eq!(sh.ui.screen(), Screen::Multiplayer);
    // The server's invite code, typed in any case, joins it too.
    let code = server.invite().encode().to_uppercase();
    sh.act(UiAction::Join { addr: code, password: None });
    run_until("the lobby by invite", Duration::from_secs(10), &mut [&mut sh], Some(&mut server), |s| {
        s[0].ui.screen() == Screen::Lobby && members(s[0]) == 1
    });
    sh.act(UiAction::LeaveLobby);
    // Blocked: gone from the list for good.
    sh.act(UiAction::BlockHost(key));
    assert!(listed(&sh, key).is_none());
    assert_eq!(sh.ui.settings().online.finding.blocked, [rl_ui::key_hex(&key)]);
    server.shutdown(&mut std::io::sink());
}

/// Review Focus 3: a Public game without a public address (no STUN
/// server, none configured) is still hosted and announced; the Internet
/// tab lists it as "cannot be joined yet", its invite has no address and
/// says so, the lobby says why it is not listed, and a direct join works.
#[test]
fn a_public_game_without_an_address_is_listed_unjoinable() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    use_relays(&mut h, &[relay.url()]);
    host_as(&mut h, Visibility::Public);
    let port = h.local_port().unwrap();
    run_until("the lobby's lines", Duration::from_secs(5), &mut [&mut h], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.invite.is_some() && l.listing.is_some())
    });
    let lobby = h.data.lobby.clone().unwrap();
    assert_eq!(
        lobby.listing.as_deref(),
        Some(format!("Not listed: no public address (forward port {port} or set it in Options)").as_str())
    );
    let invite = Invite::decode(lobby.invite.as_deref().unwrap()).unwrap();
    assert_eq!(invite.addr, None, "no address to give");
    assert_eq!(invite.relay_urls(), [relay.url()]);

    let mut c = menus(&dir, Store::in_memory(), "Anna", 1.0);
    use_relays(&mut c, &[relay.url()]);
    c.act(UiAction::Open(Screen::Multiplayer));
    run_until("the game is listed", Duration::from_secs(20), &mut [&mut h, &mut c], None, |s| {
        listed(s[1], invite.key).is_some()
    });
    let game = listed(&c, invite.key).unwrap();
    assert_eq!(game.addr, None, "cannot be joined yet");
    assert_eq!(game.name, "Hosty");
    // Its invite cannot be joined either, and says why.
    c.act(UiAction::Join { addr: lobby.invite.clone().unwrap(), password: None });
    assert_eq!(c.ui.message(), Some(rl_game::shell::INVITE_WITHOUT_ADDRESS));
    // A damaged code says so.
    let mut damaged = lobby.invite.clone().unwrap();
    let last = damaged.pop().unwrap();
    damaged.push(if last == 'q' { 'p' } else { 'q' });
    c.act(UiAction::Join { addr: damaged, password: None });
    assert_eq!(c.ui.message(), Some("this invite code is damaged"));
    // The address still joins.
    c.act(UiAction::Join { addr: format!("127.0.0.1:{port}"), password: None });
    run_until("the direct join", Duration::from_secs(10), &mut [&mut h, &mut c], None, |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    // The host leaves: its closing note takes the game off the list.
    c.act(UiAction::LeaveLobby);
    h.act(UiAction::LeaveLobby);
    run_until("the game is gone", Duration::from_secs(10), &mut [&mut h, &mut c], None, |s| {
        listed(s[1], invite.key).is_none()
    });
}

/// Review Focus 4: relays that cannot be reached (a closed port; or none
/// set) leave hosting and direct joins as they were; the Internet tab says
/// "No relay reachable", and no frame waits for a relay.
#[test]
fn the_internet_tab_without_relays() {
    let Some(dir) = content_dir() else { return };
    let closed = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("ws://127.0.0.1:{}", l.local_addr().unwrap().port())
    };
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    use_relays(&mut h, std::slice::from_ref(&closed));
    host_as(&mut h, Visibility::Public);
    let port = h.local_port().unwrap();
    let mut c = menus(&dir, Store::in_memory(), "Anna", 1.0);
    use_relays(&mut c, std::slice::from_ref(&closed));
    c.act(UiAction::Open(Screen::Multiplayer));
    let slowest =
        run_until("no relay reachable", Duration::from_secs(10), &mut [&mut h, &mut c], None, |s| {
            s[1].data.internet.status == InternetStatus::NoRelay
        });
    assert!(slowest < Duration::from_millis(250), "a frame took {slowest:?}");
    c.act(UiAction::Join { addr: format!("127.0.0.1:{port}"), password: None });
    run_until("the direct join", Duration::from_secs(10), &mut [&mut h, &mut c], None, |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    assert!(h.data.lobby.as_ref().unwrap().listing.as_deref().is_some_and(|l| l.starts_with("Not listed")));
    // No relays at all: at once.
    let mut none = menus(&dir, Store::in_memory(), "Solo", 1.0);
    none.act(UiAction::Open(Screen::Multiplayer));
    frame(&mut [&mut none], Instant::now(), &mut |_| Keys(0));
    assert_eq!(none.data.internet.status, InternetStatus::NoRelay);
}
