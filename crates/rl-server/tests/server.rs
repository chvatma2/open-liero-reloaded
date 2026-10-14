//! The dedicated server (networking spec §7.4) driven in-process against
//! `rl_online` clients on 127.0.0.1: a rotation played to its next entry,
//! admins' chat commands, bans kept across a restart, the example config,
//! announcements on an in-process relay and the invite code; plan 12b's
//! joins through Nostr, port mapping on a `FakeGateway`, the TURN relay and
//! its notes. Nothing leaves the machine: relays, STUN and TURN servers and
//! routers are in-process, every test config sets `port_mapping: false` or
//! aims at a `FakeGateway`, and Nostr joins pin their base to 127.0.0.1
//! (test rule NT-1). Classic on Dirt Level from `RL_CONTENT_DIR` (skipped
//! with a notice without it); every wait is bounded, each test under 20 s.

use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rl_content::Content;
use rl_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
use rl_discovery::{
    GameList, GameSummary, Invite, ListedGame, Pool, PoolConfig, PoolEvent, Region, StunResponder, TestRelay,
    TurnList, parse_announcement, parse_turn_note,
};
use rl_lobby::{Identity, NOT_ADMIN, RELAYED_BAN};
use rl_net::endpoint::{EndpointConfig, IceConfig};
use rl_net::portmap::fake::{FakeGateway, Misbehaviour};
use rl_online::content::DownloadState;
use rl_online::join::NostrJoin;
use rl_online::{Join, OnlineEvent, OnlinePeer, PeerContent, PeerRole};
use rl_proto::{Look, Path as LinkPath, Phase, PublicKey, SeatWire};
use rl_server::config::{Config, RelayConfig, TestGateway, Visibility};
use rl_server::files::{IDENTITY_FILE, hex};
use rl_server::{NOT_AN_ADMIN_TARGET, Server, run};
use rl_sim::Keys;

const DIRT: &str = "olx-level-dirt-level";
const FACTORY: &str = "olx-level-liero-factory";

/// The clients here have every pack: no download cache.
fn no_downloads() -> PeerContent {
    PeerContent::without_downloads("a test")
}

fn content_dir() -> Option<PathBuf> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(PathBuf::from(dir))
}

fn content(dir: &Path) -> Content {
    Content::open(dir).unwrap()
}

fn identity() -> Identity {
    loop {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>().unwrap()) {
            return id;
        }
    }
}

fn look(name: &str) -> Look {
    Look {
        name: name.into(),
        colour: [200, 80, 40],
        skin: "olx-skin-default".into(),
        skin_hash: rl_proto::NO_HASH.into(),
    }
}

/// A server's config: port 0, invite only (no LAN or Nostr announcements,
/// no relays, no STUN, no port mapping: nothing leaves the machine),
/// Classic on Dirt Level then on Liero Factory, each `minutes` long.
fn config(admins: &[PublicKey], fill_bots: u8, start_when_ready: bool, minutes: f32) -> Config {
    let admins: Vec<String> = admins.iter().map(|k| format!("{:?}", hex(k))).collect();
    let text = format!(
        r#"(
            name: "Test server",
            visibility: Invite,
            relays: [],
            stun: [],
            port_mapping: false,
            password: None,
            port: 0,
            max_players: 8,
            max_spectators: 4,
            admins: [{}],
            fill_bots: {fill_bots},
            rotation: [
                (mod: "olx-classic", level: "{DIRT}", time_limit: {minutes:?}, kill_limit: -1, lives: -2, bonuses: false),
                (mod: "olx-classic", level: "{FACTORY}", time_limit: {minutes:?}, kill_limit: 15, lives: -2, bonuses: true),
            ],
            start_when_ready: {start_when_ready},
            min_players: 2,
            bans_file: Some("bans.ron"),
        )"#,
        admins.join(", ")
    );
    Config::parse(&text).unwrap()
}

/// One client of the test, with everything it reported.
struct Client {
    p: OnlinePeer,
    key: PublicKey,
    events: Vec<OnlineEvent>,
    pressed: u32,
    /// Says ready whenever the lobby gathers with it seated and not ready.
    auto_ready: bool,
    /// When it chatted.
    said: Vec<Instant>,
}

impl Client {
    fn poll(&mut self, now: Instant) {
        let pressed = &mut self.pressed;
        let seed = u32::from(self.key[0]);
        let mut keys = || {
            *pressed += 1;
            let k = (*pressed / 25).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 24;
            Keys((k as u8) & !Keys::CHANGE)
        };
        self.events.extend(self.p.poll(now, &mut keys, &mut |_, _, _| {}));
        if self.auto_ready {
            let me = self.p.lobby().me();
            let unready = self.p.lobby().state().is_some_and(|s| {
                s.phase == Phase::Gathering
                    && s.members
                        .iter()
                        .any(|m| Some(m.id) == me && !m.ready && matches!(m.seat, SeatWire::Player { .. }))
            });
            if unready {
                self.p.lobby().set_ready(true);
            }
        }
    }

    fn has(&self, f: impl Fn(&OnlineEvent) -> bool) -> bool {
        self.events.iter().any(f)
    }

    fn notices(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::Notice(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    fn starts(&self) -> Vec<&rl_proto::MatchStartWire> {
        self.events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::MatchStarting { start, .. } => Some(start),
                _ => None,
            })
            .collect()
    }

    fn welcomed(&mut self) -> bool {
        self.p.lobby().me().is_some() && !self.p.is_closed()
    }

    fn refused(&self) -> Option<&str> {
        self.events.iter().find_map(|e| match e {
            OnlineEvent::Refused(r) => Some(r.as_str()),
            _ => None,
        })
    }
}

/// A server and its clients, polled together on the real clock.
struct Rig {
    content_dir: PathBuf,
    /// The server's content (the clients': `content_dir`).
    server_content: PathBuf,
    data: tempfile::TempDir,
    server: Option<Server>,
    log: Vec<u8>,
    clients: Vec<Client>,
}

impl Rig {
    fn new(content_dir: PathBuf, config: Config) -> Rig {
        Rig::new_on(content_dir.clone(), content_dir, config)
    }

    fn new_on(content_dir: PathBuf, server_content: PathBuf, config: Config) -> Rig {
        Rig::new_in(tempfile::tempdir().unwrap(), content_dir, server_content, config)
    }

    /// A rig whose server's data directory is `data`.
    fn new_in(data: tempfile::TempDir, content_dir: PathBuf, server_content: PathBuf, config: Config) -> Rig {
        let mut rig =
            Rig { content_dir, server_content, data, server: None, log: Vec::new(), clients: Vec::new() };
        rig.restart(config);
        rig
    }

    /// Stops the server (if one runs) and starts it again with `config`.
    fn restart(&mut self, config: Config) {
        if let Some(s) = self.server.take() {
            s.shutdown(&mut self.log);
        }
        let s =
            Server::start(config, content(&self.server_content), self.data.path(), &mut self.log).unwrap();
        self.server = Some(s);
    }

    fn server(&mut self) -> &mut Server {
        self.server.as_mut().expect("the server runs")
    }

    fn join_as(&mut self, me: Identity, name: &str) -> usize {
        let c = content(&self.content_dir);
        self.join_with(me, look(name), c, no_downloads())
    }

    /// A client of its own look, content and download cache.
    fn join_with(&mut self, me: Identity, look: Look, content: Content, downloads: PeerContent) -> usize {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.server().local_port()));
        let role = PeerRole::Client { join: Join::direct(addr), password: None, expected_host: None };
        self.join_role(me, look, content, downloads, role)
    }

    /// A client joining as `role`.
    fn join_role(
        &mut self,
        me: Identity,
        look: Look,
        content: Content,
        downloads: PeerContent,
        role: PeerRole,
    ) -> usize {
        self.join_endpoint(
            me,
            look,
            content,
            downloads,
            role,
            EndpointConfig { port: 0, ..EndpointConfig::default() },
        )
    }

    /// A client joining `invite`'s server through its relays only (N46: no
    /// address), its base pinned to 127.0.0.1; `relay_only`: through a TURN
    /// relay announced on them.
    fn join_nostr(&mut self, me: Identity, name: &str, invite: &Invite, relay_only: bool) -> usize {
        let nostr = NostrJoin {
            host: invite.key,
            pool: PoolConfig { relays: invite.relay_urls(), ..PoolConfig::default() },
            region: Region::Europe,
            turn_from_nostr: relay_only,
            allow_local: true,
        };
        let role = PeerRole::Client {
            join: Join { direct: None, nostr: Some(nostr) },
            password: None,
            expected_host: None,
        };
        let ice = IceConfig {
            base_override: Some(Ipv4Addr::LOCALHOST),
            allow_local: true,
            relay_only,
            ..IceConfig::default()
        };
        let c = content(&self.content_dir);
        self.join_endpoint(
            me,
            look(name),
            c,
            no_downloads(),
            role,
            EndpointConfig { port: 0, ice, ..EndpointConfig::default() },
        )
    }

    /// A client joining as `role` on an endpoint of `cfg`.
    fn join_endpoint(
        &mut self,
        me: Identity,
        look: Look,
        content: Content,
        downloads: PeerContent,
        role: PeerRole,
        cfg: EndpointConfig,
    ) -> usize {
        let key = me.public();
        let p = OnlinePeer::start(role, cfg, me, look, content, None, downloads).unwrap();
        self.clients.push(Client {
            p,
            key,
            events: Vec::new(),
            pressed: 0,
            auto_ready: false,
            said: Vec::new(),
        });
        self.clients.len() - 1
    }

    /// Clients joining under fresh keys, waited for until welcomed.
    fn join_all(&mut self, names: &[&str]) -> Vec<usize> {
        let ids: Vec<usize> = names.iter().map(|n| self.join_as(identity(), n)).collect();
        self.run_until(Duration::from_secs(10), "the clients join", |r| {
            ids.iter().all(|&c| r.clients[c].welcomed())
        });
        ids
    }

    fn poll(&mut self) {
        let now = Instant::now();
        if let Some(s) = &mut self.server {
            s.poll(now, &mut self.log);
        }
        for c in &mut self.clients {
            c.poll(now);
        }
    }

    /// Polls until `done` holds; panics after `limit` with `what` and the log.
    fn run_until(&mut self, limit: Duration, what: &str, mut done: impl FnMut(&mut Rig) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < limit, "{what} within {limit:?}; the log:\n{}", self.log_text());
            self.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// A chat line from client `c`, within the lobby's chat limit (5 lines
    /// in 5 s: commands are chat lines).
    fn say(&mut self, c: usize, text: &str) {
        let window = Duration::from_micros(rl_lobby::CHAT_WINDOW_US) + Duration::from_millis(300);
        if self.clients[c].said.len() >= rl_lobby::CHAT_LINES {
            let oldest = self.clients[c].said[self.clients[c].said.len() - rl_lobby::CHAT_LINES];
            self.run_until(window * 2, "the chat window frees a line", |_| oldest.elapsed() >= window);
        }
        self.clients[c].said.push(Instant::now());
        self.clients[c].p.lobby().chat(text);
    }

    /// Waits until client `c` hears a notice starting with `start`.
    fn hears(&mut self, c: usize, start: &str) {
        let what = format!("client {c} hears {start:?}");
        self.run_until(Duration::from_secs(10), &what, |r| {
            r.clients[c].notices().iter().any(|n| n.starts_with(start))
        });
    }

    fn log_text(&self) -> String {
        String::from_utf8_lossy(&self.log).into_owned()
    }

    fn member_names(&mut self, c: usize) -> Vec<String> {
        let state =
            self.clients[c].p.lobby().state().cloned().unwrap_or_else(|| panic!("client {c}'s lobby"));
        state.members.iter().map(|m| m.look.name.clone()).collect()
    }
}

#[test]
fn a_server_runs_a_rotation() {
    let Some(dir) = content_dir() else { return };
    let mut rig = Rig::new(dir, config(&[], 3, true, 0.1));
    // Its identity, made at the first start, owner-only.
    let file = rig.data.path().join(IDENTITY_FILE);
    assert!(file.is_file(), "the server's identity file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let clients = rig.join_all(&["Anna", "Bert"]);
    for &c in &clients {
        rig.clients[c].auto_ready = true;
    }
    // Entry 0 starts once both are ready: the two players and one bot (fill 3).
    rig.run_until(Duration::from_secs(10), "entry 0 starts", |r| {
        clients.iter().all(|&c| r.clients[c].starts().len() == 1)
    });
    let first = rig.clients[0].starts()[0].clone();
    assert_eq!(first.setup.level_id, DIRT);
    assert_eq!(first.setup.bots.len(), 1, "filled to three worms");
    assert_eq!(first.slots.iter().filter(|s| s.member.is_some()).count(), 2);
    assert!(!first.setup.bonuses);
    // It ends on its time limit; the lobby returns, offering entry 1.
    rig.run_until(Duration::from_secs(20), "the match ends", |r| {
        clients.iter().all(|&c| r.clients[c].has(|e| matches!(e, OnlineEvent::MatchEnded(_))))
    });
    // The server's record of it is every client's (rl-cli's real-transport verdict reads it).
    rig.run_until(Duration::from_secs(10), "the server and the clients keep the match's record", |r| {
        r.server().last_record().is_some() && clients.iter().all(|&c| r.clients[c].p.last_record().is_some())
    });
    let record = rig.server().last_record().cloned().unwrap();
    assert_eq!(record.start, first);
    assert!(!record.checksums.is_empty());
    for &c in &clients {
        assert_eq!(rig.clients[c].p.last_record(), Some(&record), "client {c}'s record");
    }
    rig.run_until(Duration::from_secs(12), "the lobby gathers for entry 1", |r| {
        r.clients[0]
            .p
            .lobby()
            .state()
            .is_some_and(|s| s.phase == Phase::Gathering && s.setup.level_id == FACTORY)
    });
    // Entry 1 starts by itself once both are ready again.
    rig.run_until(Duration::from_secs(10), "entry 1 starts", |r| {
        clients.iter().all(|&c| r.clients[c].starts().len() == 2)
    });
    let second = rig.clients[1].starts()[1].clone();
    assert_eq!(second.setup.level_id, FACTORY);
    assert!(second.setup.bonuses);
    assert_eq!(second.setup.rules.kill_limit, 15);
    // One line per event.
    let log = rig.log_text();
    for want in ["join: Anna (", "join: Bert (", "via direct", "match start", "match end", "winner"] {
        assert!(log.contains(want), "{want:?} in the log:\n{log}");
    }
    assert_eq!(log.matches("match start").count(), 2, "{log}");
    assert!(log.contains("127.0.0.1"), "joins name the address:\n{log}");
}

#[test]
fn admins_kick_and_ban_by_chat() {
    let Some(dir) = content_dir() else { return };
    let (admin, zed_id) = (identity(), identity());
    let admins = [admin.public(), zed_id.public()];
    let cfg = || config(&admins, 0, false, 1.0);
    let mut rig = Rig::new(dir, cfg());
    let server_key = rig.server().key();
    let anna = rig.join_as(admin.clone(), "Anna");
    let zed = rig.join_as(zed_id, "Zed");
    let bob_id = identity();
    let bob_secret = bob_id.secret();
    let bob = rig.join_as(bob_id, "Bob");
    let cid = rig.join_all(&["Cid"])[0];
    rig.run_until(Duration::from_secs(10), "everyone is in", |r| {
        [anna, zed, bob].iter().all(|&c| r.clients[c].welcomed()) && r.member_names(cid).len() == 4
    });
    // A kick, by a name in any case: Cid may come back.
    rig.say(anna, "/kick cid");
    rig.run_until(Duration::from_secs(10), "Cid is kicked", |r| {
        r.clients[cid].has(|e| matches!(e, OnlineEvent::Kicked { banned: false }))
    });
    let back = rig.join_all(&["Cid"])[0];
    assert!(rig.clients[back].welcomed());
    // A ban is by key (spec §5.2): Bob's key stays out, his address does not.
    let bob_key = rig.clients[bob].key;
    rig.say(anna, "/ban BOB");
    rig.run_until(Duration::from_secs(10), "Bob is banned", |r| {
        r.clients[bob].has(|e| matches!(e, OnlineEvent::Kicked { banned: true }))
    });
    // Saved at once, with who he was.
    let bans = rig.data.path().join("bans.ron");
    let text = std::fs::read_to_string(&bans).expect("the bans file, written at the ban");
    assert!(text.contains(&hex(&bob_key)) && text.contains("Bob"), "{text}");
    assert!(!text.contains("127.0.0.1"), "a key-only ban: {text}");
    let again = rig.join_as(Identity::from_secret(bob_secret).unwrap(), "Bob");
    let eve = rig.join_all(&["Eve"])[0];
    rig.run_until(Duration::from_secs(10), "Bob's rejoin is refused", |r| {
        r.clients[again].refused().is_some()
    });
    assert_eq!(rig.clients[again].refused(), Some("you are banned from this lobby"));
    assert!(rig.clients[eve].welcomed(), "another key from the same address joins");
    // Admins (and oneself) cannot be kicked or banned.
    rig.say(anna, "/kick Zed");
    rig.hears(anna, NOT_AN_ADMIN_TARGET);
    rig.say(anna, "/ban anna");
    rig.run_until(Duration::from_secs(10), "refused twice", |r| {
        r.clients[anna].notices().iter().filter(|n| **n == NOT_AN_ADMIN_TARGET).count() == 2
    });
    assert!(!rig.clients[zed].has(|e| matches!(e, OnlineEvent::Kicked { .. })));
    assert!(!rig.clients[anna].has(|e| matches!(e, OnlineEvent::Kicked { .. })));
    // A ban with the address: a new key from there is refused too.
    rig.say(anna, "/banip Eve");
    rig.run_until(Duration::from_secs(10), "Eve is banned", |r| {
        r.clients[eve].has(|e| matches!(e, OnlineEvent::Kicked { banned: true }))
    });
    assert!(std::fs::read_to_string(&bans).unwrap().contains("127.0.0.1"));
    let fay = rig.join_as(identity(), "Fay");
    rig.run_until(Duration::from_secs(10), "Fay is refused", |r| r.clients[fay].refused().is_some());
    assert_eq!(rig.clients[fay].refused(), Some("you are banned from this lobby"));
    // Lifted by name: the key and the address.
    let eve_secret_key = rig.clients[eve].key;
    rig.say(anna, "/unban eve");
    rig.hears(anna, "unbanned Eve");
    let text = std::fs::read_to_string(&bans).unwrap();
    assert!(!text.contains("127.0.0.1") && !text.contains(&hex(&eve_secret_key)), "{text}");
    let fay = rig.join_all(&["Fay"])[0];
    assert!(rig.clients[fay].welcomed());
    let log = rig.log_text();
    for want in ["kick Cid", "ban Bob", "banip Eve", "unban Eve", "refused /kick Zed"] {
        assert!(log.contains(want), "{want:?} in the log:\n{log}");
    }

    // Stopping says goodbye to everyone.
    rig.restart(cfg());
    rig.run_until(Duration::from_secs(10), "Anna hears the server close", |r| {
        r.clients[anna].has(|e| matches!(e, OnlineEvent::Disconnected(_)))
    });
    assert_eq!(rig.server().key(), server_key, "the same identity after a restart");
    // After the restart Bob's key ban holds; Anna, from the same address, joins.
    let later = rig.join_as(Identity::from_secret(bob_secret).unwrap(), "Bob");
    let anna = rig.join_as(admin, "Anna");
    rig.run_until(Duration::from_secs(10), "Bob refused, Anna welcomed", |r| {
        r.clients[later].refused().is_some() && r.clients[anna].welcomed()
    });
    assert_eq!(rig.clients[later].refused(), Some("you are banned from this lobby"));
    // Lifted by a key prefix.
    rig.say(anna, &format!("/unban {}", &hex(&bob_key)[..12]));
    rig.hears(anna, "unbanned Bob");
    let bob = rig.join_as(Identity::from_secret(bob_secret).unwrap(), "Bob");
    rig.run_until(Duration::from_secs(10), "Bob comes back", |r| r.clients[bob].welcomed());
}

/// Ruling T13-C: a player joining a server full of bots changes only the
/// bots, so the others stay ready.
#[test]
fn a_joiner_leaves_the_others_ready() {
    let Some(dir) = content_dir() else { return };
    let mut rig = Rig::new(dir, config(&[], 4, false, 1.0));
    let anna = rig.join_all(&["Anna"])[0];
    rig.clients[anna].p.lobby().set_ready(true);
    let anna_ready = |r: &mut Rig| {
        let me = r.clients[anna].p.lobby().me();
        r.clients[anna]
            .p
            .lobby()
            .state()
            .is_some_and(|s| s.members.iter().any(|m| Some(m.id) == me && m.ready))
    };
    rig.run_until(Duration::from_secs(10), "Anna is ready with three bots", |r| {
        anna_ready(r) && r.clients[anna].p.lobby().state().is_some_and(|s| s.setup.bots.len() == 3)
    });
    let bert = rig.join_all(&["Bert"])[0];
    rig.run_until(Duration::from_secs(10), "two bots for two players", |r| {
        r.clients[bert].p.lobby().state().is_some_and(|s| s.setup.bots.len() == 2 && s.members.len() == 2)
    });
    assert!(anna_ready(&mut rig), "Anna is still ready");
}

#[test]
fn non_admins_cannot_use_admin_commands() {
    let Some(dir) = content_dir() else { return };
    let admin = identity();
    let mut rig = Rig::new(dir, config(&[admin.public()], 0, false, 1.0));
    let anna = rig.join_as(admin, "Anna");
    let bob = rig.join_all(&["Bob"])[0];
    rig.run_until(Duration::from_secs(10), "both are in", |r| {
        r.clients[anna].welcomed() && r.member_names(bob).len() == 2
    });
    let before = rig.clients[bob].p.lobby().state().unwrap().setup.clone();
    for (i, cmd) in ["/kick Anna", "/ban Anna", "/next", "/settings kill_limit 3"].iter().enumerate() {
        rig.say(bob, cmd);
        let what = format!("refusal {}", i + 1);
        rig.run_until(Duration::from_secs(10), &what, |r| {
            r.clients[bob].notices().iter().filter(|n| **n == NOT_ADMIN).count() == i + 1
        });
    }
    assert_eq!(rig.member_names(bob).len(), 2, "Anna stays");
    assert!(!rig.clients[anna].has(|e| matches!(e, OnlineEvent::Kicked { .. })));
    assert_eq!(rig.clients[bob].p.lobby().state().unwrap().setup, before, "nothing changed");
    // Anyone may list the players and ask for help.
    rig.say(bob, "/players");
    rig.hears(bob, "players:");
    let list =
        rig.clients[bob].notices().into_iter().find(|n| n.starts_with("players:")).unwrap().to_string();
    assert!(list.contains("Anna") && list.contains("Bob"), "{list}");
    rig.say(bob, "/help");
    rig.hears(bob, "/players");
    assert!(!rig.clients[bob].notices().iter().any(|n| n.contains("/kick")), "no admin commands offered");
    let log = rig.log_text();
    assert!(log.contains("refused /kick from Bob"), "{log}");
}

#[test]
fn admins_change_the_settings_and_skip_maps() {
    let Some(dir) = content_dir() else { return };
    let admin = identity();
    let mut rig = Rig::new(dir, config(&[admin.public()], 0, false, 1.0));
    let anna = rig.join_as(admin, "Anna");
    rig.run_until(Duration::from_secs(10), "Anna is in", |r| r.clients[anna].welcomed());
    let setup = |r: &mut Rig| r.clients[anna].p.lobby().state().unwrap().setup.clone();
    rig.say(anna, "/settings kill_limit 3");
    rig.run_until(Duration::from_secs(10), "the kill limit changes", |r| setup(r).rules.kill_limit == 3);
    rig.say(anna, "/settings Time_Limit 2.5");
    rig.run_until(Duration::from_secs(10), "the time limit changes", |r| {
        setup(r).rules.time_limit_min == 2.5
    });
    rig.say(anna, "/settings bonuses on");
    rig.run_until(Duration::from_secs(10), "bonuses on", |r| setup(r).bonuses);
    rig.say(anna, "/settings lives many");
    rig.hears(anna, "lives:");
    rig.say(anna, "/settings speed 3");
    rig.hears(anna, "unknown setting");
    assert_eq!(setup(&mut rig).level_id, DIRT);
    rig.say(anna, "/next");
    rig.run_until(Duration::from_secs(10), "the next map", |r| setup(r).level_id == FACTORY);
    assert_eq!(setup(&mut rig).rules.kill_limit, 15, "its own settings");
    rig.say(anna, "/help");
    rig.hears(anna, "/kick");
    rig.say(anna, "/kick Nobody");
    rig.hears(anna, "no member named");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// Maps the content can no longer build are skipped, each said once; after
/// a whole cycle of failures the server stops trying (the lobby stays).
#[test]
fn a_rotation_that_cannot_be_built_stops_after_one_cycle() {
    let Some(dir) = content_dir() else { return };
    let packs = tempfile::tempdir().unwrap();
    for p in ["olx-classic", "olx-core", "olx-theme-dirt", "olx-skin-default", DIRT, FACTORY] {
        copy_dir(&dir.join(p), &packs.path().join(p));
    }
    // A server on the copy, whose levels then go.
    let mut rig = Rig::new_on(dir, packs.path().to_path_buf(), config(&[], 3, true, 1.0));
    for level in [DIRT, FACTORY] {
        std::fs::remove_dir_all(packs.path().join(level)).unwrap();
    }
    let clients = rig.join_all(&["Anna", "Bert"]);
    for &c in &clients {
        rig.clients[c].auto_ready = true;
    }
    rig.run_until(Duration::from_secs(10), "the server gives up", |r| r.log_text().contains("error: no map"));
    for _ in 0..200 {
        rig.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    let log = rig.log_text();
    assert_eq!(log.matches("cannot start").count(), 2, "once per entry:\n{log}");
    assert_eq!(log.matches("error: no map").count(), 1, "{log}");
    assert!(clients.iter().all(|&c| rig.clients[c].starts().is_empty() && rig.clients[c].welcomed()));
}

/// A writer the test reads while `run` writes on its own thread.
#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

#[test]
fn run_stops_on_its_flag_and_says_bye() {
    let Some(dir) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let log = Shared::default();
    let stop = Arc::new(AtomicBool::new(false));
    let thread = {
        let (data, mut log, stop) = (data.path().to_path_buf(), log.clone(), Arc::clone(&stop));
        let content = content(&dir);
        std::thread::spawn(move || run(config(&[], 0, false, 1.0), content, &data, &stop, &mut log))
    };
    let begun = Instant::now();
    let port = loop {
        assert!(begun.elapsed() < Duration::from_secs(10), "the server listens: {}", log.text());
        let text = log.text();
        if let Some(p) = text.split("listening on port ").nth(1).and_then(|t| t.split_whitespace().next()) {
            break p.parse::<u16>().unwrap();
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let cfg = EndpointConfig { port: 0, ..EndpointConfig::default() };
    let role = PeerRole::Client { join: Join::direct(addr), password: None, expected_host: None };
    let mut p =
        OnlinePeer::start(role, cfg, identity(), look("Anna"), content(&dir), None, no_downloads()).unwrap();
    let mut events = Vec::new();
    let wait = |p: &mut OnlinePeer,
                events: &mut Vec<OnlineEvent>,
                what: &str,
                f: &dyn Fn(&[OnlineEvent]) -> bool| {
        let start = Instant::now();
        while !f(events) {
            assert!(start.elapsed() < Duration::from_secs(10), "{what}: {}", log.text());
            events.extend(p.poll(Instant::now(), &mut || Keys(0), &mut |_, _, _| {}));
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    wait(&mut p, &mut events, "Anna joins", &|e| e.iter().any(|e| matches!(e, OnlineEvent::Lobby(_))));
    stop.store(true, Ordering::Relaxed);
    wait(&mut p, &mut events, "Anna hears the goodbye", &|e| {
        e.iter().any(|e| matches!(e, OnlineEvent::Disconnected(why) if why == "the host closed the lobby"))
    });
    thread.join().unwrap().unwrap();
    assert!(log.text().contains("server stopped"), "{}", log.text());
}

#[test]
fn the_fixture_configs_parse() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/server/example.ron");
    let config = Config::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(config.port, rl_net::endpoint::DEFAULT_PORT);
    assert_eq!(config.rotation.len(), 2);
    assert_eq!(config.upload_kib_s, rl_server::config::DEFAULT_UPLOAD_KIB_S);
    // It leaves the lists out, so a copy follows later default lists.
    let defaults = Config::default();
    assert_eq!((&config.relays, &config.stun), (&defaults.relays, &defaults.stun));
    assert_eq!(config.visibility, Visibility::Public);
    // `cargo xtask e2e`'s: port 0, one short match for the game and a bot
    // client; `{relay}` is the run's own relay's URL.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/server/e2e.ron");
    let text = std::fs::read_to_string(path).unwrap().replace("{relay}", "ws://127.0.0.1:7777");
    let e2e = Config::parse(&text).unwrap();
    assert_eq!((e2e.port, e2e.min_players, e2e.fill_bots, e2e.rotation.len()), (0, 2, 0, 1));
    // The e2e run stays on the machine: listed on its own loopback relay at
    // 127.0.0.1 (`allow_local`), no STUN, no port mapping.
    assert_eq!(
        (e2e.visibility, &e2e.relays[..], e2e.stun.len()),
        (Visibility::Public, &["ws://127.0.0.1:7777".to_string()][..], 0)
    );
    assert_eq!(e2e.public_ip(), Some(std::net::IpAddr::from([127, 0, 0, 1])));
    assert!(!e2e.port_mapping && e2e.allow_local);
    assert_eq!(e2e.unreachable_public_address(), None, "it is announced");
    // The example documents the relay (off) and port mapping (on).
    assert!(config.port_mapping && config.relay.is_none() && !config.allow_local);
    let Some(dir) = content_dir() else { return };
    // Their rotations' mods and levels are the committed packs.
    let content = content(&dir);
    config.check(&content).unwrap();
    e2e.check(&content).unwrap();
}

/// `upload_kib_s` is the server's upload rate for content, 4096 KiB/s when
/// left out (configs from before it still load), held to 64..=65536.
#[test]
fn server_config_reads_upload_rate() {
    let with = |field: &str| {
        let text = format!(r#"(rotation: [(mod: "olx-classic", level: "{DIRT}")], {field})"#);
        Config::parse(&text).unwrap()
    };
    assert_eq!(with("").upload_kib_s, 4096);
    assert_eq!(with("").upload_rate(), rl_online::content::HOST_RATE, "the game's host rate");
    assert_eq!(with("upload_kib_s: 1024").upload_rate(), 1 << 20);
    assert_eq!(with("upload_kib_s: 0").upload_rate(), 64 * 1024);
    assert_eq!(with("upload_kib_s: 1000000").upload_rate(), 65536 * 1024);
}

/// The packs a match of Classic on Dirt Level needs, and the default skin.
const MATCH_PACKS: [&str; 5] = ["olx-core", "olx-classic", "olx-theme-dirt", DIRT, "olx-skin-default"];

/// A content directory of [`MATCH_PACKS`] from `dir` but `leave_out`,
/// plus `skin`: a skin pack of that id the server lacks (the default
/// skin's files under another id: another content hash).
fn partial_content(dir: &Path, leave_out: &[&str], skin: Option<&str>) -> tempfile::TempDir {
    let to = tempfile::tempdir().unwrap();
    for id in MATCH_PACKS.iter().filter(|id| !leave_out.contains(id)) {
        copy_dir(&dir.join(id), &to.path().join(id));
    }
    if let Some(id) = skin {
        let pack = to.path().join(id);
        copy_dir(&dir.join("olx-skin-default"), &pack);
        let manifest = std::fs::read_to_string(pack.join("pack.ron")).unwrap();
        std::fs::write(pack.join("pack.ron"), manifest.replace("\"olx-skin-default\"", &format!("{id:?}")))
            .unwrap();
    }
    to
}

/// A download cache of its own for a client.
fn client_cache() -> (tempfile::TempDir, PeerContent) {
    let dir = tempfile::tempdir().unwrap();
    let cache = rl_content::DownloadCache::open(dir.path()).unwrap();
    let cfg =
        PeerContent { cache: Ok(Arc::new(Mutex::new(cache))), upload_rate: rl_online::content::HOST_RATE };
    (dir, cfg)
}

/// Anna lacks Dirt Level, Bert wears a skin the server lacks: Anna
/// downloads the level from the server and both play. Returns the rig
/// and the directories to keep while it runs.
fn a_match_on_downloaded_content(data: tempfile::TempDir, dir: PathBuf) -> (Rig, Vec<tempfile::TempDir>) {
    let mut rig = Rig::new_in(data, dir.clone(), dir.clone(), config(&[], 0, true, 1.0));
    let anna_packs = partial_content(&dir, &[DIRT], None);
    let bert_packs = partial_content(&dir, &[], Some("test-skin"));
    let (anna_cache, anna_cfg) = client_cache();
    let (bert_cache, bert_cfg) = client_cache();
    let anna = rig.join_with(identity(), look("Anna"), content(anna_packs.path()), anna_cfg);
    let bert_look = Look { skin: "test-skin".into(), ..look("Bert") };
    let bert = rig.join_with(identity(), bert_look, content(bert_packs.path()), bert_cfg);
    rig.run_until(Duration::from_secs(10), "Anna downloads the level", |r| {
        r.clients[anna].p.downloads().iter().any(|d| d.item.id == DIRT && d.state == DownloadState::Done)
    });
    for c in [anna, bert] {
        rig.clients[c].auto_ready = true;
    }
    rig.run_until(Duration::from_secs(10), "the match starts", |r| {
        [anna, bert].iter().all(|&c| r.clients[c].starts().len() == 1)
    });
    assert_eq!(rig.clients[anna].starts()[0].setup.level_id, DIRT);
    let level = rig.clients[anna].p.content().pack_dir(DIRT).unwrap().to_path_buf();
    assert!(level.starts_with(anna_cache.path()), "Anna plays the level she downloaded: {level:?}");
    (rig, vec![anna_packs, bert_packs, anna_cache, bert_cache])
}

/// The skin packs in the server's download cache, by id.
fn cached_skins(data: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(data.join(rl_server::DOWNLOADS_DIR).join("packs")) else { return vec![] };
    let mut ids: Vec<String> = rd
        .filter_map(|e| std::fs::read_to_string(e.unwrap().path().join("pack.ron")).ok())
        .filter_map(|m| m.split("id: \"").nth(1).and_then(|t| t.split('"').next()).map(String::from))
        .collect();
    ids.sort();
    ids
}

/// The server serves its packs to a member lacking the level, and keeps a
/// member's skin it lacks in `<data dir>/downloads`, which it holds.
#[test]
fn the_server_serves_content_and_keeps_skins_in_its_cache() {
    let Some(dir) = content_dir() else { return };
    let (mut rig, _keep) = a_match_on_downloaded_content(tempfile::tempdir().unwrap(), dir);
    let downloads = rig.data.path().join(rl_server::DOWNLOADS_DIR);
    rig.run_until(Duration::from_secs(10), "the server fetches Bert's skin", |r| {
        cached_skins(r.data.path()) == ["test-skin"]
    });
    assert!(
        matches!(rl_content::DownloadCache::open(&downloads), Err(rl_content::InstallError::Locked)),
        "the server holds its cache"
    );
    assert!(!rig.log_text().contains("no download cache"), "{}", rig.log_text());
}

/// A server whose cache another holds (another server on the same data
/// directory) says so and runs without one: it still serves its own
/// packs; members' skins it lacks are not fetched (drawn as the default).
#[test]
fn a_server_without_its_cache_still_serves_its_packs() {
    let Some(dir) = content_dir() else { return };
    let data = tempfile::tempdir().unwrap();
    let held = rl_content::DownloadCache::open(&data.path().join(rl_server::DOWNLOADS_DIR)).unwrap();
    let (mut rig, _keep) = a_match_on_downloaded_content(data, dir);
    let log = rig.log_text();
    assert_eq!(log.matches("warning: no download cache").count(), 1, "{log}");
    assert!(log.contains("in use"), "{log}");
    for _ in 0..300 {
        rig.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(cached_skins(rig.data.path()).is_empty(), "nothing fetched");
    drop(held);
}

// ---- finding the server (plan 12a): Nostr, STUN, the invite ----

/// The public address the test's STUN server answers (a global one: no
/// packet is ever sent to it).
const PUBLIC: Ipv4Addr = Ipv4Addr::new(81, 2, 69, 142);

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// The last invite code the log printed (with an address or without).
fn printed_invite(log: &str) -> Option<Invite> {
    let code = log.lines().rev().find_map(|l| {
        l.strip_prefix("invite code: ").or_else(|| l.strip_prefix("invite code (joins through the relays): "))
    })?;
    Some(Invite::decode(code).unwrap_or_else(|e| panic!("the printed code {code:?}: {e}")))
}

/// A browser's view of `relay`: a pool subscribed to announcements and the
/// game list it feeds.
struct Browser {
    pool: Pool,
    list: GameList,
}

impl Browser {
    fn new(relay: &TestRelay) -> Browser {
        let mut pool = Pool::start(PoolConfig { relays: vec![relay.url()], ..PoolConfig::default() });
        pool.subscribe("games", vec![GameList::filter()]);
        Browser { pool, list: GameList::new(rl_proto::FINGERPRINT) }
    }

    fn game(&mut self, key: PublicKey) -> Option<ListedGame> {
        let mut out = Vec::new();
        self.pool.poll(&mut out);
        for e in out {
            if let PoolEvent::Event { event, .. } = e {
                self.list.offer(&event, unix_now());
            }
        }
        self.list.games().into_iter().find(|g| g.key == key)
    }
}

/// A public server is listed; STUN's answer is only an ICE candidate
/// (N47): without a mapping or a configured address its note and invite
/// carry no address, and players join through its relays.
#[test]
fn a_public_server_is_listed() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let stun = StunResponder::start(SocketAddr::from((PUBLIC, 40000)));
    let mut cfg = config(&[], 0, false, 1.0);
    cfg.visibility = Visibility::Public;
    cfg.relays = vec![relay.url()];
    cfg.stun = vec![stun.server()];
    cfg.region = Region::Europe;
    let mut rig = Rig::new(dir, cfg);
    let (key, port) = (rig.server().key(), rig.server().local_port());
    let mut browser = Browser::new(&relay);
    let mut listed = None;
    rig.run_until(Duration::from_secs(15), "a browser lists the server", |_| {
        listed = browser.game(key);
        listed.is_some()
    });
    let game = listed.unwrap();
    assert!(game.compatible, "this build's fingerprint");
    let s = &game.summary;
    assert_eq!(s.mod_hash.len(), 64, "the mod's content hash");
    assert_ne!(s.mod_hash, rl_proto::NO_HASH);
    let expected = GameSummary {
        v: rl_proto::PROTOCOL_VERSION,
        name: "Test server".into(),
        mode: "deathmatch".into(),
        map: DIRT.into(),
        map_name: "Dirt Level".into(),
        mod_id: "olx-classic".into(),
        mod_hash: s.mod_hash.clone(),
        players: 0,
        max_players: 8,
        spectators: 0,
        max_spectators: 4,
        password: false,
        region: Region::Europe,
        playing: false,
        addr: None,
        port,
        closed: false,
    };
    assert_eq!(*s, expected);
    assert!(stun.answered() >= 1, "STUN asked from the game socket");
    let log = rig.log_text();
    assert!(!log.contains("81.2.69.142"), "never advertised: {log}");
    assert!(log.contains("invite code (joins through the relays): rlx1"), "{log}");
    let invite = printed_invite(&log).expect("the invite printed once settled");
    assert_eq!((invite.key, invite.addr), (key, None));
    assert_eq!(invite.relay_urls(), vec![relay.url()], "the configured relay");

    // In a public game the invite is no secret: anyone may ask for it.
    let c = rig.join_all(&["Anna"])[0];
    rig.say(c, "/invite");
    rig.hears(c, "invite: ");
    let heard = rig.clients[c].notices().iter().find_map(|n| n.strip_prefix("invite: ")).unwrap().to_string();
    assert_eq!(Invite::decode(&heard).unwrap(), invite);

    // Stopping publishes a closing note (the pool's drop flushes it).
    rig.server.take().unwrap().shutdown(&mut rig.log);
    let closed = || {
        relay
            .events()
            .iter()
            .any(|e| e.pubkey == key && parse_announcement(e).is_ok_and(|(s, _)| s.closed && s.port == port))
    };
    let start = Instant::now();
    while !closed() {
        assert!(start.elapsed() < Duration::from_secs(5), "a closing note; the log:\n{}", rig.log_text());
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_invite_joins_the_server() {
    let Some(dir) = content_dir() else { return };
    let admin = identity();
    let mut cfg = config(&[admin.public()], 0, false, 1.0);
    cfg.public_address = Some("127.0.0.1".into());
    let mut rig = Rig::new(dir, cfg);
    let (key, port) = (rig.server().key(), rig.server().local_port());
    // Printed once settled: the key and the configured address (N47); no
    // relays in this config.
    rig.run_until(Duration::from_secs(10), "the invite", |r| printed_invite(&r.log_text()).is_some());
    let log = rig.log_text();
    assert!(
        log.contains(&format!(
            "reach: Players join directly at 127.0.0.1:{port} (your public address setting)"
        )),
        "{log}"
    );
    assert!(log.contains("invite code: rlx1"), "{log}");
    let invite = printed_invite(&log).unwrap();
    assert_eq!(invite.key, key);
    assert_eq!(invite.addr, Some(SocketAddr::from(([127, 0, 0, 1], port))));
    assert!(invite.relay_urls().is_empty());

    // Joining by the code: its address, expecting its key.
    let join = |rig: &mut Rig, me: Identity, name: &str, invite: &Invite| {
        let role = PeerRole::Client {
            join: Join::direct(invite.addr.unwrap()),
            password: None,
            expected_host: Some(invite.key),
        };
        let c = content(&rig.content_dir);
        rig.join_role(me, look(name), c, no_downloads(), role)
    };
    let a = join(&mut rig, admin, "Anna", &invite);
    let b = join(&mut rig, identity(), "Bert", &invite);
    rig.run_until(Duration::from_secs(10), "the invited join", |r| {
        r.clients[a].welcomed() && r.clients[b].welcomed()
    });
    // A code of another key at the same address is not this game.
    let forged = Invite { key: identity().public(), ..invite.clone() };
    let f = join(&mut rig, identity(), "Cid", &forged);
    rig.run_until(Duration::from_secs(10), "the forged code is refused", |r| {
        r.clients[f].refused().is_some()
    });
    assert_eq!(rig.clients[f].refused(), Some(rl_online::peer::WRONG_HOST));

    // Invite only: the admin may ask for the code, the others not.
    rig.say(a, "/invite");
    rig.hears(a, "invite: ");
    let heard = rig.clients[a].notices().iter().find_map(|n| n.strip_prefix("invite: ")).unwrap().to_string();
    assert_eq!(Invite::decode(&heard).unwrap(), invite);
    rig.say(b, "/invite");
    rig.hears(b, NOT_ADMIN);
}

#[test]
fn an_invite_server_is_not_announced() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    // STUN answers a private address (a router of a router): not public.
    let stun = StunResponder::start(SocketAddr::from(([10, 1, 2, 3], 40000)));
    let mut cfg = config(&[], 0, false, 1.0);
    cfg.relays = vec![relay.url()];
    cfg.stun = vec![stun.server()];
    let mut rig = Rig::new(dir, cfg);
    rig.run_until(Duration::from_secs(10), "the invite printed", |r| printed_invite(&r.log_text()).is_some());
    let log = rig.log_text();
    assert!(log.contains("invite code (joins through the relays): rlx1"), "{log}");
    assert_eq!(printed_invite(&log).unwrap().addr, None);
    assert!(!log.contains("10.1.2.3"), "STUN's answer is never advertised: {log}");
    assert!(stun.answered() >= 1);
    let start = Instant::now();
    rig.run_until(Duration::from_secs(5), "a second of polls", |_| start.elapsed() > Duration::from_secs(1));
    rig.server.take().unwrap().shutdown(&mut rig.log);
    assert!(relay.events().is_empty(), "nothing published");
}

/// Plan 10's configs (`DirectOnly`, none of the new fields) still load.
#[test]
fn an_old_config_still_loads() {
    let old = r#"(
        name: "Old",
        visibility: DirectOnly,
        rotation: [(mod: "olx-classic", level: "olx-level-dirt-level")],
    )"#;
    let c = Config::parse(old).unwrap();
    assert_eq!(c.visibility, Visibility::Invite);
    assert_eq!(c.relays, DEFAULT_RELAYS.iter().map(|r| r.to_string()).collect::<Vec<_>>());
    assert_eq!(c.stun, DEFAULT_STUN.iter().map(|r| r.to_string()).collect::<Vec<_>>());
    assert_eq!((c.region, c.public_address.as_deref()), (Region::Unknown, None));
    // Plan 12b's fields default: port mapping on, no local addresses, no relay.
    assert!(c.port_mapping && !c.allow_local && c.relay.is_none());
    // A server without a visibility is public.
    let c = Config::parse(r#"(rotation: [(mod: "olx-classic", level: "olx-level-dirt-level")])"#).unwrap();
    assert_eq!(c.visibility, Visibility::Public);
    // The new fields.
    let new = r#"(
        visibility: Public,
        relays: ["wss://relay.example.org"],
        stun: [],
        region: Oceania,
        public_address: Some("2001:db8::7"),
        port_mapping: false,
        allow_local: true,
        relay: Some((capacity: 8)),
        rotation: [(mod: "olx-classic", level: "olx-level-dirt-level")],
    )"#;
    let c = Config::parse(new).unwrap();
    assert_eq!((c.relays.len(), c.stun.len(), c.region), (1, 0, Region::Oceania));
    assert_eq!(c.public_ip(), Some("2001:db8::7".parse().unwrap()));
    assert!(!c.port_mapping && c.allow_local);
    assert_eq!(c.relay, Some(RelayConfig { capacity: 8, ..RelayConfig::default() }));
    // A region this version does not know reads as `Unknown` (as on the wire).
    let c =
        Config::parse(r#"(region: Mars, rotation: [(mod: "olx-classic", level: "olx-level-dirt-level")])"#);
    assert_eq!(c.unwrap().region, Region::Unknown);
    for bad in [
        r#"public_address: Some("my.home.example")"#,
        r#"relays: ["https://relay.example.org"]"#,
        r#"visibility: Hidden"#,
        // Final review B-M8: STUN servers need host:port, relays are bounded.
        r#"stun: ["stun.example.org"]"#,
        r#"stun: [":3478"]"#,
        r#"stun: ["stun.example.org:x"]"#,
    ] {
        let text = format!(r#"({bad}, rotation: [(mod: "olx-classic", level: "olx-level-dirt-level")])"#);
        assert!(Config::parse(&text).is_err(), "{bad}");
    }
}

/// Final review B-M8: relays are held to the game's bound (32) and listed
/// once each.
#[test]
fn server_relays_are_bounded_and_deduplicated() {
    let with = |relays: &[String]| {
        let list = relays.iter().map(|r| format!("{r:?}")).collect::<Vec<_>>().join(", ");
        Config::parse(&format!(r#"(relays: [{list}], rotation: [(mod: "olx-classic", level: "{DIRT}")])"#))
    };
    let r = |i: usize| format!("wss://relay{i}.example.org");
    let c = with(&[r(1), r(2), r(1), format!("{}/", r(2))]).unwrap();
    assert_eq!(c.relays, [r(1), r(2)]);
    assert!(with(&(0..32).map(r).collect::<Vec<_>>()).is_ok());
    assert!(with(&(0..33).map(r).collect::<Vec<_>>()).is_err(), "at most 32");
}

/// Declined item of final review A, B-M2: a configured `public_address`
/// the Internet cannot reach (private, local, IPv6) is not announced on
/// public relays (the note would leak the LAN address and lists refuse
/// it); with relays that are all local, it is.
#[test]
fn a_private_public_address_is_not_announced_on_public_relays() {
    let with = |addr: &str, relays: &str| {
        Config::parse(&format!(
            r#"(public_address: Some("{addr}"), relays: [{relays}], rotation: [(mod: "olx-classic", level: "{DIRT}")])"#
        ))
        .unwrap()
    };
    let public = r#""wss://relay.example.org""#;
    for addr in ["192.168.1.5", "127.0.0.1", "2a00:1450::1"] {
        assert_eq!(with(addr, public).unreachable_public_address(), Some(addr.parse().unwrap()), "{addr}");
    }
    assert_eq!(with("81.2.69.142", public).unreachable_public_address(), None);
    assert_eq!(
        with("192.168.1.5", r#""ws://192.168.1.2:7000""#).unreachable_public_address(),
        None,
        "a LAN relay"
    );
}

// ---- through routers (plan 12b): Nostr joins, port mapping, the relay ----

/// A config listening on `relay` (Invite), local addresses allowed, no
/// port mapping, its base pinned to 127.0.0.1 (NT-1: the fake gateway's
/// ports are never asked with mapping off).
fn nostr_config(relay: &TestRelay, admins: &[PublicKey]) -> Config {
    let mut cfg = config(admins, 0, false, 1.0);
    cfg.relays = vec![relay.url()];
    cfg.allow_local = true;
    cfg.test_gateway = Some(TestGateway {
        pmp: SocketAddr::from(([127, 0, 0, 1], 9)),
        ssdp: SocketAddr::from(([127, 0, 0, 1], 9)),
        base: Ipv4Addr::LOCALHOST,
    });
    cfg
}

/// N46, N54: an invite server with no address prints a code without one;
/// a client with only that code reaches its lobby through the relay, and
/// the log names the join's path.
#[test]
fn an_invite_server_without_an_address_is_joined_through_nostr() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut rig = Rig::new(dir, nostr_config(&relay, &[]));
    let key = rig.server().key();
    rig.run_until(Duration::from_secs(10), "the invite", |r| printed_invite(&r.log_text()).is_some());
    let log = rig.log_text();
    assert!(log.contains("invite code (joins through the relays): rlx1"), "{log}");
    let invite = printed_invite(&log).unwrap();
    assert_eq!((invite.key, invite.addr, invite.relay_urls()), (key, None, vec![relay.url()]));
    let c = rig.join_nostr(identity(), "Anna", &invite, false);
    rig.run_until(Duration::from_secs(15), "Anna joins through the relay", |r| r.clients[c].welcomed());
    rig.run_until(Duration::from_secs(5), "the join logged", |r| r.log_text().contains("join: Anna ("));
    let log = rig.log_text();
    assert!(log.contains("from 127.0.0.1 via direct key "), "{log}");
    assert!(log.contains("reach: Port mapping is off: "), "{log}");
    assert!(log.contains("reach: Taking joins through 1 of 1 relays"), "{log}");
}

/// N47, N51: a public server whose router (a `FakeGateway` speaking UPnP
/// only, external address 203.0.113.7, a documentation address: hence
/// `allow_local`) maps its port announces the mapped address and port;
/// a clean stop deletes the mapping (ruling NT-2).
#[test]
fn a_mapped_server_announces_its_mapped_address() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let external = Ipv4Addr::new(203, 0, 113, 7);
    let gw = FakeGateway::start(true, false, false, external, Misbehaviour::Honest);
    let mut cfg = nostr_config(&relay, &[]);
    cfg.visibility = Visibility::Public;
    cfg.port_mapping = true;
    cfg.test_gateway = Some(TestGateway { pmp: gw.pmp(), ssdp: gw.ssdp(), base: Ipv4Addr::LOCALHOST });
    let mut rig = Rig::new(dir, cfg);
    let (key, port) = (rig.server().key(), rig.server().local_port());
    let mut browser = Browser::new(&relay);
    browser.list.allow_local(true);
    let mut listed = None;
    rig.run_until(Duration::from_secs(15), "listed at the mapped address", |_| {
        listed = browser.game(key).filter(|g| g.summary.addr.is_some());
        listed.is_some()
    });
    let mapped: Vec<_> = gw.mappings();
    let udp = mapped.iter().find(|m| m.1).expect("a UDP mapping").2;
    assert!(mapped.iter().any(|m| !m.1 && m.2 == udp), "TCP on the same port: {mapped:?}");
    let s = listed.unwrap().summary;
    assert_eq!((s.addr, s.port), (Some(IpAddr::V4(external)), udp));
    let log = rig.log_text();
    assert!(log.contains(&format!("reach: Port {port} is open on your router (UPnP)")), "{log}");
    assert_eq!(printed_invite(&log).unwrap().addr, Some(SocketAddr::from((external, udp))));
    rig.server.take().unwrap().shutdown(&mut rig.log);
    assert!(gw.mappings().is_empty(), "deleted at the stop: {:?}", gw.mappings());
}

/// A relay server's config: `nostr_config` with a configured address and
/// a TURN relay on ephemeral ports with generous limits (one IP: 127.0.0.1).
fn relay_config(relay: &TestRelay, admins: &[PublicKey]) -> Config {
    let mut cfg = nostr_config(relay, admins);
    cfg.public_address = Some("127.0.0.1".into());
    cfg.region = Region::Europe;
    cfg.relay = Some(RelayConfig {
        port: 0,
        ports: (0, 0),
        user_kib_s: 65536,
        allocations_per_ip: 64,
        capacity: 256,
    });
    cfg
}

/// A `TurnList` on `relay`.
struct Turns {
    pool: Pool,
    list: TurnList,
}

impl Turns {
    fn new(relay: &TestRelay) -> Turns {
        let mut pool = Pool::start(PoolConfig { relays: vec![relay.url()], ..PoolConfig::default() });
        pool.subscribe("turn", vec![TurnList::filter(unix_now())]);
        let mut list = TurnList::new();
        list.allow_local(true);
        Turns { pool, list }
    }

    fn picks(&mut self) -> Vec<rl_discovery::TurnPick> {
        let mut out = Vec::new();
        self.pool.poll(&mut out);
        for e in out {
            if let PoolEvent::Event { event, .. } = e {
                self.list.offer(&event, unix_now());
            }
        }
        self.list.pick(Region::Europe, 2)
    }
}

/// Waits until the relay's note is listed; its pick.
fn announced_relay(rig: &mut Rig, turns: &mut Turns) -> rl_discovery::TurnPick {
    let mut picks = Vec::new();
    rig.run_until(Duration::from_secs(10), "the relay's note", |_| {
        picks = turns.picks();
        !picks.is_empty()
    });
    picks.remove(0)
}

/// N52: `--relay` starts our TURN server once its public address is known
/// (configured here), announces it, and a joiner whose links may only be
/// relayed reaches the lobby through it.
#[test]
fn a_relay_server_announces_itself_and_relays() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut rig = Rig::new(dir, relay_config(&relay, &[]));
    let mut turns = Turns::new(&relay);
    let pick = announced_relay(&mut rig, &mut turns);
    let at = rig.server().relay_addr().expect("the relay runs");
    assert_eq!(pick.key, rig.server().key(), "signed by the server's identity");
    assert_eq!(pick.addr, SocketAddr::from(([127, 0, 0, 1], at.port())));
    for c in [&pick.user, &pick.pass] {
        assert!(c.len() == 16 && c.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{c}");
    }
    let log = rig.log_text();
    assert!(log.contains("relay: waiting for the public address"), "{log}");
    let started =
        format!("relay: TURN on port {0}, relay ports ephemeral, announced as 127.0.0.1:{0}", at.port());
    assert!(log.contains(&started), "{log}");
    let invite = printed_invite(&log).unwrap();
    let c = rig.join_nostr(identity(), "Rob", &invite, true);
    rig.run_until(Duration::from_secs(20), "Rob joins through the TURN relay", |r| r.clients[c].welcomed());
    assert_eq!(rig.clients[c].p.connection().map(|c| c.0), Some(LinkPath::Relayed));
    rig.run_until(Duration::from_secs(5), "the join logged", |r| r.log_text().contains("join: Rob ("));
    assert!(rig.log_text().contains("via relayed key "), "{}", rig.log_text());
    rig.run_until(Duration::from_secs(5), "bytes relayed", |r| {
        r.server().relay_stats().is_some_and(|s| s.relayed_bytes > 0 && s.allocations >= 1)
    });
    rig.run_until(Duration::from_secs(5), "the allocation logged", |r| {
        r.log_text().contains("relay: 1 allocations")
    });
}

/// The relay's closing note (capacity 0) goes out at the stop, and the
/// list drops the relay despite its 10 s gap.
#[test]
fn the_relay_closes_its_note_at_shutdown() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let mut rig = Rig::new(dir, relay_config(&relay, &[]));
    let key = rig.server().key();
    let mut turns = Turns::new(&relay);
    announced_relay(&mut rig, &mut turns);
    rig.server.take().unwrap().shutdown(&mut rig.log);
    assert!(rig.log_text().contains("relay: closing note published"), "{}", rig.log_text());
    let closed = || {
        relay.events().iter().any(|e| e.pubkey == key && parse_turn_note(e).is_ok_and(|n| n.capacity == 0))
    };
    let begun = Instant::now();
    while !(closed() && turns.picks().is_empty()) {
        assert!(begun.elapsed() < Duration::from_secs(5), "the closing note; the log:\n{}", rig.log_text());
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// N59 (Task 7 review): an admin's `/banip` of a member reached through a
/// TURN relay bans its key only — the address is the relay's — live and in
/// the bans file; the admin is told. After a restart from the bans file
/// the key is still refused, and the relay's address (127.0.0.1, every
/// client's here) is not banned.
#[test]
fn a_relayed_member_is_banned_by_key_only_across_a_restart() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let admin = identity();
    let cfg = relay_config(&relay, &[admin.public()]);
    let mut rig = Rig::new(dir, cfg.clone());
    let mut turns = Turns::new(&relay);
    announced_relay(&mut rig, &mut turns);
    let anna = rig.join_as(admin.clone(), "Anna");
    let invite = printed_invite(&rig.log_text()).unwrap();
    let rob_id = identity();
    let rob_secret = rob_id.secret();
    let rob = rig.join_nostr(rob_id, "Rob", &invite, true);
    rig.run_until(Duration::from_secs(20), "both are in", |r| {
        r.clients[anna].welcomed() && r.clients[rob].welcomed() && r.member_names(anna).len() == 2
    });
    let rob_key = rig.clients[rob].key;
    rig.say(anna, "/banip rob");
    rig.hears(anna, RELAYED_BAN);
    rig.run_until(Duration::from_secs(10), "Rob is banned", |r| {
        r.clients[rob].has(|e| matches!(e, OnlineEvent::Kicked { banned: true }))
    });
    assert!(rig.log_text().contains(&format!("banip Rob: {RELAYED_BAN}")), "{}", rig.log_text());
    let bans = rig.data.path().join("bans.ron");
    let text = std::fs::read_to_string(&bans).expect("the bans file, written at the ban");
    assert!(text.contains(&hex(&rob_key)) && text.contains("Rob"), "{text}");
    assert!(!text.contains("127.0.0.1"), "a key-only ban: {text}");

    rig.restart(cfg);
    assert!(rig.log_text().contains("bans: 1 keys, 0 addresses"), "{}", rig.log_text());
    let again = rig.join_as(Identity::from_secret(rob_secret).unwrap(), "Rob");
    let cid = rig.join_as(identity(), "Cid");
    rig.run_until(Duration::from_secs(10), "Rob refused, Cid welcomed", |r| {
        r.clients[again].refused().is_some() && r.clients[cid].welcomed()
    });
    assert_eq!(rig.clients[again].refused(), Some("you are banned from this lobby"));
}

/// A config mapping its port at `gw` (127.0.0.1 base, `allow_local`).
fn mapped_config(gw: &FakeGateway, mut cfg: Config) -> Config {
    cfg.port_mapping = true;
    cfg.allow_local = true;
    cfg.test_gateway = Some(TestGateway { pmp: gw.pmp(), ssdp: gw.ssdp(), base: Ipv4Addr::LOCALHOST });
    cfg
}

/// Ruling NT-2 (Task 8 review I2): a clean stop returns only once the
/// router deleted the mapping. The router here (`LateUpnp`) removes a
/// mapping only 300 ms after the request, and no Nostr pool's drop holds
/// the stop up: without the wait the mapping would still be there.
#[test]
fn a_clean_stop_waits_for_the_router_to_delete_the_mapping() {
    let Some(dir) = content_dir() else { return };
    let gw = FakeGateway::start(true, false, false, Ipv4Addr::new(203, 0, 113, 7), Misbehaviour::LateUpnp);
    let mut rig = Rig::new(dir, mapped_config(&gw, config(&[], 0, false, 1.0)));
    let port = rig.server().local_port();
    let open = format!("reach: Port {port} is open on your router (UPnP)");
    rig.run_until(Duration::from_secs(10), "the port mapped", |r| r.log_text().contains(&open));
    assert_eq!(gw.mappings().len(), 2, "TCP and UDP");
    rig.server.take().unwrap().shutdown(&mut rig.log);
    assert!(gw.mappings().is_empty(), "deleted when the stop returns: {:?}", gw.mappings());
    assert!(!rig.log_text().contains("did not confirm"), "{}", rig.log_text());
}

/// N47: the invite waits for the reachability at most `SETTLE_WAIT`, then
/// goes out without an address (the router is slow: `LateUpnp`), and is
/// printed again once the mapping gives the server a direct address.
#[test]
fn the_invite_waits_at_most_settle_wait_and_follows_the_address() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let external = Ipv4Addr::new(203, 0, 113, 7);
    let gw = FakeGateway::start(true, false, false, external, Misbehaviour::LateUpnp);
    let mut rig = Rig::new(dir, mapped_config(&gw, nostr_config(&relay, &[])));
    let port = rig.server().local_port();
    // The server's clock already past `SETTLE_WAIT`; the router still busy.
    let later = || Instant::now() + rl_server::SETTLE_WAIT;
    let Rig { server, log, .. } = &mut rig;
    let server = server.as_mut().unwrap();
    server.poll(later(), log);
    let first = String::from_utf8_lossy(log).into_owned();
    assert!(
        first.contains("invite code (joins through the relays): rlx1"),
        "printed at SETTLE_WAIT: {first}"
    );
    assert!(!first.contains("reach: Port"), "before the router answered: {first}");
    let begun = Instant::now();
    while !String::from_utf8_lossy(log).contains("\ninvite code: rlx1") {
        assert!(
            begun.elapsed() < Duration::from_secs(10),
            "the second invite: {}",
            String::from_utf8_lossy(log)
        );
        server.poll(later(), log);
        std::thread::sleep(Duration::from_millis(2));
    }
    let text = rig.log_text();
    assert_eq!(text.lines().filter(|l| l.starts_with("invite code")).count(), 2, "{text}");
    let mapped = gw.mappings().iter().find(|m| m.1).unwrap().2;
    assert_eq!(mapped, port, "the local port");
    assert_eq!(printed_invite(&text).unwrap().addr, Some(SocketAddr::from((external, port))));
}
