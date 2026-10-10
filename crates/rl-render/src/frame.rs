//! The world pass's geometry and its instances (CPU only): where the view
//! lands in the art-scale target, and one textured quad per sprite, per
//! line pixel and for the terrain.

use rl_data::image::Rgba;
use rl_scene::{DrawList, Item, Sprite, View};

use crate::atlas::{AtlasLayout, TexRect, WHITE};

/// World pixels drawn around the view on every side, so a sub-pixel camera
/// moves the upscale's source rectangle instead of the world's pixels
/// (spec §8.2: no shimmering).
pub const MARGIN: i32 = 1;

/// The widest or tallest view drawn, in world pixels (a hostile or broken
/// view is clamped, never a huge texture).
pub const MAX_VIEW: f32 = 4096.0;

/// The opaque magenta of a missing atlas entry.
pub const MISSING: Rgba = Rgba::MAGENTA;

/// The world-pass target: its world-pixel origin (the view's corner,
/// truncated, minus the margin), its size in world pixels and its art
/// scale (target texels per world pixel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldFrame {
    pub origin: (i32, i32),
    pub size: (u32, u32),
    pub scale: u32,
}

/// A sanitised view: finite, at most [`MAX_VIEW`] on a side.
fn sane(view: View) -> View {
    let f = |v: f32, lim: f32| if v.is_finite() { v.clamp(-lim, lim) } else { 0.0 };
    View {
        x: f(view.x, 1e7),
        y: f(view.y, 1e7),
        w: f(view.w, MAX_VIEW).max(1.0),
        h: f(view.h, MAX_VIEW).max(1.0),
    }
}

impl WorldFrame {
    /// The target for `view` at `scale` art pixels per world pixel.
    pub fn new(view: View, scale: u32) -> WorldFrame {
        let v = sane(view);
        let origin = (v.x.floor() as i32 - MARGIN, v.y.floor() as i32 - MARGIN);
        let span = |w: f32| w.ceil() as u32 + 1 + 2 * MARGIN as u32;
        WorldFrame { origin, size: (span(v.w), span(v.h)), scale: scale.max(1) }
    }

    /// The target's size in texels.
    pub fn target_size(&self) -> (u32, u32) {
        (self.size.0 * self.scale, self.size.1 * self.scale)
    }

    /// The view as a rectangle of target texels `[x, y, w, h]`: what the
    /// upscale pass samples.
    pub fn source_rect(&self, view: View) -> [f32; 4] {
        let v = sane(view);
        let s = self.scale as f32;
        [(v.x - self.origin.0 as f32) * s, (v.y - self.origin.1 as f32) * s, v.w * s, v.h * s]
    }

    /// World pixel `(x, y)` as a target texel coordinate.
    fn texel(&self, x: f32, y: f32) -> [f32; 2] {
        let s = self.scale as f32;
        [(x - self.origin.0 as f32) * s, (y - self.origin.1 as f32) * s]
    }
}

/// One textured quad of the world pass: `dst` in target texels, `src` in
/// texture texels (a negative width mirrors it), multiplied by `tint`
/// (0 to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub dst: [f32; 4],
    pub src: [f32; 4],
    pub tint: [f32; 4],
}

impl Instance {
    /// The vertex-buffer layout: `dst`, `src`, `tint`.
    pub fn raw(&self) -> [f32; 12] {
        let [a, b, c, d] = self.dst;
        let [e, f, g, h] = self.src;
        let [i, j, k, l] = self.tint;
        [a, b, c, d, e, f, g, h, i, j, k, l]
    }
}

fn tint(c: Rgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a].map(|v| f32::from(v) / 255.0)
}

/// The terrain quad: the colour map (`art` texels) over the map's
/// `map` world pixels.
pub fn terrain_instance(frame: &WorldFrame, map: (u32, u32), art: (u32, u32)) -> Instance {
    let [x, y] = frame.texel(0.0, 0.0);
    let s = frame.scale as f32;
    Instance {
        dst: [x, y, map.0 as f32 * s, map.1 as f32 * s],
        src: [0.0, 0.0, art.0 as f32, art.1 as f32],
        tint: [1.0; 4],
    }
}

/// Appends the instances of `list`'s layers in drawing order: each sprite
/// snapped to whole target texels (sprites keep their pixel grid), each
/// line as one world-pixel block per [`rl_scene::Line::pixels`] pixel
/// (spec §19 E3). A sprite whose image the atlas lacks is a magenta
/// square; anything wholly outside the target is skipped.
pub fn push_instances(list: &DrawList, atlas: &AtlasLayout, frame: &WorldFrame, out: &mut Vec<Instance>) {
    let (tw, th) = frame.target_size();
    let (tw, th) = (tw as f32, th as f32);
    let visible = |d: &[f32; 4]| {
        d[2] > 0.0 && d[3] > 0.0 && d[0] < tw && d[1] < th && d[0] + d[2] > 0.0 && d[1] + d[3] > 0.0
    };
    let white = [WHITE.x as f32, WHITE.y as f32, 1.0, 1.0];
    let s = frame.scale as f32;
    for (_, item) in list.items() {
        match item {
            Item::Sprite(sp) => {
                if let Some(i) = sprite(sp, atlas, frame)
                    && visible(&i.dst)
                {
                    out.push(i);
                }
            }
            Item::Line(line) => line.pixels(|x, y| {
                let [px, py] = frame.texel(x as f32, y as f32);
                let dst = [px, py, s, s];
                if visible(&dst) {
                    out.push(Instance { dst, src: white, tint: tint(line.colour) });
                }
            }),
        }
    }
}

/// A sprite's quad, its source clipped to its image (`None` when nothing
/// is left or the numbers are not finite).
fn sprite(sp: &Sprite, atlas: &AtlasLayout, frame: &WorldFrame) -> Option<Instance> {
    let snap = |v: f32| (v + 0.5).floor();
    let [x, y] = frame.texel(sp.pos[0], sp.pos[1]);
    let s = frame.scale as f32;
    let mut dst = [snap(x), snap(y), snap(sp.size[0] * s), snap(sp.size[1] * s)];
    if !dst.iter().all(|v| v.is_finite()) || dst[2] <= 0.0 || dst[3] <= 0.0 {
        return None;
    }
    let Some(e) = atlas.entry(sp.atlas) else {
        let src = [WHITE.x as f32, WHITE.y as f32, 1.0, 1.0];
        return Some(Instance { dst, src, tint: tint(MISSING) });
    };
    let r = sp.src;
    if r.w == 0 || r.h == 0 || r.x >= e.w || r.y >= e.h {
        return None;
    }
    let (w, h) = (r.w.min(e.w - r.x), r.h.min(e.h - r.y));
    // A clipped source shrinks the quad by the same share.
    dst[2] = snap(dst[2] * w as f32 / r.w as f32);
    dst[3] = snap(dst[3] * h as f32 / r.h as f32);
    let src = TexRect { x: e.x + r.x, y: e.y + r.y, w, h };
    let src = if sp.flip {
        [(src.x + src.w) as f32, src.y as f32, -(src.w as f32), src.h as f32]
    } else {
        [src.x as f32, src.y as f32, src.w as f32, src.h as f32]
    };
    Some(Instance { dst, src, tint: tint(sp.tint) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::pack;
    use rl_data::image::RgbaImage;
    use rl_scene::{AtlasId, AtlasImage, Layer, Line, Rect};

    fn atlas() -> AtlasLayout {
        let image = |w, h| AtlasImage { name: "i".into(), image: RgbaImage::new(w, h), px_per_world: 2 };
        pack(&[image(8, 4), image(32, 6)], 1024).layout
    }

    fn sprite_at(atlas: u32, pos: [f32; 2], src: Rect, flip: bool) -> Sprite {
        let size = [src.w as f32 / 2.0, src.h as f32 / 2.0];
        Sprite { atlas: AtlasId(atlas), src, pos, size, flip, tint: Rgba::rgb(255, 255, 255) }
    }

    #[test]
    fn world_frame_has_a_margin_and_samples_the_view() {
        let view = View { x: 100.25, y: 50.0, w: 320.0, h: 240.0 };
        let f = WorldFrame::new(view, 2);
        assert_eq!(f.origin, (99, 49));
        assert_eq!(f.size, (323, 243));
        assert_eq!(f.target_size(), (646, 486));
        assert_eq!(f.source_rect(view), [2.5, 2.0, 640.0, 480.0]);
        let [x, y, w, h] = f.source_rect(view);
        assert!(x >= 2.0 && y >= 2.0 && x + w <= 644.0 && y + h <= 484.0, "a margin all round");
        let wild = WorldFrame::new(View { x: f32::NAN, y: 1e30, w: f32::INFINITY, h: -5.0 }, 0);
        assert!(wild.size.0 < 10 && wild.size.1 < 10 && wild.scale == 1, "{wild:?}");
    }

    #[test]
    fn instances_follow_draw_order_at_art_scale() {
        let atlas = atlas();
        let e1 = atlas.entry(AtlasId(1)).unwrap();
        let mut list = DrawList::new();
        list.view = View { x: 10.0, y: 20.0, w: 100.0, h: 80.0 };
        let src = Rect { x: 8, y: 0, w: 8, h: 6 };
        list.sprite(Layer::Worms, sprite_at(1, [15.5, 30.0], src, false));
        list.sprite(Layer::Entities, sprite_at(1, [15.5, 30.0], src, true));
        list.push(Layer::Beams, Item::Line(Line { a: (12, 22), b: (14, 23), colour: Rgba::rgb(255, 0, 0) }));
        let f = WorldFrame::new(list.view, 2);
        let mut out = Vec::new();
        push_instances(&list, &atlas, &f, &mut out);
        assert_eq!(out.len(), 5, "entities, 3 line pixels, worms");
        // (15.5 - 9) * 2 = 13, (30 - 19) * 2 = 22; 8 x 6 texels at 2x art.
        assert_eq!(out[0].dst, [13.0, 22.0, 8.0, 6.0]);
        let (x, y) = (e1.x as f32, e1.y as f32);
        assert_eq!(out[0].src, [x + 16.0, y, -8.0, 6.0], "flipped");
        assert_eq!(out[4].src, [x + 8.0, y, 8.0, 6.0]);
        assert_eq!(out[1].dst, [6.0, 6.0, 2.0, 2.0], "a line pixel is a world pixel");
        assert_eq!(out[3].dst, [10.0, 8.0, 2.0, 2.0]);
        assert_eq!(out[1].src, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(out[1].tint, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(out[1].raw()[..4], out[1].dst);
    }

    #[test]
    fn missing_atlas_entries_are_magenta_squares() {
        let atlas = atlas();
        let mut list = DrawList::new();
        list.view = View { x: 0.0, y: 0.0, w: 100.0, h: 80.0 };
        list.sprite(Layer::Worms, sprite_at(9, [10.0, 10.0], Rect { x: 0, y: 0, w: 4, h: 4 }, false));
        // A source past its image draws nothing; one overlapping it is clipped.
        list.sprite(Layer::Worms, sprite_at(0, [10.0, 10.0], Rect { x: 8, y: 0, w: 4, h: 4 }, false));
        list.sprite(Layer::Worms, sprite_at(0, [10.0, 10.0], Rect { x: 4, y: 0, w: 8, h: 4 }, false));
        let mut out = Vec::new();
        push_instances(&list, &atlas, &WorldFrame::new(list.view, 2), &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].tint, [1.0, 0.0, 1.0, 1.0]);
        assert_eq!(out[0].src, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(out[0].dst, [22.0, 22.0, 4.0, 4.0]);
        assert_eq!(out[1].dst[2..], [4.0, 4.0], "clipped to the image's last 4 columns");
    }

    #[test]
    fn instances_outside_the_target_are_skipped() {
        let atlas = atlas();
        let mut list = DrawList::new();
        list.view = View { x: 0.0, y: 0.0, w: 50.0, h: 50.0 };
        let src = Rect { x: 0, y: 0, w: 8, h: 4 };
        list.sprite(Layer::Worms, sprite_at(0, [500.0, 10.0], src, false));
        list.sprite(Layer::Worms, sprite_at(0, [-30.0, 10.0], src, false));
        list.sprite(Layer::Worms, sprite_at(0, [f32::NAN, 10.0], src, false));
        list.push(Layer::Beams, Item::Line(Line { a: (-100, 0), b: (100, 0), colour: Rgba::BLACK }));
        let mut out = Vec::new();
        push_instances(&list, &atlas, &WorldFrame::new(list.view, 2), &mut out);
        assert_eq!(out.len(), 53, "the line's pixels from x = -1 to 51");
        let t = terrain_instance(&WorldFrame::new(list.view, 2), (600, 400), (1200, 800));
        assert_eq!(t.dst, [2.0, 2.0, 1200.0, 800.0]);
        assert_eq!(t.src, [0.0, 0.0, 1200.0, 800.0]);
    }

    #[test]
    fn instance_building_allocates_nothing_once_warm() {
        let atlas = atlas();
        let mut list = DrawList::new();
        list.view = View { x: 0.5, y: 0.0, w: 320.0, h: 240.0 };
        for i in 0..500 {
            let src = Rect { x: 0, y: 0, w: 8, h: 4 };
            list.sprite(Layer::Entities, sprite_at(i % 3, [i as f32 * 0.5, 100.0], src, i % 2 == 0));
        }
        list.push(Layer::Beams, Item::Line(Line { a: (0, 0), b: (300, 200), colour: Rgba::BLACK }));
        let frame = WorldFrame::new(list.view, 2);
        let mut out = Vec::new();
        push_instances(&list, &atlas, &frame, &mut out);
        let n = out.len();
        let m = allocation_counter::measure(|| {
            out.clear();
            push_instances(&list, &atlas, &frame, &mut out);
        });
        assert_eq!((m.count_total, out.len()), (0, n));
    }
}
