//! The match HUD's inputs (spec §8.3, §21 G5): Tab's scoreboard key, the
//! minimap's texture, and the [`HudView`] a session shows this frame; the
//! HUD itself is `olr_ui::hud` (layout and painting, Modern style).

use egui::{Color32, Pos2, TextureHandle, TextureOptions, pos2, vec2};
use olr_scene::Blend;
use olr_ui::hud::{Dot, HudView, MinimapView, Notice, WormHud, minimap};

use crate::overlay::ScreenMap;
use crate::session::{Driver, Session};

/// The results show this many ticks after the end (`GAMEOVER_WAIT`, 3 s,
/// `CClient_Draw.cpp:893-905`); "Game Over" shows until then.
pub const RESULTS_AFTER: u64 = 300;

/// The minimap's picture is made again at most this often (ticks): the
/// terrain changes it (OLX updates the edited area at once; 10 Hz is as
/// good to the eye).
pub const MINIMAP_TICKS: u64 = 10;

/// OLX's `ShowScore`: the scoreboard shows while Tab is held. A window
/// that loses focus with Tab held never gets the release, so losing focus
/// hides it, as `olr-input` releases the game's keys then.
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
/// (`olr_ui::hud::minimap::image`) when it is new, after a restart and
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

/// What a frame adds to the session for the HUD.
#[derive(Clone, Debug, Default)]
pub struct Frame<'a> {
    /// Tab held: the scoreboard.
    pub scores: bool,
    /// The minimap's texture.
    pub minimap: Option<egui::TextureId>,
    /// Seconds since the start (OLX's blink).
    pub time: f64,
    /// Started from the menus: Esc pauses rather than quits.
    pub menus: bool,
    /// The level editor's Play: Esc stops it (back to the editor).
    pub edited: bool,
    /// The fast-forward of a watched game (a replay, bots alone;
    /// `speed::shown`).
    pub speed: Option<&'a str>,
    /// The weapon selection is open: the world's weapons are a
    /// placeholder until Done, so the HUD shows none.
    pub selecting: bool,
    /// Where the HUD goes on a window wider than 16:9 (the options').
    pub arrangement: olr_ui::HudArrangement,
}

/// The alive worms of `s` on screen (`map`), each with its index, drawn
/// at `alpha` between the last two ticks (where the scene draws them):
/// their names go there.
pub fn worms_on_screen(s: &Session, map: &ScreenMap, alpha: f32) -> Vec<(usize, Pos2)> {
    let blend = Blend::new(s.present.scene.prev(), alpha, s.present.scene.options.interpolation);
    s.with_world(|world| {
        world
            .worms()
            .iter()
            .enumerate()
            .filter(|(_, w)| w.alive)
            .map(|(i, w)| (i, map.to_points(blend.worm(i, w))))
            .collect()
    })
}

/// What the HUD shows of `s` with frame `f`: the followed worm (the
/// player's own, `Session::local`, or the one watched; no weapons during
/// the selection), the event log, the clock and kill limit, a replay's
/// progress, the fast-forward speed, the scoreboard while `scores` (the
/// followed worm's row highlighted), the minimap (the player's dot the
/// big one; "Game Over" until the results show) and the keys. An online
/// match's lives, kills, deaths and scoreboard are its confirmed tick's
/// (spec §3.4); its health and weapons the newest predicted tick's.
pub fn view(s: &Session, f: &Frame) -> HudView {
    let Frame { scores, minimap, time, menus, edited, speed, .. } = *f;
    let followed = s.followed();
    let local = s.local().map(i32::from);
    // The world read once, under an online peer's lock.
    let (mut worm, map, dots) = s.with_world(|world| {
        let worm = usize::try_from(followed).ok().and_then(|i| WormHud::of(world, i, &s.names));
        let t = world.terrain();
        let map = vec2(t.width() as f32, t.height() as f32);
        let dots: Vec<Dot> = minimap
            .map(|_| {
                world
                    .worms()
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| w.alive)
                    .map(|(i, w)| {
                        let [r, g, b] = s.colour(i);
                        Dot {
                            pos: pos2(w.pos.x, w.pos.y),
                            colour: Color32::from_rgb(r, g, b),
                            big: local.unwrap_or(followed) == i as i32,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        (worm, map, dots)
    });
    if let Some(w) = worm.as_mut().filter(|_| f.selecting) {
        w.slots.clear();
    }
    // The deaths and suicides are the board's (every one, not the worm's
    // score-gated counters: `docs/porting/STATUS.md`); online, the lives
    // and kills too (the confirmed tick's).
    if let Some(w) = worm.as_mut()
        && let Some(c) = s.scores().iter().find(|c| c.worm == followed)
    {
        (w.deaths, w.suicides) = (c.deaths, c.suicides);
        if let Driver::Online(_) = &s.driver {
            (w.lives, w.kills) = (c.lives, c.kills);
        }
    }
    let mut notices = Vec::new();
    if let Some(speed) = speed.filter(|_| !s.human()) {
        notices.push(Notice { text: format!("Fast-forward {speed}"), warn: false });
    }
    if s.spectating() {
        notices.push(Notice { text: format!("Watching {}", s.names.of(followed)), warn: false });
    }
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
    let minimap = minimap.map(|texture| MinimapView {
        texture,
        map,
        dots,
        blink: (time % f64::from(minimap::BLINK_PERIOD)) as f32,
    });
    let hint = match s.driver {
        Driver::Sandbox => "Shift+arrows weapon   F2 debug   F5 restart   Esc quit",
        Driver::Match(_) if s.human() => {
            "Shift+arrows weapon   Tab scores   F2 debug   F5 rematch   Esc quit"
        }
        Driver::Match(_) => "Arrows follow   PgUp/PgDn speed   Tab scores   F2 debug   F5 rematch   Esc quit",
        Driver::Replay(_) => {
            "Arrows follow   PgUp/PgDn speed   Tab scores   F2 debug   F5 from the start   Esc quit"
        }
        Driver::Online(_) if local.is_some() => "Shift+arrows weapon   Tab scores   F2 debug   Esc quit",
        Driver::Online(_) => "Arrows follow   Tab scores   F2 debug   Esc quit",
    };
    let hint = match (edited, menus) {
        (true, _) => hint.replace("Esc quit", "Esc stop"),
        (false, true) => hint.replace("Esc quit", "Esc pause"),
        (false, false) => hint.to_string(),
    };
    HudView {
        worm,
        log: s.log.lines().cloned().collect(),
        time_left_ms: s.time_left_ms,
        kill_limit: s.kill_limit(),
        notices,
        scoreboard: (scores && s.results.is_none())
            .then(|| olr_ui::hud::rows(s.scores(), &s.names, Some(followed))),
        minimap,
        game_over: s.results.is_some() && !s.results_shown(),
        hint: Some(hint),
        arrangement: f.arrangement,
    }
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
