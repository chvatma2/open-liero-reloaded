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
//!
//! Pads: `crate::pad` turns a pad's d-pad, left stick, A and B into
//! egui's focus keys ([`MenuData::pad_events`], issue #31).

pub mod about;
mod editor;
mod host;
mod host_lost;
mod ingame;
mod join;
mod keys;
mod loadouts;
mod lobby;
mod local_play;
mod main;
mod multiplayer;
mod options;
mod profiles;
mod replays;
pub mod report;
pub mod results;
pub mod update;
mod weapon_select;

pub use host_lost::{HOST_LOST_REPLAY, HOST_LOST_TITLE, STEPPED_DOWN_TITLE};
pub use join::{JOIN_HELP, JOIN_KEYBOARD_1, PRESS_TO_JOIN};
pub use keys::{
    GAME_KEYS, MOUSE_CONTROLS, MOUSE_KEYS_NOTE, MOUSE_MADE, MOUSE_NOTE, RESET_KEYS, RESET_KEYS2, RESET_MOUSE,
    RESET_PAD, seat_note, shared_note,
};
pub use loadouts::{NO_WEAPON, READ_ONLY, UNIQUE_HINT};
pub use local_play::JOIN_HINT;
pub use weapon_select::WAITING;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use egui::{
    Align, Button, Color32, Event, Key, Label, Layout, Rect, RichText, Sense, TextureHandle, TextureOptions,
    Ui, UiBuilder, Vec2, pos2, vec2,
};
use olr_content::Catalog;
use olr_input::{Capture, Seat};
use olr_sim::{Keys, MAX_SLOTS};

use crate::hud::LogLine;
use crate::hud::Results;
use crate::join::{JoinStep, SeatInput};
use crate::online::{DownloadCacheView, InternetView, LanEntry, LobbyView};
use crate::pad::PadNav;
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
    /// Each local player's keys held now, in local order (issue #32: the
    /// app feeds them every frame; empty: `held` is the one player's):
    /// each player's weapon selection reads its own.
    pub held_by: Vec<Keys>,
    /// Every seat's input now (keyboard 1, keyboard 2, each pad heard;
    /// [`MenuData::set_seats`], the app's, every frame): the join step
    /// reads them.
    pub seats: Vec<SeatInput>,
    /// The seats that pressed something this run: local play's Start
    /// opens the join step when two or more did (issue #32).
    pub seats_used: BTreeSet<Seat>,
    /// The finished match's results (the app sets them when they show).
    pub results: Option<Results>,
    /// The views the game draws (the app's, every frame: 1 for one
    /// player, 2 to 4 in split screen, a split replay's too): with two or
    /// more the pause menu and the results centre on the window, not on
    /// the solo playfield (issue #49).
    pub views: usize,
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
    /// Where a join stands while the menus wait for it (the app's
    /// `olr_online::JoinStep` in words: "Asking the game through the
    /// relays…"); `None`: "Connecting…".
    pub joining: Option<String>,
    /// The in-game chat's line while its field is open (`T` online; the
    /// app sends no key to the game meanwhile).
    pub ingame_chat: Option<String>,
    /// The options' download cache row (the app keeps it current while the
    /// options show); `Err(why)`: there is none.
    pub download_cache: Result<DownloadCacheView, String>,
    /// Why the online match's host was lost (plan 12c): the Host lost
    /// screen's reason, set by the app with [`UiAction::HostLost`].
    pub host_lost: Option<String>,
    /// That match's replay is saved (the app sets it once the file is
    /// written; a watcher that joined mid-match has none): the Host lost
    /// screen says it is in Replays.
    pub host_lost_replay: bool,
    /// This player was the match's host and stepped down (its game
    /// stalled, N70): the screen's title is [`STEPPED_DOWN_TITLE`].
    pub host_lost_hosting: bool,
    /// The level packs of the data directory's `packs/` (the Editor
    /// screen's Edit; every other level is edited as a copy), by id.
    pub editable: BTreeSet<String>,
    /// The editor's recovery copies: (id, level name, damaged).
    pub recoveries: Vec<(String, String, bool)>,
    /// The skin packs of the data directory's `packs/` (the Skin tab's
    /// Edit; every other skin is edited as a copy, issue #14), by id.
    pub editable_skins: BTreeSet<String>,
    /// The mod packs of the data directory's `packs/` (the Mod tab's Edit;
    /// every mod may be copied, issue #17), by id.
    pub editable_mods: BTreeSet<String>,
    /// The mod editor's recovery copies: (id, mod name, damaged).
    pub mod_recoveries: Vec<(String, String, bool)>,
    /// The material editor's recovery copies (issue #44): (id, material
    /// name, damaged).
    pub material_recoveries: Vec<(String, String, bool)>,
    /// What the app said of the editor's last command (a copy or a new
    /// level that failed), on the Editor screen.
    pub editor_message: Option<String>,
    /// The game's version (the app's: a release's name, or a local
    /// build's), on the main menu and in About; empty: the crate's.
    pub version: String,
    /// The Report screen's (issue #3): `available` (the app's: a relay to
    /// send to), the gathered context, the texts, the replay, the status.
    pub report: crate::report::ReportData,
    /// The update check's (issue #4): the release offered, the install's
    /// progress, the Flatpak branch; the app's.
    pub update: crate::update::UpdateData,
    /// The menus by gamepad (issue #31): the app feeds it every pad event
    /// and adds [`MenuData::pad_events`] to each pass's input.
    pub pad: PadNav,
    /// A text field had the focus as the last frame ended.
    typing: bool,
    /// The weapon selection in progress: one per local player.
    selects: Vec<WeaponSelect>,
    /// The join step's presses (issue #32).
    join: JoinStep,
    /// The thumbnails decoded so far by level id (`None`: unreadable, its
    /// placeholder stays; tried once).
    thumbs: BTreeMap<String, Option<TextureHandle>>,
    /// Decodes left this frame ([`THUMB_DECODES_PER_FRAME`]).
    decodes_left: usize,
    /// Problems for the app to log (`take_warnings`).
    warnings: Vec<String>,
    /// Every problem `warn_once` has passed on (each is logged once).
    warned: BTreeSet<String>,
    /// The loadout editor, while open (local play, the host's lobby).
    loadout_editor: Option<loadouts::Editor>,
    /// The "Advanced…" bots window is open.
    bots_open: bool,
    multiplayer: multiplayer::View,
    host: host::View,
    lobby_ui: lobby::View,
    profiles: profiles::View,
    options: options::View,
    editor: editor::View,
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
            held_by: Vec::new(),
            seats: Vec::new(),
            seats_used: BTreeSet::new(),
            results: None,
            views: 1,
            lan_games: Vec::new(),
            internet: InternetView::default(),
            selected_game: None,
            default_relays: Vec::new(),
            default_stun: Vec::new(),
            lobby: None,
            chat: Vec::new(),
            online_status: None,
            joining: None,
            ingame_chat: None,
            download_cache: Err("there is no download cache".into()),
            host_lost: None,
            host_lost_replay: false,
            host_lost_hosting: false,
            editable: BTreeSet::new(),
            recoveries: Vec::new(),
            editable_skins: BTreeSet::new(),
            editable_mods: BTreeSet::new(),
            mod_recoveries: Vec::new(),
            material_recoveries: Vec::new(),
            editor_message: None,
            version: String::new(),
            report: Default::default(),
            update: Default::default(),
            pad: PadNav::default(),
            typing: false,
            selects: Vec::new(),
            join: JoinStep::default(),
            thumbs: BTreeMap::new(),
            decodes_left: THUMB_DECODES_PER_FRAME,
            warnings: Vec::new(),
            warned: BTreeSet::new(),
            loadout_editor: None,
            bots_open: false,
            multiplayer: multiplayer::View::default(),
            host: host::View::default(),
            lobby_ui: lobby::View::default(),
            profiles: profiles::View::default(),
            options: options::View::default(),
            editor: editor::View::default(),
            shown: None,
        }
    }

    /// The content changed (listed again): new lists, thumbnails made again.
    pub fn set_catalog(&mut self, catalog: Catalog) {
        self.catalog = catalog;
        self.thumbs.clear();
    }

    /// The level packs in the data directory's packs (editable in place)
    /// and the recoveries (`(id, level name, damaged)`), for the Editor
    /// screen (the app's, whenever they change).
    pub fn set_editor_packs(&mut self, editable: BTreeSet<String>, recoveries: Vec<(String, String, bool)>) {
        self.editable = editable;
        self.recoveries = recoveries;
    }

    /// The skin packs in the data directory's packs (edited in place), for
    /// the Editor screen's Skin tab (the app's, whenever they change).
    pub fn set_editable_skins(&mut self, editable: BTreeSet<String>) {
        self.editable_skins = editable;
    }

    /// The mod packs in the data directory's packs (edited in place) and
    /// the mod editor's recoveries (`(id, mod name, damaged)`), for the
    /// Editor screen's Mod tab (the app's, whenever they change).
    pub fn set_editor_mods(&mut self, editable: BTreeSet<String>, recoveries: Vec<(String, String, bool)>) {
        self.editable_mods = editable;
        self.mod_recoveries = recoveries;
    }

    /// The material editor's recoveries (`(id, material name, damaged)`),
    /// for the Editor screen's Material tab (the app's, whenever they
    /// change).
    pub fn set_material_recoveries(&mut self, recoveries: Vec<(String, String, bool)>) {
        self.material_recoveries = recoveries;
    }

    /// The weapons the selection shows now, by mod id (`None` when no
    /// selection is open): local player 1's.
    pub fn weapon_choice(&self) -> Option<[i32; MAX_SLOTS]> {
        self.weapon_choice_of(0)
    }

    /// Local player `k`'s weapons in the selection now.
    pub fn weapon_choice_of(&self, k: usize) -> Option<[i32; MAX_SLOTS]> {
        self.selects.get(k).map(WeaponSelect::slots)
    }

    /// Every seat's input now (the app's, every frame), noting the seats
    /// that press something (`seats_used`).
    pub fn set_seats(&mut self, seats: Vec<SeatInput>) {
        for s in &seats {
            if s.held != 0 || s.start {
                self.seats_used.insert(s.seat);
            }
        }
        self.seats = seats;
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

    /// `w` for the app to log, unless it was already.
    fn warn_once(&mut self, w: String) {
        if self.warned.insert(w.clone()) {
            self.warnings.push(w);
        }
    }

    /// Whether a window is open over the page (the loadout editor, the
    /// bots): Esc closes it instead of going back.
    pub fn window_open(&self) -> bool {
        self.loadout_editor.is_some() || self.bots_open
    }

    /// Before a pass that draws the menus: the keys the pads pressed, for
    /// egui's input (`PadNav::events`; none while the options capture an
    /// input: the capture has the pad alone).
    pub fn pad_events(&mut self, ctx: &egui::Context, state: &UiState) -> Vec<Event> {
        if self.capture.is_some() {
            self.pad.forget();
            return Vec::new();
        }
        self.pad.events(ctx, state)
    }

    /// The problems found since the last call (unreadable thumbnails, a
    /// loadout gone or changed), for the app to log once.
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
    let typing = data.typing
        && matches!(
            screen,
            Screen::Lobby | Screen::Multiplayer | Screen::Host | Screen::InGame | Screen::Report
        );
    data.pad.begin_pass(ui.ctx());
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
            Screen::Editor => data.editor = editor::View::default(),
            Screen::Report => data.report.enter(data.replays_dir.as_deref(), &data.catalog),
            Screen::WeaponSelect => {
                let menu = data.weapons.clone();
                // The lobby's choice is this player's alone.
                let n = if state.is_online() { 1 } else { state.local_players() };
                data.selects = menu.map_or_else(Vec::new, |m| {
                    (0..n)
                        .map(|k| {
                            let remembered = state.player_profile(k).map(|p| p.weapons.as_slice());
                            WeaponSelect::new(&m, remembered.unwrap_or(&[]))
                        })
                        .collect()
                });
            }
            Screen::Join => data.join = JoinStep::default(),
            _ => {}
        }
    }
    if screen != Screen::Options {
        data.capture = None;
    }
    if screen != Screen::WeaponSelect {
        data.selects.clear();
    }
    if screen != Screen::InGame {
        data.ingame_chat = None;
    }
    if !matches!(screen, Screen::LocalPlay | Screen::Lobby) {
        data.loadout_editor = None;
        data.bots_open = false;
    }
    // The capture takes the key: egui must not also act on it (Space
    // pressing the focused button, Tab moving the focus, Esc leaving).
    if data.capture.is_some() {
        ui.ctx().input_mut(|i| i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_))));
    }
    // A dialog of the options is open: Esc closes it, and stays.
    let dialog =
        (screen == Screen::Options && data.options.keys.busy(data.capture.is_some())) || data.window_open();
    match screen {
        Screen::InGame => ingame::show(ui, state, data, &mut out),
        Screen::WeaponSelect => weapon_select::show(ui, state, data, &mut out),
        Screen::Results => results::show(ui, state, data, &mut out),
        _ => {
            let frame = egui::Frame::central_panel(ui.style()).fill(theme::BACKGROUND);
            egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                tall_enough(ui, |ui| match screen {
                    Screen::Main => main::show(ui, state, data, &mut out),
                    Screen::LocalPlay => local_play::show(ui, state, data, &mut out),
                    Screen::Join => join::show(ui, state, data, &mut out),
                    Screen::Profiles => profiles::show(ui, state, data, &mut out),
                    Screen::Replays => replays::show(ui, state, data, &mut out),
                    Screen::Options => options::show(ui, state, data, &mut out),
                    Screen::About => about::show(ui, data, &mut out),
                    Screen::Multiplayer => multiplayer::show(ui, state, data, &mut out),
                    Screen::Host => host::show(ui, state, data, &mut out),
                    Screen::Lobby => lobby::show(ui, state, data, &mut out),
                    Screen::HostLost => host_lost::show(ui, data, &mut out),
                    Screen::Editor => editor::show(ui, data, &mut out),
                    Screen::Report => report::show(ui, state, data, &mut out),
                    Screen::Update => update::show(ui, state, data, &mut out),
                    _ => {}
                })
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
    data.pad.end_pass(ui);
    out
}

/// The shortest a menu screen is laid out, in points: in a window less
/// tall (a large UI scale: 240 points at 3) the whole screen is laid out
/// this tall and scrolls (issue #49).
const MIN_HEIGHT: f32 = 480.0;

/// Draws a menu screen with `add` into `ui`, or, in a [`short`] window,
/// into a column [`MIN_HEIGHT`] tall in a vertical scroll area (its own
/// scroll areas inside, which follow the focus first).
fn tall_enough(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    if !short(ui.ctx()) {
        return add(ui);
    }
    egui::ScrollArea::vertical().id_salt("short window").auto_shrink([false, false]).show(ui, |ui| {
        let r = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), MIN_HEIGHT));
        let mut screen = ui.new_child(UiBuilder::new().max_rect(r).layout(Layout::top_down(Align::Min)));
        add(&mut screen);
        ui.advance_cursor_after_rect(r.union(screen.min_rect()));
        crate::pad::follow_focus(ui);
    });
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

/// A start's primary button: `Loading…` while the match loads. The button
/// says it, not a line under it: a page's footer finds its height a frame
/// late, and the frame the app shows while it loads is the first (the
/// playtest of build 714: the line fell partly out of the window).
fn start_label(starting: bool) -> &'static str {
    if starting { "Loading…" } else { "Start" }
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

/// The room a form's controls need beside their names: in a column
/// narrower than both (a large UI scale on a small window, issue #49) each
/// name stands above its control, which then has the column's width.
const FORM_CONTROLS: f32 = 300.0;

/// A slider's value field beside it (with a unit: "100 %", "6.5 min").
const SLIDER_VALUE: f32 = 72.0;

/// Whether a form in `ui` puts each name above its control.
fn stacked(ui: &Ui) -> bool {
    ui.available_width() < FORM_LABEL + FORM_CONTROLS
}

/// One row of a form: `name` right-aligned in a column [`FORM_LABEL`]
/// wide, then the control `add` makes (given the name's id to be labelled
/// by), so a form's controls all start on one line. In a narrow column
/// ([`stacked`]) the name is a line of its own above the control.
fn form_row<R>(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    if stacked(ui) {
        let label = if name.is_empty() { egui::Id::NULL } else { ui.label(name).id };
        return form_line(ui, |ui| {
            fit_sliders(ui);
            add(ui, label)
        });
    }
    form_line(ui, |ui| {
        let label = form_name(ui, |ui| ui.label(name));
        fit_sliders(ui);
        add(ui, label.id)
    })
}

/// A form's row whose name is a checkbox switching the control on
/// (`on`): `add` gets whether it is on now and whether it just changed.
fn form_check_row<R>(ui: &mut Ui, on: bool, name: &str, add: impl FnOnce(&mut Ui, bool, bool) -> R) -> R {
    let mut on = on;
    if stacked(ui) {
        let changed = ui.checkbox(&mut on, name).changed();
        return form_line(ui, |ui| {
            fit_sliders(ui);
            add(ui, on, changed)
        });
    }
    form_line(ui, |ui| {
        let changed = form_name(ui, |ui| ui.checkbox(&mut on, name)).changed();
        fit_sliders(ui);
        add(ui, on, changed)
    })
}

/// Sliders no wider than what is left of `ui` beside their value field.
fn fit_sliders(ui: &mut Ui) {
    let room = (ui.available_width() - SLIDER_VALUE).floor().max(40.0);
    let s = ui.spacing_mut();
    s.slider_width = s.slider_width.min(room);
}

/// `width`, or less in a column too narrow for it (ruling UI-1: the
/// menus fit at every window size and UI scale; a text field's frame and
/// margin aside).
fn fit(ui: &Ui, width: f32) -> f32 {
    width.min(ui.available_width() - 16.0).max(40.0)
}

/// A row of `n` tabs of `size` centred over the column: each tab's size
/// and the indent before the first. A column too narrow for them (a small
/// window at a large UI scale, 427 points wide at 3 pixels a point) gives
/// each an equal share of its width instead (ruling UI-1), so every tab
/// stays in the window. The indent is what to `add_space` before the first
/// tab, 0 for none: that space also costs the row's own item spacing, so
/// a row filling the column would overflow it.
fn tab_row(ui: &Ui, size: Vec2, n: usize) -> (Vec2, f32) {
    let n = n.max(1) as f32;
    let gap = ui.spacing().item_spacing.x;
    let room = ui.available_width();
    let width = size.x.min(((room - gap * (n - 1.0)) / n).floor()).max(1.0);
    let row = width * n + gap * (n - 1.0);
    let lead = (room - row) / 2.0 - gap;
    (Vec2::new(width, size.y), lead.max(0.0))
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

/// A form's name cell: [`FORM_LABEL`] wide (less in a narrow column), its
/// widget right-aligned.
fn form_name(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> egui::Response) -> egui::Response {
    let h = form_height(ui);
    let right = Layout::right_to_left(Align::Center);
    // At most 40 % of a narrow column (a large UI scale, ruling UI-1).
    let w = FORM_LABEL.min((ui.available_width() * 0.4).floor());
    ui.allocate_ui_with_layout(vec2(w, h), right, |ui| {
        ui.set_min_width(w);
        add(ui)
    })
    .inner
}

/// The room a dialog keeps from the window's sides (and its frame's).
const DIALOG_MARGIN: f32 = 32.0;

/// Sets a dialog's (a modal window's) `ui` `width` wide, or as wide as
/// the window allows (issue #49: a large UI scale), its text wrapping in
/// it.
fn dialog_width(ui: &mut Ui, width: f32) {
    let w = width.min(ui.ctx().content_rect().width() - 2.0 * DIALOG_MARGIN).max(80.0);
    ui.set_min_width(w);
    ui.set_max_width(w);
}

/// A dialog's (a modal window's) content: `width` wide or as wide as the
/// window allows ([`dialog_width`]), in a scroll area as tall as the
/// window allows, so every control stays reachable in a small window
/// (issue #49).
fn dialog<R>(ui: &mut Ui, id: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    dialog_width(ui, width);
    let h = (ui.ctx().content_rect().height() - 2.0 * DIALOG_MARGIN).max(40.0);
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(h)
        .show(ui, |ui| {
            let r = add(ui);
            crate::pad::follow_focus(ui);
            r
        })
        .inner
}

/// Whether the window is shorter than a page ([`MIN_HEIGHT`]): its page
/// or dialog scrolls, and the lists in it have no scroll areas of their
/// own ([`list_area`]).
fn short(ctx: &egui::Context) -> bool {
    ctx.content_rect().height() < MIN_HEIGHT
}

/// A list in a dialog or a page: in `area`, or, in a [`short`] window,
/// with no scroll area of its own, as what holds it scrolls instead (a list
/// of a few rows' height would be hard to use, and a scroll area in one
/// scrolling with the window cannot show its rows reliably).
fn list_area<R>(ui: &mut Ui, area: egui::ScrollArea, add: impl FnOnce(&mut Ui) -> R) -> R {
    if short(ui.ctx()) { ui.vertical(add).inner } else { area.show(ui, add).inner }
}

/// Why the last start failed, in red.
fn message(ui: &mut Ui, state: &UiState) {
    if let Some(m) = state.message() {
        ui.label(RichText::new(m).color(Color32::from_rgb(255, 110, 90)));
    }
}

/// The playfield in game (the window above the HUD bar), and the offset
/// of its centre from the window's: the in-game panels (weapons, pause,
/// results) of one player anchor there, centred over the picture.
pub(crate) fn playfield(ctx: &egui::Context) -> (Rect, Vec2) {
    let screen = ctx.content_rect();
    let field = crate::hud::playfield(screen);
    (field, field.center() - screen.center())
}

/// Where the pause menu and the results centre, and the offset of that
/// centre from the window's: over the playfield with one view (as solo
/// play always had them), on the whole window with split screen's two to
/// four `views` (they are everyone's, issue #49).
pub(crate) fn panels_area(ctx: &egui::Context, views: usize) -> (Rect, Vec2) {
    if views >= 2 { (ctx.content_rect(), Vec2::ZERO) } else { playfield(ctx) }
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
