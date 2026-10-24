//! The Look group (issue #19), as the level editor edits its glow groups
//! (`olr-editor`'s `ui/look.rs`): a projectile's `glow`, `light`,
//! `end_flash` and `explosion`, a weapon's `muzzle_light`, each a switch
//! and its colour, intensity, reach and animation; issue #42: beside the
//! explosion light, a projectile's `explosion_heat` and `heat`, and the
//! mod's own (`mod.ron`'s `look`: its `explosion` and `explosion_heat`);
//! issue #69: a Beam weapon's `beam` light, tip light and heat. Presentation only: the
//! simulation never reads a look. Each field commits once (a number when
//! its drag stops or its typing ends, a colour's `#rrggbb` when its field
//! loses the keyboard); the whole look is set back as one change.

use egui::Ui;
use olr_data::image::Rgba;
use olr_format::Colour;
use olr_format::look::{
    AnimationSource, BeamHeatSource, BeamLookSource, ExplosionHeatSource, ExplosionLightSource, KeySource,
    LightSource, ModWideLookSource, ProjectileHeatSource, ProjectileLookSource, PulseShape, ReachSource,
    WeaponLookSource,
};
use olr_look::engine::{
    self, MAX_EXPLOSION_HEAT_AMOUNT, MAX_EXPLOSION_HEAT_RADIUS, MAX_PROJECTILE_HEAT,
    MAX_PROJECTILE_HEAT_RADIUS,
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

/// A reach: world pixels up to `max`, or unlimited when `unlimited` is
/// offered.
fn reach(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    part: &str,
    now: ReachSource,
    max: u16,
    unlimited: bool,
) -> Option<ReachSource> {
    let mut out = None;
    let label = labels::look(part, "reach (px)");
    if let ReachSource::Pixels(px) = now
        && let Some(v) = number(ui, cx, &format!("{key}/reach"), &label, px.into(), 1..=max.into())
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
    light_from(ui, cx, key, part, now, || NEW_LIGHT, None)
}

/// [`light`] switched on as `new`, its reach within `beam_reach` world
/// pixels and never unlimited when that is given (a beam's, issue #69).
fn light_from(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    part: &str,
    now: Option<&LightSource>,
    new: impl FnOnce() -> LightSource,
    beam_reach: Option<u16>,
) -> Option<Option<LightSource>> {
    let mut on = now.is_some();
    let r = ui.checkbox(&mut on, part);
    if r.changed() {
        return Some(on.then(new));
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
            let (max, unlimited) = beam_reach.map_or((4096, true), |m| (m, false));
            if let Some(r) = reach(ui, cx, key, part, l.reach, max, unlimited) {
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

/// An explosion light's switch (`hover` its meaning) and fields.
fn explosion(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    now: Option<&ExplosionLightSource>,
    hover: &str,
) -> Option<Option<ExplosionLightSource>> {
    let part = labels::EXPLOSION;
    let mut on = now.is_some();
    let r = ui.checkbox(&mut on, part).on_hover_text(hover);
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
            if let Some(r) = reach(ui, cx, key, part, e.reach, 4096, false) {
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

/// An explosion heat when it is switched on: the engine's.
const NEW_EXPLOSION_HEAT: ExplosionHeatSource = ExplosionHeatSource { amount: 100, radius: None };

/// A projectile's heat a tick when it is switched on: a flame's.
const NEW_HEAT: ProjectileHeatSource = ProjectileHeatSource { amount: 5, radius: 4 };

/// An explosion heat's switch (`hover` its meaning) and fields (issue
/// #42): its amount, percent of the engine's, and its own radius or the
/// engine's.
fn explosion_heat(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    now: Option<&ExplosionHeatSource>,
    hover: &str,
) -> Option<Option<ExplosionHeatSource>> {
    let part = labels::EXPLOSION_HEAT;
    let mut on = now.is_some();
    let r = ui.checkbox(&mut on, part).on_hover_text(hover);
    if r.changed() {
        return Some(on.then_some(NEW_EXPLOSION_HEAT));
    }
    let h = now?;
    let mut out = None;
    ui.indent(("olr-mod-editor-look", key), |ui| {
        ui.horizontal(|ui| {
            let max = i64::from(MAX_EXPLOSION_HEAT_AMOUNT);
            let label = labels::look(part, "amount (%)");
            if let Some(v) = number(ui, cx, &format!("{key}/amount"), &label, h.amount.into(), 0..=max) {
                out = Some(ExplosionHeatSource { amount: v as u16, ..h.clone() });
            }
        });
        ui.horizontal(|ui| {
            let mut own = h.radius.is_some();
            let r = ui
                .checkbox(&mut own, "Own radius")
                .on_hover_text("Off: the engine's for the explosion, which grows with its damage");
            relabel(&r, egui::WidgetType::Checkbox, &labels::look(part, labels::OWN_RADIUS));
            if r.changed() {
                out = Some(ExplosionHeatSource { radius: own.then_some(16), ..h.clone() });
            }
            let max = MAX_EXPLOSION_HEAT_RADIUS as i64;
            if let Some(radius) = h.radius
                && let Some(v) = number(
                    ui,
                    cx,
                    &format!("{key}/radius"),
                    &labels::look(part, "radius (px)"),
                    radius.into(),
                    1..=max,
                )
            {
                out = Some(ExplosionHeatSource { radius: Some(v as u16), ..h.clone() });
            }
        });
    });
    out.map(Some)
}

/// A projectile's heat a tick: its switch, amount and radius (issue #42).
fn heat(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    now: Option<&ProjectileHeatSource>,
) -> Option<Option<ProjectileHeatSource>> {
    let part = labels::HEAT;
    let mut on = now.is_some();
    let r = ui.checkbox(&mut on, part).on_hover_text(
        "Heat it leaves where it is each tick it lives (flames, napalm, a hot slug), in a level whose materials heat",
    );
    if r.changed() {
        return Some(on.then_some(NEW_HEAT));
    }
    let h = now?;
    let mut out = None;
    ui.indent(("olr-mod-editor-look", key), |ui| {
        ui.horizontal(|ui| {
            let max = i64::from(MAX_PROJECTILE_HEAT);
            let label = labels::look(part, "amount a tick");
            if let Some(v) = number(ui, cx, &format!("{key}/amount"), &label, h.amount.into(), 1..=max) {
                out = Some(ProjectileHeatSource { amount: v as u16, ..h.clone() });
            }
            let max = i64::from(MAX_PROJECTILE_HEAT_RADIUS);
            let label = labels::look(part, "radius (px)");
            if let Some(v) = number(ui, cx, &format!("{key}/radius"), &label, h.radius.into(), 1..=max) {
                out = Some(ProjectileHeatSource { radius: v as u16, ..h.clone() });
            }
        });
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
    let hover = "Its explosion's light, in place of the mod's or the engine's";
    if let Some(e) = explosion(ui, cx, &format!("{path}/explosion"), look.explosion.as_ref(), hover) {
        new = Some(ProjectileLookSource { explosion: e, ..look.clone() });
    }
    let hover = "Its explosions' heat in a level whose materials heat, in place of the mod's or the engine's";
    let now = look.explosion_heat.as_ref();
    if let Some(h) = explosion_heat(ui, cx, &format!("{path}/explosion_heat"), now, hover) {
        new = Some(ProjectileLookSource { explosion_heat: h, ..look.clone() });
    }
    if let Some(h) = heat(ui, cx, &format!("{path}/heat"), look.heat.as_ref()) {
        new = Some(ProjectileLookSource { heat: h, ..look.clone() });
    }
    if let Some(n) = new {
        let value = if n == ProjectileLookSource::default() { None } else { Some(n) };
        cx.commits.push(ModAction::Set { path: format!("{path}.look"), value: ron(&value) });
    }
}

/// The mod's own look (`mod.ron`'s `look`, issue #42): the explosion light
/// and heat of every projectile without its own.
pub(super) fn mod_wide(ui: &mut Ui, cx: &mut Cx, now: Option<&ModWideLookSource>) {
    let look = now.cloned().unwrap_or_default();
    ui.weak("Every projectile's explosions without their own, in a lit level: presentation only.");
    let mut new = None;
    let hover = "The light of every projectile's explosion without its own, in place of the engine's";
    if let Some(e) = explosion(ui, cx, "mod/explosion", look.explosion.as_ref(), hover) {
        new = Some(ModWideLookSource { explosion: e, ..look.clone() });
    }
    let hover = "The heat of every projectile's explosion without its own, in a level whose materials heat, in place of the engine's";
    if let Some(h) = explosion_heat(ui, cx, "mod/explosion_heat", look.explosion_heat.as_ref(), hover) {
        new = Some(ModWideLookSource { explosion_heat: h, ..look.clone() });
    }
    if let Some(n) = new {
        let value = if n == ModWideLookSource::default() { None } else { Some(n) };
        cx.commits.push(ModAction::Set { path: "mod.look".into(), value: ron(&value) });
    }
}

/// A beam's heat when it is switched on: the engine's.
const NEW_BEAM_HEAT: BeamHeatSource =
    BeamHeatSource { amount: engine::BEAM_HEAT.amount, radius: engine::BEAM_HEAT.radius as u16 };

/// An engine light as the format writes it (a beam's, issue #69).
fn light_source(l: olr_look::Light) -> LightSource {
    LightSource {
        colour: Colour(Rgba::rgb(l.colour[0], l.colour[1], l.colour[2])),
        intensity: l.intensity,
        reach: match l.reach {
            olr_look::Reach::Pixels(px) => ReachSource::Pixels(px),
            olr_look::Reach::Unlimited => ReachSource::Unlimited,
        },
        animation: AnimationSource::Steady,
        phase_ms: None,
    }
}

/// A beam's heat a sub-step: its switch, amount and radius (issue #69).
fn beam_heat(
    ui: &mut Ui,
    cx: &mut Cx,
    key: &str,
    now: Option<&BeamHeatSource>,
) -> Option<Option<BeamHeatSource>> {
    let part = labels::BEAM_HEAT;
    let mut on = now.is_some();
    let r = ui.checkbox(&mut on, part).on_hover_text(
        "Heat it leaves where it stops on rock or dirt, in a level whose materials heat; off: the engine's",
    );
    if r.changed() {
        return Some(on.then_some(NEW_BEAM_HEAT));
    }
    let h = now?;
    let mut out = None;
    ui.indent(("olr-mod-editor-look", key), |ui| {
        ui.horizontal(|ui| {
            let max = i64::from(MAX_PROJECTILE_HEAT);
            let label = labels::look(part, "amount a sub-step");
            if let Some(v) = number(ui, cx, &format!("{key}/amount"), &label, h.amount.into(), 0..=max) {
                out = Some(BeamHeatSource { amount: v as u16, ..h.clone() });
            }
            let max = i64::from(MAX_PROJECTILE_HEAT_RADIUS);
            let label = labels::look(part, "radius (px)");
            if let Some(v) = number(ui, cx, &format!("{key}/radius"), &label, h.radius.into(), 1..=max) {
                out = Some(BeamHeatSource { radius: v as u16, ..h.clone() });
            }
        });
    });
    out.map(Some)
}

/// The weapon at `path`'s Look group: its muzzle light; a Beam weapon's
/// (`beam` its colour) also its beam light, tip light and heat (issue
/// #69), each off the engine's.
pub(super) fn weapon(
    ui: &mut Ui,
    cx: &mut Cx,
    path: &str,
    now: Option<&WeaponLookSource>,
    beam: Option<Colour>,
) {
    let look = now.cloned().unwrap_or_default();
    ui.weak("Its light while it fires, in a lit level: presentation only.");
    let mut new = None;
    if let Some(l) =
        light(ui, cx, &format!("{path}/muzzle"), labels::MUZZLE_LIGHT, look.muzzle_light.as_ref())
    {
        new = Some(WeaponLookSource { muzzle_light: l, ..look.clone() });
    }
    if let Some(colour) = beam {
        let b = look.beam.clone().unwrap_or_default();
        let mut set = None;
        let rgb = [colour.0.r, colour.0.g, colour.0.b];
        let max = Some(engine::MAX_BEAM_REACH);
        let now = b.light.as_ref();
        let on = || light_source(engine::beam_light(rgb));
        if let Some(l) = light_from(ui, cx, &format!("{path}/beam_light"), labels::BEAM_LIGHT, now, on, max) {
            set = Some(BeamLookSource { light: l, ..b.clone() });
        }
        let now = b.tip_light.as_ref();
        let on = || light_source(engine::beam_tip_light(rgb));
        if let Some(l) = light_from(ui, cx, &format!("{path}/beam_tip"), labels::BEAM_TIP_LIGHT, now, on, max)
        {
            set = Some(BeamLookSource { tip_light: l, ..b.clone() });
        }
        if let Some(h) = beam_heat(ui, cx, &format!("{path}/beam_heat"), b.heat.as_ref()) {
            set = Some(BeamLookSource { heat: h, ..b.clone() });
        }
        if let Some(s) = set {
            let beam = if s == BeamLookSource::default() { None } else { Some(s) };
            new = Some(WeaponLookSource { beam, ..look.clone() });
        }
    }
    if let Some(n) = new {
        let value = if n == WeaponLookSource::default() { None } else { Some(n) };
        cx.commits.push(ModAction::Set { path: format!("{path}.look"), value: ron(&value) });
    }
}
