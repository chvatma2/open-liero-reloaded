# olr-proto

The network wire format (networking spec §2, §3.1, §5.4): inputs datagrams,
control messages, the frame delta codec and the simulation fingerprint;
plan 12b: a member's `Path`, the STUN binding codec and `is_global`.

- **Layer 4, `#![no_std]` + `alloc`, wasm32.** Depends on `olr-data` (`text`: name rules), serde and postcard
  only (keys are plain bytes).
- **Datagrams** (unreliable inputs channel): `Datagram::Client(ClientInputs)`
  carries the sender's own keys for ticks `first..`, its `ack` and next
  `tick`; `Datagram::Host(HostInputs)` carries `Frame`s for ticks `first..`,
  `your_ack`, the arrival `margin` and `gone`. Both carry a `Stamp` for round trips.
  `Datagram::Resume(HostResume { tick, at_ms, first, frames, gone })` (plan 12c,
  ruling RS-1, appended): a new host's resume, `Control::Resume`'s
  unreliable twin (frames <= `MAX_FRAMES`, each within the slots), sent
  every tick until the player's inputs come.
  **Control** (reliable channel): `Checksum`, `Late { from, to }` (one per
  answered datagram, spanning its filled ticks, on-time ones between two
  fill episodes included; `from < to` or `Invalid`), `Desync`,
  `StateBegin`, `StateChunk`, `Bye`, `StateLoaded`, `StateRefused`; plan 12c
  (host migration, spec §3.7): `Collect { known }` (1..=`MAX_SLOTS`),
  `Report(InputsReport { ack, origin, first, frames, gone, own_first, own })`
  (`origin`: where the reporter's log begins, 0 or a joined spectator's
  first tick after its state, at most `ack`, ruling SK-9; frames <=
  `MAX_REPORT_FRAMES` 3000, own <= `MAX_REPORT_OWN` 256, no `u32` overflow of
  `first + len` or `own_first + len`, each frame `changed ⊆ known` with a key
  per changed bit) and `Resume { tick, at_ms, first, frames, gone }` (`at_ms`:
  when, on the receiver's own stamp clock, its inputs reach the new host as
  it runs `tick`, 0 unknown; the final inputs the peer lacks, delivered
  with the resume; the same frame bounds as a report's). A worst-case report or resume exceeds `MAX_CONTROL`: the
  sender trims it (olr-netcode).
- **Withdrawal marks** (plan 12d, decision N83, ruling PF12d-4): every
  message that carries final inputs (`HostInputs`, `HostResume`,
  `InputsReport`, `Control::Resume`) has `gone: Vec<(u8, u32)>` after its
  `frames`: `(slot, tick)` for each worm withdrawn at a tick within the
  frames, once per message, not a bit per frame (`Frame` and the frame codec
  are unchanged; an empty list is one byte). `valid_marks(first, len,
  marks, slots)`: at most `MAX_SLOTS` entries, each slot below `slots` and
  listed once, each tick in `first..first + len`; else `Invalid`.
  `decode_datagram` checks it against the match's slots, `decode_control`
  against `MAX_SLOTS` (the receiver's log ignores a slot beyond its match).
  The receiver takes a mark with its tick's value (olr-netcode).
- **Signalling** (`signal.rs`, WebSocket, one postcard message per frame,
  `MAX_SIGNAL`): `Signal::{Offer, Answer, Refused}`, `Refusal` (`Version`,
  `Full`, `Banned`, `Busy`, `Malformed`, `Password`, `Exhausted`, `Throttled`, `NotHost` (a standby before its peer took over),
  `Closed` (a migrated lobby takes back its own players only, N72): also the lobby's
  `ToClient::Refused`); sdp <= 8 KiB.
- **Lobby** (`lobby.rs`, reliable channel, `MAX_LOBBY` = 60 KiB: a full
  lobby at every bound is about 40 KB, `worst_case_messages_fit`): `ToHost`,
  `ToClient`, `LobbyState`, `MatchStartWire`, `MatchEndWire` and friends.
  Joining: `Challenge` → `Hello` (with the joiner's 32-byte `nonce`) →
  `HostProof { signature }` (the host proves its key, N37) → `Prove {
  signature, password }` (the password proof only now, to a proven host;
  olr-lobby binds it to the joiner's key and the link) → `Welcome`.
  **Tag byte** first on every reliable payload: `0` netcode `Control`,
  `1` `ToHost`, `2` `ToClient`; `tag()` splits it (`Tagged`), `None` for an
  empty payload or unknown tag. The netcode session still sends untagged
  `Control`; olr-online tags and untags at the boundary.
- **Migration lobby messages** (plan 12c): `ToHost::Standby { port, relays }`
  (port >= `MIN_STANDBY_PORT` 1024; relays valid, <= `MAX_STANDBY_RELAYS` 2) and
  `ToClient::Succession(Vec<SuccessorWire { member, key, direct, relays,
  can_host }>)` (`valid_succession`: <= `MAX_MEMBERS` entries, unique ids (any `u8`: ids count up) and unique keys,
  relays valid, direct ports >= 1024 and not 0.0.0.0, broadcast or multicast). `valid_relay_url`: `ws://` or `wss://` plus a host,
  printable ASCII without spaces, <= `MAX_RELAY_URL` 128 bytes. A worst-case
  `Succession` fits `MAX_LOBBY` (`tests/migration.rs`). The host counts
  `Standby` changes at most once per `STANDBY_WINDOW_US` (olr-lobby): the
  decoder bounds one message only.
- **Content** (`content.rs`, plan 11, N32): `TAG_CONTENT = 3` carries
  `ContentMsg::{List, Want, Begin, Chunk, Unavailable, Cancel}` both ways
  (`encode_content` with the tag, `decode_content` without; `valid_content`);
  `MAX_CONTENT` (`CONTENT_CHUNK` + 256) bounds one message (a worst-case
  64-item `List` fits). `List { revision, items }`: `ToHost::Have {
  revision, missing }` answers that revision (only the host lists). `Item
  { kind, id, hash, size, required }`; a list has no hash twice and every
  item is `required` (`valid_content` refuses an optional one; skins are
  never listed, plan 13c).
  `MAX_ARCHIVE_BYTES` restates olr-pack's `MAX_ARCHIVE`. `valid_hash` is 64
  lowercase hex; `NO_HASH` is valid and means unknown. `Look::skin_hash`
  and `Member::download` (percent) are new; `PROTOCOL_VERSION` is 3 (plan 13c: skins are matched by id and hash, never transferred).
- **LAN** (`lan.rs`): `LanAnnounce` on UDP `LAN_PORT` (23401), `MAX_LAN`;
  `players` the seated members (at most `max_players`, itself at most
  `MAX_PLAYERS`), `bots` apart (appended; at most `MAX_PLAYERS`: bots take
  no seat, the user's report of 2026-10-15).
- **Paths** (N49, N61): `Path { Direct, MappedPort, HolePunched, Relayed }`,
  how a link reaches its peer (olr-net classifies ICE's selected pair);
  `Member::path` (appended; `None` for the host's own player).
- **STUN and addresses** (N57, moved from olr-discovery; `core::net`):
  `stun::{binding_request, parse_binding_response, binding_response,
  is_stun, binding_request_transaction}` (RFC 5389 binding codec, bounded,
  never panics; `is_stun`: first two bits 0, the magic cookie, a length
  that is a multiple of 4 and matches) and `is_global` (`addr.rs`, also
  re-exported at the root: hand-written v4/v6 ranges, v4-mapped and
  v4-compatible unwrapped). olr-discovery re-exports both; olr-net's ICE
  gathering and natsim use them. They are under `src`, so the fingerprint
  covers them.
- **Bounds** (violation = `ProtoError::Invalid`): names 1..=`MAX_NAME` (20)
  characters, no control, bidi, zero-width or separator code points, no edge
  whitespace; chat, notice and end reason <= `MAX_CHAT` (200) characters
  (chat: no bidi controls); members <= 48 with unique ids; slots <= 32; player
  seats <= 32 with unique slots; `SlotWire.bot` indexes `setup.bots`;
  input delays <= 3; `MatchEndWire::last_tick` below `u32::MAX`; packs <= 8; bots <= 31; `missing` <= 8; pack, skin, mod
  and level ids ASCII `[a-z0-9-]` (olr-pack's manifest rule), 1..=64 bytes;
  hashes 64 lowercase hex; rule floats finite; restrictions <= 256 bytes.
  `Signature` (64 bytes) uses a custom serde tuple (serde stops at 32).
  `valid_name`, `valid_pack_id`, `valid_look`, `valid_setup` and
  `valid_chat` are public so producers (olr-lobby) never send what the
  decoders refuse.
- **Frame delta rule** (`frames.rs`): `known` = slots whose final input the
  host has; `changed ⊆ known` = slots differing from the previous tick, one
  key byte each in slot order. An unchanged known slot repeats the previous
  frame's value, or `base(slot)` (the receiver's value at `first - 1`) in the
  first frame; before tick 0 every slot is `0`. `decode_frames` may call
  `out` before a later frame proves invalid: collect, then apply on `Ok`.
- **Limits:** `MAX_DATAGRAM`, `MAX_CONTROL`, `MAX_FRAMES`, `MAX_KEYS`,
  `STATE_CHUNK`, `MAX_STATE`, `MAX_SLOTS`; `OVERHEAD` is for budgets.
- **Never panic:** every decoder bounds and validates; `tests/fuzz.rs` (marks
  included: `host_datagrams_with_marks_never_panic`), `tests/fuzz_lobby.rs`.
- **Fingerprint** (`build.rs`, `fingerprint_impl.rs`): FNV-1a over the
  sources of olr-math, olr-data, olr-terrain, olr-sim, olr-modes, olr-netcode
  (the session's semantics: checksum cadence, acks, resends, `WorldSim`'s
  checksum) and this crate (`\r` stripped; every `.rs` under each `src`,
  `rerun-if-changed` on each; an unreadable source is a build panic). It
  changes whenever that source changes; peers only play with an equal
  fingerprint (spec §2.2). olr-netcode depends on this crate: `build.rs`
  only reads its files, never depends on it. `tests/fingerprint.rs`
  recomputes it over the list.

Test: `cargo nextest run -p olr-proto` (`tests/stun.rs`: the RFC 5769
vector, `is_stun`, fuzzed responses; `tests/addr.rs`: `is_global`'s ranges).
