//! Finding games (networking spec §6): relays, announcements and the game
//! list. The Nostr core is sans-IO (no sockets, no threads, no clock: time
//! is always an argument); the relay pool's drivers own the sockets and
//! threads.

pub mod announce;
pub mod defaults;
pub mod handshake;
pub mod hosting;
mod http_date;
mod invite;
pub mod list;
pub mod nip44;
pub mod nostr;
pub mod ping;
pub mod pool;
mod relay;
pub mod stun;
pub mod turn_note;

pub use announce::{
    AnnounceError, Announcer, GameSummary, Hosting, Region, announcement, parse_announcement,
};
pub use handshake::{Asker, Inbound, Inbox, Reply, Request};
pub use hosting::{Publisher, invite, relay_hints};
pub use invite::{Invite, InviteError, MAX_CODE, MAX_DEFAULT_HINTS, MAX_URL_BYTES, MAX_URL_HINTS, RelayHint};
pub use list::{GameList, ListedGame, Ping};
pub use nostr::{
    Event, Filter, FromRelay, KIND_ANNOUNCE, KIND_HANDSHAKE, MAX_CONTENT, MAX_FRAME, NostrError, ToRelay,
    canonical, decode_from_relay, decode_to_relay, encode_from_relay, encode_to_relay,
};
pub use ping::Pinger;
pub use pool::{Pool, PoolConfig, PoolEvent, RelayStatus, Timings};
pub use stun::{binding_request, parse_binding_response, public_ip};
pub use turn_note::{KIND_TURN, TurnList, TurnNote, TurnPick, parse_turn_note};

#[cfg(feature = "test-relay")]
pub mod test_relay;
#[cfg(feature = "test-relay")]
pub use stun::StunResponder;
#[cfg(feature = "test-relay")]
pub use test_relay::{Misbehaviour, TestRelay};
