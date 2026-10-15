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
pub mod migration;
pub mod outbox;
pub mod peer;
pub mod reach;
pub mod replay;

pub use bundle::GameBundle;
pub use content::PeerContent;
pub use events::{NetStats, OnlineEvent};
pub use join::{Join, JoinStep, NostrJoin};
pub use migration::{
    AttemptOutcome, Attempted, CUT_OFF, HOST_LOST, LostWhy, MIGRATION_CONNECT_TIMEOUT, MigrationConfig,
    MigrationStep, stepped_down,
};
pub use peer::{LobbyHandle, OnlinePeer, PeerRole, RECORD_AFTER_END, Shutdown, StartError};
pub use reach::{HostListen, Listening, Reachability, RelayJoin};

// The game polls a peer from its UI thread and from its network pump
// (rl-game's `net::pump`, ruling PF12c-2): no `Rc` or other `!Send` part
// may enter the peer's stack.
const _: () = {
    fn send<T: Send>() {}
    let _ = send::<OnlinePeer>;
};
