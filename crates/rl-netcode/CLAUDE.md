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
  - Pace (`due_ticks`, replaced in Task 7): elapsed ticks minus ticks run,
    at most `MAX_CATCHUP` per `advance`; a player peer stalls at
    `MAX_PREDICTION` ticks past its frontier (`Event::Stalled`).
  - `receive` never panics: undecodable input is `Event::Malformed`.
- **Harness** (`harness.rs`, `toy.rs`): `run` plays a `Scenario` — the host
  (`HOST` = 0) and its clients and spectators over `Network`, in virtual
  time (1 ms steps, idle steps skipped), clocks skewed by `clock_ppm`,
  outages, LAN links for the last `settle_ticks` — and returns a `Report`
  (confirmed logs, checksums of ticks `t % 10 == 9`, stats, events).
  `verdict` checks every peer confirmed the host's inputs and matches an
  offline replay of them. `ToyFactory` drives `ToySim` with seeded keys.

Test: `cargo nextest run -p rl-netcode`.
