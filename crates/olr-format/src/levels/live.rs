//! A level's live copies in their played form (issue #85; the design
//! `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`, calls
//! 8, 11, 13 and 19): `live.ron` and the files under `live/`, written by
//! `olr_edit::live::bake_live` when a level is saved and read with the
//! level. Unlike `editor/`, they are play files: `olr_pack::content_hash`
//! covers them, so downloads, replays and online setups carry and check
//! a level's live copies.
//!
//! Each live copy is stored at its placed size, split into parts (a
//! targeted layer its own part, a targeted mixed layer a background and a
//! rock part, untargeted layers flattened in runs of one kind), every
//! state a complete look: a value for every property any state sets.
//! Every value is an `i64`, positions in world pixels. [`check_live`]
//! holds every rule; the reader refuses a played form that breaks one,
//! and with it the level (live copies are gameplay, unlike a look).

use std::collections::BTreeSet;

use olr_data::image::RgbaImage;
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{MAX_SIDE, valid_copy_id};
use crate::FormatError;
use crate::assets::{
    MAX_ASSET_NAME, MAX_ASSET_SOUND_BYTES, MAX_HEALTH, MAX_SCRIPT_BYTES, MAX_STATES, valid_exposed_name,
    valid_sound_name, valid_state_name,
};
use crate::defaults as d;
use crate::image_io::{self, LEVEL_ART};
use crate::look::{AnimationSource, GlowGroupSource, LookSource, MAX_GROUPS, compile_animation};
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};

/// The played form's file in a level pack.
pub const LIVE_FILE: &str = "live.ron";
/// The folder of the played form's pictures, scripts and sounds.
pub const LIVE_DIR: &str = "live";
/// The most live copies a level has.
pub const MAX_LIVE_COPIES: usize = 64;
/// The most scripts a level's played form holds: the level's, and a
/// default and an own per live copy.
pub const MAX_SCRIPTS: usize = 1 + 2 * MAX_LIVE_COPIES;
/// The most sounds a level's played form holds.
pub const MAX_LIVE_SOUNDS: usize = 64;
/// The most parts a live copy has in a state.
pub const MAX_PARTS: usize = 32;
/// The most rock parts a level's live copies show at once (each copy's
/// state with the most, summed; issue #90): `olr_sim::MAX_LIVE_SOLID_PARTS`,
/// the simulation's index keeps a bit per part.
pub const MAX_LIVE_SOLID_PARTS: usize = 64;
/// The most pictures a live copy has.
pub const MAX_PICTURES: usize = 128;
/// The most values (properties) a live copy has.
pub const MAX_LIVE_PROPS: usize = 96;
/// The most animations in a live copy's animation table.
pub const MAX_ANIMATIONS: usize = 256;
/// The most files the played form has: half of
/// `olr_pack::archive::MAX_ARCHIVE_FILES` (a test holds them so), the
/// rest for the ground and its look, so a saved level can always be
/// downloaded.
pub const MAX_LIVE_FILES: usize = 2048;
/// The most directories the played form has: `live/` and one per copy
/// (within `olr_pack::archive::MAX_DIRS`).
pub const MAX_LIVE_DIRS: usize = 1 + MAX_LIVE_COPIES;
/// The most bytes the played form's pictures decode to.
pub const MAX_LIVE_BYTES: u64 = 64 << 20;
/// The largest magnitude of a value (a position stretched 4096 times
/// across 4096 asset pixels stays within it).
pub const MAX_LIVE_VALUE: i64 = 1 << 24;
/// The longest path of the played form in a pack
/// (`live/63/127-groups.png`): every path is made of indices, so a level's
/// live files stay within the shipped content's 80 characters whatever
/// its assets are named.
pub const MAX_LIVE_PATH: usize = 22;

/// `live.ron`: a level's live copies as matches play them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveFile {
    /// The live copies, bottom to top, at most `MAX_LIVE_COPIES` (64);
    /// the `i`-th one's pictures are in `live/<i>/`.
    pub copies: Vec<LiveCopyFile>,
    /// The scripts, each `live/<k>.rhai` for its index `k` (UTF-8 Rhai, at
    /// most 64 KiB), at most `MAX_SCRIPTS` (129: the level's, and a default
    /// and an own per copy); identical texts are stored once. Default
    /// none. A script that does not compile refuses the level. Each tick
    /// (issue #88, `olr-script`) the level script runs, then each copy's
    /// default and own script: `start()` once, due timers, the handlers
    /// `entered(state)`, `spawned(worm)`, `killed(victim, killer)`,
    /// `exploded(x, y, damage)` and `damaged(worm, amount, by)` for the
    /// events since the last script phase, then `tick()`. Each run's API
    /// is `this`: `me`, `id`, `copy(id)`, `state`, `set_state`, `get`,
    /// `set`, `has` (of this copy, or of a copy `c` as the first
    /// argument), `switch_off_default`, `after(ticks, "fn")`,
    /// `every(ticks, "fn")`, `cancel("fn")`, `rand(n)`, `chance(percent)`,
    /// `time`, `game_over`, `worm_count`, `worm_alive(i)`, `worm_x(i)`,
    /// `worm_y(i)` and `mem` (a map of at most 32 integers, booleans and
    /// strings, kept between runs). Integers only; at most 5,000
    /// operations a run and 20,000 a tick (the runs past it are skipped,
    /// due timers firing next tick); a run
    /// that fails changes nothing, and 16 failures switch its script off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<String>,
    /// The level script: an index into `scripts`. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_script: Option<u16>,
    /// The sounds, each `live/<k>.ogg` or `live/<k>.wav` for its index `k`
    /// (at most 2 MiB and 60 s, as a mod's), at most `MAX_LIVE_SOUNDS`
    /// (64); identical files are stored once. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<String>,
}

/// A live copy: a placed copy that stays an object in the match.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCopyFile {
    /// The copy's id (`editor/placed.ron`'s): scripts reach it by it.
    /// Unique among the live copies.
    pub id: String,
    /// The name of its asset, 1 to 64 characters, for messages.
    pub asset: String,
    /// Its top-left, world pixels; its box lies inside the level.
    pub at: (i32, i32),
    /// Its placed size, world pixels, each side 1 to 4096.
    pub size: (u32, u32),
    /// Its values, at most `MAX_LIVE_PROPS` (96): every property the
    /// asset exposes or any state sets, and `Health` for a destructible
    /// asset. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<LivePropFile>,
    /// Its pictures, at most `MAX_PICTURES` (128); the `k`-th one's files
    /// are `live/<copy>/<k>.png` and its optional masks.
    pub pictures: Vec<LivePictureFile>,
    /// Its states, 1 to 32, in the asset's order.
    pub states: Vec<LiveStateFile>,
    /// The index of its starting state. Default 0.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub start_state: u16,
    /// The copy's own values (prop index, value) of properties no state
    /// sets, applied once after its starting state's (the others are in
    /// every state's values). Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub start_values: Vec<(u16, i64)>,
    /// The copy's animation table: every distinct animation its defaults,
    /// states and values name, in order of first appearance, at most
    /// `MAX_ANIMATIONS` (256); an `Animation` value is an index into it.
    /// Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<AnimationSource>,
    /// The asset's glow groups, the copy's own at run time (two copies
    /// change colour apart); a picture's groups name them 1-based. The
    /// level's groups and every copy's are at most 255 together. Default
    /// none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<GlowGroupSource>,
    /// The scripts run on the copy, in order (indices into the level's
    /// `scripts`): the asset's default unless the copy switched it off,
    /// then the copy's own. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<u16>,
    /// The asset's sounds by name (each `[a-z0-9_-]` then `.wav` or `.ogg`,
    /// unique), each an index into the level's `sounds`. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<(String, u16)>,
    /// The copy can be destroyed. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<LiveHealthFile>,
}

/// One value of a live copy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePropFile {
    /// Its exposed name (`[a-z_][a-z0-9_]*`, 1 to 32 bytes, unique in the
    /// copy); none for a property only states set, which scripts cannot
    /// reach. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What it drives.
    pub drives: Target,
    /// Its least value (world pixels for a position).
    pub min: i64,
    /// Its greatest value, at least `min`.
    pub max: i64,
    /// Its value where no state and no copy sets it, `min` to `max`.
    pub default: i64,
}

/// What a live copy's value drives: the asset's `PropertySource` with its
/// layer mapped to the copy's parts of that layer (`LivePartFile::layer`,
/// the layer's index in the asset, bottom first). Values: an `Int` as it
/// is (positions in world pixels, scaled with the copy's stretch), a
/// `Bool` 0 or 1, a `Colour` `0xRRGGBB` (`-1`: the glow group's own,
/// each pixel's colour), an `Animation` an index into the copy's
/// animation table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Target {
    /// The copy's offset from where it is placed, world pixels.
    X,
    /// As `X`, down.
    Y,
    /// 0 or 1: the copy is drawn and solid.
    Visible,
    /// 0 or 1: 1 mirrors the copy left to right within its box.
    Facing,
    /// 0 to the copy's health points.
    Health,
    /// A layer's offset inside the copy, world pixels.
    OffsetX(u16),
    /// As `OffsetX`, down.
    OffsetY(u16),
    /// 0 or 1: a layer shown (hidden rock is not solid).
    LayerVisible(u16),
    /// Percent 0 to 100: a background layer's opacity.
    Opacity(u16),
    /// Percent 0 to 100: a layer's glow strength (each pixel mask ×
    /// value).
    GlowStrength(u16),
    /// `0xRRGGBB` (or -1, its own): the layer's glow group's colour.
    GlowColour(u16),
    /// Percent 0 to 1000: the layer's glow group's intensity.
    LightIntensity(u16),
    /// World pixels 1 to 4096: the layer's glow group's reach.
    LightReach(u16),
    /// An index into the animation table: the layer's glow group's
    /// animation.
    GlowAnimation(u16),
    /// Degrees 0 to 359: the layer's beam's direction.
    BeamDirection(u16),
    /// Degrees 1 to 90: the layer's beam's half-angle.
    BeamSpread(u16),
    /// Percent 1 to 99: a glass layer's opacity.
    GlassOpacity(u16),
    /// Percent 0 to 100: a glass layer's tint.
    GlassTint(u16),
}

impl Target {
    /// The layer it names; none for the copy's own properties.
    pub fn layer(&self) -> Option<u16> {
        use Target as T;
        match *self {
            T::X | T::Y | T::Visible | T::Facing | T::Health => None,
            T::OffsetX(l)
            | T::OffsetY(l)
            | T::LayerVisible(l)
            | T::Opacity(l)
            | T::GlowStrength(l)
            | T::GlowColour(l)
            | T::LightIntensity(l)
            | T::LightReach(l)
            | T::GlowAnimation(l)
            | T::BeamDirection(l)
            | T::BeamSpread(l)
            | T::GlassOpacity(l)
            | T::GlassTint(l) => Some(l),
        }
    }

    /// The range every value of it lies in (an animation's: up to the
    /// table's last index, `animations` long).
    pub fn bounds(&self, animations: usize) -> (i64, i64) {
        use Target as T;
        match self {
            T::Visible | T::Facing | T::LayerVisible(_) => (0, 1),
            T::Health => (0, i64::from(MAX_HEALTH)),
            T::GlowColour(_) => (-1, 0xff_ffff),
            T::GlowAnimation(_) => (0, animations as i64 - 1),
            T::Opacity(_) | T::GlowStrength(_) | T::GlassTint(_) => (0, 100),
            T::LightIntensity(_) => (0, i64::from(crate::look::MAX_INTENSITY)),
            T::LightReach(_) => (1, i64::from(crate::look::MAX_REACH)),
            T::BeamDirection(_) => (0, 359),
            T::BeamSpread(_) => (1, 90),
            T::GlassOpacity(_) => (1, 99),
            T::X | T::Y | T::OffsetX(_) | T::OffsetY(_) => (-MAX_LIVE_VALUE, MAX_LIVE_VALUE),
        }
    }
}

/// One of a live copy's pictures: `live/<copy>/<k>.png` (RGBA, its size ×
/// the level's art scale) and the masks it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePictureFile {
    /// World pixels, each side 1 to the copy's.
    pub size: (u32, u32),
    /// `<k>-rock.png`: its solid mask, grey per world pixel (0 open, any
    /// other value solid). A rock part's picture has it, a background
    /// part's not. Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub rock: bool,
    /// `<k>-glow.png`: its unscaled glow mask, grey per art pixel (the
    /// shown glow is mask × the part's strength, as the level's glow).
    /// Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub glow: bool,
    /// `<k>-groups.png`: the copy's own glow group per world pixel, grey,
    /// 1-based into the copy's `groups` (0 none). Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub groups: bool,
    /// `<k>-beam.png`: direction and spread per world pixel, grey-alpha,
    /// as `look/glow-beam.png`. Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub beam: bool,
    /// `<k>-glass.png`: its glass, RGBA per art pixel, as
    /// `look/glass.png`. Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub glass: bool,
}

/// A state of a live copy: a complete look.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveStateFile {
    /// The asset's state name (unique in the copy, case aside); scripts
    /// name the state by it.
    pub name: String,
    /// (prop index, value) for every property any state of the asset
    /// sets: this state's value, else the copy's own, else the property's
    /// default (a static copy's pose). Entering the state sets them all.
    /// Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<(u16, i64)>,
    /// Its parts, bottom to top, at most `MAX_PARTS` (32).
    pub parts: Vec<LivePartFile>,
    /// An index into the level's `sounds`, played once when the state is
    /// entered. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enter_sound: Option<u16>,
    /// An index into the level's `sounds`, looped while the state lasts.
    /// Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_sound: Option<u16>,
}

/// One part of a live copy in a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePartFile {
    /// The asset layer it shows (its index, bottom first), which values
    /// target; none for untargeted layers flattened together. A targeted
    /// mixed layer's background and rock parts share it. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<u16>,
    /// Rock or background.
    pub kind: PartKind,
    /// An index into the copy's `pictures`: a rock part's picture has a
    /// rock mask, a background part's none.
    pub picture: u16,
    /// Its picture's top-left in the copy, world pixels; the picture lies
    /// inside the copy. Default `(0, 0)`.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub at: (i32, i32),
    /// Percent 0 to 100: its glow strength as authored (shown: mask ×
    /// strength). Default 0.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub glow_strength: u8,
}

/// What a part is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartKind {
    /// Solid, drawn as front art; never dug.
    Rock,
    /// Scenery, drawn as back art.
    Background,
}

/// A destructible live copy's health.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveHealthFile {
    /// 1 to 10000.
    pub points: u32,
    /// The index of the state entered at 0.
    pub at_zero: u16,
}

/// A live picture's pixels: the art (its size × the level's art scale)
/// and the masks it has.
#[derive(Clone, Debug, PartialEq)]
pub struct LivePicture {
    pub art: RgbaImage,
    /// Per world pixel: 0 open, else solid.
    pub rock: Option<Vec<u8>>,
    /// Per art pixel, 0-255.
    pub glow: Option<Vec<u8>>,
    /// Per world pixel, 1-based into the copy's groups.
    pub groups: Option<Vec<u8>>,
    /// Per world pixel, two bytes (direction / 2 + 1, spread).
    pub beam: Option<Vec<u8>>,
    /// Per art pixel, RGBA.
    pub glass: Option<RgbaImage>,
}

impl LivePicture {
    /// Its entry in `live.ron` (`size` world pixels).
    pub fn entry(&self, size: (u32, u32)) -> LivePictureFile {
        LivePictureFile {
            size,
            rock: self.rock.is_some(),
            glow: self.glow.is_some(),
            groups: self.groups.is_some(),
            beam: self.beam.is_some(),
            glass: self.glass.is_some(),
        }
    }
}

/// A level's played form in memory: `live.ron`, each copy's pictures,
/// the scripts' texts and the sounds' bytes (both indexed as `file`'s).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveSource {
    pub file: LiveFile,
    pub pictures: Vec<Vec<LivePicture>>,
    pub scripts: Vec<String>,
    pub sounds: Vec<Vec<u8>>,
}

/// The glow a part shows at a pixel of mask value `mask` at strength
/// `strength` percent: the level bake's (`(round(strength × 2.55) ×
/// mask + 127) / 255`).
pub fn glow_value(strength: u8, mask: u8) -> u8 {
    let s = (u32::from(strength.min(100)) * 255 + 50) / 100;
    ((s * u32::from(mask) + 127) / 255) as u8
}

/// Script `k`'s path: `live/<k>.rhai`.
pub fn live_script_file(k: usize) -> String {
    format!("{LIVE_DIR}/{k}.rhai")
}

/// Sound `k`'s path with extension `ext` (`ogg` or `wav`).
pub fn live_sound_file(k: usize, ext: &str) -> String {
    format!("{LIVE_DIR}/{k}.{ext}")
}

/// The path of copy `copy`'s picture `k` with `suffix` (`""`, `-rock`,
/// `-glow`, `-groups`, `-beam` or `-glass`).
pub fn live_picture_file(copy: usize, k: usize, suffix: &str) -> String {
    format!("{LIVE_DIR}/{copy}/{k}{suffix}.png")
}

/// The files a picture entry names, as suffixes.
fn suffixes(p: &LivePictureFile) -> impl Iterator<Item = &'static str> {
    [
        ("", true),
        ("-rock", p.rock),
        ("-glow", p.glow),
        ("-groups", p.groups),
        ("-beam", p.beam),
        ("-glass", p.glass),
    ]
    .into_iter()
    .filter_map(|(s, on)| on.then_some(s))
}

/// Every path of the played form `file` names (`live.ron` first).
pub fn live_paths(file: &LiveFile) -> Vec<String> {
    let mut out = vec![LIVE_FILE.to_string()];
    out.extend(file.scripts.iter().cloned());
    out.extend(file.sounds.iter().cloned());
    for (c, copy) in file.copies.iter().enumerate() {
        for (k, p) in copy.pictures.iter().enumerate() {
            out.extend(suffixes(p).map(|s| live_picture_file(c, k, s)));
        }
    }
    out
}

/// The bytes picture `p`'s pixels take decoded at art scale `s`.
fn decoded_bytes(p: &LivePictureFile, s: u32) -> u64 {
    let world = u64::from(p.size.0) * u64::from(p.size.1);
    let art = world * u64::from(s) * u64::from(s);
    let mut n = 4 * art;
    n += if p.rock { world } else { 0 };
    n += if p.glow { art } else { 0 };
    n += if p.groups { world } else { 0 };
    n += if p.beam { 2 * world } else { 0 };
    n += if p.glass { 4 * art } else { 0 };
    n
}

fn invalid(path: &str, message: impl Into<String>) -> FormatError {
    FormatError::invalid(LIVE_FILE, path, message)
}

/// What can be checked from `live.ron` alone before any other file is
/// read: the counts, the scripts' and sounds' paths, each picture's size,
/// the files' count and the pictures' decoded bytes at art scale `s`
/// (the level editor's Live switch checks a planned played form so).
pub fn check_live_budget(f: &LiveFile, s: u32) -> Result<(), FormatError> {
    if f.copies.len() > MAX_LIVE_COPIES {
        return Err(invalid("copies", format!("{} live copies: at most {MAX_LIVE_COPIES}", f.copies.len())));
    }
    if f.scripts.len() > MAX_SCRIPTS {
        return Err(invalid("scripts", format!("{} scripts: at most {MAX_SCRIPTS}", f.scripts.len())));
    }
    for (k, p) in f.scripts.iter().enumerate() {
        if *p != live_script_file(k) {
            return Err(invalid(
                &format!("scripts[{k}]"),
                format!("{p:?}: script {k} is {:?}", live_script_file(k)),
            ));
        }
    }
    if f.sounds.len() > MAX_LIVE_SOUNDS {
        return Err(invalid("sounds", format!("{} sounds: at most {MAX_LIVE_SOUNDS}", f.sounds.len())));
    }
    for (k, p) in f.sounds.iter().enumerate() {
        if *p != live_sound_file(k, "ogg") && *p != live_sound_file(k, "wav") {
            return Err(invalid(
                &format!("sounds[{k}]"),
                format!(
                    "{p:?}: sound {k} is {:?} or {:?}",
                    live_sound_file(k, "ogg"),
                    live_sound_file(k, "wav")
                ),
            ));
        }
    }
    let mut files = 1 + f.scripts.len() + f.sounds.len();
    let mut bytes = 0u64;
    for (c, copy) in f.copies.iter().enumerate() {
        let path = |field: &str| format!("copies[{c}].{field}");
        if copy.pictures.len() > MAX_PICTURES {
            return Err(invalid(
                &path("pictures"),
                format!("copy {:?}: {} pictures, at most {MAX_PICTURES}", copy.id, copy.pictures.len()),
            ));
        }
        for (k, p) in copy.pictures.iter().enumerate() {
            let (w, h) = p.size;
            if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
                return Err(invalid(
                    &format!("copies[{c}].pictures[{k}].size"),
                    format!("copy {:?}: {w} × {h}: each side 1 to {MAX_SIDE}", copy.id),
                ));
            }
            files += suffixes(p).count();
            bytes = bytes.saturating_add(decoded_bytes(p, s));
        }
    }
    if files > MAX_LIVE_FILES {
        return Err(invalid(
            "copies",
            format!("{files} files: the played form has at most {MAX_LIVE_FILES}"),
        ));
    }
    // Issue #90: the rock parts the simulation collides with.
    let rock: usize = f
        .copies
        .iter()
        .map(|c| {
            c.states
                .iter()
                .map(|s| s.parts.iter().filter(|p| p.kind == PartKind::Rock).count())
                .max()
                .unwrap_or(0)
        })
        .sum();
    if rock > MAX_LIVE_SOLID_PARTS {
        return Err(invalid(
            "copies",
            format!("{rock} rock parts shown at once: the live copies show at most {MAX_LIVE_SOLID_PARTS}"),
        ));
    }
    if bytes > MAX_LIVE_BYTES {
        return Err(invalid(
            "copies",
            format!("the pictures decode to {bytes} bytes, more than {MAX_LIVE_BYTES}"),
        ));
    }
    Ok(())
}

/// A value list's rules (`(prop, value)` pairs): each prop in range,
/// once, its value within the prop's limits.
fn check_values(
    copy: &LiveCopyFile,
    values: &[(u16, i64)],
    path: &str,
) -> Result<BTreeSet<u16>, FormatError> {
    let mut seen = BTreeSet::new();
    for (i, (p, v)) in values.iter().enumerate() {
        let at = format!("{path}[{i}]");
        let Some(prop) = copy.props.get(usize::from(*p)) else {
            return Err(invalid(
                &at,
                format!("copy {:?}: no value {p} ({} values)", copy.id, copy.props.len()),
            ));
        };
        if !seen.insert(*p) {
            return Err(invalid(&at, format!("copy {:?}: value {p} set twice", copy.id)));
        }
        if !(prop.min..=prop.max).contains(v) {
            return Err(invalid(
                &at,
                format!("copy {:?}: {v} outside value {p}'s {} to {}", copy.id, prop.min, prop.max),
            ));
        }
    }
    Ok(seen)
}

/// One copy's rules (copy `c` of a level of `level` world pixels at art
/// scale `s`, its pictures `pictures` when read; `sounds` and `scripts`
/// the level's counts).
fn check_copy(
    c: usize,
    copy: &LiveCopyFile,
    pictures: Option<&[LivePicture]>,
    (level, s): ((u32, u32), u32),
    (scripts, sounds): (usize, usize),
) -> Result<(), FormatError> {
    let path = |field: &str| format!("copies[{c}].{field}");
    let bad = |field: &str, m: String| invalid(&path(field), format!("copy {:?}: {m}", copy.id));
    if !valid_copy_id(&copy.id) {
        return Err(invalid(&path("id"), format!("{:?} is not a copy's id", copy.id)));
    }
    let n = copy.asset.chars().count();
    if !(1..=MAX_ASSET_NAME).contains(&n) {
        return Err(bad("asset", format!("a name of {n} characters: 1 to {MAX_ASSET_NAME}")));
    }
    let (w, h) = copy.size;
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(bad("size", format!("{w} × {h}: each side 1 to {MAX_SIDE}")));
    }
    let inside = copy.at.0 >= 0
        && copy.at.1 >= 0
        && i64::from(copy.at.0) + i64::from(w) <= i64::from(level.0)
        && i64::from(copy.at.1) + i64::from(h) <= i64::from(level.1);
    if !inside {
        return Err(bad(
            "at",
            format!(
                "its box at {:?} of {w} × {h} reaches outside the level's {} × {}",
                copy.at, level.0, level.1
            ),
        ));
    }
    // The animation table and the groups, which values and pictures name.
    if copy.animations.len() > MAX_ANIMATIONS {
        return Err(bad(
            "animations",
            format!("{} animations: at most {MAX_ANIMATIONS}", copy.animations.len()),
        ));
    }
    for (i, a) in copy.animations.iter().enumerate() {
        compile_animation(a, LIVE_FILE, &path(&format!("animations[{i}]")))?;
    }
    if copy.groups.len() > MAX_GROUPS {
        return Err(bad("groups", format!("{} glow groups: at most {MAX_GROUPS}", copy.groups.len())));
    }
    let look = LookSource { glow_groups: copy.groups.clone(), ..LookSource::default() };
    crate::look::compile_look_source(&look, LIVE_FILE).map_err(|e| match e {
        FormatError::Invalid { message, path: p, .. } => invalid(
            &path(&p.replacen("look.glow_groups", "groups", 1)),
            format!("copy {:?}: {message}", copy.id),
        ),
        e => e,
    })?;
    // Props.
    if copy.props.len() > MAX_LIVE_PROPS {
        return Err(bad("props", format!("{} values: at most {MAX_LIVE_PROPS}", copy.props.len())));
    }
    let layers: BTreeSet<u16> =
        copy.states.iter().flat_map(|s| s.parts.iter().filter_map(|p| p.layer)).collect();
    let (mut names, mut targets) = (BTreeSet::new(), BTreeSet::new());
    for (i, p) in copy.props.iter().enumerate() {
        let field = format!("props[{i}]");
        if let Some(name) = &p.name {
            if !valid_exposed_name(name) {
                return Err(bad(&field, format!("{name:?} is not an exposed name")));
            }
            if !names.insert(name.as_str()) {
                return Err(bad(&field, format!("{name:?}: two values of this name")));
            }
        }
        if !targets.insert(p.drives) {
            return Err(bad(&field, format!("{:?} is driven twice", p.drives)));
        }
        if let Some(l) = p.drives.layer()
            && !layers.contains(&l)
        {
            return Err(bad(&field, format!("{:?}: no part shows layer {l}", p.drives)));
        }
        let (lo, hi) = match p.drives {
            Target::Health => match &copy.health {
                Some(h) => (0, i64::from(h.points)),
                None => return Err(bad(&field, "Health: the copy has no health".into())),
            },
            t => t.bounds(copy.animations.len()),
        };
        if p.min < lo || p.max > hi || p.min > p.max {
            return Err(bad(&field, format!("{:?}: {} to {}, within {lo} to {hi}", p.drives, p.min, p.max)));
        }
        if !(p.min..=p.max).contains(&p.default) {
            return Err(bad(&field, format!("default {} outside {} to {}", p.default, p.min, p.max)));
        }
    }
    // Pictures.
    if let Some(pictures) = pictures
        && pictures.len() != copy.pictures.len()
    {
        return Err(bad(
            "pictures",
            format!("{} pictures read for {} listed", pictures.len(), copy.pictures.len()),
        ));
    }
    for (k, entry) in copy.pictures.iter().enumerate() {
        let field = format!("pictures[{k}]");
        let (pw, ph) = entry.size;
        if pw == 0 || ph == 0 || pw > w || ph > h {
            return Err(bad(&field, format!("{pw} × {ph}: each side 1 to the copy's")));
        }
        let Some(p) = pictures.map(|v| &v[k]) else { continue };
        let file = |suffix: &str| live_picture_file(c, k, suffix);
        let misfit = |suffix: &str, m: String| FormatError::invalid(&file(suffix), "", m);
        if p.entry(entry.size) != *entry {
            return Err(bad(&field, "its files are not the ones listed".into()));
        }
        let (world, art) = (pw as usize * ph as usize, pw as usize * ph as usize * (s * s) as usize);
        if (p.art.width(), p.art.height()) != (pw * s, ph * s) {
            return Err(misfit(
                "",
                format!(
                    "{} × {}: {} × {} (its size × art scale {s})",
                    p.art.width(),
                    p.art.height(),
                    pw * s,
                    ph * s
                ),
            ));
        }
        for (suffix, len, want) in [
            ("-rock", p.rock.as_ref().map(Vec::len), world),
            ("-glow", p.glow.as_ref().map(Vec::len), art),
            ("-groups", p.groups.as_ref().map(Vec::len), world),
            ("-beam", p.beam.as_ref().map(Vec::len), 2 * world),
        ] {
            if let Some(len) = len
                && len != want
            {
                return Err(misfit(suffix, format!("{len} values, the picture needs {want}")));
            }
        }
        if let Some(g) = &p.glass
            && (g.width(), g.height()) != (pw * s, ph * s)
        {
            return Err(misfit("-glass", format!("{} × {}: {} × {}", g.width(), g.height(), pw * s, ph * s)));
        }
        if let Some(v) =
            p.groups.as_ref().and_then(|g| g.iter().find(|v| usize::from(**v) > copy.groups.len()))
        {
            return Err(misfit(
                "-groups",
                format!("group {v}: the copy has {} glow groups", copy.groups.len()),
            ));
        }
        if let Some(b) = &p.beam
            && b.chunks(2).any(|t| t[0] > 180 || t[1] > 90 || (t[0] > 0 && t[1] == 0))
        {
            return Err(misfit("-beam", "a direction byte up to 180 with a spread of 1 to 90".into()));
        }
    }
    // States.
    if copy.states.is_empty() || copy.states.len() > MAX_STATES {
        return Err(bad("states", format!("{} states: 1 to {MAX_STATES}", copy.states.len())));
    }
    let mut state_names = BTreeSet::new();
    for (i, st) in copy.states.iter().enumerate() {
        let field = |f: &str| format!("states[{i}].{f}");
        if !valid_state_name(&st.name) {
            return Err(bad(&field("name"), format!("{:?} is not a state's name", st.name)));
        }
        if !state_names.insert(st.name.to_ascii_lowercase()) {
            return Err(bad(&field("name"), format!("state {:?}: two states of this name", st.name)));
        }
        check_values(copy, &st.values, &path(&field("values")))?;
        if st.parts.len() > MAX_PARTS {
            return Err(bad(&field("parts"), format!("{} parts: at most {MAX_PARTS}", st.parts.len())));
        }
        for (j, part) in st.parts.iter().enumerate() {
            let at = field(&format!("parts[{j}]"));
            let Some(pic) = copy.pictures.get(usize::from(part.picture)) else {
                return Err(bad(
                    &at,
                    format!("no picture {} ({} pictures)", part.picture, copy.pictures.len()),
                ));
            };
            match (part.kind, pic.rock) {
                (PartKind::Rock, false) => {
                    return Err(bad(&at, "a rock part's picture has a rock mask".into()));
                }
                (PartKind::Background, true) => {
                    return Err(bad(&at, "a background part's picture has no rock mask".into()));
                }
                _ => {}
            }
            let fits = part.at.0 >= 0
                && part.at.1 >= 0
                && i64::from(part.at.0) + i64::from(pic.size.0) <= i64::from(w)
                && i64::from(part.at.1) + i64::from(pic.size.1) <= i64::from(h);
            if !fits {
                return Err(bad(&at, format!("its picture at {:?} reaches outside the copy", part.at)));
            }
            if part.glow_strength > 100 {
                return Err(bad(&at, format!("glow strength {}: 0 to 100", part.glow_strength)));
            }
        }
        for (f, sound) in [("enter_sound", st.enter_sound), ("loop_sound", st.loop_sound)] {
            if let Some(k) = sound
                && usize::from(k) >= sounds
            {
                return Err(bad(&field(f), format!("no sound {k} ({sounds} sounds)")));
            }
        }
    }
    if usize::from(copy.start_state) >= copy.states.len() {
        return Err(bad("start_state", format!("no state {}", copy.start_state)));
    }
    check_values(copy, &copy.start_values, &path("start_values"))?;
    for (i, k) in copy.scripts.iter().enumerate() {
        if usize::from(*k) >= scripts {
            return Err(bad(&format!("scripts[{i}]"), format!("no script {k} ({scripts} scripts)")));
        }
    }
    let mut sound_names = BTreeSet::new();
    for (i, (name, k)) in copy.sounds.iter().enumerate() {
        if !valid_sound_name(name) || !sound_names.insert(name.as_str()) {
            return Err(bad(&format!("sounds[{i}]"), format!("{name:?}: a sound's name, once")));
        }
        if usize::from(*k) >= sounds {
            return Err(bad(&format!("sounds[{i}]"), format!("no sound {k} ({sounds} sounds)")));
        }
    }
    if let Some(hp) = &copy.health {
        if !(1..=MAX_HEALTH).contains(&hp.points) {
            return Err(bad("health.points", format!("{}: 1 to {MAX_HEALTH}", hp.points)));
        }
        if usize::from(hp.at_zero) >= copy.states.len() {
            return Err(bad("health.at_zero", format!("no state {}", hp.at_zero)));
        }
    }
    Ok(())
}

/// Whether `bytes` start as a sound file does (`OggS`, or `RIFF`…`WAVE`).
fn sound_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"OggS") || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"))
}

/// Every rule of a level's played form `src` in a level of `size` world
/// pixels at art scale `art_scale` whose look has `level_groups` glow
/// groups: the counts and paths ([`MAX_LIVE_COPIES`], [`MAX_SCRIPTS`],
/// [`MAX_LIVE_SOUNDS`], [`MAX_PICTURES`], [`MAX_LIVE_FILES`],
/// [`MAX_LIVE_BYTES`]); copy ids unique; each copy's box inside the
/// level; every index in range; names unique; every value within its
/// limits; each picture's files of its size; a rock part's picture with a
/// rock mask, a background part's without; parts inside their copy; the
/// level's groups and every copy's at most 255; scripts within
/// `MAX_SCRIPT_BYTES`, sounds within `MAX_ASSET_SOUND_BYTES` and WAV or
/// Ogg. Each error names its path.
pub fn check_live(
    src: &LiveSource,
    size: (u32, u32),
    art_scale: u32,
    level_groups: usize,
) -> Result<(), FormatError> {
    let f = &src.file;
    let s = art_scale.max(1);
    check_live_budget(f, s)?;
    if src.scripts.len() != f.scripts.len() || src.sounds.len() != f.sounds.len() {
        return Err(invalid("scripts", "the scripts and sounds read are not the ones listed"));
    }
    if src.pictures.len() != f.copies.len() {
        return Err(invalid("copies", "the pictures read are not the copies'"));
    }
    if let Some(k) = f.level_script
        && usize::from(k) >= f.scripts.len()
    {
        return Err(invalid("level_script", format!("no script {k} ({} scripts)", f.scripts.len())));
    }
    for (k, t) in src.scripts.iter().enumerate() {
        if t.len() as u64 > MAX_SCRIPT_BYTES {
            return Err(FormatError::Io {
                file: live_script_file(k),
                message: format!("{} bytes, more than {MAX_SCRIPT_BYTES}", t.len()),
            });
        }
    }
    for (k, b) in src.sounds.iter().enumerate() {
        let file = &f.sounds[k];
        if b.len() as u64 > MAX_ASSET_SOUND_BYTES {
            return Err(FormatError::Io {
                file: file.clone(),
                message: format!("{} bytes, more than {MAX_ASSET_SOUND_BYTES}", b.len()),
            });
        }
        if !sound_magic(b) {
            return Err(FormatError::invalid(file, "", "not a WAV or Ogg file"));
        }
    }
    let mut ids = BTreeSet::new();
    let mut groups = level_groups;
    for (c, copy) in f.copies.iter().enumerate() {
        check_copy(c, copy, Some(&src.pictures[c]), (size, s), (f.scripts.len(), f.sounds.len()))?;
        if !ids.insert(copy.id.as_str()) {
            return Err(invalid(
                &format!("copies[{c}].id"),
                format!("copy {:?}: two live copies of this id", copy.id),
            ));
        }
        groups += copy.groups.len();
        if groups > MAX_GROUPS {
            return Err(invalid(
                &format!("copies[{c}].groups"),
                format!(
                    "copy {:?}: the level's glow groups and the copies' are more than {MAX_GROUPS}",
                    copy.id
                ),
            ));
        }
    }
    Ok(())
}

fn io(file: &str, message: String) -> FormatError {
    FormatError::Io { file: file.into(), message }
}

fn read_file(src: &dyn ContentSource, file: &str, max: u64) -> Result<Vec<u8>, FormatError> {
    let path = VPath::new(file).map_err(|e| io(file, e.to_string()))?;
    src.read_max(&path, max).map_err(|e| io(file, e.to_string()))
}

fn write_file(sink: &mut dyn ContentSink, file: &str, bytes: &[u8]) -> Result<(), FormatError> {
    let path = VPath::new(file).map_err(|e| io(file, e.to_string()))?;
    sink.write(&path, bytes).map_err(|e| io(file, e.to_string()))
}

/// A PNG's header size checked against `want` before it is decoded.
fn header_fits(file: &str, bytes: &[u8], want: (u32, u32)) -> Result<(), FormatError> {
    match image_io::png_size(bytes) {
        Some(got) if got == want => Ok(()),
        Some((w, h)) => Err(FormatError::invalid(file, "", format!("{w} × {h}: {} × {}", want.0, want.1))),
        None => Err(io(file, "not a PNG".into())),
    }
}

fn read_rgba(src: &dyn ContentSource, file: &str, want: (u32, u32)) -> Result<RgbaImage, FormatError> {
    let bytes = read_file(src, file, crate::ron_io::MAX_FILE_BYTES)?;
    header_fits(file, &bytes, want)?;
    image_io::decode_rgba_within(&bytes, LEVEL_ART).map_err(|m| io(file, m))
}

fn read_grey(src: &dyn ContentSource, file: &str, want: (u32, u32)) -> Result<Vec<u8>, FormatError> {
    let bytes = read_file(src, file, crate::ron_io::MAX_FILE_BYTES)?;
    header_fits(file, &bytes, want)?;
    let (w, h, v) = image_io::decode_grey_within(&bytes, LEVEL_ART).map_err(|m| io(file, m))?;
    if (w, h) != want {
        return Err(FormatError::invalid(file, "", format!("{w} × {h}: {} × {}", want.0, want.1)));
    }
    Ok(v)
}

fn read_grey_alpha(src: &dyn ContentSource, file: &str, want: (u32, u32)) -> Result<Vec<u8>, FormatError> {
    let bytes = read_file(src, file, crate::ron_io::MAX_FILE_BYTES)?;
    header_fits(file, &bytes, want)?;
    let (w, h, v) = image_io::decode_grey_alpha_within(&bytes, LEVEL_ART).map_err(|m| io(file, m))?;
    if (w, h) != want {
        return Err(FormatError::invalid(file, "", format!("{w} × {h}: {} × {}", want.0, want.1)));
    }
    Ok(v)
}

/// Reads a level's played form, `None` without `live.ron`. `live.ron` is
/// checked for its counts, paths, sizes, files and bytes before any
/// other file is read; each picture's header before it is decoded; then
/// [`check_live`] (`level_groups`: the level's look's glow groups).
pub fn read_live_files(
    src: &dyn ContentSource,
    size: (u32, u32),
    art_scale: u32,
    level_groups: usize,
) -> Result<Option<LiveSource>, FormatError> {
    let path = VPath::new(LIVE_FILE).map_err(|e| io(LIVE_FILE, e.to_string()))?;
    if !src.exists(&path) {
        return Ok(None);
    }
    let file: LiveFile = from_canonical(LIVE_FILE, &read_text(src, LIVE_FILE)?)?;
    let s = art_scale.max(1);
    check_live_budget(&file, s)?;
    let mut scripts = Vec::with_capacity(file.scripts.len());
    for p in &file.scripts {
        let bytes = read_file(src, p, MAX_SCRIPT_BYTES)?;
        scripts.push(String::from_utf8(bytes).map_err(|_| io(p, "not UTF-8".into()))?);
    }
    let mut sounds = Vec::with_capacity(file.sounds.len());
    for p in &file.sounds {
        sounds.push(read_file(src, p, MAX_ASSET_SOUND_BYTES)?);
    }
    let mut pictures = Vec::with_capacity(file.copies.len());
    for (c, copy) in file.copies.iter().enumerate() {
        let mut out = Vec::with_capacity(copy.pictures.len());
        for (k, e) in copy.pictures.iter().enumerate() {
            let world = e.size;
            let art = (world.0 * s, world.1 * s);
            let f = |suffix: &str| live_picture_file(c, k, suffix);
            let glass = if e.glass { Some(read_rgba(src, &f("-glass"), art)?) } else { None };
            out.push(LivePicture {
                art: read_rgba(src, &f(""), art)?,
                rock: if e.rock { Some(read_grey(src, &f("-rock"), world)?) } else { None },
                glow: if e.glow { Some(read_grey(src, &f("-glow"), art)?) } else { None },
                groups: if e.groups { Some(read_grey(src, &f("-groups"), world)?) } else { None },
                beam: if e.beam { Some(read_grey_alpha(src, &f("-beam"), world)?) } else { None },
                glass,
            });
        }
        pictures.push(out);
    }
    let live = LiveSource { file, pictures, scripts, sounds };
    check_live(&live, size, art_scale, level_groups)?;
    Ok(Some(live))
}

/// Writes a played form (checked first by the level's writer):
/// `live.ron`, the scripts, the sounds and each picture's files.
pub fn write_live_files(sink: &mut dyn ContentSink, live: &LiveSource) -> Result<(), FormatError> {
    let f = &live.file;
    write_text(sink, LIVE_FILE, &to_canonical(LIVE_FILE, f)?)?;
    for (p, t) in f.scripts.iter().zip(&live.scripts) {
        write_file(sink, p, t.as_bytes())?;
    }
    for (p, b) in f.sounds.iter().zip(&live.sounds) {
        write_file(sink, p, b)?;
    }
    for (c, (copy, pictures)) in f.copies.iter().zip(&live.pictures).enumerate() {
        for (k, (e, p)) in copy.pictures.iter().zip(pictures).enumerate() {
            let (w, h) = e.size;
            let (aw, ah) = (p.art.width(), p.art.height());
            let f = |suffix: &str| live_picture_file(c, k, suffix);
            write_file(sink, &f(""), &image_io::encode_rgba(&p.art))?;
            if let Some(v) = &p.rock {
                write_file(sink, &f("-rock"), &image_io::encode_grey(w, h, v))?;
            }
            if let Some(v) = &p.glow {
                write_file(sink, &f("-glow"), &image_io::encode_grey(aw, ah, v))?;
            }
            if let Some(v) = &p.groups {
                write_file(sink, &f("-groups"), &image_io::encode_grey(w, h, v))?;
            }
            if let Some(v) = &p.beam {
                write_file(sink, &f("-beam"), &image_io::encode_grey_alpha(w, h, v))?;
            }
            if let Some(g) = &p.glass {
                write_file(sink, &f("-glass"), &image_io::encode_rgba(g))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::{GlowColourSource, ReachSource};
    use olr_data::image::Rgba;
    use olr_vfs::MemFs;

    const LEVEL: (u32, u32) = (40, 30);

    fn group(id: &str) -> GlowGroupSource {
        GlowGroupSource {
            id: id.into(),
            colour: GlowColourSource::Tint(crate::Colour(Rgba::rgb(255, 200, 100))),
            intensity: 300,
            reach: ReachSource::Pixels(40),
            animation: AnimationSource::Steady,
            phase_ms: None,
        }
    }

    fn picture(size: (u32, u32), s: u32, rock: bool) -> LivePicture {
        let mut art = RgbaImage::new(size.0 * s, size.1 * s);
        art.set(0, 0, Rgba::rgb(9, 8, 7));
        let world = (size.0 * size.1) as usize;
        LivePicture {
            art,
            rock: rock.then(|| vec![255; world]),
            glow: (!rock).then(|| vec![128; world * (s * s) as usize]),
            groups: (!rock).then(|| vec![1; world]),
            beam: (!rock).then(|| [91u8, 30].repeat(world)),
            glass: None,
        }
    }

    /// A lamp of two states over a rock base, a level script and a sound.
    fn sample(s: u32) -> LiveSource {
        let pictures = vec![picture((4, 2), s, true), picture((4, 3), s, false), picture((4, 3), s, false)];
        let mut pictures = pictures;
        pictures[2].art.set(1, 1, Rgba::rgb(1, 2, 3));
        let part = |layer, kind, picture, at, glow_strength| LivePartFile {
            layer,
            kind,
            picture,
            at,
            glow_strength,
        };
        let copy = LiveCopyFile {
            id: "lamp".into(),
            asset: "Lamp".into(),
            at: (10, 5),
            size: (4, 5),
            props: vec![
                LivePropFile {
                    name: Some("glow".into()),
                    drives: Target::GlowStrength(1),
                    min: 0,
                    max: 100,
                    default: 60,
                },
                LivePropFile {
                    name: None,
                    drives: Target::GlowColour(1),
                    min: -1,
                    max: 0xff_ffff,
                    default: -1,
                },
                LivePropFile {
                    name: Some("head_y".into()),
                    drives: Target::OffsetY(0),
                    min: -4,
                    max: 6,
                    default: 0,
                },
                LivePropFile { name: None, drives: Target::Health, min: 0, max: 40, default: 40 },
            ],
            pictures: pictures.iter().zip([(4, 2), (4, 3), (4, 3)]).map(|(p, z)| p.entry(z)).collect(),
            states: vec![
                LiveStateFile {
                    name: "On".into(),
                    values: vec![(1, -1)],
                    parts: vec![
                        part(Some(0), PartKind::Rock, 0, (0, 3), 0),
                        part(Some(1), PartKind::Background, 1, (0, 0), 60),
                    ],
                    enter_sound: None,
                    loop_sound: Some(0),
                },
                LiveStateFile {
                    name: "Off".into(),
                    values: vec![(1, 0x102030)],
                    parts: vec![
                        part(Some(0), PartKind::Rock, 0, (0, 3), 0),
                        part(Some(1), PartKind::Background, 2, (0, 0), 60),
                    ],
                    enter_sound: Some(0),
                    loop_sound: None,
                },
            ],
            start_state: 1,
            start_values: vec![(0, 80)],
            animations: vec![AnimationSource::Steady],
            groups: vec![group("lamps")],
            scripts: vec![1],
            sounds: vec![("tick.wav".into(), 0)],
            health: Some(LiveHealthFile { points: 40, at_zero: 1 }),
        };
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend([0; 8]);
        LiveSource {
            file: LiveFile {
                copies: vec![copy],
                scripts: vec![live_script_file(0), live_script_file(1)],
                level_script: Some(0),
                sounds: vec![live_sound_file(0, "wav")],
            },
            pictures: vec![pictures],
            scripts: vec!["// the level\n".into(), "fn tick() {}\n".into()],
            sounds: vec![wav],
        }
    }

    fn refused(src: &LiveSource) -> String {
        check_live(src, LEVEL, 2, 0).unwrap_err().to_string()
    }

    #[test]
    fn a_live_file_round_trips() {
        let src = sample(2);
        check_live(&src, LEVEL, 2, 0).unwrap();
        let mut fs = MemFs::new();
        write_live_files(&mut fs, &src).unwrap();
        let mut written: Vec<String> = fs.files().map(|(p, _)| p.as_str().to_string()).collect();
        let mut paths = live_paths(&src.file);
        written.sort();
        paths.sort();
        assert_eq!(written, paths);
        assert!(paths.iter().all(|p| p.len() <= MAX_LIVE_PATH), "{paths:?}");
        assert!(
            written.contains(&"live/0/1-beam.png".to_string()) && written.contains(&"live/1.rhai".into())
        );
        assert_eq!(read_live_files(&fs, LEVEL, 2, 0).unwrap(), Some(src.clone()));
        // Canonical text: defaults left out, read back the same.
        let text = to_canonical(LIVE_FILE, &src.file).unwrap();
        assert!(!text.contains("glass: false") && !text.contains("glow_strength: 0"), "{text}");
        assert_eq!(from_canonical::<LiveFile>(LIVE_FILE, &text).unwrap(), src.file);
        // A level without `live.ron` has none.
        assert_eq!(read_live_files(&MemFs::new(), LEVEL, 2, 0).unwrap(), None);
        // The longest path there can be.
        assert_eq!(live_picture_file(MAX_LIVE_COPIES - 1, MAX_PICTURES - 1, "-groups").len(), MAX_LIVE_PATH);
        assert_eq!(glow_value(100, 77), 77);
        assert_eq!(glow_value(50, 255), 128);
    }

    #[test]
    fn live_files_refuse_what_does_not_check() {
        let with = |edit: &dyn Fn(&mut LiveSource)| {
            let mut s = sample(2);
            edit(&mut s);
            refused(&s)
        };
        let has = |e: String, parts: &[&str]| {
            for p in parts {
                assert!(e.contains(p), "{p:?} not in {e}");
            }
        };
        // An index out of range.
        has(with(&|s| s.file.copies[0].states[0].parts[0].picture = 3), &["parts[0]", "no picture 3"]);
        has(with(&|s| s.file.copies[0].states[0].values[0].0 = 9), &["no value 9"]);
        has(with(&|s| s.file.copies[0].start_state = 2), &["start_state", "no state 2"]);
        has(with(&|s| s.file.copies[0].states[1].enter_sound = Some(1)), &["enter_sound", "no sound 1"]);
        has(with(&|s| s.file.copies[0].scripts = vec![2]), &["scripts[0]", "no script 2"]);
        has(with(&|s| s.file.level_script = Some(5)), &["level_script"]);
        has(with(&|s| s.file.copies[0].props[1].drives = Target::GlowColour(7)), &["no part shows layer 7"]);
        has(with(&|s| s.file.copies[0].health = None), &["Health", "no health"]);
        // A duplicate name.
        has(with(&|s| s.file.copies[0].states[1].name = "on".into()), &["states[1].name", "two states"]);
        has(with(&|s| s.file.copies[0].props[2].name = Some("glow".into())), &["props[2]", "two values"]);
        has(
            with(&|s| {
                let c = s.file.copies[0].clone();
                s.file.copies.push(c);
                s.pictures.push(s.pictures[0].clone());
            }),
            &["copies[1].id", "two live copies"],
        );
        // A value outside its limits.
        has(with(&|s| s.file.copies[0].start_values = vec![(0, 101)]), &["start_values[0]", "101 outside"]);
        has(with(&|s| s.file.copies[0].states[1].values[0].1 = 0x1_000_000), &["outside"]);
        has(with(&|s| s.file.copies[0].props[0].max = 1000), &["props[0]", "within 0 to 100"]);
        has(with(&|s| s.file.copies[0].props[2].default = 7), &["default 7"]);
        // A misfit picture.
        has(with(&|s| s.pictures[0][0].art = RgbaImage::new(8, 5)), &["live/0/0.png", "8 × 5"]);
        has(with(&|s| s.pictures[0][1].glow = Some(vec![1; 3])), &["live/0/1-glow.png", "3 values"]);
        has(with(&|s| s.pictures[0][1].groups = Some(vec![2; 12])), &["live/0/1-groups.png", "group 2"]);
        has(with(&|s| s.file.copies[0].states[0].parts[1].at = (1, 0)), &["reaches outside the copy"]);
        // A rock part without its mask, a background part with one.
        has(
            with(&|s| {
                s.pictures[0][0].rock = None;
                s.file.copies[0].pictures[0].rock = false;
            }),
            &["a rock part's picture has a rock mask"],
        );
        has(
            with(&|s| s.file.copies[0].states[0].parts[1].picture = 0),
            &["a background part's picture has no rock mask"],
        );
        // Outside the level.
        has(with(&|s| s.file.copies[0].at = (37, 0)), &["copies[0].at", "outside the level"]);
        // Too many copies, scripts past 129, glow groups past 255, files past the cap.
        has(
            with(&|s| {
                let c = s.file.copies[0].clone();
                s.file.copies = vec![c; MAX_LIVE_COPIES + 1];
            }),
            &["live copies: at most 64"],
        );
        // Issue #90: more rock parts than the simulation indexes.
        has(
            with(&|s| {
                let mut c = s.file.copies[0].clone();
                let rock = c.states[0].parts.iter().find(|p| p.kind == PartKind::Rock).copied().unwrap();
                c.states[0].parts = vec![rock; MAX_PARTS];
                s.file.copies = vec![c; 3];
            }),
            &["96 rock parts shown at once", "at most 64"],
        );
        has(
            with(&|s| {
                s.file.scripts = (0..=MAX_SCRIPTS).map(live_script_file).collect();
                s.scripts = vec![String::new(); MAX_SCRIPTS + 1];
            }),
            &["130 scripts: at most 129"],
        );
        has(with(&|s| s.file.scripts[1] = "live/x.rhai".into()), &["scripts[1]"]);
        let e = check_live(&sample(2), LEVEL, 2, 255).unwrap_err().to_string();
        has(e, &["copies[0].groups", "more than 255"]);
        has(
            with(&|s| {
                let p = s.file.copies[0].pictures[1];
                s.file.copies[0].pictures = vec![p; MAX_PICTURES];
                let c = s.file.copies[0].clone();
                s.file.copies = vec![c; 6];
            }),
            &["files: the played form has at most 2048"],
        );
        has(
            with(&|s| s.file.copies[0].pictures = vec![s.file.copies[0].pictures[0]; MAX_PICTURES + 1]),
            &["129 pictures"],
        );
        // Sounds and scripts are bounded and of their kind.
        has(with(&|s| s.sounds[0] = b"not a sound".to_vec()), &["live/0.wav", "not a WAV or Ogg"]);
        // A hostile live.ron never panics, and refuses before reading pictures.
        let mut fs = MemFs::new();
        write_live_files(&mut fs, &sample(2)).unwrap();
        fs.write(&VPath::new(LIVE_FILE).unwrap(), b"(copies: [(id: \"x\"").unwrap();
        assert!(read_live_files(&fs, LEVEL, 2, 0).is_err());
        let mut big = sample(2);
        big.file.copies[0].pictures[0].size = (4096, 4096);
        let mut fs = MemFs::new();
        write_text(&mut fs, LIVE_FILE, &to_canonical(LIVE_FILE, &big.file).unwrap()).unwrap();
        let e = read_live_files(&fs, LEVEL, 2, 0).unwrap_err().to_string();
        assert!(e.contains("decode to"), "{e}");
    }
}
