//! The egui overlay at the display's resolution (spec §19 E10): worm
//! names (the player's current weapon in its place while changing it),
//! damage popups, the HUD (`rl_ui::hud`, from `hud::view`), the
//! command line's results and the F2 debug panel. It only paints; it takes
//! no input from the game.

use egui::{Align2, Color32, FontId, Painter, Pos2};
use rl_math::Vec2;
use rl_scene::View;

use crate::session::{Driver, POPUP_TICKS, Session};

/// Maps world pixels to the overlay's points: the view rectangle is drawn
/// into `dest` (output pixels, `rl_render::dest_rect`).
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
    pub alpha: f32,
    pub map: ScreenMap,
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
    /// The change-weapon key is held now (the input's, not a tick's):
    /// the player's worm shows its weapon (`Session::weapon_name`).
    pub changing: bool,
    /// Where the HUD goes on a window wider than 16:9 (the options').
    pub arrangement: rl_ui::HudArrangement,
    /// F3's network overlay while an online match plays (spec §7.3).
    pub net: Option<crate::net::NetView>,
    /// The host says this client's inputs are late (spec §3.2): the
    /// warning at the top.
    pub lagging: bool,
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

/// The label over worm `i`, named `name`: the player's weapon (`weapon`,
/// from `Session::weapon_name`) in its name's place while it shows, in
/// OLX's colour, else the name. OLX draws no name over a local human
/// worm and the weapon name 15 world pixels above it (`y-30` at double
/// resolution), where the name label is here.
///
/// OLX: src/common/CWorm.cpp:1304-1331
pub fn worm_label<'a>(i: usize, name: &'a str, weapon: Option<&'a (usize, String)>) -> (&'a str, Color32) {
    match weapon {
        Some((w, text)) if *w == i => (text, WEAPON_NAME_COLOUR),
        _ => (name, NAME_COLOUR),
    }
}

fn shadowed(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, colour: Color32) {
    p.text(pos + egui::vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, colour);
}

/// Paints the overlay into `ui` (the whole window).
pub fn paint(ui: &egui::Ui, o: &Overlay) {
    // Names and popups stay on the picture, never on the bars around it.
    let world_p = ui.painter().with_clip_rect(o.map.points_rect());
    let p = &world_p;
    let s = o.session;
    let small = FontId::proportional(13.0);
    let k = o.map.scale();
    let worms = crate::hud::worms_on_screen(s, &o.map, o.alpha);
    if o.names {
        let bounds = o.map.points_rect().intersect(ui.max_rect());
        let weapon = s.weapon_name(o.changing);
        let ids: Vec<i32> = s.with_world(|w| {
            worms.iter().map(|&(i, _)| w.worms().get(i).map_or(i as i32, |w| w.id)).collect()
        });
        for (&(i, at), &id) in worms.iter().zip(&ids) {
            let name = s.names.of(id);
            let (text, colour) = worm_label(i, &name, weapon.as_ref());
            let galley = p.layout_no_wrap(text.to_string(), small.clone(), colour);
            let r = name_rect(at - egui::vec2(0.0, 12.0 * k), galley.size(), bounds);
            p.galley_with_override_text_color(
                r.min + egui::vec2(1.0, 1.0),
                galley.clone(),
                Color32::from_black_alpha(200),
            );
            p.galley(r.min, galley, colour);
        }
    }
    for pop in &s.popups {
        let t = pop.age as f32 / POPUP_TICKS as f32;
        let at = o.map.to_points(pop.pos) - egui::vec2(0.0, (20.0 + 20.0 * t) * k);
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
    let frame = crate::hud::Frame {
        scores: o.show_scores,
        minimap: o.minimap,
        time: o.time,
        menus: o.menus,
        speed: o.speed.as_deref(),
        selecting: o.selecting,
        arrangement: o.arrangement,
    };
    let hud = crate::hud::view(s, &frame);
    rl_ui::hud::show(ui, &hud);
    if o.debug {
        debug(ui.painter(), ui.max_rect(), o);
    }
    if let Some(net) = &o.net {
        network(ui.painter(), o.map.points_rect().intersect(ui.max_rect()), net);
    }
    if o.lagging {
        let r = o.map.points_rect();
        shadowed(
            ui.painter(),
            r.center_top() + egui::vec2(0.0, 28.0),
            Align2::CENTER_TOP,
            CONNECTION_PROBLEM,
            FontId::proportional(20.0),
            Color32::from_rgb(255, 90, 60),
        );
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
        rl_ui::screens::results::panel(ui.ctx(), r, |ui| {
            ui.label(egui::RichText::new(keys).color(Color32::from_gray(200)));
        });
    }
}

/// The warning while the host fills this client's late inputs (spec §3.2).
pub const CONNECTION_PROBLEM: &str = "Connection problem";

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

    /// While the player changes weapons its worm's label is the weapon,
    /// in OLX's white; every other worm keeps its name.
    #[test]
    fn the_players_label_is_its_weapon_while_changing() {
        let weapon = (2, "Big Nuke".to_string());
        assert_eq!(worm_label(2, "You", Some(&weapon)), ("Big Nuke", WEAPON_NAME_COLOUR));
        assert_eq!(worm_label(0, "Bot 1", Some(&weapon)), ("Bot 1", NAME_COLOUR));
        assert_eq!(worm_label(2, "You", None), ("You", NAME_COLOUR));
        assert_ne!(WEAPON_NAME_COLOUR, NAME_COLOUR, "told apart from a name");
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
