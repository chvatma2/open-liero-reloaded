//! Issue #92: the live copies' sounds of a presented tick, for the audio
//! (`olr_audio::Audio::play_live`): the one-shots to start (a state's
//! `enter_sound` from [`EventKind::LiveState`], a script's `this.play`
//! from [`EventKind::LiveSound`]) and the loops that should be playing
//! (each copy in a state with a `loop_sound`), each at its copy's centre.
//! A hidden copy (its `Visible` 0) is silent. Presentation only: the
//! simulation never reads it. `olr-present` fills it per presented tick
//! (online, the starts only from the events it admits); here, between
//! the two crates that use it, it is plain data.

use olr_math::Vec2;
use olr_sim::{EventKind, LiveCopyView, LiveView};

/// An asset sound to start once: level sound `sound` (an index into the
/// level's sounds) of copy `copy`, at `pos` (world px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveStart {
    pub copy: u16,
    pub sound: u16,
    pub pos: Vec2,
}

/// An asset sound looping while copy `copy` is in a state whose
/// `loop_sound` is level sound `sound`, the copy now at `pos` (world px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveLoop {
    pub copy: u16,
    pub sound: u16,
    pub pos: Vec2,
}

/// A presented tick's live sounds: the one-shots it starts and the loops
/// its world plays, copy order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveSounds {
    pub starts: Vec<LiveStart>,
    pub loops: Vec<LiveLoop>,
}

/// Copy `c`'s centre (world px): its rectangle's, which its `X` and `Y`
/// move.
pub fn copy_centre(c: &LiveCopyView<'_>) -> Vec2 {
    let (x, y, w, h) = c.rect();
    Vec2::new(x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0)
}

impl LiveSounds {
    /// No starts and no loops.
    pub fn clear(&mut self) {
        self.starts.clear();
        self.loops.clear();
    }

    /// The start event `e` makes, if any, its copy as `live` shows it
    /// (silent when hidden): a [`EventKind::LiveState`] whose state has an
    /// `enter_sound`, or a [`EventKind::LiveSound`] of a sound the level
    /// has. Whether one was pushed.
    pub fn push_start(&mut self, live: LiveView<'_>, e: &EventKind) -> bool {
        let (copy, sound) = match *e {
            EventKind::LiveState { copy, state } => {
                let s =
                    live.level().copies.get(usize::from(copy)).and_then(|c| c.states.get(usize::from(state)));
                match s.and_then(|s| s.enter_sound) {
                    Some(sound) => (copy, sound),
                    None => return false,
                }
            }
            EventKind::LiveSound { copy, sound } => (copy, sound),
            _ => return false,
        };
        match live.copy(copy) {
            Some(c) if c.visible() && sound < live.level().sounds => {
                self.starts.push(LiveStart { copy, sound, pos: copy_centre(&c) });
                true
            }
            _ => false,
        }
    }

    /// The loops `live` plays (none without live copies): each shown copy
    /// whose state has a `loop_sound` the level has, at its centre, in
    /// copy order. Allocates nothing once warm.
    pub fn set_loops(&mut self, live: Option<LiveView<'_>>) {
        self.loops.clear();
        let Some(live) = live else { return };
        let sounds = live.level().sounds;
        for c in live.copies() {
            let s = c.setup().states.get(usize::from(c.state())).and_then(|s| s.loop_sound);
            if let Some(sound) = s.filter(|&s| s < sounds)
                && c.visible()
            {
                self.loops.push(LiveLoop { copy: c.index(), sound, pos: copy_centre(&c) });
            }
        }
    }
}
