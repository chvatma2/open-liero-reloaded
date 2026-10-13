# rl-discovery

Finding games (networking spec §6, plan 12a): the Nostr core, and on it the
test relay, relay pool, announcements, game list, STUN and invite codes
(later tasks).

- **Layer 5, std.** Depends on rl-proto (`PublicKey`, `Signature`), rl-lobby
  (`Identity`, `verify`), serde_json, sha2, hex, thiserror, and for the
  relay drivers rustls, webpki-roots and tungstenite. The core is
  sans-IO (no sockets, no threads, no clock: time is an argument) because
  plan 13 builds parts of this crate for WASM. Sockets and threads belong
  only to drivers (`relay`).
- **`nostr`** (decision N38: no Nostr library). The NIP-01 subset we use:
  - `Event` with `sign(identity, created_at, kind, tags, content, aux)`
    (signs the 32-byte id with the install's BIP-340 key: Nostr's
    `pubkey`/`sig` are exactly x-only BIP-340), `verify()` (recomputes the
    id from the fields, then checks the signature), `tag(name)` and
    `expiration()` (NIP-40).
  - `canonical(..)` is written by hand, not with serde_json's escaper:
    NIP-01 escapes `\n \" \\ \r \t \b \f` and writes every other character
    verbatim (raw control characters and DEL included), which differs from
    JSON's own escaper. The wire encoding (`encode_*`) uses serde_json and
    may escape differently; ids always come from `canonical`.
  - Frames: `ToRelay` (`EVENT`, `REQ`, `CLOSE`) and `FromRelay` (`EVENT`,
    `EOSE`, `OK`, `NOTICE`, `CLOSED`). `decode_*` refuse text above
    `MAX_FRAME` (128 KiB) and rely on serde_json's recursion limit (128)
    for nesting. Decoding does not verify signatures: callers call
    `Event::verify`. `NostrError::is_frame_error()` tells text that is no
    relay frame at all (`Json`, `TooLarge`, `NotAFrame`) from a frame we
    skip (`Unknown` type, `Malformed` field). Hex fields must be exact
    length. `OK` and `CLOSED`
    without a message are accepted (seen in the wild); unknown filter keys
    are ignored; `#x` filters need a single character `x`. Authors in
    filters are full 32-byte keys (no prefixes). `encode_from_relay` and
    `decode_to_relay` exist for the test relay.
  - Kinds: `KIND_ANNOUNCE` 31470 (addressable, tag `d` = `rust-liero`),
    `KIND_HANDSHAKE` 21470 (ephemeral, plan 12b). `MAX_CONTENT` (4 KiB) is
    the limit for our own events' content; callers enforce it.
- **Kinds registry check, 2026-10-13.** `schema.yaml` of
  github.com/nostr-protocol/registry-of-kinds (kinds section) and the NIPs
  README kinds table list nothing in 31400-31499 or 21400-21499 (31470 and
  21470 are free). Re-check before a release.
- **Fixture** `tests/fixtures/relay-events.json`: three real kind-1 events a
  public relay served on 2026-10-13, fetched once by hand (a WebSocket REQ
  from node) and embedded; they verify with our code. No test touches the
  network.

- **`test_relay`** (feature `test-relay`; rl-game, rl-server and rl-cli
  tests enable it in dev-dependencies, never normal builds). `TestRelay`:
  `start()` (`ws://127.0.0.1:<port>`), `start_tls()` (`wss://localhost:<port>`
  plus the DER root; rcgen self-signed, rustls with the aws-lc-rs provider;
  the relay listens on 127.0.0.1 only, so a client connecting to `localhost`
  must fall back from `::1`, or connect to `port()` directly), `url()`,
  `port()`, `events()`, `set_misbehaviour(Honest | NeverEose | Garbage |
  DropAfter(n) | Slow(d) | Silent | Drip | SlowReads | Refuse(msg) |
  RefuseKind(k) | KeepFor(secs) | NoForward)` (Refuse: every event gets
  `OK false msg`, such as a paid relay's `restricted: …`; RefuseKind: that
  kind gets `blocked: …`; KeepFor: a stored event is served only while the
  relay's clock is under `created_at + secs`; NoForward: ephemeral events
  accepted, forwarded to nobody; SlowReads: reads
  of at most 1 KiB, a 1 ms pause every 8, under any TLS; Silent: no reads,
  no writes, new connections held without a handshake; Drip: one byte per 40 ms,
  forever: an endless handshake response on new connections (on the TCP
  stream, under any TLS), an endless 64 KiB frame on established ones),
  `set_time(unix)` (the clock for NIP-40; default the system clock).
  Verifies events; regular events dedupe by id,
  replaceable kinds by (key, kind), addressable by (key, kind, `d`), newest
  wins; ephemeral 20000-29999 only forwarded; expired events are refused on
  publish and not served. REQ: kinds, authors, `#x`, since, limit (per
  filter, newest first), then EOSE; no filters select nothing. Frames are
  limited to `MAX_FRAME`. One thread per connection; drop stops it (joins
  bounded at 2 s). Tests that keep a subscription must say so: the test
  client's `query` CLOSEs after EOSE, `subscribe` does not.
- **`pool`** and **`relay`** (its driver): `Pool::start(PoolConfig {
  relays, extra_roots, max_frame })` runs one driver thread per relay
  (`Pool::start_with(cfg, Timings)` shortens the deadlines for tests);
  `publish`, `subscribe`, `unsubscribe`, `poll` and `statuses` never block
  (they only touch per-relay mutexes the drivers never hold across IO).
  - **Connect:** the URL's host is resolved, IPv4 addresses first, each
    tried with a 5 s timeout; the TLS and WebSocket handshakes then get 5 s
    in all (tungstenite's `AttackCheck` also caps the response head at
    64 KiB and 512 reads, and fails many tiny reads);
    `client_tls_with_config` with our rustls `ClientConfig`: aws-lc-rs,
    Mozilla's roots from `webpki-roots` plus
    `extra_roots`, never the platform's. `ws://` is allowed. A bad URL is
    `Failed` for good; anything else retries after 1, 2, 4 … 60 s (a
    connection up 60 s resets the backoff).
  - **Serve:** a non-blocking socket (ruling PF12-4: no `SO_RCVTIMEO`/
    `SO_SNDTIMEO`, whose expiry leaves a Windows socket undefined) behind
    an I/O budget in `Sock`. Each turn of the loop first sends (the
    turn's sends share a cumulative 5 s deadline, then
    `Dropped("send timed out")`; a full send buffer waits `POLL`, 10 ms,
    between flushes), then reads for at most one `TICK` (50 ms), waiting
    `POLL` only when nothing is left to read, so bursts drain at full
    speed. Each handshake round reads for one `TICK` and waits `POLL`.
    Reads also stop once the pool is stopping. A relay streaming or
    dripping bytes thus cannot hold the driver past a deadline or a stop.
    On `WouldBlock`, rustls keeps partial records and unsent output,
    tungstenite partial frames and unflushed output, and the handshake
    resumes. Subscriptions are reconciled from the pool's set
    (`REQ`/`CLOSE`) and sent again after every reconnect.
  - **Liveness:** after 30 s with nothing received, a WebSocket ping;
    nothing at all within 10 s of it is `Dropped("no answer to ping")`.
  - **Dropped** (then backoff): a frame above `max_frame` (clamped to
    `MAX_FRAME`), a frame that is not a Nostr message (not JSON, or not an
    array headed by a type; unknown types such as `AUTH` and malformed
    single events are skipped), more than 200 frames/s or 1 MiB/s
    sustained (token buckets holding 5 s of burst: a `REQ`'s stored events
    arrive at once), no `EOSE` within 10 s of a `REQ` (`CLOSED` cancels the
    wait), the liveness and send deadlines, a close or socket error.
  - **No `OK` within 5 s** of an `EVENT` is reported as `PoolEvent::Ok {
    accepted: false, message: "no answer within 5 s" }`; the connection
    stays (a later `OK` is ignored).
  - **Events** are verified in the driver (a forged id never shadows the
    real event in the dedupe) and only for subscriptions the connection
    has; `EOSE` and `OK` only answer what was asked. `poll` appends relay
    by relay in index order, each relay's reports in arrival order, and
    dedupes events by id (an LRU of 4096 ids).
  - **Queues:** outgoing at most 256 per relay (oldest dropped; a queued
    replaceable/addressable event is replaced by a newer one for its
    slot); incoming at most 4096 events or 8 MiB per relay between polls
    (the newest dropped, `dropped_events()`). Queued events leave the
    queue before they are sent: those of a turn whose send fails, or sent
    on a connection lost before its `OK`, are lost (not retried after the
    reconnect; announcements are re-published every minute).
  - **Drop** stops every driver: each notices within about a `TICK`
    (even mid-read or mid-send), sends what is queued (a closing note)
    and the Close frame within 1 s, and is joined within 2 s in all (one
    stuck in a TCP connect or DNS lookup is detached).
- **`stun`** (N40): `binding_request(txn)`, `parse_binding_response(bytes, &txn)`
  (XOR-MAPPED-ADDRESS, MAPPED-ADDRESS only when no XOR one; v4 and v6; the
  address as given, no filtering: callers use `list::is_global`; bounded,
  integrity and fingerprint unchecked) and `public_ip(servers, deadline)`:
  `host:port` strings, IPv4 only, ephemeral `0.0.0.0:0` socket, 500 ms x 2
  tries per server, no lookup started past `deadline` (one lookup may
  block longer): run it on a thread. Only a reply from the server's own
  address with our transaction counts. `StunResponder` (feature `test-relay`;
  `start`, `start_plain`, `server()`, `answered()`) is the test server.
  Tests use the RFC 5769 section 2.2 response.
- **`invite`** (N42) and **`defaults`**: `Invite::{encode, decode, relay_urls}`,
  `rlx1…` bech32m of a postcard `Wire` (version byte 1, key, `RelayHint`s,
  IPv4 addr as 4 bytes + port). `decode` trims, takes all upper case (not
  mixed), refuses text over `MAX_CODE` (200) as `Invalid` (the `bech32`
  0.12 crate's own Bech32m limit is 1023, so the 90 limit is simply not
  applied), text without the `rlx1` prefix is `NotInvite`, any checksum or
  case failure, a plain-bech32 checksum or an HRP other than exactly `rlx`
  (`rlx1foo1…` is HRP `rlx1foo`) `Damaged`, version above 1 `Newer` (checked before parsing),
  version 0, bad postcard, trailing bytes or bad hints `Invalid`. Hints (PF12-6): at most
  3 `Default` and 1 `Url` (`wss://`/`ws://`, at most 64 bytes); a
  `Default` index beyond this build's list (a newer build's code) is
  skipped, not `Invalid`, so the other hints serve; the worst case is 191 characters, within `MAX_CODE`. `encode` is
  infallible and truncates: it drops bad hints, then those beyond the
  limits (first win), so its code always decodes. `DEFAULT_RELAYS` is append-only (indices are wire format;
  `defaults`' test pins the released prefix). Both lists were chosen by
  `rl-cli nostr-probe`/`stun-probe` on 2026-10-13 (plan 12a Task 8; the
  table is in STATUS.md N38).
- Deps: `rustls` (default aws-lc-rs) and `webpki-roots` (workspace pins),
  `tungstenite` with `rustls-tls-webpki-roots` (for `Connector::Rustls`;
  its own default roots are never used). `rcgen =0.14.10` (`aws_lc_rs`
  only, no pem) is optional behind `test-relay`. The crate's own
  dev-dependency on itself turns the feature on for its tests.
- **`announce`** (N39): `GameSummary` (JSON, mod id under `mod`; `mode`
  free, at most 16 chars; unknown `region` reads as `Unknown`);
  `GameSummary::of_lobby(&LobbyState, &Hosting)`, the one summary of a
  hosted lobby for rl-server and rl-game (plan 12a Task 9: here, since
  `LobbyState` and `LanAnnounce` are rl-proto's, with no new same-layer
  pair): players = seated members + bots, spectators = the other members,
  each held to its maximum, `playing` from `Starting` to `Playing`, mode
  "deathmatch", the mod's hash from the setup's packs (else `NO_HASH`),
  `map_name` cut to 40 characters (blank or invalid: the level id);
  `lan()` is the same game's `LanAnnounce`. `announcement(..)`
  (tags `d`, `t`, `f` = 16 lowercase hex, `expiration` +180 s, +10 s when
  `closed`), `parse_announcement` (validates names, ids and counts, no address policy; refuses
  a missing or malformed `f`; does not verify the signature) and the sans-IO
  `Announcer` (every 60 s, at a change after 12 s, `closing()` once, only
  after a first publish).
- **`list`** (N41): `GameList::offer` checks cheaply first (kind, size, age
  at most 180 s, not in the future by over 60 s, expiration, blocked, rate
  gap), then verifies and parses. One note per key per 10 s of `now_unix`
  (a closing note of a listed game is exempt); only newer `created_at` than
  the key's last replaces it (a tombstone is kept after a close); at most
  500 games (new keys refused when full) and 4096 remembered keys (when full
  the oldest unlisted one is evicted). Announced addresses must be global
  (`list::is_global`, v4-mapped unwrapped) unless `allow_local(true)`. Refusals
  that need no signature check (stale, inside the gap) come first. `games()`
  sorts compatible first, then ping (measured, unknown/measuring, no answer),
  name, key. `filter()` has no `since` (no clock); `filter_since(now)` does.

Test: `cargo nextest run -p rl-discovery` (`tests/announce.rs` and `tests/list.rs`: announcements and the game list; `tests/nostr.rs`: the published
events verify, signing round-trips, canonical escaping, tampering, frames
round-trip, fuzzed frames never panic; `tests/test_relay.rs`: the relay;
`tests/stun.rs`: STUN; `tests/pool.rs`: the pool against the test relay (silent, dripping,
slow, slow-reading: every frame arrives intact through partial writes,
over `ws://` and `wss://`), a flooding relay and unreachable relays: a
listener on `127.0.0.1` that never accepts, and TEST-NET-1 `192.0.2.1`, never
routed; unit tests of the backoff, limiter, queues, dedupe and default
timings in `src`).
