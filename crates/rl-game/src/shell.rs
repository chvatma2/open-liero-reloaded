//! The menus driving the game (spec §21 G1-G2), without a window: the
//! `rl_ui::UiState`, its widgets' data, the settings and profiles files,
//! and the game playing. `app.rs` feeds it the widgets' actions and does
//! what comes back ([`Outcome`]); everything else happens here, so it is
//! tested headless (`tests/menu.rs`).
//!
//! A start (local play's Start, the sandbox, a replay) is loaded by
//! [`Shell::poll`] after the frame that asked for it, so the menus show
//! "Loading…" meanwhile; until then `UiState` ignores another start, Back
//! and the menu buttons (plan 7 Review Focus 4). A start that fails (a
//! pack missing or broken, a file that is no replay) returns to the
//! screen it came from with the reason (Review Focus 3). A Deathmatch
//! starts with OLX's weapon selection over its level: the world waits (no
//! tick) until Done arms the player (`Player::HumanWith` in the match's
//! options, the world built again with them, so the replay holds them).
//! The pause menu and the selection stop the ticks ([`Shell::tick`]):
//! replays record ticks only, so a pause never shows in one. Every
//! Deathmatch's replay is saved in the data directory's `replays/` when it
//! ends, is restarted or the game quits (`rl_ui::replays::auto_save`); the
//! results' Save replay keeps it under a name of its own
//! (`rl_ui::replays::save_kept`).
//!
//! The menus list and play the bundled content (`--content`) with the
//! user data's `packs/` layered over it (spec §9.4: an imported pack
//! replaces the bundled one of its id). The Import screen's import runs
//! on its own thread (`rl_ui::import::ImportJob`), polled each frame by
//! [`Shell::poll`]; when it ends the content is listed again, so new and
//! re-imported packs show (their thumbnails made again) without a restart.
//! No game starts while it runs (`UiState` refuses), and a game playing
//! keeps the packs it loaded.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{ColorImage, TextureHandle, TextureOptions};
use rl_audio::Audio;
use rl_content::{Content, LoadedSkin, MatchOptions, Player};
use rl_input::Bindings;
use rl_scene::{Interpolation, Viewport};
use rl_sim::{Keys, MAX_SLOTS};
use rl_ui::import::{ImportJob, ImportProgress};
use rl_ui::screens::MenuData;
use rl_ui::skin_preview::{self, PreviewKey, PreviewTextures};
use rl_ui::weapon_select::{WeaponMenu, WeaponSelect};
use rl_ui::{AppCommand, AudioSettings, Profiles, Screen, Settings, Store, UiAction, UiState, VideoSettings};

use crate::session::Session;
use crate::setup::{self, Assets, Game, PlayerLook, SandboxOptions};
use crate::video::{self, Overrides};

/// What the shell needs besides the menus' state.
#[derive(Clone, Debug)]
pub struct ShellConfig {
    /// The content directory (`--content`).
    pub content: PathBuf,
    pub interpolation: Interpolation,
    /// The window's view (`app::viewport`).
    pub viewport: Viewport,
}

/// A game playing: what it was loaded from, and its session.
pub struct Playing {
    pub assets: Assets,
    pub session: Session,
}

/// What the window must do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Quit,
    /// The video options changed (window size, fullscreen, UI scale).
    Video(VideoSettings),
    /// The player's keys changed.
    Bindings(Bindings),
    /// A new game plays: its images go to the renderer, the clock starts
    /// afresh (the loading time is not caught up).
    Started,
}

/// What to load.
enum Launch {
    Match(Box<MatchOptions>),
    Sandbox,
    Replay(PathBuf),
}

/// The sandbox's idle targets when started from the menu.
pub const SANDBOX_TARGETS: usize = 3;

pub struct Shell {
    pub ui: UiState,
    pub data: MenuData,
    /// The game playing, if any.
    pub play: Option<Playing>,
    cfg: ShellConfig,
    store: Store,
    /// The sound output while no session holds it.
    audio: Option<Audio>,
    pending: Option<Launch>,
    paused: bool,
    /// `MatchOver` was reported for the game playing.
    over_reported: bool,
    launches: u32,
    unsaved_settings: Option<Settings>,
    unsaved_profiles: Option<Profiles>,
    /// Warnings logged once.
    logged: BTreeSet<String>,
    /// The content the menus list (the skin preview loads its skins).
    content: Option<Content>,
    /// The skin preview: the skin loaded last (by the id asked for) and
    /// the textures shown (dropping them frees them).
    preview_skin: Option<(String, LoadedSkin)>,
    preview_textures: Vec<TextureHandle>,
    /// The command line's video flags over the saved options, and the
    /// options as loaded (`video::Overrides`).
    overrides: Option<(VideoSettings, Overrides)>,
    /// The Import screen's import while it runs.
    import: Option<ImportJob>,
}

/// How long quitting waits for an import to stop after its item.
pub const IMPORT_STOP_WAIT: Duration = Duration::from_secs(2);

impl Shell {
    /// The main menu over content directory `cfg.content` with the data
    /// directory's imported packs over it (listed for the menus now), with
    /// `audio` for every game.
    pub fn new(cfg: ShellConfig, ui: UiState, store: Store, audio: Audio) -> Shell {
        let mut data = MenuData::new(Default::default(), store.replays_dir());
        data.packs_dir = store.packs_dir();
        data.notices = store.notices();
        let mut shell = Shell {
            ui,
            data,
            play: None,
            cfg,
            store,
            audio: Some(audio),
            pending: None,
            paused: false,
            over_reported: false,
            launches: 0,
            unsaved_settings: None,
            unsaved_profiles: None,
            logged: BTreeSet::new(),
            content: None,
            preview_skin: None,
            preview_textures: Vec::new(),
            overrides: None,
            import: None,
        };
        shell.list_content();
        let audio = shell.ui.settings().audio.clone();
        shell.set_volume(&audio);
        shell
    }

    /// A game started without the menus (the command line's): `play`
    /// plays; nothing is saved.
    pub fn playing(cfg: ShellConfig, assets: Assets, session: Session) -> Shell {
        let ui = UiState::new(Settings::default(), Profiles::default(), 0);
        let data = MenuData::new(Default::default(), None);
        Shell {
            ui,
            data,
            play: Some(Playing { assets, session }),
            cfg,
            store: Store::in_memory(),
            audio: None,
            pending: None,
            paused: false,
            over_reported: false,
            launches: 1,
            unsaved_settings: None,
            unsaved_profiles: None,
            logged: BTreeSet::new(),
            content: None,
            preview_skin: None,
            preview_textures: Vec::new(),
            overrides: None,
            import: None,
        }
    }

    /// The content directories, highest priority first: the data
    /// directory's imported packs, then the bundled content.
    fn roots(&self) -> Vec<PathBuf> {
        self.data.packs_dir.iter().cloned().chain([self.cfg.content.clone()]).collect()
    }

    /// Lists the content for the menus (again, after an import): the
    /// catalog, its thumbnails made again (`MenuData::set_catalog`), the
    /// skin preview loaded again.
    fn list_content(&mut self) {
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        self.content = match Content::open_layers(&roots) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("warning: {e:#}: no mods or levels to play");
                None
            }
        };
        let catalog = self.content.as_ref().map(Content::catalog).unwrap_or_default();
        self.log(self.content.as_ref().map(|c| c.warnings().to_vec()).unwrap_or_default());
        self.log(catalog.warnings.clone());
        self.data.set_catalog(catalog);
        self.preview_skin = None;
        self.data.preview = None;
        self.preview_textures.clear();
    }

    /// The import running, if any.
    pub fn importing(&self) -> bool {
        self.import.is_some()
    }

    /// The Import screen's Import: the job started on its own thread
    /// (one at a time; without a data directory, the reason shown).
    fn start_import(&mut self, path: PathBuf, out: &mut Vec<Outcome>) {
        if self.import.is_some() {
            return;
        }
        let Some(packs) = self.data.packs_dir.clone() else {
            self.data.import = ImportProgress::failed("there is no data directory to import into".into());
            for c in self.ui.update(UiAction::ImportFinished) {
                self.carry(c, out);
            }
            return;
        };
        let path = rl_ui::import::expand_typed_path(&path.to_string_lossy());
        println!("rl-game: importing {} into {}", path.display(), packs.display());
        self.data.import = ImportProgress::started();
        self.import = Some(ImportJob::spawn(path, packs));
    }

    /// The import's reports since the last frame; when it has ended, the
    /// content listed again and the menus told.
    fn poll_import(&mut self) -> Vec<Outcome> {
        let Some(job) = &mut self.import else { return Vec::new() };
        if !job.poll(&mut self.data.import) {
            return Vec::new();
        }
        self.import = None;
        match &self.data.import.result {
            Some(rl_ui::import::ImportResult::Failed(why)) => eprintln!("warning: import: {why}"),
            Some(r) => println!("rl-game: import done: {r:?}"),
            None => {}
        }
        self.list_content();
        self.act(UiAction::ImportFinished)
    }

    /// The profiles screen's skin preview (plan 7 ruling Q2: the menus
    /// hold no GPU handle, the app uploads): after the menus were drawn,
    /// the frames of the skin and colour they asked for
    /// (`MenuData::preview_wanted`) as egui textures, shown from the next
    /// frame; none while no screen asks.
    pub fn update_preview(&mut self, ctx: &egui::Context) {
        let wanted = self.data.preview_wanted.clone();
        if wanted.as_ref() == self.data.preview.as_ref().map(|t| &t.key) {
            return;
        }
        let Some(key) = wanted else {
            self.data.preview = None;
            self.preview_textures.clear();
            return;
        };
        let skin = self.preview_skin(&key);
        let frames = skin_preview::frames(&skin, key.colour);
        let mut textures = Vec::with_capacity(frames.len() * 2);
        let mut upload = |face, f: u32| {
            let img = frames.get(f, face);
            let size = [img.width() as usize, img.height() as usize];
            let image = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            let t = ctx.load_texture(format!("skin preview {f} {face:?}"), image, TextureOptions::NEAREST);
            let id = t.id();
            textures.push(t);
            id
        };
        let n = frames.len() as u32;
        let right = (0..n).map(|f| upload(rl_sim::Dir::Right, f)).collect();
        let left = (0..n).map(|f| upload(rl_sim::Dir::Left, f)).collect();
        self.preview_textures = textures;
        self.data.preview = Some(PreviewTextures { key, right, left });
    }

    /// The skin `key` names (as a match would load it: else the default
    /// skin, else a placeholder), loaded once while it stays the same.
    fn preview_skin(&mut self, key: &PreviewKey) -> LoadedSkin {
        if let Some((id, skin)) = &self.preview_skin
            && *id == key.skin_id
        {
            return skin.clone();
        }
        let skin = match &self.content {
            Some(c) => c.skin_or_default(&c.skin_id(&key.skin_id).unwrap_or_else(|| key.skin_id.clone())),
            None => LoadedSkin::placeholder(),
        };
        self.log(skin.warnings.clone());
        self.preview_skin = Some((key.skin_id.clone(), skin.clone()));
        skin
    }

    /// Games loaded so far (started or failed).
    pub fn launches(&self) -> u32 {
        self.launches
    }

    /// The pause menu holds the game.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The weapon selection is open: the player's weapons are not chosen
    /// yet (the HUD shows none).
    pub fn selecting(&self) -> bool {
        self.play.is_some() && self.ui.screen() == Screen::WeaponSelect
    }

    /// Worm names show over the game: in game and not paused, so never
    /// through the weapon selection, the pause menu or the results.
    pub fn names_shown(&self) -> bool {
        self.play.is_some() && self.ui.screen() == Screen::InGame && !self.paused
    }

    pub fn session(&self) -> Option<&Session> {
        self.play.as_ref().map(|p| &p.session)
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.play.as_mut().map(|p| &mut p.session)
    }

    /// The window's view changed.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.cfg.viewport = viewport;
        if let Some(s) = self.session_mut() {
            s.set_viewport(viewport);
        }
    }

    /// The player did `a` (or the app reports it): the menus' commands
    /// carried out; what the window must do.
    pub fn act(&mut self, a: UiAction) -> Vec<Outcome> {
        let mut out = Vec::new();
        for c in self.ui.update(a) {
            self.carry(c, &mut out);
        }
        out
    }

    fn carry(&mut self, c: AppCommand, out: &mut Vec<Outcome>) {
        match c {
            AppCommand::StartMatch(o) => self.pending = Some(Launch::Match(Box::new(o))),
            AppCommand::StartSandbox => self.pending = Some(Launch::Sandbox),
            AppCommand::PlayReplay(path) => self.pending = Some(Launch::Replay(path)),
            AppCommand::EndMatch => self.end(),
            AppCommand::SetPaused(on) => self.paused = on,
            AppCommand::ArmPlayer(weapons) => self.arm(weapons),
            AppCommand::SaveReplay => {
                let saved = self.keep_replay();
                for c in self.ui.update(UiAction::ReplaySaved(saved)) {
                    self.carry(c, out);
                }
            }
            AppCommand::Restart => self.restart(),
            AppCommand::ApplyVideo(v) => {
                if let Some((_, o)) = &mut self.overrides {
                    o.follow(&v);
                }
                self.set_interpolation(video::interpolation(v.interpolation));
                out.push(Outcome::Video(v));
            }
            AppCommand::ApplyAudio(a) => self.set_volume(&a),
            AppCommand::ApplyBindings(b) => out.push(Outcome::Bindings(b)),
            AppCommand::SaveSettings(mut s) => {
                if let Some((file, o)) = &self.overrides {
                    o.unsaved(file, &mut s.video);
                }
                self.unsaved_settings = Some(s);
            }
            AppCommand::SaveProfiles(p) => self.unsaved_profiles = Some(p),
            AppCommand::StartImport(path) => self.start_import(path, out),
            AppCommand::CancelImport => {
                if let Some(job) = &self.import {
                    job.cancel();
                }
            }
            AppCommand::Quit => out.push(Outcome::Quit),
        }
    }

    /// The import's progress (and, when it ends, the content listed
    /// again); then loads the game asked for in an earlier frame, if any,
    /// and reports to the menus whether it started.
    pub fn poll(&mut self) -> Vec<Outcome> {
        let mut out = self.poll_import();
        let Some(launch) = self.pending.take() else { return out };
        out.extend(self.launch_reported(launch));
        out
    }

    fn launch_reported(&mut self, launch: Launch) -> Vec<Outcome> {
        match self.launch(launch) {
            Ok(()) => {
                let mut out = self.act(UiAction::MatchStarted);
                // OLX skips the selection when there is no choice to make.
                if self.ui.screen() == Screen::WeaponSelect
                    && let Some(menu) = self.data.weapons.as_ref().filter(|m| !m.needs_choice())
                {
                    let weapons = WeaponSelect::new(menu, &[]).slots();
                    out.extend(self.act(UiAction::ChooseWeapons { weapons, names: Vec::new() }));
                }
                out.push(Outcome::Started);
                out
            }
            Err(e) => {
                let why = format!("{e:#}");
                eprintln!("warning: could not start: {why}");
                self.act(UiAction::MatchFailed(why))
            }
        }
    }

    /// Logs, once each, what the menus found wrong this frame (an
    /// unreadable level thumbnail).
    pub fn log_menu_warnings(&mut self) {
        let w = self.data.take_warnings();
        self.log(w);
    }

    fn log(&mut self, warnings: impl IntoIterator<Item = String>) {
        for w in warnings {
            if self.logged.insert(w.clone()) {
                eprintln!("warning: {w}");
            }
        }
    }

    fn launch(&mut self, launch: Launch) -> anyhow::Result<()> {
        self.launches += 1;
        let profile = self.ui.profiles().selected().cloned().unwrap_or_default();
        let roots = self.roots();
        let root: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let mut assets = match launch {
            Launch::Match(o) => setup::load_match(&root, &o, &profile.skin_id)?,
            Launch::Sandbox => {
                let m = &self.ui.settings().gameplay.match_prefs;
                let o = SandboxOptions {
                    mod_id: String::new(),
                    level_id: String::new(),
                    seed: 1,
                    targets: SANDBOX_TARGETS,
                    bonuses: m.bonuses,
                };
                setup::load(&root, &m.mod_id, &m.level_id, &profile.skin_id, o)?
            }
            Launch::Replay(path) => setup::load_replay(&root, &path, &profile.skin_id)?,
        };
        assets.player = Some(PlayerLook { name: profile.name, colour: profile.colour });
        self.log(std::mem::take(&mut assets.warnings));
        let mut session = Session::new(&assets, self.cfg.viewport, self.cfg.interpolation, Audio::silent())?;
        let mut audio = self.audio.take().unwrap_or_else(Audio::silent);
        audio.stop_all();
        audio.load(&assets.m, assets.content.core());
        self.log(audio.take_warnings());
        session.audio = audio;
        self.data.weapons = weapon_menu(&assets);
        self.data.results = None;
        self.play = Some(Playing { assets, session });
        self.paused = false;
        self.over_reported = false;
        Ok(())
    }

    /// The weapon selection's choice: the human (the local slot) starts
    /// with `weapons` (none enabled: -1s, the random loadout stays). The
    /// world is built again from the options with them; no tick has
    /// played. Weapons the match cannot be built with leave its options as
    /// they were.
    fn arm(&mut self, weapons: [i32; MAX_SLOTS]) {
        let Some(p) = &mut self.play else { return };
        let Some(slot) = p.session.local().map(usize::from) else { return };
        let Game::Deathmatch { options, .. } = &mut p.assets.game else { return };
        if weapons.iter().any(|&w| w < 0) {
            return;
        }
        let Some(mine) = options.players.get_mut(slot) else { return };
        let before = std::mem::replace(mine, Player::HumanWith(weapons));
        if let Err(e) = p.session.rebuild(&p.assets) {
            // Options `build_match` refuses would fail every rematch too:
            // the match keeps the ones it was built with.
            eprintln!("warning: arming the player: {e:#}");
            if let Game::Deathmatch { options, .. } = &mut p.assets.game
                && let Some(mine) = options.players.get_mut(slot)
            {
                *mine = before;
            }
        }
    }

    /// The results' Save replay: the match's replay as a file of its own
    /// in the data directory's `replays/` (never removed automatically).
    fn keep_replay(&mut self) -> Result<String, String> {
        let dir = self.store.replays_dir().ok_or("there is no data directory")?;
        let r = self.play.as_mut().and_then(|p| p.session.replay()).ok_or("there is no replay")?;
        let path = rl_ui::replays::save_kept(&dir, &r.encode()).map_err(|e| e.to_string())?;
        println!("rl-game: kept replay {}", path.display());
        Ok(path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))
    }

    /// One tick of the game playing, with the player holding `keys`,
    /// unless the pause menu or the weapon selection holds it (the
    /// replay records ticks only: a pause is not in it). Whether the
    /// world ticked (not held, nor a replay at its end).
    pub fn tick(&mut self, keys: Keys) -> bool {
        let held = self.paused || self.ui.screen() == Screen::WeaponSelect;
        self.session_mut().filter(|_| !held).is_some_and(|s| s.tick(keys))
    }

    /// The game ends: its replay saved, the sound output kept.
    fn end(&mut self) {
        if let Some(mut p) = self.play.take() {
            if let Some(r) = p.session.take_replay() {
                self.save_replay(&r);
            }
            let mut audio = std::mem::replace(&mut p.session.audio, Audio::silent());
            audio.stop_all();
            self.audio = Some(audio);
        }
        self.data.weapons = None;
        self.data.results = None;
        self.paused = false;
        self.over_reported = false;
    }

    /// F5: the game again in place (a Deathmatch's rematch, its replay saved).
    fn restart(&mut self) {
        let Some(p) = &mut self.play else { return };
        match p.session.restart(&p.assets) {
            Ok(Some(r)) => self.save_replay(&r),
            Ok(None) => {}
            Err(e) => eprintln!("warning: restarting: {e:#}"),
        }
        self.over_reported = false;
    }

    /// Saves `r` with the automatic replays (one of no tick is not kept).
    fn save_replay(&mut self, r: &rl_replay::Replay) {
        let Some(dir) = self.store.replays_dir().filter(|_| r.ticks() > 0) else { return };
        match rl_ui::replays::auto_save(&dir, &r.encode()) {
            Ok(path) => println!("rl-game: saved replay {} ({} ticks)", path.display(), r.ticks()),
            Err(e) => eprintln!("warning: saving the replay in {}: {e}", dir.display()),
        }
    }

    /// The command line's video flags (`overrides`) win over `file`, the
    /// options as loaded: the menus show them (the caller made the
    /// `UiState` with them applied), and the file keeps its own values
    /// until the player changes those options.
    pub fn set_overrides(&mut self, file: VideoSettings, overrides: Overrides) {
        self.overrides = Some((file, overrides));
    }

    /// How games are drawn between ticks (new games, and the one playing).
    pub fn interpolation(&self) -> Interpolation {
        self.cfg.interpolation
    }

    fn set_interpolation(&mut self, i: Interpolation) {
        self.cfg.interpolation = i;
        if let Some(s) = self.session_mut() {
            s.present.scene.options.interpolation = i;
        }
    }

    /// The sound output's volume now (the game's, else the menus').
    pub fn volume(&self) -> f32 {
        match (self.session(), &self.audio) {
            (Some(s), _) => s.audio.volume(),
            (None, Some(a)) => a.volume(),
            (None, None) => 0.0,
        }
    }

    /// The sound volume: master times effects (music has none yet).
    fn set_volume(&mut self, a: &AudioSettings) {
        let v = a.master * a.sfx;
        if let Some(audio) = &mut self.audio {
            audio.set_volume(v);
        }
        if let Some(s) = self.session_mut() {
            s.audio.set_volume(v);
        }
    }

    /// After the ticks of a frame: tells the menus once when the results
    /// show (`Session::results_shown`).
    pub fn after_ticks(&mut self) -> Vec<Outcome> {
        let shown = self.session().is_some_and(Session::results_shown);
        if !shown || self.over_reported {
            return Vec::new();
        }
        self.over_reported = true;
        self.data.results = self.session().and_then(|s| s.results.clone());
        self.act(UiAction::MatchOver)
    }

    /// Writes the settings and profiles changed since the last call (the
    /// app calls it about once a second, so a dragged slider is not saved
    /// every frame); a failure is a warning.
    pub fn flush(&mut self) {
        if let Some(s) = self.unsaved_settings.take()
            && let Err(e) = self.store.save_settings(&s)
        {
            eprintln!("warning: saving the settings: {e:#}");
        }
        if let Some(p) = self.unsaved_profiles.take()
            && let Err(e) = self.store.save_profiles(&p)
        {
            eprintln!("warning: saving the profiles: {e:#}");
        }
    }

    /// At exit: the playing game's replay saved, the settings written, an
    /// import stopped after its item (waited for at most
    /// [`IMPORT_STOP_WAIT`]; a pack it was writing stays hidden).
    pub fn finish(&mut self) {
        if let Some(mut job) = self.import.take()
            && !job.stop(IMPORT_STOP_WAIT)
        {
            eprintln!("warning: quitting during an import: its last pack is not imported");
        }
        self.end();
        self.flush();
    }
}

/// The weapon selection's menu for `a`'s game: a Deathmatch's mod weapons
/// with its restrictions (`None` for another game).
pub fn weapon_menu(a: &Assets) -> Option<WeaponMenu> {
    let Game::Deathmatch { options, .. } = &a.game else { return None };
    let rules = a.m.weapon_rules(&options.restrictions);
    let names: Vec<String> = a.m.sim.weapons().iter().map(|w| w.name.clone()).collect();
    let enabled = names.iter().map(|n| rules.is_enabled(n)).collect();
    Some(WeaponMenu { names, enabled, fallback: rules.find_enabled_weapon(), seed: options.seed })
}
