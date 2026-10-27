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
//! Nothing moves and nothing reads the parts yet (#89-#91).

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

/// `v` within `min..=max`, never a panic (`i64::clamp` panics when `min >
/// max`: a broken setup keeps `max`).
fn within(v: i64, min: i64, max: i64) -> i64 {
    v.max(min).min(max)
}

impl LiveCopy {
    /// Value `prop`'s limits in the world's units: its prop's, a position
    /// in 1/[`LIVE_SUBPIXELS`] pixel, `Health` also within `0..=points`.
    fn limits(&self, prop: &LiveProp) -> (i64, i64) {
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
}

impl fmt::Display for LiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiveError::NoCopy(c) => write!(f, "no live copy {c}"),
            LiveError::NoState { copy, state } => write!(f, "live copy {copy} has no state {state}"),
            LiveError::NoProp { copy, prop } => write!(f, "live copy {copy} has no value {prop}"),
        }
    }
}

/// The live copies' part of a world (issue #86): per copy its state and
/// its values, one `i64` per [`LiveCopy::props`] entry, flat in one
/// buffer, so nothing a tick does allocates. `LiveWorld` is where #88 adds
/// script state and #89 motions: each extends this struct, its checksum
/// ([`LiveWorld::hash`]) and its transfer ([`LiveWire`]).
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
    /// Setup: where the match's frame clock started (scripts' `time()`).
    pub(crate) start: AbsTime,
    /// The scripts' state (issue #88), present when the setup has a
    /// director.
    pub(crate) script: Option<ScriptWorld>,
}

impl LiveWorld {
    /// The copies of `level` at their defaults, before they enter their
    /// starting states ([`LiveWorld::start`]).
    pub(crate) fn new(
        level: Arc<LiveLevel>,
        director: Option<Box<dyn Director>>,
        seed: u32,
        start: AbsTime,
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
        let script = director.map(|d| ScriptWorld::new(d, &level, seed));
        LiveWorld { level, offsets, states, values, start, script }
    }

    /// Every copy's state, every value, the values' offsets, and the
    /// scripts' part, borrowed apart.
    pub(crate) fn parts_mut(&mut self) -> (&[u16], &[i64], &[u32], &mut Option<ScriptWorld>) {
        (&self.states, &self.values, &self.offsets, &mut self.script)
    }

    /// At setup: every copy enters its starting state (an event each, with
    /// the first tick's), then its own starting values apply.
    pub(crate) fn start(&mut self, events: &mut TickEvents) {
        let level = self.level.clone();
        for (i, c) in level.copies.iter().enumerate() {
            let Ok(copy) = u16::try_from(i) else { break };
            if usize::from(c.start_state) < c.states.len() {
                self.enter(copy, c.start_state, events);
            }
            for &(prop, v) in &c.start_values {
                if let Some(p) = c.props.get(usize::from(prop)) {
                    self.put(copy, prop, p.target.scale(v));
                }
            }
        }
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
        let level = self.level.clone();
        let Some(s) = level.copies.get(usize::from(copy)).and_then(|c| c.states.get(usize::from(state)))
        else {
            return;
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
        events.push(EventKind::LiveState { copy, state });
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
        Ok(())
    }

    /// [`crate::World::set_live_value`].
    pub(crate) fn set_value(&mut self, copy: u16, prop: u16, value: i64) -> Result<(), LiveError> {
        let c = self.level.copies.get(usize::from(copy)).ok_or(LiveError::NoCopy(copy))?;
        if usize::from(prop) >= c.props.len() {
            return Err(LiveError::NoProp { copy, prop });
        }
        self.put(copy, prop, value);
        Ok(())
    }

    /// The live part of [`crate::World::checksum`]: every copy's state,
    /// then every value, after their counts.
    /// then, in a world with scripts (only then, so a level without them
    /// hashes as before), the scripts' state (issue #88).
    pub(crate) fn hash(&self, h: &mut Fnv) {
        let LiveWorld { level: _, offsets: _, states, values, start: _, script } = self;
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
        }
    }

    /// The live part of [`crate::World::state`].
    pub(crate) fn wire(&self) -> LiveWire {
        let LiveWorld { level: _, offsets: _, states, values, start: _, script } = self;
        LiveWire {
            states: states.to_vec(),
            values: values.to_vec(),
            script: script.as_ref().map(|s| s.wire()),
        }
    }

    /// Whether `w` fits this world's setup: as many copies and values,
    /// each state one of its copy's, each value within its limits, and a
    /// scripts' part exactly when this world has one, fitting it
    /// (`ScriptWorld::can_load`).
    pub(crate) fn can_load(&self, w: &LiveWire) -> bool {
        let LiveWire { states, values, script } = w;
        let script_ok = match (&self.script, script) {
            (None, None) => true,
            (Some(s), Some(w)) => s.can_load(w, &self.level),
            _ => false,
        };
        script_ok
            && states.len() == self.states.len()
            && values.len() == self.values.len()
            && self.level.copies.iter().enumerate().all(|(i, c)| {
                let Ok(copy) = u16::try_from(i) else { return false };
                let r = self.range(copy);
                states.get(i).is_some_and(|&s| usize::from(s) < c.states.len())
                    && r.len() == c.props.len()
                    && values[r].iter().zip(&c.props).all(|(&v, p)| c.clamped(p, v) == v)
            })
    }

    /// Takes `w`, checked by [`LiveWorld::can_load`]; keeps every buffer.
    pub(crate) fn load(&mut self, w: LiveWire) {
        let LiveWire { states, values, script } = w;
        self.states.copy_from_slice(&states);
        self.values.copy_from_slice(&values);
        if let (Some(s), Some(w)) = (&mut self.script, script) {
            s.load(w);
        }
    }
}

/// The live copies' part of a [`crate::WorldState`]: each copy's state and
/// every value, as [`LiveWorld`] holds them, and the scripts' state in a
/// world with scripts (issue #88).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveWire {
    states: Vec<u16>,
    values: Vec<i64>,
    script: Option<ScriptWire>,
}

impl LiveWire {
    /// The scripts' part, for tests of [`crate::World::load_state`]'s
    /// checks.
    #[doc(hidden)]
    pub fn script_mut_for_test(&mut self) -> &mut Option<ScriptWire> {
        &mut self.script
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
        Some(LiveCopyView { index: copy, setup, state, values: self.world.values(copy) })
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
