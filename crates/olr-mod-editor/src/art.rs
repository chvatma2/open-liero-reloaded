//! The Art section's image being painted (issue #19): a mod image, a
//! strip of square frames (`frame count = width ÷ height`), with its glow
//! mask, decoded once from the pack and painted with the skin editor's
//! tools (`olr_skin_editor::paint`) frame by frame. A stroke paints this
//! copy; the state writes it back to the pack when the stroke ends (one
//! step), the colour image only when its pixels changed and the mask only
//! when the glow did. Also the live preview's frame and its dark look.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::skin::glow_level;
use olr_edit::mods::art;
use olr_edit::pack::Pack;
use olr_format::image_io::{self, IMAGES};
use olr_format::mods::{AnimMode, Gfx};
use olr_skin_editor::Frame;
pub use olr_skin_editor::Tool;
pub use olr_skin_editor::paint::Target;
pub use olr_skin_editor::state::{GLOW_STRENGTHS, GRID_ZOOM, MAX_RECENT, ZOOMS};

/// The targets the Art section paints: an image has no player colour.
pub const TARGETS: [Target; 2] = [Target::Colour, Target::Glow];
/// The preview's frames a second when the image is not animated.
pub const PREVIEW_FPS: f64 = 10.0;
/// How long the preview takes to turn once through 360° of heading.
pub const PREVIEW_TURN_S: f64 = 3.0;
/// The preview's darkness: a pixel that does not glow draws at this
/// share of its colour, as in a dark level.
pub const PREVIEW_DARK: f32 = 0.25;

/// A mod image being painted.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtDoc {
    path: String,
    /// The strip as the pack holds it (colour and glow; no tint).
    saved: Frame,
    /// The strip as painted.
    strip: Frame,
    /// Why the image's glow mask was set aside (it does not fit), if so.
    mask_note: Option<String>,
}

impl ArtDoc {
    /// Image `path` of `pack`, with its glow (none when its mask does not
    /// fit: the note says so, and painting glow replaces the mask); why
    /// not when it is no image the editor can paint. Never a panic.
    pub fn load(pack: &Pack, path: &str) -> Result<ArtDoc, String> {
        let p = olr_vfs::VPath::new(path).map_err(|e| e.to_string())?;
        let bytes = pack.other.get(&p).ok_or_else(|| format!("no image {path}"))?;
        art::image_frames(bytes).map_err(|e| e.to_string())?;
        let img = image_io::decode_rgba_within(bytes, IMAGES)?;
        let (w, h) = (img.width(), img.height());
        let mut strip = Frame::blank(w, h);
        for y in 0..h {
            for x in 0..w {
                strip.px[(y * w + x) as usize] = img.get(x, y);
            }
        }
        let mask_note = match art::glow(pack, path) {
            Some(g) if g.len() == strip.glow.len() => {
                strip.glow = g;
                None
            }
            _ => Some("its glow mask does not fit the image: painting glow makes a new one".into()),
        };
        Ok(ArtDoc { path: path.to_string(), saved: strip.clone(), strip, mask_note })
    }

    pub fn path(&self) -> &str {
        &self.path
    }
    /// The strip's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.strip.w, self.strip.h)
    }
    /// A frame's side: the strip's height.
    pub fn side(&self) -> u32 {
        self.strip.h
    }
    /// The frame count (at least one: `load` refuses a narrower strip).
    pub fn frames(&self) -> usize {
        (self.strip.w / self.strip.h.max(1)).max(1) as usize
    }
    pub fn mask_note(&self) -> Option<&str> {
        self.mask_note.as_deref()
    }
    /// The whole strip as painted.
    pub fn strip(&self) -> &Frame {
        &self.strip
    }

    /// Frame `f` (clamped to the last), `side` square.
    pub fn frame(&self, f: usize) -> Frame {
        let f = f.min(self.frames() - 1) as u32;
        let s = self.side();
        let mut out = Frame::blank(s, s);
        for y in 0..s {
            for x in 0..s {
                let (i, j) = ((y * s + x) as usize, (y * self.strip.w + f * s + x) as usize);
                out.px[i] = self.strip.px[j];
                out.glow[i] = self.strip.glow[j];
            }
        }
        out
    }

    /// Puts `frame` (`side` square) in place of frame `f`.
    pub fn set_frame(&mut self, f: usize, frame: &Frame) {
        let s = self.side();
        if f >= self.frames() || (frame.w, frame.h) != (s, s) {
            return;
        }
        for y in 0..s {
            for x in 0..s {
                let (i, j) = ((y * s + x) as usize, (y * self.strip.w + f as u32 * s + x) as usize);
                self.strip.px[j] = frame.px[i];
                self.strip.glow[j] = frame.glow[i];
            }
        }
    }

    /// Whether the colour pixels differ from the pack's.
    pub fn colour_changed(&self) -> bool {
        self.strip.px != self.saved.px
    }
    /// Whether the glow differs from the pack's (or its mask was set aside
    /// and something glows now).
    pub fn glow_changed(&self) -> bool {
        self.strip.glow != self.saved.glow
    }
    /// The glow values, row by row.
    pub fn glow(&self) -> &[u8] {
        &self.strip.glow
    }

    /// The colour pixels as an image.
    pub fn image(&self) -> RgbaImage {
        strip_image(&self.strip)
    }
    /// The colour pixels as the PNG the pack gets.
    pub fn png(&self) -> Vec<u8> {
        image_io::encode_rgba(&self.image())
    }

    /// The strip with frame `f` copied in after it (for Add frame): PNG
    /// bytes and glow values, one frame wider.
    pub fn with_frame_added(&self, f: usize) -> (Vec<u8>, Vec<u8>) {
        let s = self.side() as usize;
        let at = (f.min(self.frames() - 1) + 1) * s;
        self.respliced(|row: &mut Vec<usize>| {
            let copy: Vec<usize> = row[at - s..at].to_vec();
            row.splice(at..at, copy);
        })
    }

    /// The strip without frame `f` (for Remove frame); `None` for the
    /// only frame.
    pub fn with_frame_removed(&self, f: usize) -> Option<(Vec<u8>, Vec<u8>)> {
        if self.frames() < 2 {
            return None;
        }
        let s = self.side() as usize;
        let at = f.min(self.frames() - 1) * s;
        Some(self.respliced(|row: &mut Vec<usize>| {
            row.drain(at..at + s);
        }))
    }

    /// A strip whose each row is this one's columns as `edit` lists them.
    fn respliced(&self, edit: impl Fn(&mut Vec<usize>)) -> (Vec<u8>, Vec<u8>) {
        let mut cols: Vec<usize> = (0..self.strip.w as usize).collect();
        edit(&mut cols);
        let (w, h) = (cols.len() as u32, self.strip.h);
        let mut out = Frame::blank(w, h);
        for y in 0..h as usize {
            for (x, &c) in cols.iter().enumerate() {
                let (i, j) = (y * w as usize + x, y * self.strip.w as usize + c);
                out.px[i] = self.strip.px[j];
                out.glow[i] = self.strip.glow[j];
            }
        }
        (image_io::encode_rgba(&strip_image(&out)), out.glow)
    }
}

fn strip_image(f: &Frame) -> RgbaImage {
    let mut img = RgbaImage::new(f.w, f.h);
    for (i, &c) in f.px.iter().enumerate() {
        img.set(i as u32 % f.w, i as u32 / f.w, c);
    }
    img
}

/// `frame` drawn as in a dark level: a pixel that does not glow at
/// [`PREVIEW_DARK`] of its colour, a glowing one at its glow's quarter
/// (as the game draws it, `olr_data::skin::glow_level`) when that is
/// brighter.
pub fn dark(frame: &Frame) -> RgbaImage {
    let mut img = RgbaImage::new(frame.w, frame.h);
    for (i, &c) in frame.px.iter().enumerate() {
        let level = glow_level(frame.glow[i]) as f32 / olr_data::skin::GLOW_LEVELS as f32;
        let k = level.max(PREVIEW_DARK);
        let scale = |v: u8| (f32::from(v) * k).round().min(255.0) as u8;
        img.set(
            i as u32 % frame.w,
            i as u32 / frame.w,
            Rgba { r: scale(c.r), g: scale(c.g), b: scale(c.b), a: c.a },
        );
    }
    img
}

/// The frame the preview shows at `t` seconds for an image of `frames`
/// frames drawn as `gfx` (its first user's, if any): an animation plays
/// at its rate (once and ping-pong as OLX runs them, looping in the
/// preview); frames following the heading turn through it once every
/// [`PREVIEW_TURN_S`]; otherwise every frame in turn at [`PREVIEW_FPS`].
///
/// OLX: src/common/CProjectile.cpp:238-291 (the heading's frame and the
/// strip's wrap); the animation as `olr-sim`'s `proj_sim` steps it, ping-
/// pong turning at either end.
pub fn preview_frame(gfx: Option<&Gfx>, frames: usize, t: f64) -> usize {
    let n = frames.max(1);
    let t = if t.is_finite() { t.max(0.0) } else { 0.0 };
    let wrap = |f: f64| (f.floor() as i64).rem_euclid(n as i64) as usize;
    match gfx {
        Some(Gfx::Image { animation: Some(a), .. }) if a.fps > 0.0 && n > 1 => {
            let step = (t * f64::from(a.fps)).floor() as i64;
            match a.mode {
                AnimMode::PingPong => {
                    let period = 2 * (n as i64 - 1);
                    let p = step.rem_euclid(period);
                    (if p < n as i64 { p } else { period - p }) as usize
                }
                _ => wrap(step as f64),
            }
        }
        Some(Gfx::Image {
            frames_follow_heading, frames_follow_heading_mirrored, heading_frames, ..
        }) if (*frames_follow_heading || *frames_follow_heading_mirrored) && *heading_frames > 0 => {
            let angle = (t / PREVIEW_TURN_S).fract() * 360.0;
            let offset = 360.0 / f64::from(*heading_frames);
            wrap(angle / offset)
        }
        _ => wrap(t * PREVIEW_FPS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_format::mods::Animation;

    fn image(animation: Option<Animation>, heading: i32) -> Gfx {
        Gfx::Image {
            file: "gfx/x.png".into(),
            rotating: false,
            rotation_step_deg: 0,
            rotation_speed: 0,
            frames_follow_heading: heading > 0,
            frames_follow_heading_mirrored: false,
            heading_frames: heading,
            animation,
        }
    }

    #[test]
    fn the_preview_plays_turns_or_steps() {
        let anim = |mode| image(Some(Animation { fps: 4.0, mode }), 0);
        let at = |g: &Gfx, t: f64| preview_frame(Some(g), 4, t);
        let lp = anim(AnimMode::Loop);
        assert_eq!([0.0, 0.25, 0.5, 0.75, 1.0].map(|t| at(&lp, t)), [0, 1, 2, 3, 0]);
        let pp = anim(AnimMode::PingPong);
        assert_eq!([0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5].map(|t| at(&pp, t)), [0, 1, 2, 3, 2, 1, 0]);
        let heading = image(None, 4);
        assert_eq!([0.0, 0.75, 1.5, 2.25, 3.0].map(|t| at(&heading, t)), [0, 1, 2, 3, 0]);
        assert_eq!(preview_frame(None, 3, 0.25), 2);
        assert_eq!(preview_frame(None, 1, 99.0), 0);
        assert_eq!(preview_frame(Some(&lp), 4, f64::NAN), 0);
    }

    #[test]
    fn the_dark_look_keeps_glowing_pixels_bright() {
        let mut f = Frame::blank(2, 1);
        f.px = vec![Rgba::rgb(200, 100, 40), Rgba::rgb(200, 100, 40)];
        f.glow = vec![0, 255];
        let d = dark(&f);
        assert_eq!(d.get(0, 0), Rgba::rgb(50, 25, 10));
        assert_eq!(d.get(1, 0), Rgba::rgb(200, 100, 40));
    }
}
