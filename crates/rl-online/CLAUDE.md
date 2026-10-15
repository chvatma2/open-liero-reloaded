# rl-online

Online play over real connections (networking spec §4): from the lobby to
the match and back, a host peer and its clients through `rl-net`'s
endpoint, each match through `rl-netcode`'s session, its replay after it.

- **`OnlinePeer: Send`** (a compile-time assert in `lib.rs`): rl-game
  polls a peer from its UI thread and from its network pump (`rl-game
  net`, ruling PF12c-2), so no `Rc` or other `!Send` part may enter the
  peer's stack.
- **Layer 5, std; not wasm.** Depends on `rl-net` (same layer: the
  endpoint, its gate and limits, `random_bytes`), `rl-discovery` (same
  layer, ruling N55: the relay `Pool`, `Inbox`/`Asker`, `TurnList`), `rl-netcode` (`Session`,
  `WorldSim`, `TickRecorder`), `rl-proto` (tags, lobby messages), `rl-lobby`
  (`LobbyHost`, `LobbyClient`), `rl-content` (packs, `build_match`),
  `rl-sim`, `rl-bots`, `rl-modes`, `rl-replay`, `rl-terrain`, `rl-pack`
  (archives, content hashes), `rl-data` (`PackKind`), `rl-vfs` (`DirFs`),
  serde, postcard (the desync bundle), sha2 and hex (archive hashes) and
  anyhow.
  Time is the caller's (`poll(now, ..)`); the only clocks read here are
  the Nostr side's: event dates need the system's unix time (`join`'s
  `unix_now`), and the host's listener thread keeps its own pace.
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
- **`peer.rs`, `OnlinePeer`** (its `LobbyHandle` in `peer/lobby_handle.rs`): `start(role, endpoint, me, look, content,
  setup, content_cfg)` (`PeerRole::Host { lobby, listen }`, `Server {
  lobby, listen }`: a host without a local player, the dedicated
  server's, `look` unused; `listen: HostListen`, `reach.rs`; `Client {
  join, password, expected_host }`: `join: Join`, `join.rs`; a join
  through Nostr expects its host's key without `expected_host`; a host
  whose key is not `expected_host`, or
  whose `HostProof` fails, ends it with `Refused(WRONG_HOST)`, "this is not
  the game you were invited to" (without `expected_host`, a failed proof
  is `UNPROVEN_HOST`, "this game could not prove who it is": nobody was
  invited); a client not welcomed `JOIN_DEADLINE`
  (10 s) after its link opened ends with `Disconnected(NO_ANSWER)`, "the
  game did not answer" (a host that never sends `Challenge`, `HostProof`
  or `Welcome`: the link's keepalives alone never time it out); `content_cfg`:
  `PeerContent`, below); `poll(now, local_keys, on_tick) ->
  Vec<OnlineEvent>`; `content()` (what matches and replays load from),
  `downloads()`, `download_prompt()` (the bytes waiting for approval, as
  of the last poll), `approve_downloads()`;
  `lobby()` (`LobbyHandle`: ready, spectate, look, loadout, chat,
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
  `reachability()` (a host's `Reachability` now; `OnlineEvent::
  Reachability` when its lines, `settled()` or `direct()` change: the
  struct is compared first, so a poll formats nothing when it is the
  same); `connection()` (a client's `(Path, remote)` once its link opened,
  the path following ICE; `remote` the host's end, or for `Relayed` the
  TURN relay's address, `Endpoint::relay_server`: ruling FR12b-4;
  `OnlineEvent::Connected { path, remote }` once, alike); `shutdown(self) -> Shutdown` (ruling NT-2: `leave`, the listener
  and the joiner's pool stopped on their threads, `Endpoint::shutdown`,
  the rest of the peer dropped on a thread `rl-online drop` (its content
  worker waits up to 2 s for a job); never blocks; `Shutdown::wait(until)`
  waits at most 1 s for the port mapping's deletion, `true` once the
  router confirmed it or without one; `Shutdown::poll` the same verdict
  without waiting, `None` while the router is asked; rl-server, the game and
  rl-cli's bot client wait on it at exit);
  `listen_on(pool)` (a host listens on these relays from now on, or on
  none: the relays edited in the game's Options while hosting; the old
  listener's thread drops its pool, a new one starts; nothing on a
  client, a closed peer or the same relays; never blocks);
  `LobbyHandle::revision` (the host's `LobbyHost::revision`).
  `test_seed_override`, `test_serve_instead` (a host serves another
  pack's archive for a pack) and `test_prompt_limit` (a client's prompt
  bytes) exist only with the `test-hooks` feature (this crate's own
  dev-dependency). It forwards `rl-net/test-hooks`, so any test build
  with it keeps rl-net's port mapper away from real routers.
  - **Joins** (N46): `Joiner` (`join.rs`) until the link opens. A
    direct attempt's endpoint is in the outbox; its `Disconnected` before
    `Connected` (not `Refused`) with `nostr` drops it and joins through
    Nostr (`Joining(Relays)`), else the usual text. Through Nostr the
    outbox is empty (`Outbox::empty`, `local_port` 0) until the
    signalled endpoint is made (`Gathering`). After the host's answer, a
    `Disconnected` (`Timeout`/`Closed`) is `no_path(port, relayed)`. A
    `Refused { Busy }` through Nostr is `BUSY` (the host's pending
    setups, PF12b-2), not "you are in this lobby already". On
    `Connected` the joiner goes (its pool dropped on a thread, R10-10).
  - **Listening** (N54): the host's `reach::HostReach` (its listener, the
    configured address, what was last reported) starts a `reach::
    Listener` with `listen.pool`; each poll answers its offers with `Endpoint::
    answer_signalled(&req.signal, req.mapped, now)` (`req.mapped` is
    always `None` from our joiners: a joiner maps no port, N47) and hands
    the answer and `Endpoint::mapped()` back to be sealed and published.
    **What became of each join through the relays** (ruling FR12b-3):
    `reach::RelayJoin` (`Answered`, `Refused(reason)`, `NotConnected`:
    the endpoint's `JoinFailed`, `Skewed { by }`: the listener thread
    saw a handshake ignored as `TOO_OLD`/`IN_THE_FUTURE`, `by` its date
    minus the clock, at most one every `NOTE_EVERY`), as
    `OnlineEvent::RelayJoin { join, more }`: at most one of each kind
    every `NOTE_EVERY` (5 s), `more` counting those not said since;
    `RelayJoin::line(more)` is the lobby's and the log's line ("A player
    asked to join through the relays: answered", "…: refused, busy with
    other joins", "…: refused (<reason>)", "A join through the relays
    was answered but did not connect within 15 s: no network path",
    "Ignored a join through the relays dated N s ahead of|behind this
    PC's clock: check both PCs' clocks", then " (N more since)").
    Test hook `OnlinePeer::test_clock_offset(secs)` (`test-hooks`): a
    joiner's handshake dated off the clock.
    The host's `HostEvent::Notice` (N59's `RELAYED_BAN`) is an
    `OnlineEvent::Notice`; `LobbyHandle::ban_with_address(member)` is the
    host's own (`by: None`), `ban_with_address_by(member, by)` an admin's.
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
    peer lingers until its backlog and the endpoint's are delivered. A
    joiner through Nostr has no endpoint yet (`Outbox::empty`; every call
    does nothing): `set_endpoint` once made, `take_endpoint` for a failed
    direct attempt or a shutdown.
  - Reliable payloads carry the tag byte: the session's untagged `Control`
    gets `0` on the way out and loses it on the way in; lobby messages are
    `1`/`2`, `ContentMsg` `3` (the content side; a host takes them from
    members only). A payload that does not decode drops the peer (host) or
    ends the connection (client).
  - **Host**: the endpoint's gate refuses the lobby's banned addresses (a
    set mirrored from `Kick { ban: true }` when the lobby banned the
    address, and from `ban_ip`/`unban_ip`). `Connected` → `connected` with
    a fresh nonce and the link's `dtls_fingerprints` (a client's
    `connected` takes them too). Each poll (and before
    `ban_with_address_by`) every member's path follows the endpoint's
    `path(peer)` (`LobbyHost::set_path`; final review B-M1: the tags and
    N59's ban decision use the path now, not as it was at `Connected`).
    Lobby messages leave at most every `LOBBY_DRAIN`
    (250 ms; a start, an end, a kick, a join (its welcome before the
    content list), the `HostProof` answering a `Hello` and a mid-match
    join at once). A
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
    peers in the slots `Seat::Player(i)`, the others `Seat::Spectator`;
    members waiting for their content are left out, so they never time
    out, and join through `ContentComplete`);
    `playing(0)` at the first tick. A client builds the same world,
    compares the checksum (`MISMATCH` → `Refused` and it leaves), plays
    its slot with its input delay, or watches; a `Start` while its match
    runs is ignored (`START_IGNORED`). A member waiting during a match
    (welcomed then, or lacking content at its start: N33) gets `Start` at
    its `HostEvent::ContentComplete` (its `Have` after the content list)
    and the session's `add_spectator`; its client, seeing the lobby
    already starting or playing, is `Role::Joining`. Matches build from
    `content()` (the overlay); `check_packs` stays the last guard.
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
    closed. Refusals, kicks and lost connections end a client the same way,
    each with its reason in words (`rl_net::endpoint::Why::Unsent`, its own
    failed send, is "this game could not send to the host", not the
    host's fault).
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
- **`content/`** (plan 11, N32): pack downloads over `TAG_CONTENT`,
  sans-IO (time is `now_us`). `PROMPT_BYTES`, the rates (`PEER_RATE`,
  `HOST_RATE`, `MATCH_RATE`, `SKIN_RATE`), `QUEUED`, `SILENCE_US` (10 s);
  `pack_kind(ItemKind)`, `archive_limit` (`MAX_SKIN_ARCHIVE` for skins,
  else `MAX_ARCHIVE`: `rl_content::archive_limits(kind).bytes`; an
  install is held to the whole `archive_limits`).
  - `serve.rs`, `Server::new(rate)`: `offer(hash, Arc<PackArchive>)`,
    `withdraw`, `receive(peer, Want|Cancel)`, `peer_left`, `poll(now_us,
    room, out)`. One transfer per receiver, the first queued want that is
    offered; a want for what is not offered, or beyond the archive, is
    answered `Unavailable` (one per want, at most `MAX_ITEMS` waiting);
    at most `MAX_ITEMS` queued wants per peer. A want for the current
    transfer restarts it at its offset. `Begin` (archive SHA-256, size,
    offset) precedes each transfer's first chunk, a resumed one's too.
    Token buckets in byte-microseconds: `PEER_RATE` per receiver and the
    server's rate shared (each holds at most an eighth of a second, at
    least a chunk), round-robin from the receiver after the last served;
    a chunk goes while `room(peer)` (capped at `QUEUED`) is at least
    `CONTENT_CHUNK + 64`. `Unavailable` and a chunkless `Begin` skip the
    room and the buckets: one per `Want` received, so the peer's own
    sending bounds them.
  - `fetch.rs`, `Fetcher::new(Arc<Mutex<DownloadCache>>)`: `list(source,
    items, local)` returns the missing items (not in `local` by
    `Content::has`, not in the cache; `NO_HASH` never fetched), required
    first, skins after; a transfer the new list drops gets `Cancel` (its
    partial stays; the same hash listed with another size is cancelled,
    its partial discarded, and fetched afresh); a `Failed` item is tried
    again at each new list (`Unavailable` stays). Items not approved before
    wait for `approve` when more than `PROMPT_BYTES` of them are still to
    download (each one's size less its partial's bytes; ruling PF11-6,
    N35); `waiting_approval` is that number, steady while approved items
    download. `poll(now_us, out)` asks for the next item,
    evicting first (`evict` and `trim_partials`, `keep` the list's hashes,
    `CACHE_BYTES`; every fetcher's list and the host's own list are
    registered with the shared cache through a `rl_content::Keeper`, so
    each eviction keeps the union of the process's lists: review Minor 2), resuming from a partial of the listed size. `receive`
    takes the source's `Begin`, `Chunk`, `Unavailable`: a `Begin` whose
    size is not the listed one, above the item's limit or past its end
    fails the item (`Cancel`); another archive than the partial's discards
    it (taken at once from offset 0, else `Want { offset: 0 }`); the same
    archive at another offset is asked again from the bytes held. Chunks
    count only after an accepted `Begin` and at the bytes held (checked
    arithmetic; past the end fails the item). The last chunk submits
    `Job::Install`; the caller routes `Done::Installed` to `finished`,
    which completes the item, or discards the partial and fetches it once
    more from 0, then `Failed`. No chunk for `SILENCE_US`: asked again
    once, then `Failed("the host stopped sending")`. `Verifying` has no
    timeout (the worker answers every job, a panic too). `missing_required`,
    `installed` (the list's cached packs, for `Content::with_packs`),
    `source_left` (partials stay; the next list resumes).
  - `peer_side.rs` (called from `peer.rs`'s poll, after the lobby's
    events): `PeerContent { cache: Result<Arc<Mutex<DownloadCache>>,
    String>, upload_rate }` (ruling PF11-4: the peers of one process share
    one cache; `Err(why)`: no downloads, and a client lacking content gets
    a `Notice` naming why; `without_downloads(why)`). Every hash and
    archive is made on the worker (`Content::known_hash`/`remember_hash`):
    a list naming a pack this peer has but has not hashed yet waits for
    the worker. Chunks go to a peer while its outbox (`Outbox::buffered`)
    holds less than `QUEUED`. `OnlineEvent::Downloads` at most every
    `REPORT_EVERY_US` (250 ms), `DownloadPrompt { bytes }` when a list is
    over the prompt. The content of matches is this peer's packs with the
    downloaded ones laid over (`Content::with_packs`, their verified
    hashes remembered).
    - **Host**: the list is the setup's packs (required) and every
      member's and bot's skin it has, once by hash and once by id (the
      first: a match loads skins by id, so another of the id is never
      shown; review Minor 1) (a bot's skin as the
      host has it, ruling R6-2), each listed once archived; no list
      goes out until every required item is archived (one that cannot be
      is listed with size 0 and refused when asked). A changed list goes
      to every member, the current one to each joiner after its
      `Welcome`; each list has a revision (1, then one more per change
      of its required items: a skin change keeps it, as `Have` names
      required items only) given to the lobby (`set_content_revision`),
      so a member is lacking (not ready, waiting at a start) until its
      `Have` answers the current list. The host lists an optional item
      never under a required id or one of its packs of another kind
      (`may_list`); it fetches or lays over one only when it has no pack
      of that id at all (`may_take`; ruling PF11-5: local packs win, so a
      member's other `olx-skin-default` is drawn as this peer's own). An
      offer its member no longer wears is dropped, fetched skins the list
      no longer names are forgotten, and progress is kept only for what
      the list names. Served at `upload_rate`, `MATCH_RATE` while the phase is
      `Starting`/`Playing`. `Member::download` from the archive bytes sent
      to each member of what its `Have` names (queued, not delivered; a
      `Want` from an offset starts its count there). The list is built
      again only when the lobby's revision or what it is made of changed
      (worker results, offers, fetched skins, peers leaving). A member offers its own
      skin with a one-item `List` (ruling R6-1: id and hash its look's);
      a skin the host lacks is fetched from that member (one fetch per
      hash; a new fetch from one member's address at most every
      `SKIN_FETCH_EVERY_US`, 5 s, kept that long after it leaves, and at
      most `SKIN_FETCHES_PER_MINUTE` (12) for all members together: an
      offer waits, and only the look worn then counts; a dropped,
      refused or failed offer's partial is discarded), installed, then listed and served to the others. Its own
      look's skin hash is filled from its content.
    - **Client**: its look's skin hash filled once hashed (in `Hello`
      when it comes first, else a `SetLook`); its skin's archive offered
      to the host and served at `SKIN_RATE` (that hash only). A host's
      `List` goes to its `Fetcher` (its own content and the cache; its
      optional items that `may_take` refuses are dropped: under a
      required id or an id it has a pack of), then `Have {
      revision, missing: missing_required }` after every list and whenever
      that changes; installs are routed to `Fetcher::finished`.
  - `worker.rs`, `Worker::start(cache)` (`None`: installs fail): one thread (`rl-online-content`)
    running `Job::{Archive, Install, Hash}` in order; `take` returns the
    `Done`s. A job that panics (a parser of untrusted content) is caught
    (`catch_unwind`) and fails with the panic's message; the thread goes
    on. An archive is checked against its content hash (a pack
    changed since it was hashed is not served) and carries its SHA-256
    (`PackArchive`). An install locks the cache only to begin and finish.
    Dropping it stops the thread after its job (joined within 2 s, else
    left to end).
- **`join.rs`** (N46, N49): `NostrJoin { host, pool, region,
  turn_from_nostr, allow_local }`, `Join { direct, nostr }`
  (`Join::direct(addr)`), `JoinStep` (`Display`: the waiting texts),
  `join_relays(hints, own)` (ruling PF12b-3: the invite's hints first,
  then the player's own, each once — a trailing `/` naming the same
  relay, final review B-M6 —, at most `MAX_JOIN_RELAYS` 8; the
  joiner cuts its pool's list the same way), the texts `NO_RELAYS`,
  `NO_REPLY`, `RELAYS_REFUSED` (every relay refused the join's events by
  NIP-01 policy, `blocked:`/`restricted:`/`invalid:` … but not the
  transient `rate-limited:` or `error:`, or could never be reached:
  web-of-trust relays refuse fresh keys, so this is not blamed on the
  host; the pool's own "no answer within …" is no refusal),
  `RELAYS_INVALID` (the same, with one refusal at least `invalid:`, most
  often an event dated off the relay's clock: it asks after this PC's
  clock; final review B-M2), `BUSY`,
  `no_path`, `path_text`, `path_tag`, `nat_text`. Through Nostr
  (`Nostr`, polled before the endpoint): `Relays` (a `Pool`; with
  `turn_from_nostr` a `TurnList` subscription until 1 s after its first
  `EOSE`, at most 4 s), `Gathering` (`Endpoint::signalled` with the
  caller's first 2 TURN servers then `TurnList::pick(region, 2 - own)`,
  `allow_local` added to the endpoint's), `Asking` (once gathered: an
  `Asker` with the offer, its answer subscription first; the first event
  after that subscription's first `EOSE` or 2 s, then every resend, each
  to every relay), `Connecting` (the reply to `take_reply`; the host's
  first candidate's port kept for `no_path`, and the first TURN server
  with an allocation; `Connecting` is reported for an `Answer` only, a
  `Refused` ending the join with its text). Failures before an answer: every relay `Failed`
  and none ever connected → `NO_RELAYS`; `RELAYS_REFUSED`; the asker
  expired → `NO_REPLY`.
- **`reach.rs`** (N47, N54): `HostListen { pool, public_address }`
  (`off()`), `Listening` (`Off`, `Connecting`, `On { relays, of }`,
  `NoRelay`: every relay failed or dropped at least once and none is
  connected, so it does not flap while they retry), `Reachability {
  port, configured, reach, listening, stun_settled }` (`direct()`: a
  usable mapping with TCP, else the configured address, else (ruling
  N47-a) `Reach::open`, the machine's own address when STUN shows no NAT;
  `settled()`: the
  mapping is not `Trying` and `stun_settled`, `Endpoint::gathered`;
  `lines()`: the router's line (with no NAT and no configured address,
  a failed or off mapping says "No router: players reach you directly at
  <open>" instead, N47-a), the configured address's, a symmetric NAT's,
  the listener's, in that order). `HostReach` (crate-private):
  `start(listen, me)`, `listen_on(pool)` (another listener on other
  relays; a listener that cannot start leaves no relays recorded, so the
  next call starts one, final review B-M5), `answer(ep, now)`, `now(ep)`, `changed(ep)` (a
  report when the lines, `settled` or `direct` changed), `stop`.
  `Listener` (crate-private): a
  thread `rl-online listen` owning the `Pool` and the `Inbox` (whose
  signature checks stay off the caller's thread), subscribing
  `Inbox::filter` again every `RESUBSCRIBE` (30 s); new offers go to the
  peer with when they came (unbounded: the inbox's rate bounds it; one
  older than `OFFER_STALE`, 8 s, is skipped, its joiner gone after 10 s),
  repeats' cached replies are
  published, answers sealed with `Inbox::reply` and published; dropping
  it stops the thread, which drops its pool itself.
- **`events.rs`**: `OnlineEvent` (plan 12b: `Joining(JoinStep)`,
  `Reachability(Reachability)`, `Connected { path, remote }`,
  `RelayJoin { join, more }`, `ListenRelay { url, status }`: a host's
  listener's relay "connected" or "unreachable: why", once until it
  changes, final review B-M4), `NetStats` (round trip, input delay, the
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
client's record and replay equal the server's (`MatchEndWire::last_tick`);
a client expecting another host key is refused (`WRONG_HOST`), one
expecting the host's joins; a host that opens the link and stays silent
is given up `JOIN_DEADLINE` after the link opened, not before
(`a_silent_host_is_given_up`: a bare `rl_net` endpoint, one virtual
clock, no content needed).
Content (each peer a cache in a temporary directory; `common::
partial_content` copies the match's packs but some, `with_test_skin`
adds a skin pack of another hash): a client without the level, or the mod,
downloads it (reports coalesced, ending in `Done`), is ready, plays and
makes the host's replay; a member's skin the host lacks travels through
the host to another client; a skin change keeps every ready player
ready; a member's skin pack under the level's id
is never fetched or listed, and the host's match still builds; another
pack's archive served for the level
fails after one retry, the client stays, not ready, lacking it; a
spectator joining mid-match without the level waits, downloads at the
match's rate (no faster than `MATCH_RATE` allows), then watches, and
nobody is dropped; a spectator lacking
the level at a start (its download held by the prompt) is still there
10.5 s into the match, then watches once complete; no cache (`Err`) is a
notice and an unwritable one a `Failed` download with the I/O reason;
content over the prompt waits for `approve_downloads`.
`tests/through.rs` (plan 12b; `TestRelay`s and a `TurnServer` on
127.0.0.1, `base_override: 127.0.0.1`, every datagram on 127.0.0.0/8,
ruling NT-1): a join through Nostr without an address (`Relays` before
`Asking`, path `Direct`, the host's member's path; another without
`expected_host`: it expects the host's key); a join through a
TURN relay found on Nostr (`relay_only`, the relay on 127.0.0.3, path
`Relayed` naming the relay's address, not the host's, FR12b-4; a 6 s
match to equal confirmed inputs and checksums); a lost offer sent again
(`DropEphemeral(2)`, connected within 8 s); a relay that never forwards
survived; a host's answer with no path (`relay_only`, no TURN server,
rl-net's `setup_timeout` hook at 3 s) is `no_path(port, None)`; a
refused offer (another fingerprint) says why and never `Connecting`;
`NO_REPLY` 10 s ± 1 s after `Asking` (it names the clocks); a joiner
whose clock is 2 minutes ahead (`test_clock_offset(120)`) ends with
`NO_REPLY` while its host says `Skewed { by: ~120 }` (FR12b-3); the
host's `Answered`, `Refused(Version)` and `NotConnected` lines in the
join, refusal and no-path tests; `RELAYS_REFUSED`
(`RefuseKind(21470)`); `RELAYS_INVALID` (a joiner's clock 2 minutes
behind: the relay refuses its expired events, B-M2); `NO_RELAYS` (a closed port, within 6 s); a
direct failure falling back to Nostr; a same-LAN joiner whose router does
not hairpin (rl-net's `connect_stall` hook: the connect fails after
`CONNECT_TIMEOUT`) joining through Nostr directly; a flood (Review Focus
1, PF12b-2, FR12b-1: for 8 s, 100 forged, tampered or undecryptable
events a second and, after every 20 of them, a valid offer whose joiner,
once answered, sends valid checks from a forged source and answers none;
an honest joiner starting 3 s in, the pending table full of checked
offers, connects during the flood, more than 8 flood offers answered,
all `Answer`); a host's reachability on a
`FakeGateway` (external 127.0.0.2, UPnP) and one relay, its exact lines;
a host's shutdown returning at once and its `wait` seeing the mapping
deleted (NT-2); a host listening on edited relays (`listen_on`: a join
through the new relay alone, then none: `Listening::Off`). `join.rs`' and `reach.rs`' unit tests: every text
exactly, a host without NAT joined directly at its own address with its
"No router" line (N47-a), the relays' order, refusals told from a slow relay or a
transient one, the answer's port, `direct`, `settled`, the listener's
status, stale offers skipped, a failed listener restart not holding the
old relays (B-M5). `peer.rs`: a join through Nostr expects
its host's key.
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
`tests/content.rs` (in memory, a virtual clock, a real worker; Dirt Level
from `RL_CONTENT_DIR`, else a generated level): a level downloads through
`Queued`, `Receiving`, `Verifying`, `Done` and installs; a download cut
after 3 chunks resumes in a new fetcher at `3 * CONTENT_CHUNK`; a changed
list cancels the transfer and keeps its partial; another pack's archive
under the wanted hash fails after one retry; lists over `PROMPT_BYTES`
wait for `approve`; rl-proto's archive limits equal rl-pack's (PF11-1);
the worker's jobs. `serve.rs`' unit tests: pacing (3 receivers at 1 MiB/s
for 10 s send 9.5-10.5 MiB, a third each; one receiver held to
`PEER_RATE`), refusals, `Begin` before every transfer and resume, one
transfer at a time, `Cancel`, the outbox room. `fetch.rs`' unit tests:
order, `Unavailable`, hostile `Begin`s and chunks, another archive,
silence, a source leaving, a re-sent list with another size, a failed
item retried once a list, the prompt counting only the bytes still to
download of items not approved. `worker.rs`' unit test: a panicking job
(a `#[cfg(test)]` hook in `run`) fails and the next job completes.
`peer_side.rs`' unit tests: a list waits for the worker's hashes (nothing
hashed on the caller's thread); a list's optional items under a required
id or a local pack of another kind are refused; `Want`s for unlisted
hashes keep no progress; an offer its member no longer wears is dropped
and the list rests; a skin under a local id is neither fetched by a
client nor by a host (PF11-5); an outbox that never drains holds at most
`QUEUED` of content.
