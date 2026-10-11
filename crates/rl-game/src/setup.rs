//! What the game plays (the sandbox, spec §19 E11; a Deathmatch against
//! bots; a replay): which packs to load, the match to build, and the
//! assets loaded once for every restart.

use std::path::Path;

use anyhow::Context as _;
use rl_bots::{BotConfig, Difficulty};
use rl_content::{
    Content, LoadedLevel, LoadedMod, LoadedSkin, LoadedTheme, MatchOptions, Player, SandboxSetup, SpawnSpec,
};
use rl_data::manifest::PackKind;
use rl_modes::DeathmatchSettings;
use rl_replay::{Packs, Replay};
use rl_sim::{MAX_WORMS, SimSettings};

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
        restrictions: rl_content::DEFAULT_PRESET.into(),
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
        }
    }
}

/// The match [`DeathmatchOptions`] describe on packs `mod_id` and
/// `level_id`: the human is worm 0, then the bots (OLX's bots at one
/// difficulty), under OLX's LX56 settings, the default restrictions
/// ([`rl_content::DEFAULT_PRESET`]) and the Deathmatch rules given.
/// The bots are capped so the worms fit `MAX_WORMS`.
pub fn match_options(o: &DeathmatchOptions, mod_id: &str, level_id: &str) -> MatchOptions {
    let bot = BotConfig { difficulty: o.difficulty, ..BotConfig::default() };
    let bots = o.bots.clamp(1, MAX_WORMS - 1);
    MatchOptions {
        mod_id: mod_id.into(),
        level_id: level_id.into(),
        seed: o.seed,
        settings: SimSettings { bonuses: o.bonuses, ..SimSettings::lx56() },
        players: std::iter::once(Player::Human).chain((0..bots).map(|_| Player::Bot(bot))).collect(),
        deathmatch: DeathmatchSettings {
            lives: o.lives,
            kill_limit: o.kill_limit,
            time_limit: o.time_limit,
            ..DeathmatchSettings::default()
        },
        restrictions: rl_content::DEFAULT_PRESET.into(),
    }
}

/// The pack id `given` means among `ids`: itself, or an OLX import's id
/// (`prefix` and the slug of `given`, so `--level "Liero Factory"` and
/// `--mod MW 1.0` work).
pub fn resolve_id(given: &str, prefix: &str, ids: &[String]) -> anyhow::Result<String> {
    rl_content::resolve_id(given, prefix, ids)
        .with_context(|| format!("no pack {given:?}; there are: {}", ids.join(", ")))
}

/// OLX's default skin's pack id.
pub const DEFAULT_SKIN: &str = "olx-skin-default";

/// What the game plays.
pub enum Game {
    /// The sandbox (spec §19 E11).
    Sandbox(SandboxSetup),
    /// A Deathmatch, the human (worm 0) against bots; `packs` are its mod,
    /// level and theme for the replay.
    Deathmatch { options: Box<MatchOptions>, packs: Packs },
    /// A replay, played with the normal renderer.
    Replay(Box<Replay>),
}

/// The human's name and colour (the menus' profile), for worm 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerLook {
    pub name: String,
    pub colour: [u8; 3],
}

/// Everything a session draws and plays, loaded once: a restart (F5)
/// builds a new world from it without decoding any image or sound again.
pub struct Assets {
    pub content: Content,
    pub m: LoadedMod,
    pub level: LoadedLevel,
    pub theme: LoadedTheme,
    /// The human's skin (worm 0's when a human plays it).
    pub skin: LoadedSkin,
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
}

/// The packs `mod_id`, `level_id` (with its theme) and skin `skin` (a
/// name or id; the default skin when missing), resolved and loaded.
fn open(
    content: Content,
    mod_id: String,
    level_id: String,
    skin: &str,
    game: Game,
) -> anyhow::Result<Assets> {
    let skin_id = content.skin_id(skin).unwrap_or_else(|| skin.to_string());
    let m = content.load_mod(&mod_id)?;
    let lvl = content.load_level(&level_id)?;
    let theme = content.load_theme(&lvl.theme).with_context(|| format!("level {level_id}"))?;
    let skin = content.skin_or_default(&skin_id);
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
        bot_skin,
        game,
        mod_id,
        level_id,
        warnings,
        player: None,
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
    open(content, mod_id, level_id, skin, Game::Sandbox(setup))
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
    open(content, mod_id, level_id, skin, Game::Deathmatch { options, packs })
}

/// The Deathmatch `o` describes (the menus' local play), its packs by id.
/// A missing or broken pack is an error naming it.
pub fn load_match(roots: &[&Path], o: &MatchOptions, skin: &str) -> anyhow::Result<Assets> {
    let content = Content::open_layers(roots)?;
    let packs = content.replay_packs(&o.mod_id, &o.level_id)?;
    let options = Box::new(o.clone());
    open(content, o.mod_id.clone(), o.level_id.clone(), skin, Game::Deathmatch { options, packs })
}

/// The replay in file `file`, on the packs it names (refused when they
/// are missing or differ from the recorded ones).
pub fn load_replay(roots: &[&Path], file: &Path, skin: &str) -> anyhow::Result<Assets> {
    let bytes = std::fs::read(file).with_context(|| format!("{}", file.display()))?;
    let replay = Replay::decode(&bytes).with_context(|| format!("{}", file.display()))?;
    let content = Content::open_layers(roots)?;
    content.check_replay_packs(&replay.setup).with_context(|| format!("{}", file.display()))?;
    let p = &replay.setup.packs;
    let (mod_id, level_id) = (p.mod_pack.id.clone(), p.level.id.clone());
    anyhow::ensure!(
        content.load_level(&level_id)?.theme == p.theme.id,
        "{}: level {level_id} has another theme than the recorded {}",
        file.display(),
        p.theme.id
    );
    open(content, mod_id, level_id, skin, Game::Replay(Box::new(replay)))
}

impl Assets {
    /// A new world of the sandbox, from the assets loaded (an error for
    /// another game).
    pub fn new_world(&self) -> anyhow::Result<rl_sim::World> {
        let Game::Sandbox(setup) = &self.game else { anyhow::bail!("not a sandbox") };
        rl_content::new_world_from(&self.m, &self.level, &self.theme, setup)
    }

    /// How many worms the game starts with.
    pub fn worms(&self) -> usize {
        match &self.game {
            Game::Sandbox(s) => s.worms.len(),
            Game::Deathmatch { options, .. } => options.players.len(),
            Game::Replay(r) => r.setup.worms.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(targets: usize) -> SandboxOptions {
        SandboxOptions {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-liero-factory".into(),
            seed: 1,
            targets,
            bonuses: false,
        }
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
        let Some(dir) = std::env::var_os("RL_CONTENT_DIR").filter(|d| Path::new(d).is_dir()) else {
            eprintln!("RL_CONTENT_DIR not set; skipping the imported levels");
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
        assert_eq!(m.restrictions, rl_content::DEFAULT_PRESET);
        let many = match_options(&DeathmatchOptions { bots: 100, ..Default::default() }, "m", "l");
        assert_eq!(many.players.len(), MAX_WORMS);
        let none = match_options(&DeathmatchOptions { bots: 0, ..Default::default() }, "m", "l");
        assert_eq!(none.players.len(), 2, "a Deathmatch needs two worms (F5)");
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
}
