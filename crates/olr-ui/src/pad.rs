//! The menus by gamepad (issue #31): any pad's d-pad and left stick move
//! the focus between widgets, A activates the focused one, B goes back
//! (as Esc). No OLX source: OLX's menus take no pad.
//!
//! It works through egui's own keyboard focus, so a pad does what the
//! keys do and nothing new reaches the keyboard or the mouse: before a
//! pass, [`PadNav::events`] turns the oldest press (one a frame) into the
//! key egui knows (an arrow moves the focus, or a focused slider; Tab
//! focuses the first widget when none is; Enter clicks; Esc is Back);
//! the pass's own part ([`PadNav::begin_pass`], [`PadNav::end_pass`],
//! from `screens::show`) does what keys cannot:
//! - a drop-down ([`pad_choice`]) steps through its choices with left and
//!   right; A opens its list, A on an entry picks it and closes the list,
//!   B closes it, the focus back on the drop-down;
//! - a text field (and a slider's value field, which is one when focused)
//!   is skipped: the focus moves on the same way (there is no on-screen
//!   keyboard);
//! - up or down finding nothing (egui looks within 45° of the way) goes
//!   on in the widgets' order, so every widget can be reached;
//! - the focused widget has a ring ([`crate::theme::FOCUS`]) while the pad
//!   leads (until a mouse button is pressed);
//! - a scroll area shows the widget the pad focused ([`follow_focus`]).
//!
//! Who: any pad in the menus; nobody while a match runs (the game has
//! the pads) or on the weapon selection (the game's keys drive it, a
//! pad's too); while paused, only the pad whose Start paused (any pad
//! after Esc paused).

use std::collections::VecDeque;

use egui::{Context, Event, FocusDirection, Id, Key, Modifiers, Popup, Ui};
use olr_input::{Axis, Button, PadEvent, PadInput, STICK_THRESHOLD};

use crate::state::{Screen, UiState};

/// What a pad asks of the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A: click the focused widget.
    Activate,
    /// B: as Esc.
    Back,
}

impl Nav {
    fn direction(self) -> Option<FocusDirection> {
        match self {
            Nav::Up => Some(FocusDirection::Up),
            Nav::Down => Some(FocusDirection::Down),
            Nav::Left => Some(FocusDirection::Left),
            Nav::Right => Some(FocusDirection::Right),
            Nav::Activate | Nav::Back => None,
        }
    }
}

/// A direction held this long (seconds) starts repeating...
pub const REPEAT_DELAY: f64 = 0.4;
/// ...once every this long.
pub const REPEAT_EVERY: f64 = 0.12;
/// A stick pressed past [`STICK_THRESHOLD`] (olr-input's, OLX's) is let go
/// under this (no flicker at the threshold).
pub const STICK_RELEASE: f32 = 0.4;

/// The passes a focus move is watched after it is made (a slider's
/// value field becomes a text field a pass after it is focused).
const LANDING_PASSES: u8 = 2;
/// The moves a pass makes after one press at most (text fields skipped
/// one after another).
const LANDING_TRIES: u8 = 4;
/// The passes after a move in which scroll areas follow the focus.
const FOLLOW_PASSES: u8 = 3;

/// One input a pad holds that means a [`Nav`].
#[derive(Clone, Debug)]
struct Held {
    pad: usize,
    input: PadInput,
    nav: Nav,
    /// When it repeats next (set when first seen held).
    next: Option<f64>,
    /// Held across a time the menus did not hear the pads: never repeats.
    stale: bool,
}

/// A focus move the pad asked for, watched over the next passes: a text
/// field it lands on is skipped (the focus moves on the same way); one that
/// finds nothing up or down (egui looks for a widget within 45° of the
/// way) goes on in the widgets' order instead (Down the next, Up the
/// previous, round at either end), so every widget can be reached.
#[derive(Clone, Copy, Debug)]
struct Landing {
    dir: FocusDirection,
    /// The focused widget before the move.
    from: Option<Id>,
    /// The move is this pass's to make (a text field kept the arrows).
    now: bool,
    /// Passes watched since the move was made.
    passes: u8,
    /// Moves the passes made (skips, the widgets' order).
    tries: u8,
}

impl Landing {
    fn new(dir: FocusDirection, from: Option<Id>, now: bool) -> Landing {
        Landing { dir, from, now, passes: 0, tries: 0 }
    }

    /// At a pass's start: moves on when it must; what is left to watch.
    fn watch(mut self, ctx: &Context) -> Option<Landing> {
        let focused = ctx.memory(|m| m.focused());
        let typing = ctx.text_edit_focused();
        let order = match self.dir {
            FocusDirection::Down | FocusDirection::Right | FocusDirection::Next => FocusDirection::Next,
            _ => FocusDirection::Previous,
        };
        let again = if self.now {
            Some(self.dir)
        } else if self.passes == 0 {
            // egui moves at this pass's end.
            None
        } else if focused == self.from {
            // Nothing that way: left and right stay (they also move a
            // slider), unless on a text field.
            let vertical = !matches!(self.dir, FocusDirection::Left | FocusDirection::Right);
            (vertical || typing).then_some(order)
        } else if typing {
            Some(self.dir)
        } else {
            None
        };
        match again {
            Some(dir) if self.tries < LANDING_TRIES => {
                ctx.memory_mut(|m| m.move_focus(dir));
                Some(Landing { dir, from: focused, now: false, passes: 0, tries: self.tries + 1 })
            }
            _ => {
                self.now = false;
                self.passes += 1;
                (self.passes <= LANDING_PASSES).then_some(self)
            }
        }
    }
}

/// Which pads the menus hear now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Nobody,
    Anyone,
    Only(usize),
}

impl Who {
    fn hears(self, pad: usize) -> bool {
        match self {
            Who::Nobody => false,
            Who::Anyone => true,
            Who::Only(p) => p == pad,
        }
    }
}

/// The pads' state for the menus (in `MenuData::pad`): the app feeds it
/// every pad event ([`PadNav::on_pad`]) and, before each pass that draws
/// the menus, adds [`crate::screens::MenuData::pad_events`] to egui's
/// input.
#[derive(Clone, Debug, Default)]
pub struct PadNav {
    held: Vec<Held>,
    /// Presses not yet carried out, oldest first.
    queue: VecDeque<(usize, Nav)>,
    /// Pads holding Start now.
    starts: Vec<usize>,
    /// The last pad whose Start was pressed while the match ran.
    last_start: Option<usize>,
    /// A match ran at the last frame (not paused).
    running: bool,
    /// The pad that paused it (`None`: the keyboard did; any pad then).
    pauser: Option<usize>,
    /// The pad leads: the focus ring shows, scroll areas follow.
    active: bool,
    /// The focus move just asked for, watched.
    landing: Option<Landing>,
    /// The widget A clicked last (a drop-down, when a list opened).
    opener: Option<Id>,
    /// A picked an entry of an open list: close it at the next frame.
    close_list: bool,
    /// Passes left in which scroll areas follow the focus.
    follow: u8,
    /// The drop-downs the last pass drew ([`pad_choice`]).
    combos: Vec<Id>,
}

impl PadNav {
    /// A pad's event (`PadEvent::from_gilrs`), pad `pad` (gilrs's id).
    pub fn on_pad(&mut self, pad: usize, ev: PadEvent) {
        match ev {
            PadEvent::Button(Button::Start, v) => {
                let at = self.starts.iter().position(|p| *p == pad);
                match (v >= 0.5, at) {
                    (true, None) => {
                        self.starts.push(pad);
                        self.last_start = Some(pad);
                    }
                    (false, Some(i)) => {
                        self.starts.swap_remove(i);
                    }
                    _ => {}
                }
            }
            PadEvent::Button(b, v) => {
                let nav = match b {
                    Button::DPadUp => Nav::Up,
                    Button::DPadDown => Nav::Down,
                    Button::DPadLeft => Nav::Left,
                    Button::DPadRight => Nav::Right,
                    Button::South => Nav::Activate,
                    Button::East => Nav::Back,
                    _ => return,
                };
                self.set(pad, PadInput::Button(b), nav, v >= 0.5);
            }
            PadEvent::Axis(a, v) => {
                let (plus, minus) = match a {
                    Axis::LeftStickX => (Nav::Right, Nav::Left),
                    Axis::LeftStickY => (Nav::Up, Nav::Down),
                    _ => return,
                };
                for (input, nav, v) in [(PadInput::AxisPlus(a), plus, v), (PadInput::AxisMinus(a), minus, -v)]
                {
                    let held = self.held.iter().any(|h| h.pad == pad && h.input == input);
                    let down = if held { v > STICK_RELEASE } else { v >= STICK_THRESHOLD };
                    self.set(pad, input, nav, down);
                }
            }
            PadEvent::Disconnected => {
                self.held.retain(|h| h.pad != pad);
                self.starts.retain(|p| *p != pad);
                self.queue.retain(|(p, _)| *p != pad);
            }
        }
    }

    fn set(&mut self, pad: usize, input: PadInput, nav: Nav, down: bool) {
        let at = self.held.iter().position(|h| h.pad == pad && h.input == input);
        match (down, at) {
            (true, None) => {
                self.held.push(Held { pad, input, nav, next: None, stale: false });
                self.queue.push_back((pad, nav));
            }
            (false, Some(i)) => {
                self.held.remove(i);
            }
            _ => {}
        }
    }

    /// The menus hear no pad now: the presses are dropped and what is
    /// held does not repeat (the editors, a capture, a match running).
    pub fn forget(&mut self) {
        self.queue.clear();
        for h in &mut self.held {
            h.stale = true;
        }
        self.active = false;
        self.landing = None;
        self.close_list = false;
    }

    /// Which pads the menus hear in `state`, and who paused: the pad whose
    /// Start was pressed while the match ran, as the pause came.
    pub fn who(&mut self, state: &UiState) -> Who {
        let screen = state.screen();
        if screen == Screen::InGame && !state.is_paused() {
            self.running = true;
            self.last_start = None;
            self.pauser = None;
            return Who::Nobody;
        }
        if std::mem::take(&mut self.running) {
            self.pauser = if state.is_paused() { self.last_start.take() } else { None };
        }
        if !state.is_paused() {
            self.pauser = None;
        }
        match (screen, self.pauser) {
            (Screen::WeaponSelect, _) => Who::Nobody,
            (_, Some(p)) => Who::Only(p),
            (_, None) => Who::Anyone,
        }
    }

    /// The next press `who` may make at time `now` (seconds): the oldest
    /// queued, else a held direction due to repeat.
    pub fn next(&mut self, now: f64, who: Who) -> Option<Nav> {
        while let Some((pad, nav)) = self.queue.pop_front() {
            if who.hears(pad) {
                if let Some(h) = self.held.iter_mut().find(|h| h.pad == pad && h.nav == nav) {
                    h.next = Some(now + REPEAT_DELAY);
                }
                return Some(nav);
            }
        }
        let mut due = None;
        for h in &mut self.held {
            if h.stale || !who.hears(h.pad) || h.nav.direction().is_none() {
                continue;
            }
            match h.next {
                None => h.next = Some(now + REPEAT_DELAY),
                Some(t) if now >= t && due.is_none() => {
                    h.next = Some(now + REPEAT_EVERY);
                    due = Some(h.nav);
                }
                Some(_) => {}
            }
        }
        due
    }

    /// The pad leads (the ring shows).
    pub fn active(&self) -> bool {
        self.active
    }

    /// Before a pass that draws the menus: the keys for egui (a press a
    /// frame); `ctx` is as the last pass left it.
    pub fn events(&mut self, ctx: &Context, state: &UiState) -> Vec<Event> {
        let who = self.who(state);
        if who == Who::Nobody {
            self.forget();
            return Vec::new();
        }
        if std::mem::take(&mut self.close_list) {
            self.close_lists(ctx);
        }
        let now = ctx.input(|i| i.time);
        let Some(nav) = self.next(now, who) else { return Vec::new() };
        self.active = true;
        self.follow = FOLLOW_PASSES;
        let focused = ctx.memory(|m| m.focused());
        let typing = ctx.text_edit_focused();
        match (nav, nav.direction(), focused) {
            (Nav::Back, _, _) => {
                // An open list closes, else Esc.
                if Popup::is_any_open(ctx) {
                    self.close_lists(ctx);
                    Vec::new()
                } else {
                    key(Key::Escape)
                }
            }
            (_, _, None) => {
                // Nothing focused: the first widget is.
                self.landing = Some(Landing::new(FocusDirection::Next, None, false));
                key(Key::Tab)
            }
            (_, Some(dir), Some(id)) => {
                let step = match nav {
                    Nav::Left => -1,
                    Nav::Right => 1,
                    _ => 0,
                };
                if step != 0 && self.combos.contains(&id) {
                    ctx.data_mut(|d| d.insert_temp(step_id(), (id, step)));
                    return Vec::new();
                }
                // A text field keeps the arrows: the pass moves the focus.
                self.landing = Some(Landing::new(dir, focused, typing));
                if typing { Vec::new() } else { key(arrow(nav)) }
            }
            (_, None, Some(id)) => {
                // Activate. A text field needs a keyboard.
                if typing {
                    return Vec::new();
                }
                if Popup::is_any_open(ctx) {
                    self.close_list = true;
                } else {
                    self.opener = Some(id);
                }
                key(Key::Enter)
            }
        }
    }

    /// Closes the open lists, the focus back on the drop-down that opened
    /// them.
    fn close_lists(&mut self, ctx: &Context) {
        Popup::close_all(ctx);
        if let Some(id) = self.opener.take() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }

    /// At the start of a menus' pass (`screens::show`, before any widget).
    pub fn begin_pass(&mut self, ctx: &Context) {
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.active = false;
        }
        if let Some(l) = self.landing.take() {
            self.landing = l.watch(ctx);
            // The next passes watch it (the app draws every frame; a test
            // runs passes while one is asked for).
            ctx.request_repaint();
        }
        let follow = self.active && self.follow > 0;
        self.follow = self.follow.saturating_sub(1);
        ctx.data_mut(|d| d.insert_temp(follow_id(), follow));
    }

    /// At the end of a menus' pass: the focus ring, and the drop-downs
    /// drawn for the next frame's left and right.
    pub fn end_pass(&mut self, ui: &Ui) {
        let ctx = ui.ctx();
        self.combos = ctx.data_mut(|d| d.remove_temp::<Vec<Id>>(combos_id())).unwrap_or_default();
        ctx.data_mut(|d| d.remove::<(Id, i32)>(step_id()));
        if !self.active {
            return;
        }
        let Some(r) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) else { return };
        if r.interact_rect.is_positive() {
            crate::theme::focus_ring(&ctx.layer_painter(r.layer_id), r.interact_rect);
        }
    }
}

/// A key pressed and let go, as egui-winit gives one.
fn key(key: Key) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .to_vec()
}

fn arrow(nav: Nav) -> Key {
    match nav {
        Nav::Up => Key::ArrowUp,
        Nav::Down => Key::ArrowDown,
        Nav::Left => Key::ArrowLeft,
        _ => Key::ArrowRight,
    }
}

fn step_id() -> Id {
    Id::new("olr pad step")
}

fn combos_id() -> Id {
    Id::new("olr pad drop-downs")
}

fn follow_id() -> Id {
    Id::new("olr pad follow")
}

/// A drop-down's choice by pad (issue #31): with the drop-down `combo`
/// (`ComboBox::show_ui`'s response) focused, the pad's left and right
/// give the choice before or after `current` among `choices` (none past
/// either end); the keyboard's arrows still move the focus.
pub fn pad_choice<T: PartialEq + Clone>(
    ui: &Ui,
    combo: &egui::Response,
    choices: &[T],
    current: &T,
) -> Option<T> {
    let ctx = ui.ctx();
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Id>>(combos_id()).push(combo.id));
    let (id, step) = ctx.data(|d| d.get_temp::<(Id, i32)>(step_id()))?;
    if id != combo.id {
        return None;
    }
    ctx.data_mut(|d| d.remove::<(Id, i32)>(step_id()));
    let at = choices.iter().position(|c| c == current)?;
    let to = at.checked_add_signed(step as isize)?;
    choices.get(to).cloned()
}

/// Ends a scroll area's content (`ui`): shows the focused widget when the
/// pad just moved the focus to it (egui scrolls to no focused widget by
/// itself).
pub fn follow_focus(ui: &Ui) {
    let ctx = ui.ctx();
    if !ctx.data(|d| d.get_temp::<bool>(follow_id())).unwrap_or(false) {
        return;
    }
    let Some(r) = ctx.memory(|m| m.focused()).and_then(|id| ctx.read_response(id)) else { return };
    if r.layer_id == ui.layer_id() && ui.min_rect().contains(r.rect.center()) {
        ui.scroll_to_rect(r.rect, None);
    }
}
