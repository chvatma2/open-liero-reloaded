//! OLX's Deathmatch rules as pure functions of the worms: the scoreboard
//! row, `CGameMode::CompareWormsScore`, `Winner` and `HighestScoredWorm`
//! (`src/server/CGameMode.cpp:388-434`), and the results order.

use rl_sim::{Score, SimSettings, Worm, olx_score};

/// OLX `WRM_OUT` (`include/CWorm.h`): no lives left.
pub(crate) const WRM_OUT: i32 = -1;
/// OLX `WRM_UNLIM`: unlimited lives.
pub(crate) const WRM_UNLIM: i32 = -2;

/// The scoreboard row of `w`: `CWorm::getScore` with the match's settings,
/// the lives, kills, deaths, suicides and the damage score; not left (the
/// mode marks the rows of the worms that left). The deaths and suicides
/// are the worm's score-gated ones; Deathmatch's board replaces them with
/// its counts of every one.
pub(crate) fn row(w: &Worm, s: &SimSettings) -> Score {
    Score {
        worm: w.id,
        score: olx_score(w, s),
        lives: w.lives,
        kills: w.kills,
        deaths: w.deaths,
        suicides: w.suicides,
        damage: w.damage,
        left: false,
    }
}

/// `Round` (`src/common/MathLib.cpp:85-88`): `(int)ceil((double)x - 0.5)`,
/// so halves round down (`0.5` to 0, `-0.5` to -1). NaN gives 0.
pub fn olx_round(x: f32) -> i32 {
    libm::ceil(x as f64 - 0.5) as i32
}

/// `CGameMode::CompareWormsScore(w1, w2)` (`CGameMode.cpp:409-434`): above
/// 0 when `a` ranks before `b`, below 0 after, 0 for a tie. With limited
/// lives (`lives >= 0`) a worm at the kill limit ranks first whichever it is
/// compared with (so two at the limit each rank before the other), then
/// more lives; then a worm not out before an out one, the higher score, and
/// the damage score, rounded (a difference under one half is a tie).
pub fn compare(a: &Score, b: &Score, lives: i32, kill_limit: i32) -> i32 {
    if lives >= 0 {
        if kill_limit > 0 {
            if a.score >= kill_limit {
                return 1;
            }
            if b.score >= kill_limit {
                return -1;
            }
        }
        if a.lives != b.lives {
            return if a.lives > b.lives { 1 } else { -1 };
        }
    }
    match (a.lives != WRM_OUT, b.lives != WRM_OUT) {
        (true, false) => return 1,
        (false, true) => return -1,
        _ => {}
    }
    if a.score != b.score {
        return if a.score > b.score { 1 } else { -1 };
    }
    olx_round(a.damage - b.damage)
}

/// `CGameMode::HighestScoredWorm` (`CGameMode.cpp:398-407`): the first
/// worm, replaced by each later one that compares above the leader; -1 for
/// no worm.
pub(crate) fn highest(rows: impl Iterator<Item = Score>, lives: i32, kill_limit: i32) -> i32 {
    let mut best: Option<Score> = None;
    for r in rows {
        best = match best {
            Some(b) if compare(&r, &b, lives, kill_limit) <= 0 => Some(b),
            _ => Some(r),
        };
    }
    best.map_or(-1, |b| b.worm)
}

/// Whether worm index `i` is in the gone mask `gone` (`rl_sim::World::gone`).
pub(crate) fn is_gone(gone: u32, i: usize) -> bool {
    gone.checked_shr(i as u32).unwrap_or(0) & 1 != 0
}

/// The worms still in the game: OLX's `game.worms()` no longer holds a
/// worm that left (`Game::removeWorm`); here it stays, in `gone`.
pub(crate) fn in_game(worms: &[Worm], gone: u32) -> impl Iterator<Item = &Worm> {
    worms.iter().enumerate().filter(move |&(i, _)| !is_gone(gone, i)).map(|(_, w)| w)
}

/// `CGameMode::Winner` (`CGameMode.cpp:388-396`): with fewer than two
/// worms not out, the first not out (`getFirstAliveWorm`, which counts
/// lives, not `bAlive`); otherwise the highest scored worm
/// (`HighestScoredWorm`, `:398-407`) among those still in the game: a worm
/// that left (in `gone`) never wins.
pub(crate) fn winner(worms: &[Worm], s: &SimSettings, lives: i32, kill_limit: i32, gone: u32) -> i32 {
    let in_game_count = worms.iter().filter(|w| w.lives != WRM_OUT).count();
    if in_game_count < 2
        && let Some(w) = worms.iter().find(|w| w.lives != WRM_OUT)
    {
        return w.id;
    }
    highest(in_game(worms, gone).map(|w| row(w, s)), lives, kill_limit)
}

/// The results order (`CClient::UpdateScore`, `CClient_Draw.cpp:1840-1897`):
/// best first by [`compare`]. OLX `std::sort`s with it, whose order among
/// ties (and for the comparator's inconsistencies) is unspecified; this is
/// a stable insertion sort, so ties keep the id order. It does not allocate.
pub(crate) fn sort(rows: &mut [Score], lives: i32, kill_limit: i32) {
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 && compare(&rows[j], &rows[j - 1], lives, kill_limit) > 0 {
            rows.swap(j, j - 1);
            j -= 1;
        }
    }
}
