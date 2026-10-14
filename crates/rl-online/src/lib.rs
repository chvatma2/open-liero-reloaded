//! Online play over real connections (networking spec §4): a host's lobby
//! and its peers, or a client and its host, through `rl-net`'s endpoint;
//! each start's match through `rl-netcode`'s session, its end, the replay,
//! and back to the lobby; joins through Nostr relays and the host's
//! listener there (`join`, `reach`).

pub mod bundle;
pub mod content;
pub mod events;
pub mod join;
pub mod matches;
pub mod outbox;
pub mod peer;
pub mod reach;
pub mod replay;

pub use bundle::GameBundle;
pub use content::PeerContent;
pub use events::{NetStats, OnlineEvent};
pub use join::{Join, JoinStep, NostrJoin};
pub use peer::{LobbyHandle, OnlinePeer, PeerRole, Shutdown, StartError};
pub use reach::{HostListen, Listening, Reachability};
