//! The selection's properties, each committed once (`number`, `text`).
//! An element's are shortened (issue #8): its name, what it is (Rock,
//! Dirt or Background), its glow, its surface materials (issue #43), its
//! place, and the rest under "More", folded by default
//! (`EditorSettings::more_open`). A folder's (issue #61): its name,
//! Visible and Locked, Ungroup, then a group's shared fields for its
//! elements.

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{Dither, ElementKind, ElementMaterial, LevelObject, MAX_PIXEL_SIZE, MAX_SIDE};

use super::{EditorData, labels, number, slider, slider_shown, text};
use crate::folder_ops::FolderField;
use crate::state::{EditorAction, EditorState, Selection};
use olr_data::level::{MAX_DEPTH, MIN_DEPTH};
use olr_format::levels::{Element, ElementGlass, MAX_GLASS_FROST};

const MATERIALS: [(ElementMaterial, &str); 3] = [
    (ElementMaterial::Rock, "Rock"),
    (ElementMaterial::Dirt, "Dirt"),
    (ElementMaterial::Background, "Background"),
];

const DITHERS: [(Dither, &str); 3] =
    [(Dither::None, "None"), (Dither::Ordered, "Ordered"), (Dither::Diffusion, "Diffusion")];

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    match state.selection() {
        Selection::Object(i) => {
            let Some(LevelObject::Spawn { at, .. }) = doc.level().objects.get(*i) else { return };
            ui.heading(format!("Spawn {}", i + 1));
            ui.label(format!("at ({}, {})", at.0, at.1));
            if ui.button("Delete").on_hover_text("Delete").clicked() {
                out.push(EditorAction::Remove);
            }
        }
        Selection::Element(id) => element(ui, state, data, id, out),
        Selection::Folder(id) => folder(ui, state, data, id, out),
        Selection::Many(_) => group(ui, state, out),
        Selection::None => {}
    }
}

/// An element's panel: name, what it is, glow and place; "More" holds the
/// rest.
fn element(ui: &mut Ui, state: &EditorState, data: &mut EditorData, id: &str, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let Some(index) = doc.index_of(id) else { return };
    let e = &doc.items()[index].element;
    let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::Set(id.to_string(), p));
    ui.heading(match e.kind {
        ElementKind::Image { .. } => "Image",
        ElementKind::Drawn { .. } => "Drawn layer",
        ElementKind::Raster => "Raster",
    });
    ui.weak(&e.id);
    if let Some(name) = text(ui, data, &format!("{id}/name"), labels::NAME, &e.name, false) {
        set(out, Property::Name(name));
    }
    let material = match &e.kind {
        ElementKind::Image { material, .. } | ElementKind::Drawn { material, .. } => Some(*material),
        ElementKind::Raster => None,
    };
    if let Some(material) = material {
        // A mixed layer is none of the three; a choice makes it plain (C4).
        let mixed = matches!(e.kind, ElementKind::Drawn { mixed: true, .. });
        let current = (!mixed).then_some(material);
        if let Some(m) = what_is_it(ui, current, e.locked) {
            set(out, Property::Material(m));
        }
        if mixed {
            ui.weak("Mixed: rock, dirt and background painted in one layer");
        }
        // An asset carries no distant layer (issue #77).
        if material == ElementMaterial::Background
            && !mixed
            && !state.active_document().is_some_and(|d| d.is_asset())
        {
            distant(ui, state, data, e, out);
        }
        if material == ElementMaterial::Background && e.distant.is_none() {
            glass(ui, state, data, e, out);
        }
    }
    super::look::element_glow(ui, state, data, e, out);
    super::look::element_surfaces(ui, state, e, out);
    place(ui, data, e, out);
    ui.separator();
    let open = state.settings().more_open;
    let r = egui::CollapsingHeader::new(labels::MORE)
        .id_salt("olr-editor-more")
        .open(Some(open))
        .show(ui, |ui| more(ui, data, e, index, out));
    if r.header_response.clicked() {
        out.push(EditorAction::ShowMore(!open));
    }
    save_as_asset(ui, state, out);
}

/// Save as asset… at the bottom of the properties (issue #9): the pixel
/// rectangle when there is one, else the selection.
fn save_as_asset(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    ui.separator();
    let hint = if state.pixel_rect().is_some() {
        "the selected rectangle, cut from every visible layer, into your assets"
    } else {
        "the selected elements into your assets"
    };
    if ui.button(labels::SAVE_AS_ASSET).on_hover_text(hint).clicked() {
        out.push(EditorAction::SaveAsAsset);
    }
}

/// "What is it?": Rock, Dirt or Background, one segmented switch; `current`
/// none for a mixed layer. Returns the choice, when it changes anything.
fn what_is_it(ui: &mut Ui, current: Option<ElementMaterial>, locked: bool) -> Option<ElementMaterial> {
    ui.label(egui::RichText::new(labels::WHAT_IS_IT).strong());
    let mut chosen = None;
    // In rows where one does not fit (issue #78).
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (m, n) in MATERIALS {
            let r = ui.add_enabled(!locked, egui::Button::selectable(current == Some(m), n)).on_hover_text(
                match m {
                    ElementMaterial::Rock => "worms and shots stop at it; nothing digs it",
                    ElementMaterial::Dirt => "worms and shots dig through it",
                    ElementMaterial::Background => "only seen: everything passes",
                },
            );
            if r.clicked() && current != Some(m) {
                chosen = Some(m);
            }
        }
    });
    chosen
}

/// Issue #11: the Distant switch and, when on, the Depth slider from near
/// to far; the hint when nothing in front is see-through yet.
fn distant(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    e: &Element,
    out: &mut Vec<EditorAction>,
) {
    let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::Set(e.id.clone(), p));
    let mut on = e.distant.is_some();
    let r = ui.add_enabled(!e.locked, egui::Checkbox::new(&mut on, labels::DISTANT)).on_hover_text(
        "decoration far behind the level: it scrolls slower and takes less light; \
         it shows where the back is see-through",
    );
    if r.changed() {
        set(out, Property::Distant(on.then_some(crate::ops::DEFAULT_DEPTH)));
    }
    let Some(depth) = e.distant else { return };
    if !e.locked
        && let Some(v) = slider(
            ui,
            data,
            &format!("{}/depth", e.id),
            labels::DEPTH,
            ("near", "far"),
            depth.into(),
            i64::from(MIN_DEPTH)..=i64::from(MAX_DEPTH),
        )
    {
        set(out, Property::Distant(Some(v as u8)));
    }
    if state.active_document().is_some_and(|d| !d.has_holes()) {
        ui.colored_label(ui.visuals().warn_fg_color, labels::DISTANT_HINT);
    }
}

/// The background glass design: the Glass switch and, when on, its
/// Opacity, Tint and Frost sliders (a locked element's hidden, as Glow's);
/// the hints while no distant layer shows through it or the level is not
/// lit (tint and frost show only lit).
fn glass(ui: &mut Ui, state: &EditorState, data: &mut EditorData, e: &Element, out: &mut Vec<EditorAction>) {
    let set = |out: &mut Vec<EditorAction>, g: Option<ElementGlass>| {
        out.push(EditorAction::Set(e.id.clone(), Property::Glass(g)))
    };
    let Some(doc) = state.active_document() else { return };
    let mut on = e.glass.is_some();
    let r = ui.add_enabled(!e.locked, egui::Checkbox::new(&mut on, labels::GLASS)).on_hover_text(
        "its pixels are see-through glass over the distant layers: lit as the layer behind, \
         its glow casts light but shows none",
    );
    if r.changed() {
        let opacity = crate::ops::DEFAULT_GLASS_OPACITY;
        set(out, on.then_some(ElementGlass { opacity, tint: 0, frost: 0 }));
    }
    let Some(g) = e.glass else { return };
    if !doc.see_through() {
        ui.colored_label(ui.visuals().warn_fg_color, labels::GLASS_HINT);
    }
    if e.locked {
        return;
    }
    let key = |k: &str| format!("{}/glass-{k}", e.id);
    if let Some(v) =
        slider(ui, data, &key("opacity"), labels::OPACITY, ("clear", "solid"), g.opacity.into(), 1..=99)
    {
        set(out, Some(ElementGlass { opacity: v as u8, ..g }));
    }
    let lit = doc.level().look.is_some();
    if let Some(v) =
        slider(ui, data, &key("tint"), labels::TINT, ("clear", "coloured"), g.tint.into(), 0..=100)
    {
        set(out, Some(ElementGlass { tint: v as u8, ..g }));
    }
    if !lit {
        ui.colored_label(ui.visuals().warn_fg_color, labels::GLASS_LOOK_HINT);
    }
    let frost = (labels::FROST, ("clear", "frosted"));
    let px = |v: f64| format!("{:.1} px", v / 10.0);
    if let Some(v) =
        slider_shown(ui, data, &key("frost"), frost, g.frost.into(), 0..=MAX_GLASS_FROST.into(), Some(px))
    {
        set(out, Some(ElementGlass { frost: v as u8, ..g }));
    }
    if !lit {
        ui.colored_label(ui.visuals().warn_fg_color, labels::GLASS_LOOK_HINT);
    }
}

/// "Place": X and Y, and an image's Width and Height.
fn place(ui: &mut Ui, data: &mut EditorData, e: &Element, out: &mut Vec<EditorAction>) {
    let id = &e.id;
    let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::Set(id.clone(), p));
    let (at, size) = match &e.kind {
        ElementKind::Drawn { at, .. } => (*at, None),
        ElementKind::Image { at, size, .. } => (*at, Some(*size)),
        ElementKind::Raster => return,
    };
    ui.separator();
    ui.label(egui::RichText::new(labels::PLACE).strong());
    let coord = i64::from(i32::MIN)..=i64::from(i32::MAX);
    let x = number(ui, data, &format!("{id}/x"), "X", at.0.into(), coord.clone());
    let y = number(ui, data, &format!("{id}/y"), "Y", at.1.into(), coord);
    if x.is_some() || y.is_some() {
        let to = (x.map_or(at.0, |v| v as i32), y.map_or(at.1, |v| v as i32));
        set(out, Property::At(to));
    }
    let Some(size) = size else { return };
    let side = 1..=i64::from(MAX_SIDE);
    let w = number(ui, data, &format!("{id}/w"), "Width", size.0.into(), side.clone());
    let h = number(ui, data, &format!("{id}/h"), "Height", size.1.into(), side);
    if w.is_some() || h.is_some() {
        let to = (w.map_or(size.0, |v| v as u32), h.map_or(size.1, |v| v as u32));
        set(out, Property::Size(to));
    }
}

/// "More": Visible, Locked, Lets light through, Split into layers, Merge
/// down, and an image's pixel size, colours, dither and threshold.
fn more(ui: &mut Ui, data: &mut EditorData, e: &Element, index: usize, out: &mut Vec<EditorAction>) {
    let id = &e.id;
    let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::Set(id.clone(), p));
    let mut visible = e.visible;
    if ui.checkbox(&mut visible, labels::VISIBLE).changed() {
        set(out, Property::Visible(visible));
    }
    let mut locked = e.locked;
    if ui.checkbox(&mut locked, labels::LOCKED).changed() {
        set(out, Property::Locked(locked));
    }
    super::look::element_clear(ui, e, out);
    if ui
        .add_enabled(!e.locked && e.visible, egui::Button::new(labels::SPLIT))
        .on_hover_text("its rock, dirt and background as drawn layers, to paint and erase")
        .clicked()
    {
        out.push(EditorAction::Split(id.clone()));
    }
    if index > 0
        && ui
            .add_enabled(!e.locked, egui::Button::new(labels::MERGE_DOWN))
            .on_hover_text("into the drawn layer below: rock, then dirt, then background wins (Ctrl+E)")
            .clicked()
    {
        out.push(EditorAction::MergeDown(id.clone()));
    }
    let ElementKind::Image { pixel_size, colours, dither, alpha_threshold, .. } = &e.kind else { return };
    ui.separator();
    if let Some(v) = number(
        ui,
        data,
        &format!("{id}/pixel"),
        labels::PIXEL_SIZE,
        (*pixel_size).into(),
        1..=MAX_PIXEL_SIZE.into(),
    ) {
        set(out, Property::PixelSize(v as u32));
    }
    let mut reduce = colours.is_some();
    if ui.checkbox(&mut reduce, "Reduce colours").changed() {
        set(out, Property::Colours(reduce.then_some(16)));
    }
    if let Some(n) = colours
        && let Some(v) = number(ui, data, &format!("{id}/colours"), labels::COLOURS, (*n).into(), 2..=256)
    {
        set(out, Property::Colours(Some(v as u16)));
    }
    let name = DITHERS.iter().find(|(d, _)| d == dither).map_or("", |(_, n)| n);
    egui::ComboBox::from_label(labels::DITHER).selected_text(name).show_ui(ui, |ui| {
        for (d, n) in DITHERS {
            if ui.selectable_label(d == *dither, n).clicked() && d != *dither {
                set(out, Property::Dither(d));
            }
        }
    });
    if let Some(v) =
        number(ui, data, &format!("{id}/threshold"), labels::THRESHOLD, (*alpha_threshold).into(), 0..=255)
    {
        set(out, Property::Threshold(v as u8));
    }
}

/// A folder's panel (issue #61): its Name (committed when typing ends),
/// Visible and Locked (its own), Ungroup, then its elements' count and
/// shared material, Delete and Save as asset.
fn folder(ui: &mut Ui, state: &EditorState, data: &mut EditorData, id: &str, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let Some(f) = doc.folders().into_iter().find(|f| f.id == id) else { return };
    let set = |out: &mut Vec<EditorAction>, field: FolderField| {
        out.push(EditorAction::SetFolder(id.to_string(), field))
    };
    ui.heading("Folder");
    ui.weak(&f.id);
    if let Some(name) = text(ui, data, &format!("{id}/name"), labels::NAME, &f.name, false) {
        set(out, FolderField::Name(name));
    }
    let mut visible = f.visible;
    if ui.checkbox(&mut visible, labels::VISIBLE).changed() {
        set(out, FolderField::Visible(visible));
    }
    let mut locked = f.locked;
    if ui.checkbox(&mut locked, labels::LOCKED).changed() {
        set(out, FolderField::Locked(locked));
    }
    if ui
        .button(labels::UNGROUP)
        .on_hover_text(format!("its members take its place ({})", labels::UNGROUP_KEYS))
        .clicked()
    {
        out.push(EditorAction::Ungroup);
    }
    ui.separator();
    let ids = state.selected_elements();
    ui.weak(format!("{} elements", ids.len()));
    shared(ui, state, &ids, olr_edit::folders::folder_locked(&doc.composition(), id), out);
    if ui.button("Delete").on_hover_text("the folder and everything in it").clicked() {
        out.push(EditorAction::Remove);
    }
    save_as_asset(ui, state, out);
}

/// A group's panel (plan 14e R8): the count, Visible and Locked (on when
/// all are), the material when all are images or drawn layers ("Different"
/// when they disagree); no Merge down nor Split. A selected folder stands
/// for its elements (issue #61).
fn group(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let ids = state.selected_elements();
    let members: Vec<_> =
        ids.iter().filter_map(|id| doc.index_of(id)).map(|i| &doc.items()[i].element).collect();
    ui.heading(format!("{} elements", members.len()));
    let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::SetMany(ids.to_vec(), p));
    let mut visible = members.iter().all(|e| e.visible);
    if ui.checkbox(&mut visible, labels::VISIBLE).changed() {
        set(out, Property::Visible(visible));
    }
    let mut locked = members.iter().all(|e| e.locked);
    if ui.checkbox(&mut locked, labels::LOCKED).changed() {
        set(out, Property::Locked(locked));
    }
    shared(ui, state, &ids, false, out);
    if ui.button("Delete").clicked() {
        out.push(EditorAction::Remove);
    }
    save_as_asset(ui, state, out);
}

/// The fields elements `ids` share (plan 14e R8): the material when all
/// are images or drawn layers ("Different" when they disagree), disabled
/// when `locked`.
fn shared(ui: &mut Ui, state: &EditorState, ids: &[String], locked: bool, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let members: Vec<_> =
        ids.iter().filter_map(|id| doc.index_of(id)).map(|i| &doc.items()[i].element).collect();
    let set = |out: &mut Vec<EditorAction>, p: Property| out.push(EditorAction::SetMany(ids.to_vec(), p));
    let materials: Option<Vec<(ElementMaterial, bool)>> = members
        .iter()
        .map(|e| match &e.kind {
            ElementKind::Image { material, .. } => Some((*material, false)),
            ElementKind::Drawn { material, mixed, .. } => Some((*material, *mixed)),
            ElementKind::Raster => None,
        })
        .collect();
    if let Some(materials) = materials {
        let first = materials.first().copied();
        let same = materials.iter().all(|m| Some(*m) == first);
        let current = match first {
            Some((m, false)) if same => Some(m),
            _ => None,
        };
        if let Some(m) = what_is_it(ui, current, locked) {
            set(out, Property::Material(m));
        }
        if !same {
            ui.weak("Different");
        } else if matches!(first, Some((_, true))) {
            ui.weak("Mixed");
        }
    }
}
