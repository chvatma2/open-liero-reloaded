//! The server's config file (networking spec §7.4), RON, every field
//! documented in `fixtures/server/example.ron`.

use std::net::IpAddr;
#[cfg(feature = "test-hooks")]
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use anyhow::{Context as _, bail};
use rl_content::Content;
use rl_discovery::Region;
use rl_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
use rl_discovery::hosting::local_relays;
use rl_discovery::list::is_global;
use rl_discovery::turn_note::MAX_CAPACITY;
use rl_lobby::HostConfig;
use rl_modes::DeathmatchSettings;
use rl_net::endpoint::DEFAULT_PORT;
use rl_proto::{MAX_BOTS, MAX_PLAYERS, MAX_SPECTATORS, PublicKey};
use serde::Deserialize;

use crate::files::parse_key;
use crate::rotation::Rotation;

/// Who can find the server (decision N43). Every server prints its invite
/// code and is joinable by address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Visibility {
    /// Announced on Nostr (every minute, on the configured relays) and on
    /// the LAN.
    Public,
    /// Announced on the LAN only (UDP broadcast on `LAN_PORT` every 2 s).
    Lan,
    /// Not announced: joined by its invite code or address. Plan 10's
    /// `DirectOnly` reads as this.
    #[serde(alias = "DirectOnly")]
    Invite,
}

/// One map of the rotation: a mod, a level and the match's settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The mod's pack id, such as `olx-classic`.
    #[serde(rename = "mod")]
    pub mod_id: String,
    /// The level's pack id, such as `olx-level-dirt-level`.
    pub level: String,
    /// Minutes; 0 or less: none.
    #[serde(default = "default_time_limit")]
    pub time_limit: f32,
    /// Kills that end the match; 0 or less: none.
    #[serde(default = "default_kill_limit")]
    pub kill_limit: i32,
    /// Below 0 unlimited; `N` lets a worm die `N + 1` times.
    #[serde(default = "default_lives")]
    pub lives: i32,
    #[serde(default = "yes")]
    pub bonuses: bool,
}

fn default_time_limit() -> f32 {
    DeathmatchSettings::default().time_limit
}

fn default_kill_limit() -> i32 {
    DeathmatchSettings::default().kill_limit
}

fn default_lives() -> i32 {
    DeathmatchSettings::default().lives
}

fn yes() -> bool {
    true
}

/// The server's settings.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// The lobby's name (sanitized like a player's).
    pub name: String,
    pub visibility: Visibility,
    /// Joiners must know it.
    pub password: Option<String>,
    /// TCP and UDP (spec §5.1); 0 picks a free one (tests).
    pub port: u16,
    /// Player seats, 1 to 32.
    pub max_players: u8,
    /// Spectators, at most 16.
    pub max_spectators: u8,
    /// Public keys (64 hex characters) whose chat commands the server
    /// takes; the log names every joiner's key.
    pub admins: Vec<String>,
    /// Bots are added while fewer players than this are seated.
    pub fill_bots: u8,
    /// The maps in turn; at least one.
    pub rotation: Vec<Entry>,
    /// Start as soon as every player is ready and at least `min_players`
    /// are seated; either way a match starts (forced: players not ready
    /// watch it) [`crate::AUTO_START`] after the lobby gathered, with at
    /// least `min_players`.
    pub start_when_ready: bool,
    pub min_players: u8,
    /// Bans kept across restarts (keys and addresses), relative to the
    /// data directory; `None`: bans last until the server stops.
    pub bans_file: Option<PathBuf>,
    /// KiB per second the server sends content in all (packs and skins to
    /// members who lack them; plan 11), held to [`UPLOAD_KIB_S`] by
    /// [`Config::upload_rate`]; during a match `rl_online`'s `MATCH_RATE`.
    pub upload_kib_s: u32,
    /// Nostr relays (`wss://` or `ws://`) a public server announces on;
    /// the invite names them. Default `DEFAULT_RELAYS`.
    pub relays: Vec<String>,
    /// The region the game list shows.
    pub region: Region,
    /// The server's public IP address, when STUN cannot learn it (or
    /// should not be asked).
    pub public_address: Option<String>,
    /// STUN servers (`host:port`) the game socket asks for its
    /// server-reflexive address (an ICE candidate, never advertised: N47)
    /// and the router's NAT behaviour, at the start and every 25 s.
    /// Default `DEFAULT_STUN`; empty: none asked.
    pub stun: Vec<String>,
    /// Ask the router to open the port (UPnP-IGD, NAT-PMP, PCP; N51), TCP
    /// and UDP, for Public and Invite servers; with a relay, its ports
    /// too. Default true.
    pub port_mapping: bool,
    /// N62: accept local, private and documentation addresses where public
    /// ones are expected (the router's external address, the configured
    /// `public_address`, the relay's address and its peers): office and
    /// LAN setups, the Docker tests. Default false.
    pub allow_local: bool,
    /// Run a TURN relay for players whose routers cannot meet (N52),
    /// announced on `relays`. `None`: none (`--relay` sets the default).
    pub relay: Option<RelayConfig>,
    /// Tests only (feature `test-hooks`): aim the port mapping at a
    /// `FakeGateway` and pin the base address.
    #[cfg(feature = "test-hooks")]
    #[serde(skip)]
    pub test_gateway: Option<TestGateway>,
}

/// A volunteer TURN relay's settings (N52). `port: 0` and `ports: (0, 0)`
/// take ephemeral ports (tests).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RelayConfig {
    /// The TURN listening port (UDP). Default 3478.
    pub port: u16,
    /// The relay ports, first and last (UDP; one per allocation). Default
    /// (49160, 49191).
    pub ports: (u16, u16),
    /// Allocations in all, 1 to `MAX_CAPACITY` (4096), and at most the
    /// number of relay ports (one allocation each). Default 32.
    pub capacity: u16,
    /// KiB per second relayed for each player's IP, each way (at least 1).
    /// Default 32.
    pub user_kib_s: u32,
    /// Allocations each player's IP may hold (at least 1). Default 4.
    pub allocations_per_ip: u8,
}

impl Default for RelayConfig {
    fn default() -> Self {
        RelayConfig { port: 3478, ports: (49160, 49191), capacity: 32, user_kib_s: 32, allocations_per_ip: 4 }
    }
}

impl RelayConfig {
    /// The relay ports as a range (`0..=0`: OS-assigned, tests).
    pub fn port_range(&self) -> std::ops::RangeInclusive<u16> {
        self.ports.0..=self.ports.1
    }

    /// Why the settings cannot work, if they cannot.
    fn check(&self) -> anyhow::Result<()> {
        let (first, last) = self.ports;
        if (first == 0) != (last == 0) || first > last {
            bail!("relay ports must be (first, last), first <= last, or (0, 0)");
        }
        if self.port != 0 && self.port_range().contains(&self.port) {
            bail!("the relay's port {} is among its relay ports", self.port);
        }
        if self.capacity == 0 || self.capacity > MAX_CAPACITY {
            bail!("relay capacity must be 1 to {MAX_CAPACITY}");
        }
        // One allocation per relay port (ephemeral ones take any number).
        let count = u32::from(last - first) + 1;
        if first != 0 && u32::from(self.capacity) > count {
            bail!("relay capacity {} is more than its {count} relay ports", self.capacity);
        }
        if self.user_kib_s == 0 {
            bail!("relay user_kib_s must be at least 1");
        }
        if self.allocations_per_ip == 0 {
            bail!("relay allocations_per_ip must be at least 1");
        }
        Ok(())
    }
}

/// Tests only (feature `test-hooks`): where the port mapping asks (a
/// `FakeGateway`'s PCP/NAT-PMP and SSDP sockets) and the base address
/// (the only host candidate; 127.0.0.1 keeps every datagram on the
/// machine, test rule NT-1).
#[cfg(feature = "test-hooks")]
#[derive(Clone, Debug, PartialEq)]
pub struct TestGateway {
    pub pmp: SocketAddr,
    pub ssdp: SocketAddr,
    pub base: Ipv4Addr,
}

/// The default of `upload_kib_s`: `rl_online`'s `HOST_RATE`, 4 MiB/s.
pub const DEFAULT_UPLOAD_KIB_S: u32 = 4096;
/// What `upload_kib_s` is held to (a value outside is clamped).
pub const UPLOAD_KIB_S: std::ops::RangeInclusive<u32> = 64..=65536;
/// Most relays (the game's bound too).
pub const MAX_RELAYS: usize = 32;

impl Default for Config {
    fn default() -> Self {
        Config {
            name: "rust-liero server".into(),
            visibility: Visibility::Public,
            password: None,
            port: DEFAULT_PORT,
            max_players: 16,
            max_spectators: 8,
            admins: Vec::new(),
            fill_bots: 0,
            rotation: Vec::new(),
            start_when_ready: true,
            min_players: 1,
            bans_file: None,
            upload_kib_s: DEFAULT_UPLOAD_KIB_S,
            relays: DEFAULT_RELAYS.iter().map(|r| r.to_string()).collect(),
            region: Region::Unknown,
            public_address: None,
            stun: DEFAULT_STUN.iter().map(|s| s.to_string()).collect(),
            port_mapping: true,
            allow_local: false,
            relay: None,
            #[cfg(feature = "test-hooks")]
            test_gateway: None,
        }
    }
}

/// A STUN server as `host:port`: a host and a port number.
fn valid_stun(s: &str) -> bool {
    s.rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0))
}

impl Config {
    /// Reads a config file's text and checks what needs no content: the
    /// bounds, the admins' keys, a rotation, the relays (each once, at
    /// most [`MAX_RELAYS`]) and the STUN servers (`host:port`).
    pub fn parse(text: &str) -> anyhow::Result<Config> {
        let mut config: Config = ron::from_str(text).context("the config file")?;
        // Each relay once (a trailing slash names the same relay).
        let mut seen: Vec<String> = Vec::new();
        config.relays.retain(|r| {
            let key = r.trim_end_matches('/').to_owned();
            let new = !seen.contains(&key);
            seen.push(key);
            new
        });
        if config.relays.len() > MAX_RELAYS {
            bail!("at most {MAX_RELAYS} relays");
        }
        if let Some(s) = config.stun.iter().find(|s| !valid_stun(s)) {
            bail!("STUN server {s:?}: host:port");
        }
        config.admin_keys()?;
        if config.rotation.is_empty() {
            bail!("the rotation has no map");
        }
        if !(1..=MAX_PLAYERS).contains(&usize::from(config.max_players)) {
            bail!("max_players must be 1 to {MAX_PLAYERS}");
        }
        if usize::from(config.max_spectators) > MAX_SPECTATORS {
            bail!("max_spectators must be at most {MAX_SPECTATORS}");
        }
        if usize::from(config.fill_bots) > MAX_BOTS {
            bail!("fill_bots must be at most {MAX_BOTS}");
        }
        if config.min_players == 0 || config.min_players > config.max_players {
            bail!("min_players must be 1 to max_players");
        }
        if let Some(a) = &config.public_address
            && a.parse::<IpAddr>().is_err()
        {
            bail!("public_address {a:?} is not an IP address");
        }
        if let Some(r) = config.relays.iter().find(|r| !(r.starts_with("wss://") || r.starts_with("ws://"))) {
            bail!("relay {r:?}: a wss:// or ws:// URL");
        }
        if let Some(relay) = &config.relay {
            relay.check()?;
        }
        Ok(config)
    }

    /// The configured public address (checked by [`Config::parse`]).
    pub fn public_ip(&self) -> Option<IpAddr> {
        self.public_address.as_deref().and_then(|a| a.parse().ok())
    }

    /// The configured public address when the Internet cannot reach it
    /// (IPv6, which invites and joiners do not use, or not global) and a
    /// relay is public: such a server is not announced on Nostr (its note
    /// would leak the LAN address, and every list refuses it). With
    /// `allow_local` (N62) only IPv6 is.
    pub fn unreachable_public_address(&self) -> Option<IpAddr> {
        self.public_ip()
            .filter(|ip| ip.is_ipv6() || (!is_global(*ip) && !self.allow_local))
            .filter(|_| !local_relays(&self.relays))
    }

    /// Bytes per second the server sends content in all: `upload_kib_s`
    /// KiB, clamped to [`UPLOAD_KIB_S`].
    pub fn upload_rate(&self) -> u64 {
        u64::from(self.upload_kib_s.clamp(*UPLOAD_KIB_S.start(), *UPLOAD_KIB_S.end())) * 1024
    }

    /// The admins' keys.
    pub fn admin_keys(&self) -> anyhow::Result<Vec<PublicKey>> {
        self.admins
            .iter()
            .map(|k| parse_key(k).with_context(|| format!("admin key {k:?}: 64 hex characters")))
            .collect()
    }

    /// The lobby's settings.
    pub fn host_config(&self) -> anyhow::Result<HostConfig> {
        Ok(HostConfig {
            name: self.name.clone(),
            password: self.password.clone(),
            max_players: self.max_players,
            max_spectators: self.max_spectators,
            admins: self.admin_keys()?,
        })
    }

    /// Checks the rotation against `content`: every mod and level there,
    /// every entry a setup the wire takes.
    pub fn check(&self, content: &Content) -> anyhow::Result<()> {
        Rotation::new(content, &self.rotation, self.fill_bots).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(fields: &str) -> anyhow::Result<Config> {
        Config::parse(&format!(
            r#"({fields} rotation: [(mod: "olx-classic", level: "olx-level-dirt-level")])"#
        ))
    }

    /// Plan 12b's fields: mapping on, local addresses refused and no relay
    /// unless asked; a relay's defaults (N52).
    #[test]
    fn the_new_fields_default() {
        let c = with("").unwrap();
        assert!(c.port_mapping && !c.allow_local);
        assert_eq!(c.relay, None);
        let c = with("port_mapping: false, allow_local: true, relay: Some(()),").unwrap();
        assert!(!c.port_mapping && c.allow_local);
        let r = c.relay.unwrap();
        assert_eq!(
            r,
            RelayConfig {
                port: 3478,
                ports: (49160, 49191),
                capacity: 32,
                user_kib_s: 32,
                allocations_per_ip: 4
            }
        );
        assert_eq!(r.port_range(), 49160..=49191);
        let r = with("relay: Some((port: 0, ports: (0, 0), capacity: 256)),").unwrap().relay.unwrap();
        assert_eq!((r.port, r.port_range(), r.capacity), (0, 0..=0, 256));
    }

    #[test]
    fn a_relay_config_is_checked() {
        for bad in [
            "(ports: (49170, 49160))",
            "(ports: (0, 49160))",
            "(port: 49165)",
            "(capacity: 0)",
            "(capacity: 5000)",
            "(user_kib_s: 0)",
            "(allocations_per_ip: 0)",
            "(turn: [])",
        ] {
            assert!(with(&format!("relay: Some({bad}),")).is_err(), "{bad}");
        }
        // Task 8 review: one allocation per relay port, so no more than them
        // (ephemeral ports take any capacity).
        assert!(with("relay: Some((port: 3478, ports: (40000, 40000), capacity: 4096)),").is_err());
        assert!(with("relay: Some((port: 3478, ports: (40000, 40009), capacity: 10)),").is_ok());
        assert!(with("relay: Some((port: 3478, ports: (40000, 40009), capacity: 11)),").is_err());
        assert!(with("relay: Some((port: 0, ports: (0, 0), capacity: 4096)),").is_ok());
    }

    /// N62: with `allow_local` a private `public_address` is announced
    /// (an office's relay and lists accept it); IPv6 never is.
    #[test]
    fn allow_local_announces_a_private_public_address() {
        let public = r#"relays: ["wss://relay.example.org"],"#;
        let c =
            with(&format!(r#"public_address: Some("192.168.1.5"), {public} allow_local: true,"#)).unwrap();
        assert_eq!(c.unreachable_public_address(), None);
        let c =
            with(&format!(r#"public_address: Some("2a00:1450::1"), {public} allow_local: true,"#)).unwrap();
        assert!(c.unreachable_public_address().is_some());
    }
}
