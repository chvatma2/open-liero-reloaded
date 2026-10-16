//! What an online peer reports to the game, and its connection's numbers.

use std::net::SocketAddr;

use olr_netcode::world_sim::{ConfirmedTick, TickedTick};
use olr_proto::{LobbyState, MatchEndWire, MatchStartWire, Path};
use olr_terrain::Area;

use crate::content::Download;
use crate::join::JoinStep;
use crate::migration::{LostWhy, MigrationStep};
use crate::reach::{Reachability, RelayJoin};

/// What happened during one [`crate::OnlinePeer::poll`], in order.
#[derive(Clone, Debug, PartialEq)]
pub enum OnlineEvent {
    /// The lobby changed (also once when joined).
    Lobby(LobbyState),
    /// A chat line, by the sender's name and member id (a dedicated
    /// server reads its admins' commands by the id).
    Chat { from: String, member: u8, text: String },
    /// Something to show in the lobby's log: a member joined, was kicked,
    /// a notice from the host, a replay that failed.
    Notice(String),
    /// A match starts: every peer built its world from `start`; `my_slot`
    /// is the worm this peer plays (`None`: it watches).
    MatchStarting { start: MatchStartWire, my_slot: Option<u8> },
    /// The match's state was replaced by the host's after `tick` (a
    /// repair, or a late joiner's first state). It comes before the
    /// [`OnlineEvent::Restored`] that repaints the whole map: the
    /// presentation starts again from the world
    /// (`Presentation::state_loaded`, `NetPresenter::reset(tick)`).
    StateLoaded { tick: u32 },
    /// The map areas a rollback or a loaded state put back during this
    /// poll's advance (`Session::take_restored_areas`). It comes before the
    /// advance's [`OnlineEvent::Ticked`]: rebuild these areas of the colour
    /// map from the terrain first, then apply the ticks' edits in order.
    Restored(Vec<Area>),
    /// Every tick simulated during this poll, in order, re-simulated ones
    /// included, with its events (sounds and effects, spec §3.4).
    Ticked(Vec<TickedTick>),
    /// Newly confirmed ticks, in order (the HUD, the log and the results:
    /// spec §3.4).
    Confirmed(Vec<ConfirmedTick>),
    /// The match ended (the host's results); the lobby follows.
    MatchEnded(MatchEndWire),
    /// This client's inputs miss the host's window (3 s; spec §3.2), or no
    /// longer do.
    Lagging(bool),
    /// A member left the lobby.
    Left(String),
    /// A member timed out (OLX's "<player> has timed out").
    TimedOut(String),
    /// The host kicked this peer (and `banned` it).
    Kicked { banned: bool },
    /// The host refused this peer, or this peer cannot play the host's
    /// match (its simulation or content differs): it left.
    Refused(String),
    /// The connection to the host is gone.
    Disconnected(String),
    /// The finished match as an `.rlrp` replay (decision N18), made after
    /// the match from its confirmed inputs.
    ReplayReady(Vec<u8>),
    /// A desync (spec §3.5, §9.10): this peer's simulation and another's
    /// differed (a checksum the session compared, or one the replay's
    /// re-simulation missed). The bytes are an encoded
    /// [`crate::GameBundle`] for the game to save; at most
    /// [`crate::bundle::MAX_BUNDLES_PER_MATCH`] a match.
    DesyncBundle(Vec<u8>),
    /// The content downloads changed (a client's from its host, a host's
    /// skins from its members): every item, at most every
    /// [`crate::content::REPORT_EVERY_US`] (4 a second).
    Downloads(Vec<Download>),
    /// The content this client lacks is more than the prompt's bytes
    /// ([`crate::content::PROMPT_BYTES`]): nothing downloads until
    /// [`crate::OnlinePeer::approve_downloads`] (or the player leaves).
    DownloadPrompt { bytes: u64 },
    /// A client's join moved to this step (N46): the waiting text.
    Joining(JoinStep),
    /// A host's reachability changed (its lines, whether it settled, its
    /// direct address): the lobby's lines, the server's log, its notes.
    Reachability(Reachability),
    /// A host's join through the relays was answered, refused, did not
    /// connect, or was dated off this PC's clock (ruling FR12b-3);
    /// `join.line(more)` is its lobby and log line.
    RelayJoin { join: RelayJoin, more: u32 },
    /// A host's listener's relay `url` is "connected" or "unreachable:
    /// why", once until it changes (final review B-M4: a host with no other
    /// pool on its relays says which one fails).
    ListenRelay { url: String, status: String },
    /// A client's link to its host opened, by `path` (N49); `remote` is
    /// the host's end, or for `Relayed` the TURN relay's address (the
    /// server its allocation is on, ruling FR12b-4). Once.
    Connected { path: Path, remote: SocketAddr },
    /// The host is gone during a match and the match is moving (spec §3.7):
    /// the HUD's line ([`MigrationStep`]'s `Display`), each time it changes.
    Migrating(MigrationStep),
    /// The match resumed under `host` (a name; `me`: this peer hosts now).
    Migrated { host: String, me: bool },
    /// No successor took over (why: [`LostWhy`], its `Display` the Host
    /// lost screen's text), this host stepped down, or this host heard
    /// none of its members: the match is over for this peer; its replay
    /// follows (`ReplayReady`).
    HostLost(LostWhy),
    /// A line about a migration for the console and the log only (ruling
    /// FR12c-4): how each attempt to reach a successor ended, with its
    /// name, its address and why; a join the successor's migrated lobby
    /// refused. Never shown to other players.
    MigrationLog(String),
}

/// The connection's numbers for the overlay, over the last second of a
/// match (zero outside one).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetStats {
    /// The round trip: a client's to its host, a host's largest.
    pub rtt_ms: Option<u32>,
    /// The match's input delay in ticks (the host's is 0).
    pub input_delay: u8,
    /// The deepest rollback of the last second, in ticks.
    pub rollback_depth: u32,
    /// A client's share of the host's inputs datagrams that did not arrive
    /// over the last second (the host sends one per tick); `None` on a host.
    pub loss: Option<f32>,
    /// A client's own inputs the host replaced, a host's rollbacks, per
    /// second.
    pub corrections_per_s: f32,
    /// Stall episodes since the match started.
    pub stalls: u64,
}
