//! The video options applied to the game (plan 7 task 4, spec §21 G3):
//! what the options menu changed, turned into window requests and live
//! settings, the size and mode the window opens with, and the command
//! line's flags on top of the saved options.
//!
//! The window's size and mode change only through `window::Requests`
//! (paced: at most one change every `MIN_CHANGE_INTERVAL`, the latest
//! wins), and only when the preset or fullscreen actually changed:
//! toggling interpolation or the filter never spends a change. Everything
//! else (the UI scale, VSync, the filter, interpolation, the blood amount)
//! applies at once.

use olr_scene::Interpolation;
use olr_ui::{FrameRate, HudArrangement, UpscaleFilter, VideoSettings};

use crate::window::Requests;

/// What a change sets at once, besides the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Live {
    pub ui_scale: f32,
    /// The frame rate changed: uncapped (`true`) or VSync.
    pub uncapped: Option<bool>,
    /// The upscale filter changed.
    pub filter: Option<olr_render::UpscaleFilter>,
}

/// The size (physical pixels) and mode the window opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opening {
    pub size: (u32, u32),
    pub fullscreen: bool,
}

/// The video options the window shows.
#[derive(Clone, Debug)]
pub struct Video {
    shown: VideoSettings,
}

impl Video {
    /// The window shows `shown` (it opened with [`Video::opening`]).
    pub fn new(shown: VideoSettings) -> Video {
        Video { shown }
    }

    /// The window for `v`, made at that size and mode (creating the
    /// window is no resize; fullscreen keeps the size for when it ends).
    pub fn opening(v: &VideoSettings) -> Opening {
        Opening { size: v.window.size(), fullscreen: v.fullscreen }
    }

    /// The options now say `v`: the window's size and mode asked of
    /// `requests` when they changed, the rest returned to apply at once.
    pub fn change(&mut self, v: VideoSettings, requests: &mut Requests) -> Live {
        let old = std::mem::replace(&mut self.shown, v.clone());
        if v.fullscreen != old.fullscreen {
            requests.fullscreen(v.fullscreen);
        }
        // A window's size; fullscreen keeps it for when it ends.
        if !v.fullscreen && (v.window != old.window || old.fullscreen) {
            let (w, h) = v.window.size();
            requests.size(w, h);
        }
        Live {
            ui_scale: v.ui_scale,
            uncapped: (v.frame_rate != old.frame_rate).then_some(v.frame_rate == FrameRate::Uncapped),
            filter: (v.filter != old.filter).then_some(filter(v.filter)),
        }
    }
}

/// The renderer's filter for the options' choice.
pub fn filter(f: UpscaleFilter) -> olr_render::UpscaleFilter {
    match f {
        UpscaleFilter::Nearest => olr_render::UpscaleFilter::IntegerNearest,
        UpscaleFilter::Sharp => olr_render::UpscaleFilter::SharpBilinear,
    }
}

// The options' blood slider ends where the entities' amount does (issue #6;
// the shell hands it to `Presentation::set_blood`).
const _: () = assert!(*olr_ui::BLOOD_RANGE.end() == olr_fx::BLOOD_MAX);

/// The scene's interpolation for the options' choice.
pub fn interpolation(on: bool) -> Interpolation {
    if on { Interpolation::Linear } else { Interpolation::Latest }
}

/// The video flags given on the command line (`--fullscreen`,
/// `--uncapped`, `--filter`, `--no-interp`): they win over the saved
/// options for this run, the options show them, and the file keeps its
/// own values until the player changes that option in the menus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub fullscreen: Option<bool>,
    pub frame_rate: Option<FrameRate>,
    pub filter: Option<UpscaleFilter>,
    pub interpolation: Option<bool>,
    pub hud_layout: Option<HudArrangement>,
}

impl Overrides {
    /// `v` with the flags' values.
    pub fn apply(&self, v: &mut VideoSettings) {
        if let Some(x) = self.fullscreen {
            v.fullscreen = x;
        }
        if let Some(x) = self.frame_rate {
            v.frame_rate = x;
        }
        if let Some(x) = self.filter {
            v.filter = x;
        }
        if let Some(x) = self.interpolation {
            v.interpolation = x;
        }
        if let Some(x) = self.hud_layout {
            v.hud_layout = x;
        }
    }

    /// The options now say `v`: an option set differently from its flag is
    /// the player's from now on (saved as set).
    pub fn follow(&mut self, v: &VideoSettings) {
        fn keep<T: PartialEq>(flag: &mut Option<T>, now: &T) {
            if flag.as_ref().is_some_and(|f| f != now) {
                *flag = None;
            }
        }
        keep(&mut self.fullscreen, &v.fullscreen);
        keep(&mut self.frame_rate, &v.frame_rate);
        keep(&mut self.filter, &v.filter);
        keep(&mut self.interpolation, &v.interpolation);
        keep(&mut self.hud_layout, &v.hud_layout);
    }

    /// `v` as the file keeps it: each option a flag still sets has the
    /// file's value (`file`, as loaded).
    pub fn unsaved(&self, file: &VideoSettings, v: &mut VideoSettings) {
        if self.fullscreen.is_some() {
            v.fullscreen = file.fullscreen;
        }
        if self.frame_rate.is_some() {
            v.frame_rate = file.frame_rate;
        }
        if self.filter.is_some() {
            v.filter = file.filter;
        }
        if self.interpolation.is_some() {
            v.interpolation = file.interpolation;
        }
        if self.hud_layout.is_some() {
            v.hud_layout = file.hud_layout;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_new_preset_or_mode_asks_for_a_window_change() {
        let t = std::time::Instant::now() + crate::window::MIN_CHANGE_INTERVAL;
        let base = VideoSettings::default();
        let mut video = Video::new(base.clone());
        let mut r = Requests::default();
        let live =
            video.change(VideoSettings { interpolation: false, ui_scale: 2.0, ..base.clone() }, &mut r);
        assert_eq!(live, Live { ui_scale: 2.0, uncapped: None, filter: None });
        assert_eq!(r.due(t), None);
        // Fullscreen on: the mode only; off again: the mode and the size.
        let full = VideoSettings { fullscreen: true, ..base.clone() };
        video.change(full.clone(), &mut r);
        let c = r.due(t).unwrap();
        assert_eq!((c.fullscreen, c.size), (Some(true), None));
        video.change(base.clone(), &mut r);
        let c = r.due(t + crate::window::MIN_CHANGE_INTERVAL).unwrap();
        assert_eq!((c.fullscreen, c.size), (Some(false), Some((1280, 720))));
    }
}
