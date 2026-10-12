# rl-proto

The network wire format (networking spec §2, §3.1, §5.4): inputs datagrams,
control messages, the frame delta codec and the simulation fingerprint.

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on serde and postcard
  only (keys are plain bytes).
- **Datagrams** (unreliable inputs channel): `Datagram::Client(ClientInputs)`
  carries the sender's own keys for ticks `first..`, its `ack` and next
  `tick`; `Datagram::Host(HostInputs)` carries `Frame`s for ticks `first..`,
  `your_ack` and the arrival `margin`. Both carry a `Stamp` for round trips.
  **Control** (reliable channel): `Checksum`, `Late`, `Desync`, `StateBegin`,
  `StateChunk`, `Bye`.
- **Frame delta rule** (`frames.rs`): `known` = slots whose final input the
  host has; `changed ⊆ known` = slots differing from the previous tick, one
  key byte each in slot order. An unchanged known slot repeats the previous
  frame's value, or `base(slot)` (the receiver's value at `first - 1`) in the
  first frame; before tick 0 every slot is `0`. `decode_frames` may call
  `out` before a later frame proves invalid: collect, then apply on `Ok`.
- **Limits:** `MAX_DATAGRAM`, `MAX_CONTROL`, `MAX_FRAMES`, `MAX_KEYS`,
  `STATE_CHUNK`, `MAX_STATE`, `MAX_SLOTS`; `OVERHEAD` is for budgets.
- **Never panic:** every decoder bounds and validates; `tests/fuzz.rs`.
- **Fingerprint** (`build.rs`, `fingerprint_impl.rs`): FNV-1a over the
  sources of rl-math, rl-data, rl-terrain, rl-sim, rl-modes and this crate
  (`\r` stripped; all of this crate's `src`). It changes whenever simulation source changes;
  peers only play with an equal fingerprint (spec §2.2).

Test: `cargo nextest run -p rl-proto`.
