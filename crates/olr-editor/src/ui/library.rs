//! The level tab's asset library (issue #83, under the tools in the left
//! panel; issue #9's Assets tab before): Search (by name, case aside), the
//! thumbnails grouped Built-in then Yours, each a button named by its
//! asset with its name and tags under it; a click picks one for the next
//! press on the canvas (a second click unpicks), a drag carries it to the
//! canvas. Under them, for a picked one Place, Edit asset and (the user's
//! only) Remove asset, and for any New asset… and Import asset….
//! Thumbnail textures are cached by asset and the library's generation
//! (`EditorData::asset_thumbs`).

use egui::Ui;

use super::{EditorData, labels};
use crate::catalog::{AssetEntry, AssetKey};
use crate::state::{DocTab, EditorAction, EditorState};

/// A thumbnail's size (points): half `THUMB_SIZE`.
const THUMB: egui::Vec2 = egui::vec2(80.0, 48.0);
/// A card's width: the thumbnail's button.
const CARD: f32 = THUMB.x + 8.0;

/// Whether asset `a` answers the search `query` (lowercase, trimmed): its
/// name holds it, case aside.
pub fn matches(a: &AssetEntry, query: &str) -> bool {
    query.is_empty() || a.name.to_lowercase().contains(query)
}

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    // Thumbnails of an older library go.
    if data.asset_generation != state.assets_generation() {
        data.asset_thumbs.clear();
        data.asset_generation = state.assets_generation();
    }
    let search = egui::TextEdit::singleline(&mut data.asset_search)
        .hint_text(labels::SEARCH_ASSETS)
        .desired_width(ui.available_width());
    let r = ui.add(search);
    super::relabel(&r, egui::WidgetType::TextEdit, None, labels::SEARCH_ASSETS);
    let query = data.asset_search.trim().to_lowercase();
    let all = &state.catalog().assets;
    for (builtin, title) in [(true, labels::BUILT_IN), (false, labels::YOURS)] {
        ui.strong(title);
        let group: Vec<&AssetEntry> =
            all.iter().filter(|a| a.key.builtin == builtin && matches(a, &query)).collect();
        if group.is_empty() {
            let none = if !query.is_empty() {
                "None named so."
            } else if builtin {
                "None found."
            } else {
                "None yet: Save to library a copy's asset, make one with New asset…, or import one."
            };
            ui.add(egui::Label::new(egui::RichText::new(none).weak()).wrap());
        }
        // Rows of as many cards as fit (a column inside a wrapped row
        // never wraps).
        let gap = ui.spacing().item_spacing.x;
        let per_row = (((ui.available_width() + gap) / (CARD + gap)).floor() as usize).max(1);
        for row in group.chunks(per_row) {
            ui.horizontal(|ui| {
                for a in row {
                    card(ui, state, data, a, out);
                }
            });
        }
    }
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        // Issue #82: a copy of it at the view's centre, selected (the
        // keyboard's way to place).
        if let Some(key) = state.picked_asset().filter(|_| state.doc_tab() == DocTab::Level)
            && ui.button(labels::PLACE).on_hover_text("a copy of it at the view's centre").clicked()
        {
            out.push(EditorAction::PlaceAssetCentred(key.clone()));
        }
        if let Some(key) = state.picked_asset()
            && ui
                .button(labels::EDIT_LIBRARY_ASSET)
                .on_hover_text("open it in the asset tab; saving keeps it in your assets")
                .clicked()
        {
            out.push(EditorAction::EditAsset(key.clone()));
        }
        if let Some(key) = state.picked_asset().filter(|k| !k.builtin)
            && ui.button(labels::REMOVE_ASSET).on_hover_text("asks first").clicked()
        {
            out.push(EditorAction::RemoveAsset(key.id.clone()));
        }
        // Issue #77: the asset tab opens a blank asset.
        if ui.button(labels::NEW_ASSET).on_hover_text("a blank asset to paint in the asset tab").clicked() {
            out.push(EditorAction::NewAssetDialog);
        }
        if ui.button(labels::IMPORT_ASSET).on_hover_text("add an asset folder someone shared").clicked() {
            out.push(EditorAction::Browse(crate::browser::Purpose::ImportAsset));
        }
    });
}

/// An asset's thumbnail (a click picks it, a drag carries it to the
/// canvas), its name and its tag.
fn card(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    a: &AssetEntry,
    out: &mut Vec<EditorAction>,
) {
    let picked = state.picked_asset() == Some(&a.key);
    ui.vertical(|ui| {
        ui.set_width(CARD);
        let texture = thumb(ui, data, a);
        let button = match &texture {
            Some(t) => egui::Button::image(egui::Image::new(t).fit_to_exact_size(THUMB)),
            None => egui::Button::new("?").min_size(THUMB),
        };
        let r = ui.add(button.selected(picked).sense(egui::Sense::click_and_drag()));
        super::relabel(&r, egui::WidgetType::Button, Some(picked), &a.name);
        r.dnd_set_drag_payload(a.key.clone());
        if r.clicked() {
            let again = picked && state.placing();
            out.push(EditorAction::PickAsset((!again).then(|| a.key.clone())));
        }
        ui.add(egui::Label::new(&a.name).truncate());
        ui.add(egui::Label::new(egui::RichText::new(a.tag()).small().weak()).truncate());
    });
}

/// `a`'s thumbnail texture, made when first drawn.
fn thumb(ui: &Ui, data: &mut EditorData, a: &AssetEntry) -> Option<egui::TextureHandle> {
    let img = a.thumb.as_ref()?;
    let key: &AssetKey = &a.key;
    Some(
        data.asset_thumbs
            .entry(key.clone())
            .or_insert_with(|| {
                let (w, h) = (img.width(), img.height());
                let raw: Vec<u8> = (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .flat_map(|(x, y)| {
                        let p = img.get(x, y);
                        [p.r, p.g, p.b, p.a]
                    })
                    .collect();
                ui.ctx().load_texture(
                    format!("olr-editor-asset-{}-{}", key.builtin, key.id),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [img.width() as usize, img.height() as usize],
                        &raw,
                    ),
                    egui::TextureOptions::NEAREST,
                )
            })
            .clone(),
    )
}
