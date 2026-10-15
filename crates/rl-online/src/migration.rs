//! Host migration over real connections (networking spec §3.7, plan 12c):
//! how a client takes part ([`MigrationConfig`]), what it reports while the
//! match moves ([`MigrationStep`]) and the texts of its endings. The
//! drivers are `peer/migrate.rs`'s; every rule (timing, the successor
//! tried, the merge) is `rl_netcode`'s.

use std::time::Duration;

use rl_discovery::{PoolConfig, Region};
use rl_net::endpoint::{EndpointConfig, IceConfig};

/// A direct join to a successor gives up after this long (N73).
pub const MIGRATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// Why a match ends for a peer that found no successor (spec §3.7: the
/// "host lost" screen).
pub const HOST_LOST: &str = "The host left and no other player could take over the game.";

/// The log line of a migration called off: the old host was heard again
/// before anyone took over (N69).
pub const HOST_BACK: &str = "The host is back: the match goes on";

/// Why a host that stalled `secs` seconds stepped down (N70).
pub fn stepped_down(secs: f32) -> String {
    format!("Your game stopped for {secs:.1} s, so the match moved on without you.")
}

/// How a client takes part in a host migration (spec §3.7).
#[derive(Clone, Debug)]
pub struct MigrationConfig {
    /// Its standby endpoint (N63): `port` is forced to 0 and
    /// `ice.port_mapping` off. `None`: it never hosts (N79).
    pub standby: Option<EndpointConfig>,
    /// The relays it listens on once it hosts, and the first 2 it reports;
    /// `None`: no Nostr (direct joins only).
    pub relays: Option<PoolConfig>,
    /// Its joins to a successor through Nostr: their ICE (STUN, TURN),
    /// region and TURN notes (as a `NostrJoin`'s).
    pub ice: IceConfig,
    pub region: Region,
    pub turn_from_nostr: bool,
    pub allow_local: bool,
    /// Off: the host's loss ends this peer as before (tests that need the
    /// old ending).
    pub enabled: bool,
}

impl MigrationConfig {
    /// No migration (the old ending).
    pub fn off() -> MigrationConfig {
        MigrationConfig {
            standby: None,
            relays: None,
            ice: IceConfig::default(),
            region: Region::Unknown,
            turn_from_nostr: false,
            allow_local: false,
            enabled: false,
        }
    }

    /// A standby on port 0 without STUN, no relays: LAN and loopback
    /// games, tests.
    pub fn direct() -> MigrationConfig {
        MigrationConfig {
            standby: Some(EndpointConfig { port: 0, ..EndpointConfig::default() }),
            enabled: true,
            ..MigrationConfig::off()
        }
    }

    /// The relays it reports with its standby: the first 2 of `relays`.
    pub(crate) fn reported_relays(&self) -> Vec<String> {
        self.relays
            .as_ref()
            .map(|p| p.relays.iter().take(rl_proto::MAX_STANDBY_RELAYS).cloned().collect())
            .unwrap_or_default()
    }
}

/// Where a client's migration stands, for its HUD (N81).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationStep {
    /// The host is gone: no successor's turn yet, or none answering.
    Finding,
    /// Joining the successor, by its name.
    Moving(String),
    /// This peer's turn: it takes the match over.
    TakingOver,
}

impl std::fmt::Display for MigrationStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationStep::Finding => write!(f, "The host is gone: finding who takes over…"),
            MigrationStep::Moving(name) => write!(f, "The host is gone: moving the game to {name}…"),
            MigrationStep::TakingOver => write!(f, "The host is gone: you are taking over the game…"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_texts() {
        assert_eq!(MigrationStep::Finding.to_string(), "The host is gone: finding who takes over…");
        assert_eq!(
            MigrationStep::Moving("Anna".into()).to_string(),
            "The host is gone: moving the game to Anna…"
        );
        assert_eq!(MigrationStep::TakingOver.to_string(), "The host is gone: you are taking over the game…");
        assert_eq!(stepped_down(1.73), "Your game stopped for 1.7 s, so the match moved on without you.");
        assert_eq!(HOST_LOST, "The host left and no other player could take over the game.");
    }

    #[test]
    fn off_and_direct() {
        let off = MigrationConfig::off();
        assert!(!off.enabled && off.standby.is_none() && off.relays.is_none());
        let direct = MigrationConfig::direct();
        assert!(direct.enabled && direct.relays.is_none());
        let standby = direct.standby.unwrap();
        assert_eq!(standby.port, 0);
        assert!(standby.ice.stun.is_empty() && !standby.ice.port_mapping);
        let relays = rl_discovery::PoolConfig {
            relays: vec!["ws://a".into(), "ws://b".into(), "ws://c".into()],
            ..Default::default()
        };
        let cfg = MigrationConfig { relays: Some(relays), ..MigrationConfig::direct() };
        assert_eq!(cfg.reported_relays(), ["ws://a", "ws://b"]);
    }
}
