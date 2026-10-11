//! The options' keys (OLX's controls tab,
//! `src/client/DeprecatedGUI/Menu_Options.cpp`: a keyboard and a gamepad
//! column per control, the "Input for:" box, Reset): every action's keys
//! and gamepad inputs, each removable unless it is the action's last of its
//! device, "+" to capture another, and a key another action has resolved
//! explicitly: moved (unless that leaves the other action without one) or
//! shared (plan 7 Review Focus 2). Browser shortcuts the keys make are
//! pointed out (spec §8.5).
//!
//! The capture itself is [`MenuData::capture`](super::MenuData::capture):
//! the app feeds it the window's key presses and the gamepads' events
//! (egui hears no modifier keys alone, and no gamepad).

use egui::{Button, Color32, Grid, Id, Key, Modal, RichText, Ui, WidgetInfo, WidgetType};
use rl_input::{Action, Binding, Bindings, Capture, Device, KeyCode, Resolve, key_name};

pub use crate::settings::GAME_KEYS;
use crate::state::{UiAction, UiState};

/// The section's own state.
#[derive(Clone, Debug, Default)]
pub(super) struct View {
    /// The action the open capture is for.
    action: Option<Action>,
    /// A captured input other actions have, waiting for the player's choice.
    conflict: Option<(Action, Binding)>,
    /// Why the last captured key was not bound.
    note: Option<String>,
}

impl View {
    /// A dialog is open: Esc closes it rather than leaving the options.
    pub(super) fn busy(&self, capturing: bool) -> bool {
        capturing || self.conflict.is_some()
    }
}

pub(super) fn show(
    ui: &mut Ui,
    state: &UiState,
    capture: &mut Option<Capture>,
    v: &mut View,
    out: &mut Vec<UiAction>,
) {
    let before = state.bindings().clone();
    let mut b = before.clone();
    // The capture the app fed: the input, or nothing yet.
    if capture.is_some() != v.action.is_some() {
        *capture = None;
        v.action = None;
    }
    if let (Some(c), Some(action)) = (capture.as_ref(), v.action)
        && let Some(input) = c.result()
    {
        *capture = None;
        v.action = None;
        take(&mut b, v, action, input);
    }

    let own = state.profiles().selected().filter(|p| p.bindings.is_some());
    let whose = match own {
        Some(p) => format!("{} has its own keys: these are theirs.", p.name),
        None => "The keys of every profile without its own.".to_string(),
    };
    ui.label(RichText::new(whose).weak());
    ui.add_space(6.0);
    Grid::new("keys").num_columns(3).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
        ui.label("");
        ui.label(RichText::new("Keyboard").strong());
        ui.label(RichText::new("Gamepad").strong());
        ui.end_row();
        for a in Action::ALL {
            ui.label(a.name());
            for d in Device::ALL {
                ui.horizontal(|ui| row(ui, &mut b, a, d, capture, v));
            }
            ui.end_row();
        }
    });
    if let Some(n) = &v.note {
        ui.label(RichText::new(n).color(Color32::from_rgb(255, 110, 90)));
    }
    for keys in b.browser_conflicts() {
        let names: Vec<String> = keys.iter().map(|k| key_name(*k)).collect();
        let text = format!("{}: a browser keeps this shortcut for itself.", names.join(" + "));
        ui.label(RichText::new(text).color(Color32::from_rgb(255, 190, 90)));
    }
    ui.add_space(8.0);
    if ui.button("Reset to OLX's keys").clicked() {
        b = Bindings::olx();
        v.note = None;
    }

    capture_dialog(ui, capture, v);
    conflict_dialog(ui, &mut b, v);

    if b != before {
        match own {
            Some(_) => {
                let mut p = state.profiles().clone();
                p.list[p.selected].bindings = Some(b);
                out.push(UiAction::SetProfiles(p));
            }
            None => out.push(UiAction::SetBindings(b)),
        }
    }
}

/// Action `a`'s inputs of device `d`, each removable but the last, and "+".
fn row(ui: &mut Ui, b: &mut Bindings, a: Action, d: Device, capture: &mut Option<Capture>, v: &mut View) {
    let inputs = b.of(a, d);
    let last = inputs.len() == 1;
    for input in inputs {
        let r = ui.add_enabled(!last, Button::new(format!("{input} ×")).small());
        let label = format!("Remove {input} from {}", a.name());
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, !last, &label));
        let r =
            r.on_disabled_hover_text(format!("{}'s last {}: add another first.", a.name(), d.input_noun()));
        if r.clicked() {
            // Refused only for the last, which is not clickable.
            let _ = b.unbind(a, input);
            v.note = None;
        }
    }
    let label = match d {
        Device::Keyboard => format!("Add key for {}", a.name()),
        Device::Gamepad => format!("Add pad input for {}", a.name()),
    };
    let r = ui.add(Button::new("+").small());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    if r.on_hover_text(&label).clicked() {
        *capture = Some(Capture::new(d));
        v.action = Some(a);
        v.note = None;
    }
}

/// What a captured `input` does for `action`: bound when free, a choice
/// when another action has it, refused when the game keeps it.
fn take(b: &mut Bindings, v: &mut View, action: Action, input: Binding) {
    if let Binding::Key(k) = input
        && let Some((_, what)) = GAME_KEYS.iter().find(|(g, _)| *g == k)
    {
        if k != KeyCode::Escape {
            v.note = Some(format!("{} is the game's ({what}): choose another key.", key_name(k)));
        }
        return;
    }
    let owners = b.actions_of(input);
    if owners.contains(&action) {
        v.note = Some(format!("{input} is already {}'s.", action.name()));
    } else if owners.is_empty() {
        let _ = b.bind(action, input, Resolve::Share);
    } else {
        v.conflict = Some((action, input));
    }
}

/// OLX's "Input for:" box, until the app captured an input (Esc cancels).
fn capture_dialog(ui: &mut Ui, capture: &mut Option<Capture>, v: &mut View) {
    let (Some(c), Some(action)) = (capture.as_ref(), v.action) else { return };
    let ask = match c.device() {
        Device::Keyboard => format!("Press a key for {}", action.name()),
        Device::Gamepad => format!("Press a gamepad button or push a stick for {}", action.name()),
    };
    let r = Modal::new(Id::new("options capture")).show(ui.ctx(), |ui| {
        ui.set_min_width(320.0);
        ui.vertical_centered(|ui| {
            ui.heading(ask);
            ui.label(RichText::new("(Escape to cancel)").weak());
            ui.add_space(8.0);
            ui.button("Cancel").clicked()
        })
        .inner
    });
    let esc = super::pressed_once(ui, Key::Escape);
    if r.inner || r.should_close() || esc {
        *capture = None;
        v.action = None;
    }
}

/// A captured input other actions have: moved here (unless one of them
/// would have none of its device left), shared, or nothing.
fn conflict_dialog(ui: &mut Ui, b: &mut Bindings, v: &mut View) {
    let Some((action, input)) = v.conflict else { return };
    let owners: Vec<String> = b.actions_of(input).iter().map(|a| format!("{}'s", a.name())).collect();
    let refused = b.clone().bind(action, input, Resolve::Move).err();
    let mut choice = None;
    let r = Modal::new(Id::new("options conflict")).show(ui.ctx(), |ui| {
        ui.set_min_width(320.0);
        ui.vertical_centered(|ui| {
            ui.heading(format!("{input} is {}.", owners.join(" and ")));
            ui.add_space(8.0);
            let move_to =
                ui.add_enabled(refused.is_none(), Button::new(format!("Move it to {}", action.name())));
            if move_to.clicked() {
                choice = Some(Resolve::Move);
            }
            if let Some(e) = refused {
                ui.label(RichText::new(format!("{e}.")).weak());
            }
            if ui.button("Use it for both").clicked() {
                choice = Some(Resolve::Share);
            }
            ui.button("Cancel").clicked()
        })
        .inner
    });
    let esc = super::pressed_once(ui, Key::Escape);
    if let Some(how) = choice {
        // A refused move changes nothing (its button is disabled anyway).
        let _ = b.bind(action, input, how);
        v.conflict = None;
    } else if r.inner || r.should_close() || esc {
        v.conflict = None;
    }
}
