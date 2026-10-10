//! Sound (spec §8.4, §19 E1/E12): plays the simulation's `Sound` events
//! through kira the way OLX's OpenAL driver does (presentation note §8).
//!
//! - **Mix** ([`mix`]): positional sounds at gain 0.7 with OpenAL's
//!   inverse-distance-clamped attenuation (reference 50 px, rolloff 2) for a
//!   listener 20 px out of the world plane at the view centre; THROW and
//!   PICKUP at the listener with gain 1.0. Pure maths.
//! - **Voices** ([`voices`]): OLX's per-sound limits (throw 4, other game
//!   sounds 2, mod sounds 10) and a global one; a start beyond them is
//!   skipped. Live voices are re-mixed as the listener moves, like OpenAL
//!   sources that stay put. Pure policy.
//! - **Device** ([`Audio`]): kira's default backend; when no output device
//!   opens the game runs silently and says so once (E12).

#![forbid(unsafe_code)]

pub use audio::{Audio, is_global};
pub use mix::{
    DEFAULT_VOLUME, GLOBAL_GAIN, LISTENER_DISTANCE, Mix, REFERENCE_DISTANCE, ROLLOFF, SILENCE_DB,
    SOURCE_GAIN, attenuation, decibels, mix,
};
pub use voices::{GLOBAL_VOICES, Voice, Voices, max_plays};

/// kira, for callers that pick a backend ([`Audio::with_backend`]).
pub use kira;

mod audio;
pub mod mix;
pub mod voices;
