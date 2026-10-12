//! What an online peer reports to the game, and its connection's numbers.

use rl_netcode::world_sim::{ConfirmedTick, TickedTick};
use rl_proto::{LobbyState, MatchEndWire, MatchStartWire};
use rl_terrain::Area;

/// What happened during one [`crate::OnlinePeer::poll`], in order.
#[derive(Clone, Debug, PartialEq)]
pub enum OnlineEvent {
    /// The lobby changed (also once when joined).
    Lobby(LobbyState),
    /// A chat line, by the sender's name.
    Chat { from: String, text: String },
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
