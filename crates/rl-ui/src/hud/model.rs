//! What the match HUD says (spec §8.3; OLX's HUD:
//! `docs/porting/olx-notes/presentation.md` §9, its results screen:
//! `bots-and-deathmatch.md` §3.4): the worms' names, the event log from
//! the mode's `Message` events, the time-limit clock, the scoreboard rows
//! and the results of a finished match.

use std::collections::VecDeque;

use rl_modes::text;
use rl_sim::{EndReason, MatchResult, Message, Score, World};

/// The event log keeps this many lines; the bar shows the newest that
/// fit (OLX's local chat showed 6 for 3 s over the playfield,
/// `DrawLocalChat`, `CClient_Draw.cpp:2211-2236`; the bar has room for
/// more, and keeps them).
pub const LOG_KEEP: usize = 64;

/// OLX's `WRM_UNLIM` and `WRM_OUT` lives.
const UNLIMITED: i32 = -2;
const OUT: i32 = -1;

/// Who plays each worm, for its name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Names(pub Vec<String>);

impl Names {
    /// Worm `id`'s name; an unknown id is `"worm <id>"`.
    pub fn of(&self, id: i32) -> String {
        usize::try_from(id).ok().and_then(|i| self.0.get(i)).cloned().unwrap_or_else(|| format!("worm {id}"))
    }
}

/// `Message`'s text with OLX's default wording (`rl_modes::text::olx_text`)
/// and the names filled in. A kill whose weapon is unknown (always, so far:
/// `rl-sim` does not keep the weapon an injury came from) drops
/// `" with <weapon>"`, as OLX does (`CGameMode::Kill`). `None` for a text
/// Deathmatch does not send.
pub fn message_text(m: &Message, names: &Names) -> Option<String> {
    let template = text::olx_text(m.text)?;
    let [a, b, c] = m.args;
    let s = if m.text == text::KILLED {
        let s = template.replace("<killer>", &names.of(a)).replace("<victim>", &names.of(b));
        if c < 0 { s.replace(" with <weapon>", "") } else { s.replace("<weapon>", &format!("weapon {c}")) }
    } else if m.text == text::KILL_LIMIT {
        template.replace("<limit>", &a.to_string()).replace("<player>", &names.of(b))
    } else {
        template.replace("<player>", &names.of(a))
    };
    Some(s)
}

/// What a log line is, for its colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
    /// The game's: kills, deaths, the mode's messages.
    #[default]
    Event,
    /// A player's words (a multiplayer chat, later).
    Chat,
}

/// One line of the event log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogLine {
    pub text: String,
    pub kind: LineKind,
}

impl LogLine {
    /// A game event's line.
    pub fn event(text: impl Into<String>) -> LogLine {
        LogLine { text: text.into(), kind: LineKind::Event }
    }
}

/// The event log: the last [`LOG_KEEP`] lines, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Log {
    lines: VecDeque<LogLine>,
}

impl Log {
    /// Adds a line; the oldest goes past [`LOG_KEEP`].
    pub fn push(&mut self, line: LogLine) {
        self.lines.push_back(line);
        if self.lines.len() > LOG_KEEP {
            self.lines.pop_front();
        }
    }

    /// The lines kept, oldest first.
    pub fn lines(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// The lines' texts, oldest first.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|l| l.text.as_str())
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

/// `m:ss` for a time in ms, rounded up to the second (a clock showing
/// `0:00` has run out).
pub fn clock(ms: u64) -> String {
    let s = ms.div_ceil(1000);
    format!("{}:{:02}", s / 60, s % 60)
}

/// A worm's lives as OLX's results show them: `∞`, `out` or the number.
pub fn lives_text(lives: i32) -> String {
    match lives {
        UNLIMITED => "∞".into(),
        OUT => "out".into(),
        n if n < 0 => "∞".into(),
        n => n.to_string(),
    }
}

/// One row of the scoreboard or the results, in the mode's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub worm: i32,
    pub name: String,
    pub score: i32,
    pub lives: String,
    pub kills: i32,
    pub deaths: i32,
    pub suicides: i32,
    /// `Round(fDamage)`.
    pub damage: i32,
    /// The worm the HUD follows (the player's own; a watched game's
    /// followed worm): its row is highlighted.
    pub followed: bool,
    /// The worm left the match: a results row only, its lives "left" (N85).
    pub left: bool,
}

fn row(s: &Score, names: &Names, followed: Option<i32>) -> Row {
    Row {
        worm: s.worm,
        name: names.of(s.worm),
        score: s.score,
        lives: if s.left { "left".into() } else { lives_text(s.lives) },
        kills: s.kills,
        deaths: s.deaths,
        suicides: s.suicides,
        damage: rl_modes::olx_round(s.damage),
        followed: Some(s.worm) == followed,
        left: s.left,
    }
}

/// The live scoreboard: the mode's rows (`World::scores`, best first) but
/// those of worms that left, worm `followed`'s highlighted.
pub fn rows(scores: &[Score], names: &Names, followed: Option<i32>) -> Vec<Row> {
    scores.iter().filter(|s| !s.left).map(|s| row(s, names, followed)).collect()
}

/// A finished match, as the results overlay shows it (OLX's results
/// screen, `CClient::UpdateScore`).
#[derive(Clone, Debug, PartialEq)]
pub struct Results {
    /// The winner's name, or `None` (OLX's "noone").
    pub winner: Option<String>,
    /// Why it ended.
    pub reason: String,
    pub rows: Vec<Row>,
    /// The time left on the clock when it ended, if there was a limit.
    pub time_left_ms: Option<u64>,
}

impl Results {
    /// The results of `world`'s match, once it ended, worm `followed`'s
    /// row highlighted.
    pub fn of(
        world: &World,
        names: &Names,
        time_left_ms: Option<u64>,
        followed: Option<i32>,
    ) -> Option<Results> {
        Some(Results::new(world.result()?, world.scores(), names, time_left_ms, followed))
    }

    /// The results of a match that ended with `r` and `scores` (the mode's
    /// order; online play's from a confirmed tick).
    pub fn new(
        r: MatchResult,
        scores: &[Score],
        names: &Names,
        time_left_ms: Option<u64>,
        followed: Option<i32>,
    ) -> Results {
        let reason = match r.reason {
            EndReason::KillLimit => "kill limit".to_string(),
            EndReason::TimeLimit => "time limit".to_string(),
            EndReason::LastStanding => "last worm standing".to_string(),
            EndReason::Other(n) => format!("the mode's reason {n}"),
        };
        Results {
            winner: (r.winner >= 0).then(|| names.of(r.winner)),
            reason,
            rows: scores.iter().map(|s| row(s, names, followed)).collect(),
            time_left_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rl_sim::TextId;

    fn names() -> Names {
        Names(vec!["You".into(), "Bot 1".into()])
    }

    fn score(worm: i32, kills: i32) -> Score {
        Score { worm, score: kills, lives: -2, kills, deaths: 1, suicides: 0, damage: 0.0, left: false }
    }

    #[test]
    fn a_left_row_is_off_the_scoreboard_and_left_in_the_results() {
        let names = names();
        let mut scores = [score(0, 3), score(1, 1)];
        scores[1].left = true;
        scores[1].lives = -1;
        let live = rows(&scores, &names, Some(0));
        assert_eq!(live.iter().map(|r| r.worm).collect::<Vec<_>>(), [0]);
        let r = Results::new(
            MatchResult { winner: 0, reason: EndReason::TimeLimit },
            &scores,
            &names,
            None,
            Some(0),
        );
        let left: Vec<(i32, &str, bool)> =
            r.rows.iter().map(|r| (r.worm, r.lives.as_str(), r.left)).collect();
        assert_eq!(left[1], (1, "left", true));
        assert_eq!((r.rows[1].kills, r.rows[1].deaths), (1, 1), "its counts so far");
    }

    #[test]
    fn messages_name_the_worms() {
        let n = names();
        let m = |text: TextId, args| message_text(&Message { text, args }, &n).unwrap();
        assert_eq!(m(text::KILLED, [1, 0, -1]), "Bot 1 killed You");
        assert_eq!(m(text::KILLED, [0, 1, 3]), "You killed Bot 1 with weapon 3");
        assert_eq!(m(text::SUICIDE, [0, 0, 0]), "You commited suicide");
        assert_eq!(m(text::FIRST_BLOOD, [1, 0, 0]), "Bot 1 drew first blood");
        assert_eq!(m(text::KILL_LIMIT, [15, 1, 0]), "Kill limit 15 hit by worm Bot 1");
        assert_eq!(m(text::TIME_LIMIT, [0; 3]), "Timelimit has been reached");
        assert_eq!(m(text::SPREE[0], [7, 0, 0]), "worm 7 is on a killing spree!");
        assert_eq!(message_text(&Message { text: TextId(999), args: [0; 3] }, &n), None);
    }

    #[test]
    fn log_keeps_the_last_lines_oldest_first() {
        let mut log = Log::default();
        for i in 0..LOG_KEEP + 5 {
            log.push(LogLine::event(format!("line {i}")));
        }
        assert_eq!(log.lines().count(), LOG_KEEP);
        assert_eq!(log.texts().next(), Some("line 5"), "the oldest went");
        assert_eq!(log.texts().last(), Some(format!("line {}", LOG_KEEP + 4).as_str()));
        log.push(LogLine { text: "hi".into(), kind: LineKind::Chat });
        assert_eq!(log.lines().last().map(|l| l.kind), Some(LineKind::Chat));
        log.clear();
        assert_eq!(log.lines().count(), 0);
    }

    #[test]
    fn clock_and_lives_read_like_olx() {
        assert_eq!(clock(360_000), "6:00");
        assert_eq!(clock(59_001), "1:00");
        assert_eq!(clock(9_500), "0:10");
        assert_eq!(clock(0), "0:00");
        assert_eq!(lives_text(-2), "∞");
        assert_eq!(lives_text(-1), "out");
        assert_eq!(lives_text(3), "3");
    }
}
