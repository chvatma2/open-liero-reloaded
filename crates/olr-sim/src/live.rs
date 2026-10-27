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
//! through sorted tables. Data only: issue #86 puts it in the world.

use alloc::string::String;
use alloc::vec::Vec;

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
