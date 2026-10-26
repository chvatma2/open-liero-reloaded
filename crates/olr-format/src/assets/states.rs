//! An asset's states, exposed properties, sounds and health (issue #71;
//! the design `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`):
//! the types `asset.ron` holds them in and their rules.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::AssetFile;
use crate::Colour;
use crate::FormatError;
use crate::levels::{Element, ElementKind, ElementMaterial};
use crate::look::{AnimationSource, GlowColourSource, GlowGroupSource, ReachSource};

/// The most states an asset has.
pub const MAX_STATES: usize = 32;
/// The longest state name, in characters.
pub const MAX_STATE_NAME: usize = 32;
/// The most exposed properties an asset has.
pub const MAX_EXPOSED: usize = 64;
/// The longest exposed name, in bytes.
pub const MAX_EXPOSED_NAME: usize = 32;
/// The most sounds an asset holds.
pub const MAX_ASSET_SOUNDS: usize = 16;
/// The longest sound file name's stem (before `.wav` or `.ogg`), in bytes.
pub const MAX_SOUND_STEM: usize = 32;
/// The largest sound file, in bytes: `olr_pack::sounds::MAX_SOUND_BYTES`
/// (a test in `olr-edit` holds them equal).
pub const MAX_ASSET_SOUND_BYTES: u64 = 2 << 20;
/// The largest default script, in bytes.
pub const MAX_SCRIPT_BYTES: u64 = 64 << 10;
/// The most health points an asset has.
pub const MAX_HEALTH: u32 = 10_000;
/// The farthest a position (`X`, `Y`, `OffsetX`, `OffsetY`) moves either
/// way, asset pixels.
pub const MAX_POSITION: i32 = 4096;
/// The name of the state an asset without `states` has.
pub const NORMAL_STATE: &str = "Normal";

/// A named look of the asset (the design's call 3): its own pictures of
/// some drawn layers, its own values, its sounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateSource {
    /// 1 to `MAX_STATE_NAME` (32) characters: letters, digits, spaces, `-`
    /// and `_`, no space first or last; unique in the asset ignoring ASCII
    /// case. Scripts and the editor name the state by it.
    pub name: String,
    /// The ids of the drawn layers whose picture this state paints itself,
    /// each at most once; their pictures are stacked below the starting
    /// state's in the layer's own files (see the files). Every other layer
    /// shows the starting state's pixels. Not an image, raster or distant
    /// layer; the starting state owns none. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub own: Vec<String>,
    /// What this state sets, at most one value per property (and per glow
    /// group for the group properties). A state is a complete look:
    /// entering it sets every property any state sets, to this state's
    /// value or else to the property's default. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<StateValue>,
    /// A name in `sounds`, looped while the state lasts. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_sound: Option<String>,
    /// A name in `sounds`, played once when the state is entered. Default
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enter_sound: Option<String>,
}

impl StateSource {
    /// A state of `name` that owns, sets and plays nothing.
    pub fn new(name: &str) -> StateSource {
        StateSource {
            name: name.into(),
            own: Vec::new(),
            values: Vec::new(),
            loop_sound: None,
            enter_sound: None,
        }
    }
}

/// The states of an asset that has no `states`: one, `Normal`, owning,
/// setting and playing nothing. Written `[(name: "Normal")]`.
pub fn normal_states() -> Vec<StateSource> {
    vec![StateSource::new(NORMAL_STATE)]
}

/// Whether `states` is [`normal_states`] (left out when written).
pub fn is_normal_states(states: &[StateSource]) -> bool {
    states == normal_states().as_slice()
}

/// A value a state sets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateValue {
    /// What it sets.
    pub property: PropertySource,
    /// Of the property's type, within the exposed entry's `min` to `max`
    /// when the property is exposed, else within the property's range.
    pub value: ValueSource,
}

/// A settable value given a name scripts use (the design's call 5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposedSource {
    /// `[a-z_][a-z0-9_]*`, 1 to `MAX_EXPOSED_NAME` (32) bytes, unique in
    /// the asset; scripts and a placed copy's values name it (`claw_x`).
    pub name: String,
    /// What it sets; at most one exposed entry per property, and per glow
    /// group for the group properties.
    pub property: PropertySource,
    /// The property's default, used wherever no state value and no copy's
    /// value applies: of its type, within `min` to `max`. A `GlowColour`
    /// on a group whose colour is `Own` needs one; an exposed `Health`'s is
    /// `health.points`. Default none: the layer's own setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<ValueSource>,
    /// The least value, an `Int` property only, within its range; required
    /// for `X`, `Y`, `OffsetX` and `OffsetY`. Default none: the range's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i32>,
    /// The greatest value, an `Int` property only, within its range, at
    /// least `min`; required for `X`, `Y`, `OffsetX` and `OffsetY`. Default
    /// none: the range's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i32>,
}

/// What a value sets. A layer property names an element id of the asset:
/// an image or drawn layer, never a raster or distant one. Positions are
/// asset pixels, scaled with a stretched copy.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PropertySource {
    /// `Int`, -4096 to 4096: the asset's offset from where the copy is
    /// placed, asset pixels.
    X,
    /// `Int`, -4096 to 4096: as `X`, down.
    Y,
    /// `Bool`: the asset is drawn and solid.
    Visible,
    /// `Bool`: `true` mirrors the asset left to right within its box.
    Facing,
    /// `Int`, 0 to `health.points`: an asset with `health` only.
    Health,
    /// `Int`, -4096 to 4096: a Rock or Background layer's offset inside the
    /// asset, asset pixels (a mixed drawn layer counts).
    OffsetX(String),
    /// `Int`, -4096 to 4096: as `OffsetX`, down.
    OffsetY(String),
    /// `Bool`: a Rock or Background layer shown; hidden rock is not solid
    /// (a door).
    LayerVisible(String),
    /// `Int`, percent 0 to 100: a Background layer's opacity (a mixed
    /// layer's background pixels).
    Opacity(String),
    /// `Int`, percent 0 to 100: a glowing Rock or Background layer's glow
    /// strength, the painted mask kept (each pixel mask × value; 0 dark).
    GlowStrength(String),
    /// `Colour`, opaque: a glowing layer's group's colour, `Tint(colour)`
    /// (`Filtered(colour)` for a `Filtered` group).
    GlowColour(String),
    /// `Int`, percent 0 to 1000: a glowing layer's group's intensity.
    LightIntensity(String),
    /// `Int`, 1 to 4096 world pixels: a glowing layer's group's reach, a
    /// group whose reach is `Pixels`.
    LightReach(String),
    /// `Animation`, as a glow group's: a glowing layer's group's
    /// animation.
    GlowAnimation(String),
    /// `Int`, degrees 0 to 359: a layer's beam's direction (its glow has a
    /// `beam`).
    BeamDirection(String),
    /// `Int`, degrees 1 to 90: a layer's beam's half-angle.
    BeamSpread(String),
    /// `Int`, percent 1 to 99: a glass layer's opacity.
    GlassOpacity(String),
    /// `Int`, percent 0 to 100: a glass layer's tint.
    GlassTint(String),
}

impl PropertySource {
    /// The layer it names; none for the asset's own properties.
    pub fn layer(&self) -> Option<&str> {
        use PropertySource as P;
        match self {
            P::X | P::Y | P::Visible | P::Facing | P::Health => None,
            P::OffsetX(id)
            | P::OffsetY(id)
            | P::LayerVisible(id)
            | P::Opacity(id)
            | P::GlowStrength(id)
            | P::GlowColour(id)
            | P::LightIntensity(id)
            | P::LightReach(id)
            | P::GlowAnimation(id)
            | P::BeamDirection(id)
            | P::BeamSpread(id)
            | P::GlassOpacity(id)
            | P::GlassTint(id) => Some(id),
        }
    }

    /// Whether it acts on the layer's glow group (every layer sharing the
    /// group changes with it).
    pub fn of_group(&self) -> bool {
        use PropertySource as P;
        matches!(self, P::GlowColour(_) | P::LightIntensity(_) | P::LightReach(_) | P::GlowAnimation(_))
    }

    /// Whether it is a position (`min` and `max` required when exposed).
    pub fn is_position(&self) -> bool {
        use PropertySource as P;
        matches!(self, P::X | P::Y | P::OffsetX(_) | P::OffsetY(_))
    }

    /// The same property of layer `id` (the asset's own unchanged).
    pub fn on(&self, id: &str) -> PropertySource {
        use PropertySource as P;
        let id = id.to_string();
        match self {
            P::X | P::Y | P::Visible | P::Facing | P::Health => self.clone(),
            P::OffsetX(_) => P::OffsetX(id),
            P::OffsetY(_) => P::OffsetY(id),
            P::LayerVisible(_) => P::LayerVisible(id),
            P::Opacity(_) => P::Opacity(id),
            P::GlowStrength(_) => P::GlowStrength(id),
            P::GlowColour(_) => P::GlowColour(id),
            P::LightIntensity(_) => P::LightIntensity(id),
            P::LightReach(_) => P::LightReach(id),
            P::GlowAnimation(_) => P::GlowAnimation(id),
            P::BeamDirection(_) => P::BeamDirection(id),
            P::BeamSpread(_) => P::BeamSpread(id),
            P::GlassOpacity(_) => P::GlassOpacity(id),
            P::GlassTint(_) => P::GlassTint(id),
        }
    }
}

/// A property's value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueSource {
    /// A whole number.
    Int(i32),
    /// Yes or no.
    Bool(bool),
    /// An opaque colour, `Colour("#ff2020")`.
    Colour(Colour),
    /// A glow group's animation.
    Animation(AnimationSource),
}

/// The type of a property's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Int,
    Bool,
    Colour,
    Animation,
}

impl ValueSource {
    /// Its type.
    pub fn kind(&self) -> ValueKind {
        match self {
            ValueSource::Int(_) => ValueKind::Int,
            ValueSource::Bool(_) => ValueKind::Bool,
            ValueSource::Colour(_) => ValueKind::Colour,
            ValueSource::Animation(_) => ValueKind::Animation,
        }
    }
}

/// A property's value type and, for an `Int`, its range (inclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyType {
    pub kind: ValueKind,
    pub range: Option<(i32, i32)>,
}

/// The asset can be destroyed (the design's call 21).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthSource {
    /// 1 to `MAX_HEALTH` (10000), in the units of worm damage.
    pub points: u32,
    /// The name of the state entered when the health reaches 0.
    pub at_zero: String,
}

/// Whether element `e` is a Rock or Background image or drawn layer (a
/// mixed drawn layer counts), not distant: what a layer property needs.
fn drivable(e: &Element) -> bool {
    if e.is_distant() {
        return false;
    }
    match &e.kind {
        ElementKind::Drawn { mixed: true, .. } => true,
        ElementKind::Image { material, .. } | ElementKind::Drawn { material, .. } => {
            matches!(material, ElementMaterial::Rock | ElementMaterial::Background)
        }
        ElementKind::Raster => false,
    }
}

/// Element `id`'s glow group in `file`.
fn group_of<'a>(file: &'a AssetFile, e: &Element) -> Option<&'a GlowGroupSource> {
    let g = e.glow.as_ref()?;
    file.glow_groups.iter().find(|x| x.id == g.group)
}

/// What property `p` of `file` is: its type and range, or why it cannot
/// be set there (the table on the asset page).
pub fn property_type(file: &AssetFile, p: &PropertySource) -> Result<PropertyType, String> {
    use PropertySource as P;
    let int = |lo: i32, hi: i32| Ok(PropertyType { kind: ValueKind::Int, range: Some((lo, hi)) });
    let of = |kind: ValueKind| Ok(PropertyType { kind, range: None });
    let Some(id) = p.layer() else {
        return match p {
            P::X | P::Y => int(-MAX_POSITION, MAX_POSITION),
            P::Visible | P::Facing => of(ValueKind::Bool),
            _ => match &file.health {
                Some(h) => int(0, i32::try_from(h.points).unwrap_or(i32::MAX)),
                None => Err("the asset has no health".into()),
            },
        };
    };
    let e = file.elements.iter().find(|e| e.id == id).ok_or_else(|| format!("no layer {id:?}"))?;
    if matches!(e.kind, ElementKind::Raster) {
        return Err(format!("{id:?} is a raster: no property of it can be set"));
    }
    if e.is_distant() {
        return Err(format!("{id:?} is a distant layer: no property of it can be set"));
    }
    if !drivable(e) {
        return Err(format!("{id:?} is a dirt layer: no property of it can be set"));
    }
    let glowing = || e.glow.is_some().then_some(()).ok_or_else(|| format!("{id:?} does not glow"));
    let beam = || {
        e.glow.as_ref().and_then(|g| g.beam).map(|_| ()).ok_or_else(|| format!("{id:?}'s glow has no beam"))
    };
    match p {
        P::OffsetX(_) | P::OffsetY(_) => int(-MAX_POSITION, MAX_POSITION),
        P::LayerVisible(_) => of(ValueKind::Bool),
        P::Opacity(_) => {
            let background = matches!(
                e.kind,
                ElementKind::Drawn { mixed: true, .. }
                    | ElementKind::Image { material: ElementMaterial::Background, .. }
                    | ElementKind::Drawn { material: ElementMaterial::Background, .. }
            );
            if background { int(0, 100) } else { Err(format!("{id:?} is not a background layer")) }
        }
        P::GlowStrength(_) => glowing().and_then(|()| int(0, 100)),
        P::GlowColour(_) => glowing().and_then(|()| of(ValueKind::Colour)),
        P::LightIntensity(_) => glowing().and_then(|()| int(0, i32::from(crate::look::MAX_INTENSITY))),
        P::LightReach(_) => {
            glowing()?;
            match group_of(file, e).map(|g| &g.reach) {
                Some(ReachSource::Pixels(_)) => int(1, i32::from(crate::look::MAX_REACH)),
                _ => Err(format!("{id:?}'s glow group's reach is not Pixels")),
            }
        }
        P::GlowAnimation(_) => glowing().and_then(|()| of(ValueKind::Animation)),
        P::BeamDirection(_) => beam().and_then(|()| int(0, 359)),
        P::BeamSpread(_) => beam().and_then(|()| int(1, 90)),
        P::GlassOpacity(_) | P::GlassTint(_) if e.glass.is_none() => Err(format!("{id:?} is not glass")),
        P::GlassOpacity(_) => int(1, 99),
        P::GlassTint(_) => int(0, 100),
        // Answered above, before a layer is looked up.
        P::X | P::Y | P::Visible | P::Facing | P::Health => Err("not a layer property".into()),
    }
}

/// What property `p` is in `file` before anything sets it: the layer's
/// (or the asset's) own setting; none for an `Own` group's colour (it has
/// no one colour) and a property `file` cannot set.
pub fn own_value(file: &AssetFile, p: &PropertySource) -> Option<ValueSource> {
    use PropertySource as P;
    use ValueSource as V;
    property_type(file, p).ok()?;
    let e = p.layer().and_then(|id| file.elements.iter().find(|e| e.id == id));
    let glow = e.and_then(|e| e.glow.as_ref());
    let group = e.and_then(|e| group_of(file, e));
    Some(match p {
        P::X | P::Y | P::OffsetX(_) | P::OffsetY(_) => V::Int(0),
        P::Visible => V::Bool(true),
        P::Facing => V::Bool(false),
        P::Health => V::Int(i32::try_from(file.health.as_ref()?.points).ok()?),
        P::LayerVisible(_) => V::Bool(e?.visible),
        P::Opacity(_) => V::Int(100),
        P::GlowStrength(_) => V::Int(i32::from(glow?.strength)),
        P::GlowColour(_) => match &group?.colour {
            GlowColourSource::Own => return None,
            GlowColourSource::Tint(c) | GlowColourSource::Filtered(c) => V::Colour(*c),
        },
        P::LightIntensity(_) => V::Int(i32::from(group?.intensity)),
        P::LightReach(_) => match group?.reach {
            ReachSource::Pixels(n) => V::Int(i32::from(n)),
            ReachSource::Unlimited => return None,
        },
        P::GlowAnimation(_) => V::Animation(group?.animation.clone()),
        P::BeamDirection(_) => V::Int(i32::from(glow?.beam?.direction)),
        P::BeamSpread(_) => V::Int(i32::from(glow?.beam?.spread)),
        P::GlassOpacity(_) => V::Int(i32::from(e?.glass?.opacity)),
        P::GlassTint(_) => V::Int(i32::from(e?.glass?.tint)),
    })
}

/// What two settings of one value share: a group property's kind and
/// group, else the property itself. Two exposed entries, or two values of
/// one state, with one key set one value.
pub fn property_key(file: &AssetFile, p: &PropertySource) -> String {
    let group = p
        .layer()
        .filter(|_| p.of_group())
        .and_then(|id| file.elements.iter().find(|e| e.id == id))
        .and_then(|e| e.glow.as_ref());
    match group {
        Some(g) => format!("{:?} of group {:?}", p.on(""), g.group),
        None => format!("{p:?}"),
    }
}

/// Whether `v` is a value of type `t` within `range` (an `Int`'s; else the
/// type's own rules): why not, if not.
pub fn check_value(
    t: PropertyType,
    range: Option<(i32, i32)>,
    v: &ValueSource,
    file: &str,
    path: &str,
) -> Result<(), FormatError> {
    let bad = |m: String| FormatError::invalid(file, path, m);
    if v.kind() != t.kind {
        return Err(bad(format!("{v:?}: the property takes {:?} values", t.kind)));
    }
    match v {
        ValueSource::Int(n) => {
            if let Some((lo, hi)) = range.or(t.range)
                && !(lo..=hi).contains(n)
            {
                return Err(bad(format!("{n}: {lo} to {hi}")));
            }
        }
        ValueSource::Colour(c) if c.0.a != 255 => {
            return Err(bad(format!("{}: a glow colour is opaque", c.to_hex())));
        }
        ValueSource::Animation(a) => {
            crate::look::compile_animation(a, file, path)?;
        }
        _ => {}
    }
    Ok(())
}

/// Whether `name` is a state's name: 1 to `MAX_STATE_NAME` characters of
/// letters, digits, spaces, `-` and `_`, no space first or last.
pub fn valid_state_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=MAX_STATE_NAME).contains(&n)
        && name.trim() == name
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
}

/// Whether `name` is an exposed name: `[a-z_][a-z0-9_]*`, 1 to
/// `MAX_EXPOSED_NAME` bytes.
pub fn valid_exposed_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=MAX_EXPOSED_NAME).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// Whether `name` is a sound's file name: `[a-z0-9_-]{1,32}` then `.wav`
/// or `.ogg`.
pub fn valid_sound_name(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    (1..=MAX_SOUND_STEM).contains(&stem.len())
        && matches!(ext, "wav" | "ogg")
        && stem.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
}

/// The index of the starting state: `start`'s, else the first.
pub fn start_index(file: &AssetFile) -> Option<usize> {
    match &file.start {
        None => (!file.states.is_empty()).then_some(0),
        Some(s) => file.states.iter().position(|x| x.name == *s),
    }
}

/// How many states own each layer (by id): the pictures its files stack
/// below the starting state's.
pub fn owners(file: &AssetFile) -> BTreeMap<&str, u32> {
    let mut out: BTreeMap<&str, u32> = BTreeMap::new();
    for s in &file.states {
        for id in &s.own {
            *out.entry(id.as_str()).or_default() += 1;
        }
    }
    out
}

/// The counts and names that make paths or bound work, checked before any
/// file is read: at most `MAX_STATES` states, `MAX_EXPOSED` exposed
/// entries, `MAX_ASSET_SOUNDS` sounds, each sound's name.
pub(super) fn check_counts(file: &AssetFile) -> Result<(), FormatError> {
    let invalid = |path: &str, m: String| FormatError::invalid(super::ASSET_FILE, path, m);
    let n = file.states.len();
    if n == 0 || n > MAX_STATES {
        return Err(invalid("states", format!("{n} states: an asset has 1 to {MAX_STATES}")));
    }
    if file.exposed.len() > MAX_EXPOSED {
        return Err(invalid("exposed", format!("{} entries: at most {MAX_EXPOSED}", file.exposed.len())));
    }
    if file.sounds.len() > MAX_ASSET_SOUNDS {
        return Err(invalid("sounds", format!("{} sounds: at most {MAX_ASSET_SOUNDS}", file.sounds.len())));
    }
    let mut seen = BTreeSet::new();
    for (i, s) in file.sounds.iter().enumerate() {
        if !valid_sound_name(s) {
            return Err(invalid(
                &format!("sounds[{i}]"),
                format!("{s:?}: a sound is 1 to {MAX_SOUND_STEM} of a-z, 0-9, _ and -, then .wav or .ogg"),
            ));
        }
        if !seen.insert(s.to_ascii_lowercase()) {
            return Err(invalid(&format!("sounds[{i}]"), format!("{s:?}: listed twice")));
        }
    }
    Ok(())
}

/// The rules of `file`'s states, exposed entries, sounds and health (its
/// elements and groups checked before), each an error naming its path.
pub(super) fn check_states(file: &AssetFile) -> Result<(), FormatError> {
    use PropertySource as P;
    let at = super::ASSET_FILE;
    let invalid = |path: &str, m: String| FormatError::invalid(at, path, m);
    check_counts(file)?;
    // States: names, then the start.
    let mut names = BTreeSet::new();
    for (i, s) in file.states.iter().enumerate() {
        if !valid_state_name(&s.name) {
            return Err(invalid(
                &format!("states[{i}].name"),
                format!(
                    "{:?}: a state's name is 1 to {MAX_STATE_NAME} letters, digits, spaces, - and _",
                    s.name
                ),
            ));
        }
        if !names.insert(s.name.to_ascii_lowercase()) {
            return Err(invalid(
                &format!("states[{i}].name"),
                format!("state {:?}: another state has this name (case aside)", s.name),
            ));
        }
    }
    let Some(start) = start_index(file) else {
        let name = file.start.as_deref().unwrap_or_default();
        return Err(invalid("start", format!("no state {name:?}")));
    };
    if let Some(id) = file.states[start].own.first() {
        return Err(invalid(
            &format!("states[{start}].own"),
            format!(
                "state {:?} is the starting state: its pictures are the layers' own ({id:?} owned)",
                file.states[start].name
            ),
        ));
    }
    let sound = |s: &Option<String>| s.as_ref().is_none_or(|s| file.sounds.contains(s));
    // Exposed entries, before the states' values (their limits apply).
    let mut exposed: BTreeMap<String, usize> = BTreeMap::new();
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    for (i, x) in file.exposed.iter().enumerate() {
        let path = |f: &str| format!("exposed[{i}].{f}");
        if !valid_exposed_name(&x.name) {
            return Err(invalid(
                &path("name"),
                format!(
                    "{:?}: a name is 1 to {MAX_EXPOSED_NAME} of a-z, 0-9 and _, not starting with a digit",
                    x.name
                ),
            ));
        }
        if exposed.insert(x.name.clone(), i).is_some() {
            return Err(invalid(&path("name"), format!("{:?}: exposed twice", x.name)));
        }
        let t = property_type(file, &x.property).map_err(|m| invalid(&path("property"), m))?;
        if let Some(j) = keys.insert(property_key(file, &x.property), i) {
            return Err(invalid(
                &path("property"),
                format!(
                    "{:?} sets what exposed[{j}] ({:?}) sets: one name per value (a glow group's settings once)",
                    x.name, file.exposed[j].name
                ),
            ));
        }
        let (lo, hi) = match t.range {
            Some((lo, hi)) => {
                if x.property.is_position() && (x.min.is_none() || x.max.is_none()) {
                    return Err(invalid(
                        &path("min"),
                        format!("{:?}: a position is exposed with a min and a max", x.name),
                    ));
                }
                (x.min.unwrap_or(lo), x.max.unwrap_or(hi))
            }
            None => {
                if x.min.is_some() || x.max.is_some() {
                    return Err(invalid(
                        &path("min"),
                        format!("{:?}: only a number has a min and a max", x.name),
                    ));
                }
                (0, 0)
            }
        };
        if let Some((rlo, rhi)) = t.range {
            for (field, v) in [("min", x.min), ("max", x.max)] {
                if let Some(v) = v
                    && !(rlo..=rhi).contains(&v)
                {
                    return Err(invalid(&path(field), format!("{v}: {rlo} to {rhi}")));
                }
            }
            if lo > hi {
                return Err(invalid(&path("min"), format!("{lo} is above max {hi}")));
            }
        }
        let range = t.range.map(|_| (lo, hi));
        match &x.default {
            Some(v) => check_value(t, range, v, at, &path("default"))?,
            None if matches!(x.property, P::GlowColour(_)) && own_value(file, &x.property).is_none() => {
                return Err(invalid(
                    &path("default"),
                    format!(
                        "{:?}: its glow group takes each pixel's colour, so it needs a default colour",
                        x.name
                    ),
                ));
            }
            None => {
                if let Some(v) = own_value(file, &x.property) {
                    check_value(t, range, &v, at, &path("default"))
                        .map_err(|e| invalid(&path("default"), format!("none, so the layer's own: {e}")))?;
                }
            }
        }
        if x.property == P::Health
            && let (Some(ValueSource::Int(v)), Some(h)) = (&x.default, &file.health)
            && i64::from(*v) != i64::from(h.points)
        {
            return Err(invalid(
                &path("default"),
                format!("{v}: an exposed health starts at health.points, {}", h.points),
            ));
        }
    }
    // Each state's own pictures, values and sounds.
    for (i, s) in file.states.iter().enumerate() {
        let path = |f: &str| format!("states[{i}].{f}");
        let mut owned = BTreeSet::new();
        for (k, id) in s.own.iter().enumerate() {
            let p = format!("states[{i}].own[{k}]");
            let Some(e) = file.elements.iter().find(|e| e.id == *id) else {
                return Err(invalid(&p, format!("state {:?}: no layer {id:?}", s.name)));
            };
            match &e.kind {
                ElementKind::Drawn { .. } if !e.is_distant() => {}
                ElementKind::Drawn { .. } => {
                    return Err(invalid(&p, format!("state {:?}: {id:?} is a distant layer", s.name)));
                }
                ElementKind::Image { .. } => {
                    return Err(invalid(
                        &p,
                        format!(
                            "state {:?}: {id:?} is an image, the same in every state: turn it into a drawn layer to paint it in a state",
                            s.name
                        ),
                    ));
                }
                ElementKind::Raster => {
                    return Err(invalid(&p, format!("state {:?}: {id:?} is a raster", s.name)));
                }
            }
            if !owned.insert(id.as_str()) {
                return Err(invalid(&p, format!("state {:?}: {id:?} owned twice", s.name)));
            }
        }
        let mut keys: BTreeMap<String, usize> = BTreeMap::new();
        for (k, v) in s.values.iter().enumerate() {
            let p = format!("states[{i}].values[{k}]");
            let t = property_type(file, &v.property)
                .map_err(|m| invalid(&format!("{p}.property"), format!("state {:?}: {m}", s.name)))?;
            if let Some(j) = keys.insert(property_key(file, &v.property), k) {
                return Err(invalid(
                    &format!("{p}.property"),
                    format!(
                        "state {:?}: values[{j}] sets this already (a glow group's settings once)",
                        s.name
                    ),
                ));
            }
            let x = file.exposed.iter().find(|x| x.property == v.property);
            let range = x.and(t.range).map(|(lo, hi)| {
                let x = x.map_or((None, None), |x| (x.min, x.max));
                (x.0.unwrap_or(lo), x.1.unwrap_or(hi))
            });
            check_value(t, range, &v.value, at, &format!("{p}.value"))?;
        }
        if !sound(&s.loop_sound) {
            return Err(invalid(
                &path("loop_sound"),
                format!(
                    "state {:?}: no sound {:?} in sounds",
                    s.name,
                    s.loop_sound.as_deref().unwrap_or_default()
                ),
            ));
        }
        if !sound(&s.enter_sound) {
            return Err(invalid(
                &path("enter_sound"),
                format!(
                    "state {:?}: no sound {:?} in sounds",
                    s.name,
                    s.enter_sound.as_deref().unwrap_or_default()
                ),
            ));
        }
    }
    if let Some(h) = &file.health {
        if !(1..=MAX_HEALTH).contains(&h.points) {
            return Err(invalid("health.points", format!("{}: 1 to {MAX_HEALTH}", h.points)));
        }
        if !file.states.iter().any(|s| s.name == h.at_zero) {
            return Err(invalid("health.at_zero", format!("no state {:?}", h.at_zero)));
        }
    }
    Ok(())
}
