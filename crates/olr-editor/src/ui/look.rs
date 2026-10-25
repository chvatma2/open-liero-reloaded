//! The look's panels (plan 15a Task 18; look system §The level editor;
//! issue #8's layout): the level's Lighting section (ambient and glow
//! groups; the rarely used values under "Fine tuning"), its Materials
//! (issue #43), an element's glow (with "Make parts glow…"), its surface
//! materials and whether it lets light through, and the animation editor. Every field commits once, as every editor field
//! does; colours are `#rrggbb` text.

use egui::Ui;
use olr_data::image::Rgba;
use olr_edit::element::Property;
use olr_format::Colour;
use olr_format::levels::{Element, ElementGlow, ElementKind, ElementMaterial, GlowBeam};
use olr_format::look::{
    AnimationSource, DEFAULT_OCCLUSION_REACH, GlowColourSource, GlowGroupSource, KeySource, LookSource,
    OcclusionSource, PulseShape, ReachSource, SolidFloorSource,
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
    occlusion(ui, data, look.occlusion, &mut |o| set(out, &|l| l.occlusion = o));
    if let Some(v) = number(ui, data, "look/haze", labels::HAZE, look.haze.into(), 0..=100) {
        set(out, &|l| l.haze = v as u8);
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

/// The ambient occlusion's strength and, when on, reach (issue #10): right
/// after the ambient colour, not under Fine tuning (it changes a level's
/// look a lot). Strength 0 removes it.
fn occlusion(
    ui: &mut Ui,
    data: &mut EditorData,
    now: Option<OcclusionSource>,
    set: &mut dyn FnMut(Option<OcclusionSource>),
) {
    let strength = now.map_or(0, |o| o.strength);
    if let Some(v) = number(ui, data, "look/occlusion", labels::LOOK_OCCLUSION, strength.into(), 0..=100) {
        let reach = now.map_or(DEFAULT_OCCLUSION_REACH, |o| o.reach);
        set((v > 0).then_some(OcclusionSource { strength: v as u16, reach }));
    }
    let Some(o) = now else { return };
    let (lo, hi) = (olr_look::MIN_OCCLUSION_REACH, olr_look::MAX_OCCLUSION_REACH);
    let key = "look/occlusion-reach";
    if let Some(v) =
        number(ui, data, key, labels::LOOK_OCCLUSION_REACH, o.reach.into(), lo.into()..=hi.into())
    {
        set(Some(OcclusionSource { reach: v as u16, ..o }));
    }
}

/// A glow group's fields: its colour (the pixels' own, or a tint), intensity,
/// reach and animation. Returns the changed group.
fn group_fields(ui: &mut Ui, data: &mut EditorData, g: &GlowGroupSource) -> Option<GlowGroupSource> {
    let key = format!("group/{}", g.id);
    let mut out = None;
    // Own, Tint or Filtered (the background glass design), one segmented
    // switch; a tint's colour carries over to Filtered and back.
    let tint = match g.colour {
        GlowColourSource::Tint(c) | GlowColourSource::Filtered(c) => c,
        GlowColourSource::Own => WHITE,
    };
    let kinds = [
        (GlowColourSource::Own, "Own", "each pixel lights in its art's colour"),
        (GlowColourSource::Tint(tint), "Tint", "every pixel lights in the group's colour"),
        (
            GlowColourSource::Filtered(tint),
            labels::FILTERED,
            "each pixel lights in its art's hue at full strength, times the group's colour: \
             light through tinted glass",
        ),
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (kind, name, hint) in kinds {
            let on = std::mem::discriminant(&kind) == std::mem::discriminant(&g.colour);
            if ui.add(egui::Button::selectable(on, name)).on_hover_text(hint).clicked() && !on {
                out = Some(GlowGroupSource { colour: kind, ..g.clone() });
            }
        }
    });
    if let GlowColourSource::Tint(c) | GlowColourSource::Filtered(c) = g.colour
        && let Some(c) = colour(ui, data, &format!("{key}/colour"), labels::GROUP_COLOUR, c)
    {
        let colour = match g.colour {
            GlowColourSource::Filtered(_) => GlowColourSource::Filtered(c),
            _ => GlowColourSource::Tint(c),
        };
        out = Some(GlowGroupSource { colour, ..g.clone() });
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
    // Issue #62: a distant layer's light is cast from where it lies with
    // the camera at the level's centre.
    let hint = if e.is_distant() { labels::DISTANT_GLOW_HINT } else { "the whole element glows" };
    if ui.add_enabled(!e.locked, egui::Checkbox::new(&mut on, labels::GLOW)).on_hover_text(hint).changed() {
        let glow = on.then(|| ElementGlow { strength: 100, group: String::new(), mask: false, beam: None });
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
        directional(ui, data, doc, e, g, out);
    }
    if ui
        .add_enabled(!e.locked, egui::Button::new(labels::MAKE_PARTS_GLOW))
        .on_hover_text("the Glow brush, By colour: click a colour on the canvas to light those pixels")
        .clicked()
    {
        out.extend([EditorAction::Tool(Tool::Glow), EditorAction::GlowByColour(true)]);
    }
}

/// An element glow's Directional switch, and its Direction and Spread
/// when on (the directional-glow design). Rock and background layers
/// only, not distant ones; a dirt layer gets a hint instead. A group of
/// unlimited reach cannot take a beam, so the switch is off then.
fn directional(
    ui: &mut Ui,
    data: &mut EditorData,
    doc: &crate::document::Document,
    e: &Element,
    g: &ElementGlow,
    out: &mut Vec<EditorAction>,
) {
    let material = match &e.kind {
        ElementKind::Image { material, .. } | ElementKind::Drawn { material, .. } => *material,
        ElementKind::Raster => return,
    };
    if e.is_distant() {
        return;
    }
    if material == ElementMaterial::Dirt {
        ui.weak(labels::DIRECTIONAL_DIRT_HINT);
        return;
    }
    let unlimited = doc
        .level()
        .look
        .iter()
        .flat_map(|l| &l.glow_groups)
        .any(|x| x.id == g.group && x.reach == ReachSource::Unlimited);
    let set_glow =
        |out: &mut Vec<EditorAction>, glow| out.push(EditorAction::SetGlow { id: e.id.clone(), glow });
    let mut on = g.beam.is_some();
    let r = ui.add_enabled(on || !unlimited, egui::Checkbox::new(&mut on, labels::DIRECTIONAL));
    let r = if unlimited {
        r.on_disabled_hover_text("the glow group's reach is unlimited: give it a reach in pixels first")
    } else {
        r.on_hover_text("its pixels cast a cone of light instead of a halo")
    };
    if r.changed() {
        let beam = on.then_some(GlowBeam { direction: 0, spread: 35 });
        set_glow(out, Some(ElementGlow { beam, ..g.clone() }));
    }
    let Some(b) = g.beam else { return };
    let key = format!("{}/glow-beam", e.id);
    if let Some(v) =
        number(ui, data, &format!("{key}/direction"), labels::DIRECTION, b.direction.into(), 0..=359)
    {
        set_glow(out, Some(ElementGlow { beam: Some(GlowBeam { direction: v as u16, ..b }), ..g.clone() }));
    }
    if let Some(v) = number(ui, data, &format!("{key}/spread"), labels::SPREAD, b.spread.into(), 1..=90) {
        set_glow(out, Some(ElementGlow { beam: Some(GlowBeam { spread: v as u8, ..b }), ..g.clone() }));
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

/// The swatch of a material's tint (the Materials view's), `side` points.
pub(super) fn tint_swatch(ui: &mut Ui, value: u8, side: f32, name: &str) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let fill = match crate::state::surface_tint(value) {
        Some(c) => egui::Color32::from_rgb(c.r, c.g, c.b),
        None => egui::Color32::from_gray(90),
    };
    ui.painter().rect(
        rect,
        2.0,
        fill,
        ui.visuals().widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    super::relabel(&r, egui::WidgetType::Other, None, name);
    r
}

/// The level's Materials (issue #43), beside its lighting: each with its
/// Materials view tint, who uses it, and Remove (only when nothing uses
/// it); "Add from library" copies a built-in or user's material in.
pub(super) fn level_materials(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    ui.separator();
    ui.label(egui::RichText::new(labels::MATERIALS).strong())
        .on_hover_text("what the level's pixels are made of: how they take heat from explosions");
    for (i, m) in doc.materials().iter().enumerate() {
        let users = doc.material_users(&m.id);
        ui.horizontal(|ui| {
            let side = ui.spacing().interact_size.y * 0.7;
            tint_swatch(ui, u8::try_from(i + 1).unwrap_or(0), side, &format!("{} colour", m.name));
            ui.label(&m.name);
            let hover = if users.is_empty() {
                "remove it from the level".to_string()
            } else {
                format!("used by {}: change their surfaces first", users.join(", "))
            };
            let r = ui
                .add_enabled(users.is_empty(), egui::Button::new(labels::REMOVE).small())
                .on_hover_text(&hover)
                .on_disabled_hover_text(&hover);
            super::relabel(&r, egui::WidgetType::Button, None, &labels::remove_material(&m.name));
            if r.clicked() {
                out.push(EditorAction::RemoveMaterial(m.id.clone()));
            }
        });
        if !users.is_empty() {
            ui.weak(format!("used by {}", users.join(", ")));
        }
    }
    let library = &state.catalog().materials;
    ui.add_enabled_ui(!library.is_empty(), |ui| {
        ui.menu_button(labels::ADD_MATERIAL, |ui| {
            for (builtin, heading) in [(true, labels::BUILT_IN), (false, labels::YOURS)] {
                let mut entries = library.iter().filter(|e| e.builtin == builtin).peekable();
                if entries.peek().is_none() {
                    continue;
                }
                ui.label(egui::RichText::new(heading).strong());
                for e in entries {
                    let has = doc.material_value(&e.material.id).is_some();
                    let r = ui
                        .add_enabled(!has, egui::Button::new(&e.material.name))
                        .on_disabled_hover_text("the level has it");
                    if r.clicked() {
                        out.push(EditorAction::AddMaterial { builtin, id: e.material.id.clone() });
                        ui.close();
                    }
                }
            }
        })
        .response
        .on_hover_text("copy a material from the library into the level");
    });
}

/// An element's surface materials (issue #43): its Surface (its own
/// pixels', or a distant layer's whole), and its Back surface when it has
/// back art; each one of the level's materials or none. The Surface
/// brush paints them per pixel over these.
pub(super) fn element_surfaces(ui: &mut Ui, state: &EditorState, e: &Element, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
    ui.separator();
    let materials = doc.materials();
    let hint = if materials.is_empty() {
        "add materials to the level first (its settings, with nothing selected)"
    } else {
        "what its pixels are made of: how they take heat"
    };
    let pickers = if e.has_back_art() && !e.is_distant() {
        &[(false, labels::SURFACE), (true, labels::BACK_SURFACE)][..]
    } else {
        &[(false, labels::SURFACE)][..]
    };
    for &(back, label) in pickers {
        let now = if back { &e.back_surface } else { &e.surface };
        let name = |id: Option<&String>| match id {
            Some(id) => materials.iter().find(|m| m.id == *id).map_or_else(|| id.clone(), |m| m.name.clone()),
            None => labels::NO_SURFACE.to_string(),
        };
        ui.horizontal(|ui| {
            let l = ui.label(label);
            ui.add_enabled_ui(!e.locked, |ui| {
                egui::ComboBox::from_id_salt(format!("{}/{label}", e.id))
                    .selected_text(name(now.as_ref()))
                    .show_ui(ui, |ui| {
                        let choices = std::iter::once(None).chain(materials.iter().map(|m| Some(&m.id)));
                        for id in choices {
                            if ui.selectable_label(id == now.as_ref(), name(id)).clicked()
                                && id != now.as_ref()
                            {
                                let id = id.cloned();
                                let p = if back { Property::BackSurface(id) } else { Property::Surface(id) };
                                out.push(EditorAction::Set(e.id.clone(), p));
                            }
                        }
                    })
                    .response
                    .labelled_by(l.id)
                    .on_hover_text(hint);
            });
        });
    }
    if e.surface_map || e.back_surface_map {
        ui.weak("painted with the Surface brush (U)");
    }
}
