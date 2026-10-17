//! How each worm looked in the match (presentation only: never part of the
//! simulation, its setup or its checksums): name, colour and skin.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// The longest name, in characters (olr-ui's profile limit and olr-proto's
/// `MAX_NAME`).
pub const MAX_NAME_CHARS: usize = 20;

/// The longest skin pack id, in bytes (olr-proto's `MAX_ID`).
pub const MAX_SKIN_ID: usize = 64;

/// OLX's default skin pack, the skin of every bot (`olr_content::DEFAULT_SKIN`).
pub const DEFAULT_SKIN: &str = "olx-skin-default";

/// The "content hash unknown" of a bot's skin (`olr_content::NO_SKIN_HASH`,
/// olr-proto's `NO_HASH`).
pub const NO_SKIN_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// One worm's look: what a watcher sees to tell the players apart. The skin
/// is named by pack id and content hash, never stored (a skin is never
/// downloaded: a viewer without it draws the default skin).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WormLook {
    pub name: String,
    pub colour: [u8; 3],
    /// A skin pack id.
    pub skin: String,
    /// The skin pack's content hash (64 lowercase hex), [`NO_SKIN_HASH`]
    /// for a bot's.
    pub skin_hash: String,
}

impl WormLook {
    /// A look with the default skin and no known hash.
    pub fn plain(name: &str, colour: [u8; 3]) -> WormLook {
        WormLook { name: name.into(), colour, skin: DEFAULT_SKIN.into(), skin_hash: NO_SKIN_HASH.into() }
    }

    /// "Worm N" (`i` counts from 0) in grey with the default skin: what
    /// a recorder that knows nothing about its worms records.
    pub fn numbered(i: usize) -> WormLook {
        WormLook::plain(&format!("Worm {}", i + 1), [160, 160, 160])
    }

    /// Whether the skin is a pack id and a content hash a decoder accepts.
    pub(crate) fn skin_is_valid(&self) -> bool {
        valid_skin(&self.skin, &self.skin_hash)
    }

    /// This look for worm `i`: the name through [`sanitise_name`], and a
    /// skin that is no pack id with a content hash replaced by the default
    /// skin (a recorder never writes a replay that does not decode).
    pub fn sanitised(self, i: usize) -> WormLook {
        let name = sanitise_name(&self.name, i);
        if self.skin_is_valid() {
            WormLook { name, ..self }
        } else {
            WormLook { name, skin: DEFAULT_SKIN.into(), skin_hash: NO_SKIN_HASH.into(), ..self }
        }
    }
}

/// `looks` made `n` long and sanitised: extra looks are dropped and missing
/// ones are [`WormLook::numbered`].
pub(crate) fn fit(looks: Vec<WormLook>, n: usize) -> Vec<WormLook> {
    let mut looks: Vec<WormLook> =
        looks.into_iter().take(n).enumerate().map(|(i, l)| l.sanitised(i)).collect();
    looks.extend((looks.len()..n).map(WormLook::numbered));
    looks
}

/// Bidi overrides, isolates and marks.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Control characters and the format and separator code points that make a
/// name ambiguous: olr-proto's `valid_name` refuses exactly these in a
/// lobby (olr-replay is a lower layer and cannot call it; olr-online's
/// tests check that this agrees with it).
fn is_hidden(c: char) -> bool {
    c.is_control()
        || is_bidi(c)
        || matches!(
            c,
            '\u{00AD}'
                | '\u{200B}'..='\u{200D}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{2060}'..='\u{2064}'
                | '\u{FEFF}'
        )
}

/// A name a lobby would accept: no control, hidden or bidi character,
/// trimmed, at most [`MAX_NAME_CHARS`] characters (cut, then trimmed
/// again); "Worm N" for worm `i` when nothing is left.
pub fn sanitise_name(name: &str, i: usize) -> String {
    let kept: String = name.chars().filter(|&c| !is_hidden(c)).collect();
    let cut: String = kept.trim().chars().take(MAX_NAME_CHARS).collect();
    let cut = cut.trim();
    if cut.is_empty() { format!("Worm {}", i + 1) } else { cut.into() }
}

/// Whether `id` and `hash` are a skin pack id and a content hash.
pub(crate) fn valid_skin(id: &str, hash: &str) -> bool {
    valid_pack_id(id) && valid_hash(hash)
}

/// A pack id: ASCII `[a-z0-9-]`, 1 to [`MAX_SKIN_ID`] bytes.
fn valid_pack_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SKIN_ID
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 64 lowercase hex digits.
fn valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned_and_cut() {
        assert_eq!(sanitise_name("  Ann\u{7}\u{202E}x \n", 0), "Annx");
        assert_eq!(sanitise_name(&"é".repeat(50), 0).chars().count(), MAX_NAME_CHARS);
        assert_eq!(sanitise_name("\u{200B}\t ", 2), "Worm 3");
        assert_eq!(sanitise_name("abcdefghijklmnopqrs tuv", 0), "abcdefghijklmnopqrs");
    }

    #[test]
    fn a_bad_skin_becomes_the_default() {
        let l = WormLook { name: "a".into(), colour: [1, 2, 3], skin: "Bad Skin".into(), skin_hash: "x".into() };
        let l = l.sanitised(0);
        assert_eq!((l.skin.as_str(), l.skin_hash.as_str()), (DEFAULT_SKIN, NO_SKIN_HASH));
        assert_eq!(l.colour, [1, 2, 3]);
        assert!(WormLook::plain("a", [0; 3]).skin_is_valid());
    }
}
