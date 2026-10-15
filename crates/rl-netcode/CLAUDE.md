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
    it are final, the HUD's ticks (spec §3.4). `peer_rtt_ms(peer)`: the
    round trip to one peer (a host's `Stats::rtt_ms` is the largest), for
    the lobby's pings and the next match's input delays.
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
    its pace, a stalled one too. `Pace::phase(now)` is the timeline's
    place in its tick, `[0, 1)`; `Session::tick_phase(now_us)` gives it
    for drawing between the last two ticks (plan 10 task 12), `None` when
    the last advance left due ticks unrun (stalled or catching up) and for
    spectators (their ticks follow the host's frames).
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
  compares (peers' reports wait up to 1024 ticks for its own; each peer's
  compared reports are kept, the last `PEER_STREAM_KEEP` (4096):
  `Session::peer_checksums`, a desync bundle's other stream): a
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
- **Migration (session)** (`session/migrate.rs`, constants and
  `fit_report` in `migration.rs`; spec §3.7, N67, N69, N76). Who takes
  over and who rejoins whom is the drivers' part (harness, rl-online).
  - Silence: a client or spectator pushes `Event::HostSilent` once when its
    host sent nothing for `HOST_SILENT_US` (2 s), counted only over time it
    advanced (a gap over 1 s between two of its own `advance` calls restarts
    the clock: its own sleep is not its host's silence); a datagram from the
    host after it is `Event::HostHeard` (once). A reliable message is no
    return: a silent host's `Bye` is `PeerLeft` alone (a driver acting on
    `HostHeard` never unsuspends toward a leaving host).
  - `suspend()`: `advance` runs no new tick, queues no frames and never
    stalls; it still re-simulates an owed rollback or loaded state,
    confirms and sends checksums, and sends a frame-less keepalive
    (`ClientInputs` without keys, `HostInputs` without frames) every
    `KEEPALIVE_US` (250 ms), so a collecting new host never looks silent.
    Margins are not taken; `tick_phase` is `None`. `unsuspend(now)` (the old
    host is back, N69) restarts the pace at the current tick; nothing after
    `rehost` or `take_over`.
  - `rehost(host, now)`: a client's or spectator's host is now `host`, its
    peer state (acks, round trip, silence, time sync) fresh; suspended until
    that host's `Control::Resume`. `Collect`/`Resume` from anyone else are
    ignored, and so is a `Resume` more than `MAX_AHEAD` (6000) ticks past
    our tick. A `Collect` (whose `known` must have one entry per slot, else
    `Malformed`) is answered with a `Report`: final inputs from
    `max(min(known), origin)` to our horizon, our own provisional inputs
    from our slot's prefix, our frontier as `ack`; `fit_report` halves the
    span from its end until it fits `MAX_REPORT_FRAMES` and `MAX_CONTROL`,
    own cut to `MAX_REPORT_OWN`.
  - `take_over(bots, now)`: a client or spectator becomes the host; its
    local slots are its own slot (if a player; it keeps the match's input
    delay, N2: its keys go `d` ticks ahead to `own` and are final when
    simulated, so the driver is called twice a tick, own slot at `t + d`,
    bots at `t`, as `Driver::local_inputs` says) and the bots; every other slot is awaited. Nothing is
    filled until the resume. `rejoined(peer, seat, now)` (false after the
    resume, for a seat out of range or held, or a peer already back) sends
    `Collect` with our `known_upto`s; `awaiting_reports()` lists rejoined
    peers without a report.
  - The merge (N67): a report's frames decode with our values at `first -
    1` and are taken in tick order (`New` extends, `Same` fine, `Conflict`
    ignored, `Stats::report_conflicts`). A report that does not decode for
    the match's slots (the decoder cannot check a frame's `known` bits
    against them) or starts below our log's origin is `Malformed`, takes
    nothing and ends the wait for it. A report from a peer not rejoined, or
    a second one, changes nothing; one after the resume is taken the same
    way (its own inputs at once). Trust bound (N67): a rejoined, key-proven
    member's report sets final values for any slot it names, every tick
    the new host lacks up to `current + MAX_AHEAD`; an honest one never
    reaches past the old host's horizon.
  - `resume(now)` (once) returns R, our current tick: reported own inputs
    extend their slots, then bot slots below R get their previous final
    value and our own slot our pressed keys; awaited slots nobody took are
    abandoned (`Keys(0)` from their first unknown tick, as a lost peer's,
    N6), and the lagging rule for R runs at once (our tick can be
    `MAX_PREDICTION` past an absent slot: stalled there, the tick's own
    fill would never run). Each rejoined peer gets `Resume { tick: R }`
    and frames from its report's `ack` (`resend`); the pace restarts at R
    and `now`. A client's `Resume` restarts its pace at `max(current, R)`
    (the pause is not caught up) with time sync as after a jump. Both push
    `Event::Resumed { tick }` and count `Stats::migrations`.
  - `tests/migration.rs`: three sessions over a wire the test controls,
    then the harness's migrations (below).
- **Successor choice** (`migration.rs`, N65, N68, N74): `Migration::new(
  can_host, me, now)` is one peer's view from the moment it found its host
  gone; `step(now)`: the `k`-th entry that can host has the window
  `[began + k·WINDOW_US, +WINDOW_US)` (5 s): `Try(i)` for another's,
  `Host` for its own (for good once `committed()`; one not committed by
  its window's end follows the next window, D8), `Wait` past the last
  window, `GiveUp` from `began + GIVE_UP_US` (10 s) unless committed. A
  `contact(i)` (the successor answered as a host) keeps `Try(i)` until
  `GiveUp` or `lost_contact(i)`. `window_end(now)`: the deadline for peers
  not yet back. `RETRY_US` (250 ms): a refusal's retry; `MAX_MIGRATIONS`
  (4): the driver's count per match. A peer welcomed by a new host drops
  its `Migration` (A9).
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
  recorded again; a tick confirmed before it was simulated, as a
  spectator's often is, comes out once simulated). `state_loaded(tick)`
  (the driver calls it on `Event::StateLoaded`) drops the old timeline's
  candidates up to `tick`: confirmed ticks then have a gap, so `scores`
  and `result` (the world's own, self-correcting) are the truth, and the
  match end is read from `result`, not only from a `GameOver` event.
  `tests/world_sim.rs`' `tick_records_follow_rollbacks` checks them
  against an offline replay of the final inputs,
  `a_loaded_state_restarts_the_tick_records` the gap,
  `ticks_confirmed_before_they_are_simulated_are_recorded` a spectator's
  late ones.
- **Harness** (`harness.rs`, `toy.rs`): `run` plays a `Scenario` — the host
  (`HOST` = 0) and its clients and spectators over `Network`, in virtual
  time (1 ms steps, idle steps skipped), clocks skewed by `clock_ppm`,
  outages, freezes (a suspended laptop: no frames, the clock stands still,
  arrivals wait; `Scenario::host_freeze` freezes the host for `until -
  from` ticks' worth of time, its own tick standing still), spectators
  joining at `join_tick` (`Role::Joining`, `add_spectator`, at the current
  host), exits (`Exit { peer, tick, after_us, how }`: `after_us` of virtual
  time after the current host's tick reached `tick`, which stands still
  during a migration, so a peer can die between its rejoin and the resume;
  `Crash`, no frame and nothing in or out from then; `Leave`,
  `Session::leave` transmitted), profile changes (`ProfileChange`, on the
  peer's link to whoever hosts it then; outages too, an outage naming a
  host covers all its peers' links, and a link made during an outage has
  it), planted
  faults (`corrupt`: at the peer's first frame
  after the host's tick `t` with `t` confirmed,
  `Factory::corrupt(sim, index)` through `change_confirmed`), an
  `Observer` (`run_observed`: `on_event` per event and `on_frame` per
  peer frame, with the session; `run` is `run_observed` with `()`), LAN links
  for the last `settle_ticks` — and returns a `Report` (confirmed logs,
  checksums of ticks `t % 10 == 9` recorded while simulating, the
  sessions' confirmed `checksums`, stats, events, `dropped`). The host
  drops a peer it reports timed out, left (`PeerLeft`) or failed to
  repair (`RepairFailed`, `Report::repair_failed`); any host does (a new
  one too), but only of the peers that follow it (an old host left alone
  drops nobody). The run ends when the current host is past `ticks +
  settle_ticks` and every other peer not dropped, gone or lost re-simulated
  `ticks` confirmed ticks, or when every peer not dropped is gone or lost
  (no host remains; still `Ok`).
  - **Migrations** (`Scenario::migration: Some(MigrationPlan { host_human,
    slow_joins })`; with `None` a host gone moves nothing, as before), as
    rl-online drives them, every rule in virtual time:
    - succession: players in `peers` order, spectators from the start (can
      host), joined spectators (followers); a new host's is the same
      without itself, and a late peer moves to its end as a follower;
    - a peer's `HostSilent`, or `PeerLeft` from its host, begins its
      `Migration` (virtual time) and `suspend`s it; with `MAX_MIGRATIONS`
      behind it, host lost at once;
    - attempts (the peer's current profile): a successor whose gate is open
      (its `Host` step, or a host) answers after one ping (`contact`) and
      welcomes it two pings later (or `slow_joins`' time); one alive but
      not hosting refuses after one ping, asked again `RETRY_US` later; a
      dead or frozen one, or across an outage, gives no answer until the
      window moves on; a gate that closes (window end uncommitted, N69,
      give-up, crash) breaks its contacts at once (`lost_contact`);
    - a rejoin: `Network::connect`, the successor commits on the first
      (`committed`, `take_over` with the bot slots: every slot no player
      holds but `host_human`; `Factory::host_driver` replaces its driver,
      `ToyFactory` keeps the default `driver(peer, None)`), then
      `rejoined`; the peer `rehost`s and drops its migration. After the
      resume `rejoined` is false: the peer gets a new `Role::Joining`
      session and the host's `add_spectator` (N72, `Report::late`);
    - the new host resumes when every expected peer (every other
      succession entry that can host, dead ones too: it cannot know) is
      back and every peer back reported, or at its window's end with every
      peer back reported (N68, C1); expecting nobody, at once;
    - N69: a peer neither committed nor in contact that gets `HostHeard`
      `unsuspend`s and drops its migration;
    - N70: a host whose virtual frame gap reaches `STEP_DOWN_US` (its
      freeze included), with an entry that can host, `leave`s and is gone;
    - `GiveUp`: `Report::host_lost`, no more frames.
  - `Report` gains `hosts` (in order, `HOST` first), `migrations`
    (`Migrated { from, to, silent_us, resumed_us, tick }`: the first peer's
    `HostSilent`/`PeerLeft` to the resume), `host_lost`, `exited` (crashed,
    left, stepped down), `late`, `held` (what each migration's survivors
    held: the new host as it took over, each peer back at the resume; per
    slot its known prefix), `left_from` (the host each exited or host-lost
    peer followed as it went; an exited host: itself). `host_log` is the
    last host's.
  - `verdict` checks every survivor (not dropped, exited or host lost)
    confirmed the last host's inputs and matches an offline replay of them
    (a joined spectator from its state, a late peer from its last, a
    corrupted peer from its last loaded state), with no desync but a
    corrupted peer's, no refused state, no malformed message or
    inconsistent input (at any peer not dropped); every `held` value is in
    the last host's log; a peer that went (`left_from`: an exited host, an
    exited or host-lost peer) confirmed rows that agree with it up to, per
    slot, the furthest prefix the survivors of its host's migration held
    (an exited host's last frames may have reached nobody, B3), or all of
    them when its host is the last (a joined or late spectator from its
    last state). A host gone stalls its
    clients at the prediction limit: `expect_no_stalls` flags those too.
  - `ToyFactory` drives `ToySim` with seeded keys; its fault moves slot 0.

`tests/play.rs` drives a host and one client over a link the test
controls (`tests/common`: hold or drop either direction, see each
datagram's answer): leaving, coalesced `Late`, lagging, stall episodes,
repair timing.

Test: `cargo nextest run -p rl-netcode` (`tests/world_sim.rs` plays MW 1.0
on Castle Strike from `RL_CONTENT_DIR`, skipped with a notice without it;
rl-content is a dev-dependency only).
