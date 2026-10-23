//! The open material's fields (issue #44): its name and file, Edit a copy
//! for a built-in one (whose fields show but never change), the notes,
//! the heat's numbers (each typed or slid, within its bounds, the bounds
//! beside it), the light and the colour ramp: the engine's or its own
//! keys (temperature, colour, glow; added and removed), drawn as a strip.
//! Issue #57: the light soak, the level's or its own 0 to 12 px.

use egui::{RichText, Ui};
use olr_format::materials::MaterialSource;
use olr_look::{
    MAX_CAPACITY, MAX_CONDUCTIVITY, MAX_COOLING, MAX_GLOW, MAX_RAMP_KEYS, MAX_SENSITIVITY, MAX_SOAK,
    MAX_TEMPERATURE, MIN_CAPACITY, MIN_COOLING,
};

use super::{MaterialEditorData, caption, labels, relabel, strip};
use crate::state::{Edit, MaterialAction, MaterialEditorState, ramp_of};

/// A number's name, value, bounds, help and edit.
type NumberRow<'a> = (&'a str, u16, (u16, u16), &'a str, fn(i64) -> Edit);

struct Cx<'a> {
    data: &'a mut MaterialEditorData,
    /// The fields' changes, applied before the frame's clicks.
    commits: &'a mut Vec<MaterialAction>,
}

pub(super) fn show(
    ui: &mut Ui,
    state: &MaterialEditorState,
    data: &mut MaterialEditorData,
    out: &mut Vec<MaterialAction>,
    commits: &mut Vec<MaterialAction>,
) {
    let Some(m) = state.doc() else {
        ui.add_space(20.0);
        ui.weak("Choose a material on the left, or make a New one.");
        return;
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.heading(format!("{} ({})", m.name, m.id));
        if state.read_only() {
            ui.weak("built in, read-only");
            if ui.button(labels::EDIT_A_COPY).clicked() {
                out.push(MaterialAction::Copy);
            }
        }
    });
    for p in state.problems() {
        ui.colored_label(ui.visuals().error_fg_color, p);
    }
    ui.add_space(6.0);
    let mut cx = Cx { data, commits };
    ui.add_enabled_ui(!state.read_only(), |ui| body(ui, &mut cx, m));
}

fn body(ui: &mut Ui, cx: &mut Cx, m: &MaterialSource) {
    caption(ui, labels::NOTES);
    let mut notes = cx.data.drafts.get(labels::NOTES).cloned().unwrap_or_else(|| m.notes.clone());
    let r = ui.add(egui::TextEdit::multiline(&mut notes).desired_rows(2).desired_width(f32::INFINITY));
    relabel(&r, egui::WidgetType::TextEdit, labels::NOTES);
    let r = r.on_hover_text("Free text for authors; the game ignores it");
    if r.changed() {
        cx.data.drafts.insert(labels::NOTES.into(), notes.clone());
    }
    if r.lost_focus()
        && let Some(t) = cx.data.drafts.remove(labels::NOTES)
        && t != m.notes
    {
        cx.commits.push(MaterialAction::Set(Edit::Notes(t)));
    }
    ui.add_space(8.0);
    light_soak(ui, cx, m.light_soak);
    ui.add_space(8.0);
    let mut heats = m.heat.is_some();
    let r = ui.checkbox(&mut heats, labels::HEATS).on_hover_text(
        "Off: it never heats. On: explosions leave it hot by the values below (stone's to start from)",
    );
    if r.changed() {
        cx.commits.push(MaterialAction::Set(Edit::Heats(heats)));
    }
    let Some(h) = &m.heat else { return };
    ui.add_space(4.0);
    ui.label(RichText::new("Heat: percent of stone's (100)").weak());
    egui::Grid::new("olr-material-editor-heat").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
        let rows: [NumberRow; 4] = [
            (
                labels::SENSITIVITY,
                h.sensitivity.into(),
                (0, MAX_SENSITIVITY.into()),
                "How much of the deposited heat a pixel takes (0: it never heats)",
                Edit::Sensitivity,
            ),
            (
                labels::CAPACITY,
                h.capacity,
                (MIN_CAPACITY, MAX_CAPACITY),
                "How much heat it takes to warm a pixel: higher warms less from the same blast and holds its heat longer",
                Edit::Capacity,
            ),
            (
                labels::CONDUCTIVITY,
                h.conductivity,
                (0, MAX_CONDUCTIVITY),
                "How fast heat spreads to neighbouring pixels that conduct, and so how deep it soaks in (0: none)",
                Edit::Conductivity,
            ),
            (
                labels::COOLING,
                h.cooling,
                (MIN_COOLING, MAX_COOLING),
                "How fast a pixel beside air loses its heat to it",
                Edit::Cooling,
            ),
        ];
        for (name, value, bounds, help, edit) in rows {
            number(ui, cx, name, value.into(), bounds, help, edit);
            ui.end_row();
        }
    });
    ui.add_space(10.0);
    ui.strong("How it looks hot");
    egui::Grid::new("olr-material-editor-look").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
        number(
            ui,
            cx,
            labels::LIGHT,
            h.look.light.into(),
            (0, MAX_GLOW.into()),
            "Percent of its glow cast as light into the level (0: it shows hot but lights nothing)",
            Edit::Light,
        );
        ui.end_row();
    });
    ui.add_space(6.0);
    let mut own = !h.look.ramp.is_empty();
    let r = ui.checkbox(&mut own, labels::OWN_RAMP).on_hover_text(
        "Off: the engine's lava ramp (dull red, orange, yellow-white). On: its own keys, starting from the engine's",
    );
    if r.changed() {
        cx.commits.push(MaterialAction::Set(Edit::OwnRamp(own)));
    }
    let width = ui.available_width().clamp(200.0, 640.0);
    strip(ui, &ramp_of(h), egui::vec2(width, 36.0));
    ui.horizontal(|ui| {
        ui.weak("0: cold");
        ui.add_space((width - 170.0).max(0.0));
        ui.weak(format!("{MAX_TEMPERATURE}: hottest"));
    });
    if own {
        ui.add_space(4.0);
        ui.weak("Below the first key a pixel shows its art; between keys colour and glow blend.");
        egui::Grid::new("olr-material-editor-ramp").num_columns(5).spacing([10.0, 6.0]).show(ui, |ui| {
            let keys = &h.look.ramp;
            for (i, k) in keys.iter().enumerate() {
                ui.label(format!("Key {}", i + 1));
                let lo = if i == 0 { 1 } else { keys[i - 1].temperature + 1 };
                let hi = keys.get(i + 1).map_or(MAX_TEMPERATURE, |n| n.temperature - 1);
                ui.horizontal(|ui| {
                    field(
                        ui,
                        cx,
                        &labels::key_temperature(i),
                        k.temperature.into(),
                        (lo, hi.max(lo)),
                        move |v| Edit::KeyTemperature(i, v),
                    );
                });
                ui.horizontal(|ui| colour(ui, cx, i, [k.colour.0.r, k.colour.0.g, k.colour.0.b]));
                ui.horizontal(|ui| {
                    field(ui, cx, &labels::key_glow(i), k.glow.into(), (0, MAX_GLOW.into()), move |v| {
                        Edit::KeyGlow(i, v)
                    });
                });
                let r = ui.small_button("✖");
                relabel(&r, egui::WidgetType::Button, &labels::remove_key(i));
                if r.on_hover_text("Remove this key").clicked() {
                    cx.commits.push(MaterialAction::Set(Edit::RemoveKey(i)));
                }
                ui.end_row();
            }
        });
    }
    ui.add_space(4.0);
    let room = h.look.ramp.len() < MAX_RAMP_KEYS;
    let r = ui.add_enabled(room, egui::Button::new(labels::ADD_KEY)).on_hover_text(format!(
        "A key in the widest gap, coloured as the ramp is there (at most {MAX_RAMP_KEYS})"
    ));
    if r.clicked() {
        cx.commits.push(MaterialAction::Set(Edit::AddKey));
    }
}

/// The light soak (issue #57): the level's, or its own world pixels.
fn light_soak(ui: &mut Ui, cx: &mut Cx, soak: Option<u8>) {
    let shown = soak.map_or(labels::LIGHT_SOAK_LEVEL.to_string(), labels::soak_px);
    ui.horizontal(|ui| {
        let r = egui::ComboBox::from_label(labels::LIGHT_SOAK).selected_text(shown).show_ui(ui, |ui| {
            let mut pick = |ui: &mut Ui, v: Option<u8>, text: &str| {
                if ui.selectable_label(soak == v, text).clicked() && soak != v {
                    cx.commits.push(MaterialAction::Set(Edit::LightSoak(v.map(i64::from))));
                }
            };
            pick(ui, None, labels::LIGHT_SOAK_LEVEL);
            for px in 0..=MAX_SOAK {
                pick(ui, Some(px), &labels::soak_px(px));
            }
        });
        r.response.on_hover_text(
            "How deep light seeps into its rock and dirt from the air beside them: thin for concrete \
             and stone, deeper for furniture. Level's: the level's own light soak",
        );
    });
}

/// A number's row of the grid: its name, its field and slider, its bounds.
fn number(
    ui: &mut Ui,
    cx: &mut Cx,
    name: &str,
    value: i64,
    (lo, hi): (u16, u16),
    help: &str,
    edit: fn(i64) -> Edit,
) {
    caption(ui, name).on_hover_text(help);
    ui.horizontal(|ui| field(ui, cx, name, value, (lo, hi), edit)).response.on_hover_text(help);
    slider(ui, cx, name, value, (lo, hi), edit);
    ui.weak(format!("{lo} to {hi}"));
}

/// A number typed, committed when it loses the keyboard (Enter, Tab, a
/// click elsewhere); the state clamps it to its bounds.
fn field(ui: &mut Ui, cx: &mut Cx, name: &str, value: i64, (lo, hi): (u16, u16), edit: impl Fn(i64) -> Edit) {
    let shown = value.to_string();
    let mut t = cx.data.drafts.get(name).cloned().unwrap_or_else(|| shown.clone());
    let r = ui.add(
        egui::TextEdit::singleline(&mut t)
            .id(egui::Id::new(("olr-material-editor", name)))
            .desired_width(56.0),
    );
    relabel(&r, egui::WidgetType::TextEdit, name);
    let r = r.on_hover_text(format!("{lo} to {hi}"));
    if r.changed() {
        cx.data.drafts.insert(name.to_string(), t);
    }
    if r.lost_focus()
        && let Some(draft) = cx.data.drafts.remove(name)
        && let Ok(v) = draft.trim().parse::<f64>()
        && v.is_finite()
    {
        let v = v.round().clamp(i64::MIN as f64, i64::MAX as f64) as i64;
        if v != value {
            cx.commits.push(MaterialAction::Set(edit(v)));
        }
    }
}

/// A slider over the bounds, committed when the drag stops.
fn slider(ui: &mut Ui, cx: &mut Cx, name: &str, value: i64, (lo, hi): (u16, u16), edit: fn(i64) -> Edit) {
    let mut v = match &cx.data.slide {
        Some((n, v)) if n == name => *v,
        _ => value as f64,
    };
    let s = egui::Slider::new(&mut v, f64::from(lo)..=f64::from(hi)).show_value(false).integer();
    let r = ui.add(s);
    relabel(&r, egui::WidgetType::Slider, &labels::slider(name));
    if r.changed() {
        cx.data.slide = Some((name.to_string(), v));
    }
    if r.drag_stopped() || (r.changed() && !r.dragged()) {
        cx.data.slide = None;
        let v = v.round() as i64;
        if v != value {
            cx.commits.push(MaterialAction::Set(edit(v)));
        }
    }
}

/// Key `i`'s colour: a picker and its `#rrggbb` field.
fn colour(ui: &mut Ui, cx: &mut Cx, i: usize, rgb: [u8; 3]) {
    let mut c = rgb;
    let r = egui::color_picker::color_edit_button_srgb(ui, &mut c);
    relabel(&r, egui::WidgetType::ColorButton, &labels::key_colour_picker(i));
    if r.changed() && c != rgb {
        cx.commits.push(MaterialAction::Set(Edit::KeyColour(i, c)));
    }
    let name = labels::key_colour(i);
    let shown = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
    let mut t = cx.data.drafts.get(&name).cloned().unwrap_or_else(|| shown.clone());
    let r = ui.add(
        egui::TextEdit::singleline(&mut t)
            .id(egui::Id::new(("olr-material-editor", &name)))
            .desired_width(70.0),
    );
    relabel(&r, egui::WidgetType::TextEdit, &name);
    if r.changed() {
        cx.data.drafts.insert(name.clone(), t);
    }
    if r.lost_focus()
        && let Some(draft) = cx.data.drafts.remove(&name)
        && let Some(c) = parse_hex(&draft)
        && c != rgb
    {
        cx.commits.push(MaterialAction::Set(Edit::KeyColour(i, c)));
    }
}

/// A `#rrggbb` (or `rrggbb`) colour.
fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([v(0)?, v(2)?, v(4)?])
}
