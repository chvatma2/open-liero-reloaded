//! Online play in the game's menus (plan 10 task 12): what the shell keeps
//! of an online game besides its match ([`Net`]), the conversions between
//! the lobby's wire types (`olr-proto`, `olr-lobby`) and the menus' plain
//! views (`olr_ui::{LobbyView, MemberView, MatchPrefs, LanEntry}`), the
//! network overlay's numbers ([`NetMeter`], [`NetView`]), the desync
//! bundles' files ([`save_bundle`]) and the download cache
//! ([`open_download_cache`], plan 11). Every online peer is polled behind
//! a [`pump::PeerHandle`], whose network pump keeps it polled while the UI
//! thread is busy (ruling PF12c-2).

pub mod pump;

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use olr_bots::{BotConfig, Difficulty, LoadoutPolicy, Personality};
use olr_content::{Content, DownloadCache, InstallError, WeaponChoice};
use olr_lobby::NotStartable;
use olr_net::lan::{Announcer, LanGame};
use olr_online::content::{Download, DownloadState};
use olr_online::matches::{bot_config, bot_wire, deathmatch, loadout_of, rules_wire, setup_packs};
use olr_online::{NetStats, OnlinePeer, StartError};
use olr_proto::{
    ItemKind, LanAnnounce, LoadoutWire, LobbyState, Look, MatchSetupWire, Member, Phase, SeatWire,
};
use olr_ui::weapon_select::WeaponMenu;
use olr_ui::{
    BotLevel, BotLoadout, BotPersonality, DownloadView, LanEntry, LobbyView, MatchPrefs, MemberView,
    PHASE_LOBBY, RulePrefs, SEAT_SPECTATOR,
};

use crate::setup::DEFAULT_SKIN;
pub use pump::{PeerHandle, PolledEvents};

/// The lobby's chat lines the menus keep (oldest go first).
pub const CHAT_KEEP: usize = 200;
/// The overlay's rollback percentile spans this long (spec §7.3).
pub const DEPTH_WINDOW: Duration = Duration::from_secs(5);
/// The overlay samples the rollback histogram at most this often.
const SAMPLE_EVERY: Duration = Duration::from_millis(100);
/// A hidden window's frames (decision N21) come this often: the network
/// and the 100 Hz ticks go on without drawing, and without spinning.
pub const HIDDEN_FRAME: Duration = Duration::from_millis(4);
/// A hidden window's frames while the network is live without an online
/// game (the Multiplayer screen's lists, lingering relays, a lookup): no
/// match to keep at 100 Hz, so a slow wake is enough (Task 9 re-review).
pub const HIDDEN_IDLE_FRAME: Duration = Duration::from_millis(60);
/// Where desync bundles go in the data directory.
pub const DESYNCS_DIR: &str = "desyncs";
/// Download cache directories tried, the first and `-2` to `-N`, while
/// another game holds the one before (ruling PF11-3).
pub const CACHE_DIRS: u32 = 4;

/// A download cache shared by the game's peers (ruling PF11-4), or why
/// there is none.
pub type SharedCache = Result<Arc<Mutex<DownloadCache>>, String>;

/// A mod's and a level's pack ids.
pub type ModLevel = (String, String);

/// What the lobby's weapon menu was made of: the mod, the setup's loadout
/// and the peer's content (it changes as downloads land).
#[derive(Clone)]
pub struct WeaponsKey {
    pub mod_id: String,
    pub loadout: LoadoutWire,
    pub content: Arc<Content>,
}

impl PartialEq for WeaponsKey {
    fn eq(&self, o: &WeaponsKey) -> bool {
        self.mod_id == o.mod_id && self.loadout == o.loadout && Arc::ptr_eq(&self.content, &o.content)
    }
}

/// What the shell keeps of an online game: its peer (while no match holds
/// it: a match's is its session's `OnlineDriver`), whether it hosts, and
/// what it showed last.
pub struct Net {
    /// The peer behind its handle, its pump running from the peer's start
    /// to its release; `None` while a match session plays it.
    pub peer: Option<PeerHandle>,
    pub host: bool,
    /// The lobby is up (`LobbyJoined` reported).
    pub joined: bool,
    /// The host says this client's inputs are late (spec §3.2).
    pub lagging: bool,
    /// LAN announcements while hosting a Public or LAN game.
    pub announcer: Option<Announcer>,
    /// What a hosted game does to be found (plan 12a): its invite, a
    /// Public game's notes, its lobby lines (plan 12b: its reachability).
    pub hosted: Option<crate::finding::Hosted>,
    /// A client's path to its host and the host's end (or the TURN relay's)
    /// as last shown (`OnlinePeer::connection`, following ICE).
    pub connection: Option<(olr_proto::Path, std::net::SocketAddr)>,
    /// The hosted game has a password (the LAN announcement says so).
    pub password: bool,
    /// The hosted game's most players (the LAN announcement's).
    pub max_players: u8,
    /// The LAN announcement of the lobby as last seen.
    pub announce: Option<LanAnnounce>,
    /// The host's setup packs (with their content hashes) of the mod and
    /// level last set: a lobby edit of the rules does not hash them again.
    pub setup_packs: Option<(ModLevel, Vec<(String, String)>)>,
    /// The lobby's weapon choice: its mod and loadout, the content it
    /// was made from (the peer's: a downloaded mod counts), and the menu.
    pub weapons: Option<(WeaponsKey, Option<WeaponMenu>)>,
    /// The content downloads as last reported (`OnlineEvent::Downloads`).
    pub downloads: Vec<DownloadView>,
    /// The members' paths the host's console said (`path_news`, C-M1).
    pub paths_said: std::collections::BTreeMap<olr_proto::PublicKey, olr_proto::Path>,
    /// The overlay's rollback depths.
    pub meter: NetMeter,
    /// `--net-sim`: ready whenever the lobby gathers.
    pub auto_ready: bool,
    /// The host is gone and the match is moving (plan 12c): the HUD's top
    /// line (`MigrationStep`'s text) until the handover (`Migrated`) or the
    /// call-off (`HOST_BACK`).
    pub migrating: Option<String>,
    /// This player took the match over from a lost host: it hosts a lobby
    /// closed to newcomers (N72, `olr_ui::MIGRATED_LISTING`).
    pub migrated: bool,
    /// This player reached the new host after its resume (N72): its match
    /// view is frozen until the new host's start lets it watch, or the
    /// lobby leaves the match (results, gathering: then the Lobby shows).
    pub late: bool,
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
    /// The game's `peer`, its pump started.
    pub fn new(peer: OnlinePeer, host: bool) -> Net {
        Net {
            peer: Some(PeerHandle::pumped(peer)),
            host,
            joined: false,
            lagging: false,
            announcer: None,
            hosted: None,
            connection: None,
            password: false,
            max_players: 0,
            announce: None,
            setup_packs: None,
            weapons: None,
            downloads: Vec::new(),
            meter: NetMeter::default(),
            auto_ready: false,
            paths_said: std::collections::BTreeMap::new(),
            migrating: None,
            migrated: false,
            late: false,
        }
    }
}

/// The host's console lines for its members' paths (final review C-M1):
/// "join: <name> via <tag>" when a member's path first shows, "path:
/// <name> now via <tag>" when it changes; `said` keeps what was said, by
/// key (members gone are forgotten).
pub fn path_news(
    said: &mut std::collections::BTreeMap<olr_proto::PublicKey, olr_proto::Path>,
    members: &[Member],
) -> Vec<String> {
    said.retain(|key, _| members.iter().any(|m| m.key == *key));
    let mut lines = Vec::new();
    for m in members {
        let Some(path) = m.path else { continue };
        let tag = olr_online::join::path_tag(path);
        match said.insert(m.key, path) {
            None => lines.push(format!("join: {} via {tag}", m.look.name)),
            Some(old) if old != path => lines.push(format!("path: {} now via {tag}", m.look.name)),
            Some(_) => {}
        }
    }
    lines
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
        fingerprint: olr_lobby::fingerprint(&m.key),
        colour: m.look.colour,
        seat: match m.seat {
            SeatWire::Player { slot } => format!("Player {}", u32::from(slot) + 1),
            SeatWire::Spectator => SEAT_SPECTATOR.into(),
            SeatWire::Waiting => "Waiting".into(),
        },
        ready: m.ready,
        missing: m.missing.clone(),
        download: m.download,
        ping_ms: m.ping_ms,
        admin: m.admin,
        path: m.path.map(|p| olr_online::join::path_tag(p).to_string()),
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
/// host's answer, a client's "Waiting for the host"; `downloads` and the
/// download `prompt`: the peer's, plan 11).
pub fn lobby_view(
    state: &LobbyState,
    me: u8,
    host: bool,
    can_start: Result<(), String>,
    downloads: Vec<DownloadView>,
    prompt: Option<u64>,
) -> LobbyView {
    LobbyView {
        name: state.name.clone(),
        me,
        host,
        phase: phase_text(state.phase),
        members: state.members.iter().map(member_view).collect(),
        setup: prefs_of(&state.setup),
        can_start,
        downloads,
        prompt,
        invite: None,
        listing: None,
        connection: None,
        reach: Vec::new(),
    }
}

/// One content download as the lobby lists it.
pub fn download_view(d: &Download) -> DownloadView {
    let kind = match d.item.kind {
        ItemKind::Mod => "mod",
        ItemKind::Level => "level",
        ItemKind::Theme => "theme",
    };
    let state = match &d.state {
        DownloadState::Queued => "queued".to_string(),
        DownloadState::Receiving => "downloading".to_string(),
        DownloadState::Verifying => "checking".to_string(),
        DownloadState::Done => "done".to_string(),
        DownloadState::Failed(why) => format!("failed: {why}"),
        DownloadState::Unavailable => "not available".to_string(),
    };
    DownloadView { name: d.item.id.clone(), kind: kind.into(), have: d.have, size: d.item.size, state }
}

/// The game's download cache (ruling PF11-3): `dir` (`Store::cache_dir`),
/// else, while another game holds it (`InstallError::Locked`), `dir-2` to
/// `dir-`[`CACHE_DIRS`]. `Err(why)` without a directory or when none
/// opens: downloads are off and the lobby says why.
pub fn open_download_cache(dir: Option<PathBuf>) -> SharedCache {
    let Some(dir) = dir else { return Err("there is no cache directory".into()) };
    for d in cache_dirs(Some(dir.clone())) {
        match DownloadCache::open(&d) {
            Ok(c) => return Ok(Arc::new(Mutex::new(c))),
            Err(InstallError::Locked) => continue,
            Err(e) => return Err(format!("{}: {e}", d.display())),
        }
    }
    Err(format!("{CACHE_DIRS} download caches in {} are in use by other games", dir.display()))
}

/// Every directory a game's download cache may be in (ruling PF11-3):
/// `dir` (`Store::cache_dir`), then `dir-2` to `dir-`[`CACHE_DIRS`]; none
/// without `dir`. Replays look their packs up in all of them
/// (`DownloadCache::lookup`), whichever cache a game holds.
pub fn cache_dirs(dir: Option<PathBuf>) -> Vec<PathBuf> {
    let Some(dir) = dir else { return Vec::new() };
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (1..=CACHE_DIRS)
        .map(|i| if i == 1 { dir.clone() } else { dir.with_file_name(format!("{name}-{i}")) })
        .collect()
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
            Personality::Idle => BotPersonality::Idle,
        },
        loadout: match first.loadout {
            LoadoutPolicy::OlxPreset => BotLoadout::OlxPreset,
            LoadoutPolicy::Random => BotLoadout::Random,
        },
        rules: RulePrefs::from(&deathmatch(&s.rules)),
        bonuses: false,
        restrictions: olr_content::DEFAULT_PRESET.into(),
        loading_time: s.loadout.loading_time,
    }
}

/// The lobby's setup of the menus' `p` (not through `MatchPrefs::sanitize`:
/// a lobby of people may have no bot): `packs` (its mod's, level's and
/// theme's with their content hashes: `setup_packs`, `Net::packs`), the
/// bots named "Bot N" in OLX's bot colours away from `host_colour`, in the
/// default skin.
pub fn setup_of(p: &MatchPrefs, packs: Vec<(String, String)>, host_colour: [u8; 3]) -> MatchSetupWire {
    let n = usize::from(p.bots);
    let colours = olr_ui::bot_colours(host_colour, n);
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
                skin_hash: olr_proto::NO_HASH.into(),
            };
            bot_wire(&c, look)
        })
        .collect();
    MatchSetupWire {
        mod_id: p.mod_id.clone(),
        level_id: p.level_id.clone(),
        packs,
        rules: rules_wire(&p.rules.deathmatch()),
        loadout: LoadoutWire { loading_time: p.loading_time, ..LoadoutWire::default() },
        bots,
    }
}

/// The weapon screen's menu for a lobby's mod and loadout (the lobby's
/// "Choose weapons…", decision N19); `None` when the mod is not here.
pub fn weapon_menu(content: &Content, mod_id: &str, loadout: &LoadoutWire) -> Option<WeaponMenu> {
    let m = content.sim_mod(mod_id).ok()?;
    let rules = content.rules(mod_id, &WeaponChoice::Loadout(loadout_of(loadout))).ok()?;
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
        bots: g.info.bots,
        password: g.info.password,
        addr: g.addr,
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
/// session's cumulative depth histogram (`olr_netcode::Stats::depth_hist`).
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
/// `desync-<unix seconds>-<n>.oldg`, the first `n` free; returns the file.
/// Bounded by olr-online's `bundle::save`: 4 MiB a bundle, 64 MiB of
/// bundles (the oldest go).
pub fn save_bundle(dir: &Path, bytes: &[u8], unix_secs: u64) -> io::Result<PathBuf> {
    olr_online::bundle::save(dir, bytes, unix_secs)
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
    /// Final review C-M1: the host's console says each member's path when
    /// it first shows and when it changes, once; members gone are forgotten.
    #[test]
    fn the_hosts_console_says_each_members_path() {
        let member = |id: u8, name: &str, path: Option<olr_proto::Path>| olr_proto::Member {
            id,
            key: [id; 32],
            look: Look { name: name.into(), colour: [0; 3], skin: String::new(), skin_hash: String::new() },
            seat: olr_proto::SeatWire::Spectator,
            ready: false,
            missing: Vec::new(),
            ping_ms: None,
            admin: false,
            download: None,
            path,
        };
        let mut said = std::collections::BTreeMap::new();
        let host = member(0, "Host", None);
        let ann = |p| member(1, "Ann", Some(p));
        let lines = path_news(&mut said, &[host.clone(), ann(olr_proto::Path::HolePunched)]);
        assert_eq!(lines, ["join: Ann via punched"]);
        assert!(path_news(&mut said, &[host.clone(), ann(olr_proto::Path::HolePunched)]).is_empty());
        let lines = path_news(&mut said, &[host.clone(), ann(olr_proto::Path::Relayed)]);
        assert_eq!(lines, ["path: Ann now via relayed"]);
        assert!(path_news(&mut said, std::slice::from_ref(&host)).is_empty());
        assert_eq!(path_news(&mut said, &[host, ann(olr_proto::Path::Relayed)]), ["join: Ann via relayed"]);
    }

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

    /// Ruling PF11-3: a cache directory another game holds is skipped
    /// for `-2` .. `-4`; then there is none, saying why; no directory is
    /// no cache.
    #[test]
    fn a_cache_held_by_another_game_moves_to_the_next_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("downloads");
        let held: Vec<_> = (0..CACHE_DIRS).map(|_| open_download_cache(Some(dir.clone())).unwrap()).collect();
        let dirs: Vec<PathBuf> = held.iter().map(|c| c.lock().unwrap().dir().to_path_buf()).collect();
        let names = ["downloads", "downloads-2", "downloads-3", "downloads-4"];
        assert_eq!(dirs, names.map(|n| tmp.path().join(n)));
        let Err(why) = open_download_cache(Some(dir.clone())) else { panic!("a fifth cache") };
        assert!(why.contains("in use by other games"), "{why}");
        drop(held);
        let again = open_download_cache(Some(dir.clone())).unwrap();
        assert_eq!(again.lock().unwrap().dir(), dir, "the first again once free");
        assert!(open_download_cache(None).is_err_and(|why| why.contains("no cache directory")));
        // Replays look in every one of them.
        assert_eq!(cache_dirs(Some(dir)), names.map(|n| tmp.path().join(n)));
        assert!(cache_dirs(None).is_empty());
    }

    #[test]
    fn downloads_show_their_kind_and_state_in_words() {
        let item = |kind| olr_proto::Item {
            kind,
            id: "olx-level-x".into(),
            hash: "0".repeat(64),
            size: 1000,
            required: true,
        };
        let d = |kind, state| download_view(&Download { item: item(kind), have: 250, state });
        let v = d(ItemKind::Level, DownloadState::Receiving);
        assert_eq!((v.name.as_str(), v.kind.as_str(), v.have, v.size), ("olx-level-x", "level", 250, 1000));
        let states = [
            (DownloadState::Queued, "queued"),
            (DownloadState::Receiving, "downloading"),
            (DownloadState::Verifying, "checking"),
            (DownloadState::Done, "done"),
            (DownloadState::Failed("the host stopped sending".into()), "failed: the host stopped sending"),
            (DownloadState::Unavailable, "not available"),
        ];
        for (state, text) in states {
            assert_eq!(d(ItemKind::Level, state).state, text);
        }
        let kinds = [(ItemKind::Mod, "mod"), (ItemKind::Theme, "theme")];
        for (kind, text) in kinds {
            assert_eq!(d(kind, DownloadState::Done).kind, text);
        }
    }

    #[test]
    fn bundles_take_the_next_free_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(DESYNCS_DIR);
        let a = save_bundle(&dir, b"one", 1_700_000_000).unwrap();
        let b = save_bundle(&dir, b"two", 1_700_000_000).unwrap();
        assert_eq!(a.file_name().unwrap(), "desync-1700000000-1.oldg");
        assert_eq!(b.file_name().unwrap(), "desync-1700000000-2.oldg");
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
