//! The menus' egui widgets (spec §8.3, §21 G1). Each screen draws from
//! the [`UiState`] and returns [`UiAction`]s; the caller feeds them to
//! [`UiState::update`] and carries out the commands. No GPU handle: level
//! thumbnails are egui textures the app's renderer uploads like egui's own,
//! decoded only when their card shows, at most
//! [`THUMB_DECODES_PER_FRAME`] a frame, and kept.
//!
//! Keys: Esc is Back on every screen (in game it toggles the pause menu;
//! in the options it first closes an open dialog), F5 restarts the game in
//! place; a held key's repeats do neither. While the options capture a
//! key, egui hears no key at all: the capture has it alone.

pub mod about;
mod ingame;
mod keys;
mod local_play;
mod main;
mod options;
mod profiles;
mod replays;
pub mod results;
mod weapon_select;

pub use keys::GAME_KEYS;

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{Color32, Event, Key, RichText, TextureHandle, TextureOptions, Ui};
use rl_content::Catalog;
use rl_input::Capture;
use rl_sim::{Keys, MAX_SLOTS};

use crate::hud::Results;
use crate::replays::ReplayFile;
use crate::skin_preview::{PreviewKey, PreviewTextures};
use crate::state::{Screen, UiAction, UiState};
use crate::weapon_select::{WeaponMenu, WeaponSelect};

/// The most level thumbnails decoded in one frame; the rest show their
/// placeholder until a later frame (a long list never stalls a frame).
pub const THUMB_DECODES_PER_FRAME: usize = 2;

/// What the menus show besides the state: the content's mods, levels and
/// skins, the saved replays, the skin preview, and the widgets' own state.
pub struct MenuData {
    pub catalog: Catalog,
    /// The data directory's `replays/` (none without a data directory).
    pub replays_dir: Option<PathBuf>,
    /// The files in it, read when the replays screen opens.
    pub replays: Vec<ReplayFile>,
    /// The replays screen's path field.
    pub replay_path: String,
    /// The skin preview the profiles screen showed this frame (`None` on
    /// other screens): the app makes it and puts it in `preview`.
    pub preview_wanted: Option<PreviewKey>,
    /// The app's preview textures (plan 7 ruling Q2: the app uploads them).
    pub preview: Option<PreviewTextures>,
    /// The options' "press a key" box while it is open: the app feeds it
    /// every key press and gamepad event (`Capture::on_window_event`,
    /// `on_gamepad`); the options read the input from it.
    pub capture: Option<Capture>,
    /// The match's weapons for the weapon selection (the app sets them
    /// when a Deathmatch starts).
    pub weapons: Option<WeaponMenu>,
    /// The game's keys the player holds now (the app feeds them every
    /// frame): the weapon selection reads them, keyboard or gamepad.
    pub held: Keys,
    /// The finished match's results (the app sets them when they show).
    pub results: Option<Results>,
    /// The weapon selection in progress.
    select: Option<WeaponSelect>,
    /// The thumbnails decoded so far by level id (`None`: unreadable, its
    /// placeholder stays; tried once).
    thumbs: BTreeMap<String, Option<TextureHandle>>,
    /// Decodes left this frame ([`THUMB_DECODES_PER_FRAME`]).
    decodes_left: usize,
    /// Problems for the app to log (`take_warnings`).
    warnings: Vec<String>,
    profiles: profiles::View,
    options: options::View,
    /// The screen shown last frame (a screen reads its files on entry).
    shown: Option<Screen>,
}

impl MenuData {
    pub fn new(catalog: Catalog, replays_dir: Option<PathBuf>) -> MenuData {
        MenuData {
            catalog,
            replays_dir,
            replays: Vec::new(),
            replay_path: String::new(),
            preview_wanted: None,
            preview: None,
            capture: None,
            weapons: None,
            held: Keys(0),
            results: None,
            select: None,
            thumbs: BTreeMap::new(),
            decodes_left: THUMB_DECODES_PER_FRAME,
            warnings: Vec::new(),
            profiles: profiles::View::default(),
            options: options::View::default(),
            shown: None,
        }
    }

    /// The content changed (an import): new lists, thumbnails made again.
    pub fn set_catalog(&mut self, catalog: Catalog) {
        self.catalog = catalog;
        self.thumbs.clear();
    }

    /// The weapons the selection shows now, by mod id (`None` when no
    /// selection is open).
    pub fn weapon_choice(&self) -> Option<[i32; MAX_SLOTS]> {
        self.select.as_ref().map(WeaponSelect::slots)
    }

    /// Reads the replays directory again.
    pub fn refresh_replays(&mut self) {
        self.replays = self.replays_dir.as_deref().map(crate::replays::list).unwrap_or_default();
    }

    /// Level `id`'s thumbnail as an egui texture, decoded on first use
    /// (the caller asks only for cards in view): `None` while it has
    /// none, cannot be read, or this frame's decodes are spent (then
    /// another frame is asked for).
    fn thumb(&mut self, ctx: &egui::Context, id: &str) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(id) {
            return t.clone();
        }
        let r = self.catalog.level(id)?.thumb.clone()?;
        if self.decodes_left == 0 {
            ctx.request_repaint();
            return None;
        }
        self.decodes_left -= 1;
        let t = match r.load() {
            Ok(img) => {
                let size = [img.width() as usize, img.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                Some(ctx.load_texture(format!("thumb {id}"), image, TextureOptions::LINEAR))
            }
            Err(e) => {
                self.warnings.push(format!("level pack {id}: {}: {e}, not shown", r.path.display()));
                None
            }
        };
        self.thumbs.insert(id.to_string(), t.clone());
        t
    }

    /// How many thumbnails have been decoded (or found unreadable) so far.
    pub fn thumbs_decoded(&self) -> usize {
        self.thumbs.len()
    }

    /// The problems found since the last call (unreadable thumbnails), for
    /// the app to log once.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

/// Draws the screen `state` shows into `ui` (the whole window) and
/// returns what the player did, in order.
pub fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData) -> Vec<UiAction> {
    let mut out = Vec::new();
    let screen = state.screen();
    data.preview_wanted = None;
    data.decodes_left = THUMB_DECODES_PER_FRAME;
    if data.shown != Some(screen) {
        data.shown = Some(screen);
        match screen {
            Screen::Replays => data.refresh_replays(),
            Screen::Profiles => data.profiles = profiles::View::new(state.profiles().selected),
            Screen::Options => data.options = options::View::default(),
            Screen::WeaponSelect => {
                let remembered = state.profiles().selected().map(|p| p.weapons.as_slice()).unwrap_or(&[]);
                data.select = data.weapons.as_ref().map(|m| WeaponSelect::new(m, remembered));
            }
            _ => {}
        }
    }
    if screen != Screen::Options {
        data.capture = None;
    }
    if screen != Screen::WeaponSelect {
        data.select = None;
    }
    // The capture takes the key: egui must not also act on it (Space
    // pressing the focused button, Tab moving the focus, Esc leaving).
    if data.capture.is_some() {
        ui.ctx().input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_))));
    }
    // A dialog of the options is open: Esc closes it, and stays.
    let dialog = screen == Screen::Options && data.options.keys.busy(data.capture.is_some());
    match screen {
        Screen::InGame => ingame::show(ui, state, &mut out),
        Screen::WeaponSelect => weapon_select::show(ui, data, &mut out),
        Screen::Results => results::show(ui, state, data, &mut out),
        _ => {
            let frame = egui::Frame::central_panel(ui.style()).fill(Color32::from_rgb(16, 16, 20));
            egui::CentralPanel::default().frame(frame).show(ui, |ui| match screen {
                Screen::Main => main::show(ui, state, &mut out),
                Screen::LocalPlay => local_play::show(ui, state, data, &mut out),
                Screen::Profiles => profiles::show(ui, state, data, &mut out),
                Screen::Replays => replays::show(ui, state, data, &mut out),
                Screen::Options => options::show(ui, state, data, &mut out),
                Screen::About => about::show(ui, data, &mut out),
                _ => placeholder(ui, screen, &mut out),
            });
        }
    }
    let (esc, f5) = (pressed_once(ui, Key::Escape), pressed_once(ui, Key::F5));
    if esc && !dialog {
        out.push(UiAction::Back);
    }
    if f5 {
        out.push(UiAction::Restart);
    }
    out
}

/// A screen's title row with its Back button.
fn header(ui: &mut Ui, title: &str, out: &mut Vec<UiAction>) {
    ui.horizontal(|ui| {
        if ui.button("Back").clicked() {
            out.push(UiAction::Back);
        }
        ui.add_space(12.0);
        ui.heading(title);
    });
    ui.separator();
}

/// Why the last start failed, in red.
fn message(ui: &mut Ui, state: &UiState) {
    if let Some(m) = state.message() {
        ui.label(RichText::new(m).color(Color32::from_rgb(255, 110, 90)));
    }
}

/// The screens later tasks fill (import).
fn placeholder(ui: &mut Ui, screen: Screen, out: &mut Vec<UiAction>) {
    let title = match screen {
        Screen::Import => "Import",
        _ => "",
    };
    header(ui, title, out);
    ui.label(RichText::new("Not available yet.").weak());
}

/// `key` went down this frame: a press, not a held key's repeat (egui's
/// `key_pressed` counts repeats, which made a held Esc flicker the pause).
pub(crate) fn pressed_once(ui: &Ui, key: Key) -> bool {
    ui.input(|i| {
        i.events
            .iter()
            .any(|e| matches!(e, Event::Key { key: k, pressed: true, repeat: false, .. } if *k == key))
    })
}
