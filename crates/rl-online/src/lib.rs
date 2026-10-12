//! Online play over real connections (networking spec §4): a host's lobby
//! and its peers, or a client and its host, through `rl-net`'s endpoint;
//! each start's match through `rl-netcode`'s session, its end, the replay,
//! and back to the lobby.

pub mod events;
pub mod matches;
pub mod peer;
pub mod replay;

pub use events::{NetStats, OnlineEvent};
pub use peer::{LobbyHandle, OnlinePeer, PeerRole, StartError};
