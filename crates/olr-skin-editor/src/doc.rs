//! The open skin (issue #14): a `SkinSource` of OLX's layout or double
//! resolution, cut into its 21 frames (7 aim columns by 3 walk rows), each
//! with its colour, player-colour (tint) and glow planes.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::skin::SkinLight;
use olr_format::skins::{DOUBLE_SKIN, OLX_SKIN, SkinSource};

/// Aim columns.
pub const AIM_FRAMES: usize = OLX_SKIN.2 as usize;
/// Walk rows.
pub const WALK_FRAMES: usize = OLX_SKIN.3 as usize;
/// Every frame: `walk * AIM_FRAMES + aim`.
pub const FRAMES: usize = AIM_FRAMES * WALK_FRAMES;

/// Why a skin pack does not open in the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Neither OLX's layout nor double resolution's.
    Layout(String),
    /// A plane whose size is not the layout's.
    Size(&'static str),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Layout(what) => write!(
                f,
                "the editor opens skins of 7 × 3 frames of 20 × 18 or 40 × 36 pixels; this one is {what}"
            ),
            OpenError::Size(file) => write!(f, "{file} does not fit the skin's layout"),
        }
    }
}

impl std::error::Error for OpenError {}

/// One frame's pixels, row by row: colour (transparent is all 0), tint
/// (0 keep, 255 the player's colour) and glow (0 none to 255 full).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub px: Vec<Rgba>,
    pub tint: Vec<u8>,
    pub glow: Vec<u8>,
}

impl Frame {
    /// A frame of nothing.
    pub fn blank(w: u32, h: u32) -> Frame {
        let n = w as usize * h as usize;
        Frame { w, h, px: vec![Rgba::default(); n], tint: vec![0; n], glow: vec![0; n] }
    }

    /// The index of `(x, y)`, if inside.
    pub fn at(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h)
            .then(|| y as usize * self.w as usize + x as usize)
    }

    /// The frame mirrored left to right, or top to bottom when `vertical`.
    pub fn flipped(&self, vertical: bool) -> Frame {
        self.remapped(|x, y| if vertical { (x, self.h - 1 - y) } else { (self.w - 1 - x, y) })
    }

    /// The frame moved by `(dx, dy)` art pixels, what leaves one edge
    /// coming back at the other (nothing is lost).
    pub fn shifted(&self, dx: i32, dy: i32) -> Frame {
        let (w, h) = (self.w as i64, self.h as i64);
        self.remapped(|x, y| {
            (((x as i64 - dx as i64).rem_euclid(w)) as u32, ((y as i64 - dy as i64).rem_euclid(h)) as u32)
        })
    }

    /// A frame whose pixel `(x, y)` is this one's `from(x, y)`.
    fn remapped(&self, from: impl Fn(u32, u32) -> (u32, u32)) -> Frame {
        let mut out = Frame::blank(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                let (sx, sy) = from(x, y);
                let (i, s) = ((y * self.w + x) as usize, (sy * self.w + sx) as usize);
                out.px[i] = self.px[s];
                out.tint[i] = self.tint[s];
                out.glow[i] = self.glow[s];
            }
        }
        out
    }
}

/// The skin being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinDoc {
    /// The source as it opened; its planes are kept up to date but for
    /// `glow`, which is always present here and saved only when used.
    src: SkinSource,
    /// Bumped by every change, per frame and in all.
    generation: u64,
    frame_generation: [u64; FRAMES],
}

/// A transparent pixel as the format lifts it: every channel 0.
fn plain(p: Rgba) -> Rgba {
    if p.a == 0 { Rgba::default() } else { p }
}

impl SkinDoc {
    /// The editor's copy of `src`: OLX's layout or double resolution's,
    /// planes of the layout's size, refused otherwise (never a panic). A
    /// tint other than 0 counts as the player's colour.
    pub fn new(mut src: SkinSource) -> Result<SkinDoc, OpenError> {
        let layout = (src.art_scale, src.frame_size, src.aim_frames, src.walk_frames);
        if layout != OLX_SKIN && layout != DOUBLE_SKIN {
            return Err(OpenError::Layout(format!(
                "art scale {}, {} × {} frames of {} × {}",
                layout.0, layout.2, layout.3, layout.1.0, layout.1.1
            )));
        }
        let (w, h) = (src.frame_size.0 * src.aim_frames, src.frame_size.1 * src.walk_frames);
        let n = w as usize * h as usize;
        if (src.image.width(), src.image.height()) != (w, h) {
            return Err(OpenError::Size("skin.png"));
        }
        if src.tint.len() != n {
            return Err(OpenError::Size("tint.png"));
        }
        if src.glow.as_ref().is_some_and(|g| g.len() != n) {
            return Err(OpenError::Size("glow.png"));
        }
        for t in &mut src.tint {
            *t = if *t == 0 { 0 } else { 255 };
        }
        src.glow.get_or_insert_with(|| vec![0; n]);
        Ok(SkinDoc { src, generation: 0, frame_generation: [0; FRAMES] })
    }

    /// A blank skin of double resolution (or OLX's layout).
    pub fn blank(name: &str, double: bool) -> SkinDoc {
        // A blank skin is always of a layout `new` takes.
        SkinDoc::new(olr_edit::skin::blank(name, double)).unwrap_or_else(|_| unreachable!())
    }

    /// The skin as its files hold it: no `glow.png` when nothing glows.
    pub fn to_source(&self) -> SkinSource {
        let mut s = self.src.clone();
        if s.glow.as_ref().is_some_and(|g| g.iter().all(|&v| v == 0)) {
            s.glow = None;
        }
        s
    }

    pub fn name(&self) -> &str {
        &self.src.name
    }

    pub fn light(&self) -> Option<SkinLight> {
        self.src.light
    }

    pub(crate) fn set_name(&mut self, name: String) {
        self.src.name = name;
        self.generation += 1;
    }

    pub(crate) fn set_light(&mut self, light: Option<SkinLight>) {
        self.src.light = light;
        self.generation += 1;
    }

    /// Art pixels per world pixel: 2 (OLX's layout) or 4 (double).
    pub fn art_scale(&self) -> u32 {
        self.src.art_scale
    }

    /// Whether the skin is of OLX's layout (it can be made double).
    pub fn is_classic(&self) -> bool {
        self.src.art_scale == OLX_SKIN.0
    }

    /// A frame's size in art pixels.
    pub fn frame_size(&self) -> (u32, u32) {
        self.src.frame_size
    }

    /// Bumped by every change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Bumped by every change of frame `f`.
    pub fn frame_generation(&self, f: usize) -> u64 {
        self.frame_generation.get(f).copied().unwrap_or(0)
    }

    /// The top-left art pixel of frame `f` in the grid.
    fn origin(&self, f: usize) -> (u32, u32) {
        let (fw, fh) = self.src.frame_size;
        ((f % AIM_FRAMES) as u32 * fw, (f / AIM_FRAMES) as u32 * fh)
    }

    /// Frame `f` (0 to 20) as it is now.
    pub fn frame(&self, f: usize) -> Frame {
        let (fw, fh) = self.src.frame_size;
        let mut out = Frame::blank(fw, fh);
        if f >= FRAMES {
            return out;
        }
        let (ox, oy) = self.origin(f);
        let w = self.src.image.width() as usize;
        let glow = self.src.glow.as_deref().unwrap_or(&[]);
        for y in 0..fh {
            for x in 0..fw {
                let (i, s) = ((y * fw + x) as usize, (oy + y) as usize * w + (ox + x) as usize);
                out.px[i] = plain(self.src.image.get(ox + x, oy + y));
                out.tint[i] = self.src.tint[s];
                out.glow[i] = glow.get(s).copied().unwrap_or(0);
            }
        }
        out
    }

    /// Puts `frame` in place of frame `f`; one of another size changes
    /// nothing.
    pub(crate) fn set_frame(&mut self, f: usize, frame: &Frame) {
        let (fw, fh) = self.src.frame_size;
        if f >= FRAMES || (frame.w, frame.h) != (fw, fh) {
            return;
        }
        let (ox, oy) = self.origin(f);
        let w = self.src.image.width() as usize;
        for y in 0..fh {
            for x in 0..fw {
                let (i, s) = ((y * fw + x) as usize, (oy + y) as usize * w + (ox + x) as usize);
                let p = plain(frame.px[i]);
                self.src.image.set(ox + x, oy + y, p);
                // A transparent pixel neither tints nor glows.
                self.src.tint[s] = if p.a == 0 { 0 } else { frame.tint[i] };
                if let Some(g) = &mut self.src.glow {
                    g[s] = if p.a == 0 { 0 } else { frame.glow[i] };
                }
            }
        }
        self.generation += 1;
        self.frame_generation[f] += 1;
    }

    /// The whole source, swapped (Make double resolution and its undo).
    pub(crate) fn set_source(&mut self, src: SkinSource) {
        // Only sources the editor made (from a `SkinDoc`) come here.
        if let Ok(doc) = SkinDoc::new(src) {
            self.src = doc.src;
        }
        self.generation += 1;
        for g in &mut self.frame_generation {
            *g += 1;
        }
    }

    /// The source as held (glow always present).
    pub(crate) fn source(&self) -> &SkinSource {
        &self.src
    }

    /// The colour image of frame `f` (for thumbnails and the canvas).
    pub fn frame_image(&self, f: usize) -> RgbaImage {
        let fr = self.frame(f);
        let mut img = RgbaImage::new(fr.w, fr.h);
        for (i, p) in fr.px.iter().enumerate() {
            img.set(i as u32 % fr.w, i as u32 / fr.w, *p);
        }
        img
    }
}
