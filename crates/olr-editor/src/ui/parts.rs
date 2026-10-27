//! The asset tab's right column under the states (issue #80; the asset-based
//! levels design's "The asset tab", calls 10, 11, 19 and 21): the tabs
//! Layers | Properties | Sounds | Script, and the middle's script editor
//! while Script is chosen.
//!
//! - **Properties**: the layer tree unrolled, the Whole asset row first,
//!   each row exactly one of `olr_edit::asset::offers`, its value in the
//!   shown state (posed: the state's own value, marked, else the exposed
//!   default, else the layer's own), Clear, Expose, an exposed entry's
//!   Name, Default, Min and Max, "Also changes" under a group property,
//!   and Destructible with its health.
//! - **Sounds**: the asset's sounds (name, length, ▶, Remove), Import
//!   sound…, the shown state's loop and enter sounds.
//! - **Script**: New / Remove script, its size, its syntax errors
//!   (`olr_script::check` of the draft, checked as it changes; a click puts
//!   the cursor there). The editor's text is one draft committed as one
//!   step when it loses the keyboard or after a pause of [`SCRIPT_PAUSE`]
//!   seconds of UI time.

use egui::Ui;
use olr_edit::asset::{ExposedChange, Offer};
use olr_format::assets::{
    HealthSource, MAX_HEALTH, MAX_SCRIPT_BYTES, PropertySource, ValueKind, ValueSource,
};
use olr_format::levels::{ElementKind, ElementMaterial};
use olr_format::look::AnimationSource;
use olr_script::ScriptIssue;

use super::{EditorData, labels, look, relabel};
use crate::browser::Purpose;
use crate::state::{EditorAction, EditorState};

/// The right column's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Column {
    #[default]
    Layers,
    Properties,
    Sounds,
    Script,
}

/// A pause in typing (seconds of the UI's time) that commits the script.
pub const SCRIPT_PAUSE: f64 = 1.0;

/// The script editor's id (its cursor is set from an error).
fn script_id() -> egui::Id {
    egui::Id::new("olr-editor-script")
}

/// The script editor's draft: the text, the document's script it last
/// followed (`seen`), the text last committed (`sent`), when it last
/// changed (UI time), its issues (checked when it changes) and where the
/// cursor goes next.
#[derive(Default)]
pub(super) struct ScriptDraft {
    seen: Option<String>,
    text: String,
    sent: Option<String>,
    since: f64,
    checked: Option<(String, Vec<ScriptIssue>)>,
    goto: Option<(u32, u32)>,
}

impl ScriptDraft {
    /// Whether the text waits to be committed.
    fn pending(&self) -> bool {
        self.seen.is_some()
            && self.seen.as_deref() != Some(self.text.as_str())
            && self.sent.as_deref() != Some(self.text.as_str())
    }

    fn issues(&mut self) -> &[ScriptIssue] {
        if self.checked.as_ref().is_none_or(|(t, _)| *t != self.text) {
            self.checked = Some((self.text.clone(), olr_script::check(&self.text)));
        }
        self.checked.as_ref().map_or(&[], |(_, i)| &i[..])
    }
}

/// A picker's width: egui's, at most the room left (its arrow and padding
/// aside); its selected text is truncated to it.
fn picker_width(ui: &Ui) -> f32 {
    let s = ui.spacing();
    let room = ui.available_width() - s.icon_width - 2.0 * s.button_padding.x - s.item_spacing.x;
    s.combo_width.min(room).max(s.interact_size.x)
}

/// The tabs, in a wrapped row.
pub(super) fn tabs(ui: &mut Ui, data: &mut EditorData) {
    ui.horizontal_wrapped(|ui| {
        let tabs = [
            (Column::Layers, labels::LAYERS),
            (Column::Properties, labels::PROPERTIES),
            (Column::Sounds, labels::SOUNDS),
            (Column::Script, labels::SCRIPT),
        ];
        for (c, label) in tabs {
            if ui.add(egui::Button::selectable(data.column == c, label)).clicked() {
                data.column = c;
            }
        }
    });
}

/// The script draft follows the document's script when nothing waits to be
/// committed (an undo, another asset), and commits what waits after the
/// pause or when the editor is not shown (once a frame, before the panels).
pub(super) fn sync_script(ui: &Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let script = state.active_document().and_then(|d| d.parts_snap()).and_then(|p| p.script);
    let now = ui.input(|i| i.time);
    let d = &mut data.script;
    let unchanged =
        d.text == d.seen.clone().unwrap_or_default() || d.sent.as_deref() == Some(d.text.as_str());
    if d.seen != script && unchanged {
        *d = ScriptDraft {
            text: script.clone().unwrap_or_default(),
            seen: script,
            since: now,
            ..ScriptDraft::default()
        };
        return;
    }
    if d.pending() {
        let editing = data.column == Column::Script && ui.ctx().memory(|m| m.has_focus(script_id()));
        if !editing || now - d.since >= SCRIPT_PAUSE {
            d.sent = Some(d.text.clone());
            out.push(EditorAction::SetScript(d.text.clone()));
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SCRIPT_PAUSE));
        }
    }
}

/// A property's name in the Properties tab.
fn property_name(p: &PropertySource) -> &'static str {
    use PropertySource as P;
    match p {
        P::X => "X",
        P::Y => "Y",
        P::Visible => "Visible",
        P::Facing => "Facing",
        P::Health => "Health",
        P::OffsetX(_) => "Offset X",
        P::OffsetY(_) => "Offset Y",
        P::LayerVisible(_) => "Visible",
        P::Opacity(_) => "Opacity",
        P::GlowStrength(_) => "Glow strength",
        P::GlowColour(_) => "Glow colour",
        P::LightIntensity(_) => "Light intensity",
        P::LightReach(_) => "Light reach",
        P::GlowAnimation(_) => "Glow animation",
        P::BeamDirection(_) => "Beam direction",
        P::BeamSpread(_) => "Beam spread",
        P::GlassOpacity(_) => "Glass opacity",
        P::GlassTint(_) => "Glass tint",
    }
}

/// An integer `DragValue` (`key` names its draft) shown beside `shown`,
/// named `name` for AccessKit, committed once (`ui::number`'s rule).
fn int(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    (shown, name): (&str, &str),
    current: i64,
    range: std::ops::RangeInclusive<i64>,
) -> Option<i64> {
    data.drawn.insert(key.into());
    let mut v = data.drafts.get(key).copied().unwrap_or(current as f64);
    ui.horizontal_wrapped(|ui| {
        ui.label(shown);
        let r = ui.add(
            egui::DragValue::new(&mut v)
                .range(*range.start() as f64..=*range.end() as f64)
                .speed(0.25)
                .max_decimals(0)
                .update_while_editing(false),
        );
        relabel(&r, egui::WidgetType::DragValue, None, name);
        if r.dragged() || r.has_focus() {
            data.drafts.insert(key.into(), v);
            return None;
        }
        data.drafts.remove(key);
        let v = (v.round() as i64).clamp(*range.start(), *range.end());
        (v != current).then_some(v)
    })
    .inner
}

/// A one-line text field (`key` names its draft) shown beside `shown`,
/// named `name`, committed when it loses the keyboard (`ui::text`'s rule).
fn line(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    (shown, name): (&str, &str),
    current: &str,
) -> Option<String> {
    data.drawn.insert(key.into());
    let mut v = data.texts.get(key).cloned().unwrap_or_else(|| current.to_string());
    ui.horizontal_wrapped(|ui| {
        ui.label(shown);
        let r = ui.add(egui::TextEdit::singleline(&mut v).desired_width(110.0));
        relabel(&r, egui::WidgetType::TextEdit, None, name);
        if r.has_focus() {
            data.texts.insert(key.into(), v);
            return None;
        }
        data.texts.remove(key);
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        (r.lost_focus() && !escaped && v != current).then_some(v)
    })
    .inner
}

/// A value's editor by its kind: an `Int` a `DragValue` within `range`, a
/// `Bool` a checkbox, a `Colour` a `#rrggbb` field, an `Animation` the
/// look panel's animation editor. The new value when one is committed.
fn value_field(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    (shown, name): (&str, &str),
    (kind, range): (ValueKind, Option<(i32, i32)>),
    now: Option<&ValueSource>,
) -> Option<ValueSource> {
    match kind {
        ValueKind::Int => {
            let (lo, hi) = range.unwrap_or((i32::MIN, i32::MAX));
            let current = match now {
                Some(ValueSource::Int(n)) => i64::from(*n),
                _ => i64::from(lo.max(0).min(hi)),
            };
            int(ui, data, key, (shown, name), current, i64::from(lo)..=i64::from(hi))
                .map(|v| ValueSource::Int(v as i32))
        }
        ValueKind::Bool => {
            let was = matches!(now, Some(ValueSource::Bool(true)));
            let mut b = was;
            let r = ui.checkbox(&mut b, shown);
            relabel(&r, egui::WidgetType::Checkbox, Some(b), name);
            (b != was).then_some(ValueSource::Bool(b))
        }
        ValueKind::Colour => {
            let text = match now {
                Some(ValueSource::Colour(c)) => look::hex(*c),
                _ => String::new(),
            };
            let shown = if now.is_none() { format!("{shown} (each pixel's own)") } else { shown.to_string() };
            line(ui, data, key, (&shown, name), &text)
                .and_then(|t| look::parse_hex(&t))
                .map(ValueSource::Colour)
                .filter(|v| Some(v) != now)
        }
        ValueKind::Animation => {
            let a = match now {
                Some(ValueSource::Animation(a)) => a.clone(),
                _ => AnimationSource::Steady,
            };
            ui.label(shown);
            look::animation(ui, data, key, &a).map(ValueSource::Animation)
        }
    }
}

/// The Properties tab: the Whole asset row's properties and Destructible,
/// then each Rock or Background layer's, top first (a folder's name as a
/// heading), a dirt layer greyed.
pub(super) fn properties(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let Some(doc) = state.active_document() else { return };
    let offers = doc.offers();
    ui.weak(format!("Values in {}", doc.shown_state_name()));
    ui.strong(labels::WHOLE_ASSET);
    for o in offers.iter().filter(|o| o.property.layer().is_none()) {
        row(ui, state, data, labels::WHOLE_ASSET, o, out);
    }
    destructible(ui, state, data, out);
    let folders = doc.folders();
    let mut heading: Vec<String> = Vec::new();
    for item in doc.items().iter().rev() {
        let e = &item.element;
        let path = doc.folders_holding(&e.id);
        if path != heading {
            let names: Vec<&str> = path
                .iter()
                .filter_map(|id| folders.iter().find(|f| f.id == *id))
                .map(|f| f.name.as_str())
                .collect();
            if !names.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(format!("📁 {}", names.join(" / "))).strong());
            }
            heading = path;
        }
        let dirt = matches!(
            e.kind,
            ElementKind::Image { material: ElementMaterial::Dirt, .. }
                | ElementKind::Drawn { material: ElementMaterial::Dirt, mixed: false, .. }
        );
        let mine: Vec<&Offer> = offers.iter().filter(|o| o.property.layer() == Some(e.id.as_str())).collect();
        ui.separator();
        if dirt {
            ui.add_enabled(
                false,
                egui::Label::new(format!("{}: {}", e.name, labels::NOT_OFFERED_DIRT)).truncate(),
            );
            continue;
        }
        if mine.is_empty() {
            continue;
        }
        ui.add(egui::Label::new(egui::RichText::new(&e.name).strong()).truncate());
        for o in mine {
            row(ui, state, data, &e.name, o, out);
        }
    }
}

/// One property's row: its value in the shown state (marked when the state
/// sets it, with Clear), Expose, and when exposed its Name, Default, Min
/// and Max; "Also changes" under a group property shared with other layers.
fn row(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    owner: &str,
    o: &Offer,
    out: &mut Vec<EditorAction>,
) {
    let Some(doc) = state.active_document() else { return };
    let p = &o.property;
    let row = labels::property(owner, property_name(p));
    let key = format!("prop/{}", olr_edit::asset::property_label(p));
    let shown = doc.shown_value(p);
    let exposed = doc.exposed_entry(p).cloned();
    // A group's setting exposed through another of its layers.
    let through = doc.asset_header().and_then(|f| {
        let k = olr_format::assets::property_key(&f, p);
        f.exposed
            .iter()
            .find(|x| x.property != *p && olr_format::assets::property_key(&f, &x.property) == k)
            .map(|x| x.name.clone())
    });
    let range = match &exposed {
        Some(x) => o.range.map(|(lo, hi)| (x.min.unwrap_or(lo), x.max.unwrap_or(hi))),
        None => o.range,
    };
    ui.push_id(&key, |ui| {
        let value = |ui: &mut Ui, data: &mut EditorData, out: &mut Vec<EditorAction>| {
            let name = property_name(p);
            let mark = if shown.own { format!("● {name}") } else { name.to_string() };
            let v = value_field(
                ui,
                data,
                &format!("{key}/value"),
                (&mark, &row),
                (o.kind, range),
                shown.value.as_ref(),
            );
            if let Some(v) = v {
                out.push(EditorAction::SetValue { property: p.clone(), value: Some(v) });
            }
            if shown.own {
                let r = ui.small_button(labels::CLEAR_VALUE).on_hover_text("this state follows the default");
                relabel(&r, egui::WidgetType::Button, None, &labels::clear_value(&row));
                if r.clicked() {
                    out.push(EditorAction::SetValue { property: p.clone(), value: None });
                }
            }
        };
        // An animation's editor takes lines of its own.
        if o.kind == ValueKind::Animation {
            value(ui, data, out);
        } else {
            ui.horizontal_wrapped(|ui| value(ui, data, out));
        }
        let mut on = exposed.is_some();
        let r = ui.add_enabled(through.is_none(), egui::Checkbox::new(&mut on, labels::EXPOSE));
        relabel(&r, egui::WidgetType::Checkbox, Some(on), &labels::expose(&row));
        let r = match &through {
            Some(n) => r.on_disabled_hover_text(format!("its glow group's setting is exposed as {n}")),
            None => r.on_hover_text("give it a name scripts and copies use"),
        };
        if r.changed() {
            match (&exposed, on) {
                (None, true) => out.push(EditorAction::Expose { property: p.clone(), default: None }),
                (Some(x), false) => out.push(EditorAction::Unexpose(x.name.clone())),
                _ => {}
            }
        }
        if let Some(x) = &exposed {
            ui.indent("exposed", |ui| exposed_fields(ui, data, &key, &row, o, x, out));
        }
        if p.of_group() && !o.sharing.is_empty() {
            let names: Vec<String> = o
                .sharing
                .iter()
                .map(|id| {
                    doc.items()
                        .iter()
                        .find(|i| i.element.id == *id)
                        .map_or(id.clone(), |i| i.element.name.clone())
                })
                .collect();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{}: {}", labels::ALSO_CHANGES, names.join(", "))).weak(),
                )
                .wrap(),
            );
        }
    });
}

/// An exposed entry's Name, Default and, for an `Int`, Min and Max.
fn exposed_fields(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    row: &str,
    o: &Offer,
    x: &olr_format::assets::ExposedSource,
    out: &mut Vec<EditorAction>,
) {
    if let Some(new) =
        line(ui, data, &format!("{key}/name"), (labels::NAME, &labels::exposed_name(row)), &x.name)
    {
        out.push(EditorAction::RenameExposed { name: x.name.clone(), new });
    }
    let change = |change| EditorAction::SetExposed { name: x.name.clone(), change };
    if x.property == PropertySource::Health {
        ui.weak("Default: the health's points");
    } else {
        let range = o.range.map(|(lo, hi)| (x.min.unwrap_or(lo), x.max.unwrap_or(hi)));
        let shown = ("Default", labels::exposed_default(row));
        if let Some(v) = value_field(
            ui,
            data,
            &format!("{key}/default"),
            (shown.0, &shown.1),
            (o.kind, range),
            x.default.as_ref(),
        ) {
            out.push(change(ExposedChange::Default(v)));
        }
    }
    if let Some((lo, hi)) = o.range {
        let all = i64::from(lo)..=i64::from(hi);
        let min = i64::from(x.min.unwrap_or(lo));
        let max = i64::from(x.max.unwrap_or(hi));
        if let Some(v) =
            int(ui, data, &format!("{key}/min"), ("Min", &labels::exposed_min(row)), min, all.clone())
        {
            out.push(change(ExposedChange::Min(v as i32)));
        }
        if let Some(v) = int(ui, data, &format!("{key}/max"), ("Max", &labels::exposed_max(row)), max, all) {
            out.push(change(ExposedChange::Max(v as i32)));
        }
    }
}

/// Destructible, and when on its Health points and the state At 0.
fn destructible(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let health = doc.parts_snap().and_then(|p| p.health);
    let mut on = health.is_some();
    let r = ui.checkbox(&mut on, labels::DESTRUCTIBLE).on_hover_text("worms can destroy it");
    if r.changed() {
        out.push(EditorAction::Destructible(on));
    }
    let Some(h) = health else { return };
    ui.indent("health", |ui| {
        let points = int(
            ui,
            data,
            "health/points",
            (labels::HEALTH, labels::HEALTH),
            h.points.into(),
            1..=MAX_HEALTH.into(),
        );
        if let Some(v) = points {
            out.push(EditorAction::SetHealth(HealthSource { points: v as u32, at_zero: h.at_zero.clone() }));
        }
        ui.horizontal_wrapped(|ui| {
            let l = ui.label(labels::AT_ZERO);
            egui::ComboBox::from_id_salt("olr-editor-at-zero")
                .width(picker_width(ui))
                .truncate()
                .selected_text(&h.at_zero)
                .show_ui(ui, |ui| {
                    for st in doc.states() {
                        if ui.selectable_label(st.name == h.at_zero, &st.name).clicked()
                            && st.name != h.at_zero
                        {
                            out.push(EditorAction::SetHealth(HealthSource {
                                points: h.points,
                                at_zero: st.name.clone(),
                            }));
                        }
                    }
                })
                .response
                .labelled_by(l.id);
        });
    });
}

/// The Sounds tab: the asset's sounds, Import sound…, and the shown
/// state's loop and enter sounds.
pub(super) fn sounds(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let Some(parts) = doc.parts_snap() else { return };
    let states = doc.states();
    if parts.sounds.is_empty() {
        ui.weak("No sounds yet");
    }
    for (name, bytes) in &parts.sounds {
        let user = states.iter().find_map(|s| {
            if s.loop_sound.as_deref() == Some(name.as_str()) {
                Some(format!("{}'s loop sound", s.name))
            } else if s.enter_sound.as_deref() == Some(name.as_str()) {
                Some(format!("{}'s enter sound", s.name))
            } else {
                None
            }
        });
        ui.horizontal_wrapped(|ui| {
            let r = ui.small_button("▶");
            relabel(&r, egui::WidgetType::Button, None, &labels::play(name));
            if r.on_hover_text(format!("play {name}")).clicked() {
                out.push(EditorAction::PlaySound(name.clone()));
            }
            let seconds =
                olr_pack::sounds::sound_seconds(bytes).map_or_else(|| "?".into(), |s| format!("{s:.1} s"));
            ui.add(egui::Label::new(format!("{name} ({seconds})")).truncate());
            let r = ui.add_enabled(user.is_none(), egui::Button::new(labels::REMOVE).small());
            relabel(&r, egui::WidgetType::Button, None, &labels::remove_sound(name));
            let r = match &user {
                Some(u) => r.on_disabled_hover_text(format!("it is {u}")),
                None => r,
            };
            if r.clicked() {
                out.push(EditorAction::RemoveSound(name.clone()));
            }
        });
    }
    if ui.button(labels::IMPORT_SOUND).on_hover_text("a WAV or Ogg sound").clicked() {
        out.push(EditorAction::Browse(Purpose::ImportSound));
    }
    ui.separator();
    let Some(st) = doc.shown_state().and_then(|k| states.get(k)) else { return };
    let pick = |ui: &mut Ui, (shown, name): (String, &str), now: &Option<String>| -> Option<Option<String>> {
        let mut chosen = None;
        let l = ui.label(shown);
        let r = egui::ComboBox::from_id_salt(name)
            .width(picker_width(ui))
            .truncate()
            .selected_text(now.as_deref().unwrap_or(labels::NO_SOUND))
            .show_ui(ui, |ui| {
                if ui.selectable_label(now.is_none(), labels::NO_SOUND).clicked() && now.is_some() {
                    chosen = Some(None);
                }
                for s in parts.sounds.keys() {
                    if ui.selectable_label(now.as_ref() == Some(s), s).clicked() && now.as_ref() != Some(s) {
                        chosen = Some(Some(s.clone()));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        relabel(&r, egui::WidgetType::ComboBox, None, name);
        chosen
    };
    let looped = pick(ui, (format!("Loop while in {}", st.name), labels::LOOP_SOUND), &st.loop_sound);
    if let Some(s) = looped {
        out.push(EditorAction::StateSounds { loop_sound: Some(s), enter_sound: None });
    }
    let entered = pick(ui, (format!("Play on entering {}", st.name), labels::ENTER_SOUND), &st.enter_sound);
    if let Some(s) = entered {
        out.push(EditorAction::StateSounds { loop_sound: None, enter_sound: Some(s) });
    }
}

/// The Script tab's column: New / Remove script, the size, the errors.
pub(super) fn script_column(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let Some(doc) = state.active_document() else { return };
    let has = doc.parts_snap().is_some_and(|p| p.script.is_some());
    ui.add(egui::Label::new(egui::RichText::new(labels::SCRIPT_NOT_RUN).weak()).wrap());
    if !has {
        if ui.button(labels::NEW_SCRIPT).on_hover_text("a script run on every live copy").clicked() {
            out.push(EditorAction::NewScript);
        }
        return;
    }
    if ui.button(labels::REMOVE_SCRIPT).clicked() {
        out.push(EditorAction::RemoveScript);
    }
    let d = &mut data.script;
    ui.label(format!("{} of {} bytes", d.text.len(), MAX_SCRIPT_BYTES));
    let issues = d.issues().to_vec();
    if issues.is_empty() {
        ui.weak(labels::NO_SCRIPT_ERRORS);
    }
    for i in &issues {
        let text = labels::script_error(i);
        let r =
            ui.add(egui::Button::new(egui::RichText::new(&text).color(ui.visuals().error_fg_color)).wrap());
        relabel(&r, egui::WidgetType::Button, None, &text);
        if r.on_hover_text("show it in the script").clicked() {
            data.script.goto = Some((i.line, i.column));
            data.column = Column::Script;
        }
    }
}

/// The char index of `line`:`column` (from 1) in `text`, at most its end.
fn char_at(text: &str, (line, column): (u32, u32)) -> usize {
    let mut at = 0;
    for (n, l) in text.split('\n').enumerate() {
        let len = l.chars().count();
        if n + 1 == line.max(1) as usize {
            return at + (column.max(1) as usize - 1).min(len);
        }
        at += len + 1;
    }
    text.chars().count()
}

/// The middle while Script is chosen: line numbers in a gutter (the error's
/// line marked) beside the monospace editor.
pub(super) fn script_editor(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let has = state.active_document().and_then(|d| d.parts_snap()).is_some_and(|p| p.script.is_some());
    if !has {
        ui.centered_and_justified(|ui| {
            ui.label(format!("No script: {} in the Script tab", labels::NEW_SCRIPT))
        });
        return;
    }
    let now = ui.input(|i| i.time);
    let error_line = data.script.issues().first().map(|i| i.line);
    egui::ScrollArea::both().id_salt("olr-editor-script").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            let d = &mut data.script;
            let lines = d.text.split('\n').count().max(1);
            let width = lines.to_string().len();
            let gutter: String = (1..=lines)
                .map(|n| {
                    let mark = if Some(n as u32) == error_line { "▶" } else { " " };
                    format!("{mark}{n:>width$}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let colour = if error_line.is_some() {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.add(egui::Label::new(egui::RichText::new(gutter).monospace().color(colour)).selectable(false));
            // As tall as the room (a longer script scrolls).
            let row = ui.text_style_height(&egui::TextStyle::Monospace);
            let rows = ((ui.available_height() - 2.0 * ui.spacing().button_padding.y) / row).floor().max(1.0);
            // Lines are not wrapped: the gutter's numbers stay beside them
            // (a long line scrolls the editor across).
            let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let colour = ui.visuals().widgets.inactive.text_color();
                let job =
                    egui::text::LayoutJob::simple(text.as_str().to_owned(), font, colour, f32::INFINITY);
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let output = egui::TextEdit::multiline(&mut d.text)
                .code_editor()
                .layouter(&mut layouter)
                .id(script_id())
                .desired_width(f32::INFINITY)
                .desired_rows(rows as usize)
                .show(ui);
            let r = output.response.response;
            relabel(&r, egui::WidgetType::TextEdit, None, labels::SCRIPT_EDITOR);
            if r.changed() {
                d.since = now;
            }
            // Losing the keyboard commits (before the click that took it acts).
            if r.lost_focus() && d.pending() {
                d.sent = Some(d.text.clone());
                out.push(EditorAction::SetScript(d.text.clone()));
            }
            if let Some(at) = d.goto.take() {
                let mut st = output.state;
                let c = egui::text::CCursor::new(char_at(&d.text, at));
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(c)));
                st.store(ui.ctx(), script_id());
                r.request_focus();
            }
        });
    });
}
