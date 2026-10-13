//! Finding games (networking spec §6): relays, announcements and the game
//! list. This crate so far holds the Nostr core only, sans-IO: no sockets,
//! no threads, no clock (time is always an argument).

pub mod nostr;

pub use nostr::{
    Event, Filter, FromRelay, KIND_ANNOUNCE, KIND_HANDSHAKE, MAX_CONTENT, MAX_FRAME, NostrError, ToRelay,
    canonical, decode_from_relay, decode_to_relay, encode_from_relay, encode_to_relay,
};

#[cfg(feature = "test-relay")]
pub mod test_relay;
#[cfg(feature = "test-relay")]
pub use test_relay::{Misbehaviour, TestRelay};
