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
  - `receive` never panics: undecodable input is `Event::Malformed`.
- **Harness** (`harness.rs`, `toy.rs`): `run` plays a `Scenario` — the host
  (`HOST` = 0) and its clients and spectators over `Network`, in virtual
  time (1 ms steps, idle steps skipped), clocks skewed by `clock_ppm`,
  outages, freezes (a suspended laptop: no frames, the clock stands still,
  arrivals wait), LAN links for the last `settle_ticks` — and returns a
  `Report` (confirmed logs, checksums of ticks `t % 10 == 9`, stats,
  events, `dropped`). The host drops a peer it reports timed out. The run
  ends when the host is past `ticks + settle_ticks` and every other peer
  re-simulated `ticks` confirmed ticks. `verdict` checks every peer not
  dropped confirmed the host's inputs and matches an offline replay of
  them. `ToyFactory` drives `ToySim` with seeded keys.

Test: `cargo nextest run -p rl-netcode`.
