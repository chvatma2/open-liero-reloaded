//! Lobby messages on the reliable channel (networking spec §5.2, §6.2,
//! §7.2): joining, the lobby's state, chat, match start and end, and the
//! tag byte that tells them from the netcode's `Control` messages.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::content::{TAG_CONTENT, valid_hash};
use crate::signal::Refusal;
use crate::{ProtoError, decode_exact};

/// Largest accepted lobby message: a full lobby at every bound fits
/// (`worst_case_messages_fit`, about 40 KB).
pub const MAX_LOBBY: usize = 60 * 1024;
/// Longest name, in characters (rl-ui's profile limit).
pub const MAX_NAME: usize = 20;
/// Longest chat line, in characters.
pub const MAX_CHAT: usize = 200;
/// Most players in a lobby.
pub const MAX_PLAYERS: usize = 32;
/// Most spectators in a lobby.
pub const MAX_SPECTATORS: usize = 16;
/// Most members (players plus spectators).
pub const MAX_MEMBERS: usize = MAX_PLAYERS + MAX_SPECTATORS;
/// Most packs in a match setup.
pub const MAX_PACKS: usize = 8;
/// Most bots in a match setup.
pub const MAX_BOTS: usize = 31;
/// Most missing pack ids a client reports.
pub const MAX_MISSING: usize = 8;
/// Longest id (pack, mod, level), in bytes.
pub const MAX_ID: usize = 64;
/// Largest input delay, in ticks.
pub const MAX_INPUT_DELAY: u8 = 3;
/// Longest restrictions string, in bytes.
pub const MAX_RESTRICTIONS: usize = 256;

/// Reliable-channel tag bytes.
pub const TAG_NETCODE: u8 = 0;
pub const TAG_TO_HOST: u8 = 1;
pub const TAG_TO_CLIENT: u8 = 2;
// `TAG_CONTENT = 3` is in `content.rs`.

/// X-only secp256k1 public key (BIP-340).
pub type PublicKey = [u8; 32];
/// BIP-340 Schnorr signature.
pub type Signature = [u8; 64];

/// serde for `[u8; 64]` (serde itself stops at 32): a 64-byte tuple.
mod sig64 {
    use core::fmt;
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::ser::SerializeTuple;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        let mut t = s.serialize_tuple(64)?;
        for b in v {
            t.serialize_element(b)?;
        }
        t.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = [u8; 64];
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("64 bytes")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<[u8; 64], A::Error> {
                let mut out = [0u8; 64];
                for (i, o) in out.iter_mut().enumerate() {
                    *o = a.next_element()?.ok_or_else(|| A::Error::invalid_length(i, &self))?;
                }
                Ok(out)
            }
        }
        d.deserialize_tuple(64, V)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub name: String,
    pub colour: [u8; 3],
    /// Skin pack id.
    pub skin: String,
    /// Content hash of the skin pack (64 lowercase hex); `NO_HASH` when unknown.
    pub skin_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToHost {
    Hello {
        key: PublicKey,
        look: Look,
        /// SHA-256 of the host's nonce and the password.
        password: Option<[u8; 32]>,
        /// The joiner's fresh nonce: the host signs it in `HostProof`.
        nonce: [u8; 32],
    },
    /// Signature over the host's challenge, sent once the host's
    /// `HostProof` verified.
    Prove {
        #[serde(with = "sig64")]
        signature: Signature,
    },
    SetLook(Look),
    Ready(bool),
    Spectate(bool),
    Loadout(Option<[i32; 5]>),
    Chat(String),
    /// Pack ids this client lacks of the host's content list `revision`
    /// (`ContentMsg::List`); 0 before any.
    Have {
        revision: u32,
        missing: Vec<String>,
    },
    Admin(AdminCommand),
    Bye,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AdminCommand {
    Kick(PublicKey),
    Ban(PublicKey),
    Next,
    Settings(MatchSetupWire),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ToClient {
    Challenge {
        nonce: [u8; 32],
        host_key: PublicKey,
        needs_password: bool,
    },
    /// `you` is the receiver's member id.
    Welcome {
        you: u8,
        lobby: LobbyState,
    },
    Refused(Refusal),
    /// Full state on every change.
    Lobby(LobbyState),
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
    Bye,
    /// The host's signature over the joiner's `Hello` nonce and key and
    /// both DTLS fingerprints (rl-lobby's `host_proof_message`): it holds
    /// the key its `Challenge` named. Sent right after `Hello`.
    HostProof {
        #[serde(with = "sig64")]
        signature: Signature,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LobbyState {
    pub name: String,
    pub phase: Phase,
    pub setup: MatchSetupWire,
    pub members: Vec<Member>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Gathering,
    Starting,
    Playing { tick: u32 },
    Results,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: u8,
    pub key: PublicKey,
    pub look: Look,
    pub seat: SeatWire,
    pub ready: bool,
    pub missing: Vec<String>,
    pub ping_ms: Option<u16>,
    pub admin: bool,
    /// Percent of the required content received, while downloading.
    pub download: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeatWire {
    Player { slot: u8 },
    Spectator,
    Waiting,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchSetupWire {
    pub mod_id: String,
    pub level_id: String,
    /// Pack id and content hash (64 hex characters).
    pub packs: Vec<(String, String)>,
    pub rules: RulesWire,
    pub bonuses: bool,
    pub loading_time: i32,
    pub restrictions: String,
    pub bots: Vec<BotWire>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RulesWire {
    pub lives: i32,
    pub kill_limit: i32,
    pub time_limit_min: f32,
    pub allow_negative_score: bool,
    pub death_decreases_score: f32,
    pub suicide_decreases_score: bool,
    pub sprees: [i32; 5],
    pub dying_sprees: [i32; 5],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotWire {
    pub difficulty: u8,
    pub personality: u8,
    pub loadout: u8,
    pub look: Look,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchStartWire {
    pub seed: u32,
    pub setup: MatchSetupWire,
    pub slots: Vec<SlotWire>,
    /// Member id and its input delay in ticks.
    pub input_delay: Vec<(u8, u8)>,
    pub start_checksum: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotWire {
    /// `None`: a bot.
    pub member: Option<u8>,
    /// Index into `setup.bots`.
    pub bot: Option<u8>,
    pub look: Look,
    pub loadout: Option<[i32; 5]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchEndWire {
    pub reason: String,
    /// Slot, score, kills, deaths, lives.
    pub rows: Vec<(u8, i32, i32, i32, i32)>,
    /// The match's last recorded tick: every peer's record (and replay) is
    /// ticks `0..=last_tick` (ruling T14-A). Below `u32::MAX`, so the
    /// record's length `last_tick + 1` fits.
    pub last_tick: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Notice {
    Joined(u8),
    Left(u8),
    Kicked(u8),
    Banned(u8),
    NotReady(Vec<u8>),
    Started,
    Message(String),
}

/// A reliable-channel payload split by its tag byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tagged<'a> {
    Netcode(&'a [u8]),
    ToHost(&'a [u8]),
    ToClient(&'a [u8]),
    Content(&'a [u8]),
}

/// Splits the tag byte off; `None` for an empty payload or an unknown tag.
pub fn tag(bytes: &[u8]) -> Option<Tagged<'_>> {
    let (t, rest) = bytes.split_first()?;
    match *t {
        TAG_NETCODE => Some(Tagged::Netcode(rest)),
        TAG_TO_HOST => Some(Tagged::ToHost(rest)),
        TAG_TO_CLIENT => Some(Tagged::ToClient(rest)),
        TAG_CONTENT => Some(Tagged::Content(rest)),
        _ => None,
    }
}

// ---- validation ----

/// At most `max` characters, no control characters.
pub(crate) fn valid_text(s: &str, max: usize) -> bool {
    s.chars().count() <= max && !s.chars().any(char::is_control)
}

/// Bidi overrides, isolates and marks: refused in chat and names.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Unicode format (Cf) and separator code points that make names
/// ambiguous: bidi controls, zero-width characters, line and paragraph
/// separators, the BOM.
fn is_hidden(c: char) -> bool {
    is_bidi(c)
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

/// A name: 1 to `max` characters, no control, hidden or bidi characters,
/// not blank, no leading or trailing whitespace.
pub fn valid_name(s: &str, max: usize) -> bool {
    valid_text(s, max) && !s.is_empty() && s.trim() == s && !s.chars().any(is_hidden)
}

/// A pack id: ASCII `[a-z0-9-]` (rl-pack's manifest rule), 1 to `MAX_ID` bytes.
pub fn valid_pack_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A look: a valid name, a pack-id skin and its hash.
pub fn valid_look(l: &Look) -> bool {
    valid_name(&l.name, MAX_NAME) && valid_pack_id(&l.skin) && valid_hash(&l.skin_hash)
}

fn valid_missing(m: &[String]) -> bool {
    m.len() <= MAX_MISSING && m.iter().all(|s| valid_pack_id(s))
}

/// A match setup within the wire's bounds.
pub fn valid_setup(s: &MatchSetupWire) -> bool {
    valid_pack_id(&s.mod_id)
        && valid_pack_id(&s.level_id)
        && s.packs.len() <= MAX_PACKS
        && s.packs.iter().all(|(id, h)| valid_pack_id(id) && valid_hash(h))
        && s.rules.time_limit_min.is_finite()
        && s.rules.death_decreases_score.is_finite()
        && s.restrictions.len() <= MAX_RESTRICTIONS
        && !s.restrictions.chars().any(char::is_control)
        && s.bots.len() <= MAX_BOTS
        && s.bots.iter().all(|b| valid_look(&b.look))
}

fn valid_lobby(l: &LobbyState) -> bool {
    valid_name(&l.name, MAX_NAME)
        && valid_setup(&l.setup)
        && l.members.len() <= MAX_MEMBERS
        && l.members.iter().all(|m| valid_look(&m.look) && valid_missing(&m.missing))
        && unique(l.members.iter().map(|m| m.id))
        && l.members.iter().filter(|m| matches!(m.seat, SeatWire::Player { .. })).count() <= MAX_PLAYERS
        && unique(l.members.iter().filter_map(|m| match m.seat {
            SeatWire::Player { slot } => Some(slot),
            _ => None,
        }))
}

fn valid_start(s: &MatchStartWire) -> bool {
    valid_setup(&s.setup)
        && s.slots.len() <= MAX_PLAYERS
        && s.slots.iter().all(|x| valid_look(&x.look))
        && s.slots.iter().all(|x| x.bot.is_none_or(|b| (b as usize) < s.setup.bots.len()))
        && s.input_delay.len() <= MAX_MEMBERS
        && s.input_delay.iter().all(|&(_, d)| d <= MAX_INPUT_DELAY)
}

fn unique(it: impl Iterator<Item = u8>) -> bool {
    let mut seen = [false; 256];
    for v in it {
        if core::mem::replace(&mut seen[v as usize], true) {
            return false;
        }
    }
    true
}

/// A chat line, notice or end reason: at most `MAX_CHAT` characters, no
/// control characters or bidi controls.
pub fn valid_chat(s: &str) -> bool {
    valid_text(s, MAX_CHAT) && !s.chars().any(is_bidi)
}

pub fn encode_to_host(m: &ToHost) -> Vec<u8> {
    postcard::to_allocvec(m).expect("serialising into a Vec cannot fail")
}

pub fn decode_to_host(b: &[u8]) -> Result<ToHost, ProtoError> {
    if b.len() > MAX_LOBBY {
        return Err(ProtoError::TooLong);
    }
    let m: ToHost = decode_exact(b)?;
    let ok = match &m {
        ToHost::Hello { look, .. } | ToHost::SetLook(look) => valid_look(look),
        ToHost::Chat(t) => valid_chat(t),
        ToHost::Have { missing, .. } => valid_missing(missing),
        ToHost::Admin(AdminCommand::Settings(s)) => valid_setup(s),
        _ => true,
    };
    if ok { Ok(m) } else { Err(ProtoError::Invalid) }
}

pub fn encode_to_client(m: &ToClient) -> Vec<u8> {
    postcard::to_allocvec(m).expect("serialising into a Vec cannot fail")
}

pub fn decode_to_client(b: &[u8]) -> Result<ToClient, ProtoError> {
    if b.len() > MAX_LOBBY {
        return Err(ProtoError::TooLong);
    }
    let m: ToClient = decode_exact(b)?;
    let ok = match &m {
        ToClient::Welcome { lobby, .. } | ToClient::Lobby(lobby) => valid_lobby(lobby),
        ToClient::Chat { text, .. } => valid_chat(text),
        ToClient::Notice(Notice::Message(t)) => valid_chat(t),
        ToClient::Notice(Notice::NotReady(ids)) => ids.len() <= MAX_MEMBERS,
        ToClient::Start(s) => valid_start(s),
        ToClient::Ended(e) => valid_chat(&e.reason) && e.rows.len() <= MAX_PLAYERS && e.last_tick < u32::MAX,
        _ => true,
    };
    if ok { Ok(m) } else { Err(ProtoError::Invalid) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::*;
    use alloc::string::ToString;
    use alloc::vec;

    pub(crate) fn look() -> Look {
        Look {
            name: "Martin".to_string(),
            colour: [1, 2, 3],
            skin: "olx-skin-afro".to_string(),
            skin_hash: "ab".repeat(32),
        }
    }

    pub(crate) fn setup() -> MatchSetupWire {
        MatchSetupWire {
            mod_id: "olx-classic".to_string(),
            level_id: "olx-level-dirt-level".to_string(),
            packs: vec![("olx-classic".to_string(), "ab".repeat(32))],
            rules: RulesWire {
                lives: 10,
                kill_limit: -1,
                time_limit_min: 5.5,
                allow_negative_score: true,
                death_decreases_score: 0.5,
                suicide_decreases_score: false,
                sprees: [1, 2, 3, 4, 5],
                dying_sprees: [5, 4, 3, 2, 1],
            },
            bonuses: true,
            loading_time: 100,
            restrictions: String::new(),
            bots: vec![BotWire { difficulty: 2, personality: 1, loadout: 0, look: look() }],
        }
    }

    fn member(id: u8) -> Member {
        Member {
            id,
            key: [id; 32],
            look: look(),
            seat: if id < 32 { SeatWire::Player { slot: id } } else { SeatWire::Spectator },
            ready: true,
            missing: vec!["olx-skin-x".to_string()],
            ping_ms: Some(12),
            admin: id == 0,
            download: None,
        }
    }

    fn lobby(n: usize) -> LobbyState {
        LobbyState {
            name: "Office".to_string(),
            phase: Phase::Playing { tick: 77 },
            setup: setup(),
            members: (0..n).map(|i| member(i as u8)).collect(),
        }
    }

    fn start(slots: usize) -> MatchStartWire {
        MatchStartWire {
            seed: 9,
            setup: setup(),
            slots: (0..slots)
                .map(|i| SlotWire {
                    member: Some(i as u8),
                    bot: None,
                    look: look(),
                    loadout: Some([1, 2, 3, 4, 5]),
                })
                .collect(),
            input_delay: vec![(0, 2), (1, 3)],
            start_checksum: 0xfeed,
        }
    }

    fn to_host_round(m: ToHost) {
        assert_eq!(decode_to_host(&encode_to_host(&m)), Ok(m));
    }

    fn to_client_round(m: ToClient) {
        assert_eq!(decode_to_client(&encode_to_client(&m)), Ok(m));
    }

    #[test]
    fn to_host_round_trips() {
        to_host_round(ToHost::Hello { key: [7; 32], look: look(), password: Some([9; 32]), nonce: [5; 32] });
        to_host_round(ToHost::Prove { signature: core::array::from_fn(|i| i as u8) });
        to_host_round(ToHost::SetLook(look()));
        to_host_round(ToHost::Ready(true));
        to_host_round(ToHost::Spectate(false));
        to_host_round(ToHost::Loadout(Some([1, -2, 3, 4, 5])));
        to_host_round(ToHost::Chat("hello".to_string()));
        to_host_round(ToHost::Have { revision: 3, missing: vec!["olx-skin-a".to_string()] });
        to_host_round(ToHost::Admin(AdminCommand::Kick([3; 32])));
        to_host_round(ToHost::Admin(AdminCommand::Settings(setup())));
        to_host_round(ToHost::Bye);
    }

    #[test]
    fn to_client_round_trips() {
        to_client_round(ToClient::Challenge { nonce: [1; 32], host_key: [2; 32], needs_password: true });
        to_client_round(ToClient::Welcome { you: 3, lobby: lobby(4) });
        to_client_round(ToClient::Refused(Refusal::Full));
        to_client_round(ToClient::Lobby(lobby(MAX_MEMBERS)));
        to_client_round(ToClient::Chat { from: 1, text: "x".repeat(MAX_CHAT) });
        to_client_round(ToClient::Notice(Notice::NotReady(vec![1, 2])));
        to_client_round(ToClient::Notice(Notice::Message("hi".to_string())));
        to_client_round(ToClient::Start(start(MAX_PLAYERS)));
        to_client_round(ToClient::Ended(MatchEndWire {
            reason: "kill limit".to_string(),
            rows: vec![(0, 5, 3, 2, 1)],
            last_tick: 899,
        }));
        to_client_round(ToClient::Kicked { banned: true });
        to_client_round(ToClient::Bye);
        to_client_round(ToClient::HostProof { signature: core::array::from_fn(|i| 255 - i as u8) });
    }

    #[test]
    fn name_of_21_characters_is_refused_and_20_multibyte_accepted() {
        let mut l = look();
        l.name = "n".repeat(MAX_NAME + 1);
        assert_eq!(decode_to_host(&encode_to_host(&ToHost::SetLook(l))), Err(ProtoError::Invalid));
        // 20 characters of 4 bytes each are within the limit (characters, not bytes).
        to_host_round(ToHost::SetLook(Look { name: "\u{1F600}".repeat(MAX_NAME), ..look() }));
    }

    #[test]
    fn name_with_control_character_is_refused() {
        let mut l = look();
        l.name = "a\nb".to_string();
        let m = ToHost::Hello { key: [0; 32], look: l, password: None, nonce: [0; 32] };
        assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
    }

    #[test]
    fn chat_of_201_characters_is_refused() {
        let m = ToHost::Chat("c".repeat(MAX_CHAT + 1));
        assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
        let m = ToClient::Chat { from: 0, text: "c".repeat(MAX_CHAT + 1) };
        assert_eq!(decode_to_client(&encode_to_client(&m)), Err(ProtoError::Invalid));
    }

    #[test]
    fn forty_nine_members_are_refused() {
        let m = ToClient::Lobby(lobby(MAX_MEMBERS + 1));
        assert_eq!(decode_to_client(&encode_to_client(&m)), Err(ProtoError::Invalid));
    }

    #[test]
    fn thirty_three_slots_are_refused() {
        let m = ToClient::Start(start(MAX_PLAYERS + 1));
        assert_eq!(decode_to_client(&encode_to_client(&m)), Err(ProtoError::Invalid));
    }

    #[test]
    fn pack_id_with_slash_is_refused() {
        let m = ToHost::Have { revision: 0, missing: vec!["a/b".to_string()] };
        assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
        let mut s = setup();
        s.packs[0].0 = "../x".to_string();
        let m = ToHost::Admin(AdminCommand::Settings(s));
        assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
        let mut l = look();
        l.skin = "Up/per".to_string();
        assert_eq!(decode_to_host(&encode_to_host(&ToHost::SetLook(l))), Err(ProtoError::Invalid));
    }

    #[test]
    fn missing_is_bounded() {
        let ids = |n: usize| (0..n).map(|i| alloc::format!("p{i}")).collect::<Vec<_>>();
        to_host_round(ToHost::Have { revision: u32::MAX, missing: ids(MAX_MISSING) });
        let m = ToHost::Have { revision: 0, missing: ids(MAX_MISSING + 1) };
        assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
        let m = ToHost::Have { revision: 0, missing: vec!["a".repeat(MAX_ID + 1)] };
        assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
    }

    #[test]
    fn setup_bounds() {
        let mut s = setup();
        s.packs = (0..=MAX_PACKS).map(|i| (alloc::format!("p{i}"), "0".repeat(64))).collect();
        assert_eq!(
            decode_to_host(&encode_to_host(&ToHost::Admin(AdminCommand::Settings(s)))),
            Err(ProtoError::Invalid)
        );
        let mut s = setup();
        s.packs[0].1 = "ab".repeat(31);
        assert_eq!(
            decode_to_host(&encode_to_host(&ToHost::Admin(AdminCommand::Settings(s)))),
            Err(ProtoError::Invalid)
        );
        let mut s = setup();
        s.bots = vec![setup().bots[0].clone(); MAX_BOTS + 1];
        assert_eq!(
            decode_to_host(&encode_to_host(&ToHost::Admin(AdminCommand::Settings(s)))),
            Err(ProtoError::Invalid)
        );
        let mut s = setup();
        s.rules.time_limit_min = f32::NAN;
        assert_eq!(
            decode_to_host(&encode_to_host(&ToHost::Admin(AdminCommand::Settings(s)))),
            Err(ProtoError::Invalid)
        );
    }

    #[test]
    fn mod_and_level_ids_are_pack_ids() {
        for bad in ["../x", "Up", "a/b", ""] {
            let mut s = setup();
            s.mod_id = bad.to_string();
            let m = ToHost::Admin(AdminCommand::Settings(s));
            assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid), "{bad:?}");
            let mut s = setup();
            s.level_id = bad.to_string();
            let mut l = lobby(1);
            l.setup = s;
            let m = ToClient::Lobby(l);
            assert_eq!(decode_to_client(&encode_to_client(&m)), Err(ProtoError::Invalid), "{bad:?}");
        }
    }

    #[test]
    fn names_are_identity_safe() {
        let bad = [
            "",
            "   ",
            " a",
            "a ",
            "a\u{202E}b",
            "a\u{2066}b",
            "a\u{200E}b",
            "a\u{200F}b",
            "a\u{061C}b",
            "a\u{200B}b",
            "a\u{200C}b",
            "a\u{200D}b",
            "a\u{2060}b",
            "a\u{FEFF}b",
            "a\u{2028}b",
            "a\u{2029}b",
        ];
        for n in bad {
            let l = Look { name: n.to_string(), ..look() };
            assert_eq!(
                decode_to_host(&encode_to_host(&ToHost::SetLook(l))),
                Err(ProtoError::Invalid),
                "{n:?}"
            );
        }
        to_host_round(ToHost::SetLook(Look { name: "a b".to_string(), ..look() }));
    }

    #[test]
    fn chat_refuses_bidi_controls() {
        for c in ['\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}'] {
            let m = ToHost::Chat(alloc::format!("a{c}b"));
            assert_eq!(decode_to_host(&encode_to_host(&m)), Err(ProtoError::Invalid));
        }
    }

    #[test]
    fn start_and_lobby_invariants() {
        let refused = |m: ToClient| {
            assert_eq!(decode_to_client(&encode_to_client(&m)), Err(ProtoError::Invalid));
        };
        let mut s = start(2);
        s.slots[0].bot = Some(1); // setup has one bot
        refused(ToClient::Start(s));
        let mut s = start(2);
        s.slots[0].bot = Some(0);
        to_client_round(ToClient::Start(s));
        let mut s = start(2);
        s.input_delay = vec![(0, MAX_INPUT_DELAY + 1)];
        refused(ToClient::Start(s));
        let mut l = lobby(3);
        l.members[1].id = 0;
        refused(ToClient::Lobby(l));
        let mut l = lobby(3);
        l.members[1].seat = SeatWire::Player { slot: 0 };
        refused(ToClient::Lobby(l));
        let mut l = lobby(1);
        l.name = " ".to_string();
        refused(ToClient::Lobby(l));
    }

    /// A look at every bound: 20 four-byte characters, a 64-byte skin id.
    fn worst_look(i: usize) -> Look {
        Look {
            name: "\u{1F600}".repeat(MAX_NAME),
            colour: [255; 3],
            skin: alloc::format!("{i:02}{}", "s".repeat(MAX_ID - 2)),
            skin_hash: "f".repeat(64),
        }
    }

    fn worst_setup() -> MatchSetupWire {
        let mut s = setup();
        s.mod_id = "m".repeat(MAX_ID);
        s.level_id = "l".repeat(MAX_ID);
        s.packs = (0..MAX_PACKS)
            .map(|i| (alloc::format!("{i}{}", "p".repeat(MAX_ID - 1)), "a".repeat(64)))
            .collect();
        s.restrictions = "r".repeat(MAX_RESTRICTIONS);
        s.rules.lives = i32::MIN;
        s.rules.kill_limit = i32::MIN;
        s.rules.sprees = [i32::MIN; 5];
        s.rules.dying_sprees = [i32::MIN; 5];
        s.loading_time = i32::MIN;
        s.bots = (0..MAX_BOTS)
            .map(|i| BotWire { difficulty: 255, personality: 255, loadout: 255, look: worst_look(i) })
            .collect();
        s
    }

    #[test]
    fn worst_case_messages_fit() {
        let lobby = LobbyState {
            name: "\u{1F600}".repeat(MAX_NAME),
            phase: Phase::Playing { tick: u32::MAX },
            setup: worst_setup(),
            members: (0..MAX_MEMBERS)
                .map(|i| Member {
                    id: 200 + i as u8,
                    key: [0xff; 32],
                    look: worst_look(i),
                    seat: if i < MAX_PLAYERS {
                        SeatWire::Player { slot: 200 + i as u8 }
                    } else {
                        SeatWire::Spectator
                    },
                    ready: true,
                    missing: (0..MAX_MISSING)
                        .map(|j| alloc::format!("{j}{}", "x".repeat(MAX_ID - 1)))
                        .collect(),
                    ping_ms: Some(u16::MAX),
                    admin: true,
                    download: Some(100),
                })
                .collect(),
        };
        let welcome = ToClient::Welcome { you: 255, lobby: lobby.clone() };
        let bytes = encode_to_client(&welcome);
        assert!(bytes.len() <= MAX_LOBBY, "{} bytes", bytes.len());
        assert!(bytes.len() > 32 * 1024, "the old limit was too small: {} bytes", bytes.len());
        to_client_round(welcome);
        to_client_round(ToClient::Lobby(lobby));
        let start = MatchStartWire {
            seed: u32::MAX,
            setup: worst_setup(),
            slots: (0..MAX_PLAYERS)
                .map(|i| SlotWire {
                    member: Some(255),
                    bot: Some(0),
                    look: worst_look(i),
                    loadout: Some([i32::MIN; 5]),
                })
                .collect(),
            input_delay: (0..MAX_MEMBERS).map(|i| (200 + i as u8, MAX_INPUT_DELAY)).collect(),
            start_checksum: u64::MAX,
        };
        assert!(encode_to_client(&ToClient::Start(start.clone())).len() <= MAX_LOBBY);
        to_client_round(ToClient::Start(start));
        to_host_round(ToHost::Admin(AdminCommand::Settings(worst_setup())));
        let hello = ToHost::Hello {
            key: [0xff; 32],
            look: worst_look(0),
            password: Some([0xff; 32]),
            nonce: [0xff; 32],
        };
        assert!(encode_to_host(&hello).len() <= MAX_LOBBY);
        to_host_round(hello);
        let proof = ToClient::HostProof { signature: [0xff; 64] };
        assert!(encode_to_client(&proof).len() <= MAX_LOBBY);
        to_client_round(proof);
        let have = ToHost::Have {
            revision: u32::MAX,
            missing: (0..MAX_MISSING).map(|j| alloc::format!("{j}{}", "x".repeat(MAX_ID - 1))).collect(),
        };
        assert!(encode_to_host(&have).len() <= MAX_LOBBY);
        to_host_round(have);
        // Content: a full list (longest ids, largest sizes) and a full chunk.
        let list = ContentMsg::List {
            revision: u32::MAX,
            items: (0..MAX_ITEMS)
                .map(|i| Item {
                    kind: ItemKind::Theme,
                    id: alloc::format!("{i:02}{}", "i".repeat(MAX_ID - 2)),
                    hash: alloc::format!("{i:064x}"),
                    size: MAX_ARCHIVE_BYTES,
                    required: true,
                })
                .collect(),
        };
        let chunk =
            ContentMsg::Chunk { hash: "f".repeat(64), offset: u64::MAX, bytes: vec![0xff; CONTENT_CHUNK] };
        for m in [list, chunk] {
            let bytes = encode_content(&m);
            assert!(bytes.len() <= MAX_CONTENT + 1, "{} bytes", bytes.len());
            assert_eq!(decode_content(&bytes[1..]), Ok(m));
        }
    }

    #[test]
    fn oversized_and_trailing_are_refused() {
        assert_eq!(decode_to_host(&vec![0u8; MAX_LOBBY + 1]), Err(ProtoError::TooLong));
        assert_eq!(decode_to_client(&vec![0u8; MAX_LOBBY + 1]), Err(ProtoError::TooLong));
        let mut b = encode_to_host(&ToHost::Bye);
        b.push(0);
        assert_eq!(decode_to_host(&b), Err(ProtoError::Decode));
        let mut b = encode_to_client(&ToClient::Bye);
        b.push(0);
        assert_eq!(decode_to_client(&b), Err(ProtoError::Decode));
    }

    #[test]
    fn tag_demultiplexes() {
        assert_eq!(tag(&[]), None);
        assert_eq!(tag(&[0, 5, 6]), Some(Tagged::Netcode(&[5, 6])));
        assert_eq!(tag(&[1]), Some(Tagged::ToHost(&[])));
        assert_eq!(tag(&[2, 9]), Some(Tagged::ToClient(&[9])));
        assert_eq!(tag(&[3, 9]), Some(Tagged::Content(&[9])));
        assert_eq!(tag(&[4, 9]), None);
    }
}
