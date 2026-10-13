# rl-online

Online play over real connections (networking spec §4): from the lobby to
the match and back, a host peer and its clients through `rl-net`'s
endpoint, each match through `rl-netcode`'s session, its replay after it.

- **Layer 5, std; not wasm.** Depends on `rl-net` (same layer: the
  endpoint, its gate and limits, `random_bytes`), `rl-netcode` (`Session`,
  `WorldSim`, `TickRecorder`), `rl-proto` (tags, lobby messages), `rl-lobby`
  (`LobbyHost`, `LobbyClient`), `rl-content` (packs, `build_match`),
  `rl-sim`, `rl-bots`, `rl-modes`, `rl-replay`, `rl-terrain`, serde,
  postcard (the desync bundle) and anyhow.
  Time is the caller's (`poll(now, ..)`); no clock is read here.
- **`matches.rs`**: `options(start)` (slot `i` is worm `i`: a member's
  `Player::HumanWith` its loadout or `Player::Human`, a bot's `Player::Bot`;
  LX56 with the setup's bonuses and loading time; empty restrictions are
  `DEFAULT_PRESET`), the bots' wire numbers (`bot_config`/`bot_wire`:
  difficulty 0-3 Easy..Xtreme, personality 0 OLX / 1 chaos, loadout 0
  presets / 1 random; out of range refused), the rules (`deathmatch`,
  `rules_wire`, every field named), `setup_packs` (mod, level, theme with
  content hashes), `check_packs`, `resolve` (a chosen loadout the
  restrictions refuse becomes random, alike everywhere), `build(content,
  start)` → `MatchWorld { sim, bots }` (its own area cache), `build_options`
  (`rl-cli`'s `net-sim` builds with it too) and `HostBots` (bots by slot,
  thinking in slot order; `press` with the host player's keys from a
  callback; `Driver<WorldSim>` for bots alone, `rl-cli`'s).
- **`peer.rs`, `OnlinePeer`**: `start(role, endpoint, me, look, content,
  setup)` (`PeerRole::Host`, `Server`: a host without a local player, the
  dedicated server's, `look` unused; `Client`); `poll(now, local_keys,
  on_tick) -> Vec<OnlineEvent>`;
  `lobby()` (`LobbyHandle`: ready, spectate, look, loadout, chat, have,
  setup, kick, move_seat (the host's), start, force_start, can_start
  (the host's answer; `NotHost`, `Running`), state, me; the host's alone:
  `end_match` (ends the running match now, as at its end, the confirmed
  ticks its record), `tell` (a notice to one member), `addr_of`,
  `ban_key`/`ban_ip` (a bans file; the address also in the gate),
  `ban_with_address` (key and address), `unban_key`/`unban_ip`, `bans`;
  `kick(member, true)` bans the key alone, spec §5.2);
  `OnlineEvent::Chat` carries the sender's member id; `match_session[_mut]`,
  `my_slot` (`None` once the match ended); `tick_phase(now)` (the
  session's, for the game's interpolation); `take_replay` (the
  `replay::PendingReplay` being made, handed out so it outlives the peer;
  a client closing once its match has a result makes it first from what
  it confirmed); `take_ended_world` (the
  match's world as last presented, kept at its end: the host's after the
  presented advance, a client's at `Ended`, before the poll's unpresented
  advance; cleared at the next start); `stats` (`NetStats`, per second);
  `last_record` (the finished match's `MatchRecord`); `leave`;
  `LobbyHandle::revision` (the host's `LobbyHost::revision`).
  `test_seed_override` exists only with the `test-hooks` feature (this
  crate's own dev-dependency).
  - **Reliable sends are never dropped** (`outbox.rs`, `Outbox`): every
    reliable message (the session's, the lobby's) waits in a per-peer FIFO
    backlog fed into the endpoint while its `buffered` for that peer is
    below `FEED_BELOW` (1 MiB), so the endpoint's `MAX_QUEUED` (4 MiB,
    fatal for the link) is never reached by a state transfer (up to
    `MAX_STATE`, 64 MiB). Ordinary messages passing `MAX_BACKLOG` (8 MiB)
    in a peer's backlog mean the peer does not read: it is closed (the
    host says "<name> did not keep up and was disconnected" and forgets
    it; a client ends, `Disconnected`). Only a state transfer may pass
    that bound (`outbox::Part`, told apart by `peer::state_part`: its
    `StateBegin` announces its chunks, counted apart up to
    `MAX_STATE_BACKLOG`), one per peer at a time: a second `StateBegin`
    while the first is still queued closes the peer as above. So a host
    holds at most 8 MiB per peer, plus the states outstanding. A closing
    peer lingers until its backlog and the endpoint's are delivered.
  - Reliable payloads carry the tag byte: the session's untagged `Control`
    gets `0` on the way out and loses it on the way in; lobby messages are
    `1`/`2`. A payload that does not decode drops the peer (host) or ends
    the connection (client).
  - **Host**: the endpoint's gate refuses the lobby's banned addresses (a
    set mirrored from `Kick { ban: true }` when the lobby banned the
    address, and from `ban_ip`/`unban_ip`). `Connected` → `connected` with
    a fresh nonce. Lobby messages leave at most every `LOBBY_DRAIN`
    (250 ms; a start, an end, a kick and a mid-match join at once). A
    `Kick` or `Close` closes the peer once its messages are delivered
    (`CLOSE_LINGER`, 1 s at most) and is `peer_lost` in the match.
    An admin's `Settings` is taken when the host has its mod, level and
    packs and the lobby takes it; else that admin gets a notice starting
    `SETUP_REFUSED`. Pings: the join handshake's round trip (challenge to
    hello), then the session's `peer_rtt_ms`, every `PING_EVERY` (2 s) into
    `set_ping`, and the next start's input delays.
  - **Start** (ruling PF1): `LobbyHost::start`/`force_start` → `build` →
    `start_checksum` = `World::checksum` of the start world →
    `announce_start` (drained at once) → `Session` (`Role::Host`: member
    peers in the slots `Seat::Player(i)`, the others `Seat::Spectator`);
    `playing(0)` at the first tick. A client builds the same world,
    compares the checksum (`MISMATCH` → `Refused` and it leaves), plays
    its slot with its input delay, or watches; a `Start` while its match
    runs is ignored (`START_IGNORED`). A member welcomed during a
    match gets `Start` after its welcome and the session's
    `add_spectator`; its client, seeing the lobby already starting or
    playing, is `Role::Joining`.
  - **Advance** (every poll): receive, `advance` with a driver giving the
    local keys (the host's bots from `HostBots`) and calling `on_tick` and
    the `TickRecorder` in `simulated`, transmit; then `Restored` (before
    `Ticked`: rebuild those areas, then apply the ticks' edits), `Ticked`,
    `Confirmed` (`take_confirmed(Session::confirmed())`). `StateLoaded` →
    `TickRecorder::state_loaded` and `OnlineEvent::StateLoaded { tick }`
    (a state loads while receiving, so it comes before that poll's
    `Restored`); a client's `Lagging` → `Lagging`; the
    host's `PeerTimedOut`/`RepairFailed`/`JoinRefused` drop the peer;
    `StateSent` → `allow_burst` on the sending side, a client's
    `StateBegin` → `allow_burst` on receiving. A client's endpoint lets its
    host send `HOST_RELIABLE_BYTES_PER_S` (4 MiB/s).
  - **End**: the first confirmed tick with `result` is the end; the match
    records `RECORD_AFTER_END` (300) more confirmed ticks (reached once
    the last is confirmed and simulated, `final_upto`: a dedicated server,
    with no slot of its own, confirms inputs its clients sent ahead before
    it simulates them, and its record would lack the last tick's
    checksum; found by `rl-cli`'s real-transport test), then the host
    sends `match_ended` (the last confirmed scores: slot, score, kills,
    deaths, lives; `last_tick`, the record's last tick, `end_at`) and `MatchEnded`, and keeps the session sending for
    `HOST_DRAIN` (2 s) so clients confirm the last ticks. A client ends at
    `Ended`, takes its `last_tick` as its own end (ruling T14-A: a `/next`
    end has no result tick, and every peer's record and replay must be
    equal) and keeps its session until it confirmed and simulated that tick
    (at most `CLIENT_FINISH`, 3 s; then the shorter record it has). Each peer then makes its replay (decision N18).
  - **Leaving**: the session's `Bye` and the lobby's (`Bye` to every member
    from a host), one endpoint poll to send them, then every connection
    closed. Refusals, kicks and lost connections end a client the same way.
- **`replay.rs`**: `MatchRecord { start, rows, checksums }` (rows below the
  last tick, the session's confirmed checksums); `make` re-simulates the
  rows from `build_match`'s start world with its `Recorder` and returns the
  `.rlrp` bytes, with a warning when a confirmed checksum differs (the
  replay is kept; the peer reports it as a `Notice`); `ReplayJob::spawn`
  runs it on a thread (`rl-online-replay`); a peer runs one at a time (a
  match ending while one is made gets `REPLAY_SKIPPED`, so a hostile host
  cannot pile up threads). A spectator that joined mid-match has no rows
  from tick 0 and makes none. `PendingReplay` (the job, its record, host
  and slot) `poll`s to its events: a missed checksum's `DesyncBundle` and
  `Notice`, then `ReplayReady`, or the failure's `Notice`.
- **`bundle.rs`** (spec §3.5, §9.10): `GameBundle` (what happened, host
  or not, `my_slot`, `FINGERPRINT`, platform `<arch>-<os>`, the
  `MatchStartWire`, the confirmed input rows from tick 0, this peer's
  checksums, the other side's stream (`other_checksums`: on the host the
  peer's compared reports, `Session::peer_checksums`; on a client the
  host's checksum of the mismatch), the mismatch `(tick, local,
  remote)`), encoded as `RLDG`, a
  u16 LE version and postcard (`.rldg`; not `rl-cli`'s `.rlds`, which
  rebuilds its world from a `NetSpec`). The session's `Desync` (both
  sides: a client's when the host reports it, the host's per peer) and a
  replay's checksum warning (its record) each give an
  `OnlineEvent::DesyncBundle(bytes)`, at most `MAX_BUNDLES_PER_MATCH` (3)
  desyncs a match; the game and the server save them with `bundle::save`
  (`desync-<unix s>-<n>.rldg`, the caller's time; at most
  `MAX_BUNDLE_BYTES` 4 MiB a bundle and `MAX_BUNDLES_DIR_BYTES` 64 MiB of
  bundles in the directory, the oldest removed; other files untouched).
- **`events.rs`**: `OnlineEvent`, `NetStats` (round trip, input delay, the
  last second's deepest rollback, a client's datagram loss, corrections
  per second, stall episodes).

Test: `cargo nextest run -p rl-online` (`tests/online.rs`, on 127.0.0.1
with port 0, Classic on Dirt Level from `RL_CONTENT_DIR`, skipped with a
notice without it; each under 20 s, every wait bounded): a host and two
clients play a 6 s match to equal confirmed inputs, checksums and replay
bytes; a late joiner watches from a state, then plays the next match; a
client whose world differs is refused at the start; a client that vanishes
mid-match leaves `Keys(0)` in its slot while the others finish; a banned
player cannot rejoin; an admin's setup is taken or refused with a notice;
a health planted in a client's world makes a bundle on both sides; a
dedicated server (`PeerRole::Server`) has no member of its own, hears chat
by member id, tells one member, knows its members' addresses, refuses a
banned key, and ends a match early; a server without bots records the
checksum of a match's last tick (a 2.997 s limit: the last tick, 599, is a
checksum tick), as its clients do; `/next` (`end_match`) on such a
server, while it has confirmed a checksum tick it has not simulated, ends
at the last tick it simulated with every checksum up to there, and every
client's record and replay equal the server's (`MatchEndWire::last_tick`).
`tests/matches.rs`: a proptest of starts within rl-proto's bounds (256
cases, about 30 build) never panics `build`; a slot both member and bot,
a single slot, an unknown mod and a pack hash mismatch are refused.
`tests/replay.rs`: a replay of confirmed inputs plays; a corrupted
confirmed checksum warns and the same replay is kept. `peer.rs`' unit
tests: one replay worker at a time; the session's state messages told
apart. `outbox.rs`' unit tests (127.0.0.1): a 10 MB state between two
lobby messages arrives whole and in order with nothing dropped; a peer that
never reads, flooded with lobby states, is closed at about 8 MiB of
backlog, reported once; a second state while one is queued closes the
peer, and chunks no `StateBegin` announced count as ordinary.
