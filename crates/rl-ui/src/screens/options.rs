//! The options (spec §8.3, §21 G3; OLX's options menu,
//! `src/client/DeprecatedGUI/Menu_Options.cpp`), in four sections:
//!
//! - **Video:** the window's size preset (the window is never resized by
//!   dragging), fullscreen, VSync or uncapped, the upscale filter,
//!   interpolation and the UI scale. Each change is applied at once by the
//!   app, the window's size and mode through its paced requests (at most
//!   one change every 250 ms, however fast the player clicks).
//! - **Audio:** the master volume (OLX's sound volume) and the effects'.
//! - **Keys** (`keys.rs`).
//! - **Gameplay:** the match local play starts with.
//!
//! Every change is saved (the app writes the file at most once a second).

use egui::{ComboBox, RichText, Slider, Ui};

use super::{MenuData, header, keys, local_play};
use crate::match_prefs::MatchPrefs;
use crate::settings::{FrameRate, UI_SCALE_RANGE, UpscaleFilter, WindowPreset};
use crate::state::{UiAction, UiState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Video,
    Audio,
    Keys,
    Gameplay,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Video, Tab::Audio, Tab::Keys, Tab::Gameplay];

    fn label(self) -> &'static str {
        match self {
            Tab::Video => "Video",
            Tab::Audio => "Audio",
            Tab::Keys => "Keys",
            Tab::Gameplay => "Gameplay",
        }
    }
}

/// The screen's own state.
#[derive(Clone, Debug, Default)]
pub(super) struct View {
    tab: Tab,
    /// The UI scale while its slider is dragged: applied on release, so the
    /// slider does not move under the pointer as the menus grow.
    scale: Option<f32>,
    pub(super) keys: keys::View,
}

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    header(ui, "Options", out);
    let mut v = std::mem::take(&mut data.options);
    ui.horizontal(|ui| {
        for t in Tab::ALL {
            ui.selectable_value(&mut v.tab, t, RichText::new(t.label()).size(18.0));
        }
    });
    ui.separator();
    ui.add_space(6.0);
    match v.tab {
        Tab::Video => video(ui, state, &mut v, out),
        Tab::Audio => audio(ui, state, out),
        Tab::Keys => keys::show(ui, state, &mut data.capture, &mut v.keys, out),
        Tab::Gameplay => gameplay(ui, state, data, out),
    }
    if v.tab != Tab::Keys {
        data.capture = None;
    }
    data.options = v;
}

fn video(ui: &mut Ui, state: &UiState, view: &mut View, out: &mut Vec<UiAction>) {
    let before = &state.settings().video;
    let mut v = before.clone();
    egui::Grid::new("video").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        let label = ui.label("Window size");
        ComboBox::from_id_salt("window size")
            .selected_text(v.window.label())
            .width(200.0)
            .show_ui(ui, |ui| {
                for p in WindowPreset::ALL {
                    ui.selectable_value(&mut v.window, p, p.label());
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut v.fullscreen, "Fullscreen");
        ui.end_row();
        ui.label("Frame rate");
        ui.horizontal(|ui| {
            ui.radio_value(&mut v.frame_rate, FrameRate::VSync, "VSync");
            ui.radio_value(&mut v.frame_rate, FrameRate::Uncapped, "Uncapped");
        });
        ui.end_row();
        ui.label("Upscaling");
        ui.horizontal(|ui| {
            ui.radio_value(&mut v.filter, UpscaleFilter::Sharp, "Sharp")
                .on_hover_text("Fills the window, keeping the aspect.");
            ui.radio_value(&mut v.filter, UpscaleFilter::Nearest, "Nearest")
                .on_hover_text("Whole multiples of the art, black bars around.");
        });
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut v.interpolation, "Smooth motion").on_hover_text(
            "Draws between the 100 Hz ticks; off draws the latest tick at whole pixels, like OLX.",
        );
        ui.end_row();
        ui.label("UI scale");
        let mut scale = view.scale.unwrap_or(v.ui_scale);
        let r = ui.add(Slider::new(&mut scale, UI_SCALE_RANGE).step_by(0.05).fixed_decimals(2));
        if r.dragged() {
            view.scale = Some(scale);
        } else if view.scale.take().is_some() || r.changed() {
            v.ui_scale = scale;
        }
        ui.end_row();
    });
    ui.add_space(8.0);
    ui.label(RichText::new("The window changes size or mode at most every quarter second.").weak());
    if v != *before {
        out.push(UiAction::SetVideo(v));
    }
}

fn audio(ui: &mut Ui, state: &UiState, out: &mut Vec<UiAction>) {
    let before = &state.settings().audio;
    let mut a = before.clone();
    let volume = |ui: &mut Ui, v: &mut f32, name: &str| {
        let mut pct = (*v * 100.0).round() as i32;
        if ui.add(Slider::new(&mut pct, 0..=100).suffix(" %").text(name)).changed() {
            *v = pct as f32 / 100.0;
        }
    };
    volume(ui, &mut a.master, "Master volume");
    volume(ui, &mut a.sfx, "Effects volume");
    if a != *before {
        out.push(UiAction::SetAudio(a));
    }
}

fn gameplay(ui: &mut Ui, state: &UiState, data: &MenuData, out: &mut Vec<UiAction>) {
    ui.label(RichText::new("The match local play starts with (it changes them too):").weak());
    let before = &state.settings().gameplay.match_prefs;
    let mut p = before.clone();
    egui::ScrollArea::vertical().id_salt("options match").show(ui, |ui| {
        local_play::rules(ui, data, &mut p);
        ui.add_space(8.0);
        local_play::bots(ui, &mut p);
        ui.add_space(8.0);
        if ui.button("Reset the match to OLX's settings").clicked() {
            // The mod and level stay; the mod's presets decide the weapons.
            let mut d = MatchPrefs {
                mod_id: p.mod_id.clone(),
                level_id: p.level_id.clone(),
                ..MatchPrefs::default()
            };
            if let Some(m) = data.catalog.mod_info(&d.mod_id) {
                d.set_mod(m);
            }
            p = d;
        }
    });
    if p != *before {
        out.push(UiAction::SetMatch(p));
    }
}
