//! `cargo xtask net-nat` (plan 12b Task 11, networking spec §9.7, ruling
//! N58): the Docker NAT tests. Home routers in containers (nftables
//! masquerade, `masquerade fully-random`, miniupnpd) between isolated LANs
//! and a test WAN on documentation addresses (203.0.113.0/24), a Nostr
//! relay, a STUN server and a TURN relay (`rl-server --relay`) on the WAN,
//! and our host-built binaries playing each scenario of [`SCENARIOS`]: a
//! direct join, port mapping (UPnP, PCP, NAT-PMP), hole punching, the TURN
//! relay, its absence, finding a listed game, and `rl-server --nat-check`.
//! Every network is `--internal` with a dummy parent (macvlan): nothing
//! reaches the host, its router or the Internet. The image build
//! (`tools/net-nat/Dockerfile`, FROM the host's own distribution, so the
//! binaries run unchanged) is the only network use, and its image is
//! cached by tag. Every container lives at most [`LIFE`] seconds of its
//! own, and Ctrl+C removes them: an interrupted run leaves nothing running
//! for long. Topology and timings: `tools/net-nat/CLAUDE.md`.

use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::{e2e, olx_timing, oracle, root};

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
/// The binaries mounted at `/rl/bin`: `e2e`'s dev builds
/// ([`e2e::build_binaries`]), so `full` builds them once.
pub const BIN: &str = "target/debug";
/// The label key of every container and network (its value: the run's
/// id); a scenario's containers also carry `rl-nat.scenario`.
const LABEL: &str = "rl-nat";
/// Each scenario's bound, from its first container to its verdict.
pub const LIMIT: Duration = Duration::from_secs(60);
/// Every container's own lifetime in seconds (`RL_NAT_LIFE`, `timeout`):
/// past [`LIMIT`] and the host's stop, so a run that was killed leaves
/// nothing running for long.
pub const LIFE: u32 = 120;
/// With `--keep`: an hour to look at the kept containers.
pub const KEEP_LIFE: u32 = 3600;
/// A joiner gives up after this (its `--timeout`), inside [`LIMIT`].
const JOINER_TIMEOUT: &str = "40";
/// How often logs and states are looked at.
const POLL: Duration = Duration::from_millis(200);
/// Lines of each container's log a failure prints.
const TAIL: usize = 40;
/// How long a run waits for another one (the machine-wide lock) at most.
const LOCK_WAIT: Duration = Duration::from_secs(600);
/// The WAN's Nostr relay (in the infrastructure container, with the STUN server).
pub const RELAY_URL: &str = "ws://203.0.113.10:7777";
/// Our STUN servers: the infrastructure's and the TURN relay's binding answers.
pub const STUN: [&str; 2] = ["203.0.113.10:3479", "203.0.113.20:3478"];
/// The listed game's name (`finding`).
const LISTED_NAME: &str = "nat finding";
/// The WAN's subnet: the routers' outsides and the infrastructure.
pub const WAN_SUBNET: &str = "203.0.113.0/24";

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
    /// Cone, plus miniupnpd with PCP requests dropped: NAT-PMP only.
    E,
}

impl Lan {
    pub const ALL: [Lan; 5] = [Lan::A, Lan::B, Lan::C, Lan::D, Lan::E];

    /// 1 to 5: the subnet's second byte and the router's WAN address' 10x.
    fn n(self) -> u8 {
        self as u8 + 1
    }

    fn letter(self) -> &'static str {
        ["a", "b", "c", "d", "e"][self as usize]
    }

    pub fn subnet(self) -> String {
        format!("10.{}.0.0/24", self.n())
    }

    /// The router's LAN address (`.2`), every node's gateway.
    pub fn router(self) -> String {
        self.addr(2)
    }

    /// The router's WAN address, 203.0.113.101 to .105.
    pub fn router_wan(self) -> String {
        format!("203.0.113.{}", 100 + self.n())
    }

    /// `router.sh`'s kind.
    pub fn kind(self) -> &'static str {
        ["cone", "symmetric", "upnp", "pcp", "pmp"][self as usize]
    }

    fn addr(self, host: u8) -> String {
        format!("10.{}.0.{host}", self.n())
    }
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
    /// The TURN relay logged at least one allocation (`relay: <n>
    /// allocations`, n ≥ 1: it logs the count when it changes, once a
    /// second, so two allocations may show as 2 alone).
    pub relay_allocates: bool,
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
    relay_allocates: false,
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
pub const SCENARIOS: [Scenario; 9] = [
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
            host_log: &[&["reach: Port 23400 is open on your router (PCP)"], &["via open port"]],
            code: Some(Code::Address),
            ..EXPECT
        },
    },
    Scenario {
        name: "nat-pmp",
        host: Host::Server(Lan::E, Config::Mapped),
        joiner: Some(Joiner { lan: Lan::A, way: Way::Invite, flags: &["--lobby-only"] }),
        expect: Expect {
            stdout: &["path: direct"],
            host_log: &[&["reach: Port 23400 is open on your router (NAT-PMP)"], &["via open port"]],
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
            relay_allocates: true,
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
        format!(
            "/etc/os-release names no VERSION_ID ({id} without a release): no image of the same release to \
             build on"
        )
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

/// Why a failed image build is a skip (N58: a host whose image cannot be
/// built skips), from `docker build --progress=plain`'s output: apt could
/// not reach the mirrors (offline), or the release lacks a package. Any
/// other failure (a broken Dockerfile or script) is `None`: a failure.
pub fn build_skip(base: &str, log: &str) -> Option<String> {
    let offline =
        ["Temporary failure resolving", "Failed to fetch", "Could not resolve", "Network is unreachable"];
    if offline.iter().any(|w| log.contains(w)) {
        return Some(format!(
            "apt cannot reach {base}'s mirrors to build the image (offline?): it is built once, with network \
             access, then cached"
        ));
    }
    let missing = log.lines().find_map(|l| {
        let rest = l.split_once("E: Unable to locate package ").map(|(_, p)| p).or_else(|| {
            let (before, _) = l.split_once(" has no installation candidate")?;
            before.rsplit_once("Package ").map(|(_, p)| p)
        })?;
        Some(rest.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
    })?;
    Some(format!(
        "{base} has no package {missing} (the image needs iproute2, nftables and miniupnpd-nftables from apt)"
    ))
}

/// Why a failed network creation is a skip: this Docker cannot make
/// macvlan networks (rootless Docker, a kernel without the module, some
/// Docker Desktop setups). Any other failure is `None`.
pub fn network_skip(err: &str) -> Option<String> {
    let lower = err.to_lowercase();
    (lower.contains("macvlan") || lower.contains("not supported") || lower.contains("plugin not found")).then(|| {
        format!(
            "Docker cannot create macvlan networks ({}): the tests need rootful Docker with the macvlan module \
             (`sudo modprobe macvlan`)",
            err.lines().last().unwrap_or(err).trim()
        )
    })
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

/// The most allocations the TURN relay's log shows (`relay: <n> allocations`).
pub fn allocations(log: &str) -> u32 {
    log.lines()
        .filter_map(|l| l.split_once("relay: ")?.1.strip_suffix(" allocations")?.trim().parse().ok())
        .max()
        .unwrap_or(0)
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
    if expect.relay_allocates && allocations(&seen.relay_log) == 0 {
        wrong.push("the relay's log shows no allocation (\"relay: <n> allocations\")".into());
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
    /// Its role (`infra`, `relay`, `router-a`, `host`, `joiner`, ...):
    /// the container is `rl-nat-<run>-<role>`.
    pub role: String,
    /// Units start in stages; a stage starts once the last is ready.
    pub stage: u8,
    /// `docker run` arguments after `-d` (the image, then the command; a
    /// `{invite}` argument is replaced by the host's code).
    pub args: Vec<String>,
    pub ready: Ready,
}

/// One run: its id (in every container's and network's name and label,
/// so two runs never share a name), the containers' lifetime, and where
/// things are on this machine.
pub struct Run {
    pub id: String,
    pub life: u32,
    pub bin: PathBuf,
    pub content: PathBuf,
    pub configs: PathBuf,
    pub image: String,
}

impl Run {
    pub fn container(&self, role: &str) -> String {
        format!("{LABEL}-{}-{role}", self.id)
    }

    /// The WAN's network (`wan`) or a LAN's (its letter).
    pub fn network(&self, net: &str) -> String {
        format!("{LABEL}-{}-{net}", self.id)
    }

    pub fn lan(&self, lan: Lan) -> String {
        self.network(lan.letter())
    }

    /// Every network: name and subnet.
    pub fn networks(&self) -> Vec<(String, String)> {
        let mut all = vec![(self.network("wan"), WAN_SUBNET.to_string())];
        all.extend(Lan::ALL.iter().map(|l| (self.lan(*l), l.subnet())));
        all
    }

    /// `docker network create`'s arguments for one network: macvlan,
    /// `--internal` and no `parent`, so Docker gives it a dummy parent of
    /// its own (nothing on it reaches the host or beyond), labelled with
    /// the run. Not a bridge: with `br_netfilter`, Docker's rules for an
    /// internal bridge drop every bridged packet addressed outside its
    /// subnet, which is all of a router's traffic.
    pub fn network_args(&self, name: &str, subnet: &str) -> Vec<String> {
        let label = format!("{LABEL}={}", self.id);
        ["network", "create", "-d", "macvlan", "--internal", "--label", &label, "--subnet", subnet, name]
            .map(String::from)
            .to_vec()
    }
}

/// The containers of `sc`, in stages: the WAN's infrastructure (Nostr
/// relay and STUN server); the TURN relay, the routers of the LANs used
/// and the public server; the host; the joiner. Every one joins only our
/// networks (never Docker's default bridge), carries the run's labels and
/// ends by itself within `run.life` seconds (`RL_NAT_LIFE` for the
/// scripts, `timeout` around our binaries).
pub fn units(sc: &Scenario, run: &Run) -> Vec<Unit> {
    let strings = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<String>>();
    let life = run.life.to_string();
    let mounts = {
        let bin = |name: &str| format!("{}:/rl/bin/{name}:ro", run.bin.join(name).display());
        vec![
            "-v".to_string(),
            bin("rl-server"),
            "-v".into(),
            bin("rl-cli"),
            "-v".into(),
            format!("{}:/rl/content:ro", run.content.display()),
            "-v".into(),
            format!("{}:/rl/config:ro", run.configs.display()),
        ]
    };
    let unit =
        |role: &str, stage: u8, nets: &[(String, String)], extra: &[String], cmd: Vec<String>, ready| {
            let mut args = strings(&["--name", &run.container(role), "--label"]);
            args.push(format!("{LABEL}={}", run.id));
            args.push("--label".into());
            args.push(format!("{LABEL}.scenario={}", sc.name));
            args.extend(strings(&["--init", "-e", &format!("RL_NAT_LIFE={life}")]));
            for (net, ip) in nets {
                args.push("--network".into());
                args.push(format!("name={net},ip={ip}"));
            }
            args.extend(extra.iter().cloned());
            args.push(run.image.clone());
            args.extend(cmd);
            Unit { role: role.into(), stage, args, ready }
        };
    // Our binaries on the WAN, bounded: SIGTERM at the end of their life,
    // SIGKILL 5 s later (in a LAN, `node.sh` does the same); the exit
    // status stays the binary's (`--preserve-status`: a host stopped
    // with SIGTERM still exits 0, not timeout's 124).
    let bounded = |cmd: Vec<String>| {
        let mut all = strings(&["timeout", "--preserve-status", "-k", "5", &life]);
        all.extend(cmd);
        all
    };
    let wan = |ip: &str| vec![(run.network("wan"), ip.to_string())];
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
    let mut node_opts = mounts.clone();
    node_opts.extend(strings(&["--cap-add", "NET_ADMIN"]));

    let mut out = vec![unit(
        "infra",
        0,
        &wan("203.0.113.10"),
        &mounts,
        strings(&[
            "sh",
            "-c",
            "timeout --preserve-status -k 5 \"$RL_NAT_LIFE\" /rl/bin/rl-cli nostr-relay --listen 0.0.0.0:7777 & \
             exec timeout --preserve-status -k 5 \"$RL_NAT_LIFE\" /rl/bin/rl-cli stun-server --listen 0.0.0.0:3479",
        ]),
        Ready::Lines(vec!["nostr relay:", "stun server:"]),
    )];
    let mut relay_cmd = server(Config::Relay);
    relay_cmd.push("--relay".into());
    out.push(unit(
        "relay",
        1,
        &wan("203.0.113.20"),
        &mounts,
        bounded(relay_cmd),
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
            &[(run.network("wan"), lan.router_wan()), (run.lan(lan), lan.router())],
            &strings(&["--cap-add", "NET_ADMIN", "--sysctl", "net.ipv4.ip_forward=1"]),
            strings(&["/rl-nat/router.sh", lan.kind(), &lan.router_wan(), &lan.router()]),
            Ready::Lines(vec!["router ready"]),
        ));
    }
    match sc.host {
        Host::Public => out.push(unit(
            "public",
            1,
            &wan("203.0.113.30"),
            &mounts,
            bounded(server(Config::Public)),
            Ready::Invite(None),
        )),
        Host::Server(lan, config) => out.push(unit(
            "host",
            2,
            &[(run.lan(lan), lan.addr(10))],
            &node_opts,
            on_lan(lan, server(config)),
            Ready::Invite(sc.expect.code.filter(|c| *c == Code::Address)),
        )),
        Host::NatCheck(lan) => out.push(unit(
            "host",
            2,
            &[(run.lan(lan), lan.addr(10))],
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
            &[(run.lan(j.lan), j.lan.addr(20))],
            &node_opts,
            on_lan(j.lan, cmd),
            Ready::Exit,
        ));
    }
    out
}

/// Set by Ctrl+C (SIGINT, SIGTERM, SIGHUP) while a run is active: its
/// waits stop, its guards remove the containers and networks, then the
/// process exits 130.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// A run is active (outside one, or at a second Ctrl+C, the signal exits at once).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Installs the handler once per process (`ctrlc`, as `rl-server`).
fn watch_interrupts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let handler = || {
            if ACTIVE.load(Ordering::SeqCst) && !INTERRUPTED.swap(true, Ordering::SeqCst) {
                eprintln!(
                    "net-nat: interrupted: removing its containers and networks (again to exit at once)"
                );
            } else {
                std::process::exit(130);
            }
        };
        if let Err(e) = ctrlc::set_handler(handler) {
            eprintln!(
                "warning: no Ctrl+C handler ({e}): an interrupted run's containers end within {LIFE} s"
            );
        }
    });
}

fn interrupted() -> Result<(), String> {
    if INTERRUPTED.load(Ordering::SeqCst) { Err("interrupted".into()) } else { Ok(()) }
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
fn logs(container: &str) -> (String, String) {
    match Command::new("docker").args(["logs", container]).output() {
        Ok(o) => {
            (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
        }
        Err(_) => Default::default(),
    }
}

/// A container's exit status, once it exited.
fn exited(container: &str) -> Option<i64> {
    let state = docker(&["inspect", "-f", "{{.State.Running}} {{.State.ExitCode}}", container]).ok()?;
    let (running, code) = state.trim().split_once(' ')?;
    (running == "false").then(|| code.parse().ok()).flatten()
}

/// Removes every container whose label `filter` matches (`rl-nat`: any
/// run's; `rl-nat=<id>`: one run's).
fn remove_containers(filter: &str) {
    if let Ok(ids) = docker(&["ps", "-aq", "--filter", &format!("label={filter}")]) {
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

/// Removes the containers, then the networks, whose label `filter` matches.
fn remove_networks(filter: &str) {
    remove_containers(filter);
    if let Ok(ids) = docker(&["network", "ls", "-q", "--filter", &format!("label={filter}")]) {
        for id in ids.split_whitespace() {
            if let Err(e) = docker(&["network", "rm", id]) {
                eprintln!("warning: {e}");
            }
        }
    }
}

/// The machine-wide lock (`<temp>/rl-nat.lock`): one run at a time, as
/// the runs share subnets (Docker refuses overlapping ones) and the
/// leftover cleanup. Waits for another run at most [`LOCK_WAIT`].
fn lock() -> Result<File, String> {
    let path = std::env::temp_dir().join("rl-nat.lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let until = Instant::now() + LOCK_WAIT;
    let mut said = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < until => {
                if !said {
                    println!("     net-nat: waiting for another net-nat run ({})", path.display());
                    said = true;
                }
                interrupted()?;
                std::thread::sleep(Duration::from_secs(1));
            }
            Err(TryLockError::WouldBlock) => {
                return Err(format!(
                    "another net-nat run held {} for {} s",
                    path.display(),
                    LOCK_WAIT.as_secs()
                ));
            }
            Err(TryLockError::Error(e)) => return Err(format!("locking {}: {e}", path.display())),
        }
    }
}

/// The networks of one run, removed (with any container left) when
/// dropped unless kept.
struct Networks<'a> {
    run: &'a Run,
    keep: bool,
}

impl<'a> Networks<'a> {
    /// Every network of `run`. `Ok(Err(why))`: this Docker cannot make
    /// macvlan networks (a skip).
    fn create(run: &'a Run, keep: bool) -> Result<Result<Networks<'a>, String>, String> {
        let made = Networks { run, keep };
        for (name, subnet) in run.networks() {
            let args = run.network_args(&name, &subnet);
            if let Err(e) = docker(&args.iter().map(String::as_str).collect::<Vec<_>>()) {
                return match network_skip(&e) {
                    Some(why) => Ok(Err(why)),
                    None => Err(e),
                };
            }
        }
        Ok(Ok(made))
    }
}

impl Drop for Networks<'_> {
    fn drop(&mut self) {
        if !self.keep {
            remove_networks(&format!("{LABEL}={}", self.run.id));
        }
    }
}

/// A scenario's containers: their logs saved to its directory and the
/// containers removed (unless kept) when dropped, so a failing,
/// interrupted or panicking scenario leaves nothing running.
struct Containers<'a> {
    run: &'a Run,
    dir: &'a Path,
    roles: Vec<String>,
    keep: bool,
}

impl Containers<'_> {
    fn save_logs(&self) {
        for role in &self.roles {
            let (out, err) = logs(&self.run.container(role));
            let _ = std::fs::write(self.dir.join(format!("{role}.out")), out);
            let _ = std::fs::write(self.dir.join(format!("{role}.err")), err);
        }
    }

    /// The last [`TAIL`] lines of each container's output, for a failure.
    fn tails(&self) -> String {
        let mut text = String::new();
        for role in &self.roles {
            let (out, err) = logs(&self.run.container(role));
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
            remove_containers(&format!("{LABEL}={}", self.run.id));
        }
    }
}

/// Plays one scenario: its units stage by stage, each waited for until
/// [`LIMIT`] from the start, then the host stopped when asked, and the
/// verdict. `Err`: why it failed and the containers' log tails.
fn play(sc: &Scenario, run: &Run, dir: &Path, keep: bool) -> Result<(), String> {
    remove_containers(&format!("{LABEL}={}", run.id));
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let deadline = Instant::now() + LIMIT;
    let mut started = Containers { run, dir, roles: Vec::new(), keep };
    let result = run_units(sc, run, &units(sc, run), deadline, &mut started).and_then(|()| {
        let mut seen = Seen::default();
        let main = run.container(if sc.joiner.is_some() { "joiner" } else { "host" });
        seen.exit = exited(&main);
        (seen.stdout, seen.stderr) = logs(&main);
        if sc.expect.host_stops {
            let host = run.container("host");
            let _ = docker(&["stop", "-t", "5", &host]);
            seen.host_exit = exited(&host);
        }
        seen.host_log = logs(&run.container(if sc.host == Host::Public { "public" } else { "host" })).0;
        seen.relay_log = logs(&run.container("relay")).0;
        let wrong = verdict(&sc.expect, &seen);
        if wrong.is_empty() { Ok(()) } else { Err(wrong.join("; ")) }
    });
    result.map_err(|why| format!("{why}\n{}", started.tails()))
}

/// Starts `units` stage by stage, waiting for each to be ready.
fn run_units(
    sc: &Scenario,
    run: &Run,
    units: &[Unit],
    deadline: Instant,
    started: &mut Containers,
) -> Result<(), String> {
    let mut invite: Option<String> = None;
    for stage in 0..=units.iter().map(|u| u.stage).max().unwrap_or(0) {
        let now: Vec<&Unit> = units.iter().filter(|u| u.stage == stage).collect();
        for u in &now {
            interrupted()?;
            if u.args.iter().any(|a| a == "{invite}") && invite.is_none() {
                return Err(format!("{}: no invite code to join with", sc.name));
            }
            let mut args: Vec<&str> = vec!["run", "-d"];
            args.extend(u.args.iter().map(|a| match (a.as_str(), &invite) {
                ("{invite}", Some(code)) => code.as_str(),
                _ => a.as_str(),
            }));
            started.roles.push(u.role.clone());
            docker(&args)?;
        }
        for u in &now {
            let found = wait_ready(u, &run.container(&u.role), deadline)?;
            if found.is_some() {
                invite = found;
            }
        }
    }
    Ok(())
}

/// Waits until `u` (the container `name`) is ready; an invite code when
/// it was waited for.
fn wait_ready(u: &Unit, name: &str, deadline: Instant) -> Result<Option<String>, String> {
    loop {
        interrupted()?;
        let (out, _) = logs(name);
        match &u.ready {
            Ready::Lines(lines) if lines.iter().all(|l| out.contains(l)) => return Ok(None),
            Ready::Invite(code) => {
                if let Some((c, with)) = invite_code(&out)
                    && (with || *code != Some(Code::Address))
                {
                    return Ok(Some(c));
                }
            }
            Ready::Exit if exited(name).is_some() => return Ok(None),
            _ => {}
        }
        if !matches!(u.ready, Ready::Exit)
            && let Some(code) = exited(name)
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

/// The image for `base`, built when its tag is missing (the only network
/// use: the base and the packages), then the superseded `rl-nat` images
/// pruned. `Ok(Err(why))`: a skip (N58): offline without the image, a
/// base that cannot be pulled, apt offline or a package missing.
fn ensure_image(root: &Path, base: &str, offline: bool) -> Result<Result<String, String>, String> {
    let mut files = Vec::new();
    for f in TOOL_FILES {
        let path = root.join(TOOL_DIR).join(f);
        files.push((f, std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?));
    }
    let tag = image_tag(&files.iter().map(|(n, d)| (*n, d.as_slice())).collect::<Vec<_>>(), base);
    if docker(&["image", "inspect", &tag]).is_err() {
        if offline {
            return Ok(Err(format!(
                "--offline, and the image {tag} is not built yet (it needs the network once)"
            )));
        }
        if docker(&["image", "inspect", base]).is_err()
            && let Err(e) = docker(&["pull", base])
        {
            return Ok(Err(format!("the base image {base} cannot be pulled ({e})")));
        }
        println!("     net-nat: building {tag} FROM {base}");
        let built = Instant::now();
        let context = root.join(TOOL_DIR);
        let out = Command::new("docker")
            .args(["build", "--progress=plain", "-t", &tag, "--build-arg", &format!("BASE={base}")])
            .arg(&context)
            .output()
            .map_err(|e| format!("running docker: {e}"))?;
        if !out.status.success() {
            let log = String::from_utf8_lossy(&out.stderr);
            return match build_skip(base, &log) {
                Some(why) => Ok(Err(why)),
                None => Err(format!("building {tag}:\n{}", tail(&log, TAIL))),
            };
        }
        println!("     net-nat: built {tag} in {:.1} s", built.elapsed().as_secs_f32());
    }
    oracle::prune_old(&tag, false);
    Ok(Ok(tag))
}

/// Runs the scenarios (`only`, else all): prerequisites, the binaries,
/// the image, the lock, configs and networks, then each scenario.
/// `verbose` prints a line per scenario. `Err`: what failed, every
/// failing scenario's report. Interrupted, it removes what it made and
/// exits 130.
pub fn run(only: &[String], keep: bool, offline: bool, verbose: bool) -> Result<Outcome, String> {
    let scenarios = select(only)?;
    let root = root();
    let base = match prerequisites(&root) {
        Ok(base) => base,
        Err(why) => return Ok(Outcome::Skipped(why)),
    };
    watch_interrupts();
    ACTIVE.store(true, Ordering::SeqCst);
    let result = run_locked(&root, &base, &scenarios, keep, offline, verbose);
    ACTIVE.store(false, Ordering::SeqCst);
    if INTERRUPTED.load(Ordering::SeqCst) {
        eprintln!("net-nat: interrupted; its containers and networks are removed");
        std::process::exit(130);
    }
    result
}

fn run_locked(
    root: &Path,
    base: &str,
    scenarios: &[&Scenario],
    keep: bool,
    offline: bool,
    verbose: bool,
) -> Result<Outcome, String> {
    e2e::build_binaries(root)?;
    interrupted()?;
    let image = match ensure_image(root, base, offline)? {
        Ok(tag) => tag,
        Err(why) => return Ok(Outcome::Skipped(why)),
    };
    let _lock = lock()?;
    // Anything labelled `rl-nat` now is an interrupted run's (the lock is ours).
    remove_networks(LABEL);
    let out = root.join(OUT);
    let configs = out.join("configs");
    std::fs::create_dir_all(&configs).map_err(|e| format!("{}: {e}", configs.display()))?;
    for c in Config::ALL {
        std::fs::write(configs.join(c.file()), c.text()).map_err(|e| format!("{}: {e}", c.file()))?;
    }
    let run = Run {
        id: std::process::id().to_string(),
        life: if keep { KEEP_LIFE } else { LIFE },
        bin: root.join(BIN),
        content: root.join(CONTENT),
        configs,
        image,
    };
    let _networks = match Networks::create(&run, keep)? {
        Ok(n) => n,
        Err(why) => return Ok(Outcome::Skipped(why)),
    };
    let mut failed = Vec::new();
    for sc in scenarios {
        let t = Instant::now();
        let result = play(sc, &run, &out.join(sc.name), keep);
        let secs = t.elapsed().as_secs_f32();
        interrupted()?;
        match result {
            Ok(()) if verbose => println!("     net-nat: {} passed in {secs:.1} s", sc.name),
            Ok(()) => {}
            Err(why) => failed.push(format!("scenario {} failed after {secs:.1} s: {why}", sc.name)),
        }
    }
    if failed.is_empty() { Ok(Outcome::Passed(scenarios.len())) } else { Err(failed.join("\n")) }
}

/// `cargo xtask net-nat [--keep] [--offline] [--only <scenario>]...`.
pub fn command(only: &[String], keep: bool, offline: bool) -> anyhow::Result<bool> {
    select(only).map_err(anyhow::Error::msg)?;
    let t = Instant::now();
    match run(only, keep, offline, true) {
        Ok(Outcome::Skipped(why)) => {
            println!("skip net-nat ({why})");
            Ok(true)
        }
        Ok(Outcome::Passed(n)) => {
            println!("ok   net-nat ({}, {:.1}s)", scenarios(n), t.elapsed().as_secs_f32());
            Ok(true)
        }
        Err(why) => {
            println!("FAIL net-nat ({:.1}s)\n{why}", t.elapsed().as_secs_f32());
            Ok(false)
        }
    }
}

/// `full`'s step: every scenario; `Ok` with the `ok` line's detail
/// (`9 scenarios, `), after a `skip` line when a prerequisite is missing.
pub fn step(offline: bool) -> Result<String, String> {
    match run(&[], false, offline, false)? {
        Outcome::Skipped(why) => {
            println!("skip net-nat ({why})");
            Ok(String::new())
        }
        Outcome::Passed(n) => Ok(format!("{}, ", scenarios(n))),
    }
}

/// `1 scenario`, `9 scenarios`.
fn scenarios(n: usize) -> String {
    format!("{n} scenario{}", if n == 1 { "" } else { "s" })
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

    fn scenario(name: &str) -> &'static Scenario {
        SCENARIOS.iter().find(|s| s.name == name).unwrap()
    }

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
            [
                "direct",
                "upnp",
                "pcp",
                "nat-pmp",
                "hole-punching",
                "relay",
                "no-relay",
                "finding",
                "nat-check"
            ]
        );
        assert_eq!(select(&[]).unwrap().len(), 9);
        let two = select(&["relay".into(), "direct".into()]).unwrap();
        assert_eq!(two.iter().map(|s| s.name).collect::<Vec<_>>(), ["direct", "relay"], "in table order");
        let why = select(&["browser".into()]).unwrap_err();
        assert!(why.contains("browser") && why.contains("hole-punching"), "{why}");
    }

    fn run(life: u32) -> Run {
        Run {
            id: "4242".into(),
            life,
            bin: "/repo/target/debug".into(),
            content: "/repo/content/packs".into(),
            configs: "/repo/target/net-nat/configs".into(),
            image: "rl-nat:0123456789ab".into(),
        }
    }

    /// Nothing leaves: every container joins only the run's networks, at
    /// an address of their subnets, never Docker's default bridge or the
    /// host's network, carries the labels its cleanup goes by, and ends by
    /// itself within its lifetime.
    #[test]
    fn every_container_stays_on_our_networks_and_ends_by_itself() {
        for life in [LIFE, KEEP_LIFE] {
            let run = run(life);
            let nets = run.networks();
            for sc in &SCENARIOS {
                let units = units(sc, &run);
                assert!(units.iter().any(|u| u.role == "infra") && units.iter().any(|u| u.role == "relay"));
                for u in &units {
                    let what = format!("{} {}", sc.name, u.role);
                    // Docker's options: everything before the image.
                    let image = u.args.iter().position(|a| a == &run.image).unwrap();
                    let at = |flag: &str| {
                        u.args[..image]
                            .windows(2)
                            .filter(|w| w[0] == flag)
                            .map(|w| w[1].clone())
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(at("--name"), [format!("rl-nat-4242-{}", u.role)], "{what}");
                    assert_eq!(
                        at("--label"),
                        ["rl-nat=4242".to_string(), format!("rl-nat.scenario={}", sc.name)]
                    );
                    assert_eq!(at("-e"), [format!("RL_NAT_LIFE={life}")], "{what}");
                    let attached = at("--network");
                    assert!(!attached.is_empty(), "{what}");
                    for a in attached {
                        let (name, ip) = a.strip_prefix("name=").unwrap().split_once(",ip=").unwrap();
                        let (_, subnet) = nets.iter().find(|(n, _)| n == name).expect("one of the run's");
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
                    // The command after the image is bounded: node.sh and
                    // router.sh by RL_NAT_LIFE, the rest by `timeout`.
                    let cmd = &u.args[image + 1..];
                    let life = life.to_string();
                    match cmd[0].as_str() {
                        "/rl-nat/node.sh" | "/rl-nat/router.sh" => {}
                        "timeout" => {
                            assert_eq!(cmd[1..5], ["--preserve-status", "-k", "5", life.as_str()], "{what}")
                        }
                        "sh" => {
                            assert_eq!(cmd[1], "-c");
                            for program in cmd[2].split('&') {
                                let p = program.trim().trim_start_matches("exec ");
                                assert!(
                                    p.starts_with("timeout --preserve-status -k 5 \"$RL_NAT_LIFE\" "),
                                    "{what}: {p}"
                                );
                            }
                        }
                        other => panic!("{what}: an unbounded command {other}"),
                    }
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
                let routers: Vec<&str> =
                    units.iter().filter(|u| u.role.starts_with("router-")).map(|u| u.role.as_str()).collect();
                let mut deduped = routers.clone();
                deduped.dedup();
                assert_eq!(routers, deduped, "each router once");
            }
        }
        let upnp = units(scenario("upnp"), &run(LIFE));
        let host = upnp.iter().find(|u| u.role == "host").unwrap();
        assert_eq!(host.ready, Ready::Invite(Some(Code::Address)), "the code with the mapped address");
        assert!(host.args.windows(2).any(|w| w == ["/rl-nat/node.sh", "10.3.0.2"]));
        let pmp = units(scenario("nat-pmp"), &run(LIFE));
        let router = pmp.iter().find(|u| u.role == "router-e").unwrap();
        assert!(router.args.windows(2).any(|w| w == ["/rl-nat/router.sh", "pmp"]));
    }

    /// The isolation rests on these: macvlan, `--internal`, no `parent`
    /// option (Docker then makes a dummy parent), our subnets only, named
    /// and labelled by the run.
    #[test]
    fn the_networks_are_internal_macvlan_without_a_parent() {
        let run = run(LIFE);
        let nets = run.networks();
        let subnets: Vec<&str> = nets.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(
            subnets,
            ["203.0.113.0/24", "10.1.0.0/24", "10.2.0.0/24", "10.3.0.0/24", "10.4.0.0/24", "10.5.0.0/24"]
        );
        for (name, subnet) in &nets {
            assert!(name.starts_with("rl-nat-4242-"), "{name}");
            let args = run.network_args(name, subnet);
            assert_eq!(args[..5], ["network", "create", "-d", "macvlan", "--internal"]);
            assert!(args.windows(2).any(|w| w == ["--label", "rl-nat=4242"]));
            assert!(args.windows(2).any(|w| w[0] == "--subnet" && w[1] == *subnet));
            assert_eq!(args.last(), Some(name));
            assert!(
                !args.iter().any(|a| a == "-o" || a.starts_with("--opt") || a.contains("parent")),
                "{args:?}"
            );
        }
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
        let relay = &scenario("relay").expect;
        let mut seen = Seen {
            exit: Some(0),
            stdout: "path: relayed\nmatch 1: olx-classic\n".into(),
            host_log: "x invite code (joins through the relays): rlx1a\njoin: b via relayed\nmatch end (time limit)\n"
                .into(),
            relay_log: "t relay: TURN on port 3478\nt relay: 2 allocations\nt relay: 0 allocations\n".into(),
            host_exit: Some(0),
            ..Seen::default()
        };
        assert!(
            verdict(relay, &seen).is_empty(),
            "two allocations at once count: {:?}",
            verdict(relay, &seen)
        );
        seen.exit = None;
        seen.host_exit = Some(137);
        seen.relay_log = "t relay: 0 allocations\n".into();
        let wrong = verdict(relay, &seen).join("; ");
        assert!(
            wrong.contains("did not exit") && wrong.contains("137") && wrong.contains("no allocation"),
            "{wrong}"
        );
        let pcp = &scenario("pcp").expect;
        let mapped = Seen {
            exit: Some(0),
            stdout: "path: direct\n".into(),
            host_log:
                "reach: Port 23400 is open on your router (PCP): x\ninvite code: rlx1b\nvia open port\n"
                    .into(),
            ..Seen::default()
        };
        assert!(verdict(pcp, &mapped).is_empty());
        let pmp = &scenario("nat-pmp").expect;
        let wrong = verdict(pmp, &mapped).join("; ");
        assert!(wrong.contains("(NAT-PMP)"), "PCP is not NAT-PMP: {wrong}");
        let unmapped = Seen { host_log: "invite code (joins through the relays): rlx1c\n".into(), ..mapped };
        let wrong = verdict(pcp, &unmapped).join("; ");
        assert!(wrong.contains("(PCP)") && wrong.contains("lacks an address"), "{wrong}");
    }

    #[test]
    fn allocations_are_read_from_the_relay_log() {
        assert_eq!(allocations(""), 0);
        assert_eq!(
            allocations(
                "2026-10-14 15:37:38 relay: 1 allocations\n2026-10-14 15:37:54 relay: 0 allocations\n"
            ),
            1
        );
        assert_eq!(allocations("relay: 3 allocations\nrelay: 2 allocations\n"), 3);
        assert_eq!(
            allocations("relay: TURN on port 3478, relay ports 49160-49191\nrelay: x allocations\n"),
            0
        );
    }

    /// N58: a host whose image cannot be built skips, when apt is offline
    /// or the release lacks a package (the lines measured here with
    /// `docker build --progress=plain`); any other failure fails.
    #[test]
    fn image_build_failures_that_skip() {
        let offline = "#6 7.350 W: Failed to fetch http://archive.ubuntu.com/ubuntu/dists/resolute/InRelease  \
                       Temporary failure resolving 'archive.ubuntu.com'\n#6 7.364 E: Unable to locate package iproute2\n";
        let why = build_skip("ubuntu:26.04", offline).unwrap();
        assert!(
            why.contains("mirrors") && why.contains("ubuntu:26.04"),
            "offline before a missing package: {why}"
        );
        let missing = "#6 6.763 E: Unable to locate package miniupnpd-nftables\n";
        let why = build_skip("ubuntu:20.04", missing).unwrap();
        assert!(why.contains("ubuntu:20.04 has no package miniupnpd-nftables"), "{why}");
        let candidate = "#6 E: Package 'miniupnpd-nftables' has no installation candidate\n";
        assert!(build_skip("debian:11", candidate).unwrap().contains("no package miniupnpd-nftables"));
        let broken = "#8 ERROR: failed to compute cache key: \"/router.sh\": not found\n";
        assert_eq!(build_skip("ubuntu:26.04", broken), None, "a broken Dockerfile fails");
    }

    #[test]
    fn network_failures_that_skip() {
        let why = network_skip(
            "`docker network create` failed: Error response from daemon: plugin \"macvlan\" not found",
        )
        .unwrap();
        assert!(why.contains("macvlan") && why.contains("modprobe"), "{why}");
        assert!(network_skip("failed to create the macvlan port: operation not supported").is_some());
        assert_eq!(
            network_skip("Error response from daemon: Pool overlaps with other one on this address space"),
            None
        );
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
