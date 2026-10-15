//! An online match in the game (networking spec §3.4, §4; decision N20):
//! the `rl_online::OnlinePeer` that plays it, polled once a frame by
//! `Session::frame`, and its presentation under rollback.
//!
//! Per poll, inside the advance, the scene reads every simulated tick (the
//! world as of that tick: `Scene::after_tick`, or
//! `after_tick_resimulated` for a tick a rollback ran again). After it the
//! poll's events are presented in one fixed order, whatever their order in
//! the poll ([`present_poll`]):
//! 1. a loaded state (`OnlineEvent::StateLoaded`): the scene starts again
//!    from the world and the entities go (`Presentation::state_loaded`),
//!    and the shown ticks after it are kept (`NetPresenter::reset`);
//! 2. the restored areas and the simulated ticks (`NetPresenter::apply`:
//!    the areas rebuilt from the final terrain, then each tick's edits and
//!    effects once per event; debug builds check the colour terrain after
//!    every advance), and the sounds they play;
//! 3. `NetPresenter::confirmed` with the session's confirmed tick;
//! 4. the confirmed ticks, which `Session` turns into the log, the tally,
//!    the scoreboard, the results and the popups (spec §3.4: never a
//!    predicted tick).
//!
//! The newest predicted world is drawn (`OnlineDriver::with_world`).
//!
//! The peer is shared with its network pump (`net::pump`, ruling
//! PF12c-2), which polls it while the UI thread does not: its ticks are not
//! presented, and the next poll's presentation starts again from the world
//! as after a loaded state (`PolledEvents::pumped`).

use std::time::Instant;

use rl_netcode::world_sim::{ConfirmedTick, TickedTick};
use rl_online::OnlineEvent;
use rl_present::Presentation;
use rl_present::net::NetPresenter;
use rl_proto::MatchStartWire;
use rl_sim::{Event, Keys, Score, World};
use rl_terrain::Area;

use crate::net::{PeerHandle, PolledEvents};

/// Who plays an online match's worms: the peer (the local player's keys,
/// the host's bots, everyone else's over the network), and what the
/// presentation has shown of its ticks.
pub struct OnlineDriver {
    /// The peer, shared with its network pump (`net::pump`).
    peer: PeerHandle,
    presenter: NetPresenter,
    /// The scores after the last confirmed tick (the HUD's scoreboard).
    confirmed_scores: Vec<Score>,
    confirmed_tick: Option<u32>,
    /// The slots the host's bots play (the tally's bot kills).
    bots: Vec<i32>,
    /// The sounds one poll plays (reused).
    sounds: Vec<Event>,
    /// Times the presentation started again from the world after the
    /// pump ran ticks.
    resyncs: u64,
    /// The new ticks the last poll ran itself (the F2 meter's).
    polled_ticks: u32,
    /// The last poll loaded a state or put map areas back (a rollback):
    /// its frame draws the newest tick as it is.
    jumped: bool,
}

/// What [`OnlineDriver::present`] gives back: the confirmed ticks, the
/// shell's events, the peer's newest tick, and whether the presentation
/// started again from the world (the pump ran ticks).
pub(crate) struct Shown {
    pub confirmed: Vec<ConfirmedTick>,
    pub rest: Vec<OnlineEvent>,
    pub current_tick: Option<u32>,
    pub resynced: bool,
}

impl OnlineDriver {
    /// The match `start` that `peer` has just started (its
    /// `OnlineEvent::MatchStarting`).
    pub fn new(peer: PeerHandle, start: &MatchStartWire) -> OnlineDriver {
        let bots =
            (0..start.slots.len()).filter(|&i| start.slots[i].member.is_none()).map(|i| i as i32).collect();
        OnlineDriver {
            peer,
            presenter: NetPresenter::new(),
            confirmed_scores: Vec::new(),
            confirmed_tick: None,
            bots,
            sounds: Vec::new(),
            resyncs: 0,
            polled_ticks: 0,
            jumped: false,
        }
    }

    /// What to draw the frame at `now` with between the last two ticks
    /// (spec §3.4): the pace's tick phase (`OnlinePeer::tick_phase`), or 1
    /// (the newest tick as it is) while the match stalls or catches up,
    /// while its pace is held (a new host's resume hold, a jump back: no
    /// tick runs, `tick_phase` is `None`, plan 12c Task 8b), for a
    /// spectator, and on the frame after a loaded state or a rollback.
    pub fn alpha(&self, now: Instant) -> f32 {
        if self.jumped {
            return 1.0;
        }
        self.peer.with(|p| p.tick_phase(now)).unwrap_or(1.0)
    }

    /// The peer's handle (its lock: `PeerHandle::with`).
    pub fn handle(&self) -> &PeerHandle {
        &self.peer
    }

    /// The peer's handle, to change (tests stop its pump).
    pub fn handle_mut(&mut self) -> &mut PeerHandle {
        &mut self.peer
    }

    /// The peer back (the match is over: the lobby goes on), its pump
    /// still running.
    pub fn into_handle(self) -> PeerHandle {
        self.peer
    }

    /// `f` with the newest predicted world, while the peer plays the
    /// match (the peer's lock held meanwhile: `f` must not use the peer).
    pub fn with_world<R>(&self, f: impl FnOnce(Option<&World>) -> R) -> R {
        self.peer.with(|p| f(p.match_session().map(|s| s.sim().world())))
    }

    /// The scores after the last confirmed tick (none before the first).
    pub fn confirmed_scores(&self) -> &[Score] {
        &self.confirmed_scores
    }

    /// The last tick confirmed so far.
    pub fn confirmed_tick(&self) -> Option<u32> {
        self.confirmed_tick
    }

    /// Whether one of the host's bots plays worm `worm`.
    pub fn is_bot(&self, worm: i32) -> bool {
        self.bots.contains(&worm)
    }

    /// Times the presentation started again from the world because the
    /// network pump ran ticks it never saw.
    pub fn resyncs(&self) -> u64 {
        self.resyncs
    }

    /// Advances after which the colour terrain had drifted from the
    /// world's (debug builds: `NetPresenter::drifts`).
    pub fn drifts(&self) -> u64 {
        self.presenter.drifts()
    }

    /// One poll of the peer at `now`: the local player's keys from
    /// `local_keys` (called once per new tick: the input latch), the scene
    /// reading every simulated tick.
    pub(crate) fn poll(
        &mut self,
        now: Instant,
        local_keys: &mut dyn FnMut() -> Keys,
        present: &mut Presentation,
    ) -> PolledEvents {
        let mut ran = 0;
        let polled = self.peer.poll(now, local_keys, &mut |world, events, info| {
            if info.resim {
                present.scene.after_tick_resimulated(world, events);
            } else {
                ran += 1;
                present.scene.after_tick(world, events);
            }
        });
        self.polled_ticks = ran;
        polled
    }

    /// The new ticks the last poll ran itself (not the pump's, nor
    /// re-simulated ones): what the F2 meter counts.
    pub fn polled_ticks(&self) -> u32 {
        self.polled_ticks
    }

    /// A poll's `events` presented ([`present_poll`]), its sounds played
    /// through `play` (with the camera's listener); the confirmed ticks
    /// for the HUD, and the events that are not the presentation's (the
    /// lobby's, the match's end, the connection's) for the shell.
    ///
    /// `pumped` is the poll's (`PolledEvents::pumped`): the pump ran ticks
    /// before it, so the presentation starts again from the world as after
    /// a loaded state, then the poll's own ticks follow. Ticks the pump ran
    /// after the poll (`PeerHandle::with_pumped`, under the same lock) make
    /// the poll's ticks stale: the presentation starts again from the world
    /// without them (`with_pumped` takes the mark, so the next poll does not
    /// start again for them). Once the session drew its frozen world
    /// (`frozen`: the match ended here) nothing starts again (minor 1).
    pub(crate) fn present(
        &mut self,
        mut events: Vec<OnlineEvent>,
        pumped: Option<u32>,
        frozen: bool,
        present: &mut Presentation,
        play: &mut dyn FnMut(&[Event], &Presentation),
    ) -> Shown {
        let OnlineDriver { peer, presenter, sounds, .. } = self;
        sounds.clear();
        let (p, current_tick, resynced) = peer.with_pumped(|peer, since| {
            let session = peer.match_session();
            let world = session.map(|s| s.sim().world());
            let resync = match since.filter(|_| !frozen) {
                Some(t) => {
                    events.retain(|e| {
                        !matches!(
                            e,
                            OnlineEvent::Ticked(_)
                                | OnlineEvent::Restored(_)
                                | OnlineEvent::StateLoaded { .. }
                        )
                    });
                    Some(pumped.map_or(t, |p| p.max(t)))
                }
                None => pumped.filter(|_| !frozen),
            };
            if let (Some(tick), Some(w)) = (resync, world) {
                let t = w.terrain();
                let whole = Area { x0: 0, y0: 0, x1: t.width() as i32, y1: t.height() as i32 };
                events.splice(0..0, [OnlineEvent::StateLoaded { tick }, OnlineEvent::Restored(vec![whole])]);
            }
            let confirmed_below = session.map(|s| s.confirmed());
            let p = present_poll(presenter, present, world, confirmed_below, events, sounds);
            (p, session.map(|s| s.current_tick()), resync.is_some())
        });
        self.jumped = p.jumped;
        self.resyncs += u64::from(resynced);
        if !self.sounds.is_empty() {
            play(&self.sounds, present);
        }
        if let Some(last) = p.confirmed.last() {
            self.confirmed_scores.clone_from(&last.scores);
            self.confirmed_tick = Some(last.tick);
        }
        Shown { confirmed: p.confirmed, rest: p.rest, current_tick, resynced }
    }
}

/// One poll's events, presented.
pub(crate) struct Presented {
    /// The confirmed ticks, in order.
    pub confirmed: Vec<ConfirmedTick>,
    /// The events that are not the presentation's, in order.
    pub rest: Vec<OnlineEvent>,
    /// A state loaded or map areas came back (a rollback).
    pub jumped: bool,
}

/// Presents one poll's `events` on `present` with `presenter`, in the
/// module's order (a loaded state first, then the restored areas and the
/// ticks, then the confirmed tick), whatever their order in `events`;
/// `world` is the world the poll left (`None` once the peer ended the
/// match: its last ticks are not drawn), `confirmed_below` the session's
/// confirmed tick. The sounds to play are appended to `sounds`.
pub(crate) fn present_poll(
    presenter: &mut NetPresenter,
    present: &mut Presentation,
    world: Option<&World>,
    confirmed_below: Option<u32>,
    events: Vec<OnlineEvent>,
    sounds: &mut Vec<Event>,
) -> Presented {
    let mut loaded = Vec::new();
    let mut restored: Vec<Area> = Vec::new();
    let mut ticked: Vec<TickedTick> = Vec::new();
    let mut confirmed = Vec::new();
    let mut rest = Vec::new();
    for e in events {
        match e {
            OnlineEvent::StateLoaded { tick } => loaded.push(tick),
            OnlineEvent::Restored(a) => restored.extend(a),
            OnlineEvent::Ticked(t) => ticked.extend(t),
            OnlineEvent::Confirmed(c) => confirmed.extend(c),
            other => rest.push(other),
        }
    }
    let jumped = !loaded.is_empty() || !restored.is_empty();
    if let Some(world) = world {
        for tick in loaded {
            present.state_loaded(world);
            presenter.reset(tick);
        }
        if !restored.is_empty() || !ticked.is_empty() {
            presenter.apply(present, world, &restored, &ticked, sounds);
        }
    }
    if let Some(tick) = confirmed_below {
        presenter.confirmed(tick);
    }
    Presented { confirmed, rest, jumped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
    use crate::setup;
    use rl_audio::Audio;
    use rl_content::{MatchOptions, Player};
    use rl_scene::{Interpolation, Viewport};
    use rl_sim::{EventKind, TickEvents};

    /// `count` ticks of `world` from tick `from`, everyone firing and
    /// walking, as the netcode's recorder hands them out.
    fn play(world: &mut World, from: u32, count: u32) -> Vec<TickedTick> {
        let keys = vec![Keys(Keys::FIRE | Keys::RIGHT); world.worms().len()];
        let mut events = TickEvents::new();
        (from..from + count)
            .map(|tick| {
                world.tick(&keys, &mut events);
                TickedTick {
                    tick,
                    resim: false,
                    substeps: world.substeps_last_tick(),
                    lost: events.lost_area(),
                    events: events.as_slice().to_vec(),
                }
            })
            .collect()
    }

    /// A loaded state is presented before the poll's ticks wherever it
    /// stands among the poll's events (plan 10 task 10, carried from task
    /// 9): the entities the ticks after the state spawn live on (a load
    /// handled after them would clear them), and the colour terrain is the
    /// world's.
    #[test]
    fn a_loaded_state_is_presented_before_the_polls_ticks() {
        let Some(dir) = std::env::var_os("RL_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir())
        else {
            eprintln!("RL_CONTENT_DIR not set; skipping");
            return;
        };
        let o = MatchOptions::deathmatch("olx-classic", "olx-level-dirt-level", 3, vec![Player::Human; 2]);
        let a = setup::load_match(&[std::path::Path::new(&dir)], &o, "default").unwrap();
        let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
        let mut outcomes = Vec::new();
        for load_last in [false, true] {
            let mut s = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
            let mut presenter = NetPresenter::new();
            let mut sounds = Vec::new();
            let first = play(&mut s.world, 0, 300);
            let p = present_poll(
                &mut presenter,
                &mut s.present,
                Some(&s.world),
                Some(0),
                vec![OnlineEvent::Ticked(first)],
                &mut sounds,
            );
            assert!(p.confirmed.is_empty() && p.rest.is_empty());
            assert!(!p.jumped, "plain ticks: drawn between the last two");
            // The state after tick 299 loads (the world goes on from it), the
            // whole map comes back, and ticks 300.. run in the same poll.
            let t = s.world.terrain();
            let whole = Area { x0: 0, y0: 0, x1: t.width() as i32, y1: t.height() as i32 };
            let after = play(&mut s.world, 300, 100);
            let spawns = after
                .iter()
                .flat_map(|t| &t.events)
                .filter(|e| matches!(e.kind, EventKind::SpawnCosmetic { .. }))
                .count();
            assert!(spawns > 0, "the ticks after the state spawn effects");
            let mut events = vec![
                OnlineEvent::Restored(vec![whole]),
                OnlineEvent::Ticked(after),
                OnlineEvent::Lagging(false),
            ];
            let load = OnlineEvent::StateLoaded { tick: 299 };
            if load_last {
                events.push(load);
            } else {
                events.insert(0, load);
            }
            let spawned = presenter.spawned();
            let p =
                present_poll(&mut presenter, &mut s.present, Some(&s.world), Some(300), events, &mut sounds);
            assert_eq!(p.rest, [OnlineEvent::Lagging(false)], "the shell's events are handed back");
            assert!(p.jumped, "a loaded state: its frame draws the newest tick as it is");
            assert_eq!(presenter.spawned() - spawned, spawns as u64, "every effect after the state spawned");
            assert!(!s.present.fx.is_empty(), "the load came first: its clearing kept the new effects");
            assert!(!s.present.terrain_drift(&s.world));
            assert_eq!(presenter.drifts(), 0);
            outcomes.push((s.present.fx.len(), sounds.len()));
        }
        assert_eq!(outcomes[0], outcomes[1], "the same presentation whatever the events' order");
    }
}
