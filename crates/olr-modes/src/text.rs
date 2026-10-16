//! The texts Deathmatch sends as `Message` events (`EventKind::Message`):
//! OLX's network texts (`NetworkTexts`, `src/client/Options.cpp:787-822`) and
//! the two game-over texts of `CGameMode::CheckGameOver`
//! (`src/server/CGameMode.cpp:298`, `309`).
//!
//! A message carries the text's id and three numbers (`Message::args`):
//! worm ids, a weapon id or a count, as each constant says. Unused numbers
//! are 0. [`olx_text`] gives OLX's default English text with its
//! placeholders, for a HUD to fill in.

use olr_sim::TextId;

/// `"<killer> killed <victim> with <weapon>"`: `[killer, victim, weapon]`.
/// The weapon is always -1 (unknown): `olr-sim` does not keep the weapon an
/// injury came from, and OLX drops `" with <weapon>"` when it is unknown.
pub const KILLED: TextId = TextId(1);
/// `"<player> commited suicide"` (sic): `[player, 0, 0]`.
pub const SUICIDE: TextId = TextId(2);
/// `"<player> drew first blood"`: `[player, 0, 0]`.
pub const FIRST_BLOOD: TextId = TextId(3);
/// `"<player> is out of the game"`: `[player, 0, 0]`.
pub const PLAYER_OUT: TextId = TextId(4);
/// `"<player> has won the match"`: `[player, 0, 0]`.
pub const PLAYER_HAS_WON: TextId = TextId(5);
/// `"Timelimit has been reached"`: no numbers.
pub const TIME_LIMIT: TextId = TextId(6);
/// `"Kill limit <limit> hit by worm <player>"`: `[limit, player, 0]`.
pub const KILL_LIMIT: TextId = TextId(7);
/// Killing sprees at `DeathmatchSettings::sprees[i]` kills in a row:
/// `[player, 0, 0]`.
pub const SPREE: [TextId; 5] = [TextId(10), TextId(11), TextId(12), TextId(13), TextId(14)];
/// Dying sprees at `DeathmatchSettings::dying_sprees[i]` deaths in a row:
/// `[player, 0, 0]`.
pub const DYING_SPREE: [TextId; 5] = [TextId(20), TextId(21), TextId(22), TextId(23), TextId(24)];

/// OLX's default text for `id`, with its placeholders (`<player>`,
/// `<killer>`, `<victim>`, `<weapon>`, `<limit>`), or `None` for an id
/// Deathmatch does not send.
pub fn olx_text(id: TextId) -> Option<&'static str> {
    Some(match id.0 {
        1 => "<killer> killed <victim> with <weapon>",
        2 => "<player> commited suicide",
        3 => "<player> drew first blood",
        4 => "<player> is out of the game",
        5 => "<player> has won the match",
        6 => "Timelimit has been reached",
        7 => "Kill limit <limit> hit by worm <player>",
        10 => "<player> is on a killing spree!",
        11 => "<player> is on a rampage!",
        12 => "<player> is dominating!",
        13 => "<player> is unstoppable!",
        14 => "<player> is GODLIKE!",
        20 => "<player> is on a dying spree!",
        21 => "<player> is a target!",
        22 => "<player> is a sitting duck!",
        23 => "<player> is free kills!",
        24 => "<player> is WORTHLESS!",
        _ => return None,
    })
}
