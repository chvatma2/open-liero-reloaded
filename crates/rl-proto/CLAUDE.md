# rl-proto

The network wire format (networking spec §2, §3.1, §5.4): inputs datagrams,
control messages, the frame delta codec and the simulation fingerprint.

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on serde and postcard
  only (keys are plain bytes).
- **Datagrams** (unreliable inputs channel): `Datagram::Client(ClientInputs)`
  carries the sender's own keys for ticks `first..`, its `ack` and next
  `tick`; `Datagram::Host(HostInputs)` carries `Frame`s for ticks `first..`,
  `your_ack` and the arrival `margin`. Both carry a `Stamp` for round trips.
  **Control** (reliable channel): `Checksum`, `Late { from, to }` (one per
  answered datagram, spanning its filled ticks, on-time ones between two
  fill episodes included; `from < to` or `Invalid`), `Desync`,
  `StateBegin`, `StateChunk`, `Bye`, `StateLoaded`, `StateRefused`.
- **Signalling** (`signal.rs`, WebSocket, one postcard message per frame,
  `MAX_SIGNAL`): `Signal::{Offer, Answer, Refused}`, `Refusal` (`Version`,
  `Full`, `Banned`, `Busy`, `Malformed`, `Password`, `Exhausted`, `Throttled`: also the lobby's
  `ToClient::Refused`); sdp <= 8 KiB.
- **Lobby** (`lobby.rs`, reliable channel, `MAX_LOBBY` = 60 KiB: a full
  lobby at every bound is about 40 KB, `worst_case_messages_fit`): `ToHost`,
  `ToClient`, `LobbyState`, `MatchStartWire`, `MatchEndWire` and friends.
  Joining: `Challenge` → `Hello` (with the joiner's 32-byte `nonce`) →
  `HostProof { signature }` (the host proves its key, N37) → `Prove {
  signature, password }` (the password proof only now, to a proven host;
  rl-lobby binds it to the joiner's key and the link) → `Welcome`.
  **Tag byte** first on every reliable payload: `0` netcode `Control`,
  `1` `ToHost`, `2` `ToClient`; `tag()` splits it (`Tagged`), `None` for an
  empty payload or unknown tag. The netcode session still sends untagged
  `Control`; rl-online tags and untags at the boundary.
- **Content** (`content.rs`, plan 11, N32): `TAG_CONTENT = 3` carries
  `ContentMsg::{List, Want, Begin, Chunk, Unavailable, Cancel}` both ways
  (`encode_content` with the tag, `decode_content` without; `valid_content`);
  `MAX_CONTENT` (`CONTENT_CHUNK` + 256) bounds one message (a worst-case
  64-item `List` fits). `List { revision, items }`: `ToHost::Have {
  revision, missing }` answers that revision (a client's one-item skin
  offer to the host is revision 0). `Item { kind, id, hash, size,
  required }`; a list has no hash twice, skins are <= 64 KiB and never
  required.
  `MAX_ARCHIVE_BYTES` restates rl-pack's `MAX_ARCHIVE`. `valid_hash` is 64
  lowercase hex; `NO_HASH` is valid and means unknown. `Look::skin_hash`
  and `Member::download` (percent) are new; `PROTOCOL_VERSION` is 2.
- **LAN** (`lan.rs`): `LanAnnounce` on UDP `LAN_PORT` (23401), `MAX_LAN`.
- **Paths** (N49, N61): `Path { Direct, MappedPort, HolePunched, Relayed }`,
  how a link reaches its peer (rl-net classifies ICE's selected pair);
  `Member::path` (appended; `None` for the host's own player).
- **STUN and addresses** (N57, moved from rl-discovery; `core::net`):
  `stun::{binding_request, parse_binding_response, binding_response,
  is_stun, binding_request_transaction}` (RFC 5389 binding codec, bounded,
  never panics; `is_stun`: first two bits 0, the magic cookie, a length
  that is a multiple of 4 and matches) and `is_global` (`addr.rs`, also
  re-exported at the root: hand-written v4/v6 ranges, v4-mapped and
  v4-compatible unwrapped). rl-discovery re-exports both; rl-net's ICE
  gathering and natsim use them. They are under `src`, so the fingerprint
  covers them.
- **Bounds** (violation = `ProtoError::Invalid`): names 1..=`MAX_NAME` (20)
  characters, no control, bidi, zero-width or separator code points, no edge
  whitespace; chat, notice and end reason <= `MAX_CHAT` (200) characters
  (chat: no bidi controls); members <= 48 with unique ids; slots <= 32; player
  seats <= 32 with unique slots; `SlotWire.bot` indexes `setup.bots`;
  input delays <= 3; `MatchEndWire::last_tick` below `u32::MAX`; packs <= 8; bots <= 31; `missing` <= 8; pack, skin, mod
  and level ids ASCII `[a-z0-9-]` (rl-pack's manifest rule), 1..=64 bytes;
  hashes 64 lowercase hex; rule floats finite; restrictions <= 256 bytes.
  `Signature` (64 bytes) uses a custom serde tuple (serde stops at 32).
  `valid_name`, `valid_pack_id`, `valid_look`, `valid_setup` and
  `valid_chat` are public so producers (rl-lobby) never send what the
  decoders refuse.
- **Frame delta rule** (`frames.rs`): `known` = slots whose final input the
  host has; `changed ⊆ known` = slots differing from the previous tick, one
  key byte each in slot order. An unchanged known slot repeats the previous
  frame's value, or `base(slot)` (the receiver's value at `first - 1`) in the
  first frame; before tick 0 every slot is `0`. `decode_frames` may call
  `out` before a later frame proves invalid: collect, then apply on `Ok`.
- **Limits:** `MAX_DATAGRAM`, `MAX_CONTROL`, `MAX_FRAMES`, `MAX_KEYS`,
  `STATE_CHUNK`, `MAX_STATE`, `MAX_SLOTS`; `OVERHEAD` is for budgets.
- **Never panic:** every decoder bounds and validates; `tests/fuzz.rs`, `tests/fuzz_lobby.rs`.
- **Fingerprint** (`build.rs`, `fingerprint_impl.rs`): FNV-1a over the
  sources of rl-math, rl-data, rl-terrain, rl-sim, rl-modes, rl-netcode
  (the session's semantics: checksum cadence, acks, resends, `WorldSim`'s
  checksum) and this crate (`\r` stripped; every `.rs` under each `src`,
  `rerun-if-changed` on each; an unreadable source is a build panic). It
  changes whenever that source changes; peers only play with an equal
  fingerprint (spec §2.2). rl-netcode depends on this crate: `build.rs`
  only reads its files, never depends on it. `tests/fingerprint.rs`
  recomputes it over the list.

Test: `cargo nextest run -p rl-proto` (`tests/stun.rs`: the RFC 5769
vector, `is_stun`, fuzzed responses; `tests/addr.rs`: `is_global`'s ranges).
