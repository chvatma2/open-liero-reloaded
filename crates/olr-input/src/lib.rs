//! Keyboard, gamepad and mouse input for one player, turned into the simulation's
//! [`Keys`](olr_sim::Keys) once per tick (spec §8.5, §19 E5), with a Dig
//! action that presses the dig gesture.
//!
//! - **Keyboard:** winit physical key codes, so bindings mean the same place
//!   on any layout.
//! - **Gamepads:** gilrs events; a trigger counts from 75 % of its travel
//!   and a stick from 60 % of its deflection, like OLX.
//! - **Latching:** presses and releases between two ticks are latched, so
//!   a tap shorter than a tick still reaches the simulation, which latches
//!   again between its physics sub-steps like OLX's `CInput`.
//!
//! No devices are opened here: the caller owns the window and the `Gilrs`
//! context and passes their events in, on any thread.

#![forbid(unsafe_code)]

pub use bindings::{
    Action, Bindings, DIG, MOUSE_BUTTONS, MOUSE_KEYS, MOUSE_MADE, MouseInput, PadInput, STICK_THRESHOLD,
    TRIGGER_THRESHOLD, is_browser_reserved,
};
pub use dig::DIG_REPEAT_TICKS;
pub use latch::TickLatch;
pub use mouse::{LocalWorm, MouseController, Notch};
pub use rebind::{Binding, Capture, Device, Mode, Need, RebindError, Resolve, key_name};
pub use state::{InputState, NOTCH_PIXELS, PadEvent};

/// Re-exported for building [`Bindings`].
pub use gilrs::{Axis, Button};
pub use winit::keyboard::KeyCode;

mod bindings;
mod dig;
mod latch;
pub mod mouse;
mod rebind;
mod state;
