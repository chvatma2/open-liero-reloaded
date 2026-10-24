//! Heat (issue #39, spec `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`):
//! one temperature per world pixel of a level with surface materials,
//! deposited by the caller ([`HeatDeposit`]), spread between neighbours,
//! cooled to the air, drawn by each pixel's material's colour ramp and cast
//! into the glow light. Presentation only. No OLX counterpart.
//!
//! The temperature lives in an `R16Uint` texture at world size, the engine's
//! scale (0-[`olr_look::MAX_TEMPERATURE`]) × [`HEAT_SCALE`]; a step reads it
//! and writes a scratch texture over the active boxes, copied back, so the
//! texture the look reads is always whole. The boxes are tracked on the CPU
//! ([`HeatSim`]): deposits add them, conduction grows them a pixel a step
//! (at most [`HEAT_SPREAD`] past what was deposited), and each is dropped
//! (zeroed) once an upper bound of its temperature is below the level's
//! visible threshold. No GPU read-back. With no box nothing runs.

use olr_look::{MAX_TEMPERATURE, SurfaceMaps, SurfaceMaterial};
use olr_scene::{DrawList, View};
use olr_terrain::Rect;

use super::timing::{PassGroup, Timer};
use super::{entry, pipeline};

/// The match time a heat step covers, ms: heat advances by whole steps of
/// the match clock ([`super::Renderer::advance_heat`]), never per frame.
pub const HEAT_STEP_MS: u32 = 20;

/// The most heat deposits a frame takes; later ones are dropped.
pub const MAX_HEAT_DEPOSITS: usize = 64;

/// A deposit's largest radius, world pixels.
pub const MAX_HEAT_RADIUS: f32 = 64.0;

/// A deposit's largest amount: the temperature it gives stone at its centre.
pub const MAX_HEAT_AMOUNT: f32 = MAX_TEMPERATURE as f32;

/// The most heat steps a frame runs; the rest wait for the next frame.
pub const MAX_HEAT_STEPS: u32 = 250;

/// Texel units a degree of the engine's scale (`R16Uint`: 1000 is 64000).
pub const HEAT_SCALE: f32 = 64.0;

/// The hottest texel value.
const HEAT_MAX: f32 = MAX_TEMPERATURE as f32 * HEAT_SCALE;

/// How far heat may spread past a deposit's circle, world pixels: a box
/// stops growing there (heat that far out is long cold).
pub(super) const HEAT_SPREAD: i32 = 48;

/// The most layer resets waiting for a frame (issue #67); past them a
/// rectangle joins the last one's (a larger rectangle compares more
/// pixels and resets the same ones).
const MAX_HEAT_LAYERS: usize = 64;

/// The most boxes kept; past them the two closest are joined.
const MAX_BOXES: usize = 8;

/// Steps a box is kept after its last deposit, however cool its bound:
/// faint hits below the visible threshold may pile up (2 s).
const HOLD_STEPS: u32 = 100;

/// Steps a box is kept at most (5 min of match time): a hostile material
/// that never cools lets go of the pass.
const MAX_LIFE_STEPS: u32 = 15_000;

/// Stone's share of a neighbour's difference taken a step (conductivity
/// 100, capacity 100).
const CONDUCT: f32 = 0.05;

/// The largest share a pixel takes from one neighbour: four of them sum
/// below 1, so a step is a convex combination of the neighbours (stable
/// for every allowed value, never above the hottest).
const MAX_COUPLING: f32 = 0.24;

/// Stone's share of its temperature lost to the air a step (cooling 100).
const COOL_RATE: f32 = 0.006;

/// Stone's least loss to the air a step, degrees: a pixel cools to nothing.
const COOL_MIN: f32 = 0.05;

/// A pixel not beside the air loses this share of an exposed one's loss
/// (every pixel cools in the end, even inside an insulator).
const BULK: f32 = 0.25;

/// The rock or dirt pixels a deposit heats along its line of sight
/// (issue #56): the first `HEAT_SKIN` solid ones a walk from its centre
/// meets; what lies past them takes nothing (conduction carries it in).
pub const HEAT_SKIN: usize = 2;

/// The share of a deposit the skin takes, × [`HEAT_SKIN_WEIGHTS`] (issue
/// #56; the maintainer tunes it in play).
pub const HEAT_SKIN_GAIN: f32 = 1.0;

/// The skin's k-th solid pixel's weight along the walk (issue #56): the
/// face whole, the next pixel in half.
pub const HEAT_SKIN_WEIGHTS: [f32; HEAT_SKIN] = [1.0, 0.5];

/// The dirt pixels a deposit heats along its line of sight (issue #65):
/// the first `DIRT_SKIN` solid ones while all met so far are dirt; once
/// the walk meets rock, [`HEAT_SKIN`] holds for all after it (rock
/// shields the dirt behind it).
pub const DIRT_SKIN: usize = 6;

/// The share of a deposit dirt takes, × [`DIRT_SKIN_WEIGHTS`], with no
/// falloff within its radius (issue #65): the hole is where the blast
/// was, so its wall takes the whole of it. The maintainer tunes it in
/// play.
pub const DIRT_HEAT_GAIN: f32 = 2.5;

/// The dirt skin's k-th solid pixel's weight along the walk (issue #65).
pub const DIRT_SKIN_WEIGHTS: [f32; DIRT_SKIN] = [1.0, 1.0, 0.8, 0.6, 0.4, 0.2];

/// The most of a deposit's amount any pixel takes, × its material's gain:
/// the dirt skin's face, [`HEAT_SKIN_GAIN`]'s, or a back pixel's whole
/// slice ([`Bounds`]: a box is kept while its dirt may still glow).
const MOST_REACH: f32 = {
    let (dirt, rock) = (DIRT_HEAT_GAIN * DIRT_SKIN_WEIGHTS[0], HEAT_SKIN_GAIN * HEAT_SKIN_WEIGHTS[0]);
    let most = if dirt > rock { dirt } else { rock };
    if most > 1.0 { most } else { 1.0 }
};

/// The most pixels a deposit's walk visits: a segment no longer than
/// [`MAX_HEAT_RADIUS`] crosses at most its two extents' cells and one.
const HEAT_WALK_STEPS: i32 = 2 * MAX_HEAT_RADIUS as i32 + 4;

/// The light a fully glowing hot pixel casts, × its colour (linear) × its
/// material's `light` (a glow group's at 100 % intensity casts 1).
pub(super) const HEAT_CAST: f32 = 1.0;

/// The reach of a hot pixel's light, world pixels.
pub(super) const HEAT_REACH: f32 = 200.0;

/// The colour ramps' temperature bins (`RAMP_BINS` columns from 0 to
/// [`MAX_TEMPERATURE`], one row a material value).
const RAMP_BINS: u32 = 256;

/// The heat texture's format.
const HEAT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

/// The materials table's floats: a head vec4, then two vec4s for each of
/// the 256 material values (0 none).
const PARAM_FLOATS: usize = 4 + 8 * 256;

/// Heat given to a level at one point (issue #39): for the next frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatDeposit {
    /// World pixels.
    pub pos: [f32; 2],
    /// World pixels, 0.5 to [`MAX_HEAT_RADIUS`]; the heat falls off as
    /// `1 - (d / radius)²`.
    pub radius: f32,
    /// The temperature it gives stone (sensitivity 100, capacity 100) at
    /// its centre, 0 to [`MAX_HEAT_AMOUNT`]; a material takes `amount ×
    /// sensitivity / capacity`.
    pub amount: f32,
}

impl HeatDeposit {
    /// The deposit within its bounds; `None` when it gives nothing or is
    /// not a number.
    fn clamped(self) -> Option<HeatDeposit> {
        let finite = self.pos.iter().all(|p| p.is_finite() && p.abs() < 1e6);
        if !finite || !self.radius.is_finite() || self.amount.is_nan() || self.amount <= 0.0 {
            return None;
        }
        Some(HeatDeposit {
            pos: self.pos,
            radius: self.radius.clamp(0.5, MAX_HEAT_RADIUS),
            amount: self.amount.min(MAX_HEAT_AMOUNT),
        })
    }

    /// The world pixels it can reach: `[x0, y0, x1, y1)`.
    fn rect(&self) -> [i32; 4] {
        let r = self.radius;
        [
            (self.pos[0] - r).floor() as i32,
            (self.pos[1] - r).floor() as i32,
            (self.pos[0] + r).ceil() as i32 + 1,
            (self.pos[1] + r).ceil() as i32 + 1,
        ]
    }
}

/// What the level's materials allow (the bounds the boxes keep).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Bounds {
    /// The most any material's pixel warms from a deposit of 1 (issue
    /// #65: dirt's gain included).
    gain: f32,
    /// The slowest cooling's share a step and least loss (degrees), as a
    /// pixel inside rock loses them.
    alpha: f32,
    beta: f32,
    /// The coolest temperature any material shows (its ramp's first key).
    visible: f32,
}

/// The level's materials as the shaders read them: the table (a head, then
/// each value's two vec4s: gain, conduction, 1 / capacity, cooling; light),
/// the colour ramps' texels and the bounds. `None` when no material heats.
pub(super) fn tables(materials: &[SurfaceMaterial]) -> Option<(Vec<f32>, Vec<u8>, Bounds)> {
    let mut params = vec![0.0f32; PARAM_FLOATS];
    let mut ramp = vec![0u8; (RAMP_BINS * 256 * 4) as usize];
    let mut b =
        Bounds { gain: 0.0, alpha: f32::INFINITY, beta: f32::INFINITY, visible: MAX_TEMPERATURE as f32 };
    let mut any = false;
    for (i, m) in materials.iter().take(olr_look::MAX_MATERIALS).enumerate() {
        let Some(h) = m.heats() else { continue };
        any = true;
        let v = i + 1;
        let c = f32::from(h.capacity()) / 100.0;
        let gain = f32::from(h.sensitivity()) / 100.0 / c;
        let k = f32::from(h.conductivity()) / 100.0 * CONDUCT;
        let q = f32::from(h.cooling()) / 100.0;
        let at = 4 + 8 * v;
        params[at..at + 8].copy_from_slice(&[
            gain,
            k,
            1.0 / c,
            q,
            f32::from(h.light()) / 100.0,
            0.0,
            0.0,
            0.0,
        ]);
        b.gain = b.gain.max(gain * MOST_REACH);
        b.alpha = b.alpha.min(q * COOL_RATE * BULK);
        b.beta = b.beta.min(q * COOL_MIN * BULK);
        b.visible = b.visible.min(f32::from(h.ramp().first().map_or(MAX_TEMPERATURE, |k| k.temperature)));
        for bin in 0..RAMP_BINS {
            let t = (bin * u32::from(MAX_TEMPERATURE) / (RAMP_BINS - 1)) as u16;
            if let Some((rgb, glow)) = h.look_at(t) {
                let o = ((v as u32 * RAMP_BINS + bin) * 4) as usize;
                ramp[o..o + 4].copy_from_slice(&[
                    rgb[0],
                    rgb[1],
                    rgb[2],
                    (u32::from(glow) * 255 / 100) as u8,
                ]);
            }
        }
    }
    any.then_some((params, ramp, b))
}

/// The surface maps as one `Rg8Uint` texel a world pixel (front, back);
/// `None` when they do not fit `world` or name no material.
pub(super) fn surface_texels(maps: &SurfaceMaps, world: (u32, u32)) -> Option<Vec<u8>> {
    if !maps.fits(world) || maps.is_empty() {
        return None;
    }
    Some(maps.front.iter().zip(&maps.back).flat_map(|(&f, &b)| [f, b]).collect())
}

/// Where a view showed the level (issue #41): a deposit's slices on the
/// distant layers are placed as the view nearest it draws them
/// ([`super::Renderer::deposit_heat_seen`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatView {
    /// The view's rectangle, world pixels ([`DrawList::view`]).
    pub view: View,
    /// The camera its distant layers scroll by ([`DrawList::camera`]).
    pub camera: (f32, f32),
}

impl HeatView {
    /// How `list` shows the level.
    pub fn of(list: &DrawList) -> HeatView {
        HeatView { view: list.view, camera: list.camera() }
    }

    /// A view whose distant layers scroll by its centre (play's: a list
    /// without [`DrawList::parallax`]).
    pub fn centred(view: View) -> HeatView {
        HeatView { view, camera: (view.x + view.w / 2.0, view.y + view.h / 2.0) }
    }
}

/// The view of `views` nearest `pos` (by the distance to its rectangle,
/// 0 inside; the first of those as near), `None` for none.
fn nearest(views: &[HeatView], pos: [f32; 2]) -> Option<&HeatView> {
    let away = |v: &HeatView| {
        let gap = |p: f32, a: f32, len: f32| (a - p).max(p - (a + len)).max(0.0);
        let (dx, dy) = (gap(pos[0], v.view.x, v.view.w), gap(pos[1], v.view.y, v.view.h));
        let d = dx * dx + dy * dy;
        if d.is_nan() { f32::INFINITY } else { d }
    };
    views
        .iter()
        .fold(None, |best: Option<(&HeatView, f32)>, v| {
            let d = away(v);
            match best {
                Some((_, b)) if b <= d => best,
                _ => Some((v, d)),
            }
        })
        .map(|(v, _)| v)
}

/// Whether a distant layer of material value `v` at `depth` percent gets
/// heat (issue #41): its material heats ([`tables`]' `params`: 1 /
/// capacity above 0) and the largest heat radius reaches its depth
/// ([`olr_look::engine::layer_z`], issue #65: depth 45 at most).
pub(super) fn layer_heats(params: &[f32], v: u8, depth: u8) -> bool {
    let heats = v > 0 && params.get(4 + 8 * usize::from(v) + 2).is_some_and(|&c| c > 0.0);
    let depth = depth.clamp(olr_data::level::MIN_DEPTH, olr_data::level::MAX_DEPTH);
    heats && olr_look::engine::layer_z(depth) < MAX_HEAT_RADIUS
}

/// A distant layer's surface texels (issue #41): `v` front and back on
/// each of its `world` pixels (`scale` art pixels a side) that its image
/// covers anywhere, none elsewhere.
pub(super) fn layer_texels(
    image: &olr_data::image::RgbaImage,
    scale: u32,
    world: (u32, u32),
    v: u8,
) -> Vec<u8> {
    let mut out = vec![0u8; world.0 as usize * world.1 as usize * 2];
    for y in 0..world.1 {
        for x in 0..world.0 {
            let covered = (0..scale).any(|j| {
                (0..scale).any(|i| {
                    let (ax, ay) = (x * scale + i, y * scale + j);
                    ax < image.width() && ay < image.height() && image.get(ax, ay).a != 0
                })
            });
            if covered {
                let o = (y as usize * world.0 as usize + x as usize) * 2;
                out[o..o + 2].copy_from_slice(&[v, v]);
            }
        }
    }
    out
}

/// A distant layer's solid map (issue #41): all background, so every
/// pixel faces the air (cooling whole) and takes the deposit whole, by
/// its falloff: its back depth is 0, as the deposit is already the
/// ball's slice at the layer (issue #65).
fn layer_solid(device: &wgpu::Device, queue: &super::uploads::Queue, world: (u32, u32)) -> wgpu::Texture {
    use wgpu::TextureUsages as U;
    let t = plain_texture(
        device,
        "olr-render distant solid",
        world,
        wgpu::TextureFormat::R8Uint,
        U::TEXTURE_BINDING | U::COPY_DST | U::COPY_SRC,
    );
    let ones = vec![1u8; world.0 as usize * world.1 as usize];
    queue.write_texture(
        t.as_image_copy(),
        &ones,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(world.0),
            rows_per_image: Some(world.1),
        },
        wgpu::Extent3d { width: world.0, height: world.1, depth_or_array_layers: 1 },
    );
    t
}

/// An active box: world pixels `[x0, y0, x1, y1)` that may hold heat.
#[derive(Clone, Copy, Debug, PartialEq)]
struct HeatBox {
    rect: [i32; 4],
    /// The farthest it grows (its deposits' circles and [`HEAT_SPREAD`]).
    limit: [i32; 4],
    /// An upper bound of its temperature (degrees).
    hot: f32,
    /// Steps since its last deposit, and since it was made.
    quiet: u32,
    age: u32,
}

fn union(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

fn intersect(a: [i32; 4], b: [i32; 4]) -> [i32; 4] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}

fn empty(r: [i32; 4]) -> bool {
    r[2] <= r[0] || r[3] <= r[1]
}

fn area(r: [i32; 4]) -> i64 {
    i64::from(r[2] - r[0]).max(0) * i64::from(r[3] - r[1]).max(0)
}

/// Whether `a` and `b` overlap or touch (a pixel of one beside one of the other).
fn near(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// One GPU operation a frame encodes, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum HeatOp {
    /// The whole texture to zero (a reset).
    Clear,
    /// Deposits `[start, end)` of the frame's over the boxes.
    Deposit(u32, u32),
    /// A step over the boxes.
    Step,
    /// A dropped box's pixels to zero.
    Zero([u32; 4]),
    /// Issue #67: the pixels of world rectangle `[x, y, w, h]` that
    /// changed layer since the heat last saw them to zero (over the boxes
    /// within it), then the rectangle seen.
    Layer([u32; 4]),
}

impl HeatOp {
    /// The group its pass is timed in.
    fn group(self) -> PassGroup {
        match self {
            HeatOp::Clear => PassGroup::HeatClear,
            HeatOp::Deposit(..) => PassGroup::HeatDeposit,
            HeatOp::Step => PassGroup::HeatStep,
            HeatOp::Zero(_) => PassGroup::HeatZero,
            HeatOp::Layer(_) => PassGroup::HeatLayer,
        }
    }
}

/// What waits for a frame, in call order (issue #67).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Waiting {
    Deposit(HeatDeposit),
    /// [`HeatSim::relayer`]'s world rectangle.
    Layer([i32; 4]),
}

/// The CPU side of a level's heat: the clock, the frame's deposits and the
/// active boxes; [`HeatSim::plan`] turns them into the frame's
/// [`HeatOp`]s, the same however the match time was split between frames.
#[derive(Debug)]
pub(super) struct HeatSim {
    /// The heat texture's size (world pixels the device keeps).
    size: (i32, i32),
    bounds: Bounds,
    boxes: Vec<HeatBox>,
    /// Match time not yet a whole step.
    ms: u32,
    /// Steps the match time has asked for, and those taken.
    due: u64,
    done: u64,
    /// Deposits and layer resets waiting, in call order, each with the
    /// step it lands before.
    pending: Vec<(u64, Waiting)>,
    clear: bool,
    /// The frame's ops, its deposits (in the order [`HeatOp::Deposit`]
    /// ranges index) and its boxes' rectangles after each op (clipped,
    /// `[x, y, w, h]`), reused.
    pub(super) ops: Vec<(HeatOp, std::ops::Range<usize>)>,
    pub(super) deposits: Vec<HeatDeposit>,
    pub(super) rects: Vec<[u32; 4]>,
}

impl HeatSim {
    pub(super) fn new(size: (u32, u32), bounds: Bounds) -> HeatSim {
        HeatSim {
            size: (size.0.min(i32::MAX as u32) as i32, size.1.min(i32::MAX as u32) as i32),
            bounds,
            boxes: Vec::with_capacity(MAX_BOXES + 1),
            ms: 0,
            due: 0,
            done: 0,
            pending: Vec::with_capacity(MAX_HEAT_DEPOSITS + MAX_HEAT_LAYERS),
            clear: false,
            ops: Vec::new(),
            deposits: Vec::with_capacity(MAX_HEAT_DEPOSITS),
            rects: Vec::new(),
        }
    }

    /// Whether any pixel may be hot (a box, or a deposit waiting).
    /// The active boxes' pixels, and how many boxes.
    pub(super) fn area(&self) -> (u64, u32) {
        let px = |b: &HeatBox| u64::from(b.rect[2].max(0) as u32) * u64::from(b.rect[3].max(0) as u32);
        (self.boxes.iter().map(px).sum(), self.boxes.len() as u32)
    }

    pub(super) fn active(&self) -> bool {
        !self.boxes.is_empty() || !self.pending.is_empty()
    }

    /// The deposits waiting for a frame.
    pub(super) fn waiting(&self) -> usize {
        self.pending.iter().filter(|(_, w)| matches!(w, Waiting::Deposit(_))).count()
    }

    /// The steps the match time asked for that have not run.
    pub(super) fn behind(&self) -> u64 {
        self.due - self.done
    }

    /// Queues `d` for the next frame (dropped past [`MAX_HEAT_DEPOSITS`],
    /// or when it gives nothing or misses the level).
    pub(super) fn deposit(&mut self, d: HeatDeposit) {
        let Some(d) = d.clamped() else { return };
        if self.waiting() >= MAX_HEAT_DEPOSITS || empty(intersect(d.rect(), self.level())) {
            return;
        }
        self.pending.push((self.due, Waiting::Deposit(d)));
    }

    /// Queues issue #67's layer reset of world rectangle `r` (`[x0, y0,
    /// x1, y1)`), whose solid map was just rewritten: after every deposit
    /// and step already queued, before any queued later. One right after
    /// another joins it; past [`MAX_HEAT_LAYERS`] it joins the last.
    pub(super) fn relayer(&mut self, r: [i32; 4]) {
        let r = intersect(r, self.level());
        if empty(r) {
            return;
        }
        let layers = self.pending.iter().filter(|(_, w)| matches!(w, Waiting::Layer(_))).count();
        let due = self.due;
        let next = matches!(self.pending.last(), Some(&(at, Waiting::Layer(_))) if at == due);
        let last = self.pending.iter_mut().rev().find_map(|(_, w)| match w {
            Waiting::Layer(l) => Some(l),
            Waiting::Deposit(_) => None,
        });
        if let Some(l) = last
            && (next || layers >= MAX_HEAT_LAYERS)
        {
            *l = union(*l, r);
            return;
        }
        self.pending.push((due, Waiting::Layer(r)));
    }

    /// Advances the match clock by `ms`.
    pub(super) fn advance(&mut self, ms: u32) {
        let total = u64::from(self.ms) + u64::from(ms);
        self.due += total / u64::from(HEAT_STEP_MS);
        self.ms = (total % u64::from(HEAT_STEP_MS)) as u32;
        if !self.active() {
            self.done = self.due;
        }
    }

    /// Forgets every box, deposit and the clock: the next frame zeroes the
    /// whole texture.
    pub(super) fn reset(&mut self) {
        self.boxes.clear();
        self.pending.clear();
        (self.ms, self.due, self.done) = (0, 0, 0);
        self.clear = true;
    }

    fn level(&self) -> [i32; 4] {
        [0, 0, self.size.0, self.size.1]
    }

    /// The frame's ops (none when nothing is hot): the waiting deposits
    /// each before its step, the steps due (at most [`MAX_HEAT_STEPS`]; the
    /// rest wait), each dropped box zeroed.
    pub(super) fn plan(&mut self) {
        self.ops.clear();
        self.deposits.clear();
        self.rects.clear();
        if std::mem::take(&mut self.clear) {
            self.push(HeatOp::Clear, false);
        }
        let mut steps = 0;
        let mut next = 0;
        // The segment (up to `until`) whose boxes were found not to cool in it.
        let mut checked = u64::MAX;
        loop {
            // The deposits landing before the next step, together, and the
            // layer resets between them in call order (issue #67).
            let mut start = self.deposits.len();
            while let Some(&(at, w)) = self.pending.get(next)
                && at <= self.done
            {
                match w {
                    Waiting::Deposit(d) => {
                        self.deposits.push(d);
                        self.add(d);
                    }
                    Waiting::Layer(r) => {
                        self.deposit_op(start);
                        self.layer(r);
                        start = self.deposits.len();
                    }
                }
                next += 1;
            }
            self.deposit_op(start);
            let until = self.pending.get(next).map_or(self.due, |&(at, _)| at);
            if self.done >= until || steps >= MAX_HEAT_STEPS {
                if self.done >= self.due || steps >= MAX_HEAT_STEPS {
                    break;
                }
                continue;
            }
            if self.boxes.is_empty() {
                self.done = until;
                continue;
            }
            // Every box would drop before the next deposit: zero them now
            // (stepping never warms a box, so once per segment).
            if checked != until {
                checked = until;
                if self.cools_within(until - self.done) {
                    while let Some(b) = self.boxes.pop() {
                        self.zero(b.rect);
                    }
                    self.done = until;
                    continue;
                }
            }
            // The step covers each box grown by the pixel heat spreads.
            self.grow();
            self.push(HeatOp::Step, true);
            self.cool();
            self.done += 1;
            steps += 1;
        }
        self.pending.drain(..next);
        if self.pending.is_empty() && self.boxes.is_empty() {
            self.done = self.due;
        }
        // Waiting deposits land before the steps still due.
        for p in &mut self.pending {
            p.0 = p.0.max(self.done);
        }
    }

    /// Records the deposits from `start` as one op, when there are any.
    fn deposit_op(&mut self, start: usize) {
        if self.deposits.len() > start {
            self.push(HeatOp::Deposit(start as u32, self.deposits.len() as u32), true);
        }
    }

    /// Records a layer reset of `r` (issue #67) over the boxes within it.
    fn layer(&mut self, r: [i32; 4]) {
        let r = intersect(r, self.level());
        if empty(r) {
            return;
        }
        let start = self.rects.len();
        for b in &self.boxes {
            let o = intersect(b.rect, r);
            if !empty(o) {
                self.rects.push([o[0] as u32, o[1] as u32, (o[2] - o[0]) as u32, (o[3] - o[1]) as u32]);
            }
        }
        let rect = [r[0] as u32, r[1] as u32, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32];
        self.ops.push((HeatOp::Layer(rect), start..self.rects.len()));
    }

    /// Records `op` with the boxes' rectangles when `boxes`.
    fn push(&mut self, op: HeatOp, boxes: bool) {
        let start = self.rects.len();
        if boxes {
            for b in &self.boxes {
                let r = intersect(b.rect, [0, 0, self.size.0, self.size.1]);
                if !empty(r) {
                    self.rects.push([r[0] as u32, r[1] as u32, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32]);
                }
            }
        }
        self.ops.push((op, start..self.rects.len()));
    }

    fn zero(&mut self, r: [i32; 4]) {
        let r = intersect(r, self.level());
        if !empty(r) {
            let rect = [r[0] as u32, r[1] as u32, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32];
            self.ops.push((HeatOp::Zero(rect), 0..0));
        }
    }

    /// Adds deposit `d`'s box and raises the bounds of those it joins.
    fn add(&mut self, d: HeatDeposit) {
        let level = self.level();
        let rect = intersect(d.rect(), level);
        let g = HEAT_SPREAD;
        let limit = intersect([rect[0] - g, rect[1] - g, rect[2] + g, rect[3] + g], level);
        let warm = (d.amount * self.bounds.gain).min(MAX_TEMPERATURE as f32);
        // A box it reaches gains its heat on top of what it holds.
        let hot = self.boxes.iter().filter(|b| near(b.rect, rect)).fold(0.0f32, |h, b| h.max(b.hot));
        self.boxes.push(HeatBox {
            rect,
            limit,
            hot: (hot + warm).min(MAX_TEMPERATURE as f32),
            quiet: 0,
            age: 0,
        });
        self.join();
    }

    /// Joins boxes that overlap or touch, then the two whose union is
    /// smallest while there are more than [`MAX_BOXES`].
    fn join(&mut self) {
        let merge = |a: HeatBox, b: HeatBox| HeatBox {
            rect: union(a.rect, b.rect),
            limit: union(a.limit, b.limit),
            hot: a.hot.max(b.hot),
            quiet: a.quiet.min(b.quiet),
            age: a.age.min(b.age),
        };
        'again: loop {
            for i in 0..self.boxes.len() {
                for j in i + 1..self.boxes.len() {
                    if near(self.boxes[i].rect, self.boxes[j].rect) {
                        let b = self.boxes.swap_remove(j);
                        self.boxes[i] = merge(self.boxes[i], b);
                        continue 'again;
                    }
                }
            }
            break;
        }
        while self.boxes.len() > MAX_BOXES {
            let mut best = (0, 1, i64::MAX);
            for i in 0..self.boxes.len() {
                for j in i + 1..self.boxes.len() {
                    let a = area(union(self.boxes[i].rect, self.boxes[j].rect));
                    if a < best.2 {
                        best = (i, j, a);
                    }
                }
            }
            let b = self.boxes.swap_remove(best.1);
            self.boxes[best.0] = merge(self.boxes[best.0], b);
            self.join();
        }
    }

    /// Grows each box a pixel (to its limit): where a step's heat may go.
    fn grow(&mut self) {
        let level = self.level();
        for b in &mut self.boxes {
            let grown = [b.rect[0] - 1, b.rect[1] - 1, b.rect[2] + 1, b.rect[3] + 1];
            b.rect = intersect(intersect(grown, b.limit), level);
        }
        self.join();
    }

    /// A step's bookkeeping after the GPU's: each box cools; one whose
    /// bound shows nothing (or too old) is dropped and zeroed.
    fn cool(&mut self) {
        let Bounds { alpha, beta, visible, .. } = self.bounds;
        for b in &mut self.boxes {
            b.hot = (b.hot * (1.0 - alpha) - beta).max(0.0);
            b.quiet = b.quiet.saturating_add(1);
            b.age = b.age.saturating_add(1);
        }
        let mut i = 0;
        while i < self.boxes.len() {
            let b = self.boxes[i];
            let cold = b.hot < visible && b.quiet >= HOLD_STEPS;
            if cold || b.age >= MAX_LIFE_STEPS || empty(b.rect) {
                self.boxes.swap_remove(i);
                self.zero(b.rect);
            } else {
                i += 1;
            }
        }
    }

    /// Whether every box is dropped within `steps` steps.
    fn cools_within(&self, steps: u64) -> bool {
        let Bounds { alpha, beta, visible, .. } = self.bounds;
        self.boxes.iter().all(|b| {
            let (mut hot, mut quiet, mut age) = (b.hot, b.quiet, b.age);
            for _ in 0..steps.min(u64::from(MAX_LIFE_STEPS) + 1) {
                hot = (hot * (1.0 - alpha) - beta).max(0.0);
                quiet = quiet.saturating_add(1);
                age = age.saturating_add(1);
                if (hot < visible && quiet >= HOLD_STEPS) || age >= MAX_LIFE_STEPS {
                    return true;
                }
            }
            false
        })
    }
}

/// The heat passes' pipelines and layouts (one set per lit renderer).
pub(super) struct HeatPipelines {
    /// What the lit terrain and the emission read: the table, the heat,
    /// the surface maps, the colour ramps, the solid map.
    pub(super) look_layout: wgpu::BindGroupLayout,
    step_layout: wgpu::BindGroupLayout,
    step: wgpu::RenderPipeline,
    deposit: wgpu::RenderPipeline,
    zero: wgpu::RenderPipeline,
    /// Issue #67's layer reset.
    layer: wgpu::RenderPipeline,
    /// A level without heat's look bind group (the table's head off).
    pub(super) none: wgpu::BindGroup,
}

const UINT: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Uint,
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

const FLOAT: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: false },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

/// The WGSL the heat passes and readers share, after the Rust constants.
pub(super) fn heat_common() -> String {
    format!(
        "const HEAT_SCALE: f32 = {HEAT_SCALE:?};\nconst HEAT_MAX: f32 = {HEAT_MAX:?};\nconst RAMP_BINS: i32 = {RAMP_BINS};\nconst HEAT_CAST: f32 = {HEAT_CAST:?};\nconst HEAT_REACH: f32 = {HEAT_REACH:?};\nconst MAX_COUPLING: f32 = {MAX_COUPLING:?};\nconst COOL_RATE: f32 = {COOL_RATE:?};\nconst COOL_MIN: f32 = {COOL_MIN:?};\nconst BULK: f32 = {BULK:?};\nconst HEAT_SKIN: u32 = {HEAT_SKIN}u;\nconst HEAT_SKIN_GAIN: f32 = {HEAT_SKIN_GAIN:?};\nconst HEAT_SKIN_WEIGHTS = array<f32, {HEAT_SKIN}>({});\nconst DIRT_SKIN: u32 = {DIRT_SKIN}u;\nconst DIRT_HEAT_GAIN: f32 = {DIRT_HEAT_GAIN:?};\nconst DIRT_SKIN_WEIGHTS = array<f32, {DIRT_SKIN}>({});\nconst HEAT_WALK_STEPS: i32 = {HEAT_WALK_STEPS};\nconst MAX_HEAT_DEPOSITS: u32 = {MAX_HEAT_DEPOSITS}u;\n{}",
        HEAT_SKIN_WEIGHTS.map(|w| format!("{w:?}")).join(", "),
        DIRT_SKIN_WEIGHTS.map(|w| format!("{w:?}")).join(", "),
        include_str!("../look/heat_common.wgsl")
    )
}

/// [`heat_common`] and the heat look's bindings at bind group `group` with
/// `heat_at` (the lit terrain's group 3, the emission's 1).
pub(super) fn heat_reader(group: u32) -> String {
    format!(
        "{}\n{}",
        heat_common(),
        include_str!("../look/heat_reader.wgsl").replace("HEAT_GROUP", &group.to_string())
    )
}

fn heat_texture(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    plain_texture(device, label, size, HEAT_FORMAT, usage)
}

pub(super) fn plain_texture(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn uniform(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(super) fn view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor::default())
}

fn look_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params: &wgpu::Buffer,
    [heat, surface, ramp, solid]: [&wgpu::TextureView; 4],
) -> wgpu::BindGroup {
    let tv = wgpu::BindingResource::TextureView;
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("olr-render heat look"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: tv(heat) },
            wgpu::BindGroupEntry { binding: 2, resource: tv(surface) },
            wgpu::BindGroupEntry { binding: 3, resource: tv(ramp) },
            wgpu::BindGroupEntry { binding: 4, resource: tv(solid) },
        ],
    })
}

impl HeatPipelines {
    pub(super) fn new(device: &wgpu::Device) -> HeatPipelines {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let look_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render heat look"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, UINT),
                entry(2, frag, UINT),
                entry(3, frag, FLOAT),
                entry(4, frag, UINT),
            ],
        });
        let step_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render heat step"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, UINT),
                entry(2, frag, UINT),
                entry(3, frag, UINT),
                entry(4, frag, super::UNIFORM),
                entry(5, frag, UINT),
            ],
        });
        let source = format!("{}\n{}", heat_common(), include_str!("../look/heat.wgsl"));
        let ranges = wgpu::vertex_attr_array![0 => Float32x4];
        let make = |label, vs, fs, layouts: &[Option<&wgpu::BindGroupLayout>], instanced: bool| {
            let buffers = [Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ranges,
            })];
            pipeline(
                device,
                label,
                &source,
                (vs, fs),
                layouts,
                if instanced { &buffers } else { &[] },
                &[(HEAT_FORMAT, None)],
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let step = make("olr-render heat step", "vs_full", "fs_heat_step", &[Some(&step_layout)], false);
        let deposit =
            make("olr-render heat deposit", "vs_deposit", "fs_heat_deposit", &[Some(&step_layout)], true);
        let zero = make("olr-render heat zero", "vs_full", "fs_heat_zero", &[], false);
        let layer = make("olr-render heat layer", "vs_full", "fs_heat_layer", &[Some(&step_layout)], false);
        let tex = |label, format| {
            plain_texture(device, label, (1, 1), format, wgpu::TextureUsages::TEXTURE_BINDING)
        };
        let params = uniform(device, "olr-render no heat", (PARAM_FLOATS * 4) as u64);
        let none = look_bind(
            device,
            &look_layout,
            &params,
            [
                &view(&tex("olr-render no heat", HEAT_FORMAT)),
                &view(&tex("olr-render no surface", wgpu::TextureFormat::Rg8Uint)),
                &view(&tex("olr-render no ramp", wgpu::TextureFormat::Rgba8Unorm)),
                &view(&tex("olr-render no solid", wgpu::TextureFormat::R8Uint)),
            ],
        );
        HeatPipelines { look_layout, step_layout, step, deposit, zero, layer, none }
    }
}

/// A level's heat on the GPU (made only when its materials heat) and its
/// CPU side.
pub(super) struct HeatTextures {
    /// The temperatures the look reads (`R16Uint`, world size, cropped as
    /// the solid map).
    pub(super) heat: wgpu::Texture,
    heat_view: wgpu::TextureView,
    /// The surface texels (front, back; `Rg8Uint`), which the material
    /// soak reads too (issue #57: one copy of the map).
    pub(super) surface_view: wgpu::TextureView,
    /// A step's target, copied back over the boxes.
    scratch: wgpu::Texture,
    scratch_view: wgpu::TextureView,
    /// The look's solid map, and the solid map as the heat last saw it
    /// (issue #67: `R8Uint`, the solid map's size): a pixel whose class
    /// differs changed layer and starts cold ([`HeatOp::Layer`]).
    solid: wgpu::Texture,
    seen: wgpu::Texture,
    params: wgpu::Buffer,
    /// The materials' colour ramps (`HeatLook`), rewritten by
    /// [`HeatTextures::set_tables`].
    ramp: wgpu::Texture,
    /// The table as written, its head's `on` set while a box is active.
    table: Vec<f32>,
    deposits: wgpu::Buffer,
    /// The frame's deposit ranges, one vec4 a deposit op (instances).
    ranges: wgpu::Buffer,
    step_bind: wgpu::BindGroup,
    /// The lit terrain's and the emission's.
    pub(super) look_bind: wgpu::BindGroup,
    pub(super) sim: HeatSim,
    /// Whether the table's head says a box is active.
    on: bool,
    /// Steps and passes run so far (tests).
    pub(super) steps: u64,
    pub(super) passes: u64,
}

impl HeatTextures {
    /// The heat of a level of `world` pixels (the textures `size`, the
    /// device's crop) with `surface` texels ([`surface_texels`]) and
    /// [`tables`]' output; `solid` is the look's solid map (issue #67:
    /// copied as the solid map seen, in a submit of its own). A back pixel
    /// lies `back_depth` world pixels behind the deposits' plane (issue
    /// #65: the level's [`olr_look::engine::BACK_DEPTH_PX`]; a distant
    /// layer's 0, its deposits already sliced) and takes the ball's slice
    /// there.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &super::uploads::Queue,
        pipes: &HeatPipelines,
        world: (u32, u32),
        size: (u32, u32),
        surface: &[u8],
        (table, ramp_texels, bounds): (Vec<f32>, Vec<u8>, Bounds),
        solid: &wgpu::Texture,
        back_depth: f32,
    ) -> HeatTextures {
        use wgpu::TextureUsages as U;
        let heat = heat_texture(
            device,
            "olr-render heat",
            size,
            U::TEXTURE_BINDING | U::RENDER_ATTACHMENT | U::COPY_DST | U::COPY_SRC,
        );
        let scratch =
            heat_texture(device, "olr-render heat scratch", size, U::RENDER_ATTACHMENT | U::COPY_SRC);
        let seen = plain_texture(
            device,
            "olr-render heat seen",
            (solid.width(), solid.height()),
            wgpu::TextureFormat::R8Uint,
            U::TEXTURE_BINDING | U::COPY_DST,
        );
        let mut e = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("olr-render heat seen") });
        e.copy_texture_to_texture(solid.as_image_copy(), seen.as_image_copy(), solid.size());
        queue.submit([e.finish()]);
        let solid_view = view(solid);
        let surface_tex = plain_texture(
            device,
            "olr-render surface",
            size,
            wgpu::TextureFormat::Rg8Uint,
            U::TEXTURE_BINDING | U::COPY_DST,
        );
        let (w, h) = (size.0.min(world.0), size.1.min(world.1));
        if w > 0 && h > 0 && surface.len() as u64 >= u64::from(world.0) * u64::from(world.1) * 2 {
            queue.write_texture(
                surface_tex.as_image_copy(),
                surface,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(world.0 * 2),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
        }
        let ramp = plain_texture(
            device,
            "olr-render heat ramps",
            (RAMP_BINS, 256),
            wgpu::TextureFormat::Rgba8Unorm,
            U::TEXTURE_BINDING | U::COPY_DST,
        );
        queue.write_texture(
            ramp.as_image_copy(),
            &ramp_texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(RAMP_BINS * 4),
                rows_per_image: Some(256),
            },
            ramp.size(),
        );
        let params = uniform(device, "olr-render heat table", (PARAM_FLOATS * 4) as u64);
        let mut table = table;
        table[..4].copy_from_slice(&[size.0 as f32, size.1 as f32, 0.0, back_depth]);
        queue.write_buffer(&params, 0, bytemuck::cast_slice(&table));
        let deposits = uniform(device, "olr-render heat deposits", (MAX_HEAT_DEPOSITS * 16) as u64);
        let ranges = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render heat deposit ranges"),
            size: (MAX_HEAT_DEPOSITS * 16) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (heat_view, scratch_view, surface_view) = (view(&heat), view(&scratch), view(&surface_tex));
        let tv = wgpu::BindingResource::TextureView;
        let step_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render heat step"),
            layout: &pipes.step_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tv(&heat_view) },
                wgpu::BindGroupEntry { binding: 2, resource: tv(&surface_view) },
                wgpu::BindGroupEntry { binding: 3, resource: tv(&solid_view) },
                wgpu::BindGroupEntry { binding: 4, resource: deposits.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: tv(&view(&seen)) },
            ],
        });
        let look_bind = look_bind(
            device,
            &pipes.look_layout,
            &params,
            [&heat_view, &surface_view, &view(&ramp), &solid_view],
        );
        HeatTextures {
            heat,
            heat_view,
            surface_view,
            scratch,
            scratch_view,
            solid: solid.clone(),
            seen,
            params,
            ramp,
            table,
            deposits,
            ranges,
            step_bind,
            look_bind,
            sim: HeatSim::new(size, bounds),
            on: false,
            steps: 0,
            passes: 0,
        }
    }

    /// Gives the heat new materials' values ([`tables`]' output, issue
    /// #45), its temperatures, boxes and clock kept: the next frame steps
    /// and draws by them.
    pub(super) fn set_tables(
        &mut self,
        queue: &super::uploads::Queue,
        (table, ramp_texels, bounds): (Vec<f32>, Vec<u8>, Bounds),
    ) {
        let head: [f32; 4] = [self.table[0], self.table[1], self.table[2], self.table[3]];
        self.table = table;
        self.table[..4].copy_from_slice(&head);
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&self.table));
        queue.write_texture(
            self.ramp.as_image_copy(),
            &ramp_texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(RAMP_BINS * 4),
                rows_per_image: Some(256),
            },
            self.ramp.size(),
        );
        self.sim.bounds = bounds;
    }

    /// Plans the frame's heat and writes its deposits; the table's head
    /// says whether the look reads the heat (off once nothing is hot, so
    /// an idle level reads none).
    pub(super) fn prepare(&mut self, queue: &super::uploads::Queue) {
        self.sim.plan();
        let on = self.sim.active() || self.sim.ops.iter().any(|(op, _)| *op != HeatOp::Clear);
        if on != self.on {
            self.on = on;
            self.table[2] = f32::from(u8::from(on));
            queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&self.table[..4]));
        }
        let n = self.sim.deposits.len().min(MAX_HEAT_DEPOSITS);
        if n > 0 {
            // On the stack: no allocation a frame.
            let mut packed = [[0.0f32; 4]; MAX_HEAT_DEPOSITS];
            for (p, d) in packed.iter_mut().zip(&self.sim.deposits) {
                *p = [d.pos[0], d.pos[1], d.radius, d.amount];
            }
            queue.write_buffer(&self.deposits, 0, bytemuck::cast_slice(&packed[..n]));
            let mut ranges = [[0.0f32; 4]; MAX_HEAT_DEPOSITS];
            let mut k = 0;
            for (op, _) in &self.sim.ops {
                if let HeatOp::Deposit(a, b) = *op
                    && k < MAX_HEAT_DEPOSITS
                {
                    ranges[k] = [a as f32, b as f32, 0.0, 0.0];
                    k += 1;
                }
            }
            queue.write_buffer(&self.ranges, 0, bytemuck::cast_slice(&ranges[..k]));
        }
    }

    /// Whether the look reads the heat this frame.
    pub(super) fn on(&self) -> bool {
        self.on
    }

    /// Encodes the frame's planned ops ([`HeatTextures::prepare`]).
    /// The frame's operations into `encoder`, each pass timed in its
    /// [`PassGroup`] and the copies back in [`PassGroup::HeatCopy`] when
    /// `timer` is on; the first pass starts the frame's slot 0 when
    /// `begins_frame`.
    pub(super) fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pipes: &HeatPipelines,
        timer: Option<&Timer>,
        begins_frame: bool,
    ) {
        let mut deposit = 0u32;
        for i in 0..self.sim.ops.len() {
            let (op, rects) = self.sim.ops[i].clone();
            let rects = &self.sim.rects[rects];
            self.passes += 1;
            let (target, load) = match op {
                HeatOp::Clear => (&self.heat_view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)),
                HeatOp::Zero(_) => (&self.heat_view, wgpu::LoadOp::Load),
                HeatOp::Deposit(..) | HeatOp::Step | HeatOp::Layer(_) => {
                    (&self.scratch_view, wgpu::LoadOp::Load)
                }
            };
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("olr-render heat"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: timer
                        .and_then(|t| t.pass(op.group(), (i == 0 && begins_frame).then_some(0), None)),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                match op {
                    HeatOp::Clear => {}
                    HeatOp::Zero([x, y, w, h]) => {
                        pass.set_pipeline(&pipes.zero);
                        pass.set_scissor_rect(x, y, w, h);
                        pass.draw(0..3, 0..1);
                    }
                    HeatOp::Deposit(..) | HeatOp::Step | HeatOp::Layer(_) => {
                        match op {
                            HeatOp::Deposit(..) => {
                                pass.set_pipeline(&pipes.deposit);
                                pass.set_vertex_buffer(0, self.ranges.slice(..));
                            }
                            HeatOp::Step => pass.set_pipeline(&pipes.step),
                            _ => pass.set_pipeline(&pipes.layer),
                        }
                        pass.set_bind_group(0, &self.step_bind, &[]);
                        let instance = deposit..deposit + 1;
                        for &[x, y, w, h] in rects {
                            pass.set_scissor_rect(x, y, w, h);
                            pass.draw(0..3, instance.clone());
                        }
                    }
                }
            }
            if let HeatOp::Deposit(..) | HeatOp::Step | HeatOp::Layer(_) = op {
                match op {
                    HeatOp::Deposit(..) => deposit += 1,
                    HeatOp::Step => self.steps += 1,
                    _ => {}
                }
                let copies = timer.and_then(|t| Some((t, t.commands(PassGroup::HeatCopy)?)));
                if let Some((t, (begin, _))) = copies {
                    encoder.write_timestamp(&t.set, begin);
                }
                for &[x, y, w, h] in rects {
                    let at = |texture| wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    };
                    encoder.copy_texture_to_texture(
                        at(&self.scratch),
                        at(&self.heat),
                        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    );
                }
                if let Some((t, (_, end))) = copies {
                    encoder.write_timestamp(&t.set, end);
                }
            }
            // Issue #67: the solid map as now is what the heat has seen,
            // over a layer reset's rectangle, or whole once cleared.
            let seen = match op {
                HeatOp::Layer(r) => Some(r),
                HeatOp::Clear => Some([0, 0, self.seen.width(), self.seen.height()]),
                _ => None,
            };
            if let Some([x, y, w, h]) = seen {
                let (w, h) =
                    (w.min(self.seen.width().saturating_sub(x)), h.min(self.seen.height().saturating_sub(y)));
                if w > 0 && h > 0 {
                    let at = |texture| wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    };
                    encoder.copy_texture_to_texture(
                        at(&self.solid),
                        at(&self.seen),
                        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    );
                }
            }
        }
    }

    /// The solid map of world rectangle `r` was just rewritten with
    /// `bytes` (`width` a row, as the solid map's upload; issue #67):
    /// while anything may be hot, a layer reset queued in call order
    /// ([`HeatSim::relayer`]); else nothing is hot, nothing to reset, and
    /// the bytes are what the heat has seen, written as the solid map is
    /// (no pass).
    pub(super) fn relayer(&mut self, queue: &super::uploads::Queue, (bytes, width): (&[u8], u32), r: Rect) {
        if self.sim.active() {
            let (x, y) = (r.x.min(i32::MAX as u32) as i32, r.y.min(i32::MAX as u32) as i32);
            let (w, h) = (r.w.min(i32::MAX as u32) as i32, r.h.min(i32::MAX as u32) as i32);
            self.sim.relayer([x, y, x.saturating_add(w), y.saturating_add(h)]);
        } else {
            super::look::write_bytes(queue, &self.seen, (bytes, width), r);
        }
    }
}

impl super::Renderer {
    /// Gives the level its surface materials and maps (issue #39): with
    /// lighting on, a level whose maps are its size and name a material
    /// that heats gets a heat texture, made afresh by every
    /// [`super::Renderer::upload_terrain`] (a new level, a restart) and
    /// now when the terrain is up. `None`, or materials that never heat,
    /// make none: the level draws and costs exactly as without. Issue
    /// #57: such maps and a material that sets its own `light_soak` make
    /// the material soak (`soak.rs`), heat or none; without one the soak
    /// is the level's alone, as before.
    ///
    /// Issue #41: the distant layers take these materials (by
    /// [`Renderer::set_distant_surface`]'s values) at the next
    /// [`Renderer::set_distant`].
    pub fn set_surface(&mut self, materials: &[SurfaceMaterial], maps: Option<&SurfaceMaps>) {
        let (tables, soak) = (tables(materials), super::soak::soak_table(materials));
        self.surface = (tables.is_some() || soak.is_some()).then(|| super::Surface {
            maps: maps.and_then(|m| Some((m.world, surface_texels(m, m.world)?))),
            tables,
            soak,
        });
        if let Some(look) = &mut self.look
            && let Some(t) = &mut look.textures
        {
            let layouts = (&look.heat, &look.soak_material_layout);
            t.set_surface(&self.device, &self.queue, layouts, self.surface.as_ref());
        }
    }

    /// Gives the level new values of its surface materials (issue #45: the
    /// material editor's live preview) and keeps its heat: the same list,
    /// in the same order, as [`Renderer::set_surface`] was given, a
    /// field changed. The temperatures, the active boxes and the clock stay;
    /// the next frame steps and draws by the new values (the distant
    /// layers' too), and soaks by their soaks (issue #57). `false`,
    /// changing nothing, when there is no heat to keep (none was made, or
    /// none of `materials` heats): then [`Renderer::set_surface`] with the
    /// maps is the call.
    pub fn set_surface_materials(&mut self, materials: &[SurfaceMaterial]) -> bool {
        let (Some(tables), Some(surface)) = (tables(materials), self.surface.as_mut()) else {
            return false;
        };
        if self.look.as_ref().and_then(|l| l.textures.as_ref()?.heat.as_ref()).is_none()
            && self.distant.iter().all(|d| d.heat.is_none())
        {
            return false;
        }
        surface.tables = Some(tables.clone());
        surface.soak = super::soak::soak_table(materials);
        let level = self.look.as_mut().and_then(|l| l.textures.as_mut()?.heat.as_mut());
        let layers = self.distant.iter_mut().filter_map(|d| d.heat.as_mut());
        for h in level.into_iter().chain(layers) {
            h.set_tables(&self.queue, tables.clone());
        }
        // The soak lends the heat's map: only its table changes, or it
        // comes or goes.
        if let Some(look) = &mut self.look
            && let Some(t) = &mut look.textures
        {
            t.set_soak(&self.device, &self.queue, &look.soak_material_layout, self.surface.as_ref());
        }
        true
    }

    /// Each distant layer's surface material value (issue #41), in
    /// [`Renderer::set_distant`]'s order: `v` is [`Renderer::set_surface`]'s
    /// `materials[v - 1]`, 0 (or a layer past the list) none. It takes
    /// effect at the next [`Renderer::set_distant`].
    pub fn set_distant_surface(&mut self, values: &[u8]) {
        self.distant_surface.clear();
        self.distant_surface.extend_from_slice(values);
    }

    /// Makes each distant layer's heat afresh (issue #41), or none: a layer
    /// gets one with lighting on and the terrain up when its material heats
    /// and the largest heat radius ([`MAX_HEAT_RADIUS`]) reaches its depth.
    pub(super) fn make_distant_heat(&mut self) {
        let scale = self.terrain.as_ref().map(|t| t.art_scale);
        let max = self.max_dim();
        for d in &mut self.distant {
            d.heat = None;
            let tables = self.surface.as_ref().and_then(|s| s.tables.as_ref());
            let (Some(state), Some(scale), Some(tables)) = (&self.look, scale, tables) else {
                continue;
            };
            let v = self.distant_surface.get(d.index).copied().unwrap_or(0);
            if !layer_heats(&tables.0, v, d.layer.depth) {
                continue;
            }
            let s = scale.max(1);
            let world = ((d.art.0 / s).clamp(1, max), (d.art.1 / s).clamp(1, max));
            let texels = layer_texels(&d.layer.image, s, world, v);
            let solid = layer_solid(&self.device, &self.queue, world);
            d.heat = Some(HeatTextures::new(
                &self.device,
                &self.queue,
                &state.heat,
                world,
                world,
                &texels,
                tables.clone(),
                &solid,
                0.0,
            ));
        }
    }

    fn heat_sims(&mut self) -> impl Iterator<Item = &mut HeatSim> {
        let level = self.look.as_mut().and_then(|l| l.textures.as_mut()?.heat.as_mut());
        let layers = self.distant.iter_mut().filter_map(|d| d.heat.as_mut());
        level.into_iter().chain(layers).map(|h| &mut h.sim)
    }

    fn all_heat(&self) -> impl Iterator<Item = &HeatTextures> {
        let level = self.heat_textures();
        level.into_iter().chain(self.distant.iter().filter_map(|d| d.heat.as_ref()))
    }

    /// Heat for the level at `deposit` (issue #39), landing before the
    /// next step: the next lit frame draws it. At most
    /// [`MAX_HEAT_DEPOSITS`] wait for a frame; the radius and amount are
    /// clamped, one not a number dropped. Nothing without heat. The
    /// level's plane only: [`Renderer::deposit_heat_seen`] heats the
    /// distant layers too.
    pub fn deposit_heat(&mut self, deposit: HeatDeposit) {
        if let Some(h) = self.look.as_mut().and_then(|l| l.textures.as_mut()?.heat.as_mut()) {
            h.sim.deposit(deposit);
        }
    }

    /// [`Renderer::deposit_heat`], and its slices on the distant layers
    /// that have heat (issue #41): the deposit is a ball of its radius
    /// centred on the level's plane; a layer of depth `d` lying
    /// [`olr_look::engine::layer_z`] behind it (issue #65: behind the back
    /// wall, on one depth line with it) takes the ball's
    /// circle there ([`olr_look::engine::heat_slice`]), centred on the
    /// layer pixel drawn behind the deposit by the view of `views` nearest
    /// it (the first of those as near; [`crate::frame::distant_origin`] of
    /// its frame). From then on the heat stays on the layer and moves with
    /// it. No views, no slices.
    pub fn deposit_heat_seen(&mut self, deposit: HeatDeposit, views: &[HeatView]) {
        self.deposit_heat(deposit);
        let (Some(d), Some(view)) = (deposit.clamped(), nearest(views, deposit.pos)) else { return };
        let Some(t) = &self.terrain else { return };
        let s = t.art_scale.max(1);
        let map = (t.art.0 / s, t.art.1 / s);
        let frame = crate::frame::WorldFrame::new(view.view, self.scale());
        let ball = olr_look::engine::ExplosionHeat { radius: d.radius, amount: d.amount };
        for l in &mut self.distant {
            let Some(h) = l.heat.as_mut() else { continue };
            let Some(slice) = olr_look::engine::heat_slice(ball, l.layer.depth) else { continue };
            let at = l.layer.position(view.camera, map);
            let Some(origin) = crate::frame::distant_origin(&frame, at) else { continue };
            let pos = [(f64::from(d.pos[0]) - origin.0) as f32, (f64::from(d.pos[1]) - origin.1) as f32];
            h.sim.deposit(HeatDeposit { pos, radius: slice.radius, amount: slice.amount });
        }
    }

    /// The match time elapsed since the last call, ms (issue #39): the
    /// heat steps once each [`HEAT_STEP_MS`] of it, ticks never drawn
    /// included, at the next lit frame (at most [`MAX_HEAT_STEPS`] a
    /// frame, the rest the frames after). Never the wall clock: paused,
    /// pass 0.
    pub fn advance_heat(&mut self, elapsed_ms: u32) {
        self.heat_sims().for_each(|s| s.advance(elapsed_ms));
    }

    /// Clears the heat (a replay seek), the distant layers' too: the next
    /// lit frame zeroes it. [`super::Renderer::upload_terrain`] starts it
    /// afresh too.
    pub fn reset_heat(&mut self) {
        self.heat_sims().for_each(HeatSim::reset);
    }

    /// Runs the heat's waiting deposits and due steps now, as the next lit
    /// frame would (at most [`MAX_HEAT_STEPS`] steps), in a submit of their
    /// own (issue #40): a host that deposits more than
    /// [`MAX_HEAT_DEPOSITS`] between frames, or lets more steps fall due,
    /// drops and delays nothing. However the work is split between these
    /// and frames, the heat is the same. Nothing without heat.
    pub fn run_heat(&mut self) {
        let Some(state) = self.look.as_mut() else { return };
        let level = state.textures.as_mut().and_then(|t| t.heat.as_mut());
        let layers = self.distant.iter_mut().filter_map(|d| d.heat.as_mut());
        let mut encoder = None;
        for h in level.into_iter().chain(layers) {
            h.prepare(&self.queue);
            if h.sim.ops.is_empty() {
                continue;
            }
            let e = encoder.get_or_insert_with(|| {
                self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("olr-render heat"),
                })
            });
            h.encode(e, &state.heat, None, false);
        }
        if let Some(e) = encoder {
            self.queue.submit([e.finish()]);
        }
    }

    /// The heat deposits waiting for the next frame (at most
    /// [`MAX_HEAT_DEPOSITS`]; 0 without heat): the most of the level's and
    /// each distant layer's (issue #41).
    pub fn heat_waiting(&self) -> usize {
        self.all_heat().map(|h| h.sim.waiting()).max().unwrap_or(0)
    }

    /// The heat steps the match time asked for that have not run yet (0
    /// without heat): past [`MAX_HEAT_STEPS`], the next frame leaves some.
    pub fn heat_behind(&self) -> u64 {
        self.all_heat().map(|h| h.sim.behind()).max().unwrap_or(0)
    }

    /// Whether the level has heat (its materials heat, lighting on), or a
    /// distant layer does (issue #41).
    pub fn has_heat(&self) -> bool {
        self.all_heat().next().is_some()
    }

    /// Whether any pixel may be hot, the distant layers' too: frames run
    /// heat passes.
    pub fn heat_active(&self) -> bool {
        self.all_heat().any(|h| h.sim.active())
    }

    /// Distant layer `i`'s heat texture (`R16Uint` at the layer's world
    /// size, [`HEAT_SCALE`] a degree; issue #41, tests): `i` in
    /// [`super::Renderer::set_distant`]'s list; `None` for a layer without
    /// heat.
    pub fn distant_heat_texture(&self, i: usize) -> Option<&wgpu::Texture> {
        self.distant.iter().find(|d| d.index == i)?.heat.as_ref().map(|h| &h.heat)
    }

    fn heat_textures(&self) -> Option<&HeatTextures> {
        self.look.as_ref()?.textures.as_ref()?.heat.as_ref()
    }

    /// The heat texture (`R16Uint`, [`HEAT_SCALE`] a degree; tests).
    pub fn heat_texture(&self) -> Option<&wgpu::Texture> {
        self.heat_textures().map(|h| &h.heat)
    }

    /// The active heat boxes' pixels and their count, the level's and the
    /// distant layers' summed (the render profile's workload).
    pub(super) fn heat_area(&self) -> (u64, u32) {
        self.all_heat().map(|h| h.sim.area()).fold((0, 0), |(a, n), (b, m)| (a + b, n + m))
    }

    /// Heat steps run so far (tests).
    #[doc(hidden)]
    pub fn heat_steps(&self) -> u64 {
        self.heat_textures().map_or(0, |h| h.steps)
    }

    /// Heat passes run so far (tests: none while nothing is hot).
    #[doc(hidden)]
    pub fn heat_passes(&self) -> u64 {
        self.heat_textures().map_or(0, |h| h.passes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_look::Heat;

    fn stone() -> SurfaceMaterial {
        SurfaceMaterial {
            light_soak: None,
            id: "stone".into(),
            heat: Some(Heat {
                sensitivity: 100,
                capacity: 100,
                conductivity: 100,
                cooling: 100,
                ramp: vec![],
                light: 100,
            }),
        }
    }

    fn sim() -> HeatSim {
        HeatSim::new((200, 100), tables(&[stone()]).unwrap().2)
    }

    fn blast(x: f32, y: f32) -> HeatDeposit {
        HeatDeposit { pos: [x, y], radius: 8.0, amount: 1000.0 }
    }

    #[test]
    fn only_heating_materials_make_tables() {
        let cold = SurfaceMaterial { id: "cold".into(), heat: None, light_soak: None };
        assert!(tables(std::slice::from_ref(&cold)).is_none());
        let mut numb = stone();
        numb.heat.as_mut().unwrap().sensitivity = 0;
        assert!(tables(&[cold, numb.clone()]).is_none(), "sensitivity 0 never heats");
        let (params, ramp, b) = tables(&[numb, stone()]).unwrap();
        assert_eq!(&params[4 + 8..4 + 16], &[0.0; 8], "value 1 does not heat");
        assert_eq!(&params[4 + 16..4 + 20], &[1.0, CONDUCT, 1.0, 1.0]);
        assert_eq!(b.visible, 100.0, "the lava ramp shows from 100");
        let row = |v: usize, bin: usize| &ramp[(v * RAMP_BINS as usize + bin) * 4..][..4];
        assert_eq!(row(2, 0), &[0; 4], "cold shows its art");
        assert_eq!(row(2, 255), &[255, 244, 220, 255], "the ramp's last key at 1000");
    }

    #[test]
    fn nothing_hot_plans_nothing() {
        let mut s = sim();
        s.advance(10_000);
        s.plan();
        assert!(s.ops.is_empty() && !s.active());
        s.deposit(HeatDeposit { amount: f32::NAN, ..blast(10.0, 10.0) });
        s.deposit(blast(-500.0, 10.0));
        s.plan();
        assert!(s.ops.is_empty(), "a deposit of nothing or off the level is dropped");
    }

    #[test]
    fn steps_follow_the_match_clock() {
        let mut s = sim();
        s.deposit(blast(50.0, 50.0));
        s.advance(HEAT_STEP_MS * 3 - 1);
        s.plan();
        let kinds: Vec<_> = s.ops.iter().map(|o| o.0).collect();
        assert_eq!(kinds, [HeatOp::Deposit(0, 1), HeatOp::Step, HeatOp::Step]);
        s.advance(1);
        s.plan();
        assert_eq!(s.ops.iter().map(|o| o.0).collect::<Vec<_>>(), [HeatOp::Step], "the leftover ms count");
    }

    #[test]
    fn plans_are_the_same_however_the_time_is_split() {
        let run = |split: bool| {
            let mut s = sim();
            let mut all = Vec::new();
            s.deposit(blast(50.0, 50.0));
            for i in 0..40 {
                s.advance(HEAT_STEP_MS);
                if i == 17 {
                    s.deposit(blast(150.0, 60.0));
                }
                if split {
                    s.plan();
                    all.extend(s.ops.iter().map(|(op, r)| (*op, s.rects[r.clone()].to_vec())));
                }
            }
            s.plan();
            all.extend(s.ops.iter().map(|(op, r)| (*op, s.rects[r.clone()].to_vec())));
            // Deposit ranges index each frame's own deposits.
            all.into_iter()
                .map(|(op, r)| (if let HeatOp::Deposit(..) = op { HeatOp::Deposit(0, 1) } else { op }, r))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(true), run(false));
    }

    #[test]
    fn boxes_grow_to_their_limit_and_drop_once_cool() {
        let mut s = sim();
        s.deposit(blast(100.0, 50.0));
        s.advance(HEAT_STEP_MS);
        s.plan();
        assert_eq!(s.boxes[0].rect, [91, 41, 110, 60], "the circle grown a pixel");
        s.advance(HEAT_STEP_MS * 200);
        s.plan();
        let r = s.boxes[0].rect;
        assert_eq!(r, [100 - 8 - HEAT_SPREAD, 0, 109 + HEAT_SPREAD, 100], "at its limit, in the level");
        // Long after, it is dropped and zeroed, without stepping there.
        s.advance(HEAT_STEP_MS * 100_000);
        s.plan();
        assert!(!s.active());
        assert!(s.ops.iter().all(|(op, _)| matches!(op, HeatOp::Zero(_))), "{:?}", s.ops);
        assert!(!s.ops.is_empty());
    }

    #[test]
    fn a_frame_runs_at_most_its_steps() {
        let mut s = sim();
        s.deposit(blast(100.0, 50.0));
        s.advance(HEAT_STEP_MS * (MAX_HEAT_STEPS + 10));
        s.plan();
        assert_eq!(s.ops.iter().filter(|o| o.0 == HeatOp::Step).count(), MAX_HEAT_STEPS as usize);
        s.plan();
        assert_eq!(s.ops.iter().filter(|o| o.0 == HeatOp::Step).count(), 10, "the rest next frame");
    }

    /// Issue #67: a layer reset lands in call order, after the deposits
    /// and steps queued before it and before those queued after; over the
    /// boxes within its rectangle (none: the rectangle seen alone); one
    /// right after another joins it, and a capped count joins the last.
    #[test]
    fn layer_resets_land_in_call_order() {
        let mut s = sim();
        s.deposit(blast(50.0, 50.0));
        s.advance(HEAT_STEP_MS);
        s.relayer([40, 40, 60, 60]);
        s.relayer([55, 45, 70, 50]);
        s.deposit(blast(52.0, 50.0));
        s.relayer([150, 10, 160, 20]);
        s.advance(HEAT_STEP_MS);
        s.plan();
        let kinds: Vec<_> = s.ops.iter().map(|o| o.0).collect();
        assert_eq!(
            kinds,
            [
                HeatOp::Deposit(0, 1),
                HeatOp::Step,
                HeatOp::Layer([40, 40, 30, 20]),
                HeatOp::Deposit(1, 2),
                HeatOp::Layer([150, 10, 10, 10]),
                HeatOp::Step,
            ]
        );
        assert_eq!(s.waiting(), 0);
        let rects = |i: usize| s.rects[s.ops[i].1.clone()].to_vec();
        assert_eq!(rects(2), [[41, 41, 19, 19]], "over the box within it");
        assert!(rects(4).is_empty(), "no box there: seen only");
        // Past the cap, a reset joins the last; layers count no deposit.
        let mut s = sim();
        s.deposit(blast(50.0, 50.0));
        for i in 0..MAX_HEAT_LAYERS as i32 + 5 {
            s.relayer([i, 0, i + 1, 1]);
            s.deposit(blast(50.0, 50.0));
        }
        assert_eq!(s.waiting(), MAX_HEAT_DEPOSITS);
        s.plan();
        let layers = s.ops.iter().filter(|o| matches!(o.0, HeatOp::Layer(_))).count();
        assert_eq!(layers, MAX_HEAT_LAYERS);
        assert!(s.ops.iter().any(|o| o.0 == HeatOp::Layer([MAX_HEAT_LAYERS as u32 - 1, 0, 6, 1])));
        // Off the level, nothing.
        let mut s = sim();
        s.deposit(blast(50.0, 50.0));
        s.relayer([-10, -10, 0, 5]);
        s.plan();
        assert!(!s.ops.iter().any(|o| matches!(o.0, HeatOp::Layer(_))));
    }

    #[test]
    fn deposits_are_capped_and_boxes_bounded() {
        let mut s = sim();
        for i in 0..1000 {
            s.deposit(HeatDeposit {
                pos: [(i * 37 % 200) as f32, (i * 11 % 100) as f32],
                radius: 1.0,
                amount: 1e30,
            });
        }
        s.advance(HEAT_STEP_MS);
        s.plan();
        assert_eq!(s.deposits.len(), MAX_HEAT_DEPOSITS);
        assert!(s.boxes.len() <= MAX_BOXES);
        assert!(s.deposits.iter().all(|d| d.amount <= MAX_HEAT_AMOUNT && d.radius <= MAX_HEAT_RADIUS));
        assert!(s.boxes.iter().all(|b| b.hot <= MAX_TEMPERATURE as f32));
    }

    /// Issue #41: the nearest view by its rectangle, the first of those as
    /// near; a layer heats only by a heating material within reach; a
    /// layer's texels follow its image's coverage.
    #[test]
    fn distant_layers_pick_their_view_reach_and_texels() {
        let v = |x: f32| HeatView { view: View { x, y: 0.0, w: 10.0, h: 10.0 }, camera: (x, 0.0) };
        let views = [v(0.0), v(20.0), v(5.0)];
        assert_eq!(nearest(&views, [25.0, 5.0]).unwrap().camera.0, 20.0, "inside");
        assert_eq!(nearest(&views, [8.0, 5.0]).unwrap().camera.0, 0.0, "inside two: the first");
        assert_eq!(nearest(&views, [18.0, -3.0]).unwrap().camera.0, 20.0, "the nearest edge");
        assert_eq!(nearest(&views, [f32::NAN, 0.0]).unwrap().camera.0, 0.0);
        assert!(nearest(&[], [0.0, 0.0]).is_none());
        let cold = SurfaceMaterial { id: "cold".into(), heat: None, light_soak: None };
        let (params, ..) = tables(&[stone(), cold]).unwrap();
        // Issue #65: z = 14 + 1.1 × depth.
        assert!(layer_heats(&params, 1, 45) && !layer_heats(&params, 1, 46), "z < 64");
        assert!(layer_heats(&params, 1, 0), "depth 0 counts as 1");
        assert!(
            !layer_heats(&params, 2, 10) && !layer_heats(&params, 0, 10) && !layer_heats(&params, 255, 10)
        );
        let mut image = olr_data::image::RgbaImage::new(6, 4);
        image.set(5, 0, olr_data::image::Rgba::rgb(1, 2, 3));
        image.set(0, 3, olr_data::image::Rgba::rgb(1, 2, 3));
        assert_eq!(layer_texels(&image, 2, (3, 2), 7), [0, 0, 0, 0, 7, 7, 7, 7, 0, 0, 0, 0]);
    }

    /// Issue #65: dirt takes up to [`DIRT_HEAT_GAIN`] × a deposit, so the
    /// bound covers it: a box is kept while its dirt face, cooling as
    /// slowly as any pixel may, still shows.
    #[test]
    fn a_hot_dirt_box_outlives_its_glow() {
        let (.., bounds) = tables(&[stone()]).unwrap();
        assert_eq!(bounds.gain, DIRT_HEAT_GAIN * DIRT_SKIN_WEIGHTS[0]);
        for amount in [50.0, 100.0, 300.0, 1000.0] {
            let mut s = sim();
            s.deposit(HeatDeposit { amount, ..blast(100.0, 50.0) });
            let mut face = (amount * DIRT_HEAT_GAIN * DIRT_SKIN_WEIGHTS[0]).min(MAX_TEMPERATURE as f32);
            for step in 0..20_000 {
                s.advance(HEAT_STEP_MS);
                s.plan();
                face -= BULK * (COOL_RATE * face + COOL_MIN);
                if face >= bounds.visible {
                    assert!(s.active(), "{amount}: dropped at step {step} with the face at {face}");
                } else if !s.active() {
                    break;
                }
            }
        }
    }

    #[test]
    fn a_reset_clears_the_texture_and_forgets_the_boxes() {
        let mut s = sim();
        s.deposit(blast(100.0, 50.0));
        s.advance(HEAT_STEP_MS);
        s.plan();
        s.reset();
        s.advance(HEAT_STEP_MS);
        s.plan();
        assert_eq!(s.ops.iter().map(|o| o.0).collect::<Vec<_>>(), [HeatOp::Clear]);
        assert!(!s.active());
    }
}
