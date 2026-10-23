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
    /// OLX's `Bonuses` option: bonuses spawn and fall. Only the tests turn
    /// it on (the soak): bonuses are gone from play (issue #5).
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
    /// No human plays: `bots` bots (2 to `MAX_WORMS`) alone, watched
    /// (`--spectate`; fast-forward allowed, `speed.rs`).
    pub spectate: bool,
    /// Local human players (`--locals`, issue #29): 1 to [`MAX_LOCALS`],
    /// worms 0 to `locals - 1`, before the bots.
    pub locals: usize,
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
            spectate: false,
            locals: 1,
        }
    }
}

/// The match [`DeathmatchOptions`] describe on packs `mod_id` and
/// `level_id`: the humans are worms 0 to `locals - 1` (one, or up to
/// [`MAX_LOCALS`] in split screen, issue #29), then the bots (OLX's bots at
/// one difficulty), under OLX's LX56 settings, the `Default` loadout (every
/// weapon at 100 %: issue #5) and the Deathmatch rules given.
/// The bots are capped so the worms fit `MAX_WORMS`, and number at least
/// one against a lone human (a Deathmatch needs two worms). Spectating, the
/// bots play alone (2 to `MAX_WORMS` of them).
pub fn match_options(o: &DeathmatchOptions, mod_id: &str, level_id: &str) -> MatchOptions {
    let bot = BotConfig { difficulty: o.difficulty, ..BotConfig::default() };
    let players: Vec<Player> = if o.spectate {
        vec![Player::Bot(bot); o.bots.clamp(2, MAX_WORMS)]
    } else {
        let locals = o.locals.clamp(1, MAX_LOCALS);
        let bots = o.bots.clamp(2usize.saturating_sub(locals), MAX_WORMS - locals);
        let humans = std::iter::repeat_n(Player::Human, locals);
        humans.chain((0..bots).map(|_| Player::Bot(bot))).collect()
    };
    MatchOptions {
        mod_id: mod_id.into(),
        level_id: level_id.into(),
        seed: o.seed,
        settings: SimSettings::lx56(),
        players,
        deathmatch: DeathmatchSettings {
            lives: o.lives,
            kill_limit: o.kill_limit,
            time_limit: o.time_limit,
            ..DeathmatchSettings::default()
        },
        weapons: olr_content::WeaponChoice::default_loadout(),
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

/// The most local players (split screen, issue #29): one view each
/// (`olr_scene::MAX_VIEWS`), as many local slots as a replay keeps.
pub const MAX_LOCALS: usize = olr_scene::MAX_VIEWS;

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

/// One local player's look and skin (issue #29: several play one
/// machine's match, each its own).
#[derive(Clone, Debug)]
pub struct LocalPlayer {
    /// Its name and colour (a profile's); `None`: "You" when it plays
    /// alone, "Player N" among several, in the default colours
    /// (`olr_present::default_colour` of its local number).
    pub look: Option<PlayerLook>,
    pub skin: LoadedSkin,
    /// The pack id and content hash of `skin`, as a replay records them
    /// (the default skin and `NO_SKIN_HASH` when the skin asked for is no
    /// skin pack here).
    pub skin_id: String,
    pub skin_hash: String,
}

impl LocalPlayer {
    /// The player wearing skin `skin` (a name or id; the default skin when
    /// missing) of `content`, without a look.
    fn wearing(content: &Content, skin: &str) -> LocalPlayer {
        let skin_id = content.skin_id(skin).unwrap_or_else(|| skin.to_string());
        let loaded = content.skin_or_default(&skin_id);
        let (skin_id, skin_hash) = match content.pack_kind(&skin_id) {
            Some(PackKind::Skin) => match content.pack_hash(&skin_id, PackKind::Skin) {
                Ok(h) => (skin_id.clone(), h),
                Err(_) => (DEFAULT_SKIN.to_string(), olr_content::NO_SKIN_HASH.to_string()),
            },
            _ => (DEFAULT_SKIN.to_string(), olr_content::NO_SKIN_HASH.to_string()),
        };
        LocalPlayer { look: None, skin: loaded, skin_id, skin_hash }
    }
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
    /// The local players' looks and skins, in local order (the order of
    /// [`Assets::local_slots`]; issue #29): player `k` plays local slot
    /// `k`. One in a solo game (its look the menus' profile); none for a
    /// replay or an online match watched. A local slot without one plays
    /// in the default look and skin.
    pub locals: Vec<LocalPlayer>,
    /// Everyone else's: OLX's default skin, as OLX's bot profiles have
    /// (`src/client/ProfileSystem.cpp:59`).
    pub bot_skin: LoadedSkin,
    pub game: Game,
    /// The mod and level pack ids.
    pub mod_id: String,
    pub level_id: String,
    /// Every warning of the loading, to log once.
    pub warnings: Vec<String>,
    /// An online match's or a replay's skin of every slot, by slot (two
    /// slots may wear skins of one id and different content): the local
    /// player's by its id, every other (and every worm of a replay, whose
    /// viewer has no own slot) by id and content hash
    /// (`Content::skin_matching`), the default skin when it does not
    /// match. Empty in every other game.
    pub slot_skins: Vec<LoadedSkin>,
    /// An editor's Play (`load_edited`): its replay is never saved.
    pub edited: bool,
    /// The mod editor's practice with one weapon (issue #20): the
    /// targets' health bars and the last hit's damage show.
    pub practice: bool,
}

/// The packs `mod_id`, `level_id` (with its theme) and the local players'
/// skins `skins` (each a name or id; the default skin when missing),
/// resolved and loaded.
fn open(
    content: Arc<Content>,
    mod_id: String,
    level_id: String,
    skins: &[&str],
    game: Game,
) -> anyhow::Result<Assets> {
    let lvl = content.load_level(&level_id)?;
    let m = content.load_mod(&mod_id)?;
    open_loaded(content, (mod_id, m), level_id, lvl, skins, game)
}

/// [`open`] with the mod and level given (an editor's Play: the level
/// editor's level, the mod editor's mod, loaded from no pack).
fn open_loaded(
    content: Arc<Content>,
    (mod_id, m): (String, LoadedMod),
    level_id: String,
    lvl: LoadedLevel,
    skins: &[&str],
    game: Game,
) -> anyhow::Result<Assets> {
    let theme = content.load_theme(&lvl.theme).with_context(|| format!("level {level_id}"))?;
    let locals: Vec<LocalPlayer> =
        skins.iter().take(MAX_LOCALS).map(|skin| LocalPlayer::wearing(&content, skin)).collect();
    let bot_skin = match locals.iter().find(|l| l.skin_id == DEFAULT_SKIN) {
        Some(l) => l.skin.clone(),
        None => content.skin_or_default(DEFAULT_SKIN),
    };
    let mut warnings: Vec<String> = content.warnings().to_vec();
    let loaded = m.warnings.iter().chain(&theme.warnings).chain(&bot_skin.warnings);
    for w in loaded.chain(locals.iter().flat_map(|l| &l.skin.warnings)) {
        if !warnings.contains(w) {
            warnings.push(w.clone());
        }
    }
    Ok(Assets {
        content,
        m,
        level: lvl,
        theme,
        locals,
        bot_skin,
        game,
        mod_id,
        level_id,
        warnings,
        slot_skins: Vec::new(),
        edited: false,
        practice: false,
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
    open(Arc::new(content), mod_id, level_id, &[skin], Game::Sandbox(setup))
}

/// A Deathmatch against bots on the packs `--mod` and `--level` name,
/// every local player (`--locals`) in skin `skin`.
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
    let humans = options.players.iter().filter(|p| !matches!(p, Player::Bot(_))).count();
    let skins = vec![skin; humans];
    open(Arc::new(content), mod_id, level_id, &skins, Game::Deathmatch { options, packs, spawn: None })
}

/// The Deathmatch `o` describes (the menus' local play), its packs by id,
/// its local player in skin `skin`. A missing or broken pack is an error
/// naming it.
pub fn load_match(roots: &[&Path], o: &MatchOptions, skin: &str) -> anyhow::Result<Assets> {
    load_match_for(roots, o, &[skin])
}

/// [`load_match`] with several local players (issue #29): local player
/// `k` (the `k`-th human of `o.players`) wears `skins[k]`; one without a
/// skin given plays in the default look and skin.
pub fn load_match_for(roots: &[&Path], o: &MatchOptions, skins: &[&str]) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let packs = content.replay_packs(&o.mod_id, &o.level_id)?;
    let options = Box::new(o.clone());
    let game = Game::Deathmatch { options, packs, spawn: None };
    open(Arc::new(content), o.mod_id.clone(), o.level_id.clone(), skins, game)
}

/// The level an editor's Play is on: the level editor's edited level
/// itself (no pack), or an installed one by pack id (the mod editor's).
#[derive(Clone, Debug, PartialEq)]
pub enum PlayLevel {
    Edited(Box<LevelSource>),
    Installed(String),
}

/// The mod an editor's Play runs: an installed one by pack id (the level
/// editor's), or the mod editor's pack as it is, unsaved changes included
/// (issue #18), compiled and validated as a saved one is
/// (`olr_edit::pack::mod_pack`).
#[derive(Clone, Debug, PartialEq)]
pub enum PlayMod {
    Installed(String),
    Edited(Box<olr_edit::pack::Pack>),
}

/// What an editor's Play asks for (plan 14c R3; issue #18): its level, its
/// mod, and 0 to `olr_editor::MAX_BOTS` bots; or, for the mod editor's
/// practice (issue #20), the weapon, targets and reload of `practice`
/// (`bots` unread).
#[derive(Clone, Debug, PartialEq)]
pub struct EditedPlay {
    pub level: PlayLevel,
    pub game_mod: PlayMod,
    pub bots: u8,
    pub practice: Option<olr_mod_editor::Practice>,
}

/// Fast reload's `LoadingTime` (issue #20): 0 fills a slot's charge at
/// once (`olr_sim`'s `simulate_worm_weapon`), so only the weapon's rate of
/// fire paces it.
pub const FAST_LOADING_TIME: i32 = 0;

/// The sandbox of a practice (issue #20) with mod `m`'s weapon `p.weapon`
/// on packs `mod_id` and `level_id`: the player (worm 0) holds it in its
/// first slot and the mod's first other weapons in the rest (the
/// sandbox's own loadout, `sandbox_setup`), against `p.targets` (1 to
/// `olr_mod_editor::MAX_TARGETS`) idle targets at `FindSpot` positions,
/// which press nothing, as `olr_bots::Personality::Idle`, and respawn by
/// `MaxRespawnTime`. Reloading is the weapon's own (`LoadingTime` 100 %,
/// LX56's), or [`FAST_LOADING_TIME`] with Fast reload. A weapon the mod
/// lacks is an error.
pub fn practice_setup(
    m: &LoadedMod,
    mod_id: &str,
    level_id: &str,
    p: &olr_mod_editor::Practice,
) -> anyhow::Result<SandboxSetup> {
    let weapons = m.sim.weapons();
    let Some(chosen) = weapons.iter().position(|w| w.id == p.weapon) else {
        anyhow::bail!("the mod has no weapon {:?}", p.weapon);
    };
    let others = (0..weapons.len()).filter(|&i| i != chosen);
    let loadout = std::iter::once(chosen).chain(others).take(5).map(|i| i as i32).collect();
    let targets = usize::from(p.targets.clamp(1, olr_mod_editor::MAX_TARGETS));
    let o =
        SandboxOptions { mod_id: mod_id.into(), level_id: level_id.into(), seed: 1, targets, bonuses: false };
    let mut setup = sandbox_setup(&o);
    setup.settings.loading_time =
        if p.fast_reload { FAST_LOADING_TIME } else { SimSettings::lx56().loading_time };
    setup.worms[0].weapons = loadout;
    Ok(setup)
}

/// The level id an edited play's assets carry (the level is the editor's,
/// in no pack): for messages; `Assets::edited` marks the play.
pub const EDITED_LEVEL: &str = "edited level";

/// An editor's Play (plan 14c R3, R4; issue #18) on its level: the sandbox
/// for 0 bots (the player alone, no targets), else a Deathmatch against
/// that many bots (capped at `olr_editor::MAX_BOTS`) of the Local Play
/// screen's saved level, personality and loadout (`prefs`), under OLX's
/// Deathmatch defaults. On the edited level the human worm first spawns at
/// its first spawn when it lies inside the level and out of rock
/// (`LoadedLevel::first_spawn`), else at a free spot; on an installed level
/// at a free spot, as any match. The theme, core and skin `skin` come by id
/// from `roots`, and so do an installed level and mod; the edited mod is
/// refused when it would not save, then loaded as an installed one
/// (`Content::load_mod_pack`). A practice (issue #20) is its own sandbox
/// instead ([`practice_setup`]; the human at a free spot, as any match).
/// Its replay is never saved (the shell's `Launch::Edited`), so its pack
/// refs have no hash.
pub fn load_edited(
    roots: &[&Path],
    play: &EditedPlay,
    skin: &str,
    prefs: &olr_ui::MatchPrefs,
) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let (level, level_id, spawn) = match &play.level {
        PlayLevel::Edited(src) => {
            let level = LoadedLevel::from_source(src).context("the edited level")?;
            let spawn = level.first_spawn();
            (level, EDITED_LEVEL.to_string(), spawn)
        }
        PlayLevel::Installed(id) => (content.load_level(id)?, id.clone(), None),
    };
    let (mod_id, m) = match &play.game_mod {
        PlayMod::Installed(id) => (id.clone(), content.load_mod(id)?),
        PlayMod::Edited(pack) => {
            let id = pack.manifest.id.clone();
            let read = olr_edit::pack::mod_pack(pack).context("the edited mod")?;
            let m = content.load_mod_pack(&id, read).context("the edited mod")?;
            (id, m)
        }
    };
    let bots = play.bots.min(olr_editor::MAX_BOTS);
    let game = if let Some(p) = &play.practice {
        let mut setup = practice_setup(&m, &mod_id, &level_id, p).context("the practice")?;
        setup.worms[0].pos = spawn;
        Game::Sandbox(setup)
    } else if bots == 0 {
        let o = SandboxOptions {
            mod_id: mod_id.clone(),
            level_id: level_id.clone(),
            seed: 1,
            targets: 0,
            bonuses: false,
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
            weapons: olr_content::MatchLoadout::default(),
            ..prefs.clone()
        };
        let r = |id: &str| olr_replay::PackRef { id: id.into(), hash: String::new() };
        let packs = Packs { mod_pack: r(&mod_id), level: r(&level_id), theme: r(&level.theme) };
        Game::Deathmatch { options: Box::new(p.options(1)), packs, spawn }
    };
    let mut a = open_loaded(Arc::new(content), (mod_id, m), level_id, level, &[skin], game)?;
    a.edited = true;
    a.practice = play.practice.is_some();
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
    let mine = my_slot.and_then(|i| start.slots.get(usize::from(i))).map(|w| w.look.skin.as_str());
    let game = Game::Online { start: Box::new(start.clone()), my_slot };
    let mut a = open(content, s.mod_id.clone(), s.level_id.clone(), mine.as_slice(), game)?;
    for (i, w) in start.slots.iter().enumerate() {
        let skin = if let Some(me) = a.locals.first().filter(|_| my_slot.is_some_and(|m| usize::from(m) == i))
        {
            me.skin.clone()
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
    let mut a = open(Arc::new(content), mod_id, level_id, &[], Game::Replay(Box::new(replay)))?;
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

    /// Whether this is an editor's Play (`load_edited`), whose replay is
    /// never saved.
    pub fn edited(&self) -> bool {
        self.edited
    }

    /// Whether this is the mod editor's practice (issue #20).
    pub fn practice(&self) -> bool {
        self.practice
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

    /// The worms the players here play, in local order (issue #29): the
    /// sandbox's 0, a Deathmatch's humans (every worm that is not a bot,
    /// in worm order, at most [`MAX_LOCALS`]), an online match's own slot;
    /// none for a replay, bots alone and an online match watched.
    pub fn local_slots(&self) -> Vec<u8> {
        match &self.game {
            Game::Sandbox(_) => vec![0],
            Game::Deathmatch { options, .. } => (0..options.players.len())
                .filter(|&i| !matches!(options.players[i], Player::Bot(_)))
                .filter_map(|i| u8::try_from(i).ok())
                .take(MAX_LOCALS)
                .collect(),
            Game::Replay(_) => Vec::new(),
            Game::Online { my_slot, .. } => my_slot.iter().copied().collect(),
        }
    }

    /// The first local player's worm ([`Assets::local_slots`]); `None` for
    /// a replay, bots alone and an online match watched.
    pub fn local_slot(&self) -> Option<u8> {
        self.local_slots().first().copied()
    }

    /// The worms the views start on, one view each: the local players'
    /// own, else (a replay) the worms of the players whose game made it
    /// (`ReplaySetup::viewers`: a split-screen replay plays back with every
    /// view), else worm 0.
    pub fn first_followed(&self) -> Vec<u8> {
        let locals = self.local_slots();
        match &self.game {
            _ if !locals.is_empty() => locals,
            Game::Replay(r) if !r.setup.viewers.is_empty() => r.setup.viewers.clone(),
            _ => vec![0],
        }
    }

    /// Local player `k`'s look and skin, if it has its own.
    pub fn local_player(&self, k: usize) -> Option<&LocalPlayer> {
        self.locals.get(k)
    }

    /// Sets the first local player's look (the menus' profile).
    pub fn set_player_look(&mut self, look: PlayerLook) {
        if let Some(p) = self.locals.first_mut() {
            p.look = Some(look);
        }
    }

    /// The skin of online slot `slot`, else the default skin.
    pub fn skin_of_slot(&self, slot: usize) -> &LoadedSkin {
        self.slot_skins.get(slot).unwrap_or(&self.bot_skin)
    }

    /// What this game is, for the log (issue #2): its kind and seed, the
    /// mod and level, who plays.
    pub fn describe(&self) -> String {
        let kind = match &self.game {
            Game::Sandbox(_) => "sandbox".to_string(),
            Game::Deathmatch { options, .. } if self.human_plays() => {
                format!("Deathmatch seed {}", options.seed)
            }
            Game::Deathmatch { options, .. } => format!("Deathmatch of bots (watched) seed {}", options.seed),
            Game::Replay(r) => format!("replay of {:.1} s", f64::from(r.ticks()) / 100.0),
            Game::Online { start, .. } => format!("online match seed {}", start.seed),
        };
        let who = match &self.game {
            Game::Deathmatch { options, .. } => {
                let bots: Vec<Difficulty> = options
                    .players
                    .iter()
                    .filter_map(|p| match p {
                        Player::Bot(b) => Some(b.difficulty),
                        _ => None,
                    })
                    .collect();
                let level = bots.first().map_or(String::new(), |d| format!(" ({d:?})"));
                format!("{} players, {} bots{level}", options.players.len() - bots.len(), bots.len())
            }
            Game::Online { start, my_slot } => {
                let bots = start.slots.iter().filter(|s| s.member.is_none()).count();
                let me = my_slot.map_or("watching".to_string(), |s| format!("playing slot {s}"));
                format!("{} players, {bots} bots, {me}", start.slots.len() - bots)
            }
            _ => format!("{} worms", self.worms()),
        };
        format!("{kind}: {} on {}, {who}", self.mod_id, self.level_id)
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
            level: PlayLevel::Edited(Box::new(level.clone())),
            game_mod: PlayMod::Installed("olx-classic".into()),
            bots,
            practice: None,
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

        // Plan 15a Task 19: Play carries the edited level's look.
        let mut lit = level.clone();
        lit.look = Some(olr_format::look::LookSource::default());
        let a = load_edited(&[root], &play(&lit, 0), DEFAULT_SKIN, &prefs).unwrap();
        assert!(a.level.has_look(), "the edited look plays");
        assert!(a.level.surface.is_none());
        // Issue #43: so do its surface materials, which heat in play.
        let mut hot = lit.clone();
        let stone = olr_format::materials::MaterialSource {
            light_soak: None,
            id: "stone".into(),
            name: "Stone".into(),
            notes: String::new(),
            heat: None,
        };
        hot.look.as_mut().unwrap().materials.push(stone);
        let n = (hot.size.0 * hot.size.1) as usize;
        hot.surface =
            Some(olr_format::levels::SurfaceMaps { world: hot.size, front: vec![1; n], back: vec![0; n] });
        let a = load_edited(&[root], &play(&hot, 0), DEFAULT_SKIN, &prefs).unwrap();
        assert_eq!(a.level.surface, hot.surface, "the edited level's materials play");
        let a = load_edited(&[root], &play(&level, 0), DEFAULT_SKIN, &prefs).unwrap();
        assert!(!a.level.has_look());
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

        // The HUD's keys are the editor's Play's: Esc stops it (no pause menu).
        let vp = olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
        let s = crate::session::Session::new(
            &a,
            vp,
            olr_scene::Interpolation::Linear,
            olr_audio::Audio::silent(),
        )
        .unwrap();
        let frame = crate::hud::Frame { menus: true, edited: true, ..Default::default() };
        let hint = crate::hud::view(&s, &frame).hint.unwrap();
        assert!(hint.contains("Esc stop") && !hint.contains("pause"), "{hint}");
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
        let bad = EditedPlay { game_mod: PlayMod::Installed("no-such-mod".into()), ..play(&level, 0) };
        assert!(!load_edited(&[root], &play(&level, 0), DEFAULT_SKIN, &prefs).unwrap().practice());
        assert!(load_edited(&[root], &bad, DEFAULT_SKIN, &prefs).is_err());
    }

    /// Issue #18: the mod editor's Play runs its mod as it is, unsaved
    /// changes included (the session plays `Assets::m`), on an installed
    /// level; a mod that would not save does not start.
    #[test]
    fn edited_play_runs_the_unsaved_mod_on_an_installed_level() {
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let root = Path::new(&dir);
        let mut pack = olr_edit::pack::load(&root.join("olx-classic")).unwrap();
        pack.manifest.id = "my-classic".into();
        olr_edit::mods::set(&mut pack, "weapons/bazooka.fire.shot.speed", "150").unwrap();
        let prefs = olr_ui::MatchPrefs::default();
        let play = |pack: &olr_edit::pack::Pack, bots| EditedPlay {
            level: PlayLevel::Installed("olx-level-dirt-level".into()),
            game_mod: PlayMod::Edited(Box::new(pack.clone())),
            bots,
            practice: None,
        };
        let speed = |a: &Assets| {
            let w = a.m.sim.weapons().iter().find(|w| w.id == "bazooka").unwrap();
            w.spawn.as_ref().unwrap().speed
        };
        for bots in [0, 2] {
            let a = load_edited(&[root], &play(&pack, bots), DEFAULT_SKIN, &prefs).unwrap();
            assert_eq!(speed(&a), 150, "the unsaved speed");
            assert!(a.edited() && !a.practice());
            assert_eq!((a.mod_id.as_str(), a.level_id.as_str()), ("my-classic", "olx-level-dirt-level"));
            match &a.game {
                Game::Sandbox(s) => assert_eq!((bots, s.worms[0].pos), (0, None)),
                Game::Deathmatch { options, spawn, .. } => {
                    assert_eq!((options.players.len(), *spawn), (3, None), "a free spot")
                }
                _ => panic!("a sandbox or a Deathmatch"),
            }
            let s = crate::session::Session::new(
                &a,
                olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
                olr_scene::Interpolation::Linear,
                olr_audio::Audio::silent(),
            )
            .unwrap();
            let w = s.world.sim_mod().weapons().iter().find(|w| w.id == "bazooka").unwrap();
            assert_eq!(w.spawn.as_ref().unwrap().speed, 150, "the session's world plays it");
        }
        let installed = EditedPlay { game_mod: PlayMod::Installed("olx-classic".into()), ..play(&pack, 0) };
        assert_eq!(speed(&load_edited(&[root], &installed, DEFAULT_SKIN, &prefs).unwrap()), 140);
        // A cycle: refused as a save is.
        let cycle = r#"{"s": (shared: "bazooka.spawn")}"#;
        olr_edit::mods::set_lenient(&mut pack, "projectiles/bazooka.spawn.spawns", cycle).unwrap();
        let e = load_edited(&[root], &play(&pack, 0), DEFAULT_SKIN, &prefs).err().unwrap();
        assert!(format!("{e:#}").contains("the edited mod"), "{e:#}");
    }

    /// Issue #20: the mod editor's practice is a sandbox of the unsaved
    /// mod on its level: the player holds the weapon first (then the mod's
    /// first others), 1 to 4 idle targets (the sandbox's, pressing
    /// nothing), the weapon's own reload (`LoadingTime` 100) unless Fast
    /// reload (0); a weapon the mod lacks does not start.
    #[test]
    fn practice_holds_the_weapon_against_idle_targets() {
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let root = Path::new(&dir);
        let mut pack = olr_edit::pack::load(&root.join("olx-classic")).unwrap();
        pack.manifest.id = "my-classic".into();
        olr_edit::mods::set(&mut pack, "weapons/bazooka.fire.shot.speed", "150").unwrap();
        let prefs = olr_ui::MatchPrefs::default();
        let practice = |weapon: &str, targets, fast_reload| EditedPlay {
            level: PlayLevel::Installed("olx-level-castlestrike".into()),
            game_mod: PlayMod::Edited(Box::new(pack.clone())),
            bots: 5,
            practice: Some(olr_mod_editor::Practice { weapon: weapon.into(), targets, fast_reload }),
        };
        let a = load_edited(&[root], &practice("cannon", 3, false), DEFAULT_SKIN, &prefs).unwrap();
        assert!(a.edited() && a.practice());
        assert_eq!(a.level_id, "olx-level-castlestrike");
        let index = |id: &str| a.m.sim.weapons().iter().position(|w| w.id == id).unwrap() as i32;
        let (cannon, bazooka) = (index("cannon"), index("bazooka"));
        assert!(cannon > 4, "not among the sandbox's first five");
        let Game::Sandbox(setup) = &a.game else { panic!("a sandbox") };
        assert_eq!(setup.worms.len(), 4, "the player and 3 targets");
        assert_eq!(setup.worms[0].weapons, vec![cannon, 0, 1, 2, 3], "the weapon, then the mod's first");
        assert!(setup.worms.iter().all(|w| w.pos.is_none()), "free spots");
        assert_eq!(setup.settings.loading_time, 100, "the weapon's own reload");
        let s = crate::session::Session::new(
            &a,
            olr_scene::Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
            olr_scene::Interpolation::Linear,
            olr_audio::Audio::silent(),
        )
        .unwrap();
        assert!(matches!(s.driver, crate::session::Driver::Sandbox), "the targets press nothing");
        assert_eq!(s.world.settings().loading_time, 100);
        let w = &s.world.worms()[0];
        assert_eq!(w.slots[0].weapon, cannon);
        let shot = s.world.sim_mod().weapons()[bazooka as usize].spawn.as_ref().unwrap().speed;
        assert_eq!(shot, 150, "the unsaved weapon");

        let a = load_edited(&[root], &practice("bazooka", 1, true), DEFAULT_SKIN, &prefs).unwrap();
        let Game::Sandbox(setup) = &a.game else { panic!("a sandbox") };
        assert_eq!(setup.worms.len(), 2);
        assert_eq!(setup.worms[0].weapons, vec![bazooka, 1, 2, 3, 4], "the first: not twice");
        assert_eq!(setup.settings.loading_time, FAST_LOADING_TIME);
        // Beyond the menu's targets: capped.
        let a = load_edited(&[root], &practice("bazooka", 200, false), DEFAULT_SKIN, &prefs).unwrap();
        assert_eq!(a.worms(), 1 + usize::from(olr_mod_editor::MAX_TARGETS));
        let e =
            load_edited(&[root], &practice("no-such-weapon", 1, false), DEFAULT_SKIN, &prefs).err().unwrap();
        assert!(format!("{e:#}").contains("no weapon"), "{e:#}");
    }

    /// Both editors' Play offer the same bots (no content needed).
    #[test]
    fn the_editors_bots_agree() {
        assert_eq!(olr_mod_editor::MAX_BOTS, olr_editor::MAX_BOTS);
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
            loadout: olr_proto::LoadoutWire::default(),
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
        assert_eq!(m.weapons, olr_content::WeaponChoice::default_loadout(), "the command line plays Default");
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
        // Issue #29: local players first, then the bots; two players need
        // no bot, one does; at most MAX_LOCALS.
        let locals = |locals, bots| {
            let o = DeathmatchOptions { locals, bots, ..Default::default() };
            let p = match_options(&o, "m", "l").players;
            (p.iter().filter(|p| matches!(p, Player::Human)).count(), p.len())
        };
        assert_eq!(locals(4, 3), (4, 7));
        assert!(
            match_options(&DeathmatchOptions { locals: 4, ..Default::default() }, "m", "l").players[..4]
                .iter()
                .all(|p| *p == Player::Human)
        );
        assert_eq!(locals(2, 0), (2, 2));
        assert_eq!(locals(1, 0), (1, 2));
        assert_eq!(locals(9, 0), (MAX_LOCALS, MAX_LOCALS));
        assert_eq!(locals(4, 100), (4, MAX_WORMS));
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
