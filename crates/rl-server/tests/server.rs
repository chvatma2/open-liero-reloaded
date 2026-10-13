//! The dedicated server (networking spec §7.4) driven in-process against
//! `rl_online` clients on 127.0.0.1: a rotation played to its next entry,
//! admins' chat commands, bans kept across a restart, the example config.
//! Classic on Dirt Level from `RL_CONTENT_DIR` (skipped with a notice
//! without it); every wait is bounded, each test under 20 s.

use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rl_content::Content;
use rl_lobby::{Identity, NOT_ADMIN};
use rl_net::endpoint::EndpointConfig;
use rl_online::{OnlineEvent, OnlinePeer, PeerRole};
use rl_proto::{Look, Phase, PublicKey, SeatWire};
use rl_server::config::Config;
use rl_server::files::{IDENTITY_FILE, hex};
use rl_server::{NOT_AN_ADMIN_TARGET, Server, run};
use rl_sim::Keys;

const DIRT: &str = "olx-level-dirt-level";
const FACTORY: &str = "olx-level-liero-factory";

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

/// A server's config: port 0, no LAN announcements, Classic on Dirt Level
/// then on Liero Factory, each `minutes` long.
fn config(admins: &[PublicKey], fill_bots: u8, start_when_ready: bool, minutes: f32) -> Config {
    let admins: Vec<String> = admins.iter().map(|k| format!("{:?}", hex(k))).collect();
    let text = format!(
        r#"(
            name: "Test server",
            visibility: DirectOnly,
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
        let data = tempfile::tempdir().unwrap();
        let mut rig =
            Rig { content_dir, server_content, data, server: None, log: Vec::new(), clients: Vec::new() };
        rig.restart(config);
        rig
    }

    /// Stops the server (if one runs) and starts it again with `config`.
    fn restart(&mut self, config: Config) {
        if let Some(mut s) = self.server.take() {
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
        let key = me.public();
        let addr = SocketAddr::from(([127, 0, 0, 1], self.server().local_port()));
        let role = PeerRole::Client { addr, password: None };
        let cfg = EndpointConfig { port: 0, ..EndpointConfig::default() };
        let p = OnlinePeer::start(role, cfg, me, look(name), content(&self.content_dir), None).unwrap();
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
    for want in ["join Anna", "join Bert", "match start", "match end", "winner"] {
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
    let role = PeerRole::Client { addr, password: None };
    let mut p = OnlinePeer::start(role, cfg, identity(), look("Anna"), content(&dir), None).unwrap();
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
    // `cargo xtask e2e`'s: port 0, one short match for the game and a bot client.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/server/e2e.ron");
    let e2e = Config::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!((e2e.port, e2e.min_players, e2e.fill_bots, e2e.rotation.len()), (0, 2, 0, 1));
    let Some(dir) = content_dir() else { return };
    // Their rotations' mods and levels are the committed packs.
    let content = content(&dir);
    config.check(&content).unwrap();
    e2e.check(&content).unwrap();
}
