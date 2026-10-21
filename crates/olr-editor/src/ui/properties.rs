//! The selection's properties, each committed once (`number`, `text`).
//! An element's are shortened (issue #8): its name, what it is (Rock,
//! Dirt or Background), its glow, its place, and the rest under "More",
//! folded by default (`EditorSettings::more_open`).

use egui::Ui;
use olr_edit::element::Property;
use olr_format::levels::{Dither, ElementKind, ElementMaterial, LevelObject, MAX_PIXEL_SIZE, MAX_SIDE};

use super::{EditorData, labels, number, text};
use crate::state::{EditorAction, EditorState, Selection};
use olr_format::levels::Element;

const MATERIALS: [(ElementMaterial, &str); 3] = [
    (ElementMaterial::Rock, "Rock"),
    (ElementMaterial::Dirt, "Dirt"),
    (ElementMaterial::Background, "Background"),
];

const DITHERS: [(Dither, &str); 3] =
    [(Dither::None, "None"), (Dither::Ordered, "Ordered"), (Dither::Diffusion, "Diffusion")];

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
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
        Selection::Many(ids) => group(ui, state, ids, out),
        Selection::None => {}
    }
}

/// An element's panel: name, what it is, glow and place; "More" holds the
/// rest.
fn element(ui: &mut Ui, state: &EditorState, data: &mut EditorData, id: &str, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
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
    }
    super::look::element_glow(ui, state, data, e, out);
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
    ui.horizontal(|ui| {
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

/// A group's panel (plan 14e R8): the count, Visible and Locked (on when
/// all are), the material when all are images or drawn layers ("Different"
/// when they disagree); no Merge down nor Split.
fn group(ui: &mut Ui, state: &EditorState, ids: &[String], out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
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
        if let Some(m) = what_is_it(ui, current, false) {
            set(out, Property::Material(m));
        }
        if !same {
            ui.weak("Different");
        } else if matches!(first, Some((_, true))) {
            ui.weak("Mixed");
        }
    }
    if ui.button("Delete").clicked() {
        out.push(EditorAction::Remove);
    }
    save_as_asset(ui, state, out);
}
