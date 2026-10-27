//! The copy panel's document side (issue #84; the asset-based levels
//! design's "The level tab", calls 2, 10 and 20): what a placed copy's
//! panel offers (`CopyFacts`: Live per call 2 through `olr_edit::asset::
//! live_refusal`, the starting states, the exposed values with their
//! types and limits, the asset's default script, Distant), the copies'
//! and the level's scripts set as one step each (`olr_edit::placed::
//! set_copy_script`, `set_level_script`), and the scripts' syntax errors
//! as the level's warnings (`olr_script::check`; nothing runs them yet).

use std::sync::Arc;

use olr_edit::asset::{Asset, LiveRefusal, decode, decode_own, live_refusal, value_in_state};
use olr_edit::library::{self, Libraries, LibraryError, Updated};
use olr_format::assets::{AssetFile, ValueKind, ValueSource, property_type};
use olr_format::levels::{AssetOrigin, GROUND_KEY, distant_refusal};
use olr_pack::Severity;

use crate::document::{DocError, Document, Problem};
use crate::ops::{Change, DEFAULT_DEPTH};

/// The Live switch's reason on an asset holding dirt (call 2).
pub const LIVE_DIRT: &str = "it holds dirt: only rock and background can be live";

/// The Live switch's reason on a distant copy.
pub const LIVE_DISTANT: &str = "a distant copy is not live: switch Distant off first";

/// The Distant switch's reason on a Live copy.
pub const DISTANT_LIVE: &str = "a live copy is not distant: switch Live off first";

/// The default script a New script starts with.
pub const NEW_COPY_SCRIPT: &str = "// This copy's script: it runs while the copy is Live.\n";

/// The level script a New script starts with.
pub const NEW_LEVEL_SCRIPT: &str = "// The level's script: it may drive the level's copies by their ids.\n";

/// How the copy panel shows the Live switch (call 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveOffer {
    /// Not shown: the asset has nothing to drive.
    Hidden,
    /// Shown off and disabled, the reason as hover text and under it.
    Disabled(String),
    /// A switch.
    Offered,
}

/// One exposed property of a copy's asset, in the asset's order.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueRow {
    /// Its exposed name (what the copy's `values` key it by).
    pub name: String,
    pub kind: ValueKind,
    /// An `Int`'s limits (the exposed `min` and `max`, else its range).
    pub range: Option<(i32, i32)>,
    /// The value the copy starts with: its own, else the asset's in the
    /// copy's starting state (its value there, the exposed default, the
    /// layer's own); none for an `Own` group's colour without either.
    pub value: Option<ValueSource>,
    /// Whether the copy sets it itself.
    pub set: bool,
}

/// What a placed copy's panel offers (issue #84).
#[derive(Clone, Debug, PartialEq)]
pub struct CopyFacts {
    /// Its asset's name.
    pub asset_name: String,
    pub live: LiveOffer,
    /// The asset's states, in order, and the starting state's name.
    pub states: Vec<String>,
    pub start: String,
    pub values: Vec<ValueRow>,
    /// Whether the asset has a default script.
    pub default_script: bool,
    /// The Distant switch: `None` hidden (the asset holds more than
    /// Background images and plain drawn layers), `Some(None)` offered,
    /// `Some(Some(why))` shown disabled.
    pub distant: Option<Option<String>>,
}

/// Where a script of the level lives (issue #84): a copy's own
/// (`editor/scripts/<id>.rhai`) or the level's (`editor/level.rhai`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptTarget {
    Copy(String),
    Level,
}

impl ScriptTarget {
    /// Its file in the pack, as the script editor's title shows it.
    pub fn file(&self) -> String {
        match self {
            ScriptTarget::Copy(id) => olr_format::levels::copy_script_file(id),
            ScriptTarget::Level => olr_format::levels::LEVEL_SCRIPT_FILE.to_string(),
        }
    }

    /// Its name in the problems ("crane's script", "the level script").
    pub fn said(&self) -> String {
        match self {
            ScriptTarget::Copy(id) => format!("{id}'s script"),
            ScriptTarget::Level => "the level script".into(),
        }
    }
}

/// A script's syntax errors as warnings: "crane's script, line 3: …".
fn script_warnings(whose: &str, text: &str, out: &mut Vec<Problem>) {
    for i in olr_script::check(text) {
        out.push(Problem {
            severity: Severity::Warning,
            text: format!("{whose}, line {}: {}", i.line, i.message),
        });
    }
}

impl Document {
    /// The level's asset `key` decoded: from the copies' cache, else now
    /// (`Own` ones as a level's own).
    fn asset_decoded(&self, key: &str) -> Option<Arc<Asset>> {
        if let Some(a) = self.copy_cache.decoded.get(key) {
            return Some(a.clone());
        }
        let p = self.placement()?;
        let src = p.assets.get(key)?;
        let own = p.file.assets.iter().any(|a| a.key == key && a.origin == AssetOrigin::Own);
        let asset = if own { decode_own(src) } else { decode(src) };
        asset.ok().map(Arc::new)
    }

    /// The header of copy `id`'s asset.
    pub fn copy_asset_file(&self, id: &str) -> Option<&AssetFile> {
        let p = self.placement()?;
        let c = p.file.copies.iter().find(|c| c.id == id)?;
        p.assets.get(&c.asset).map(|a| &a.file)
    }

    /// What copy `id`'s panel offers; `None` for no such copy or the
    /// ground (its panel is its own).
    pub fn copy_facts(&self, id: &str) -> Option<CopyFacts> {
        let p = self.placement()?;
        let c = p.file.copies.iter().find(|c| c.id == id)?;
        if c.asset == GROUND_KEY {
            return None;
        }
        let src = p.assets.get(&c.asset)?;
        let file = &src.file;
        let refusal = match self.asset_decoded(&c.asset) {
            Some(a) => live_refusal(&a),
            None => Some(LiveRefusal::NothingToDrive),
        };
        // A copy Live already (a hand-made file) can always be switched off.
        let live = match refusal {
            Some(LiveRefusal::NothingToDrive) if !c.live => LiveOffer::Hidden,
            _ if c.live => LiveOffer::Offered,
            Some(LiveRefusal::Dirt) => LiveOffer::Disabled(LIVE_DIRT.into()),
            Some(r) => LiveOffer::Disabled(r.to_string()),
            None if c.distant.is_some() => LiveOffer::Disabled(LIVE_DISTANT.into()),
            None => LiveOffer::Offered,
        };
        let states: Vec<String> = file.states.iter().map(|s| s.name.clone()).collect();
        let start = file.start.clone().or_else(|| states.first().cloned()).unwrap_or_default();
        // The state the copy starts in, by index.
        let shown = c.state.as_ref().unwrap_or(&start);
        let state = states.iter().position(|s| s == shown).unwrap_or(0);
        let values = file
            .exposed
            .iter()
            .filter_map(|x| {
                let t = property_type(file, &x.property).ok()?;
                let range = t.range.map(|(lo, hi)| (x.min.unwrap_or(lo), x.max.unwrap_or(hi)));
                let own = c.values.get(&x.name).cloned();
                let set = own.is_some();
                let value = own.or_else(|| value_in_state(file, state, &x.property));
                Some(ValueRow { name: x.name.clone(), kind: t.kind, range, value, set })
            })
            .collect();
        let distant = distant_refusal(file, c.distant.unwrap_or(DEFAULT_DEPTH)).is_none().then(|| {
            // Distant is offered on a near copy only while it is not Live.
            (c.live && c.distant.is_none()).then(|| DISTANT_LIVE.to_string())
        });
        Some(CopyFacts {
            asset_name: file.name.clone(),
            live,
            states,
            start,
            values,
            default_script: src.script.is_some(),
            distant,
        })
    }

    /// The script of `target`, if it has one.
    pub fn script_of(&self, target: &ScriptTarget) -> Option<&str> {
        let p = self.placement()?;
        match target {
            ScriptTarget::Copy(id) => p.scripts.get(id).map(String::as_str),
            ScriptTarget::Level => p.level_script.as_deref(),
        }
    }

    /// Sets (`Some`) or removes `target`'s script: one step.
    pub(crate) fn set_target_script(
        &mut self,
        target: &ScriptTarget,
        script: Option<String>,
    ) -> Result<Change, DocError> {
        let ((), edit) = match target {
            ScriptTarget::Copy(id) => {
                self.placement_op(|pack| olr_edit::placed::set_copy_script(pack, id, script))?
            }
            ScriptTarget::Level => {
                self.placement_op(|pack| olr_edit::placed::set_level_script(pack, script))?
            }
        };
        Ok(self.change(edit))
    }

    /// The syntax errors of the level's scripts as warnings (issue #84):
    /// each copy's, the level's, and the default scripts of the level's
    /// assets, by script and line.
    pub fn script_problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let Some(p) = self.placement() else { return out };
        for c in p.file.copies.iter().filter(|c| c.script) {
            if let Some(text) = p.scripts.get(&c.id) {
                script_warnings(&ScriptTarget::Copy(c.id.clone()).said(), text, &mut out);
            }
        }
        if let Some(text) = &p.level_script {
            script_warnings(&ScriptTarget::Level.said(), text, &mut out);
        }
        for a in &p.file.assets {
            if let Some(src) = p.assets.get(&a.key)
                && let Some(text) = &src.script
            {
                script_warnings(&format!("{}'s default script", src.file.name), text, &mut out);
            }
        }
        out
    }

    /// The level's asset `key` takes the library's version in `libs`
    /// whatever it breaks (`olr_edit::library::update` forced: each break
    /// resolved): one step, and what it resolved.
    pub(crate) fn force_update(
        &mut self,
        key: &str,
        libs: &Libraries,
    ) -> Result<(Change, Updated), DocError> {
        let (updated, edit) = self.placement_op_with(|pack| {
            let updated = library::update(pack, &[key.to_string()], true, libs)?;
            crate::links::merge_same(pack);
            Ok::<Updated, LibraryError>(updated)
        })?;
        Ok((self.change(edit), updated))
    }

    /// The level's copy of asset `key` as the library would take it back
    /// (Add to your library: call 17), decoded.
    pub fn restorable(&self, key: &str) -> Result<Asset, String> {
        let p = self.placement().ok_or("a plain level has no library assets")?;
        let src = p.assets.get(key).ok_or_else(|| format!("the level has no asset {key:?}"))?;
        decode(src).map_err(|e| e.to_string())
    }
}

impl crate::state::EditorState {
    /// The script the level tab's middle shows (issue #84), while it is
    /// there (an undo may take it, or its copy).
    pub fn script_open(&self) -> Option<&ScriptTarget> {
        let t = self.script_open.as_ref()?;
        let doc = self.doc.as_ref()?;
        let copy = match t {
            ScriptTarget::Copy(id) => doc.placed_copy(id).is_some(),
            ScriptTarget::Level => true,
        };
        (copy && doc.script_of(t).is_some()).then_some(t)
    }

    /// New script / Edit script: `target`'s script in the level tab's
    /// middle, a new one made first (one step; a locked copy refuses).
    pub(crate) fn open_script(&mut self, target: ScriptTarget) -> Vec<crate::state::EditorCommand> {
        if self.shown != crate::state::DocTab::Level {
            return Vec::new();
        }
        let Some(doc) = &self.doc else { return Vec::new() };
        if let ScriptTarget::Copy(id) = &target
            && (doc.placed_copy(id).is_none() || doc.is_ground_copy(id))
        {
            return Vec::new();
        }
        let mut cmds = Vec::new();
        if doc.script_of(&target).is_none() {
            let text = match target {
                ScriptTarget::Copy(_) => NEW_COPY_SCRIPT,
                ScriptTarget::Level => NEW_LEVEL_SCRIPT,
            };
            let t = target.clone();
            match self.copy_edit_named(None, |d| d.set_target_script(&t, Some(text.into())).map(|c| (c, ())))
            {
                Some((c, ())) => cmds = c,
                None => return Vec::new(),
            }
        }
        self.script_open = Some(target);
        cmds
    }

    /// The script editor's text of `target` committed: one step (none when
    /// the script went meanwhile: an undo, Remove script).
    pub(crate) fn commit_script(
        &mut self,
        target: &ScriptTarget,
        text: String,
    ) -> Vec<crate::state::EditorCommand> {
        if self.shown != crate::state::DocTab::Level
            || self.doc.as_ref().is_none_or(|d| d.script_of(target).is_none())
        {
            return Vec::new();
        }
        self.copy_edit_named(None, |d| d.set_target_script(target, Some(text)).map(|c| (c, ())))
            .map(|r| r.0)
            .unwrap_or_default()
    }

    /// Remove script, answered: `target`'s script removed, one step; the
    /// editor on it closes.
    pub(crate) fn remove_script(&mut self, target: &ScriptTarget) -> Vec<crate::state::EditorCommand> {
        let cmds = self
            .copy_edit_named(None, |d| d.set_target_script(target, None).map(|c| (c, ())))
            .map(|r| r.0)
            .unwrap_or_default();
        if !cmds.is_empty() && self.script_open.as_ref() == Some(target) {
            self.script_open = None;
        }
        cmds
    }
}
