//! `cargo xtask net-nat` (plan 12b Task 11, networking spec §9.7, ruling
//! N58): the Docker NAT tests. Home routers in containers (nftables
//! masquerade, `masquerade fully-random`, miniupnpd) between isolated LANs
//! and a test WAN on documentation addresses (203.0.113.0/24), a Nostr
//! relay, a STUN server and a TURN relay (`rl-server --relay`) on the WAN,
//! and our host-built release binaries playing each scenario of
//! [`SCENARIOS`]: a direct join, port mapping (UPnP; PCP or NAT-PMP), hole
//! punching, the TURN relay, its absence, finding a listed game, and
//! `rl-server --nat-check`. Every network is `--internal` with a dummy
//! parent (macvlan): nothing reaches the host, its router or the Internet.
//! The image build (`tools/net-nat/Dockerfile`, FROM the host's own
//! distribution, so the binaries run unchanged) is the only network use,
//! and its image is cached by tag. Topology and timings:
//! `tools/net-nat/CLAUDE.md`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::{olx_timing, oracle, root};

/// The image's build context.
pub const TOOL_DIR: &str = "tools/net-nat";
/// The files the image is made from (with the base, its tag's hash).
pub const TOOL_FILES: [&str; 4] = ["Dockerfile", "router.sh", "node.sh", "miniupnpd.conf.in"];
/// The image repository; superseded tags are pruned like the oracle's.
pub const REPO: &str = "rl-nat";
/// The configs (`configs/`) and each scenario's container logs (`<scenario>/`).
pub const OUT: &str = "target/net-nat";
/// The content mounted at `/rl/content`.
pub const CONTENT: &str = "content/packs";
/// The label of every container and network: leftovers of an interrupted
/// run are removed by it.
const LABEL: &str = "rl-nat";
/// Each scenario's bound, from its first container to its verdict.
pub const LIMIT: Duration = Duration::from_secs(60);
/// A joiner gives up after this (its `--timeout`), inside [`LIMIT`].
const JOINER_TIMEOUT: &str = "40";
/// How often logs and states are looked at.
const POLL: Duration = Duration::from_millis(200);
/// Lines of each container's log a failure prints.
const TAIL: usize = 40;
/// The WAN's Nostr relay (in the infrastructure container, with the STUN server).
pub const RELAY_URL: &str = "ws://203.0.113.10:7777";
/// Our STUN servers: the infrastructure's and the TURN relay's binding answers.
pub const STUN: [&str; 2] = ["203.0.113.10:3479", "203.0.113.20:3478"];
/// The listed game's name (`finding`).
const LISTED_NAME: &str = "nat finding";
/// The WAN: the routers' outsides and the infrastructure.
pub const WAN: (&str, &str) = ("rl-nat-wan", "203.0.113.0/24");

/// A home network behind its router.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lan {
    /// Port-restricted cone (`masquerade`).
    A,
    /// Symmetric (`masquerade fully-random`).
    B,
    /// Cone, plus miniupnpd's UPnP-IGD.
    C,
    /// Cone, plus miniupnpd's NAT-PMP and PCP.
    D,
}

impl Lan {
    pub const ALL: [Lan; 4] = [Lan::A, Lan::B, Lan::C, Lan::D];

    /// 1 to 4: the subnet's second byte and the router's WAN address' 10x.
    fn n(self) -> u8 {
        self as u8 + 1
    }

    fn letter(self) -> char {
        ['a', 'b', 'c', 'd'][self as usize]
    }

    pub fn network(self) -> String {
        format!("rl-nat-{}", self.letter())
    }

    pub fn subnet(self) -> String {
        format!("10.{}.0.0/24", self.n())
    }

    /// The router's LAN address (`.2`), every node's gateway.
    pub fn router(self) -> String {
        self.addr(2)
    }

    /// The router's WAN address, 203.0.113.101 to .104.
    pub fn router_wan(self) -> String {
        format!("203.0.113.{}", 100 + self.n())
    }

    /// `router.sh`'s kind.
    pub fn kind(self) -> &'static str {
        ["cone", "symmetric", "upnp", "pcp"][self as usize]
    }

    fn addr(self, host: u8) -> String {
        format!("10.{}.0.{host}", self.n())
    }
}

/// Every network: name and subnet.
pub fn networks() -> Vec<(String, String)> {
    let mut all = vec![(WAN.0.to_string(), WAN.1.to_string())];
    all.extend(Lan::ALL.iter().map(|l| (l.network(), l.subnet())));
    all
}

/// The config file a server reads (`target/net-nat/configs/<name>.ron`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Config {
    /// The TURN relay at 203.0.113.20.
    Relay,
    /// The public game server at 203.0.113.30.
    Public,
    /// Invite only, its port mapped by the router.
    Mapped,
    /// Invite only, no port mapping.
    Invite,
    /// Public (listed on the relay), no port mapping.
    Listed,
}

impl Config {
    pub const ALL: [Config; 5] =
        [Config::Relay, Config::Public, Config::Mapped, Config::Invite, Config::Listed];

    pub fn file(self) -> &'static str {
        ["relay.ron", "public.ron", "mapped.ron", "invite.ron", "listed.ron"][self as usize]
    }

    /// The config's text (RON, `rl-server`'s `Config`): every one on the
    /// WAN's relay with `allow_local`, one short Classic match on Dirt
    /// Level that starts when the players are ready (a bot fills the
    /// second seat of a LAN host).
    pub fn text(self) -> String {
        let (name, visibility, mapping, address, stun, bots) = match self {
            Config::Relay => ("nat relay", "Invite", false, Some("203.0.113.20"), &STUN[..1], 0),
            Config::Public => ("nat public", "Public", false, Some("203.0.113.30"), &STUN[..], 0),
            Config::Mapped => ("nat host", "Invite", true, None, &STUN[..], 2),
            Config::Invite => ("nat host", "Invite", false, None, &STUN[..], 2),
            Config::Listed => (LISTED_NAME, "Public", false, None, &STUN[..], 2),
        };
        let quoted = |items: &[&str]| items.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", ");
        let address = address.map_or_else(|| "None".into(), |a| format!("Some({a:?})"));
        format!(
            "// Written by `cargo xtask net-nat` (xtask/src/nat.rs).\n(\n    name: {name:?},\n    \
             visibility: {visibility},\n    relays: [{}],\n    stun: [{}],\n    allow_local: true,\n    \
             port_mapping: {mapping},\n    public_address: {address},\n    max_players: 4,\n    \
             fill_bots: {bots},\n    min_players: 1,\n    start_when_ready: true,\n    rotation: [\n        \
             (mod: \"olx-classic\", level: \"olx-level-dirt-level\", time_limit: 0.2, kill_limit: 0, \
             lives: -2, bonuses: true),\n    ],\n)\n",
            quoted(&[RELAY_URL]),
            quoted(stun),
        )
    }
}

/// The game host of a scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    /// `rl-server` on the WAN at 203.0.113.30 ([`Config::Public`]).
    Public,
    /// `rl-server` in a LAN (at `.10`).
    Server(Lan, Config),
    /// `rl-server --nat-check` in a LAN, no joiner.
    NatCheck(Lan),
}

/// How the joiner finds the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    /// `--join <address>`.
    Join(&'static str),
    /// `--invite <the code from the host's log>`.
    Invite,
    /// `--listed <name>`.
    Listed(&'static str),
}

/// `rl-cli net-client` in a LAN (at `.20`).
#[derive(Clone, Copy, Debug)]
pub struct Joiner {
    pub lan: Lan,
    pub way: Way,
    /// More flags (`--lobby-only`, `--no-nostr-turn`, `--matches 1`).
    pub flags: &'static [&'static str],
}

/// Which invite line the host prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    /// `invite code: rlx1…`: the code carries an address (waited for).
    Address,
    /// `invite code (joins through the relays): rlx1…`.
    Relays,
}

/// What a scenario must show.
#[derive(Clone, Copy, Debug)]
pub struct Expect {
    /// The joiner's exit status (the nat-check's without one).
    pub exit: i32,
    /// Its stdout holds each.
    pub stdout: &'static [&'static str],
    /// Its stderr holds each.
    pub stderr: &'static [&'static str],
    /// The host's log holds one of each group.
    pub host_log: &'static [&'static [&'static str]],
    /// The TURN relay's log holds each.
    pub relay_log: &'static [&'static str],
    /// The host's last invite line, when it matters.
    pub code: Option<Code>,
    /// The host is stopped (SIGTERM) after the joiner and must exit 0.
    pub host_stops: bool,
}

const EXPECT: Expect = Expect {
    exit: 0,
    stdout: &[],
    stderr: &[],
    host_log: &[],
    relay_log: &[],
    code: None,
    host_stops: false,
};

/// One scenario: its own fresh containers, bounded by [`LIMIT`].
#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub host: Host,
    pub joiner: Option<Joiner>,
    pub expect: Expect,
}

/// The scenarios, in the order they run (spec §9.7; its "browser host"
/// is plan 13's, ruling PL12b-3). Each path is checked on both ends: the
/// joiner's `path:` line and the host's `via` (N49).
pub const SCENARIOS: [Scenario; 8] = [
    Scenario {
        name: "direct",
        host: Host::Public,
        joiner: Some(Joiner { lan: Lan::A, way: Way::Join("203.0.113.30:23400"), flags: &["--lobby-only"] }),
        expect: Expect { stdout: &["path: direct"], host_log: &[&["via direct"]], ..EXPECT },
    },
    Scenario {
        name: "upnp",
        host: Host::Server(Lan::C, Config::Mapped),
        joiner: Some(Joiner { lan: Lan::A, way: Way::Invite, flags: &["--lobby-only"] }),
        expect: Expect {
            stdout: &["path: direct"],
            host_log: &[&["reach: Port 23400 is open on your router (UPnP)"], &["via open port"]],
            code: Some(Code::Address),
            ..EXPECT
        },
    },
    Scenario {
        name: "pcp",
        host: Host::Server(Lan::D, Config::Mapped),
        joiner: Some(Joiner { lan: Lan::A, way: Way::Invite, flags: &["--lobby-only"] }),
        expect: Expect {
            stdout: &["path: direct"],
            host_log: &[
                &[
                    "reach: Port 23400 is open on your router (PCP)",
                    "reach: Port 23400 is open on your router (NAT-PMP)",
                ],
                &["via open port"],
            ],
            code: Some(Code::Address),
            ..EXPECT
        },
    },
    Scenario {
        name: "hole-punching",
        host: Host::Server(Lan::A, Config::Invite),
        joiner: Some(Joiner { lan: Lan::C, way: Way::Invite, flags: &["--lobby-only", "--no-nostr-turn"] }),
        expect: Expect {
            stdout: &["path: punched"],
            host_log: &[&["via punched"]],
            code: Some(Code::Relays),
            ..EXPECT
        },
    },
    Scenario {
        name: "relay",
        host: Host::Server(Lan::A, Config::Invite),
        joiner: Some(Joiner { lan: Lan::B, way: Way::Invite, flags: &["--matches", "1"] }),
        expect: Expect {
            stdout: &["path: relayed", "match 1:"],
            host_log: &[&["via relayed"], &["match end"]],
            relay_log: &["relay: 1 allocations"],
            code: Some(Code::Relays),
            host_stops: true,
            ..EXPECT
        },
    },
    Scenario {
        name: "no-relay",
        host: Host::Server(Lan::A, Config::Invite),
        joiner: Some(Joiner { lan: Lan::B, way: Way::Invite, flags: &["--no-nostr-turn"] }),
        expect: Expect {
            exit: 1,
            stderr: &["no network path to the game"],
            code: Some(Code::Relays),
            ..EXPECT
        },
    },
    Scenario {
        name: "finding",
        host: Host::Server(Lan::A, Config::Listed),
        joiner: Some(Joiner { lan: Lan::B, way: Way::Listed(LISTED_NAME), flags: &["--lobby-only"] }),
        expect: Expect { stdout: &["path: relayed"], host_log: &[&["via relayed"]], ..EXPECT },
    },
    Scenario {
        name: "nat-check",
        host: Host::NatCheck(Lan::B),
        joiner: None,
        expect: Expect { stdout: &["NAT: symmetric NAT"], ..EXPECT },
    },
];

/// The scenarios `--only` names (all when none), in table order; an
/// unknown name is an error listing the known ones.
pub fn select(only: &[String]) -> Result<Vec<&'static Scenario>, String> {
    if let Some(bad) = only.iter().find(|n| !SCENARIOS.iter().any(|s| s.name == n.as_str())) {
        let known: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
        return Err(format!("no scenario {bad:?}: the scenarios are {}", known.join(", ")));
    }
    Ok(SCENARIOS.iter().filter(|s| only.is_empty() || only.iter().any(|n| n == s.name)).collect())
}

/// The image's base, `<ID>:<VERSION_ID>` of `/etc/os-release` (N58: the
/// host's own distribution, so host-built binaries run unchanged), or why
/// there is none (a skip reason). The Dockerfile installs with apt, so
/// the distribution must be Debian or derived from it.
pub fn base_image(os_release: &str) -> Result<String, String> {
    let field = |key: &str| {
        os_release.lines().find_map(|l| {
            let value = l.trim().strip_prefix(key)?.strip_prefix('=')?;
            Some(value.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        })
    };
    let id = field("ID").filter(|v| !v.is_empty()).ok_or("/etc/os-release names no ID")?;
    let version = field("VERSION_ID").filter(|v| !v.is_empty()).ok_or_else(|| {
        format!("/etc/os-release names no VERSION_ID ({id} without a release): no image of the same release to build on")
    })?;
    let like = format!("{id} {}", field("ID_LIKE").unwrap_or_default());
    if !like.split_whitespace().any(|w| w == "debian" || w == "ubuntu") {
        return Err(format!(
            "the image installs its packages with apt, and {id} is not Debian or derived from it"
        ));
    }
    Ok(format!("{id}:{version}"))
}

/// `rl-nat:<the first 12 hex of SHA-256(files ++ base)>`: an edit to any
/// file, or another base, builds a new image.
pub fn image_tag(files: &[(&str, &[u8])], base: &str) -> String {
    let mut h = Sha256::new();
    for (name, data) in files {
        h.update(name.as_bytes());
        h.update([0u8]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(data);
    }
    h.update(b"base\0");
    h.update(base.as_bytes());
    format!("{REPO}:{}", &hex::encode(h.finalize())[..12])
}

/// The last invite code `rl-server` logged, and whether it carries an
/// address (`invite code: rlx1…`, not `invite code (joins through the
/// relays): rlx1…`).
pub fn invite_code(log: &str) -> Option<(String, bool)> {
    log.lines().rev().find_map(|l| {
        let rest = &l[l.find("invite code")? + "invite code".len()..];
        let (with_address, code) = match rest.strip_prefix(": ") {
            Some(code) => (true, code),
            None => (false, rest.strip_prefix(" (joins through the relays): ")?),
        };
        let code = code.split_whitespace().next()?;
        code.starts_with("rlx1").then(|| (code.to_string(), with_address))
    })
}

/// The last `n` lines of `text`.
fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// What a scenario's containers showed.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// The joiner's (or nat-check's) exit status; `None`: still running at the bound.
    pub exit: Option<i64>,
    pub stdout: String,
    pub stderr: String,
    pub host_log: String,
    pub relay_log: String,
    /// The host's exit status once stopped (when stopped).
    pub host_exit: Option<i64>,
}

/// Everything `seen` misses of `expect`, in words (empty: passed).
pub fn verdict(expect: &Expect, seen: &Seen) -> Vec<String> {
    let mut wrong = Vec::new();
    match seen.exit {
        Some(code) if code == i64::from(expect.exit) => {}
        Some(code) => wrong.push(format!("exited with {code}, not {}", expect.exit)),
        None => wrong.push(format!("did not exit within {} s", LIMIT.as_secs())),
    }
    for want in expect.stdout {
        if !seen.stdout.contains(want) {
            wrong.push(format!("stdout lacks {want:?}"));
        }
    }
    for want in expect.stderr {
        if !seen.stderr.contains(want) {
            wrong.push(format!("stderr lacks {want:?}"));
        }
    }
    for group in expect.host_log {
        if !group.iter().any(|w| seen.host_log.contains(w)) {
            wrong.push(format!(
                "the host's log lacks {}",
                group.iter().map(|w| format!("{w:?}")).collect::<Vec<_>>().join(" or ")
            ));
        }
    }
    for want in expect.relay_log {
        if !seen.relay_log.contains(want) {
            wrong.push(format!("the relay's log lacks {want:?}"));
        }
    }
    if let Some(code) = expect.code {
        match invite_code(&seen.host_log) {
            Some((_, with)) if with == (code == Code::Address) => {}
            Some((_, with)) => wrong
                .push(format!("the host's invite code {} an address", if with { "has" } else { "lacks" })),
            None => wrong.push("the host logged no invite code".into()),
        }
    }
    if expect.host_stops && seen.host_exit != Some(0) {
        wrong.push(match seen.host_exit {
            Some(code) => format!("the host exited with {code} when stopped, not 0"),
            None => "the host did not stop within 5 s of SIGTERM".into(),
        });
    }
    wrong
}

/// When a container counts as ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ready {
    /// Its stdout has every one of these.
    Lines(Vec<&'static str>),
    /// Its log has an invite code (with an address, for `Some(Address)`).
    Invite(Option<Code>),
    /// It exited (the joiner, the nat-check).
    Exit,
}

/// One container of a scenario.
#[derive(Clone, Debug)]
pub struct Unit {
    /// `rl-nat-<role>`.
    pub role: String,
    /// Units start in stages; a stage starts once the last is ready.
    pub stage: u8,
    /// `docker run` arguments after `-d` (the image, then the command; a
    /// `{invite}` argument is replaced by the host's code).
    pub args: Vec<String>,
    pub ready: Ready,
}

/// Where things are on this machine.
pub struct Paths {
    pub bin: PathBuf,
    pub content: PathBuf,
    pub configs: PathBuf,
    pub image: String,
}

/// The containers of `sc`, in stages: the WAN's infrastructure (Nostr
/// relay and STUN server); the TURN relay, the routers of the LANs used
/// and the public server; the host; the joiner. Every one joins only our
/// networks (never Docker's default bridge) and carries [`LABEL`].
pub fn units(sc: &Scenario, paths: &Paths) -> Vec<Unit> {
    let mounts = || -> Vec<String> {
        let bin = |name: &str| format!("{}:/rl/bin/{name}:ro", paths.bin.join(name).display());
        vec![
            "-v".into(),
            bin("rl-server"),
            "-v".into(),
            bin("rl-cli"),
            "-v".into(),
            format!("{}:/rl/content:ro", paths.content.display()),
            "-v".into(),
            format!("{}:/rl/config:ro", paths.configs.display()),
        ]
    };
    let unit = |role: &str, stage: u8, nets: &[(String, String)], extra: &[&str], cmd: Vec<String>, ready| {
        let mut args: Vec<String> =
            vec!["--name".into(), format!("rl-nat-{role}"), "--label".into(), format!("{LABEL}={}", sc.name)];
        args.push("--init".into());
        for (net, ip) in nets {
            args.push("--network".into());
            args.push(format!("name={net},ip={ip}"));
        }
        args.extend(extra.iter().map(|s| s.to_string()));
        args.push(paths.image.clone());
        args.extend(cmd);
        Unit { role: role.into(), stage, args, ready }
    };
    let wan = |ip: &str| vec![(WAN.0.to_string(), ip.to_string())];
    let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let server = |config: Config| {
        let config = format!("/rl/config/{}", config.file());
        strings(&[
            "/rl/bin/rl-server",
            "--config",
            &config,
            "--content",
            "/rl/content",
            "--data-dir",
            "/tmp/rl",
        ])
    };
    let on_lan = |lan: Lan, cmd: Vec<String>| {
        let mut all = vec!["/rl-nat/node.sh".to_string(), lan.router()];
        all.extend(cmd);
        all
    };
    let with_mounts = |extra: &[&str]| {
        let mut all = mounts();
        all.extend(extra.iter().map(|s| s.to_string()));
        all
    };
    let node_opts = with_mounts(&["--cap-add", "NET_ADMIN"]);
    let node_opts: Vec<&str> = node_opts.iter().map(String::as_str).collect();
    let wan_opts = mounts();
    let wan_opts: Vec<&str> = wan_opts.iter().map(String::as_str).collect();

    let mut out = vec![unit(
        "infra",
        0,
        &wan("203.0.113.10"),
        &wan_opts,
        strings(&[
            "sh",
            "-c",
            "/rl/bin/rl-cli nostr-relay --listen 0.0.0.0:7777 & exec /rl/bin/rl-cli stun-server --listen 0.0.0.0:3479",
        ]),
        Ready::Lines(vec!["nostr relay:", "stun server:"]),
    )];
    let mut relay_cmd = server(Config::Relay);
    relay_cmd.push("--relay".into());
    out.push(unit(
        "relay",
        1,
        &wan("203.0.113.20"),
        &wan_opts,
        relay_cmd,
        Ready::Lines(vec!["relay: TURN on port"]),
    ));
    let host_lan = match sc.host {
        Host::Public => None,
        Host::Server(lan, _) | Host::NatCheck(lan) => Some(lan),
    };
    let mut lans: Vec<Lan> = host_lan.into_iter().chain(sc.joiner.map(|j| j.lan)).collect();
    lans.sort_by_key(|l| *l as u8);
    lans.dedup();
    for lan in lans {
        out.push(unit(
            &format!("router-{}", lan.letter()),
            1,
            &[(WAN.0.to_string(), lan.router_wan()), (lan.network(), lan.router())],
            &["--cap-add", "NET_ADMIN", "--sysctl", "net.ipv4.ip_forward=1"],
            strings(&["/rl-nat/router.sh", lan.kind(), &lan.router_wan(), &lan.router()]),
            Ready::Lines(vec!["router ready"]),
        ));
    }
    match sc.host {
        Host::Public => out.push(unit(
            "public",
            1,
            &wan("203.0.113.30"),
            &wan_opts,
            server(Config::Public),
            Ready::Invite(None),
        )),
        Host::Server(lan, config) => out.push(unit(
            "host",
            2,
            &[(lan.network(), lan.addr(10))],
            &node_opts,
            on_lan(lan, server(config)),
            Ready::Invite(sc.expect.code.filter(|c| *c == Code::Address)),
        )),
        Host::NatCheck(lan) => out.push(unit(
            "host",
            2,
            &[(lan.network(), lan.addr(10))],
            &node_opts,
            on_lan(
                lan,
                strings(&[
                    "/rl/bin/rl-server",
                    "--nat-check",
                    "--config",
                    &format!("/rl/config/{}", Config::Invite.file()),
                ]),
            ),
            Ready::Exit,
        )),
    }
    if let Some(j) = sc.joiner {
        let mut cmd = strings(&[
            "/rl/bin/rl-cli",
            "net-client",
            "--bot",
            "--name",
            "nat joiner",
            "--content",
            "/rl/content",
            "--relay",
            RELAY_URL,
            "--stun",
            STUN[0],
            "--stun",
            STUN[1],
            "--allow-local",
            "--timeout",
            JOINER_TIMEOUT,
        ]);
        cmd.extend(match j.way {
            Way::Join(addr) => strings(&["--join", addr]),
            Way::Invite => strings(&["--invite", "{invite}"]),
            Way::Listed(name) => strings(&["--listed", name]),
        });
        cmd.extend(strings(j.flags));
        out.push(unit(
            "joiner",
            3,
            &[(j.lan.network(), j.lan.addr(20))],
            &node_opts,
            on_lan(j.lan, cmd),
            Ready::Exit,
        ));
    }
    out
}

/// `docker <args>`: its stdout, or why it failed.
fn docker(args: &[&str]) -> Result<String, String> {
    let out = Command::new("docker").args(args).output().map_err(|e| format!("running docker: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "`docker {}` failed: {}",
            args.join(" "),
            tail(String::from_utf8_lossy(&out.stderr).trim(), TAIL)
        ))
    }
}

/// A container's stdout and stderr so far (empty when it is gone).
fn logs(role: &str) -> (String, String) {
    match Command::new("docker").args(["logs", &format!("rl-nat-{role}")]).output() {
        Ok(o) => {
            (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
        }
        Err(_) => Default::default(),
    }
}

/// A container's exit status, once it exited.
fn exited(role: &str) -> Option<i64> {
    let state =
        docker(&["inspect", "-f", "{{.State.Running}} {{.State.ExitCode}}", &format!("rl-nat-{role}")])
            .ok()?;
    let (running, code) = state.trim().split_once(' ')?;
    (running == "false").then(|| code.parse().ok()).flatten()
}

/// Removes every container labelled [`LABEL`] (a scenario's, or an
/// interrupted run's).
fn remove_containers() {
    if let Ok(ids) = docker(&["ps", "-aq", "--filter", &format!("label={LABEL}")]) {
        let ids: Vec<&str> = ids.split_whitespace().collect();
        if !ids.is_empty() {
            let mut args = vec!["rm", "-f"];
            args.extend(ids);
            if let Err(e) = docker(&args) {
                eprintln!("warning: {e}");
            }
        }
    }
}

/// Removes every network labelled [`LABEL`] (containers first).
fn remove_networks() {
    remove_containers();
    if let Ok(ids) = docker(&["network", "ls", "-q", "--filter", &format!("label={LABEL}")]) {
        for id in ids.split_whitespace() {
            if let Err(e) = docker(&["network", "rm", id]) {
                eprintln!("warning: {e}");
            }
        }
    }
}

/// The networks for one run, removed (with any container left) when
/// dropped unless kept.
struct Networks {
    keep: bool,
}

impl Networks {
    /// Our networks: `--internal` macvlan networks, each on a dummy parent
    /// of its own (Docker makes one for an internal macvlan network
    /// without a `parent`). Not bridges: Docker's rules for an internal
    /// bridge drop every bridged packet addressed outside its subnet
    /// (`br_netfilter`), which is all of a router's traffic. Nothing on
    /// them reaches the host or beyond.
    fn create(keep: bool) -> Result<Networks, String> {
        remove_networks();
        let made = Networks { keep };
        for (name, subnet) in networks() {
            docker(&[
                "network",
                "create",
                "-d",
                "macvlan",
                "--internal",
                "--label",
                LABEL,
                "--subnet",
                &subnet,
                &name,
            ])?;
        }
        Ok(made)
    }
}

impl Drop for Networks {
    fn drop(&mut self) {
        if !self.keep {
            remove_networks();
        }
    }
}

/// A scenario's containers: their logs saved to its directory and the
/// containers removed (unless kept) when dropped, so a failing or
/// panicking scenario leaves nothing running.
struct Containers<'a> {
    dir: &'a Path,
    roles: Vec<String>,
    keep: bool,
}

impl Containers<'_> {
    fn save_logs(&self) {
        for role in &self.roles {
            let (out, err) = logs(role);
            let _ = std::fs::write(self.dir.join(format!("{role}.out")), out);
            let _ = std::fs::write(self.dir.join(format!("{role}.err")), err);
        }
    }

    /// The last [`TAIL`] lines of each container's output, for a failure.
    fn tails(&self) -> String {
        let mut text = String::new();
        for role in &self.roles {
            let (out, err) = logs(role);
            for (stream, s) in [("stdout", out), ("stderr", err)] {
                if !s.trim().is_empty() {
                    text.push_str(&format!(
                        "--- {role} {stream} (last {TAIL} lines)\n{}\n",
                        tail(s.trim_end(), TAIL)
                    ));
                }
            }
        }
        text
    }
}

impl Drop for Containers<'_> {
    fn drop(&mut self) {
        self.save_logs();
        if !self.keep {
            remove_containers();
        }
    }
}

/// Plays one scenario: its units stage by stage, each waited for until
/// [`LIMIT`] from the start, then the host stopped when asked, and the
/// verdict. `Err`: why it failed and the containers' log tails.
fn play(sc: &Scenario, paths: &Paths, dir: &Path, keep: bool) -> Result<(), String> {
    remove_containers();
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let deadline = Instant::now() + LIMIT;
    let mut started = Containers { dir, roles: Vec::new(), keep };
    let result = run_units(sc, &units(sc, paths), deadline, &mut started).and_then(|()| {
        let mut seen = Seen::default();
        let main = if sc.joiner.is_some() { "joiner" } else { "host" };
        seen.exit = exited(main);
        (seen.stdout, seen.stderr) = logs(main);
        if sc.expect.host_stops {
            let _ = docker(&["stop", "-t", "5", "rl-nat-host"]);
            seen.host_exit = exited("host");
        }
        let host = if sc.host == Host::Public { "public" } else { "host" };
        seen.host_log = logs(host).0;
        seen.relay_log = logs("relay").0;
        let wrong = verdict(&sc.expect, &seen);
        if wrong.is_empty() { Ok(()) } else { Err(wrong.join("; ")) }
    });
    result.map_err(|why| format!("{why}\n{}", started.tails()))
}

/// Starts `units` stage by stage, waiting for each to be ready.
fn run_units(
    sc: &Scenario,
    units: &[Unit],
    deadline: Instant,
    started: &mut Containers,
) -> Result<(), String> {
    let mut invite: Option<String> = None;
    for stage in 0..=units.iter().map(|u| u.stage).max().unwrap_or(0) {
        let now: Vec<&Unit> = units.iter().filter(|u| u.stage == stage).collect();
        for u in &now {
            if u.args.iter().any(|a| a == "{invite}") && invite.is_none() {
                return Err(format!("{}: no invite code to join with", sc.name));
            }
            let mut args: Vec<&str> = vec!["run", "-d"];
            args.extend(u.args.iter().map(|a| match (a.as_str(), &invite) {
                ("{invite}", Some(code)) => code.as_str(),
                _ => a.as_str(),
            }));
            docker(&args)?;
            started.roles.push(u.role.clone());
        }
        for u in &now {
            let found = wait_ready(u, deadline)?;
            if found.is_some() {
                invite = found;
            }
        }
    }
    Ok(())
}

/// Waits until `u` is ready; an invite code when it was waited for.
fn wait_ready(u: &Unit, deadline: Instant) -> Result<Option<String>, String> {
    loop {
        let (out, _) = logs(&u.role);
        match &u.ready {
            Ready::Lines(lines) if lines.iter().all(|l| out.contains(l)) => return Ok(None),
            Ready::Invite(code) => {
                if let Some((c, with)) = invite_code(&out)
                    && (with || *code != Some(Code::Address))
                {
                    return Ok(Some(c));
                }
            }
            Ready::Exit if exited(&u.role).is_some() => return Ok(None),
            _ => {}
        }
        if !matches!(u.ready, Ready::Exit)
            && let Some(code) = exited(&u.role)
        {
            return Err(format!("{} exited with {code} before it was ready", u.role));
        }
        if Instant::now() >= deadline {
            let secs = LIMIT.as_secs();
            return Err(match &u.ready {
                Ready::Lines(l) => format!("{} logged no {l:?} within {secs} s", u.role),
                Ready::Invite(Some(Code::Address)) => {
                    format!("{} logged no invite code with an address within {secs} s", u.role)
                }
                Ready::Invite(_) => format!("{} logged no invite code within {secs} s", u.role),
                Ready::Exit => format!("{} did not exit within {secs} s", u.role),
            });
        }
        std::thread::sleep(POLL);
    }
}

/// How a run ended.
pub enum Outcome {
    /// A prerequisite is missing (the reason and the fix).
    Skipped(String),
    /// Every selected scenario passed.
    Passed(usize),
}

/// Why `net-nat` cannot run here, if it cannot: Docker, the host's
/// distribution, the content. `Ok`: the image's base.
fn prerequisites(root: &Path) -> Result<String, String> {
    if let Some(why) = olx_timing::docker_missing() {
        return Err(why);
    }
    let os = std::fs::read_to_string("/etc/os-release")
        .map_err(|e| format!("no /etc/os-release ({e}): the image is built FROM the host's distribution"))?;
    let base = base_image(&os)?;
    if !root.join(CONTENT).join("olx-classic").is_dir() {
        return Err(format!("no content in {CONTENT}: run `cargo xtask fetch-olx-content`"));
    }
    Ok(base)
}

/// The binaries, in release: `rl-server`, and `rl-cli` with its dev
/// tools (`nat-tools`) in a cargo run of its own (cargo leaves
/// `rl-cli/nat-tools` off when both packages are built in one run, and
/// rl-server then needs no test relay).
fn build(root: &Path) -> Result<(), String> {
    for args in [&["-p", "rl-server"][..], &["-p", "rl-cli", "--features", "nat-tools"]] {
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "--release", "--quiet"])
            .args(args)
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        if !out.status.success() {
            return Err(tail(&String::from_utf8_lossy(&out.stderr), 200));
        }
    }
    Ok(())
}

/// The image for `base`, built when its tag is missing (the only network
/// use: the base and the packages), then the superseded `rl-nat` images
/// pruned. `Ok(Err(why))`: the base cannot be had (a skip).
fn ensure_image(root: &Path, base: &str) -> Result<Result<String, String>, String> {
    let mut files = Vec::new();
    for f in TOOL_FILES {
        let path = root.join(TOOL_DIR).join(f);
        files.push((f, std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?));
    }
    let tag = image_tag(&files.iter().map(|(n, d)| (*n, d.as_slice())).collect::<Vec<_>>(), base);
    if docker(&["image", "inspect", &tag]).is_err() {
        if docker(&["image", "inspect", base]).is_err()
            && let Err(e) = docker(&["pull", base])
        {
            return Ok(Err(format!("the base image {base} cannot be pulled ({e})")));
        }
        println!("     net-nat: building {tag} FROM {base}");
        let built = Instant::now();
        let context = root.join(TOOL_DIR);
        docker(&[
            "build",
            "-q",
            "-t",
            &tag,
            "--build-arg",
            &format!("BASE={base}"),
            &context.to_string_lossy(),
        ])
        .map_err(|e| format!("building {tag}: {e}"))?;
        println!("     net-nat: built {tag} in {:.1} s", built.elapsed().as_secs_f32());
    }
    oracle::prune_old(&tag, false);
    Ok(Ok(tag))
}

/// Runs the scenarios (`only`, else all): prerequisites, build, image,
/// configs and networks, then each scenario. `verbose` prints a line per
/// scenario. `Err`: what failed, every failing scenario's report.
pub fn run(only: &[String], keep: bool, verbose: bool) -> Result<Outcome, String> {
    let scenarios = select(only)?;
    let root = root();
    let base = match prerequisites(&root) {
        Ok(base) => base,
        Err(why) => return Ok(Outcome::Skipped(why)),
    };
    build(&root)?;
    let image = match ensure_image(&root, &base)? {
        Ok(tag) => tag,
        Err(why) => return Ok(Outcome::Skipped(why)),
    };
    let out = root.join(OUT);
    let configs = out.join("configs");
    std::fs::create_dir_all(&configs).map_err(|e| format!("{}: {e}", configs.display()))?;
    for c in Config::ALL {
        std::fs::write(configs.join(c.file()), c.text()).map_err(|e| format!("{}: {e}", c.file()))?;
    }
    let paths = Paths { bin: root.join("target/release"), content: root.join(CONTENT), configs, image };
    let _networks = Networks::create(keep)?;
    let mut failed = Vec::new();
    for sc in &scenarios {
        let t = Instant::now();
        let result = play(sc, &paths, &out.join(sc.name), keep);
        let secs = t.elapsed().as_secs_f32();
        match result {
            Ok(()) if verbose => println!("     net-nat: {} passed in {secs:.1} s", sc.name),
            Ok(()) => {}
            Err(why) => failed.push(format!("scenario {} failed after {secs:.1} s: {why}", sc.name)),
        }
    }
    if failed.is_empty() { Ok(Outcome::Passed(scenarios.len())) } else { Err(failed.join("\n")) }
}

/// `cargo xtask net-nat [--keep] [--only <scenario>]...`.
pub fn command(only: &[String], keep: bool) -> anyhow::Result<bool> {
    select(only).map_err(anyhow::Error::msg)?;
    let t = Instant::now();
    match run(only, keep, true) {
        Ok(Outcome::Skipped(why)) => {
            println!("skip net-nat ({why})");
            Ok(true)
        }
        Ok(Outcome::Passed(n)) => {
            println!("ok   net-nat ({n} scenarios, {:.1}s)", t.elapsed().as_secs_f32());
            Ok(true)
        }
        Err(why) => {
            println!("FAIL net-nat ({:.1}s)\n{why}", t.elapsed().as_secs_f32());
            Ok(false)
        }
    }
}

/// `full`'s step: every scenario; `Ok` with the `ok` line's detail
/// (`8 scenarios, `), after a `skip` line when a prerequisite is missing.
pub fn step() -> Result<String, String> {
    match run(&[], false, false)? {
        Outcome::Skipped(why) => {
            println!("skip net-nat ({why})");
            Ok(String::new())
        }
        Outcome::Passed(n) => Ok(format!("{n} scenarios, ")),
    }
}

/// `tidy`: removes superseded `rl-nat` images (none when the current tag
/// cannot be computed here).
pub fn prune() {
    let root = root();
    let Ok(base) =
        std::fs::read_to_string("/etc/os-release").map_err(|e| e.to_string()).and_then(|os| base_image(&os))
    else {
        return;
    };
    let files: Result<Vec<_>, _> =
        TOOL_FILES.iter().map(|f| std::fs::read(root.join(TOOL_DIR).join(f)).map(|d| (*f, d))).collect();
    if let Ok(files) = files {
        oracle::prune_old(
            &image_tag(&files.iter().map(|(n, d)| (*n, d.as_slice())).collect::<Vec<_>>(), &base),
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_image_from_os_release() {
        let ubuntu = "PRETTY_NAME=\"Ubuntu 26.04 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"26.04\"\nID=ubuntu\nID_LIKE=debian\n";
        assert_eq!(base_image(ubuntu), Ok("ubuntu:26.04".to_string()));
        let debian = "PRETTY_NAME=\"Debian GNU/Linux 12 (bookworm)\"\nNAME=\"Debian GNU/Linux\"\nVERSION_ID=\"12\"\nID=debian\n";
        assert_eq!(base_image(debian), Ok("debian:12".to_string()));
        let testing = "PRETTY_NAME=\"Debian GNU/Linux trixie/sid\"\nID=debian\n";
        let why = base_image(testing).unwrap_err();
        assert!(why.contains("VERSION_ID"), "{why}");
        let fedora = "NAME=\"Fedora Linux\"\nVERSION_ID=40\nID=fedora\n";
        let why = base_image(fedora).unwrap_err();
        assert!(why.contains("apt") && why.contains("fedora"), "{why}");
        let mint = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\nVERSION_ID=\"22\"\n";
        assert_eq!(base_image(mint), Ok("linuxmint:22".to_string()), "pulled, or a skip if no such image");
    }

    #[test]
    fn invite_code_is_read_from_the_log() {
        let log = "2026-10-14 15:31:03 listening on port 23400 (TCP and UDP)\n\
                   2026-10-14 15:31:04 invite code (joins through the relays): rlx1aaa\n";
        assert_eq!(invite_code(log), Some(("rlx1aaa".to_string(), false)));
        let later = format!("{log}2026-10-14 15:31:05 invite code: rlx1bbb\n");
        assert_eq!(invite_code(&later), Some(("rlx1bbb".to_string(), true)));
        assert_eq!(invite_code("listening on port 23400\n"), None);
    }

    #[test]
    fn scenario_names_are_unique_and_selectable() {
        let names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            ["direct", "upnp", "pcp", "hole-punching", "relay", "no-relay", "finding", "nat-check"]
        );
        assert_eq!(select(&[]).unwrap().len(), 8);
        let two = select(&["relay".into(), "direct".into()]).unwrap();
        assert_eq!(two.iter().map(|s| s.name).collect::<Vec<_>>(), ["direct", "relay"], "in table order");
        let why = select(&["browser".into()]).unwrap_err();
        assert!(why.contains("browser") && why.contains("hole-punching"), "{why}");
    }

    fn paths() -> Paths {
        Paths {
            bin: "/repo/target/release".into(),
            content: "/repo/content/packs".into(),
            configs: "/repo/target/net-nat/configs".into(),
            image: "rl-nat:0123456789ab".into(),
        }
    }

    /// Nothing leaves: every container joins only our networks, at an
    /// address of their subnets, never Docker's default bridge or the
    /// host's network, and carries the label its cleanup goes by.
    #[test]
    fn every_container_stays_on_our_networks() {
        let nets = networks();
        for sc in &SCENARIOS {
            let units = units(sc, &paths());
            assert!(units.iter().any(|u| u.role == "infra") && units.iter().any(|u| u.role == "relay"));
            for u in &units {
                let at = |flag: &str| {
                    u.args.windows(2).filter(|w| w[0] == flag).map(|w| w[1].clone()).collect::<Vec<_>>()
                };
                assert_eq!(at("--label"), [format!("rl-nat={}", sc.name)], "{} {}", sc.name, u.role);
                let attached = at("--network");
                assert!(!attached.is_empty(), "{} {}", sc.name, u.role);
                for a in attached {
                    let (name, ip) = a.strip_prefix("name=").unwrap().split_once(",ip=").unwrap();
                    let (_, subnet) = nets.iter().find(|(n, _)| n == name).expect("one of ours");
                    let prefix = subnet.trim_end_matches(".0/24");
                    assert!(ip.starts_with(&format!("{prefix}.")), "{ip} in {subnet}");
                }
                assert!(!u.args.iter().any(|a| {
                    a == "--privileged"
                        || a == "--net"
                        || a.starts_with("--net=")
                        || a.starts_with("--network=")
                }));
                let mounts = at("-v");
                assert!(mounts.iter().all(|m| m.ends_with(":ro")), "read-only mounts: {mounts:?}");
            }
            let joiner = units.iter().find(|u| u.role == "joiner");
            assert_eq!(joiner.is_some(), sc.joiner.is_some());
            if let Some(j) = joiner {
                assert!(j.args.iter().any(|a| a == "--allow-local") && j.ready == Ready::Exit);
                assert_eq!(
                    j.args.iter().any(|a| a == "{invite}"),
                    sc.joiner.unwrap().way == Way::Invite,
                    "{}",
                    sc.name
                );
                assert!(units.iter().any(|u| u.role == "host" || u.role == "public"));
            }
            // The routers of the LANs used, each once.
            let routers: Vec<&str> =
                units.iter().filter(|u| u.role.starts_with("router-")).map(|u| u.role.as_str()).collect();
            let mut deduped = routers.clone();
            deduped.dedup();
            assert_eq!(routers, deduped);
        }
        let upnp = units(&SCENARIOS[1], &paths());
        let host = upnp.iter().find(|u| u.role == "host").unwrap();
        assert_eq!(host.ready, Ready::Invite(Some(Code::Address)), "the code with the mapped address");
        assert!(host.args.windows(2).any(|w| w == ["/rl-nat/node.sh", "10.3.0.2"]));
    }

    #[test]
    fn configs_are_ron_on_our_servers_only() {
        for c in Config::ALL {
            let text = c.text();
            ron::from_str::<ron::Value>(&text).unwrap_or_else(|e| panic!("{}: {e}\n{text}", c.file()));
            assert!(text.contains("allow_local: true") && text.contains(RELAY_URL), "{text}");
            assert!(text.contains("\"203.0.113.10:3479\""), "{text}");
        }
        let relay = Config::Relay.text();
        assert!(relay.contains("public_address: Some(\"203.0.113.20\")") && !relay.contains(STUN[1]));
        assert!(Config::Mapped.text().contains("port_mapping: true"));
        assert!(Config::Invite.text().contains("port_mapping: false"));
        assert!(Config::Listed.text().contains("visibility: Public"));
    }

    #[test]
    fn the_verdict_names_what_is_missing() {
        let relay = &SCENARIOS[4].expect;
        let mut seen = Seen {
            exit: Some(0),
            stdout: "path: relayed\nmatch 1: olx-classic\n".into(),
            host_log: "x invite code (joins through the relays): rlx1a\njoin: b via relayed\nmatch end (time limit)\n"
                .into(),
            relay_log: "relay: 1 allocations\n".into(),
            host_exit: Some(0),
            ..Seen::default()
        };
        assert!(verdict(relay, &seen).is_empty(), "{:?}", verdict(relay, &seen));
        seen.exit = None;
        seen.host_exit = Some(137);
        seen.relay_log.clear();
        let wrong = verdict(relay, &seen).join("; ");
        assert!(
            wrong.contains("did not exit") && wrong.contains("137") && wrong.contains("relay's log"),
            "{wrong}"
        );
        let pcp = &SCENARIOS[2].expect;
        let mapped = Seen {
            exit: Some(0),
            stdout: "path: direct\n".into(),
            host_log:
                "reach: Port 23400 is open on your router (NAT-PMP): x\ninvite code: rlx1b\nvia open port\n"
                    .into(),
            ..Seen::default()
        };
        assert!(verdict(pcp, &mapped).is_empty(), "either method");
        let unmapped = Seen { host_log: "invite code (joins through the relays): rlx1c\n".into(), ..mapped };
        let wrong = verdict(pcp, &unmapped).join("; ");
        assert!(wrong.contains("(PCP)\" or") && wrong.contains("lacks an address"), "{wrong}");
    }

    #[test]
    fn image_tag_changes_with_the_scripts() {
        let files = [("Dockerfile", &b"FROM x"[..]), ("router.sh", b"a"), ("node.sh", b"b")];
        let tag = image_tag(&files, "ubuntu:26.04");
        assert!(tag.starts_with("rl-nat:") && tag.len() == "rl-nat:".len() + 12, "{tag}");
        assert_eq!(tag, image_tag(&files, "ubuntu:26.04"));
        let edited = [("Dockerfile", &b"FROM x"[..]), ("router.sh", b"a "), ("node.sh", b"b")];
        assert_ne!(tag, image_tag(&edited, "ubuntu:26.04"));
        assert_ne!(tag, image_tag(&files, "debian:12"));
    }
}
