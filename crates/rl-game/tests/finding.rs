//! Finding games from the menus (plan 12a, spec §7.1, N39-N43): the
//! Internet tab against rl-discovery's in-process test relay, a dedicated
//! server in-process announcing to it, invites, and relays that cannot be
//! reached; plan 12b: joins through the relays (N46), the lobby's paths
//! and reachability, and the router's mapping deleted at quitting (NT-2).
//! Nothing leaves the machine (ruling NT-1): every relay is the test relay
//! or a closed local port, no STUN server is asked, every endpoint's
//! candidates are on 127.0.0.1, and a router is a loopback `FakeGateway`.
//! Classic on Dirt Level from `RL_CONTENT_DIR`; skipped with a notice
//! without it.

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::time::{Duration, Instant};

use rl_content::Content;
use rl_discovery::{Invite, TestRelay};
use rl_game::shell::Shell;
use rl_online::peer::WRONG_HOST;
use rl_server::Server;
use rl_server::config::Config;
use rl_sim::Keys;
use rl_ui::{FindingSettings, HostPrefs, InternetEntry, InternetStatus, Screen, Store, UiAction, Visibility};

use crate::online::{LEVEL, MOD, app_of, content_dir, frame, loopback, members, menus};

/// `sh`'s relays are `relays` (and no STUN server, no port mapping).
fn use_relays(sh: &mut Shell, relays: &[String]) {
    let f = FindingSettings {
        relays: Some(relays.to_vec()),
        stun: Some(Vec::new()),
        port_mapping: false,
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

/// A server on Dirt Level announcing on `relay` from 127.0.0.1 (`public`:
/// its public address setting; else it has none), no STUN server, no port
/// mapping, its candidates on 127.0.0.1 (the test gateway's pinned base;
/// the gateway itself, a closed loopback port, is never asked).
fn server_config(relay: &str, public: bool) -> Config {
    let address = if public { r#"public_address: Some("127.0.0.1"),"# } else { "" };
    let text = format!(
        r#"(
            name: "Office server",
            visibility: Public,
            relays: ["{relay}"],
            stun: [],
            port_mapping: false,
            {address}
            region: Europe,
            port: 0,
            max_players: 8,
            rotation: [(mod: "{MOD}", level: "{LEVEL}", time_limit: 1.0, kill_limit: -1, lives: -2, bonuses: false)],
        )"#
    );
    let mut cfg = Config::parse(&text).unwrap();
    let closed = SocketAddr::from((Ipv4Addr::LOCALHOST, 9));
    cfg.test_gateway =
        Some(rl_server::config::TestGateway { pmp: closed, ssdp: closed, base: Ipv4Addr::LOCALHOST });
    cfg
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
        server_config(&relay.url(), true),
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
    let addr = game.addr;
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

/// Plan 12b (N46, N47): a Public game without an address (no mapping, no
/// public address setting) is listed and joined through the relays: the
/// host's lobby says how it is reached, its invite (without an address)
/// joins, as does the listed game; the joiner's lobby says how it is
/// connected and the host tags it; a damaged code still says so, a direct
/// join still works, and the closing note takes the game off the list.
#[test]
fn a_public_game_without_an_address_is_joined_through_the_relays() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    use_relays(&mut h, &[relay.url()]);
    host_as(&mut h, Visibility::Public);
    let port = h.local_port().unwrap();
    run_until("listed, taking joins", Duration::from_secs(10), &mut [&mut h], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| {
            l.listing.as_deref() == Some("Listed on the Internet")
                && l.reach.iter().any(|r| r == "Taking joins through 1 of 1 relays")
        })
    });
    let lobby = h.data.lobby.clone().unwrap();
    assert_eq!(
        lobby.reach,
        [
            "Port mapping is off: players connect by hole punching or a TURN relay",
            "Taking joins through 1 of 1 relays"
        ]
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
    assert_eq!(game.addr, None, "joined through the relays");
    assert_eq!(game.name, "Hosty");
    // Its invite joins through the relays; the waiting text follows.
    c.act(UiAction::Join { addr: lobby.invite.clone().unwrap(), password: None });
    assert!(c.ui.is_connecting(), "{:?}", c.ui.message());
    let mut steps = Vec::new();
    run_until("the lobby by invite", Duration::from_secs(15), &mut [&mut h, &mut c], None, |s| {
        if let Some(step) = &s[1].data.joining
            && !steps.contains(step)
        {
            steps.push(step.clone());
        }
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    assert!(steps.contains(&"Asking the game through the relays…".to_string()), "{steps:?}");
    run_until("the paths", Duration::from_secs(5), &mut [&mut h, &mut c], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.members.iter().any(|m| m.path.is_some()))
            && s[1].data.lobby.as_ref().is_some_and(|l| l.connection.is_some())
    });
    let connection = c.data.lobby.as_ref().unwrap().connection.clone().unwrap();
    assert_eq!(connection, format!("connected directly to 127.0.0.1:{port}"));
    let tags: Vec<_> = h.data.lobby.as_ref().unwrap().members.iter().map(|m| m.path.clone()).collect();
    assert_eq!(tags, [None, Some("direct".to_string())], "the host has no path, its joiner's is direct");
    assert_eq!(c.data.lobby.as_ref().unwrap().reach, Vec::<String>::new(), "a client has no reach lines");
    c.act(UiAction::LeaveLobby);
    // The listed game joins through the relays too.
    c.act(UiAction::JoinListed { addr: None, key: invite.key, password: None });
    run_until("the lobby from the list", Duration::from_secs(15), &mut [&mut h, &mut c], None, |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    c.act(UiAction::LeaveLobby);
    // A damaged code says so.
    let mut damaged = lobby.invite.clone().unwrap();
    let last = damaged.pop().unwrap();
    damaged.push(if last == 'q' { 'p' } else { 'q' });
    c.act(UiAction::Join { addr: damaged, password: None });
    assert_eq!(c.ui.message(), Some("this invite code is damaged"));
    // The address still joins.
    run_until("the host forgets the joiner", Duration::from_secs(10), &mut [&mut h, &mut c], None, |s| {
        members(s[0]) == 1
    });
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

/// Plan 12b (N46): a dedicated server without an address (no public
/// address setting, no port mapping, no STUN server) is listed on the
/// relay; the game's Internet tab, its pool on that relay, selects it and
/// Join reaches its lobby through Nostr, connected directly to its
/// loopback port.
#[test]
fn a_listed_server_without_an_address_is_joined_through_nostr() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let data = tempfile::tempdir().unwrap();
    let mut server = Server::start(
        server_config(&relay.url(), false),
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
    let game = listed(&sh, key).unwrap();
    assert_eq!(game.addr, None, "no address");
    sh.data.selected_game = Some(key);
    sh.act(UiAction::JoinListed { addr: game.addr, key, password: None });
    assert!(sh.ui.is_connecting(), "{:?}", sh.ui.message());
    run_until("the lobby", Duration::from_secs(15), &mut [&mut sh], Some(&mut server), |s| {
        s[0].ui.screen() == Screen::Lobby && members(s[0]) == 1
    });
    run_until("the connection", Duration::from_secs(5), &mut [&mut sh], Some(&mut server), |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.connection.is_some())
    });
    let lobby = sh.data.lobby.clone().unwrap();
    assert_eq!(lobby.name, "Office server");
    assert_eq!(lobby.connection, Some(format!("connected directly to 127.0.0.1:{port}")));
    assert_eq!(lobby.members[0].path.as_deref(), Some("direct"), "the server's view of this player");
    sh.act(UiAction::LeaveLobby);
    server.shutdown(&mut std::io::sink());
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
/// announces on the new relays at once, and its invite hints them; plan
/// 12b: it listens on them too, so its invite (without an address) joins
/// through the new relay alone.
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
    let code = h.data.lobby.as_ref().unwrap().invite.clone().unwrap();
    let invite = Invite::decode(&code).unwrap();
    assert_eq!(invite.relay_urls(), [new.url()], "the invite hints the new relays");
    assert_eq!(invite.addr, None);
    run_until("listening on the new relay", Duration::from_secs(10), &mut [&mut h], None, |s| {
        s[0].data
            .lobby
            .as_ref()
            .is_some_and(|l| l.reach.iter().any(|r| r == "Taking joins through 1 of 1 relays"))
    });
    // A joiner whose own relays are none: the invite's hint alone.
    let mut c = menus(&dir, Store::in_memory(), "Anna", 1.0);
    c.act(UiAction::Open(Screen::Multiplayer));
    c.act(UiAction::Join { addr: code, password: None });
    run_until("joined through the new relay", Duration::from_secs(15), &mut [&mut h, &mut c], None, |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
}

/// A typed host name is looked up on a thread, never in the frame: the
/// Join returns at once, still connecting without a peer, and the lobby
/// follows a few frames later.
#[test]
fn a_typed_name_is_looked_up_off_the_frame() {
    let Some(dir) = content_dir() else { return };
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    host_as(&mut h, Visibility::Lan);
    let port = h.local_port().unwrap();
    let mut c = menus(&dir, Store::in_memory(), "Anna", 1.0);
    c.act(UiAction::Open(Screen::Multiplayer));
    c.act(UiAction::Join { addr: format!("localhost:{port}"), password: None });
    assert!(c.ui.is_connecting(), "{:?}", c.ui.message());
    assert_eq!(c.local_port(), None, "no peer before the name is looked up");
    run_until("the lobby", Duration::from_secs(10), &mut [&mut h, &mut c], None, |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    // Back while it is looked up gives up: nothing joins afterwards.
    c.act(UiAction::LeaveLobby);
    c.act(UiAction::Join { addr: format!("localhost:{port}"), password: None });
    c.act(UiAction::Back);
    assert!(!c.ui.is_connecting());
    run_until("frames", Duration::from_secs(5), &mut [&mut h, &mut c], None, |_| true);
    std::thread::sleep(Duration::from_millis(200));
    frame(&mut [&mut h, &mut c], Instant::now(), &mut |_| Keys(0));
    assert_eq!(c.local_port(), None, "the lookup's answer joins nothing");
}

/// Final review B-I3, plan 12b: an Invite-only host is told how its code
/// joins: without an address through its relays, and with no relays
/// either, that it joins nothing.
#[test]
fn an_invite_only_game_without_an_address_says_so() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    use_relays(&mut h, &[relay.url()]);
    host_as(&mut h, Visibility::Invite);
    run_until("the lobby's lines", Duration::from_secs(10), &mut [&mut h], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.invite.is_some() && l.listing.is_some())
    });
    let lobby = h.data.lobby.clone().unwrap();
    assert_eq!(Invite::decode(lobby.invite.as_deref().unwrap()).unwrap().addr, None);
    assert_eq!(lobby.listing.as_deref(), Some("This code joins through the relays"));
    h.act(UiAction::LeaveLobby);
    // No relays: nothing to join through.
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    host_as(&mut h, Visibility::Invite);
    run_until("the lobby's lines", Duration::from_secs(10), &mut [&mut h], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.invite.is_some() && l.listing.is_some())
    });
    assert_eq!(
        h.data.lobby.as_ref().unwrap().listing.as_deref(),
        Some("This code has no address and no relays: it joins nothing (set relays in Options)")
    );
}

/// Ruling NT-2: leaving a hosted game starts deleting its router mapping
/// without blocking the frame, and the exit waits for the router (here a
/// loopback `FakeGateway` that deletes only 300 ms after the request,
/// `LateUpnp`), at most a second, after the shell is gone.
#[test]
fn quitting_deletes_the_router_mapping() {
    use rl_net::portmap::fake::{FakeGateway, Misbehaviour};
    let Some(dir) = content_dir() else { return };
    // The external address on loopback too (NT-1).
    let gw = FakeGateway::start(true, false, false, Ipv4Addr::new(127, 0, 0, 2), Misbehaviour::LateUpnp);
    let (pmp, ssdp) = (gw.pmp(), gw.ssdp());
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    h.test_ice(move |ice| {
        loopback(ice);
        let SocketAddr::V4(pmp) = pmp else { unreachable!() };
        ice.gateway = Some(pmp);
        ice.ssdp = Some(ssdp);
        ice.allow_local = true;
    });
    let f = FindingSettings { port_mapping: true, ..h.ui.settings().online.finding.clone() };
    h.act(UiAction::SetFinding(f));
    host_as(&mut h, Visibility::Invite);
    let port = h.local_port().unwrap();
    let open =
        format!("Port {port} is open on your router (UPnP): players join directly at 127.0.0.2:{port}");
    run_until("the port mapped", Duration::from_secs(10), &mut [&mut h], None, |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.reach.contains(&open))
    });
    assert_eq!(gw.mappings().len(), 2, "TCP and UDP");
    let lobby = h.data.lobby.clone().unwrap();
    assert_eq!(lobby.listing, None, "the code has the address");
    let invite = Invite::decode(lobby.invite.as_deref().unwrap()).unwrap();
    assert_eq!(invite.addr, Some(SocketAddr::from(([127, 0, 0, 2], port))), "N47: the mapping's address");
    // Leaving returns at once; the router still holds the mapping.
    let t = Instant::now();
    h.act(UiAction::LeaveLobby);
    assert!(t.elapsed() < Duration::from_millis(200), "leaving blocked for {:?}", t.elapsed());
    assert_eq!(gw.mappings().len(), 2, "being deleted");
    assert_eq!(h.unmapping_pending(), 1);
    // Quitting: the shell finishes, then the exit waits for the router.
    h.finish();
    let unmapping = h.take_unmapping();
    drop(h);
    let t = Instant::now();
    assert!(unmapping.wait(Duration::from_secs(1)), "the router confirmed");
    assert!(t.elapsed() < Duration::from_secs(1) + Duration::from_millis(100));
    assert_eq!(gw.mappings(), vec![], "deleted when the wait returns");
}

// ---- The app's own frame path (review Critical 1, Important 1) ----

/// Frames of the app `app` (its network frame, as the real loop and its
/// hidden window run it, then the shell's poll) and of the shells
/// `others` on the real clock, until `done` holds for the app's shell.
fn app_until(
    what: &str,
    limit: Duration,
    app: &mut rl_game::app::App,
    others: &mut [&mut Shell],
    mut done: impl FnMut(&Shell) -> bool,
) {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        let now = Instant::now();
        app.network_frame(now);
        app.shell_mut().poll();
        frame(others, now, &mut |_| Keys(0));
        if done(app.shell()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The real app polls the Internet tab with no online game: a game
/// announced on the (loopback) test relay is listed.
#[test]
fn the_apps_internet_tab_lists_games_without_an_online_game() {
    let relay = TestRelay::start();
    let empty = tempfile::tempdir().unwrap();
    let mut sh = menus(empty.path(), Store::in_memory(), "Anna", 1.0);
    use_relays(&mut sh, &[relay.url()]);
    let host = rl_lobby::Identity::from_secret([44; 32]).unwrap();
    announce_on(&relay, &host, "Office game", unix_now());
    let mut app = app_of(sh);
    app.shell_mut().act(UiAction::Open(Screen::Multiplayer));
    assert!(!app.shell().online_active(), "no online game");
    app_until("listed", Duration::from_secs(10), &mut app, &mut [], |s| {
        s.data.internet.games.iter().any(|g| g.name == "Office game")
    });
    assert_eq!(app.shell().data.internet.status, InternetStatus::Listed);
}

/// The real app fills the LAN list with no online game: an announcement
/// sent to the list's port on 127.0.0.1 (never broadcast) shows.
#[test]
fn the_apps_lan_list_fills_without_an_online_game() {
    let empty = tempfile::tempdir().unwrap();
    let mut sh = menus(empty.path(), Store::in_memory(), "Anna", 1.0);
    sh.set_lan(true);
    sh.test_lan_port(0);
    let mut app = app_of(sh);
    app.shell_mut().act(UiAction::Open(Screen::Multiplayer));
    app_until("listening", Duration::from_secs(5), &mut app, &mut [], |s| s.lan_listen_port().is_some());
    let port = app.shell().lan_listen_port().unwrap();
    let mut announcer = rl_net::lan::Announcer::new(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).unwrap();
    let game = rl_proto::LanAnnounce {
        fingerprint: rl_proto::FINGERPRINT,
        name: "Desk game".into(),
        port: 23400,
        players: 1,
        max_players: 8,
        spectators: 0,
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        password: false,
        playing: false,
    };
    let start = Instant::now();
    while !app.shell().data.lan_games.iter().any(|g| g.name == "Desk game") {
        assert!(start.elapsed() < Duration::from_secs(10), "the LAN list fills");
        announcer.tick(Instant::now(), &game);
        app.network_frame(Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The real app joins a typed host name: looked up on a thread ("Looking
/// up …" meanwhile), then the lobby, with no online game before (review
/// Important 1: it hung there).
#[test]
fn the_app_joins_a_typed_host_name() {
    let Some(dir) = content_dir() else { return };
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    host_as(&mut h, Visibility::Lan);
    let port = h.local_port().unwrap();
    let mut app = app_of(menus(&dir, Store::in_memory(), "Anna", 1.0));
    app.shell_mut().act(UiAction::Open(Screen::Multiplayer));
    app.shell_mut().act(UiAction::Join { addr: format!("localhost:{port}"), password: None });
    assert_eq!(
        app.shell().data.joining.as_deref(),
        Some("Looking up localhost:{port}…".replace("{port}", &port.to_string()).as_str())
    );
    assert!(!app.shell().online_active(), "no peer while the name is looked up");
    app_until("the lobby", Duration::from_secs(10), &mut app, &mut [&mut h], |s| {
        s.ui.screen() == Screen::Lobby && members(s) == 2
    });
}

/// Review Minor 3: the shutdowns of peers with nothing to delete (a LAN
/// host, a client) are forgotten at once, so the list stays short.
#[test]
fn finished_shutdowns_are_forgotten() {
    let Some(dir) = content_dir() else { return };
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    for _ in 0..3 {
        host_as(&mut h, Visibility::Lan);
        h.act(UiAction::LeaveLobby);
        assert_eq!(h.unmapping_pending(), 0);
    }
}

/// Ruling RH-1: hosting again at once on the same port, while the router
/// still deletes the last game's mapping (`LateDelete`: a deletion takes
/// effect 300 ms late, an addition at once), leaves the new game mapped:
/// the new mapping is not added until the old deletion is done.
#[test]
fn hosting_again_on_the_same_port_keeps_its_mapping() {
    use rl_net::portmap::fake::{FakeGateway, LATE, Misbehaviour};
    let Some(dir) = content_dir() else { return };
    let gw = FakeGateway::start(true, false, false, Ipv4Addr::new(127, 0, 0, 2), Misbehaviour::LateDelete);
    let (pmp, ssdp) = (gw.pmp(), gw.ssdp());
    let mut h = menus(&dir, Store::in_memory(), "Hosty", 1.0);
    h.test_ice(move |ice| {
        loopback(ice);
        let SocketAddr::V4(pmp) = pmp else { unreachable!() };
        ice.gateway = Some(pmp);
        ice.ssdp = Some(ssdp);
        ice.allow_local = true;
    });
    let f = FindingSettings { port_mapping: true, ..h.ui.settings().online.finding.clone() };
    h.act(UiAction::SetFinding(f));
    host_as(&mut h, Visibility::Invite);
    let port = h.local_port().unwrap();
    let open =
        format!("Port {port} is open on your router (UPnP): players join directly at 127.0.0.2:{port}");
    let mapped = |s: &[&mut Shell]| s[0].data.lobby.as_ref().is_some_and(|l| l.reach.contains(&open));
    run_until("the port mapped", Duration::from_secs(10), &mut [&mut h], None, |s| mapped(s));
    assert_eq!(gw.mappings().len(), 2, "TCP and UDP");
    // Leave, and host again at once on the same port.
    h.set_port(Some(port));
    h.act(UiAction::LeaveLobby);
    host_as(&mut h, Visibility::Invite);
    assert_eq!(h.local_port(), Some(port), "the same port");
    run_until("mapped again", Duration::from_secs(10), &mut [&mut h], None, |s| mapped(s));
    // Past the old deletion's effect: the new game's mapping stays.
    let until = Instant::now() + LATE * 3;
    while Instant::now() < until {
        frame(&mut [&mut h], Instant::now(), &mut |_| Keys(0));
        std::thread::sleep(Duration::from_millis(10));
    }
    let ports: Vec<u16> = gw.mappings().iter().map(|m| m.2).collect();
    assert_eq!(ports, [port, port], "the new game's TCP and UDP mapping: {:?}", gw.mappings());
}

/// Task 9 re-review: a hidden window wakes every 4 ms only for a live
/// online game; the lists alone (or lingering relays) every 60 ms; and not
/// at all with nothing network-related live.
#[test]
fn a_hidden_window_wakes_by_what_is_live() {
    use rl_game::net::{HIDDEN_FRAME, HIDDEN_IDLE_FRAME};
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut sh = menus(&dir, Store::in_memory(), "Anna", 1.0);
    use_relays(&mut sh, &[relay.url()]);
    assert_eq!(sh.hidden_wake(), None, "the main menu");
    sh.act(UiAction::Open(Screen::Multiplayer));
    assert_eq!(sh.hidden_wake(), Some(HIDDEN_IDLE_FRAME), "the lists");
    frame(&mut [&mut sh], Instant::now(), &mut |_| Keys(0));
    sh.act(UiAction::Back);
    assert_eq!(sh.hidden_wake(), Some(HIDDEN_IDLE_FRAME), "the relays linger");
    host_as(&mut sh, Visibility::Lan);
    assert_eq!(sh.hidden_wake(), Some(HIDDEN_FRAME), "an online game");
    assert_eq!((HIDDEN_FRAME.as_millis(), HIDDEN_IDLE_FRAME.as_millis()), (4, 60));
}
