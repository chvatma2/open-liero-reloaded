//! Content download messages (plan 11, N32): the match's content list and
//! the resumable chunked transfer of pack archives, on the reliable channel
//! under `TAG_CONTENT`.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::lobby::valid_pack_id;
use crate::{ProtoError, decode_exact};

/// Reliable-channel tag of a `ContentMsg`.
pub const TAG_CONTENT: u8 = 3;
/// Archive bytes in one `Chunk`.
pub const CONTENT_CHUNK: usize = 32 * 1024;
/// Largest accepted content message: a full chunk or a full list.
pub const MAX_CONTENT: usize = CONTENT_CHUNK + 256;
/// Most items in a `List`.
pub const MAX_ITEMS: usize = 64;
/// Largest archive, restating rl-pack's `MAX_ARCHIVE` (rl-proto cannot
/// depend on rl-pack; a test in rl-online asserts the two are equal).
pub const MAX_ARCHIVE_BYTES: u64 = 512 << 20;
/// Largest skin archive in a `List`.
pub const MAX_SKIN_BYTES: u64 = 64 * 1024;
/// "Unknown" hash: valid, and never fetched.
pub const NO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Mod,
    Level,
    Theme,
    Skin,
}

/// One pack of the match's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: ItemKind,
    pub id: String,
    /// Content hash (64 lowercase hex).
    pub hash: String,
    /// Archive size in bytes.
    pub size: u64,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentMsg {
    /// The match's content (host to client), whole, on every change.
    List(Vec<Item>),
    /// Send the archive of `hash` from byte `offset`.
    Want { hash: String, offset: u64 },
    /// The archive follows: its size and its SHA-256 (hex), from `offset`.
    Begin { hash: String, archive: String, size: u64, offset: u64 },
    /// Archive bytes at `offset` (at most `CONTENT_CHUNK`).
    Chunk { hash: String, offset: u64, bytes: Vec<u8> },
    /// This peer will not send `hash` (not listed, failed, too large).
    Unavailable { hash: String },
    /// Stop sending `hash`.
    Cancel { hash: String },
}

/// 64 lowercase hex characters.
pub fn valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A message within the wire's bounds.
pub fn valid_content(m: &ContentMsg) -> bool {
    match m {
        ContentMsg::List(items) => {
            items.len() <= MAX_ITEMS
                && items.iter().enumerate().all(|(i, it)| {
                    valid_pack_id(&it.id)
                        && valid_hash(&it.hash)
                        && it.size <= MAX_ARCHIVE_BYTES
                        && (it.kind != ItemKind::Skin || (it.size <= MAX_SKIN_BYTES && !it.required))
                        && items[..i].iter().all(|o| o.hash != it.hash)
                })
        }
        ContentMsg::Want { hash, .. } | ContentMsg::Unavailable { hash } | ContentMsg::Cancel { hash } => {
            valid_hash(hash)
        }
        ContentMsg::Begin { hash, archive, size, offset } => {
            valid_hash(hash) && valid_hash(archive) && *size <= MAX_ARCHIVE_BYTES && offset <= size
        }
        ContentMsg::Chunk { hash, bytes, .. } => valid_hash(hash) && bytes.len() <= CONTENT_CHUNK,
    }
}

/// Encodes with the tag byte.
pub fn encode_content(m: &ContentMsg) -> Vec<u8> {
    postcard::to_extend(m, alloc::vec![TAG_CONTENT]).expect("serialising into a Vec cannot fail")
}

/// Decodes a message without its tag byte.
pub fn decode_content(bytes: &[u8]) -> Result<ContentMsg, ProtoError> {
    if bytes.len() > MAX_CONTENT {
        return Err(ProtoError::TooLong);
    }
    let m: ContentMsg = decode_exact(bytes)?;
    if valid_content(&m) { Ok(m) } else { Err(ProtoError::Invalid) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn h(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn item(kind: ItemKind, c: char, size: u64, required: bool) -> Item {
        Item { kind, id: alloc::format!("pack-{c}"), hash: h(c), size, required }
    }

    fn round(m: ContentMsg) {
        let b = encode_content(&m);
        assert_eq!(b[0], TAG_CONTENT);
        assert!(b.len() <= MAX_CONTENT + 1);
        assert_eq!(decode_content(&b[1..]), Ok(m));
    }

    fn refused(m: ContentMsg) {
        let b = postcard::to_allocvec(&m).unwrap();
        assert_eq!(decode_content(&b), Err(ProtoError::Invalid), "{m:?}");
    }

    #[test]
    fn valid_messages_round_trip() {
        round(ContentMsg::List(vec![
            item(ItemKind::Mod, 'a', 1 << 20, true),
            item(ItemKind::Level, 'b', 5, true),
            item(ItemKind::Theme, 'c', 0, true),
            item(ItemKind::Skin, 'd', MAX_SKIN_BYTES, false),
        ]));
        round(ContentMsg::List(vec![]));
        round(ContentMsg::Want { hash: h('a'), offset: u64::MAX });
        round(ContentMsg::Begin { hash: h('a'), archive: h('b'), size: MAX_ARCHIVE_BYTES, offset: 7 });
        round(ContentMsg::Chunk { hash: h('a'), offset: 9, bytes: vec![1; CONTENT_CHUNK] });
        round(ContentMsg::Chunk { hash: NO_HASH.to_string(), offset: 0, bytes: vec![] });
        round(ContentMsg::Unavailable { hash: h('c') });
        round(ContentMsg::Cancel { hash: h('d') });
    }

    #[test]
    fn invalid_messages_are_refused() {
        refused(ContentMsg::Want { hash: "abc".into(), offset: 0 });
        refused(ContentMsg::Cancel { hash: h('A') });
        refused(ContentMsg::Unavailable { hash: "g".repeat(64) });
        refused(ContentMsg::Chunk { hash: h('a'), offset: 0, bytes: vec![0; CONTENT_CHUNK + 1] });
        refused(ContentMsg::Begin { hash: h('a'), archive: h('b'), size: 10, offset: 11 });
        refused(ContentMsg::Begin { hash: h('a'), archive: h('b'), size: MAX_ARCHIVE_BYTES + 1, offset: 0 });
        refused(ContentMsg::Begin { hash: h('a'), archive: "x".into(), size: 10, offset: 0 });
        let mut twice = item(ItemKind::Level, 'a', 1, true);
        twice.id = "other".into();
        refused(ContentMsg::List(vec![item(ItemKind::Mod, 'a', 1, true), twice]));
        refused(ContentMsg::List(vec![item(ItemKind::Skin, 'a', 1, true)]));
        refused(ContentMsg::List(vec![item(ItemKind::Skin, 'a', MAX_SKIN_BYTES + 1, false)]));
        refused(ContentMsg::List(vec![item(ItemKind::Mod, 'a', MAX_ARCHIVE_BYTES + 1, true)]));
        let mut bad_id = item(ItemKind::Mod, 'a', 1, true);
        bad_id.id = "Bad Id".into();
        refused(ContentMsg::List(vec![bad_id]));
        let many = (0..=MAX_ITEMS)
            .map(|i| Item {
                kind: ItemKind::Mod,
                id: alloc::format!("p{i}"),
                hash: alloc::format!("{i:064x}"),
                size: 1,
                required: true,
            })
            .collect();
        refused(ContentMsg::List(many));
    }

    #[test]
    fn oversized_and_trailing_are_refused() {
        assert_eq!(decode_content(&vec![0u8; MAX_CONTENT + 1]), Err(ProtoError::TooLong));
        let mut b = encode_content(&ContentMsg::Cancel { hash: h('a') });
        b.push(0);
        assert_eq!(decode_content(&b[1..]), Err(ProtoError::Decode));
    }
}
