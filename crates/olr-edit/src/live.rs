//! A level's live copies baked to their played form (issue #85; the format
//! is `olr_format::levels`' `live`): what [`crate::placed::bake`] writes
//! for every Live copy instead of baking it into the ground.
//!
//! A copy is planned from its asset alone ([`plan`]): its values (each
//! exposed property, then each property a state sets, by
//! `property_key`, then `Health`; a hidden targeted layer gets a
//! `LayerVisible` of default 0), its part slots (a layer a value targets
//! is a part of its own, two for a mixed or a rock layer with back art:
//! background below rock; the other shown layers flattened in runs of one
//! kind, the stack kept) and its pictures (a slot's picture in a state is
//! shared with every state owning none of its layers' pictures alike).
//! [`live_refusal`](crate::asset::live_refusal) refuses an asset past
//! `MAX_PARTS`, `MAX_PICTURES` or `MAX_LIVE_PROPS`, or with a layer
//! wholly outside its box. The copy's stretch then places it (positions
//! scaled `W / w` and `H / h`, half away from zero, limits, defaults,
//! state values and own values alike) and each picture is made by the
//! composition's bake of the slot's layers in a level of the slot's box
//! (the asset box's part it covers, at the level's art scale): a rock
//! part takes the front art and glow of its rock cells, a background
//! part the back art and glow. A targeted layer's glow mask is unscaled
//! (baked at 100 %, its strength the part's `glow_strength`), a run's is
//! the bake's (strengths applied, the part's strength 100). Live parts
//! carry no surface materials and no glass map (STATUS.md).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_format::assets::{PropertySource, ValueKind, ValueSource, own_value, property_key, property_type};
use olr_format::levels::{
    Composition, ElementKind, ElementMaterial, LevelSource, LiveCopyFile, LiveFile, LiveHealthFile,
    LivePartFile, LivePicture, LivePictureFile, LivePropFile, LiveSource, LiveStateFile, MAX_LIVE_PROPS,
    MAX_LIVE_VALUE, MAX_PARTS, MAX_PICTURES, PartKind, PlacedCopy, PlacedSource, Target, check_live,
    check_live_budget, element_box, live_script_file, live_sound_file, stretch_offset,
};
use olr_format::look::{AnimationSource, LookSource};

use crate::asset::{Asset, LiveRefusal, PosedPiece, header, posed};
use crate::compose::{Decoded, GlowMasks, PaintedSurfaces, Rect, bake_elements_with_opacity};
use crate::placed::{Frame, PlacedError, lay};

/// One part slot of a live copy: the layer it shows (none for a run),
/// its kind and its asset layers (indices of `Asset::pieces`), bottom to
/// top.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    layer: Option<u16>,
    kind: PartKind,
    layers: Vec<usize>,
    /// An open run untargeted layers of its kind may join.
    run: bool,
}

/// One value of a live copy as planned: its exposed name, the property it
/// was given for, what it drives, its `property_key`, its type and range
/// (an `Int`'s, the exposed limits applied) and its default.
#[derive(Clone, Debug, PartialEq)]
struct Prop {
    name: Option<String>,
    target: Target,
    key: String,
    kind: ValueKind,
    range: Option<(i32, i32)>,
    default: Option<ValueSource>,
}

/// A live copy's plan from its asset alone (issue #85): its values, the
/// ones its states set, its part slots and each state's picture per slot.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    props: Vec<Prop>,
    /// The props any state sets, ascending.
    set: Vec<usize>,
    slots: Vec<Slot>,
    /// Each picture: its slot and, per slot layer, the state whose own
    /// picture it shows (none: the layer's own).
    pictures: Vec<(usize, Vec<Option<usize>>)>,
    /// Per state, the picture of each slot.
    parts: Vec<Vec<usize>>,
}

/// The kind of part a layer makes: one, or a background and a rock part
/// (a mixed layer, a rock layer with back art).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayerKind {
    One(PartKind),
    Pair,
}

fn layer_kind(kind: &ElementKind) -> LayerKind {
    match kind {
        ElementKind::Drawn { mixed: true, .. } => LayerKind::Pair,
        ElementKind::Drawn { material: ElementMaterial::Rock, back: true, .. } => LayerKind::Pair,
        ElementKind::Drawn { material: ElementMaterial::Rock, .. }
        | ElementKind::Image { material: ElementMaterial::Rock, .. } => LayerKind::One(PartKind::Rock),
        _ => LayerKind::One(PartKind::Background),
    }
}

/// What `p` drives in the played form, its layer by index (`index`).
fn target_of(p: &PropertySource, index: impl Fn(&str) -> Option<usize>) -> Option<Target> {
    use PropertySource as P;
    use Target as T;
    let l = |id: &str| index(id).and_then(|i| u16::try_from(i).ok());
    Some(match p {
        P::X => T::X,
        P::Y => T::Y,
        P::Visible => T::Visible,
        P::Facing => T::Facing,
        P::Health => T::Health,
        P::OffsetX(id) => T::OffsetX(l(id)?),
        P::OffsetY(id) => T::OffsetY(l(id)?),
        P::LayerVisible(id) => T::LayerVisible(l(id)?),
        P::Opacity(id) => T::Opacity(l(id)?),
        P::GlowStrength(id) => T::GlowStrength(l(id)?),
        P::GlowColour(id) => T::GlowColour(l(id)?),
        P::LightIntensity(id) => T::LightIntensity(l(id)?),
        P::LightReach(id) => T::LightReach(l(id)?),
        P::GlowAnimation(id) => T::GlowAnimation(l(id)?),
        P::BeamDirection(id) => T::BeamDirection(l(id)?),
        P::BeamSpread(id) => T::BeamSpread(l(id)?),
        P::GlassOpacity(id) => T::GlassOpacity(l(id)?),
        P::GlassTint(id) => T::GlassTint(l(id)?),
    })
}

/// The plan of a live copy of `asset` (see the module's doc). Refused,
/// as [`live_refusal`](crate::asset::live_refusal) says it: a layer
/// wholly outside the asset's box, more than `MAX_PARTS` slots,
/// `MAX_PICTURES` pictures or `MAX_LIVE_PROPS` values.
pub(crate) fn plan(asset: &Asset) -> Result<Plan, LiveRefusal> {
    let file = header(asset);
    let index = |id: &str| asset.pieces.iter().position(|p| p.element.id == id);
    let mut props: Vec<Prop> = Vec::new();
    let mut by_key: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |props: &mut Vec<Prop>,
                   name: Option<&str>,
                   p: &PropertySource,
                   limits: (Option<i32>, Option<i32>),
                   default: Option<ValueSource>|
     -> Option<usize> {
        let key = property_key(&file, p);
        if let Some(i) = by_key.get(&key) {
            return Some(*i);
        }
        let target = target_of(p, index)?;
        let t = property_type(&file, p).ok()?;
        let range = t.range.map(|(lo, hi)| (limits.0.unwrap_or(lo), limits.1.unwrap_or(hi)));
        let default = default.or_else(|| own_value(&file, p));
        props.push(Prop {
            name: name.map(str::to_string),
            target,
            key: key.clone(),
            kind: t.kind,
            range,
            default,
        });
        by_key.insert(key, props.len() - 1);
        Some(props.len() - 1)
    };
    for x in &file.exposed {
        add(&mut props, Some(&x.name), &x.property, (x.min, x.max), x.default.clone());
    }
    let mut set = BTreeSet::new();
    for st in &file.states {
        for v in &st.values {
            if let Some(i) = add(&mut props, None, &v.property, (None, None), None) {
                set.insert(i);
            }
        }
    }
    if let Some(h) = &asset.health
        && !props.iter().any(|p| p.target == Target::Health)
    {
        let points = i32::try_from(h.points).unwrap_or(i32::MAX);
        props.push(Prop {
            name: None,
            target: Target::Health,
            key: "Health".into(),
            kind: ValueKind::Int,
            range: Some((0, points)),
            default: Some(ValueSource::Int(points)),
        });
    }
    // The layers values target, and each layer shown as its folders show
    // it; a hidden targeted layer shows when a value says so.
    let targeted: BTreeSet<usize> = props.iter().filter_map(|p| p.target.layer()).map(usize::from).collect();
    let folders = Composition { elements: Vec::new(), folders: asset.folders.clone() };
    let shown = |i: usize| crate::folders::fold(&folders, &asset.pieces[i].element).visible;
    for &l in &targeted {
        let lv = Target::LayerVisible(l as u16);
        if !shown(l) && !props.iter().any(|p| p.target == lv) {
            props.push(Prop {
                name: None,
                target: lv,
                key: format!("hidden {l}"),
                kind: ValueKind::Bool,
                range: None,
                default: Some(ValueSource::Bool(false)),
            });
        }
    }
    // The slots, bottom to top.
    let mut slots: Vec<Slot> = Vec::new();
    for (i, p) in asset.pieces.iter().enumerate() {
        let kind = layer_kind(&p.element.kind);
        let one = |kind, layer, run| Slot { layer, kind, layers: vec![i], run };
        if targeted.contains(&i) {
            let l = Some(i as u16);
            match kind {
                LayerKind::One(k) => slots.push(one(k, l, false)),
                LayerKind::Pair => {
                    slots.push(one(PartKind::Background, l, false));
                    slots.push(one(PartKind::Rock, l, false));
                }
            }
        } else if shown(i) {
            match kind {
                LayerKind::One(k) => match slots.last_mut() {
                    Some(s) if s.run && s.kind == k => s.layers.push(i),
                    _ => slots.push(one(k, None, true)),
                },
                LayerKind::Pair => {
                    slots.push(one(PartKind::Background, None, false));
                    slots.push(one(PartKind::Rock, None, false));
                }
            }
        }
    }
    for s in &slots {
        for &l in &s.layers {
            let e = &asset.pieces[l].element;
            let inside = element_box(&e.kind, asset.size).is_some_and(|((x, y), (w, h))| {
                i64::from(x) < i64::from(asset.size.0)
                    && i64::from(y) < i64::from(asset.size.1)
                    && i64::from(x) + i64::from(w) > 0
                    && i64::from(y) + i64::from(h) > 0
            });
            if !inside {
                return Err(LiveRefusal::Outside(e.id.clone()));
            }
        }
    }
    if slots.len() > MAX_PARTS {
        return Err(LiveRefusal::TooManyParts(slots.len()));
    }
    // Each state's picture per slot: shared while no layer of the slot
    // shows a picture of the state's own.
    let mut pictures: Vec<(usize, Vec<Option<usize>>)> = Vec::new();
    let mut found: BTreeMap<(usize, Vec<Option<usize>>), usize> = BTreeMap::new();
    let mut parts = Vec::with_capacity(asset.states.len());
    for (k, st) in asset.states.iter().enumerate() {
        let mut row = Vec::with_capacity(slots.len());
        for (j, s) in slots.iter().enumerate() {
            let owners: Vec<Option<usize>> = s
                .layers
                .iter()
                .map(|&l| st.own.contains_key(&asset.pieces[l].element.id).then_some(k))
                .collect();
            let n = pictures.len();
            let at = *found.entry((j, owners.clone())).or_insert_with(|| {
                pictures.push((j, owners));
                n
            });
            row.push(at);
        }
        parts.push(row);
    }
    if pictures.len() > MAX_PICTURES {
        return Err(LiveRefusal::TooManyPictures(pictures.len()));
    }
    if props.len() > MAX_LIVE_PROPS {
        return Err(LiveRefusal::TooManyValues(props.len()));
    }
    Ok(Plan { props, set: set.into_iter().collect(), slots, pictures, parts })
}

/// A copy's frame: its asset's size and its placed size.
#[derive(Clone, Copy)]
struct Stretch {
    from: (u32, u32),
    to: (u32, u32),
}

impl Stretch {
    /// Value `v` of `target` as the played form holds it: a position
    /// scaled by the stretch (half away from zero), a yes 1, a colour
    /// `0xRRGGBB`, an animation its index in `anims` (added when new).
    fn number(&self, target: Target, v: &ValueSource, anims: &mut Vec<AnimationSource>) -> i64 {
        match v {
            ValueSource::Int(n) => self.int(target, *n),
            ValueSource::Bool(b) => i64::from(*b),
            ValueSource::Colour(c) => {
                let Rgba { r, g, b, .. } = c.0;
                i64::from(r) << 16 | i64::from(g) << 8 | i64::from(b)
            }
            ValueSource::Animation(a) => match anims.iter().position(|x| x == a) {
                Some(i) => i as i64,
                None => {
                    anims.push(a.clone());
                    anims.len() as i64 - 1
                }
            },
        }
    }

    fn int(&self, target: Target, n: i32) -> i64 {
        let n = i64::from(n);
        let v = match target {
            Target::X | Target::OffsetX(_) => stretch_offset(n, self.from.0, self.to.0),
            Target::Y | Target::OffsetY(_) => stretch_offset(n, self.from.1, self.to.1),
            _ => n,
        };
        v.clamp(-MAX_LIVE_VALUE, MAX_LIVE_VALUE)
    }
}

/// Copy `c` of `asset` in the played form without its pictures' pixels:
/// its file entry (pictures listed with their sizes and masks) and each
/// picture's slot box. `scripts` and `sounds` gather the level's (by
/// content).
struct Planned {
    file: LiveCopyFile,
    plan: Plan,
    boxes: Vec<(i32, i32, u32, u32)>,
}

/// What a picture of `slot` has besides its art: a glow mask and groups
/// when a layer of it glows, beams when one has a beam.
fn picture_entry(asset: &Asset, slot: &Slot, size: (u32, u32)) -> LivePictureFile {
    let glows = slot.layers.iter().filter_map(|&l| asset.pieces[l].element.glow.as_ref());
    let (mut glow, mut beam) = (false, false);
    for g in glows {
        glow = true;
        beam |= g.beam.is_some();
    }
    LivePictureFile { size, rock: slot.kind == PartKind::Rock, glow, groups: glow, beam, glass: false }
}

/// The world box (in the copy) slot `slot`'s layers cover, placed by
/// `frame` and cut to the copy: a 1 × 1 box at the origin when nothing
/// is left (a layer stretched to nothing).
fn slot_box(asset: &Asset, slot: &Slot, frame: &Frame) -> (i32, i32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for &l in &slot.layers {
        let pp = PosedPiece { piece: asset.pieces[l].clone(), opacity: 100, offset: (0, 0) };
        let Some((e, _, _)) = lay(&pp, frame) else { continue };
        let Some(((x, y), (w, h))) = element_box(&e.kind, frame.to) else { continue };
        x0 = x0.min(i64::from(x).max(0));
        y0 = y0.min(i64::from(y).max(0));
        x1 = x1.max((i64::from(x) + i64::from(w)).min(i64::from(frame.to.0)));
        y1 = y1.max((i64::from(y) + i64::from(h)).min(i64::from(frame.to.1)));
    }
    if x0 >= x1 || y0 >= y1 {
        return (0, 0, 1, 1);
    }
    (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// Index of `item` in `list`, added when new.
fn index_of<T: PartialEq>(list: &mut Vec<T>, item: T) -> u16 {
    match list.iter().position(|x| *x == item) {
        Some(i) => i as u16,
        None => {
            list.push(item);
            (list.len() - 1) as u16
        }
    }
}

/// Copy `c` of `asset` planned in a level of art scale `s`; the level's
/// scripts and sounds gathered into `scripts` and `sounds` (by content;
/// a sound by its extension and bytes).
fn plan_copy(
    c: &PlacedCopy,
    asset: &Asset,
    s: u32,
    src: &PlacedSource,
    scripts: &mut Vec<String>,
    sounds: &mut Vec<(String, Arc<Vec<u8>>)>,
) -> Result<Planned, PlacedError> {
    let fail = |message: String| PlacedError::Copy { copy: c.id.clone(), message };
    let plan = plan(asset).map_err(|why| fail(format!("cannot be Live: {why}")))?;
    let start = match &c.state {
        Some(name) => {
            asset.state(name).ok_or_else(|| fail(format!("{:?} has no state {name:?}", asset.name)))?
        }
        None => asset.start,
    };
    // The copy's values check as a static copy's would.
    posed(asset, start, &c.values).map_err(|e| fail(e.to_string()))?;
    let file = header(asset);
    let to = c.size.unwrap_or(asset.size);
    let stretch = Stretch { from: asset.size, to };
    let mut anims: Vec<AnimationSource> = Vec::new();
    let defaults: Vec<Option<i64>> = plan
        .props
        .iter()
        .map(|p| p.default.as_ref().map(|v| stretch.number(p.target, v, &mut anims)))
        .collect();
    let own = |p: &Prop| p.name.as_ref().and_then(|n| c.values.get(n));
    let states: Vec<Vec<(u16, i64)>> = asset
        .states
        .iter()
        .map(|st| {
            plan.set
                .iter()
                .map(|&i| {
                    let p = &plan.props[i];
                    let in_state = st.values.iter().find(|v| property_key(&file, &v.property) == p.key);
                    let v = match in_state.map(|v| &v.value).or_else(|| own(p)) {
                        Some(v) => Some(stretch.number(p.target, v, &mut anims)),
                        None => defaults[i],
                    };
                    (i, v)
                })
                .map(|(i, v)| (i as u16, v.unwrap_or(i64::MIN)))
                .collect()
        })
        .collect();
    let start_values: Vec<(u16, i64)> = plan
        .props
        .iter()
        .enumerate()
        .filter(|(i, _)| !plan.set.contains(i))
        .filter_map(|(i, p)| Some((i as u16, stretch.number(p.target, own(p)?, &mut anims))))
        .collect();
    let props: Vec<LivePropFile> = plan
        .props
        .iter()
        .zip(&defaults)
        .map(|(p, d)| {
            let (min, max) = match (p.kind, p.range) {
                (ValueKind::Int, Some((lo, hi))) => (stretch.int(p.target, lo), stretch.int(p.target, hi)),
                (ValueKind::Bool, _) => (0, 1),
                (ValueKind::Colour, _) => (-1, 0xff_ffff),
                (ValueKind::Animation, _) => (0, anims.len().max(1) as i64 - 1),
                (ValueKind::Int, None) => p.target.bounds(anims.len()),
            };
            let (min, max) = (min.min(max), max.max(min));
            let default = match (d, p.kind) {
                (Some(v), _) => *v,
                (None, ValueKind::Colour) => -1,
                (None, _) => min,
            };
            LivePropFile {
                name: p.name.clone(),
                drives: p.target,
                min,
                max,
                default: default.clamp(min, max),
            }
        })
        .collect();
    // A value no state sets in a state takes the default.
    let states_values: Vec<Vec<(u16, i64)>> = states
        .into_iter()
        .map(|vs| {
            vs.into_iter()
                .map(|(i, v)| {
                    let p = &props[usize::from(i)];
                    (i, if v == i64::MIN { p.default } else { v.clamp(p.min, p.max) })
                })
                .collect()
        })
        .collect();
    let start_values = start_values
        .into_iter()
        .map(|(i, v)| {
            let p = &props[usize::from(i)];
            (i, v.clamp(p.min, p.max))
        })
        .collect();
    // Sounds and scripts, the level's by content.
    let mut copy_sounds = Vec::new();
    for (name, bytes) in &asset.sounds {
        let ext = name.rsplit('.').next().unwrap_or("ogg").to_ascii_lowercase();
        let k = match sounds.iter().position(|(e, b)| *e == ext && b == bytes) {
            Some(k) => k,
            None => {
                sounds.push((ext, bytes.clone()));
                sounds.len() - 1
            }
        };
        copy_sounds.push((name.clone(), k as u16));
    }
    let sound = |name: &Option<String>| {
        name.as_ref().and_then(|n| copy_sounds.iter().find(|(x, _)| x == n)).map(|(_, k)| *k)
    };
    let mut copy_scripts = Vec::new();
    if c.default_script
        && let Some(text) = &asset.script
    {
        copy_scripts.push(index_of(scripts, text.clone()));
    }
    if c.script
        && let Some(text) = src.scripts.get(&c.id)
    {
        copy_scripts.push(index_of(scripts, text.clone()));
    }
    // The pictures' boxes and entries; each state's parts.
    let frame = Frame { size: asset.size, art_scale: asset.art_scale, at: (0, 0), to, s };
    let slot_boxes: Vec<(i32, i32, u32, u32)> =
        plan.slots.iter().map(|sl| slot_box(asset, sl, &frame)).collect();
    let boxes: Vec<(i32, i32, u32, u32)> = plan.pictures.iter().map(|(j, _)| slot_boxes[*j]).collect();
    let pictures: Vec<LivePictureFile> = plan
        .pictures
        .iter()
        .zip(&boxes)
        .map(|((j, _), b)| picture_entry(asset, &plan.slots[*j], (b.2, b.3)))
        .collect();
    let live_states = asset
        .states
        .iter()
        .zip(&plan.parts)
        .zip(states_values)
        .map(|((st, row), values)| LiveStateFile {
            name: st.name.clone(),
            values,
            parts: row
                .iter()
                .enumerate()
                .map(|(j, &k)| {
                    let slot = &plan.slots[j];
                    let glows = slot.layers.iter().filter_map(|&l| asset.pieces[l].element.glow.as_ref());
                    let glow_strength = match slot.layer {
                        Some(_) => glows.map(|g| g.strength).max().unwrap_or(0),
                        None => glows.map(|_| 100).max().unwrap_or(0),
                    };
                    let b = slot_boxes[j];
                    LivePartFile {
                        layer: slot.layer,
                        kind: slot.kind,
                        picture: k as u16,
                        at: (b.0, b.1),
                        glow_strength,
                    }
                })
                .collect(),
            enter_sound: sound(&st.enter_sound),
            loop_sound: sound(&st.loop_sound),
        })
        .collect();
    let health = match &asset.health {
        Some(h) => Some(LiveHealthFile {
            points: h.points,
            at_zero: asset.state(&h.at_zero).ok_or_else(|| fail(format!("no state {:?}", h.at_zero)))? as u16,
        }),
        None => None,
    };
    let file = LiveCopyFile {
        id: c.id.clone(),
        asset: asset.name.clone(),
        at: c.at,
        size: to,
        props,
        pictures,
        states: live_states,
        start_state: start as u16,
        start_values,
        animations: anims,
        groups: asset.groups.clone(),
        scripts: copy_scripts,
        sounds: copy_sounds,
        health,
    };
    Ok(Planned { file, plan, boxes })
}

/// The level's played form without pictures: `live.ron` (pictures
/// listed), each live copy's plan and its pictures' boxes, the scripts'
/// texts and the sounds' bytes. `None` without a live copy.
#[allow(clippy::type_complexity)]
fn plan_level(
    src: &PlacedSource,
    assets: &BTreeMap<&str, Asset>,
    s: u32,
) -> Result<Option<(LiveFile, Vec<Planned>, Vec<String>, Vec<Vec<u8>>)>, PlacedError> {
    let live: Vec<&PlacedCopy> = src.file.copies.iter().filter(|c| c.live).collect();
    if live.is_empty() {
        return Ok(None);
    }
    let (mut scripts, mut sounds) = (Vec::new(), Vec::new());
    let mut planned = Vec::with_capacity(live.len());
    for c in live {
        let Some(asset) = assets.get(c.asset.as_str()) else {
            return Err(PlacedError::Copy { copy: c.id.clone(), message: format!("no asset {:?}", c.asset) });
        };
        planned.push(plan_copy(c, asset, s, src, &mut scripts, &mut sounds)?);
    }
    let level_script = src.level_script.as_ref().map(|t| index_of(&mut scripts, t.clone()));
    let file = LiveFile {
        copies: planned.iter().map(|p| p.file.clone()).collect(),
        scripts: (0..scripts.len()).map(live_script_file).collect(),
        level_script,
        sounds: sounds.iter().enumerate().map(|(k, (ext, _))| live_sound_file(k, ext)).collect(),
    };
    let sounds = sounds.into_iter().map(|(_, b)| b.as_ref().clone()).collect();
    Ok(Some((file, planned, scripts, sounds)))
}

/// The pixels of picture `k` of a planned copy of `asset` at art scale
/// `s`: its slot's layers (in the states the picture shows) laid in the
/// copy's frame and baked as a composition in a level of the picture's
/// box (a transparent back tile, the asset's glow groups), then the rock
/// part's front art, glow, groups and beams of its rock cells, or the
/// background part's back art, back glow, groups and beams.
fn make_picture(asset: &Asset, planned: &Planned, k: usize, s: u32) -> LivePicture {
    let (j, owners) = &planned.plan.pictures[k];
    let slot = &planned.plan.slots[*j];
    let entry = planned.file.pictures[k];
    let (bx, by, bw, bh) = planned.boxes[k];
    let frame =
        Frame { size: asset.size, art_scale: asset.art_scale, at: (-bx, -by), to: planned.file.size, s };
    let mut items = Vec::new();
    let mut masks = GlowMasks::new();
    for (n, (&l, owner)) in slot.layers.iter().zip(owners).enumerate() {
        let mut piece = asset.pieces[l].clone();
        if let Some(own) = (*owner).and_then(|st| asset.states[st].own.get(&piece.element.id)) {
            piece.decoded = own.decoded.clone();
            if own.glow_mask.is_some() {
                piece.glow_mask = own.glow_mask.clone();
            }
        }
        let pp = PosedPiece { piece, opacity: 100, offset: (0, 0) };
        let Some((mut e, d, [glow, _, _])) = lay(&pp, &frame) else { continue };
        e.id = format!("p{n}");
        e.visible = true;
        e.folder = None;
        e.distant = None;
        e.clear = false;
        if slot.layer.is_some()
            && let Some(g) = e.glow.as_mut()
        {
            g.strength = 100;
        }
        if let Some(m) = glow {
            masks.insert(e.id.clone(), m);
        }
        items.push((e, d));
    }
    let look = (!asset.groups.is_empty())
        .then(|| LookSource { glow_groups: asset.groups.clone(), ..LookSource::default() });
    let mut level = mini_level((bw, bh), s, look);
    let refs: Vec<(&olr_format::levels::Element, &Decoded)> = items.iter().map(|(e, d)| (e, d)).collect();
    let all = Rect { x: 0, y: 0, w: bw, h: bh };
    bake_elements_with_opacity(
        &mut level,
        &refs,
        &[],
        &masks,
        &PaintedSurfaces::new(),
        &RgbaImage::new(1, 1),
        all,
    );
    let rock = slot.kind == PartKind::Rock;
    let solid = |x: u32, y: u32| level.material[(y * bw + x) as usize] == Material::Rock;
    let (aw, ah) = (bw * s, bh * s);
    let mut art = RgbaImage::new(aw, ah);
    for ay in 0..ah {
        for ax in 0..aw {
            if !rock {
                art.set(ax, ay, level.back.get(ax, ay));
            } else if solid(ax / s, ay / s) {
                art.set(ax, ay, level.front.get(ax, ay));
            }
        }
    }
    let cells = (bw * bh) as usize;
    let world = |f: &dyn Fn(usize) -> u8| -> Vec<u8> {
        (0..cells).map(|i| if !rock || solid(i as u32 % bw, i as u32 / bw) { f(i) } else { 0 }).collect()
    };
    let glow = entry.glow.then(|| match &level.glow {
        Some(g) => (0..(aw * ah) as usize)
            .map(|i| {
                let (ax, ay) = (i as u32 % aw, i as u32 / aw);
                match rock {
                    true if solid(ax / s, ay / s) => g.front[i],
                    true => 0,
                    false => g.back[i],
                }
            })
            .collect(),
        None => vec![0; (aw * ah) as usize],
    });
    let groups = entry.groups.then(|| match &level.glow {
        Some(g) => world(&|i| if rock { g.groups[i] } else { g.groups_back[i] }),
        None => vec![0; cells],
    });
    let beam = entry.beam.then(|| match &level.glow_beams {
        Some(b) => {
            let side = if rock { &b.front } else { &b.back };
            let mut out = vec![0; 2 * cells];
            for i in 0..cells {
                if !rock || solid(i as u32 % bw, i as u32 / bw) {
                    out[2 * i..2 * i + 2].copy_from_slice(&side[2 * i..2 * i + 2]);
                }
            }
            out
        }
        None => vec![0; 2 * cells],
    });
    let rock_mask = rock.then(|| world(&|_| 255));
    LivePicture { art, rock: rock_mask, glow, groups, beam, glass: None }
}

/// A blank level of `size` at art scale `s` with `look`: what a picture
/// is baked in.
fn mini_level(size: (u32, u32), s: u32, look: Option<LookSource>) -> LevelSource {
    LevelSource {
        name: String::new(),
        notes: String::new(),
        size,
        art_scale: s,
        theme: String::new(),
        objects: Vec::new(),
        material: vec![Material::Background; size.0 as usize * size.1 as usize],
        front: RgbaImage::new(size.0 * s, size.1 * s),
        back: RgbaImage::new(size.0 * s, size.1 * s),
        thumb: None,
        look,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
        live: None,
    }
}

/// The played form of `src`'s Live copies in `level` (its size, art scale
/// and look as baked: the level's glow groups and every copy's are at
/// most 255), `assets` the placement's decoded by key; `None` without a
/// Live copy. Each copy planned and placed (the module's doc), its
/// pictures made, then the whole checked (`check_live`: the file and
/// byte caps among the rest), an error naming what broke.
pub fn bake_live(
    src: &PlacedSource,
    assets: &BTreeMap<&str, Asset>,
    level: &LevelSource,
) -> Result<Option<LiveSource>, PlacedError> {
    let s = level.art_scale.max(1);
    let Some((file, planned, scripts, sounds)) = plan_level(src, assets, s)? else { return Ok(None) };
    check_live_budget(&file, s)?;
    let pictures = planned
        .iter()
        .map(|p| {
            let c = src.file.copies.iter().find(|c| c.live && c.id == p.file.id);
            let asset = c.and_then(|c| assets.get(c.asset.as_str()));
            match asset {
                Some(a) => (0..p.file.pictures.len()).map(|k| make_picture(a, p, k, s)).collect(),
                None => Vec::new(),
            }
        })
        .collect();
    let live = LiveSource { file, pictures, scripts, sounds };
    let groups = level.look.as_ref().map_or(0, |l| l.glow_groups.len());
    check_live(&live, level.size, s, groups)?;
    Ok(Some(live))
}

/// Whether `src`'s Live copies' played form keeps within its caps in a
/// level of art scale `s` (`check_live_budget`: at most `MAX_LIVE_FILES`
/// files, `MAX_LIVE_BYTES` decoded): planned from the assets, no picture
/// made (the level editor's Live switch, `placed::set_copies`).
pub fn check_budget(src: &PlacedSource, assets: &BTreeMap<&str, Asset>, s: u32) -> Result<(), PlacedError> {
    let s = s.max(1);
    if let Some((file, ..)) = plan_level(src, assets, s)? {
        check_live_budget(&file, s)?;
    }
    Ok(())
}
