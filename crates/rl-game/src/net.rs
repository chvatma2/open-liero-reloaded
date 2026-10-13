//! Online play in the game's menus (plan 10 task 12): what the shell keeps
//! of an online game besides its match ([`Net`]), the conversions between
//! the lobby's wire types (`rl-proto`, `rl-lobby`) and the menus' plain
//! views (`rl_ui::{LobbyView, MemberView, MatchPrefs, LanEntry}`), the
//! network overlay's numbers ([`NetMeter`], [`NetView`]) and the desync
//! bundles' files ([`save_bundle`]).

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rl_bots::{BotConfig, Difficulty, LoadoutPolicy, Personality};
use rl_content::Content;
use rl_lobby::NotStartable;
use rl_net::lan::{Announcer, LanGame};
use rl_online::matches::{bot_config, bot_wire, deathmatch, rules_wire, setup_packs};
use rl_online::{NetStats, OnlinePeer, StartError};
use rl_proto::{LanAnnounce, LobbyState, Look, MatchSetupWire, Member, Phase, SeatWire};
use rl_ui::weapon_select::WeaponMenu;
use rl_ui::{
    BotLevel, BotLoadout, BotPersonality, LanEntry, LobbyView, MatchPrefs, MemberView, PHASE_LOBBY,
    RulePrefs, SEAT_SPECTATOR,
};

use crate::setup::DEFAULT_SKIN;

/// The lobby's chat lines the menus keep (oldest go first).
pub const CHAT_KEEP: usize = 200;
/// The overlay's rollback percentile spans this long (spec §7.3).
pub const DEPTH_WINDOW: Duration = Duration::from_secs(5);
/// The overlay samples the rollback histogram at most this often.
const SAMPLE_EVERY: Duration = Duration::from_millis(100);
/// A hidden window's frames (decision N21) come this often: the network
/// and the 100 Hz ticks go on without drawing, and without spinning.
pub const HIDDEN_FRAME: Duration = Duration::from_millis(4);
/// Where desync bundles go in the data directory.
pub const DESYNCS_DIR: &str = "desyncs";

/// A mod's and a level's pack ids.
pub type ModLevel = (String, String);

/// What the shell keeps of an online game: its peer (while no match holds
/// it: a match's is its session's `OnlineDriver`), whether it hosts, and
/// what it showed last.
pub struct Net {
    /// The peer; `None` while a match session plays it.
    pub peer: Option<OnlinePeer>,
    pub host: bool,
    /// The lobby is up (`LobbyJoined` reported).
    pub joined: bool,
    /// The host says this client's inputs are late (spec §3.2).
    pub lagging: bool,
    /// LAN announcements while hosting a LAN game.
    pub announcer: Option<Announcer>,
    /// The hosted game has a password (the LAN announcement says so).
    pub password: bool,
    /// The hosted game's most players (the LAN announcement's).
    pub max_players: u8,
    /// The LAN announcement of the lobby as last seen.
    pub announce: Option<LanAnnounce>,
    /// The setup's packs this client checked last (`LobbyHandle::have`).
    pub checked_packs: Option<Vec<(String, String)>>,
    /// The host's setup packs (with their content hashes) of the mod and
    /// level last set: a lobby edit of the rules does not hash them again.
    pub setup_packs: Option<(ModLevel, Vec<(String, String)>)>,
    /// The lobby's weapon choice: its mod and restrictions, and the menu.
    pub weapons: Option<((String, String), Option<WeaponMenu>)>,
    /// The overlay's rollback depths.
    pub meter: NetMeter,
    /// `--net-sim`: ready whenever the lobby gathers.
    pub auto_ready: bool,
}

impl Net {
    /// The setup packs of `mod_id` on `level_id` (`setup_packs`), hashed
    /// once while they stay the same.
    pub fn packs(
        &mut self,
        content: &Content,
        mod_id: &str,
        level_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        if let Some(((m, l), packs)) = &self.setup_packs
            && m == mod_id
            && l == level_id
        {
            return Ok(packs.clone());
        }
        let packs = setup_packs(content, mod_id, level_id)?;
        self.setup_packs = Some(((mod_id.into(), level_id.into()), packs.clone()));
        Ok(packs)
    }
}

impl Net {
    pub fn new(peer: OnlinePeer, host: bool) -> Net {
        Net {
            peer: Some(peer),
            host,
            joined: false,
            lagging: false,
            announcer: None,
            password: false,
            max_players: 0,
            announce: None,
            checked_packs: None,
            setup_packs: None,
            weapons: None,
            meter: NetMeter::default(),
            auto_ready: false,
        }
    }
}

/// The lobby's phase as the menus show it: [`PHASE_LOBBY`] while players
/// gather (and while the last results show: the next start waits for
/// them), else what is going on.
pub fn phase_text(p: Phase) -> String {
    match p {
        Phase::Gathering | Phase::Results => PHASE_LOBBY.into(),
        Phase::Starting => "Starting".into(),
        Phase::Playing { .. } => "Playing".into(),
    }
}

/// One member as the lobby screen shows it.
pub fn member_view(m: &Member) -> MemberView {
    MemberView {
        id: m.id,
        name: m.look.name.clone(),
        fingerprint: rl_lobby::fingerprint(&m.key),
        colour: m.look.colour,
        seat: match m.seat {
            SeatWire::Player { slot } => format!("Player {}", u32::from(slot) + 1),
            SeatWire::Spectator => SEAT_SPECTATOR.into(),
            SeatWire::Waiting => "Waiting".into(),
        },
        ready: m.ready,
        missing: m.missing.clone(),
        ping_ms: m.ping_ms,
        admin: m.admin,
    }
}

/// Why the host cannot start, in words (the Start button's tooltip).
pub fn start_reason(e: &StartError, state: &LobbyState) -> String {
    let name = |id: &u8| state.members.iter().find(|m| m.id == *id).map_or("?", |m| m.look.name.as_str());
    match e {
        StartError::NotHost => "only the host starts".into(),
        StartError::Running => "a match is running".into(),
        StartError::Build(why) => format!("the match cannot be built: {why}"),
        StartError::Lobby(n) => match n {
            NotStartable::Phase => "the last match is still ending".into(),
            NotStartable::NoPlayers => "nobody plays".into(),
            NotStartable::TooFewWorms => "two worms at least: add a bot".into(),
            NotStartable::TooManyWorms => "32 worms at most".into(),
            NotStartable::NotReady(ids) => {
                format!("not ready: {}", ids.iter().map(name).collect::<Vec<_>>().join(", "))
            }
            NotStartable::Stale => "the lobby changed".into(),
        },
    }
}

/// The lobby as player `me` sees it (`host`: it hosts; `can_start`: the
/// host's answer, a client's "Waiting for the host").
pub fn lobby_view(state: &LobbyState, me: u8, host: bool, can_start: Result<(), String>) -> LobbyView {
    LobbyView {
        name: state.name.clone(),
        me,
        host,
        phase: phase_text(state.phase),
        members: state.members.iter().map(member_view).collect(),
        setup: prefs_of(&state.setup),
        can_start,
    }
}

fn level_of(d: Difficulty) -> BotLevel {
    match d {
        Difficulty::Easy => BotLevel::Easy,
        Difficulty::Medium => BotLevel::Medium,
        Difficulty::Hard => BotLevel::Hard,
        Difficulty::Xtreme => BotLevel::Xtreme,
    }
}

/// The lobby's setup as the menus' match panels show it: the bots' common
/// difficulty (each one's when they differ), personality and loadout from
/// the first bot. A bot whose wire numbers are out of range counts with
/// the defaults (the lobby refuses such a setup anyway).
pub fn prefs_of(s: &MatchSetupWire) -> MatchPrefs {
    let bots: Vec<BotConfig> = s.bots.iter().map(|b| bot_config(b).unwrap_or_default()).collect();
    let first = bots.first().copied().unwrap_or_default();
    let levels: Vec<BotLevel> = bots.iter().map(|b| level_of(b.difficulty)).collect();
    let difficulty = levels.first().copied().unwrap_or_default();
    MatchPrefs {
        mod_id: s.mod_id.clone(),
        level_id: s.level_id.clone(),
        bots: u8::try_from(bots.len()).unwrap_or(u8::MAX),
        difficulty,
        per_bot: levels.iter().any(|&l| l != difficulty).then_some(levels),
        personality: match first.personality {
            Personality::Deathmatch => BotPersonality::Olx,
            Personality::Chaos => BotPersonality::Chaos,
        },
        loadout: match first.loadout {
            LoadoutPolicy::OlxPreset => BotLoadout::OlxPreset,
            LoadoutPolicy::Random => BotLoadout::Random,
        },
        rules: RulePrefs::from(&deathmatch(&s.rules)),
        bonuses: s.bonuses,
        restrictions: s.restrictions.clone(),
        loading_time: s.loading_time,
    }
}

/// The lobby's setup of the menus' `p` (not through `MatchPrefs::sanitize`:
/// a lobby of people may have no bot): `packs` (its mod's, level's and
/// theme's with their content hashes: `setup_packs`, `Net::packs`), the
/// bots named "Bot N" in OLX's bot colours away from `host_colour`, in the
/// default skin.
pub fn setup_of(p: &MatchPrefs, packs: Vec<(String, String)>, host_colour: [u8; 3]) -> MatchSetupWire {
    let n = usize::from(p.bots);
    let colours = rl_ui::bot_colours(host_colour, n);
    let bots = (0..n)
        .map(|i| {
            let c = BotConfig {
                personality: p.personality.personality(),
                difficulty: p.bot_level(i).difficulty(),
                loadout: p.loadout.policy(),
                ..BotConfig::default()
            };
            let look = Look {
                name: format!("Bot {}", i + 1),
                colour: colours[i],
                skin: DEFAULT_SKIN.into(),
                skin_hash: rl_proto::NO_HASH.into(),
            };
            bot_wire(&c, look)
        })
        .collect();
    MatchSetupWire {
        mod_id: p.mod_id.clone(),
        level_id: p.level_id.clone(),
        packs,
        rules: rules_wire(&p.rules.deathmatch()),
        bonuses: p.bonuses,
        loading_time: p.loading_time,
        restrictions: p.restrictions.clone(),
        bots,
    }
}

/// The packs of `setup` this install lacks, or has with other contents.
pub fn missing_packs(content: &Content, setup: &MatchSetupWire) -> Vec<String> {
    setup
        .packs
        .iter()
        .filter(|(id, hash)| {
            let ours = content.pack_kind(id).and_then(|kind| content.pack_hash(id, kind).ok());
            ours.as_deref() != Some(hash.as_str())
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// The weapon screen's menu for a lobby's mod and restrictions (the
/// lobby's "Choose weapons…", decision N19); `None` when the mod is not
/// here.
pub fn weapon_menu(content: &Content, mod_id: &str, restrictions: &str) -> Option<WeaponMenu> {
    let m = content.sim_mod(mod_id).ok()?;
    let rules = content.weapon_rules(mod_id, restrictions).ok()?;
    let names: Vec<String> = m.weapons().iter().map(|w| w.name.clone()).collect();
    let enabled = names.iter().map(|n| rules.is_enabled(n)).collect();
    Some(WeaponMenu { names, enabled, fallback: rules.find_enabled_weapon(), seed: 0 })
}

/// A game announced on the network as the Multiplayer screen lists it
/// (the level by name when this install has it).
pub fn lan_entry(g: &LanGame, level_name: impl Fn(&str) -> Option<String>) -> LanEntry {
    LanEntry {
        name: g.info.name.clone(),
        map: level_name(&g.info.level_id).unwrap_or_else(|| g.info.level_id.clone()),
        players: g.info.players,
        max_players: g.info.max_players,
        password: g.info.password,
        addr: g.addr,
    }
}

/// The LAN announcement of a hosted lobby on `port`.
pub fn announcement(state: &LobbyState, port: u16, max_players: u8, password: bool) -> LanAnnounce {
    let players = state.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).count();
    let spectators = state.members.len() - players;
    LanAnnounce {
        fingerprint: rl_proto::FINGERPRINT,
        name: state.name.clone(),
        port,
        players: u8::try_from(players + state.setup.bots.len()).unwrap_or(u8::MAX),
        max_players,
        spectators: u8::try_from(spectators).unwrap_or(u8::MAX),
        mod_id: state.setup.mod_id.clone(),
        level_id: state.setup.level_id.clone(),
        password,
        playing: matches!(state.phase, Phase::Starting | Phase::Playing { .. }),
    }
}

/// The network overlay's numbers (F3; spec §7.3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetView {
    pub stats: NetStats,
    /// The deepest rollback of the last frame that rolled back, in ticks.
    pub rollback_last: Option<u32>,
    /// The rollbacks' depth p99 over the last [`DEPTH_WINDOW`] (none
    /// without a rollback).
    pub rollback_p99: Option<u32>,
    /// How far the newest predicted tick runs past the ticks with every
    /// slot's final input (`current_tick - frontier`).
    pub frontier_lag: u32,
    /// "Connection problem" (spec §3.2's warning).
    pub lagging: bool,
}

impl NetView {
    /// The overlay's lines.
    pub fn lines(&self) -> Vec<String> {
        let s = &self.stats;
        let opt = |v: Option<u32>| v.map_or("-".to_string(), |v| v.to_string());
        vec![
            format!("ping {} ms   input delay {} ticks", opt(s.rtt_ms), s.input_delay),
            format!(
                "rollback last {}  p99 {} (5 s)  1 s max {}",
                opt(self.rollback_last),
                opt(self.rollback_p99),
                s.rollback_depth
            ),
            format!(
                "loss {}   corrections {:.1}/s   frontier lag {} ticks",
                s.loss.map_or("-".to_string(), |l| format!("{:.1} %", l * 100.0)),
                s.corrections_per_s,
                self.frontier_lag
            ),
        ]
    }
}

/// The rollbacks' depths over the last [`DEPTH_WINDOW`], from the
/// session's cumulative depth histogram (`rl_netcode::Stats::depth_hist`).
#[derive(Clone, Debug, Default)]
pub struct NetMeter {
    /// Histograms sampled at most every [`SAMPLE_EVERY`], oldest first: one
    /// at or before the window's start, as its base.
    samples: VecDeque<(Instant, [u64; 64])>,
    /// The histogram at the last frame.
    last: Option<[u64; 64]>,
    /// The deepest rollback of the last frame that rolled back.
    pub last_depth: Option<u32>,
}

impl NetMeter {
    /// The histogram `hist` at `now` (once a frame).
    pub fn sample(&mut self, now: Instant, hist: &[u64; 64]) {
        if let Some(prev) = &self.last
            && let Some(d) = (0..64).rev().find(|&d| hist[d] > prev[d])
        {
            self.last_depth = Some(d as u32);
        }
        self.last = Some(*hist);
        if self.samples.back().is_none_or(|(t, _)| now.saturating_duration_since(*t) >= SAMPLE_EVERY) {
            self.samples.push_back((now, *hist));
        }
        // Keep one sample at or before the window's start as the base.
        while self.samples.len() > 2 && now.saturating_duration_since(self.samples[1].0) >= DEPTH_WINDOW {
            self.samples.pop_front();
        }
    }

    /// The depth p99 of the rollbacks over the window (nearest rank).
    pub fn p99(&self) -> Option<u32> {
        let (Some((_, base)), Some(now)) = (self.samples.front(), &self.last) else { return None };
        let counts: Vec<u64> = (0..64).map(|d| now[d].saturating_sub(base[d])).collect();
        let n: u64 = counts.iter().sum();
        if n == 0 {
            return None;
        }
        let rank = (n * 99).div_ceil(100);
        let mut seen = 0;
        (0..64)
            .find(|&d| {
                seen += counts[d];
                seen >= rank
            })
            .map(|d| d as u32)
    }

    pub fn clear(&mut self) {
        *self = NetMeter::default();
    }
}

/// Saves desync bundle `bytes` in `dir` (made if missing) as
/// `desync-<unix seconds>-<n>.rldg`, the first `n` free; returns the file.
/// Bounded by rl-online's `bundle::save`: 4 MiB a bundle, 64 MiB of
/// bundles (the oldest go).
pub fn save_bundle(dir: &Path, bytes: &[u8], unix_secs: u64) -> io::Result<PathBuf> {
    rl_online::bundle::save(dir, bytes, unix_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hist(entries: &[(usize, u64)]) -> [u64; 64] {
        let mut h = [0; 64];
        for &(d, n) in entries {
            h[d] = n;
        }
        h
    }

    /// The last rollback's depth and the window's p99 (nearest rank); the
    /// rollbacks older than the window drop out.
    #[test]
    fn the_meter_reads_rollback_depths_over_five_seconds() {
        let t0 = Instant::now();
        let mut m = NetMeter::default();
        m.sample(t0, &hist(&[]));
        assert_eq!((m.last_depth, m.p99()), (None, None));
        // 99 rollbacks of 2 ticks and one of 9 in the first second.
        m.sample(t0 + Duration::from_millis(500), &hist(&[(2, 99), (9, 1)]));
        assert_eq!((m.last_depth, m.p99()), (Some(9), Some(2)));
        m.sample(t0 + Duration::from_millis(600), &hist(&[(2, 99), (9, 1), (4, 1)]));
        assert_eq!(m.last_depth, Some(4));
        assert_eq!(m.p99(), Some(4), "101 rollbacks: rank 100 is a 4");
        // Six seconds later with one more 3: the old ones are out.
        for i in 0..60 {
            m.sample(t0 + Duration::from_millis(700 + 100 * i), &hist(&[(2, 99), (9, 1), (4, 1)]));
        }
        m.sample(t0 + Duration::from_millis(6800), &hist(&[(2, 99), (9, 1), (4, 1), (3, 1)]));
        assert_eq!((m.last_depth, m.p99()), (Some(3), Some(3)));
        assert!(m.samples.len() <= 52, "{} samples kept", m.samples.len());
    }

    #[test]
    fn bundles_take_the_next_free_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(DESYNCS_DIR);
        let a = save_bundle(&dir, b"one", 1_700_000_000).unwrap();
        let b = save_bundle(&dir, b"two", 1_700_000_000).unwrap();
        assert_eq!(a.file_name().unwrap(), "desync-1700000000-1.rldg");
        assert_eq!(b.file_name().unwrap(), "desync-1700000000-2.rldg");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
    }

    #[test]
    fn the_overlay_says_what_it_measures() {
        let v = NetView {
            stats: NetStats { rtt_ms: Some(84), input_delay: 2, loss: Some(0.012), ..NetStats::default() },
            rollback_last: Some(3),
            rollback_p99: Some(6),
            frontier_lag: 4,
            lagging: false,
        };
        let text = v.lines().join("\n");
        for part in ["ping 84 ms", "input delay 2", "last 3", "p99 6", "loss 1.2 %", "frontier lag 4"] {
            assert!(text.contains(part), "{part}: {text}");
        }
    }
}
