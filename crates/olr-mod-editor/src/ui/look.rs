//! The Look group (issue #19), as the level editor edits its glow groups
//! (`olr-editor`'s `ui/look.rs`): a projectile's `glow`, `light`,
//! `end_flash` and `explosion`, a weapon's `muzzle_light`, each a switch
//! and its colour, intensity, reach and animation. Presentation only: the
//! simulation never reads a look. Each field commits once (a number when
//! its drag stops or its typing ends, a colour's `#rrggbb` when its field
//! loses the keyboard); the whole look is set back as one change.

use egui::Ui;
use olr_data::image::Rgba;
use olr_format::Colour;
use olr_format::look::{
    AnimationSource, ExplosionLightSource, KeySource, LightSource, ProjectileLookSource, PulseShape,
    ReachSource, WeaponLookSource,
};

use super::{Cx, caption, labels, relabel};
use crate::state::{ModAction, ron};

/// A light when it is switched on.
const NEW_LIGHT: LightSource = LightSource {
    colour: Colour(Rgba::rgb(255, 192, 128)),
    intensity: 50,
    reach: ReachSource::Pixels(32),
    animation: AnimationSource::Steady,
    phase_ms: None,
};

/// An explosion light when it is switched on (Liero v1.0 Reloaded's
/// medium blast).
fn new_explosion() -> ExplosionLightSource {
    ExplosionLightSource {
        colour: Colour(Rgba::rgb(255, 188, 112)),
        intensity: 180,
        radius: 7,
        reach: ReachSource::Pixels(110),
        animation: None,
        phase_ms: None,
    }
}

fn hex(c: Colour) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0.r, c.0.g, c.0.b)
}

/// A whole number, committed when its drag stops or its typing ends.
fn number(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    label: &str,
    now: i64,
    range: std::ops::RangeInclusive<i64>,
) -> Option<i64> {
    let key = format!("look:{key}");
    let mut v: i64 = cx.data.drafts.get(&key).and_then(|t| t.parse().ok()).unwrap_or(now);
    caption(ui, label);
    let r = ui.add(egui::DragValue::new(&mut v).range(range));
    relabel(&r, egui::WidgetType::DragValue, label);
    if r.changed() {
        cx.data.drafts.insert(key.clone(), v.to_string());
    }
    let done = r.drag_stopped() || r.lost_focus() || (r.changed() && !r.dragged() && !r.has_focus());
    if done && cx.data.drafts.remove(&key).is_some() && v != now {
        return Some(v);
    }
    None
}

/// A colour's `#rrggbb`, committed when the field loses the keyboard; an
/// unreadable or see-through entry is dropped.
fn colour(ui: &mut Ui, cx: &mut Cx, key: &str, label: &str, now: Colour) -> Option<Colour> {
    let key = format!("look:{key}");
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(now.0.r, now.0.g, now.0.b));
    let mut t = cx.data.drafts.get(&key).cloned().unwrap_or_else(|| hex(now));
    let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(70.0));
    relabel(&r, egui::WidgetType::TextEdit, label);
    if r.changed() {
        cx.data.drafts.insert(key.clone(), t.clone());
    }
    if r.lost_focus() && cx.data.drafts.remove(&key).is_some() {
        return Colour::parse(t.trim()).ok().filter(|c| c.0.a == 255 && *c != now);
    }
    None
}

/// A reach: world pixels, or unlimited when `unlimited` is offered.
fn reach(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    part: &str,
    now: ReachSource,
    unlimited: bool,
) -> Option<ReachSource> {
    let mut out = None;
    if let ReachSource::Pixels(px) = now
        && let Some(v) =
            number(ui, cx, &format!("{key}/reach"), &labels::look(part, "reach (px)"), px.into(), 1..=4096)
    {
        out = Some(ReachSource::Pixels(v as u16));
    }
    if unlimited {
        let mut on = now == ReachSource::Unlimited;
        let name = labels::look(part, labels::UNLIMITED);
        let r = ui.checkbox(&mut on, labels::UNLIMITED);
        relabel(&r, egui::WidgetType::Checkbox, &name);
        if r.changed() {
            out = Some(if on { ReachSource::Unlimited } else { ReachSource::Pixels(64) });
        }
    }
    out
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

/// A kind's first animation (the level editor's).
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

/// The animation editor: a kind (or, with `engine`, the engine's own
/// flash: `None`) and the kind's fields. The changed animation.
fn animation(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    part: &str,
    now: Option<&AnimationSource>,
    engine: bool,
) -> Option<Option<AnimationSource>> {
    let name = labels::look(part, "animation");
    let kind = now.map(kind_of);
    let shown = kind.map_or("The engine's flash", |k| KINDS[k]);
    let mut out = None;
    ui.horizontal(|ui| {
        caption(ui, &name);
        let r = egui::ComboBox::from_id_salt(("olr-mod-editor-look-anim", key))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if engine
                    && ui.selectable_label(kind.is_none(), "The engine's flash").clicked()
                    && kind.is_some()
                {
                    out = Some(None);
                }
                for (i, n) in KINDS.iter().enumerate() {
                    if ui.selectable_label(kind == Some(i), *n).clicked() && kind != Some(i) {
                        out = Some(Some(default_of(i)));
                    }
                }
            })
            .response;
        relabel(&r, egui::WidgetType::ComboBox, &name);
    });
    if out.is_some() {
        return out;
    }
    let field = |what: &str| labels::look(part, what);
    match now.cloned()? {
        AnimationSource::Steady => None,
        AnimationSource::Pulse { period_ms, low, high, shape } => {
            let (mut p, mut l, mut h, mut sq) = (None, None, None, false);
            ui.horizontal(|ui| {
                p = number(
                    ui,
                    cx,
                    &format!("{key}/period"),
                    &field("period (ms)"),
                    period_ms.into(),
                    1..=600_000,
                );
                l = number(ui, cx, &format!("{key}/low"), &field("low (%)"), low.into(), 0..=1000);
                h = number(ui, cx, &format!("{key}/high"), &field("high (%)"), high.into(), 0..=1000);
            });
            let mut square = shape == PulseShape::Square;
            let r = ui.checkbox(&mut square, "Square");
            relabel(&r, egui::WidgetType::Checkbox, &field("square pulse"));
            sq |= r.changed();
            (p.is_some() || l.is_some() || h.is_some() || sq).then(|| {
                Some(AnimationSource::Pulse {
                    period_ms: p.map_or(period_ms, |v| v as u32),
                    low: l.map_or(low, |v| v as u16),
                    high: h.map_or(high, |v| v as u16),
                    shape: if square { PulseShape::Square } else { PulseShape::Sine },
                })
            })
        }
        AnimationSource::Flicker { speed, depth } => {
            let (mut s, mut d) = (None, None);
            ui.horizontal(|ui| {
                s = number(ui, cx, &format!("{key}/speed"), &field("flicker speed"), speed.into(), 1..=1000);
                d = number(
                    ui,
                    cx,
                    &format!("{key}/depth"),
                    &field("flicker depth (%)"),
                    depth.into(),
                    0..=100,
                );
            });
            (s.is_some() || d.is_some()).then(|| {
                Some(AnimationSource::Flicker {
                    speed: s.map_or(speed, |v| v as u16),
                    depth: d.map_or(depth, |v| v as u16),
                })
            })
        }
        AnimationSource::Keys { mut keys, mut looped } => {
            let r = ui.checkbox(&mut looped, "Loop");
            relabel(&r, egui::WidgetType::Checkbox, &field("keys loop"));
            let mut changed = r.changed();
            let mut remove = None;
            for (i, k) in keys.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let at = number(
                        ui,
                        cx,
                        &format!("{key}/k{i}/at"),
                        &field(&format!("key {} (ms)", i + 1)),
                        k.at_ms.into(),
                        0..=600_000,
                    );
                    let lv = number(
                        ui,
                        cx,
                        &format!("{key}/k{i}/level"),
                        &field(&format!("key {} (%)", i + 1)),
                        k.level.into(),
                        0..=1000,
                    );
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
            if keys.len() < 64 && ui.small_button("Add key").clicked() {
                let last = keys.last().map_or(0, |k| k.at_ms);
                keys.push(KeySource { at_ms: last.saturating_add(100), level: 100, colour: None });
                changed = true;
            }
            // Times never go back (the format's rule): sorted, stable.
            keys.sort_by_key(|k| k.at_ms);
            changed.then_some(Some(AnimationSource::Keys { keys, looped }))
        }
    }
}

/// A light's switch and fields under `part`'s name; the changed light
/// (`Some(None)`: switched off).
fn light(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    part: &str,
    now: Option<&LightSource>,
) -> Option<Option<LightSource>> {
    let mut on = now.is_some();
    let r = ui.checkbox(&mut on, part);
    if r.changed() {
        return Some(on.then_some(NEW_LIGHT));
    }
    let l = now?;
    let mut out = None;
    ui.indent(("olr-mod-editor-look", key), |ui| {
        ui.horizontal(|ui| {
            caption(ui, &labels::look(part, "colour"));
            if let Some(c) = colour(ui, cx, &format!("{key}/colour"), &labels::look(part, "colour"), l.colour)
            {
                out = Some(LightSource { colour: c, ..l.clone() });
            }
            if let Some(v) = number(
                ui,
                cx,
                &format!("{key}/intensity"),
                &labels::look(part, "intensity (%)"),
                l.intensity.into(),
                0..=1000,
            ) {
                out = Some(LightSource { intensity: v as u16, ..l.clone() });
            }
        });
        ui.horizontal(|ui| {
            if let Some(r) = reach(ui, cx, key, part, l.reach, true) {
                out = Some(LightSource { reach: r, ..l.clone() });
            }
        });
        if let Some(Some(a)) = animation(ui, cx, &format!("{key}/animation"), part, Some(&l.animation), false)
        {
            out = Some(LightSource { animation: a, ..l.clone() });
        }
    });
    out.map(Some)
}

/// An explosion light's switch and fields.
fn explosion(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    now: Option<&ExplosionLightSource>,
) -> Option<Option<ExplosionLightSource>> {
    let part = labels::EXPLOSION;
    let mut on = now.is_some();
    let r = ui
        .checkbox(&mut on, part)
        .on_hover_text("Its explosion's light, in place of the mod's or the engine's");
    if r.changed() {
        return Some(on.then(new_explosion));
    }
    let e = now?;
    let mut out = None;
    ui.indent(("olr-mod-editor-look", key), |ui| {
        ui.horizontal(|ui| {
            caption(ui, &labels::look(part, "colour"));
            if let Some(c) = colour(ui, cx, &format!("{key}/colour"), &labels::look(part, "colour"), e.colour)
            {
                out = Some(ExplosionLightSource { colour: c, ..e.clone() });
            }
            if let Some(v) = number(
                ui,
                cx,
                &format!("{key}/intensity"),
                &labels::look(part, "intensity (%)"),
                e.intensity.into(),
                0..=1000,
            ) {
                out = Some(ExplosionLightSource { intensity: v as u16, ..e.clone() });
            }
        });
        ui.horizontal(|ui| {
            let max = i64::from(olr_look::engine::MAX_EXPLOSION_RADIUS);
            if let Some(v) = number(
                ui,
                cx,
                &format!("{key}/radius"),
                &labels::look(part, "radius (px)"),
                e.radius.into(),
                1..=max,
            ) {
                out = Some(ExplosionLightSource { radius: v as u16, ..e.clone() });
            }
            if let Some(r) = reach(ui, cx, key, part, e.reach, false) {
                out = Some(ExplosionLightSource { reach: r, ..e.clone() });
            }
        });
        if let Some(a) = animation(ui, cx, &format!("{key}/animation"), part, e.animation.as_ref(), true) {
            let phase_ms = if a.is_some() { e.phase_ms } else { None };
            out = Some(ExplosionLightSource { animation: a, phase_ms, ..e.clone() });
        }
    });
    out.map(Some)
}

/// The projectile at `path`'s Look group.
pub(super) fn projectile(ui: &mut Ui, cx: &mut Cx, path: &str, now: Option<&ProjectileLookSource>) {
    let look = now.cloned().unwrap_or_default();
    let mut new = None;
    ui.weak("How it looks in a lit level: presentation only, never play.");
    ui.horizontal(|ui| {
        let mut on = look.glow.is_some();
        let r = ui.checkbox(&mut on, labels::HAS_GLOW).on_hover_text(
            "The whole sprite lights itself this much in the dark; its image's glow mask (Art) lights single pixels",
        );
        if r.changed() {
            new = Some(ProjectileLookSource { glow: on.then_some(100), ..look.clone() });
        }
        if let Some(g) = look.glow
            && let Some(v) = number(ui, cx, &format!("{path}/glow"), labels::GLOW, g.into(), 0..=100)
        {
            new = Some(ProjectileLookSource { glow: Some(v as u8), ..look.clone() });
        }
    });
    if let Some(l) = light(ui, cx, &format!("{path}/light"), labels::LIGHT, look.light.as_ref()) {
        new = Some(ProjectileLookSource { light: l, ..look.clone() });
    }
    if let Some(l) = light(ui, cx, &format!("{path}/end_flash"), labels::END_FLASH, look.end_flash.as_ref()) {
        new = Some(ProjectileLookSource { end_flash: l, ..look.clone() });
    }
    if let Some(e) = explosion(ui, cx, &format!("{path}/explosion"), look.explosion.as_ref()) {
        new = Some(ProjectileLookSource { explosion: e, ..look.clone() });
    }
    if let Some(n) = new {
        let value = if n == ProjectileLookSource::default() { None } else { Some(n) };
        cx.commits.push(ModAction::Set { path: format!("{path}.look"), value: ron(&value) });
    }
}

/// The weapon at `path`'s Look group: its muzzle light.
pub(super) fn weapon(ui: &mut Ui, cx: &mut Cx, path: &str, now: Option<&WeaponLookSource>) {
    let look = now.cloned().unwrap_or_default();
    ui.weak("Its light while it fires, in a lit level: presentation only.");
    if let Some(l) =
        light(ui, cx, &format!("{path}/muzzle"), labels::MUZZLE_LIGHT, look.muzzle_light.as_ref())
    {
        let value = l.map(|m| WeaponLookSource { muzzle_light: Some(m) });
        cx.commits.push(ModAction::Set { path: format!("{path}.look"), value: ron(&value) });
    }
}
