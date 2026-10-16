//! What can go wrong recording, decoding and replaying.

use alloc::string::String;
use core::fmt;

use olr_sim::{PlaceBonusError, SetupError};

/// Why [`crate::Replay::decode`] refused its input.
#[derive(Clone, Debug, PartialEq)]
pub enum DecodeError {
    /// It does not start with [`crate::MAGIC`]: not a replay.
    Magic,
    /// It is of another format version than the one this build reads.
    Version { found: u16, supported: u16 },
    /// It is cut short, has trailing bytes, or its encoding is broken.
    Malformed(String),
    /// It decodes, but describes something no recorder writes.
    Invalid(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Magic => write!(f, "not a replay (no OLRP header)"),
            DecodeError::Version { found, supported } => {
                write!(f, "replay format version {found}, this build reads only version {supported}")
            }
            DecodeError::Malformed(m) => write!(f, "malformed replay: {m}"),
            DecodeError::Invalid(m) => write!(f, "invalid replay: {m}"),
        }
    }
}

/// Why a [`crate::Recorder`] refused a call.
#[derive(Clone, Debug, PartialEq)]
pub enum RecordError {
    /// A setup call, or [`crate::Recorder::start`], after `start`.
    Started,
    /// [`crate::Recorder::push`] or `finish` before [`crate::Recorder::start`].
    NotStarted,
    /// More than [`crate::MAX_SETUP_CALLS`] setup calls.
    TooManyCalls,
    /// The world refused the setup call.
    Call(ReplayError),
    /// [`crate::MAX_TICKS`] ticks are recorded already.
    Full,
    /// The world's frame is not the one the recorded ticks lead to: a tick
    /// was played without being recorded (or recorded twice).
    OutOfStep { recorded: u32, frame: u64 },
    /// [`crate::Recorder::withdraw`] of a worm the world does not have, or
    /// that is gone already.
    NoWorm(u8),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::Started => write!(f, "the recording started already"),
            RecordError::NotStarted => write!(f, "the recording has not started (Recorder::start)"),
            RecordError::TooManyCalls => write!(f, "more than {} setup calls", crate::MAX_SETUP_CALLS),
            RecordError::Call(e) => write!(f, "{e}"),
            RecordError::Full => write!(f, "the replay is full ({} ticks)", crate::MAX_TICKS),
            RecordError::OutOfStep { recorded, frame } => {
                write!(f, "{recorded} ticks recorded but the world is at server frame {frame}")
            }
            RecordError::NoWorm(w) => write!(f, "worm {w} does not exist or has withdrawn already"),
        }
    }
}

/// Why a replay's world could not be built.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayError {
    /// `World::new` refused the setup.
    Setup(SetupError),
    /// A worm spawns outside the level.
    Spawn { worm: usize, at: (i32, i32) },
    /// Setup call `index` names a worm the world does not have.
    NoWorm { index: usize, worm: u32 },
    /// Setup call `index` sets a health that is not a number.
    Health { index: usize },
    /// Setup call `index`'s bonus was refused.
    Bonus { index: usize, error: PlaceBonusError },
    /// The mode's settings are not numbers or outside OLX's ranges.
    Mode(String),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReplayError::Setup(e) => write!(f, "replay setup: {e}"),
            ReplayError::Spawn { worm, at } => {
                write!(f, "replay setup: worm {worm} spawns at {at:?}, off the level")
            }
            ReplayError::NoWorm { index, worm } => write!(f, "replay setup call {index}: no worm {worm}"),
            ReplayError::Health { index } => write!(f, "replay setup call {index}: health is not finite"),
            ReplayError::Bonus { index, error } => {
                write!(f, "replay setup call {index}: bonus refused: {error}")
            }
            ReplayError::Mode(m) => write!(f, "replay mode: {m}"),
        }
    }
}

/// The first recorded checksum the replay did not reproduce: the world
/// diverged during the ticks since the previous checksum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    /// After how many ticks.
    pub tick: u32,
    /// The recorded checksum.
    pub expected: u64,
    /// The replay's.
    pub actual: u64,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.tick == 0 {
            return write!(
                f,
                "replay setup differs: checksum {:016x} before the first tick, recorded {:016x}",
                self.actual, self.expected
            );
        }
        write!(
            f,
            "replay diverged by tick {}: checksum {:016x}, recorded {:016x}",
            self.tick, self.actual, self.expected
        )
    }
}

impl core::error::Error for DecodeError {}
impl core::error::Error for RecordError {}
impl core::error::Error for ReplayError {}
impl core::error::Error for Mismatch {}
