//! An asset's states and values (issue #71; the format is
//! `olr_format::assets`' states): an asset posed in a state with a copy's
//! values ([`posed`], what a static copy bakes and the previews draw), the
//! properties a layer offers ([`offers`]) and why a copy cannot be Live
//! ([`live_refusal`]).

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_format::assets::{
    AssetFile, PropertySource, PropertyType, StateSource, StateValue, ValueKind, ValueSource, check_value,
    property_key, property_type,
};
use olr_format::levels::{Element, ElementKind, ElementMaterial, element_box};
use olr_format::look::{GlowColourSource, GlowGroupSource, ReachSource};
use olr_format::materials::MaterialSource;

use super::{Asset, AssetError, Piece, shifted};
use crate::compose::Decoded;

/// A drawn layer's picture in a state that paints it itself: its art,
/// cells and back art in its box ([`Decoded::Drawn`]) and its glow mask
/// (its box × art scale), when the layer has one.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnPicture {
    pub decoded: Decoded,
    pub glow_mask: Option<Arc<Vec<u8>>>,
}

/// One of an asset's states decoded: its name, its own pictures by layer
/// id, the values it sets and its sounds.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub name: String,
    pub own: BTreeMap<String, OwnPicture>,
    pub values: Vec<StateValue>,
    pub loop_sound: Option<String>,
    pub enter_sound: Option<String>,
}

impl State {
    /// A state of `name` that owns, sets and plays nothing.
    pub fn new(name: &str) -> State {
        State {
            name: name.into(),
            own: BTreeMap::new(),
            values: Vec::new(),
            loop_sound: None,
            enter_sound: None,
        }
    }

    /// As `asset.ron` holds it (its own layers by id).
    pub fn source(&self) -> StateSource {
        StateSource {
            name: self.name.clone(),
            own: self.own.keys().cloned().collect(),
            values: self.values.clone(),
            loop_sound: self.loop_sound.clone(),
            enter_sound: self.enter_sound.clone(),
        }
    }
}

/// One piece of a posed asset: the piece in asset coordinates, its
/// opacity, percent (100 unless `Opacity` sets it; carried, not applied to
/// the art: the bake mixes the piece's back pixels at it, issue #73), and
/// how far the positions (`OffsetX`/`Y`, then `X`/`Y`, after `Facing`
/// mirrored the box) moved its box, asset pixels: a stretched copy scales
/// this move apart from the box (issue #73).
#[derive(Clone, Debug, PartialEq)]
pub struct PosedPiece {
    pub piece: Piece,
    pub opacity: u8,
    pub offset: (i64, i64),
}

/// An asset in a state with a copy's values ([`posed`]): its pieces bottom
/// to top and its glow groups and materials as they are then.
#[derive(Clone, Debug, PartialEq)]
pub struct Posed {
    pub pieces: Vec<PosedPiece>,
    pub groups: Vec<GlowGroupSource>,
    pub materials: Vec<MaterialSource>,
}

/// `asset.ron`'s fields of `asset` but its folders (elements as the pieces
/// hold them): what the format's property rules read.
pub(crate) fn header(asset: &Asset) -> AssetFile {
    AssetFile {
        art_scale: asset.art_scale,
        folders: asset.folders.clone(),
        glow_groups: asset.groups.clone(),
        materials: asset.materials.clone(),
        states: asset.states.iter().map(State::source).collect(),
        start: (asset.start > 0).then(|| asset.states.get(asset.start).map(|s| s.name.clone())).flatten(),
        exposed: asset.exposed.clone(),
        sounds: asset.sounds.keys().cloned().collect(),
        health: asset.health.clone(),
        ..AssetFile::new(&asset.name, asset.size, asset.pieces.iter().map(|p| p.element.clone()).collect())
    }
}

/// The value of each property `state` of `asset` decides, by
/// `property_key` (a group's settings once): the first of the state's own
/// value, the copy's (`copy`, by exposed name, when the property is
/// exposed), the exposed default; a property no state sets and none
/// exposes keeps the layer's own setting and is not listed. With the
/// property it was given for.
fn resolved(
    file: &AssetFile,
    state: &StateSource,
    copy: &BTreeMap<String, ValueSource>,
) -> BTreeMap<String, (PropertySource, ValueSource)> {
    let mut out: BTreeMap<String, (PropertySource, ValueSource)> = BTreeMap::new();
    for v in &state.values {
        out.entry(property_key(file, &v.property)).or_insert_with(|| (v.property.clone(), v.value.clone()));
    }
    for x in &file.exposed {
        let key = property_key(file, &x.property);
        if out.contains_key(&key) {
            continue;
        }
        if let Some(v) = copy.get(&x.name).or(x.default.as_ref()) {
            out.insert(key, (x.property.clone(), v.clone()));
        }
    }
    out
}

/// The asset as a static copy of it in state `state` (an index of
/// `asset.states`) with copy values `copy` (by exposed name; a copy's
/// value is that copy's default) bakes and the previews draw. A state is a
/// complete look: each property takes the state's own value, else the
/// copy's (exposed only), else the exposed default, else the layer's (or
/// the asset's: shown, not facing) own setting. The state's own pictures
/// replace the starting state's, then the values apply: `OffsetX`/`Y` move
/// a piece, `LayerVisible` false and `Opacity` 0 drop one, `GlowStrength`
/// replaces the glow's strength (the mask kept; 0 removes the glow),
/// `GlowColour` (a `Tint`, a `Filtered` group's kept `Filtered`),
/// `LightIntensity`, `LightReach` and `GlowAnimation` change the group,
/// the beam and glass values theirs; `Facing` then mirrors every piece
/// within the asset's box (a beam's direction `d` to `(180 − d) mod 360`),
/// `X`/`Y` move them all and `Visible` false leaves none. An unknown name
/// or a value of the wrong type or range in `copy` is an error.
pub fn posed(asset: &Asset, state: usize, copy: &BTreeMap<String, ValueSource>) -> Result<Posed, AssetError> {
    posed_with(asset, state, copy, false)
}

/// [`posed`] where things are (issue #79, the asset tab's canvas): with
/// no copy values, and the positions (`X`, `Y`, `OffsetX`/`Y`) and
/// `Facing` ignored, so every piece keeps its box and a stroke lands where
/// its layer is drawn.
pub fn posed_in_place(asset: &Asset, state: usize) -> Result<Posed, AssetError> {
    posed_with(asset, state, &BTreeMap::new(), true)
}

/// [`posed`], the positions and `Facing` ignored when `in_place`.
fn posed_with(
    asset: &Asset,
    state: usize,
    copy: &BTreeMap<String, ValueSource>,
    in_place: bool,
) -> Result<Posed, AssetError> {
    use PropertySource as P;
    use ValueSource as V;
    let st =
        asset.states.get(state).ok_or_else(|| AssetError::Bad(format!("the asset has no state {state}")))?;
    let file = header(asset);
    for (name, v) in copy {
        let x = asset
            .exposed
            .iter()
            .find(|x| x.name == *name)
            .ok_or_else(|| AssetError::Bad(format!("{:?} exposes no value {name:?}", asset.name)))?;
        let t = property_type(&file, &x.property).map_err(AssetError::Bad)?;
        let range = t.range.map(|(lo, hi)| (x.min.unwrap_or(lo), x.max.unwrap_or(hi)));
        check_value(t, range, v, &asset.name, name)?;
    }
    let values = resolved(&file, &st.source(), copy);
    let mut groups = asset.groups.clone();
    let mut pieces: Vec<Option<PosedPiece>> = asset
        .pieces
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if let Some(own) = st.own.get(&p.element.id) {
                p.decoded = own.decoded.clone();
                if own.glow_mask.is_some() {
                    p.glow_mask = own.glow_mask.clone();
                }
            }
            Some(PosedPiece { piece: p, opacity: 100, offset: (0, 0) })
        })
        .collect();
    let (mut facing, mut shown, mut at) = (false, true, (0i64, 0i64));
    for (property, value) in values.values() {
        if in_place && matches!(property, P::X | P::Y | P::Facing | P::OffsetX(_) | P::OffsetY(_)) {
            continue;
        }
        let int = match value {
            V::Int(n) => *n,
            _ => 0,
        };
        let yes = matches!(value, V::Bool(true));
        match property {
            P::X => at.0 = int.into(),
            P::Y => at.1 = int.into(),
            P::Visible => shown = yes,
            P::Facing => facing = yes,
            P::Health => {}
            p => {
                let Some(id) = p.layer() else { continue };
                let Some(slot) =
                    pieces.iter_mut().find(|x| x.as_ref().is_some_and(|x| x.piece.element.id == id))
                else {
                    continue;
                };
                let Some(pp) = slot.as_mut() else { continue };
                let e = &mut pp.piece.element;
                let group = e.glow.as_ref().and_then(|g| groups.iter_mut().find(|x| x.id == g.group));
                match (p, value) {
                    (P::OffsetX(_), _) => {
                        pp.piece = shifted(pp.piece.clone(), int.into(), 0);
                        pp.offset.0 += i64::from(int);
                    }
                    (P::OffsetY(_), _) => {
                        pp.piece = shifted(pp.piece.clone(), 0, int.into());
                        pp.offset.1 += i64::from(int);
                    }
                    (P::LayerVisible(_), _) if !yes => *slot = None,
                    (P::Opacity(_), _) if int <= 0 => *slot = None,
                    (P::Opacity(_), _) => pp.opacity = int.clamp(0, 100) as u8,
                    (P::GlowStrength(_), _) if int <= 0 => {
                        e.glow = None;
                        pp.piece.glow_mask = None;
                    }
                    (P::GlowStrength(_), _) => {
                        if let Some(g) = e.glow.as_mut() {
                            g.strength = int.clamp(1, 100) as u8;
                        }
                    }
                    (P::GlowColour(_), V::Colour(c)) => {
                        if let Some(g) = group {
                            g.colour = match g.colour {
                                GlowColourSource::Filtered(_) => GlowColourSource::Filtered(*c),
                                _ => GlowColourSource::Tint(*c),
                            };
                        }
                    }
                    (P::LightIntensity(_), _) => {
                        if let Some(g) = group {
                            g.intensity = int.clamp(0, i32::from(u16::MAX)) as u16;
                        }
                    }
                    (P::LightReach(_), _) => {
                        if let Some(g) = group
                            && matches!(g.reach, ReachSource::Pixels(_))
                        {
                            g.reach = ReachSource::Pixels(int.clamp(1, i32::from(u16::MAX)) as u16);
                        }
                    }
                    (P::GlowAnimation(_), V::Animation(a)) => {
                        if let Some(g) = group {
                            g.animation = a.clone();
                        }
                    }
                    (P::BeamDirection(_), _) => {
                        if let Some(b) = e.glow.as_mut().and_then(|g| g.beam.as_mut()) {
                            b.direction = int.rem_euclid(360) as u16;
                        }
                    }
                    (P::BeamSpread(_), _) => {
                        if let Some(b) = e.glow.as_mut().and_then(|g| g.beam.as_mut()) {
                            b.spread = int.clamp(1, 90) as u8;
                        }
                    }
                    (P::GlassOpacity(_), _) => {
                        if let Some(g) = e.glass.as_mut() {
                            g.opacity = int.clamp(1, 99) as u8;
                        }
                    }
                    (P::GlassTint(_), _) => {
                        if let Some(g) = e.glass.as_mut() {
                            g.tint = int.clamp(0, 100) as u8;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut out: Vec<PosedPiece> = pieces.into_iter().flatten().collect();
    if !shown {
        out.clear();
    }
    if facing {
        for p in &mut out {
            p.piece = mirrored(&p.piece, asset.size, asset.art_scale);
            p.offset.0 = -p.offset.0;
        }
    }
    if at != (0, 0) {
        for p in &mut out {
            p.piece = shifted(p.piece.clone(), at.0, at.1);
            p.offset = (p.offset.0 + at.0, p.offset.1 + at.1);
        }
    }
    Ok(Posed { pieces: out, groups, materials: asset.materials.clone() })
}

/// `img` mirrored left to right.
fn flip_image(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let mut out = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set(w - 1 - x, y, img.get(x, y));
        }
    }
    out
}

/// Rows of `width` values each mirrored left to right (a grid whose
/// length is not a multiple of `width` is left as it is).
fn flip_rows<T: Clone>(v: &[T], width: u32) -> Vec<T> {
    let w = width as usize;
    if w == 0 || !v.len().is_multiple_of(w) {
        return v.to_vec();
    }
    v.chunks(w).flat_map(|row| row.iter().rev().cloned()).collect()
}

/// `p` mirrored left to right within an asset of `size` at art scale `s`:
/// its box, art, cells, back art, glow mask and surface maps, a beam's
/// direction `d` turned to `(180 − d) mod 360`. An image's processed grid
/// is mirrored (its original and mask are not).
fn mirrored(p: &Piece, size: (u32, u32), s: u32) -> Piece {
    let mut p = p.clone();
    let boxed = element_box(&p.element.kind, size);
    let width = boxed.map_or(size.0, |(_, b)| b.0);
    match &mut p.element.kind {
        ElementKind::Image { at, size: b, .. } => at.0 = mirror_x(at.0, b.0, size.0),
        ElementKind::Drawn { at, .. } => at.0 = mirror_x(at.0, width, size.0),
        ElementKind::Raster => {}
    }
    p.decoded = match &p.decoded {
        Decoded::Image { grid, mask } => {
            Decoded::Image { grid: grid.as_ref().map(|g| Arc::new(flip_image(g))), mask: mask.clone() }
        }
        Decoded::Drawn { art, cells, back } => Decoded::Drawn {
            art: flip_image(art),
            cells: cells.as_ref().map(|c| flip_rows(c, width)),
            back: back.as_ref().map(flip_image),
        },
        Decoded::Raster { material, front, back } => Decoded::Raster {
            material: flip_rows(material, size.0),
            front: flip_image(front),
            back: flip_image(back),
        },
    };
    let art_width = width.saturating_mul(s);
    p.glow_mask = p.glow_mask.map(|m| Arc::new(flip_rows(&m, art_width)));
    p.surface_map = p.surface_map.map(|m| Arc::new(flip_rows(&m, width)));
    p.back_surface_map = p.back_surface_map.map(|m| Arc::new(flip_rows(&m, width)));
    if let Some(b) = p.element.glow.as_mut().and_then(|g| g.beam.as_mut()) {
        b.direction = ((180 - i32::from(b.direction)).rem_euclid(360)) as u16;
    }
    p
}

/// A box at `x` of `w` mirrored within `0..side`.
fn mirror_x(x: i32, w: u32, side: u32) -> i32 {
    (i64::from(side) - i64::from(x) - i64::from(w)).clamp(i32::MIN.into(), i32::MAX.into()) as i32
}

/// A property an asset's layer (or the asset itself) offers to expose or
/// set in a state: its value type and range (an `Int`'s), and the other
/// layers sharing its glow group (a group property changes them too).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub property: PropertySource,
    pub kind: ValueKind,
    pub range: Option<(i32, i32)>,
    pub sharing: Vec<String>,
}

/// Every property of `asset` that can be exposed or set (the design's
/// table; the editor's tree and the CLI's checks share it): the asset's
/// own (`X`, `Y`, `Visible`, `Facing`, `Health` with health), then each
/// layer's, bottom to top, in the table's order. Not offered: Dirt layers,
/// rasters, distant layers, material, scale and rotation.
pub fn offers(asset: &Asset) -> Vec<Offer> {
    offers_of(&header(asset))
}

/// [`offers`] of an asset's `asset.ron` (issue #80: the asset tab reads
/// it without the pixels).
pub fn offers_of(file: &AssetFile) -> Vec<Offer> {
    use PropertySource as P;
    let mut all = vec![P::X, P::Y, P::Visible, P::Facing, P::Health];
    for e in &file.elements {
        let id = e.id.clone();
        all.extend([
            P::OffsetX(id.clone()),
            P::OffsetY(id.clone()),
            P::LayerVisible(id.clone()),
            P::Opacity(id.clone()),
            P::GlowStrength(id.clone()),
            P::GlowColour(id.clone()),
            P::LightIntensity(id.clone()),
            P::LightReach(id.clone()),
            P::GlowAnimation(id.clone()),
            P::BeamDirection(id.clone()),
            P::BeamSpread(id.clone()),
            P::GlassOpacity(id.clone()),
            P::GlassTint(id),
        ]);
    }
    all.into_iter()
        .filter_map(|property| {
            let PropertyType { kind, range } = property_type(file, &property).ok()?;
            let sharing = sharing(file, &property);
            Some(Offer { property, kind, range, sharing })
        })
        .collect()
}

/// The other layers glowing in the group of group property `p`'s layer.
fn sharing(file: &AssetFile, p: &PropertySource) -> Vec<String> {
    let Some(id) = p.layer().filter(|_| p.of_group()) else { return Vec::new() };
    let group = file.elements.iter().find(|e| e.id == id).and_then(|e| e.glow.as_ref());
    let Some(group) = group.map(|g| g.group.as_str()) else { return Vec::new() };
    file.elements
        .iter()
        .filter(|e| e.id != id && e.glow.as_ref().is_some_and(|g| g.group == group))
        .map(|e| e.id.clone())
        .collect()
}

/// The value state `state` of `file` sets `p` to itself (by
/// `property_key`: a group's setting through any of its layers), if it
/// does.
pub fn state_sets<'a>(file: &'a AssetFile, state: usize, p: &PropertySource) -> Option<&'a StateValue> {
    let key = property_key(file, p);
    file.states.get(state)?.values.iter().find(|v| property_key(file, &v.property) == key)
}

/// The value `p` takes in state `state` of `file` with no copy's values
/// (issue #80, the asset tab's Properties; [`posed`]'s order): the
/// state's own value, else the exposed default (by `property_key`), else
/// the layer's (or the asset's) own setting; none for an `Own` group's
/// colour nothing sets.
pub fn value_in_state(file: &AssetFile, state: usize, p: &PropertySource) -> Option<ValueSource> {
    let key = property_key(file, p);
    state_sets(file, state, p)
        .map(|v| v.value.clone())
        .or_else(|| {
            file.exposed
                .iter()
                .find(|x| property_key(file, &x.property) == key)
                .and_then(|x| x.default.clone())
        })
        .or_else(|| olr_format::assets::own_value(file, p))
}

/// Why a copy of an asset cannot be Live (the design's call 2; #84's
/// switch hides itself for the first, disables itself for the others).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveRefusal {
    /// A single state, no exposed values, no sounds and no script.
    NothingToDrive,
    /// A Dirt layer, or a mixed layer with a dirt cell.
    Dirt,
    /// A raster or a distant layer.
    RasterOrDistant,
    /// Issue #85: a layer wholly outside the asset's box (by id).
    Outside(String),
    /// Issue #85: more parts in a state than `MAX_PARTS` (32).
    TooManyParts(usize),
    /// Issue #85: more pictures than `MAX_PICTURES` (128).
    TooManyPictures(usize),
    /// Issue #85: more values than `MAX_LIVE_PROPS` (96).
    TooManyValues(usize),
}

impl std::fmt::Display for LiveRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            LiveRefusal::NothingToDrive => {
                "it has a single state, no exposed values, no sounds and no script"
            }
            LiveRefusal::Dirt => "it holds dirt",
            LiveRefusal::RasterOrDistant => "it holds a raster or distant layer",
            LiveRefusal::Outside(id) => return write!(f, "its layer {id:?} lies outside it"),
            LiveRefusal::TooManyParts(n) => {
                return write!(f, "it plays as {n} parts, more than {}", olr_format::levels::MAX_PARTS);
            }
            LiveRefusal::TooManyPictures(n) => {
                return write!(f, "it needs {n} pictures, more than {}", olr_format::levels::MAX_PICTURES);
            }
            LiveRefusal::TooManyValues(n) => {
                return write!(f, "it has {n} values, more than {}", olr_format::levels::MAX_LIVE_PROPS);
            }
        })
    }
}

/// Whether `asset` drives anything: more than one state, exposed values,
/// sounds or a script.
pub fn drives(asset: &Asset) -> bool {
    asset.states.len() > 1 || !asset.exposed.is_empty() || !asset.sounds.is_empty() || asset.script.is_some()
}

/// Whether `e` is a plain Dirt image or drawn layer (a mixed layer's cells
/// decide for it).
pub(super) fn plain_dirt(e: &Element) -> bool {
    matches!(
        e.kind,
        ElementKind::Image { material: ElementMaterial::Dirt, .. }
            | ElementKind::Drawn { material: ElementMaterial::Dirt, mixed: false, .. }
    )
}

/// Why a copy of `asset` cannot be Live, if it cannot: nothing to drive
/// first, then dirt (a plain Dirt layer, or a dirt cell of a mixed layer in
/// any state), then a raster or distant layer, then (issue #85, the played
/// form's plan, `crate::live`) a layer wholly outside the asset's box and
/// the parts', pictures' and values' limits.
pub fn live_refusal(asset: &Asset) -> Option<LiveRefusal> {
    if !drives(asset) {
        return Some(LiveRefusal::NothingToDrive);
    }
    let dirt_cells = |d: &Decoded| match d {
        Decoded::Drawn { cells: Some(c), .. } => c.contains(&Material::Dirt),
        _ => false,
    };
    let dirt = asset.pieces.iter().any(|p| {
        plain_dirt(&p.element)
            || (matches!(p.element.kind, ElementKind::Drawn { mixed: true, .. })
                && (dirt_cells(&p.decoded)
                    || asset
                        .states
                        .iter()
                        .filter_map(|s| s.own.get(&p.element.id))
                        .any(|o| dirt_cells(&o.decoded))))
    });
    if dirt {
        return Some(LiveRefusal::Dirt);
    }
    if asset.pieces.iter().any(|p| matches!(p.element.kind, ElementKind::Raster) || p.element.is_distant()) {
        return Some(LiveRefusal::RasterOrDistant);
    }
    crate::live::plan(asset).err()
}

/// What a landing (`land`, the editor's placing; until #82 makes it a
/// copy, `placed::add_copy`) says of an asset whose states, values, sounds,
/// health or script it leaves behind; none for an asset without them.
pub fn left_behind(asset: &Asset) -> Option<String> {
    (drives(asset) || asset.health.is_some())
        .then(|| format!("{}'s states, values, sounds and script stay in the asset", asset.name))
}

/// The pieces and glow groups a landing places: the asset in its starting
/// state posed with its defaults (an `Opacity` carried no further).
pub fn landed_pieces(asset: &Asset) -> Result<(Vec<Piece>, Vec<GlowGroupSource>), AssetError> {
    let p = posed(asset, asset.start, &BTreeMap::new())?;
    Ok((p.pieces.into_iter().map(|p| p.piece).collect(), p.groups))
}
