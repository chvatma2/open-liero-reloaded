//! The menus' egui widgets (spec §8.3, §21 G1). Each screen draws from
//! the [`UiState`] and returns [`UiAction`]s; the caller feeds them to
//! [`UiState::update`] and carries out the commands. No GPU handle: level
//! thumbnails are egui textures the app's renderer uploads like egui's own.
//!
//! Keys: Esc is Back on every screen (in game it toggles the pause menu;
//! in the options it first closes an open dialog), F5 restarts the game in
//! place.

mod keys;
mod local_play;
mod main;
mod options;
mod profiles;
mod replays;

pub use keys::GAME_KEYS;

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{Align2, Color32, Key, RichText, TextureHandle, TextureOptions, Ui};
use rl_content::Catalog;
use rl_input::Capture;

use crate::replays::ReplayFile;
use crate::skin_preview::{PreviewKey, PreviewTextures};
use crate::state::{Screen, UiAction, UiState};

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
    thumbs: BTreeMap<String, TextureHandle>,
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
            thumbs: BTreeMap::new(),
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

    /// Reads the replays directory again.
    pub fn refresh_replays(&mut self) {
        self.replays = self.replays_dir.as_deref().map(crate::replays::list).unwrap_or_default();
    }

    /// Level `id`'s thumbnail as an egui texture, made on first use.
    fn thumb(&mut self, ctx: &egui::Context, id: &str) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(id) {
            return Some(t.clone());
        }
        let img = self.catalog.level(id)?.thumb.as_ref()?;
        let size = [img.width() as usize, img.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        let t = ctx.load_texture(format!("thumb {id}"), image, TextureOptions::NEAREST);
        self.thumbs.insert(id.to_string(), t.clone());
        Some(t)
    }
}

/// Draws the screen `state` shows into `ui` (the whole window) and
/// returns what the player did, in order.
pub fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData) -> Vec<UiAction> {
    let mut out = Vec::new();
    let screen = state.screen();
    data.preview_wanted = None;
    if data.shown != Some(screen) {
        data.shown = Some(screen);
        match screen {
            Screen::Replays => data.refresh_replays(),
            Screen::Profiles => data.profiles = profiles::View::new(state.profiles().selected),
            Screen::Options => data.options = options::View::default(),
            _ => {}
        }
    }
    if screen != Screen::Options {
        data.capture = None;
    }
    // A dialog of the options is open: Esc closes it, and stays.
    let dialog = screen == Screen::Options && data.options.keys.busy(data.capture.is_some());
    match screen {
        Screen::InGame | Screen::WeaponSelect | Screen::Results => in_game(ui, state, &mut out),
        _ => {
            let frame = egui::Frame::central_panel(ui.style()).fill(Color32::from_rgb(16, 16, 20));
            egui::CentralPanel::default().frame(frame).show(ui, |ui| match screen {
                Screen::Main => main::show(ui, state, &mut out),
                Screen::LocalPlay => local_play::show(ui, state, data, &mut out),
                Screen::Profiles => profiles::show(ui, state, data, &mut out),
                Screen::Replays => replays::show(ui, state, data, &mut out),
                Screen::Options => options::show(ui, state, data, &mut out),
                _ => placeholder(ui, screen, &mut out),
            });
        }
    }
    let (esc, f5) = ui.input(|i| (i.key_pressed(Key::Escape), i.key_pressed(Key::F5)));
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

/// The screens later milestones fill (import, about).
fn placeholder(ui: &mut Ui, screen: Screen, out: &mut Vec<UiAction>) {
    let title = match screen {
        Screen::Import => "Import",
        Screen::About => "About",
        _ => "",
    };
    header(ui, title, out);
    ui.label(RichText::new("Not available yet.").weak());
}

/// Over the match: the pause menu, and on the results a way back.
fn in_game(ui: &mut Ui, state: &UiState, out: &mut Vec<UiAction>) {
    let (body, results): (&str, bool) = match state.screen() {
        Screen::InGame if state.is_paused() => ("Paused", false),
        Screen::Results => ("", true),
        _ => return,
    };
    let area = egui::Area::new(egui::Id::new("pause menu"));
    let area = if results {
        area.anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -48.0))
    } else {
        area.anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
    };
    area.show(ui.ctx(), |ui| {
        egui::Frame::new().fill(Color32::from_black_alpha(210)).corner_radius(6.0).inner_margin(16.0).show(
            ui,
            |ui| {
                ui.vertical_centered(|ui| {
                    if results {
                        if ui.button("Leave").clicked() {
                            out.push(UiAction::LeaveMatch);
                        }
                        return;
                    }
                    ui.heading(body);
                    ui.add_space(8.0);
                    if ui.add_sized([200.0, 32.0], egui::Button::new("Resume")).clicked() {
                        out.push(UiAction::Resume);
                    }
                    if ui.add_sized([200.0, 32.0], egui::Button::new("Leave match")).clicked() {
                        out.push(UiAction::LeaveMatch);
                    }
                });
            },
        );
    });
}
