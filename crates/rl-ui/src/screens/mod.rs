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
mod host;
mod import;
mod ingame;
mod keys;
mod lobby;
mod local_play;
mod main;
mod multiplayer;
mod options;
mod profiles;
mod replays;
pub mod results;
mod weapon_select;

pub use keys::GAME_KEYS;

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{
    Align, Button, Color32, Event, Key, Label, Layout, Rect, RichText, Sense, TextureHandle, TextureOptions,
    Ui, UiBuilder, Vec2, pos2, vec2,
};
use rl_content::Catalog;
use rl_input::Capture;
use rl_sim::{Keys, MAX_SLOTS};

use crate::hud::LogLine;
use crate::hud::Results;
use crate::import::ImportProgress;
use crate::online::{DownloadCacheView, InternetView, LanEntry, LobbyView};
use crate::replays::ReplayFile;
use crate::skin_preview::{PreviewKey, PreviewTextures};
use crate::state::{Screen, UiAction, UiState};
use crate::theme;
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
    /// What the player should know of their files (`Store::notices`: a
    /// newer game's settings left untouched), shown on the main menu.
    pub notices: Vec<String>,
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
    /// The Import screen's path field (the last path imported when it
    /// first opens).
    pub import_path: String,
    /// Where imports go: the data directory's `packs/` (none: no import).
    pub packs_dir: Option<PathBuf>,
    /// The import running or done (the app updates it every frame).
    pub import: ImportProgress,
    /// The games announced on the network (the app keeps it current).
    pub lan_games: Vec<LanEntry>,
    /// The games announced on the Internet (the app keeps it current
    /// while the Multiplayer screen shows).
    pub internet: InternetView,
    /// The Internet game the player selected (the app measures its ping).
    pub selected_game: Option<[u8; 32]>,
    /// The game's relays and STUN servers when the settings name none (the
    /// options show them; their Reset returns to them).
    pub default_relays: Vec<String>,
    pub default_stun: Vec<String>,
    /// The lobby the player is in (the app updates it every frame).
    pub lobby: Option<LobbyView>,
    /// The lobby's chat, oldest first.
    pub chat: Vec<LogLine>,
    /// What the online screens say besides errors (the app's status).
    pub online_status: Option<String>,
    /// The in-game chat's line while its field is open (`T` online; the
    /// app sends no key to the game meanwhile).
    pub ingame_chat: Option<String>,
    /// The options' download cache row (the app keeps it current while the
    /// options show); `Err(why)`: there is none.
    pub download_cache: Result<DownloadCacheView, String>,
    /// A text field had the focus as the last frame ended.
    typing: bool,
    /// The weapon selection in progress.
    select: Option<WeaponSelect>,
    /// The thumbnails decoded so far by level id (`None`: unreadable, its
    /// placeholder stays; tried once).
    thumbs: BTreeMap<String, Option<TextureHandle>>,
    /// Decodes left this frame ([`THUMB_DECODES_PER_FRAME`]).
    decodes_left: usize,
    /// Problems for the app to log (`take_warnings`).
    warnings: Vec<String>,
    multiplayer: multiplayer::View,
    host: host::View,
    lobby_ui: lobby::View,
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
            notices: Vec::new(),
            preview_wanted: None,
            preview: None,
            capture: None,
            weapons: None,
            held: Keys(0),
            results: None,
            import_path: String::new(),
            packs_dir: None,
            import: ImportProgress::default(),
            lan_games: Vec::new(),
            internet: InternetView::default(),
            selected_game: None,
            default_relays: Vec::new(),
            default_stun: Vec::new(),
            lobby: None,
            chat: Vec::new(),
            online_status: None,
            ingame_chat: None,
            download_cache: Err("there is no download cache".into()),
            typing: false,
            select: None,
            thumbs: BTreeMap::new(),
            decodes_left: THUMB_DECODES_PER_FRAME,
            warnings: Vec::new(),
            multiplayer: multiplayer::View::default(),
            host: host::View::default(),
            lobby_ui: lobby::View::default(),
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
                self.warnings.push(format!("level pack {id}: {}: {e}, not shown", r.path().display()));
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
    // Esc in a text field only leaves the field (egui lets go of it at the
    // start of the pass, so the field was in focus as the last frame ended).
    let typing =
        data.typing && matches!(screen, Screen::Lobby | Screen::Multiplayer | Screen::Host | Screen::InGame);
    data.preview_wanted = None;
    data.decodes_left = THUMB_DECODES_PER_FRAME;
    if data.shown != Some(screen) {
        let from = data.shown.replace(screen);
        match screen {
            // Back from Host or the lobby keeps what was typed.
            Screen::Multiplayer if !matches!(from, Some(Screen::Host | Screen::Lobby)) => {
                data.multiplayer = multiplayer::View::new(&state.settings().online.last_address);
            }
            Screen::Host => data.host = host::View::new(state),
            // The weapon screen is a visit: the chat draft stays.
            Screen::Lobby if from != Some(Screen::WeaponSelect) => data.lobby_ui = lobby::View::default(),
            Screen::Replays => data.refresh_replays(),
            Screen::Profiles => data.profiles = profiles::View::new(state.profiles().selected),
            Screen::Options => data.options = options::View::default(),
            Screen::Import if data.import_path.is_empty() => {
                data.import_path = state.settings().import.last_path.clone();
            }
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
    if screen != Screen::InGame {
        data.ingame_chat = None;
    }
    // The capture takes the key: egui must not also act on it (Space
    // pressing the focused button, Tab moving the focus, Esc leaving).
    if data.capture.is_some() {
        ui.ctx().input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_))));
    }
    // A dialog of the options is open: Esc closes it, and stays.
    let dialog = screen == Screen::Options && data.options.keys.busy(data.capture.is_some());
    match screen {
        Screen::InGame => ingame::show(ui, state, data, &mut out),
        Screen::WeaponSelect => weapon_select::show(ui, data, &mut out),
        Screen::Results => results::show(ui, state, data, &mut out),
        _ => {
            let frame = egui::Frame::central_panel(ui.style()).fill(theme::BACKGROUND);
            egui::CentralPanel::default().frame(frame).show(ui, |ui| match screen {
                Screen::Main => main::show(ui, state, &data.notices, &mut out),
                Screen::LocalPlay => local_play::show(ui, state, data, &mut out),
                Screen::Profiles => profiles::show(ui, state, data, &mut out),
                Screen::Replays => replays::show(ui, state, data, &mut out),
                Screen::Options => options::show(ui, state, data, &mut out),
                Screen::About => about::show(ui, data, &mut out),
                Screen::Import => import::show(ui, state, data, &mut out),
                Screen::Multiplayer => multiplayer::show(ui, state, data, &mut out),
                Screen::Host => host::show(ui, state, data, &mut out),
                Screen::Lobby => lobby::show(ui, state, data, &mut out),
                _ => {}
            });
        }
    }
    let (esc, f5) = (pressed_once(ui, Key::Escape), pressed_once(ui, Key::F5));
    if esc && !dialog && !typing {
        out.push(UiAction::Back);
    }
    if f5 {
        out.push(UiAction::Restart);
    }
    data.typing = ui.ctx().text_edit_focused();
    if screen == Screen::Lobby {
        lobby::intercept_back(data, &mut out);
    }
    out
}

/// The space a page's column keeps from the window's sides when the
/// window is narrower than the column.
const PAGE_MARGIN: f32 = 24.0;

/// The tallest a page's column grows: on a tall window it is centred,
/// not stretched from the top to the bottom.
const PAGE_HEIGHT: f32 = 900.0;

/// Back's size in a page's title row.
const BACK: Vec2 = Vec2::new(96.0, 34.0);

/// Starts a page, the frame every screen of the main menu shares: a
/// column at most `width` points wide and [`PAGE_HEIGHT`] high centred in
/// `ui` (the window less a margin on each side when narrower), its title centred on top with Back
/// at the column's left edge (Esc is Back too: `show`). Returns the
/// column to draw the page into: its primary action first (`footer`),
/// then its body, the sections (`section`).
fn page(ui: &mut Ui, title: &str, width: f32, out: &mut Vec<UiAction>) -> Ui {
    let full = ui.max_rect();
    let w = width.min(full.width() - 2.0 * PAGE_MARGIN).max(0.0);
    let h = full.height().clamp(0.0, PAGE_HEIGHT);
    let column = Rect::from_center_size(full.center(), vec2(w, h));
    let mut col = ui.new_child(
        UiBuilder::new().id_salt(("page", title)).max_rect(column).layout(Layout::top_down(Align::Min)),
    );
    let (row, _) = col.allocate_exact_size(vec2(w, BACK.y + 6.0), Sense::hover());
    let heading = Label::new(RichText::new(title).heading().strong()).selectable(false);
    col.place(row, heading);
    let back = Rect::from_min_size(pos2(row.left(), row.center().y - BACK.y / 2.0), BACK);
    if col.place(back, Button::new(RichText::new("Back").size(17.0))).clicked() {
        out.push(UiAction::Back);
    }
    col.spacing_mut().slider_width = 180.0;
    col.add_space(4.0);
    col.separator();
    col.add_space(8.0);
    col
}

/// A page's primary action and its status, centred at the bottom of the
/// column (`page`). Returns the rest of the column, for the body.
fn footer(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) -> Ui {
    egui::Panel::bottom(egui::Id::new(("page footer", id)))
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .resizable(false)
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                add(ui);
                ui.add_space(8.0);
            })
        });
    let rest = ui.available_rect_before_wrap();
    ui.new_child(
        UiBuilder::new().id_salt(("page body", id)).max_rect(rest).layout(Layout::top_down(Align::Min)),
    )
}

/// Splits what is left of `ui` into `n` columns side by side, as tall as
/// what is left (egui's `columns` reach down to the bottom of the
/// `ui`, past a footer).
fn columns<R>(ui: &mut Ui, n: usize, add: impl FnOnce(&mut [Ui]) -> R) -> R {
    let rest = ui.available_rect_before_wrap();
    let gap = 12.0;
    let w = ((rest.width() - gap * (n as f32 - 1.0)) / n as f32).max(0.0);
    let mut cols: Vec<Ui> = (0..n)
        .map(|i| {
            let min = rest.min + vec2(i as f32 * (w + gap), 0.0);
            let r = Rect::from_min_size(min, vec2(w, rest.height()));
            let b = UiBuilder::new().id_salt(("column", i)).max_rect(r).layout(Layout::top_down(Align::Min));
            ui.new_child(b)
        })
        .collect();
    let r = add(&mut cols);
    let bottom = cols.iter().map(|c| c.min_rect().bottom()).fold(rest.top(), f32::max);
    ui.advance_cursor_after_rect(Rect::from_min_max(rest.min, pos2(rest.right(), bottom)));
    r
}

/// A page's primary action: a large button.
fn primary(ui: &mut Ui, text: &str, enabled: bool) -> egui::Response {
    let text = RichText::new(text).size(20.0).color(Color32::WHITE);
    let button = Button::new(text).min_size(vec2(220.0, 44.0)).fill(theme::PRIMARY);
    ui.add_enabled(enabled, button)
}

/// A group of a page's widgets in a panel (`theme::section_frame`), the
/// full width of where it is, under its `title`; `add` gets the title's
/// id (a single picker may be labelled by it).
fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    framed(ui, title, None, add)
}

/// A [`section`] reaching down to the bottom of where it is (panels side
/// by side end on one line).
fn tall_section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    let margin = theme::section_frame().total_margin().sum().y;
    let h = (ui.available_height() - margin).max(0.0);
    framed(ui, title, Some(h), add)
}

fn framed<R>(ui: &mut Ui, title: &str, height: Option<f32>, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    theme::section_frame()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(h) = height {
                ui.set_min_height(h);
                ui.set_max_height(h);
            }
            let t = ui.label(RichText::new(title).size(17.0).strong().color(theme::ACCENT));
            ui.add_space(2.0);
            add(ui, t.id)
        })
        .inner
}

/// The width of a form's names (`form_row`).
const FORM_LABEL: f32 = 150.0;

/// One row of a form: `name` right-aligned in a column [`FORM_LABEL`]
/// wide, then the control `add` makes (given the name's id to be labelled
/// by), so a form's controls all start on one line.
fn form_row<R>(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    form_line(ui, |ui| {
        let label = form_name(ui, |ui| ui.label(name));
        add(ui, label.id)
    })
}

/// A form's row whose name is a checkbox switching the control on
/// (`on`): `add` gets whether it is on now and whether it just changed.
fn form_check_row<R>(ui: &mut Ui, on: bool, name: &str, add: impl FnOnce(&mut Ui, bool, bool) -> R) -> R {
    form_line(ui, |ui| {
        let mut on = on;
        let changed = form_name(ui, |ui| ui.checkbox(&mut on, name)).changed();
        add(ui, on, changed)
    })
}

/// A form row's height: a button's, so the name and a picker share a line.
fn form_height(ui: &Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y
}

/// A form's row, its widgets centred on one line [`form_height`] high
/// (egui's `horizontal` starts a row lower, and centres on that).
fn form_line<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let size = vec2(ui.available_width(), form_height(ui));
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), add).inner
}

/// A form's name cell: [`FORM_LABEL`] wide, its widget right-aligned.
fn form_name(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> egui::Response) -> egui::Response {
    let h = form_height(ui);
    let right = Layout::right_to_left(Align::Center);
    ui.allocate_ui_with_layout(vec2(FORM_LABEL, h), right, |ui| {
        ui.set_min_width(FORM_LABEL);
        add(ui)
    })
    .inner
}

/// Why the last start failed, in red.
fn message(ui: &mut Ui, state: &UiState) {
    if let Some(m) = state.message() {
        ui.label(RichText::new(m).color(Color32::from_rgb(255, 110, 90)));
    }
}

/// The playfield in game (the window above the HUD bar), and the offset
/// of its centre from the window's: the in-game panels (weapons, pause,
/// results) anchor there, centred over the picture.
pub(crate) fn playfield(ctx: &egui::Context) -> (Rect, Vec2) {
    let screen = ctx.content_rect();
    let field = crate::hud::playfield(screen);
    (field, field.center() - screen.center())
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
