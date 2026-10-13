//! Chat commands (networking spec §7.4): admins' `/kick <name>`,
//! `/ban <name>` (the key, spec §5.2), `/banip <name>` (the key and the
//! address), `/unban <name|key prefix|address>`, `/next`,
//! `/settings <field> <value>`; anyone's `/players` and `/help`; `/invite`
//! (anyone's in a public game, else the admins'). Parsed
//! defensively: a chat line is at most 200 characters, a name at most
//! `MAX_NAME`, a value at most `MAX_VALUE`, an unban's argument at most
//! `MAX_UNBAN`.

use std::net::IpAddr;

use rl_proto::{MAX_NAME, PublicKey};

use crate::files::hex;

/// The longest value `/settings` reads.
pub const MAX_VALUE: usize = 16;
/// The longest argument `/unban` reads (a whole key).
pub const MAX_UNBAN: usize = 64;
/// A key prefix `/unban` takes has at least this many hex characters.
pub const MIN_KEY_PREFIX: usize = 8;

/// A command from the chat.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// A member by name (any case).
    Kick(String),
    /// Bans the member's key.
    Ban(String),
    /// Bans the member's key and address.
    BanIp(String),
    /// Lifts a ban: a banned name, a key prefix or an address.
    Unban(String),
    /// Ends the match, or skips to the next map while gathering.
    Next,
    Settings(Setting),
    Players,
    Help,
    /// The server's invite code.
    Invite,
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
    /// Only admins may. The invite code is no secret in a `public` game
    /// (its note is), so anyone may ask for it there.
    pub fn admin_only(&self, public: bool) -> bool {
        match self {
            Command::Players | Command::Help => false,
            Command::Invite => !public,
            _ => true,
        }
    }
}

/// The help, one notice a line: the commands the asker may use (in a
/// `public` game, or not).
pub fn help(admin: bool, public: bool) -> Vec<&'static str> {
    let mut lines = vec!["/players: who is here", "/help: these commands"];
    if admin || public {
        lines.push("/invite: the server's invite code");
    }
    if admin {
        lines.extend([
            "/kick <name>, /ban <name>: remove a member (a ban keeps its key out)",
            "/banip <name>: ban its key and its address; /unban <name|key prefix|address>",
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
        "banip" => name(arg, "banip").map(Command::BanIp),
        "unban" => match arg.chars().count() {
            0 => Err("usage: /unban <name|key prefix|address>".into()),
            n if n > MAX_UNBAN => Err("no ban matches that".into()),
            _ => Ok(Command::Unban(arg.to_string())),
        },
        "next" => Ok(Command::Next),
        "settings" => setting(arg).map(Command::Settings),
        "players" => Ok(Command::Players),
        "help" => Ok(Command::Help),
        "invite" => Ok(Command::Invite),
        _ => Err(format!("unknown command /{}: try /help", word.chars().take(MAX_NAME).collect::<String>())),
    })
}

/// What `/unban <arg>` lifts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unban {
    Address(IpAddr),
    /// The key (and the address its ban recorded).
    Key(PublicKey),
}

/// The ban `/unban <arg>` names among `banned` (each key with the name it
/// was banned under, if known): an address, else a name (any case), else
/// a key prefix of at least [`MIN_KEY_PREFIX`] hex characters; exactly
/// one must match.
pub fn unban_target(arg: &str, banned: &[(PublicKey, Option<&str>)]) -> Result<Unban, String> {
    if let Ok(ip) = arg.parse::<IpAddr>() {
        return Ok(Unban::Address(ip));
    }
    let lower = arg.to_lowercase();
    let by_name: Vec<PublicKey> =
        banned.iter().filter(|(_, n)| n.is_some_and(|n| n.to_lowercase() == lower)).map(|b| b.0).collect();
    let matches = if by_name.is_empty()
        && lower.len() >= MIN_KEY_PREFIX
        && lower.chars().all(|c| c.is_ascii_hexdigit())
    {
        banned.iter().map(|b| b.0).filter(|k| hex(k).starts_with(&lower)).collect()
    } else {
        by_name
    };
    match matches[..] {
        [key] => Ok(Unban::Key(key)),
        [] => Err(format!("no ban matches {arg} (a name, {MIN_KEY_PREFIX}+ hex of a key, or an address)")),
        _ => Err(format!("several bans match {arg}: give more of the key")),
    }
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
        assert_eq!(parse("/BanIP Bob"), Some(Ok(Command::BanIp("Bob".into()))));
        assert_eq!(parse("/unban 10.0.0.1"), Some(Ok(Command::Unban("10.0.0.1".into()))));
        assert_eq!(parse("/next"), Some(Ok(Command::Next)));
        assert_eq!(parse("/players"), Some(Ok(Command::Players)));
        assert_eq!(parse("/help me"), Some(Ok(Command::Help)));
        assert_eq!(parse("/Invite"), Some(Ok(Command::Invite)));
        assert_eq!(parse("/settings Kill_Limit 3"), Some(Ok(Command::Settings(Setting::KillLimit(3)))));
        assert_eq!(parse("/settings time_limit 2.5"), Some(Ok(Command::Settings(Setting::TimeLimit(2.5)))));
        assert_eq!(parse("/settings lives -2"), Some(Ok(Command::Settings(Setting::Lives(-2)))));
        assert_eq!(parse("/settings bonuses OFF"), Some(Ok(Command::Settings(Setting::Bonuses(false)))));
    }

    #[test]
    fn bad_commands_say_why() {
        for line in [
            "/kick",
            "/banip",
            "/unban",
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
    fn unban_finds_one_ban() {
        let (a, b): ([u8; 32], [u8; 32]) = ([0xab; 32], [0xac; 32]);
        let banned = [(a, Some("Bob")), (b, None)];
        assert_eq!(unban_target("bob", &banned), Ok(Unban::Key(a)));
        assert_eq!(unban_target("acacacac", &banned), Ok(Unban::Key(b)));
        assert_eq!(unban_target(&hex(&a), &banned), Ok(Unban::Key(a)));
        assert_eq!(unban_target("10.0.0.7", &banned), Ok(Unban::Address("10.0.0.7".parse().unwrap())));
        assert_eq!(unban_target("::1", &banned), Ok(Unban::Address("::1".parse().unwrap())));
        assert!(unban_target("acac", &banned).is_err(), "too short a prefix");
        assert!(unban_target("Cid", &banned).is_err());
        let twice = [(a, Some("Bob")), (b, Some("BOB"))];
        assert!(unban_target("bob", &twice).is_err(), "ambiguous");
        assert!(unban_target("a", &[(a, None), ([0xaa; 32], None)]).is_err());
    }

    #[test]
    fn help_offers_admins_more() {
        assert!(help(false, true).iter().all(|l| !l.contains("/kick")));
        assert!(help(true, false).iter().any(|l| l.contains("/kick")));
        assert!(help(true, true).iter().all(|l| l.chars().count() <= rl_proto::MAX_CHAT));
        // The invite: anyone's in a public game, else the admins'.
        let offers = |admin, public| help(admin, public).iter().any(|l| l.contains("/invite"));
        assert!(offers(false, true) && offers(true, true) && offers(true, false));
        assert!(!offers(false, false));
    }

    #[test]
    fn the_invite_is_anyones_only_in_a_public_game() {
        assert!(!Command::Invite.admin_only(true));
        assert!(Command::Invite.admin_only(false));
        assert!(!Command::Players.admin_only(false));
        assert!(Command::Next.admin_only(true));
    }
}
