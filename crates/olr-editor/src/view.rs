//! Screen points ↔ world pixels (ruling R7): the only floats besides the
//! egui code. The canvas shows the level's art at `ZOOMS[zoom]` screen
//! points per art pixel, its origin at `offset` (canvas-relative points).

/// Screen points per art pixel.
pub const ZOOMS: [f32; 10] = [0.25, 0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 16.0];

/// A world point a hair inside its pixel, so a pixel's own corner maps back
/// to it whatever the rounding.
const BIAS: f32 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// An index into `ZOOMS`.
    pub zoom: usize,
    /// The art origin, canvas-relative.
    pub offset: [f32; 2],
}

impl View {
    /// The largest zoom showing the whole level (its art size) in `canvas`,
    /// centred (the smallest zoom when none fits).
    pub fn fit(canvas: [f32; 2], art: (u32, u32)) -> View {
        let zoom = (0..ZOOMS.len())
            .rev()
            .find(|&z| art.0 as f32 * ZOOMS[z] <= canvas[0] && art.1 as f32 * ZOOMS[z] <= canvas[1])
            .unwrap_or(0);
        let scale = ZOOMS[zoom];
        let mut view = View {
            zoom,
            offset: [(canvas[0] - art.0 as f32 * scale) / 2.0, (canvas[1] - art.1 as f32 * scale) / 2.0],
        };
        view.snap();
        view
    }

    /// `ZOOMS[zoom]`.
    pub fn scale(&self) -> f32 {
        ZOOMS[self.zoom.min(ZOOMS.len() - 1)]
    }

    /// The world pixel under canvas point `p` (floored).
    pub fn to_world(&self, p: [f32; 2], art_scale: u32) -> (i32, i32) {
        let unit = self.scale() * art_scale.max(1) as f32;
        let axis = |v: f32, o: f32| ((v - o) / unit + BIAS).floor() as i32;
        (axis(p[0], self.offset[0]), axis(p[1], self.offset[1]))
    }

    /// The canvas point of world pixel `world`'s top-left corner.
    pub fn to_screen(&self, world: (i32, i32), art_scale: u32) -> [f32; 2] {
        let unit = self.scale() * art_scale.max(1) as f32;
        [self.offset[0] + world.0 as f32 * unit, self.offset[1] + world.1 as f32 * unit]
    }

    /// Zooms by `steps` index steps about canvas point `about`: the art
    /// point under it stays (to within the pixel snap).
    pub fn zoom_about(&mut self, steps: i32, about: [f32; 2]) {
        let old = self.scale();
        let art = [(about[0] - self.offset[0]) / old, (about[1] - self.offset[1]) / old];
        let zoom = (self.zoom as i64 + i64::from(steps)).clamp(0, ZOOMS.len() as i64 - 1);
        self.zoom = zoom as usize;
        let new = self.scale();
        self.offset = [about[0] - art[0] * new, about[1] - art[1] * new];
        self.snap();
    }

    pub fn pan(&mut self, delta: [f32; 2]) {
        self.offset = [self.offset[0] + delta[0], self.offset[1] + delta[1]];
        self.snap();
    }

    /// At integer scales the offset is whole points: art pixels land on
    /// screen points (pixel-snapped).
    fn snap(&mut self) {
        if self.scale() >= 1.0 {
            self.offset = self.offset.map(f32::round);
        }
    }
}
