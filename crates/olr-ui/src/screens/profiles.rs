//! The profiles (OLX's player menu, `src/client/DeprecatedGUI/Menu_Player.cpp`):
//! the list with New, Delete (confirmed, never the last) and "Play as
//! this"; the edited profile's name (validated: `Profiles::check_name`),
//! colour (OLX's red, green and blue sliders) and skin (every imported
//! skin, searchable); and a live preview of its skin in its colour, faced
//! right and left, walking and aiming as OLX's menu animates it.

use egui::{Button, Color32, RichText, ScrollArea, Sense, Slider, TextEdit, Ui, Vec2};
use olr_sim::Dir;

use olr_content::Catalog;

use super::{MenuData, columns, page, tall_section};
use crate::profiles::{NameError, Profiles};
use crate::skin_preview::{PreviewAnim, PreviewKey, PreviewTextures};
use crate::state::{UiAction, UiState};

/// The page's column: three panels side by side.
const WIDTH: f32 = 1000.0;

/// The preview's size: a worm (10 × 9 world pixels) drawn 8 points a
/// world pixel, 4 a pixel of OLX's 20 × 18 sprite (a double-resolution
/// skin's 40 × 36 fills the same size at 2, issue #12).
const PREVIEW_SIZE: Vec2 = Vec2::new(80.0, 72.0);

/// The widgets' own state: which profile is edited, its name as typed,
/// the skin search, a pending delete and the preview's animation.
#[derive(Clone, Debug, Default)]
pub(super) struct View {
    editing: usize,
    /// The name field and the profile it belongs to.
    name: String,
    name_of: Option<usize>,
    error: Option<NameError>,
    search: String,
    confirm_delete: bool,
    anim: PreviewAnim,
    /// The preview stands still (a click toggles it, as in OLX).
    still: bool,
}

impl View {
    /// The screen opens on the profile playing.
    pub(super) fn new(selected: usize) -> View {
        View { editing: selected, ..View::default() }
    }

    /// Edits profile `i`: its name in the field, no delete pending.
    fn edit(&mut self, i: usize) {
        self.editing = i;
        self.name_of = None;
        self.confirm_delete = false;
    }
}

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, "Profiles", WIDTH, out);
    let ui = &mut ui;
    let before = state.profiles();
    let mut p = before.clone();
    let mut v = std::mem::take(&mut data.profiles);
    if v.editing >= p.list.len() {
        v.editing = p.selected.min(p.list.len().saturating_sub(1));
    }
    // No repaint is asked for: the game draws every frame (its loop polls),
    // and a test harness settles.
    if !v.still {
        v.anim.step(ui.input(|inp| inp.stable_dt));
    }
    columns(ui, 3, |c| {
        tall_section(&mut c[0], "Players", |ui, _| list(ui, &mut p, &mut v));
        tall_section(&mut c[1], "Look", |ui, _| editor(ui, &mut p, &mut v, data.preview.as_ref()));
        let mut search = std::mem::take(&mut v.search);
        tall_section(&mut c[2], "Skin", |ui, title| {
            skins(ui, &mut p, v.editing, &mut search, &data.catalog, title)
        });
        v.search = search;
    });
    let edited = &p.list[v.editing];
    data.preview_wanted = Some(PreviewKey { skin_id: edited.skin_id.clone(), colour: edited.colour });
    data.profiles = v;
    if p != *before {
        out.push(UiAction::SetProfiles(p));
    }
}

/// The profiles, the one playing marked, and what can be done with them.
fn list(ui: &mut Ui, p: &mut Profiles, v: &mut View) {
    ScrollArea::vertical().id_salt("profiles list").max_height(360.0).show(ui, |ui| {
        for (n, prof) in p.list.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.selectable_label(n == v.editing, &prof.name).clicked() && n != v.editing {
                    v.edit(n);
                }
                if n == p.selected {
                    ui.label(RichText::new("playing").small().color(Color32::from_rgb(120, 200, 120)));
                }
            });
        }
        crate::pad::follow_focus(ui);
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("New").clicked() {
            v.edit(p.add());
        }
        if ui.add_enabled(p.list.len() > 1, Button::new("Delete")).clicked() {
            v.confirm_delete = true;
        }
        let can_play = v.editing != p.selected;
        if ui.add_enabled(can_play, Button::new("Play as this")).clicked() {
            p.selected = v.editing;
        }
    });
    if v.confirm_delete {
        ui.label(format!("Delete {}?", p.list[v.editing].name));
        ui.horizontal(|ui| {
            if ui.button("Yes, delete").clicked() {
                p.remove(v.editing);
                v.edit(p.selected);
            }
            if ui.button("No").clicked() {
                v.confirm_delete = false;
            }
        });
    }
}

/// The edited profile's name and colour, and its preview.
fn editor(ui: &mut Ui, p: &mut Profiles, v: &mut View, preview: Option<&PreviewTextures>) {
    let i = v.editing;
    // The name field follows the profile, unless it holds a refused name.
    if v.name_of != Some(i) || (v.error.is_none() && v.name.trim() != p.list[i].name) {
        v.name = p.list[i].name.clone();
        v.name_of = Some(i);
        v.error = None;
    }
    let label = ui.label(RichText::new("Name").strong());
    let field =
        TextEdit::singleline(&mut v.name).char_limit(crate::MAX_NAME_CHARS).desired_width(f32::INFINITY);
    if ui.add(field).labelled_by(label.id).changed() {
        v.error = p.rename(i, &v.name).err();
    }
    if let Some(e) = v.error {
        ui.label(RichText::new(e.to_string()).color(Color32::from_rgb(255, 110, 90)));
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Colour").strong());
    let c = &mut p.list[i].colour;
    for (k, name) in ["Red", "Green", "Blue"].into_iter().enumerate() {
        ui.add(Slider::new(&mut c[k], 0..=255).text(name));
    }
    ui.color_edit_button_srgb(c);
    ui.add_space(8.0);
    let key = PreviewKey { skin_id: p.list[i].skin_id.clone(), colour: p.list[i].colour };
    let size = PREVIEW_SIZE;
    let ready = preview.filter(|t| t.key == key && !t.right.is_empty());
    let f = v.anim.frame() as usize;
    let clicked = ui
        .horizontal(|ui| {
            let mut clicked = false;
            for face in [Dir::Right, Dir::Left] {
                let (rect, resp) = ui.allocate_exact_size(size + Vec2::splat(8.0), Sense::click());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 4.0, Color32::BLACK);
                if let Some(t) = ready {
                    let ids = if face == Dir::Right { &t.right } else { &t.left };
                    let id = ids[f.min(ids.len() - 1)];
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    painter.image(id, egui::Rect::from_center_size(rect.center(), size), uv, Color32::WHITE);
                }
                clicked |= resp.clicked();
            }
            clicked
        })
        .inner;
    if clicked {
        v.still = !v.still;
    }
    ui.label(RichText::new("Click the worm to stop or start it.").small().weak());
}

/// Every imported skin, searchable; a click gives the edited profile it.
fn skins(ui: &mut Ui, p: &mut Profiles, i: usize, search: &mut String, catalog: &Catalog, label: egui::Id) {
    let field = TextEdit::singleline(search).hint_text("Search skins").desired_width(f32::INFINITY);
    ui.add(field).labelled_by(label);
    let current = catalog.skin(&p.list[i].skin_id);
    if let Some(s) = current {
        ui.label(format!("Current: {}", s.name));
    } else {
        let text = format!("{}: not imported, the default is used", p.list[i].skin_id);
        ui.label(RichText::new(text).color(Color32::from_rgb(255, 110, 90)));
    }
    let needle = search.trim().to_lowercase();
    let shown: Vec<_> = catalog
        .skins
        .iter()
        .filter(|s| needle.is_empty() || s.name.to_lowercase().contains(&needle) || s.id.contains(&needle))
        .collect();
    ui.label(RichText::new(format!("{} of {} skins", shown.len(), catalog.skins.len())).small().weak());
    ScrollArea::vertical().id_salt("skins").auto_shrink([false, false]).show(ui, |ui| {
        for s in shown {
            let on = current.is_some_and(|c| c.id == s.id);
            if ui.selectable_label(on, &s.name).clicked() && !on {
                p.list[i].skin_id = s.id.clone();
            }
        }
        crate::pad::follow_focus(ui);
    });
}
