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
            port_mapping: false,
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

/// The relays outlive the Multiplayer screen by `RELAY_LINGER` (30 s): a
/// visit within it reuses the pool (no reconnecting to every relay), the
/// pool closes after it and the selection goes with it.
#[test]
fn the_relays_linger_after_the_multiplayer_screen() {
    let relay = TestRelay::start();
    let empty = tempfile::tempdir().unwrap();
    let mut sh = menus(empty.path(), Store::in_memory(), "Anna", 1.0);
    use_relays(&mut sh, &[relay.url()]);
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    sh.act(UiAction::Open(Screen::Multiplayer));
    frame(&mut [&mut sh], at(0), &mut |_| Keys(0));
    assert_eq!(sh.relay_pools_opened(), 1);
    sh.data.selected_game = Some([5; 32]);
    sh.act(UiAction::Back);
    assert_eq!(sh.ui.screen(), Screen::Main);
    frame(&mut [&mut sh], at(10), &mut |_| Keys(0));
    assert!(sh.relays_open(), "lingering");
    assert_eq!(sh.data.selected_game, Some([5; 32]));
    sh.act(UiAction::Open(Screen::Multiplayer));
    frame(&mut [&mut sh], at(11), &mut |_| Keys(0));
    assert_eq!(sh.relay_pools_opened(), 1, "the visit reuses the pool");
    sh.act(UiAction::Back);
    frame(&mut [&mut sh], at(12), &mut |_| Keys(0));
    frame(&mut [&mut sh], at(41), &mut |_| Keys(0));
    assert!(sh.relays_open(), "29 s after leaving");
    frame(&mut [&mut sh], at(43), &mut |_| Keys(0));
    assert!(!sh.relays_open(), "closed 30 s after leaving");
    assert_eq!(sh.data.selected_game, None, "the selection goes with the pool");
    sh.act(UiAction::Open(Screen::Multiplayer));
    frame(&mut [&mut sh], at(44), &mut |_| Keys(0));
    assert_eq!(sh.relay_pools_opened(), 2);
}

/// A game's note, as a host would announce it, published on `relay` by a
/// pool of the test's own; returns once the relay holds it.
fn announce_on(relay: &TestRelay, id: &rl_lobby::Identity, name: &str, created_at: u64) {
    let summary = rl_discovery::GameSummary {
        v: rl_proto::PROTOCOL_VERSION,
        name: name.into(),
        mode: "deathmatch".into(),
        map: LEVEL.into(),
        map_name: "Dirt Level".into(),
        mod_id: MOD.into(),
        mod_hash: rl_proto::NO_HASH.into(),
        players: 1,
        max_players: 8,
        spectators: 0,
        max_spectators: 4,
        password: false,
        region: rl_discovery::Region::Europe,
        playing: false,
        addr: Some([127, 0, 0, 1].into()),
        port: 23400,
        closed: false,
    };
    let note = rl_discovery::announcement(id, &summary, rl_proto::FINGERPRINT, created_at, [7; 32]);
    let mut pool = rl_discovery::Pool::start(rl_discovery::PoolConfig {
        relays: vec![relay.url()],
        ..rl_discovery::PoolConfig::default()
    });
    pool.publish(note.clone());
    let start = Instant::now();
    while !relay.events().iter().any(|e| e.id == note.id) {
        assert!(start.elapsed() < Duration::from_secs(10), "the relay takes the note");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// Final review B-I2: relays edited in Options apply to the pool already
/// open (lingering or in use): it is closed and one on the new relays
/// opened, so a game announced only on the new relay is listed.
#[test]
fn relay_edits_apply_to_the_open_pool() {
    let (old, new) = (TestRelay::start(), TestRelay::start());
    let empty = tempfile::tempdir().unwrap();
    let mut sh = menus(empty.path(), Store::in_memory(), "Anna", 1.0);
    use_relays(&mut sh, &[old.url()]);
    sh.act(UiAction::Open(Screen::Multiplayer));
    frame(&mut [&mut sh], Instant::now(), &mut |_| Keys(0));
    assert_eq!(sh.relay_pools_opened(), 1);
    let host = rl_lobby::Identity::from_secret([42; 32]).unwrap();
    announce_on(&new, &host, "On the new relay", unix_now());
    // Options: the new relay instead, while the pool lingers.
    sh.act(UiAction::Back);
    frame(&mut [&mut sh], Instant::now(), &mut |_| Keys(0));
    use_relays(&mut sh, &[new.url()]);
    sh.act(UiAction::Open(Screen::Multiplayer));
    run_until("listed from the new relay", Duration::from_secs(10), &mut [&mut sh], None, |s| {
        listed(s[0], host.public()).is_some()
    });
    assert_eq!(sh.relay_pools_opened(), 2, "a pool on the new relays");
}

/// Final review B-I2: blocking and unblocking in Options apply to the open
/// list at once: a blocked host's game goes, and an unblocked host's next
/// note lists it again (the list never kept a blocked host's notes).
#[test]
fn block_edits_apply_to_the_open_list() {
    let relay = TestRelay::start();
    let empty = tempfile::tempdir().unwrap();
    let mut sh = menus(empty.path(), Store::in_memory(), "Anna", 1.0);
    use_relays(&mut sh, &[relay.url()]);
    let host = rl_lobby::Identity::from_secret([43; 32]).unwrap();
    let t = unix_now();
    announce_on(&relay, &host, "Den", t);
    sh.act(UiAction::Open(Screen::Multiplayer));
    run_until("listed", Duration::from_secs(10), &mut [&mut sh], None, |s| {
        listed(s[0], host.public()).is_some()
    });
    let set_blocked = |sh: &mut Shell, blocked: Vec<String>| {
        let f = FindingSettings { blocked, ..sh.ui.settings().online.finding.clone() };
        sh.act(UiAction::SetFinding(f));
    };
    set_blocked(&mut sh, vec![rl_ui::key_hex(&host.public())]);
    run_until("hidden", Duration::from_secs(5), &mut [&mut sh], None, |s| {
        listed(s[0], host.public()).is_none()
    });
    set_blocked(&mut sh, Vec::new());
    announce_on(&relay, &host, "Den again", t + 12);
    run_until("listed again", Duration::from_secs(10), &mut [&mut sh], None, |s| {
        listed(s[0], host.public()).is_some_and(|g| g.name == "Den again")
    });
    assert_eq!(sh.relay_pools_opened(), 1, "blocking needs no new pool");
}

/// Final review B-I2: a Public host whose relays change in Options
/// announces on the new relays at once, and its invite hints them.
#[test]
fn a_public_host_follows_relay_edits() {
    let Some(dir) = content_dir() else { return };
    let (old, new) = (TestRelay::start(), TestRelay::start());
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    use_relays(&mut h, &[old.url()]);
    host_as(&mut h, Visibility::Public);
    run_until("announced on the old relay", Duration::from_secs(10), &mut [&mut h], None, |_| {
        !old.events().is_empty()
    });
    use_relays(&mut h, &[new.url()]);
    run_until("announced on the new relay", Duration::from_secs(10), &mut [&mut h], None, |_| {
        !new.events().is_empty()
    });
    let invite = Invite::decode(h.data.lobby.as_ref().unwrap().invite.as_deref().unwrap()).unwrap();
    assert_eq!(invite.relay_urls(), [new.url()], "the invite hints the new relays");
}

/// Final review B-I3: an Invite-only host is told when its code has no
/// address (no STUN server, none configured), as a Public host is.
#[test]
fn an_invite_only_game_without_an_address_says_so() {
    let Some(dir) = content_dir() else { return };
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    host_as(&mut h, Visibility::Invite);
    let port = h.local_port().unwrap();
    run_until("the lobby's lines", Duration::from_secs(5), &mut [&mut h], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.invite.is_some())
    });
    let lobby = h.data.lobby.clone().unwrap();
    assert_eq!(Invite::decode(lobby.invite.as_deref().unwrap()).unwrap().addr, None);
    assert_eq!(
        lobby.listing.as_deref(),
        Some(
            format!("This code has no address: forward port {port} or set the public address in Options")
                .as_str()
        )
    );
}
