//! Replays (spec §20 F8): the packs a match is made of, by id and content
//! hash, and the world a replay starts from, refused when the content
//! differs from what was recorded (plan 6 Review Focus 3).

use anyhow::Context as _;
use olr_data::manifest::PackKind;
use olr_replay::{Assets, PackRef, Packs, Replay, ReplayRun, ReplaySetup};
use olr_sim::World;

use crate::{Content, LoadedLevel, LoadedMod, LoadedTheme};

impl Content {
    /// The content hash of pack `id` of `kind` (`olr_pack::content_hash`),
    /// memoised per pack directory for this `Content`'s life (staleness
    /// within a session is accepted).
    pub fn pack_hash(&self, id: &str, kind: PackKind) -> anyhow::Result<String> {
        let src = self.pack(id, kind)?;
        let key = src.root().to_path_buf();
        if let Some(h) = self.hashes.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return Ok(h);
        }
        let hash = olr_pack::content_hash(&src).with_context(|| format!("hashing {kind:?} pack {id}"))?;
        if let Ok(mut m) = self.hashes.lock() {
            m.insert(key, hash.clone());
        }
        Ok(hash)
    }

    /// The packs of a match of mod `mod_id` on level `level_id` (and the
    /// level's theme), with their content hashes, for `olr_replay::Recorder::new`.
    pub fn replay_packs(&self, mod_id: &str, level_id: &str) -> anyhow::Result<Packs> {
        let theme = self.load_level(level_id)?.theme;
        let r = |id: &str, kind| -> anyhow::Result<PackRef> {
            Ok(PackRef { id: id.into(), hash: self.pack_hash(id, kind)? })
        };
        Ok(Packs {
            mod_pack: r(mod_id, PackKind::Mod)?,
            level: r(level_id, PackKind::Level)?,
            theme: r(&theme, PackKind::Theme)?,
        })
    }

    /// Checks that `setup`'s packs are here with the recorded content
    /// hashes. A missing pack, or one whose content hash is not the
    /// recorded one, is an error: the replay would diverge.
    pub fn check_replay_packs(&self, setup: &ReplaySetup) -> anyhow::Result<()> {
        let p = &setup.packs;
        for (r, kind) in
            [(&p.mod_pack, PackKind::Mod), (&p.level, PackKind::Level), (&p.theme, PackKind::Theme)]
        {
            let hash = self.pack_hash(&r.id, kind)?;
            anyhow::ensure!(
                hash == r.hash,
                "the replay was recorded with {kind:?} pack {} of content hash {}, but this one's is {hash}: \
                 a different version of the pack",
                r.id,
                r.hash
            );
        }
        Ok(())
    }

    /// What `setup`'s packs resolve to, once checked
    /// ([`Content::check_replay_packs`]).
    pub fn replay_assets(&self, setup: &ReplaySetup) -> anyhow::Result<Assets> {
        self.check_replay_packs(setup)?;
        let p = &setup.packs;
        let m = self.load_mod(&p.mod_pack.id)?;
        let level = self.load_level(&p.level.id)?;
        let theme = self.load_theme(&p.theme.id)?;
        Ok(replay_assets_from(&m, &level, &theme))
    }

    /// The world `replay` starts from, made of this content (refused when
    /// it differs from the recorded content: [`Content::replay_assets`]).
    pub fn world_for_replay(&self, replay: &Replay) -> anyhow::Result<World> {
        let assets = self.replay_assets(&replay.setup)?;
        replay.world(assets).map_err(anyhow::Error::new)
    }

    /// Plays `replay` on this content to its end, checking every recorded
    /// checksum: the first that differs is the error, an
    /// [`olr_replay::Mismatch`] (`downcast_ref` gives its tick). A world that
    /// cannot be built is an [`olr_replay::ReplayError`], or a context error
    /// for missing or different packs.
    pub fn play_replay(&self, replay: &Replay) -> anyhow::Result<ReplayRun> {
        let world = self.world_for_replay(replay)?;
        replay.play(world).map_err(anyhow::Error::new)
    }
}

/// A replay's [`Assets`] from a mod (as [`Content::load_mod`]), level and
/// theme already loaded (a game that draws the replay loads them once), as
/// [`crate::match_setup`] uses them. The caller checks the packs first
/// ([`Content::check_replay_packs`]).
pub fn replay_assets_from(m: &LoadedMod, level: &LoadedLevel, theme: &LoadedTheme) -> Assets {
    Assets {
        sim_mod: m.sim.clone(),
        terrain: olr_terrain::Terrain::from_grid(&level.material),
        holes: theme.stamps(),
        green_mask: theme.green_mask(),
        live: level.live_sim(),
        director: level.live_director(),
    }
}
