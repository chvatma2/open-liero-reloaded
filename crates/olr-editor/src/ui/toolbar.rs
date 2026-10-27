//! The two bars over the canvas (issue #8): the session bar (File ▾,
//! Undo, Redo, the unsaved mark, then at the right View ▾ and Play with
//! its ▾) and the tool options bar, which shows only the chosen tool's
//! options: the brush for the tools that dab with it, the paint for those
//! that lay it, then the tool's own. The asset tab's session bar takes
//! their pieces (issue #78, `ui::asset`), and its tools panel the options
//! (`options_panel`, wrapped to the panel).

use egui::{Ui, WidgetType};
use olr_look::ViewMode;

use super::icons::{self, Icon};
use super::{EditorData, labels, number, relabel};
use crate::browser::Purpose;
use crate::state::{ClockAction, DocTab, EditorAction, EditorState, MAX_BOTS};
use crate::tools::{Brush, BrushShape, MAX_BRUSH, PaintChoice, TextureRef, Tool};
use olr_format::levels::ElementMaterial;

/// The space the unsaved mark keeps, shown or not: nothing moves when it
/// comes and goes.
const MARK_WIDTH: f32 = 110.0;
/// The space "By colour" and its tolerance keep, the tolerance shown or
/// not: Glow selection and the glow % never move.
const BY_COLOUR_WIDTH: f32 = 220.0;
/// "Paint as"'s picker width.
const PAINT_AS_WIDTH: f32 = 110.0;
/// The Surface brush's material picker's width.
const SURFACE_WIDTH: f32 = 130.0;
/// The crater picker's width.
const CRATER_WIDTH: f32 = 80.0;
/// The palette button's width, its swatch shown or not.
const TEXTURES_WIDTH: f32 = 120.0;
/// A swatch's side in the palette, and in its button (points).
const SWATCH: f32 = 28.0;
const BUTTON_SWATCH: f32 = 16.0;
/// The Play menu's mod list: taller lists scroll.
const MODS_HEIGHT: f32 = 200.0;
/// "Paint as": the layer's own material, or one of the three.
const PAINT_AS: [(Option<ElementMaterial>, &str); 4] = [
    (None, "Layer's"),
    (Some(ElementMaterial::Rock), "Rock"),
    (Some(ElementMaterial::Dirt), "Dirt"),
    (Some(ElementMaterial::Background), "Background"),
];

/// Every control as tall as a button, so a bar's labels, fields and
/// pickers share its buttons' centre line (the host's theme makes buttons
/// taller than labels: the playtest's crooked Play, Mod, Bots). Returns
/// that height.
pub(super) fn button_height(ui: &mut Ui) -> f32 {
    let h = ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(h);
    ui.spacing().interact_size.y
}

/// The session bar: each control's place is fixed by what precedes it
/// (from the left, or from the right for View and Play), never by what a
/// control shows (the playtest: one wrapped row moved its controls as they
/// changed width, and pushed the mod picker past the window's edge).
pub(super) fn session(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    button_height(ui);
    let asset = state.doc_tab() == DocTab::Asset;
    ui.horizontal(|ui| {
        tab_switch(ui, state, out);
        ui.separator();
        file_menu(ui, state, out);
        ui.separator();
        undo_redo(ui, state, out);
        let mark = if state.active_dirty() { "unsaved changes" } else { "" };
        ui.add_sized(
            [MARK_WIDTH, ui.spacing().interact_size.y],
            egui::Label::new(egui::RichText::new(mark).weak()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Play is the level's (issue #77).
            if !asset {
                play_menu(ui, state, data, out);
                if ui.button(labels::PLAY).on_hover_text("play a test match on this level").clicked() {
                    out.push(EditorAction::Play);
                }
                ui.separator();
            }
            view_menu(ui, state, data, out);
        });
    });
}

/// Issue #77: the Level | Asset switch (Asset disabled while no asset is
/// open).
pub(super) fn tab_switch(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let asset = state.doc_tab() == DocTab::Asset;
    if ui.add(egui::Button::selectable(!asset, labels::LEVEL_TAB)).clicked() && asset {
        out.push(EditorAction::ShowTab(DocTab::Level));
    }
    let open = state.asset_document().is_some();
    let r = ui
        .add_enabled(open, egui::Button::selectable(asset, labels::ASSET_TAB))
        .on_disabled_hover_text(labels::ASSET_TAB_HINT);
    if r.clicked() && !asset {
        out.push(EditorAction::ShowTab(DocTab::Asset));
    }
}

/// File ▾: Open… (the level tab's), Save and Close.
pub(super) fn file_menu(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let asset = state.doc_tab() == DocTab::Asset;
    menu(ui, "File", labels::FILE, None, |ui| {
        // Open… opens a level: the level tab's.
        if !asset && ui.button(labels::OPEN).clicked() {
            out.push(EditorAction::Browse(Purpose::OpenPack));
        }
        if ui.button(labels::SAVE).on_hover_text("Ctrl+S").clicked() {
            out.push(EditorAction::Save);
        }
        if ui.button(labels::CLOSE).clicked() {
            out.push(EditorAction::Close);
        }
    });
}

/// Undo and Redo.
pub(super) fn undo_redo(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let hint = state.undo_name().map_or_else(|| "Ctrl+Z".to_string(), |n| format!("Undo {n} (Ctrl+Z)"));
    if ui.add_enabled(state.can_undo(), egui::Button::new(labels::UNDO)).on_hover_text(hint).clicked() {
        out.push(EditorAction::Undo);
    }
    if ui.add_enabled(state.can_redo(), egui::Button::new(labels::REDO)).on_hover_text("Ctrl+Y").clicked() {
        out.push(EditorAction::Redo);
    }
}

/// A menu's button: `text` and a ▾, drawn (a font may lack the glyph),
/// named `label`.
fn menu(
    ui: &mut Ui,
    text: &str,
    label: &str,
    config: Option<egui::containers::menu::MenuConfig>,
    add: impl FnOnce(&mut Ui),
) -> egui::Response {
    /// The arrow's width (points); spaces make its room in the button.
    const ARROW: f32 = 8.0;
    let room = if text.is_empty() { "    " } else { "     " };
    let mut button = egui::containers::menu::MenuButton::new(format!("{text}{room}"));
    if let Some(config) = config {
        button = button.config(config);
    }
    let (r, _) = button.ui(ui, add);
    let colour = ui.style().interact(&r).text_color();
    let c = egui::pos2(r.rect.right() - ui.spacing().button_padding.x - ARROW / 2.0, r.rect.center().y);
    let arrow = vec![
        c + egui::vec2(-ARROW / 2.0, -ARROW / 4.0),
        c + egui::vec2(ARROW / 2.0, -ARROW / 4.0),
        c + egui::vec2(0.0, ARROW / 4.0),
    ];
    ui.painter().add(egui::Shape::convex_polygon(arrow, colour, egui::Stroke::NONE));
    relabel(&r, WidgetType::Button, None, label);
    r
}

/// Menus that stay open while their fields are used: a click outside, or
/// Escape, closes them.
fn stays_open() -> egui::containers::menu::MenuConfig {
    egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
}

/// Play's ▾: the test match's mod and bots.
fn play_menu(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let r = menu(ui, "", labels::PLAY_OPTIONS, Some(stays_open()), |ui| {
        ui.label(egui::RichText::new("Mod").strong());
        egui::ScrollArea::vertical().id_salt("olr-editor-play-mods").max_height(MODS_HEIGHT).show(ui, |ui| {
            for m in &state.catalog().mods {
                if ui.selectable_label(m.id == state.settings().play_mod, &m.name).clicked()
                    && m.id != state.settings().play_mod
                {
                    out.push(EditorAction::PlayMod(m.id.clone()));
                }
            }
        });
        ui.separator();
        if let Some(bots) =
            number(ui, data, "play-bots", "Bots", state.settings().play_bots.into(), 0..=MAX_BOTS.into())
        {
            out.push(EditorAction::PlayBots(bots as u8));
        }
    });
    let mods = &state.catalog().mods;
    let current = mods
        .iter()
        .find(|m| m.id == state.settings().play_mod)
        .map_or(state.settings().play_mod.as_str(), |m| m.name.as_str());
    let bots = state.settings().play_bots;
    r.on_hover_text(format!("the test match: {current}, {bots} bot{}", if bots == 1 { "" } else { "s" }));
}

/// View ▾: the collision view, the grid, Preview parallax (issue #66),
/// the light views, the Materials view (issue #43) and the lights' clock.
pub(super) fn view_menu(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    menu(ui, "View", labels::VIEW, Some(stays_open()), |ui| {
        let mut material = state.material_view();
        if ui.checkbox(&mut material, labels::MATERIAL_VIEW).changed() {
            out.push(EditorAction::MaterialView(material));
        }
        let mut grid = state.grid();
        if ui.checkbox(&mut grid, labels::GRID).changed() {
            out.push(EditorAction::Grid(grid));
        }
        // An asset has no distant layers (issue #78); the level's ground
        // has (issue #81).
        let mut parallax = state.preview_parallax();
        if (state.doc_tab() == DocTab::Level || state.ground_open())
            && ui.checkbox(&mut parallax, labels::PREVIEW_PARALLAX).changed()
        {
            out.push(EditorAction::PreviewParallax(parallax));
        }
        ui.separator();
        for (mode, label) in [
            (ViewMode::Lit, labels::VIEW_LIT),
            (ViewMode::FullBright, labels::VIEW_FULL),
            (ViewMode::LightOnly, labels::VIEW_LIGHT),
        ] {
            if ui.selectable_label(state.view_mode() == mode, label).clicked() {
                out.push(EditorAction::SetViewMode(mode));
            }
        }
        let mut surfaces = state.surface_view();
        if ui
            .checkbox(&mut surfaces, labels::SURFACE_VIEW)
            .on_hover_text("each pixel tinted by its surface material (the back where the front is dug)")
            .changed()
        {
            out.push(EditorAction::SurfaceView(surfaces));
        }
        ui.separator();
        let (label, action) = if state.clock_running() {
            (labels::CLOCK_PAUSE, ClockAction::Pause)
        } else {
            (labels::CLOCK_PLAY, ClockAction::Play)
        };
        if ui.button(label).on_hover_text("run the lights' animations").clicked() {
            out.push(EditorAction::Clock(action));
        }
        // Whole milliseconds; a running clock shows its time and runs on from a scrub.
        let now = state.clock_ms().floor() as i64;
        if let Some(t) = number(ui, data, "clock", labels::CLOCK_SCRUB, now, 0..=86_400_000) {
            out.push(EditorAction::Clock(ClockAction::Scrub(t as f64)));
        }
    });
}

/// The tool options bar: the chosen tool's options only, from the left:
/// the brush, the paint, then the tool's own (so the brush's controls sit
/// in one place for every tool that dabs). A tool without options leaves
/// it empty, as tall as ever.
pub(super) fn options_bar(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let h = button_height(ui);
    ui.horizontal(|ui| {
        ui.set_min_height(h);
        let tool = state.tool();
        let mut sections = 0;
        let mut separate = |ui: &mut Ui| {
            if sections > 0 {
                ui.separator();
            }
            sections += 1;
        };
        if tool.uses_brush() {
            separate(ui);
            brush(ui, state, data, out);
        }
        if tool.lays_paint() {
            separate(ui);
            paint(ui, state, data, out);
        }
        if !matches!(tool, Tool::Select | Tool::Spawn | Tool::Eyedropper) {
            separate(ui);
            options(ui, state, data, out);
        }
    });
}

/// The asset tab's tool options (issue #78): the options bar's sections
/// under the tools, one under another, each wrapped to the panel (a
/// picker under its label, a group of toggles in rows).
pub(super) fn options_panel(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let tool = state.tool();
    ui.strong(format!("{} options", tool.name()));
    if tool.uses_brush() {
        brush(ui, state, data, out);
        ui.separator();
    }
    if tool.lays_paint() {
        paint(ui, state, data, out);
        ui.separator();
    }
    if !matches!(tool, Tool::Select | Tool::Spawn | Tool::Eyedropper) {
        options(ui, state, data, out);
    } else if !tool.uses_brush() && !tool.lays_paint() {
        ui.weak("No options");
    }
}

/// Whether `ui` is the options panel's (issue #78): a column, not the
/// options bar's row.
fn in_panel(ui: &Ui) -> bool {
    !ui.layout().is_horizontal()
}

/// `add` in the bar's row as it is, or in the panel in wrapped rows.
fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    if in_panel(ui) { ui.horizontal_wrapped(add).inner } else { add(ui) }
}

/// The brush: Round, Square, the craters and the size.
fn brush(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let brush = state.brush();
    group(ui, |ui| {
        for (shape, name, icon) in
            [(BrushShape::Round, "Round", Icon::Round), (BrushShape::Square, "Square", Icon::Square)]
        {
            if icon_toggle(ui, icon, name, brush.shape == shape).clicked() {
                out.push(EditorAction::Brush(Brush { shape, ..brush }));
            }
        }
    });
    craters(ui, state, out);
    if let Some(size) = number(ui, data, "brush-size", "Size", brush.size.into(), 1..=MAX_BRUSH.into()) {
        out.push(EditorAction::Brush(Brush { size: size as u32, ..brush }));
    }
}

/// A square toggle showing `icon`, named `name`.
fn icon_toggle(ui: &mut Ui, icon: Icon, name: &str, selected: bool) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let (rect, r) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&r, selected);
        ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(side * 0.6, side * 0.6));
        icons::paint(ui.painter(), inner, icon, egui::Stroke::new(1.5, v.text_color()));
    }
    relabel(&r, WidgetType::Button, Some(selected), name);
    r.on_hover_text(name)
}

/// The paint: the colour (its button chooses it and opens its picker), the
/// theme's tiles and the palette.
fn paint(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let paint = state.paint();
    let is_colour = matches!(paint, PaintChoice::Colour(_));
    let mut colour = match paint {
        PaintChoice::Colour(c) => egui::Color32::from_rgb(c.r, c.g, c.b),
        _ => egui::Color32::from_rgb(140, 90, 40),
    };
    group(ui, |ui| {
        let r =
            egui::color_picker::color_edit_button_srgba(ui, &mut colour, egui::color_picker::Alpha::Opaque);
        if is_colour {
            let stroke = ui.visuals().selection.stroke;
            ui.painter().rect_stroke(r.rect.expand(2.0), 2.0, stroke, egui::StrokeKind::Outside);
        }
        relabel(&r, WidgetType::ColorButton, Some(is_colour), labels::COLOUR);
        let changed = r.changed();
        if r.on_hover_text("paint with a colour").clicked() || changed {
            let rgba = olr_data::image::Rgba::rgb(colour.r(), colour.g(), colour.b());
            out.push(EditorAction::Paint(PaintChoice::Colour(rgba)));
        }
        for (choice, name) in
            [(PaintChoice::FrontTile, labels::FRONT_TILE), (PaintChoice::BackTile, labels::BACK_TILE)]
        {
            if ui.selectable_label(*paint == choice, name).clicked() {
                out.push(EditorAction::Paint(choice));
            }
        }
    });
    palette(ui, state, data, out);
}

/// `add` in a slot `width` points wide, whatever it shows: a picker's long
/// choice is cut, never wider (the playtest's moving controls); in the
/// options panel at most the panel's width (issue #78).
fn fixed(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui, f32)) {
    let width = if in_panel(ui) { width.min(ui.available_width()) } else { width };
    let size = egui::vec2(width, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_size(size);
        ui.set_max_width(width);
        add(ui, width);
    });
}

/// The crater brushes (plan 14e R4): the level theme's holes, none shown
/// without any.
fn craters(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let n = state.holes().len().min(usize::from(u8::MAX) + 1);
    if n == 0 {
        return;
    }
    let brush = state.brush();
    let current = crater_name(brush.shape, n);
    let label = ui.label(labels::CRATER);
    fixed(ui, CRATER_WIDTH, |ui, width| {
        egui::ComboBox::from_id_salt("olr-editor-crater")
            .selected_text(current)
            .width(width)
            .truncate()
            .show_ui(ui, |ui| {
                for i in 0..n {
                    let shape = BrushShape::Crater(i as u8);
                    if ui.selectable_label(brush.shape == shape, format!("Crater {}", i + 1)).clicked() {
                        out.push(EditorAction::Brush(Brush { shape, ..brush }));
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
}

/// The crater picker's text for `shape` with the theme's `holes`: a hole
/// the theme lacks reads "None", as it dabs round (plan 14e minors).
fn crater_name(shape: BrushShape, holes: usize) -> String {
    match shape {
        BrushShape::Crater(i) if usize::from(i) < holes => format!("Crater {}", u32::from(i) + 1),
        _ => "None".into(),
    }
}

/// The longest side of a swatch's picture, in pixels.
const SWATCH_SIDE: u32 = 64;

/// `img` as a swatch's picture: at most `SWATCH_SIDE` a side (nearest
/// pixel; a 512² texture would upload a megabyte for a 28-point square,
/// plan 14e minors); an empty image is one clear pixel.
fn swatch_image(img: &olr_data::image::RgbaImage) -> egui::ColorImage {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return egui::ColorImage::from_rgba_unmultiplied([1, 1], &[0; 4]);
    }
    let longest = w.max(h);
    let side =
        |v: u32| (u64::from(v) * u64::from(SWATCH_SIDE.min(longest)) / u64::from(longest)).max(1) as u32;
    let (sw, sh) = (side(w), side(h));
    let at = |v: u32, from: u32, to: u32| (u64::from(v) * u64::from(from) / u64::from(to)) as u32;
    let mut raw = Vec::with_capacity(sw as usize * sh as usize * 4);
    for y in 0..sh {
        for x in 0..sw {
            let p = img.get(at(x, w, sw), at(y, h, sh));
            raw.extend([p.r, p.g, p.b, p.a]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([sw as usize, sh as usize], &raw)
}

/// The swatch of `key` showing `img`, made when first asked for.
fn swatch(
    ui: &Ui,
    data: &mut EditorData,
    key: &str,
    img: &olr_data::image::RgbaImage,
) -> egui::TextureHandle {
    data.swatches
        .entry(key.to_string())
        .or_insert_with(|| {
            ui.ctx().load_texture(
                format!("olr-editor-swatch-{key}"),
                swatch_image(img),
                egui::TextureOptions::NEAREST,
            )
        })
        .clone()
}

/// A choice of the palette: its paint, its swatch's key, its image and its
/// label.
struct Entry<'a> {
    choice: PaintChoice,
    key: String,
    image: &'a olr_data::image::RgbaImage,
    name: String,
}

/// The palette's choices, grouped "Theme", "Built-in" and "Yours".
fn palette_entries(state: &EditorState) -> Vec<(&'static str, Vec<Entry<'_>>)> {
    let Some(doc) = state.active_document() else { return Vec::new() };
    let theme = &doc.level().theme;
    let entry = |choice, key, image, name: &str| Entry { choice, key, image, name: name.to_string() };
    let ours = vec![
        entry(PaintChoice::FrontTile, format!("theme/{theme}/front"), doc.front_tile(), labels::THEME_FRONT),
        entry(PaintChoice::BackTile, format!("theme/{theme}/back"), doc.back_tile(), labels::THEME_BACK),
    ];
    let builtin = state
        .builtin_textures()
        .iter()
        .enumerate()
        .map(|(i, t)| {
            entry(PaintChoice::Texture(TextureRef::Builtin(i)), format!("builtin/{i}"), &t.image, t.name)
        })
        .collect();
    let yours = state
        .catalog()
        .textures
        .iter()
        .map(|t| {
            let choice = PaintChoice::Texture(TextureRef::User(t.name.clone()));
            entry(choice, format!("user/{}", t.name), &t.image, &t.name)
        })
        .collect();
    vec![("Theme", ours), ("Built-in", builtin), ("Yours", yours)]
}

/// The "Textures…" button, its swatch the current paint's, and its popup
/// of swatches (plan 14e): a click paints with one, Remove takes the chosen
/// one of the user's out of the library, "Import texture…" opens the browser.
fn palette(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let groups = palette_entries(state);
    // Swatches no longer listed, or of an older library, go.
    if data.swatch_generation != state.textures_generation() {
        data.swatches.retain(|k, _| !k.starts_with("user/"));
        data.swatch_generation = state.textures_generation();
    }
    let listed: std::collections::BTreeSet<&str> =
        groups.iter().flat_map(|(_, g)| g.iter().map(|e| e.key.as_str())).collect();
    data.swatches.retain(|k, _| listed.contains(k.as_str()));
    let paint = state.paint();
    let current = groups.iter().flat_map(|(_, g)| g.iter()).find(|e| e.choice == *paint);
    let button = match current {
        Some(e) => {
            let tex = swatch(ui, data, &e.key, e.image);
            let image = egui::Image::new(&tex).fit_to_exact_size(egui::vec2(BUTTON_SWATCH, BUTTON_SWATCH));
            egui::Button::image_and_text(image, labels::TEXTURES)
        }
        None => egui::Button::new(labels::TEXTURES),
    };
    let r = ui.add_sized([TEXTURES_WIDTH, ui.spacing().interact_size.y], button);
    egui::Popup::menu(&r).show(|ui| {
        for (title, entries) in &groups {
            ui.label(egui::RichText::new(*title).strong());
            if entries.is_empty() {
                ui.label(egui::RichText::new("none yet: import one from a picture").weak());
            }
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(6.0 * (SWATCH + 80.0));
                for Entry { choice, key, image, name } in entries {
                    let tex = swatch(ui, data, key, image);
                    let image = egui::Image::new(&tex).fit_to_exact_size(egui::vec2(SWATCH, SWATCH));
                    let r =
                        ui.add(egui::Button::image_and_text(image, name.as_str()).selected(choice == paint));
                    if r.clicked() {
                        out.push(EditorAction::Paint(choice.clone()));
                    }
                }
                // The chosen texture of the user's is removed here: a context
                // menu inside the palette never shows (batch B review).
                if let PaintChoice::Texture(TextureRef::User(n)) = paint
                    && entries.iter().any(|e| e.choice == *paint)
                    && ui
                        .button(labels::REMOVE)
                        .on_hover_text(format!("remove {n} from your textures"))
                        .clicked()
                {
                    out.push(EditorAction::RemoveTexture(n.clone()));
                }
            });
        }
        if ui.button(labels::IMPORT_TEXTURE).clicked() {
            out.push(EditorAction::Browse(Purpose::ImportTexture));
        }
    });
}

/// The selected drawn layer's own material, when one is selected.
fn selected_material(state: &EditorState) -> Option<ElementMaterial> {
    let crate::state::Selection::Element(id) = state.selection() else { return None };
    let doc = state.active_document()?;
    match &doc.items()[doc.index_of(id)?].element.kind {
        olr_format::levels::ElementKind::Drawn { material, .. } => Some(*material),
        _ => None,
    }
}

/// "Paint as"'s name for `m`: "Layer's" says the selected layer's material
/// (a mixed layer's is hidden behind "Mixed" in its properties).
fn paint_as_name(m: Option<ElementMaterial>, layer: Option<ElementMaterial>) -> String {
    let name = |m: ElementMaterial| PAINT_AS.iter().find(|(p, _)| *p == Some(m)).map_or("", |(_, n)| n);
    match (m, layer) {
        (Some(m), _) => name(m).into(),
        (None, Some(l)) => format!("Layer's ({})", name(l)),
        (None, None) => "Layer's".into(),
    }
}

/// "Paint as": the material new paint lays.
fn paint_as(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let current = state.paint_material();
    let layer = selected_material(state);
    let name = paint_as_name(current, layer);
    let label = ui.label(labels::PAINT_AS);
    fixed(ui, PAINT_AS_WIDTH, |ui, width| {
        egui::ComboBox::from_id_salt("olr-editor-paint-as")
            .selected_text(name)
            .width(width)
            .truncate()
            .show_ui(ui, |ui| {
                for (m, _) in PAINT_AS {
                    let n = paint_as_name(m, layer);
                    if ui.selectable_label(m == current, n).clicked() && m != current {
                        out.push(EditorAction::PaintMaterial(m));
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
}

/// The chosen tool's own options: "Paint as" for the painting tools and
/// Fill, "Restore" for the eraser, "Tolerance" for Remove colour and Fill;
/// "By colour" (and its tolerance) for Select pixels and the Glow brush,
/// then the glow %: the brush's, and Select pixels' "Glow selection" (here,
/// as a tool change drops the pixel selection; one glow % for both).
fn options(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    match state.tool() {
        Tool::Brush | Tool::Rect | Tool::Ellipse | Tool::Line => paint_as(ui, state, out),
        Tool::Eraser => {
            let mut restore = state.restore();
            if ui
                .checkbox(&mut restore, labels::RESTORE)
                .on_hover_text("bring back a picture's erased pixels")
                .changed()
            {
                out.push(EditorAction::Restore(restore));
            }
        }
        Tool::Wand => {
            if let Some(t) =
                number(ui, data, "tolerance", labels::TOLERANCE, state.tolerance().into(), 0..=100)
            {
                out.push(EditorAction::Tolerance(t as u8));
            }
        }
        // Fill lays "Paint as" too (plan 14e minors).
        Tool::Fill => {
            paint_as(ui, state, out);
            if let Some(t) =
                number(ui, data, "fill-tolerance", labels::TOLERANCE, state.fill_tolerance().into(), 0..=100)
            {
                out.push(EditorAction::Tolerance(t as u8));
            }
        }
        Tool::SelectPixels | Tool::Glow => {
            // By colour's space is kept, its tolerance shown or not; in the
            // panel the tolerance goes under it.
            if in_panel(ui) {
                by_colour(ui, state, data, out);
            } else {
                let size = egui::vec2(BY_COLOUR_WIDTH, ui.spacing().interact_size.y);
                ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.set_min_size(size);
                    by_colour(ui, state, data, out);
                });
            }
            ui.separator();
            if state.tool() == Tool::SelectPixels
                && ui
                    .add_enabled(state.pixel_rect().is_some(), egui::Button::new(labels::GLOW_SELECTION))
                    .on_hover_text("the selected pixels glow at the glow % (0 clears); pick them first")
                    .clicked()
            {
                out.push(EditorAction::GlowSelection(state.glow_strength()));
            }
            if let Some(v) = number(
                ui,
                data,
                "glow-strength",
                labels::GLOW_STRENGTH_BRUSH,
                state.glow_strength().into(),
                0..=100,
            ) {
                out.push(EditorAction::GlowStrength(v as u8));
            }
        }
        Tool::Surface => surface_options(ui, state, out),
        Tool::Haze => {
            if let Some(v) = number(
                ui,
                data,
                "haze-strength",
                labels::HAZE_STRENGTH_BRUSH,
                state.haze_strength().into(),
                0..=100,
            ) {
                out.push(EditorAction::HazeStrength(v as u8));
            }
        }
        Tool::Select | Tool::Spawn | Tool::Eyedropper => {}
    }
}

/// The Surface brush's options (issue #43): its material (one of the
/// level's, or the element's own default), and Front and Back when the
/// selected element has back art.
fn surface_options(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let current = state.surface_paint();
    let name = |id: Option<&str>| match id {
        Some(id) => {
            doc.materials().iter().find(|m| m.id == id).map_or_else(|| id.to_string(), |m| m.name.clone())
        }
        None => labels::DEFAULT_SURFACE.to_string(),
    };
    let label = ui.label(labels::BRUSH_MATERIAL);
    fixed(ui, SURFACE_WIDTH, |ui, width| {
        egui::ComboBox::from_id_salt("olr-editor-surface-paint")
            .selected_text(name(current))
            .width(width)
            .truncate()
            .show_ui(ui, |ui| {
                let choices =
                    std::iter::once(None).chain(doc.materials().iter().map(|m| Some(m.id.as_str())));
                for id in choices {
                    if ui.selectable_label(id == current, name(id)).clicked() && id != current {
                        out.push(EditorAction::SurfacePaint(id.map(str::to_string)));
                    }
                }
            })
            .response
            .labelled_by(label.id)
            .on_hover_text("the element's own surface (Element's) or one of the level's materials");
    });
    let back_art = match state.selection() {
        crate::state::Selection::Element(id) => doc.index_of(id).is_some_and(|i| {
            let e = &doc.items()[i].element;
            e.has_back_art() && !e.is_distant()
        }),
        _ => false,
    };
    if back_art {
        group(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (back, n, hover) in [
                (false, labels::SURFACE_FRONT, "paint the element's own pixels"),
                (true, labels::SURFACE_BACK, "paint its back art: what shows where its front is dug"),
            ] {
                if ui.selectable_label(state.surface_back() == back, n).on_hover_text(hover).clicked() {
                    out.push(EditorAction::SurfaceBack(back));
                }
            }
        });
    }
}

/// "By colour", and its tolerance when on: Select pixels' or the Glow brush's.
fn by_colour(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let glow = state.tool() == Tool::Glow;
    let (mut on, hover) = if glow {
        (state.glow_by_colour(), "a click lights the connected pixels alike in colour")
    } else {
        (state.select_by_colour(), "a click picks the connected pixels alike in colour; Ctrl adds")
    };
    if ui.checkbox(&mut on, labels::BY_COLOUR).on_hover_text(hover).changed() {
        out.push(if glow { EditorAction::GlowByColour(on) } else { EditorAction::SelectByColour(on) });
    }
    if on && let Some(t) = number(ui, data, "tolerance", labels::TOLERANCE, state.tolerance().into(), 0..=100)
    {
        out.push(EditorAction::Tolerance(t as u8));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan 14d minors: "Layer's" names the layer's own material.
    #[test]
    fn paint_as_names_the_layers_material() {
        assert_eq!(paint_as_name(None, Some(ElementMaterial::Background)), "Layer's (Background)");
        assert_eq!(paint_as_name(None, None), "Layer's");
        assert_eq!(paint_as_name(Some(ElementMaterial::Rock), Some(ElementMaterial::Dirt)), "Rock");
    }

    /// Plan 14e minors: a crater the theme lacks reads "None", as it dabs round.
    #[test]
    fn the_crater_picker_names_only_offered_holes() {
        assert_eq!(crater_name(BrushShape::Crater(1), 3), "Crater 2");
        assert_eq!(crater_name(BrushShape::Crater(4), 3), "None");
        assert_eq!(crater_name(BrushShape::Round, 3), "None");
    }

    /// Plan 14e minors: a swatch is at most 64 pixels a side.
    #[test]
    fn swatches_are_small() {
        let big = olr_data::image::RgbaImage::new(512, 256);
        assert_eq!(swatch_image(&big).size, [64, 32]);
        let small = olr_data::image::RgbaImage::new(8, 8);
        assert_eq!(swatch_image(&small).size, [8, 8]);
        assert_eq!(swatch_image(&olr_data::image::RgbaImage::new(0, 0)).size, [1, 1]);
    }
}
