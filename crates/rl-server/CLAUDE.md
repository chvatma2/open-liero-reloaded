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
  `rl-modes`, `rl-sim`, serde, ron, clap, ctrlc (the binary's stop) and
  anyhow. Not `rl-ui` (egui): the identity file's owner-only writing is replicated in `files.rs`.
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
  why not: `/kick <name>`, `/ban <name>` (the key alone, spec §5.2 and
  ruling T13-B), `/banip <name>` (the key and the address; a name ≤ 20
  characters, the rest of the line), `/unban <name|key prefix|address>`
  (≤ 64 characters; `unban_target`: an address, else a banned name in any
  case, else ≥ 8 hex characters of one key; exactly one ban), `/next`,
  `/settings <time_limit|kill_limit|lives|bonuses> <value>` (≤ 16
  characters, finite), `/players`, `/help` (a deviation from
  the brief: anyone may ask for help; it lists only what the asker may use).
  Command words and fields in any case.
- **`files.rs`**: `server-identity.ron` (`(version: 1, secret: "<64 hex>")`,
  the game's `identity.ron` format, written atomically and owner-only;
  made at the first start; a damaged file is an error, never silently
  replaced), the bans file (`BanList`: `(keys: ["<64 hex>"], ips:
  ["1.2.3.4"], who: {"<64 hex>": (name: "Bob", ip: Some("1.2.3.4"))})`,
  `who` the name and `/banip` address of a key, for `/unban`; atomic;
  missing = none; a bad key or over 4 MiB is an error), `hex`,
  `parse_key`.
- **`lib.rs`, `Server`**: `start(config, content, data_dir, log)` (identity,
  bans into the lobby and the gate, the rotation, `PeerRole::Server` on the
  config's port, LAN `Announcer` for `Lan`), `poll(now, log)`, `shutdown`
  (`Bye` to everyone), `local_port`, `key`. Each poll: the peer's events
  (chat logged, a `/` line run as a command for that member id: admins by
  the lobby's `admin` flag, others get `NOT_ADMIN`; names matched ignoring
  case; admins (and the asker) cannot be kicked or banned,
  `NOT_AN_ADMIN_TARGET`; answers are notices to the asker alone,
  `LobbyHandle::tell`;
  setting and map changes are said to everyone), joins and leaves from the
  lobby's members (fingerprint, address, full key), the match's desyncs
  and states sent (`Session::stats`), the bans file written at once after
  `/ban`, `/banip`, `/unban` and checked every `BANS_EVERY` (1 s, an
  admin's ban from the game's lobby; a write failure retried then and said
  at most every minute), then while gathering: the rotation's setup with
  `fill_bots - seated players` bots (a change of bots alone keeps the
  players' ready marks, ruling T13-C; an admin's own setup from the game's
  lobby is kept for one match), and a start: `start_when_ready` and every
  player ready with at least `min_players` seated, or forced (`force_start`)
  `AUTO_START` (60 s) after the lobby gathered. The rotation advances when
  a match starts (so `/settings` during a match is for the next one); a
  start the content cannot build skips the entry, said once per entry
  until a start succeeds (or drops an admin's setup); when every entry
  failed in a row no start is tried until the server restarts (an error
  line; the lobby stays open). `/next` ends a running
  match (`LobbyHandle::end_match`) or skips the offered map. Desync bundles
  go to `<data>/desyncs/` through rl-online's bounded `bundle::save`.
  `run(config, content, data_dir, stop, log)` polls every `POLL_EVERY` (2 ms) until `stop`, then shuts down;
  its log says "listening on port N" at the start (tests on port 0 read
  it). `run_with(.., observe)` shows the server to `observe` after each
  poll; `last_record` is the last finished match's `MatchRecord` (`rl-cli`'s
  real-transport verdict, spec §9.4, compares every client's with it).
- **`main.rs`**: clap; the data directory defaults to the config file's;
  the log goes to stdout and `<data>/server.log`, each line stamped with
  the UTC time. **Stopping** (ruling T13-A): Ctrl+C, SIGTERM or closing
  the console (Windows), through `ctrlc` 3.5.2 (`termination`), or `quit`
  (`stop`, `exit`) typed on the terminal: each sets `STOP`, and `run`
  shuts down with `Bye` to everyone.
- Chat, names and end reasons in log lines are sanitized by the lobby
  (no control characters): a member cannot forge a log line.

Test: `cargo nextest run -p rl-server` (`tests/server.rs`, on 127.0.0.1
with port 0, Classic from `RL_CONTENT_DIR`, skipped with a notice without
it; each under 20 s, every wait bounded): a rotation (two bot-driven
clients ready, entry 0 on Dirt Level with one fill bot, its end, the lobby
offering entry 1 on Liero Factory, which starts once both are ready again;
the identity file owner-only; the log's lines; the server's record equal
to the clients'); admins' `/kick`, `/ban`
(by key: another key from the same address joins; the bans file written at
once), admins not kickable, `/banip` (a new key from the address refused),
`/unban` by name (key and address) and by key prefix, a restart says `Bye`
and keeps the identity and the ban; a joiner on a server full of bots
leaves the others ready (T13-C); a rotation the content cannot build
skipped once per entry, then no more starts; non-admins refused with
`NOT_ADMIN`, `/players` and `/help` for anyone; `/settings` and `/next`;
`run` stopping on its flag with `Bye`; the example and e2e configs parse and their
rotations are in the committed packs. Unit tests: command parsing, `/unban` targets, hex keys,
the bans and identity files, notices split at 200 characters, UTC dates.
Cross-build: `cargo build --release --target x86_64-pc-windows-gnu -p rl-server`.
