//! Chat commands (networking spec §7.4): admins' `/kick <name>`,
//! `/ban <name>`, `/next`, `/settings <field> <value>`; anyone's
//! `/players` and `/help`. Parsed defensively: a chat line is at most 200
//! characters, a name at most `MAX_NAME`, a value at most `MAX_VALUE`.

use rl_proto::MAX_NAME;

/// The longest value `/settings` reads.
pub const MAX_VALUE: usize = 16;

/// A command from the chat.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// A member by name (any case).
    Kick(String),
    Ban(String),
    /// Ends the match, or skips to the next map while gathering.
    Next,
    Settings(Setting),
    Players,
    Help,
}

/// A setting of the next match.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    /// Minutes; 0 or less: none.
    TimeLimit(f32),
    KillLimit(i32),
    Lives(i32),
    Bonuses(bool),
}

impl Command {
    /// Only admins may.
    pub fn admin_only(&self) -> bool {
        !matches!(self, Command::Players | Command::Help)
    }
}

/// The help, one notice a line: the commands the asker may use.
pub fn help(admin: bool) -> Vec<&'static str> {
    let mut lines = vec!["/players: who is here", "/help: these commands"];
    if admin {
        lines.extend([
            "/kick <name>, /ban <name>: remove a member (a ban keeps its key and address out)",
            "/next: end the match, or skip to the next map",
            "/settings <field> <value>: time_limit (minutes), kill_limit, lives, bonuses (on/off)",
        ]);
    }
    lines
}

/// The command in a chat line: `None` when it is not one (no leading
/// `/`), else the command or why it is not understood.
pub fn parse(line: &str) -> Option<Result<Command, String>> {
    let rest = line.trim().strip_prefix('/')?;
    let (word, arg) = match rest.split_once(char::is_whitespace) {
        Some((w, a)) => (w, a.trim()),
        None => (rest, ""),
    };
    let word = word.to_lowercase();
    Some(match word.as_str() {
        "kick" => name(arg, "kick").map(Command::Kick),
        "ban" => name(arg, "ban").map(Command::Ban),
        "next" => Ok(Command::Next),
        "settings" => setting(arg).map(Command::Settings),
        "players" => Ok(Command::Players),
        "help" => Ok(Command::Help),
        _ => Err(format!("unknown command /{}: try /help", word.chars().take(MAX_NAME).collect::<String>())),
    })
}

fn name(arg: &str, cmd: &str) -> Result<String, String> {
    if arg.is_empty() {
        Err(format!("usage: /{cmd} <name>"))
    } else if arg.chars().count() > MAX_NAME {
        Err("no member has a name that long".into())
    } else {
        Ok(arg.to_string())
    }
}

fn setting(arg: &str) -> Result<Setting, String> {
    const USAGE: &str = "usage: /settings <time_limit|kill_limit|lives|bonuses> <value>";
    let mut words = arg.split_whitespace();
    let (Some(field), Some(value), None) = (words.next(), words.next(), words.next()) else {
        return Err(USAGE.into());
    };
    let field = field.to_lowercase();
    if value.chars().count() > MAX_VALUE {
        return Err(format!("{field}: that value is too long"));
    }
    let bad = |what: &str| format!("{field}: {what}");
    match field.as_str() {
        "time_limit" => match value.parse::<f32>() {
            Ok(m) if m.is_finite() => Ok(Setting::TimeLimit(m)),
            _ => Err(bad("minutes, such as 5 or 2.5")),
        },
        "kill_limit" => value.parse().map(Setting::KillLimit).map_err(|_| bad("a whole number")),
        "lives" => value.parse().map(Setting::Lives).map_err(|_| bad("a whole number (below 0: unlimited)")),
        "bonuses" => match value.to_lowercase().as_str() {
            "on" | "true" | "yes" | "1" => Ok(Setting::Bonuses(true)),
            "off" | "false" | "no" | "0" => Ok(Setting::Bonuses(false)),
            _ => Err(bad("on or off")),
        },
        _ => Err(format!("unknown setting {}: {USAGE}", field.chars().take(MAX_NAME).collect::<String>())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse() {
        assert_eq!(parse("hello"), None);
        assert_eq!(parse(" /KICK  Anna (2) "), Some(Ok(Command::Kick("Anna (2)".into()))));
        assert_eq!(parse("/ban Bob"), Some(Ok(Command::Ban("Bob".into()))));
        assert_eq!(parse("/next"), Some(Ok(Command::Next)));
        assert_eq!(parse("/players"), Some(Ok(Command::Players)));
        assert_eq!(parse("/help me"), Some(Ok(Command::Help)));
        assert_eq!(parse("/settings Kill_Limit 3"), Some(Ok(Command::Settings(Setting::KillLimit(3)))));
        assert_eq!(parse("/settings time_limit 2.5"), Some(Ok(Command::Settings(Setting::TimeLimit(2.5)))));
        assert_eq!(parse("/settings lives -2"), Some(Ok(Command::Settings(Setting::Lives(-2)))));
        assert_eq!(parse("/settings bonuses OFF"), Some(Ok(Command::Settings(Setting::Bonuses(false)))));
    }

    #[test]
    fn bad_commands_say_why() {
        for line in [
            "/kick",
            "/kick ThisNameIsFarTooLongToBeAnyMembers",
            "/settings",
            "/settings lives",
            "/settings lives 1 2",
            "/settings lives many",
            "/settings time_limit NaN",
            "/settings time_limit inf",
            "/settings kill_limit 99999999999999999999",
            "/settings speed 3",
            "/dance",
            "/",
        ] {
            assert!(matches!(parse(line), Some(Err(_))), "{line:?}");
        }
    }

    #[test]
    fn any_line_parses_without_panicking() {
        for line in ["/\u{0}", "/kick \u{202e}x", "/settings é é", &"/".repeat(300), "/ kick x"] {
            let _ = parse(line);
        }
    }

    #[test]
    fn help_offers_admins_more() {
        assert!(help(false).iter().all(|l| !l.contains("/kick")));
        assert!(help(true).iter().any(|l| l.contains("/kick")));
        assert!(help(true).iter().all(|l| l.chars().count() <= rl_proto::MAX_CHAT));
    }
}
