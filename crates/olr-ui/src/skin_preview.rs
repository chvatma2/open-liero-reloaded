//! The profiles' live skin preview (plan 7 task 3): every frame of a skin
//! in a profile's colour, faced right and left, drawn with the game's own
//! colorization and frame layout (`olr_content::LoadedSkin::colorized`,
//! `olr_content::skin_sprite`), and OLX's player-menu animation over them.
//!
//! This crate holds no GPU handle (plan 7 ruling Q2): the profiles screen
//! asks for a [`PreviewKey`] (`MenuData::preview_wanted`), the app makes
//! the [`frames`], uploads them as egui textures and hands back their ids
//! ([`PreviewTextures`] in `MenuData::preview`).

use egui::TextureId;
use olr_content::{FRAME_WIDTH, LoadedSkin};
use olr_data::image::{Rgba, RgbaImage};
use olr_sim::Dir;

/// What a preview shows: a skin (an id or OLX name, as a profile holds it)
/// in a colour.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PreviewKey {
    pub skin_id: String,
    pub colour: [u8; 3],
}

/// Every frame's sprite of a colorized sheet, faced right and left.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewFrames {
    right: Vec<RgbaImage>,
    left: Vec<RgbaImage>,
}

impl PreviewFrames {
    /// The number of frames (OLX's sheets have 21).
    pub fn len(&self) -> usize {
        self.right.len()
    }

    pub fn is_empty(&self) -> bool {
        self.right.is_empty()
    }

    /// Frame `f` (clamped to the last) facing `face`.
    pub fn get(&self, f: u32, face: Dir) -> &RgbaImage {
        let v = match face {
            Dir::Right => &self.right,
            Dir::Left => &self.left,
        };
        &v[(f as usize).min(v.len() - 1)]
    }
}

/// `skin`'s frames in `colour`: the sheet colorized as the game colorizes
/// a worm's, each frame cut and mirrored as the game draws it.
pub fn frames(skin: &LoadedSkin, colour: [u8; 3]) -> PreviewFrames {
    let sheet = skin.colorized(Rgba::rgb(colour[0], colour[1], colour[2]));
    let n = (sheet.width() / FRAME_WIDTH).max(1);
    let cut = |face| (0..n).map(|f| olr_content::skin_sprite(&sheet, f, face)).collect();
    PreviewFrames { right: cut(Dir::Right), left: cut(Dir::Left) }
}

/// The textures the app uploaded for [`PreviewKey`] `key`: frame `f`
/// facing right is `right[f]`.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewTextures {
    pub key: PreviewKey,
    pub right: Vec<TextureId>,
    pub left: Vec<TextureId>,
}

/// OLX's player-menu animation: the worm walks (7.5 frames a second over
/// three walk frames) while its aim sweeps between 60° down and 90° up
/// at 200° a second.
///
/// OLX: src/client/DeprecatedGUI/Menu_Player.cpp:746, :756-774
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewAnim {
    /// Degrees, −90 up to 60 down.
    angle: f32,
    /// The walk frame, 0 to 3.
    walk: f32,
    /// OLX's `bAimingUp` (the angle grows, which aims down).
    up: bool,
}

impl Default for PreviewAnim {
    /// OLX's start: level aim, the first walk frame.
    fn default() -> Self {
        PreviewAnim { angle: 0.0, walk: 0.0, up: true }
    }
}

impl PreviewAnim {
    /// Advances by `dt` seconds (at most a quarter second, so a stalled
    /// frame does not jump).
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        self.angle += if self.up { dt * 200.0 } else { -dt * 200.0 };
        self.walk += dt * 7.5;
        if self.angle >= 60.0 {
            self.angle = 60.0;
            self.up = !self.up;
        } else if self.angle <= -90.0 {
            self.angle = -90.0;
            self.up = !self.up;
        }
        if self.walk >= 3.0 {
            self.walk = 0.0;
        }
    }

    /// The sheet frame shown: `walk * 7 + angle / 151 * 7 + 4`, OLX's
    /// menu formula (it truncates toward zero, unlike the game's).
    pub fn frame(&self) -> u32 {
        let f = (self.walk as i32) * 7 + (self.angle / 151.0 * 7.0) as i32 + 4;
        f.max(0) as u32
    }
}
