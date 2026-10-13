//! What the lobby's host and client share (networking spec §4, §5.2, §7.2):
//! the host's configuration, the events both report, the limits, and the
//! sanitizers that keep everything the lobby produces within the wire's
//! bounds (rl-proto's decoders refuse anything else).

use alloc::string::String;
use alloc::vec::Vec;

use rl_netcode::PeerId;
use rl_proto::{
    AdminCommand, Look, MAX_CHAT, MAX_MEMBERS, MAX_NAME, MatchEndWire, MatchStartWire, Notice, PublicKey,
    Refusal, valid_chat, valid_look, valid_name,
};

/// An unproven peer is dropped this long after its transport opened.
pub const PROOF_TIMEOUT_US: u64 = 10_000_000;
/// At most `CHAT_LINES` chat lines per member within `CHAT_WINDOW_US`.
pub const CHAT_LINES: usize = 5;
pub const CHAT_WINDOW_US: u64 = 5_000_000;
/// At most `LOOK_CHANGES` look changes per member within `LOOK_WINDOW_US`;
/// more are ignored.
pub const LOOK_CHANGES: usize = 4;
pub const LOOK_WINDOW_US: u64 = 1_000_000;
/// A member's download progress changes at most this often in the lobby.
pub const DOWNLOAD_EVERY_US: u64 = 250_000;
/// How long the host stays in `Phase::Results` before gathering again.
pub const RESULTS_US: u64 = 5_000_000;
/// This many wrong password proofs from one IP address within
/// [`PASSWORD_WINDOW_US`] hold that address off for [`PASSWORD_LOCKOUT_US`]
/// (`Refusal::Throttled`): guessing the password takes a minute per 3 tries.
pub const PASSWORD_TRIES: usize = 3;
pub const PASSWORD_WINDOW_US: u64 = 60_000_000;
pub const PASSWORD_LOCKOUT_US: u64 = 60_000_000;
/// At most this many addresses' wrong proofs are remembered (the oldest
/// forgotten first).
pub const PASSWORD_ADDRESSES: usize = 1024;
/// The skin of a look whose skin id is not a pack id (rl-game's default).
pub const DEFAULT_SKIN: &str = "olx-skin-default";
/// Notices to one member.
pub const SLOW_DOWN: &str = "slow down";
pub const NOT_ADMIN: &str = "only admins can do that";
pub const NOT_THE_HOST: &str = "the host cannot be kicked";
pub const MISSING_CONTENT: &str = "get the missing content first";

/// The host's lobby settings (spec §7.2, §7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostConfig {
    /// The lobby's name (sanitized; "Lobby" if nothing is left).
    pub name: String,
    pub password: Option<String>,
    /// Player seats, clamped to `1..=32`.
    pub max_players: u8,
    /// Spectators, at most 16.
    pub max_spectators: u8,
    /// Keys that may kick, ban and send admin commands.
    pub admins: Vec<PublicKey>,
}

/// What the host reports to its owner (the online layer).
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    /// A member was welcomed.
    Joined(u8),
    /// A member that sits a match out (waiting seat: it joined during the
    /// match, or lacked content at its start) now has its content
    /// complete: once per wait, from its `Have`. A mid-match joiner
    /// spectates now (not at `Joined`); one that never sends `Have` stays
    /// waiting.
    ContentComplete(u8),
    /// A member left (`Bye` or its transport closed).
    Left(u8),
    /// Close this peer's transport: a refusal, a timeout, a protocol
    /// violation or a kick (`ban` for a ban). A kicked member is gone from
    /// the lobby; no `Left` follows.
    Kick { peer: PeerId, ban: bool },
    /// Close this peer's transport: it said `Bye` (a member also gets
    /// `Left`).
    Close { peer: PeerId },
    /// A chat line relayed to everyone, the host's own included.
    Chat { from: u8, text: String },
    /// The lobby became startable: every player is ready (a host that
    /// starts when ready, the dedicated server, starts now).
    StartRequested,
    /// An admin's `Next` or `Settings`, for the owner to act on (the lobby
    /// does `Kick` and `Ban` itself).
    Admin { from: u8, cmd: AdminCommand },
}

/// What a client reports to its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientEvent {
    /// Joined as member `you`; `LobbyClient::state` holds the lobby.
    Welcome {
        you: u8,
    },
    Refused(Refusal),
    /// The lobby changed.
    Lobby(rl_proto::LobbyState),
    Chat {
        from: u8,
        text: String,
    },
    Notice(Notice),
    Start(MatchStartWire),
    Ended(MatchEndWire),
    Kicked {
        banned: bool,
    },
    /// The challenge named another host key than the one expected
    /// (nothing was sent; close the transport). The named key is not
    /// proven (see `LobbyClient::new`).
    WrongHost {
        key: rl_proto::PublicKey,
    },
    /// The host said goodbye, the transport closed, or the host broke the
    /// protocol (a welcome or lobby without this member).
    Closed,
}

/// Why the lobby cannot start a match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotStartable {
    /// Not gathering (a match is starting, running or showing results).
    Phase,
    /// No player: neither a member nor the host's own (a dedicated server
    /// with bots only).
    NoPlayers,
    /// Fewer than 2 worms, bots included.
    TooFewWorms,
    /// More than 32 worms, bots included.
    TooManyWorms,
    /// These players are not ready (or lack content).
    NotReady(Vec<u8>),
    /// `announce_start` with a start made before a later change.
    Stale,
}

/// A host-side input the wire could not carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyError {
    /// The match setup breaks the wire's bounds (rl-proto's `valid_setup`).
    InvalidSetup,
    /// The setup changes only while the lobby gathers.
    NotGathering,
}

impl core::fmt::Display for NotStartable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            NotStartable::Phase => "the last match is still ending",
            NotStartable::NoPlayers => "nobody plays",
            NotStartable::TooFewWorms => "two worms at least: add a bot",
            NotStartable::TooManyWorms => "32 worms at most",
            NotStartable::NotReady(_) => "not every player is ready",
            NotStartable::Stale => "the lobby changed",
        })
    }
}

impl core::fmt::Display for LobbyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            LobbyError::InvalidSetup => "the setup is out of bounds",
            LobbyError::NotGathering => "the setup changes only between matches",
        })
    }
}

/// Whether `c` may stand inside a name.
fn name_char(c: char) -> bool {
    let mut buf = [0u8; 12];
    let s = {
        let mut n = 0;
        for x in ['a', c, 'a'] {
            n += x.encode_utf8(&mut buf[n..]).len();
        }
        &buf[..n]
    };
    core::str::from_utf8(s).is_ok_and(|s| valid_name(s, 3))
}

/// A name the wire accepts: forbidden characters removed, edges trimmed,
/// cut to `MAX_NAME` characters; `fallback` if nothing is left.
pub fn sanitize_name(name: &str, fallback: &str) -> String {
    let kept: String = name.chars().filter(|&c| c == ' ' || name_char(c)).collect();
    let cut: String = kept.trim().chars().take(MAX_NAME).collect();
    let cut = cut.trim_end();
    if valid_name(cut, MAX_NAME) { cut.into() } else { fallback.into() }
}

/// A look the wire accepts: its name sanitized ("Player" if empty), its skin
/// `DEFAULT_SKIN` if not a pack id.
pub fn sanitize_look(look: &Look) -> Look {
    let mut l = Look {
        name: sanitize_name(&look.name, "Player"),
        colour: look.colour,
        skin: look.skin.clone(),
        skin_hash: look.skin_hash.clone(),
    };
    if !valid_look(&l) {
        l.skin = DEFAULT_SKIN.into();
        l.skin_hash = rl_proto::NO_HASH.into();
    }
    l
}

/// A chat line the wire accepts: control characters and bidi controls
/// removed, cut to `MAX_CHAT` characters; `None` if nothing visible is left.
pub fn sanitize_chat(text: &str) -> Option<String> {
    let mut buf = [0u8; 4];
    let line: String = text.chars().filter(|c| valid_chat(c.encode_utf8(&mut buf))).take(MAX_CHAT).collect();
    if line.trim().is_empty() { None } else { Some(line) }
}

/// `name`, or with the lowest suffix " (n)", n ≥ 2, that makes it differ
/// ignoring case from every name in `taken`; the base is cut so the
/// result keeps within `MAX_NAME` characters. With at most `MAX_MEMBERS`
/// taken names one of the first `MAX_MEMBERS + 1` suffixes is free.
pub fn unique_name<'a>(name: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    let lower = |s: &str| s.to_lowercase();
    let free = |candidate: &str| {
        let c = lower(candidate);
        taken.clone().all(|t| lower(t) != c)
    };
    if free(name) {
        return name.into();
    }
    for n in 2..=MAX_MEMBERS + 2 {
        let suffix = alloc::format!(" ({n})");
        let room = MAX_NAME.saturating_sub(suffix.chars().count());
        let base: String = name.chars().take(room).collect();
        let candidate = alloc::format!("{}{suffix}", base.trim_end());
        if free(&candidate) {
            return candidate;
        }
    }
    name.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    /// Errors read as words, never as their Rust names.
    #[test]
    fn errors_read_as_words() {
        let all = [
            format!("{}", LobbyError::InvalidSetup),
            format!("{}", LobbyError::NotGathering),
            format!("{}", NotStartable::Phase),
            format!("{}", NotStartable::NoPlayers),
            format!("{}", NotStartable::TooFewWorms),
            format!("{}", NotStartable::TooManyWorms),
            format!("{}", NotStartable::NotReady(alloc::vec![1, 2])),
            format!("{}", NotStartable::Stale),
        ];
        assert_eq!(all[1], "the setup changes only between matches");
        for text in all {
            assert!(
                text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || " :,".contains(c)),
                "{text}"
            );
        }
    }

    #[test]
    fn names_are_made_valid() {
        assert_eq!(sanitize_name("  Ann\u{7}ie \u{202E}", "P"), "Annie");
        assert_eq!(sanitize_name("a b", "P"), "a b");
        assert_eq!(sanitize_name("\u{200B}\u{7}", "P"), "P");
        assert_eq!(sanitize_name(&"n".repeat(30), "P"), "n".repeat(MAX_NAME));
        assert_eq!(sanitize_name(&alloc::format!("{} x", "n".repeat(19)), "P"), "n".repeat(19));
    }

    #[test]
    fn looks_get_a_default_skin() {
        let l = sanitize_look(&Look {
            name: String::new(),
            colour: [1, 2, 3],
            skin: "Bad/Skin".into(),
            skin_hash: rl_proto::NO_HASH.into(),
        });
        assert_eq!(
            l,
            Look {
                name: "Player".into(),
                colour: [1, 2, 3],
                skin: DEFAULT_SKIN.into(),
                skin_hash: rl_proto::NO_HASH.into()
            }
        );
    }

    #[test]
    fn chat_is_cleaned_and_cut() {
        assert_eq!(sanitize_chat("a\u{7}\u{2066}b"), Some("ab".into()));
        assert_eq!(sanitize_chat(" \u{7} "), None);
        assert_eq!(sanitize_chat(&"é".repeat(300)).unwrap().chars().count(), MAX_CHAT);
    }

    #[test]
    fn suffixes_skip_taken_ones() {
        let taken = ["Ann", "ann (2)"];
        assert_eq!(unique_name("ANN", taken.iter().copied()), "ANN (3)");
        assert_eq!(unique_name("Bob", taken.iter().copied()), "Bob");
    }
}
