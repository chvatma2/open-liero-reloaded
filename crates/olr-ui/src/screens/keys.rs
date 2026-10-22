//! The options' keys (OLX's controls tab,
//! `src/client/DeprecatedGUI/Menu_Options.cpp`: a keyboard and a gamepad
//! column per control, the "Input for:" box, Reset), seat by seat (issue
//! #28: Keyboard 1, Keyboard 2 on the same keyboard, the Gamepads, every
//! pad's seat sharing them): every action's inputs of the seat's devices,
//! each removable unless it is the action's last the seat needs (a
//! keyboard seat keys, the gamepads pad inputs), "+" to capture another,
//! and an input another action has resolved explicitly: moved (unless
//! that leaves the other action without one) or shared (plan 7 Review
//! Focus 2). Browser shortcuts the keys make are pointed out (spec §8.5),
//! and the keys both keyboards bind (one press would be both players').
//! Keyboard 1's page has the switch of mouse controls (issue #24,
//! experimental; no other seat has them). While it is on (issue #25) its
//! columns are mouse controls' own: their keyboard keys (the keyboard's
//! own come back untouched when it is off) and a Mouse column (buttons and
//! the wheel); Up, Down, Select weapon and Strafe are the mouse's, and
//! every other action keeps a key or a mouse input. Reset restores the
//! shown page's defaults.
//!
//! The capture itself is [`MenuData::capture`](super::MenuData::capture):
//! the app feeds it the window's key presses, mouse buttons and wheel and
//! the gamepads' events (egui hears no modifier keys alone, and no
//! gamepad); while it captures a mouse input egui hears no click.

use egui::{Button, Color32, Grid, Id, Key, Modal, RichText, Ui, WidgetInfo, WidgetType};
use olr_input::{
    Action, Binding, Bindings, Capture, Device, KeyCode, MOUSE_BUTTONS, MOUSE_KEYS, Mode, PAD_PAUSE,
    RebindError, Resolve, SeatKind, key_name,
};

pub use crate::settings::GAME_KEYS;
use crate::settings::PAD_PAUSE_DOES;
use crate::state::{UiAction, UiState};

/// The switch of mouse controls (issue #24).
pub const MOUSE_CONTROLS: &str = "Mouse controls (experimental)";

/// Its note: what it does, and that it may change.
pub const MOUSE_NOTE: &str = "The worm aims at the cursor and faces it; by default A and D move, Space jumps, \
     S digs, left click shoots, right click throws the rope, the wheel changes weapon. Experimental: it may \
     change.";

/// What the keys show while mouse controls are on (issue #25).
pub const MOUSE_KEYS_NOTE: &str =
    "Mouse controls' own keys and mouse inputs: the keyboard's come back when they are off.";

/// A Keyboard or Mouse cell of an action mouse controls press themselves.
pub const MOUSE_MADE: &str = "the mouse's";

/// Keyboard 1's Reset, mouse controls off: OLX's keys.
pub const RESET_KEYS: &str = "Reset to OLX's keys";

/// Keyboard 1's Reset, mouse controls on: their keys and mouse inputs.
pub const RESET_MOUSE: &str = "Reset mouse controls' keys";

/// Keyboard 2's Reset: OLX's player 2's keys.
pub const RESET_KEYS2: &str = "Reset to OLX's player 2 keys";

/// The gamepads' Reset.
pub const RESET_PAD: &str = "Reset to OLX's pad inputs";

/// What each seat's page says of it (issue #28).
pub fn seat_note(seat: SeatKind) -> &'static str {
    match seat {
        SeatKind::Keyboard1 => "The first player on the keyboard, and the mouse.",
        SeatKind::Keyboard2 => "A second player on the same keyboard (OLX's player 2): every profile's.",
        SeatKind::Gamepad => "Every gamepad plays with these; Start pauses.",
    }
}

/// The section's own state.
#[derive(Clone, Debug, Default)]
pub(super) struct View {
    /// The seat shown (issue #28).
    seat: SeatKind,
    /// The action the open capture is for.
    action: Option<Action>,
    /// The open capture is for the online chat key.
    chat: bool,
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
    let before = state.bindings();
    let mut b = before.clone();
    let mouse_on = state.settings().mouse.enabled;
    let seat = v.seat;
    let mode = seat.mode(mouse_on);
    // The capture the app fed: the input, or nothing yet.
    if capture.is_some() != (v.action.is_some() || v.chat) || capture.as_ref().is_some_and(Capture::cancelled)
    {
        *capture = None;
        v.action = None;
        v.chat = false;
    }
    if let (Some(c), true) = (capture.as_ref(), v.chat)
        && let Some(input) = c.result()
    {
        *capture = None;
        v.chat = false;
        take_chat(v, input, out);
    } else if let (Some(c), Some(action)) = (capture.as_ref(), v.action)
        && let Some(input) = c.result()
    {
        *capture = None;
        v.action = None;
        take(&mut b, mode, v, action, input);
    }

    ui.horizontal(|ui| {
        ui.label("Controls of");
        for s in SeatKind::ALL {
            if ui.selectable_label(s == seat, s.name()).clicked() && s != seat {
                v.seat = s;
                v.note = None;
            }
        }
    });
    ui.label(RichText::new(seat_note(seat)).weak());
    ui.add_space(6.0);

    if seat == SeatKind::Keyboard1 {
        let mut mouse = mouse_on;
        if ui.checkbox(&mut mouse, MOUSE_CONTROLS).changed() {
            out.push(UiAction::SetMouseControls(mouse));
        }
        ui.label(RichText::new(MOUSE_NOTE).weak());
        ui.add_space(6.0);
        if mode == Mode::Mouse {
            ui.label(RichText::new(MOUSE_KEYS_NOTE).weak());
        }
    }
    // Keyboard 2's are the options' whatever the profile.
    let own = state.profiles().selected().filter(|p| p.bindings.is_some() && seat != SeatKind::Keyboard2);
    let whose = match own {
        Some(p) => format!("{} has its own keys: these are theirs.", p.name),
        None => "The keys of every profile without its own.".to_string(),
    };
    if seat != SeatKind::Keyboard2 {
        ui.label(RichText::new(whose).weak());
    }
    ui.add_space(6.0);
    let devices = seat.devices(mouse_on);
    Grid::new("keys").num_columns(1 + devices.len()).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
        ui.label("");
        for d in devices {
            let name = match d {
                Device::Keyboard => "Keyboard",
                Device::Mouse => "Mouse",
                Device::Gamepad => "Gamepad",
            };
            ui.label(RichText::new(name).strong());
        }
        ui.end_row();
        for a in Action::ALL {
            ui.label(a.name());
            for &d in devices {
                ui.horizontal(|ui| row(ui, &mut b, mode, a, d, capture, v));
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    if seat == SeatKind::Keyboard1 {
        ui.horizontal(|ui| {
            ui.label("Chat (online matches)");
            let key = key_name(state.settings().online.chat_key);
            let r = ui.add(Button::new(key).small());
            let label = "Change chat key";
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
            if r.on_hover_text(label).clicked() {
                *capture = Some(Capture::new(Device::Keyboard));
                v.chat = true;
                v.action = None;
                v.note = None;
            }
        });
    }
    if let Some(n) = &v.note {
        ui.label(RichText::new(n).color(Color32::from_rgb(255, 110, 90)));
    }
    if seat != SeatKind::Gamepad {
        for keys in b.browser_conflicts() {
            let names: Vec<String> = keys.iter().map(|k| key_name(*k)).collect();
            let text = format!("{}: a browser keeps this shortcut for itself.", names.join(" + "));
            ui.label(RichText::new(text).color(Color32::from_rgb(255, 190, 90)));
        }
        for k in b.shared_keys(mouse_on) {
            ui.label(RichText::new(shared_note(k)).color(Color32::from_rgb(255, 190, 90)));
        }
    }
    ui.add_space(8.0);
    // Each page's Reset leaves the others' bindings alone.
    let reset = match (seat, mode) {
        (SeatKind::Keyboard1, Mode::Mouse) => RESET_MOUSE,
        (SeatKind::Keyboard1, _) => RESET_KEYS,
        (SeatKind::Keyboard2, _) => RESET_KEYS2,
        (SeatKind::Gamepad, _) => RESET_PAD,
    };
    if ui.button(reset).clicked() {
        let olx = Bindings::olx();
        match reset {
            RESET_MOUSE => (b.mouse_keys, b.mouse) = (MOUSE_KEYS.to_vec(), MOUSE_BUTTONS.to_vec()),
            RESET_KEYS2 => b.keys2 = olx.keys2,
            RESET_PAD => b.pad = olx.pad,
            _ => b.keys = olx.keys,
        }
        v.note = None;
    }

    capture_dialog(ui, capture, v);
    conflict_dialog(ui, &mut b, mode, v);

    if b != before {
        match own {
            Some(p) => {
                let mut list = state.profiles().clone();
                let keys2 = p.bindings.as_ref().map_or_else(Vec::new, |o| o.keys2.clone());
                list.list[list.selected].bindings = Some(Bindings { keys2, ..b });
                out.push(UiAction::SetProfiles(list));
            }
            None => {
                // Keyboard 2's page of a profile with its own keys: the
                // options' keyboard 2 alone changes.
                let keys1 = state.settings().input.clone();
                let b = if seat == SeatKind::Keyboard2 { Bindings { keys2: b.keys2, ..keys1 } } else { b };
                out.push(UiAction::SetBindings(b));
            }
        }
    }
}

/// What the options say of a key both keyboard seats bind (issue #28).
pub fn shared_note(k: KeyCode) -> String {
    format!("{}: both keyboards have it, so one press is both players'.", key_name(k))
}

/// Action `a`'s inputs of device `d` in `mode`, each removable but the
/// last it needs, and "+"; [`MOUSE_MADE`] for an action mouse controls
/// press themselves.
fn row(
    ui: &mut Ui,
    b: &mut Bindings,
    mode: Mode,
    a: Action,
    d: Device,
    capture: &mut Option<Capture>,
    v: &mut View,
) {
    if !mode.binds(a, d) {
        ui.label(RichText::new(MOUSE_MADE).weak()).on_hover_text("Mouse controls press it themselves.");
        return;
    }
    for input in b.of(mode, a, d) {
        let refused = b.clone().unbind(mode, a, input).err();
        let can = refused.is_none();
        let r = ui.add_enabled(can, Button::new(format!("{input} ×")).small());
        let label = format!("Remove {input} from {}", a.name());
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, can, &label));
        let why = match refused {
            Some(RebindError::Unreachable { need, .. }) => {
                format!("{}'s last {}: add another first.", a.name(), need.noun())
            }
            Some(e) => format!("{e}."),
            None => String::new(),
        };
        let r = r.on_disabled_hover_text(why);
        if r.clicked() {
            // Refused only when not clickable.
            let _ = b.unbind(mode, a, input);
            v.note = None;
        }
    }
    let label = match d {
        Device::Keyboard => format!("Add key for {}", a.name()),
        Device::Gamepad => format!("Add pad input for {}", a.name()),
        Device::Mouse => format!("Add mouse input for {}", a.name()),
    };
    let r = ui.add(Button::new("+").small());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    if r.on_hover_text(&label).clicked() {
        *capture = Some(Capture::new(d));
        v.action = Some(a);
        v.note = None;
    }
}

/// What a captured `input` does for `action` in `mode`: bound when free,
/// a choice when another action has it, refused when the game keeps it
/// (a pad's Start pauses).
fn take(b: &mut Bindings, mode: Mode, v: &mut View, action: Action, input: Binding) {
    if input == Binding::Pad(PAD_PAUSE) {
        v.note = Some(format!("{input} is the game's ({PAD_PAUSE_DOES}): choose another input."));
        return;
    }
    if let Binding::Key(k) = input
        && let Some((_, what)) = GAME_KEYS.iter().find(|(g, _)| *g == k)
    {
        if k != KeyCode::Escape {
            v.note = Some(format!("{} is the game's ({what}): choose another key.", key_name(k)));
        }
        return;
    }
    let owners = b.actions_of(mode, input);
    if owners.contains(&action) {
        v.note = Some(format!("{input} is already {}'s.", action.name()));
    } else if owners.is_empty() {
        if let Err(e) = b.bind(mode, action, input, Resolve::Share) {
            v.note = Some(format!("{e}."));
        }
    } else {
        v.conflict = Some((action, input));
    }
}

/// A captured input for the chat key: any key but the game's.
fn take_chat(v: &mut View, input: Binding, out: &mut Vec<UiAction>) {
    let Binding::Key(k) = input else { return };
    if let Some((_, what)) = GAME_KEYS.iter().find(|(g, _)| *g == k) {
        if k != KeyCode::Escape {
            v.note = Some(format!("{} is the game's ({what}): choose another key.", key_name(k)));
        }
        return;
    }
    out.push(UiAction::SetChatKey(k));
}

/// OLX's "Input for:" box, until the app captured an input (Esc cancels).
fn capture_dialog(ui: &mut Ui, capture: &mut Option<Capture>, v: &mut View) {
    let Some(c) = capture.as_ref() else { return };
    let name = match (v.action, v.chat) {
        (Some(a), _) => a.name(),
        (None, true) => "Chat",
        (None, false) => return,
    };
    let mouse = c.device() == Device::Mouse;
    let ask = match c.device() {
        Device::Keyboard => format!("Press a key for {name}"),
        Device::Gamepad => format!("Press a gamepad button or push a stick for {name}"),
        Device::Mouse => format!("Click a mouse button or turn the wheel for {name}"),
    };
    let r = Modal::new(Id::new("options capture")).show(ui.ctx(), |ui| {
        ui.set_min_width(320.0);
        ui.vertical_centered(|ui| {
            ui.heading(ask);
            ui.label(RichText::new("(Escape to cancel)").weak());
            // A click is the mouse's capture: no Cancel button to click.
            if mouse {
                return false;
            }
            ui.add_space(8.0);
            ui.button("Cancel").clicked()
        })
        .inner
    });
    let esc = super::pressed_once(ui, Key::Escape);
    if r.inner || (r.should_close() && !mouse) || esc {
        *capture = None;
        v.action = None;
        v.chat = false;
    }
}

/// A captured input other actions have: moved here (unless one of them
/// would have none of its device left), shared, or nothing.
fn conflict_dialog(ui: &mut Ui, b: &mut Bindings, mode: Mode, v: &mut View) {
    let Some((action, input)) = v.conflict else { return };
    let owners: Vec<String> = b.actions_of(mode, input).iter().map(|a| format!("{}'s", a.name())).collect();
    let refused = b.clone().bind(mode, action, input, Resolve::Move).err();
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
        let _ = b.bind(mode, action, input, how);
        v.conflict = None;
    } else if r.inner || r.should_close() || esc {
        v.conflict = None;
    }
}
