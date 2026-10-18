//! What the game plays (the sandbox, spec §19 E11; a Deathmatch against
//! bots; a replay): which packs to load, the match to build, and the
//! assets loaded once for every restart.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use olr_bots::{BotConfig, Difficulty};
use olr_content::{
    Content, DownloadCache, LoadedLevel, LoadedMod, LoadedSkin, LoadedTheme, MatchOptions, Player,
    SandboxSetup, SpawnSpec,
};
use olr_data::manifest::PackKind;
use olr_format::levels::LevelSource;
use olr_modes::DeathmatchSettings;
use olr_proto::MatchStartWire;
use olr_replay::{Packs, Replay};
use olr_sim::{MAX_WORMS, SimSettings};

/// What the command line chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxOptions {
    pub mod_id: String,
    pub level_id: String,
    pub seed: u32,
    /// Idle target worms besides the player.
    pub targets: usize,
    /// OLX's `Bonuses` option: bonuses spawn and fall.
    pub bonuses: bool,
}

/// The world [`SandboxOptions`] describe: the player (worm 0) and the
/// targets, each at a `FindSpot` position with the mod's first five
/// weapons, under OLX's LieroX 0.56 settings (unlimited lives). The targets
/// are capped so the worms fit OLX's `MAX_WORMS`.
pub fn sandbox_setup(o: &SandboxOptions) -> SandboxSetup {
    let worms = 1 + o.targets.min(MAX_WORMS - 1);
    SandboxSetup {
        mod_id: o.mod_id.clone(),
        level_id: o.level_id.clone(),
        seed: o.seed,
        settings: SimSettings { bonuses: o.bonuses, ..SimSettings::lx56() },
        worms: vec![SpawnSpec::default(); worms],
        restrictions: olr_content::DEFAULT_PRESET.into(),
    }
}

/// What the command line chose for a Deathmatch against bots.
#[derive(Clone, Debug, PartialEq)]
pub struct DeathmatchOptions {
    pub seed: u32,
    /// Bots besides the player (1 to `MAX_WORMS - 1`).
    pub bots: usize,
    pub difficulty: Difficulty,
    /// `Lives`: below 0 unlimited.
    pub lives: i32,
    /// `KillLimit`: 0 or less is off.
    pub kill_limit: i32,
    /// `TimeLimit` in minutes: 0 or less is off.
    pub time_limit: f32,
    /// OLX's `Bonuses` option.
    pub bonuses: bool,
    /// No human plays: `bots` bots (2 to `MAX_WORMS`) alone, watched
    /// (`--spectate`; fast-forward allowed, `speed.rs`).
    pub spectate: bool,
}

impl Default for DeathmatchOptions {
    /// OLX's Deathmatch defaults against 3 Medium bots.
    fn default() -> Self {
        let d = DeathmatchSettings::default();
        DeathmatchOptions {
            seed: 1,
            bots: 3,
            difficulty: Difficulty::Medium,
            lives: d.lives,
            kill_limit: d.kill_limit,
            time_limit: d.time_limit,
            bonuses: false,
            spectate: false,
        }
    }
}

/// The match [`DeathmatchOptions`] describe on packs `mod_id` and
/// `level_id`: the human is worm 0, then the bots (OLX's bots at one
/// difficulty), under OLX's LX56 settings, the default restrictions
/// ([`olr_content::DEFAULT_PRESET`]) and the Deathmatch rules given.
/// The bots are capped so the worms fit `MAX_WORMS`. Spectating, the bots
/// play alone (2 to `MAX_WORMS` of them).
pub fn match_options(o: &DeathmatchOptions, mod_id: &str, level_id: &str) -> MatchOptions {
    let bot = BotConfig { difficulty: o.difficulty, ..BotConfig::default() };
    let players: Vec<Player> = if o.spectate {
        vec![Player::Bot(bot); o.bots.clamp(2, MAX_WORMS)]
    } else {
        let bots = o.bots.clamp(1, MAX_WORMS - 1);
        std::iter::once(Player::Human).chain((0..bots).map(|_| Player::Bot(bot))).collect()
    };
    MatchOptions {
        mod_id: mod_id.into(),
        level_id: level_id.into(),
        seed: o.seed,
        settings: SimSettings { bonuses: o.bonuses, ..SimSettings::lx56() },
        players,
        deathmatch: DeathmatchSettings {
            lives: o.lives,
            kill_limit: o.kill_limit,
            time_limit: o.time_limit,
            ..DeathmatchSettings::default()
        },
        restrictions: olr_content::DEFAULT_PRESET.into(),
        pool_capacity: olr_sim::MatchSetup::DEFAULT_POOL_CAPACITY,
    }
}

/// The pack id `given` means among `ids`: itself, or an OLX import's id
/// (`prefix` and the slug of `given`, so `--level "Liero Factory"` and
/// `--mod MW 1.0` work).
pub fn resolve_id(given: &str, prefix: &str, ids: &[String]) -> anyhow::Result<String> {
    olr_content::resolve_id(given, prefix, ids)
        .with_context(|| format!("no pack {given:?}; there are: {}", ids.join(", ")))
}

/// OLX's default skin's pack id.
pub const DEFAULT_SKIN: &str = "olx-skin-default";

/// What the game plays.
pub enum Game {
    /// The sandbox (spec §19 E11).
    Sandbox(SandboxSetup),
    /// A Deathmatch, the human (any slot) against bots; `packs` are its mod,
    /// level and theme for the replay; `spawn` is where the human worm
    /// first spawns (`None`: a free spot; only the editor's Play sets it).
    Deathmatch { options: Box<MatchOptions>, packs: Packs, spawn: Option<(i32, i32)> },
    /// A replay, played with the normal renderer.
    Replay(Box<Replay>),
    /// An online match (plan 10): its world is the netcode's (an
    /// `olr_online::OnlinePeer`'s), built from `start` by every peer;
    /// `my_slot` is the worm this player plays (`None`: watching).
    Online { start: Box<MatchStartWire>, my_slot: Option<u8> },
}

/// The human's name and colour (the menus' profile), for its slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerLook {
    pub name: String,
    pub colour: [u8; 3],
}

/// Everything a session draws and plays, loaded once: a restart (F5)
/// builds a new world from it without decoding any image or sound again.
pub struct Assets {
    /// What it was loaded from (an online match's: its peer's content,
    /// downloads laid over; shared, never copied).
    pub content: Arc<Content>,
    pub m: LoadedMod,
    pub level: LoadedLevel,
    pub theme: LoadedTheme,
    /// The human's skin (its slot's when a human plays).
    pub skin: LoadedSkin,
    /// The pack id and content hash of [`Assets::skin`], as a replay
    /// records them (the default skin and `NO_SKIN_HASH` when the profile's
    /// skin is no skin pack here).
    pub skin_id: String,
    pub skin_hash: String,
    /// Everyone else's: OLX's default skin, as OLX's bot profiles have
    /// (`src/client/ProfileSystem.cpp:59`).
    pub bot_skin: LoadedSkin,
    pub game: Game,
    /// The mod and level pack ids.
    pub mod_id: String,
    pub level_id: String,
    /// Every warning of the loading, to log once.
    pub warnings: Vec<String>,
    /// The human's look (a profile's); `None`: "You" in the default colour.
    pub player: Option<PlayerLook>,
    /// An online match's or a replay's skin of every slot, by slot (two
    /// slots may wear skins of one id and different content): the local
    /// player's by its id, every other (and every worm of a replay, whose
    /// viewer has no own slot) by id and content hash
    /// (`Content::skin_matching`), the default skin when it does not
    /// match. Empty in every other game.
    pub slot_skins: Vec<LoadedSkin>,
    /// The level editor's Play (`load_edited`): its replay is never saved.
    pub edited: bool,
}

/// The packs `mod_id`, `level_id` (with its theme) and skin `skin` (a
/// name or id; the default skin when missing), resolved and loaded.
fn open(
    content: Arc<Content>,
    mod_id: String,
    level_id: String,
    skin: &str,
    game: Game,
) -> anyhow::Result<Assets> {
    let lvl = content.load_level(&level_id)?;
    open_level(content, mod_id, level_id, lvl, skin, game)
}

/// [`open`] with the level given (the editor's, loaded from no pack).
fn open_level(
    content: Arc<Content>,
    mod_id: String,
    level_id: String,
    lvl: LoadedLevel,
    skin: &str,
    game: Game,
) -> anyhow::Result<Assets> {
    let skin_id = content.skin_id(skin).unwrap_or_else(|| skin.to_string());
    let m = content.load_mod(&mod_id)?;
    let theme = content.load_theme(&lvl.theme).with_context(|| format!("level {level_id}"))?;
    let skin = content.skin_or_default(&skin_id);
    let (skin_id, skin_hash) = match content.pack_kind(&skin_id) {
        Some(PackKind::Skin) => match content.pack_hash(&skin_id, PackKind::Skin) {
            Ok(h) => (skin_id.clone(), h),
            Err(_) => (DEFAULT_SKIN.to_string(), olr_content::NO_SKIN_HASH.to_string()),
        },
        _ => (DEFAULT_SKIN.to_string(), olr_content::NO_SKIN_HASH.to_string()),
    };
    let bot_skin = if skin_id == DEFAULT_SKIN { skin.clone() } else { content.skin_or_default(DEFAULT_SKIN) };
    let mut warnings = content.warnings().to_vec();
    let loaded = m.warnings.iter().chain(&theme.warnings).chain(&skin.warnings).chain(&bot_skin.warnings);
    warnings.extend(loaded.cloned());
    Ok(Assets {
        content,
        m,
        level: lvl,
        theme,
        skin,
        skin_id,
        skin_hash,
        bot_skin,
        game,
        mod_id,
        level_id,
        warnings,
        player: None,
        slot_skins: Vec::new(),
        edited: false,
    })
}

/// The mod and level `--mod` and `--level` name in `content`.
fn resolve(content: &Content, mod_name: &str, level: &str) -> anyhow::Result<(String, String)> {
    let mod_id = resolve_id(mod_name, "olx-", &content.ids(PackKind::Mod)).context("mod")?;
    let level_id = resolve_id(level, "olx-level-", &content.ids(PackKind::Level)).context("level")?;
    Ok((mod_id, level_id))
}

/// The sandbox on the packs `--mod`, `--level` and `--skin` name in
/// content directories `roots` (layered: `Content::open_layers`).
pub fn load(
    roots: &[&Path],
    mod_name: &str,
    level: &str,
    skin: &str,
    o: SandboxOptions,
) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let (mod_id, level_id) = resolve(&content, mod_name, level)?;
    let setup = sandbox_setup(&SandboxOptions { mod_id: mod_id.clone(), level_id: level_id.clone(), ..o });
    open(Arc::new(content), mod_id, level_id, skin, Game::Sandbox(setup))
}

/// A Deathmatch against bots on the packs `--mod` and `--level` name.
pub fn load_deathmatch(
    roots: &[&Path],
    mod_name: &str,
    level: &str,
    skin: &str,
    o: &DeathmatchOptions,
) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let (mod_id, level_id) = resolve(&content, mod_name, level)?;
    let packs = content.replay_packs(&mod_id, &level_id)?;
    let options = Box::new(match_options(o, &mod_id, &level_id));
    open(Arc::new(content), mod_id, level_id, skin, Game::Deathmatch { options, packs, spawn: None })
}

/// The Deathmatch `o` describes (the menus' local play), its packs by id.
/// A missing or broken pack is an error naming it.
pub fn load_match(roots: &[&Path], o: &MatchOptions, skin: &str) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let packs = content.replay_packs(&o.mod_id, &o.level_id)?;
    let options = Box::new(o.clone());
    let game = Game::Deathmatch { options, packs, spawn: None };
    open(Arc::new(content), o.mod_id.clone(), o.level_id.clone(), skin, game)
}

/// What the level editor's Play asks for (plan 14c R3): the edited level
/// itself (no pack), the mod by pack id, and 0 to `olr_editor::MAX_BOTS`
/// bots.
#[derive(Clone, Debug, PartialEq)]
pub struct EditedPlay {
    pub level: LevelSource,
    pub mod_id: String,
    pub bots: u8,
}

/// The level id an edited play's assets carry (the level is the editor's,
/// in no pack): for messages; `Assets::edited` marks the play.
pub const EDITED_LEVEL: &str = "edited level";

/// The editor's Play (plan 14c R3, R4) on the edited level: the sandbox
/// for 0 bots (the player alone, no targets), else a Deathmatch against
/// that many bots (capped at `olr_editor::MAX_BOTS`) of the Local Play
/// screen's saved level, personality and loadout (`prefs`), under OLX's
/// Deathmatch defaults. The human worm first spawns at the level's first
/// spawn when it lies inside the level and out of rock
/// (`LoadedLevel::first_spawn`), else at a free spot. The mod, theme,
/// core and skin `skin` come by id from `roots`. Its replay is never
/// saved (the shell's `Launch::Edited`), so its level pack ref has no hash.
pub fn load_edited(
    roots: &[&Path],
    play: &EditedPlay,
    skin: &str,
    prefs: &olr_ui::MatchPrefs,
) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let level = LoadedLevel::from_source(&play.level).context("the edited level")?;
    let spawn = level.first_spawn();
    let (mod_id, level_id) = (play.mod_id.clone(), EDITED_LEVEL.to_string());
    let bots = play.bots.min(olr_editor::MAX_BOTS);
    let game = if bots == 0 {
        let o = SandboxOptions {
            mod_id: mod_id.clone(),
            level_id: level_id.clone(),
            seed: 1,
            targets: 0,
            bonuses: prefs.bonuses,
        };
        let mut setup = sandbox_setup(&o);
        setup.worms[0].pos = spawn;
        Game::Sandbox(setup)
    } else {
        let p = olr_ui::MatchPrefs {
            mod_id: mod_id.clone(),
            level_id: level_id.clone(),
            bots,
            per_bot: None,
            rules: olr_ui::MatchPrefs::default().rules,
            restrictions: olr_content::DEFAULT_PRESET.into(),
            ..prefs.clone()
        };
        let r = |id: &str| olr_replay::PackRef { id: id.into(), hash: String::new() };
        let packs = Packs { mod_pack: r(&mod_id), level: r(&level_id), theme: r(&level.theme) };
        Game::Deathmatch { options: Box::new(p.options(1)), packs, spawn }
    };
    let mut a = open_level(Arc::new(content), mod_id, level_id, level, skin, game)?;
    a.edited = true;
    Ok(a)
}

/// The online match `start` describes (`olr_online`'s `MatchStarting`),
/// played as slot `my_slot` (`None`: watching), from `content`, the peer's
/// (`OnlinePeer::content`: this install's packs with the downloads laid
/// over, the match's world was built from it): its mod, level and theme,
/// and every slot's skin (spec §4, plan 13c): the local player's by its
/// id, every other slot's by id and content hash, so a skin this install
/// lacks or has with other content is the default skin, with a warning
/// (each said once; skins are never downloaded). The peer already checked
/// the packs.
pub fn load_online(
    content: Arc<Content>,
    start: &MatchStartWire,
    my_slot: Option<u8>,
) -> anyhow::Result<Assets> {
    let s = &start.setup;
    let mine = my_slot.and_then(|i| start.slots.get(usize::from(i))).map_or(DEFAULT_SKIN, |w| &w.look.skin);
    let game = Game::Online { start: Box::new(start.clone()), my_slot };
    let mut a = open(content, s.mod_id.clone(), s.level_id.clone(), mine, game)?;
    for (i, w) in start.slots.iter().enumerate() {
        let skin = if my_slot.is_some_and(|m| usize::from(m) == i) {
            a.skin.clone()
        } else {
            slot_skin(&mut a, &w.look.name, &w.look.skin, &w.look.skin_hash)
        };
        a.slot_skins.push(skin);
    }
    Ok(a)
}

/// The skin of a worm that is not the local player's (`Content::skin_matching`:
/// the exact id and content hash, else the default skin); what it said
/// about `who`'s skin goes to `a.warnings`, each line once.
fn slot_skin(a: &mut Assets, who: &str, id: &str, hash: &str) -> LoadedSkin {
    let (skin, why) = a.content.skin_matching(id, hash);
    let said: Vec<String> = why
        .into_iter()
        .map(|why| format!("{who}'s skin: {why}"))
        .chain(skin.warnings.iter().cloned())
        .collect();
    for line in said {
        if !a.warnings.contains(&line) {
            a.warnings.push(line);
        }
    }
    skin
}

/// The largest replay file read: [`olr_replay::MAX_TICKS`] ticks of
/// [`MAX_WORMS`] worms' keys (a byte each, 23 MB; deflated, a little more
/// at worst) twice over, plus 16 MiB for the header, setup and checksums.
pub const MAX_REPLAY_BYTES: u64 = 2 * olr_replay::MAX_TICKS as u64 * MAX_WORMS as u64 + (16 << 20);

/// The bytes of the replay file `file` (a path the player typed): a
/// regular file only (a FIFO or a device is refused before it is opened,
/// so it never blocks the UI thread), its header checked before the rest
/// is read, at most [`MAX_REPLAY_BYTES`].
pub fn read_replay_file(file: &Path) -> anyhow::Result<Vec<u8>> {
    use std::io::Read as _;
    let meta = std::fs::metadata(file)?;
    anyhow::ensure!(meta.is_file(), "not a regular file");
    let mb = |n: u64| n.div_ceil(1_000_000);
    let too_large =
        |n| anyhow::anyhow!("{} MB is too large for a replay (at most {} MB)", mb(n), mb(MAX_REPLAY_BYTES));
    if meta.len() > MAX_REPLAY_BYTES {
        return Err(too_large(meta.len()));
    }
    let f = std::fs::File::open(file)?;
    anyhow::ensure!(f.metadata()?.is_file(), "not a regular file");
    let mut bytes = Vec::with_capacity(olr_replay::HEADER_BYTES);
    (&f).take(olr_replay::HEADER_BYTES as u64).read_to_end(&mut bytes)?;
    Replay::check_header(&bytes)?;
    bytes.reserve(usize::try_from(meta.len()).unwrap_or(0));
    // It may grow after its length was read: never past the limit.
    (&f).take(MAX_REPLAY_BYTES + 1 - bytes.len() as u64).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REPLAY_BYTES {
        return Err(too_large(bytes.len() as u64));
    }
    Ok(bytes)
}

/// `content` with the packs of `packs` it lacks (or has with another
/// content hash) laid over from the download caches in `cache_dirs` by
/// content hash (`Content::with_packs`; plan 11): a replay of a match on
/// downloaded content plays back. The caches are read without opening
/// them (`DownloadCache::lookup`: no lock, nothing written), so a cache
/// another game holds counts, and a command-line replay never creates or
/// locks one. A pack in none stays missing (the caller's
/// `check_replay_packs` names it).
pub fn with_cached_packs(content: Content, packs: &Packs, cache_dirs: &[PathBuf]) -> anyhow::Result<Content> {
    let mut laid = Vec::new();
    for r in [&packs.mod_pack, &packs.level, &packs.theme] {
        if content.has(&r.id, &r.hash) {
            continue;
        }
        if let Some(dir) = cache_dirs.iter().find_map(|d| DownloadCache::lookup(d, &r.hash)) {
            laid.push((r.id.clone(), dir));
        }
    }
    if laid.is_empty() {
        return Ok(content);
    }
    // Hashed again by `check_replay_packs`: a cached pack changed on disk
    // since its install is refused like a local one.
    content.with_packs(&laid).context("the downloaded packs")
}

/// The replay in file `file` (read by [`read_replay_file`]), on the packs
/// it names: this install's, else the download caches' in `cache_dirs` by
/// content hash ([`with_cached_packs`]); refused when they are missing or
/// differ from the recorded ones.
pub fn load_replay(roots: &[&Path], file: &Path, cache_dirs: &[PathBuf]) -> anyhow::Result<Assets> {
    let bytes = read_replay_file(file).with_context(|| format!("{}", file.display()))?;
    let replay = Replay::decode(&bytes).with_context(|| format!("{}", file.display()))?;
    let content = Content::open_layers(roots)?;
    let content = with_cached_packs(content, &replay.setup.packs, cache_dirs)?;
    content.check_replay_packs(&replay.setup).with_context(|| format!("{}", file.display()))?;
    let p = &replay.setup.packs;
    let (mod_id, level_id) = (p.mod_pack.id.clone(), p.level.id.clone());
    anyhow::ensure!(
        content.load_level(&level_id)?.theme == p.theme.id,
        "{}: level {level_id} has another theme than the recorded {}",
        file.display(),
        p.theme.id
    );
    let looks = replay.setup.looks.clone();
    let mut a = open(Arc::new(content), mod_id, level_id, DEFAULT_SKIN, Game::Replay(Box::new(replay)))?;
    // A replay's viewer has no own slot: every worm's skin is its recorded
    // id and content hash, else the default skin (never downloaded).
    for l in &looks {
        let skin = slot_skin(&mut a, &l.name, &l.skin, &l.skin_hash);
        a.slot_skins.push(skin);
    }
    Ok(a)
}

impl Assets {
    /// A new world of the sandbox, from the assets loaded (an error for
    /// another game).
    pub fn new_world(&self) -> anyhow::Result<olr_sim::World> {
        let Game::Sandbox(setup) = &self.game else { anyhow::bail!("not a sandbox") };
        olr_content::new_world_from(&self.m, &self.level, &self.theme, setup)
    }

    /// Whether this is the level editor's Play (`load_edited`), whose
    /// replay is never saved.
    pub fn edited(&self) -> bool {
        self.edited
    }

    /// Whether a person plays the game: the sandbox, a Deathmatch with a
    /// human; not a replay, nor a match of bots alone. Only a game nobody
    /// plays may run fast (`speed.rs`), so an online match counts as
    /// played even when watched: it runs at the host's pace.
    pub fn human_plays(&self) -> bool {
        match &self.game {
            Game::Online { .. } => true,
            _ => self.local_slot().is_some(),
        }
    }

    /// The worm this player plays: the sandbox's 0, a Deathmatch's first
    /// that is not a bot, an online match's own slot; `None` for a replay,
    /// bots alone and an online match watched.
    pub fn local_slot(&self) -> Option<u8> {
        match &self.game {
            Game::Sandbox(_) => Some(0),
            Game::Deathmatch { options, .. } => {
                options.players.iter().position(|p| !matches!(p, Player::Bot(_))).map(|i| i as u8)
            }
            Game::Replay(_) => None,
            Game::Online { my_slot, .. } => *my_slot,
        }
    }

    /// The worm the camera starts on: the player's own, else (a replay)
    /// the worm of the player whose game made it, else worm 0.
    pub fn first_followed(&self) -> u8 {
        match &self.game {
            Game::Replay(r) if self.local_slot().is_none() => r.setup.viewer.unwrap_or(0),
            _ => self.local_slot().unwrap_or(0),
        }
    }

    /// The skin of online slot `slot`, else the default skin.
    pub fn skin_of_slot(&self, slot: usize) -> &LoadedSkin {
        self.slot_skins.get(slot).unwrap_or(&self.bot_skin)
    }

    /// How many worms the game starts with.
    pub fn worms(&self) -> usize {
        match &self.game {
            Game::Sandbox(s) => s.worms.len(),
            Game::Deathmatch { options, .. } => options.players.len(),
            Game::Replay(r) => r.setup.worms.len(),
            Game::Online { start, .. } => start.slots.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default skin's name (OLX's "default").
    const DEFAULT_NAME: &str = "default";

    fn options(targets: usize) -> SandboxOptions {
        SandboxOptions {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-liero-factory".into(),
            seed: 1,
            targets,
            bonuses: false,
        }
    }

    fn slot(name: &str, skin: &str, hash: &str) -> olr_proto::SlotWire {
        olr_proto::SlotWire {
            member: None,
            bot: None,
            look: olr_proto::Look {
                name: name.into(),
                colour: [1, 2, 3],
                skin: skin.into(),
                skin_hash: hash.into(),
            },
            loadout: None,
        }
    }

    /// Plan 14c R3, R4: the editor's Play, a sandbox for 0 bots, else a
    /// Deathmatch; the human worm first spawns at the level's first spawn
    /// (a free spot without one).
    #[test]
    fn edited_play_builds_a_sandbox_or_a_match() {
        use olr_format::levels::LevelObject;
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let root = Path::new(&dir);
        let fs = olr_vfs::DirFs::new(root.join("olx-level-dirt-level"));
        let mut level = olr_format::levels::read_level_files(&fs).unwrap();
        // A background cell with room around it.
        let (w, h) = (level.size.0 as i32, level.size.1 as i32);
        let open = |x: i32, y: i32| {
            (y - 6..y + 6).all(|yy| {
                (x - 6..x + 6).all(|xx| {
                    (0..w).contains(&xx)
                        && (0..h).contains(&yy)
                        && level.material[(yy * w + xx) as usize] == olr_data::level::Material::Background
                })
            })
        };
        let at = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).find(|&(x, y)| open(x, y)).unwrap();
        level.objects = vec![LevelObject::Spawn { at, notes: String::new() }];
        let play = |level: &olr_format::levels::LevelSource, bots| EditedPlay {
            level: level.clone(),
            mod_id: "olx-classic".into(),
            bots,
        };
        let prefs = olr_ui::MatchPrefs::default();
        let spawned = |a: &Assets| {
            let vp = olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
            let mut s = crate::session::Session::new(
                a,
                vp,
                olr_scene::Interpolation::Linear,
                olr_audio::Audio::silent(),
            )
            .unwrap();
            s.tick(olr_sim::Keys(0));
            let p = s.world.worms()[0].pos;
            (p.x as i32, p.y as i32)
        };

        let a = load_edited(&[root], &play(&level, 0), DEFAULT_SKIN, &prefs).unwrap();
        let Game::Sandbox(setup) = &a.game else { panic!("0 bots is the sandbox") };
        assert_eq!(setup.worms.len(), 1, "no targets");
        assert!(a.edited(), "its replay is never saved");
        let o = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: 1,
            targets: 0,
            bonuses: false,
        };
        assert!(
            !load(&[root], "Classic", "Dirt Level", DEFAULT_SKIN, o).unwrap().edited(),
            "a menu sandbox is not"
        );
        assert_eq!(setup.worms[0].pos, Some(at));
        let p = spawned(&a);
        assert!((p.0 - at.0).abs() <= 2 && (p.1 - at.1).abs() <= 2, "{p:?} near {at:?}");

        let a = load_edited(&[root], &play(&level, 3), DEFAULT_SKIN, &prefs).unwrap();
        let Game::Deathmatch { options, spawn, .. } = &a.game else { panic!("bots: a Deathmatch") };
        assert_eq!(options.players.len(), 4);
        assert!(matches!(options.players[0], Player::Human));
        assert_eq!(*spawn, Some(at));
        let p = spawned(&a);
        assert!((p.0 - at.0).abs() <= 2 && (p.1 - at.1).abs() <= 2, "{p:?} near {at:?}");

        level.objects.clear();
        let a = load_edited(&[root], &play(&level, 0), DEFAULT_SKIN, &prefs).unwrap();
        let Game::Sandbox(setup) = &a.game else { panic!() };
        assert_eq!(setup.worms[0].pos, None);
        let a = load_edited(&[root], &play(&level, 2), DEFAULT_SKIN, &prefs).unwrap();
        let Game::Deathmatch { spawn, .. } = &a.game else { panic!() };
        assert_eq!(*spawn, None);
        // More bots than Play allows are capped; a missing mod is an error.
        let a = load_edited(&[root], &play(&level, 200), DEFAULT_SKIN, &prefs).unwrap();
        assert_eq!(a.worms(), 1 + usize::from(olr_editor::MAX_BOTS));
        let bad = EditedPlay { mod_id: "no-such-mod".into(), ..play(&level, 0) };
        assert!(load_edited(&[root], &bad, DEFAULT_SKIN, &prefs).is_err());
    }

    /// The skin constants olr-content and olr-proto/olr-game each define
    /// agree (no content needed).
    #[test]
    fn the_skin_constants_agree() {
        assert_eq!(olr_content::NO_SKIN_HASH, olr_proto::NO_HASH);
        assert_eq!(olr_content::DEFAULT_SKIN, DEFAULT_SKIN);
    }

    /// Plan 13c, ruling 9: online skins by slot, id and hash. Two slots
    /// wearing one id with different hashes do not collide: the one whose
    /// hash is this install's gets the skin, the other the default skin
    /// (never the local pack of that name); the local player's own slot
    /// uses its skin by id; a bot's `NO_HASH` gets only the default skin;
    /// the warning is said once.
    #[test]
    fn online_skins_are_matched_by_id_and_hash_per_slot() {
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let content = Arc::new(Content::open(Path::new(&dir)).unwrap());
        let ant = "olx-skin-ant";
        let ant_hash = content.pack_hash(ant, PackKind::Skin).unwrap();
        let other = "ab".repeat(32);
        assert_ne!(ant_hash, other);
        let setup = olr_proto::MatchSetupWire {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-dirt-level".into(),
            packs: vec![],
            rules: olr_proto::RulesWire {
                lives: 10,
                kill_limit: 0,
                time_limit_min: 0.0,
                allow_negative_score: false,
                death_decreases_score: 0.0,
                suicide_decreases_score: false,
                sprees: [0; 5],
                dying_sprees: [0; 5],
            },
            bonuses: false,
            loading_time: 100,
            restrictions: String::new(),
            bots: vec![],
        };
        let start = MatchStartWire {
            seed: 1,
            setup,
            slots: vec![
                slot("Anna", ant, &ant_hash),
                slot("Bert", ant, &other),
                slot("Cleo", ant, &ant_hash),
                slot("Bot", ant, olr_proto::NO_HASH),
                slot("Dan", "olx-skin-nobody", &other),
                // A second Bert with the same mismatch: the warning's
                // deduplication is what keeps it said once.
                slot("Bert", ant, &other),
            ],
            input_delay: vec![],
            start_checksum: 0,
        };
        let name = |a: &Assets, i: usize| a.skin_of_slot(i).skin.def.name.clone();
        // Watching: everyone by id and hash.
        let a = load_online(Arc::clone(&content), &start, None).unwrap();
        assert_eq!(a.slot_skins.len(), 6);
        let got: Vec<String> = (0..6).map(|i| name(&a, i)).collect();
        assert_eq!(got, ["Ant", DEFAULT_NAME, "Ant", DEFAULT_NAME, DEFAULT_NAME, DEFAULT_NAME], "{got:?}");
        assert_eq!(a.warnings.iter().filter(|w| w.contains("Bert's skin")).count(), 1, "{:?}", a.warnings);
        // Bert plays slot 1: his own skin by id, whatever his hash says.
        let a = load_online(Arc::clone(&content), &start, Some(1)).unwrap();
        let got: Vec<String> = (0..6).map(|i| name(&a, i)).collect();
        assert_eq!(got, ["Ant", "Ant", "Ant", DEFAULT_NAME, DEFAULT_NAME, DEFAULT_NAME], "{got:?}");
        // The other Bert (slot 5) still warns, once.
        assert_eq!(a.warnings.iter().filter(|w| w.contains("Bert's skin")).count(), 1, "{:?}", a.warnings);
        assert!(a.warnings.iter().any(|w| w.contains("Bot's skin")), "{:?}", a.warnings);
    }

    #[test]
    fn sandbox_setup_spawns_targets() {
        let s = sandbox_setup(&options(3));
        assert_eq!(s.worms.len(), 4, "the player and 3 targets");
        assert!(
            s.worms.iter().all(|w| w.pos.is_none() && w.weapons.is_empty()),
            "FindSpot, first five weapons"
        );
        assert_eq!(s.settings.lives, SimSettings::lx56().lives);
        assert_eq!(sandbox_setup(&options(0)).worms.len(), 1);
        assert_eq!(sandbox_setup(&options(1000)).worms.len(), MAX_WORMS);
        assert!(sandbox_setup(&SandboxOptions { bonuses: true, ..options(0) }).settings.bonuses);

        // On every imported level and mod, when there is content: every
        // worm alive on its own free spot.
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("OLR_CONTENT_DIR not set; skipping the imported levels");
            return;
        };
        for m in ["Classic", "MW 1.0"] {
            for level in ["Liero Factory", "CastleStrike", "FossilFacility", "Dirt Level"] {
                let a = load(&[Path::new(&dir)], m, level, "default", options(3)).unwrap();
                let world = a.new_world().unwrap();
                assert_eq!(world.worms().len(), 4, "{m} on {level}");
                assert!(world.worms().iter().all(|w| w.alive && w.slots.len() == 5), "{m} on {level}");
                let mut spots: Vec<_> =
                    world.worms().iter().map(|w| (w.pos.x as i32, w.pos.y as i32)).collect();
                spots.sort_unstable();
                spots.dedup();
                assert_eq!(spots.len(), 4, "{m} on {level}: {spots:?}");
                assert_eq!(a.new_world().unwrap(), world, "a restart is the same world");
            }
        }
    }

    #[test]
    fn deathmatch_options_put_the_human_first() {
        let o = DeathmatchOptions {
            bots: 4,
            difficulty: Difficulty::Hard,
            time_limit: 1.5,
            ..Default::default()
        };
        let m = match_options(&o, "olx-classic", "olx-level-dirt-level");
        assert_eq!(m.players.len(), 5);
        assert_eq!(m.players[0], Player::Human);
        assert!(
            m.players[1..].iter().all(|p| matches!(p, Player::Bot(c) if c.difficulty == Difficulty::Hard))
        );
        assert_eq!((m.deathmatch.kill_limit, m.deathmatch.time_limit, m.deathmatch.lives), (15, 1.5, -2));
        assert_eq!(m.restrictions, olr_content::DEFAULT_PRESET);
        let many = match_options(&DeathmatchOptions { bots: 100, ..Default::default() }, "m", "l");
        assert_eq!(many.players.len(), MAX_WORMS);
        let none = match_options(&DeathmatchOptions { bots: 0, ..Default::default() }, "m", "l");
        assert_eq!(none.players.len(), 2, "a Deathmatch needs two worms (F5)");
        // Spectating: bots alone, 2 to MAX_WORMS of them.
        let watch =
            |bots| match_options(&DeathmatchOptions { bots, spectate: true, ..Default::default() }, "m", "l");
        assert!(watch(4).players.iter().all(|p| matches!(p, Player::Bot(_))));
        assert_eq!(watch(4).players.len(), 4);
        assert_eq!(watch(1).players.len(), 2);
        assert_eq!(watch(100).players.len(), MAX_WORMS);
    }

    #[test]
    fn pack_ids_resolve_from_olx_names() {
        let ids: Vec<String> = ["olx-classic", "olx-mw-1-0"].map(String::from).to_vec();
        assert_eq!(resolve_id("olx-classic", "olx-", &ids).unwrap(), "olx-classic");
        assert_eq!(resolve_id("Classic", "olx-", &ids).unwrap(), "olx-classic");
        assert_eq!(resolve_id("MW 1.0", "olx-", &ids).unwrap(), "olx-mw-1-0");
        let levels = vec!["olx-level-liero-factory".to_string()];
        assert_eq!(
            resolve_id("Liero Factory.lxl", "olx-level-", &levels).unwrap(),
            "olx-level-liero-factory"
        );
        assert!(resolve_id("nope", "olx-", &ids).unwrap_err().to_string().contains("olx-classic"));
    }

    /// `load_replay` in a thread: its error, or `None` when it did not
    /// return within 10 s (a FIFO would block the UI thread forever).
    fn load_replay_soon(file: &Path) -> Option<String> {
        let file = file.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = load_replay(&[], &file, &[]).err().map(|e| format!("{e:#}"));
            let _ = tx.send(r.unwrap_or_default());
        });
        rx.recv_timeout(std::time::Duration::from_secs(10)).ok()
    }

    /// A typed replay path is read bounded: a regular file only (a FIFO
    /// never blocks), its header checked first, at most
    /// `MAX_REPLAY_BYTES`, each refusal saying why.
    #[test]
    fn replay_files_are_read_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let e = load_replay_soon(d).expect("a directory returns");
        assert!(e.contains("not a regular file"), "{e}");
        std::fs::write(d.join("text.olrp"), b"hello, not a replay at all").unwrap();
        let e = load_replay_soon(&d.join("text.olrp")).unwrap();
        assert!(e.contains("not a replay"), "{e}");
        // A file with a good header but far too large: refused by its
        // length, unread (sparse, so the test writes nothing).
        let big = d.join("big.olrp");
        let mut head = olr_replay::MAGIC.to_vec();
        head.extend_from_slice(&olr_replay::FORMAT_VERSION.to_le_bytes());
        std::fs::write(&big, &head).unwrap();
        std::fs::OpenOptions::new().write(true).open(&big).unwrap().set_len(MAX_REPLAY_BYTES + 1).unwrap();
        let e = load_replay_soon(&big).unwrap();
        assert!(e.contains("too large"), "{e}");
        assert!(MAX_REPLAY_BYTES >= 2 * u64::from(olr_replay::MAX_TICKS) * MAX_WORMS as u64);
        // A good header, the size allowed: read, then refused by its body.
        std::fs::write(d.join("short.olrp"), &head).unwrap();
        let e = load_replay_soon(&d.join("short.olrp")).unwrap();
        assert!(e.contains("malformed"), "{e}");
        #[cfg(unix)]
        {
            let fifo = d.join("fifo.olrp");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            if made.is_ok_and(|s| s.success()) {
                let e = load_replay_soon(&fifo).expect("a FIFO never blocks the read");
                assert!(e.contains("not a regular file"), "{e}");
            } else {
                eprintln!("mkfifo unavailable; skipping the FIFO");
            }
        }
    }
}
