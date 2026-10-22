//! The egui overlay at the display's resolution (spec §19 E10): worm
//! names (each local player's current weapon in its place while changing
//! it, and its reload bar),
//! damage popups, the mod editor's practice's last hit (issue #20), the
//! HUD (`olr_ui::hud`, from `hud::view`), the command line's results and
//! the F2 debug panel. It only paints; it takes
//! no input from the game.

use egui::{Align2, Color32, FontId, Painter, Pos2};
use olr_math::Vec2;
use olr_scene::View;

use crate::session::{Driver, Hit, POPUP_TICKS, Session};

/// Maps world pixels to the overlay's points: the view rectangle is drawn
/// into `dest` (output pixels, `olr_render::dest_rect`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenMap {
    pub view: View,
    pub dest: [f32; 4],
    pub pixels_per_point: f32,
}

impl ScreenMap {
    pub fn to_points(&self, p: Vec2) -> Pos2 {
        let k = self.dest[2] / self.view.w.max(1.0);
        let ppp = self.pixels_per_point.max(0.01);
        Pos2::new(
            (self.dest[0] + (p.x - self.view.x) * k) / ppp,
            (self.dest[1] + (p.y - self.view.y) * k) / ppp,
        )
    }

    /// The picture (`dest`) in points.
    pub fn points_rect(&self) -> egui::Rect {
        let ppp = self.pixels_per_point.max(0.01);
        let [x, y, w, h] = self.dest.map(|v| v / ppp);
        egui::Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, h))
    }

    /// Points per world pixel.
    pub fn scale(&self) -> f32 {
        self.dest[2] / self.view.w.max(1.0) / self.pixels_per_point.max(0.01)
    }
}

/// The debug counters (F2), measured by the loop.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub fps: f32,
    /// The last second's mean and worst frame time.
    pub frame_ms: f32,
    pub frame_ms_max: f32,
    /// The last second's mean and worst tick time (simulation, entities,
    /// sound, scene, terrain upload).
    pub tick_ms: f32,
    pub tick_ms_max: f32,
    /// The last second's mean time a frame kept the CPU busy: the frame
    /// without waiting for the surface (VSync). `1000 / busy_ms` is the
    /// frame rate the game could reach uncapped (the GPU permitting).
    pub busy_ms: f32,
    pub ticks_per_second: u32,
    /// Ticks the catch-up cap dropped since the start.
    pub dropped: u64,
    /// The adapter, backend and present mode.
    pub gpu: String,
    pub audio: String,
}

/// What the overlay shows this frame.
pub struct Overlay<'a> {
    pub session: &'a Session,
    /// The level editor's Play (its HUD's keys, the Stop button).
    pub edited: bool,
    pub alpha: f32,
    /// Each view's screen map, in view order (one, or split screen's,
    /// issue #30); never empty.
    pub maps: &'a [ScreenMap],
    pub debug: bool,
    /// Tab held: the scoreboard.
    pub show_scores: bool,
    pub stats: &'a Stats,
    /// Started from the menus: Esc pauses (the pause menu leaves), it
    /// does not quit; the results are the menus' screen.
    pub menus: bool,
    /// The minimap's texture (`hud::MinimapTexture`).
    pub minimap: Option<egui::TextureId>,
    /// Seconds since the start (the minimap's blink).
    pub time: f64,
    /// A watched game running fast: its speed as the HUD shows it
    /// (`speed::shown`); `None` at 1×.
    pub speed: Option<String>,
    /// The weapon selection is open (`Shell::selecting`).
    pub selecting: bool,
    /// Worm names show: not through the menus' panels (`Shell::names_shown`).
    pub names: bool,
    /// Each local player's change-weapon key is held now (its input's,
    /// not a tick's), in local order: that player's worm shows its weapon
    /// (`Session::weapon_name_of`; issue #29).
    pub changing: [bool; crate::setup::MAX_LOCALS],
    /// Where the HUD goes on a window wider than 16:9 (the options').
    pub arrangement: olr_ui::HudArrangement,
    /// F3's network overlay while an online match plays (spec §7.3).
    pub net: Option<crate::net::NetView>,
    /// The host says this client's inputs are late (spec §3.2): the
    /// warning at the top.
    pub lagging: bool,
    /// The host is gone and the match is moving (plan 12c,
    /// `Shell::migration_line`): said at the top instead.
    pub migrating: Option<String>,
    /// Mouse controls' cursor (issue #24, `pointer.rs`), in points, while
    /// the game has the pointer: the target marker.
    pub marker: Option<Pos2>,
}

impl Overlay<'_> {
    /// The first view's screen map (the first player's: mouse controls'
    /// marker, the top lines, F3's numbers).
    pub fn map(&self) -> ScreenMap {
        self.maps.first().copied().unwrap_or(ScreenMap {
            view: View { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            dest: [0.0; 4],
            pixels_per_point: 1.0,
        })
    }
}

/// Mouse controls' target marker (issue #24) at `at`: a small ring with a
/// dot, outlined so it shows on any ground.
pub fn target_marker(p: &Painter, at: Pos2, k: f32) {
    let r = (2.5 * k).clamp(4.0, 12.0);
    let dark = egui::Stroke::new(3.0, Color32::from_black_alpha(160));
    let light = egui::Stroke::new(1.5, Color32::from_gray(240));
    p.circle_stroke(at, r, dark);
    p.circle_stroke(at, r, light);
    p.circle_filled(at, 1.5, Color32::from_gray(240));
}

/// Where a name label of `size` goes for a worm at `at` (points): centred
/// above it, its bottom at `at`, moved inside `bounds` (the picture) when
/// it would leave them; one wider or taller than `bounds` starts at their
/// left or top.
pub fn name_rect(at: Pos2, size: egui::Vec2, bounds: egui::Rect) -> egui::Rect {
    let fit = |v: f32, lo: f32, hi: f32, len: f32| {
        let v = if v.is_finite() { v } else { lo };
        if !(lo.is_finite() && hi.is_finite()) || hi - len < lo { lo } else { v.clamp(lo, hi - len) }
    };
    let x = fit(at.x - size.x / 2.0, bounds.left(), bounds.right(), size.x);
    let y = fit(at.y - size.y, bounds.top(), bounds.bottom(), size.y);
    egui::Rect::from_min_size(Pos2::new(x, y), size)
}

/// OLX's `clPlayerName` (`data/frontend/colours.cfg`: `#FFFFFF`), the
/// weapon name's colour.
pub const WEAPON_NAME_COLOUR: Color32 = Color32::WHITE;

/// The worm names' colour.
const NAME_COLOUR: Color32 = Color32::from_gray(230);

/// The label over worm `i`, named `name`, when it has one: a local
/// player's weapon (`weapons`, the worms showing theirs, from
/// `Session::weapon_name_of`) while it shows, in OLX's colour; else the
/// name, except over a local player's own worm (`locals`, every local
/// player's: issue #29): OLX draws no name over a local human worm, and
/// the weapon name 15 world pixels above it (`y-30` at double resolution),
/// where the name label is here.
///
/// OLX: src/common/CWorm.cpp:1304-1331
pub fn worm_label<'a>(
    i: usize,
    locals: &[usize],
    name: &'a str,
    weapons: &'a [(usize, String)],
) -> Option<(&'a str, Color32)> {
    match weapons.iter().find(|(w, _)| *w == i) {
        Some((_, text)) => Some((text, WEAPON_NAME_COLOUR)),
        None if locals.contains(&i) => None,
        None => Some((name, NAME_COLOUR)),
    }
}

/// The reload bar's width in world pixels (it scales with the view, as
/// the worms do).
pub const RELOAD_WIDTH: f32 = 14.0;

/// The reload bar's height in world pixels, at least 3 points.
const RELOAD_HEIGHT: f32 = 1.5;

/// The reload bar's fill: OLX's reloading red (`(128, 64, 64)`,
/// `CClient_Draw.cpp:1221-1257`) brightened to read over the map.
const RELOAD_COLOUR: Color32 = Color32::from_rgb(230, 90, 80);

/// The fraction reloaded of the current weapon, `slots[current]`, while it
/// reloads (its charge); `None` when it is ready or there is no such slot.
pub fn reload_fraction(slots: &[olr_sim::Slot], current: i32) -> Option<f32> {
    let slot = slots.get(usize::try_from(current).ok()?)?;
    slot.reloading.then_some(slot.charge)
}

/// The reload bar under a name label `name` (points), at `k` points per
/// world pixel: its background, `RELOAD_WIDTH` world pixels wide and
/// centred under the label, and its fill to `fraction` (clamped; not a
/// number is empty).
pub fn reload_bar(name: egui::Rect, k: f32, fraction: f32) -> (egui::Rect, egui::Rect) {
    let (width, height) = (RELOAD_WIDTH * k, (RELOAD_HEIGHT * k).max(3.0));
    let back = egui::Rect::from_min_size(
        Pos2::new(name.center().x - width / 2.0, name.bottom() + 1.0),
        egui::vec2(width, height),
    );
    let f = if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) };
    (back, egui::Rect::from_min_size(back.min, egui::vec2(width * f, height)))
}

/// A damage as the practice shows it: at most two decimals, none when
/// whole (`25`, `12.5`).
pub fn damage_text(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The practice's last hit (issue #20) on worm `name`: its damage, and the
/// health it took when that was less (`Last hit: 25 on Bot 1 (took 10)`).
pub fn last_hit_text(hit: &Hit, name: &str) -> String {
    let mut text = format!("Last hit: {} on {name}", damage_text(hit.amount));
    if hit.dealt < hit.amount {
        text += &format!(" (took {})", damage_text(hit.dealt));
    }
    text
}

/// The practice's last hit's colour: the popups' red, lightened.
const LAST_HIT_COLOUR: Color32 = Color32::from_rgb(255, 170, 140);

fn shadowed(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, colour: Color32) {
    p.text(pos + egui::vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, colour);
}

/// What every view's labels show: the local players' worms (no name of
/// their own), their weapon names while they show, their reloads.
struct Labels {
    locals: Vec<usize>,
    weapons: Vec<(usize, String)>,
    reloads: Vec<(usize, f32)>,
}

impl Labels {
    fn of(o: &Overlay) -> Labels {
        let s = o.session;
        // Each local player's weapon name and reload (issue #29: every
        // player's own).
        let locals: Vec<usize> = s.locals().iter().map(|&l| usize::from(l)).collect();
        let weapons: Vec<(usize, String)> =
            (0..locals.len()).filter_map(|k| s.weapon_name_of(k, o.changing.get(k) == Some(&true))).collect();
        // The players' own worms' reload, under their names (the user's
        // request: only one's own; the others' stays hidden, as in OLX).
        let reloads: Vec<(usize, f32)> = s.with_world(|w| {
            let reload = |i: usize| {
                let worm = w.worms().get(i)?;
                Some((i, reload_fraction(&worm.slots, worm.current_weapon)?))
            };
            locals.iter().filter_map(|&i| reload(i)).collect()
        });
        Labels { locals, weapons, reloads }
    }
}

/// One view's worm names, reload bars and damage popups, on its own
/// picture (`map`; split screen draws them in every view, issue #30).
fn view_labels(ui: &egui::Ui, o: &Overlay, map: &ScreenMap, labels: Option<&Labels>) {
    // Names and popups stay on the picture, never on the bars around it.
    let world_p = ui.painter().with_clip_rect(map.points_rect());
    let p = &world_p;
    let s = o.session;
    let small = FontId::proportional(13.0);
    let k = map.scale();
    if let Some(Labels { locals, weapons, reloads }) = labels {
        let worms = crate::hud::worms_on_screen(s, map, o.alpha);
        let bounds = map.points_rect().intersect(ui.max_rect());
        let ids: Vec<i32> = s.with_world(|w| {
            worms.iter().map(|&(i, _)| w.worms().get(i).map_or(i as i32, |w| w.id)).collect()
        });
        for (&(i, at), &id) in worms.iter().zip(&ids) {
            let name = s.names.of(id);
            let label = worm_label(i, locals, &name, weapons);
            // Without a label (the player's own worm), the reload bar keeps
            // the place the name's label would take.
            let (text, colour) = label.unwrap_or((&name, NAME_COLOUR));
            let galley = p.layout_no_wrap(text.to_string(), small.clone(), colour);
            let r = name_rect(at - egui::vec2(0.0, 12.0 * k), galley.size(), bounds);
            if label.is_some() {
                p.galley_with_override_text_color(
                    r.min + egui::vec2(1.0, 1.0),
                    galley.clone(),
                    Color32::from_black_alpha(200),
                );
                p.galley(r.min, galley, colour);
            }
            if let Some(&(_, f)) = reloads.iter().find(|(j, _)| *j == i) {
                let (back, fill) = reload_bar(r, k, f);
                p.rect_filled(back, 1.0, Color32::from_black_alpha(170));
                p.rect_filled(fill, 1.0, RELOAD_COLOUR);
                let edge = egui::Stroke::new(1.0, Color32::from_white_alpha(150));
                p.rect_stroke(back.expand(0.5), 1.0, edge, egui::StrokeKind::Outside);
            }
        }
    }
    for pop in &s.popups {
        let t = pop.age as f32 / POPUP_TICKS as f32;
        let at = map.to_points(pop.pos) - egui::vec2(0.0, (20.0 + 20.0 * t) * k);
        let fade = (255.0 * (1.0 - t)) as u8;
        let text = format!("-{}", pop.amount.round() as i32);
        shadowed(
            p,
            at,
            Align2::CENTER_BOTTOM,
            &text,
            FontId::proportional(15.0),
            Color32::from_rgba_unmultiplied(255, 80, 60, fade),
        );
    }
}

/// Paints the overlay into `ui` (the whole window).
pub fn paint(ui: &egui::Ui, o: &Overlay) {
    let s = o.session;
    let map = o.map();
    if let Some(at) = o.marker {
        target_marker(&ui.painter().with_clip_rect(map.points_rect()), at, map.scale());
    }
    let labels = o.names.then(|| Labels::of(o));
    for m in o.maps {
        view_labels(ui, o, m, labels.as_ref());
    }
    let p = &ui.painter().with_clip_rect(map.points_rect());
    // The practice's last hit, at the picture's top centre (the HUD's keys
    // and F2's counters take the corners, the Stop button the bottom left).
    if let Some(hit) = s.last_hit() {
        let r = map.points_rect();
        let text = last_hit_text(&hit, &s.names.of(hit.worm));
        let at = egui::pos2(r.center().x, r.top() + 8.0);
        shadowed(p, at, Align2::CENTER_TOP, &text, FontId::proportional(16.0), LAST_HIT_COLOUR);
    }
    let frame = crate::hud::Frame {
        scores: o.show_scores,
        minimap: o.minimap,
        time: o.time,
        menus: o.menus,
        edited: o.edited,
        speed: o.speed.as_deref(),
        selecting: o.selecting,
        arrangement: o.arrangement,
    };
    let hud = crate::hud::view(s, &frame);
    olr_ui::hud::show(ui, &hud);
    if o.debug {
        debug(ui.painter(), ui.max_rect(), o);
    }
    if let Some(net) = &o.net {
        network(ui.painter(), map.points_rect().intersect(ui.max_rect()), net);
    }
    if let Some((text, colour)) = top_line(o.migrating.as_deref(), o.lagging) {
        let r = map.points_rect();
        // A long line (a long name) is cut to the picture, never past it.
        let font = FontId::proportional(20.0);
        let p = ui.painter().with_clip_rect(r);
        let galley = p.layout(text.to_string(), font, colour, (r.width() - 16.0).max(40.0));
        let at = egui::pos2(r.center().x - galley.size().x / 2.0, r.top() + 28.0);
        p.galley_with_override_text_color(
            at + egui::vec2(1.0, 1.0),
            galley.clone(),
            Color32::from_black_alpha(200),
        );
        p.galley(at, galley, colour);
    }
    // With the menus, the results are their screen (buttons); without,
    // OLX's keys under them. OLX shows "Game Over" for `GAMEOVER_WAIT`
    // first; a replay that ends sooner shows them at its end.
    if let Some(r) = s.results.as_ref().filter(|_| s.results_shown() && !o.menus) {
        let keys = if matches!(s.driver, Driver::Replay(_)) {
            "F5 watch again    Esc quit"
        } else {
            "F5 rematch    Esc quit"
        };
        olr_ui::screens::results::panel(ui.ctx(), r, |ui| {
            ui.label(egui::RichText::new(keys).color(Color32::from_gray(200)));
        });
    }
}

/// The warning while the host fills this client's late inputs (spec §3.2).
pub const CONNECTION_PROBLEM: &str = "Connection problem";

/// A host migration's line (plan 12c): amber, news rather than a fault.
pub const MIGRATING_COLOUR: Color32 = Color32::from_rgb(255, 200, 80);

/// "Connection problem"'s colour: a fault.
pub const LAGGING_COLOUR: Color32 = Color32::from_rgb(255, 90, 60);

/// The editor's Play's Stop button at `bottom_left` (the playfield's
/// corner, in points: the HUD's keys take the top right, F2's counters the
/// top left), as the label "Stop": whether it was clicked (plan 14c R3; Esc
/// stops too).
pub fn stop_button(ui: &egui::Ui, bottom_left: Pos2) -> bool {
    egui::Area::new(egui::Id::new("olr-edited-play-stop"))
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(bottom_left + egui::vec2(8.0, -8.0))
        .show(ui.ctx(), |ui| ui.button(egui::RichText::new("Stop").size(18.0)).on_hover_text("Esc").clicked())
        .inner
}

/// The playfield's top line and its colour: a host migration under way
/// (`migrating`, plan 12c), else "Connection problem" while the host says
/// this client lags, else none.
pub fn top_line(migrating: Option<&str>, lagging: bool) -> Option<(&str, Color32)> {
    match (migrating, lagging) {
        (Some(m), _) => Some((m, MIGRATING_COLOUR)),
        (None, true) => Some((CONNECTION_PROBLEM, LAGGING_COLOUR)),
        (None, false) => None,
    }
}

/// F3: the network's numbers (spec §7.3) at the playfield's top right.
fn network(p: &Painter, field: egui::Rect, net: &crate::net::NetView) {
    let lines = net.lines();
    let font = FontId::monospace(13.0);
    let (h, w) = (16.0, 380.0);
    let at = field.right_top() + egui::vec2(-w - 10.0, 10.0);
    p.rect_filled(
        egui::Rect::from_min_size(
            at - egui::vec2(6.0, 4.0),
            egui::vec2(w + 6.0, h * lines.len() as f32 + 8.0),
        ),
        4.0,
        Color32::from_black_alpha(150),
    );
    for (i, l) in lines.iter().enumerate() {
        p.text(
            at + egui::vec2(0.0, h * i as f32),
            Align2::LEFT_TOP,
            l,
            font.clone(),
            Color32::from_rgb(180, 220, 255),
        );
    }
}

/// F2: frame and tick timing, counts, the GPU and the sound device.
fn debug(p: &Painter, rect: egui::Rect, o: &Overlay) {
    let s = o.session;
    let st = o.stats;
    let lines = [
        format!(
            "{:.0} fps   frame {:.2} ms (max {:.2})   busy {:.2} ms",
            st.fps, st.frame_ms, st.frame_ms_max, st.busy_ms
        ),
        format!(
            "tick {:.3} ms (max {:.3})   {} ticks/s   dropped {}",
            st.tick_ms, st.tick_ms_max, st.ticks_per_second, st.dropped
        ),
        format!(
            "entities {}   projectiles {}   worms {}   voices {}",
            s.present.fx.len(),
            s.with_world(|w| w.projectiles().count()),
            s.with_world(|w| w.worms().iter().filter(|w| w.alive).count()),
            s.audio.playing()
        ),
        format!("{:?} interpolation, alpha {:.2}", s.present.scene.options.interpolation, o.alpha),
        match &s.driver {
            Driver::Match(d) => format!(
                "{} bots think p50 {:.3} ms p99 {:.3} ms (last 10 s), units max {} of {}",
                d.bots.len(),
                d.recent_think_ms(0.5),
                d.recent_think_ms(0.99),
                d.units_max,
                d.budget_min
            ),
            _ => String::new(),
        },
        st.gpu.clone(),
        st.audio.clone(),
    ];
    let font = FontId::monospace(13.0);
    let mut at = rect.left_top() + egui::vec2(10.0, 10.0);
    let h = 16.0;
    let w = 470.0;
    p.rect_filled(
        egui::Rect::from_min_size(at - egui::vec2(6.0, 4.0), egui::vec2(w, h * lines.len() as f32 + 8.0)),
        4.0,
        Color32::from_black_alpha(150),
    );
    for l in &lines {
        p.text(at, Align2::LEFT_TOP, l, font.clone(), Color32::from_rgb(180, 255, 180));
        at.y += h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review minor 7: a migration's line takes the top line, in amber,
    /// over "Connection problem" (a client whose host is gone may still be
    /// flagged lagging); without one, lagging shows in red; else nothing.
    #[test]
    fn the_top_line_is_the_migrations_else_lagging() {
        let moving = "The host is gone: moving the game to Anna…";
        assert_eq!(top_line(Some(moving), true), Some((moving, MIGRATING_COLOUR)));
        assert_eq!(top_line(Some(moving), false), Some((moving, MIGRATING_COLOUR)));
        assert_eq!(top_line(None, true), Some((CONNECTION_PROBLEM, LAGGING_COLOUR)));
        assert_eq!(top_line(None, false), None);
    }

    /// The reload bar sits centred under the name, `RELOAD_WIDTH` world
    /// pixels wide, filled to the fraction (a bad one clamped).
    #[test]
    fn the_reload_bar_sits_under_the_name() {
        let name = egui::Rect::from_min_size(Pos2::new(260.0, 186.0), egui::vec2(80.0, 14.0));
        let (back, fill) = reload_bar(name, 2.0, 0.25);
        assert_eq!(back.center().x, name.center().x);
        assert!(back.top() >= name.bottom() && back.height() > 0.0);
        assert_eq!(back.width(), RELOAD_WIDTH * 2.0);
        assert_eq!((fill.left(), fill.top(), fill.bottom()), (back.left(), back.top(), back.bottom()));
        assert_eq!(fill.width(), back.width() * 0.25);
        for f in [f32::NAN, -1.0, 2.0, f32::INFINITY] {
            let (back, fill) = reload_bar(name, 2.0, f);
            assert!((0.0..=back.width()).contains(&fill.width()), "{f}: {fill:?}");
        }
    }

    /// Only a current weapon that reloads shows a bar, filled to its
    /// charge; a bad index shows none.
    #[test]
    fn only_a_reloading_weapon_shows_a_bar() {
        use olr_sim::Slot;
        let ready = Slot::ready(0);
        let reloading = Slot { charge: 0.4, reloading: true, ..Slot::ready(1) };
        let slots = [ready, reloading];
        assert_eq!(reload_fraction(&slots, 0), None);
        assert_eq!(reload_fraction(&slots, 1), Some(0.4));
        assert_eq!(reload_fraction(&slots, 2), None);
        assert_eq!(reload_fraction(&slots, -1), None);
    }

    /// A worm's name sits centred above it, moved inside the view when
    /// the worm is at its edge (never clipped there).
    #[test]
    fn names_stay_inside_the_view() {
        let view = egui::Rect::from_min_size(Pos2::new(20.0, 0.0), egui::vec2(600.0, 360.0));
        let size = egui::vec2(80.0, 14.0);
        let mid = name_rect(Pos2::new(300.0, 200.0), size, view);
        assert_eq!(mid, egui::Rect::from_min_size(Pos2::new(260.0, 186.0), size), "centred above");
        for at in [
            Pos2::new(21.0, 200.0),
            Pos2::new(619.0, 200.0),
            Pos2::new(300.0, 2.0),
            Pos2::new(-500.0, -40.0),
            Pos2::new(5000.0, 9000.0),
        ] {
            let r = name_rect(at, size, view);
            assert!(view.contains_rect(r), "{at:?}: {r:?}");
            assert_eq!(r.size(), size);
        }
        assert_eq!(name_rect(Pos2::new(21.0, 200.0), size, view).left(), 20.0);
        assert_eq!(name_rect(Pos2::new(619.0, 200.0), size, view).right(), 620.0);
        assert_eq!(name_rect(Pos2::new(300.0, 2.0), size, view).top(), 0.0);
        // Wider than the view: from its left edge.
        let wide = name_rect(Pos2::new(300.0, 200.0), egui::vec2(900.0, 14.0), view);
        assert_eq!(wide.left(), 20.0);
        // No size or not a number: no panic.
        let _ = name_rect(Pos2::new(f32::NAN, 1.0), size, egui::Rect::NOTHING);
    }

    /// The player's own worm shows no name (OLX draws none; the user's
    /// ruling, plan 15a Task 8).
    #[test]
    fn own_worm_has_no_name_label() {
        assert_eq!(worm_label(2, &[2], "You", &[]), None);
        // Issue #29: no local player's worm shows its name.
        assert_eq!(worm_label(0, &[2, 0], "Player 2", &[]), None);
    }

    /// While the player changes weapons its worm's label is the weapon,
    /// in OLX's white.
    #[test]
    fn own_worm_shows_its_weapon_while_changing() {
        let weapon = [(2, "Big Nuke".to_string())];
        assert_eq!(worm_label(2, &[2], "You", &weapon), Some(("Big Nuke", WEAPON_NAME_COLOUR)));
        assert_ne!(WEAPON_NAME_COLOUR, NAME_COLOUR, "told apart from a name");
        // Issue #29: each local player's worm shows its own weapon.
        let both = [(2, "Big Nuke".to_string()), (0, "Minigun".to_string())];
        assert_eq!(worm_label(0, &[2, 0], "Player 2", &both), Some(("Minigun", WEAPON_NAME_COLOUR)));
        assert_eq!(worm_label(2, &[2, 0], "Player 1", &both), Some(("Big Nuke", WEAPON_NAME_COLOUR)));
    }

    /// Every other worm keeps its name, and a game with no local slot (a
    /// replay, bots alone) names every worm.
    #[test]
    fn other_worms_keep_their_names() {
        let weapon = [(2, "Big Nuke".to_string())];
        assert_eq!(worm_label(0, &[2], "Bot 1", &weapon), Some(("Bot 1", NAME_COLOUR)));
        assert_eq!(worm_label(2, &[], "Bot 3", &[]), Some(("Bot 3", NAME_COLOUR)));
    }

    #[test]
    fn world_pixels_map_to_points() {
        // A 400 × 240 view at (100, 50) drawn 3× into 1200 × 720 pixels
        // offset by (40, 0), at 2 pixels per point.
        let m = ScreenMap {
            view: View { x: 100.0, y: 50.0, w: 400.0, h: 240.0 },
            dest: [40.0, 0.0, 1200.0, 720.0],
            pixels_per_point: 2.0,
        };
        assert_eq!(m.to_points(Vec2::new(100.0, 50.0)), Pos2::new(20.0, 0.0));
        assert_eq!(m.to_points(Vec2::new(500.0, 290.0)), Pos2::new(620.0, 360.0));
        assert_eq!(m.scale(), 1.5);
        assert_eq!(
            m.points_rect(),
            egui::Rect::from_min_size(Pos2::new(20.0, 0.0), egui::vec2(600.0, 360.0))
        );
    }
}
