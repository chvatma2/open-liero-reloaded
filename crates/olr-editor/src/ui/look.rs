//! The look's panels (plan 15a Task 18; look system §The level editor;
//! issue #8's layout): the level's Lighting section (ambient and glow
//! groups; the rarely used values under "Fine tuning"), an element's glow
//! (with "Make parts glow…") and whether it lets light through, and the
//! animation editor. Every field commits once, as every editor field
//! does; colours are `#rrggbb` text.

use egui::Ui;
use olr_data::image::Rgba;
use olr_edit::element::Property;
use olr_format::Colour;
use olr_format::levels::{Element, ElementGlow, ElementKind};
use olr_format::look::{
    AnimationSource, GlowColourSource, GlowGroupSource, KeySource, LookSource, PulseShape, ReachSource,
    SolidFloorSource,
};

use super::{EditorData, labels, number, text};
use crate::state::{EditorAction, EditorState};
use crate::tools::Tool;

/// The editor's `#rrggbb`.
fn hex(c: Colour) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0.r, c.0.g, c.0.b)
}

/// `#rrggbb` or `rrggbb` as an opaque colour.
fn parse_hex(s: &str) -> Option<Colour> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some(Colour(Rgba::rgb(v(0)?, v(2)?, v(4)?)))
}

/// A colour field: `#rrggbb` text, committed once; an unreadable entry is
/// dropped.
fn colour(ui: &mut Ui, data: &mut EditorData, key: &str, label: &str, now: Colour) -> Option<Colour> {
    text(ui, data, key, label, &hex(now), false).and_then(|t| parse_hex(&t)).filter(|c| *c != now)
}

const WHITE: Colour = Colour(Rgba::rgb(255, 255, 255));

/// A reach: world pixels, or unlimited.
fn reach(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    label: &str,
    now: ReachSource,
) -> Option<ReachSource> {
    let mut unlimited = now == ReachSource::Unlimited;
    if ui.checkbox(&mut unlimited, format!("{label}: {}", labels::UNLIMITED)).changed() {
        return Some(if unlimited { ReachSource::Unlimited } else { ReachSource::Pixels(64) });
    }
    let ReachSource::Pixels(px) = now else { return None };
    number(ui, data, key, label, px.into(), 1..=4096).map(|v| ReachSource::Pixels(v as u16))
}

const KINDS: [&str; 4] = ["Steady", "Pulse", "Flicker", "Keys"];

fn kind_of(a: &AnimationSource) -> usize {
    match a {
        AnimationSource::Steady => 0,
        AnimationSource::Pulse { .. } => 1,
        AnimationSource::Flicker { .. } => 2,
        AnimationSource::Keys { .. } => 3,
    }
}

/// A kind's first animation.
fn default_of(kind: usize) -> AnimationSource {
    match kind {
        1 => AnimationSource::Pulse { period_ms: 1000, low: 30, high: 100, shape: PulseShape::Sine },
        2 => AnimationSource::Flicker { speed: 8, depth: 25 },
        3 => AnimationSource::Keys {
            keys: vec![
                KeySource { at_ms: 0, level: 100, colour: None },
                KeySource { at_ms: 500, level: 0, colour: None },
            ],
            looped: true,
        },
        _ => AnimationSource::Steady,
    }
}

/// The animation editor: a kind picker and the kind's fields; `Keys` a
/// list with add and remove. Returns the changed animation.
fn animation(
    ui: &mut Ui,
    data: &mut EditorData,
    key: &str,
    now: &AnimationSource,
) -> Option<AnimationSource> {
    let kind = kind_of(now);
    let mut out = None;
    let label = ui.label(labels::ANIMATION);
    egui::ComboBox::from_id_salt(format!("{key}/kind"))
        .selected_text(KINDS[kind])
        .show_ui(ui, |ui| {
            for (i, name) in KINDS.iter().enumerate() {
                if ui.selectable_label(i == kind, *name).clicked() && i != kind {
                    out = Some(default_of(i));
                }
            }
        })
        .response
        .labelled_by(label.id);
    if out.is_some() {
        return out;
    }
    match now.clone() {
        AnimationSource::Steady => None,
        AnimationSource::Pulse { period_ms, low, high, shape } => {
            let p = number(ui, data, &format!("{key}/period"), "Period ms", period_ms.into(), 1..=600_000);
            let l = number(ui, data, &format!("{key}/low"), "Low %", low.into(), 0..=1000);
            let h = number(ui, data, &format!("{key}/high"), "High %", high.into(), 0..=1000);
            let mut square = shape == PulseShape::Square;
            let s = ui.checkbox(&mut square, "Square").changed();
            (p.is_some() || l.is_some() || h.is_some() || s).then(|| AnimationSource::Pulse {
                period_ms: p.map_or(period_ms, |v| v as u32),
                low: l.map_or(low, |v| v as u16),
                high: h.map_or(high, |v| v as u16),
                shape: if square { PulseShape::Square } else { PulseShape::Sine },
            })
        }
        AnimationSource::Flicker { speed, depth } => {
            let s = number(ui, data, &format!("{key}/speed"), "Speed", speed.into(), 1..=1000);
            let d = number(ui, data, &format!("{key}/depth"), "Depth %", depth.into(), 0..=100);
            (s.is_some() || d.is_some()).then(|| AnimationSource::Flicker {
                speed: s.map_or(speed, |v| v as u16),
                depth: d.map_or(depth, |v| v as u16),
            })
        }
        AnimationSource::Keys { mut keys, mut looped } => {
            let mut changed = ui.checkbox(&mut looped, "Loop").changed();
            let mut remove = None;
            for (i, k) in keys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let at = number(ui, data, &format!("{key}/k{i}/at"), "ms", k.at_ms.into(), 0..=600_000);
                    let lv = number(ui, data, &format!("{key}/k{i}/level"), "%", k.level.into(), 0..=1000);
                    if let Some(v) = at {
                        (k.at_ms, changed) = (v as u32, true);
                    }
                    if let Some(v) = lv {
                        (k.level, changed) = (v as u16, true);
                    }
                    if ui.small_button("✕").on_hover_text("Remove the key").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove.filter(|_| keys.len() > 1) {
                keys.remove(i);
                changed = true;
            }
            if keys.len() < 64 && ui.button("Add key").clicked() {
                let last = keys.last().map_or(0, |k| k.at_ms);
                keys.push(KeySource { at_ms: last.saturating_add(100), level: 100, colour: None });
                changed = true;
            }
            // Times never go back (the format's rule): sorted, stable.
            keys.sort_by_key(|k| k.at_ms);
            changed.then_some(AnimationSource::Keys { keys, looped })
        }
    }
}

/// The level panel's Lighting section: Add or Remove look, the ambient
/// light, "Fine tuning" (folded by default, `EditorSettings::
/// fine_tuning_open`: max light, steps, softness, soak, solid floor and the
/// grade), then the glow groups.
pub(super) fn level_look(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let Some(doc) = state.document() else { return };
    ui.separator();
    ui.heading(labels::LOOK);
    let Some(look) = doc.level().look.clone() else {
        if ui.button(labels::ADD_LOOK).on_hover_text("darkness, light, glow and a grade").clicked() {
            out.push(EditorAction::SetLook(Some(LookSource::default())));
        }
        return;
    };
    let set = |out: &mut Vec<EditorAction>, f: &dyn Fn(&mut LookSource)| {
        let mut l = look.clone();
        f(&mut l);
        out.push(EditorAction::SetLook(Some(l)));
    };
    if let Some(v) =
        number(ui, data, "look/ambient", labels::LOOK_AMBIENT, look.ambient.intensity.into(), 0..=100)
    {
        set(out, &|l| l.ambient.intensity = v as u16);
    }
    let ambient = look.ambient.colour.unwrap_or(WHITE);
    if let Some(c) = colour(ui, data, "look/ambient-colour", labels::LOOK_AMBIENT_COLOUR, ambient) {
        set(out, &|l| l.ambient.colour = Some(c));
    }
    ui.separator();
    ui.label(egui::RichText::new("Glow groups").strong());
    for group in &look.glow_groups {
        egui::CollapsingHeader::new(&group.id)
            .id_salt(format!("group/{}", group.id))
            .default_open(true)
            .show(ui, |ui| {
                if let Some(g) = group_fields(ui, data, group) {
                    out.push(EditorAction::SetGroup { id: group.id.clone(), group: g });
                }
                if ui.button(labels::REMOVE).clicked() {
                    out.push(EditorAction::RemoveGroup(group.id.clone()));
                }
            });
    }
    if ui.button(labels::ADD_GROUP).clicked() {
        let taken: Vec<&str> = look.glow_groups.iter().map(|g| g.id.as_str()).collect();
        let id = (1..).map(|n| format!("group-{n}")).find(|id| !taken.contains(&id.as_str())).unwrap();
        out.push(EditorAction::AddGroup(GlowGroupSource {
            id,
            colour: GlowColourSource::Own,
            intensity: 100,
            reach: ReachSource::Pixels(64),
            animation: AnimationSource::Steady,
            phase_ms: None,
        }));
    }
    ui.separator();
    let open = state.settings().fine_tuning_open;
    let r = egui::CollapsingHeader::new(labels::FINE_TUNING)
        .id_salt("olr-editor-fine-tuning")
        .open(Some(open))
        .show(ui, |ui| {
            if let Some(v) =
                number(ui, data, "look/max", labels::LOOK_MAX_LIGHT, look.max_light.into(), 100..=200)
            {
                set(out, &|l| l.max_light = v as u16);
            }
            if let Some(v) =
                number(ui, data, "look/steps", labels::LOOK_STEPS, look.light_steps.into(), 0..=32)
            {
                // 1 band is no band: 0 or 2 to 32.
                set(out, &|l| l.light_steps = if v == 1 { 2 } else { v as u8 });
            }
            if let Some(v) =
                number(ui, data, "look/softness", labels::LOOK_SOFTNESS, look.softness.into(), 0..=4)
            {
                set(out, &|l| l.softness = v as u8);
            }
            if let Some(v) = number(ui, data, "look/soak", labels::LOOK_SOAK, look.light_soak.into(), 0..=12)
            {
                set(out, &|l| l.light_soak = v as u8);
            }
            let floor = look.solid_floor;
            if let Some(v) =
                number(ui, data, "look/floor", labels::LOOK_FLOOR, floor.intensity.into(), 0..=100)
            {
                set(out, &|l| l.solid_floor = SolidFloorSource { intensity: v as u16, ..floor });
            }
            let g = &look.grade;
            if let Some(v) =
                number(ui, data, "look/exposure", labels::LOOK_EXPOSURE, g.exposure.into(), -200..=200)
            {
                set(out, &|l| l.grade.exposure = v as i16);
            }
            if let Some(v) =
                number(ui, data, "look/contrast", labels::LOOK_CONTRAST, g.contrast.into(), 0..=200)
            {
                set(out, &|l| l.grade.contrast = v as u16);
            }
            if let Some(v) =
                number(ui, data, "look/saturation", labels::LOOK_SATURATION, g.saturation.into(), 0..=200)
            {
                set(out, &|l| l.grade.saturation = v as u16);
            }
            if let Some(c) = colour(ui, data, "look/tint", labels::LOOK_TINT, g.tint.unwrap_or(WHITE)) {
                set(out, &|l| l.grade.tint = Some(c));
            }
        });
    if r.header_response.clicked() {
        out.push(EditorAction::ShowFineTuning(!open));
    }
    if ui.button(labels::REMOVE_LOOK).on_hover_text("glowing elements stop glowing").clicked() {
        out.push(EditorAction::SetLook(None));
    }
}

/// A glow group's fields: its colour (the pixels' own, or a tint), intensity,
/// reach and animation. Returns the changed group.
fn group_fields(ui: &mut Ui, data: &mut EditorData, g: &GlowGroupSource) -> Option<GlowGroupSource> {
    let key = format!("group/{}", g.id);
    let mut out = None;
    let mut own = g.colour == GlowColourSource::Own;
    if ui.checkbox(&mut own, "Own colour").on_hover_text("each pixel lights in its art's colour").changed() {
        let colour = if own { GlowColourSource::Own } else { GlowColourSource::Tint(WHITE) };
        out = Some(GlowGroupSource { colour, ..g.clone() });
    }
    if let GlowColourSource::Tint(c) = g.colour
        && let Some(c) = colour(ui, data, &format!("{key}/colour"), labels::GROUP_COLOUR, c)
    {
        out = Some(GlowGroupSource { colour: GlowColourSource::Tint(c), ..g.clone() });
    }
    if let Some(v) =
        number(ui, data, &format!("{key}/intensity"), labels::GROUP_INTENSITY, g.intensity.into(), 0..=1000)
    {
        out = Some(GlowGroupSource { intensity: v as u16, ..g.clone() });
    }
    if let Some(r) = reach(ui, data, &format!("{key}/reach"), labels::GROUP_REACH, g.reach) {
        out = Some(GlowGroupSource { reach: r, ..g.clone() });
    }
    if let Some(a) = animation(ui, data, &format!("{key}/animation"), &g.animation) {
        out = Some(GlowGroupSource { animation: a, ..g.clone() });
    }
    out
}

/// An image or drawn element's Glow: the checkbox (a new glow gets its own
/// group; the only way to light or unlight the whole element), its
/// strength and group, and "Make parts glow…": the Glow brush with By
/// colour on, the casual way to light lamps and screens (click their
/// colour).
pub(super) fn element_glow(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    e: &Element,
    out: &mut Vec<EditorAction>,
) {
    if matches!(e.kind, ElementKind::Raster) {
        return;
    }
    let Some(doc) = state.document() else { return };
    ui.separator();
    let set_glow =
        |out: &mut Vec<EditorAction>, glow| out.push(EditorAction::SetGlow { id: e.id.clone(), glow });
    let mut on = e.glow.is_some();
    if ui
        .add_enabled(!e.locked, egui::Checkbox::new(&mut on, labels::GLOW))
        .on_hover_text("the whole element glows")
        .changed()
    {
        let glow = on.then(|| ElementGlow { strength: 100, group: String::new(), mask: false });
        set_glow(out, glow);
    }
    if let Some(g) = e.glow.as_ref().filter(|_| !e.locked) {
        let key = format!("{}/glow", e.id);
        if let Some(v) = number(ui, data, &key, labels::GLOW_STRENGTH, g.strength.into(), 1..=100) {
            set_glow(out, Some(ElementGlow { strength: v as u8, ..g.clone() }));
        }
        let groups: Vec<String> =
            doc.level().look.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.clone())).collect();
        let label = ui.label(labels::GLOW_GROUP);
        egui::ComboBox::from_id_salt(format!("{}/glow-group", e.id))
            .selected_text(&g.group)
            .show_ui(ui, |ui| {
                for id in &groups {
                    if ui.selectable_label(*id == g.group, id).clicked() && *id != g.group {
                        set_glow(out, Some(ElementGlow { group: id.clone(), ..g.clone() }));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        if g.mask {
            ui.weak("painted with the Glow brush (H)");
        }
    }
    if ui
        .add_enabled(!e.locked, egui::Button::new(labels::MAKE_PARTS_GLOW))
        .on_hover_text("the Glow brush, By colour: click a colour on the canvas to light those pixels")
        .clicked()
    {
        out.extend([EditorAction::Tool(Tool::Glow), EditorAction::GlowByColour(true)]);
    }
}

/// Whether an image's or drawn layer's rock and dirt let light through
/// (plan 15a; under the properties' "More").
pub(super) fn element_clear(ui: &mut Ui, e: &Element, out: &mut Vec<EditorAction>) {
    if matches!(e.kind, ElementKind::Raster) {
        return;
    }
    let mut clear = e.clear;
    if ui
        .add_enabled(!e.locked, egui::Checkbox::new(&mut clear, labels::CLEAR))
        .on_hover_text("its rock and dirt let light through; worms and shots still collide")
        .changed()
    {
        out.push(EditorAction::Set(e.id.clone(), Property::Clear(clear)));
    }
}
