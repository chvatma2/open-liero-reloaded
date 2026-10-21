# olr-server

The headless dedicated server (networking spec §7.4): a lobby without a
local player, a map rotation, bots filling empty seats, admins' chat
commands, bans kept in a file, LAN and Nostr announcements, an invite code
and a log line per event; plan 12b: its port mapped at the router, joins
through its Nostr relays, a volunteer TURN relay (`--relay`) and a network
check (`--nat-check`).
No window, no GPU, no audio: it simulates only to run the bots and check
checksums.

```
olr-server --config server.ron [--content content/packs] [--data-dir <dir>] [--relay]
olr-server --nat-check [--config server.ron]
```

Without `--content`: `olr_content::default_content_dir` (`content/packs`
beside the executable, else in the working directory).

- **Layer 6, std; a library plus the binary** (plan 10 ruling PF2:
  `olr-cli`'s real-transport tests run the server in-process; `xtask/layers.ron`
  pairs `olr-cli` with `olr-server`). Depends on `olr-online` (`OnlinePeer`
  with `PeerRole::Server` and `HostListen`, `LobbyHandle`, `Reachability`,
  `join::{path_tag, nat_text}`), `olr-lobby` (fingerprints,
  `NOT_ADMIN`, `RELAYED_BAN`, `DEFAULT_SKIN`), `olr-net` (`EndpointConfig`
  and `IceConfig`, LAN `Announcer`, `random_bytes`, `portmap::PortMapper`,
  `turn::TurnServer`, `local::base_addresses`, `probe::NatBehaviour`),
  `olr-discovery` (the summary and `Hosting`, `Pool`, `Publisher`,
  `invite`, `turn_note`/`TurnList`, the STUN codec, shared with the game;
  its `test-relay` feature for the tests), `olr-proto`, `olr-content`, `olr-bots`,
  `olr-modes`, `olr-sim`, `olr-log` (`panic_report` alone: the game's
  run log is not the server's), serde, ron, clap, ctrlc (the binary's stop) and
  anyhow. Not `olr-ui` (egui): the identity file's owner-only writing is replicated in `files.rs`.
- **`config.rs`**: `Config::parse` (RON, `deny_unknown_fields`, every field
  defaulted but the rotation; bounds: 1-32 players, ≤ 16 spectators,
  `fill_bots` ≤ 31, `min_players` 1..=`max_players`, admins 64 hex
  characters), `Visibility::{Public, Lan, Invite}` (N43; default
  `Public`; plan 10's `DirectOnly` reads as `Invite`), `relays` (default
  `DEFAULT_RELAYS`; each `wss://` or `ws://`, each once (a trailing `/`
  names the same relay), at most `MAX_RELAYS` 32), `region` (default
  `Unknown`; an unknown name reads as `Unknown`), `public_address` (an IP,
  checked; `public_ip()`; `unreachable_public_address()`: one that is IPv6
  or not global while a relay is public), `stun` (default `DEFAULT_STUN`;
  each `host:port`; `[]` asks none; final review B-M8), rotation `Entry` (`mod`,
  `level`, `time_limit` minutes, `kill_limit`, `lives`, `loadout`: a
  shipped loadout's name written bare, `loadout: "Rifle only"`, `None`
  Default; `bonuses`, issue #5, ignored but still read, written bare, so
  an older config loads: `Config::warnings` names each entry with one and
  `main` prints them),
  `host_config`, `check(content)` (the rotation against the packs),
  `upload_kib_s` (plan 11: content sent in all, KiB/s; default
  `DEFAULT_UPLOAD_KIB_S` 4096, so older configs load; `upload_rate()`
  clamps it to `UPLOAD_KIB_S`, 64..=65536, in bytes/s). Plan 12b (all
  defaulted, so older configs load): `port_mapping` (default true; N51,
  Public and Invite servers, the relay's ports), `allow_local` (default
  false; N62: the mapping's external address, `public_address` — then
  `unreachable_public_address` names only IPv6 —, the relay's address and
  its peers may be non-global), `relay: Option<RelayConfig>` (N52; `port`
  3478, `ports` (49160, 49191), `capacity` 32, `user_kib_s` 32,
  `allocations_per_ip` 4; checked: ports first ≤ last or `(0, 0)`
  (ephemeral, tests; `port: 0` too), the port outside them, capacity
  1..=`MAX_CAPACITY` 4096 and at most the number of relay ports (one
  allocation each; any with `(0, 0)`), the rest ≥ 1). No TURN server list (ruling
  PF12b-4: a host never allocates, N48). Test hook (feature
  `test-hooks`, `#[serde(skip)]`) `test_gateway: Option<TestGateway {
  pmp, ssdp, base, lifetime }>`: the port mapping aimed at a `FakeGateway`, the base
  pinned (NT-1: 127.0.0.1), the mappings' lifetime (`None`: the
  protocols'). `stun` now feeds the game socket's ICE
  (server-reflexive candidate, `NatBehaviour`), never an advertised
  address (N47). `fixtures/server/example.ron` documents every field (its
  admin key is a placeholder no one holds).
- **`rotation.rs`**: `Rotation::new` hashes each entry's packs once
  (`setup_packs`), resolves its loadout (issue #5: the entry's among the
  mod's `ModInfo::loadouts`, Default first, so a name the mod does not ship
  is an error naming those it has; none: `Default`; one too large for the
  wire, `loadout_wire`, an error) and checks the setup with 0 and
  `fill_bots` bots (`valid_setup`); `setup(bots)` (the loadout in full:
  its weapons, loading time and unique rule; bots "Bot N" in
  OLX's colours, default difficulty); `next` is the entry the lobby offers;
  `advance`; `set(Setting)` changes the next entry for the rest of the run
  (refused out of the wire's bounds).
- **`admin.rs`**: `parse(line)` → `None` (not a command) or the command or
  why not: `/kick <name>`, `/ban <name>` (the key alone, spec §5.2 and
  ruling T13-B), `/banip <name>` (the key and the address; a name ≤ 20
  characters, the rest of the line), `/unban <name|key prefix|address>`
  (≤ 64 characters; `unban_target`: an address, else a banned name in any
  case, else ≥ 8 hex characters of one key; exactly one ban), `/next`,
  `/settings <time_limit|kill_limit|lives> <value>` (≤ 16
  characters, finite), `/players`, `/help` (a deviation from
  the brief: anyone may ask for help; it lists only what the asker may use),
  `/invite` (the code; anyone's in a `Public` game, whose note is public
  anyway, else admins': `admin_only(public)`, `help(admin, public)`).
  Command words and fields in any case.
- **`files.rs`**: `server-identity.ron` (`(version: 1, secret: "<64 hex>")`,
  the game's `identity.ron` format, written atomically and owner-only;
  made at the first start; a damaged file is an error, never silently
  replaced), the bans file (`BanList`: `(keys: ["<64 hex>"], ips:
  ["1.2.3.4"], who: {"<64 hex>": (name: "Bob", ip: Some("1.2.3.4"))})`,
  `who` the name and `/banip` address of a key, for `/unban`; atomic;
  missing = none; a bad key or over 4 MiB is an error), `hex`,
  `parse_key`.
- **Finding** (plan 12a): the relay hints, the invite and the notes'
  publisher are olr-discovery's `hosting` (shared with the game since Task
  10; this crate's `finding.rs` moved there). Plan 12b retired the
  address watch here: the address comes from `Reachability::direct()`.
- **`relay.rs`** (`--relay`, N52): `Relay` (crate-private) with fresh
  credentials each start (16 lowercase hex characters each,
  `random_bytes`). With `port_mapping`, one `PortMapper` (ruling PM-2: one
  discovery) for the listening port (unless 0) first and each relay port
  (unless `(0, 0)`) as `MapRequest::extra`, UDP, exact (pre-flight fix
  #11: the relay reports `public:<local port>`, so a port the router
  would move is logged, `"relay: port {p} not mapped: {why}"`, never
  shifted), on the base and gateway the OS gives
  (`local::base_addresses`, on a thread `olr-server relay base`) or the
  `test_gateway`; `"relay: port {p} mapped ({method}) at {external}"`
  for the listening port, `"relay: {k} of {n} relay ports mapped"` once
  all answered. **Start** waits until the router answered for every port
  (Task 8 review I1), then (pre-flight fix #10) only once its public IPv4
  is known — `public_address` (IPv6 said and skipped); else the listening
  port's usable exact mapping's external IP (waiting while it is
  `Trying`); else, once STUN settled, the game socket's srflx IP, only
  when `NatBehaviour::Open` (`"relay: no port mapping: relying on {ip}
  being reachable"`); a non-global IP only with `allow_local`. Otherwise
  `NO_PUBLIC_ADDRESS` ("relay: not announced: no public address (set
  public_address, or enable port mapping)") and it never starts.
  `"relay: waiting for the public address"` at first. **Relay ports**
  (review I1): `RelayPorts::Ephemeral` for `(0, 0)`; when the router
  maps (the first port mapped, usable) only the relay ports it mapped
  exactly; else (no mapping, or a router that mapped nothing: a manual
  forward) the whole range. The note's capacity is the config's, at most
  that many ports (`"relay: capacity {n}: one allocation per relay
  port"`); none mapped: `"relay: not started: the router mapped none of
  its relay ports exactly"`. `TurnServer` on
  `0.0.0.0:port` with those ports, `TurnLimits` from the config (requests 20/s
  and `max_session` `MAX_SESSION`, 6 h, olr-net's defaults: the Task 3
  review's session cap), `allow_local`; `"relay: TURN on port {p}, relay
  ports {runs} (or ephemeral), announced as {ip}:{p}"` (runs such as
  `49160-49175, 49177-49191`); a `turn_note`
  (the server's identity, region, capacity, load = allocations,
  `ANNOUNCE_TTL`) at once and every `REPUBLISH` (60 s), `created_at` never
  going back; `"relay: {n} allocations"` at each change (looked at every
  second). A relay whose address is its listening port's mapping's
  follows the router's new external address (ruling FR12b-2): `"relay:
  the router's address is now {ip}: restarted"`, the TURN server started
  again on it and announced at once (`created_at` after the last note's). `shutdown`: the closing note (capacity 0, `CLOSE_TTL`, one
  second after the last), the server dropped, and an `Unmapping` (the
  mapper's `close` on a thread; `wait(until)` true once the router
  confirmed every deletion).
- **`nat_check.rs`** (`--nat-check`, N60): `run(config, deadline, out)`
  (`run_with(config, Some(&TestGateway), ..)` under `test-hooks`): the
  base address and gateway (`local::base_addresses`, or the hooks'); every
  configured STUN server asked from one socket (a retry after 500 ms),
  each answer or "no answer", then `"NAT: {nat_text}"` (`Open` when an
  answer is our base and port, else the first two answers alike or not:
  a symmetric NAT prints `"NAT: symmetric NAT"`, a stable wording Task 11
  greps); the router (`PortMapper` on a random port of 49152-65535, UDP,
  exact; at most 5 s; its result, a non-public external address said;
  `close`d: "router: the mapping is deleted" only when the router
  confirmed it, else "the router did not confirm it within 1 s"; under
  `test-hooks`, aimed at a fake router that always answers, it waits up to
  `TEST_CLOSE_WAIT`, 30 s, for that answer: the 1 s was overrun once on a
  loaded machine, failing `nat_check_reports_every_step`); the relays (a
  `Pool` subscribed to `TurnList::filter`; each one's status once all
  answered or `deadline`, each connected one with its clock against this
  PC's (`Pool::clocks`, the handshake's `Date`: "connected, its clock
  agrees with this PC's" within a second, else "N s ahead of|behind this
  PC's"; ruling FR12b-3), a `"clock: …"` warning when one is more than
  `MAX_SKEW` off, each connected one's handshake probe (final review
  B-M3: a `Probe` per relay, its own pool and a fresh key, a kind-21470
  event of `MAX_HANDSHAKE_CONTENT` - 64 bytes to that key once its
  subscription's `EOSE` came: `"relay {url}: forwards handshakes from
  new keys"` when it comes back, else `"… does not forward handshakes
  from new keys ({the relay's refusal | no echo within N s})"`), then
  `"TURN notes: n"` and each open relay's
  address, region and load); `"nat-check done"`. Errors only when `out`
  fails. The binary passes `NAT_CHECK_WAIT` (5 s). Public servers only
  when run by hand: the tests aim it at in-process fakes.
- **`lib.rs`, `Server`**: `start(config, content, data_dir, log)` (identity,
  bans into the lobby and the gate, the rotation, `PeerRole::Server` on the
  config's port with `HostListen` (N54: Public and Invite servers listen
  on `relays` when there are any, Lan servers do not; `public_address`
  as the configured address) and `IceConfig { stun, port_mapping:
  port_mapping && not Lan, allow_local }` (no TURN: a host never
  allocates; the `test_gateway`'s `gateway`, `ssdp`, `base_override`
  under `test-hooks`), LAN `Announcer` for `Public` and `Lan`, a Nostr
  `Pool` on `relays` for `Public` notes (none, with a warning, when
  `relays` is empty, or when `unreachable_public_address` names the
  configured address: its note would leak the LAN address to public
  relays, and every list refuses it; declined item of final review A,
  B-M2) and for a relay's notes (any visibility; one pool for both); the
  `Relay` when `relay` is set (not started without relays); the download cache
  `<data>/downloads`, `DOWNLOADS_DIR`, logged, shared with the peer as
  `PeerContent { cache, upload_rate }`: the server serves its packs to
  members lacking them (skins are never transferred: plan 13c);
  when it cannot be opened (`InstallError::Locked`: another server on
  the data directory) a warning line, and the server runs without one,
  still serving its packs), `poll(now, log)`, `shutdown(self, log)` (the bans saved; a
  public server's closing note, `created_at` after the last note's, and
  the relay's; `OnlinePeer::shutdown` — `Bye` to everyone, listening
  stopped, the endpoint's mapping deleted on a thread —, the pool dropped,
  which flushes it within 2 s; then ruling NT-2: it waits on the peer's
  `Shutdown` and the relay's `Unmapping` until `DROP_WAIT` (1 s) after
  they began, a warning if the router did not confirm), `local_port`,
  `key`, `invite()` (the key, the relays — none for a Lan server — and
  `Reachability::direct()`, N47: the usable mapping's external address
  and port, else `public_address` on the local port, else (ruling N47-a:
  a VPS) STUN's address when it is the machine's own, on the local port;
  never one STUN learned through a NAT),
  `relay_addr()` and `relay_stats()` (the TURN server's, once it runs),
  `confirmed_rows()` (the running match's rows below its session's
  frontier: what `olr-cli`'s real-transport verdict holds a server that
  freezes or leaves to). **Host migration** (plan 12c): the server sends
  succession lists and steps down through olr-online; at each match start
  it logs `"succession: … can take over"` (`LobbyHandle::succession`'s
  `can_host` entries, players by the start's slots and spectators apart:
  `succession_line`, "2 players", "1 player and 1 spectator", "nobody");
  on `HostLost(LostWhy::SteppedDown { secs })` (N70:
  a server stalled past `STEP_DOWN_US` during a match with someone to
  take over; a server is never a client, so its host losses are this and
  `CutOff`; any other is logged `"host lost: {why}"`, final review B-M6)
  `"stalled for {secs:.1} s: the match moved to the next player; the lobby
  is open again"`, `secs` olr-online's gap; `OnlineEvent::MigrationLog` is
  `"migration: {line}"`; on
  `HostLost(LostWhy::CutOff)` (ruling SB-2: it heard none of its two or more
  members for `HOST_SILENT_US`, its own network down; FR12c-1: with one
  it plays on) `"heard none of the players:
  the match moved to the next player; the lobby is open again"`.
  Each `OnlineEvent::Reachability` is kept and every new line of its
  `lines()` logged once, `"reach: …"`; each `OnlineEvent::RelayJoin` is
  `"relays: {line}"` (ruling FR12b-3: answered, refused, not connected,
  dated off the clock); each `OnlineEvent::ListenRelay` is `"relay {url}:
  {status}"` when the server has no pool of its own on its relays (an
  Invite server without `--relay`; final review B-M4). The invite is printed once
  `Reachability::settled()` (the router answered or gave up, and STUN's
  first round) or `SETTLE_WAIT` (5 s) after the start, and again whenever
  `direct()` changes: `"invite code: olr1…"` with an address, `"invite
  code (joins through the relays): olr1…"` without (`"(no address and no
  relays: it joins nothing)"` when it has neither). Announcements:
  `GameSummary::of_lobby` with `Hosting` (the level's pack name, the
  config's maximums, password flag, region, `direct()`'s IP as `addr` and
  its port as `public_port`); the LAN gets `Hosting::lan(&summary)` (the
  local port) every poll, Nostr an `announcement` signed with the
  server's identity when the `Announcer` says (every minute, at a change
  after 12 s), not before it settled (at most 5 s); `created_at` is the wall clock read at each note
  (`unix_now`; final review A-I2: a monotonic clock anchored at the start
  dated every note expired after a suspend or an NTP step). Relay statuses (not `Connecting`) and refused
  notes are logged once per relay until they change. Each poll (the lobby copied
  only when `LobbyHandle::revision` changed): the peer's events
  (chat logged, a `/` line run as a command for that member id: admins by
  the lobby's `admin` flag, others get `NOT_ADMIN`; names matched ignoring
  case; admins (and the asker) cannot be kicked or banned,
  `NOT_AN_ADMIN_TARGET`; answers are notices to the asker alone,
  `LobbyHandle::tell`;
  setting and map changes are said to everyone), joins and leaves from the
  lobby's members (`"join: {name} ({fp}) from {ip} via {path_tag}[ (admin)]
  key {hex}"`, the member's `path`, N49), the match's desyncs
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
  go to `<data>/desyncs/` through olr-online's bounded `bundle::save`.
  `run(config, content, data_dir, stop, log)` polls every `POLL_EVERY` (2 ms) until `stop`, then shuts down;
  its log says "listening on port N" at the start (tests on port 0 read
  it). `run_with(.., observe)` shows the server to `observe` after each
  poll; `last_record` is the last finished match's `MatchRecord` (`olr-cli`'s
  real-transport verdict, spec §9.4, compares every client's with it).
- **`/banip`** (N59, the Task 7 review): `ban_with_address_by(member,
  Some(admin))`; a member whose `path` is `Relayed` is banned by key only
  (its address is the relay's: banning it would close the relay to
  everyone after a restart): the lobby tells the admin `RELAYED_BAN`
  ("reached through a TURN relay: banned by key only"), the log says
  `"banip {name}: {RELAYED_BAN}"`, and its `Who` keeps no address, so the
  bans file holds none.
- **`main.rs`**: clap; `--config` (required but with `--nat-check`, which
  then uses `Config::default()`'s servers), `--relay` (sets `relay` to
  `RelayConfig::default()` when the config has none), `--nat-check`
  (`nat_check::run` to stdout, exit 0); the data directory defaults to the config file's;
  the log goes to stdout and `<data>/server.log` (`files::RotatingLog`:
  past `LOG_MAX_BYTES`, 10 MiB, at a line start, `server.log` becomes
  `server.log.1`, `.1` `.2`; `LOG_KEEP` 2 old files; `server.log` moves
  first, through `server.log.0`, so a rename that fails — Windows, a
  viewer holding it open — touches no old file: the log goes on in
  `server.log`, said once on stderr, never tried again that run), each line stamped
  with the UTC time. **Crashes** (issue #2): `files::log_panics` installs a
  panic hook that appends every panic (any thread; `olr_log::panic_report`:
  message, location, thread, a backtrace) to `server.log` through a handle
  of its own, each line stamped (`panic: …`, the rest indented,
  `files::panic_lines`), flushed, then runs the previous hook (stderr);
  the file, rotation, format and terminal output are otherwise as before
  (`tests/panic_log.rs`, a binary of its own). **Stopping** (ruling T13-A): Ctrl+C, SIGTERM or closing
  the console (Windows), through `ctrlc` 3.5.2 (`termination`), or `quit`
  (`stop`, `exit`) typed on the terminal: each sets `STOP`, and `run`
  shuts down with `Bye` to everyone.
- Chat, names and end reasons in log lines are sanitized by the lobby
  (no control characters): a member cannot forge a log line.

Test: `cargo nextest run -p olr-server` (the dev-dependency on
`olr-net/test-hooks` keeps the port mapper off real routers in tests, and
the one on itself turns on its own `test-hooks`; every test config sets
`port_mapping: false` or aims `test_gateway` at a `FakeGateway`, and no
test uses the default relays or STUN servers;
`tests/server.rs`, on 127.0.0.1 with port 0, Classic from `OLR_CONTENT_DIR`, skipped with a notice without
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
`run` stopping on its flag with `Bye`; plan 12c's
`the_server_logs_its_succession` (two loopback clients with
`MigrationConfig::direct()`, each welcomed before the next joins: the
log counts 2 once; the server not polled for 1.7 s steps down, its
`stalled for 1.7 s` line, one client hosts and both migrate, nobody loses
its host; about 2.5 s); the example and e2e configs parse and their
rotations are in the committed packs; `upload_kib_s` (default, clamped);
a member lacking Dirt Level downloads it from the server and plays,
the server's `<data>/downloads` held (`Locked`); with that cache held by
another, a warning, the level still served; finding the server (plan 12a, with olr-discovery's
in-process `TestRelay` and `StunResponder`: nothing leaves the machine, so
the shared `config` is `Invite` with `relays: []`, `stun: []` and
`port_mapping: false`): a public server listed by
a `GameList` subscribed to the relay with the right summary and no
address (STUN's answer never advertised, N47), `/invite` for anyone, a
closing note at shutdown; the printed invite joins (`expected_host`;
`public_address` 127.0.0.1, its `reach:` line), a forged key is refused
(`WRONG_HOST`), `/invite` admins only; an invite-only server publishes
nothing and advertises no STUN answer; an old config (`DirectOnly`, none
of plan 12a's or 12b's fields) loads. Plan 12b (`TestRelay`, a pinned
127.0.0.1 base, `allow_local`): an invite server without an address
joined through Nostr (`via direct`, the `reach:` lines); a server with
no NAT (STUN from a `TurnServer` on 127.0.0.1: its own address) invited
at 127.0.0.1 and its port, "reach: No router: players reach you
directly at …" (N47-a); a public server
mapped by a UPnP-only `FakeGateway` at 203.0.113.7 listed at that address
and the mapped port (TCP and UDP), its `reach: Port … (UPnP)` line, the
mapping gone after a clean stop; NT-2 (review I2): a `LateUpnp` router
(deletion effective 300 ms late) and no Nostr pool, the mapping gone the
moment `shutdown` returns (RED without the wait); the invite printed at
`SETTLE_WAIT` without an address while the (`LateUpnp`) router is busy,
then again with the mapped address; a router whose external address
changes (renewals every second): the listing, the invite and the
`reach:` line follow it (FR12b-2); `--relay` (`public_address`
127.0.0.1, ephemeral ports, generous limits) picked from a `TurnList`
(the server's key, its port, 16-hex credentials), a `relay_only` client
joining through it (`Relayed`, `via relayed`, relayed bytes, `relay: 1
allocations`); its closing note at the stop (the list empties); a relayed
member's `/banip` by key only (the admin told, the file without the
address) kept across a restart from the bans file (the key refused, a new
key from 127.0.0.1 welcomed). Unit tests: the new config fields and the
relay's checks (capacity against the port count), `allow_local`'s public
address; only exactly-mapped relay ports handed out (one mapper, a held
port said and not shifted, the TURN server on port 0 given the other
three, capacity 3 announced, the closing note, every deletion confirmed),
port runs in words; the succession line's counts; a relay on its mapping's address restarted and
announced on the router's new address (FR12b-2); `--nat-check`'s report with a `TurnServer` as STUN (`NAT: no
NAT`), a `FakeGateway` (the mapping made, said not public, deleted), a
`TestRelay` with a TURN note (and its handshake probe echoed), one
whose clock is 2 minutes ahead (its clock line, the clock warning, the
probe refused as expired) and one refusing kind 21470 (the probe
refused), two `StunResponder`s seeing different
ports (`NAT: symmetric NAT`), a `LateUpnp` router's deletion confirmed
before "deleted" is said, no STUN servers; the NAT judgement. Also: command parsing, `/unban` targets, hex keys,
the bans and identity files, the log's rotation (and a rename that
fails, injected), notices split at 200
characters, UTC dates (the hints, invites and address watch are tested
in olr-discovery's `hosting`).
Cross-build: `cargo build --release --target x86_64-pc-windows-gnu -p olr-server`.
