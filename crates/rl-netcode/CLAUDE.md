# rl-netcode

The rollback netcode (networking spec §3) and the deterministic network
model that tests it in one process (spec §9.1).

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on rl-sim (keys, the
  world adapter of `world_sim.rs`), rl-proto (same layer: the wire format),
  rl-terrain (`Area`, for the restored areas), rl-modes (the deathmatch's
  time left in `ConfirmedTick`) and rl-math (`Vec2`).
- **Sans-IO**: no sockets, no clock, no threads; time is an argument
  (`now_us`).
- **`Simulation`** is what the session drives (tick, save/restore,
  checksum, state transfer); `ToySim` is a tiny deterministic implementation
  for the session's tests. `decode_state` never panics: it returns
  `StateRejected`.
- **Network model** (`netsim`): `Network` carries datagrams between peers
  with per-direction latency, jitter, Gilbert-Elliott loss, duplication,
  outages; reliable messages arrive in order. Deterministic from its seed.
  Each direction's loss, delay and duplication is a pub `LinkModel`
  (profile + its own `Rng`), which `rl-net`'s impairment stage reuses over
  real sockets (spec §9.4).

  | profile | ping ms | jitter ms | loss | burst | duplicate |
  |---|---|---|---|---|---|
  | lan | 1 | 0 | 0 | 1 | 0 |
  | good | 30 | 5 | 0 | 1 | 0 |
  | typical | 80 | 20 | 0.01 | 1 | 0 |
  | bad | 150 | 50 | 0.05 | 2 | 0.01 |
  | awful | 300 | 100 | 0.15 | 4 | 0.02 |

- **Session** (`session.rs`, `inputs.rs`): one `Session<S>` for host,
  client and spectator; time and messages in, ticks and messages out.
  - `InputLog`: per tick one final value per slot; each slot's knowledge is
    a contiguous prefix (`known_upto`), set once (`Same`/`Conflict` below,
    `Gap` beyond); ticks below the `frontier` are confirmed.
  - The host orders inputs: its local slots (those no peer holds) are final
    when simulated; a client's keys become final in arrival order (the first
    value stays). A client applies its own input `d = input_delay(rtt)`
    ticks later (`0..d` are `Keys(0)`); a spectator simulates confirmed
    ticks only. `confirmed()` (`max(frontier, state floor)`): ticks below
    it are final, the HUD's ticks (spec §3.4).
  - Prediction: final, else a client's own provisional input, else the
    slot's last final value, else `0`. A final value differing from the keys
    a simulated tick used marks a rollback; the next `advance` restores the
    snapshot before it and re-simulates (`Event::Rollback`, `depth_hist`).
    Snapshots are kept from the frontier on, then `forget_before`.
  - Messages: the host sends each peer frames up to its horizon (≤
    `MAX_FRAMES`, halved until ≤ `MAX_DATAGRAM`) with `your_ack`; a client
    sends its own inputs from `your_ack` (≤ `MAX_KEYS`) and its frontier as
    `ack`. Every datagram carries a `Stamp`; `rtt_ms` is a 1/8 moving
    average. Redundancy (plan 9 task 11, spec §3.1's budget): to a peer
    whose measured round trip is at most `COPIES_MAX_RTT_MS` (100 ms), the
    host's frames start at the lowest tick its log changed at before any of
    its last `COPIES` (4) datagrams to that peer (`dirty`; never below the
    peer's `ack`), so each change goes out four times. Losses beyond that
    show in the peer's echo: the echoed datagram was sent when our frontier
    was `f` (`sends`, the last 256), so the peer's `ack` below `f` means
    something was lost, and the next datagram starts at its `ack`
    (`resend`; echoes of datagrams older than that resend are not taken).
    A longer or unmeasured round trip gets every frame from its `ack` in
    every datagram: a resend's extra round trip would not fit under
    `MAX_PREDICTION` (`bad` stalled with 4, 6 and 8 copies). A receiver
    lacking a slot's input before `first` decodes that slot from a dummy
    base: its values there are `Gap`s anyway. `bandwidth32` (32 worms,
    typical) went from 28.7 to 16.7 KB/s received; the feel numbers did
    not change.
  - Pace (`clock.rs`, `Pace`): local time to due ticks (minus ticks run,
    at most `MAX_CATCHUP` per `advance`), adjustable by ±1 % (re-anchored,
    so a change affects only later time) and shiftable by whole ticks (a
    jump). A player peer stalls at `MAX_PREDICTION` ticks past its frontier
    (`Event::Stalled` once per episode, a run of stalled advances; an
    advance that runs ticks without stalling ends it; `Stats::stalls`
    counts episodes). Every peer sends at most one datagram per tick of
    its pace, a stalled one too.
  - Time sync (decision N7): a client datagram whose newest input is newer
    than all before is a margin sample at the host (that tick minus the
    host's current tick); `HostInputs.margin` is the median of the last 16
    (`Margins`). The client steers to `TARGET_MARGIN`: an error beyond
    `JUMP_TICKS` shifts the pace (margins are then stale until the host
    holds 16 of its inputs made after the jump), a smaller one sets the
    speed to `2000 ppm × error` plus an integral term (no lasting error
    from a drifting clock). A client always sends its newest input, so a
    client the host filled past still yields samples.
  - Lagging-player rule (host): before its tick `t`, a player's input
    unknown at `t − fill_window` becomes its previous final value
    (`Stats::fills`); values arriving later for filled ticks are ignored
    and answered once, one `Control::Late { from, to }` per datagram
    spanning the filled ticks it hit (it can include on-time ticks between
    two fill episodes; `Event::Late` at the client; the decoder refuses
    `from >= to`). 300 consecutive host ticks with a fill:
    `Event::PeerLate`. The client's own warning (spec §3.2):
    `Event::Lagging(true)` once its `Late`s span `LAGGING_TICKS` (300)
    ticks within `LAGGING_WINDOW_US` (4 s), `Lagging(false)` after
    `LAGGING_CLEAR_US` (2 s) without one. Silence for
    `SILENCE_TIMEOUT_US`: `Event::PeerTimedOut` (host and client). The
    driver decides; `peer_lost` abandons a peer: its slot is `Keys(0)` at
    every new tick (decision N6), nothing more is sent to it or taken.
    `leave()` says `Control::Bye` (a client to its host, the host to every
    peer) and abandons them; a `Bye` received is `peer_lost` at once
    (no 10 s timeout) and `Event::PeerLeft`.
  - Hardened against a hostile host: a jump moves the pace at most
    `MAX_PREDICTION` ticks back and `SILENCE_TIMEOUT_US / TICK_US` ahead;
    the integral term takes `HostInputs.tick` only when not older than the
    last taken and at most 100 ticks past our own current tick. A client
    with more unacknowledged inputs than `MAX_KEYS` sends the newest ones.
    `peer_lost` also drops what is queued for the peer.
  - `receive` never panics: undecodable input is `Event::Malformed`.
- **Sync** (`sync.rs`, spec §3.5, §3.6): after simulating tick `t` with
  `(t + 1) % checksum_every == 0` (every 10) the session records
  `Simulation::checksum` as pending; a rollback drops pending ticks from
  its start; at the end of `advance` pending ticks below the confirmed
  ticks (and a pending rollback) are confirmed (`Session::checksums`, all
  kept). Clients and spectators send each as `Control::Checksum`; the host
  compares (peers' reports wait up to 1024 ticks for its own): a
  difference is `Event::Desync` (`Stats::desyncs`), `Control::Desync` (the
  client's `Event::Desync`, `local` its own or 0) and a repair: at the end
  of the next advance the host sends `encode_confirmed_state` (the state
  after its last confirmed simulated tick: restore, encode, re-simulate
  unreported) as `StateBegin` and `STATE_CHUNK` chunks
  (`Event::StateSent`). One repair per peer at a time: the peer's
  mismatches are ignored until a report after the state's tick matches
  (repaired) or one from `REPAIR_MARGIN` (300) ticks after the peer's
  `Control::StateLoaded { tick }` arrived on differs (diverged again: a
  new desync); the window starts at the load, not the sending, so a
  multi-MB state over a slow link is not re-sent while it downloads. A
  refusal (`StateRejected` at the receiver, which sends
  `Control::StateRefused { tick }`) re-arms at once (another state, or
  past `MAX_REPAIRS` `RepairFailed`). Neither within `LOAD_TIMEOUT_US`
  (60 s; a silent peer, or a hopeless link): `Event::RepairFailed`. After
  `MAX_REPAIRS` (3) states, or with a state too large to send,
  `Event::RepairFailed` and no more repairs.
  `add_spectator` for a player's peer is refused (`Event::JoinRefused`).
  The receiver refuses a transfer with an index out of range, a repeated
  chunk or more bytes than announced (`StateRejected`, later chunks
  ignored); a whole one is `StateArrived` (the bytes), then
  `decode_state`: refused, `StateRejected` and the old state stays;
  loaded, `StateLoaded` and the timeline restarts after it: no
  snapshots, the **state floor** at `tick + 1` (ticks below it count as
  confirmed for rollback and checksums; their inputs still arrive and are
  recorded, never rolling back), the ticks we had run simulated again at
  the next advance (a client behind the state skips to it); a client or
  spectator then sends `Control::StateLoaded`. `sim_mut` (doc-hidden:
  tests and the harness only) also moves the floor to the current tick
  (a change outside the timeline is never rolled back across).
  `change_confirmed(driver, f)` plants a fault properly: `f` changes the
  state after the last confirmed tick and the unconfirmed ticks run again
  on top, so predicted inputs are not made final (the harness's
  `corrupt`).
- **Spectators mid-match**: `Role::Joining` simulates nothing and ignores
  frames until a state loads, then is a `Spectator` whose input log
  begins after the state (`InputLog::start_at`). The host's
  `add_spectator` owes it the state like a repair; its frames then start
  there with no base (`origin`). A frame datagram from before the
  joiner's origin (sent before the state, arriving after it) cannot be
  decoded (its base is before the log) and is ignored, not `Malformed`.
- **WorldSim** (`world_sim.rs`): the `rl_sim::World` adapter, rollback
  on (`enable_rollback`); `save`/`restore`/`forget_before` are the
  world's (`restore` refused is a session bug: `expect`); `checksum` is
  `sync_checksum` (N4); states are `WorldState::{encode, decode}` +
  `load_state` (N5), every `StateError` a refusal; `events()` are the last
  simulated tick's (stale after a restore or a load until the next tick).
  `take_restored_areas` (on `WorldSim`, and on `Session<WorldSim>`, which
  leaves the timeline alone, unlike `sim_mut`) drains the map areas the
  restores and loads put back. **For plan 10's colour map**: after every
  `advance` (non-empty after `Event::Rollback`, `Event::StateLoaded`, or a
  host's repair), take them, `ColorMap::rebuild_area` each from the
  terrain, then apply the advance's ticks' `TerrainEdit`s (collected in
  `Driver::simulated`, re-simulated ones included) in order; lost: blood
  stains and the level's dirt art inside the areas.
  `tests/world_sim.rs`' `colour_maps_follow_rollbacks_and_loads` keeps
  every peer's colour map that way and checks its material every frame.
  **Tick records** (`rl_netcode::world_sim`, for plan 10's game):
  `TickRecorder::simulated(sim, info)` from `Driver::simulated` keeps a
  `TickedTick` per simulated tick (resim flag, substeps, lost area, all
  events; `take_ticked`) and one `ConfirmedTick` candidate per tick
  (gameplay events only, `World::scores`, the deathmatch's time left via
  `mode_state()` downcast, `World::result`, every worm's position),
  replaced when the tick is re-simulated; `take_confirmed(confirmed)`
  hands out those below `Session::confirmed()`, in order, each tick once
  (a tick re-simulated after it was taken, after a loaded state, is not
  recorded again). `state_loaded(tick)` (the driver calls it on
  `Event::StateLoaded`) drops the old timeline's candidates up to `tick`:
  confirmed ticks then have a gap, so `scores` and `result` (the world's
  own, self-correcting) are the truth, and the match end is read from
  `result`, not only from a `GameOver` event. `tests/world_sim.rs`'
  `tick_records_follow_rollbacks` checks them against an offline replay of
  the final inputs, `a_loaded_state_restarts_the_tick_records` the gap.
- **Harness** (`harness.rs`, `toy.rs`): `run` plays a `Scenario` — the host
  (`HOST` = 0) and its clients and spectators over `Network`, in virtual
  time (1 ms steps, idle steps skipped), clocks skewed by `clock_ppm`,
  outages, freezes (a suspended laptop: no frames, the clock stands still,
  arrivals wait; `Scenario::host_freeze` freezes the host for `until -
  from` ticks' worth of time, its own tick standing still), spectators
  joining at `join_tick` (`Role::Joining`, `add_spectator`), planted
  faults (`corrupt`: at the peer's first frame
  after the host's tick `t` with `t` confirmed,
  `Factory::corrupt(sim, index)` through `change_confirmed`), an
  `Observer` (`run_observed`: `on_event` per event and `on_frame` per
  peer frame, with the session; `run` is `run_observed` with `()`), LAN links
  for the last `settle_ticks` — and returns a `Report` (confirmed logs,
  checksums of ticks `t % 10 == 9` recorded while simulating, the
  sessions' confirmed `checksums`, stats, events, `dropped`). The host
  drops a peer it reports timed out, left (`PeerLeft`) or failed to
  repair (`RepairFailed`, `Report::repair_failed`). The run ends when the
  host is past `ticks + settle_ticks` and every other peer re-simulated
  `ticks` confirmed
  ticks. `verdict` checks every peer not
  dropped confirmed the host's inputs and matches an offline replay of
  them (a joined spectator from its state, a corrupted peer from its last
  loaded state), with no desync but a corrupted peer's and no refused
  state. `ToyFactory` drives `ToySim` with seeded keys; its fault moves
  slot 0.

`tests/play.rs` drives a host and one client over a link the test
controls (`tests/common`: hold or drop either direction, see each
datagram's answer): leaving, coalesced `Late`, lagging, stall episodes,
repair timing.

Test: `cargo nextest run -p rl-netcode` (`tests/world_sim.rs` plays MW 1.0
on Castle Strike from `RL_CONTENT_DIR`, skipped with a notice without it;
rl-content is a dev-dependency only).
