# rl-netcode

The rollback netcode (networking spec §3) and the deterministic network
model that tests it in one process (spec §9.1).

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on rl-sim (keys, the
  world adapter of `world_sim.rs`), rl-proto (same layer: the wire format).
- **Sans-IO**: no sockets, no clock, no threads; time is an argument
  (`now_us`).
- **`Simulation`** is what the session drives (tick, save/restore,
  checksum, state transfer); `ToySim` is a tiny deterministic implementation
  for the session's tests. `decode_state` never panics: it returns
  `StateRejected`.
- **Network model** (`netsim`): `Network` carries datagrams between peers
  with per-direction latency, jitter, Gilbert-Elliott loss, duplication,
  outages; reliable messages arrive in order. Deterministic from its seed.

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
    ticks only.
  - Prediction: final, else a client's own provisional input, else the
    slot's last final value, else `0`. A final value differing from the keys
    a simulated tick used marks a rollback; the next `advance` restores the
    snapshot before it and re-simulates (`Event::Rollback`, `depth_hist`).
    Snapshots are kept from the frontier on, then `forget_before`.
  - Messages: the host sends each peer frames from its `ack` (≤ `MAX_FRAMES`,
    halved until ≤ `MAX_DATAGRAM`) with `your_ack`; a client sends its own
    inputs from `your_ack` (≤ `MAX_KEYS`) and its frontier as `ack`. Every
    datagram carries a `Stamp`; `rtt_ms` is a 1/8 moving average.
  - Pace (`clock.rs`, `Pace`): local time to due ticks (minus ticks run,
    at most `MAX_CATCHUP` per `advance`), adjustable by ±1 % (re-anchored,
    so a change affects only later time) and shiftable by whole ticks (a
    jump). A player peer stalls at `MAX_PREDICTION` ticks past its frontier
    (`Event::Stalled`). Every peer sends at most one datagram per tick of
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
    (`Stats::fills`); a value arriving later for a filled tick is ignored
    and answered once by `Control::Late` (`Event::Late` at the client).
    300 consecutive host ticks with a fill: `Event::PeerLate`. Silence for
    `SILENCE_TIMEOUT_US`: `Event::PeerTimedOut` (host and client). The
    driver decides; `peer_lost` abandons a peer: its slot is `Keys(0)` at
    every new tick (decision N6), nothing more is sent to it or taken.
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
  (repaired) or one from `REPAIR_MARGIN` (300) ticks after the sending on
  differs (refused, or diverged again: a new desync). After `MAX_REPAIRS`
  (3) states, or with a state too large to send, `Event::RepairFailed`
  and no more repairs. `add_spectator` for a player's peer is refused
  (`Event::JoinRefused`).
  The receiver refuses a transfer with an index out of range, a repeated
  chunk or more bytes than announced (`StateRejected`, later chunks
  ignored); a whole one is `StateArrived` (the bytes), then
  `decode_state`: refused, `StateRejected` and the old state stays;
  loaded, `StateLoaded` and the timeline restarts after it: no
  snapshots, the **state floor** at `tick + 1` (ticks below it count as
  confirmed for rollback and checksums; their inputs still arrive and are
  recorded, never rolling back), the ticks we had run simulated again at
  the next advance (a client behind the state skips to it). `sim_mut`
  also moves the floor to the current tick (a change outside the timeline
  is never rolled back across: planted faults).
- **Spectators mid-match**: `Role::Joining` simulates nothing and ignores
  frames until a state loads, then is a `Spectator` whose input log
  begins after the state (`InputLog::start_at`). The host's
  `add_spectator` owes it the state like a repair; its frames then start
  there with no base (`origin`).
- **WorldSim** (`world_sim.rs`): the `rl_sim::World` adapter, rollback
  on (`enable_rollback`); `save`/`restore`/`forget_before` are the
  world's (`restore` refused is a session bug: `expect`); `checksum` is
  `sync_checksum` (N4); states are `WorldState::{encode, decode}` +
  `load_state` (N5), every `StateError` a refusal; `events()` are the last
  simulated tick's (stale after a restore or a load until the next tick).
- **Harness** (`harness.rs`, `toy.rs`): `run` plays a `Scenario` — the host
  (`HOST` = 0) and its clients and spectators over `Network`, in virtual
  time (1 ms steps, idle steps skipped), clocks skewed by `clock_ppm`,
  outages, freezes (a suspended laptop: no frames, the clock stands still,
  arrivals wait), spectators joining at `join_tick` (`Role::Joining`,
  `add_spectator`), planted faults (`corrupt`: at the peer's first frame
  after the host's tick, `Factory::corrupt(sim_mut, index)`), LAN links
  for the last `settle_ticks` — and returns a `Report` (confirmed logs,
  checksums of ticks `t % 10 == 9` recorded while simulating, the
  sessions' confirmed `checksums`, stats, events, `dropped`). The host
  drops a peer it reports timed out or failed to repair (`RepairFailed`,
  `Report::repair_failed`). The run ends when the host is past `ticks +
  settle_ticks` and every other peer re-simulated `ticks` confirmed
  ticks. `verdict` checks every peer not
  dropped confirmed the host's inputs and matches an offline replay of
  them (a joined spectator from its state, a corrupted peer from its last
  loaded state), with no desync but a corrupted peer's and no refused
  state. `ToyFactory` drives `ToySim` with seeded keys; its fault moves
  slot 0.

Test: `cargo nextest run -p rl-netcode` (`tests/world_sim.rs` plays MW 1.0
on Castle Strike from `RL_CONTENT_DIR`, skipped with a notice without it;
rl-content is a dev-dependency only).
