//! What one drawn frame shows: the terrain view and, in OLX's layer order,
//! sprites and lines at world pixels.

use core::fmt::Write as _;

use olr_data::image::Rgba;

use crate::assets::SceneAssets;

/// An image of [`SceneAssets`] (an index into [`SceneAssets::images`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AtlasId(pub u32);

/// A rectangle of source pixels in an atlas image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One image blit. `pos` (top-left corner) and `size` are world pixels:
/// OLX's sprites are 2× art, so they often fall on half pixels. With `flip`
/// the source is mirrored horizontally; `tint` multiplies every texel (white
/// leaves it unchanged; a tinted [`SceneAssets::white`] texel is a solid
/// block of colour).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub atlas: AtlasId,
    pub src: Rect,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub flip: bool,
    pub tint: Rgba,
    pub shade: Shade,
}

/// How an item takes the look's light (look system, plan 15a R5): lit by
/// the world's light, showing at least `self_light` percent whatever the
/// darkness (a glow), or a `marker` drawn exactly as its art (the
/// crosshair, health bars). Only drawn with a look; without one every item
/// shows as its art.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shade {
    /// Percent, 0-100.
    pub self_light: u8,
    pub marker: bool,
}

impl Shade {
    /// Lit by the world (every item's default).
    pub const LIT: Shade = Shade { self_light: 0, marker: false };
    /// Shows at its own full brightness (explosions, muzzle flashes, beams).
    pub const GLOW: Shade = Shade { self_light: 100, marker: false };
    /// Drawn as its art, unlit and ungraded (the crosshair, health bars).
    pub const MARKER: Shade = Shade { self_light: 0, marker: true };

    /// `describe`'s suffix: nothing for [`Shade::LIT`].
    fn suffix(self) -> String {
        match self {
            Shade { marker: true, .. } => " marker".into(),
            Shade { self_light: 0, .. } => String::new(),
            Shade { self_light, .. } => format!(" glow {self_light}"),
        }
    }
}

/// A solid 1-world-pixel line (spec §19 E3) through the pixels
/// [`Line::pixels`] lists: OLX's `perform_line` from `a` to `b`, both
/// included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub a: (i32, i32),
    pub b: (i32, i32),
    pub colour: Rgba,
    pub shade: Shade,
}

impl Line {
    /// The world pixels of the line, in drawing order.
    pub fn pixels(&self, plot: impl FnMut(i32, i32)) {
        perform_line(self.a, self.b, plot);
    }
}

/// One thing to draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Sprite(Sprite),
    Line(Line),
}

/// The layers of a viewport, drawn in this order after the terrain
/// (`CViewport::gusRender`, presentation note §11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// Cosmetic entities, appended by the caller (`olr-fx`), in slot order.
    Entities,
    /// Laser sights and worm beams (`DrawEntities` draws the beams after
    /// every other entity).
    Beams,
    /// Projectiles in pool slot order.
    Projectiles,
    /// The client's bonuses in slot order.
    Bonuses,
    /// Alive worms in id order, each: rope, health bar, crosshair, skin,
    /// muzzle flash.
    Worms,
}

impl Layer {
    /// Every layer in drawing order.
    pub const ALL: [Layer; 5] =
        [Layer::Entities, Layer::Beams, Layer::Projectiles, Layer::Bonuses, Layer::Worms];

    fn name(self) -> &'static str {
        match self {
            Layer::Entities => "entities",
            Layer::Beams => "beams",
            Layer::Projectiles => "projectiles",
            Layer::Bonuses => "bonuses",
            Layer::Worms => "worms",
        }
    }
}

/// The world rectangle the viewport shows, in world pixels (the camera's
/// `WorldX`, `WorldY`, `Width`, `Height`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl View {
    /// `CViewport::posInside` of a world position: inside the view, edges
    /// included.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    /// Whether a `w` × `h` rectangle at `(x, y)` overlaps the view.
    pub fn overlaps(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        x + w >= self.x && x <= self.x + self.w && y + h >= self.y && y <= self.y + self.h
    }
}

/// A frame to draw: the terrain (the colour map seen through `view`, black
/// outside the map), then the layers in [`Layer::ALL`] order. Building
/// reuses the buffers, so a steady state allocates nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    /// The terrain view rectangle.
    pub view: View,
    /// The map's size in world pixels.
    pub map: (u32, u32),
    layers: [Vec<Item>; 5],
}

impl DrawList {
    /// Items each layer has room for before it grows (a busy OLX frame
    /// has a few hundred; laser sights add up to about a thousand dots).
    pub const RESERVED: usize = 2048;

    /// The room of [`Layer::Entities`]: one item per cosmetic entity of
    /// `olr-fx`'s play pool (`olr_fx::PLAY_ENTITIES`, which checks it is no
    /// larger), so a frame full of effects never allocates either.
    pub const ENTITIES_RESERVED: usize = 16384;

    /// An empty list with [`DrawList::RESERVED`] items of room per layer
    /// ([`DrawList::ENTITIES_RESERVED`] for the entities), so ordinary
    /// frames never allocate.
    pub fn new() -> DrawList {
        DrawList {
            layers: core::array::from_fn(|i| {
                let room =
                    if i == Layer::Entities as usize { Self::ENTITIES_RESERVED } else { Self::RESERVED };
                Vec::with_capacity(room)
            }),
            ..DrawList::default()
        }
    }

    /// Empties every layer, keeping the buffers.
    pub fn clear(&mut self) {
        self.layers.iter_mut().for_each(Vec::clear);
    }

    pub fn layer(&self, layer: Layer) -> &[Item] {
        &self.layers[layer as usize]
    }

    pub fn push(&mut self, layer: Layer, item: Item) {
        self.layers[layer as usize].push(item);
    }

    pub fn sprite(&mut self, layer: Layer, sprite: Sprite) {
        self.push(layer, Item::Sprite(sprite));
    }

    /// A one-world-pixel block of `colour` at world pixel `(x, y)`.
    pub fn pixel(&mut self, layer: Layer, white: AtlasId, x: f32, y: f32, colour: Rgba) {
        let src = Rect { x: 0, y: 0, w: 1, h: 1 };
        self.sprite(
            layer,
            Sprite {
                atlas: white,
                src,
                pos: [x, y],
                size: [1.0, 1.0],
                flip: false,
                tint: colour,
                shade: Default::default(),
            },
        );
    }

    /// Every item in drawing order, with its layer.
    pub fn items(&self) -> impl Iterator<Item = (Layer, &Item)> {
        Layer::ALL.into_iter().flat_map(move |l| self.layer(l).iter().map(move |i| (l, i)))
    }

    /// A compact, deterministic text form (one item per line, images by
    /// name) for snapshot tests and debugging.
    pub fn describe(&self, assets: &SceneAssets) -> String {
        let mut s = String::new();
        let v = self.view;
        let _ =
            writeln!(s, "view {},{} {}x{} map {}x{}", n(v.x), n(v.y), n(v.w), n(v.h), self.map.0, self.map.1);
        for l in Layer::ALL {
            let _ = writeln!(s, "[{}]", l.name());
            for item in self.layer(l) {
                match item {
                    Item::Line(line) => {
                        let _ = writeln!(
                            s,
                            "line {},{} {},{} {}{}",
                            line.a.0,
                            line.a.1,
                            line.b.0,
                            line.b.1,
                            hex(line.colour),
                            line.shade.suffix()
                        );
                    }
                    Item::Sprite(sp) if sp.atlas == assets.white && sp.size == [1.0, 1.0] => {
                        let _ = writeln!(
                            s,
                            "px {},{} {}{}",
                            n(sp.pos[0]),
                            n(sp.pos[1]),
                            hex(sp.tint),
                            sp.shade.suffix()
                        );
                    }
                    Item::Sprite(sp) => {
                        let r = sp.src;
                        let _ = write!(
                            s,
                            "{} {},{} {}x{} at {},{} {}x{}",
                            assets.image(sp.atlas).name,
                            r.x,
                            r.y,
                            r.w,
                            r.h,
                            n(sp.pos[0]),
                            n(sp.pos[1]),
                            n(sp.size[0]),
                            n(sp.size[1])
                        );
                        if sp.flip {
                            s.push_str(" flip");
                        }
                        if sp.tint != Rgba::rgb(255, 255, 255) {
                            let _ = write!(s, " {}", hex(sp.tint));
                        }
                        s.push_str(&sp.shade.suffix());
                        s.push('\n');
                    }
                }
            }
        }
        s
    }
}

/// A number without a trailing `.0` and never `-0`.
fn n(v: f32) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v}")
}

fn hex(c: Rgba) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    }
}

/// Allegro's `do_line` as OLX's `perform_line` runs it: every pixel from
/// `a` to `b` (both included) along the major axis, in that order.
///
/// OLX: src/client/GfxPrimitives.cpp:1906-1995
pub fn perform_line(a: (i32, i32), b: (i32, i32), mut plot: impl FnMut(i32, i32)) {
    let (dx, dy) = (b.0 as i64 - a.0 as i64, b.1 as i64 - a.1 as i64);
    // (y is the primary axis, primary sign, secondary sign, `dd >= 0` test)
    let (swap, ps, ss, ge) = if dx >= 0 {
        if dy >= 0 {
            if dx >= dy { (false, 1, 1, true) } else { (true, 1, 1, true) }
        } else if dx >= -dy {
            (false, 1, -1, false)
        } else {
            (true, -1, 1, true)
        }
    } else if dy >= 0 {
        if -dx >= dy { (false, -1, 1, true) } else { (true, 1, -1, false) }
    } else if -dx >= -dy {
        (false, -1, -1, false)
    } else {
        (true, -1, -1, false)
    };
    let (mut pri, pri_end, mut sec, d_pri, d_sec) = if swap {
        (a.1 as i64, b.1 as i64, a.0 as i64, dy, dx)
    } else {
        (a.0 as i64, b.0 as i64, a.1 as i64, dx, dy)
    };
    let mut put = |pri: i64, sec: i64| {
        let (x, y) = if swap { (sec, pri) } else { (pri, sec) };
        plot(x as i32, y as i32);
    };
    if d_pri == 0 {
        put(pri, sec);
        return;
    }
    let i1 = 2 * d_sec;
    let mut dd = i1 - ss * (ps * d_pri);
    let i2 = dd - ss * (ps * d_pri);
    while if ps > 0 { pri <= pri_end } else { pri >= pri_end } {
        put(pri, sec);
        if if ge { dd >= 0 } else { dd <= 0 } {
            sec += ss;
            dd += i2;
        } else {
            dd += i1;
        }
        pri += ps;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        perform_line(a, b, |x, y| v.push((x, y)));
        v
    }

    /// Every item is lit by default; `describe` names a shade only when it
    /// differs, so the committed snapshots stay as they are (plan 15a R5).
    #[test]
    fn shade_is_lit_by_default_and_described_only_when_not() {
        assert_eq!(Shade::default(), Shade::LIT);
        assert_eq!((Shade::LIT.self_light, Shade::LIT.marker), (0, false));
        assert_eq!((Shade::GLOW.self_light, Shade::GLOW.marker), (100, false));
        assert_eq!((Shade::MARKER.self_light, Shade::MARKER.marker), (0, true));
        // `describe` appends the suffix to each item's line.
        assert_eq!(Shade::LIT.suffix(), "");
        assert_eq!(Shade::GLOW.suffix(), " glow 100");
        assert_eq!(Shade::MARKER.suffix(), " marker");
        assert_eq!(Shade { self_light: 50, marker: false }.suffix(), " glow 50");
    }

    #[test]
    fn perform_line_runs_from_a_to_b_inclusive_in_every_octant() {
        assert_eq!(line((0, 0), (0, 0)), [(0, 0)]);
        assert_eq!(line((0, 0), (3, 1)), [(0, 0), (1, 0), (2, 1), (3, 1)]);
        assert_eq!(line((3, 1), (0, 0)), [(3, 1), (2, 1), (1, 0), (0, 0)]);
        assert_eq!(line((0, 0), (1, 3)), [(0, 0), (0, 1), (1, 2), (1, 3)]);
        assert_eq!(line((0, 0), (3, -1)), [(0, 0), (1, 0), (2, -1), (3, -1)]);
        assert_eq!(line((0, 0), (-1, 3)), [(0, 0), (0, 1), (-1, 2), (-1, 3)]);
        assert_eq!(line((0, 0), (1, -3)), [(0, 0), (0, -1), (1, -2), (1, -3)]);
        assert_eq!(line((0, 0), (-3, -1)), [(0, 0), (-1, 0), (-2, -1), (-3, -1)]);
        assert_eq!(line((0, 0), (-1, -3)), [(0, 0), (0, -1), (-1, -2), (-1, -3)]);
        assert_eq!(line((5, 2), (5, -1)), [(5, 2), (5, 1), (5, 0), (5, -1)]);
        // Extreme coordinates neither overflow nor panic.
        assert_eq!(line((i32::MIN, 0), (i32::MIN, 0)).len(), 1);
    }

    #[test]
    fn the_entities_layer_has_room_for_plays_pool() {
        let l = DrawList::new();
        assert!(l.layers[Layer::Entities as usize].capacity() >= DrawList::ENTITIES_RESERVED);
        assert!(l.layers[Layer::Worms as usize].capacity() >= DrawList::RESERVED);
    }
}
