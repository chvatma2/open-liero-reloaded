//! The material editor's state (issue #44): the library as the host listed
//! it (the built-in materials, read-only, then the user's), the material
//! open with its undo history, the dialog, the status. `update` is pure:
//! every disk change is a [`MaterialCommand`] the host carries out and
//! answers (`Created`, `Saved`, `Renamed`, `Deleted`, `Failed`).
//!
//! New and Copy write the new material at once (the host's `Create`), as
//! the mod editor's Edit a copy makes its pack: the open material is then
//! always a file of the user's library (or a built-in one, read-only), but
//! for a recovered one whose file is gone, which Save writes again. Every
//! field is clamped to its bounds as it is set and a ramp key's
//! temperature to between its neighbours', so an edited material is valid;
//! Save still checks (`olr_format::materials::compile_material`).
//!
//! Issue #45: the live preview's controls (`MaterialAction::Preview`) act
//! whatever is open, dialogs too, and ask the host's preview through
//! `MaterialCommand::Preview`; the material compared follows the library
//! (renamed, saved, deleted: `SetCompare` again).

use crate::preview::{PreviewAction, PreviewCommand, PreviewSettings};
use olr_data::image::Rgba;
use olr_edit::material::MaterialEntry;
use olr_format::Colour;
use olr_format::materials::{
    HeatLookSource, HeatSource, MAX_MATERIAL_ID, MAX_MATERIAL_NAME, MaterialSource, RampKeySource,
    compile_material, material_file, valid_material_id,
};
use olr_look::{
    LAVA_RAMP, MAX_CAPACITY, MAX_CONDUCTIVITY, MAX_COOLING, MAX_GLOW, MAX_RAMP_KEYS, MAX_SENSITIVITY,
    MAX_TEMPERATURE, MIN_CAPACITY, MIN_COOLING, RampKey, ramp_at,
};

/// The most undo steps kept.
pub const MAX_STEPS: usize = 200;
/// The most ids [`free_id`] tries (a library holds far fewer).
pub const MAX_ID_TRIES: usize = 1 << 16;

/// A material of the library: built-in or the user's, by id (the user's
/// library may hold an id a built-in one has).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    pub builtin: bool,
    pub id: String,
}

impl Key {
    pub fn user(id: &str) -> Key {
        Key { builtin: false, id: id.to_string() }
    }

    pub fn builtin(id: &str) -> Key {
        Key { builtin: true, id: id.to_string() }
    }
}

/// What the name dialog is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A new material (the stone's heat to start from).
    New,
    /// A copy of the material open (Copy, a built-in's Edit a copy).
    Copy,
    /// The material open renamed: its name and its file (`<id>.ron`).
    Rename,
}

/// What waits for an answer to the unsaved changes' question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Another material opened.
    Select(Key),
    /// A new material written (New, Copy).
    Create(MaterialSource),
    /// The editor closed.
    Close,
}

/// A question the editor asks; while one is open only its answers, the
/// host's answers and the view act.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// A name: a new material's, a copy's, or the open one's new name.
    Name(Purpose),
    /// Delete the open material?
    Delete,
    /// The open material has unsaved changes: save them first?
    Unsaved(Next),
    /// The host's error.
    Error(String),
}

/// An answer to [`Dialog::Unsaved`] (Cancel also closes the others).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Save,
    DontSave,
    Cancel,
}

/// A change to the open material; numbers are clamped to their bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Notes(String),
    /// Whether it heats at all (on: stone's heat).
    Heats(bool),
    Sensitivity(i64),
    Capacity(i64),
    Conductivity(i64),
    Cooling(i64),
    Light(i64),
    /// Its own colour ramp (on: the engine's lava ramp's keys to start
    /// from) or the engine's (off).
    OwnRamp(bool),
    /// Key `i`'s temperature, kept between its neighbours'.
    KeyTemperature(usize, i64),
    KeyColour(usize, [u8; 3]),
    KeyGlow(usize, i64),
    /// A key added in the widest gap of the ramp, its colour and glow the
    /// ramp's there.
    AddKey,
    RemoveKey(usize),
}

/// What the panels and the host ask of the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialAction {
    /// Open this material of the library (asks first when the open one has
    /// unsaved changes).
    Select(Key),
    /// New…, Copy… (and a built-in's Edit a copy), Rename…, Delete…: their
    /// dialogs.
    New,
    Copy,
    Rename,
    Delete,
    /// The name dialog's OK.
    Named(String),
    /// The delete dialog's Delete.
    ConfirmDelete,
    Choice(Choice),
    Set(Edit),
    Undo,
    Redo,
    Save,
    Close,
    // The host's answers.
    /// A new material was written to the user's library.
    Created(MaterialSource),
    /// The open material was written as this.
    Saved(MaterialSource),
    /// The user's material `from` is now `to` (its file renamed).
    Renamed {
        from: String,
        to: MaterialSource,
    },
    /// The user's material was deleted.
    Deleted(String),
    /// A command failed: an error dialog.
    Failed(String),
    /// Something to say in the status bar.
    Note(String),
    /// The live preview's controls (issue #45).
    Preview(PreviewAction),
}

/// What the editor asks the host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MaterialCommand {
    /// Write this new material to the user's library (never over a file
    /// there); answer `Created`.
    Create(MaterialSource),
    /// Write the open material, checked, to the user's library (`<id>.ron`);
    /// answer `Saved`.
    Save(MaterialSource),
    /// The user's material `from` written as `to` (its id and name
    /// changed), the old file removed; answer `Renamed`.
    Rename { from: String, to: MaterialSource },
    /// Remove the user's material; answer `Deleted`.
    Delete(String),
    /// The editor is done (after any unsaved changes' question).
    Close,
    /// The live preview's (issue #45); no answer.
    Preview(PreviewCommand),
}

/// The material open and its history.
#[derive(Clone, Debug)]
struct Open {
    key: Key,
    /// As written in the library; `None`: a recovered material whose file
    /// is gone.
    saved: Option<MaterialSource>,
    doc: MaterialSource,
    undo: Vec<MaterialSource>,
    redo: Vec<MaterialSource>,
}

/// The material editor.
#[derive(Clone, Debug)]
pub struct MaterialEditorState {
    library: Vec<MaterialEntry>,
    open: Option<Open>,
    dialog: Option<Dialog>,
    /// What runs once the save the unsaved changes' question asked for is
    /// answered.
    pending: Option<Next>,
    status: String,
    /// Moves on with every change of the open material or its identity
    /// (the panels drop their drafts).
    generation: u64,
    /// The live preview's controls (issue #45).
    preview: PreviewSettings,
}

/// A new material named `name`, id `id`: stone's heat, the engine's ramp.
pub fn new_material(id: &str, name: &str) -> MaterialSource {
    MaterialSource {
        id: id.to_string(),
        name: name.to_string(),
        notes: String::new(),
        heat: Some(stone_heat()),
    }
}

/// The engine's reference material's heat (stone: every value 100).
fn stone_heat() -> HeatSource {
    HeatSource {
        sensitivity: 100,
        capacity: 100,
        conductivity: 100,
        cooling: 100,
        look: HeatLookSource::default(),
    }
}

/// An id for a material named `name`: its letters and digits lowercase,
/// the rest `-`, at most [`MAX_MATERIAL_ID`] bytes (`material` when none
/// is left), with `-2`, `-3`, … until `taken` refuses it no more and it is
/// a valid, portable id; empty (no material's id) when [`MAX_ID_TRIES`]
/// are all taken.
pub fn free_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base = String::new();
    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            base.push(c);
        } else if !base.ends_with('-') {
            base.push('-');
        }
    }
    base.truncate(MAX_MATERIAL_ID);
    let base = match base.trim_matches('-') {
        "" => "material".to_string(),
        b => b.to_string(),
    };
    std::iter::once(base.clone())
        .chain((2u32..).map(|i| {
            let suffix = format!("-{i}");
            let head = base[..base.len().min(MAX_MATERIAL_ID - suffix.len())].trim_end_matches('-');
            format!("{head}{suffix}")
        }))
        .take(MAX_ID_TRIES)
        .find(|c| valid_material_id(c) && !taken(c))
        .unwrap_or_default()
}

fn clamp<T: TryFrom<i64>>(v: i64, lo: i64, hi: i64) -> Option<T> {
    T::try_from(v.clamp(lo, hi)).ok()
}

fn key_source(k: &RampKey) -> RampKeySource {
    let [r, g, b] = k.colour;
    RampKeySource { temperature: k.temperature, colour: Colour(Rgba { r, g, b, a: 255 }), glow: k.glow }
}

/// `keys` as the engine reads them.
pub fn ramp_keys(keys: &[RampKeySource]) -> Vec<RampKey> {
    keys.iter()
        .map(|k| RampKey {
            temperature: k.temperature,
            colour: [k.colour.0.r, k.colour.0.g, k.colour.0.b],
            glow: k.glow,
        })
        .collect()
}

/// The ramp `heat` draws with: its own keys, else the engine's.
pub fn ramp_of(heat: &HeatSource) -> Vec<RampKey> {
    if heat.look.ramp.is_empty() { LAVA_RAMP.to_vec() } else { ramp_keys(&heat.look.ramp) }
}

/// The key added to `keys` (none when full or without a gap): in the
/// middle of the widest gap between 0, the keys and the hottest
/// temperature, coloured as the ramp is there. With its index.
fn added_key(keys: &[RampKeySource]) -> Option<(usize, RampKeySource)> {
    if keys.len() >= MAX_RAMP_KEYS {
        return None;
    }
    let mut bounds = vec![0u16];
    bounds.extend(keys.iter().map(|k| k.temperature));
    bounds.push(MAX_TEMPERATURE + 1);
    let (i, gap) =
        bounds.windows(2).map(|w| w[1].saturating_sub(w[0])).enumerate().max_by_key(|&(i, g)| {
            // The widest; of equal ones the hottest.
            (g, i)
        })?;
    if gap < 2 {
        return None;
    }
    let temperature = bounds[i] + gap / 2;
    let ramp = ramp_keys(keys);
    let (colour, glow) = ramp_at(&ramp, temperature)
        .or_else(|| ramp.first().map(|k| (k.colour, k.glow)))
        .unwrap_or(([0, 0, 0], 0));
    Some((i, key_source(&RampKey { temperature, colour, glow })))
}

/// `m` with `e` applied (clamped); `None` when it does not apply (no heat,
/// no such key, nothing to add).
fn edited(m: &MaterialSource, e: &Edit) -> Option<MaterialSource> {
    let mut m = m.clone();
    if let Edit::Notes(t) = e {
        m.notes = t.clone();
        return Some(m);
    }
    if let Edit::Heats(on) = e {
        m.heat = on.then(stone_heat);
        return Some(m);
    }
    let h = m.heat.as_mut()?;
    match *e {
        Edit::Notes(_) | Edit::Heats(_) => {}
        Edit::Sensitivity(v) => h.sensitivity = clamp(v, 0, MAX_SENSITIVITY.into())?,
        Edit::Capacity(v) => h.capacity = clamp(v, MIN_CAPACITY.into(), MAX_CAPACITY.into())?,
        Edit::Conductivity(v) => h.conductivity = clamp(v, 0, MAX_CONDUCTIVITY.into())?,
        Edit::Cooling(v) => h.cooling = clamp(v, MIN_COOLING.into(), MAX_COOLING.into())?,
        Edit::Light(v) => h.look.light = clamp(v, 0, MAX_GLOW.into())?,
        Edit::OwnRamp(on) => {
            h.look.ramp = if on { LAVA_RAMP.iter().map(key_source).collect() } else { Vec::new() };
        }
        Edit::KeyTemperature(i, v) => {
            let keys = &mut h.look.ramp;
            let lo = if i == 0 { 1 } else { i64::from(keys.get(i - 1)?.temperature) + 1 };
            let hi = keys.get(i + 1).map_or(MAX_TEMPERATURE.into(), |k| i64::from(k.temperature) - 1);
            keys.get_mut(i)?.temperature = clamp(v, lo, hi.max(lo))?;
        }
        Edit::KeyColour(i, [r, g, b]) => h.look.ramp.get_mut(i)?.colour = Colour(Rgba { r, g, b, a: 255 }),
        Edit::KeyGlow(i, v) => h.look.ramp.get_mut(i)?.glow = clamp(v, 0, MAX_GLOW.into())?,
        Edit::AddKey => {
            if h.look.ramp.is_empty() {
                // The engine's keys first: adding one starts an own ramp.
                h.look.ramp = LAVA_RAMP.iter().map(key_source).collect();
            }
            let (i, k) = added_key(&h.look.ramp)?;
            h.look.ramp.insert(i, k);
        }
        Edit::RemoveKey(i) => {
            if i >= h.look.ramp.len() {
                return None;
            }
            h.look.ramp.remove(i);
        }
    }
    Some(m)
}

impl MaterialEditorState {
    /// The editor on `library` (the host's `list_materials`: the built-in
    /// materials first), the first of the user's open, else the first.
    pub fn new(library: Vec<MaterialEntry>) -> MaterialEditorState {
        let mut s = MaterialEditorState {
            library,
            open: None,
            dialog: None,
            pending: None,
            status: String::new(),
            generation: 0,
            preview: PreviewSettings::default(),
        };
        let first = s.library.iter().find(|e| !e.builtin).or(s.library.first());
        if let Some(e) = first {
            let key = Key { builtin: e.builtin, id: e.material.id.clone() };
            s.open_key(&key);
        }
        s
    }

    /// The editor on `library` with `material`'s unsaved changes (a
    /// recovery copy) open: the user's material of its id, whose file is
    /// written again by Save when gone.
    pub fn recovered(library: Vec<MaterialEntry>, material: MaterialSource) -> MaterialEditorState {
        let mut s = MaterialEditorState::new(library);
        let key = Key::user(&material.id);
        let saved = s.entry(&key).map(|e| e.material.clone());
        s.status = format!("Recovered the unsaved changes to {}", material.name);
        s.open = Some(Open { key, saved, doc: material, undo: Vec::new(), redo: Vec::new() });
        s.generation += 1;
        s
    }

    pub fn library(&self) -> &[MaterialEntry] {
        &self.library
    }

    fn entry(&self, key: &Key) -> Option<&MaterialEntry> {
        self.library.iter().find(|e| e.builtin == key.builtin && e.material.id == key.id)
    }

    /// The material open, as edited.
    pub fn doc(&self) -> Option<&MaterialSource> {
        self.open.as_ref().map(|o| &o.doc)
    }

    /// Which material is open.
    pub fn selected(&self) -> Option<&Key> {
        self.open.as_ref().map(|o| &o.key)
    }

    /// Whether the material open is a built-in one (never changed).
    pub fn read_only(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.key.builtin)
    }

    /// Whether the material open has changes the library has not.
    pub fn dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.saved.as_ref() != Some(&o.doc))
    }

    /// Whether the material open has no file in the library (a recovered
    /// one whose file is gone).
    pub fn unsaved_file(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.saved.is_none())
    }

    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn can_undo(&self) -> bool {
        self.open.as_ref().is_some_and(|o| !o.undo.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        self.open.as_ref().is_some_and(|o| !o.redo.is_empty())
    }

    /// What keeps the material open from saving (none: it saves).
    pub fn problems(&self) -> Vec<String> {
        let Some(o) = &self.open else { return Vec::new() };
        match compile_material(&o.doc, &material_file(&o.doc.id), "") {
            Ok(_) => Vec::new(),
            Err(e) => vec![e.to_string()],
        }
    }

    /// Whether `id` is taken in the user's library or by a built-in
    /// material (a new id avoids both: a level's copy is joined by id).
    pub fn id_taken(&self, id: &str) -> bool {
        self.library.iter().any(|e| e.material.id == id) || self.open.as_ref().is_some_and(|o| o.key.id == id)
    }

    /// The id a material named `name` gets (`keep`: an id it may keep).
    pub fn id_for(&self, name: &str, keep: Option<&str>) -> String {
        if let Some(k) = keep
            && free_id(name, |_| false) == k
        {
            return k.to_string();
        }
        free_id(name, |c| self.id_taken(c))
    }

    /// The name dialog's starting text.
    pub fn suggested_name(&self, p: Purpose) -> String {
        let name = self.doc().map(|d| d.name.as_str()).unwrap_or_default();
        match p {
            Purpose::New => "New material".into(),
            Purpose::Copy => {
                let mut n: String = format!("{name} copy");
                if n.chars().count() > MAX_MATERIAL_NAME {
                    n = n.chars().take(MAX_MATERIAL_NAME).collect();
                }
                n
            }
            Purpose::Rename => name.to_string(),
        }
    }

    /// The live preview's controls as set (issue #45).
    pub fn preview(&self) -> &PreviewSettings {
        &self.preview
    }

    /// The library's material the preview compares with, as listed.
    pub fn compare_material(&self) -> Option<&MaterialSource> {
        self.preview.compare.as_ref().and_then(|k| self.entry(k)).map(|e| &e.material)
    }

    pub fn update(&mut self, a: MaterialAction) -> Vec<MaterialCommand> {
        let before = self.compare_material().cloned();
        let mut out = self.act(a);
        if self.compare_material() != before.as_ref() {
            out.push(MaterialCommand::Preview(PreviewCommand::SetCompare(self.compare_material().cloned())));
        }
        out
    }

    /// The preview's controls: they never touch the material open.
    fn preview_action(&mut self, a: PreviewAction) -> Vec<MaterialCommand> {
        let c = match a {
            PreviewAction::Blast(at) => PreviewCommand::Blast { at, size: self.preview.size },
            PreviewAction::Size(size) => {
                self.preview.size = size;
                return Vec::new();
            }
            PreviewAction::Scale(scale) => {
                if self.preview.scale == scale {
                    return Vec::new();
                }
                self.preview.scale = scale;
                PreviewCommand::TimeScale(scale.factor())
            }
            PreviewAction::Compare(key) => {
                // `update` says the new material, when it changed.
                self.preview.compare = key.filter(|k| self.entry(k).is_some());
                return Vec::new();
            }
            PreviewAction::Restart => PreviewCommand::Restart,
        };
        vec![MaterialCommand::Preview(c)]
    }

    fn act(&mut self, a: MaterialAction) -> Vec<MaterialCommand> {
        use MaterialAction as A;
        // The host's answers act whatever is open.
        match a {
            A::Created(m) => return self.created(m),
            A::Saved(m) => return self.saved(m),
            A::Renamed { from, to } => {
                self.renamed(&from, to);
                return Vec::new();
            }
            A::Deleted(id) => {
                self.deleted(&id);
                return Vec::new();
            }
            A::Failed(why) => {
                self.pending = None;
                self.dialog = Some(Dialog::Error(why));
                return Vec::new();
            }
            A::Note(n) => {
                self.status = n;
                return Vec::new();
            }
            A::Preview(p) => return self.preview_action(p),
            _ => {}
        }
        if let Some(d) = self.dialog.clone() {
            return self.answer(d, a);
        }
        match a {
            A::Select(key) => {
                if self.selected() == Some(&key) || self.entry(&key).is_none() {
                    return Vec::new();
                }
                self.guard(Next::Select(key))
            }
            A::New => {
                self.dialog = Some(Dialog::Name(Purpose::New));
                Vec::new()
            }
            A::Copy if self.open.is_some() => {
                self.dialog = Some(Dialog::Name(Purpose::Copy));
                Vec::new()
            }
            A::Rename | A::Delete if self.read_only() => {
                self.status = "A built-in material never changes: edit a copy".into();
                Vec::new()
            }
            A::Rename if self.unsaved_file() => {
                self.status = "Save the material before renaming it".into();
                Vec::new()
            }
            A::Rename if self.open.is_some() => {
                self.dialog = Some(Dialog::Name(Purpose::Rename));
                Vec::new()
            }
            A::Delete if self.open.is_some() => {
                self.dialog = Some(Dialog::Delete);
                Vec::new()
            }
            A::Set(e) => {
                self.set(&e);
                Vec::new()
            }
            A::Undo => {
                self.step(true);
                Vec::new()
            }
            A::Redo => {
                self.step(false);
                Vec::new()
            }
            A::Save => self.save(),
            A::Close => self.guard(Next::Close),
            _ => Vec::new(),
        }
    }

    /// `a` while dialog `d` is open: only its answers act.
    fn answer(&mut self, d: Dialog, a: MaterialAction) -> Vec<MaterialCommand> {
        use MaterialAction as A;
        match (d, a) {
            (_, A::Choice(Choice::Cancel)) => {
                self.dialog = None;
                Vec::new()
            }
            (Dialog::Name(p), A::Named(name)) => self.named(p, &name),
            (Dialog::Delete, A::ConfirmDelete) => {
                self.dialog = None;
                match &self.open {
                    Some(o) if !o.key.builtin && o.saved.is_some() => {
                        vec![MaterialCommand::Delete(o.key.id.clone())]
                    }
                    Some(o) if !o.key.builtin => {
                        // Never written: there is nothing to remove.
                        let id = o.key.id.clone();
                        self.deleted(&id);
                        Vec::new()
                    }
                    _ => Vec::new(),
                }
            }
            (Dialog::Unsaved(next), A::Choice(Choice::Save)) => {
                self.dialog = None;
                let out = self.save();
                if out.is_empty() {
                    // Refused (problems, a built-in): the question is over.
                    return out;
                }
                self.pending = Some(next);
                out
            }
            (Dialog::Unsaved(next), A::Choice(Choice::DontSave)) => {
                self.dialog = None;
                self.revert();
                self.go(next)
            }
            _ => Vec::new(),
        }
    }

    /// `next` at once, or the unsaved changes' question first.
    fn guard(&mut self, next: Next) -> Vec<MaterialCommand> {
        if self.dirty() {
            self.dialog = Some(Dialog::Unsaved(next));
            return Vec::new();
        }
        self.go(next)
    }

    fn go(&mut self, next: Next) -> Vec<MaterialCommand> {
        match next {
            Next::Select(key) => {
                self.open_key(&key);
                Vec::new()
            }
            Next::Create(m) => vec![MaterialCommand::Create(m)],
            Next::Close => vec![MaterialCommand::Close],
        }
    }

    /// The unsaved changes thrown away: the library's material again (one
    /// never written is closed).
    fn revert(&mut self) {
        let Some(o) = &mut self.open else { return };
        match o.saved.clone() {
            Some(m) => {
                o.doc = m;
                o.undo.clear();
                o.redo.clear();
            }
            None => self.open = None,
        }
        self.generation += 1;
    }

    fn open_key(&mut self, key: &Key) {
        let Some(e) = self.entry(key) else { return };
        let m = e.material.clone();
        self.open = Some(Open {
            key: key.clone(),
            saved: Some(m.clone()),
            doc: m,
            undo: Vec::new(),
            redo: Vec::new(),
        });
        self.generation += 1;
    }

    /// The name dialog's OK.
    fn named(&mut self, p: Purpose, name: &str) -> Vec<MaterialCommand> {
        let name = name.trim();
        let chars = name.chars().count();
        if !(1..=MAX_MATERIAL_NAME).contains(&chars) {
            self.status = format!("A material's name is 1 to {MAX_MATERIAL_NAME} characters");
            return Vec::new();
        }
        self.dialog = None;
        match p {
            Purpose::New => {
                let m = new_material(&self.id_for(name, None), name);
                self.guard(Next::Create(m))
            }
            Purpose::Copy => {
                let Some(o) = &self.open else { return Vec::new() };
                // As shown, unsaved changes included (the open one's are
                // asked about before the copy opens).
                let mut m = o.doc.clone();
                m.id = self.id_for(name, None);
                m.name = name.to_string();
                self.guard(Next::Create(m))
            }
            Purpose::Rename => {
                let Some(o) = &self.open else { return Vec::new() };
                let Some(saved) = o.saved.clone() else { return Vec::new() };
                let from = o.key.id.clone();
                let id = self.id_for(name, Some(&from));
                if id == from && name == saved.name {
                    return Vec::new();
                }
                let to = MaterialSource { id, name: name.to_string(), ..saved };
                vec![MaterialCommand::Rename { from, to }]
            }
        }
    }

    fn set(&mut self, e: &Edit) {
        if self.read_only() {
            self.status = "A built-in material never changes: edit a copy".into();
            return;
        }
        let Some(o) = &mut self.open else { return };
        let Some(m) = edited(&o.doc, e) else { return };
        if m == o.doc {
            return;
        }
        let before = std::mem::replace(&mut o.doc, m);
        o.undo.push(before);
        if o.undo.len() > MAX_STEPS {
            o.undo.remove(0);
        }
        o.redo.clear();
        self.generation += 1;
    }

    fn step(&mut self, back: bool) {
        let Some(o) = &mut self.open else { return };
        let (from, to) = if back { (&mut o.undo, &mut o.redo) } else { (&mut o.redo, &mut o.undo) };
        let Some(m) = from.pop() else { return };
        to.push(std::mem::replace(&mut o.doc, m));
        self.generation += 1;
    }

    fn save(&mut self) -> Vec<MaterialCommand> {
        let Some(o) = &self.open else { return Vec::new() };
        if o.key.builtin {
            self.status = "A built-in material never changes: edit a copy".into();
            return Vec::new();
        }
        let problems = self.problems();
        if let Some(p) = problems.first() {
            self.status = format!("Not saved: {p}");
            return Vec::new();
        }
        vec![MaterialCommand::Save(o.doc.clone())]
    }

    /// The user's library's entries in name order (then id), after the
    /// built-in ones.
    fn sort(&mut self) {
        self.library.sort_by(|a, b| {
            b.builtin
                .cmp(&a.builtin)
                .then_with(|| a.material.name.cmp(&b.material.name))
                .then_with(|| a.material.id.cmp(&b.material.id))
        });
    }

    fn put(&mut self, m: MaterialSource) {
        match self.library.iter_mut().find(|e| !e.builtin && e.material.id == m.id) {
            Some(e) => e.material = m,
            None => self.library.push(MaterialEntry { builtin: false, material: m }),
        }
        self.sort();
    }

    fn created(&mut self, m: MaterialSource) -> Vec<MaterialCommand> {
        self.status = format!("Made {}", m.name);
        let key = Key::user(&m.id);
        self.put(m);
        self.open_key(&key);
        Vec::new()
    }

    fn saved(&mut self, m: MaterialSource) -> Vec<MaterialCommand> {
        self.status = format!("Saved {}", m.name);
        if let Some(o) = &mut self.open
            && !o.key.builtin
            && o.key.id == m.id
        {
            o.saved = Some(m.clone());
        }
        self.put(m);
        match self.pending.take() {
            Some(next) => self.go(next),
            None => Vec::new(),
        }
    }

    fn renamed(&mut self, from: &str, to: MaterialSource) {
        self.status = format!("Renamed to {}", to.name);
        self.library.retain(|e| e.builtin || e.material.id != from);
        if self.preview.compare == Some(Key::user(from)) {
            self.preview.compare = Some(Key::user(&to.id));
        }
        if let Some(o) = &mut self.open
            && !o.key.builtin
            && o.key.id == from
        {
            o.key.id = to.id.clone();
            o.saved = Some(to.clone());
            for m in std::iter::once(&mut o.doc).chain(o.undo.iter_mut()).chain(o.redo.iter_mut()) {
                m.id = to.id.clone();
                m.name = to.name.clone();
            }
            self.generation += 1;
        }
        self.put(to);
    }

    fn deleted(&mut self, id: &str) {
        let name = self
            .library
            .iter()
            .find(|e| !e.builtin && e.material.id == id)
            .map(|e| e.material.name.clone())
            .or_else(|| self.doc().filter(|d| d.id == id).map(|d| d.name.clone()))
            .unwrap_or_else(|| id.to_string());
        self.status = format!("Deleted {name}");
        self.library.retain(|e| e.builtin || e.material.id != id);
        if self.preview.compare == Some(Key::user(id)) {
            self.preview.compare = None;
        }
        if self.open.as_ref().is_some_and(|o| !o.key.builtin && o.key.id == id) {
            self.open = None;
            self.generation += 1;
        }
    }
}
