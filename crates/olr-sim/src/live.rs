//! A level's live copies as the simulation knows them (issue #85; the
//! design `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`):
//! setup data shared by every world of a match through an `Arc`, like
//! [`crate::SimMod`], never hashed or transferred. No OLX counterpart.
//!
//! [`LiveLevel`] holds, per live copy, its id, place and size, its values
//! (properties: a name or none, what each drives, its limits and
//! default), its states (each a complete look: a value for every property
//! any state sets, and its parts), each picture's rock mask (a bit per
//! world pixel; the art stays in presentation), its start, health,
//! scripts and sounds. Lookups by copy id, state name and value name go
//! through sorted tables.
//!
//! Issue #86 puts the copies in the world: [`LiveWorld`] holds each
//! copy's state and values (cloned for rollback, hashed after the rest of
//! the world, transferred as [`LiveWire`]); [`LiveView`] reads them.
//! Issue #89 adds motions ([`LiveMotion`]): a position-like value moving
//! towards a target at a speed, once a tick before the physics. Issue #90
//! makes the rock parts solid: [`LiveWorld`] keeps their index
//! ([`crate::solid::SolidIndex`]), a motion steps them a whole pixel at a
//! time ([`PartMover`], `live_step.rs`), and each worm slot's crush count.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use serde::{Deserialize, Serialize};

use olr_math::AbsTime;

use crate::buf::Buf;
use crate::checksum::Fnv;
use crate::events::{EventKind, TickEvents};
use crate::script::{Director, ScriptWire, ScriptWorld};
use crate::solid::{SolidIndex, SolidPart, SolidParts};
use crate::world::MAX_WORMS;

/// What a live copy's value drives (`olr_format::levels::Target`): a
/// layer property names the asset layer its parts show
/// ([`LivePart::layer`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LiveTarget {
    X,
    Y,
    Visible,
    Facing,
    Health,
    OffsetX(u16),
    OffsetY(u16),
    LayerVisible(u16),
    Opacity(u16),
    GlowStrength(u16),
    GlowColour(u16),
    LightIntensity(u16),
    LightReach(u16),
    GlowAnimation(u16),
    BeamDirection(u16),
    BeamSpread(u16),
    GlassOpacity(u16),
    GlassTint(u16),
}

/// One value of a live copy: its exposed name (none: only states set
/// it), what it drives, its limits and its default (positions in world
/// pixels).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveProp {
    pub name: Option<String>,
    pub target: LiveTarget,
    pub min: i64,
    pub max: i64,
    pub default: i64,
}

/// Whether a part is solid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LivePartKind {
    /// Solid, never dug.
    Rock,
    /// Scenery.
    Background,
}

/// One part of a live copy in a state: its kind, the asset layer it shows
/// (none: untargeted layers flattened), its picture's top-left in the
/// copy (world pixels), the picture and its glow strength (percent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LivePart {
    pub kind: LivePartKind,
    pub layer: Option<u16>,
    pub at: (i32, i32),
    pub picture: u16,
    pub glow_strength: u8,
}

/// A state of a live copy: its name, its complete values (prop index,
/// value), its parts bottom to top and its sounds (indices into the
/// level's sounds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveState {
    pub name: String,
    pub values: Vec<(u16, i64)>,
    pub parts: Vec<LivePart>,
    pub enter_sound: Option<u16>,
    pub loop_sound: Option<u16>,
}

/// A picture's solid pixels: a bit per world pixel, row-major, `size.0 ×
/// size.1` bits in `u64` words (none for a background picture).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveSolid {
    pub size: (u32, u32),
    pub bits: Option<Vec<u64>>,
}

impl LiveSolid {
    /// A picture of `size` solid where `solid(x, y)`; `rock` false gives
    /// none.
    pub fn new(size: (u32, u32), rock: bool, solid: impl Fn(u32, u32) -> bool) -> LiveSolid {
        if !rock {
            return LiveSolid { size, bits: None };
        }
        let n = size.0 as usize * size.1 as usize;
        let mut bits = alloc::vec![0u64; n.div_ceil(64)];
        for y in 0..size.1 {
            for x in 0..size.0 {
                if solid(x, y) {
                    let i = y as usize * size.0 as usize + x as usize;
                    bits[i / 64] |= 1 << (i % 64);
                }
            }
        }
        LiveSolid { size, bits: Some(bits) }
    }

    /// Whether world pixel `(x, y)` of the picture is solid (outside it,
    /// not).
    pub fn solid(&self, x: u32, y: u32) -> bool {
        let Some(bits) = &self.bits else { return false };
        if x >= self.size.0 || y >= self.size.1 {
            return false;
        }
        let i = y as usize * self.size.0 as usize + x as usize;
        bits.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
    }
}

/// A destructible copy's health: its points and the state entered at 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveHealth {
    pub points: u32,
    pub at_zero: u16,
}

/// One live copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveCopy {
    /// Its id: scripts reach it by it.
    pub id: String,
    /// Its top-left, world pixels.
    pub at: (i32, i32),
    /// Its placed size, world pixels.
    pub size: (u32, u32),
    pub props: Vec<LiveProp>,
    pub states: Vec<LiveState>,
    /// Each picture's solid pixels, by picture index.
    pub pictures: Vec<LiveSolid>,
    pub start_state: u16,
    /// Its own values, applied once after its starting state's.
    pub start_values: Vec<(u16, i64)>,
    pub health: Option<LiveHealth>,
    /// Its scripts in run order (indices into [`LiveLevel::scripts`]).
    pub scripts: Vec<u16>,
    /// Its asset's sounds by name, each an index into the level's sounds.
    pub sounds: Vec<(String, u16)>,
    /// `(state name, index)` sorted by name.
    state_names: Vec<(String, u16)>,
    /// `(value name, index)` sorted by name.
    prop_names: Vec<(String, u16)>,
}

/// [`LiveWorld::parts_mut`]: the world's live buffers, the scripts' part
/// mutable.
pub(crate) struct LiveParts<'a> {
    pub states: &'a [u16],
    pub values: &'a [i64],
    pub motions: &'a [LiveMotion],
    pub offsets: &'a [u32],
    pub script: &'a mut Option<ScriptWorld>,
}

/// What a live copy holds, before its lookup tables are made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveCopySetup {
    pub id: String,
    pub at: (i32, i32),
    pub size: (u32, u32),
    pub props: Vec<LiveProp>,
    pub states: Vec<LiveState>,
    pub pictures: Vec<LiveSolid>,
    pub start_state: u16,
    pub start_values: Vec<(u16, i64)>,
    pub health: Option<LiveHealth>,
    pub scripts: Vec<u16>,
    pub sounds: Vec<(String, u16)>,
}

/// `(name, index)` of `names` sorted by name (a name given twice keeps
/// its first index).
fn table<'a>(names: impl Iterator<Item = Option<&'a str>>) -> Vec<(String, u16)> {
    let mut out: Vec<(String, u16)> =
        names.enumerate().filter_map(|(i, n)| Some((String::from(n?), u16::try_from(i).ok()?))).collect();
    out.sort();
    out.dedup_by(|b, a| a.0 == b.0);
    out
}

fn find(table: &[(String, u16)], name: &str) -> Option<u16> {
    table.binary_search_by(|(n, _)| n.as_str().cmp(name)).ok().map(|i| table[i].1)
}

impl LiveCopy {
    /// The copy with its lookup tables.
    pub fn new(s: LiveCopySetup) -> LiveCopy {
        let state_names = table(s.states.iter().map(|x| Some(x.name.as_str())));
        let prop_names = table(s.props.iter().map(|p| p.name.as_deref()));
        LiveCopy {
            id: s.id,
            at: s.at,
            size: s.size,
            props: s.props,
            states: s.states,
            pictures: s.pictures,
            start_state: s.start_state,
            start_values: s.start_values,
            health: s.health,
            scripts: s.scripts,
            sounds: s.sounds,
            state_names,
            prop_names,
        }
    }

    /// The index of its state `name` (exact).
    pub fn state(&self, name: &str) -> Option<u16> {
        find(&self.state_names, name)
    }

    /// The index of its value named `name` (an exposed name).
    pub fn prop(&self, name: &str) -> Option<u16> {
        find(&self.prop_names, name)
    }

    /// The index of its sound named `name` in the level's sounds.
    pub fn sound(&self, name: &str) -> Option<u16> {
        self.sounds.iter().find(|(n, _)| n == name).map(|(_, k)| *k)
    }
}

/// A level's live copies: setup data, shared by every world of a match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveLevel {
    /// The copies, bottom to top.
    pub copies: Vec<LiveCopy>,
    /// The scripts' texts (run by #88).
    pub scripts: Vec<String>,
    /// The level script, an index into `scripts`.
    pub level_script: Option<u16>,
    /// How many sounds the level's played form holds (their bytes are
    /// presentation's).
    pub sounds: u16,
    /// `(copy id, index)` sorted by id.
    ids: Vec<(String, u16)>,
}

impl LiveLevel {
    /// The level of `copies` (each with its tables) and its scripts.
    pub fn new(
        copies: Vec<LiveCopy>,
        scripts: Vec<String>,
        level_script: Option<u16>,
        sounds: u16,
    ) -> LiveLevel {
        let ids = table(copies.iter().map(|c| Some(c.id.as_str())));
        LiveLevel { copies, scripts, level_script, sounds, ids }
    }

    /// The index of copy `id`.
    pub fn copy(&self, id: &str) -> Option<u16> {
        find(&self.ids, id)
    }

    /// Copy `id`.
    pub fn copy_by_id(&self, id: &str) -> Option<&LiveCopy> {
        self.copy(id).and_then(|i| self.copies.get(usize::from(i)))
    }
}

/// How many steps a world pixel holds in a live copy's position-like
/// values ([`LiveTarget::is_position`]): the world keeps `X`, `Y`,
/// `OffsetX` and `OffsetY` in 1/256 world pixel, so a motion (#89) moves
/// in sub-pixel steps without floats. A part's footprint is at the whole
/// pixel, rounded down.
pub const LIVE_SUBPIXELS: i64 = 256;

impl LiveTarget {
    /// Whether the world keeps this value in 1/[`LIVE_SUBPIXELS`] world
    /// pixel (the level's numbers are whole pixels).
    pub fn is_position(self) -> bool {
        matches!(self, LiveTarget::X | LiveTarget::Y | LiveTarget::OffsetX(_) | LiveTarget::OffsetY(_))
    }

    /// The world's value of the level's number `v` (positions times
    /// [`LIVE_SUBPIXELS`]).
    fn scale(self, v: i64) -> i64 {
        if self.is_position() { v.saturating_mul(LIVE_SUBPIXELS) } else { v }
    }
}

/// The fastest a motion moves its value (issue #89): 8 world pixels a
/// tick, in 1/[`LIVE_SUBPIXELS`] pixel. A faster speed is clamped to it.
pub const MAX_MOTION_SPEED: i64 = 8 * LIVE_SUBPIXELS;

/// The most whole pixels a motion steps a rock part a tick (issue #90):
/// [`MAX_MOTION_SPEED`] in whole pixels.
pub const MAX_PART_STEP: i64 = MAX_MOTION_SPEED / LIVE_SUBPIXELS;

/// The health a crushing chunk takes (issue #90): a cautious start, set by
/// issue #100's playtest.
pub const CRUSH_CHUNK: f32 = 5.0;

/// A wedged worm takes a chunk on its first wedged tick and again every
/// this many consecutive wedged ticks (issue #90; set by issue #100's
/// playtest): a full worm held about 2 s dies.
pub const CRUSH_EVERY: u16 = 10;

/// How far a part that appears over a worm pushes it out, at most, world
/// pixels (issue #90).
pub const PUSH_OUT_PX: i32 = 8;

/// How a motion's step of rock parts went ([`PartMover::step`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PartStep {
    /// The parts moved (and the worms they pushed, carried or hooked).
    Taken,
    /// Rock, another part or the level's edge is in the way: the motion
    /// ends.
    Blocked,
    /// A worm with nowhere to go is in the way: wedged, it holds the
    /// motion until the next tick.
    Wait,
}

/// Steps rock parts for a motion (issue #90; the world's side,
/// `live_step.rs`).
pub(crate) trait PartMover {
    /// Moves each `(part, place)` of `moves` (indices into `solids`' parts,
    /// all of copy `copy`, each a pixel from where it is) if nothing stops
    /// them, with the worms in their way.
    fn step(
        &mut self,
        solids: &mut SolidIndex,
        level: &LiveLevel,
        copy: u16,
        moves: &[(usize, (i32, i32))],
    ) -> PartStep;
}

/// The axis a position-like value places parts on, and which.
#[derive(Clone, Copy)]
enum Axis {
    /// `X` or `Y`: every part of the copy (`y` true for `Y`).
    Copy { y: bool },
    /// `OffsetX(l)` or `OffsetY(l)`: the parts of layer `l`.
    Layer { y: bool, layer: u16 },
}

impl Axis {
    /// The axis value `prop` of `c` places parts on: none unless it is
    /// position-like and the first value driving its target.
    fn of(c: &LiveCopy, prop: u16) -> Option<Axis> {
        let t = c.props.get(usize::from(prop))?.target;
        if c.props.iter().position(|p| p.target == t) != Some(usize::from(prop)) {
            return None;
        }
        match t {
            LiveTarget::X => Some(Axis::Copy { y: false }),
            LiveTarget::Y => Some(Axis::Copy { y: true }),
            LiveTarget::OffsetX(layer) => Some(Axis::Layer { y: false, layer }),
            LiveTarget::OffsetY(layer) => Some(Axis::Layer { y: true, layer }),
            _ => None,
        }
    }

    fn places(self, p: &SolidPart) -> bool {
        match self {
            Axis::Copy { .. } => true,
            Axis::Layer { layer, .. } => p.layer == Some(layer),
        }
    }

    /// Part `p`'s position sum on this axis, 1/[`LIVE_SUBPIXELS`] pixel
    /// without the whole-pixel places (the copy's value and its layer's
    /// offset), and, for a mirrored part moved by `X`, the copy's own
    /// (the rectangle it is mirrored in).
    fn sum(self, c: &LiveCopy, values: &[i64], p: &SolidPart) -> (i64, Option<i64>) {
        let y = matches!(self, Axis::Copy { y: true } | Axis::Layer { y: true, .. });
        let (copy_t, off_t): (LiveTarget, fn(u16) -> LiveTarget) =
            if y { (LiveTarget::Y, LiveTarget::OffsetY) } else { (LiveTarget::X, LiveTarget::OffsetX) };
        let base = c.value_of(values, copy_t).unwrap_or(0);
        let off = p.layer.and_then(|l| c.value_of(values, off_t(l))).unwrap_or(0);
        let mirrored = p.flip && !y && matches!(self, Axis::Copy { .. });
        (base.saturating_add(off), mirrored.then_some(base))
    }
}

/// The value, from `v` going `dir`, at which `v + off` first enters
/// another whole pixel; `None` past `i64`'s range.
fn crossing(v: i64, off: i64, dir: i64) -> Option<i64> {
    let s = v.checked_add(off)?;
    let floor = s.div_euclid(LIVE_SUBPIXELS).checked_mul(LIVE_SUBPIXELS)?;
    if dir > 0 {
        floor.checked_add(LIVE_SUBPIXELS)?.checked_sub(off)
    } else {
        floor.checked_sub(1)?.checked_sub(off)
    }
}

/// A part's picture's top-left, world pixels, in copy `c` with `values`:
/// the copy's place and `X`/`Y`, the part's place `part_at` and its layer's
/// `OffsetX`/`OffsetY`, rounded down, then (`flip`, the copy's `Facing`)
/// mirrored within the copy's rectangle, as presentation draws it
/// (`olr_content::live_frame`).
fn part_place(
    c: &LiveCopy,
    values: &[i64],
    part_at: (i32, i32),
    layer: Option<u16>,
    pw: u32,
    flip: bool,
) -> (i32, i32) {
    let v = |t| c.value_of(values, t).unwrap_or(0);
    let (ox, oy) = (
        (i64::from(c.at.0) * LIVE_SUBPIXELS).saturating_add(v(LiveTarget::X)),
        (i64::from(c.at.1) * LIVE_SUBPIXELS).saturating_add(v(LiveTarget::Y)),
    );
    let (mut x, mut y) = (
        ox.saturating_add(i64::from(part_at.0) * LIVE_SUBPIXELS),
        oy.saturating_add(i64::from(part_at.1) * LIVE_SUBPIXELS),
    );
    if let Some(l) = layer {
        x = x.saturating_add(v(LiveTarget::OffsetX(l)));
        y = y.saturating_add(v(LiveTarget::OffsetY(l)));
    }
    let (x, y) = (whole(x), whole(y));
    if !flip {
        return (x, y);
    }
    let m = 2 * i64::from(whole(ox)) + i64::from(c.size.0) - i64::from(x) - i64::from(pw);
    (m.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32, y)
}

/// A position-like value's motion (issue #89): towards `target` (the
/// world's units, within the value's limits) by `speed` (1/
/// [`LIVE_SUBPIXELS`] pixel a tick, 1 to [`MAX_MOTION_SPEED`]) once a
/// tick. `speed` 0 (and `target` 0) is a value that does not move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveMotion {
    pub target: i64,
    pub speed: i64,
}

impl LiveMotion {
    /// Whether the value moves.
    pub fn moving(self) -> bool {
        self.speed != 0
    }
}

/// `v` within `min..=max`, never a panic (`i64::clamp` panics when `min >
/// max`: a broken setup keeps `max`).
fn within(v: i64, min: i64, max: i64) -> i64 {
    v.max(min).min(max)
}

impl LiveCopy {
    /// Value `prop`'s limits in the world's units: its prop's, a position
    /// in 1/[`LIVE_SUBPIXELS`] pixel, `Health` also within `0..=points`.
    pub fn limits(&self, prop: &LiveProp) -> (i64, i64) {
        let (mut min, mut max) = (prop.target.scale(prop.min), prop.target.scale(prop.max));
        if prop.target == LiveTarget::Health
            && let Some(h) = self.health
        {
            (min, max) = (min.max(0), max.min(i64::from(h.points)));
        }
        (min, max)
    }

    /// `v` (the world's units) clamped into value `prop`'s limits.
    fn clamped(&self, prop: &LiveProp, v: i64) -> i64 {
        let (min, max) = self.limits(prop);
        within(v, min, max)
    }

    /// The value `target` holds among `values` (the first prop driving
    /// it), if one does.
    fn value_of(&self, values: &[i64], target: LiveTarget) -> Option<i64> {
        let i = self.props.iter().position(|p| p.target == target)?;
        values.get(i).copied()
    }
}

/// Why a live setup call ([`crate::World::set_live_state`],
/// [`crate::World::set_live_value`]) was refused; nothing changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveError {
    /// The world has no such live copy (or none at all).
    NoCopy(u16),
    /// The copy has no such state.
    NoState { copy: u16, state: u16 },
    /// The copy has no such value.
    NoProp { copy: u16, prop: u16 },
    /// The copy's value is not position-like: it cannot move (#89).
    NotPosition { copy: u16, prop: u16 },
}

impl fmt::Display for LiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiveError::NoCopy(c) => write!(f, "no live copy {c}"),
            LiveError::NoState { copy, state } => write!(f, "live copy {copy} has no state {state}"),
            LiveError::NoProp { copy, prop } => write!(f, "live copy {copy} has no value {prop}"),
            LiveError::NotPosition { copy, prop } => {
                write!(f, "live copy {copy}'s value {prop} is not a position: it cannot move")
            }
        }
    }
}

/// The live copies' part of a world (issue #86): per copy its state and
/// its values, one `i64` per [`LiveCopy::props`] entry, flat in one
/// buffer, so nothing a tick does allocates, and each value's motion
/// (issue #89) beside it; the scripts' state (issue #88). Each extends
/// this struct, its checksum ([`LiveWorld::hash`]) and its transfer
/// ([`LiveWire`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LiveWorld {
    /// The setup, shared by every world of the match: never hashed or
    /// transferred.
    pub(crate) level: Arc<LiveLevel>,
    /// Setup only: copy `i`'s values are `values[offsets[i]..offsets[i +
    /// 1]]` (one more entry than copies).
    offsets: Buf<u32>,
    /// Each copy's state, an index into its [`LiveCopy::states`].
    states: Buf<u16>,
    /// Every copy's values in the world's units (positions in
    /// 1/[`LIVE_SUBPIXELS`] pixel), within their limits.
    values: Buf<i64>,
    /// Each value's motion, beside `values` (issue #89).
    motions: Buf<LiveMotion>,
    /// Setup: where the match's frame clock started (scripts' `time()`).
    pub(crate) start: AbsTime,
    /// The scripts' state (issue #88), present when the setup has a
    /// director.
    pub(crate) script: Option<ScriptWorld>,
    /// Issue #90: each worm slot's consecutive wedged ticks (crushing).
    crush: Buf<u16>,
    /// Issue #90: the shown rock parts' index, derived from the rest (in
    /// no checksum or transfer; [`LiveWorld::refresh_solids`]).
    pub(crate) solids: SolidIndex,
}

impl LiveWorld {
    /// The copies of `level` at their defaults, before they enter their
    /// starting states ([`LiveWorld::start`]).
    pub(crate) fn new(
        level: Arc<LiveLevel>,
        director: Option<Box<dyn Director>>,
        seed: u32,
        start: AbsTime,
        map: (u32, u32),
    ) -> LiveWorld {
        let n = level.copies.len();
        let mut offsets = Buf::with_capacity(n + 1);
        let mut values = Buf::with_capacity(level.copies.iter().map(|c| c.props.len()).sum());
        offsets.push(0);
        for c in &level.copies {
            values.extend(c.props.iter().map(|p| c.clamped(p, p.target.scale(p.default))));
            offsets.push(u32::try_from(values.len()).unwrap_or(u32::MAX));
        }
        let mut states = Buf::with_capacity(n);
        states.resize(n, 0);
        let mut motions = Buf::with_capacity(values.len());
        motions.resize(values.len(), LiveMotion::default());
        let script = director.map(|d| ScriptWorld::new(d, &level, seed));
        let mut crush = Buf::with_capacity(MAX_WORMS);
        crush.resize(MAX_WORMS, 0);
        let rock = level.copies.iter().flat_map(|c| &c.pictures).any(|p| p.bits.is_some());
        let solids = SolidIndex::new(map, rock);
        LiveWorld { level, offsets, states, values, motions, start, script, crush, solids }
    }

    /// Every copy's state, every value and its motion, the values'
    /// offsets, and the scripts' part, borrowed apart.
    pub(crate) fn parts_mut(&mut self) -> LiveParts<'_> {
        LiveParts {
            states: &self.states,
            values: &self.values,
            motions: &self.motions,
            offsets: &self.offsets,
            script: &mut self.script,
        }
    }

    /// At setup: every copy enters its starting state, then its own
    /// starting values apply; the parts' index follows. Their events come
    /// from [`LiveWorld::announce_start`], after the worms' (issue #90: the
    /// worms spawn beside the parts where they start).
    pub(crate) fn start(&mut self) {
        let level = self.level.clone();
        for (i, c) in level.copies.iter().enumerate() {
            let Ok(copy) = u16::try_from(i) else { break };
            if usize::from(c.start_state) < c.states.len() {
                self.look(copy, c.start_state);
            }
            for &(prop, v) in &c.start_values {
                if let Some(p) = c.props.get(usize::from(prop)) {
                    self.put(copy, prop, p.target.scale(v));
                }
            }
        }
        self.refresh_solids();
    }

    /// The starting states' [`EventKind::LiveState`], one a copy in order
    /// (with the first tick's events).
    pub(crate) fn announce_start(&self, events: &mut TickEvents) {
        for (i, c) in self.level.copies.iter().enumerate() {
            let Ok(copy) = u16::try_from(i) else { break };
            if usize::from(c.start_state) < c.states.len() {
                events.push(EventKind::LiveState { copy, state: c.start_state });
            }
        }
    }

    /// Rebuilds the rock parts' index from the copies' states and values
    /// (issue #90): after anything that may change a footprint other than
    /// a motion's step (setup, a script phase, a restore, a state load).
    /// Allocates nothing.
    pub(crate) fn refresh_solids(&mut self) {
        let mut solids = core::mem::take(&mut self.solids);
        solids.clear();
        for c in LiveView::new(self).copies() {
            let flip = c.target(LiveTarget::Facing).is_some_and(|v| v != 0);
            for (p, _) in c.rock_parts() {
                let Some(pic) = c.setup.pictures.get(usize::from(p.picture)) else { continue };
                if pic.bits.is_none() {
                    continue;
                }
                let at = part_place(c.setup, c.values, p.at, p.layer, pic.size.0, flip);
                let (picture, layer, part_at, size) = (p.picture, p.layer, p.at, pic.size);
                solids.push(SolidPart { copy: c.index, picture, layer, part_at, at, size, flip });
            }
        }
        self.solids = solids;
    }

    /// The shown rock parts, to read; `None` when none shows (the ground
    /// alone then).
    pub(crate) fn solid_parts(&self) -> Option<SolidParts<'_>> {
        (!self.solids.is_empty()).then_some(SolidParts { index: &self.solids, level: &self.level })
    }

    /// Each worm slot's consecutive wedged ticks.
    pub(crate) fn crush(&self) -> &[u16] {
        &self.crush
    }

    pub(crate) fn crush_mut(&mut self) -> &mut [u16] {
        &mut self.crush
    }

    fn range(&self, copy: u16) -> core::ops::Range<usize> {
        let i = usize::from(copy);
        let start = self.offsets.get(i).map_or(0, |&o| o as usize);
        let end = self.offsets.get(i + 1).map_or(start, |&o| o as usize);
        start..end.max(start).min(self.values.len())
    }

    /// Copy `copy`'s values.
    pub(crate) fn values(&self, copy: u16) -> &[i64] {
        &self.values[self.range(copy)]
    }

    /// Copy `copy`'s values' motions.
    pub(crate) fn motions(&self, copy: u16) -> &[LiveMotion] {
        let r = self.range(copy);
        self.motions.get(r).unwrap_or(&[])
    }

    /// Sets value `prop` of `copy` to `v` (the world's units), clamped
    /// into its limits. An index outside the setup changes nothing.
    fn put(&mut self, copy: u16, prop: u16, v: i64) {
        let Some(c) = self.level.copies.get(usize::from(copy)) else { return };
        let Some(p) = c.props.get(usize::from(prop)) else { return };
        let v = c.clamped(p, v);
        let r = self.range(copy);
        if let Some(slot) = self.values[r].get_mut(usize::from(prop)) {
            *slot = v;
        }
    }

    /// Copy `copy` enters `state`: every value the state's complete look
    /// names is set, then [`EventKind::LiveState`] is emitted.
    fn enter(&mut self, copy: u16, state: u16, events: &mut TickEvents) {
        if self.look(copy, state) {
            events.push(EventKind::LiveState { copy, state });
        }
    }

    /// Copy `copy` takes `state` and every value its complete look names;
    /// whether it has that state.
    fn look(&mut self, copy: u16, state: u16) -> bool {
        let level = self.level.clone();
        let Some(s) = level.copies.get(usize::from(copy)).and_then(|c| c.states.get(usize::from(state)))
        else {
            return false;
        };
        if let Some(slot) = self.states.get_mut(usize::from(copy)) {
            *slot = state;
        }
        let c = &level.copies[usize::from(copy)];
        for &(prop, v) in &s.values {
            if let Some(p) = c.props.get(usize::from(prop)) {
                self.put(copy, prop, p.target.scale(v));
            }
        }
        true
    }

    /// [`crate::World::set_live_state`].
    pub(crate) fn set_state(
        &mut self,
        copy: u16,
        state: u16,
        events: &mut TickEvents,
    ) -> Result<(), LiveError> {
        let c = self.level.copies.get(usize::from(copy)).ok_or(LiveError::NoCopy(copy))?;
        if usize::from(state) >= c.states.len() {
            return Err(LiveError::NoState { copy, state });
        }
        self.enter(copy, state, events);
        self.refresh_solids();
        Ok(())
    }

    /// [`crate::World::set_live_value`]: also stops the value's motion
    /// (a state change does not).
    pub(crate) fn set_value(&mut self, copy: u16, prop: u16, value: i64) -> Result<(), LiveError> {
        let c = self.level.copies.get(usize::from(copy)).ok_or(LiveError::NoCopy(copy))?;
        if usize::from(prop) >= c.props.len() {
            return Err(LiveError::NoProp { copy, prop });
        }
        self.put(copy, prop, value);
        self.set_motion(copy, prop, LiveMotion::default());
        self.refresh_solids();
        Ok(())
    }

    fn set_motion(&mut self, copy: u16, prop: u16, m: LiveMotion) {
        let r = self.range(copy);
        if let Some(slot) = self.motions.get_mut(r).and_then(|m| m.get_mut(usize::from(prop))) {
            *slot = m;
        }
    }

    /// Value `prop` of `copy` (position-like) starts moving towards
    /// `target` (the world's units, clamped into its limits) by `speed` a
    /// tick (clamped into 1..=[`MAX_MOTION_SPEED`]), in place of a motion
    /// it had (issue #89).
    pub(crate) fn move_value(
        &mut self,
        copy: u16,
        prop: u16,
        target: i64,
        speed: i64,
    ) -> Result<(), LiveError> {
        let c = self.level.copies.get(usize::from(copy)).ok_or(LiveError::NoCopy(copy))?;
        let p = c.props.get(usize::from(prop)).ok_or(LiveError::NoProp { copy, prop })?;
        if !p.target.is_position() {
            return Err(LiveError::NotPosition { copy, prop });
        }
        let m = LiveMotion { target: c.clamped(p, target), speed: within(speed, 1, MAX_MOTION_SPEED) };
        self.set_motion(copy, prop, m);
        Ok(())
    }

    /// Value `prop` of `copy` stops where it is.
    pub(crate) fn stop_value(&mut self, copy: u16, prop: u16) -> Result<(), LiveError> {
        let c = self.level.copies.get(usize::from(copy)).ok_or(LiveError::NoCopy(copy))?;
        if usize::from(prop) >= c.props.len() {
            return Err(LiveError::NoProp { copy, prop });
        }
        self.set_motion(copy, prop, LiveMotion::default());
        Ok(())
    }

    /// The motions' step (issue #89), once a tick before the physics: in
    /// level and value order, each moving value goes towards its target by
    /// its speed, clamped into its limits; one that reaches it stops and
    /// emits [`EventKind::LiveArrived`]. Integers only.
    ///
    /// Issue #90: a value that places shown rock parts (`X`, `Y` all the
    /// copy's, `OffsetX`, `OffsetY` its layer's) moves them a whole pixel
    /// at a time, as it crosses whole pixels: at each crossing the parts
    /// that change place go there through `mover` ([`PartMover`]). Rock in
    /// their way ends the motion there ([`EventKind::LiveBlocked`]); a
    /// wedged worm holds the value until the next tick. Either way the
    /// value stays short of the crossing, where its parts are.
    pub(crate) fn step_motions(&mut self, events: &mut TickEvents, mover: &mut impl PartMover) {
        if self.motions.iter().all(|m| !m.moving()) {
            return;
        }
        let level = self.level.clone();
        for (i, c) in level.copies.iter().enumerate() {
            let Ok(copy) = u16::try_from(i) else { break };
            let r = self.range(copy);
            for (k, p) in c.props.iter().enumerate().take(r.len()) {
                let at = r.start + k;
                let m = self.motions[at];
                if !m.moving() {
                    continue;
                }
                let v = self.values[at];
                let next = if v < m.target {
                    v.saturating_add(m.speed).min(m.target)
                } else {
                    v.saturating_sub(m.speed).max(m.target)
                };
                let next = c.clamped(p, next);
                let prop = k as u16;
                let (reached, blocked) = self.walk(c, copy, prop, at, next, mover);
                if blocked {
                    self.motions[at] = LiveMotion::default();
                    events.push(EventKind::LiveBlocked { copy, prop });
                } else if reached == m.target {
                    self.motions[at] = LiveMotion::default();
                    events.push(EventKind::LiveArrived { copy, prop });
                }
            }
        }
    }

    /// Moves value `at` (prop `prop` of `copy`, setup `c`) to `to`, crossing
    /// by crossing as its shown rock parts change place (issue #90).
    /// Returns where it got and whether rock stopped it.
    fn walk(
        &mut self,
        c: &LiveCopy,
        copy: u16,
        prop: u16,
        at: usize,
        to: i64,
        mover: &mut impl PartMover,
    ) -> (i64, bool) {
        let Some(axis) = Axis::of(c, prop) else {
            self.values[at] = to;
            return (to, false);
        };
        let r = self.range(copy);
        // The parts it places: their bits, and every offset at which one of
        // their whole-pixel terms crosses (the other values in the sum).
        let mut moved = 0u64;
        let mut offs = [0i64; 2 * crate::solid::MAX_LIVE_SOLID_PARTS];
        let mut n = 0;
        for (i, sp) in self.solids.parts().iter().enumerate() {
            if sp.copy != copy || !axis.places(sp) {
                continue;
            }
            moved |= 1 << i;
            let (sum, flip_term) = axis.sum(c, &self.values[r.clone()], sp);
            offs[n] = sum.saturating_sub(self.values[at]);
            n += 1;
            // A mirrored part also moves with its copy's rectangle.
            if let Some(t) = flip_term {
                offs[n] = t.saturating_sub(self.values[at]);
                n += 1;
            }
        }
        let mut v = self.values[at];
        if moved == 0 {
            self.values[at] = to;
            return (to, false);
        }
        let dir = (to - v).signum();
        let mut moves = [(0usize, (0i32, 0i32)); crate::solid::MAX_LIVE_SOLID_PARTS];
        // Every part crosses at most `MAX_PART_STEP` + 1 times; a bound
        // keeps a pathological setup from stalling.
        for _ in 0..(n + 1) * (MAX_PART_STEP as usize + 2) {
            if v == to {
                break;
            }
            let Some(cross) = offs[..n]
                .iter()
                .filter_map(|&o| crossing(v, o, dir))
                .reduce(|a, b| if dir > 0 { a.min(b) } else { a.max(b) })
            else {
                break;
            };
            if (dir > 0 && cross > to) || (dir < 0 && cross < to) {
                v = to;
                break;
            }
            self.values[at] = cross;
            let values = &self.values[r.clone()];
            let mut k = 0;
            for (i, sp) in self.solids.parts().iter().enumerate() {
                if moved & (1 << i) == 0 {
                    continue;
                }
                let place = part_place(c, values, sp.part_at, sp.layer, sp.size.0, sp.flip);
                if place != sp.at {
                    moves[k] = (i, place);
                    k += 1;
                }
            }
            if k == 0 {
                v = cross;
                continue;
            }
            match mover.step(&mut self.solids, &self.level, copy, &moves[..k]) {
                PartStep::Taken => v = cross,
                PartStep::Blocked => {
                    self.values[at] = cross - dir;
                    return (cross - dir, true);
                }
                PartStep::Wait => {
                    v = cross - dir;
                    break;
                }
            }
        }
        self.values[at] = v;
        (v, false)
    }

    /// The live part of [`crate::World::checksum`]: every copy's state,
    /// then every value, after their counts; then, in a world with
    /// scripts (only then, so a level without them hashes as before), the
    /// scripts' state (issue #88) and every value's motion (issue #89:
    /// only scripts start one); last, only while a worm is wedged (so a
    /// world nobody was crushed in hashes as before), the crush counts
    /// (issue #90).
    pub(crate) fn hash(&self, h: &mut Fnv) {
        let LiveWorld { level: _, offsets: _, states, values, motions, start: _, script, crush, solids: _ } =
            self;
        h.usize(states.len());
        for &s in states.iter() {
            h.bytes(&s.to_le_bytes());
        }
        h.usize(values.len());
        for &v in values.iter() {
            h.u64(v as u64);
        }
        if let Some(s) = script {
            s.hash(h);
            for m in motions.iter() {
                h.u64(m.target as u64);
                h.u64(m.speed as u64);
            }
        }
        if crush.iter().any(|&c| c != 0) {
            h.usize(crush.len());
            for &c in crush.iter() {
                h.bytes(&c.to_le_bytes());
            }
        }
    }

    /// The live part of [`crate::World::state`].
    pub(crate) fn wire(&self) -> LiveWire {
        let LiveWorld { level: _, offsets: _, states, values, motions, start: _, script, crush, solids: _ } =
            self;
        LiveWire {
            states: states.to_vec(),
            values: values.to_vec(),
            motions: motions.to_vec(),
            script: script.as_ref().map(|s| s.wire()),
            crush: crush.to_vec(),
        }
    }

    /// Whether `w` fits this world's setup: as many copies and values,
    /// each state one of its copy's, each value within its limits, a
    /// motion per value (a moving one position-like, its target within
    /// the limits, its speed 1 to [`MAX_MOTION_SPEED`]; none in a world
    /// without scripts), and a scripts' part exactly when this world has
    /// one, fitting it (`ScriptWorld::can_load`); a crush count per worm
    /// slot (issue #90).
    pub(crate) fn can_load(&self, w: &LiveWire) -> bool {
        let LiveWire { states, values, motions, script, crush } = w;
        let script_ok = match (&self.script, script) {
            (None, None) => true,
            (Some(s), Some(w)) => s.can_load(w, &self.level),
            _ => false,
        };
        let motion_ok = |c: &LiveCopy, p: &LiveProp, m: &LiveMotion| {
            *m == LiveMotion::default()
                || (self.script.is_some()
                    && p.target.is_position()
                    && (1..=MAX_MOTION_SPEED).contains(&m.speed)
                    && c.clamped(p, m.target) == m.target)
        };
        script_ok
            && crush.len() == self.crush.len()
            && states.len() == self.states.len()
            && values.len() == self.values.len()
            && motions.len() == self.values.len()
            && self.level.copies.iter().enumerate().all(|(i, c)| {
                let Ok(copy) = u16::try_from(i) else { return false };
                let r = self.range(copy);
                states.get(i).is_some_and(|&s| usize::from(s) < c.states.len())
                    && r.len() == c.props.len()
                    && values[r.clone()].iter().zip(&c.props).all(|(&v, p)| c.clamped(p, v) == v)
                    && motions[r].iter().zip(&c.props).all(|(m, p)| motion_ok(c, p, m))
            })
    }

    /// Takes `w`, checked by [`LiveWorld::can_load`]; keeps every buffer
    /// and rebuilds the parts' index.
    pub(crate) fn load(&mut self, w: LiveWire) {
        let LiveWire { states, values, motions, script, crush } = w;
        self.states.copy_from_slice(&states);
        self.values.copy_from_slice(&values);
        self.motions.copy_from_slice(&motions);
        self.crush.copy_from_slice(&crush);
        if let (Some(s), Some(w)) = (&mut self.script, script) {
            s.load(w);
        }
        self.refresh_solids();
    }
}

/// The live copies' part of a [`crate::WorldState`]: each copy's state and
/// every value and its motion (issue #89), as [`LiveWorld`] holds them,
/// the scripts' state in a world with scripts (issue #88), and each worm
/// slot's crush count (issue #90).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWire {
    states: Vec<u16>,
    values: Vec<i64>,
    motions: Vec<LiveMotion>,
    script: Option<ScriptWire>,
    crush: Vec<u16>,
}

impl LiveWire {
    /// The scripts' part, for tests of [`crate::World::load_state`]'s
    /// checks.
    #[doc(hidden)]
    pub fn script_mut_for_test(&mut self) -> &mut Option<ScriptWire> {
        &mut self.script
    }

    /// The crush counts by worm slot, for tests of
    /// [`crate::World::load_state`]'s checks.
    #[doc(hidden)]
    pub fn crush_mut_for_test(&mut self) -> &mut Vec<u16> {
        &mut self.crush
    }

    /// Sets value `i`'s motion (flat), for tests of
    /// [`crate::World::load_state`]'s checks.
    #[doc(hidden)]
    pub fn edit_motion_for_test(&mut self, i: usize, m: LiveMotion) {
        if let Some(x) = self.motions.get_mut(i) {
            *x = m;
        }
    }

    /// Sets copy `copy`'s state and value `i` (flat), for tests of
    /// [`crate::World::load_state`]'s checks.
    #[doc(hidden)]
    pub fn edit_for_test(&mut self, state: Option<(usize, u16)>, value: Option<(usize, i64)>) {
        if let Some((i, s)) = state
            && let Some(x) = self.states.get_mut(i)
        {
            *x = s;
        }
        if let Some((i, v)) = value
            && let Some(x) = self.values.get_mut(i)
        {
            *x = v;
        }
    }
}

/// A world's live copies, read-only ([`crate::World::live`]): for
/// presentation, bots and tools.
#[derive(Clone, Copy, Debug)]
pub struct LiveView<'a> {
    world: &'a LiveWorld,
}

impl<'a> LiveView<'a> {
    pub(crate) fn new(world: &'a LiveWorld) -> Self {
        LiveView { world }
    }

    /// The setup: names, parts and pictures.
    pub fn level(&self) -> &'a LiveLevel {
        &self.world.level
    }

    /// How many live copies there are.
    pub fn len(&self) -> usize {
        self.world.level.copies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copy `copy` (an index).
    pub fn copy(&self, copy: u16) -> Option<LiveCopyView<'a>> {
        let setup = self.world.level.copies.get(usize::from(copy))?;
        let state = self.world.states.get(usize::from(copy)).copied().unwrap_or(0);
        Some(LiveCopyView {
            index: copy,
            setup,
            state,
            values: self.world.values(copy),
            motions: self.world.motions(copy),
        })
    }

    /// Copy `id`.
    pub fn copy_by_id(&self, id: &str) -> Option<LiveCopyView<'a>> {
        self.copy(self.world.level.copy(id)?)
    }

    /// Every copy, bottom to top.
    pub fn copies(&self) -> impl Iterator<Item = LiveCopyView<'a>> + 'a {
        let v = *self;
        (0..self.len()).map_while(move |i| v.copy(u16::try_from(i).ok()?))
    }
}

/// One live copy as the world has it now ([`LiveView::copy`]).
#[derive(Clone, Copy, Debug)]
pub struct LiveCopyView<'a> {
    index: u16,
    setup: &'a LiveCopy,
    state: u16,
    values: &'a [i64],
    motions: &'a [LiveMotion],
}

/// Whole world pixels of a position in 1/[`LIVE_SUBPIXELS`] pixel, rounded
/// down.
fn whole(v: i64) -> i32 {
    let p = v.div_euclid(LIVE_SUBPIXELS);
    i32::try_from(p).unwrap_or(if p < 0 { i32::MIN } else { i32::MAX })
}

impl<'a> LiveCopyView<'a> {
    /// Its index among the level's copies.
    pub fn index(&self) -> u16 {
        self.index
    }

    /// Its id.
    pub fn id(&self) -> &'a str {
        &self.setup.id
    }

    /// Its setup data.
    pub fn setup(&self) -> &'a LiveCopy {
        self.setup
    }

    /// Its state, an index into [`LiveCopy::states`].
    pub fn state(&self) -> u16 {
        self.state
    }

    /// Its state's name ("" for a copy without states).
    pub fn state_name(&self) -> &'a str {
        self.setup.states.get(usize::from(self.state)).map_or("", |s| s.name.as_str())
    }

    /// Its values in the world's units (positions in 1/[`LIVE_SUBPIXELS`]
    /// pixel), by [`LiveCopy::props`] index.
    pub fn values(&self) -> &'a [i64] {
        self.values
    }

    /// Its value named `name` (an exposed name).
    pub fn value(&self, name: &str) -> Option<i64> {
        self.values.get(usize::from(self.setup.prop(name)?)).copied()
    }

    /// Its values' motions (issue #89), by [`LiveCopy::props`] index.
    pub fn motions(&self) -> &'a [LiveMotion] {
        self.motions
    }

    /// The value driving `target` (the first value that does), in the
    /// world's units; `None` when no value drives it (issue #87: what
    /// presentation reads to draw a part, such as `Facing`, a layer's
    /// `Opacity` or `GlowStrength`).
    pub fn target(&self, target: LiveTarget) -> Option<i64> {
        self.setup.value_of(self.values, target)
    }

    /// Where the copy is, world pixels: its placed top-left moved by its
    /// `X` and `Y` (rounded down), and its size.
    pub fn rect(&self) -> (i32, i32, u32, u32) {
        let (x, y) = self.origin();
        (whole(x), whole(y), self.setup.size.0, self.setup.size.1)
    }

    /// The copy's top-left in 1/[`LIVE_SUBPIXELS`] pixel.
    fn origin(&self) -> (i64, i64) {
        let c = self.setup;
        let at = |p: i32, t| i64::from(p) * LIVE_SUBPIXELS + c.value_of(self.values, t).unwrap_or(0);
        (at(c.at.0, LiveTarget::X), at(c.at.1, LiveTarget::Y))
    }

    /// Whether the copy is shown: its `Visible` value, if it has one, is
    /// not 0.
    pub fn visible(&self) -> bool {
        self.setup.value_of(self.values, LiveTarget::Visible).is_none_or(|v| v != 0)
    }

    /// Its state's parts that show, bottom to top, each with its picture's
    /// top-left in world pixels (rounded down): the copy's place and `X`/`Y`,
    /// the part's place, and its layer's `OffsetX`/`OffsetY`. None while
    /// the copy is hidden; a part whose layer's `LayerVisible` is 0 is left
    /// out.
    pub fn visible_parts(&self) -> impl Iterator<Item = (&'a LivePart, (i32, i32))> + 'a {
        let (c, values) = (self.setup, self.values);
        let (ox, oy) = self.origin();
        let parts = if self.visible() {
            c.states.get(usize::from(self.state)).map_or(&[][..], |s| &s.parts[..])
        } else {
            &[]
        };
        parts.iter().filter_map(move |p| {
            let (mut x, mut y) =
                (ox + i64::from(p.at.0) * LIVE_SUBPIXELS, oy + i64::from(p.at.1) * LIVE_SUBPIXELS);
            if let Some(l) = p.layer {
                if c.value_of(values, LiveTarget::LayerVisible(l)) == Some(0) {
                    return None;
                }
                x += c.value_of(values, LiveTarget::OffsetX(l)).unwrap_or(0);
                y += c.value_of(values, LiveTarget::OffsetY(l)).unwrap_or(0);
            }
            Some((p, (whole(x), whole(y))))
        })
    }

    /// [`LiveCopyView::visible_parts`] that are rock: solid where their
    /// picture's [`LiveSolid`] is.
    pub fn rock_parts(&self) -> impl Iterator<Item = (&'a LivePart, (i32, i32))> + 'a {
        self.visible_parts().filter(|(p, _)| p.kind == LivePartKind::Rock)
    }
}
