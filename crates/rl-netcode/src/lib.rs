//! The rollback netcode (networking spec §3): a sans-IO session that takes
//! messages and the time and returns messages and ticks to run, generic over
//! the [`Simulation`] it drives, and a deterministic network model to test
//! it in one process ([`netsim`]), with a harness that plays whole matches
//! over it ([`harness`]).
//!
//! `#![no_std]` + `alloc`.
#![no_std]

extern crate alloc;

mod clock;
pub mod harness;
mod inputs;
pub mod netsim;
mod rng;
mod session;
mod sim;
mod sync;
pub mod toy;
mod world_sim;

pub use clock::{JUMP_TICKS, MARGIN_SAMPLES, MAX_ADJUST_PPM, Margins, Pace, TARGET_MARGIN};
pub use inputs::{InputLog, SetResult};
pub use rng::Rng;
pub use session::{
    CHECKSUM_EVERY, Config, Driver, Event, FILL_WINDOW, LATE_WARNING_US, MAX_CATCHUP, MAX_PREDICTION, Role,
    SILENCE_TIMEOUT_US, Seat, Session, Stats, TICK_US, TickInfo, input_delay,
};
pub use sim::{Simulation, StateRejected, ToySim};
pub use world_sim::WorldSim;

/// A peer's address within one session.
pub type PeerId = u16;

/// The two data channels of every connection (spec §2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    /// Inputs: unordered, may be lost or duplicated.
    Unreliable,
    /// Everything else: ordered, never lost.
    Reliable,
}
