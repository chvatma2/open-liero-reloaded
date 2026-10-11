//! The options (spec §8.3, §21 G3; OLX's options menu,
//! `src/client/DeprecatedGUI/Menu_Options.cpp`), in three tabs:
//!
//! - **Video:** the display, one list of the window's size presets (the
//!   window is never resized by dragging) and borderless fullscreen, VSync or uncapped, the upscale filter,
//!   interpolation, the HUD's layout and the UI scale. Each change is applied at once by the
//!   app, the window's size and mode through its paced requests (at most
//!   one change every 250 ms, however fast the player clicks).
//! - **Audio:** the master volume (OLX's sound volume) and the effects'.
//! - **Keys** (`keys.rs`).
//!
//! The match (OLX's game settings) is set up in local play, before each
//! game, and remembered there: it has no tab here.
//!
//! Every change is saved (the app writes the file at most once a second).

use egui::{Button, ComboBox, RichText, Slider, Ui, vec2};

use super::{MenuData, form_row, keys, page, section};
use crate::settings::{FrameRate, HudArrangement, UI_SCALE_RANGE, UpscaleFilter, WindowPreset};
use crate::state::{UiAction, UiState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Video,
    Audio,
    Keys,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Video, Tab::Audio, Tab::Keys];

    fn label(self) -> &'static str {
        match self {
            Tab::Video => "Video",
            Tab::Audio => "Audio",
            Tab::Keys => "Keys",
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

/// The page's column, the keys' table's width (one for every tab, so
/// the tabs and Back stay in place).
const WIDTH: f32 = 720.0;

/// A tab's button.
const TAB: egui::Vec2 = vec2(120.0, 34.0);

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut v = std::mem::take(&mut data.options);
    let mut ui = page(ui, "Options", WIDTH, out);
    let ui = &mut ui;
    // The tabs, centred over the column.
    ui.horizontal(|ui| {
        let n = Tab::ALL.len() as f32;
        let row = TAB.x * n + ui.spacing().item_spacing.x * (n - 1.0);
        ui.add_space(((ui.available_width() - row) / 2.0).max(0.0));
        for t in Tab::ALL {
            let tab = Button::selectable(v.tab == t, RichText::new(t.label()).size(18.0));
            if ui.add_sized(TAB, tab).clicked() {
                v.tab = t;
            }
        }
    });
    ui.add_space(10.0);
    match v.tab {
        Tab::Video => section(ui, "Window and drawing", |ui, _| video(ui, state, &mut v, out)),
        Tab::Audio => section(ui, "Volume", |ui, _| audio(ui, state, out)),
        Tab::Keys => section(ui, "Keyboard and gamepad", |ui, _| {
            keys::show(ui, state, &mut data.capture, &mut v.keys, out)
        }),
    }
    if v.tab != Tab::Keys {
        data.capture = None;
    }
    data.options = v;
}

/// The display list's borderless fullscreen, at the monitor's size.
const FULLSCREEN: &str = "Fullscreen";

fn video(ui: &mut Ui, state: &UiState, view: &mut View, out: &mut Vec<UiAction>) {
    let before = &state.settings().video;
    let mut v = before.clone();
    ui.spacing_mut().item_spacing.y = 10.0;
    // The window sizes and fullscreen in one list: fullscreen keeps the
    // size to return to.
    form_row(ui, "Display", |ui, label| {
        let shown = if v.fullscreen { FULLSCREEN } else { v.window.label() };
        ComboBox::from_id_salt("display")
            .selected_text(shown)
            .width(200.0)
            .show_ui(ui, |ui| {
                for p in WindowPreset::ALL {
                    if ui.selectable_label(!v.fullscreen && v.window == p, p.label()).clicked() {
                        v.window = p;
                        v.fullscreen = false;
                    }
                }
                if ui.selectable_label(v.fullscreen, FULLSCREEN).clicked() {
                    v.fullscreen = true;
                }
            })
            .response
            .labelled_by(label);
    });
    form_row(ui, "Frame rate", |ui, _| {
        ui.radio_value(&mut v.frame_rate, FrameRate::VSync, "VSync");
        ui.radio_value(&mut v.frame_rate, FrameRate::Uncapped, "Uncapped");
    });
    form_row(ui, "Upscaling", |ui, _| {
        ui.radio_value(&mut v.filter, UpscaleFilter::Sharp, "Sharp")
            .on_hover_text("Fills the window, keeping the aspect.");
        ui.radio_value(&mut v.filter, UpscaleFilter::Nearest, "Nearest")
            .on_hover_text("Whole multiples of the art, black bars around.");
    });
    form_row(ui, "", |ui, _| {
        ui.checkbox(&mut v.interpolation, "Smooth motion").on_hover_text(
            "Draws between the 100 Hz ticks; off draws the latest tick at whole pixels, like OLX.",
        );
    });
    form_row(ui, "HUD layout", |ui, label| {
        ComboBox::from_id_salt("hud layout")
            .selected_text(v.hud_layout.label())
            .width(200.0)
            .show_ui(ui, |ui| {
                for a in HudArrangement::ALL {
                    ui.selectable_value(&mut v.hud_layout, a, a.label());
                }
            })
            .response
            .labelled_by(label)
            .on_hover_text("On a window wider than 16:9, side panels can hold the health, weapons and log.");
    });
    form_row(ui, "UI scale", |ui, label| {
        let mut scale = view.scale.unwrap_or(v.ui_scale);
        let r = ui
            .add(Slider::new(&mut scale, UI_SCALE_RANGE).step_by(0.05).fixed_decimals(2))
            .labelled_by(label);
        if r.dragged() {
            view.scale = Some(scale);
        } else if view.scale.take().is_some() || r.changed() {
            v.ui_scale = scale;
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new("The window changes size or mode at most every quarter second.").weak());
    if v != *before {
        out.push(UiAction::SetVideo(v));
    }
}

fn audio(ui: &mut Ui, state: &UiState, out: &mut Vec<UiAction>) {
    let before = &state.settings().audio;
    let mut a = before.clone();
    ui.spacing_mut().item_spacing.y = 10.0;
    let volume = |ui: &mut Ui, v: &mut f32, name: &str| {
        form_row(ui, name, |ui, label| {
            let mut pct = (*v * 100.0).round() as i32;
            if ui.add(Slider::new(&mut pct, 0..=100).suffix(" %")).labelled_by(label).changed() {
                *v = pct as f32 / 100.0;
            }
        });
    };
    volume(ui, &mut a.master, "Master volume");
    volume(ui, &mut a.sfx, "Effects volume");
    if a != *before {
        out.push(UiAction::SetAudio(a));
    }
}
