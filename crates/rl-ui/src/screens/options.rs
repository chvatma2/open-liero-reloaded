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
//! - **Online:** finding games (plan 12a): the region a hosted game says,
//!   the public address it gives for direct joins (a manual port forward;
//!   empty: none), whether a hosted game opens its port on the router
//!   (plan 12b, on by default), the Nostr relays and the STUN servers
//!   (editable lists, each with its Reset to the game's defaults), the
//!   player's own TURN relays (`user:pass@host:port`, none by default) and
//!   the blocked hosts (Unblock); the download cache's size and its Clear
//!   (plan 11; the app disables it while an online game is on). Relay
//!   edits reconnect the relays in use (the Internet tab's, a hosted
//!   game's listening); the other settings apply to the next game hosted
//!   or joined.
//!
//! The match (OLX's game settings) is set up in local play, before each
//! game, and remembered there: it has no tab here.
//!
//! Every change is saved (the app writes the file at most once a second).

use egui::{Button, ComboBox, Key, RichText, ScrollArea, Slider, TextEdit, Ui, vec2};

use super::{MenuData, form_row, keys, page, section, tab_row};
use crate::online::{
    FindingSettings, MAX_SERVERS, MAX_TURN, Region, megabytes, turn_shown, valid_relay, valid_stun,
    valid_turn,
};
use crate::settings::{FrameRate, HudArrangement, UI_SCALE_RANGE, UpscaleFilter, WindowPreset};
use crate::state::{UiAction, UiState};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Video,
    Audio,
    Keys,
    Online,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Video, Tab::Audio, Tab::Keys, Tab::Online];

    fn label(self) -> &'static str {
        match self {
            Tab::Video => "Video",
            Tab::Audio => "Audio",
            Tab::Keys => "Keys",
            Tab::Online => "Online",
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
    /// The Online tab's new relay, STUN server and TURN relay being typed.
    new_relay: String,
    new_stun: String,
    new_turn: String,
    /// The public address being typed (`None`: the saved one, not edited).
    address: Option<String>,
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
        let (tab_size, indent) = tab_row(ui, TAB, Tab::ALL.len());
        ui.add_space(indent);
        for t in Tab::ALL {
            let tab = Button::selectable(v.tab == t, RichText::new(t.label()).size(18.0));
            if ui.add_sized(tab_size, tab).clicked() {
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
        Tab::Online => {
            ScrollArea::vertical().id_salt("online options").auto_shrink([false, true]).show(ui, |ui| {
                online(ui, state, data, &mut v, out);
                ui.add_space(10.0);
                section(ui, "Downloaded content", |ui, _| downloads(ui, data, out));
            });
        }
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

/// The download cache's size and its Clear (it keeps nothing: the menus
/// have no match), disabled with the reason while an online game is on.
fn downloads(ui: &mut Ui, data: &MenuData, out: &mut Vec<UiAction>) {
    ui.spacing_mut().item_spacing.y = 10.0;
    match &data.download_cache {
        Ok(c) => {
            ui.horizontal(|ui| {
                ui.label(format!("Download cache: {}", megabytes(c.bytes)));
                let clear = ui.add_enabled(c.clear.is_ok(), Button::new("Clear"));
                let clear = match &c.clear {
                    Err(why) => clear.on_disabled_hover_text(why),
                    Ok(()) => clear,
                };
                if clear.clicked() {
                    out.push(UiAction::ClearDownloadCache);
                }
            });
            ui.label(RichText::new("Content downloaded from the games you joined.").weak());
        }
        Err(why) => {
            ui.label(format!("Download cache: none ({why})"));
        }
    }
}

/// Finding and reaching games: the region, the public address, port
/// mapping, the relays, the STUN servers, the player's TURN relays and the
/// blocked hosts. Every change is one `SetFinding`.
fn online(ui: &mut Ui, state: &UiState, data: &MenuData, v: &mut View, out: &mut Vec<UiAction>) {
    let before = &state.settings().online.finding;
    let mut f = before.clone();
    section(ui, "Finding games", |ui, _| {
        ui.spacing_mut().item_spacing.y = 10.0;
        form_row(ui, "Region", |ui, label| {
            ComboBox::from_id_salt("region")
                .selected_text(f.region.label())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for r in Region::ALL {
                        ui.selectable_value(&mut f.region, r, r.label());
                    }
                })
                .response
                .labelled_by(label)
                .on_hover_text("The region your hosted games say, so players can pick a near one.");
        });
        form_row(ui, "Public address", |ui, label| {
            let text = v.address.get_or_insert_with(|| f.public_address.clone());
            let r = ui
                .add(TextEdit::singleline(text).hint_text("none").desired_width(220.0))
                .labelled_by(label)
                .on_hover_text(
                    "The address players join your hosted games at directly, when you forwarded the port \
                     yourself. Empty: the port your router opens, else joins through the relays.",
                );
            let typed = text.trim();
            let ok = typed.is_empty() || typed.parse::<std::net::IpAddr>().is_ok();
            if !ok {
                ui.label(RichText::new("not an IP address").color(egui::Color32::from_rgb(255, 110, 90)));
            } else if r.changed() {
                f.public_address = typed.to_string();
            }
        });
        // Not in a form row: its long label wraps in a narrow column
        // (ruling UI-1).
        ui.checkbox(&mut f.port_mapping, PORT_MAPPING).on_hover_text(
            "Your hosted Internet and invite games ask your router to open their port, so players join \
             directly. Applies to the next game you host.",
        );
    });
    ui.add_space(10.0);
    let relays = f.relays.clone().unwrap_or_else(|| data.default_relays.clone());
    section(ui, "Relays", |ui, _| {
        if let Some(list) = server_list(
            ui,
            "relay",
            &relays,
            &mut v.new_relay,
            (valid_relay, "needs wss:// or ws://", MAX_SERVERS),
            ("wss://relay.example.org", str::to_string),
        ) {
            f.relays = Some(list);
        }
        if ui.add_enabled(f.relays.is_some(), Button::new("Reset relays to defaults")).clicked() {
            f.relays = None;
        }
    });
    ui.add_space(10.0);
    let stun = f.stun.clone().unwrap_or_else(|| data.default_stun.clone());
    section(ui, "STUN servers", |ui, _| {
        if let Some(list) = server_list(
            ui,
            "STUN server",
            &stun,
            &mut v.new_stun,
            (valid_stun, "needs host:port", MAX_SERVERS),
            ("stun.example.org:3478", str::to_string),
        ) {
            f.stun = Some(list);
        }
        if ui.add_enabled(f.stun.is_some(), Button::new("Reset STUN servers to defaults")).clicked() {
            f.stun = None;
        }
    });
    ui.add_space(10.0);
    section(ui, "TURN relays", |ui, _| {
        ui.label(
            RichText::new(
                "Your own TURN relays, for joining games no other path reaches (the first two are used; \
                 relays announced on Nostr after them).",
            )
            .weak(),
        );
        if let Some(list) = server_list(
            ui,
            "TURN relay",
            &f.turn,
            &mut v.new_turn,
            (valid_turn, "needs user:pass@host:port", MAX_TURN),
            ("user:pass@turn.example.org:3478", turn_shown),
        ) {
            f.turn = list;
        }
    });
    ui.add_space(10.0);
    section(ui, "Blocked hosts", |ui, _| blocked(ui, &mut f));
    if f != *before {
        out.push(UiAction::SetFinding(f));
    }
}

/// The Online tab's port mapping switch.
const PORT_MAPPING: &str = "Open the game's port on my router (UPnP/NAT-PMP/PCP)";

/// Why a new server cannot be added to `list` of at most `max` (`None`: it
/// can, or nothing is typed yet); `needs` says what a valid one looks like.
fn refusal(
    list: &[String],
    typed: &str,
    (valid, needs, max): (fn(&str) -> bool, &'static str, usize),
) -> Option<&'static str> {
    if list.len() >= max {
        Some("the list is full")
    } else if typed.is_empty() {
        None
    } else if !valid(typed) {
        Some(needs)
    } else if list.iter().any(|s| s == typed) {
        Some("already listed")
    } else {
        None
    }
}

/// An editable list of servers: each with Remove (shown by `shown`: a
/// TURN relay without its password), then a field and Add (enabled for a
/// new valid one while the list has fewer than `max`; else why not beside
/// it and on Add's hover). The new list, when it changed.
fn server_list(
    ui: &mut Ui,
    what: &str,
    list: &[String],
    new: &mut String,
    rule: (fn(&str) -> bool, &'static str, usize),
    (hint, shown): (&str, fn(&str) -> String),
) -> Option<Vec<String>> {
    let mut changed = None;
    if list.is_empty() {
        ui.label(RichText::new(format!("No {what}s: none is asked.")).weak());
    }
    for (i, s) in list.iter().enumerate() {
        let s = shown(s);
        ui.horizontal(|ui| {
            if ui.small_button("Remove").on_hover_text(format!("Remove {s}")).clicked() {
                let mut l = list.to_vec();
                l.remove(i);
                changed = Some(l);
            }
            ui.label(RichText::new(s).monospace());
        });
    }
    ui.horizontal(|ui| {
        let label = ui.label(format!("New {what}"));
        let r = ui.add(TextEdit::singleline(new).hint_text(hint).desired_width(300.0)).labelled_by(label.id);
        let typed = new.trim().to_string();
        let why = refusal(list, &typed, rule);
        let ok = why.is_none() && !typed.is_empty();
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let add = ui.add_enabled(ok, Button::new(format!("Add {what}")));
        let add = match why {
            Some(why) => add.on_disabled_hover_text(why),
            None => add,
        };
        if let Some(why) = why {
            ui.label(RichText::new(why).color(egui::Color32::from_rgb(255, 110, 90)));
        }
        if (add.clicked() || enter) && ok {
            let mut l = list.to_vec();
            l.push(typed);
            changed = Some(l);
            new.clear();
        }
    });
    changed
}

/// The hosts whose games are hidden, each with Unblock.
fn blocked(ui: &mut Ui, f: &mut FindingSettings) {
    if f.blocked.is_empty() {
        ui.label(RichText::new("None. The Internet tab's \"Block this host\" adds one.").weak());
    }
    let mut unblock = None;
    for (i, k) in f.blocked.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.small_button("Unblock").on_hover_text(k).clicked() {
                unblock = Some(i);
            }
            ui.label(RichText::new(format!("{}…", k.get(..16).unwrap_or(k))).monospace());
        });
    }
    if let Some(i) = unblock {
        f.blocked.remove(i);
    }
}
