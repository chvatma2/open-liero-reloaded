//! What the match HUD says (spec §8.3; OLX's HUD:
//! `docs/porting/olx-notes/presentation.md` §9, its results screen:
//! `bots-and-deathmatch.md` §3.4): the worms' names, the kill feed from
//! the mode's `Message` events, the time-limit clock, the scoreboard rows
//! and the results of a finished match.

use rl_modes::text;
use rl_sim::{EndReason, Message, Score, World};

/// OLX's local chat shows the last 6 lines (`DrawLocalChat`,
/// `CClient_Draw.cpp:2211-2236`).
pub const FEED_LINES: usize = 6;

/// ... each for 3 s (300 ticks).
pub const FEED_TICKS: u64 = 300;

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

/// The kill feed: the lines of the last [`FEED_TICKS`], at most
/// [`FEED_LINES`], oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feed {
    lines: Vec<(u64, String)>,
}

impl Feed {
    /// A line said at tick `tick`.
    pub fn push(&mut self, tick: u64, line: String) {
        self.lines.push((tick, line));
        if self.lines.len() > FEED_LINES {
            self.lines.remove(0);
        }
    }

    /// The lines still shown at tick `now`.
    pub fn visible(&self, now: u64) -> impl Iterator<Item = &str> {
        self.lines.iter().filter(move |(t, _)| now < t + FEED_TICKS).map(|(_, l)| l.as_str())
    }

    /// Every line kept, shown or not (tests).
    pub fn all(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(|(_, l)| l.as_str())
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
}

/// The scoreboard: the mode's rows (`World::scores`, best first).
pub fn rows(scores: &[Score], names: &Names) -> Vec<Row> {
    scores
        .iter()
        .map(|s| Row {
            worm: s.worm,
            name: names.of(s.worm),
            score: s.score,
            lives: lives_text(s.lives),
            kills: s.kills,
            deaths: s.deaths,
            suicides: s.suicides,
            damage: rl_modes::olx_round(s.damage),
        })
        .collect()
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
    /// The results of `world`'s match, once it ended.
    pub fn of(world: &World, names: &Names, time_left_ms: Option<u64>) -> Option<Results> {
        let r = world.result()?;
        let reason = match r.reason {
            EndReason::KillLimit => "kill limit".to_string(),
            EndReason::TimeLimit => "time limit".to_string(),
            EndReason::LastStanding => "last worm standing".to_string(),
            EndReason::Other(n) => format!("the mode's reason {n}"),
        };
        Some(Results {
            winner: (r.winner >= 0).then(|| names.of(r.winner)),
            reason,
            rows: rows(world.scores(), names),
            time_left_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rl_sim::TextId;

    fn names() -> Names {
        Names(vec!["You".into(), "Bot 1".into()])
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
    fn feed_keeps_six_lines_for_three_seconds() {
        let mut f = Feed::default();
        for i in 0..8 {
            f.push(i * 10, format!("line {i}"));
        }
        assert_eq!(f.all().count(), FEED_LINES);
        assert_eq!(f.visible(70).next(), Some("line 2"));
        // Line 2 (tick 20) is gone at tick 320; line 7 (tick 70) at 370.
        assert_eq!(f.visible(320).next(), Some("line 3"));
        assert_eq!(f.visible(370).count(), 0);
        f.clear();
        assert_eq!(f.all().count(), 0);
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
