//! `--autoplay <script.ron>` (spec §7, §21 G7): drives the real app, its
//! window and menus, from a script, the way a person would: widgets are
//! clicked by their label (found in egui's AccessKit tree, as
//! `egui_kittest` finds them), keys are tapped through egui and the game's
//! own key handling, and the game's keys are held through the input path.
//! With `--fake-clock` every frame is [`FAKE_FRAME`] of time, so a run is
//! the same whatever the machine; `xtask e2e` runs `fixtures/autoplay/` on
//! Xvfb and asserts a clean log.
//!
//! A script is RON: `(steps: [(ui: Click("Local Play")), (at_frame: 40, ui:
//! WaitFor("LocalPlay"), within: 100), ...])`. Steps run in order, one
//! that sends input at most every frame and never before its `at_frame`;
//! a step that waits (a label to show, a screen to come) fails the run
//! after `within` frames ([`DEFAULT_WITHIN`]). What each step did is a
//! JSON line in `autoplay.jsonl` (the structured log, in `--shot-dir`),
//! the last one `pass` or `fail`. A failing run ends the game with an
//! error (exit status 1).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::accesskit::{self, Node, NodeId, Role, TreeUpdate};
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
use olr_data::image::RgbaImage;
use olr_input::{Action, KeyCode};
use olr_ui::Screen;
use serde::Deserialize;

use crate::speed::Speed;

/// One frame of `--fake-clock` time: two ticks.
pub const FAKE_FRAME: Duration = Duration::from_millis(20);

/// The ticks a `--fake-clock` frame owes.
pub const FAKE_TICKS_PER_FRAME: u32 = 2;

/// How many frames a step may wait by default.
pub const DEFAULT_WITHIN: u64 = 600;

/// A click or key is pressed in the frame after its step, released in the
/// next; the step after it runs this many frames after its own, once the
/// menus acted on it and drew the result.
const INPUT_FRAMES: u64 = 3;

/// A `ClickUntil` clicks again after this many frames without its effect.
const RECLICK_FRAMES: u64 = 30;

/// The screens' names, as `WaitFor` and `Assert(Screen(..))` give them.
pub const SCREENS: [(&str, Screen); 13] = [
    ("Main", Screen::Main),
    ("LocalPlay", Screen::LocalPlay),
    ("Profiles", Screen::Profiles),
    ("Options", Screen::Options),
    ("WeaponSelect", Screen::WeaponSelect),
    ("InGame", Screen::InGame),
    ("Results", Screen::Results),
    ("Replays", Screen::Replays),
    ("Import", Screen::Import),
    ("About", Screen::About),
    ("Multiplayer", Screen::Multiplayer),
    ("Host", Screen::Host),
    ("Lobby", Screen::Lobby),
];

/// The keys a script may tap, by name: egui's key and the window's.
const KEYS: [(&str, egui::Key, KeyCode); 13] = [
    ("Escape", egui::Key::Escape, KeyCode::Escape),
    ("Enter", egui::Key::Enter, KeyCode::Enter),
    ("Space", egui::Key::Space, KeyCode::Space),
    ("Tab", egui::Key::Tab, KeyCode::Tab),
    ("F2", egui::Key::F2, KeyCode::F2),
    ("F5", egui::Key::F5, KeyCode::F5),
    ("H", egui::Key::H, KeyCode::KeyH),
    ("PageUp", egui::Key::PageUp, KeyCode::PageUp),
    ("PageDown", egui::Key::PageDown, KeyCode::PageDown),
    ("ArrowUp", egui::Key::ArrowUp, KeyCode::ArrowUp),
    ("ArrowDown", egui::Key::ArrowDown, KeyCode::ArrowDown),
    ("ArrowLeft", egui::Key::ArrowLeft, KeyCode::ArrowLeft),
    ("ArrowRight", egui::Key::ArrowRight, KeyCode::ArrowRight),
];

/// A script.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub steps: Vec<Step>,
}

fn default_within() -> u64 {
    DEFAULT_WITHIN
}

/// One step.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Not before this frame (frames count from the first, 0).
    #[serde(default)]
    pub at_frame: u64,
    pub ui: Do,
    /// The frames it may wait before the run fails.
    #[serde(default = "default_within")]
    pub within: u64,
}

/// What a step does.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum Do {
    /// Clicks the widget with this label (waiting for it to show).
    Click(String),
    /// Clicks the widget labelled `.0` until a widget labelled `.1` shows
    /// (enabled): the click's effect, not its sending, ends the step. A
    /// click can be lost in a frame egui discards (a `Grid` laid out anew
    /// when a listed game's row changes under it: the e2e's Internet tab),
    /// so one is sent again every `RECLICK_FRAMES` while the effect has not
    /// shown, until the step's `within`.
    ClickUntil(String, String),
    /// Taps a key ([`KEYS`]'s names) for the menus and the game.
    Key(String),
    /// Types this text into the field that has the keyboard (click it
    /// first), at its cursor: egui's text event, as the window gives it.
    Type(String),
    /// Holds or lets go of the key bound to a game action.
    Press(Action),
    Release(Action),
    /// Waits for a screen ([`SCREENS`]'s names).
    WaitFor(String),
    /// Waits this many frames.
    Wait(u64),
    /// Saves `<shot dir>/<name>.png` after this frame, checking that it
    /// shows something (not one flat colour).
    Screenshot(String),
    Assert(Check),
    /// Ends the run (passed). A script that runs out of steps ends too.
    Quit,
    /// Waits for the game to end by itself (the menus' Quit): passed when
    /// it does within the step's frames.
    WaitForExit,
}

/// What `Assert` checks, now.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum Check {
    /// The menus show this screen.
    Screen(String),
    /// A widget with this label shows.
    Label(String),
    /// The window's inner size in pixels.
    WindowSize(u32, u32),
    /// The game playing runs at this speed (`--speed`'s names).
    Speed(String),
    /// A game plays (true) or none (false).
    Playing(bool),
    /// The game playing has ticked at least this often since it started.
    TicksAtLeast(u64),
    /// The online lobby has this many members (players and spectators;
    /// a dedicated server is none).
    Lobby { members: usize },
}

/// The screen named `name`.
pub fn screen(name: &str) -> Option<Screen> {
    SCREENS.iter().find(|(n, _)| *n == name).map(|&(_, s)| s)
}

fn key(name: &str) -> Option<(egui::Key, KeyCode)> {
    KEYS.iter().find(|(n, ..)| *n == name).map(|&(_, e, k)| (e, k))
}

/// Parses and checks a script: every screen, key and speed named exists.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let script: Script = ron::from_str(text).map_err(|e| e.to_string())?;
    for (i, s) in script.steps.iter().enumerate() {
        let bad = |what: &str| Err(format!("step {i}: {what}"));
        match &s.ui {
            Do::WaitFor(name) | Do::Assert(Check::Screen(name)) if screen(name).is_none() => {
                let names: Vec<&str> = SCREENS.iter().map(|(n, _)| *n).collect();
                return bad(&format!("no screen {name:?}; there are {}", names.join(", ")));
            }
            Do::Key(name) if key(name).is_none() => {
                let names: Vec<&str> = KEYS.iter().map(|(n, ..)| *n).collect();
                return bad(&format!("no key {name:?}; there are {}", names.join(", ")));
            }
            Do::Assert(Check::Speed(name)) => {
                Speed::parse(name).map_err(|e| format!("step {i}: {e}"))?;
            }
            Do::Click(l) | Do::Assert(Check::Label(l)) | Do::Screenshot(l) | Do::Type(l)
                if l.trim().is_empty() =>
            {
                return bad("an empty name");
            }
            Do::ClickUntil(a, b) if a.trim().is_empty() || b.trim().is_empty() => {
                return bad("an empty name");
            }
            Do::Screenshot(name) if !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) => {
                return bad(&format!("{name:?}: a screenshot name is letters, digits, - and _"));
            }
            _ => {}
        }
    }
    Ok(script.steps)
}

/// What the app looks like after a frame's menus ran.
pub struct Seen<'a> {
    /// The frame (0 the first).
    pub frame: u64,
    /// egui's AccessKit tree this frame.
    pub tree: Option<&'a TreeUpdate>,
    /// The menus' screen (`None` without the menus).
    pub screen: Option<Screen>,
    /// The window's inner size in pixels.
    pub window: (u32, u32),
    /// The speed the game playing runs at.
    pub speed: Speed,
    /// The game playing's ticks since it started, if one plays.
    pub ticks: Option<u64>,
    /// The online lobby's members, while there is one.
    pub members: Option<usize>,
}

/// What the app must do for the script.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// A screenshot after this frame, named.
    Shot(String),
    /// A key went down (true) or up, for the game's key handling (the
    /// game's actions included: the input path).
    Key(KeyCode, bool),
    /// The key bound to an action went down (true) or up.
    Action(Action, bool),
    /// The script passed: quit.
    Quit,
    /// The script failed: stop with this error.
    Fail(String),
}

/// A labelled widget in the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub name: String,
    pub role: Role,
    /// Its bounds in points (min x, min y, max x, max y).
    pub bounds: Option<[f64; 4]>,
    pub clickable: bool,
    pub disabled: bool,
}

/// Every named widget of `tree`: a node's label, else the labels and
/// values of the nodes it is labelled by, else a text label's value (as
/// AccessKit names them, which `egui_kittest`'s `get_by_label` reads).
pub fn widgets(tree: &TreeUpdate) -> Vec<Widget> {
    let nodes: std::collections::BTreeMap<NodeId, &Node> =
        tree.nodes.iter().map(|(id, n)| (*id, n)).collect();
    let text = |n: &Node| n.label().or(n.value()).map(str::to_string);
    let mut out: Vec<Widget> = nodes
        .values()
        .filter_map(|n| {
            let name = match n.label() {
                Some(l) => l.to_string(),
                None if !n.labelled_by().is_empty() => {
                    let parts: Vec<String> =
                        n.labelled_by().iter().filter_map(|id| nodes.get(id).and_then(|m| text(m))).collect();
                    parts.join(" ")
                }
                None if n.role() == Role::Label => n.value()?.to_string(),
                None => return None,
            };
            Some(Widget {
                name,
                role: n.role(),
                bounds: n.bounds().map(|r| [r.x0, r.y0, r.x1, r.y1]),
                clickable: n.supports_action(accesskit::Action::Click),
                disabled: n.is_disabled(),
            })
        })
        .collect();
    // Top to bottom, then left to right: the first of two same-named
    // widgets is the one a reader sees first.
    out.sort_by(|a, b| {
        let key = |w: &Widget| w.bounds.map_or((f64::MAX, f64::MAX), |b| (b[1], b[0]));
        key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Where to click the widget labelled `label`: the centre of the first
/// enabled one that takes clicks, a widget before a label (else of any
/// with bounds).
pub fn click_point(widgets: &[Widget], label: &str) -> Option<Pos2> {
    let named = || widgets.iter().filter(|w| w.name == label && !w.disabled && w.bounds.is_some());
    // A label that names a widget takes clicks too (selectable text): the
    // widget it names comes first.
    let w = named()
        .find(|w| w.clickable && w.role != Role::Label)
        .or_else(|| named().find(|w| w.clickable))
        .or_else(|| named().next())?;
    let [x0, y0, x1, y1] = w.bounds?;
    Some(Pos2::new(((x0 + x1) / 2.0) as f32, ((y0 + y1) / 2.0) as f32))
}

/// Whether a screenshot shows something: at least 16 colours, and no
/// colour on more than 99.5 % of it (a menu is mostly its background).
/// Gives the numbers for the log.
pub fn sanity(img: &RgbaImage) -> Result<String, String> {
    let mut px: Vec<u32> =
        img.as_raw().as_chunks::<4>().0.iter().map(|p| u32::from_le_bytes([p[0], p[1], p[2], 0])).collect();
    let total = px.len().max(1);
    px.sort_unstable();
    let (mut colours, mut top, mut run) = (0usize, 0usize, 0usize);
    for (i, p) in px.iter().enumerate() {
        if i == 0 || *p != px[i - 1] {
            colours += 1;
            run = 0;
        }
        run += 1;
        top = top.max(run);
    }
    let share = top as f64 / total as f64;
    let said = format!(
        "{}x{}, {colours} colours, the commonest on {:.1} %",
        img.width(),
        img.height(),
        share * 100.0
    );
    if colours < 16 || share > 0.995 { Err(format!("a blank picture: {said}")) } else { Ok(said) }
}

/// The structured log: one JSON object a line.
struct Log {
    file: Option<std::fs::File>,
    path: PathBuf,
}

impl Log {
    fn line(&mut self, v: serde_json::Value) {
        if let Some(f) = &mut self.file
            && writeln!(f, "{v}").is_err()
        {
            eprintln!("warning: writing {}", self.path.display());
            self.file = None;
        }
    }
}

/// A script being played.
pub struct Autoplay {
    steps: Vec<Step>,
    next: usize,
    /// The frame the current step started waiting in.
    since: Option<u64>,
    /// No step before this frame (input still being delivered).
    busy_until: u64,
    /// A `ClickUntil`'s last click, in this frame.
    clicked_at: Option<u64>,
    /// egui events for the next frame, and for the one after.
    pending: Vec<Event>,
    later: Vec<Event>,
    /// Game keys to let go next frame.
    keys_later: Vec<KeyCode>,
    log: Log,
    /// The outcome, once known.
    pub result: Option<Result<(), String>>,
}

impl Autoplay {
    /// The script in `path`, logging to `log` (the directory is made).
    pub fn load(path: &Path, log: &Path) -> anyhow::Result<Autoplay> {
        let text = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        let steps = parse(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        if let Some(dir) = log.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::File::create(log).map_err(|e| anyhow::anyhow!("{}: {e}", log.display()))?;
        Ok(Autoplay::new(steps, Log { file: Some(file), path: log.to_path_buf() }))
    }

    /// `steps` without a log file (tests).
    pub fn from_steps(steps: Vec<Step>) -> Autoplay {
        Autoplay::new(steps, Log { file: None, path: PathBuf::new() })
    }

    fn new(steps: Vec<Step>, log: Log) -> Autoplay {
        Autoplay {
            steps,
            next: 0,
            since: None,
            busy_until: 0,
            clicked_at: None,
            pending: Vec::new(),
            later: Vec::new(),
            keys_later: Vec::new(),
            log,
            result: None,
        }
    }

    /// Before a frame's menus run: the events the script sends them.
    pub fn input(&mut self, raw: &mut RawInput) {
        raw.events.append(&mut self.pending);
        self.pending = std::mem::take(&mut self.later);
    }

    /// A primary click at `at`: pressed in the next frame, released in the
    /// one after.
    fn click(&mut self, at: Pos2) {
        let m = Modifiers::NONE;
        let button = PointerButton::Primary;
        self.pending.push(Event::PointerMoved(at));
        self.pending.push(Event::PointerButton { pos: at, button, pressed: true, modifiers: m });
        self.later.push(Event::PointerButton { pos: at, button, pressed: false, modifiers: m });
    }

    /// The step being played (for messages).
    pub fn current(&self) -> Option<&Step> {
        self.steps.get(self.next)
    }

    fn done(&mut self, frame: u64, what: &str) {
        let step = &self.steps[self.next];
        self.log.line(serde_json::json!({
            "frame": frame, "step": self.next, "do": format!("{:?}", step.ui), "event": "done", "what": what,
        }));
        self.next += 1;
        self.since = None;
        self.clicked_at = None;
    }

    fn fail(&mut self, frame: u64, why: String) -> Vec<Effect> {
        let step = self.steps.get(self.next).map(|s| format!("{:?}", s.ui)).unwrap_or_default();
        self.log.line(
            serde_json::json!({ "frame": frame, "step": self.next, "do": step, "event": "fail", "why": why }),
        );
        let msg = format!("autoplay step {} ({step}) failed at frame {frame}: {why}", self.next);
        self.result = Some(Err(msg.clone()));
        vec![Effect::Fail(msg)]
    }

    /// The end of a run: whether it passed. A run the window ended before
    /// its script did failed.
    pub fn finish(&mut self, frame: u64) -> Result<(), String> {
        if self.result.is_none() && self.current().is_some_and(|s| s.ui == Do::WaitForExit) {
            self.done(frame, "the game ended");
            self.log.line(serde_json::json!({ "frame": frame, "event": "pass", "steps": self.steps.len() }));
            self.result = Some(Ok(()));
        }
        if self.result.is_none() {
            let why = "the game ended before the script did".to_string();
            self.fail(frame, why);
        }
        self.result.clone().unwrap_or(Ok(()))
    }

    /// Logs a screenshot's check; a blank one fails the run.
    pub fn shot_taken(&mut self, frame: u64, name: &str, check: Result<String, String>) -> Vec<Effect> {
        match check {
            Ok(said) => {
                self.log.line(
                    serde_json::json!({ "frame": frame, "event": "screenshot", "name": name, "image": said }),
                );
                Vec::new()
            }
            Err(why) => self.fail(frame, format!("screenshot {name}: {why}")),
        }
    }

    /// After a frame's menus ran: plays the steps due, gives what the app
    /// must do.
    pub fn step(&mut self, seen: &Seen) -> Vec<Effect> {
        let mut out: Vec<Effect> = self.keys_later.drain(..).map(|k| Effect::Key(k, false)).collect();
        if self.result.is_some() || seen.frame < self.busy_until {
            return out;
        }
        let widgets = seen.tree.map(widgets);
        while let Some(step) = self.steps.get(self.next).cloned() {
            if seen.frame < step.at_frame {
                return out;
            }
            let since = *self.since.get_or_insert(seen.frame);
            let waited = seen.frame - since;
            let late = waited > step.within;
            match &step.ui {
                Do::ClickUntil(label, shows) => {
                    let ws = widgets.as_deref().unwrap_or_default();
                    if click_point(ws, shows).is_some() {
                        let clicks = if self.clicked_at.is_some() { "clicked" } else { "already shown" };
                        self.done(seen.frame, clicks);
                        continue;
                    }
                    let due = self.clicked_at.is_none_or(|f| seen.frame >= f + RECLICK_FRAMES);
                    match click_point(ws, label) {
                        _ if late => {
                            let mut names: Vec<String> = ws.iter().map(|w| w.name.clone()).collect();
                            names.dedup();
                            let what = format!("no {shows:?} after clicking {label:?}; showing: {names:?}");
                            out.extend(self.fail(seen.frame, what));
                        }
                        Some(at) if due => {
                            self.click(at);
                            self.clicked_at = Some(seen.frame);
                            self.busy_until = seen.frame + INPUT_FRAMES;
                        }
                        _ => {}
                    }
                    return out;
                }
                Do::Click(label) => {
                    match widgets.as_deref().and_then(|w| click_point(w, label)) {
                        Some(at) => {
                            self.click(at);
                            self.done(seen.frame, &format!("at ({:.0}, {:.0})", at.x, at.y));
                            self.busy_until = seen.frame + INPUT_FRAMES;
                        }
                        None if late => {
                            let mut names: Vec<String> =
                                widgets.unwrap_or_default().into_iter().map(|w| w.name).collect();
                            names.dedup();
                            out.extend(
                                self.fail(seen.frame, format!("no {label:?} to click; showing: {names:?}")),
                            );
                        }
                        None => {}
                    }
                    return out;
                }
                Do::Key(name) => {
                    let Some((k, code)) = key(name) else {
                        return self.fail(seen.frame, format!("no key {name}"));
                    };
                    let ev = |pressed| Event::Key {
                        key: k,
                        physical_key: Some(k),
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    };
                    self.pending.push(ev(true));
                    self.later.push(ev(false));
                    out.push(Effect::Key(code, true));
                    self.keys_later.push(code);
                    self.done(seen.frame, "tapped");
                    self.busy_until = seen.frame + INPUT_FRAMES;
                    return out;
                }
                Do::Type(text) => {
                    self.pending.push(Event::Text(text.clone()));
                    self.done(seen.frame, "typed");
                    self.busy_until = seen.frame + INPUT_FRAMES;
                    return out;
                }
                Do::Press(a) | Do::Release(a) => {
                    out.push(Effect::Action(*a, matches!(step.ui, Do::Press(_))));
                    self.done(seen.frame, "");
                }
                Do::WaitFor(name) => {
                    let want = screen(name);
                    if seen.screen.is_some() && seen.screen == want {
                        self.done(seen.frame, &format!("after {waited} frames"));
                    } else if late {
                        let now = seen.screen.map_or("no menus".to_string(), |s| format!("{s:?}"));
                        out.extend(self.fail(seen.frame, format!("still on {now} after {waited} frames")));
                        return out;
                    } else {
                        return out;
                    }
                }
                Do::Wait(n) => {
                    if waited < *n {
                        return out;
                    }
                    self.done(seen.frame, "");
                }
                Do::Screenshot(name) => {
                    out.push(Effect::Shot(name.clone()));
                    self.done(seen.frame, "");
                    // One picture a frame.
                    return out;
                }
                Do::Assert(c) => {
                    if let Err(why) = check(c, seen, widgets.as_deref()) {
                        out.extend(self.fail(seen.frame, why));
                        return out;
                    }
                    self.done(seen.frame, "holds");
                }
                Do::Quit => {
                    self.done(seen.frame, "");
                    break;
                }
                Do::WaitForExit => {
                    if late {
                        out.extend(
                            self.fail(seen.frame, format!("the game still runs after {waited} frames")),
                        );
                    }
                    return out;
                }
            }
        }
        if self.result.is_none() {
            self.log
                .line(serde_json::json!({ "frame": seen.frame, "event": "pass", "steps": self.steps.len() }));
            self.result = Some(Ok(()));
            out.push(Effect::Quit);
        }
        out
    }
}

/// Whether `c` holds.
fn check(c: &Check, seen: &Seen, widgets: Option<&[Widget]>) -> Result<(), String> {
    match c {
        Check::Screen(name) => {
            let want = screen(name);
            (seen.screen == want)
                .then_some(())
                .ok_or_else(|| format!("the screen is {:?}, not {name}", seen.screen))
        }
        Check::Label(label) => {
            let found = widgets.is_some_and(|w| w.iter().any(|w| w.name == *label));
            found.then_some(()).ok_or_else(|| format!("no {label:?} shows"))
        }
        Check::WindowSize(w, h) => {
            (seen.window == (*w, *h)).then_some(()).ok_or_else(|| format!("the window is {:?}", seen.window))
        }
        Check::Speed(name) => {
            let want = Speed::parse(name)?;
            (seen.speed == want).then_some(()).ok_or_else(|| format!("the speed is {}", seen.speed.label()))
        }
        Check::Playing(on) => {
            (seen.ticks.is_some() == *on).then_some(()).ok_or_else(|| format!("playing is {}", !on))
        }
        Check::TicksAtLeast(n) => {
            let t = seen.ticks.unwrap_or(0);
            (t >= *n).then_some(()).ok_or_else(|| format!("{t} ticks"))
        }
        Check::Lobby { members } => match seen.members {
            Some(n) if n == *members => Ok(()),
            Some(n) => Err(format!("the lobby has {n} members")),
            None => Err("no lobby".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Node, Rect, Tree, TreeId};

    fn tree(nodes: Vec<(u64, Node)>) -> TreeUpdate {
        TreeUpdate {
            nodes: nodes.into_iter().map(|(i, n)| (NodeId(i), n)).collect(),
            tree: Some(Tree::new(NodeId(0))),
            tree_id: TreeId::ROOT,
            focus: NodeId(0),
        }
    }

    fn node(role: Role, label: Option<&str>, value: Option<&str>, at: (f64, f64)) -> Node {
        let mut n = Node::new(role);
        if let Some(l) = label {
            n.set_label(l);
        }
        if let Some(v) = value {
            n.set_value(v);
        }
        n.set_bounds(Rect { x0: at.0, y0: at.1, x1: at.0 + 100.0, y1: at.1 + 20.0 });
        if role == Role::Button || role == Role::ComboBox {
            n.add_action(accesskit::Action::Click);
        }
        n
    }

    #[test]
    fn scripts_parse_and_name_what_exists() {
        let steps = parse(
            r#"(steps: [
                (ui: WaitFor("Main")),
                (at_frame: 5, ui: Click("Local Play"), within: 30),
                (ui: Key("Escape")),
                (ui: Press(Shoot)),
                (ui: Release(Shoot)),
                (ui: Wait(3)),
                (ui: Screenshot("menu-1")),
                (ui: Assert(WindowSize(1280, 720))),
                (ui: Assert(Speed("16"))),
                (ui: Quit),
                (ui: WaitForExit),
            ])"#,
        )
        .unwrap();
        assert_eq!(steps.len(), 11);
        assert_eq!((steps[1].at_frame, steps[1].within), (5, 30));
        assert_eq!(steps[0].within, DEFAULT_WITHIN);
        assert_eq!(steps[3].ui, Do::Press(Action::Shoot));
        for bad in [
            r#"(steps: [(ui: WaitFor("Nowhere"))])"#,
            r#"(steps: [(ui: Key("F13"))])"#,
            r#"(steps: [(ui: Click(""))])"#,
            r#"(steps: [(ui: Screenshot("../x"))])"#,
            r#"(steps: [(ui: Assert(Speed("3")))])"#,
            r#"(steps: [(ui: Fly)])"#,
            r#"(steps: [(ui: Quit, typo: 1)])"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        // Every script in fixtures/autoplay parses.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/autoplay");
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "ron") && p.file_name().is_some_and(|n| n != "settings.ron")
            {
                parse(&std::fs::read_to_string(&p).unwrap())
                    .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            }
        }
    }

    #[test]
    fn widgets_are_found_by_their_label() {
        let mut combo = node(Role::ComboBox, None, Some("1280 × 720"), (10.0, 50.0));
        combo.set_labelled_by(vec![NodeId(2)]);
        let mut off = node(Role::Button, Some("Start"), None, (10.0, 90.0));
        off.set_disabled();
        let t = tree(vec![
            (1, node(Role::Button, Some("Start"), None, (10.0, 10.0))),
            (2, node(Role::Label, None, Some("Window size"), (200.0, 50.0))),
            (3, combo),
            (4, off),
            (5, node(Role::Button, Some("Back"), None, (10.0, 200.0))),
        ]);
        let w = widgets(&t);
        let names: Vec<&str> = w.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["Start", "Window size", "Window size", "Start", "Back"]);
        assert_eq!(click_point(&w, "Start"), Some(Pos2::new(60.0, 20.0)), "the enabled one");
        assert_eq!(click_point(&w, "Window size"), Some(Pos2::new(60.0, 60.0)), "the combo, not its label");
        assert_eq!(click_point(&w, "Quit"), None);

        // A label above its combo, both taking clicks (egui's labels are
        // selectable text): the combo, which is what a person means.
        let mut label = node(Role::Label, None, Some("Mod"), (10.0, 10.0));
        label.add_action(accesskit::Action::Click);
        let mut combo = node(Role::ComboBox, None, Some("Liero v1.0"), (10.0, 40.0));
        combo.set_labelled_by(vec![NodeId(1)]);
        let w = widgets(&tree(vec![(1, label), (2, combo)]));
        assert_eq!(click_point(&w, "Mod"), Some(Pos2::new(60.0, 50.0)), "the combo below its label");
    }

    fn seen(frame: u64, screen: Option<Screen>, tree: Option<&TreeUpdate>) -> Seen<'_> {
        Seen { frame, tree, screen, window: (1280, 720), speed: Speed::X1, ticks: None, members: None }
    }

    #[test]
    fn text_is_typed_and_the_lobby_counted() {
        let steps = parse(
            r#"(steps: [
                (ui: Type("127.0.0.1:23400")),
                (ui: Assert(Lobby(members: 2))),
            ])"#,
        )
        .unwrap();
        assert_eq!(steps[0].ui, Do::Type("127.0.0.1:23400".into()));
        assert!(parse(r#"(steps: [(ui: Type(""))])"#).is_err(), "nothing to type");
        let mut a = Autoplay::from_steps(steps);
        assert!(a.step(&seen(0, Some(Screen::Multiplayer), None)).is_empty());
        let mut raw = RawInput::default();
        a.input(&mut raw);
        assert_eq!(raw.events, [Event::Text("127.0.0.1:23400".into())], "typed in the next frame");
        let lobby = |frame, members| Seen { members, ..seen(frame, Some(Screen::Lobby), None) };
        assert!(a.step(&lobby(1, Some(2))).is_empty(), "busy delivering the text");
        let out = a.step(&lobby(3, Some(3)));
        assert!(matches!(&out[..], [Effect::Fail(m)] if m.contains("3 members")), "{out:?}");

        let mut a = Autoplay::from_steps(parse(r#"(steps: [(ui: Assert(Lobby(members: 2)))])"#).unwrap());
        assert!(matches!(&a.step(&lobby(0, None))[..], [Effect::Fail(m)] if m.contains("no lobby")));
        let mut a = Autoplay::from_steps(parse(r#"(steps: [(ui: Assert(Lobby(members: 2)))])"#).unwrap());
        assert_eq!(a.step(&lobby(0, Some(2))), [Effect::Quit]);
    }

    #[test]
    fn steps_click_wait_and_fail_in_time() {
        let steps = parse(
            r#"(steps: [
                (ui: WaitFor("Main")),
                (ui: Click("Start")),
                (ui: Assert(WindowSize(1280, 720))),
                (ui: Click("Nothing"), within: 2),
            ])"#,
        )
        .unwrap();
        let t = tree(vec![(1, node(Role::Button, Some("Start"), None, (0.0, 0.0)))]);
        let mut a = Autoplay::from_steps(steps);
        assert!(a.step(&seen(0, Some(Screen::LocalPlay), Some(&t))).is_empty(), "waits for Main");
        assert!(a.step(&seen(1, Some(Screen::Main), Some(&t))).is_empty(), "Main, then the click is sent");
        let mut raw = RawInput::default();
        a.input(&mut raw);
        assert!(matches!(
            raw.events[..],
            [Event::PointerMoved(_), Event::PointerButton { pressed: true, .. }]
        ));
        let mut raw = RawInput::default();
        a.input(&mut raw);
        assert!(matches!(raw.events[..], [Event::PointerButton { pressed: false, .. }]));
        assert!(a.step(&seen(2, Some(Screen::Main), Some(&t))).is_empty(), "busy delivering the click");
        assert!(
            a.step(&seen(4, Some(Screen::Main), Some(&t))).is_empty(),
            "asserted, then waits for Nothing"
        );
        assert!(a.step(&seen(5, Some(Screen::Main), Some(&t))).is_empty());
        let out = a.step(&seen(8, Some(Screen::Main), Some(&t)));
        assert!(
            matches!(&out[..], [Effect::Fail(m)] if m.contains("Nothing") && m.contains("Start")),
            "{out:?}"
        );
        assert!(a.finish(9).is_err());

        let mut a =
            Autoplay::from_steps(parse(r#"(steps: [(ui: Key("PageUp")), (ui: Screenshot("x"))])"#).unwrap());
        assert_eq!(a.step(&seen(0, None, None)), [Effect::Key(KeyCode::PageUp, true)]);
        assert_eq!(a.step(&seen(1, None, None)), [Effect::Key(KeyCode::PageUp, false)]);
        assert_eq!(a.step(&seen(3, None, None)), [Effect::Shot("x".into())]);
        assert_eq!(a.step(&seen(4, None, None)), [Effect::Quit], "out of steps: passed");
        assert_eq!(a.finish(5), Ok(()));

        // The game quitting by itself passes a script waiting for it, only then.
        let mut a = Autoplay::from_steps(parse(r#"(steps: [(ui: WaitForExit, within: 2)])"#).unwrap());
        assert!(a.step(&seen(0, None, None)).is_empty());
        assert_eq!(a.finish(1), Ok(()));
        let mut a = Autoplay::from_steps(parse(r#"(steps: [(ui: WaitForExit, within: 2)])"#).unwrap());
        assert!(a.step(&seen(0, None, None)).is_empty());
        assert!(matches!(&a.step(&seen(3, None, None))[..], [Effect::Fail(_)]));
    }

    /// A click lost (a frame egui discarded) is sent again until its effect
    /// shows: the step waits on the state, not on the click's sending.
    #[test]
    fn click_until_clicks_again_until_its_effect_shows() {
        let steps =
            parse(r#"(steps: [(ui: ClickUntil("e2e server", "Join"), within: 100), (ui: Quit)])"#).unwrap();
        let row = tree(vec![(1, node(Role::Button, Some("e2e server"), None, (0.0, 0.0)))]);
        let joined = tree(vec![
            (1, node(Role::Button, Some("e2e server"), None, (0.0, 0.0))),
            (2, node(Role::Button, Some("Join"), None, (0.0, 40.0))),
        ]);
        let presses = |a: &mut Autoplay| {
            let mut raw = RawInput::default();
            a.input(&mut raw);
            raw.events.iter().filter(|e| matches!(e, Event::PointerButton { pressed: true, .. })).count()
        };
        let mut a = Autoplay::from_steps(steps);
        assert!(a.step(&seen(0, None, Some(&row))).is_empty());
        assert_eq!(presses(&mut a), 1, "clicked");
        presses(&mut a);
        // The click was lost: no Join. Not clicked again before RECLICK_FRAMES.
        assert!(a.step(&seen(5, None, Some(&row))).is_empty());
        assert_eq!(presses(&mut a), 0);
        assert!(a.step(&seen(RECLICK_FRAMES, None, Some(&row))).is_empty());
        assert_eq!(presses(&mut a), 1, "clicked again");
        presses(&mut a);
        // Its effect shows: done, and the next step runs.
        assert_eq!(a.step(&seen(RECLICK_FRAMES + 5, None, Some(&joined))), [Effect::Quit]);

        // Never shown within the step's frames: fails naming both.
        let mut a = Autoplay::from_steps(
            parse(r#"(steps: [(ui: ClickUntil("e2e server", "Join"), within: 50)])"#).unwrap(),
        );
        assert!(a.step(&seen(0, None, Some(&row))).is_empty());
        let out = a.step(&seen(60, None, Some(&row)));
        assert!(
            matches!(&out[..], [Effect::Fail(m)] if m.contains("Join") && m.contains("e2e server")),
            "{out:?}"
        );
        assert!(parse(r#"(steps: [(ui: ClickUntil("a", " "))])"#).is_err());
    }

    #[test]
    fn blank_pictures_fail_the_sanity_check() {
        assert!(sanity(&RgbaImage::new(64, 64)).is_err());
        let px: Vec<u8> = (0..64 * 64)
            .flat_map(|i| [(i % 7 * 30) as u8, (i % 5 * 40) as u8, (i / 64) as u8, 255])
            .collect();
        let img = RgbaImage::from_raw(64, 64, px).unwrap();
        assert!(sanity(&img).unwrap().contains("64x64"));
    }
}
