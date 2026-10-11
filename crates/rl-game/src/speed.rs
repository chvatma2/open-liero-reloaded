//! Fast-forward, for watching only (the user's request in plan 7 task 8):
//! a replay or a match of bots alone (`--spectate`) runs several ticks a
//! frame and draws only the last. A game a person plays always runs at
//! 1×: `--speed` is refused for it and the keys do nothing.
//!
//! The simulation never sees the speed: a frame runs more ticks of the
//! same world, so replays and checksums are the same at any speed
//! (`tests/speed.rs`). Only the presentation of the ticks between two
//! drawn frames is thinned: their sounds ([`Speed::sound`]).

use std::time::Duration;

/// How fast a watched game runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Speed {
    #[default]
    X1,
    X2,
    X4,
    X8,
    X16,
    /// As many ticks as fit in [`MAX_BUDGET`] each frame.
    Max,
}

/// At [`Speed::Max`], ticks run for at most this long a frame, so the
/// event loop still draws, reads the keys and paces the window (a 60 Hz
/// frame is 16.7 ms).
pub const MAX_BUDGET: Duration = Duration::from_millis(12);

/// At [`Speed::Max`], never more ticks than this in one frame (a frame
/// of nothing but cheap ticks still ends).
pub const MAX_TICKS_PER_FRAME: u32 = 2000;

/// At [`Speed::Max`] under `--fake-clock`, each frame runs exactly this
/// many ticks (a wall-clock budget would make the run differ).
pub const FAKE_MAX_TICKS: u32 = 64;

impl Speed {
    /// Every speed, slowest first (the keys step through them).
    pub const ALL: [Speed; 6] = [Speed::X1, Speed::X2, Speed::X4, Speed::X8, Speed::X16, Speed::Max];

    /// The next speed up (Max stays Max).
    pub fn faster(self) -> Speed {
        let i = Speed::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Speed::ALL[(i + 1).min(Speed::ALL.len() - 1)]
    }

    /// The next speed down (1× stays 1×).
    pub fn slower(self) -> Speed {
        let i = Speed::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Speed::ALL[i.saturating_sub(1)]
    }

    /// Ticks per real-time tick; `None` for [`Speed::Max`].
    pub fn factor(self) -> Option<u32> {
        match self {
            Speed::X1 => Some(1),
            Speed::X2 => Some(2),
            Speed::X4 => Some(4),
            Speed::X8 => Some(8),
            Speed::X16 => Some(16),
            Speed::Max => None,
        }
    }

    /// How the HUD names it.
    pub fn label(self) -> &'static str {
        match self {
            Speed::X1 => "1×",
            Speed::X2 => "2×",
            Speed::X4 => "4×",
            Speed::X8 => "8×",
            Speed::X16 => "16×",
            Speed::Max => "max",
        }
    }

    /// `--speed`'s value: `1`, `2`, `4`, `8`, `16` (an `x` after the
    /// number is allowed) or `max`.
    pub fn parse(s: &str) -> Result<Speed, String> {
        let t = s.trim().to_ascii_lowercase();
        let t = if t == "max" { &t } else { t.strip_suffix('x').unwrap_or(&t) };
        Ok(match t {
            "1" => Speed::X1,
            "2" => Speed::X2,
            "4" => Speed::X4,
            "8" => Speed::X8,
            "16" => Speed::X16,
            "max" => Speed::Max,
            _ => return Err(format!("{s:?} is not a speed: 1, 2, 4, 8, 16 or max")),
        })
    }

    /// Whether tick `i` of the `n` a frame runs plays its sounds. Up to
    /// 2× every tick does (twice OLX's rate at most); from 4× to 16× only
    /// the frame's last tick, so the mixer gets at most a tick's sounds a
    /// frame; at max none.
    pub fn sound(self, i: u32, n: u32) -> bool {
        match self {
            Speed::X1 | Speed::X2 => true,
            Speed::X4 | Speed::X8 | Speed::X16 => i + 1 == n,
            Speed::Max => false,
        }
    }

    /// The speed a game runs at: this one when nobody plays it (a replay,
    /// bots alone), else 1× (`human`: a person plays it).
    pub fn for_game(self, human: bool) -> Speed {
        if human { Speed::X1 } else { self }
    }

    /// How many ticks a frame runs when the clock owes `due`: `due` times
    /// the factor, or at max [`Plan::Budget`].
    pub fn plan(self, due: u32) -> Plan {
        match self.factor() {
            Some(f) => Plan::Ticks(due.saturating_mul(f)),
            None => Plan::Budget,
        }
    }
}

/// What one frame runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Exactly this many ticks.
    Ticks(u32),
    /// Ticks until [`MAX_BUDGET`] is spent (at least one, at most
    /// [`MAX_TICKS_PER_FRAME`]; [`FAKE_MAX_TICKS`] under a fake clock).
    Budget,
}

/// Runs one frame's ticks at `speed` when the clock owes `due`:
/// `tick(sound)` for each (`sound`: [`Speed::sound`]) until it gives
/// `false` (nothing ticked: paused, a replay's end); at max until
/// `spent(ticks so far)` (the app's [`MAX_BUDGET`], or [`FAKE_MAX_TICKS`]),
/// at least one tick. The ticks it ran. The app's loop and the tests'
/// (`tests/speed.rs`) are this one function.
pub fn run_frame<E>(
    speed: Speed,
    due: u32,
    mut spent: impl FnMut(u32) -> bool,
    mut tick: impl FnMut(bool) -> Result<bool, E>,
) -> Result<u32, E> {
    let mut n = 0;
    match speed.plan(due) {
        Plan::Ticks(total) => {
            while n < total {
                n += 1;
                if !tick(speed.sound(n - 1, total))? {
                    break;
                }
            }
        }
        Plan::Budget => {
            while n < MAX_TICKS_PER_FRAME && (n == 0 || !spent(n)) {
                n += 1;
                if !tick(false)? {
                    break;
                }
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_step_parse_and_thin_sound() {
        assert_eq!(Speed::X1.slower(), Speed::X1);
        assert_eq!(Speed::Max.faster(), Speed::Max);
        let mut s = Speed::X1;
        let mut seen = vec![s];
        while s != Speed::Max {
            s = s.faster();
            seen.push(s);
        }
        assert_eq!(seen, Speed::ALL);
        assert_eq!(Speed::X16.slower(), Speed::X8);
        for (text, speed) in [("1", Speed::X1), ("2x", Speed::X2), ("16", Speed::X16), ("MAX", Speed::Max)] {
            assert_eq!(Speed::parse(text), Ok(speed));
        }
        for bad in ["0", "3", "", "fast", "-1"] {
            assert!(Speed::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Speed::X4.plan(2), Plan::Ticks(8));
        assert_eq!(Speed::X1.plan(0), Plan::Ticks(0));
        assert_eq!(Speed::Max.plan(1), Plan::Budget);
        // Sound: every tick up to 2×, the last of the frame from 4×, none at max.
        assert!((0..4).all(|i| Speed::X2.sound(i, 4)));
        assert_eq!((0..4).filter(|&i| Speed::X8.sound(i, 4)).count(), 1);
        assert!(Speed::X8.sound(3, 4));
        assert!(!Speed::Max.sound(0, 1));
        // A game a person plays runs at 1×.
        assert_eq!(Speed::X16.for_game(true), Speed::X1);
        assert_eq!(Speed::X16.for_game(false), Speed::X16);
    }

    #[test]
    fn frames_run_the_planned_ticks() {
        let run = |speed, due, stop_after: u32| {
            let mut sounds = Vec::new();
            let n = run_frame::<()>(
                speed,
                due,
                |n| n >= 5,
                |sound| {
                    sounds.push(sound);
                    Ok(sounds.len() < stop_after as usize)
                },
            )
            .unwrap();
            (n, sounds)
        };
        assert_eq!(run(Speed::X1, 2, 99), (2, vec![true, true]));
        assert_eq!(run(Speed::X4, 1, 99), (4, vec![false, false, false, true]));
        assert_eq!(run(Speed::X8, 0, 99).0, 0, "nothing owed, nothing run");
        assert_eq!(run(Speed::Max, 0, 99), (5, vec![false; 5]), "max runs until its budget is spent");
        assert_eq!(run(Speed::X16, 2, 3).0, 3, "stops when nothing ticks");
        assert_eq!(run(Speed::Max, 1, 1).0, 1);
    }
}
