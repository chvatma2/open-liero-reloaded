//! The egui overlay at the display's resolution (spec §19 E10): worm
//! names, damage popups, the sandbox HUD and the F2 debug panel. It only
//! paints; it takes no input from the game.

use egui::{Align2, Color32, FontId, Painter, Pos2};
use rl_math::Vec2;
use rl_scene::{Blend, View};

use crate::session::{POPUP_TICKS, Session};

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
    pub stats: &'a Stats,
}

fn shadowed(p: &Painter, pos: Pos2, anchor: Align2, text: &str, font: FontId, colour: Color32) {
    p.text(pos + egui::vec2(1.0, 1.0), anchor, text, font.clone(), Color32::from_black_alpha(200));
    p.text(pos, anchor, text, font, colour);
}

/// The worm's name over its head: the player is "You", the targets are numbered.
fn name(i: usize) -> String {
    if i == 0 { "You".to_string() } else { format!("Target {i}") }
}

/// Paints the overlay into `ui` (the whole window).
pub fn paint(ui: &egui::Ui, o: &Overlay) {
    // Names and popups stay on the picture, never on the bars around it.
    let world_p = ui.painter().with_clip_rect(o.map.points_rect());
    let p = &world_p;
    let s = o.session;
    let world = &s.world;
    let small = FontId::proportional(13.0);
    let blend = Blend::new(s.present.scene.prev(), o.alpha, s.present.scene.options.interpolation);
    let k = o.map.scale();
    for (i, w) in world.worms().iter().enumerate().filter(|(_, w)| w.alive) {
        let at = o.map.to_points(blend.worm(i, w)) - egui::vec2(0.0, 12.0 * k);
        shadowed(p, at, Align2::CENTER_BOTTOM, &name(i), small.clone(), Color32::from_gray(230));
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
    let p = ui.painter();
    hud(p, ui.max_rect(), s);
    if o.debug {
        debug(p, ui.max_rect(), o);
    }
}

/// The sandbox HUD: the player's health, weapon and score, and the keys.
fn hud(p: &Painter, rect: egui::Rect, s: &Session) {
    let font = FontId::monospace(14.0);
    let Some(me) = s.world.worms().first() else { return };
    let line = if me.alive {
        let slot = usize::try_from(me.current_weapon).ok().and_then(|i| me.slots.get(i));
        let weapon = slot.and_then(|sl| s.world.sim_mod().weapon(sl.weapon));
        let (name, state) = match (weapon, slot) {
            (Some(w), Some(sl)) if sl.reloading => {
                (w.name.as_str(), format!("reloading {:>3.0}%", sl.charge * 100.0))
            }
            (Some(w), Some(sl)) => (w.name.as_str(), format!("{:>3.0}%", sl.charge * 100.0)),
            _ => ("-", String::new()),
        };
        format!(
            "Health {:>3.0}   {} {}   kills {} deaths {} suicides {}",
            me.health.max(0.0),
            name,
            state,
            me.kills,
            me.deaths,
            me.suicides
        )
    } else {
        format!("Dead: Jump to respawn   kills {} deaths {} suicides {}", me.kills, me.deaths, me.suicides)
    };
    let at = rect.left_bottom() + egui::vec2(10.0, -10.0);
    shadowed(p, at, Align2::LEFT_BOTTOM, &line, font.clone(), Color32::WHITE);
    // The keys, where there is room beside the HUD line.
    if rect.width() >= 1100.0 {
        let keys = "arrows move/aim  Ctrl fire  Alt jump  X rope  Shift+arrows weapon  F2 debug  F5 restart  Esc quit";
        let at = rect.right_bottom() + egui::vec2(-10.0, -10.0);
        shadowed(p, at, Align2::RIGHT_BOTTOM, keys, FontId::proportional(12.0), Color32::from_gray(190));
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
            s.world.projectiles().count(),
            s.world.worms().iter().filter(|w| w.alive).count(),
            s.audio.playing()
        ),
        format!("{:?} interpolation, alpha {:.2}", s.present.scene.options.interpolation, o.alpha),
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
