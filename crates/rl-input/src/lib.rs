//! Keyboard and gamepad input for one player, turned into the simulation's
//! 8 [`Keys`](rl_sim::Keys) once per tick (spec §8.5, §19 E5).
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

pub use bindings::{Action, Bindings, PadInput, STICK_THRESHOLD, TRIGGER_THRESHOLD, is_browser_reserved};
pub use latch::TickLatch;
pub use rebind::{Binding, Capture, Device, RebindError, Resolve, key_name};
pub use state::{InputState, PadEvent};

/// Re-exported for building [`Bindings`].
pub use gilrs::{Axis, Button};
pub use winit::keyboard::KeyCode;

mod bindings;
mod latch;
mod rebind;
mod state;
