//! The match HUD's inputs (spec §8.3, §21 G5): Tab's scoreboard key, the
//! minimap's texture, and the [`HudView`] a session shows this frame; the
//! HUD itself is `rl_ui::hud` (layout and painting, Modern style).

use egui::{Color32, Rect, TextureHandle, TextureOptions, pos2, vec2};
use rl_ui::hud::{Dot, HudView, MinimapView, Notice, WormHud, minimap};

use crate::session::{Driver, Session};

/// The results show this many ticks after the end (`GAMEOVER_WAIT`, 3 s,
/// `CClient_Draw.cpp:893-905`); "Game Over" shows until then.
pub const RESULTS_AFTER: u64 = 300;

/// The minimap's picture is made again at most this often (ticks): the
/// terrain changes it (OLX updates the edited area at once; 10 Hz is as
/// good to the eye).
pub const MINIMAP_TICKS: u64 = 10;

/// The F2 panel's height: the kill feed starts below it.
const DEBUG_PANEL: f32 = 130.0;

/// OLX's `ShowScore`: the scoreboard shows while Tab is held. A window
/// that loses focus with Tab held never gets the release, so losing focus
/// hides it, as `rl-input` releases the game's keys then.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScoreKey {
    shown: bool,
}

impl ScoreKey {
    /// Tab went down (`true`) or up.
    pub fn key(&mut self, pressed: bool) {
        self.shown = pressed;
    }

    /// The window gained or lost the keyboard focus.
    pub fn focus(&mut self, focused: bool) {
        self.shown &= focused;
    }

    /// A script's `scores on|off`.
    pub fn set(&mut self, on: bool) {
        self.shown = on;
    }

    pub fn shown(self) -> bool {
        self.shown
    }
}

/// The minimap's egui texture, made from the session's colour terrain
/// (`rl_ui::hud::minimap::image`) when it is new, after a restart and
/// every [`MINIMAP_TICKS`].
#[derive(Default)]
pub struct MinimapTexture {
    texture: Option<TextureHandle>,
    /// The session ticks and rematch it was made at.
    made: Option<(u64, u32)>,
}

impl MinimapTexture {
    /// Brings the texture up to date with `s` and gives its id.
    pub fn update(&mut self, ctx: &egui::Context, s: &Session) -> egui::TextureId {
        let now = (s.ticks(), s.rematches);
        let stale = match (self.made, &self.texture) {
            (Some((t, r)), Some(_)) => r != now.1 || now.0 < t || now.0 >= t + MINIMAP_TICKS,
            _ => true,
        };
        if stale {
            let c = s.present.color();
            let img = minimap::image(c.pixels(), c.width(), c.height(), minimap::TEXTURE_SIZE);
            match &mut self.texture {
                Some(t) => t.set(img, TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("minimap", img, TextureOptions::LINEAR)),
            }
            self.made = Some(now);
        }
        self.texture.as_ref().map_or(egui::TextureId::default(), TextureHandle::id)
    }

    /// No game: the texture is freed.
    pub fn clear(&mut self) {
        *self = MinimapTexture::default();
    }
}

/// What the HUD shows of `s`: the followed worm (0), the feed, the clock
/// and kill limit, a replay's progress, the scoreboard while `scores`,
/// the minimap (texture `minimap`; the camera's view `view` in world
/// pixels; `time` in seconds for OLX's blink), "Game Over" until the
/// results show, and the keys (`menus`: Esc pauses rather than quits).
pub fn view(
    s: &Session,
    view: Option<Rect>,
    scores: bool,
    minimap: Option<egui::TextureId>,
    time: f64,
    menus: bool,
    debug: bool,
) -> HudView {
    let replay = matches!(s.driver, Driver::Replay(_));
    let worm = WormHud::of(&s.world, 0, &s.names, !replay);
    let mut notices = Vec::new();
    if let Driver::Replay(d) = &s.driver {
        let (t, n) = (d.cursor.tick(), d.replay.ticks());
        let text = if d.is_done() {
            format!("Replay finished ({:.1} s): F5 plays it again", f64::from(n) / 100.0)
        } else {
            format!("Replay {:.1} s / {:.1} s", f64::from(t) / 100.0, f64::from(n) / 100.0)
        };
        notices.push(Notice { text, warn: false });
        if let Some(m) = d.mismatch {
            let text = format!("diverged from the recording at tick {} (other content or build?)", m.tick);
            notices.push(Notice { text, warn: true });
        }
    }
    let t = s.world.terrain();
    let map = vec2(t.width() as f32, t.height() as f32);
    let minimap = minimap.map(|texture| MinimapView {
        texture,
        map,
        view,
        dots: s
            .world
            .worms()
            .iter()
            .enumerate()
            .filter(|(_, w)| w.alive)
            .map(|(i, w)| {
                let [r, g, b] = s.colour(i);
                Dot {
                    pos: pos2(w.pos.x, w.pos.y),
                    colour: Color32::from_rgb(r, g, b),
                    big: i == 0 && !replay,
                }
            })
            .collect(),
        blink: (time % f64::from(minimap::BLINK_PERIOD)) as f32,
    });
    let hint = match s.driver {
        Driver::Sandbox => "Shift+arrows weapon   F2 debug   F5 restart   Esc quit",
        Driver::Match(_) => "Shift+arrows weapon   Tab scores   F2 debug   F5 rematch   Esc quit",
        Driver::Replay(_) => "Tab scores   F2 debug   F5 from the start   Esc quit",
    };
    let hint = if menus { hint.replace("Esc quit", "Esc pause") } else { hint.to_string() };
    HudView {
        worm,
        feed: s.feed.visible(s.ticks()).map(str::to_string).collect(),
        time_left_ms: s.time_left_ms,
        kill_limit: s.kill_limit(),
        notices,
        scoreboard: (scores && s.results.is_none()).then(|| rl_ui::hud::rows(s.world.scores(), &s.names)),
        minimap,
        game_over: s.results.is_some() && !s.results_shown(),
        hint: Some(hint),
        top_left_reserved: if debug { DEBUG_PANEL } else { 0.0 },
    }
}

/// The camera's view as a world rectangle.
pub fn view_rect(v: rl_scene::View) -> Rect {
    Rect::from_min_size(pos2(v.x, v.y), vec2(v.w, v.h))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tab shows the scoreboard while held; a window that loses focus
    /// with Tab held never gets the release, so it hides the scoreboard.
    #[test]
    fn scoreboard_hides_when_the_window_loses_focus() {
        let mut k = ScoreKey::default();
        assert!(!k.shown());
        k.key(true);
        assert!(k.shown());
        k.focus(false);
        assert!(!k.shown(), "focus lost with Tab held");
        k.focus(true);
        assert!(!k.shown(), "focus back: Tab must be pressed again");
        k.key(true);
        k.key(false);
        assert!(!k.shown());
        k.set(true);
        k.focus(true);
        assert!(k.shown(), "a script's `scores on` stays while focused");
    }
}
