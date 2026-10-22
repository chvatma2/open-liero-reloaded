//! The Art and Sounds sections' work on the state (issue #19): painting
//! the image shown (a stroke, a fill, a pasted frame: one step each,
//! written back through `olr_edit::mods::art`), new images, imports
//! through the browser, renames and removals (refused while used), a
//! sound played, and the frame-count question.

use olr_data::image::Rgba;
use olr_edit::mods::art;
use olr_format::mods::{Gfx, glow_mask_of};
use olr_skin_editor::Frame;
use olr_skin_editor::paint::{self, Ink};

use crate::art::{ArtDoc, GLOW_STRENGTHS, MAX_RECENT, Target, Tool, ZOOMS};
use crate::browser::{Browser, Import, asset_name};
use crate::state::{ArtAction, BrowseAction, Dialog, ModCommand, ModEditorState, get_as};
use crate::tree::Item;

/// A pointer far off the frame is held this far out (a line stays short).
const FAR: i32 = 512;

/// The paint tools and the view, kept from image to image.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtTools {
    pub tool: Tool,
    /// The brush, 1 or 2 pixels.
    pub size: u32,
    pub target: Target,
    pub colour: Rgba,
    pub glow_strength: u8,
    /// The last colours painted, newest first (`MAX_RECENT`).
    pub recent: Vec<Rgba>,
    /// Points per image pixel (`ZOOMS`).
    pub zoom: f32,
    pub pan: (f32, f32),
    /// The previous frame shown faintly under the one painted.
    pub onion: bool,
    /// The frame painted.
    pub frame: usize,
    /// The frame copied.
    pub clipboard: Option<Frame>,
}

impl Default for ArtTools {
    fn default() -> ArtTools {
        ArtTools {
            tool: Tool::Pencil,
            size: 1,
            target: Target::Colour,
            colour: Rgba::rgb(255, 255, 255),
            glow_strength: 255,
            recent: Vec::new(),
            zoom: 16.0,
            pan: (0.0, 0.0),
            onion: false,
            frame: 0,
            clipboard: None,
        }
    }
}

/// A stroke being painted.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtStroke {
    frame: usize,
    before: Frame,
    start: (i32, i32),
    last: (i32, i32),
}

/// An image change waiting for the frame-count question.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingImage {
    path: String,
    png: Vec<u8>,
    /// The glow of the new strip (Add or Remove frame); `None`: the mask
    /// stays when it still fits.
    glow: Option<Vec<u8>>,
    before: u32,
    after: u32,
}

/// `name` as a new asset's path in `dir` (`gfx/`, `sfx/`): the folder and,
/// for an image, `.png` added when missing.
fn asset_path(dir: &str, name: &str, image: bool) -> String {
    let name = name.trim();
    let name = name.strip_prefix(dir).unwrap_or(name);
    if image && !name.to_ascii_lowercase().ends_with(".png") {
        format!("{dir}{name}.png")
    } else {
        format!("{dir}{name}")
    }
}

impl ModEditorState {
    /// The image the Art section shows, as painted, or why it does not
    /// paint; `None` when no image is shown.
    pub fn art(&self) -> Option<&Result<ArtDoc, String>> {
        self.art.as_ref()
    }
    /// The image shown, when it paints.
    pub fn art_doc(&self) -> Option<&ArtDoc> {
        self.art.as_ref()?.as_ref().ok()
    }
    pub fn tools(&self) -> &ArtTools {
        &self.tools
    }
    /// Bumped by every change of the image shown.
    pub fn art_generation(&self) -> u64 {
        self.art_gen
    }
    /// The import browser, while it is open.
    pub fn browser(&self) -> Option<&Browser> {
        self.browser.as_ref()
    }
    /// Where the browser opens first (the host's home directory).
    pub fn set_browse_dir(&mut self, dir: std::path::PathBuf) {
        self.browse_dir = dir;
    }
    /// How image `path`'s first user draws it (its preview), if any.
    pub fn image_gfx(&self, path: &str) -> Option<Gfx> {
        let user = art::users(self.src(), path).into_iter().find(|u| u.path.ends_with(".gfx.file"))?;
        get_as(self.src(), user.path.strip_suffix(".file")?)
    }
    /// The fields naming asset `path`.
    pub fn asset_users(&self, path: &str) -> Vec<art::AssetUser> {
        art::users(self.src(), path)
    }

    /// The image shown decoded again from the pack (a new image, a change,
    /// undo and redo); the frame kept within it.
    pub(crate) fn load_art(&mut self) {
        self.stroke = None;
        self.art = match self.item() {
            Item::Art(path) => Some(ArtDoc::load(self.pack(), path)),
            _ => None,
        };
        if let Some(Ok(doc)) = &self.art {
            self.tools.frame = self.tools.frame.min(doc.frames() - 1);
        }
        self.art_gen += 1;
    }

    pub(crate) fn art_action(&mut self, a: ArtAction) {
        let t = &mut self.tools;
        match a {
            ArtAction::Tool(tool) => t.tool = tool,
            ArtAction::Size(s) => t.size = s.clamp(1, 2),
            // An image has no player colour: Tint paints colour.
            ArtAction::Target(target) => {
                t.target = if target == Target::Glow { Target::Glow } else { Target::Colour }
            }
            ArtAction::Colour(c) => t.colour = Rgba { a: 255, ..c },
            ArtAction::GlowStrength(g) => t.glow_strength = g.max(1),
            ArtAction::Zoom(steps) => {
                let i = ZOOMS.iter().position(|&z| z >= t.zoom).unwrap_or(ZOOMS.len() - 1) as i32;
                t.zoom = ZOOMS[(i + steps).clamp(0, ZOOMS.len() as i32 - 1) as usize];
            }
            ArtAction::Pan(dx, dy) => {
                if dx.is_finite() && dy.is_finite() {
                    t.pan = ((t.pan.0 + dx).clamp(-4096.0, 4096.0), (t.pan.1 + dy).clamp(-4096.0, 4096.0));
                }
            }
            ArtAction::Onion(on) => t.onion = on,
            ArtAction::Frame(f) => {
                if self.art_doc().is_some_and(|d| f < d.frames()) {
                    self.tools.frame = f;
                }
            }
            ArtAction::Press { x, y } => self.press(x, y),
            ArtAction::Drag { x, y } => self.drag(x, y),
            ArtAction::Release => self.end_stroke(),
            ArtAction::CopyFrame => {
                if let Some(doc) = self.art_doc() {
                    let f = self.tools.frame;
                    self.tools.clipboard = Some(doc.frame(f));
                    self.set_status(format!("Copied frame {}", f + 1));
                }
            }
            ArtAction::PasteFrame => self.paste_frame(),
            ArtAction::AddFrame => {
                let Some(doc) = self.art_doc() else { return };
                let (png, glow) = doc.with_frame_added(self.tools.frame);
                let n = doc.frames() as u32;
                self.ask_frames(doc.path().to_string(), png, Some(glow), n, n + 1);
            }
            ArtAction::RemoveFrame => {
                let Some(doc) = self.art_doc() else { return };
                let n = doc.frames() as u32;
                match doc.with_frame_removed(self.tools.frame) {
                    Some((png, glow)) => self.ask_frames(doc.path().to_string(), png, Some(glow), n, n - 1),
                    None => self.set_status("An image keeps at least one frame"),
                }
            }
        }
    }

    /// What the tool paints, for the target.
    fn ink(&self) -> Ink {
        let erase = self.tools.tool == Tool::Eraser;
        match self.tools.target {
            Target::Glow => Ink::Glow(if erase { 0 } else { self.tools.glow_strength }),
            _ => Ink::Colour(if erase { Rgba::default() } else { self.tools.colour }),
        }
    }

    fn press(&mut self, x: i32, y: i32) {
        let (x, y) = (x.clamp(-FAR, FAR), y.clamp(-FAR, FAR));
        let Some(doc) = self.art_doc() else { return };
        let f = self.tools.frame;
        let before = doc.frame(f);
        match self.tools.tool {
            Tool::Eyedropper => {
                if let Some(i) = before.at(x, y) {
                    match self.tools.target {
                        Target::Glow => {
                            if before.glow[i] > 0 {
                                let g = before.glow[i];
                                self.tools.glow_strength = GLOW_STRENGTHS
                                    .into_iter()
                                    .min_by_key(|&s| (i16::from(s) - i16::from(g)).abs())
                                    .unwrap_or(255);
                            }
                        }
                        _ if before.px[i].a != 0 => self.tools.colour = Rgba { a: 255, ..before.px[i] },
                        _ => {}
                    }
                    self.tools.tool = Tool::Pencil;
                }
            }
            Tool::Fill => {
                let mut after = before;
                paint::fill(&mut after, x, y, self.ink());
                self.paint_frame(f, &after);
                self.commit_art();
            }
            _ => {
                self.stroke = Some(ArtStroke { frame: f, before, start: (x, y), last: (x, y) });
                self.drag(x, y);
            }
        }
    }

    fn drag(&mut self, x: i32, y: i32) {
        let (x, y) = (x.clamp(-FAR, FAR), y.clamp(-FAR, FAR));
        let ink = self.ink();
        let (size, tool) = (self.tools.size, self.tools.tool);
        let Some(Ok(doc)) = &self.art else { return };
        let Some(s) = &mut self.stroke else { return };
        let mut frame = match tool {
            // A line or rectangle is drawn again from the frame before.
            Tool::Line | Tool::Rect => s.before.clone(),
            _ => doc.frame(s.frame),
        };
        match tool {
            Tool::Line => paint::line(&mut frame, s.start, (x, y), size, ink),
            Tool::Rect => paint::rect(&mut frame, s.start, (x, y), size, ink),
            _ => paint::line(&mut frame, s.last, (x, y), size, ink),
        }
        s.last = (x, y);
        let f = s.frame;
        self.paint_frame(f, &frame);
    }

    /// Frame `f` of the image shown painted (not yet a step).
    fn paint_frame(&mut self, f: usize, frame: &Frame) {
        if let Some(Ok(doc)) = &mut self.art
            && doc.frame(f) != *frame
        {
            doc.set_frame(f, frame);
            self.art_gen += 1;
        }
    }

    /// The stroke written back as one step (none when it changed nothing).
    pub(crate) fn end_stroke(&mut self) {
        if self.stroke.take().is_some() {
            self.commit_art();
        }
    }

    /// The image shown, as painted, into the pack as one step: the colour
    /// image only when its pixels changed, the mask only when the glow did.
    fn commit_art(&mut self) {
        let Some(doc) = self.art_doc() else { return };
        let path = doc.path().to_string();
        let png = doc.colour_changed().then(|| doc.png());
        let glow = doc.glow_changed().then(|| doc.glow().to_vec());
        if png.is_none() && glow.is_none() {
            return;
        }
        let painted_colour = png.is_some();
        let done = self.change(true, |p| {
            if let Some(b) = &png {
                art::set_image(p, &path, b)?;
            }
            if let Some(g) = &glow {
                art::set_glow(p, &path, Some(g))?;
            }
            Ok(Vec::new())
        });
        if !done {
            // Refused: the image shows as the pack holds it.
            self.load_art();
        } else if painted_colour && self.tools.target == Target::Colour && self.tools.tool != Tool::Eraser {
            let c = self.tools.colour;
            self.tools.recent.retain(|&r| r != c);
            self.tools.recent.insert(0, c);
            self.tools.recent.truncate(MAX_RECENT);
        }
    }

    fn paste_frame(&mut self) {
        let Some(clip) = self.tools.clipboard.clone() else {
            self.set_status("Copy a frame first");
            return;
        };
        let Some(doc) = self.art_doc() else { return };
        if (clip.w, clip.h) != (doc.side(), doc.side()) {
            self.set_status("The copied frame is of another size");
            return;
        }
        let f = self.tools.frame;
        self.paint_frame(f, &clip);
        self.commit_art();
    }

    /// Asks before image `path` goes from `before` to `after` frames.
    fn ask_frames(&mut self, path: String, png: Vec<u8>, glow: Option<Vec<u8>>, before: u32, after: u32) {
        self.pending_image = Some(PendingImage { path: path.clone(), png, glow, before, after });
        self.set_dialog(Some(Dialog::FrameCount { image: path, before, after }));
    }

    /// An image change (asked about, or of the same frame count) as one step.
    pub(crate) fn apply_image(&mut self, p: PendingImage) {
        let done = self.change(true, |pack| {
            art::set_image(pack, &p.path, &p.png)?;
            if let Some(g) = &p.glow {
                art::set_glow(pack, &p.path, Some(g))?;
            }
            Ok(Vec::new())
        });
        if done {
            self.set_status(if p.before == p.after {
                format!("{} replaced ({} frames, as before)", p.path, p.after)
            } else {
                format!(
                    "{} has {} frames now (it had {}): Play to check how it plays",
                    p.path, p.after, p.before
                )
            });
            self.select(Item::Art(p.path));
        }
    }

    pub(crate) fn new_image(&mut self, name: &str, side: u32, frames: u32, copy_of: Option<&str>) {
        let path = asset_path(olr_format::mods::GFX_DIR, name, true);
        let source = copy_of.and_then(|c| {
            let bytes = olr_vfs::VPath::new(c).ok().and_then(|p| self.pack().other.get(&p).cloned())?;
            Some((bytes, art::glow(self.pack(), c)))
        });
        if copy_of.is_some() && source.is_none() {
            self.set_status(format!("Not changed: no image {}", copy_of.unwrap_or_default()));
            return;
        }
        let done = self.change(true, |p| {
            match &source {
                Some((bytes, glow)) => {
                    art::import_image(p, &path, bytes)?;
                    art::set_glow(p, &path, glow.as_deref())?;
                }
                None => art::import_image(p, &path, &art::blank_image(side, frames)?)?,
            }
            Ok(Vec::new())
        });
        if done {
            self.set_dialog(None);
            self.set_status(format!("Made {path}: choose it as a projectile's image to use it"));
            self.select(Item::Art(path));
        }
    }

    pub(crate) fn rename_asset(&mut self, from: &str, to: &str) {
        let image = olr_format::mods::is_image(from);
        let dir = if image { olr_format::mods::GFX_DIR } else { olr_format::mods::SFX_DIR };
        let to = asset_path(dir, to, image);
        let shown = *self.item() == Item::Art(from.to_string());
        if self.change(true, |p| art::rename(p, from, &to).map(|()| Vec::new())) {
            self.set_status(format!("Renamed {from} to {to}"));
            if shown {
                self.select(Item::Art(to));
            }
        } else {
            let why = self.status().to_string();
            self.set_dialog(Some(Dialog::Error(why)));
        }
    }

    pub(crate) fn remove_asset(&mut self, path: &str) {
        let images = art::images(self.pack());
        let at = images.iter().position(|i| i == path);
        let shown = *self.item() == Item::Art(path.to_string());
        if self.change(true, |p| art::remove(p, path).map(|()| Vec::new())) {
            self.set_status(format!("Removed {path}"));
            // The next image shows in its place.
            if shown {
                let images = art::images(self.pack());
                let next = at.and_then(|i| images.get(i).or(images.last())).cloned();
                self.select(next.map_or(Item::World, Item::Art));
            }
        } else {
            // Refused, usually because it is used: the users listed.
            let why = self.status().trim_start_matches("Not changed: ").to_string();
            self.set_dialog(Some(Dialog::Error(format!("Not removed: {why}"))));
        }
    }

    pub(crate) fn play_sound(&mut self, path: String) -> Vec<ModCommand> {
        match self.asset(&path) {
            Some(b) => vec![ModCommand::PlaySound { bytes: b.to_vec(), path }],
            None => {
                self.set_status(format!("no sound {path}"));
                Vec::new()
            }
        }
    }

    pub(crate) fn open_browser(&mut self, import: Import) -> Vec<ModCommand> {
        if let Import::Replace(p) = &import
            && !art::images(self.pack()).contains(p)
        {
            return Vec::new();
        }
        let dir = self.browse_dir.clone();
        self.browser = Some(Browser::new(import, dir.clone()));
        self.set_dialog(Some(Dialog::Browse));
        vec![ModCommand::List(dir)]
    }

    pub(crate) fn browse(&mut self, a: BrowseAction) -> Vec<ModCommand> {
        let Some(b) = &mut self.browser else { return Vec::new() };
        match a {
            BrowseAction::To(path) => vec![ModCommand::List(b.resolve(&path))],
            BrowseAction::Up => match b.dir().parent() {
                Some(up) => vec![ModCommand::List(up.to_path_buf())],
                None => Vec::new(),
            },
            BrowseAction::Select(name) => {
                b.select(name);
                Vec::new()
            }
            BrowseAction::Choose => match b.selected().map(String::from) {
                Some(name) => self.browse(BrowseAction::Pick(name)),
                None => Vec::new(),
            },
            BrowseAction::Pick(name) => {
                let Some(e) = b.entry(&name).cloned() else { return Vec::new() };
                let path = b.dir().join(&e.name);
                if e.is_dir {
                    return vec![ModCommand::List(path)];
                }
                if !b.import().takes(&e.name) {
                    return Vec::new();
                }
                let import = b.import().clone();
                self.browse_dir = b.dir().to_path_buf();
                self.browser = None;
                self.set_dialog(None);
                vec![ModCommand::ReadFile { import, path }]
            }
        }
    }

    /// The host read file `name` for `import`.
    pub(crate) fn file_read(&mut self, import: Import, name: &str, bytes: &[u8]) {
        match import {
            Import::Image => {
                let path = self.free_path(olr_format::mods::GFX_DIR, &asset_name(name, "image", "png"));
                if self.change(true, |p| art::import_image(p, &path, bytes).map(|()| Vec::new())) {
                    let frames = art::image_frames(bytes).unwrap_or(1);
                    self.set_status(format!(
                        "Imported {path} ({frames} frame{}): choose it as a projectile's image to use it",
                        if frames == 1 { "" } else { "s" }
                    ));
                    self.select(Item::Art(path));
                } else {
                    let why = self.status().to_string();
                    self.set_dialog(Some(Dialog::Error(format!("{name}: {why}"))));
                }
            }
            Import::Sound => {
                let ext = if name.to_ascii_lowercase().ends_with(".ogg") { "ogg" } else { "wav" };
                let path = self.free_path(olr_format::mods::SFX_DIR, &asset_name(name, "sound", ext));
                if self.change(true, |p| art::import_sound(p, &path, bytes).map(|()| Vec::new())) {
                    self.set_status(format!("Imported {path}: choose it as a weapon's or an event's sound"));
                    self.select(Item::Sounds);
                } else {
                    let why = self.status().to_string();
                    self.set_dialog(Some(Dialog::Error(format!("{name}: {why}"))));
                }
            }
            Import::Replace(path) => {
                let before = self.asset(&path).and_then(|b| art::image_frames(b).ok()).unwrap_or(0);
                match art::image_frames(bytes) {
                    Ok(after) => {
                        let p = PendingImage {
                            path: path.clone(),
                            png: bytes.to_vec(),
                            glow: None,
                            before,
                            after,
                        };
                        if before == after {
                            self.apply_image(p);
                        } else {
                            self.pending_image = Some(p);
                            self.set_dialog(Some(Dialog::FrameCount { image: path, before, after }));
                        }
                    }
                    Err(e) => self.set_dialog(Some(Dialog::Error(format!("{name}: {e}")))),
                }
            }
        }
    }

    /// `file` in `dir`, or the first `<stem>-<n>.<ext>` the pack does not
    /// hold (an image's mask's path counts as taken too).
    fn free_path(&self, dir: &str, file: &str) -> String {
        let (stem, ext) = file.rsplit_once('.').unwrap_or((file, ""));
        let taken = |p: &str| {
            let held =
                |q: &str| olr_vfs::VPath::new(q).ok().is_some_and(|v| self.pack().other.contains_key(&v));
            held(p) || glow_mask_of(p).is_some_and(|m| held(&m))
        };
        std::iter::once(format!("{dir}{file}"))
            .chain((2u32..1000).map(|i| format!("{dir}{stem}-{i}.{ext}")))
            .find(|p| !taken(p))
            .unwrap_or_else(|| format!("{dir}{file}"))
    }
}
