//! The asset tab's properties, health, sounds and script (issue #80; the
//! asset-based levels design's "The asset tab", calls 10, 11, 19 and 21):
//! `olr_edit::asset`'s operations (issue #72) as steps. Each runs on the
//! asset as stored (`Document::to_asset`, the shown state's pictures put
//! back on a copy), and its result goes in as an `Edit::States` of the
//! states' values and sounds (their pictures untouched, so the shown
//! state's form holds) and an `Edit::Parts` of the exposed entries,
//! sounds, script and health; a refusal changes nothing.

use olr_edit::asset::{
    Asset, AssetError, ExposedChange, Offer, exposed_name, offers_of, state_sets, value_in_state,
};
use olr_edit::compose::Rect;
use olr_format::assets::{AssetFile, ExposedSource, HealthSource, PropertySource, ValueSource};

use crate::document::{DocError, Document};
use crate::history::{Edit, PartsSnap};
use crate::ops::NOTHING;

/// A destructible asset's health when switched on.
pub const DEFAULT_HEALTH: u32 = 100;

/// What a new script holds.
pub const NEW_SCRIPT: &str = "// Runs on every live copy of this asset.\n";

fn bad(e: AssetError) -> DocError {
    DocError::Bad(e.to_string())
}

fn not_an_asset() -> DocError {
    DocError::Bad("not an asset".into())
}

/// A property's value in the shown state (the Properties tab's row): the
/// value posed (`olr_edit::asset::value_in_state`: the state's own, else
/// the exposed default, else the layer's own setting; none for an `Own`
/// group's colour) and whether the state sets it itself.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub value: Option<ValueSource>,
    pub own: bool,
}

impl Document {
    /// The exposed entries, sounds, script and health (an asset tab's
    /// document).
    pub fn parts_snap(&self) -> Option<PartsSnap> {
        let p = self.asset.as_deref()?;
        Some(PartsSnap {
            exposed: p.exposed.clone(),
            sounds: p.sounds.clone(),
            script: p.script.clone(),
            health: p.health.clone(),
        })
    }

    /// Sets them (an `Edit::Parts` applied).
    pub(crate) fn set_parts(&mut self, snap: &PartsSnap) {
        let Some(p) = self.asset.as_deref_mut() else { return };
        p.exposed = snap.exposed.clone();
        p.sounds = snap.sounds.clone();
        p.script = snap.script.clone();
        p.health = snap.health.clone();
    }

    /// `asset.ron` as the asset now is, without pixels (an asset tab's
    /// document): what the Properties tab reads.
    pub fn asset_header(&self) -> Option<AssetFile> {
        self.asset.as_deref().map(|p| self.asset_file(p))
    }

    /// What the asset offers to expose or set (`olr_edit::asset::offers`).
    pub fn offers(&self) -> Vec<Offer> {
        self.asset_header().map(|f| offers_of(&f)).unwrap_or_default()
    }

    /// Property `p`'s value in the shown state.
    pub fn shown_value(&self, p: &PropertySource) -> Shown {
        let (Some(f), Some(k)) = (self.asset_header(), self.shown_state()) else {
            return Shown { value: None, own: false };
        };
        Shown { value: value_in_state(&f, k, p), own: state_sets(&f, k, p).is_some() }
    }

    /// The exposed entry of `p` itself, if any.
    pub fn exposed_entry(&self, p: &PropertySource) -> Option<&ExposedSource> {
        self.asset.as_deref()?.exposed.iter().find(|x| x.property == *p)
    }

    /// Whether exposing `p` needs a default asked first: a glow colour of
    /// an `Own` group (it has no one colour) nothing sets at the start.
    pub fn expose_needs_default(&self, p: &PropertySource) -> bool {
        let (Some(f), Some(start)) = (self.asset_header(), self.start_state()) else { return false };
        matches!(p, PropertySource::GlowColour(_)) && value_in_state(&f, start, p).is_none()
    }

    /// Runs one of `olr_edit::asset`'s operations on the asset as stored:
    /// the step's edits (the states' values and sounds, the exposed
    /// entries, sounds, script and health), applied, and the rectangle
    /// rebaked. A refusal changes nothing.
    fn reparts(
        &mut self,
        op: impl FnOnce(&mut Asset) -> Result<(), AssetError>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let (Some(states0), Some(parts0)) = (self.states_snap(), self.parts_snap()) else {
            return Err(not_an_asset());
        };
        let mut asset = self.to_asset();
        op(&mut asset).map_err(bad)?;
        if asset.states.len() != states0.states.len() {
            return Err(DocError::Bad("the states changed".into()));
        }
        let mut states1 = states0.clone();
        for (e, s) in states1.states.iter_mut().zip(&asset.states) {
            e.values = s.values.clone();
            e.loop_sound = s.loop_sound.clone();
            e.enter_sound = s.enter_sound.clone();
        }
        let parts1 = PartsSnap {
            exposed: asset.exposed,
            sounds: asset.sounds,
            script: asset.script,
            health: asset.health,
        };
        let mut edits = Vec::new();
        if states1 != states0 {
            edits.push(Edit::States { before: Box::new(states0), after: Box::new(states1) });
        }
        if parts1 != parts0 {
            edits.push(Edit::Parts { before: Box::new(parts0), after: Box::new(parts1) });
        }
        let mut dirty = NOTHING;
        for e in &edits {
            let r = self.apply(e, true);
            if r.w > 0 && r.h > 0 {
                dirty = Rect::union(Some(dirty), r);
            }
        }
        Ok((edits, dirty))
    }

    /// The shown state's name.
    fn shown_name(&self) -> Result<String, DocError> {
        let k = self.shown_state().ok_or_else(not_an_asset)?;
        Ok(self.states().get(k).map(|s| s.name.clone()).unwrap_or_default())
    }

    /// Sets (`Some`) or clears the shown state's value of `p`: one step.
    pub fn set_value(
        &mut self,
        p: PropertySource,
        value: Option<ValueSource>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let state = self.shown_name()?;
        self.reparts(|a| olr_edit::asset::set_state_value(a, &state, p, value))
    }

    /// Exposes `p` (one of the offers): named after its layer and itself
    /// (`olr_edit::asset::exposed_name`), its default `default` or the
    /// start's value, an `Int`'s `min` and `max` its range but a
    /// position's, which start at the start's value. One step.
    pub fn expose(
        &mut self,
        p: PropertySource,
        default: Option<ValueSource>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let f = self.asset_header().ok_or_else(not_an_asset)?;
        let start = self.start_state().unwrap_or(0);
        let now = value_in_state(&f, start, &p);
        let (min, max) = match (&now, p.is_position()) {
            (Some(ValueSource::Int(n)), true) => (Some(*n), Some(*n)),
            _ => (None, None),
        };
        let default = default.or(now);
        self.reparts(|a| {
            let name = exposed_name(a, &p);
            olr_edit::asset::expose(a, ExposedSource { name, property: p, default, min, max })
        })
    }

    /// Stops exposing `name`: one step.
    pub fn unexpose(&mut self, name: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::unexpose(a, name))
    }

    /// Renames exposed `name`: one step.
    pub fn rename_exposed(&mut self, name: &str, new: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::rename_exposed(a, name, new))
    }

    /// Changes exposed `name`'s default, min or max: one step.
    pub fn set_exposed(&mut self, name: &str, change: ExposedChange) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::set_exposed(a, name, change))
    }

    /// Destructible on ([`DEFAULT_HEALTH`] points, ending in the first
    /// state but the start, else the start) or off (refused while health is
    /// exposed): one step.
    pub fn set_destructible(&mut self, on: bool) -> Result<(Vec<Edit>, Rect), DocError> {
        let health = match on {
            false => None,
            true => {
                let start = self.start_state().unwrap_or(0);
                let states = self.states();
                let st =
                    states.iter().enumerate().find(|(i, _)| *i != start).or(states.iter().enumerate().next());
                let at_zero = st.map(|(_, s)| s.name.clone()).unwrap_or_default();
                Some(HealthSource { points: DEFAULT_HEALTH, at_zero })
            }
        };
        if on && self.asset.as_deref().is_some_and(|p| p.health.is_some()) {
            return Ok((Vec::new(), NOTHING));
        }
        self.reparts(|a| olr_edit::asset::set_health(a, health))
    }

    /// Sets the health's points and its state at 0: one step.
    pub fn set_health(&mut self, h: HealthSource) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::set_health(a, Some(h)))
    }

    /// Adds sound `name`: one step.
    pub fn add_sound(&mut self, name: &str, bytes: Vec<u8>) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::add_sound(a, name, bytes))
    }

    /// Removes sound `name` (refused while a state names it): one step.
    pub fn remove_sound(&mut self, name: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::remove_sound(a, name))
    }

    /// Sets the shown state's loop and enter sounds (`None` leaves one,
    /// `Some(None)` clears it): one step.
    pub fn set_state_sounds(
        &mut self,
        loop_sound: Option<Option<String>>,
        enter_sound: Option<Option<String>>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let state = self.shown_name()?;
        self.reparts(|a| olr_edit::asset::set_state_sounds(a, &state, loop_sound, enter_sound))
    }

    /// Sets (`Some`) or removes the default script: one step. Its syntax
    /// is not checked (issue #80: an asset saves with a broken script).
    pub fn set_script(&mut self, script: Option<String>) -> Result<(Vec<Edit>, Rect), DocError> {
        self.reparts(|a| olr_edit::asset::set_script(a, script))
    }

    /// The default script's syntax errors (`olr_script::check`).
    pub fn script_issues(&self) -> Vec<olr_script::ScriptIssue> {
        self.asset.as_deref().and_then(|p| p.script.as_deref()).map(olr_script::check).unwrap_or_default()
    }

    /// The asset's sound `name`'s bytes.
    pub fn sound(&self, name: &str) -> Option<&std::sync::Arc<Vec<u8>>> {
        self.asset.as_deref()?.sounds.get(name)
    }
}

/// A sound's name in the asset made from file name `file`: lowercased,
/// each character but `a-z`, `0-9`, `_` and `-` made `_`, the stem cut to
/// `MAX_SOUND_STEM` bytes, the extension lowercased (`add_sound` checks
/// the rest: `.wav` or `.ogg`, the name not taken).
pub fn sound_name(file: &str) -> String {
    let (stem, ext) = file.rsplit_once('.').unwrap_or((file, ""));
    let stem: String = stem
        .chars()
        .map(|c| match c.to_ascii_lowercase() {
            c @ ('a'..='z' | '0'..='9' | '_' | '-') => c,
            _ => '_',
        })
        .take(olr_format::assets::MAX_SOUND_STEM)
        .collect();
    format!("{stem}.{}", ext.to_ascii_lowercase())
}
