//! Scripted sessions (`--script`, `--demo`): key presses and commands at
//! given ticks, so the game can be run and checked without a person at the
//! keyboard (plan 5 ruling R3).
//!
//! One command per line, `<tick> <command>`; `#` starts a comment:
//!
//! - `+<action>` / `-<action>` presses or releases the key bound to an
//!   action (`up`, `down`, `left`, `right`, `shoot`, `jump`, `rope`,
//!   `select`), through the same input path as the keyboard;
//! - `shot` saves a screenshot after that tick, `restart` is F5 (a
//!   Deathmatch's rematch), `debug` F2, `quit` Esc, `scores on` / `scores
//!   off` holds or releases Tab (the scoreboard);
//! - `size <w> <h>` asks for a window of `w` × `h` physical pixels,
//!   `fullscreen on` / `fullscreen off` for borderless fullscreen or a
//!   window (both apply at the start of the next frame, the latest wins).

use rl_input::Action;

/// What a script line does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Press(Action),
    Release(Action),
    Shot,
    Restart,
    Debug,
    /// The scoreboard shown (Tab held) or not.
    Scores(bool),
    Quit,
    Size(u32, u32),
    Fullscreen(bool),
}

/// A command at a tick (ticks count from the start, across restarts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub tick: u64,
    pub command: Command,
}

fn action(name: &str) -> Option<Action> {
    Some(match name {
        "up" => Action::Up,
        "down" => Action::Down,
        "left" => Action::Left,
        "right" => Action::Right,
        "shoot" => Action::Shoot,
        "jump" => Action::Jump,
        "rope" => Action::Rope,
        "select" => Action::SelectWeapon,
        _ => return None,
    })
}

/// Parses a script; the steps come out in tick order (lines of one tick in
/// their order). An unknown command is an error naming its line.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = || format!("line {}: {line:?} is not `<tick> <command>`", n + 1);
        let (tick, cmd) = line.split_once(char::is_whitespace).ok_or_else(bad)?;
        let tick: u64 = tick.parse().map_err(|_| bad())?;
        let cmd = cmd.trim();
        let command = match cmd {
            "shot" => Command::Shot,
            "restart" => Command::Restart,
            "debug" => Command::Debug,
            "quit" => Command::Quit,
            "fullscreen on" => Command::Fullscreen(true),
            "fullscreen off" => Command::Fullscreen(false),
            "scores on" => Command::Scores(true),
            "scores off" => Command::Scores(false),
            _ if cmd.starts_with("size ") => {
                let mut wh = cmd["size ".len()..].split_whitespace().map(str::parse::<u32>);
                match (wh.next(), wh.next(), wh.next()) {
                    (Some(Ok(w)), Some(Ok(h)), None) => Command::Size(w, h),
                    _ => return Err(bad()),
                }
            }
            _ => match cmd.split_at_checked(1) {
                Some(("+", a)) => Command::Press(action(a).ok_or_else(bad)?),
                Some(("-", a)) => Command::Release(action(a).ok_or_else(bad)?),
                _ => return Err(bad()),
            },
        };
        steps.push(Step { tick, command });
    }
    steps.sort_by_key(|s| s.tick);
    Ok(steps)
}

/// The built-in demo (`--demo`), 24 s, with the F2 panel on: dig right and
/// left (OLX digs when the other direction is tapped while walking), jump,
/// throw the rope up and swing, shoot weapon 1, switch to weapon 5 and
/// shoot it, aim straight up and shoot each weapon in turn until what comes
/// down kills the worm, and respawn (jump). Screenshots along the way.
pub const DEMO: &str = "
10 debug
# dig right: hold right, tap left
20 +right
30 +left
33 -left
42 +left
45 -left
54 +left
57 -left
66 +left
69 -left
78 +left
81 -left
90 +left
93 -left
102 +left
105 -left
114 +left
117 -left
126 +left
129 -left
138 +left
141 -left
140 shot
145 -right
# dig left: hold left, tap right
150 +left
160 +right
163 -right
172 +right
175 -right
184 +right
187 -right
196 +right
199 -right
208 +right
211 -right
220 +right
223 -right
232 +right
235 -right
244 +right
247 -right
256 +right
259 -right
260 shot
265 -left
# jump, aim up, throw the rope and swing on it, let go with a jump
270 +jump
280 -jump
285 +up
320 -up
325 +rope
330 -rope
345 +right
380 shot
400 -right
420 +jump
430 -jump
# shoot weapon 1 straight ahead
440 +shoot
480 shot
520 -shoot
# hold select and tap left: weapon 5, and shoot it
530 +select
535 +left
540 -left
545 -select
550 +shoot
600 shot
640 -shoot
# aim straight up and shoot every weapon in turn: what goes up comes down
650 +up
750 -up
760 +shoot
880 -shoot
885 +select
890 +right
895 -right
900 -select
910 +shoot
1030 -shoot
1035 +select
1040 +right
1045 -right
1050 -select
1060 +shoot
1180 -shoot
1185 +select
1190 +right
1195 -right
1200 -select
1210 +shoot
1330 -shoot
1335 +select
1340 +right
1345 -right
1350 -select
1360 +shoot
1480 -shoot
1485 +select
1490 +right
1495 -right
1500 -select
1510 +shoot
1630 -shoot
1635 +select
1640 +right
1645 -right
1650 -select
1660 +shoot
1780 -shoot
1785 +select
1790 +right
1795 -right
1800 -select
1810 +shoot
1930 -shoot
1935 +select
1940 +right
1945 -right
1950 -select
900 shot
1300 shot
1700 shot
# respawn when allowed (jump), then a last look
1960 +jump
1970 -jump
2020 +jump
2030 -jump
2080 +jump
2090 -jump
2140 +jump
2150 -jump
2200 +jump
2210 -jump
2260 +jump
2270 -jump
2350 shot
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse_in_tick_order() {
        let s = parse(
            "# a comment\n10 +right\n5 shot  # trailing\n\n10 -right\n7 restart\n9 quit\n8 size 640 480\n8 debug\n9 fullscreen on\n11 scores on\n",
        )
        .unwrap();
        let c: Vec<_> = s.iter().map(|s| (s.tick, s.command)).collect();
        assert_eq!(
            c,
            [
                (5, Command::Shot),
                (7, Command::Restart),
                (8, Command::Size(640, 480)),
                (8, Command::Debug),
                (9, Command::Quit),
                (9, Command::Fullscreen(true)),
                (10, Command::Press(Action::Right)),
                (10, Command::Release(Action::Right)),
                (11, Command::Scores(true)),
            ]
        );
        for bad in [
            "x +right",
            "10",
            "10 +fly",
            "10 jump",
            "-1 shot",
            "10 size 5",
            "10 size a b",
            "10 size 1 2 3",
            "10 fullscreen",
            "10 fullscreen yes",
            "10 scores",
        ] {
            assert!(parse(bad).unwrap_err().contains("line 1"), "{bad}");
        }
        assert!(parse(DEMO).unwrap().len() > 30);
    }
}
