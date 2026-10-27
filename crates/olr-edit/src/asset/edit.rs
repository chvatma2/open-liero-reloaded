//! The operations that edit an asset's states, pictures, values, exposed
//! properties, sounds, script and health (issue #72), shared by the
//! editor's asset tab and `olr-cli asset`; and the CLI's property and value
//! syntax ([`parse_property`], [`property_label`], [`parse_value`],
//! [`value_label`]), which the editor's labels share. Each operation edits
//! a copy of the asset and keeps it only when the result checks (the
//! folder `encode` would write, `check_asset`), so a refusal leaves the
//! asset as it was; each refusal names the state, layer, property or sound.

use std::sync::Arc;

use olr_format::Colour;
use olr_format::assets::{
    ExposedSource, HealthSource, MAX_ASSET_SOUND_BYTES, MAX_ASSET_SOUNDS, MAX_EXPOSED, MAX_HEALTH,
    MAX_SCRIPT_BYTES, MAX_SOUND_STEM, MAX_STATES, PropertySource, StateValue, ValueKind, ValueSource,
    check_asset, check_value, property_key, property_type, valid_exposed_name, valid_sound_name,
    valid_state_name,
};
use olr_format::levels::{ElementKind, element_box};
use olr_format::look::AnimationSource;

use super::{Asset, AssetError, OwnPicture, State, offers, posed::header, unchecked_files};
use crate::compose::Decoded;

fn bad(m: impl Into<String>) -> AssetError {
    AssetError::Bad(m.into())
}

/// `edit` applied to a copy of `asset`, kept only when the result checks.
fn commit<T>(
    asset: &mut Asset,
    edit: impl FnOnce(&mut Asset) -> Result<T, AssetError>,
) -> Result<T, AssetError> {
    let mut a = asset.clone();
    let out = edit(&mut a)?;
    check_asset(&unchecked_files(&a)?)?;
    *asset = a;
    Ok(out)
}

/// The index of state `name`, or an error naming it and the states there are.
fn state_at(asset: &Asset, name: &str) -> Result<usize, AssetError> {
    asset.state(name).ok_or_else(|| {
        let names: Vec<&str> = asset.states.iter().map(|s| s.name.as_str()).collect();
        bad(format!("no state {name:?} (the states: {})", names.join(", ")))
    })
}

/// Whether a state other than `but` is named `name`, ASCII case aside.
fn name_taken<'a>(asset: &'a Asset, name: &str, but: Option<usize>) -> Option<&'a str> {
    asset
        .states
        .iter()
        .enumerate()
        .find(|(i, s)| Some(*i) != but && s.name.eq_ignore_ascii_case(name))
        .map(|(_, s)| s.name.as_str())
}

fn check_state_name(asset: &Asset, name: &str, but: Option<usize>) -> Result<(), AssetError> {
    if !valid_state_name(name) {
        return Err(bad(format!(
            "{name:?}: a state's name is 1 to {} letters, digits, spaces, - and _",
            olr_format::assets::MAX_STATE_NAME
        )));
    }
    if let Some(other) = name_taken(asset, name, but) {
        return Err(bad(format!("{name:?}: the asset has a state {other:?} already (case aside)")));
    }
    Ok(())
}

/// Adds state `name` at the end and returns its index. With no `from` it
/// copies the starting state's values, owning nothing (it looks like the
/// start); with `from` it copies that state's values, own pictures and
/// sounds.
pub fn add_state(asset: &mut Asset, name: &str, from: Option<&str>) -> Result<usize, AssetError> {
    check_state_name(asset, name, None)?;
    if asset.states.len() >= MAX_STATES {
        return Err(bad(format!("{name:?}: the asset has {MAX_STATES} states, the most it may")));
    }
    let state = match from {
        Some(f) => State { name: name.into(), ..asset.states[state_at(asset, f)?].clone() },
        None => State {
            values: asset.states.get(asset.start).map(|s| s.values.clone()).unwrap_or_default(),
            ..State::new(name)
        },
    };
    commit(asset, |a| {
        a.states.push(state);
        Ok(a.states.len() - 1)
    })
}

/// Removes state `name`: refused for the starting state and while
/// `health.at_zero` names it.
pub fn remove_state(asset: &mut Asset, name: &str) -> Result<(), AssetError> {
    let i = state_at(asset, name)?;
    if i == asset.start {
        return Err(bad(format!("state {name:?} is the starting state: make another state the start first")));
    }
    if asset.health.as_ref().is_some_and(|h| h.at_zero == name) {
        return Err(bad(format!("state {name:?} is health's state at 0: change or clear the health first")));
    }
    commit(asset, |a| {
        a.states.remove(i);
        if i < a.start {
            a.start -= 1;
        }
        Ok(())
    })
}

/// Renames state `name` to `new`; the start and `health.at_zero` follow.
/// Scripts are text and are not rewritten.
pub fn rename_state(asset: &mut Asset, name: &str, new: &str) -> Result<(), AssetError> {
    let i = state_at(asset, name)?;
    check_state_name(asset, new, Some(i))?;
    commit(asset, |a| {
        a.states[i].name = new.into();
        if let Some(h) = a.health.as_mut().filter(|h| h.at_zero == name) {
            h.at_zero = new.into();
        }
        Ok(())
    })
}

/// Makes state `name` the start, every state's look kept: for each layer
/// `L` the new start `B` owns, the old start comes to own `L`'s base
/// picture, every other state that followed the base for `L` a copy of it
/// (and one owning `L` without a glow mask the base's mask), then `L`'s base
/// becomes `B`'s picture (its mask, when it has one) and `B` owns nothing.
/// Values are untouched.
pub fn set_start(asset: &mut Asset, name: &str) -> Result<(), AssetError> {
    let b = state_at(asset, name)?;
    if b == asset.start {
        return Ok(());
    }
    commit(asset, |a| {
        let owned = std::mem::take(&mut a.states[b].own);
        for (id, pic) in owned {
            let Some(p) = a.pieces.iter_mut().find(|p| p.element.id == id) else {
                return Err(bad(format!("state {name:?} owns {id:?}, which is no layer")));
            };
            let base = OwnPicture { decoded: p.decoded.clone(), glow_mask: p.glow_mask.clone() };
            for (k, st) in a.states.iter_mut().enumerate() {
                if k == b {
                    continue;
                }
                match st.own.get_mut(&id) {
                    Some(o) if o.glow_mask.is_none() => o.glow_mask = base.glow_mask.clone(),
                    Some(_) => {}
                    None => {
                        st.own.insert(id.clone(), base.clone());
                    }
                }
            }
            p.decoded = pic.decoded;
            if pic.glow_mask.is_some() {
                p.glow_mask = pic.glow_mask;
            }
        }
        a.start = b;
        Ok(())
    })
}

/// Sets state `name`'s sounds: `None` leaves one as it is, `Some(None)`
/// clears it, `Some(Some(s))` names one of the asset's sounds.
pub fn set_state_sounds(
    asset: &mut Asset,
    name: &str,
    loop_sound: Option<Option<String>>,
    enter_sound: Option<Option<String>>,
) -> Result<(), AssetError> {
    let i = state_at(asset, name)?;
    for (what, s) in [("loop", &loop_sound), ("enter", &enter_sound)] {
        if let Some(Some(s)) = s
            && !asset.sounds.contains_key(s)
        {
            return Err(bad(format!("state {name:?}'s {what} sound: the asset has no sound {s:?}")));
        }
    }
    commit(asset, |a| {
        let st = &mut a.states[i];
        if let Some(s) = loop_sound {
            st.loop_sound = s;
        }
        if let Some(s) = enter_sound {
            st.enter_sound = s;
        }
        Ok(())
    })
}

/// State `state` paints layer `layer` itself with `picture`: a drawn
/// layer's box at the asset's art scale (art, the cells of a mixed layer,
/// back art of a layer with it; a glow mask only when the layer has one).
/// Refused for the starting state (paint the layer instead), an image, a
/// raster and a distant layer.
pub fn own_picture(
    asset: &mut Asset,
    state: &str,
    layer: &str,
    picture: OwnPicture,
) -> Result<(), AssetError> {
    let i = state_at(asset, state)?;
    if i == asset.start {
        return Err(bad(format!(
            "state {state:?} is the starting state: its picture of {layer:?} is the layer's own (paint the layer instead)"
        )));
    }
    let p = asset
        .pieces
        .iter()
        .find(|p| p.element.id == layer)
        .ok_or_else(|| bad(format!("no layer {layer:?}")))?;
    let e = &p.element;
    let (mixed, back) = match &e.kind {
        ElementKind::Image { .. } => {
            return Err(bad(format!(
                "{layer:?} is an image, the same in every state: turn it into a drawn layer to paint it in a state"
            )));
        }
        ElementKind::Raster => return Err(bad(format!("{layer:?} is a raster: a state cannot paint it"))),
        _ if e.is_distant() => {
            return Err(bad(format!("{layer:?} is a distant layer: a state cannot paint it")));
        }
        ElementKind::Drawn { mixed, back, .. } => (*mixed, *back),
    };
    let s = asset.art_scale;
    let (_, (w, h)) = element_box(&e.kind, asset.size).unwrap_or(((0, 0), asset.size));
    let (aw, ah) = (w * s, h * s);
    let Decoded::Drawn { art, cells, back: back_art } = &picture.decoded else {
        return Err(bad(format!("state {state:?}'s picture of {layer:?} is not drawn pixels")));
    };
    let of = |what: &str, got: (u32, u32), want: (u32, u32)| {
        bad(format!(
            "state {state:?}'s {what} of {layer:?} is {} × {}: the layer's box takes {} × {}",
            got.0, got.1, want.0, want.1
        ))
    };
    if (art.width(), art.height()) != (aw, ah) {
        return Err(of("picture", (art.width(), art.height()), (aw, ah)));
    }
    match (mixed, cells) {
        (true, Some(c)) if c.len() as u64 == u64::from(w) * u64::from(h) => {}
        (true, _) => {
            return Err(bad(format!(
                "{layer:?} is a mixed layer: state {state:?}'s picture of it needs its cells, {w} × {h}"
            )));
        }
        (false, Some(_)) => {
            return Err(bad(format!(
                "{layer:?} is not a mixed layer: state {state:?}'s picture has no cells"
            )));
        }
        (false, None) => {}
    }
    match (back, back_art) {
        (true, Some(b)) if (b.width(), b.height()) != (aw, ah) => {
            return Err(of("back art", (b.width(), b.height()), (aw, ah)));
        }
        (true, Some(_)) | (false, None) => {}
        (true, None) => {
            return Err(bad(format!(
                "{layer:?} has back art: state {state:?}'s picture of it needs its own"
            )));
        }
        (false, Some(_)) => {
            return Err(bad(format!("{layer:?} has no back art: state {state:?}'s picture has none")));
        }
    }
    if let Some(m) = &picture.glow_mask {
        let masked = e.glow.as_ref().is_some_and(|g| g.mask) && p.glow_mask.is_some();
        if !masked {
            return Err(bad(format!("{layer:?} has no glow mask: state {state:?}'s picture of it has none")));
        }
        if m.len() as u64 != u64::from(aw) * u64::from(ah) {
            return Err(bad(format!(
                "state {state:?}'s glow mask of {layer:?} holds {} values: the layer's box takes {aw} × {ah}",
                m.len()
            )));
        }
    }
    commit(asset, |a| {
        a.states[i].own.insert(layer.into(), picture);
        Ok(())
    })
}

/// State `state` shows the starting state's picture of `layer` again.
pub fn follow_start(asset: &mut Asset, state: &str, layer: &str) -> Result<(), AssetError> {
    let i = state_at(asset, state)?;
    if !asset.states[i].own.contains_key(layer) {
        return Err(bad(format!("state {state:?} does not paint {layer:?}: it follows the start already")));
    }
    commit(asset, |a| {
        a.states[i].own.remove(layer);
        Ok(())
    })
}

/// Sets (`Some`) or clears (`None`) state `state`'s value of `property`:
/// of the property's type, within the exposed entry's range when it is
/// exposed, else the property's. A value of the same glow group's setting
/// through another layer is replaced.
pub fn set_state_value(
    asset: &mut Asset,
    state: &str,
    property: PropertySource,
    value: Option<ValueSource>,
) -> Result<(), AssetError> {
    let i = state_at(asset, state)?;
    let file = header(asset);
    let label = property_label(&property);
    let key = property_key(&file, &property);
    let at = asset.states[i].values.iter().position(|v| property_key(&file, &v.property) == key);
    let Some(value) = value else {
        let Some(k) = at else { return Err(bad(format!("state {state:?} sets no {label}"))) };
        return commit(asset, |a| {
            a.states[i].values.remove(k);
            Ok(())
        });
    };
    let t = property_type(&file, &property).map_err(|m| bad(format!("state {state:?}: {label}: {m}")))?;
    of_kind(t.kind, &value).map_err(|m| bad(format!("state {state:?}: {label}: {m}")))?;
    let x = asset.exposed.iter().find(|x| x.property == property);
    let range = t.range.map(|(lo, hi)| x.map_or((lo, hi), |x| (x.min.unwrap_or(lo), x.max.unwrap_or(hi))));
    check_value(t, range, &value, &format!("state {state:?}"), &label).map_err(|e| bad(e.to_string()))?;
    commit(asset, |a| {
        let v = StateValue { property, value };
        match at {
            Some(k) => a.states[i].values[k] = v,
            None => a.states[i].values.push(v),
        }
        Ok(())
    })
}

/// What [`forget_layer`] dropped: each state's own picture of the layer
/// and value naming it (by state name), and the exposed entries naming it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forgotten {
    pub own: Vec<(String, OwnPicture)>,
    pub values: Vec<(String, StateValue)>,
    pub exposed: Vec<ExposedSource>,
}

impl Forgotten {
    /// Whether nothing named the layer.
    pub fn is_empty(&self) -> bool {
        self.own.is_empty() && self.values.is_empty() && self.exposed.is_empty()
    }
}

/// Drops layer `id` from every state's own pictures, every state value
/// naming it and every exposed entry naming it (the editor's layer
/// removal, merge and split; the caller removes the layer), returning what
/// it dropped so the caller can say so and undo it. Not checked: the
/// caller's edit is.
pub fn forget_layer(asset: &mut Asset, id: &str) -> Forgotten {
    let mut out = Forgotten::default();
    let names = |p: &PropertySource| p.layer() == Some(id);
    for st in &mut asset.states {
        if let Some(o) = st.own.remove(id) {
            out.own.push((st.name.clone(), o));
        }
        let (gone, kept): (Vec<StateValue>, Vec<StateValue>) =
            std::mem::take(&mut st.values).into_iter().partition(|v| names(&v.property));
        st.values = kept;
        out.values.extend(gone.into_iter().map(|v| (st.name.clone(), v)));
    }
    let (gone, kept): (Vec<ExposedSource>, Vec<ExposedSource>) =
        std::mem::take(&mut asset.exposed).into_iter().partition(|x| names(&x.property));
    asset.exposed = kept;
    out.exposed = gone;
    out
}

/// Exposes `x.property` as `x.name`: one of [`offers`]; an `Int` without
/// `min`/`max` takes its range (a position needs both); a glow group's
/// setting at most once (through any of its layers); an `Own` group's
/// `GlowColour` needs a default; an exposed `Health` takes `points` as its
/// default.
pub fn expose(asset: &mut Asset, mut x: ExposedSource) -> Result<(), AssetError> {
    use PropertySource as P;
    let label = property_label(&x.property);
    if !valid_exposed_name(&x.name) {
        return Err(bad(format!(
            "{:?}: an exposed name is 1 to {} of a-z, 0-9 and _, not starting with a digit",
            x.name,
            olr_format::assets::MAX_EXPOSED_NAME
        )));
    }
    if asset.exposed.iter().any(|e| e.name == x.name) {
        return Err(bad(format!("{:?} is exposed already", x.name)));
    }
    if asset.exposed.len() >= MAX_EXPOSED {
        return Err(bad(format!("{:?}: the asset exposes {MAX_EXPOSED} values, the most it may", x.name)));
    }
    let file = header(asset);
    let Some(offer) = offers(asset).into_iter().find(|o| o.property == x.property) else {
        let why = property_type(&file, &x.property).err().unwrap_or_else(|| "not offered".into());
        return Err(bad(format!("{label}: {why}")));
    };
    if let Some(v) = &x.default {
        of_kind(offer.kind, v).map_err(|m| bad(format!("{label}'s default: {m}")))?;
    }
    let key = property_key(&file, &x.property);
    if let Some(e) = asset.exposed.iter().find(|e| property_key(&file, &e.property) == key) {
        let what = match e.property == x.property {
            true => format!("{label} is exposed already as {:?}", e.name),
            false => format!(
                "{label}: its glow group's setting is exposed already as {:?} ({}): one name per group",
                e.name,
                property_label(&e.property)
            ),
        };
        return Err(bad(what));
    }
    if let Some((lo, hi)) = offer.range {
        if x.property.is_position() && (x.min.is_none() || x.max.is_none()) {
            return Err(bad(format!("{label}: a position is exposed with a min and a max")));
        }
        x.min.get_or_insert(lo);
        x.max.get_or_insert(hi);
    }
    if x.property == P::Health
        && let Some(h) = &asset.health
    {
        x.default.get_or_insert(ValueSource::Int(i32::try_from(h.points).unwrap_or(i32::MAX)));
    }
    if matches!(x.property, P::GlowColour(_))
        && x.default.is_none()
        && olr_format::assets::own_value(&file, &x.property).is_none()
    {
        return Err(bad(format!(
            "{label}: its glow group takes each pixel's colour (Own), so it needs a default colour"
        )));
    }
    commit(asset, |a| {
        a.exposed.push(x);
        Ok(())
    })
}

/// Stops exposing `name`.
pub fn unexpose(asset: &mut Asset, name: &str) -> Result<(), AssetError> {
    let i = asset
        .exposed
        .iter()
        .position(|x| x.name == name)
        .ok_or_else(|| bad(format!("nothing is exposed as {name:?}")))?;
    commit(asset, |a| {
        a.exposed.remove(i);
        Ok(())
    })
}

/// Renames exposed `name` to `new` (scripts are text and are not
/// rewritten).
pub fn rename_exposed(asset: &mut Asset, name: &str, new: &str) -> Result<(), AssetError> {
    let i = asset
        .exposed
        .iter()
        .position(|x| x.name == name)
        .ok_or_else(|| bad(format!("nothing is exposed as {name:?}")))?;
    if !valid_exposed_name(new) {
        return Err(bad(format!(
            "{new:?}: an exposed name is 1 to {} of a-z, 0-9 and _, not starting with a digit",
            olr_format::assets::MAX_EXPOSED_NAME
        )));
    }
    if asset.exposed.iter().enumerate().any(|(k, x)| k != i && x.name == new) {
        return Err(bad(format!("{new:?} is exposed already")));
    }
    commit(asset, |a| {
        a.exposed[i].name = new.into();
        Ok(())
    })
}

/// Adds sound `name` (`[a-z0-9_-]{1,32}` then `.wav` or `.ogg`, a name not
/// taken case aside), checked as a mod's (`olr_pack::sounds`: WAV or Ogg,
/// within `MAX_ASSET_SOUND_BYTES` and its length).
pub fn add_sound(asset: &mut Asset, name: &str, bytes: Vec<u8>) -> Result<(), AssetError> {
    if !valid_sound_name(name) {
        return Err(bad(format!(
            "sound {name:?}: a sound's name is 1 to {MAX_SOUND_STEM} of a-z, 0-9, _ and -, then .wav or .ogg"
        )));
    }
    if let Some(other) = asset.sounds.keys().find(|k| k.eq_ignore_ascii_case(name)) {
        return Err(bad(format!("sound {name:?}: the asset has a sound {other:?} already")));
    }
    if asset.sounds.len() >= MAX_ASSET_SOUNDS {
        return Err(bad(format!(
            "sound {name:?}: the asset holds {MAX_ASSET_SOUNDS} sounds, the most it may"
        )));
    }
    if bytes.len() as u64 > MAX_ASSET_SOUND_BYTES {
        return Err(bad(format!("sound {name:?}: {} bytes, more than {MAX_ASSET_SOUND_BYTES}", bytes.len())));
    }
    if !olr_pack::sounds::is_sound(&bytes) {
        return Err(bad(format!("sound {name:?}: not a WAV or Ogg sound")));
    }
    olr_pack::sounds::check_sound(&bytes).map_err(|why| bad(format!("sound {name:?}: {why}")))?;
    commit(asset, |a| {
        a.sounds.insert(name.into(), Arc::new(bytes));
        Ok(())
    })
}

/// Removes sound `name`: refused while a state plays it.
pub fn remove_sound(asset: &mut Asset, name: &str) -> Result<(), AssetError> {
    if !asset.sounds.contains_key(name) {
        return Err(bad(format!("the asset has no sound {name:?}")));
    }
    for st in &asset.states {
        for (what, s) in [("loop", &st.loop_sound), ("enter", &st.enter_sound)] {
            if s.as_deref() == Some(name) {
                return Err(bad(format!("sound {name:?} is state {:?}'s {what} sound", st.name)));
            }
        }
    }
    commit(asset, |a| {
        a.sounds.remove(name);
        Ok(())
    })
}

/// Sets (`Some`) or clears the default script: at most
/// `MAX_SCRIPT_BYTES`, not parsed.
pub fn set_script(asset: &mut Asset, script: Option<String>) -> Result<(), AssetError> {
    if let Some(s) = &script
        && s.len() as u64 > MAX_SCRIPT_BYTES
    {
        return Err(bad(format!("the script is {} bytes, more than {MAX_SCRIPT_BYTES}", s.len())));
    }
    commit(asset, |a| {
        a.script = script;
        Ok(())
    })
}

/// Sets (`Some`) or clears the asset's health: `points` 1 to `MAX_HEALTH`,
/// `at_zero` naming a state; an exposed `Health` follows the points (its
/// default, and a `max` above them). Clearing it while `Health` is exposed
/// is refused.
pub fn set_health(asset: &mut Asset, health: Option<HealthSource>) -> Result<(), AssetError> {
    let exposed = asset.exposed.iter().position(|x| x.property == PropertySource::Health);
    let Some(h) = health else {
        if let Some(i) = exposed {
            return Err(bad(format!(
                "health is exposed as {:?}: unexpose health first",
                asset.exposed[i].name
            )));
        }
        return commit(asset, |a| {
            a.health = None;
            Ok(())
        });
    };
    if !(1..=MAX_HEALTH).contains(&h.points) {
        return Err(bad(format!("health {}: 1 to {MAX_HEALTH} points", h.points)));
    }
    state_at(asset, &h.at_zero).map_err(|e| bad(format!("health's state at 0: {e}")))?;
    let old = asset.health.as_ref().map(|h| h.points);
    commit(asset, |a| {
        if let Some(i) = exposed {
            let points = i32::try_from(h.points).unwrap_or(i32::MAX);
            let x = &mut a.exposed[i];
            x.default = Some(ValueSource::Int(points));
            if x.max.is_some_and(|m| m > points || old.is_some_and(|o| i64::from(m) == i64::from(o))) {
                x.max = Some(points);
            }
        }
        a.health = Some(h);
        Ok(())
    })
}

/// Why `v` is not a value of `kind`, if it is not.
fn of_kind(kind: ValueKind, v: &ValueSource) -> Result<(), String> {
    if v.kind() == kind {
        return Ok(());
    }
    let what = match kind {
        ValueKind::Int => "a whole number",
        ValueKind::Bool => "true or false",
        ValueKind::Colour => "a colour, #rrggbb",
        ValueKind::Animation => "an animation, steady or in RON",
    };
    Err(format!("{} is not {what}", value_label(v)))
}

/// A layer property's variant, made from the layer's id.
type OfLayer = fn(String) -> PropertySource;

/// The layer properties' names in the CLI's syntax (`<layer>.<name>`),
/// with their variants.
const LAYER_PROPERTIES: [(&str, OfLayer); 13] = [
    ("x", PropertySource::OffsetX),
    ("y", PropertySource::OffsetY),
    ("visible", PropertySource::LayerVisible),
    ("opacity", PropertySource::Opacity),
    ("glow", PropertySource::GlowStrength),
    ("glow-colour", PropertySource::GlowColour),
    ("light", PropertySource::LightIntensity),
    ("reach", PropertySource::LightReach),
    ("animation", PropertySource::GlowAnimation),
    ("beam-direction", PropertySource::BeamDirection),
    ("beam-spread", PropertySource::BeamSpread),
    ("glass-opacity", PropertySource::GlassOpacity),
    ("glass-tint", PropertySource::GlassTint),
];

/// The asset's own properties' names.
const ASSET_PROPERTIES: [(&str, PropertySource); 5] = [
    ("x", PropertySource::X),
    ("y", PropertySource::Y),
    ("visible", PropertySource::Visible),
    ("facing", PropertySource::Facing),
    ("health", PropertySource::Health),
];

/// A property in the CLI's syntax: `x`, `y`, `visible`, `facing`,
/// `health`, or `<layer>.<name>` (`x`, `y`, `visible`, `opacity`, `glow`,
/// `glow-colour`, `light`, `reach`, `animation`, `beam-direction`,
/// `beam-spread`, `glass-opacity`, `glass-tint`), split at the last dot (an
/// element id may hold one: `door.left.visible` is layer `door.left`).
pub fn parse_property(s: &str) -> Result<PropertySource, String> {
    let names = || {
        let own: Vec<&str> = ASSET_PROPERTIES.iter().map(|(n, _)| *n).collect();
        let layer: Vec<&str> = LAYER_PROPERTIES.iter().map(|(n, _)| *n).collect();
        format!("{s:?}: a property is {} or <layer>.<{}>", own.join(", "), layer.join("|"))
    };
    match s.rsplit_once('.') {
        None => ASSET_PROPERTIES.iter().find(|(n, _)| *n == s).map(|(_, p)| p.clone()).ok_or_else(names),
        Some((layer, name)) if !layer.is_empty() => LAYER_PROPERTIES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, make)| make(layer.to_string()))
            .ok_or_else(names),
        Some(_) => Err(names()),
    }
}

/// `p` in the CLI's syntax ([`parse_property`] reads it back).
pub fn property_label(p: &PropertySource) -> String {
    if let Some(layer) = p.layer() {
        let probe = p.on("");
        let name =
            LAYER_PROPERTIES.iter().find(|(_, make)| make(String::new()) == probe).map_or("?", |(n, _)| n);
        return format!("{layer}.{name}");
    }
    ASSET_PROPERTIES.iter().find(|(_, q)| q == p).map_or("?", |(n, _)| n).to_string()
}

/// A value in the CLI's syntax, its type read from its form: a whole
/// number (`Int`), `true` or `false` (`Bool`), `#rrggbb` (`Colour`),
/// `steady` or an `AnimationSource` in RON (`Flicker(speed: 40, depth:
/// 60)`). The property's type is checked where it is set.
pub fn parse_value(s: &str) -> Result<ValueSource, String> {
    let t = s.trim();
    match t {
        "true" => return Ok(ValueSource::Bool(true)),
        "false" => return Ok(ValueSource::Bool(false)),
        "steady" => return Ok(ValueSource::Animation(AnimationSource::Steady)),
        _ => {}
    }
    if let Ok(n) = t.parse::<i32>() {
        return Ok(ValueSource::Int(n));
    }
    if t.starts_with('#') {
        return Colour::parse(t).map(ValueSource::Colour).map_err(|e| format!("{s:?}: {e}"));
    }
    if t.starts_with(|c: char| c.is_ascii_uppercase()) {
        return olr_format::ron_io::from_canonical::<AnimationSource>("value", t)
            .map(ValueSource::Animation)
            .map_err(|e| format!("{s:?}: not an animation: {e}"));
    }
    Err(format!(
        "{s:?}: a value is a whole number, true or false, #rrggbb, steady or an animation in RON (\"Flicker(speed: 40, depth: 60)\")"
    ))
}

/// `v` in the CLI's syntax ([`parse_value`] reads it back): an animation
/// as one line of RON.
pub fn value_label(v: &ValueSource) -> String {
    match v {
        ValueSource::Int(n) => n.to_string(),
        ValueSource::Bool(b) => b.to_string(),
        ValueSource::Colour(c) => c.to_hex(),
        ValueSource::Animation(AnimationSource::Steady) => "steady".into(),
        ValueSource::Animation(a) => {
            let text = olr_format::ron_io::to_canonical("value", a).unwrap_or_default();
            let line = text.lines().map(str::trim).collect::<Vec<_>>().join(" ");
            line.replace("( ", "(").replace("[ ", "[").replace(", )", ")").replace(", ]", "]")
        }
    }
}

/// The value property `p` takes in `asset` before any state or copy sets
/// it: its exposed default, else the layer's (or the asset's) own setting;
/// none for an `Own` group's colour.
pub fn default_value(asset: &Asset, p: &PropertySource) -> Option<ValueSource> {
    let file = header(asset);
    asset
        .exposed
        .iter()
        .find(|x| x.property == *p)
        .and_then(|x| x.default.clone())
        .or_else(|| olr_format::assets::own_value(&file, p))
}
