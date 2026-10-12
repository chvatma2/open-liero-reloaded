# rl-server

The headless dedicated server (networking spec §7.4): a lobby without a
local player, a map rotation, bots filling empty seats, admins' chat
commands, bans kept in a file, LAN announcements and a log line per event.
No window, no GPU, no audio: it simulates only to run the bots and check
checksums.

```
rl-server --config server.ron [--content content/packs] [--data-dir <dir>]
```

- **Layer 6, std; a library plus the binary** (plan 10 ruling PF2:
  `rl-cli`'s real-transport tests run the server in-process; `xtask/layers.ron`
  pairs `rl-cli` with `rl-server`). Depends on `rl-online` (`OnlinePeer`
  with `PeerRole::Server`, `LobbyHandle`), `rl-lobby` (fingerprints,
  `NOT_ADMIN`, `DEFAULT_SKIN`), `rl-net` (`EndpointConfig`, LAN
  `Announcer`, `random_bytes`), `rl-proto`, `rl-content`, `rl-bots`,
  `rl-modes`, `rl-sim`, serde, ron, clap and anyhow. Not `rl-ui` (egui):
  the identity file's owner-only writing is replicated in `files.rs`.
- **`config.rs`**: `Config::parse` (RON, `deny_unknown_fields`, every field
  defaulted but the rotation; bounds: 1-32 players, ≤ 16 spectators,
  `fill_bots` ≤ 31, `min_players` 1..=`max_players`, admins 64 hex
  characters), `Visibility::{Lan, DirectOnly}`, rotation `Entry` (`mod`,
  `level`, `time_limit` minutes, `kill_limit`, `lives`, `bonuses`),
  `host_config`, `check(content)` (the rotation against the packs).
  `fixtures/server/example.ron` documents every field (its admin key is a
  placeholder no one holds).
- **`rotation.rs`**: `Rotation::new` hashes each entry's packs once
  (`setup_packs`), picks its restrictions as the menus do (`DEFAULT_PRESET`,
  else the mod's first) and checks the setup with 0 and `fill_bots` bots
  (`valid_setup`); `setup(bots)` (loading time LX56's, bots "Bot N" in
  OLX's colours, default difficulty); `next` is the entry the lobby offers;
  `advance`; `set(Setting)` changes the next entry for the rest of the run
  (refused out of the wire's bounds).
- **`admin.rs`**: `parse(line)` → `None` (not a command) or the command or
  why not: `/kick <name>`, `/ban <name>` (a name ≤ 20 characters, the rest
  of the line), `/next`, `/settings <time_limit|kill_limit|lives|bonuses>
  <value>` (≤ 16 characters, finite), `/players`, `/help` (a deviation from
  the brief: anyone may ask for help; it lists only what the asker may use).
  Command words and fields in any case.
- **`files.rs`**: `server-identity.ron` (`(version: 1, secret: "<64 hex>")`,
  the game's `identity.ron` format, written atomically and owner-only;
  made at the first start; a damaged file is an error, never silently
  replaced), the bans file (`(keys: ["<64 hex>"], ips: ["1.2.3.4"])`,
  atomic; missing = none; a bad key or over 4 MiB is an error), `hex`,
  `parse_key`.
- **`lib.rs`, `Server`**: `start(config, content, data_dir, log)` (identity,
  bans into the lobby and the gate, the rotation, `PeerRole::Server` on the
  config's port, LAN `Announcer` for `Lan`), `poll(now, log)`, `shutdown`
  (`Bye` to everyone), `local_port`, `key`. Each poll: the peer's events
  (chat logged, a `/` line run as a command for that member id: admins by
  the lobby's `admin` flag, others get `NOT_ADMIN`; names matched ignoring
  case; answers are notices to the asker alone, `LobbyHandle::tell`;
  setting and map changes are said to everyone), joins and leaves from the
  lobby's members (fingerprint, address, full key), the match's desyncs
  and states sent (`Session::stats`), the bans file written when the
  lobby's bans change, then while gathering: the rotation's setup with
  `fill_bots - seated players` bots (an admin's own setup from the game's
  lobby is kept for one match), and a start: `start_when_ready` and every
  player ready with at least `min_players` seated, or forced (`force_start`)
  `AUTO_START` (60 s) after the lobby gathered. The rotation advances when
  a match starts (so `/settings` during a match is for the next one); a
  start the content cannot build skips the entry (or drops an admin's
  setup). `/next` ends a running
  match (`LobbyHandle::end_match`) or skips the offered map. Desync bundles
  go to `<data>/desyncs/desync-N.rldg`. `run(config, content, data_dir,
  stop, log)` polls every `POLL_EVERY` (2 ms) until `stop`, then shuts down;
  its log says "listening on port N" at the start (tests on port 0 read
  it). The bans file is compared with the lobby's bans every `BANS_EVERY`
  (1 s) and at the end.
- **`main.rs`**: clap; the data directory defaults to the config file's;
  the log goes to stdout and `<data>/server.log`, each line stamped with
  the UTC time. **Stopping**: type `quit` (or `stop`, `exit`) on the
  terminal. Ctrl+C (SIGINT) is not caught: the workspace forbids unsafe
  code and no signal crate is in the plan's Tech Stack, so Ctrl+C ends the
  process without `Bye` (clients notice within `SILENCE_TIMEOUT`, 10 s).
- Chat, names and end reasons in log lines are sanitized by the lobby
  (no control characters): a member cannot forge a log line.

Test: `cargo nextest run -p rl-server` (`tests/server.rs`, on 127.0.0.1
with port 0, Classic from `RL_CONTENT_DIR`, skipped with a notice without
it; each under 20 s, every wait bounded): a rotation (two bot-driven
clients ready, entry 0 on Dirt Level with one fill bot, its end, the lobby
offering entry 1 on Liero Factory, which starts once both are ready again;
the identity file owner-only; the log's lines); admins' `/kick` and `/ban`
(the banned key refused, the bans file, a restart says `Bye` and keeps the
identity and the ban, by key alone too); non-admins refused with
`NOT_ADMIN`, `/players` and `/help` for anyone; `/settings` and `/next`;
`run` stopping on its flag with `Bye`; the example config parses and its
rotation is in the committed packs. Unit tests: command parsing, hex keys,
the bans and identity files, notices split at 200 characters, UTC dates.
Cross-build: `cargo build --release --target x86_64-pc-windows-gnu -p rl-server`.
