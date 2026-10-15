//! The local slot (not worm 0) and online play through `Session`
//! (plan 10 task 10): a local match whose player is slot 2, and a session
//! with `Driver::Online` against a host on 127.0.0.1. Classic on Dirt
//! Level from `RL_CONTENT_DIR`; skipped with a notice without it.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rl_audio::Audio;
use rl_bots::{BotConfig, Difficulty};
use rl_content::{Content, DEFAULT_PRESET, MatchOptions, Player};
use rl_game::hud::{self, Frame};
use rl_game::net::PeerHandle;
use rl_game::online::OnlineDriver;
use rl_game::session::{Driver, Session, WEAPON_NAME_TICKS};
use rl_game::setup::{self, PlayerLook};
use rl_lobby::{HostConfig, Identity};
use rl_modes::DeathmatchSettings;
use rl_net::endpoint::EndpointConfig;
use rl_netcode::world_sim::TickedTick;
use rl_online::matches::{bot_wire, rules_wire, setup_packs};
use rl_online::{HostListen, Join, OnlineEvent, OnlinePeer, PeerContent, PeerRole};
use rl_proto::{Look, MatchSetupWire, SeatWire};
use rl_scene::{DrawList, Interpolation, Viewport};
use rl_sim::{EventKind, Keys, Score, World};

pub(crate) const MOD: &str = "olx-classic";
pub(crate) const LEVEL: &str = "olx-level-dirt-level";

pub(crate) fn content_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir
}

pub(crate) fn viewport() -> Viewport {
    Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0)
}

/// The player's keys at tick `t`: walk about, jump and shoot now and then.
pub(crate) fn player_keys(t: u64) -> Keys {
    let walk = if (t / 200).is_multiple_of(2) { Keys::RIGHT } else { Keys::LEFT };
    let jump = if t.is_multiple_of(97) { Keys::JUMP } else { 0 };
    let fire = if (t / 50).is_multiple_of(3) { Keys::FIRE } else { 0 };
    Keys(walk | jump | fire)
}

/// A local match whose player is slot 2 (two bots before it): its name,
/// keys, look, camera, crosshair, HUD worm, minimap dot, scoreboard row,
/// kill credit, deaths and spawns are slot 2's, never worm 0's.
#[test]
fn the_local_slot_drives_everything() {
    let Some(dir) = content_dir() else { return };
    let bot = Player::Bot(BotConfig { difficulty: Difficulty::Xtreme, ..BotConfig::default() });
    let o = MatchOptions::deathmatch(MOD, LEVEL, 11, vec![bot, bot, Player::Human]);
    let mut a = setup::load_match(&[dir.as_path()], &o, "default").unwrap();
    a.player = Some(PlayerLook { name: "Mine".into(), colour: [20, 90, 255] });
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert_eq!(s.local(), Some(2));
    assert!(s.human());
    assert_eq!(s.names.0, ["Bot 1", "Bot 2", "Mine"]);
    assert_eq!(s.colour(2), [20, 90, 255], "the profile's colour");
    assert_eq!(s.colour(0), rl_ui::bot_colours([20, 90, 255], 1)[0], "OLX's first bot colour");
    assert_eq!(s.present.scene.camera.target(), 2, "the camera follows the player");
    let looks = s.present.assets();
    assert_eq!(
        (looks.worm(0).local, looks.worm(1).local, looks.worm(2).local),
        (false, false, true),
        "only the player's crosshair"
    );
    let content = Content::open(&dir).unwrap();
    let default = content.load_skin("olx-skin-default").unwrap();
    assert_eq!(
        looks.image(looks.worm(2).skin).image,
        default.colorized(rl_data::image::Rgba::rgb(20, 90, 255)),
        "the player's skin in the player's colour"
    );

    let frame = Frame { scores: true, minimap: Some(egui::TextureId::default()), ..Frame::default() };
    let view = hud::view(&s, &frame);
    assert_eq!(view.worm.as_ref().map(|w| w.name.as_str()), Some("Mine"), "the HUD shows the player");
    let rows = view.scoreboard.expect("Tab's scoreboard");
    let followed: Vec<i32> = rows.iter().filter(|r| r.followed).map(|r| r.worm).collect();
    assert_eq!(followed, [2], "the player's row is highlighted");
    let dots = view.minimap.expect("the minimap").dots;
    let me = s.world.worms()[2].pos;
    let big: Vec<_> = dots.iter().filter(|d| d.big).map(|d| d.pos).collect();
    assert_eq!(big, [egui::pos2(me.x, me.y)], "the player's dot is the big one");

    // Play: the player's keys move worm 2, and every kill, death and
    // spawn is credited by slot.
    let (mut deaths, mut spawns, mut player_kills, mut bot_kills, mut fired) = (0, 0, 0, 0, 0);
    let mut t = 0;
    while (deaths == 0 || fired == 0) && t < 8000 {
        s.tick(player_keys(t));
        for e in s.events.iter() {
            match e.kind {
                EventKind::Kill { victim, killer } => {
                    deaths += u32::from(victim == 2);
                    if killer != victim {
                        player_kills += u64::from(killer == 2);
                        bot_kills += u64::from(killer != 2);
                    }
                }
                EventKind::Spawned { worm: 2 } => spawns += 1,
                EventKind::WeaponFired { worm: 2, .. } => fired += 1,
                _ => {}
            }
        }
        t += 1;
    }
    assert!(fired > 0, "the player's keys reach worm 2");
    assert!(deaths > 0, "the bots killed the player within {t} ticks");
    assert_eq!(s.player_deaths, deaths);
    assert_eq!(s.player_spawns, spawns);
    assert_eq!(s.tally.player_kills, player_kills);
    assert_eq!(s.tally.bot_kills, bot_kills);
    let view = hud::view(&s, &frame);
    let w = &s.world.worms()[2];
    let worm = view.worm.expect("the player's");
    assert_eq!((worm.kills, worm.deaths, worm.lives), (w.kills, w.deaths, w.lives));
}

/// A game nobody here plays (bots alone) has no local slot: the camera
/// and the HUD follow the worm chosen, round in worm order (worm 0 keeps
/// the crosshair); a player's camera stays on its own worm.
#[test]
fn a_watcher_follows_any_worm() {
    let Some(dir) = content_dir() else { return };
    let o = setup::DeathmatchOptions { bots: 3, spectate: true, ..setup::DeathmatchOptions::default() };
    let a = setup::load_deathmatch(&[dir.as_path()], "Classic", "Dirt Level", "default", &o).unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    assert_eq!(s.local(), None);
    assert!(s.spectating(), "nobody plays it");
    let looks = s.present.assets();
    assert!(
        looks.worm(0).local && (1..3).all(|i| !looks.worm(i).local),
        "worm 0's crosshair, as OLX's local worms"
    );
    assert!((0..3).all(|i| !looks.worm(i).human), "nobody plays here");
    let followed = |s: &Session| {
        let worm = hud::view(s, &Frame::default()).worm.map(|w| w.name);
        (s.followed(), s.present.scene.camera.target(), worm)
    };
    assert_eq!(followed(&s), (0, 0, Some("Bot 1".into())));
    s.follow_next(1);
    assert_eq!(followed(&s), (1, 1, Some("Bot 2".into())));
    s.follow_next(-1);
    s.follow_next(-1);
    assert_eq!(followed(&s), (2, 2, Some("Bot 3".into())), "round");
    s.follow(0);
    assert_eq!(s.followed(), 0);
    let o = setup::DeathmatchOptions { bots: 2, ..setup::DeathmatchOptions::default() };
    let a = setup::load_deathmatch(&[dir.as_path()], "Classic", "Dirt Level", "default", &o).unwrap();
    let mut s = Session::new(&a, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    s.follow_next(1);
    assert_eq!(s.followed(), 0, "the player's camera stays on its worm");
}

fn identity() -> Identity {
    loop {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>().unwrap()) {
            return id;
        }
    }
}

fn look(name: &str, colour: [u8; 3]) -> Look {
    Look { name: name.into(), colour, skin: "olx-skin-default".into(), skin_hash: rl_proto::NO_HASH.into() }
}

fn endpoint() -> EndpointConfig {
    EndpointConfig { port: 0, ..EndpointConfig::default() }
}

/// Polls `p` on the real clock with no keys.
fn poll(p: &mut OnlinePeer) -> Vec<OnlineEvent> {
    p.poll(Instant::now(), &mut || Keys(0), &mut |_, _, _| {})
}

/// Polls the host and the client on the real clock until `done` holds
/// for them; panics after `limit` with `what`.
fn until(
    what: &str,
    limit: Duration,
    host: &mut OnlinePeer,
    client: &mut OnlinePeer,
    mut done: impl FnMut(&mut OnlinePeer, &mut OnlinePeer) -> bool,
) {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        poll(host);
        poll(client);
        if done(host, client) {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Classic on Dirt Level with one bot, ending after `minutes`.
fn setup(content: &Content, minutes: f32) -> MatchSetupWire {
    MatchSetupWire {
        mod_id: MOD.into(),
        level_id: LEVEL.into(),
        packs: setup_packs(content, MOD, LEVEL).unwrap(),
        rules: rules_wire(&DeathmatchSettings { time_limit: minutes, ..DeathmatchSettings::default() }),
        bonuses: false,
        loading_time: 100,
        restrictions: DEFAULT_PRESET.into(),
        bots: vec![bot_wire(&BotConfig::default(), look("Bot", [0, 200, 0]))],
    }
}

/// A session with `Driver::Online` playing a client's slot against a host
/// with a bot on 127.0.0.1, 300 frames on a fake 10 ms clock: no panic, the
/// colour terrain never drifts (debug builds check after every advance),
/// the HUD follows the client's slot and shows the confirmed scores (the
/// host's for the same tick), and the match's end comes from a confirmed
/// tick.
#[test]
fn an_online_match_runs_headless() {
    let Some(dir) = content_dir() else { return };
    let content = || Content::open(&dir).unwrap();
    let cfg = HostConfig {
        name: "Test lobby".into(),
        password: None,
        max_players: 4,
        max_spectators: 2,
        admins: Vec::new(),
    };
    let mut host = OnlinePeer::start(
        PeerRole::Host { lobby: cfg, listen: HostListen::off() },
        endpoint(),
        identity(),
        look("Host", [200, 80, 40]),
        content(),
        Some(setup(&content(), 0.02)),
        PeerContent::without_downloads("a test"),
    )
    .unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], host.local_port()));
    let role = PeerRole::Client { join: Join::direct(addr), password: None, expected_host: None };
    let mut client = OnlinePeer::start(
        role,
        endpoint(),
        identity(),
        look("Anna", [20, 90, 255]),
        content(),
        None,
        PeerContent::without_downloads("a test"),
    )
    .unwrap();
    until("the client joins", Duration::from_secs(5), &mut host, &mut client, |_, c| {
        c.lobby().me().is_some()
    });
    client.lobby().set_ready(true);
    until("the client is ready", Duration::from_secs(3), &mut host, &mut client, |h, _| {
        h.lobby().state().is_some_and(|s| {
            s.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).all(|m| m.ready)
        })
    });
    host.lobby().start(5).unwrap();
    let mut started = None;
    let started_at = Instant::now();
    while started.is_none() {
        assert!(started_at.elapsed() < Duration::from_secs(3), "the match starts for the client");
        poll(&mut host);
        let mut events = poll(&mut client);
        if let Some(i) = events.iter().position(|e| matches!(e, OnlineEvent::MatchStarting { .. })) {
            let rest = events.split_off(i + 1);
            let Some(OnlineEvent::MatchStarting { start, my_slot }) = events.pop() else { unreachable!() };
            started = Some((start, my_slot, rest));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let (start, my_slot, rest) = started.unwrap();
    assert_eq!(my_slot, Some(1), "the host plays slot 0, the client slot 1, then the bot");

    let a = setup::load_online(client.content(), &start, my_slot).unwrap();
    let driver = OnlineDriver::new(PeerHandle::new(client), &start);
    let mut s = Session::online(&a, driver, viewport(), Interpolation::Linear, Audio::silent()).unwrap();
    let left = s.present_events(rest);
    assert!(left.iter().all(|e| !matches!(e, OnlineEvent::Ticked(_) | OnlineEvent::Confirmed(_))));
    assert_eq!(s.local(), Some(1));
    assert_eq!(s.names.0, ["Host", "Anna", "Bot"], "the lobby's names");
    assert_eq!(s.colour(1), [20, 90, 255], "the lobby's colours");
    assert_eq!(s.present.scene.camera.target(), 1);
    assert!(s.present.assets().worm(1).local && !s.present.assets().worm(0).local);

    // The host's confirmed scores by tick.
    let mut host_scores: BTreeMap<u32, (Vec<Score>, Option<u64>)> = BTreeMap::new();
    let mut list = DrawList::new();
    let frame = Frame { scores: true, ..Frame::default() };
    let t0 = Instant::now();
    let mut host_pressed = 0u64;
    let mut pressed = 0u64;
    let mut checked = 0;
    let mut slowest = Duration::ZERO;
    let mut named = 0u64;
    for i in 1..=300u64 {
        let now = t0 + Duration::from_millis(10 * i);
        let mut host_keys = || {
            host_pressed += 1;
            player_keys(host_pressed + 1000)
        };
        for e in host.poll(now, &mut host_keys, &mut |_, _, _| {}) {
            if let OnlineEvent::Confirmed(c) = e {
                host_scores.extend(c.into_iter().map(|t| (t.tick, (t.scores, t.time_left_ms))));
            }
        }
        // Ticks 50 to 52 change the weapon (Change, Change + Right, Change).
        let mut keys = || {
            pressed += 1;
            match pressed {
                50 | 52 => Keys(Keys::CHANGE),
                51 => Keys(Keys::CHANGE | Keys::RIGHT),
                _ => player_keys(pressed),
            }
        };
        let busy = Instant::now();
        let events = s.frame(now, &mut keys);
        slowest = slowest.max(busy.elapsed());
        assert!(
            events.events.iter().all(|e| !matches!(
                e,
                OnlineEvent::Refused(_) | OnlineEvent::Disconnected(_) | OnlineEvent::Kicked { .. }
            )),
            "frame {i}: {events:?}"
        );
        s.build(1.0, &mut list);
        if let Some((worm, _)) = s.weapon_name(false) {
            assert_eq!(worm, 1, "frame {i}: the client's worm shows its weapon");
            named += 1;
        }
        let Driver::Online(d) = &s.driver else { panic!("an online session") };
        let view = hud::view(&s, &frame);
        assert_eq!(view.worm.as_ref().map(|w| w.name.as_str()), Some("Anna"), "frame {i}");
        if let (Some(rows), Some(t)) = (&view.scoreboard, d.confirmed_tick())
            && let Some((scores, time_left)) = host_scores.get(&t)
        {
            let followed = Some(1);
            assert_eq!(rows, &rl_ui::hud::rows(scores, &s.names, followed), "tick {t}");
            assert_eq!(s.time_left_ms, *time_left, "tick {t}");
            checked += 1;
        }
    }
    eprintln!("the slowest online frame took {slowest:?}");
    assert!(
        (1..=WEAPON_NAME_TICKS + 5).contains(&named),
        "the client's weapon change showed its name for {named} frames"
    );
    let Driver::Online(d) = &s.driver else { panic!("an online session") };
    let confirmed = d.confirmed_tick().expect("confirmed ticks");
    assert!(confirmed > 200, "the client confirmed {confirmed} ticks");
    assert!(checked > 50, "the HUD's scores were checked against the host's {checked} times");
    assert!(s.ticks() >= u64::from(confirmed), "the newest predicted tick is drawn");
    assert!(!s.with_world(|w| s.present.terrain_drift(w)), "the colour terrain is the world's");
    // 1.2 s: the match ended on a confirmed tick, with the host's scores.
    let r = s.results.clone().expect("the time limit ended the match");
    assert_eq!(r.reason, "time limit");
    let end = s.since_end().map(|since| s.ticks() - since).expect("the end's tick");
    let (scores, _) = &host_scores[&(end as u32 - 1)];
    assert_eq!(r.rows, rl_ui::hud::rows(scores, &s.names, Some(1)));
    assert!(s.log.texts().any(|l| l == "Timelimit has been reached"));

    // On to the host's end (300 confirmed ticks after the result): from
    // `MatchEnded` the peer presents nothing more while its session goes
    // on, and the session draws the world presented last, kept once.
    let mut ended = None;
    let mut i = 300u64;
    while ended.is_none() {
        i += 1;
        assert!(i < 1500, "the match ends for the client");
        let now = t0 + Duration::from_millis(10 * i);
        host.poll(now, &mut || Keys(0), &mut |_, _, _| {});
        let events = s.frame(now, &mut || Keys(0));
        s.build(1.0, &mut list);
        if events.events.iter().any(|e| matches!(e, OnlineEvent::MatchEnded(_))) {
            ended = Some(s.with_world(World::clone));
        }
    }
    let kept = ended.unwrap();
    assert!(s.with_world(|w| std::ptr::eq(w, &s.world)), "the session's own world is drawn, not the peer's");
    assert!(!s.with_world(|w| s.present.terrain_drift(w)), "the world kept is the one presented");
    for _ in 0..100 {
        i += 1;
        let now = t0 + Duration::from_millis(10 * i);
        host.poll(now, &mut || Keys(0), &mut |_, _, _| {});
        s.frame(now, &mut || Keys(0));
        s.build(1.0, &mut list);
        assert!(s.with_world(|w| std::ptr::eq(w, &s.world)));
        assert!(s.with_world(|w| *w == kept), "frame {i}: the world drawn no longer changes");
    }
    // A next match's start in a poll, and everything after it, are handed
    // back unpresented (that match's session presents them); the ticks
    // before it are this match's.
    let tick = |tick| TickedTick { tick, resim: false, substeps: 1, lost: None, events: Vec::new() };
    let next = OnlineEvent::MatchStarting { start: start.clone(), my_slot };
    let poll =
        vec![OnlineEvent::Ticked(vec![tick(90_000)]), next.clone(), OnlineEvent::Ticked(vec![tick(90_001)])];
    assert_eq!(s.present_events(poll), [next, OnlineEvent::Ticked(vec![tick(90_001)])]);
}

// ---- Online play from the menus (plan 10 task 12) ----

use rl_game::app::{App, Config};
use rl_game::shell::{Outcome, Shell, ShellConfig};
use rl_input::Action;
use rl_ui::hud::LineKind;
use rl_ui::{HostPrefs, Profiles, Screen, Settings, Store, UiAction, UiState, Visibility};

/// A shell of the menus over `dir` playing as `name`, with Classic on Dirt
/// Level saved as its match (no bot, a `minutes` time limit), no LAN
/// (tests use no fixed port), hosting on an ephemeral port, no relay or
/// STUN server, no port mapping and every endpoint's address on
/// 127.0.0.1 (ruling NT-1: nothing leaves the machine, the router is
/// never asked).
pub(crate) fn menus(dir: &std::path::Path, store: Store, name: &str, minutes: f32) -> Shell {
    let mut settings = Settings::default();
    settings.online.finding.relays = Some(Vec::new());
    settings.online.finding.stun = Some(Vec::new());
    settings.online.finding.port_mapping = false;
    let m = &mut settings.gameplay.match_prefs;
    m.mod_id = MOD.into();
    m.level_id = LEVEL.into();
    m.bots = 0;
    m.rules.time_limit = minutes;
    let mut profiles = Profiles::default();
    profiles.list[0].name = name.into();
    let ui = UiState::new(settings, profiles, 1);
    let cfg = ShellConfig {
        content: dir.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: viewport(),
    };
    let mut sh = Shell::new(cfg, ui, store, Audio::silent());
    sh.set_lan(false);
    sh.set_port(Some(0));
    sh.test_ice(loopback);
    sh
}

/// The app of the menus `shell` without a window (no GPU backend): its
/// `network_frame` and `hidden_frame` are the real loop's.
pub(crate) fn app_of(shell: Shell) -> App {
    let cfg = Config {
        backends: wgpu::Backends::empty(),
        render: rl_render::RenderSettings::default(),
        fullscreen: false,
        uncapped: false,
        interpolation: Interpolation::Linear,
        autoquit_after: None,
        script: Vec::new(),
        shot_dir: std::env::temp_dir(),
        print_stats: false,
        latency_probe: None,
        record: None,
        menus: true,
        speed: Default::default(),
        fake_clock: false,
    };
    App::new(cfg, shell, true, None, None)
}

/// Every candidate on 127.0.0.1 (ruling NT-1).
pub(crate) fn loopback(ice: &mut rl_net::endpoint::IceConfig) {
    ice.base_override = Some(std::net::Ipv4Addr::LOCALHOST);
}

/// One frame of each shell at `now` (their keys from `keys`), as the
/// app's loop does them.
pub(crate) fn frame(
    shells: &mut [&mut Shell],
    now: Instant,
    keys: &mut dyn FnMut(usize) -> Keys,
) -> Vec<Outcome> {
    let mut out = Vec::new();
    for (i, sh) in shells.iter_mut().enumerate() {
        out.extend(sh.net_frame(now, &mut || keys(i)));
        out.extend(sh.after_ticks());
        out.extend(sh.poll());
    }
    out
}

/// Frames on the real clock until `done`; panics after `limit` with `what`.
pub(crate) fn frames_until(
    what: &str,
    limit: Duration,
    shells: &mut [&mut Shell],
    mut done: impl FnMut(&mut [&mut Shell]) -> bool,
) {
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        frame(shells, Instant::now(), &mut |i| {
            player_keys(i as u64 * 1000 + start.elapsed().as_millis() as u64 / 10)
        });
        if done(shells) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Hosts from the Host screen as the menus do.
pub(crate) fn host(sh: &mut Shell) {
    sh.act(UiAction::Open(Screen::Multiplayer));
    sh.act(UiAction::Open(Screen::Host));
    sh.act(UiAction::Host(HostPrefs { visibility: Visibility::Invite, ..HostPrefs::default() }));
}

pub(crate) fn join(sh: &mut Shell, port: u16) {
    sh.act(UiAction::Open(Screen::Multiplayer));
    sh.act(UiAction::Join { addr: format!("127.0.0.1:{port}"), password: None });
}

pub(crate) fn members(sh: &Shell) -> usize {
    sh.data.lobby.as_ref().map_or(0, |l| l.members.len())
}

/// A host and a client in its lobby, both on the Lobby screen.
fn host_and_client(dir: &std::path::Path, minutes: f32, data: Option<&std::path::Path>) -> (Shell, Shell) {
    let store = || data.map_or_else(Store::in_memory, Store::open);
    let mut h = menus(dir, store(), "Hosty", minutes);
    host(&mut h);
    assert_eq!(h.ui.screen(), Screen::Lobby, "hosting opens the lobby: {:?}", h.ui.message());
    let port = h.local_port().expect("the host's port");
    let mut c = menus(dir, Store::in_memory(), "Anna", minutes);
    join(&mut c, port);
    assert!(c.ui.is_connecting());
    frames_until("the client joins", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2 && members(s[1]) == 2
    });
    (h, c)
}

/// Both in the lobby, the host adds a bot, both ready, the host starts:
/// both in game.
pub(crate) fn start_match(h: &mut Shell, c: &mut Shell) {
    let mut setup = h.data.lobby.as_ref().unwrap().setup.clone();
    setup.bots = 1;
    h.act(UiAction::LobbySetup(setup));
    frames_until("the client sees the bot", Duration::from_secs(3), &mut [&mut *h, &mut *c], |s| {
        s[1].data.lobby.as_ref().is_some_and(|l| l.setup.bots == 1)
    });
    c.act(UiAction::LobbyReady(true));
    h.act(UiAction::LobbyReady(true));
    frames_until("the host can start", Duration::from_secs(3), &mut [&mut *h, &mut *c], |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.can_start.is_ok())
    });
    h.act(UiAction::LobbyStart);
    frames_until("both play", Duration::from_secs(5), &mut [&mut *h, &mut *c], |s| {
        s.iter().all(|sh| sh.ui.screen() == Screen::InGame && sh.plays_online())
    });
}

/// The confirmed tick and scores the HUD shows.
pub(crate) fn confirmed(sh: &Shell) -> Option<(u32, Vec<Score>)> {
    let Driver::Online(d) = &sh.session()?.driver else { return None };
    Some((d.confirmed_tick()?, d.confirmed_scores().to_vec()))
}

/// Plan 10 task 12: two shells in one process, one hosting on an
/// ephemeral port and the other joining it, reach the lobby; the host adds
/// a bot, both ready, the host starts: both play, and 200 frames later
/// both show the same confirmed scores for the same ticks. The match ends
/// on the results (its replay saved), whose Back returns the host to the
/// lobby; its forced start takes the client from its results into the
/// next match, watching; the client leaves from the match's menu (back
/// to Multiplayer) and the host's lobby shows it left.
#[test]
fn hosting_and_joining_from_the_shell() {
    let Some(dir) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let (mut h, mut c) = host_and_client(&dir, 0.04, Some(data.path()));
    let lobby = h.data.lobby.clone().unwrap();
    assert!(lobby.host && !c.data.lobby.as_ref().unwrap().host);
    let names: Vec<&str> = lobby.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Hosty", "Anna"]);
    start_match(&mut h, &mut c);
    assert_eq!(h.session().unwrap().names.0, ["Hosty", "Anna", "Bot 1"]);
    assert_eq!((h.session().unwrap().local(), c.session().unwrap().local()), (Some(0), Some(1)));

    let mut seen: [BTreeMap<u32, Vec<Score>>; 2] = Default::default();
    let mut between = 0;
    for _ in 0..200 {
        let now = Instant::now();
        frame(&mut [&mut h, &mut c], now, &mut |i| player_keys(i as u64 * 7 + 3));
        for sh in [&h, &c] {
            let a = sh.session().and_then(|s| s.online_alpha(now)).expect("an online alpha");
            assert!((0.0..=1.0).contains(&a), "{a}");
            between += usize::from(a > 0.05 && a < 0.95);
        }
        for (i, sh) in [&h, &c].into_iter().enumerate() {
            if let Some((t, s)) = confirmed(sh) {
                seen[i].insert(t, s);
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    let common: Vec<u32> = seen[0].keys().filter(|t| seen[1].contains_key(t)).copied().collect();
    assert!(common.len() > 5, "both confirmed the same ticks: {common:?}");
    for t in &common {
        assert_eq!(seen[0][t], seen[1][t], "the confirmed scores of tick {t}");
    }
    assert!(common.iter().any(|&t| t > 50), "the match went on: {common:?}");
    assert!(between > 100, "frames drawn between ticks by the pace's phase: {between} of 400");

    // The time limit ends it: the results, then the replay saved.
    frames_until("both show the results", Duration::from_secs(10), &mut [&mut h, &mut c], |s| {
        s.iter().all(|sh| sh.ui.screen() == Screen::Results)
    });
    assert!(h.data.results.is_some() && c.data.results.is_some());
    frames_until("the host's replay is saved", Duration::from_secs(10), &mut [&mut h, &mut c], |_| {
        std::fs::read_dir(data.path().join("replays")).is_ok_and(|d| d.count() > 0)
    });
    // The host goes back to the lobby; the client still reads the results.
    h.act(UiAction::Back);
    assert_eq!(h.ui.screen(), Screen::Lobby);
    assert!(h.session().is_none() && h.online_active());
    frames_until("the lobby gathers again", Duration::from_secs(8), &mut [&mut h, &mut c], |s| {
        s[0].data.lobby.as_ref().is_some_and(|l| l.can_start.as_ref().is_err_and(|e| e.contains("not ready")))
    });
    // A forced start: the client, not ready, watches; its results give way.
    h.act(UiAction::LobbyForceStart);
    frames_until("the next match plays", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s.iter().all(|sh| sh.ui.screen() == Screen::InGame && sh.plays_online())
    });
    assert_eq!(c.session().unwrap().local(), None, "the client watches");
    assert!(c.session().unwrap().results.is_none(), "the new match's view");
    // The match's menu: Leave the game returns to Multiplayer.
    c.act(UiAction::Back);
    c.act(UiAction::LeaveMatch);
    assert_eq!(c.ui.screen(), Screen::Multiplayer, "leaving returns to Multiplayer");
    assert!(!c.online_active() && c.session().is_none());
    frames_until("the host sees the client leave", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        members(s[0]) == 1
    });
    assert!(h.data.chat.iter().any(|l| l.text == "Anna left"), "{:?}", h.data.chat);
    assert!(h.session().unwrap().log.texts().any(|l| l == "Anna left"), "in the host's HUD log too");
}

/// Chat typed in game reaches the other player's HUD log as a chat line
/// ("name: text"), and its lobby chat.
#[test]
fn chat_reaches_the_other_player_in_game() {
    let Some(dir) = content_dir() else { return };
    let (mut h, mut c) = host_and_client(&dir, 1.0, None);
    start_match(&mut h, &mut c);
    assert!(c.open_chat(), "the chat key opens the field in game");
    assert_eq!(c.data.ingame_chat.as_deref(), Some(""));
    assert!(!c.open_chat(), "once");
    c.data.ingame_chat = None;
    c.act(UiAction::LobbyChat("hello host".into()));
    frames_until("the line reaches the host's HUD", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s[0].session().is_some_and(|x| x.log.lines().any(|l| l.text == "Anna: hello host"))
    });
    let line = h.session().unwrap().log.lines().find(|l| l.text == "Anna: hello host").unwrap();
    assert_eq!(line.kind, LineKind::Chat);
    assert!(h.data.chat.iter().any(|l| l.text == "Anna: hello host" && l.kind == LineKind::Chat));
    // The sender sees its own line too (the host relays it).
    frames_until("the line comes back", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s[1].session().is_some_and(|x| x.log.lines().any(|l| l.text == "Anna: hello host"))
    });
}

/// Decision N21: the app's frame without a window that can draw (what
/// `about_to_wait` runs while the window is hidden) still polls the
/// network and advances the online match.
#[test]
fn a_hidden_window_keeps_the_match_alive() {
    let Some(dir) = content_dir() else { return };
    let (mut h, c) = host_and_client(&dir, 1.0, None);
    let mut app = app_of(c);
    let mut setup = h.data.lobby.as_ref().unwrap().setup.clone();
    setup.bots = 1;
    h.act(UiAction::LobbySetup(setup));
    let hidden = |app: &mut App, h: &mut Shell| {
        frame(&mut [h], Instant::now(), &mut |_| Keys(0));
        assert!(app.network_live(), "online: the hidden window keeps polling");
        app.hidden_frame();
        std::thread::sleep(Duration::from_millis(2));
    };
    let start = Instant::now();
    while app.shell().data.lobby.as_ref().is_none_or(|l| l.setup.bots != 1) {
        assert!(start.elapsed() < Duration::from_secs(5), "the client sees the bot");
        hidden(&mut app, &mut h);
    }
    app.shell_mut().act(UiAction::LobbyReady(true));
    while h.data.lobby.as_ref().is_none_or(|l| l.can_start.is_err()) {
        assert!(start.elapsed() < Duration::from_secs(8), "the host can start");
        hidden(&mut app, &mut h);
    }
    h.act(UiAction::LobbyStart);
    while !app.shell().plays_online() {
        assert!(start.elapsed() < Duration::from_secs(10), "the hidden client plays");
        hidden(&mut app, &mut h);
    }
    let ticks = |app: &App| app.shell().session().map_or(0, Session::ticks);
    let from = ticks(&app);
    let confirmed_from = confirmed(app.shell()).map_or(0, |c| c.0);
    let played = Instant::now();
    while ticks(&app) < from + 100 || confirmed(app.shell()).is_none_or(|c| c.0 < confirmed_from + 100) {
        assert!(
            played.elapsed() < Duration::from_secs(5),
            "the match ticks while hidden: {from} to {}",
            ticks(&app)
        );
        hidden(&mut app, &mut h);
    }
    assert_eq!(app.shell().ui.screen(), Screen::InGame);
}

/// `--net-sim typical` (spec §9.10): a host with three bots in this
/// process and this player joining it over an impaired loopback link,
/// playing at once.
#[test]
fn net_sim_flag_starts_a_local_match() {
    let Some(dir) = content_dir() else { return };
    let mut sh = menus(&dir, Store::in_memory(), "Anna", 0.05);
    let profile = rl_netcode::netsim::Profile::by_name("typical").unwrap();
    sh.start_net_sim(profile).unwrap();
    frames_until("the net-sim match starts", Duration::from_secs(15), &mut [&mut sh], |s| {
        s[0].plays_online() && s[0].ui.screen() == Screen::InGame
    });
    let s = sh.session().unwrap();
    assert_eq!(s.with_world(|w| w.worms().len()), 4, "this player and three bots: the host watches");
    assert_eq!(s.local(), Some(0));
    let from = s.ticks();
    frames_until("the match plays", Duration::from_secs(5), &mut [&mut sh], |s| {
        s[0].session().is_some_and(|x| x.ticks() > from + 100) && confirmed(s[0]).is_some_and(|c| c.0 > 50)
    });
    frames_until("the overlay measures the round trip", Duration::from_secs(5), &mut [&mut sh], |s| {
        s[0].net_view().is_some_and(|n| n.stats.rtt_ms.is_some())
    });
    let net = sh.net_view().unwrap();
    assert!(net.stats.rtt_ms.is_some_and(|ms| ms >= 60), "the profile's 80 ms ping: {:?}", net.stats.rtt_ms);
    assert!(net.stats.input_delay > 0, "an input delay for the round trip");
    // The match ends; when the lobby gathers again this player readies
    // itself again and the next match starts.
    assert_eq!(sh.launches(), 1);
    frames_until("the next net-sim match starts", Duration::from_secs(15), &mut [&mut sh], |s| {
        s[0].launches() == 2 && s[0].ui.screen() == Screen::InGame
    });
}

fn replays_in(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir.join("replays")).map_or(0, |d| d.count())
}

/// A match's replay is made after it on a worker thread (decision N18):
/// leaving the game the moment the results show (the host, the client,
/// and the client quitting at once) still saves both replays.
#[test]
fn leaving_right_after_the_end_keeps_the_replay() {
    let Some(dir) = content_dir() else { return };
    let (hd, cd) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut h = menus(&dir, Store::open(hd.path()), "Hosty", 0.05);
    host(&mut h);
    let port = h.local_port().unwrap();
    let mut c = menus(&dir, Store::open(cd.path()), "Anna", 0.05);
    join(&mut c, port);
    frames_until("the client joins", Duration::from_secs(5), &mut [&mut h, &mut c], |s| {
        s[1].ui.screen() == Screen::Lobby && members(s[0]) == 2
    });
    start_match(&mut h, &mut c);
    let mut left = [false; 2];
    frames_until("both leave as their results show", Duration::from_secs(15), &mut [&mut h, &mut c], |s| {
        for (i, sh) in s.iter_mut().enumerate() {
            if !left[i] && sh.ui.screen() == Screen::Results {
                sh.act(UiAction::Back);
                sh.act(UiAction::Back);
                assert_eq!(sh.ui.screen(), Screen::Multiplayer);
                left[i] = true;
            }
            // The host left first (a loaded machine): the client is back
            // in Multiplayer ("the host closed the lobby").
            left[i] |= i == 1 && left[0] && sh.ui.screen() == Screen::Multiplayer;
        }
        left == [true, true]
    });
    // The client quits at once: quitting waits for the replay.
    c.finish();
    assert_eq!(replays_in(cd.path()), 1, "the client's replay");
    // The host stays in the menus: its replay is saved when made.
    frames_until("the host's replay is saved", Duration::from_secs(5), &mut [&mut h], |_| {
        replays_in(hd.path()) == 1
    });
    assert!(!h.online_active(), "nothing online is left");
}

/// `--host NAME` hosts through the menus' own actions, but nothing it
/// chose is remembered, not even by a save the options make later.
#[test]
fn flags_are_not_remembered() {
    let Some(dir) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let mut sh = menus(&dir, Store::open(data.path()), "Anna", 1.0);
    let online = sh.ui.settings().online.clone();
    sh.host_from_flag(Some("Flagged".into()));
    assert_eq!(sh.ui.screen(), Screen::Lobby);
    frames_until("the lobby shows", Duration::from_secs(3), &mut [&mut sh], |s| s[0].data.lobby.is_some());
    assert_eq!(sh.data.lobby.as_ref().unwrap().name, "Flagged");
    assert_eq!(sh.ui.settings().online, online);
    let mut v = sh.ui.settings().video.clone();
    v.ui_scale = 1.5;
    sh.act(UiAction::SetVideo(v));
    sh.flush();
    let saved = Store::open(data.path()).load_settings().value;
    assert_eq!(saved.video.ui_scale, 1.5, "the options' change is saved");
    assert_eq!(saved.online, online, "the flag's choices are not");
}

// ---- The network pump (plan 12c Task 5, ruling PF12c-2) ----

/// One frame of `app` as its loop runs one that draws nothing
/// (`App::hidden_frame`: `App::network_frame`, then the menus' poll).
fn app_frame(app: &mut App) {
    app.hidden_frame();
}

/// The confirmed ticks and scores `sh` shows, noted by tick.
fn note_confirmed(sh: &Shell, seen: &mut BTreeMap<u32, Vec<Score>>) {
    if let Some((t, s)) = confirmed(sh) {
        seen.insert(t, s);
    }
}

/// The results' rows without the highlighted (own) one.
fn result_rows(sh: &Shell) -> Vec<(i32, i32, i32, i32, i32)> {
    let r = sh.session().and_then(|s| s.results.as_ref()).expect("the results");
    r.rows.iter().map(|r| (r.worm, r.score, r.kills, r.deaths, r.suicides)).collect()
}

/// The colour terrain's drifts an online session counted (debug builds).
fn drifts(sh: &Shell) -> u64 {
    match sh.session().map(|s| &s.driver) {
        Some(Driver::Online(d)) => d.drifts(),
        _ => 0,
    }
}

/// Both apps' frames on the real clock for `d`, noting what each confirms.
fn both_frames(apps: [&mut App; 2], seen: &mut [BTreeMap<u32, Vec<Score>>; 2], d: Duration) {
    let [a, b] = apps;
    let start = Instant::now();
    while start.elapsed() < d {
        app_frame(a);
        app_frame(b);
        note_confirmed(a.shell(), &mut seen[0]);
        note_confirmed(b.shell(), &mut seen[1]);
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// Ruling PF12c-2: a host whose app runs no frame for 3 s (a dragged
/// window) keeps hosting: its pump polls the peer, so the client goes on
/// confirming ticks; when its frames resume the host's presentation starts
/// again from the world, and the match reaches its results with the same
/// confirmed scores on both. RED without the pump: the host sends nothing
/// for 3 s and the client's confirmed tick stands still.
#[test]
fn a_host_whose_frames_stop_keeps_hosting() {
    let Some(dir) = content_dir() else { return };
    let (mut h, mut c) = host_and_client(&dir, 0.2, None);
    start_match(&mut h, &mut c);
    assert!(h.pumped() && c.pumped(), "every online peer has its pump");
    let (mut host, mut client) = (app_of(h), app_of(c));
    let mut seen: [BTreeMap<u32, Vec<Score>>; 2] = Default::default();
    both_frames([&mut host, &mut client], &mut seen, Duration::from_millis(500));
    let before = confirmed(client.shell()).expect("the client confirms ticks").0;
    // The host's app: no frame for 3 s. The client's frames go on.
    let stopped = Instant::now();
    while stopped.elapsed() < Duration::from_secs(3) {
        app_frame(&mut client);
        note_confirmed(client.shell(), &mut seen[1]);
        std::thread::sleep(Duration::from_millis(3));
    }
    let during = confirmed(client.shell()).unwrap().0;
    assert!(
        during >= before + 250,
        "the client confirmed ticks {before} to {during} while the host drew nothing"
    );
    assert!(client.shell().plays_online() && client.shell().ui.screen() == Screen::InGame);
    // The host's frames resume: its first poll hands it the pump's events,
    // and the F2 meter counts only the ticks that frame ran (minor 9).
    let measured = host.ticks_measured();
    app_frame(&mut host);
    let counted = host.ticks_measured() - measured;
    assert!(counted < 50, "the first frame after the stop counted {counted} ticks");
    let start = Instant::now();
    while [&host, &client].iter().any(|a| a.shell().ui.screen() != Screen::Results) {
        assert!(start.elapsed() < Duration::from_secs(15), "both reach the results");
        both_frames([&mut host, &mut client], &mut seen, Duration::from_millis(10));
    }
    let host_seen = confirmed(host.shell()).unwrap().0;
    assert!(host_seen > during, "the host's view caught up: {host_seen}");
    let common: Vec<u32> = seen[0].keys().filter(|t| seen[1].contains_key(t)).copied().collect();
    assert!(common.iter().any(|&t| t > during), "both confirmed ticks after the stop: {common:?}");
    for t in &common {
        assert_eq!(seen[0][t], seen[1][t], "the confirmed scores of tick {t}");
    }
    assert_eq!(result_rows(host.shell()), result_rows(client.shell()), "the same results");
    assert_eq!((drifts(host.shell()), drifts(client.shell())), (0, 0), "the colour terrain followed");
}

/// Ruling PF12c-2: a client whose app runs no frame for 3 s stays joined:
/// its pump sends its `Keys(0)` in time, so the host fills almost none of
/// its inputs, nobody drops it, and its HUD never shows "Connection
/// problem". RED without the pump: the host fills about 300 inputs and
/// tells the client it lags.
#[test]
fn a_client_whose_frames_stop_stays_joined() {
    let Some(dir) = content_dir() else { return };
    let (mut h, mut c) = host_and_client(&dir, 0.2, None);
    start_match(&mut h, &mut c);
    let (mut host, mut client) = (app_of(h), app_of(c));
    let fills =
        |a: &App| a.shell().with_peer(|p| p.match_session().map(|s| s.stats().fills)).flatten().unwrap();
    let mut seen: [BTreeMap<u32, Vec<Score>>; 2] = Default::default();
    both_frames([&mut host, &mut client], &mut seen, Duration::from_millis(500));
    assert!(!client.shell().lagging());
    let (before, confirmed_before) = (fills(&host), confirmed(client.shell()).unwrap().0);
    // The client's app: no frame for 3 s. The host's frames go on.
    let stopped = Instant::now();
    while stopped.elapsed() < Duration::from_secs(3) {
        app_frame(&mut host);
        std::thread::sleep(Duration::from_millis(3));
    }
    let filled = fills(&host) - before;
    assert!(filled < 50, "the host filled {filled} of the client's inputs while it drew nothing");
    // Frames again: the client is still in the match, never lagging.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        // Every frame's HUD (minor 8).
        app_frame(&mut host);
        app_frame(&mut client);
        assert!(!client.shell().lagging(), "the client's HUD shows no connection problem");
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(client.shell().plays_online() && client.shell().ui.screen() == Screen::InGame, "not dropped");
    assert_eq!(members(host.shell()), 2, "the host keeps it");
    let after = confirmed(client.shell()).unwrap().0;
    assert!(after > confirmed_before + 300, "its match went on: {confirmed_before} to {after}");
    assert_eq!((drifts(host.shell()), drifts(client.shell())), (0, 0), "the colour terrain followed");
}

/// Both apps' frames on the real clock until `done`; panics after `limit`.
fn apps_until(what: &str, limit: Duration, apps: [&mut App; 2], mut done: impl FnMut([&App; 2]) -> bool) {
    let [a, b] = apps;
    let start = Instant::now();
    loop {
        assert!(start.elapsed() < limit, "{what} within {limit:?}");
        app_frame(a);
        app_frame(b);
        if done([&*a, &*b]) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Ruling PF12c-2: the host's match load (`start_online`, slowed here by
/// 2.5 s) runs without the peer's lock, so its pump keeps the match's
/// network alive: when the client's frames resume it has confirmed the
/// ticks played meanwhile. Every frame is the apps' (`App::network_frame`,
/// minor 7). RED without the pump: the host is silent for the load and
/// the client confirms nothing.
#[test]
fn a_match_load_does_not_silence_the_host() {
    let Some(dir) = content_dir() else { return };
    let (h, c) = host_and_client(&dir, 0.2, None);
    let (mut host, mut client) = (app_of(h), app_of(c));
    let mut setup = host.shell().data.lobby.as_ref().unwrap().setup.clone();
    setup.bots = 1;
    host.shell_mut().act(UiAction::LobbySetup(setup));
    apps_until("the client sees the bot", Duration::from_secs(3), [&mut host, &mut client], |a| {
        a[1].shell().data.lobby.as_ref().is_some_and(|l| l.setup.bots == 1)
    });
    client.shell_mut().act(UiAction::LobbyReady(true));
    host.shell_mut().act(UiAction::LobbyReady(true));
    apps_until("the host can start", Duration::from_secs(3), [&mut host, &mut client], |a| {
        a[0].shell().data.lobby.as_ref().is_some_and(|l| l.can_start.is_ok())
    });
    host.shell_mut().test_slow_load(Duration::from_millis(2_500));
    host.shell_mut().act(UiAction::LobbyStart);
    // The host's frame that starts the match loads for 2.5 s; the client's
    // frame after it finds what its pump confirmed meanwhile.
    let mut first = None;
    apps_until("both play", Duration::from_secs(10), [&mut host, &mut client], |a| {
        if first.is_none() && a[1].shell().plays_online() {
            first = Some(confirmed(a[1].shell()).map_or(0, |c| c.0));
        }
        a.iter().all(|x| x.shell().ui.screen() == Screen::InGame && x.shell().plays_online())
    });
    let first = first.unwrap();
    assert!(first >= 150, "the client confirmed {first} ticks while the host loaded the match");
    apps_until("the match goes on", Duration::from_secs(5), [&mut host, &mut client], |a| {
        confirmed(a[0].shell()).is_some_and(|c| c.0 > first + 100)
    });
    assert_eq!((drifts(host.shell()), drifts(client.shell())), (0, 0), "the colour terrain followed");
}

/// The resyncs an online session made after pumped ticks.
fn resyncs(sh: &Shell) -> u64 {
    match sh.session().map(|s| &s.driver) {
        Some(Driver::Online(d)) => d.resyncs(),
        _ => 0,
    }
}

/// Ruling PU-1: both players hold Right while both apps draw a frame only
/// every `every` (a slow machine or display): the slow frames play as
/// before. No pump poll, no resync (so no tick's sounds or effects lost),
/// and every confirmed final input of both players' slots holds Right.
fn slow_frames_play_as_before(every: Duration) {
    let Some(dir) = content_dir() else { return };
    let (mut h, mut c) = host_and_client(&dir, 0.2, None);
    start_match(&mut h, &mut c);
    let (mut host, mut client) = (app_of(h), app_of(c));
    host.press(Action::Right, true);
    client.press(Action::Right, true);
    let mut seen: [BTreeMap<u32, Vec<Score>>; 2] = Default::default();
    both_frames([&mut host, &mut client], &mut seen, Duration::from_millis(500));
    let confirmed_below =
        |a: &App| a.shell().with_peer(|p| p.match_session().map(|s| s.confirmed())).flatten();
    let mut next = confirmed_below(&host).expect("the host's match");
    let (mut checked, mut widest) = (0, Duration::ZERO);
    let start = Instant::now();
    let mut last = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        widest = widest.max(last.elapsed());
        last = Instant::now();
        app_frame(&mut host);
        app_frame(&mut client);
        let rows = host
            .shell()
            .with_peer(|p| {
                let s = p.match_session()?;
                let log = s.input_log();
                let rows: Vec<(u32, [Option<u8>; 2])> =
                    (next..s.confirmed()).map(|t| (t, [log.get(t, 0), log.get(t, 1)])).collect();
                Some(rows)
            })
            .flatten()
            .expect("the host's match");
        for (t, keys) in rows {
            for (slot, k) in keys.into_iter().enumerate() {
                let k = k.unwrap_or_else(|| panic!("tick {t}: slot {slot}'s final input is kept"));
                assert!(
                    k & Keys::RIGHT != 0,
                    "tick {t}: slot {slot}'s final input {k:#04x} lost the held Right"
                );
            }
            checked += 1;
            next = t + 1;
        }
        std::thread::sleep(every.saturating_sub(last.elapsed()));
    }
    assert!(checked > 100, "{checked} confirmed ticks checked");
    if widest < every + Duration::from_millis(15) {
        let polls = (host.shell().pump_polls(), client.shell().pump_polls());
        assert_eq!(polls, (0, 0), "frames every {every:?} ({widest:?} at most): the pumps never polled");
        let resynced = (resyncs(host.shell()), resyncs(client.shell()));
        assert_eq!(resynced, (0, 0), "no tick's presentation was dropped");
    } else {
        eprintln!("a loaded machine: frames up to {widest:?} apart");
    }
}

#[test]
fn frames_every_30_ms_play_as_before() {
    slow_frames_play_as_before(Duration::from_millis(30));
}

#[test]
fn frames_every_45_ms_play_as_before() {
    slow_frames_play_as_before(Duration::from_millis(45));
}
