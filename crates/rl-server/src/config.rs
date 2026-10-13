//! The server's config file (networking spec §7.4), RON, every field
//! documented in `fixtures/server/example.ron`.

use std::path::PathBuf;

use anyhow::{Context as _, bail};
use rl_content::Content;
use rl_lobby::HostConfig;
use rl_modes::DeathmatchSettings;
use rl_net::endpoint::DEFAULT_PORT;
use rl_proto::{MAX_BOTS, MAX_PLAYERS, MAX_SPECTATORS, PublicKey};
use serde::Deserialize;

use crate::files::parse_key;
use crate::rotation::Rotation;

/// Who can find the server (Nostr arrives in plan 12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Visibility {
    /// Announced on the LAN (UDP broadcast on `LAN_PORT` every 2 s) and
    /// joinable by address.
    Lan,
    /// Joinable by address only.
    DirectOnly,
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
}

/// The default of `upload_kib_s`: `rl_online`'s `HOST_RATE`, 4 MiB/s.
pub const DEFAULT_UPLOAD_KIB_S: u32 = 4096;
/// What `upload_kib_s` is held to (a value outside is clamped).
pub const UPLOAD_KIB_S: std::ops::RangeInclusive<u32> = 64..=65536;

impl Default for Config {
    fn default() -> Self {
        Config {
            name: "rust-liero server".into(),
            visibility: Visibility::Lan,
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
        }
    }
}

impl Config {
    /// Reads a config file's text and checks what needs no content: the
    /// bounds, the admins' keys, a rotation.
    pub fn parse(text: &str) -> anyhow::Result<Config> {
        let config: Config = ron::from_str(text).context("the config file")?;
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
        Ok(config)
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
