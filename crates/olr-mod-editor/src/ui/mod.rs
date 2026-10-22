//! The mod editor's egui panels (issue #17): the session bar on top (Save,
//! Close, Undo, Redo, the unsaved mark, the problems' count; at the right
//! Play and its ▾, issue #18), the left
//! list (World, the weapons in order, the shared projectiles), the item's
//! projectile tree and the chosen node's panel in the middle, the problems
//! below when shown, and the dialogs. `show` draws them from the state and
//! returns the frame's actions; the host applies `olr_ui::theme`. Widgets
//! have accessible names (`labels`) tests and scripts find them by. Issue
//! #19: the list's Art and Sounds, an image's painting panel (`art.rs`),
//! the sounds (`sounds.rs`) and the projectile's and weapon's Look
//! (`look.rs`).

mod art;
mod dialogs;
mod field;
mod list;
mod look;
mod panels;
mod sounds;

use std::collections::{BTreeMap, BTreeSet};

use egui::{ColorImage, Key, Modifiers, TextureHandle, TextureOptions, Ui};

use crate::state::{ModAction, ModEditorState};

/// The labels tests and scripts find widgets by.
pub mod labels {
    use olr_edit::mods::Issue;

    pub const SAVE: &str = "Save";
    pub const CLOSE: &str = "Close";
    pub const UNDO: &str = "Undo";
    pub const REDO: &str = "Redo";
    /// The unsaved changes' mark.
    pub const UNSAVED: &str = "Unsaved changes";
    /// The problems' list, shown or hidden.
    pub const PROBLEMS: &str = "Problems";
    // Play (issue #18).
    pub const PLAY: &str = "Play";
    /// Play's ▾: the level and the bots, and practice's (issue #20).
    pub const PLAY_OPTIONS: &str = "Play options";
    /// Play ▾'s practice with the weapon shown (issue #20).
    pub const PRACTICE: &str = "Practice with this weapon";
    /// Practice's reload at once (`LoadingTime` 0).
    pub const FAST_RELOAD: &str = "Fast reload";
    // The left list.
    pub const WORLD: &str = "World";
    pub const ADD_WEAPON: &str = "Add weapon";
    pub const MOVE_UP: &str = "Move weapon up";
    pub const MOVE_DOWN: &str = "Move weapon down";
    pub const DUPLICATE: &str = "Duplicate weapon";
    pub const REMOVE: &str = "Remove weapon";
    /// The rare fields of a panel, shown or hidden.
    pub const MORE: &str = "More";
    // The weapon.
    pub const FIRE: &str = "Fire";
    pub const GO_SHOT: &str = "Edit the shot";
    // A spawn.
    pub const SPAWNED: &str = "Spawned projectile";
    /// The spawned projectile's choice: the spawn's own.
    pub const OWN: &str = "Its own projectile";
    pub const GO_PROJECTILE: &str = "Edit the projectile";
    pub const MAKE_COPY: &str = "Make a copy for this weapon";
    pub const SHARE: &str = "Make it shared";
    /// A shared projectile's users' heading.
    pub const USED_BY: &str = "Used by";
    // A projectile.
    pub const GFX: &str = "Graphics";
    pub const COLOURS: &str = "Colours";
    pub const ANIMATED: &str = "Animated";
    pub const TRAIL: &str = "Trail";
    pub const GO_TRAIL: &str = "Edit the trail's spawn";
    pub const HAS_TIMER: &str = "Has a timer";
    pub const GO_TIMER: &str = "Edit the timer";
    // The world's presets and loadouts.
    pub const PRESET: &str = "Preset";
    pub const NEW_PRESET: &str = "New preset's name";
    pub const ADD_PRESET: &str = "Add preset";
    pub const REMOVE_PRESET: &str = "Remove preset";
    pub const LOADOUT: &str = "Shipped loadout";
    pub const LOADING_TIME: &str = "Loading time (%)";
    pub const UNIQUE: &str = "Unique weapons";
    // The dialogs.
    pub const SAVE_AND_CLOSE: &str = "Save and close";
    pub const DONT_SAVE: &str = "Don't save";
    pub const CANCEL: &str = "Cancel";
    pub const OK: &str = "OK";
    pub const CHANGE: &str = "Change";
    pub const ADD: &str = "Add";
    pub const COPY_FROM: &str = "Copy from";
    /// Add weapon's choice of a blank weapon.
    pub const BLANK: &str = "A blank weapon";
    pub const WEAPON_TO_COPY: &str = "Weapon to copy";
    // Issue #19: the Art and Sounds sections.
    pub const ART: &str = "Art";
    pub const SOUNDS: &str = "Sounds";
    pub const NEW_IMAGE: &str = "New image";
    pub const IMPORT_PNG: &str = "Import PNG";
    pub const IMPORT_SOUND: &str = "Import sound";
    pub const REPLACE_PNG: &str = "Replace with PNG";
    pub const COPY_IMAGE: &str = "Copy image";
    pub const RENAME_ASSET: &str = "Rename";
    pub const REMOVE_ASSET: &str = "Remove";
    pub const IMAGE_NAME: &str = "Image name";
    pub const FRAME_SIDE: &str = "Frame side (px)";
    pub const FRAME_COUNT: &str = "Frames";
    pub const MAKE: &str = "Make";
    pub const NEW_NAME: &str = "New name";
    pub const RENAME: &str = "Rename it";
    pub const BROWSER_PATH: &str = "Path";
    pub const UP: &str = "Up";
    pub const CHOOSE: &str = "Choose";
    /// The image's painting.
    pub const CANVAS: &str = "Image canvas";
    pub const TARGET_COLOUR: &str = "Colour";
    pub const TARGET_GLOW: &str = "Glow";
    pub const SIZE_1: &str = "1 px";
    pub const SIZE_2: &str = "2 px";
    pub const PAINT_COLOUR: &str = "Paint colour";
    pub const HEX: &str = "Hex";
    pub const GLOW_STRENGTH: &str = "Glow strength";
    pub const ONION: &str = "Onion skin";
    pub const COPY_FRAME: &str = "Copy frame";
    pub const PASTE_FRAME: &str = "Paste frame";
    pub const ADD_FRAME: &str = "Add frame";
    pub const REMOVE_FRAME: &str = "Remove frame";
    pub const ZOOM_IN: &str = "Zoom in";
    pub const ZOOM_OUT: &str = "Zoom out";
    pub const PREVIEW: &str = "Preview";
    // The Look group.
    pub const LOOK: &str = "Look";
    pub const GLOW: &str = "Glow (%)";
    pub const HAS_GLOW: &str = "Glows";
    pub const LIGHT: &str = "Light";
    pub const END_FLASH: &str = "End flash";
    pub const EXPLOSION: &str = "Explosion light";
    pub const MUZZLE_LIGHT: &str = "Muzzle light";
    pub const UNLIMITED: &str = "unlimited reach";

    /// A frame of the frames' grid.
    pub fn frame(f: usize) -> String {
        format!("Frame {}", f + 1)
    }
    /// An image of the left list.
    pub fn image(path: &str) -> String {
        format!("Image {path}")
    }
    /// A sound's Play button.
    pub fn play_sound(path: &str) -> String {
        format!("Play {path}")
    }
    /// A sound's Rename and Remove buttons.
    pub fn rename_sound(path: &str) -> String {
        format!("Rename {path}")
    }
    pub fn remove_sound(path: &str) -> String {
        format!("Remove {path}")
    }
    /// A Look part's field: `Light colour`, `End flash reach (px)`.
    pub fn look(part: &str, what: &str) -> String {
        format!("{part} {what}")
    }

    /// A field's Reset button.
    pub fn reset(field: &str) -> String {
        format!("Reset {field}")
    }
    /// A number field's slider.
    pub fn slider(field: &str) -> String {
        format!("{field} slider")
    }
    /// A node of the projectile tree.
    pub fn node(label: &str) -> String {
        format!("Tree: {label}")
    }
    /// A field of an event's section (`Hit worm: Damage`).
    pub fn event(event: &str, what: &str) -> String {
        format!("{event}: {what}")
    }
    /// A weapon's state in the preset shown (`Bazooka: Banned`).
    pub fn preset_state(weapon: &str, state: &str) -> String {
        format!("{weapon}: {state}")
    }
    /// A user of a shared projectile (its spawn's path), a link.
    pub fn used_by(path: &str) -> String {
        format!("Used by {path}")
    }
    /// Add weapon's weapon of the mod copied from.
    pub fn copy(weapon: &str) -> String {
        format!("Copy {weapon}")
    }
    /// Play ▾'s level (by its name).
    pub fn play_level(name: &str) -> String {
        format!("Play on {name}")
    }
    /// Play ▾'s number of bots.
    pub fn play_bots(n: u8) -> String {
        format!("{n} bots")
    }
    /// Play ▾'s practice level (by its name).
    pub fn practice_level(name: &str) -> String {
        format!("Practice on {name}")
    }
    /// Play ▾'s number of practice targets.
    pub fn practice_targets(n: u8) -> String {
        format!("{n} targets")
    }
    /// A problem of the list, a link to its place.
    pub fn issue(i: &Issue) -> String {
        i.to_string()
    }
}

/// Transient UI state: what is typed, dragged, opened, and the textures.
#[derive(Default)]
pub struct ModEditorData {
    /// Fields being typed into, by path.
    drafts: BTreeMap<String, String>,
    /// A slider being dragged: its path and value.
    slide: Option<(String, f64)>,
    /// The panels whose More ▸ is open (by node path).
    more: BTreeSet<String>,
    /// The gfx files' thumbnails (`None`: not an image).
    thumbs: BTreeMap<String, Option<TextureHandle>>,
    /// The focus generation scrolled to.
    scrolled: u64,
    /// The new preset's name.
    new_preset: String,
    /// The generation the thumbnails were made at (issue #19: a painted
    /// image's thumbnail follows).
    thumbs_gen: u64,
    /// New image's fields.
    new_image: NewImageDraft,
    /// Rename's text (for the path being renamed).
    rename: Option<(String, String)>,
    /// The browser's typed path (for the folder shown).
    browse_path: Option<(std::path::PathBuf, String)>,
    /// The Art section's canvas, textures and typed colour.
    art: art::ArtData,
}

/// New image's fields.
struct NewImageDraft {
    name: String,
    side: u32,
    frames: u32,
}

impl Default for NewImageDraft {
    fn default() -> NewImageDraft {
        NewImageDraft { name: String::new(), side: 8, frames: 1 }
    }
}

impl ModEditorData {
    /// Where the Art section's canvas was drawn last, if it was (tests
    /// point into it).
    pub fn art_canvas(&self) -> Option<egui::Rect> {
        self.art.canvas
    }
}

/// Draws the mod editor into `ui` and returns this frame's actions.
pub fn show(ui: &mut Ui, state: &ModEditorState, data: &mut ModEditorData) -> Vec<ModAction> {
    let mut cx = Cx { state, data, out: Vec::new(), commits: Vec::new() };
    egui::Panel::top("olr-mod-editor-session").show(ui, |ui| session(ui, &mut cx));
    egui::Panel::bottom("olr-mod-editor-status").show(ui, |ui| {
        ui.horizontal(|ui| {
            let src = state.src();
            let shared = src.shared.len();
            ui.weak(format!("{} weapons, {shared} shared projectiles", src.weapons.len()));
            ui.separator();
            ui.label(state.status());
        });
    });
    if state.show_problems() {
        egui::Panel::bottom("olr-mod-editor-problems").resizable(true).default_size(160.0).show(ui, |ui| {
            problems(ui, &mut cx);
        });
    }
    egui::Panel::left("olr-mod-editor-list").exact_size(230.0).resizable(false).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("olr-mod-editor-list").show(ui, |ui| list::items(ui, &mut cx));
    });
    if !state.nodes().is_empty() && *state.item() != crate::tree::Item::World {
        egui::Panel::left("olr-mod-editor-tree").exact_size(260.0).resizable(false).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("olr-mod-editor-tree")
                .show(ui, |ui| list::tree(ui, &mut cx));
        });
    }
    egui::CentralPanel::default().show(ui, |ui| {
        // The canvas takes the wheel (zoom) and drags: the Art panel does
        // not scroll.
        if matches!(state.item(), crate::tree::Item::Art(_)) {
            art::show(ui, &mut cx);
            return;
        }
        egui::ScrollArea::vertical().id_salt("olr-mod-editor-panel").auto_shrink([false, false]).show(
            ui,
            |ui| match state.item() {
                crate::tree::Item::Sounds => sounds::show(ui, &mut cx),
                _ => panels::show(ui, &mut cx),
            },
        );
    });
    dialogs::show(ui.ctx(), &mut cx);
    shortcuts(ui, &mut cx);
    if cx.data.scrolled != state.focus_gen() {
        cx.data.scrolled = state.focus_gen();
    }
    // A field's last value is in before a click that ended its edit acts.
    let Cx { mut commits, out, .. } = cx;
    commits.extend(out);
    commits
}

/// What a panel draws with: the state, the UI's own data, and the actions.
pub(crate) struct Cx<'a> {
    pub state: &'a ModEditorState,
    pub data: &'a mut ModEditorData,
    pub out: Vec<ModAction>,
    /// Fields' values committed this frame (they act first).
    pub commits: Vec<ModAction>,
}

/// Save, Close, Undo and Redo, the mod's name, the unsaved mark and the
/// problems' count; at the right Play and its ▾.
fn session(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    ui.horizontal(|ui| {
        if ui.button(labels::SAVE).clicked() {
            cx.out.push(ModAction::Save);
        }
        if ui.button(labels::CLOSE).clicked() {
            cx.out.push(ModAction::Close);
        }
        ui.separator();
        if ui.add_enabled(state.can_undo(), egui::Button::new(labels::UNDO)).clicked() {
            cx.out.push(ModAction::Undo);
        }
        if ui.add_enabled(state.can_redo(), egui::Button::new(labels::REDO)).clicked() {
            cx.out.push(ModAction::Redo);
        }
        ui.separator();
        ui.strong(format!("Mod: {}", state.pack().manifest.name));
        if let Some(base) = state.base_id() {
            let name = state.mods().iter().find(|(id, _)| id == base).map_or(base, |(_, n)| n.as_str());
            ui.weak(format!("(a copy of {name})"));
        }
        // The mark keeps its space: the bar never moves.
        let mark = ui.label(if state.dirty() { "●" } else { " " });
        if state.dirty() {
            relabel(&mark, egui::WidgetType::Label, labels::UNSAVED);
            mark.on_hover_text(labels::UNSAVED);
        }
        ui.separator();
        let errors = state.errors();
        let warnings = state.issues().len() - errors;
        let text = match (errors, warnings) {
            (0, 0) => "No problems".to_string(),
            (e, w) => format!("{e} error{}, {w} warning{}", plural(e), plural(w)),
        };
        let mut text = egui::RichText::new(text);
        if errors > 0 {
            text = text.color(ui.visuals().error_fg_color);
        }
        let r = ui.selectable_label(state.show_problems(), text);
        relabel(&r, egui::WidgetType::Button, labels::PROBLEMS);
        if r.clicked() {
            cx.out.push(ModAction::ShowProblems(!state.show_problems()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            play_menu(ui, cx);
            let hover = "play a test match with the mod as it is now, unsaved changes included";
            if ui.button(labels::PLAY).on_hover_text(hover).clicked() {
                cx.out.push(ModAction::Play);
            }
        });
    });
}

/// The Play menu's level list: taller lists scroll.
const LEVELS_HEIGHT: f32 = 240.0;

/// Play's ▾ (the level editor's, `olr-editor`'s `toolbar::play_menu`): the
/// test match's level (the installed ones) and bots.
fn play_menu(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let chosen = state.play_level().map(|(id, _)| id.as_str());
    // The arrow is drawn (a font may lack ▾); spaces make its room.
    let (r, _) = egui::containers::menu::MenuButton::new("    ").config(config).ui(ui, |ui| {
        ui.label(egui::RichText::new("Level").strong());
        egui::ScrollArea::vertical().id_salt("olr-mod-editor-play-levels").max_height(LEVELS_HEIGHT).show(
            ui,
            |ui| {
                for (id, name) in state.levels() {
                    let r = ui.selectable_label(Some(id.as_str()) == chosen, name);
                    relabel(&r, egui::WidgetType::Button, &labels::play_level(name));
                    if r.clicked() {
                        cx.out.push(ModAction::PlayLevel(id.clone()));
                    }
                }
            },
        );
        ui.separator();
        ui.label(egui::RichText::new("Bots").strong());
        ui.horizontal(|ui| {
            for n in 0..=crate::MAX_BOTS {
                let r = ui.selectable_label(n == state.play_bots(), n.to_string());
                relabel(&r, egui::WidgetType::Button, &labels::play_bots(n));
                if r.clicked() {
                    cx.out.push(ModAction::PlayBots(n));
                }
            }
        });
        ui.separator();
        practice_menu(ui, cx);
    });
    const ARROW: f32 = 8.0;
    let colour = ui.style().interact(&r).text_color();
    let c = r.rect.center();
    let arrow = vec![
        c + egui::vec2(-ARROW / 2.0, -ARROW / 4.0),
        c + egui::vec2(ARROW / 2.0, -ARROW / 4.0),
        c + egui::vec2(0.0, ARROW / 4.0),
    ];
    ui.painter().add(egui::Shape::convex_polygon(arrow, colour, egui::Stroke::NONE));
    relabel(&r, egui::WidgetType::Button, labels::PLAY_OPTIONS);
    let level = state.play_level().map_or("no level", |(_, name)| name.as_str());
    let bots = state.play_bots();
    r.on_hover_text(format!("the test match: {level}, {bots} bot{}", plural(bots.into())));
}

/// The practice level's list in Play ▾: shorter than Play's.
const PRACTICE_LEVELS_HEIGHT: f32 = 120.0;

/// Play ▾'s practice (issue #20): Practice with this weapon (the weapon
/// shown), its level, its target dummies and Fast reload.
fn practice_menu(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    let weapon = state.practice_weapon().unwrap_or_default();
    let r = ui.add_enabled(!weapon.is_empty(), egui::Button::new(labels::PRACTICE));
    let r = r.on_disabled_hover_text("choose a weapon in the list first");
    let hover = format!("practise with {weapon} against still targets, unsaved changes included");
    if r.on_hover_text(hover).clicked() {
        cx.out.push(ModAction::Practice);
        ui.close();
    }
    ui.label(egui::RichText::new("Practice level").strong());
    let chosen = state.practice_level().map(|(id, _)| id.as_str());
    egui::ScrollArea::vertical()
        .id_salt("olr-mod-editor-practice-levels")
        .max_height(PRACTICE_LEVELS_HEIGHT)
        .show(ui, |ui| {
            for (id, name) in state.levels() {
                let r = ui.selectable_label(Some(id.as_str()) == chosen, name);
                relabel(&r, egui::WidgetType::Button, &labels::practice_level(name));
                if r.clicked() {
                    cx.out.push(ModAction::PracticeLevel(id.clone()));
                }
            }
        });
    ui.label(egui::RichText::new("Targets").strong());
    ui.horizontal(|ui| {
        for n in 1..=crate::MAX_TARGETS {
            let r = ui.selectable_label(n == state.practice_targets(), n.to_string());
            relabel(&r, egui::WidgetType::Button, &labels::practice_targets(n));
            if r.clicked() {
                cx.out.push(ModAction::PracticeTargets(n));
            }
        }
    });
    let mut fast = state.fast_reload();
    let r = ui.checkbox(&mut fast, labels::FAST_RELOAD);
    if r.on_hover_text("reload at once instead of at the weapon's own pace").changed() {
        cx.out.push(ModAction::FastReload(fast));
    }
}

/// The problems: errors first, each a link to its place.
fn problems(ui: &mut Ui, cx: &mut Cx) {
    ui.horizontal(|ui| {
        ui.strong("Problems");
        ui.weak("A mod with errors is not saved. Click a problem to go to it.");
    });
    egui::ScrollArea::vertical().id_salt("olr-mod-editor-problems").show(ui, |ui| {
        if cx.state.issues().is_empty() {
            ui.weak("None: the mod checks clean.");
        }
        for i in cx.state.issues() {
            let colour = if i.is_error() { ui.visuals().error_fg_color } else { ui.visuals().warn_fg_color };
            let r = ui.add(egui::Button::new(egui::RichText::new(i.to_string()).color(colour)).frame(false));
            relabel(&r, egui::WidgetType::Button, &labels::issue(i));
            if r.clicked()
                && let Some(to) = i.path.clone().or_else(|| i.file.clone())
            {
                cx.out.push(ModAction::GoTo(to));
            }
        }
    });
}

/// Ctrl+Z, Ctrl+Y and Ctrl+S, not while a dialog is open or a text field
/// has the keyboard.
fn shortcuts(ui: &Ui, cx: &mut Cx) {
    if cx.state.dialog().is_some() || ui.ctx().egui_wants_keyboard_input() || ui.ctx().dragged_id().is_some()
    {
        return;
    }
    ui.input_mut(|i| {
        for (key, action) in [(Key::Z, ModAction::Undo), (Key::Y, ModAction::Redo), (Key::S, ModAction::Save)]
        {
            if i.consume_key(Modifiers::COMMAND, key) {
                cx.out.push(action);
            }
        }
    });
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Text beside a widget, painted: the widget carries the name for
/// AccessKit (a label would be a second node of the same name).
pub(crate) fn caption(ui: &mut Ui, text: &str) -> egui::Response {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let (rect, r) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    ui.painter().galley(rect.min, galley, ui.visuals().text_color());
    r
}

/// Names `r` `label` for AccessKit, whatever it shows.
pub(crate) fn relabel(r: &egui::Response, typ: egui::WidgetType, label: &str) {
    let enabled = r.enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(typ, enabled, label));
}

/// The thumbnail of the mod's image `path` (its first frame's texture and
/// that frame's share of the width), if it decodes.
pub(crate) fn thumb(ui: &Ui, cx: &mut Cx, path: &str) -> Option<(TextureHandle, f32)> {
    if cx.data.thumbs_gen != cx.state.generation() {
        cx.data.thumbs.clear();
        cx.data.thumbs_gen = cx.state.generation();
    }
    if !cx.data.thumbs.contains_key(path) {
        let img = cx.state.asset(path).and_then(|b| olr_format::image_io::decode_rgba(b).ok());
        let tex = img.map(|img| {
            let size = [img.width() as usize, img.height() as usize];
            ui.ctx().load_texture(
                format!("olr-mod-editor-thumb-{path}"),
                ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                TextureOptions::NEAREST,
            )
        });
        cx.data.thumbs.insert(path.to_string(), tex);
    }
    let tex = cx.data.thumbs.get(path)?.clone()?;
    let [w, h] = tex.size();
    // Frames are square: the first is the strip's left `h` pixels.
    let share = if w > h && h > 0 { h as f32 / w as f32 } else { 1.0 };
    Some((tex, share))
}

/// Draws `thumb` `side` points square.
pub(crate) fn draw_thumb(ui: &mut Ui, (tex, share): &(TextureHandle, f32), side: f32) {
    let img = egui::Image::new(egui::load::SizedTexture::new(tex.id(), egui::vec2(side, side)))
        .fit_to_exact_size(egui::vec2(side, side))
        .uv(egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(*share, 1.0)));
    ui.add(img);
}
